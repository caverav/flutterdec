//! The public emission surface against a `FunctionIr` whose block identity does
//! not hold.
//!
//! Every consumer inside the emitter indexes blocks by id or by start address, so
//! a graph that fails the shared ruler in `flutterdec-ir` cannot be emitted from
//! at all: an id-keyed map has already collapsed the duplicate by the time any
//! relation is read off it. What the emitter owes instead is a deterministic
//! artifact that says so, with no traversal behind it and no panic.
//!
//! An integration test on purpose. It compiles as its own crate and reaches the
//! emitter only through `pub` functions, so it proves the gate is on the public
//! path rather than on some internal helper a caller could route around.

use flutterdec_decompiler::{
    emit_program, emit_pseudocode, emit_pseudocode_with_pool_hints, PseudocodeArtifact,
    INVALID_CFG_NOTE,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

/// A call the lifter models and a symbol for its target, so a body that was
/// emitted is impossible to mistake for one that was not.
const CALLEE_VA: u64 = 0x9000;
const CALLEE_NAME: &str = "recognisableCallee";

fn call(va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #{CALLEE_VA:#x}"),
        target: format!("#{CALLEE_VA:#x}"),
    }
}

fn ret(va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Return,
        src: "ret".to_string(),
        target: String::new(),
    }
}

fn cbz(va: u64, target_va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Branch,
        src: format!("cbz x1, #{target_va:#x}"),
        target: format!("#{target_va:#x}"),
    }
}

/// Both public structs written as literals with every field named, which is how
/// every fixture in this workspace and every downstream caller builds one. If
/// either struct gained a field or were sealed, this file would stop compiling,
/// so compiling it is the source-compatibility check.
fn blk(id: usize, start_va: u64, instrs: Vec<LlirInstr>, succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va,
        instrs,
        succs,
        preds: Vec::new(),
    }
}

/// A reducible diamond the structured emitter handles, carrying a modelled call
/// on one arm. Every planted defect below is one field edit away from this, so a
/// refusal cannot be blamed on the shape.
fn diamond() -> FunctionIr {
    FunctionIr {
        function_id: 77,
        name: "identityFixture".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, 0x1008)], vec![1, 2]),
            blk(1, 0x1004, vec![call(0x1004)], vec![3]),
            blk(2, 0x1008, vec![call(0x1008)], vec![3]),
            blk(3, 0x100c, vec![ret(0x100c)], Vec::new()),
        ],
    }
}

fn symbols() -> HashMap<u64, String> {
    HashMap::from([(CALLEE_VA, CALLEE_NAME.to_string())])
}

/// Every way the shared ruler can reject a graph, reached through the public
/// emitter. One field edit per row, so the fixture shape is held fixed.
fn planted_identity_failures() -> Vec<(&'static str, FunctionIr, &'static str)> {
    let mut duplicate_id = diamond();
    duplicate_id.blocks[2].id = 1;

    let mut missing_entry = diamond();
    for (offset, b) in missing_entry.blocks.iter_mut().enumerate() {
        b.id = offset + 1;
    }
    for b in missing_entry.blocks.iter_mut() {
        b.succs = b.succs.iter().map(|s| s + 1).collect();
    }

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

    vec![
        ("duplicate id", duplicate_id, "duplicate block id 1"),
        ("missing entry 0", missing_entry, "no entry block 0"),
        (
            "non-dense id",
            sparse_id,
            "block id 9 at position 3 is not dense",
        ),
        (
            "entry not first",
            entry_not_first,
            "block id 1 at position 0 is not dense",
        ),
        (
            "duplicate start address",
            duplicate_start,
            "blocks 1 and 2 both start at 0x1004",
        ),
        (
            "successor names no block",
            missing_succ,
            "edge 1 -> 7 names a block that does not exist",
        ),
        (
            "predecessor names no block",
            missing_pred,
            "predecessor 7 of 1 names a block that does not exist",
        ),
    ]
}

