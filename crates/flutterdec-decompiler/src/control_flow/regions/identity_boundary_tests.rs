//! The analysis boundary in isolation.
//!
//! `Regions::build` is where a graph first becomes id-indexed relation vectors,
//! and `structured.rs` reads every one of them back by block id. The public
//! emitter refuses a graph that fails the shared ruler before reaching here, so
//! these cases can only be produced by calling the analysis directly -- which is
//! exactly what a later in-crate caller would do, and what this pins.
//!
//! Test-only, and protected by digest in section 7 of
//! `docs/oracle-protocol-ir-cfg-emitter.md`. `regions.rs` is product source that
//! later work edits, so it carries no digest; this file carries one.

use super::*;
use flutterdec_ir::LlirInstr;

fn blk(id: usize, start_va: u64, succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va,
        instrs: vec![LlirInstr {
            va: start_va,
            op: IROp::Other,
            src: "mov x0, x1".to_string(),
            target: String::new(),
        }],
        succs,
        preds: Vec::new(),
    }
}

fn diamond() -> FunctionIr {
    FunctionIr {
        function_id: 1,
        name: "diamond".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![1, 2]),
            blk(1, 0x1004, vec![3]),
            blk(2, 0x1008, vec![3]),
            blk(3, 0x100c, Vec::new()),
        ],
    }
}

/// The control row: this shape is reducible and must still be analysed, so a
/// decline below cannot be blamed on the fixture.
#[test]
fn a_well_formed_reducible_graph_is_still_analysed() {
    let regions = Regions::build(&diamond()).expect("a diamond is reducible");
    assert!(regions.is_join(3), "block 3 is the join");
    assert_eq!(regions.reachable_count(), 4);
}

#[test]
fn every_planted_identity_failure_declines_before_any_relation_is_built() {
    let mut duplicate_id = diamond();
    duplicate_id.blocks[2].id = 1;

    let mut sparse_id = diamond();
    sparse_id.blocks[3].id = 9;

    let mut entry_not_first = diamond();
    entry_not_first.blocks.swap(0, 1);

    let mut duplicate_start = diamond();
    duplicate_start.blocks[2].start_va = 0x1004;

    let mut missing_succ = diamond();
    missing_succ.blocks[1].succs = vec![7];

    let mut missing_pred = diamond();
    missing_pred.blocks[1].preds = vec![7];

    let mut no_entry = diamond();
    for (offset, b) in no_entry.blocks.iter_mut().enumerate() {
        b.id = offset + 1;
        b.succs = Vec::new();
    }

    for (label, ir) in [
        ("duplicate id", duplicate_id),
        ("non-dense id", sparse_id),
        ("entry not first", entry_not_first),
        ("duplicate start address", duplicate_start),
        ("successor names no block", missing_succ),
        ("predecessor names no block", missing_pred),
        ("no entry block 0", no_entry),
    ] {
        assert!(
            Regions::build(&ir).is_none(),
            "{label}: relation analysis must decline, not read another block's rows"
        );
    }
}
