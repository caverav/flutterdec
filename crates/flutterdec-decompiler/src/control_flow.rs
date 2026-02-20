use super::*;

impl<'a> FuncEmitter<'a> {
    pub(super) fn lookup_reg(&self, token: &str) -> String {
        if is_zero_reg(token) {
            return "0".to_string();
        }
        if let Some(reg) = canonical_reg(token) {
            return Self::clean_expr(self.state.reg_values.get(&reg).cloned().unwrap_or(reg));
        }
        Self::clean_expr(token.trim().trim_start_matches('#').to_string())
    }

    pub(super) fn operand_expr(&self, token: &str) -> String {
        if is_zero_reg(token) {
            return "0".to_string();
        }
        if let Some(reg) = canonical_reg(token) {
            return Self::clean_expr(self.state.reg_values.get(&reg).cloned().unwrap_or(reg));
        }

        if let Some((base, off)) = parse_mem_operand(token) {
            if base == "x29" {
                if let Some(name) = self.locals.get(&off) {
                    return Self::clean_expr(name.clone());
                }
                return Self::clean_expr(local_name(off));
            }

            let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
            return Self::clean_expr(Self::field_expr(&base_expr, off));
        }

        Self::clean_expr(token.trim().trim_start_matches('#').to_string())
    }

    pub(super) fn apply_other_lift(&mut self, ins_src: &str, indent: usize) {
        let (mnemonic, ops) = split_instruction(ins_src);

        match mnemonic.as_str() {
            "mov" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let rhs = self.operand_expr(&ops[1]);
                    self.state.reg_values.insert(dst, rhs);
                }
            }
            "add" | "sub" | "mul" | "and" | "orr" | "eor" if ops.len() >= 3 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let lhs = self.operand_expr(&ops[1]);
                    let mut rhs = self.operand_expr(&ops[2]);
                    if ops.len() > 3 {
                        rhs = format!("{} /* {} */", rhs, ops[3..].join(", "));
                    }
                    let op = match mnemonic.as_str() {
                        "add" => "+",
                        "sub" => "-",
                        "mul" => "*",
                        "and" => "&",
                        "orr" => "|",
                        "eor" => "^",
                        _ => "?",
                    };
                    let expr = if mnemonic == "add" || mnemonic == "sub" {
                        simplify_bin_expr(lhs, op, rhs)
                    } else {
                        format!("({} {} {})", lhs, op, rhs)
                    };
                    self.state.reg_values.insert(dst, expr);
                }
            }
            "lsl" | "lsr" | "asr" if ops.len() >= 3 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let lhs = self.operand_expr(&ops[1]);
                    let rhs = self.operand_expr(&ops[2]);
                    let op = match mnemonic.as_str() {
                        "lsl" => "<<",
                        "lsr" => ">>",
                        "asr" => ">>",
                        _ => "?",
                    };
                    self.state
                        .reg_values
                        .insert(dst, format!("({} {} {})", lhs, op, rhs));
                }
            }
            "ubfx" if ops.len() >= 4 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let src = self.operand_expr(&ops[1]);
                    let lsb = self.operand_expr(&ops[2]);
                    let width = self.operand_expr(&ops[3]);
                    self.state
                        .reg_values
                        .insert(dst, format!("bitField({}, {}, {})", src, lsb, width));
                }
            }
            "ldur" | "ldr" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let rhs = self.operand_expr(&ops[1]);
                    self.state.reg_values.insert(dst, rhs);
                }
            }
            "stur" | "str" if ops.len() >= 2 => {
                if let Some((base, off)) = parse_mem_operand(&ops[1]) {
                    let rhs = self.operand_expr(&ops[0]);
                    if base == "x29" {
                        let local = self
                            .locals
                            .get(&off)
                            .cloned()
                            .unwrap_or_else(|| local_name(off));
                        self.push_line(indent, &format!("{} = {};", local, rhs));
                    } else {
                        let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
                        let lhs = Self::field_expr(&base_expr, off);
                        self.push_line(indent, &format!("{} = {};", lhs, rhs));
                    }
                }
            }
            "cmp" if ops.len() >= 2 => {
                let lhs = self.operand_expr(&ops[0]);
                let rhs = self.operand_expr(&ops[1]);
                self.state.last_cmp = Some((lhs, rhs));
            }
            "ret" => {}
            _ => {}
        }
    }

    pub(super) fn branch_condition(&self, mnemonic: &str, ops: &[String]) -> Option<String> {
        if mnemonic.starts_with("b.") {
            if let Some(cmp) = &self.state.last_cmp {
                return cond_from_cmp(mnemonic, cmp);
            }
            return Some(format!("flags.{}", mnemonic.replace('.', "_")));
        }

        if mnemonic == "cbz" && !ops.is_empty() {
            let v = self.operand_expr(&ops[0]);
            return Some(format!("{} == 0", v));
        }
        if mnemonic == "cbnz" && !ops.is_empty() {
            let v = self.operand_expr(&ops[0]);
            return Some(format!("{} != 0", v));
        }
        if mnemonic == "tbz" && ops.len() >= 2 {
            let v = self.operand_expr(&ops[0]);
            let bit = self.lookup_reg(&ops[1]);
            return Some(format!("(({} >> {}) & 1) == 0", v, bit));
        }
        if mnemonic == "tbnz" && ops.len() >= 2 {
            let v = self.operand_expr(&ops[0]);
            let bit = self.lookup_reg(&ops[1]);
            return Some(format!("(({} >> {}) & 1) != 0", v, bit));
        }

        None
    }

    pub(super) fn branch_target_block(&self, target: &str) -> Option<usize> {
        let normalized = normalize_target(target);
        let va = normalized.strip_prefix("0x")?;
        let parsed = u64::from_str_radix(va, 16).ok()?;
        self.va_to_id.get(&parsed).copied()
    }

    pub(super) fn can_inline(&self, to: usize, depth: usize) -> bool {
        if depth >= 12 {
            return false;
        }
        if self.active_stack.contains(&to) {
            return false;
        }
        if self.inline_visits.get(&to).copied().unwrap_or(0) >= self.visit_limit(to) {
            return false;
        }
        self.block_by_id.contains_key(&to)
    }

    pub(super) fn has_backedge_pred(&self, id: usize) -> bool {
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        for pred in &block.preds {
            if let Some(pb) = self.block_by_id.get(pred) {
                if pb.succs.contains(&id) && pb.start_va >= block.start_va {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn has_forward_pred(&self, id: usize) -> bool {
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        for pred in &block.preds {
            if let Some(pb) = self.block_by_id.get(pred) {
                if pb.succs.contains(&id) && pb.start_va < block.start_va {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn should_wrap_loop_header(&self, id: usize, depth: usize) -> bool {
        if depth >= 10 {
            return false;
        }
        if !self.loop_context.is_empty() {
            return false;
        }
        if self.loop_context.contains(&id) {
            return false;
        }
        if self.active_stack.contains(&id) {
            return false;
        }
        if self.inline_visits.get(&id).copied().unwrap_or(0) >= self.visit_limit(id) {
            return false;
        }
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        let tail = block.instrs.last().map(|i| &i.op);
        if !matches!(tail, Some(IROp::Branch)) {
            return false;
        }
        if block.succs.len() < 2 {
            return false;
        }
        self.has_backedge_pred(id) && self.has_forward_pred(id)
    }

    pub(super) fn emit_wrapped_loop(&mut self, id: usize, indent: usize, depth: usize) {
        self.loop_context.push(id);
        self.push_line(indent, "while (true) {");
        self.emit_block(id, indent + 1, depth + 1);
        self.push_line(indent + 1, "break;");
        self.push_line(indent, "}");
        self.loop_context.pop();
    }

    pub(super) fn emit_call(&mut self, ins_target: &str, indent: usize) {
        self.total_calls += 1;
        self.state.call_index += 1;

        let tname = format!("t{}", self.state.call_index);
        let args = (0..4)
            .map(|r| {
                self.state
                    .reg_values
                    .get(&format!("x{r}"))
                    .cloned()
                    .unwrap_or_else(|| format!("arg{r}"))
            })
            .collect::<Vec<_>>()
            .join(", ");

        let target = normalize_target(ins_target);
        if target.starts_with('x') {
            self.indirect_calls += 1;
            self.raw_register_calls += 1;
            let named_target = named_indirect_target(&target);
            self.push_line(
                indent,
                &format!(
                    "final {} = dynamicCall({}, [{}]);",
                    tname, named_target, args
                ),
            );
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
            self.push_line(
                indent,
                &format!("final {} = {}({});", tname, call_name, args),
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
