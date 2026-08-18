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

/// Registers Dart AOT reserves whose value is the same at every point in a
/// function, and the value each holds.
///
/// `kReservedCpuRegisters` (`runtime/vm/constants_arm64.h`) excludes these from
/// `kDartAvailableCpuRegs`, so no Dart value is ever allocated to one. TMP,
/// TMP2, LR and CODE_REG are reserved too but hold no fixed value, so they are
/// deliberately absent: naming them would be a claim where `regN` is the truth.
///
/// SPREG is reserved but *not* invariant, so it is absent as well. 47,412
/// instructions across 5,149 functions on one sample write it, `sub x15, x15,
/// #N` to open a frame and `mov x15, x29` to close one. Pinning it rendered
/// `[x15, #8]` after a frame allocation as `sp[8]` when the address is
/// `sp - frame + 8`, which is a wrong address rather than a missing one.
///
/// AOT does re-derive HEAP_BITS inside a body, 639 instructions across 157
/// functions on one sample, restoring the same constant from THR. A write must
/// therefore not rebind these, which is why `apply_other_lift` re-asserts them.
pub(super) const PINNED_REGISTERS: &[(&str, &str)] = &[
    ("x21", "dispatchTable"),
    ("x22", "null"),
    ("x26", "thread"),
    ("x27", "pool"),
    ("x28", "heapBits"),
];

/// The fixed value a reserved register holds, if it is one.
pub(super) fn pinned_value(reg: &str) -> Option<&'static str> {
    PINNED_REGISTERS
        .iter()
        .find(|(name, _)| *name == reg)
        .map(|(_, value)| *value)
}

/// Registers a Dart call does not preserve.
///
/// `kDartVolatileCpuRegs` (`runtime/vm/constants_arm64.h`) is R0 through R14;
/// the assembler uses TMP and TMP2 at will, R18 is volatile off Fuchsia, and
/// the call itself writes LR. R19 through R28 are `kAbiPreservedCpuRegs` and
/// SPREG is preserved, so a binding for one of those survives a call.
pub(super) const CALL_CLOBBERED_REGISTERS: &[&str] = &[
    "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14",
    "x16", "x17", "x18", "x30",
];

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
    for (reg, value) in PINNED_REGISTERS {
        s.reg_values.insert((*reg).to_string(), (*value).to_string());
    }
    // SPREG on entry. Seeded but not pinned: the prologue's `sub x15, x15, #N`
    // must rebind it so slot addresses account for the frame, while the entry
    // value is what makes an unadjusted `[x15, #N]` read as `sp[N]`.
    s.reg_values.insert("x15".to_string(), "sp".to_string());
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
///
/// The operands are part of the summary because one family writes the flags
/// without naming a general register: `msr nzcv, x3` moves a value straight into
/// the flag register, and read from the mnemonic alone it looks effect-free, so
/// the `cset` after it kept claiming the comparison before it.
pub(super) fn writes_flags(mnemonic: &str, ops: &[String]) -> bool {
    if mnemonic == "msr" {
        return ops
            .first()
            .is_some_and(|target| target.trim().eq_ignore_ascii_case("nzcv"));
    }
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
            // Conditional floating-point compares and the flag-manipulation
            // forms. None of them is lifted, so each one used to leave the
            // previous comparison standing as the next condition's meaning.
            | "fccmp"
            | "fccmpe"
            | "rmif"
            | "setf8"
            | "setf16"
    )
}
