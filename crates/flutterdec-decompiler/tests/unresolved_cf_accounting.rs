//! `unresolved_cf` must equal the unresolved-control-flow statements the
//! artifact actually carries.
//!
//! The walk's own increments are not that number. A block the DFS walk refuses
//! to inline is rendered by a *nested* emitter in `append_helper_functions`,
//! whose counters are dropped when its lines are copied into the body, and
//! `inline_helper_calls` then copies that same body to every call site. Both
//! paths put unresolved-control-flow statements into the artifact that the walk
//! never counted: on the pinned LocalSend baseline one function carried four
//! `// indirect branch through reg2: target not recovered` lines against a
//! function counter of zero, and the whole-run counter read 517 against 521
//! emitted statements.
//!
//! These tests pin the artifact-side scope: the counter is recounted from the
//! finished body, so a reader can reproduce it from the artifact alone.

use flutterdec_decompiler::{
    emit_pseudocode, emit_pseudocode_direct_dfs, PseudocodeArtifact, TraversalEventKind,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

/// The three statements that report unresolved control flow, by prefix. Written
/// out here rather than imported: the emitter's constants are private, and an
/// expected value that reads the value under test proves nothing.
const UNRESOLVED_PREFIXES: [&str; 4] = [
    "// indirect branch",
    "// unresolved branch target",
    "// unresolved jump",
    // A function whose CFG did not validate is one unresolved site, and its
    // whole body is this diagnostic.
    "// invalid CFG",
];

fn instr(va: u64, op: IROp, src: &str, target: &str) -> LlirInstr {
    LlirInstr {
        va,
        op,
        src: src.to_string(),
        target: target.to_string(),
    }
}

fn blk(id: usize, start_va: u64, instrs: Vec<LlirInstr>, succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va,
        instrs,
        succs,
        preds: Vec::new(),
    }
}

fn function(name: &str, blocks: Vec<BasicBlock>) -> FunctionIr {
    FunctionIr {
        function_id: 1,
        name: name.to_string(),
        entry_va: blocks[0].start_va,
        blocks,
    }
}

/// Statements in the finished body, counted the way a reader would.
fn unresolved_statements(artifact: &PseudocodeArtifact) -> usize {
    artifact
        .source
        .lines()
        .filter(|line| {
            let text = line.trim_start();
            UNRESOLVED_PREFIXES
                .iter()
                .any(|prefix| text.starts_with(prefix))
        })
        .count()
}

fn assert_counter_is_the_body(artifact: &PseudocodeArtifact, expected: usize) {
    let counted = unresolved_statements(artifact);
    assert_eq!(
        counted, expected,
        "fixture should emit {expected} unresolved-control-flow statements:\n{}",
        artifact.source
    );
    assert_eq!(
        artifact.unresolved_cf, counted,
        "unresolved_cf must equal the statements in the body:\n{}",
        artifact.source
    );
}

/// A chain longer than the DFS inline-depth budget, ending in `br x16`.
///
/// Past the budget the walk stops inlining, so the tail block is emitted as a
/// `_block_N` helper body by a nested emitter. Before this counter moved to the
/// body, that helper's `br` was rendered into the artifact and counted nowhere.
fn deep_chain(tail_op: IROp, tail_src: &str, tail_target: &str) -> FunctionIr {
    const CHAIN: usize = 20;
    let mut blocks = Vec::new();
    for id in 0..CHAIN {
        let va = 0x1000 + (id as u64) * 0x10;
        blocks.push(blk(
            id,
            va,
            vec![
                instr(va, IROp::Other, "mov x0, x1", ""),
                instr(
                    va + 4,
                    IROp::Jump,
                    &format!("b #{:#x}", va + 0x10),
                    &format!("#{:#x}", va + 0x10),
                ),
            ],
            vec![id + 1],
        ));
    }
    let tail_va = 0x1000 + (CHAIN as u64) * 0x10;
    blocks.push(blk(
        CHAIN,
        tail_va,
        vec![instr(tail_va, tail_op, tail_src, tail_target)],
        vec![],
    ));
    function("deep_chain", blocks)
}

#[test]
fn an_indirect_branch_rendered_only_into_a_helper_body_is_counted() {
    let ir = deep_chain(IROp::IndirectBranch, "br x16", "x16");
    let artifact = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
    // The helper path is proved by the omission event that sent the tail there,
    // not by a `_block_` spelling: a helper with one call site is inlined and
    // its definition dropped, so the finished text carries no helper name.
    assert!(
        artifact
            .emission
            .event_count(TraversalEventKind::DfsDepthOmission)
            > 0,
        "fixture must reach the helper path:\n{}",
        artifact.source
    );
    assert!(
        artifact.unresolved_cf > 0,
        "a helper-rendered `br` is unresolved control flow in the artifact:\n{}",
        artifact.source
    );
    assert_counter_is_the_body(&artifact, unresolved_statements(&artifact));
}

