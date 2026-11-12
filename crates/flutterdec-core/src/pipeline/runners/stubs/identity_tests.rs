//! The no-return prune's identity boundary: the prune is a mutation path that
//! drops a whole successor list, so it has to leave a canonical graph, remove
//! exactly the cut edge and nothing else, and refuse outright a graph the shared
//! ruler rejects.
//!
//! Test-only, and protected by digest in section 7 of
//! `docs/oracle-protocol-ir-cfg-emitter.md`. `stubs.rs` is product source that
//! later work edits, so it carries no digest; this file carries one.

use super::prune_tests::{blk, call, other};
use super::*;
use flutterdec_ir::BasicBlock;

/// The prune is a mutation path: it drops a block's whole successor list. The
/// shapes below are the ones where dropping one edge leaves the other side of
/// a *shared* target to keep -- a join still reached by another path, a back
/// edge, a block that was its own successor -- so re-deriving predecessors has
/// to remove exactly the cut edge and nothing else.
#[test]
fn every_shape_the_prune_mutates_comes_out_canonical() {
    let shapes: Vec<(&str, Vec<BasicBlock>)> = vec![
        (
            "join still reached by a second path",
            vec![
                blk(0, 0x1000, vec![other(0x1000, "cbz x0, #0x1008")], vec![1, 2]),
                blk(1, 0x1004, vec![call(0x1004, "#0x9000")], vec![3]),
                blk(2, 0x1008, vec![other(0x1008, "mov x1, x2")], vec![3]),
                blk(3, 0x100c, vec![other(0x100c, "ret")], vec![]),
            ],
        ),
        (
            "the cut block is a loop's back edge",
            vec![
                blk(0, 0x1000, vec![other(0x1000, "mov x0, x1")], vec![1]),
                blk(1, 0x1004, vec![other(0x1004, "cbz x0, #0x100c")], vec![2, 3]),
                blk(2, 0x1008, vec![call(0x1008, "#0x9000")], vec![1]),
                blk(3, 0x100c, vec![other(0x100c, "ret")], vec![]),
            ],
        ),
        (
            "two cut blocks share one target",
            vec![
                blk(0, 0x1000, vec![other(0x1000, "cbz x0, #0x1008")], vec![1, 2]),
                blk(1, 0x1004, vec![call(0x1004, "#0x9000")], vec![3]),
                blk(2, 0x1008, vec![call(0x1008, "#0x9000")], vec![3]),
                blk(3, 0x100c, vec![other(0x100c, "ret")], vec![]),
            ],
        ),
        (
            "the cut block is its own successor",
            vec![
                blk(0, 0x1000, vec![other(0x1000, "mov x0, x1")], vec![1]),
                blk(1, 0x1004, vec![call(0x1004, "#0x9000")], vec![1]),
            ],
        ),
        (
            "nothing to cut at all",
            vec![
                blk(0, 0x1000, vec![call(0x1000, "#0x8000")], vec![1]),
                blk(1, 0x1004, vec![other(0x1004, "ret")], vec![]),
            ],
        ),
    ];

    for (label, blocks) in shapes {
        let mut ir = vec![FunctionIr {
            function_id: 1,
            name: "sub_1000".to_string(),
            entry_va: 0x1000,
            blocks,
        }];
        flutterdec_ir::rebuild_edges(&mut ir[0].blocks);
        assert_eq!(
            flutterdec_ir::validate_canonical_cfg(&ir[0]),
            Ok(()),
            "{label}: the fixture itself"
        );

        let stats = prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));
        assert_eq!(stats.skipped_invalid_ir, 0, "{label}");
        assert_eq!(
            flutterdec_ir::validate_canonical_cfg(&ir[0]),
            Ok(()),
            "{label}: after the prune: {:?}",
            ir[0]
                .blocks
                .iter()
                .map(|b| (b.id, b.succs.clone(), b.preds.clone()))
                .collect::<Vec<_>>()
        );
        // No block may be removed, whatever was cut: `regions.rs` needs the
        // ids dense and renumbering here would push the function onto the
        // fallback emitter.
        assert_eq!(
            ir[0].blocks.iter().map(|b| b.id).collect::<Vec<_>>(),
            (0..ir[0].blocks.len()).collect::<Vec<_>>(),
            "{label}: ids must stay dense"
        );
    }
}

