use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};
use flutterdec_ir::build_function_ir;
use std::collections::HashSet;

/// How much a split changed, for the report.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct SplitStats {
    /// Records that contained at least one further function.
    pub(super) records_split: usize,
    /// Functions recovered, i.e. pieces after the first.
    pub(super) functions_recovered: usize,
    /// Candidates the containment clause refused.
    pub(super) rejected_not_contained: usize,
    /// Candidates an intra-record branch reached.
    pub(super) rejected_branch_target: usize,
    /// Candidates abandoned because the preceding piece had no block. Should stay
    /// zero: it is reported so that a regression is visible rather than silent.
    pub(super) rejected_no_block: usize,
    /// Records abandoned because the graph built from them failed the shared
    /// identity ruler. Should stay zero for the same reason as `rejected_no_block`:
    /// the builder is the only producer here, so a nonzero count is a builder
    /// regression and must be visible rather than silently changing what is split.
    pub(super) rejected_invalid_ir: usize,
}

/// Splits a function record that spans more than one real function.
///
/// The adapter sizes a record as the gap to the next start it recovered, so every
/// function it missed is swallowed by its predecessor. The cost is not cosmetic:
/// 72% and 73% of all decoded blocks are unreachable from their own record's entry
/// on the two sample binaries, and both emitters walk from the entry, so that code
/// is decoded and then never emitted at all.
///
/// This runs on `FunctionDisassembly` rather than on the IR, and deliberately so.
/// `build_program_ir` maps one record to one `FunctionIr` and derives blocks from
/// the instruction list, so splitting the list yields dense block ids and an entry
/// at block 0 for free -- both of which `Regions::build` requires, and neither of
/// which post-hoc surgery on built IR would preserve.
pub(super) fn split_inflated_records(
    disasm: Vec<FunctionDisassembly>,
) -> (Vec<FunctionDisassembly>, SplitStats) {
    let mut stats = SplitStats::default();
    let mut next_id = disasm.iter().map(|f| f.function_id).max().unwrap_or(0) + 1;
    let mut out = Vec::with_capacity(disasm.len());
    for record in disasm {
        let splits = split_points(&record, &mut stats);
        if splits.is_empty() {
            out.push(record);
            continue;
        }
        stats.records_split += 1;
        stats.functions_recovered += splits.len();
        for piece in pieces(record, &splits, &mut next_id) {
            out.push(piece);
        }
    }
    (out, stats)
}

/// Instruction indices at which a new function begins.
///
/// Four clauses, every one of them evaluable from the instruction list plus
/// reachability within the record:
///
/// 1. the previous instruction is a terminator, because a function entry is not
///    fallen into;
/// 2. this instruction pushes a frame with writeback, which is how a Dart AOT
///    prologue opens. A catch-block entry, which is also unreachable and also
///    follows a terminator, restores a frame from `x29` instead, and would be torn
///    out of its own function if it were split on;
/// 3. no branch or jump inside the record targets this address, since a branch into
///    a frame push is intra-function control flow rather than an entry;
/// 4. the address lies above everything reachable from the *preceding* piece. A
///    candidate that fails this would amputate a function that currently emits
///    correctly, which is a regression on the part of the output that already
///    works. Applied per piece rather than once from the record's entry, because
///    reachability from the entry says nothing about a function swallowed earlier
///    in the same record: 62 and 77 candidates are rejected only this way.
///
/// Measured on the two samples: 17,988 and 22,553 candidates pass clauses 1 and 2,
/// 16,258 and 20,233 survive clause 3, and 16,101 and 20,078 survive clause 4.
fn split_points(record: &FunctionDisassembly, stats: &mut SplitStats) -> Vec<usize> {
    let instrs = &record.instructions;
    if instrs.len() < 2 {
        return Vec::new();
    }
    let candidates: Vec<usize> = (1..instrs.len())
        .filter(|i| is_terminator(&instrs[i - 1]) && pushes_frame(&instrs[*i]))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    accepted_splits(record, &build_function_ir(record), candidates, stats)
}

