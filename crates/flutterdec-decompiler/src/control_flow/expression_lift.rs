use super::*;

/// Longest substituted expression a register read will inline.
///
/// Every modelled instruction builds its value out of the text of the values it
/// reads, so a register that feeds itself grows the string geometrically. A hash
/// or mixing routine is the worst case: 217 straight-line instructions of
/// `add`/`and`/`lsl` over three registers that reference each other reached a
/// one-gigabyte allocation and killed the process. There is no branch involved, so
/// no visit or depth limit applies.
///
/// Past this size the register reads as itself, which renders as `regN`. That is an
/// admitted gap, and an admitted gap is what the whole emitter prefers to a value
/// nobody can read.
///
/// A value this size still passes, so a binary operator over two of them stores
/// roughly twice the constant: the effective bound on a stored value is about
/// 2x, not 1x. Every read that substitutes into a larger expression goes through
/// `capped_reg_value`, which is what keeps that from compounding.
const MAX_SUBSTITUTED_EXPR: usize = 512;

impl<'a> FuncEmitter<'a> {
    /// Resolve a register read that is consumed as a whole value.
    ///
    /// A register holding a pool slot becomes the slot's string here, so assignments,
    /// comparisons and returns read like Dart instead of like a pool index. The
    /// dereference paths below deliberately do not call this: `pool[40].f7` is a field
    /// read on the pooled object, and rendering the literal there would claim a field
    /// access on a string. Those keep `pool[<index> /* "value" */]` instead.
    /// The value bound to `reg`, unless it has grown past what is worth inlining.
    ///
    /// Every read that substitutes a value into a larger expression goes through
    /// here, so this is the single place the growth is bounded.
    fn capped_reg_value(&self, reg: &str) -> Option<String> {
        self.state
            .reg_values
            .get(reg)
            .filter(|value| value.len() <= MAX_SUBSTITUTED_EXPR)
            .cloned()
    }

