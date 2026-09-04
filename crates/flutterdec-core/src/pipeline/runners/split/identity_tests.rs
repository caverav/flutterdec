//! The splitter's identity boundary: every piece it hands back is built into a
//! graph of its own downstream, so each piece has to satisfy the shared ruler,
//! and a record whose graph fails that ruler must never be split on.
//!
//! Test-only, and protected by digest in section 7 of
//! `docs/oracle-protocol-ir-cfg-emitter.md`. `split.rs` is product source that
//! later work edits, so it carries no digest; this file carries one.

use super::tests::{ins, two_functions};
use super::*;


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