/// Clauses 3 and 4 against a built graph.
///
/// Split out from `split_points` so the identity gate below can be exercised
/// against a graph that fails it. `build_function_ir` is the only producer in
/// production, and it is held to that ruler itself, so nothing else can reach
/// this with a malformed graph -- which is exactly why the gate needs a test that
/// can.
fn accepted_splits(
    record: &FunctionDisassembly,
    ir: &flutterdec_ir::FunctionIr,
    candidates: Vec<usize>,
    stats: &mut SplitStats,
) -> Vec<usize> {
    let instrs = &record.instructions;
    // Before the two maps below, both of which are keyed on a block identity: a
    // duplicate id or start address collapses an entry and the containment clause
    // would then measure the reach of a block it never meant to walk, cutting a
    // record in a place nothing justifies. Refusing to split is the conservative
    // answer: the record still emits exactly as it does with splitting disabled.
    if flutterdec_ir::validate_block_identity(ir).is_err() {
        stats.rejected_invalid_ir += 1;
        return Vec::new();
    }
    let branch_targets = branch_targets(ir);
    // Every instruction address to the block that contains it, not just block
    // starts. `build_function_ir` opens a new block only after a terminator, so
    // a candidate that follows anything else is mid-block and has no leader of
    // its own, and keying this on block starts silently abandoned the rest of
    // the record. Taking the containing block instead over-approximates the
    // piece, which can only reject candidates, never wrongly accept one.
    //
    // `is_terminator` below and `IROp` now agree on which mnemonics those are,
    // `brk` and `br` included, so a raising stub's successor really does get a
    // leader; this mapping stays because the general mid-block case remains.
    let containing: std::collections::HashMap<u64, usize> = ir
        .blocks
        .iter()
        .flat_map(|b| b.instrs.iter().map(move |ins| (ins.va, b.id)))
        .collect();

    // Blocks indexed by id once. `build_function_ir` numbers them densely from
    // zero, and a linear scan per visited block made this quadratic in the block
    // count, which on an inflated record of thousands of blocks is minutes.
    let mut by_id: Vec<Option<&flutterdec_ir::BasicBlock>> = vec![None; ir.blocks.len()];
    for block in &ir.blocks {
        if let Some(slot) = by_id.get_mut(block.id) {
            *slot = Some(block);
        }
    }

    let mut accepted = Vec::new();
    let mut piece_entry = ir.blocks.first().map(|b| b.id);
    // The reach of the current piece. A rejected candidate leaves the piece
    // unchanged, so recomputing it per candidate repeated identical work.
    let mut piece_reach = piece_entry.map(|entry| highest_reachable_va(&by_id, entry));
    for index in candidates {
        let va = instrs[index].va;
        if branch_targets.contains(&va) {
            stats.rejected_branch_target += 1;
            continue;
        }
        let (Some(_), Some(reach)) = (piece_entry, piece_reach) else {
            stats.rejected_no_block += 1;
            continue;
        };
        if va <= reach {
            stats.rejected_not_contained += 1;
            continue;
        }
        accepted.push(index);
        piece_entry = containing.get(&va).copied();
        piece_reach = piece_entry.map(|entry| highest_reachable_va(&by_id, entry));
    }
    accepted
}

/// A function entry is never fallen into, so its predecessor ends a path.
fn is_terminator(ins: &AsmInstruction) -> bool {
    let m = ins.mnemonic.to_ascii_lowercase();
    match m.as_str() {
        "ret" | "brk" => true,
        // An unconditional branch, which includes a tail call. `b.<cond>` falls
        // through and so does not end a path.
        "b" | "br" => true,
        _ => false,
    }
}

/// A Dart AOT prologue opens a frame through the Dart stack pointer with
/// writeback, as in `stp x29, x30, [x15, #-0x10]!`.
fn pushes_frame(ins: &AsmInstruction) -> bool {
    let m = ins.mnemonic.to_ascii_lowercase();
    if m != "stp" && m != "str" {
        return false;
    }
    ins.op_str.contains("[x15,") && ins.op_str.trim_end().ends_with("]!")
}

/// Every address a branch or jump inside the record names.
fn branch_targets(ir: &flutterdec_ir::FunctionIr) -> HashSet<u64> {
    ir.blocks
        .iter()
        .flat_map(|b| b.instrs.iter())
        .filter(|ins| matches!(ins.op, flutterdec_ir::IROp::Branch | flutterdec_ir::IROp::Jump))
        .filter_map(|ins| parse_hex_suffix(&ins.target))
        .collect()
}

/// The highest instruction address reachable from `entry` along successor edges.
fn highest_reachable_va(by_id: &[Option<&flutterdec_ir::BasicBlock>], entry: usize) -> u64 {
    let mut seen = vec![false; by_id.len()];
    let mut stack = vec![entry];
    let mut highest = 0;
    while let Some(id) = stack.pop() {
        match seen.get_mut(id) {
            Some(flag) if !*flag => *flag = true,
            _ => continue,
        }
        if let Some(Some(block)) = by_id.get(id) {
            for ins in &block.instrs {
                highest = highest.max(ins.va);
            }
            stack.extend(block.succs.iter().copied());
        }
    }
    highest
}

