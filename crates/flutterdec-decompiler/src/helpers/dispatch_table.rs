/// Recovered Dart AOT dispatch-table call.
///
/// `flutterdec` cannot name the target method without snapshot metadata, but the
/// selector it dispatches on is encoded in the instruction stream, so calls to
/// the same method are identifiable across the whole binary.
pub(super) struct DispatchCall {
    /// `DispatchTable` selector offset, canonical: the same integer the Dart
    /// compiler assigned the selector, so a recovered selector table can be
    /// joined onto it by equality.
    pub(super) selector_offset: i64,
    /// Register holding the receiver whose class id indexed the table, when the
    /// header load is in the same block.
    pub(super) receiver: Option<String>,
    /// Argument registers with a definition reaching this call, in convention
    /// order. A lower bound on the real argument list, not a signature: a
    /// pass-through parameter defined in an earlier block is not counted, and
    /// stack-passed arguments are not modelled at all.
    pub(super) argument_registers: Vec<String>,
}

/// `DispatchTable::kOriginElement` for ARM64 (`runtime/vm/dispatch_table.h`):
/// the element the dispatch table register points at, chosen as the largest
/// consecutive `sub` immediate so small selector offsets encode in one
/// instruction.
const DISPATCH_TABLE_ORIGIN_ELEMENT: i64 = 4096;

/// `DartCallingConvention::kCpuRegistersForArgs` for ARM64
/// (`runtime/vm/constants_arm64.h`). x0 is the return register and x4 is
/// `ARGS_DESC_REG`, so neither appears; further arguments go on the stack.
pub(super) const DART_ARGUMENT_REGISTERS: [&str; 6] = ["x1", "x2", "x3", "x5", "x6", "x7"];

/// What an instruction contributes to dispatch-call recognition. Computed from
/// the pre-instruction state, then applied after the destination registers are
/// invalidated, so a register can be its own source.
enum DispatchBinding {
    /// Materialised integer, possibly assembled from `movz`/`movk` halves.
    Constant(String, i64),
    /// Register holds a class id. The receiver it came from is `None` once that
    /// register is redefined, which loses the receiver name without losing the
    /// fact that this register indexes the dispatch table.
    ClassId(String, Option<String>),
    /// Dispatch table index: selector offset relative to the origin element,
    /// with the receiver it will be applied to.
    TableIndex(String, i64, Option<String>),
    /// Loaded target entry: register now holds the callee reached through that
    /// selector offset. Resolved eagerly, because the load usually overwrites
    /// the index register it reads.
    TableEntry(String, i64, Option<String>),
}

