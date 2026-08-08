/// Frame slots the lifter can name, so every one of them gets a declaration.
///
/// This must cover exactly the mnemonics whose arms resolve an `x29`-relative
/// operand, or a slot gets referenced with no `var` line: `lib.rs` declares
/// only what this returns. `ldp` needs both of its slots, and its memory
/// operand is the third, not the second.
pub(super) fn collect_stack_offsets(ir: &FunctionIr) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();

    for block in &ir.blocks {
        for ins in &block.instrs {
            let (mnemonic, ops) = split_instruction(&ins.src);
            let (mem_index, slots) = match mnemonic.as_str() {
                "ldur" | "ldr" | "ldrb" | "ldurb" | "ldrh" | "ldurh" | "ldrsb" | "ldursb"
                | "ldrsh" | "ldursh" | "ldrsw" | "ldursw" | "stur" | "str" | "strb" | "sturb"
                | "strh" | "sturh" => (1, 1),
                "ldp" | "ldnp" | "stp" => (2, 2),
                _ => continue,
            };
            if ops.len() <= mem_index {
                continue;
            }
            let Some((base, off)) = parse_mem_operand(&ops[mem_index]) else {
                continue;
            };
            if base != "x29" {
                continue;
            }
            let stride = if ops[0].trim().starts_with('w') { 4 } else { 8 };
            for slot in 0..slots {
                out.insert(off + stride * slot as i64);
            }
        }
    }

    out
}

pub(super) fn init_state() -> LiftState {
    let mut s = LiftState::default();
    // Incoming arguments, in `DartCallingConvention` order. x0 is the return
    // register and x4 is `ARGS_DESC_REG`, so neither holds an argument:
    // seeding them as `arg0`/`arg4` made the emitter render the return slot as
    // `receiver`. Confirmed on 14,129 functions, where entry blocks read x1
    // before writing it in 55% of cases against 3.4% for x0.
    for (i, reg) in DART_ARGUMENT_REGISTERS.iter().enumerate() {
        s.reg_values.insert((*reg).to_string(), format!("arg{i}"));
    }
    // Registers Dart AOT reserves, so their value is the same everywhere.
    s.reg_values.insert("x15".to_string(), "sp".to_string());
    s.reg_values.insert("x21".to_string(), "dispatchTable".to_string());
    s.reg_values.insert("x22".to_string(), "null".to_string());
    s.reg_values.insert("x26".to_string(), "thread".to_string());
    s.reg_values.insert("x27".to_string(), "pool".to_string());
    s
}

pub(super) fn cond_from_cmp(branch: &str, cmp: &(String, String)) -> Option<String> {
    let op = match branch {
        "b.eq" => "==",
        "b.ne" => "!=",
        "b.lt" => "<",
        "b.le" => "<=",
        "b.gt" => ">",
        "b.ge" => ">=",
        "b.hi" => ">",
        "b.ls" => "<=",
        "b.lo" => "<",
        "b.hs" => ">=",
        _ => return None,
    };
    Some(format!("{} {} {}", cmp.0, op, cmp.1))
}

/// The condition that holds exactly when `cond` does not, for reading a
/// `csel` whose arms are `false` then `true`. Only the codes Dart emits are
/// mapped; anything else has no inverse here and the caller keeps the
/// conditional form rather than guessing.
pub(super) fn invert_cond(cond: &str) -> Option<&'static str> {
    Some(match cond {
        "eq" => "ne",
        "ne" => "eq",
        "lt" => "ge",
        "ge" => "lt",
        "gt" => "le",
        "le" => "gt",
        "hi" => "ls",
        "ls" => "hi",
        "lo" => "hs",
        "hs" => "lo",
        _ => return None,
    })
}

/// Whether an instruction sets NZCV. Any of these leaves `last_cmp` stale, so
/// one that is not modelled must clear it: otherwise the next `b.<cc>` or
/// `csel` renders an older comparison as its own condition.
pub(super) fn writes_flags(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "cmp"
            | "cmn"
            | "tst"
            | "ccmp"
            | "ccmn"
            | "fcmp"
            | "fcmpe"
            | "adds"
            | "subs"
            | "ands"
            | "bics"
            | "adcs"
            | "sbcs"
            | "negs"
            | "ngcs"
    )
}