/// Nothing the artifact says may have come from walking the graph.
fn assert_no_traversal(label: &str, artifact: &PseudocodeArtifact) {
    let source = &artifact.source;
    for evidence in [
        CALLEE_NAME,
        "_block_",
        "while (",
        "if (",
        "return ",
        "goto",
        // Any binding at all. A lifted instruction is the only thing that
        // assigns; the signature and the diagnostic carry no `=`.
        "= ",
        "reg1",
    ] {
        assert!(
            !source.contains(evidence),
            "{label}: `{evidence}` can only come from emitting a block:\n{source}"
        );
    }
    assert_eq!(
        artifact.total_calls, 0,
        "{label}: the graph holds two modelled calls; counting one means a block was lifted"
    );
    assert_eq!(artifact.placeholder_ifs, 0, "{label}");
    assert_eq!(artifact.repeated_blocks, 0, "{label}");
    assert_eq!(artifact.indirect_calls, 0, "{label}");
    assert_eq!(artifact.raw_register_calls, 0, "{label}");
    assert_eq!(artifact.semantic_direct_calls, 0, "{label}");
    assert_eq!(artifact.semantic_indirect_calls, 0, "{label}");
    assert_eq!(artifact.dispatch_selector_calls, 0, "{label}");
    assert_eq!(artifact.dispatch_table_calls, 0, "{label}");
    assert_eq!(artifact.unlifted_instructions, 0, "{label}");
    assert_eq!(artifact.target_va_symbol_calls, 0, "{label}");
}

/// The control row. The same fixture with its identity intact must emit a real
/// body, otherwise every assertion above would also hold for a broken gate that
/// refuses everything.
#[test]
fn the_same_fixture_with_its_identity_intact_still_emits_a_body() {
    let artifact = emit_pseudocode(&diamond(), &symbols());
    assert!(
        !artifact.source.contains(INVALID_CFG_NOTE),
        "a well-formed graph must not be refused:\n{}",
        artifact.source
    );
    assert!(
        artifact.source.contains(CALLEE_NAME),
        "the modelled call must reach the artifact:\n{}",
        artifact.source
    );
    assert_eq!(
        artifact.total_calls, 2,
        "both calls are emitted: {artifact:?}"
    );
    assert_eq!(
        artifact.unresolved_cf, 0,
        "nothing in this graph is unresolved: {artifact:?}"
    );
}

#[test]
fn every_planted_identity_failure_emits_one_diagnostic_and_no_body() {
    for (label, ir, defect) in planted_identity_failures() {
        let artifact = emit_pseudocode(&ir, &symbols());

        assert_eq!(
            artifact.source,
            format!(
                "dynamic identityFixture(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5) {{\n  // {INVALID_CFG_NOTE}: {defect}: control flow not recovered\n}}"
            ),
            "{label}: the artifact must be exactly the signature and one diagnostic"
        );
        assert_eq!(
            artifact.source.matches(INVALID_CFG_NOTE).count(),
            1,
            "{label}: exactly one invalid-CFG diagnostic"
        );
        assert_eq!(
            artifact.source.lines().filter(|l| l.contains("//")).count(),
            1,
            "{label}: the diagnostic is the only comment"
        );
        assert!(
            artifact.unresolved_cf > 0,
            "{label}: an unemitted body is unresolved control flow: {artifact:?}"
        );
        assert_eq!(
            artifact.function_id, 77,
            "{label}: the artifact still identifies its function"
        );
        assert_eq!(artifact.function_name, "identityFixture", "{label}");
        assert_no_traversal(label, &artifact);
    }
}