/// The last `0x`-prefixed hex token in a branch target operand.
fn parse_hex_suffix(target: &str) -> Option<u64> {
    target
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|t| t.trim().trim_start_matches('#').strip_prefix("0x"))
        .filter_map(|hex| u64::from_str_radix(hex, 16).ok())
        .next_back()
}

/// The record cut at each accepted point.
///
/// Only the first piece keeps the record's identity. A later piece is a function
/// the model never declared, so it gets neither the declared name nor the declared
/// owner class: propagating either would give thousands of functions a confidently
/// wrong name and class, which is worse than the `sub_<addr>` they get instead.
fn pieces(
    record: FunctionDisassembly,
    splits: &[usize],
    next_id: &mut u64,
) -> Vec<FunctionDisassembly> {
    let FunctionDisassembly {
        function_id,
        function_name,
        owner_class,
        entry_va,
        instructions,
        ..
    } = record;
    let mut bounds = vec![0usize];
    bounds.extend_from_slice(splits);
    bounds.push(instructions.len());

    let mut instrs = instructions;
    let mut out = Vec::with_capacity(bounds.len() - 1);
    // Split from the back so each drain leaves the earlier pieces untouched.
    let mut tails: Vec<Vec<AsmInstruction>> = Vec::new();
    for window in bounds.windows(2).skip(1).rev() {
        tails.push(instrs.split_off(window[0]));
    }
    tails.reverse();

    out.push(FunctionDisassembly {
        function_id,
        function_name,
        owner_class,
        entry_va,
        size: byte_size(&instrs),
        instructions: instrs,
    });
    for tail in tails {
        let Some(first) = tail.first() else { continue };
        let va = first.va;
        out.push(FunctionDisassembly {
            function_id: *next_id,
            function_name: format!("sub_{va:x}"),
            owner_class: String::new(),
            entry_va: va,
            size: byte_size(&tail),
            instructions: tail,
        });
        *next_id += 1;
    }
    out
}