/// Recognise dispatch-table calls, keyed by the `blr` address.
///
/// `FlowGraphCompiler::EmitDispatchTableCall` emits, for ARM64:
///
/// ```text
///   ldur wC, [recv, #-1]         ; object header, receiver is tag-adjusted
///   ubfx xC, xC, #0xc, #0x14     ; class id = header bits 12..31
///   add  x30, xC, #K             ; K = selector_offset - kOriginElement
///   ldr  x30, [x21, x30, lsl #3] ; x21 = DISPATCH_TABLE_REG
///   blr  x30
/// ```
///
/// `AddImmediate` materialises a negative or wide `K` as `sub`, or as
/// `movz`/`movk` into a scratch register followed by `add`, so all three
/// encodings are folded here. Anchoring on the scaled `[x21, ...]` load keeps
/// unrelated constant materialisation out.
pub(super) fn dispatch_table_calls(ir: &FunctionIr) -> HashMap<u64, DispatchCall> {
    let mut out = HashMap::new();

    for block in &ir.blocks {
        // Reset per block: the idiom is contiguous, and carrying state across a
        // control-flow join would invent selectors.
        let mut constants: HashMap<String, i64> = HashMap::new();
        let mut class_id_of: HashMap<String, Option<String>> = HashMap::new();
        let mut indices: HashMap<String, (i64, Option<String>)> = HashMap::new();
        let mut entries: HashMap<String, (i64, Option<String>)> = HashMap::new();
        // Argument registers written since the last call boundary. Dart moves
        // arguments into place immediately before the call, so this is the set
        // the call actually passes, minus any pass-through parameter it never
        // had to touch.
        let mut defined_args: HashSet<String> = HashSet::new();

        for ins in &block.instrs {
            let (mnemonic, ops) = split_instruction(&ins.src);

            if mnemonic == "blr" || mnemonic == "bl" {
                if mnemonic == "blr" {
                    if let Some(dispatch) = canonical_reg(ops.first().map_or("", String::as_str))
                        .and_then(|target| entries.get(&target))
                    {
                        let selector_offset = dispatch.0 + DISPATCH_TABLE_ORIGIN_ELEMENT;
                        // A real selector offset indexes the dispatch table, so it
                        // cannot be negative. A wide shifted `sub` can drive the
                        // sum below zero, which means the arithmetic was not a
                        // selector calculation: a failed recovery, not a selector
                        // named `sel-16773119`.
                        if selector_offset < 0 {
                            continue;
                        }
                        let receiver = dispatch.1.clone();
                        out.insert(
                            ins.va,
                            DispatchCall {
                                selector_offset,
                                argument_registers: DART_ARGUMENT_REGISTERS
                                    .iter()
                                    .filter(|reg| defined_args.contains(**reg))
                                    .filter(|reg| Some(reg.to_string()) != receiver)
                                    .map(|reg| reg.to_string())
                                    .collect(),
                                receiver,
                            },
                        );
                    }
                }
                // A call clobbers the argument and scratch registers; nothing
                // tracked here survives it.
                constants.clear();
                class_id_of.clear();
                indices.clear();
                entries.clear();
                defined_args.clear();
                continue;
            }

            let binding = dispatch_binding(&mnemonic, &ops, &constants, &class_id_of, &indices);

            let mut overwritten: Vec<String> = Vec::new();
            for reg in written_registers(&mnemonic, &ops) {
                constants.remove(&reg);
                class_id_of.remove(&reg);
                indices.remove(&reg);
                entries.remove(&reg);
                // The receiver is remembered by register name, so redefining
                // that register invalidates it as a receiver too. Without this
                // the call renders whatever value landed there instead, which
                // is a confidently wrong receiver rather than an honest unknown.
                for receiver in class_id_of.values_mut() {
                    if receiver.as_deref() == Some(reg.as_str()) {
                        *receiver = None;
                    }
                }
                for (_, receiver) in indices.values_mut().chain(entries.values_mut()) {
                    if receiver.as_deref() == Some(reg.as_str()) {
                        *receiver = None;
                    }
                }
                if DART_ARGUMENT_REGISTERS.contains(&reg.as_str()) {
                    defined_args.insert(reg.clone());
                }
                overwritten.push(reg);
            }

            // The binding was computed from the pre-instruction state, so its
            // receiver can name a register this same instruction just
            // overwrote. Keeping it would render a receiver that no longer
            // holds the receiver value.
            let live = |receiver: Option<String>| {
                receiver.filter(|name| !overwritten.iter().any(|reg| reg == name))
            };
            match binding {
                Some(DispatchBinding::Constant(reg, value)) => {
                    constants.insert(reg, value);
                }
                Some(DispatchBinding::ClassId(reg, receiver)) => {
                    class_id_of.insert(reg, live(receiver));
                }
                Some(DispatchBinding::TableIndex(reg, offset, receiver)) => {
                    indices.insert(reg, (offset, live(receiver)));
                }
                Some(DispatchBinding::TableEntry(reg, offset, receiver)) => {
                    entries.insert(reg, (offset, live(receiver)));
                }
                None => {}
            }
        }
    }

    out
}