/// Two paths into the same over-budget tail, so the helper body is copied to
/// both call sites. Every copy is a statement in the artifact, so every copy
/// counts: counting the body once would report fewer than the artifact carries.
#[test]
fn every_inlined_copy_of_a_helper_body_is_counted() {
    let mut ir = deep_chain(IROp::IndirectBranch, "br x16", "x16");
    let tail = ir.blocks.len() - 1;
    let tail_va = ir.blocks[tail].start_va;
    // The entry block branches to the tail as well, so the tail is reached by
    // two paths and its helper is called twice.
    let entry_va = ir.blocks[0].start_va;
    ir.blocks[0].instrs = vec![
        instr(
            entry_va,
            IROp::Branch,
            &format!("cbz x0, #{tail_va:#x}"),
            &format!("#{tail_va:#x}"),
        ),
        instr(
            entry_va + 4,
            IROp::Jump,
            &format!("b #{:#x}", entry_va + 0x10),
            &format!("#{:#x}", entry_va + 0x10),
        ),
    ];
    ir.blocks[0].succs = vec![tail, 1];
    let artifact = emit_pseudocode_direct_dfs(&ir, &HashMap::new());
    let statements = unresolved_statements(&artifact);
    assert!(
        statements >= 2,
        "both paths must render the effect:\n{}",
        artifact.source
    );
    assert_counter_is_the_body(&artifact, statements);
}

/// The other two statements are counted in the same scope, and by the structured
/// walk as well as by the fallback: an unresolvable jump target is unresolved
/// control flow whichever walk rendered it.
#[test]
fn an_unresolvable_jump_target_is_counted_by_both_walks() {
    let ir = function(
        "unresolvable_jump",
        vec![blk(
            0,
            0x2000,
            vec![instr(0x2000, IROp::Jump, "b x9", "x9")],
            vec![],
        )],
    );
    for artifact in [
        emit_pseudocode(&ir, &HashMap::new()),
        emit_pseudocode_direct_dfs(&ir, &HashMap::new()),
    ] {
        assert!(
            artifact.source.contains("// unresolved jump"),
            "an unrecovered jump target must be stated:\n{}",
            artifact.source
        );
        assert_counter_is_the_body(&artifact, 1);
    }
}

/// A marker that carries an inline annotation keeps its prefix, so a counter
/// keyed on the whole line would lose it. The pinned baseline has exactly one
/// such line, which is why the census that matched the whole marker reported 520
/// where 521 statements were emitted.
#[test]
fn an_annotated_marker_keeps_its_prefix() {
    let ir = function(
        "annotated",
        vec![blk(
            0,
            0x3000,
            vec![instr(0x3000, IROp::IndirectBranch, "br x2", "x2")],
            vec![],
        )],
    );
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let marker = artifact
        .source
        .lines()
        .find(|line| line.trim_start().starts_with("// indirect branch"))
        .expect("the effect must be stated")
        .to_string();
    assert!(
        marker.contains("target not recovered"),
        "unexpected marker shape: {marker}"
    );
    let annotated = artifact.source.replace(
        &marker,
        &marker.replace(
            ": target not recovered",
            " /* = slot0.f8 */: target not recovered",
        ),
    );
    let statements = annotated
        .lines()
        .filter(|line| line.trim_start().starts_with("// indirect branch"))
        .count();
    assert_eq!(
        statements, 1,
        "an annotation between the register and the tail must not hide the statement:\n{annotated}"
    );
    assert_counter_is_the_body(&artifact, 1);
}

/// The other construction site: a function whose CFG did not validate reports one
/// unresolved site, and its body is the one diagnostic line. Neither walk runs
/// there, so the two paths agree only if the same statement shapes are counted.
#[test]
fn an_invalid_cfg_reports_one_site_and_carries_one_statement() {
    let mut ir = function(
        "invalid_cfg",
        vec![
            blk(
                0,
                0x4000,
                vec![instr(0x4000, IROp::Other, "mov x0, x1", "")],
                vec![1],
            ),
            blk(
                1,
                0x4004,
                vec![instr(0x4004, IROp::Other, "ret", "")],
                vec![],
            ),
        ],
    );
    // Two blocks claiming the same id: an id-keyed map keeps one of them, so the
    // graph cannot be walked and no body may be invented from it.
    ir.blocks[1].id = 0;
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains("// invalid CFG"),
        "the defect must be named in the body:\n{}",
        artifact.source
    );
    assert_counter_is_the_body(&artifact, 1);
}
