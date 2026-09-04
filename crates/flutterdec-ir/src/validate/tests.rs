//! The well-formedness ruler's own assertions: every planted identity and edge
//! defect is named, every diagnostic is distinct, and the canonical rebuild is
//! idempotent and repairs every edge defect.
//!
//! Test-only, and protected by digest in section 7 of
//! `docs/oracle-protocol-ir-cfg-emitter.md`. `validate.rs` itself is product
//! source that later work edits, so it carries no digest; this file carries one
//! and reaches the compiler through the declaration there.

use super::*;
use crate::{BasicBlock, LlirInstr};

fn blk(id: usize, start_va: u64, succs: Vec<usize>, preds: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va,
        instrs: vec![LlirInstr {
            va: start_va,
            op: crate::IROp::Other,
            src: "mov x0, x1".to_string(),
            target: String::new(),
        }],
        succs,
        preds,
    }
}

/// Every field of both public structs written as a literal, which is the way
/// every fixture in the workspace and every downstream consumer builds one.
/// Adding a field or sealing either struct breaks this, and breaking it is a
/// source-compatibility break for every such caller.
fn diamond() -> FunctionIr {
    FunctionIr {
        function_id: 1,
        name: "diamond".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![1, 2], vec![]),
            blk(1, 0x1004, vec![3], vec![0]),
            blk(2, 0x1008, vec![3], vec![0]),
            blk(3, 0x100c, vec![], vec![1, 2]),
        ],
    }
}

#[test]
fn a_well_formed_graph_is_accepted() {
    assert_eq!(validate_block_identity(&diamond()), Ok(()));
}

/// An empty function has no identity to be wrong. Rejecting it would report a
/// defect for a record that simply decoded to nothing.
#[test]
fn a_function_with_no_blocks_is_accepted() {
    let ir = FunctionIr {
        function_id: 2,
        name: "empty".to_string(),
        entry_va: 0x1000,
        blocks: Vec::new(),
    };
    assert_eq!(validate_block_identity(&ir), Ok(()));
}

/// One planted failure per identity rule, each rejected with the defect that
/// names it rather than with whichever check happened to run first.
#[test]
fn every_planted_identity_failure_is_named() {
    let mut ir = diamond();
    ir.blocks[2].id = 1;
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::DuplicateBlockId { id: 1 }),
        "a duplicate id is what an id-keyed map collapses"
    );

    let mut ir = diamond();
    for (offset, b) in ir.blocks.iter_mut().enumerate() {
        b.id = offset + 1;
    }
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::MissingEntryBlock),
        "there is no entry to walk from"
    );

    let mut ir = diamond();
    ir.blocks[3].id = 9;
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::NonDenseBlockId { position: 3, id: 9 }),
        "a sparse id reads another block's relations"
    );

    let mut ir = diamond();
    ir.blocks[0].id = 1;
    ir.blocks[1].id = 0;
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::NonDenseBlockId { position: 0, id: 1 }),
        "the entry must be first: position and id are read interchangeably"
    );

    let mut ir = diamond();
    ir.blocks[2].start_va = 0x1004;
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::DuplicateStartVa {
            start_va: 0x1004,
            first: 1,
            second: 2
        }),
        "an address-keyed map collapses a duplicate start"
    );

    let mut ir = diamond();
    ir.blocks[1].succs = vec![7];
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::MissingSuccessorBlock { from: 1, to: 7 }),
        "an edge to a block that does not exist is not an edge"
    );

    let mut ir = diamond();
    ir.blocks[1].preds = vec![7];
    assert_eq!(
        validate_block_identity(&ir),
        Err(CfgDefect::MissingPredecessorBlock { of: 1, from: 7 }),
        "a predecessor naming no block is not a predecessor"
    );
}

/// The diagnostic text is what the emitter puts in an artifact, so it has to
/// be stable and it has to name the defect.
#[test]
fn every_defect_renders_a_distinct_one_line_diagnostic() {
    let rendered: Vec<String> = [
        CfgDefect::DuplicateBlockId { id: 1 },
        CfgDefect::MissingEntryBlock,
        CfgDefect::NonDenseBlockId { position: 3, id: 9 },
        CfgDefect::DuplicateStartVa {
            start_va: 0x1004,
            first: 1,
            second: 2,
        },
        CfgDefect::MissingSuccessorBlock { from: 1, to: 7 },
        CfgDefect::MissingPredecessorBlock { of: 1, from: 7 },
    ]
    .iter()
    .map(|d| d.to_string())
    .collect();

    assert_eq!(
        rendered,
        vec![
            "duplicate block id 1",
            "no entry block 0",
            "block id 9 at position 3 is not dense",
            "blocks 1 and 2 both start at 0x1004",
            "edge 1 -> 7 names a block that does not exist",
            "predecessor 7 of 1 names a block that does not exist",
        ]
    );
    for text in &rendered {
        assert!(!text.contains('\n'), "a diagnostic is one line: {text}");
    }
}