fn dispatch_binding(
    mnemonic: &str,
    ops: &[String],
    constants: &HashMap<String, i64>,
    class_id_of: &HashMap<String, Option<String>>,
    indices: &HashMap<String, (i64, Option<String>)>,
) -> Option<DispatchBinding> {
    let dst = canonical_reg(ops.first()?)?;

    match mnemonic {
        // `AddImmediate` with a zero offset degenerates to a register move, in
        // which case the class id indexes the table directly and the selector
        // offset is the origin element itself.
        "mov" | "movz" if ops.len() == 2 => match parse_int(&ops[1]) {
            Some(value) => Some(DispatchBinding::Constant(dst, value)),
            None => Some(DispatchBinding::TableIndex(
                dst,
                0,
                class_id_of.get(&canonical_reg(&ops[1])?)?.clone(),
            )),
        },
        "movk" if ops.len() >= 2 => {
            let half = parse_int(&ops[1])?;
            let shift = ops
                .get(2)
                .and_then(|s| s.split('#').nth(1))
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            // `movk` shifts are 0, 16, 32 or 48. The value is parsed from
            // disassembly text, so anything wider is a misparse and would panic
            // the shift in a debug build.
            let mask = 0xffffi64.checked_shl(shift)?;
            let kept = constants.get(&dst).copied().unwrap_or(0) & !mask;
            Some(DispatchBinding::Constant(dst, kept | half.checked_shl(shift)?))
        }
        // The dispatch table load is a scaled register-offset form, which no
        // other Dart idiom uses off x21.
        "ldr" if ops.len() == 2 => {
            let index = dispatch_table_index(&ops[1])?;
            let (offset, receiver) = indices.get(&index)?;
            Some(DispatchBinding::TableEntry(dst, *offset, receiver.clone()))
        }
        // The object header sits one byte below the tagged receiver
        // (`kHeapObjectTag`), which is what proves the loaded word is a header.
        "ldur" if ops.len() == 2 => match parse_mem_operand(&ops[1])? {
            (base, -1) => Some(DispatchBinding::ClassId(dst, Some(base))),
            _ => None,
        },
        // Class id is a bitfield of the header word. The position is derived
        // from the size tag's width (`ClassIdTag` in `runtime/vm/raw_object.h`)
        // and has moved between SDK versions, so it is deliberately not
        // asserted: provenance from the header load plus use as the dispatch
        // table index is what identifies the value. Hardcoding the position
        // would turn a layout change into silent zero recovery.
        "ubfx" | "ubfiz" | "lsr" if ops.len() >= 3 => {
            let src = canonical_reg(&ops[1])?;
            Some(DispatchBinding::ClassId(dst, class_id_of.get(&src)?.clone()))
        }
        // Offsets that are a multiple of 4096 encode as a shifted immediate,
        // e.g. `sub x30, x0, #1, lsl #12`.
        "add" | "sub" if ops.len() == 3 || ops.len() == 4 => {
            let src = canonical_reg(&ops[1])?;
            let magnitude = parse_int(&ops[2])
                .or_else(|| constants.get(&canonical_reg(&ops[2])?).copied())?;
            let shift = match ops.get(3) {
                Some(op) => shift_amount(op)?,
                None => 0,
            };
            let magnitude = magnitude.checked_shl(shift)?;
            let offset = if mnemonic == "sub" {
                -magnitude
            } else {
                magnitude
            };
            Some(DispatchBinding::TableIndex(
                dst,
                offset,
                class_id_of.get(&src).cloned().flatten(),
            ))
        }
        _ => None,
    }
}

/// Left-shift amount of a shifted-immediate operand, e.g. `lsl #12`.
fn shift_amount(operand: &str) -> Option<u32> {
    let (kind, amount) = operand.trim().split_once('#')?;
    if !kind.trim().eq_ignore_ascii_case("lsl") {
        return None;
    }
    amount.trim().parse().ok()
}