/// A block no cut touched must keep exactly the predecessors it had. Re-deriving
/// the whole predecessor side is what makes the cut edge disappear, and it must
/// not take an unrelated one with it.
#[test]
fn the_prune_removes_only_the_cut_edge() {
    let mut ir = vec![FunctionIr {
        function_id: 1,
        name: "sub_1000".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![other(0x1000, "cbz x0, #0x1008")], vec![1, 2]),
            blk(1, 0x1004, vec![call(0x1004, "#0x9000")], vec![3]),
            blk(2, 0x1008, vec![other(0x1008, "mov x1, x2")], vec![3]),
            blk(3, 0x100c, vec![other(0x100c, "ret")], vec![]),
        ],
    }];
    flutterdec_ir::rebuild_edges(&mut ir[0].blocks);
    assert_eq!(ir[0].blocks[3].preds, vec![1, 2]);

    prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));

    assert_eq!(ir[0].blocks[1].succs, Vec::<usize>::new(), "the cut edge");
    assert_eq!(
        ir[0].blocks[3].preds,
        vec![2],
        "block 2's edge into the join is unrelated and must survive"
    );
    assert_eq!(ir[0].blocks[0].succs, vec![1, 2], "nothing else moved");
    assert_eq!(ir[0].blocks[1].preds, vec![0]);
    assert_eq!(ir[0].blocks[2].preds, vec![0]);
}

/// The identity gate at the prune's own boundary. The reachability walk below
/// it indexes blocks by id and its result is published as blocks removed, so a
/// graph that cannot be indexed must not be walked at all -- and must not be
/// silently mutated either.
#[test]
fn a_graph_that_fails_the_ruler_is_never_pruned() {
    let fixture = || {
        let mut ir = vec![FunctionIr {
            function_id: 1,
            name: "sub_1000".to_string(),
            entry_va: 0x1000,
            blocks: vec![
                blk(0, 0x1000, vec![other(0x1000, "mov x0, x1")], vec![1]),
                blk(1, 0x1004, vec![call(0x1004, "#0x9000")], vec![2]),
                blk(2, 0x1008, vec![other(0x1008, "ret")], vec![]),
            ],
        }];
        for b in 0..ir[0].blocks.len() {
            let succs = ir[0].blocks[b].succs.clone();
            let id = ir[0].blocks[b].id;
            for s in succs {
                ir[0].blocks[s].preds.push(id);
            }
        }
        ir
    };

    // Control row: on the well-formed graph the prune does its work.
    let mut ir = fixture();
    let stats = prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));
    assert_eq!(stats.functions, 1);
    assert_eq!(stats.skipped_invalid_ir, 0);
    assert!(ir[0].blocks[1].succs.is_empty());

    /// One named way to break the fixture's block identity.
    type Breaker = (&'static str, Box<dyn Fn(&mut FunctionIr)>);
    let breakers: Vec<Breaker> = vec![
        ("duplicate id", Box::new(|f: &mut FunctionIr| f.blocks[2].id = 1)),
        ("non-dense id", Box::new(|f: &mut FunctionIr| f.blocks[2].id = 9)),
        (
            "duplicate start address",
            Box::new(|f: &mut FunctionIr| f.blocks[2].start_va = 0x1004),
        ),
        (
            "successor names no block",
            Box::new(|f: &mut FunctionIr| f.blocks[1].succs = vec![9]),
        ),
        (
            "predecessor names no block",
            Box::new(|f: &mut FunctionIr| f.blocks[2].preds = vec![9]),
        ),
        (
            "duplicate successor",
            Box::new(|f: &mut FunctionIr| f.blocks[1].succs = vec![2, 2]),
        ),
        (
            "unsorted successors",
            Box::new(|f: &mut FunctionIr| f.blocks[0].succs = vec![2, 1]),
        ),
        (
            "duplicate predecessor",
            Box::new(|f: &mut FunctionIr| f.blocks[2].preds = vec![1, 1]),
        ),
        (
            "asymmetric edge, successor side only",
            Box::new(|f: &mut FunctionIr| f.blocks[2].preds = Vec::new()),
        ),
        (
            "asymmetric edge, predecessor side only",
            Box::new(|f: &mut FunctionIr| f.blocks[1].succs = Vec::new()),
        ),
    ];
    for (label, break_it) in breakers {
        let mut ir = fixture();
        break_it(&mut ir[0]);
        let before = ir.clone();
        let stats = prune_calls_that_never_return(&mut ir, &HashSet::from([0x9000]));
        assert_eq!(
            stats.skipped_invalid_ir, 1,
            "{label}: the skip must be reported, not silent"
        );
        assert_eq!(stats.functions, 0, "{label}");
        assert_eq!(stats.blocks_cut, 0, "{label}: nothing was walked");
        assert_eq!(stats.instructions_cut, 0, "{label}");
        for (was, now) in before[0].blocks.iter().zip(&ir[0].blocks) {
            assert_eq!(was.succs, now.succs, "{label}: block {} mutated", was.id);
            assert_eq!(was.preds, now.preds, "{label}: block {} mutated", was.id);
            assert_eq!(was.instrs.len(), now.instrs.len(), "{label}");
        }
    }
}