/// One planted failure per edge rule. Every row is one field edit away from a
/// graph the ruler accepts, so a rejection is the edge and nothing else.
#[test]
fn every_planted_edge_failure_is_named() {
    assert_eq!(validate_canonical_cfg(&diamond()), Ok(()));

    let mut ir = diamond();
    ir.blocks[0].succs = vec![1, 1, 2];
    ir.blocks[1].preds = vec![0, 0];
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::UnorderedSuccessors { id: 0 }),
        "a duplicate successor makes one arm two"
    );

    let mut ir = diamond();
    ir.blocks[0].succs = vec![2, 1];
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::UnorderedSuccessors { id: 0 }),
        "arm order is output-affecting"
    );

    let mut ir = diamond();
    ir.blocks[3].preds = vec![1, 1, 2];
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::UnorderedPredecessors { id: 3 }),
        "a duplicate predecessor is a second claim about one path"
    );

    let mut ir = diamond();
    ir.blocks[3].preds = vec![2, 1];
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::UnorderedPredecessors { id: 3 }),
        "join provenance is recorded in predecessor order"
    );

    let mut ir = diamond();
    ir.blocks[3].preds = vec![2];
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::SuccessorWithoutPredecessor { from: 1, to: 3 }),
        "an edge only the successor side knows about"
    );

    let mut ir = diamond();
    ir.blocks[1].succs = Vec::new();
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::PredecessorWithoutSuccessor { of: 3, from: 1 }),
        "an edge only the predecessor side knows about"
    );

    // Identity is checked first, so an edge clause can never mask a defect
    // that would make the edge clauses index the wrong rows.
    let mut ir = diamond();
    ir.blocks[3].id = 9;
    ir.blocks[3].preds = vec![2, 1];
    assert_eq!(
        validate_canonical_cfg(&ir),
        Err(CfgDefect::NonDenseBlockId { position: 3, id: 9 }),
        "identity comes first"
    );
}

#[test]
fn every_edge_defect_renders_a_distinct_one_line_diagnostic() {
    let rendered: Vec<String> = [
        CfgDefect::UnorderedSuccessors { id: 0 },
        CfgDefect::UnorderedPredecessors { id: 3 },
        CfgDefect::SuccessorWithoutPredecessor { from: 1, to: 3 },
        CfgDefect::PredecessorWithoutSuccessor { of: 3, from: 1 },
    ]
    .iter()
    .map(|d| d.to_string())
    .collect();
    assert_eq!(
        rendered,
        vec![
            "successors of 0 are not ascending and unique",
            "predecessors of 3 are not ascending and unique",
            "edge 1 -> 3 is missing its predecessor",
            "predecessor 1 of 3 has no matching edge",
        ]
    );
    for text in &rendered {
        assert!(!text.contains('\n'), "a diagnostic is one line: {text}");
    }
}

/// The canonical path takes any of the ways a mutation can leave edges wrong
/// and produces the one form the ruler accepts, without a caller having to
/// know which of them applied.
#[test]
fn the_canonical_rebuild_repairs_every_edge_defect() {
    let mut ir = diamond();
    // Unsorted, duplicated, and pointing at a block that does not exist.
    ir.blocks[0].succs = vec![2, 1, 1, 9];
    // Stale on both sides: one predecessor that no longer has an edge, one
    // edge whose predecessor was never recorded, and a duplicate.
    ir.blocks[1].preds = vec![3, 3];
    ir.blocks[2].preds = Vec::new();
    ir.blocks[3].preds = vec![2, 1, 1];

    rebuild_edges(&mut ir.blocks);

    assert_eq!(validate_canonical_cfg(&ir), Ok(()));
    assert_eq!(
        ir.blocks[0].succs,
        vec![1, 2],
        "sorted, unique, and existing"
    );
    assert_eq!(ir.blocks[0].preds, Vec::<usize>::new());
    assert_eq!(ir.blocks[1].preds, vec![0], "derived from successors alone");
    assert_eq!(ir.blocks[2].preds, vec![0]);
    assert_eq!(ir.blocks[3].preds, vec![1, 2]);
    assert!(
        !ir.blocks[0].succs.contains(&9),
        "an edge to a block that does not exist is not an edge"
    );
}

/// The rebuild is idempotent: running it on its own output changes nothing, so
/// a pass that calls it twice cannot drift.
#[test]
fn the_canonical_rebuild_is_idempotent() {
    let mut once = diamond();
    once.blocks[0].succs = vec![2, 1, 1];
    rebuild_edges(&mut once.blocks);
    let mut twice = once.clone();
    rebuild_edges(&mut twice.blocks);
    for (a, b) in once.blocks.iter().zip(&twice.blocks) {
        assert_eq!(a.succs, b.succs, "block {}", a.id);
        assert_eq!(a.preds, b.preds, "block {}", a.id);
    }
}

