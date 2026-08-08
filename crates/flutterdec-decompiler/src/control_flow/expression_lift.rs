use super::*;

impl<'a> FuncEmitter<'a> {
    /// Resolve a register read that is consumed as a whole value.
    ///
    /// A register holding a pool slot becomes the slot's string here, so assignments,
    /// comparisons and returns read like Dart instead of like a pool index. The
    /// dereference paths below deliberately do not call this: `pool[40].f7` is a field
    /// read on the pooled object, and rendering the literal there would claim a field
    /// access on a string. Those keep `pool[<index> /* "value" */]` instead.
    fn resolved_reg_value(&self, reg: &str) -> String {
        let raw = Self::clean_expr(self.state.reg_values.get(reg).cloned().unwrap_or_else(|| reg.to_string()));
        self.annotate_pool_refs(&raw)
    }

    pub(super) fn lookup_reg(&self, token: &str) -> String {
        if is_zero_reg(token) {
            return "0".to_string();
        }
        if let Some(reg) = canonical_reg(token) {
            return self.resolved_reg_value(&reg);
        }
        Self::clean_expr(token.trim().trim_start_matches('#').to_string())
    }

    pub(super) fn operand_expr(&self, token: &str) -> String {
        if is_zero_reg(token) {
            return "0".to_string();
        }
        if let Some(reg) = canonical_reg(token) {
            return self.resolved_reg_value(&reg);
        }

        if let Some(indexed) = self.indexed_expr(token) {
            return indexed;
        }

        if let Some((base, off)) = parse_mem_operand(token) {
            if base == "x29" {
                if let Some(name) = self.locals.get(&off) {
                    return name.clone();
                }
                return local_name(off);
            }

            let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
            return Self::clean_expr(Self::field_expr(&base_expr, off));
        }

        Self::clean_expr(token.trim().trim_start_matches('#').to_string())
    }

    /// Whether the lifter models this instruction at all.
    ///
    /// Anything else reaches `apply_other_lift`'s fallthrough and is silently
    /// discarded: no statement, no state change, no counter. Floating point,
    /// vector work and load/store pairs all land here. Knowing this matters for
    /// any pass that reads "emitted nothing" as "does nothing", which would
    /// otherwise delete real computation.
    /// Indexed access rendered as an element read rather than as field zero.
    ///
    /// A shift of 3 scales by the machine word, which is the element size of a
    /// list or array, so the index register holds an element index. Any other
    /// scale is stated arithmetically rather than implied.
    pub(super) fn indexed_expr(&self, token: &str) -> Option<String> {
        let operand = parse_indexed_operand(token)?;
        let base_expr = self
            .state
            .reg_values
            .get(&operand.base)
            .cloned()
            .unwrap_or_else(|| operand.base.clone());
        let index_expr = self
            .state
            .reg_values
            .get(&operand.index)
            .cloned()
            .unwrap_or_else(|| operand.index.clone());
        // A 32-bit extended index is the low half of the register, so saying so
        // is the difference between an index and a claim about one.
        let index_expr = match operand.extend {
            IndexExtend::None => index_expr,
            IndexExtend::Unsigned32 => format!("({index_expr} & 0xffffffff)"),
            IndexExtend::Signed32 => format!("signExtend32({index_expr})"),
        };
        // A shift of 3 scales by the machine word, which is the element size of
        // a list or array, so the index register holds an element index. Any
        // other scale is stated arithmetically rather than implied.
        let subscript = match operand.shift {
            0 | 3 => index_expr,
            n => format!("({} * {})", index_expr, 1u64 << n),
        };
        Some(Self::clean_expr(format!("{base_expr}[{subscript}]")))
    }

    pub(super) fn lifts_mnemonic(mnemonic: &str) -> bool {
        matches!(
            mnemonic,
            "mov"
                | "add"
                | "sub"
                | "mul"
                | "and"
                | "orr"
                | "eor"
                | "lsl"
                | "lsr"
                | "asr"
                | "ubfx"
                | "ldur"
                | "ldr"
                | "stur"
                | "str"
                | "cmp"
                | "ret"
        )
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
                if let Some(target) = self.indexed_expr(&ops[1]) {
                    let rhs = self.operand_expr(&ops[0]);
                    self.update_selector_binding_from_assignment(&target, &rhs);
                    self.push_line(indent, &format!("{} = {};", target, rhs));
                } else if let Some((base, off)) = parse_mem_operand(&ops[1]) {
                    let rhs = self.operand_expr(&ops[0]);
                    if base == "x29" {
                        let local = self
                            .locals
                            .get(&off)
                            .cloned()
                            .unwrap_or_else(|| local_name(off));
                        self.update_selector_binding_from_assignment(&local, &rhs);
                        self.push_line(indent, &format!("{} = {};", local, rhs));
                    } else {
                        let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
                        let lhs = Self::field_expr(&base_expr, off);
                        self.update_selector_binding_from_assignment(&lhs, &rhs);
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
}
