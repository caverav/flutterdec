impl<'a> FuncEmitter<'a> {
    fn display_indirect_target_value(&self, target_value: &str) -> String {
        let rendered = self.annotate_pool_refs(target_value);
        if rendered == "x21.f0" || rendered == "reg21.f0" {
            return "dispatchTargetFn".to_string();
        }
        rendered
    }

    fn escape_hint_text(value: &str) -> String {
        let mut escaped = String::new();
        for c in value.chars() {
            match c {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                _ => escaped.push(c),
            }
        }
        escaped
    }

    fn render_pool_value_hint(&self, expr: &str) -> String {
        let t = expr.trim();
        let Some(inner) = t.strip_prefix("pool[").and_then(|s| s.strip_suffix(']')) else {
            return expr.to_string();
        };
        let Ok(idx) = inner.trim().parse::<u64>() else {
            return expr.to_string();
        };
        let Some(value) = self.pool_value_hints.get(&idx) else {
            return expr.to_string();
        };
        let escaped = Self::escape_hint_text(value);
        format!("\"{}\" /* pool[{}] */", escaped, idx)
    }

    fn annotate_pool_refs(&self, expr: &str) -> String {
        let exact = self.render_pool_value_hint(expr);
        if exact != expr {
            return exact;
        }

        let mut out = String::new();
        let bytes = expr.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if i + 5 <= bytes.len() && &bytes[i..i + 5] == b"pool[" {
                let mut j = i + 5;
                let mut idx = 0u64;
                let mut has_digit = false;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    has_digit = true;
                    idx = idx
                        .saturating_mul(10)
                        .saturating_add((bytes[j] - b'0') as u64);
                    j += 1;
                }
                if has_digit && j < bytes.len() && bytes[j] == b']' {
                    if let Some(value) = self.pool_value_hints.get(&idx) {
                        out.push_str("pool[");
                        out.push_str(&idx.to_string());
                        out.push_str(" /* \"");
                        out.push_str(&Self::escape_hint_text(value));
                        out.push_str("\" */]");
                        i = j + 1;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    fn extract_pool_indices(expr: &str) -> Vec<u64> {
        let mut out = Vec::new();
        let bytes = expr.as_bytes();
        let mut i = 0usize;
        while i + 5 <= bytes.len() {
            if &bytes[i..i + 5] == b"pool[" {
                let mut j = i + 5;
                let mut val = 0u64;
                let mut has_digit = false;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    has_digit = true;
                    val = val
                        .saturating_mul(10)
                        .saturating_add((bytes[j] - b'0') as u64);
                    j += 1;
                }
                if has_digit && j < bytes.len() && bytes[j] == b']' {
                    out.push(val);
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    fn is_generic_call_name(name: &str) -> bool {
        let t = name.trim();
        t.is_empty() || t == "unknown" || t.starts_with("sub_") || t.starts_with("fn_0x")
    }

    fn resolve_indirect_target_symbol_call_name(&self, target_expr: &str) -> Option<(String, u64)> {
        for idx in Self::extract_pool_indices(target_expr) {
            let Some(hint) = self.pool_semantic_hints.get(&idx) else {
                continue;
            };
            let Some(va) = hint.target_va else {
                continue;
            };
            let Some(symbol) = self.symbol_names.get(&va) else {
                continue;
            };
            let call_name = sanitize_name(symbol);
            if Self::is_generic_call_name(&call_name) {
                continue;
            }
            return Some((call_name, va));
        }
        None
    }

    pub(super) fn emit_call(&mut self, ins_target: &str, indent: usize) {
        self.total_calls += 1;
        self.state.call_index += 1;

        let tname = format!("t{}", self.state.call_index);
        let raw_arg_values = (0..4)
            .map(|r| {
                self.state
                    .reg_values
                    .get(&format!("x{r}"))
                    .cloned()
                    .unwrap_or_else(|| format!("arg{r}"))
            })
            .collect::<Vec<_>>();
        let selector_intent =
            infer_selector_intent_from_context(
                &raw_arg_values,
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            );
        let selector_name = infer_selector_name_from_context(
            &raw_arg_values,
            &self.pool_value_hints,
            &self.pool_semantic_hints,
        );
        let library_intent = infer_library_intent_from_context(
            &raw_arg_values,
            &self.pool_value_hints,
            &self.pool_semantic_hints,
        );
        let arg_values = raw_arg_values
            .iter()
            .map(|a| self.annotate_pool_refs(a))
            .collect::<Vec<_>>();
        let args = arg_values.join(", ");

        let target = normalize_target(ins_target);
        if target.starts_with('x') {
            self.indirect_calls += 1;
            self.raw_register_calls += 1;
            let named_target = named_indirect_target(&target);
            let target_value = self
                .state
                .reg_values
                .get(&target)
                .cloned()
                .unwrap_or_else(|| named_target.clone());
            let target_selector_intent = infer_selector_intent_from_context(
                std::slice::from_ref(&target_value),
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            );
            let target_selector_name = infer_selector_name_from_context(
                std::slice::from_ref(&target_value),
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            );
            let target_library_intent = infer_library_intent_from_context(
                std::slice::from_ref(&target_value),
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            );
            let intent = infer_call_intent_with_context(
                &named_target,
                &raw_arg_values,
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            )
            .or(selector_intent.clone())
            .or(target_selector_intent.clone())
            .or(library_intent.clone())
            .or(target_library_intent.clone());
            let selector_name = selector_name.or(target_selector_name);
            if let Some(rewritten_name) = readable_call_name_from_intent(&named_target, intent.as_deref()) {
                self.semantic_indirect_calls += 1;
                let mut comments = Vec::new();
                if let Some(v) = intent {
                    comments.push(v);
                }
                comments.push(format!("indirect via: {}", named_target));
                if target_value != named_target {
                    comments.push(format!(
                        "target: {}",
                        self.display_indirect_target_value(&target_value)
                    ));
                }
                let suffix = format!(" // {}", comments.join(", "));
                self.push_line(
                    indent,
                    &format!("final {} = {}({});{}", tname, rewritten_name, args, suffix),
                );
            } else if let Some((target_call_name, target_va)) =
                self.resolve_indirect_target_symbol_call_name(&target_value)
            {
                self.semantic_indirect_calls += 1;
                self.target_va_symbol_calls += 1;
                let target_intent = infer_call_intent_with_context(
                    &target_call_name,
                    &raw_arg_values,
                    &self.pool_value_hints,
                    &self.pool_semantic_hints,
                )
                .or(selector_intent.clone())
                .or(target_selector_intent.clone())
                .or(library_intent.clone())
                .or(target_library_intent.clone());
                let emitted_name =
                    readable_call_name_from_intent(&target_call_name, target_intent.as_deref())
                        .unwrap_or_else(|| target_call_name.clone());
                let mut comments = Vec::new();
                if let Some(v) = target_intent {
                    comments.push(v);
                }
                comments.push(format!("indirect via: {}", named_target));
                if target_value != named_target {
                    comments.push(format!(
                        "target: {}",
                        self.display_indirect_target_value(&target_value)
                    ));
                }
                comments.push(format!("target_va: 0x{target_va:x}"));
                if emitted_name != target_call_name {
                    comments.push(format!("was: {}", target_call_name));
                }
                let suffix = format!(" // {}", comments.join(", "));
                self.push_line(
                    indent,
                    &format!("final {} = {}({});{}", tname, emitted_name, args, suffix),
                );
            } else if let Some(selector) = selector_name.clone() {
                self.dispatch_selector_calls += 1;
                let (dispatch_name, constructor_like) = fallback_call_name_from_selector(&selector);
                let mut comments = Vec::new();
                comments.push(format!("selector: {}", selector));
                if constructor_like {
                    comments.push("heuristic: constructor-like selector".to_string());
                }
                comments.push(format!("indirect via: {}", named_target));
                if target_value != named_target {
                    comments.push(format!(
                        "target: {}",
                        self.display_indirect_target_value(&target_value)
                    ));
                }
                self.push_line(
                    indent,
                    &format!(
                        "final {} = {}({}); // {}",
                        tname,
                        dispatch_name,
                        args,
                        comments.join(", ")
                    ),
                );
            } else {
                let mut comments = Vec::new();
                if named_target != "dispatchTarget" && target_value != named_target {
                    comments.push(format!(
                        "target: {}",
                        self.display_indirect_target_value(&target_value)
                    ));
                }
                if named_target == "dispatchTarget" {
                    comments.push("indirect via: dispatchTarget".to_string());
                    let suffix = if comments.is_empty() {
                        String::new()
                    } else {
                        format!(" // {}", comments.join(", "))
                    };
                    if target_value != named_target {
                        self.push_line(
                            indent,
                            &format!(
                                "final {} = {}.invoke({});{}",
                                tname,
                                self.annotate_pool_refs(&target_value),
                                args,
                                suffix
                            ),
                        );
                    } else {
                        self.push_line(
                            indent,
                            &format!("final {} = dispatch.invoke({});{}", tname, args, suffix),
                        );
                    }
                } else if named_target == "cachedTarget"
                    || named_target.starts_with("indirectTarget")
                {
                    comments.push(format!("indirect via: {}", named_target));
                    let suffix = if comments.is_empty() {
                        String::new()
                    } else {
                        format!(" // {}", comments.join(", "))
                    };
                    self.push_line(
                        indent,
                        &format!(
                            "final {} = {}.invoke({});{}",
                            tname, named_target, args, suffix
                        ),
                    );
                } else {
                    let target_suffix = if comments.is_empty() {
                        String::new()
                    } else {
                        format!(" // {}", comments.join(", "))
                    };
                    self.push_line(
                        indent,
                        &format!(
                            "final {} = dynamicCall({}, [{}]);{}",
                            tname, named_target, args, target_suffix
                        ),
                    );
                }
            }
        } else {
            let call_name = if let Some(hex) = target.strip_prefix("0x") {
                if let Ok(va) = u64::from_str_radix(hex, 16) {
                    if let Some(name) = self.symbol_names.get(&va) {
                        sanitize_name(name)
                    } else {
                        format!("fn_{}", target)
                    }
                } else {
                    format!("fn_{}", target)
                }
            } else {
                format!("fn_{}", target)
            };
            let intent = infer_call_intent_with_context(
                &call_name,
                &raw_arg_values,
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            )
            .or(selector_intent);
            let emitted_call_name = readable_call_name_from_intent(&call_name, intent.as_deref())
                .unwrap_or_else(|| call_name.clone());
            if emitted_call_name != call_name {
                self.semantic_direct_calls += 1;
            }
            let mut comments = Vec::new();
            if let Some(v) = intent {
                comments.push(v);
            }
            if emitted_call_name != call_name {
                comments.push(format!("was: {}", call_name));
            }
            let suffix = if comments.is_empty() {
                String::new()
            } else {
                format!(" // {}", comments.join(", "))
            };
            self.push_line(
                indent,
                &format!(
                    "final {} = {}({});{}",
                    tname,
                    emitted_call_name,
                    args,
                    suffix
                ),
            );
        }
        self.state.reg_values.insert("x0".to_string(), tname);
    }

    pub(super) fn emit_block(&mut self, id: usize, indent: usize, depth: usize) {
        if self.should_wrap_loop_header(id, depth) {
            self.emit_wrapped_loop(id, indent, depth);
            return;
        }
        if depth >= 12 {
            self.push_line(indent, "// depth-limited block");
            return;
        }
        if self.active_stack.contains(&id) {
            if self.loop_context.contains(&id) {
                self.push_line(indent, "continue;");
            } else {
                self.loop_back_edges.insert(id);
            }
            return;
        }
        if self.inline_visits.get(&id).copied().unwrap_or(0) >= self.visit_limit(id) {
            self.emit_omitted_path(indent, Some(id));
            return;
        }

        let block = match self.block_by_id.get(&id) {
            Some(b) => *b,
            None => return,
        };

        self.emitted.insert(id);
        *self.inline_visits.entry(id).or_insert(0) += 1;
        self.active_stack.push(id);

        for ins in &block.instrs {
            match ins.op {
                IROp::Call => {
                    self.emit_call(&ins.target, indent);
                }
                IROp::LoadPool => {
                    let ops = split_operands(&ins.src);
                    if let Some(dst) = ops.first().and_then(|o| canonical_reg(o)) {
                        let rhs = if ins.target.is_empty() {
                            "pool[?]".to_string()
                        } else {
                            ins.target.clone()
                        };
                        self.state.reg_values.insert(dst, Self::clean_expr(rhs));
                    }
                }
                IROp::Branch => {
                    let (mnemonic, ops) = split_instruction(&ins.src);
                    let cond = self.branch_condition(&mnemonic, &ops);
                    let true_id = self.branch_target_block(&ins.target);
                    let false_id = {
                        let mut other = None;
                        for s in &block.succs {
                            if Some(*s) != true_id {
                                other = Some(*s);
                                break;
                            }
                        }
                        other
                    };

                    let cond_str = match cond {
                        Some(c) => Self::clean_expr(c),
                        None => {
                            self.placeholder_ifs += 1;
                            "/* cond */".to_string()
                        }
                    };

                    self.push_line(indent, &format!("if ({}) {{", cond_str));
                    if let Some(tid) = true_id {
                        if self.can_inline(tid, depth + 1) {
                            let saved = self.state.clone();
                            self.emit_block(tid, indent + 1, depth + 1);
                            self.state = saved;
                        } else {
                            self.emit_omitted_path(indent + 1, Some(tid));
                        }
                    } else {
                        let target = normalize_target(&ins.target);
                        if target.starts_with("0x") {
                            self.push_line(indent + 1, "/* external branch */");
                        } else {
                            self.unresolved_cf += 1;
                            self.push_line(indent + 1, "// unresolved branch target");
                        }
                    }
                    self.push_line(indent, "}");

                    if let Some(fid) = false_id {
                        if self.can_inline(fid, depth + 1) {
                            self.push_line(indent, "else {");
                            let saved = self.state.clone();
                            self.emit_block(fid, indent + 1, depth + 1);
                            self.state = saved;
                            self.push_line(indent, "}");
                        } else if !self.emitted.contains(&fid) {
                            self.push_line(indent, "else {");
                            self.emit_omitted_path(indent + 1, Some(fid));
                            self.push_line(indent, "}");
                        }
                    }

                    self.active_stack.pop();
                    return;
                }
                IROp::Jump => {
                    let target_id = self.branch_target_block(&ins.target);
                    if let Some(tid) = target_id {
                        if self.can_inline(tid, depth + 1) {
                            self.emit_block(tid, indent, depth + 1);
                        } else if self.active_stack.contains(&tid) {
                            if self.loop_context.contains(&tid) {
                                self.push_line(indent, "continue;");
                            } else {
                                self.loop_back_edges.insert(tid);
                            }
                        } else if !self.emitted.contains(&tid) {
                            self.emit_omitted_path(indent, Some(tid));
                        }
                    } else {
                        let target = normalize_target(&ins.target);
                        if target.starts_with("0x") {
                            self.push_line(indent, &format!("return tailCall_{}();", target));
                        } else {
                            self.unresolved_cf += 1;
                            self.push_line(indent, "// unresolved jump");
                        }
                    }
                    self.active_stack.pop();
                    return;
                }
                IROp::Return => {
                    let ret = self
                        .state
                        .reg_values
                        .get("x0")
                        .cloned()
                        .unwrap_or_else(|| "null".to_string());
                    self.push_line(indent, &format!("return {};", ret));
                    self.active_stack.pop();
                    return;
                }
                IROp::Other => {
                    self.apply_other_lift(&ins.src, indent);
                }
            }
        }

        self.active_stack.pop();
    }
}