/// Byte extent of a piece, from its own instructions rather than the record's
/// declared size, which described the whole span.
fn byte_size(instrs: &[AsmInstruction]) -> u64 {
    match (instrs.first(), instrs.last()) {
        (Some(first), Some(last)) => last.va - first.va + 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ins(va: u64, mnemonic: &str, op_str: &str) -> AsmInstruction {
        AsmInstruction {
            va,
            word: 0,
            mnemonic: mnemonic.to_string(),
            op_str: op_str.to_string(),
            annotation: String::new(),
        }
    }

    /// One record holding two functions: a prologue, a `ret`, then a second
    /// prologue. The second is what the adapter never declared.
    fn two_functions() -> FunctionDisassembly {
        FunctionDisassembly {
            function_id: 7,
            function_name: "declaredName".to_string(),
            owner_class: "SomeClass".to_string(),
            entry_va: 0x1000,
            size: 24,
            instructions: vec![
                ins(0x1000, "stp", "x29, x30, [x15, #-0x10]!"),
                ins(0x1004, "mov", "x0, x1"),
                ins(0x1008, "ret", ""),
                ins(0x100c, "stp", "x29, x30, [x15, #-0x10]!"),
                ins(0x1010, "mov", "x0, x2"),
                ins(0x1014, "ret", ""),
            ],
        }
    }

    #[test]
    fn a_record_holding_two_functions_is_split() {
        let (out, stats) = split_inflated_records(vec![two_functions()]);
        assert_eq!(stats.records_split, 1);
        assert_eq!(stats.functions_recovered, 1);
        assert_eq!(out.len(), 2, "one record becomes two functions: {out:?}");

        assert_eq!(out[0].entry_va, 0x1000);
        assert_eq!(out[0].instructions.len(), 3);
        assert_eq!(out[0].function_name, "declaredName", "the first piece keeps it");
        assert_eq!(out[0].size, 12, "size comes from the piece, not the record");

        assert_eq!(out[1].entry_va, 0x100c);
        assert_eq!(out[1].instructions.len(), 3);
        assert_eq!(
            out[1].function_name, "sub_100c",
            "a piece the model never declared must not take the declared name"
        );
        assert!(
            out[1].owner_class.is_empty(),
            "nor the declared owner class, which would be a wrong class for it"
        );
        assert_ne!(out[0].function_id, out[1].function_id, "ids must stay unique");
    }

    /// A frame push the record's own code branches to is intra-function control
    /// flow, not an entry. Splitting there would tear one function in half.
    #[test]
    fn a_branch_target_is_not_a_function_entry() {
        let mut record = two_functions();
        record.instructions[1] = ins(0x1004, "b", "#0x100c");
        let (out, stats) = split_inflated_records(vec![record]);
        assert_eq!(out.len(), 1, "no split: {out:?}");
        assert_eq!(stats.rejected_branch_target, 1);
        assert_eq!(stats.functions_recovered, 0);
    }

    /// Cutting above code the preceding piece still reaches would amputate a
    /// function that emits correctly today. Here the entry branches past the
    /// candidate, so the candidate is inside the first function's extent.
    #[test]
    fn a_candidate_the_preceding_piece_reaches_is_refused() {
        let record = FunctionDisassembly {
            function_id: 7,
            function_name: "declaredName".to_string(),
            owner_class: String::new(),
            entry_va: 0x1000,
            size: 28,
            instructions: vec![
                ins(0x1000, "stp", "x29, x30, [x15, #-0x10]!"),
                ins(0x1004, "b", "#0x1018"),
                ins(0x1008, "ret", ""),
                // candidate, but the entry jumps over it to 0x1018
                ins(0x100c, "stp", "x29, x30, [x15, #-0x10]!"),
                ins(0x1010, "mov", "x0, x2"),
                ins(0x1014, "ret", ""),
                ins(0x1018, "mov", "x0, x3"),
                ins(0x101c, "ret", ""),
            ],
        };
        let (out, stats) = split_inflated_records(vec![record]);
        assert_eq!(out.len(), 1, "no split: {out:?}");
        assert_eq!(stats.rejected_not_contained, 1);
        assert_eq!(stats.rejected_no_block, 0, "must not abandon silently");
    }

    /// A catch-block entry is also unreachable and also follows a terminator, but
    /// it belongs to the enclosing function: it restores a frame from x29 rather
    /// than pushing one. Splitting there would tear a function apart.
    ///
    /// Both halves of the prologue test matter. The mnemonic alone is not enough,
    /// since a store through any other base is not a frame push, and neither is one
    /// without writeback: `stp x29, x30, [x15, #16]` saves into an existing frame.
    #[test]
    fn only_a_writeback_push_through_the_dart_stack_pointer_is_an_entry() {
        for (mnemonic, op_str) in [
            // a catch entry restores from the frame pointer
            ("ldr", "x24, [x29, #-0x10]"),
            // right mnemonic, wrong base
            ("stp", "x29, x30, [sp, #-0x10]!"),
            // right mnemonic and base, no writeback
            ("stp", "x29, x30, [x15, #0x10]"),
            // a pre-indexed *load* through the Dart stack pointer pops a frame
            // rather than pushing one, and matches the operand shape exactly, so
            // the mnemonic has to be checked too
            ("ldr", "x1, [x15, #-0x10]!"),
            ("ldp", "x29, x30, [x15, #-0x10]!"),
        ] {
            let mut record = two_functions();
            record.instructions[3] = ins(0x100c, mnemonic, op_str);
            let (out, stats) = split_inflated_records(vec![record]);
            assert_eq!(out.len(), 1, "`{mnemonic} {op_str}` must not split: {out:?}");
            assert_eq!(stats.functions_recovered, 0, "`{mnemonic} {op_str}`");
        }
    }

    /// The splitter is a producer as well as a consumer: every piece it hands back
    /// is built into its own graph downstream, so a piece whose graph fails the
    /// shared ruler would push a function onto the fallback emitter or worse.
    #[test]
    fn the_record_and_every_piece_build_a_graph_the_ruler_accepts() {
        let (out, stats) = split_inflated_records(vec![two_functions()]);
        assert_eq!(out.len(), 2);
        assert_eq!(
            stats.rejected_invalid_ir, 0,
            "the builder must not produce a graph the splitter refuses"
        );
        assert_eq!(
            flutterdec_ir::validate_canonical_cfg(&build_function_ir(&two_functions())),
            Ok(()),
            "the pre-split record's own graph"
        );
        for piece in &out {
            assert_eq!(
                flutterdec_ir::validate_canonical_cfg(&build_function_ir(piece)),
                Ok(()),
                "piece at {:#x} must build a canonical graph",
                piece.entry_va
            );
        }
    }

    /// Splitting is a mutation path of its own: it cuts one instruction list into
    /// several and every piece is built into its own graph. The shapes below are
    /// the ones where a cut can land next to an edge -- a branch back across the
    /// cut, a conditional whose target is its own fallthrough, a raising stub that
    /// ends in `brk`, an indirect tail call -- so every piece of every one of them
    /// has to come out canonical.
    #[test]
    fn every_piece_of_every_split_shape_is_canonical() {
        let records = vec![
            ("two plain functions", two_functions().instructions),
            (
                "second piece branches within itself",
                vec![
                    ins(0x1000, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x1004, "ret", ""),
                    ins(0x1008, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x100c, "cbz", "x0, #0x1008"),
                    ins(0x1010, "ret", ""),
                ],
            ),
            (
                "conditional target is its own fallthrough",
                vec![
                    ins(0x1000, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x1004, "ret", ""),
                    ins(0x1008, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x100c, "cbz", "x0, #0x1010"),
                    ins(0x1010, "ret", ""),
                ],
            ),
            (
                "first piece ends in a trap",
                vec![
                    ins(0x1000, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x1004, "brk", "#0x1"),
                    ins(0x1008, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x100c, "ret", ""),
                ],
            ),
            (
                "first piece ends in an indirect branch",
                vec![
                    ins(0x1000, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x1004, "br", "x16"),
                    ins(0x1008, "stp", "x29, x30, [x15, #-0x10]!"),
                    ins(0x100c, "ret", ""),
                ],
            ),
        ];

        for (label, instructions) in records {
            let record = FunctionDisassembly {
                function_id: 7,
                function_name: "declaredName".to_string(),
                owner_class: "SomeClass".to_string(),
                entry_va: 0x1000,
                size: 4 * instructions.len() as u64,
                instructions,
            };
            let (out, stats) = split_inflated_records(vec![record]);
            assert_eq!(stats.rejected_invalid_ir, 0, "{label}");
            assert_eq!(stats.rejected_no_block, 0, "{label}");
            for piece in &out {
                assert_eq!(
                    flutterdec_ir::validate_canonical_cfg(&build_function_ir(piece)),
                    Ok(()),
                    "{label}: piece at {:#x}",
                    piece.entry_va
                );
            }
        }
    }

    /// The identity gate at the splitter's own map construction. Reached through
    /// `accepted_splits` because `build_function_ir` cannot produce a graph that
    /// fails the ruler; the gate exists for the day some other producer does.
    #[test]
    fn a_graph_that_fails_the_ruler_is_never_split_on() {
        let record = two_functions();
        let clean = build_function_ir(&record);
        let candidates = vec![3usize];

        let mut stats = SplitStats::default();
        assert_eq!(
            accepted_splits(&record, &clean, candidates.clone(), &mut stats),
            vec![3],
            "the control row: this candidate is accepted on the real graph"
        );
        assert_eq!(stats.rejected_invalid_ir, 0);

        for (label, break_it) in [
            (
                "duplicate id",
                Box::new(|ir: &mut flutterdec_ir::FunctionIr| ir.blocks[1].id = 0)
                    as Box<dyn Fn(&mut flutterdec_ir::FunctionIr)>,
            ),
            (
                "duplicate start address",
                Box::new(|ir: &mut flutterdec_ir::FunctionIr| {
                    let first = ir.blocks[0].start_va;
                    ir.blocks[1].start_va = first;
                }),
            ),
            (
                "non-dense id",
                Box::new(|ir: &mut flutterdec_ir::FunctionIr| ir.blocks[1].id = 9),
            ),
            (
                "successor names no block",
                Box::new(|ir: &mut flutterdec_ir::FunctionIr| ir.blocks[0].succs = vec![9]),
            ),
        ] {
            let mut broken = clean.clone();
            break_it(&mut broken);
            let mut stats = SplitStats::default();
            assert!(
                accepted_splits(&record, &broken, candidates.clone(), &mut stats).is_empty(),
                "{label}: no candidate may be accepted off a graph that cannot be indexed"
            );
            assert_eq!(
                stats.rejected_invalid_ir, 1,
                "{label}: the refusal must be reported, not silent"
            );
            assert_eq!(
                stats.rejected_branch_target + stats.rejected_not_contained + stats.rejected_no_block,
                0,
                "{label}: no clause may have been evaluated off the broken graph"
            );
        }
    }
}