/// The guard prune removes the guard's own slow path and nothing else. A block
/// unreachable for any other reason is code the adapter merged in from a
/// neighbouring function, and deleting it would silently lose real program
/// text, so it must survive with its ids still dense and its edges canonical.
#[test]
fn only_the_guard_stranded_blocks_are_pruned() {
    use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};

    let ins = |va: u64, mnemonic: &str, op_str: &str| AsmInstruction {
        va,
        word: 0,
        mnemonic: mnemonic.to_string(),
        op_str: op_str.to_string(),
        annotation: String::new(),
    };
    // The guard and its slow path at 0x1014, plus an island at 0x1020 that
    // nothing in the record reaches and that the guard never reached either.
    let d = FunctionDisassembly {
        function_id: 11,
        function_name: "guardedWithIsland".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: 0x2c,
        instructions: vec![
            ins(0x1000, "ldr", "x16, [x26, #0x38]"),
            ins(0x1004, "cmp", "x15, x16"),
            ins(0x1008, "b.ls", "#0x1014"),
            ins(0x100c, "mov", "x0, x1"),
            ins(0x1010, "ret", ""),
            // Guard slow path: calls the stub, jumps back into the body.
            ins(0x1014, "bl", "#0x9000"),
            ins(0x1018, "b", "#0x100c"),
            // Island: unreachable, and not through the guard.
            ins(0x101c, "mov", "x2, x3"),
            ins(0x1020, "ret", ""),
        ],
    };

    let ir = crate::build_function_ir(&d);
    assert_eq!(validate_canonical_cfg(&ir), Ok(()));

    let starts: Vec<u64> = ir.blocks.iter().map(|b| b.start_va).collect();
    assert!(
        !starts.contains(&0x1014),
        "the guard's slow path is the one thing pruned: {starts:x?}"
    );
    assert!(
        starts.contains(&0x101c),
        "an unrelated unreachable block must survive: {starts:x?}"
    );
    assert_eq!(
        ir.blocks
            .iter()
            .find(|b| b.start_va == 0x101c)
            .map(|b| (b.succs.clone(), b.preds.clone())),
        Some((Vec::new(), Vec::new())),
        "it survives as an orphan, with no edge invented in either direction"
    );
}

/// The builder's own output has to satisfy the ruler its consumers apply, on
/// every path it can take: a conditional with a fallthrough, a conditional
/// whose target *is* its fallthrough so the derived list holds one block
/// twice, the terminators that take no edge at all, and the guarded shape
/// whose slow path is pruned and whose ids are all remapped.
#[test]
fn the_builder_is_canonical_on_every_path_it_takes() {
    use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};

    let ins = |va: u64, mnemonic: &str, op_str: &str| AsmInstruction {
        va,
        word: 0,
        mnemonic: mnemonic.to_string(),
        op_str: op_str.to_string(),
        annotation: String::new(),
    };
    let record = |instructions: Vec<AsmInstruction>| FunctionDisassembly {
        function_id: 3,
        function_name: "built".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: 64,
        instructions,
    };

    let cases = vec![
        (
            "conditional, indirect branch and trap",
            record(vec![
                ins(0x1000, "cbz", "x0, #0x1010"),
                ins(0x1004, "br", "x16"),
                ins(0x1008, "brk", "#0x1"),
                ins(0x100c, "ret", ""),
                ins(0x1010, "ret", ""),
            ]),
            None,
        ),
        (
            "conditional whose target is its own fallthrough",
            record(vec![
                ins(0x1000, "cbz", "x0, #0x1004"),
                ins(0x1004, "mov", "x0, x1"),
                ins(0x1008, "ret", ""),
            ]),
            // One block, named once, not twice.
            Some(vec![vec![1usize], vec![]]),
        ),
        (
            "unconditional jump to its own fallthrough",
            record(vec![ins(0x1000, "b", "#0x1004"), ins(0x1004, "ret", "")]),
            Some(vec![vec![1usize], vec![]]),
        ),
        (
            "guard and its pruned slow path",
            record(vec![
                ins(0x1000, "ldr", "x16, [x26, #0x38]"),
                ins(0x1004, "cmp", "x15, x16"),
                ins(0x1008, "b.ls", "#0x1014"),
                ins(0x100c, "mov", "x0, x1"),
                ins(0x1010, "ret", ""),
                ins(0x1014, "bl", "#0x9000"),
                ins(0x1018, "b", "#0x100c"),
            ]),
            None,
        ),
        (
            "no instructions at all",
            record(Vec::new()),
            Some(Vec::new()),
        ),
    ];

    for (label, case, expected_succs) in cases {
        let ir = crate::build_function_ir(&case);
        assert_eq!(
            validate_canonical_cfg(&ir),
            Ok(()),
            "{label}: the builder must not emit a graph its own consumers refuse: {:?}",
            ir.blocks
                .iter()
                .map(|b| (b.id, b.start_va, b.succs.clone(), b.preds.clone()))
                .collect::<Vec<_>>()
        );
        if let Some(expected) = expected_succs {
            assert_eq!(
                ir.blocks
                    .iter()
                    .map(|b| b.succs.clone())
                    .collect::<Vec<_>>(),
                expected,
                "{label}: edge list"
            );
        }
    }
}