/// Instructions whose destination is a register pair, so the effect summary
/// names two registers rather than one.
///
/// `ldp` was the only pair form recognised, which left the second destination of
/// every other one holding whatever the last modelled instruction put there:
/// after `mov x1, #5`, `ldpsw x0, x1, [x2]` kept `5` bound to x1 and a later read
/// rendered that literal as a resolved fact. `ldnp` is lifted and binds both
/// halves itself, but this summary is also what the join merge
/// (`registers_written_before`) and the call-clobber retirement read, so a
/// destination missing here survives a join as well as a straight line. The
/// atomic pair forms update both halves of the compare pair for the same reason.
const PAIR_DESTINATION_MNEMONICS: [&str; 9] = [
    "ldp", "ldnp", "ldpsw", "ldxp", "ldaxp", "casp", "caspa", "caspal", "caspl",
];

/// Registers an instruction overwrites.
///
/// Anything not recognised as a pure read is treated as writing its first
/// operands. That is deliberately over-approximate: naming a register that was
/// not written drops a binding needlessly, which costs a `regN`, while missing one
/// lets a stale value read as a resolved fact.
///
/// The width the destination is spelled at does not narrow this: `w3` and `x3`
/// are one machine register, `canonical_reg` folds them onto one key, and a
/// 32-bit write leaves the high half cleared rather than preserved, so the whole
/// binding goes.
pub(super) fn written_registers(mnemonic: &str, ops: &[String]) -> Vec<String> {
    // A pre- or post-indexed access writes the base register back, so the base is
    // a destination even for a store, which otherwise writes nothing. 2,346 and
    // 1,394 such instructions on the two samples have a base that is not one of
    // the pinned registers, so without this their binding survived a join that had
    // redefined it.
    let mut written: Vec<String> = writeback_base(ops).into_iter().collect();
    let reads_only = matches!(
        mnemonic,
        "cmp" | "cmn" | "tst" | "ccmp" | "fcmp" | "ret" | "b" | "br" | "str" | "stur" | "strb"
            | "sturb" | "strh" | "sturh" | "stp"
    ) || mnemonic.starts_with("b.")
        || matches!(mnemonic, "cbz" | "cbnz" | "tbz" | "tbnz");
    if reads_only {
        return written;
    }
    // A pair form writes both destinations; everything else writes the first.
    let count = if PAIR_DESTINATION_MNEMONICS.contains(&mnemonic) {
        2
    } else {
        1
    };
    written.extend(ops.iter().take(count).filter_map(|o| canonical_reg(o)));
    written
}

/// The base register a pre- or post-indexed memory operand writes back.
///
/// Pre-indexed keeps the offset inside the brackets and marks the writeback with
/// `!`, as in `str x1, [x0, #8]!`. Post-indexed closes the brackets first and puts
/// the offset in the next operand, as in `ldr x1, [x0], #8`, so it is recognised
/// by a bracketed operand followed by an immediate one.
fn writeback_base(ops: &[String]) -> Option<String> {
    for (i, op) in ops.iter().enumerate() {
        let token = op.trim();
        let inner = match token.strip_suffix("]!") {
            Some(pre) => pre.strip_prefix('[')?,
            None => {
                let closed = token.strip_prefix('[').and_then(|t| t.strip_suffix(']'));
                let followed_by_offset = ops
                    .get(i + 1)
                    .is_some_and(|next| next.trim_start().starts_with('#'));
                match (closed, followed_by_offset) {
                    (Some(base), true) => base,
                    _ => continue,
                }
            }
        };
        return canonical_reg(inner.split(',').next()?.trim());
    }
    None
}

/// Index register of a scaled load off `DISPATCH_TABLE_REG`, e.g.
/// `[x21, x30, lsl #3]`.
fn dispatch_table_index(operand: &str) -> Option<String> {
    let inside = operand.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inside.split(',').map(str::trim);
    if canonical_reg(parts.next()?)? != "x21" {
        return None;
    }
    let index = canonical_reg(parts.next()?)?;
    // Word-scaled: entries are machine words.
    if !parts.next()?.replace(' ', "").eq_ignore_ascii_case("lsl#3") {
        return None;
    }
    Some(index)
}

/// Stable synthetic name for a selector, identical at every call site of the
/// same method across the binary.
pub(super) fn dispatch_selector_name(selector_offset: i64) -> String {
    format!("sel{selector_offset}")
}
