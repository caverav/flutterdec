impl<'a> FuncEmitter<'a> {
    fn strip_wrapped_expr(expr: &str) -> &str {
        let mut cur = expr.trim();
        while let Some(inner) = Self::strip_outer_parens_once(cur) {
            cur = inner.trim();
        }
        cur
    }

    fn is_simple_identifier_expr(expr: &str) -> bool {
        let t = expr.trim();
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    }

    fn is_simple_selector_member_expr(expr: &str) -> bool {
        let t = expr.trim();
        t.contains('.')
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
    }

    fn canonical_stack_slot_expr(expr: &str) -> Option<String> {
        let (base, off) = parse_stack_base_offset(expr.trim())?;
        Some(format!("{base}[{}]", fmt_int(off)))
    }

    fn selector_binding_key(expr: &str) -> Option<String> {
        let trimmed = Self::strip_wrapped_expr(expr);
        if trimmed.is_empty() {
            return None;
        }
        if Self::is_simple_identifier_expr(trimmed) {
            return Some(trimmed.to_string());
        }
        if Self::is_simple_selector_member_expr(trimmed) {
            return Some(trimmed.to_string());
        }
        Self::canonical_stack_slot_expr(trimmed)
    }

    fn is_simple_identifier_key(key: &str) -> bool {
        Self::is_simple_identifier_expr(key)
    }

    fn purge_selector_hints_for_base(&mut self, base: &str) {
        let prefix = format!("{base}.");
        self.state
            .selector_hints
            .retain(|k, _| k != base && !k.starts_with(&prefix));
    }

    fn propagate_selector_hints_for_base_alias(&mut self, dst_base: &str, src_base: &str) {
        if dst_base == src_base {
            return;
        }
        let src_prefix = format!("{src_base}.");
        let entries = self
            .state
            .selector_hints
            .iter()
            .filter_map(|(k, v)| {
                if k == src_base {
                    Some((dst_base.to_string(), v.clone()))
                } else {
                    k.strip_prefix(&src_prefix)
                        .map(|suffix| (format!("{dst_base}.{suffix}"), v.clone()))
                }
            })
            .collect::<Vec<_>>();
        for (k, v) in entries {
            self.state.selector_hints.insert(k, v);
        }
    }

    fn selector_hint_from_expr(&self, expr: &str) -> Option<String> {
        let mut cur = expr.trim();
        if cur.is_empty() {
            return None;
        }

        let direct = [cur.to_string()];
        if let Some(sel) = infer_selector_name_from_context(
            &direct,
            &self.pool_value_hints,
            &self.pool_semantic_hints,
        )
        .or_else(|| infer_selector_candidate_from_context(&direct, &self.pool_value_hints))
        {
            return Some(sel);
        }

        if let Some(key) = Self::selector_binding_key(cur) {
            if let Some(sel) = self.state.selector_hints.get(&key) {
                return Some(sel.clone());
            }
        }

        if let Some(inner) = cur.strip_prefix("classId(").and_then(|s| s.strip_suffix(')')) {
            if let Some(sel) = self.selector_hint_from_expr(inner) {
                return Some(sel);
            }
        }

        while let Some(inner) = Self::strip_outer_parens_once(cur) {
            cur = inner.trim();
            if cur.is_empty() {
                break;
            }
            let nested = [cur.to_string()];
            if let Some(sel) = infer_selector_name_from_context(
                &nested,
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            )
            .or_else(|| infer_selector_candidate_from_context(&nested, &self.pool_value_hints))
            {
                return Some(sel);
            }
            if let Some(key) = Self::selector_binding_key(cur) {
                if let Some(sel) = self.state.selector_hints.get(&key) {
                    return Some(sel.clone());
                }
            }
        }

        None
    }

    fn selector_context_expr(&self, expr: &str) -> String {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return expr.to_string();
        }
        let direct = [trimmed.to_string()];
        if infer_selector_name_from_context(&direct, &self.pool_value_hints, &self.pool_semantic_hints)
            .is_some()
        {
            return trimmed.to_string();
        }
        if let Some(sel) = self.selector_hint_from_expr(trimmed) {
            return format!("\"{}\"", Self::escape_hint_text(&sel));
        }
        trimmed.to_string()
    }

    fn update_selector_binding_from_assignment(&mut self, lhs: &str, rhs: &str) {
        let Some(key) = Self::selector_binding_key(lhs) else {
            return;
        };

        if Self::is_simple_identifier_key(&key) {
            self.purge_selector_hints_for_base(&key);
            if let Some(rhs_key) = Self::selector_binding_key(rhs) {
                if Self::is_simple_identifier_key(&rhs_key) {
                    self.propagate_selector_hints_for_base_alias(&key, &rhs_key);
                }
            }
        }

        if let Some(sel) = self.selector_hint_from_expr(rhs) {
            self.state.selector_hints.insert(key, sel);
        } else {
            self.state.selector_hints.remove(&key);
        }
    }

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
        // Pool slots are resolved once, when the load lands in a register. Callers
        // downstream re-annotate their operands, so bail out on text that already
        // carries a resolved hint instead of nesting a second comment inside the first.
        if expr.contains("/* pool[") || expr.contains("/* \"") {
            return expr.to_string();
        }

        let normalized = normalize_pool_page_field_exprs(expr);
        let exact = self.render_pool_value_hint(&normalized);
        if exact != normalized {
            return exact;
        }

        let mut out = String::new();
        let bytes = normalized.as_bytes();
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
        collect_pool_indices(expr)
    }

    fn is_generic_call_name(name: &str) -> bool {
        is_generic_symbol_placeholder(name)
    }

    fn is_numeric_literal_expr(expr: &str) -> bool {
        let t = expr.trim();
        if t.is_empty() {
            return false;
        }
        let rest = t.strip_prefix('-').unwrap_or(t);
        if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
            return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
        }
        rest.chars().all(|c| c.is_ascii_digit())
    }

    fn render_callable_fallback_target(expr: &str) -> Option<String> {
        let t = expr.trim();
        if t.is_empty()
            || t.eq_ignore_ascii_case("null")
            || (t.starts_with('"') && t.ends_with('"'))
            || Self::is_numeric_literal_expr(t)
        {
            return None;
        }

        let simple = t.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '_' | '.' | '[' | ']' | '-' | '+' | '*' | '/' | '$' | '#'
                )
        });
        if simple {
            Some(t.to_string())
        } else {
            Some(format!("({})", t))
        }
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

    pub(super) fn emit_call(&mut self, ins_target: &str, va: u64, indent: usize) {
        self.total_calls += 1;
        self.call_index += 1;

        let tname = format!("t{}", self.call_index);
        // Arguments in `DartCallingConvention` order, truncated to the last
        // position this call site gives evidence for. A register still holding
        // its entry seed was never written between function entry and this
        // call, so nothing here says it is an argument: 44.7% of direct calls
        // define no argument register at all, and 96% define at most three,
        // measured over 325,376 calls. Emitting a fixed six would claim an
        // arity the code does not show, the same overreach as the fixed four
        // this replaces. Interior seeds are kept, because a later defined
        // argument proves the positions before it are part of the list.
        //
        // A lower bound, like `DispatchCall::argument_registers`: a
        // pass-through argument forwarded unchanged is missed, and stack-passed
        // arguments are not modelled at all.
        let mut prefix: Vec<(bool, String)> = DART_ARGUMENT_REGISTERS
            .iter()
            .enumerate()
            .map(|(i, reg)| match self.capped_reg_value(reg).as_ref() {
                // Untouched since entry: nothing here says this is an argument.
                Some(v) if *v == format!("arg{i}") => (false, format!("arg{i}")),
                Some(v) => (true, v.clone()),
                // Invalidated, so something wrote it, but x5-x7 are also
                // general scratch in `kDartAvailableCpuRegs`, so a write is no
                // evidence of an argument. `regN` reports the gap without
                // counting towards the arity.
                None => (false, (*reg).to_string()),
            })
            .collect();
        while prefix.last().is_some_and(|(informative, _)| !informative) {
            prefix.pop();
        }
        let raw_arg_values: Vec<String> = prefix.into_iter().map(|(_, v)| v).collect();
        let selector_context_values = raw_arg_values
            .iter()
            .map(|a| self.selector_context_expr(a))
            .collect::<Vec<_>>();
        let selector_intent =
            infer_selector_intent_from_context(
                &selector_context_values,
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            );
        let selector_name = infer_selector_name_from_context(
            &selector_context_values,
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

            // A dispatch-table call names its selector in the instruction
            // stream. That is recovered, not guessed, so it takes precedence
            // over every heuristic below.
            if let Some(dispatch) = self.dispatch_calls.get(&va) {
                let selector = dispatch_selector_name(dispatch.selector_offset);
                // A receiver the recogniser could not attribute to a register is
                // reported as unrecovered rather than given a name. Rendering the
                // word `dispatch` in the receiver position read as an object the
                // call was made on, which is a claim: 2,117 and 3,368 sites, 34%
                // and 33% of all dispatch-table calls. Without a receiver the
                // selector stands alone, which says only what is known.
                let receiver = dispatch.receiver.as_ref().map(|reg| self.lookup_reg(reg));
                // Only argument registers the call site actually defined, in
                // `DartCallingConvention` order. A lower bound on the real
                // argument list: stack-passed arguments are not modelled, and a
                // pass-through parameter defined in an earlier block is missed.
                let dispatch_args = dispatch
                    .argument_registers
                    .iter()
                    .map(|reg| self.annotate_pool_refs(&self.lookup_reg(reg)))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.dispatch_table_calls += 1;
                // The argument list is never claimed to be complete, so an
                // empty one is not read as a recovered zero-arity method.
                let arity = if dispatch.argument_registers.is_empty() {
                    "args: unknown"
                } else {
                    "args: lower bound"
                };
                let callee = match &receiver {
                    Some(receiver) => format!("{receiver}.{selector}"),
                    None => selector.clone(),
                };
                let receiver_note = if receiver.is_some() {
                    ""
                } else {
                    ", receiver: unrecovered"
                };
                self.push_line(
                    indent,
                    &format!(
                        "final {} = {}({}); // dispatch table, selector_offset: {}, {}{}",
                        tname,
                        callee,
                        dispatch_args,
                        dispatch.selector_offset,
                        arity,
                        receiver_note
                    ),
                );
                self.clobber_call_registers();
                self.state.reg_values.insert("x0".to_string(), tname);
                return;
            }
            let named_target = named_indirect_target(&target);
            let target_value = self
                .capped_reg_value(&target)
                .unwrap_or_else(|| named_target.clone());
            let target_selector_context = self.selector_context_expr(&target_value);
            let target_selector_intent = infer_selector_intent_from_context(
                std::slice::from_ref(&target_selector_context),
                &self.pool_value_hints,
                &self.pool_semantic_hints,
            );
            let target_selector_name = infer_selector_name_from_context(
                std::slice::from_ref(&target_selector_context),
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
            // An SDK fact about the callee outranks a heuristic rename of it. The
            // intent rewrite below fires first otherwise, and 1,180 error-stub
            // sites resolve that way and keep the Dart-call model.
            if let Some((_, target_va)) = self
                .resolve_indirect_target_symbol_call_name(&target_value)
                .filter(|(_, va)| self.runtime_stubs.contains_key(va))
            {
                self.semantic_indirect_calls += 1;
                self.target_va_symbol_calls += 1;
                self.emit_runtime_stub_call(target_va, &tname, indent);
                return;
            }
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
                if let Some(candidate) = infer_selector_candidate_from_context(
                    &selector_context_values,
                    &self.pool_value_hints,
                ) {
                    comments.push(format!("selector candidate, unverified: {candidate}"));
                }
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
                                "final {} = {}({});{}",
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
                            "final {} = {}({});{}",
                            tname, named_target, args, suffix
                        ),
                    );
                } else {
                    let target_suffix = if comments.is_empty() {
                        String::new()
                    } else {
                        format!(" // {}", comments.join(", "))
                    };
                    let fallback_target_expr = if target_value != named_target {
                        self.display_indirect_target_value(&target_value)
                    } else {
                        named_target.clone()
                    };
                    if let Some(callable_target) =
                        Self::render_callable_fallback_target(&fallback_target_expr)
                    {
                        if !comments
                            .iter()
                            .any(|c| c == &format!("indirect via: {}", named_target))
                        {
                            comments.push(format!("indirect via: {}", named_target));
                        }
                        let suffix = if comments.is_empty() {
                            String::new()
                        } else {
                            format!(" // {}", comments.join(", "))
                        };
                        self.push_line(
                            indent,
                            &format!("final {} = {}({});{}", tname, callable_target, args, suffix),
                        );
                    } else {
                        self.push_line(
                            indent,
                            &format!(
                                "final {} = dynamicCall({}, [{}]);{}",
                                tname, named_target, args, target_suffix
                            ),
                        );
                    }
                }
            }
        } else if let Some(va) = u64::from_str_radix(target.trim_start_matches("0x"), 16)
            .ok()
            .filter(|va| self.runtime_stubs.contains_key(va))
        {
            self.emit_runtime_stub_call(va, &tname, indent);
            return;
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
            .or(selector_intent)
            .or(library_intent.clone());
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
        self.clobber_call_registers();
        self.state.reg_values.insert("x0".to_string(), tname);
    }

    /// Emits a call into a known runtime stub, modelled from the SDK rather than
    /// as a Dart call.
    ///
    /// Three facts differ. A stub's inputs are not in `DartCallingConvention`, so
    /// the inferred argument list would describe the wrong thing entirely and is
    /// dropped. A `GenerateSharedStub` saves and restores every non-reserved
    /// register around its runtime call (`stub_code_compiler_arm64.cc:300,309`),
    /// so it clobbers nothing, which matters because these are the commonest
    /// calls in the binary. And it defines a value only when the SDK stores the
    /// runtime result, which is the mint allocator alone: `stackOverflow` comes
    /// back having defined nothing, and the type test preserves `R0` and answers
    /// in `R7`, so binding either would claim a value that does not exist.
    fn emit_runtime_stub_call(&mut self, va: u64, tname: &str, indent: usize) {
        let Some(effect) = self.runtime_stubs.get(&va).copied() else {
            return;
        };
        let name = self
            .symbol_names
            .get(&va)
            .map(|n| sanitize_name(n))
            .unwrap_or_else(|| format!("fn_0x{va:x}"));
        if effect.writes_result {
            self.push_line(indent, &format!("final {tname} = {name}();"));
        } else {
            self.push_line(indent, &format!("{name}();"));
        }
        if !effect.preserves_registers {
            self.clobber_call_registers();
        }
        if effect.writes_result {
            self.state
                .reg_values
                .insert("x0".to_string(), tname.to_string());
        }
    }

    /// Drop the bindings a call does not preserve.
    ///
    /// Called after the arguments, selector context and callee target have all
    /// been resolved, because `blr x16`/`x17`/`x30` read registers in this set.
    ///
    /// Without it a value set before a call still read as that value after it.
    /// `mov x0, x22` followed by a call left x0 bound to `null`, so a later
    /// `[x0, #-1]` rendered `null._tag`: a header read off null, which cannot
    /// happen and reads as resolved fact rather than as a gap. 2,497 such reads
    /// on one sample, against a single genuine load off NULL_REG in the whole
    /// binary.
    fn clobber_call_registers(&mut self) {
        for reg in CALL_CLOBBERED_REGISTERS {
            self.state.reg_values.remove(*reg);
        }
        // NZCV is caller-saved, so a call does not preserve the comparison
        // either. Left set, `cmp` then a call then `b.eq` rendered the pre-call
        // comparison as the condition, which is the same stale-flag class as an
        // unmodelled flag writer and feeds the structurer.
        self.state.last_cmp = None;
        self.state.selector_hints.clear();
    }

    /// Predecessor map and per-block written registers, built once.
    ///
    /// Computed from `ir.blocks` rather than from `Regions`, because the DFS
    /// emitter runs precisely when region recovery was unavailable or
    /// structuring failed, and `Regions::build` returns `None` for an
    /// irreducible CFG.
    fn ensure_dfs_maps(&mut self) {
        if self.dfs_preds.is_some() {
            return;
        }
        let mut preds: HashMap<usize, Vec<usize>> = HashMap::new();
        for block in &self.ir.blocks {
            let mut written = HashSet::new();
            for ins in &block.instrs {
                let (mnemonic, ops) = split_instruction(&ins.src);
                written.extend(written_registers(&mnemonic, &ops));
                if matches!(ins.op, IROp::Call) {
                    written.extend(CALL_CLOBBERED_REGISTERS.iter().map(|r| (*r).to_string()));
                }
            }
            self.dfs_block_writes.insert(block.id, written);
            // One edge per distinct predecessor: a branch whose two targets are
            // the same block has one predecessor, not two, and would otherwise
            // trigger a merge on a block only one path reaches.
            for succ in &block.succs {
                let entry = preds.entry(*succ).or_default();
                if !entry.contains(&block.id) {
                    entry.push(block.id);
                }
            }
        }
        // The map records only edges a `succs` list names, so the implicit path
        // into the entry block is absent. That would matter if the entry block
        // were also the target of a back edge: it would show one predecessor, the
        // merge below would decline, and bindings written by the loop body would
        // describe the header on the first iteration only.
        //
        // It does not occur. Across 14,129 functions on the two sample binaries,
        // the entry block is the target of a branch or jump exactly zero times,
        // because a Dart AOT prologue -- the frame push and the stack-limit check
        // -- always precedes any loop header, so a header is never block 0. The
        // structured emitter also merges at a loop header before rendering the
        // body regardless of predecessor count, so only this fallback would be
        // affected. Adding a sentinel predecessor here would be untestable
        // against real input; if the shape ever appears, this is where to add it.
        self.dfs_preds = Some(preds);
    }

    /// Registers written by any block that can reach `id`.
    ///
    /// Returns `None` when only one path reaches `id`, because then that path
    /// does describe the state and nothing needs dropping.
    ///
    /// Backward reachability rather than the whole function: everything strictly
    /// downstream of `id` cannot have run yet, and in a large function that is
    /// most of it. A back edge puts a loop body back in the set, which is
    /// correct, since a second iteration reaches the block again.
    fn registers_written_before(&mut self, id: usize) -> Option<HashSet<String>> {
        self.ensure_dfs_maps();
        let preds = self.dfs_preds.as_ref().expect("dfs preds");
        if preds.get(&id).map(Vec::len).unwrap_or(0) < 2 {
            return None;
        }
        let mut seen: HashSet<usize> = HashSet::new();
        let mut stack: Vec<usize> = preds.get(&id).cloned().unwrap_or_default();
        let mut written = HashSet::new();
        while let Some(block) = stack.pop() {
            // `id` itself is reached only when it is backward-reachable from its
            // own predecessors, that is when it sits in a cycle. Its writes are
            // then live at re-entry on the second iteration, so they belong in
            // the set: excluding them lost everything a loop header writes.
            if !seen.insert(block) {
                continue;
            }
            if let Some(w) = self.dfs_block_writes.get(&block) {
                written.extend(w.iter().cloned());
            }
            if let Some(ps) = preds.get(&block) {
                stack.extend(ps.iter().copied());
            }
        }
        Some(written)
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

        // The DFS emitter has no join merge. It emits a block once, guarded by
        // `emitted`, under whichever path reached it first, so a register set on
        // that path reads as its value on every other path too. `mov x0, x22`
        // before a branch left x0 bound to `null`, and a shared successor then
        // rendered `null._tag`: a header read off the null object. 1,922 such
        // reads across 71 functions.
        //
        // A block with one predecessor is fine, because only one path reaches
        // it. For the rest, nothing here describes the state, so the bindings a
        // predecessor could have written are dropped. Arm isolation already
        // restores the pre-branch state around each arm, which is why this is
        // needed as well: restoring is not merging, it asserts the pre-branch
        // state still holds.
        if let Some(written) = self.registers_written_before(id) {
            self.merge_state_at_join(&written);
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
                    self.emit_call(&ins.target, ins.va, indent);
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
                        .capped_reg_value("x0")
                        .unwrap_or_else(|| "null".to_string());
                    self.push_line(indent, &format!("return {};", ret));
                    self.active_stack.pop();
                    return;
                }
                // Runtime bookkeeping: no user-level statement, and the
                // comparison must not leak into a later branch condition.
                IROp::RuntimeCheck => {}
                IROp::Other => {
                    self.apply_other_lift(&ins.src, indent);
                }
            }
        }

        // A block whose last instruction is not a terminator falls through.
        // The arms above only recurse on Branch/Jump/Return, so without this
        // the remainder of the function is silently dropped.
        if let Some(&next) = block.succs.first() {
            if block.succs.len() == 1 {
                if self.can_inline(next, depth + 1) {
                    self.emit_block(next, indent, depth + 1);
                } else if self.active_stack.contains(&next) {
                    if self.loop_context.contains(&next) {
                        self.push_line(indent, "continue;");
                    } else {
                        self.loop_back_edges.insert(next);
                    }
                } else if !self.emitted.contains(&next) {
                    self.emit_omitted_path(indent, Some(next));
                }
            }
        }

        self.active_stack.pop();
    }
}