    fn resolved_reg_value(&self, reg: &str) -> String {
        let raw = Self::clean_expr(
            self.capped_reg_value(reg)
                .unwrap_or_else(|| reg.to_string()),
        );
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

            let base_expr = self.capped_reg_value(&base).unwrap_or(base);
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
            .capped_reg_value(&operand.base)
            .unwrap_or_else(|| operand.base.clone());
        let index_expr = self
            .capped_reg_value(&operand.index)
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
                | "sbfx"
                | "sbfiz"
                | "ldur"
                | "ldr"
                | "ldrb"
                | "ldurb"
                | "ldrh"
                | "ldurh"
                | "ldrsb"
                | "ldursb"
                | "ldrsh"
                | "ldursh"
                | "ldrsw"
                | "ldursw"
                | "ldp"
                | "ldnp"
                | "stur"
                | "str"
                | "strb"
                | "sturb"
                | "strh"
                | "sturh"
                | "csel"
                | "cset"
                | "csetm"
                | "cmp"
                | "fcmp"
                | "subs"
                | "adds"
                | "ands"
                | "tst"
                | "cmn"
                | "movk"
                | "sxtw"
                | "neg"
                | "mvn"
                | "sdiv"
                | "udiv"
                | "umulh"
                | "smulh"
                | "msub"
                | "madd"
                | "ubfiz"
                | "ret"
        )
    }

    /// An operand with its trailing shift or extend modifier applied.
    ///
    /// ARM64 lets the last source operand carry `lsl`/`lsr`/`asr`/`ror` or an
    /// extend, and an extend may carry a shift amount of its own. Rendering the
    /// operand alone with the modifier as a trailing comment reads as an
    /// unmodified value: `cmp x3, x0, asr #1` became `a == b` where the truth is
    /// `a == (b >> 1)`, and that is a condition, so the structurer sees it too.
    /// `lsr` is Dart's `>>>`, because `>>` is arithmetic.
    fn shifted_operand_expr(&self, ops: &[String], index: usize) -> String {
        let base = self.operand_expr(&ops[index]);
        let Some(modifier) = ops.get(index + 1) else {
            return base;
        };
        let modifier = modifier.trim().to_ascii_lowercase();
        let (kind, amount) = match modifier.split_once('#') {
            Some((kind, amount)) => (kind.trim(), Some(amount.trim().to_string())),
            None => (modifier.as_str(), None),
        };
        // An extend narrows first, then shifts. Dropping the shift amount would
        // render a scaled index as unscaled, which is a wrong value rather than
        // a missing one: `sxtw #3` scales by the machine word.
        let extended = match kind {
            "sxtb" => Some(format!("signExtend({base}, 8)")),
            "sxth" => Some(format!("signExtend({base}, 16)")),
            "sxtw" => Some(format!("signExtend({base}, 32)")),
            "sxtx" | "uxtx" => Some(base.clone()),
            "uxtb" => Some(format!("({base} & 0xff)")),
            "uxth" => Some(format!("({base} & 0xffff)")),
            "uxtw" => Some(format!("({base} & 0xffffffff)")),
            _ => None,
        };
        if let Some(extended) = extended {
            return match amount.as_deref() {
                Some(shift) if shift != "0" => {
                    simplify_bin_expr(extended, "<<", shift.to_string())
                }
                _ => extended,
            };
        }
        match (kind, amount) {
            // Through the simplifier, so a shifted literal folds instead of
            // needing a recogniser of its own downstream.
            ("lsl", Some(n)) => simplify_bin_expr(base, "<<", n),
            ("lsr", Some(n)) => format!("({base} >>> {n})"),
            ("asr", Some(n)) => format!("({base} >> {n})"),
            ("ror", Some(n)) => format!("rotateRight({base}, {n})"),
            // Not a modifier this understands. Returning the bare operand would
            // claim it does nothing, so report what was seen.
            _ => format!("{base} /* {modifier} */"),
        }
    }

    /// Pre- and post-index addressing writes the base register back, so the
    /// base no longer holds what it held before the access. The new value is
    /// the old base plus the displacement, which this lifter does not track,
    /// so the binding is dropped rather than left describing the old address.
    fn invalidate_index_writeback(&mut self, ops: &[String]) {
        let Some(mem) = ops.iter().find(|o| o.trim_start().starts_with('[')) else {
            return;
        };
        // `[x1, #8]!` is pre-index; a displacement operand after the closing
        // bracket, as in `[x1], #8`, is post-index. Both write the base.
        let writes_back = mem.trim_end().ends_with('!')
            || ops
                .last()
                .is_some_and(|o| o.trim_start().starts_with('#') && !o.contains('['));
        if !writes_back {
            return;
        }
        let inside = mem.trim_start().trim_start_matches('[');
        let base = inside.split([',', ']']).next().unwrap_or_default().trim();
        if let Some(reg) = canonical_reg(base) {
            self.state.reg_values.remove(&reg);
        }
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
            // `movk` replaces one 16-bit lane and leaves the rest, which is why
            // this masks rather than ORs: `prior | (imm << s)` is only right when
            // the lane is already zero, and after `mov rd, #-1` it is not. A
            // wrong constant renders as a resolved literal.
            //
            // Every one of the 21,649 sites across both samples follows a
            // `mov rd, #literal`, and 21,621 shift by 16, so this is the second
            // half of a constant materialisation and the merge is exact. The
            // largest remaining unmodelled mnemonic before this.
            "movk" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let shift = match ops.get(2) {
                        None => Some(0u32),
                        Some(op) => op
                            .trim()
                            .strip_prefix("lsl")
                            .and_then(|rest| rest.trim().strip_prefix('#'))
                            .and_then(|n| n.trim().parse::<u32>().ok())
                            // Only these four lanes exist; anything else is a
                            // misparse, and shifting by it would be nonsense.
                            .filter(|s| matches!(s, 0 | 16 | 32 | 48)),
                    };
                    let imm = parse_int(ops[1].trim().trim_start_matches('#'));
                    let prior = self
                        .state
                        .reg_values
                        .get(&dst)
                        .and_then(|v| parse_int(v))
                        .map(|v| v as u64);
                    match (prior, imm, shift) {
                        (Some(prior), Some(imm), Some(shift)) => {
                            let lane = 0xffffu64 << shift;
                            let mut merged = (prior & !lane) | (((imm as u64) & 0xffff) << shift);
                            // A write to a `w` register zero-extends.
                            if ops[0].trim().starts_with('w') {
                                merged &= 0xffff_ffff;
                            }
                            self.state.reg_values.insert(dst, fmt_int(merged as i64));
                        }
                        // An expression cannot have a lane replaced, so the
                        // binding is dropped rather than guessed at.
                        _ => {
                            self.state.reg_values.remove(&dst);
                        }
                    }
                }
            }
            // Exact single-instruction forms, each named for what it computes
            // rather than translated into Dart operators that would imply
            // semantics ARM64 does not have. `sdiv` truncates toward zero and
            // divides by zero to zero; `umulh` is the high half of an unsigned
            // product; `msub` is a fused multiply-subtract.
            "sxtw" | "neg" | "mvn" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let src = self.shifted_operand_expr(&ops, 1);
                    let value = match mnemonic.as_str() {
                        "sxtw" => format!("signExtend({src}, 32)"),
                        // Bracketed for the same reason as the conditionals: a
                        // bare `-a + b` composed into a larger expression negates
                        // only the first term.
                        "neg" => format!("(-{src})"),
                        _ => format!("(~{src})"),
                    };
                    // A `w` form computes in 32 bits and zero-extends into the
                    // 64-bit register. For `and`/`orr`/`eor` of two 32-bit values
                    // the result already fits, and for `add`/`sub` the halves
                    // agree except on overflow, so neither needs saying. Negation
                    // and complement always differ: `~x` sets every high bit
                    // where the machine clears all of them. `sxtw` writes an `x`
                    // destination by definition, so it is never masked.
                    let value = if mnemonic != "sxtw" && is_w_register(&ops[0]) {
                        format!("(({value}) & 0xffffffff)")
                    } else {
                        value
                    };
                    self.state.reg_values.insert(dst, value);
                }
            }
            "sdiv" | "udiv" | "umulh" | "smulh" if ops.len() >= 3 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let a = self.operand_expr(&ops[1]);
                    let b = self.operand_expr(&ops[2]);
                    let name = match mnemonic.as_str() {
                        "sdiv" => "signedDivide",
                        "udiv" => "unsignedDivide",
                        "umulh" => "unsignedHighMultiply",
                        _ => "signedHighMultiply",
                    };
                    self.state.reg_values.insert(dst, format!("{name}({a}, {b})"));
                }
            }
            "msub" | "madd" if ops.len() >= 4 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let a = self.operand_expr(&ops[1]);
                    let b = self.operand_expr(&ops[2]);
                    let c = self.operand_expr(&ops[3]);
                    let op = if mnemonic == "msub" { "-" } else { "+" };
                    self.state
                        .reg_values
                        .insert(dst, format!("({c} {op} ({a} * {b}))"));
                }
            }
            "ubfiz" if ops.len() >= 4 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let src = self.operand_expr(&ops[1]);
                    let lsb = self.operand_expr(&ops[2]);
                    let width = self.operand_expr(&ops[3]);
                    self.state
                        .reg_values
                        .insert(dst, format!("unsignedBitFieldInsert({src}, {lsb}, {width})"));
                }
            }
            // Compressed-pointer decompression. These binaries use compressed
            // pointers: a reference field is a 32-bit offset from the heap base,
            // loaded with `ldur w`, and `x28` is HEAP_BITS, whose high half is
            // `heap_base >> 32` (`runtime/vm/constants_arm64.h`), so
            // `add rD, rS, x28, lsl #32` reconstructs the full pointer.
            //
            // The Dart-level value does not change, so the add is transparent
            // and the destination reads as whatever the compressed load read.
            // 65,759 and 103,731 sites, and 87% of every 32-bit load feeds one,
            // so rendering it as arithmetic put `+ (reg28 << 0x20)` inside a
            // quarter of all field reads.
            "add"
                if ops.len() == 4
                    && canonical_reg(&ops[2]).as_deref() == Some("x28")
                    && ops[3].trim().eq_ignore_ascii_case("lsl #32") =>
            {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let value = self.operand_expr(&ops[1]);
                    self.state.reg_values.insert(dst, value);
                }
            }
            // `kTrueOffsetFromNull` and `kFalseOffsetFromNull`
            // (`runtime/vm/pointer_tagging.h`): the two canonical bools sit at
            // fixed offsets from null, so Dart materialises them by adding to
            // NULL_REG. 11,736 and 21,165 sites on the two samples, and every
            // one of them used to render as `null + 0x20`.
            // Only these two offsets are defined bools. Any other displacement
            // off NULL_REG falls through to the arithmetic arm, where
            // `null + N` is a true statement about a canonical object near
            // null; dropping it would lose information rather than remove a
            // false claim.
            "add"
                if ops.len() == 3
                    && canonical_reg(&ops[1]).as_deref() == Some("x22")
                    && matches!(
                        ops[2].trim().trim_start_matches('#'),
                        "0x20" | "32" | "0x30" | "48"
                    ) =>
            {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let value = match ops[2].trim().trim_start_matches('#') {
                        "0x20" | "32" => "true",
                        _ => "false",
                    };
                    self.state.reg_values.insert(dst, value.to_string());
                }
            }
            // `BooleanNegateInstr` (`il_arm64.cc`) flips `kBoolValueMask`, so
            // this is `!x` whenever the operand is a bool. Only folded when the
            // operand is provably one: `TestIntInstr` on a tagged value can
            // also land on bit 4, so an arbitrary register is not enough.
            "eor"
                if ops.len() == 3
                    && matches!(ops[2].trim().trim_start_matches('#'), "0x10" | "16") =>
            {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let src = self.operand_expr(&ops[1]);
                    match src.as_str() {
                        "true" => self.state.reg_values.insert(dst, "false".to_string()),
                        "false" => self.state.reg_values.insert(dst, "true".to_string()),
                        _ => self.state.reg_values.remove(&dst),
                    };
                }
            }
            // `csel`/`cset`/`csetm`. Unmodelled before, so the destination kept
            // a stale value and a function returning `cond ? true : false`
            // emitted `return receiver;`. 2,708 csel sites, 85% of them with
            // both bools materialised into the arms, which is Dart turning a
            // comparison into a value.
            "csel" | "cset" | "csetm" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let cond = ops[ops.len() - 1].trim().to_ascii_lowercase();
                    // Without a comparison in hand the condition cannot be
                    // named, and naming it wrongly is worse than the gap.
                    let named = self
                        .state
                        .last_cmp
                        .clone()
                        .and_then(|cmp| {
                            cond_from_cmp(&format!("b.{cond}"), &cmp).map(|taken| (cmp, taken))
                        })
                        .and_then(|(cmp, taken)| match mnemonic.as_str() {
                            // Every conditional value is bracketed. A bare
                            // ternary composes wrongly: `?:` binds looser than
                            // arithmetic, so `(c) ? 1 : 0 - 1` inside a mask read
                            // as `c ? 1 : ((0 - 1) & mask)`. 195 of 282 ternaries
                            // on one sample were composed into a larger
                            // expression, so most of them rendered a value the
                            // machine never computed.
                            "cset" => Some(format!("(({taken}) ? 1 : 0)")),
                            "csetm" => Some(format!("(({taken}) ? -1 : 0)")),
                            _ if ops.len() < 4 => None,
                            _ => {
                                let lhs = self.operand_expr(&ops[1]);
                                let rhs = self.operand_expr(&ops[2]);
                                // Operand order carries the polarity, so read
                                // which arm holds which bool rather than
                                // assuming a fixed one.
                                Some(match (lhs.as_str(), rhs.as_str()) {
                                    ("true", "false") => format!("({taken})"),
                                    ("false", "true") => match invert_cond(&cond)
                                        .and_then(|inv| cond_from_cmp(&format!("b.{inv}"), &cmp))
                                    {
                                        Some(not_taken) => format!("({not_taken})"),
                                        None => format!("(({taken}) ? false : true)"),
                                    },
                                    _ => format!("(({taken}) ? {lhs} : {rhs})"),
                                })
                            }
                        });
                    match named {
                        Some(value) => self.state.reg_values.insert(dst, value),
                        None => self.state.reg_values.remove(&dst),
                    };
                }
            }
            "add" | "sub" | "mul" | "and" | "orr" | "eor" if ops.len() >= 3 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let lhs = self.operand_expr(&ops[1]);
                    // The modifier is applied, not appended as a comment. Left as
                    // a comment the expression asserted `a | b` where the truth is
                    // `a | (b >>> 32)`: 5,938 and 8,055 sites. A literal shifted
                    // by a literal folds, so a shifted-immediate pool address
                    // takes the same shape as an unshifted one.
                    let rhs = self.shifted_operand_expr(&ops, 2);
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
                    // Dart's `>>` is arithmetic, so `lsr` needs `>>>`: on a
                    // negative value the two differ, and rendering a logical
                    // shift as an arithmetic one claims a result the machine
                    // never produced.
                    let op = match mnemonic.as_str() {
                        "lsl" => "<<",
                        "lsr" => ">>>",
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
                    // A full-width extract from bit zero is a zero extension,
                    // which reads better as the mask it is. 6,289 sites.
                    let value = if parse_int(&lsb) == Some(0) && parse_int(&width) == Some(32) {
                        format!("({src} & 0xffffffff)")
                    } else {
                        format!("bitField({src}, {lsb}, {width})")
                    };
                    self.state.reg_values.insert(dst, value);
                }
            }
            // Signed field extract and its insert form.
            //
            // `SmiUntag` is `sbfm(dst, src, kSmiTagSize, kSmiBits + kSmiTagSize)`
            // (`compiler/assembler/assembler_arm64.h`), so it is the only
            // producer of a signed extract at bit 1 whose width is `kSmiBits+1`:
            // 31 under compressed pointers, 63 without. Both are accepted rather
            // than the 31 these two binaries happen to use, so the rule does not
            // encode a build configuration. 25,899 sites across both samples.
            //
            // The insert form at the same position is the tag: `BoxInteger32`
            // and `BoxInt64` (`compiler/backend/il_arm64.cc`) emit it and then
            // compare the input against the result shifted back, which is why
            // every one of the 8,262 sites is followed by exactly that compare.
            // Anything else keeps the generic name, because `sbfiz rd, rs, #l,
            // #w` sign-extends from bit `l + w`, not from `w`, and an arithmetic
            // rendering that gets that wrong reads as resolved.
            "sbfx" | "sbfiz" if ops.len() >= 4 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let src = self.operand_expr(&ops[1]);
                    let lsb = self.operand_expr(&ops[2]);
                    let width = self.operand_expr(&ops[3]);
                    let smi_position =
                        parse_int(&lsb) == Some(1) && matches!(parse_int(&width), Some(31) | Some(63));
                    let value = match (mnemonic.as_str(), smi_position) {
                        ("sbfx", true) => format!("smiUntag({src})"),
                        ("sbfx", false) => format!("signedBitField({src}, {lsb}, {width})"),
                        (_, true) => format!("smiTag({src})"),
                        _ => format!("signedBitFieldInsert({src}, {lsb}, {width})"),
                    };
                    self.state.reg_values.insert(dst, value);
                }
            }
            // Byte and half-word loads read the same field as `ldr`; the width
            // is part of the field's type, not of the address. The `s` forms
            // sign-extend, which changes the value, so they say so.
            "ldur" | "ldr" | "ldrb" | "ldurb" | "ldrh" | "ldurh" | "ldrsb" | "ldursb"
            | "ldrsh" | "ldursh" | "ldrsw" | "ldursw"
                if ops.len() >= 2 =>
            {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let rhs = self.operand_expr(&ops[1]);
                    let value = match mnemonic.as_str() {
                        "ldrsb" | "ldursb" => format!("signExtend({rhs}, 8)"),
                        "ldrsh" | "ldursh" => format!("signExtend({rhs}, 16)"),
                        "ldrsw" | "ldursw" => format!("signExtend({rhs}, 32)"),
                        _ => rhs,
                    };
                    self.state.reg_values.insert(dst, value);
                }
                self.invalidate_index_writeback(&ops);
            }
            // Load-pair reads two consecutive registers' worth, so the second
            // destination is one register width further along: 8 bytes for an
            // `x` pair, 4 for a `w` pair. The largest single unmodelled
            // mnemonic, 79,645 sites across both samples.
            "ldp" | "ldnp" if ops.len() >= 3 => {
                let stride = if ops[0].trim().starts_with('w') { 4 } else { 8 };
                let first = self.operand_expr(&ops[2]);
                if let Some(dst) = canonical_reg(&ops[0]) {
                    self.state.reg_values.insert(dst, first);
                }
                let second = parse_mem_operand(&ops[2]).map(|(base, off)| {
                    if base == "x29" {
                        self.locals
                            .get(&(off + stride))
                            .cloned()
                            .unwrap_or_else(|| local_name(off + stride))
                    } else {
                        let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
                        Self::clean_expr(Self::field_expr(&base_expr, off + stride))
                    }
                });
                if let Some(dst) = canonical_reg(&ops[1]) {
                    match second {
                        Some(value) => self.state.reg_values.insert(dst, value),
                        None => self.state.reg_values.remove(&dst),
                    };
                }
                self.invalidate_index_writeback(&ops);
            }
            "stur" | "str" | "strb" | "sturb" | "strh" | "sturh" if ops.len() >= 2 => {
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
                self.invalidate_index_writeback(&ops);
            }
            // Every instruction that sets NZCV has to land here. `last_cmp` was
            // written only by `cmp` and cleared only at joins, so a `b.<cc>` or
            // `csel` after any other flag writer rendered the *previous*
            // comparison as its condition. 33,705 conditions across the two
            // samples take their flags from something other than `cmp`, `tst`
            // alone accounting for 22,141, and conditions are what the
            // structurer branches on.
            // `fcmp` shares this arm, with one imprecision worth naming: a NaN
            // operand leaves the comparison unordered, so `b.gt` and `b.le`
            // after it are not exact negations of each other. Everything else
            // in this arm is exact.
            "cmp" | "fcmp" if ops.len() >= 2 => {
                let lhs = self.operand_expr(&ops[0]);
                let rhs = self.shifted_operand_expr(&ops, 1);
                self.state.last_cmp = Some((lhs, rhs));
            }
            // `subs` sets the same flags as `cmp` on the same operands, and
            // also keeps the difference.
            "subs" if ops.len() >= 3 => {
                let lhs = self.operand_expr(&ops[1]);
                let rhs = self.shifted_operand_expr(&ops, 2);
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let value = simplify_bin_expr(lhs.clone(), "-", rhs.clone());
                    self.state.reg_values.insert(dst, value);
                }
                self.state.last_cmp = Some((lhs, rhs));
            }
            // Mask tests. `tst` discards the result, `ands` keeps it, and both
            // compare it against zero: `tst x0, #1` is the Smi tag check. The
            // three-operand forms write a destination as well, so they must not
            // be flags-only: that would leave the stale value the fallback arm
            // exists to drop.
            "tst" | "cmn" if ops.len() >= 2 => {
                let a = self.operand_expr(&ops[0]);
                // `StoreBarrier` (`compiler/assembler/assembler_arm64.cc`) ends
                // its check with `tst(scratch, HEAP_BITS LSR #32)`, whose high
                // half is the write-barrier mask. HEAP_BITS is reserved, so a
                // test against that half is the barrier check by construction
                // and nothing else produces it. Naming the predicate leaves the
                // operand visible while saying what the condition decides:
                // 8,846 and 12,355 sites, previously reading as tag arithmetic
                // against an unnamed register.
                let barrier = mnemonic == "tst"
                    && canonical_reg(&ops[1]).as_deref() == Some("x28")
                    && ops
                        .get(2)
                        .is_some_and(|m| m.trim().eq_ignore_ascii_case("lsr #32"));
                if barrier {
                    self.state.last_cmp =
                        Some((format!("needsWriteBarrier({a})"), "0".to_string()));
                } else {
                    let b = self.shifted_operand_expr(&ops, 1);
                    let op = if mnemonic == "tst" { "&" } else { "+" };
                    self.state.last_cmp = Some((format!("({a} {op} {b})"), "0".to_string()));
                }
            }
            "ands" | "adds" if ops.len() >= 3 => {
                let a = self.operand_expr(&ops[1]);
                let b = self.shifted_operand_expr(&ops, 2);
                // Same rendering as the non-flag form of each: the computation
                // does not change because flags were set.
                let combined = if mnemonic == "ands" {
                    format!("({a} & {b})")
                } else {
                    simplify_bin_expr(a, "+", b)
                };
                if let Some(dst) = canonical_reg(&ops[0]) {
                    self.state.reg_values.insert(dst, combined.clone());
                }
                self.state.last_cmp = Some((combined, "0".to_string()));
            }
            "ret" => {}
            _ => {
                // An unmodelled instruction still writes its destination, and
                // whatever the last modelled instruction left in `reg_values`
                // would be rendered as that register's value at every later
                // read. `csel x0, x16, x17, ne` used to leave x0 holding the
                // entry value, so a function returning `cond ? true : false`
                // emitted `return receiver;`. Dropping the binding degrades the
                // read to a named register, which is the honest rendering for a
                // value this lifter cannot follow.
                for reg in written_registers(&mnemonic, &ops) {
                    self.state.reg_values.remove(&reg);
                }
                // Same for the flags: an unmodelled flag writer leaves
                // `last_cmp` describing an older comparison, which every
                // following condition would then claim as its own.
                if writes_flags(&mnemonic) {
                    self.state.last_cmp = None;
                }
            }
        }

        // A reserved register keeps its meaning even where AOT re-derives it.
        // HEAP_BITS is reloaded from THR inside 157 functions on one sample,
        // which would otherwise rebind `heapBits` to the reload expression and
        // lose the one thing known about the register.
        for reg in written_registers(&mnemonic, &ops) {
            if let Some(value) = pinned_value(&reg) {
                self.state.reg_values.insert(reg, value.to_string());
            }
        }
    }
}