/// A refusal has to be a property of the graph, not of which entry point was
/// called or of how many times it was called.
#[test]
fn the_refusal_is_identical_across_entry_points_and_repeat_calls() {
    for (label, ir, _) in planted_identity_failures() {
        let plain = emit_pseudocode(&ir, &symbols());
        let again = emit_pseudocode(&ir, &symbols());
        let with_hints = emit_pseudocode_with_pool_hints(
            &ir,
            &symbols(),
            &HashMap::from([(0x1004u64, "pooled".to_string())]),
        );
        let program = emit_program(std::slice::from_ref(&ir), &symbols());

        assert_eq!(plain.source, again.source, "{label}: not deterministic");
        assert_eq!(
            plain.source, with_hints.source,
            "{label}: pool hints must not change a refusal"
        );
        assert_eq!(program.len(), 1, "{label}");
        assert_eq!(
            plain.source, program[0].source,
            "{label}: the program surface must refuse the same way"
        );
        assert_eq!(plain.unresolved_cf, program[0].unresolved_cf, "{label}");
    }
}

/// One invalid function in a program must not stop the valid ones being emitted,
/// and must not be repaired by a neighbour either.
#[test]
fn a_program_emits_its_valid_functions_beside_a_refused_one() {
    let mut invalid = diamond();
    invalid.blocks[2].id = 1;
    invalid.function_id = 78;
    invalid.name = "refused".to_string();

    let artifacts = emit_program(&[diamond(), invalid, diamond()], &symbols());
    assert_eq!(artifacts.len(), 3);
    assert!(artifacts[0].source.contains(CALLEE_NAME));
    assert!(artifacts[1].source.contains(INVALID_CFG_NOTE));
    assert_eq!(artifacts[1].function_name, "refused");
    assert!(artifacts[2].source.contains(CALLEE_NAME));
    assert_no_traversal("in a program", &artifacts[1]);
}

/// A graph with no blocks has no identity to be wrong: nothing indexes it and
/// nothing walks it. Refusing it would report a defect for a record that decoded
/// to nothing, which is a valid graph rejected.
#[test]
fn an_empty_graph_is_not_treated_as_a_defect() {
    let ir = FunctionIr {
        function_id: 79,
        name: "empty".to_string(),
        entry_va: 0x1000,
        blocks: Vec::new(),
    };
    let artifact = emit_pseudocode(&ir, &symbols());
    assert!(
        !artifact.source.contains(INVALID_CFG_NOTE),
        "an empty function is not a malformed graph:\n{}",
        artifact.source
    );
    assert_eq!(artifact.unresolved_cf, 0, "{artifact:?}");
}

/// Shapes that would make a traversal recurse, index out of range or loop
/// forever if one ever ran on a graph the ruler rejected.
#[test]
fn a_hostile_invalid_graph_refuses_without_panicking() {
    let hostile = vec![
        // Every block claims id 0 and points at itself.
        FunctionIr {
            function_id: 80,
            name: "allZero".to_string(),
            entry_va: 0x1000,
            blocks: (0..4)
                .map(|i| blk(0, 0x1000 + i * 4, vec![ret(0x1000 + i * 4)], vec![0]))
                .collect(),
        },
        // A single block whose successor is far out of range.
        FunctionIr {
            function_id: 81,
            name: "outOfRange".to_string(),
            entry_va: 0x1000,
            blocks: vec![blk(
                0,
                0x1000,
                vec![cbz(0x1000, 0x9999)],
                vec![usize::MAX, 1],
            )],
        },
        // Ids that skip, with edges that name the skipped numbers.
        FunctionIr {
            function_id: 82,
            name: "skipped".to_string(),
            entry_va: 0x1000,
            blocks: vec![
                blk(0, 0x1000, vec![cbz(0x1000, 0x1008)], vec![2, 4]),
                blk(2, 0x1004, vec![call(0x1004)], vec![4]),
                blk(4, 0x1008, vec![ret(0x1008)], Vec::new()),
            ],
        },
    ];

    for ir in hostile {
        let label = ir.name.clone();
        let artifact = emit_pseudocode(&ir, &symbols());
        assert!(
            artifact.source.contains(INVALID_CFG_NOTE),
            "{label} must be refused:\n{}",
            artifact.source
        );
        assert_eq!(
            artifact.source.lines().count(),
            3,
            "{label}: signature, diagnostic, close:\n{}",
            artifact.source
        );
        assert_no_traversal(&label, &artifact);
    }
}
