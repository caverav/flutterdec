//! Both emitters must render an indirect branch and a trap as the unknown
//! control effects they are, and must render them the same way.
//!
//! The structured emitter and the DFS fallback are separate walks over the same
//! IR, so a control effect handled in one and not the other is a difference in
//! what the artifact claims the program does, depending only on whether
//! `Regions::build` accepted the graph. These tests pin both walks against the
//! same expectations: the reducible fixture proves the structured path (a
//! structured loop needs no back-edge summary), the irreducible one proves the
//! fallback (`Regions::build` declines two entries into one cycle).

use flutterdec_decompiler::emit_pseudocode;
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

/// `x16` reaches the artifact as `reg16`: the emitter renames registers for
/// readability, and the note goes through the same rename, so the value control
/// left through stays visible in the emitted text.
const INDIRECT_NOTE: &str = "// indirect branch through reg16: target not recovered";
const TRAP_NOTE: &str = "// trap: control does not continue";

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

fn cbz(va: u64, reg: &str, target_va: u64) -> LlirInstr {
    instr(
        va,
        IROp::Branch,
        &format!("cbz {reg}, #{target_va:#x}"),
        &format!("#{target_va:#x}"),
    )
}

fn indirect_branch(va: u64) -> LlirInstr {
    instr(va, IROp::IndirectBranch, "br x16", "x16")
}

fn trap(va: u64) -> LlirInstr {
    instr(va, IROp::Trap, "brk #0x1", "")
}

/// What neither emitter may claim about `br` or `brk`: a tail call to an address
/// nobody recovered, or a source-level return.
///
/// Neither fixture holds a single `IROp::Return`, so the only legitimate source
/// of a `return` is the collapse of an omitted path, which names the blocks it
/// dropped in its own summary line. Counting the two against each other says
/// exactly what matters: no `return` came from a control effect.
fn assert_no_fabricated_control(source: &str) {
    for fabricated in ["tailCall_", "goto"] {
        assert!(
            !source.contains(fabricated),
            "an unknown control effect must not render as `{fabricated}`:\n{source}"
        );
    }
    let collapsed: usize = source
        .lines()
        .filter(|line| line.trim_start().starts_with("// omitted complex paths:"))
        .map(|line| line.matches("block ").count())
        .sum();
    assert_eq!(
        source.matches("return ").count(),
        collapsed,
        "a return here can only come from a collapsed omitted path:\n{source}"
    );
}

fn assert_reports_both_effects(source: &str) {
    assert!(
        source.contains(INDIRECT_NOTE),
        "the indirect branch must be reported:\n{source}"
    );
    assert!(
        source.contains(TRAP_NOTE),
        "the trap must be reported:\n{source}"
    );
}

/// Reducible: a natural loop over blocks 0 and 1, then an exit branch to an
/// indirect branch and to a trap. `Regions::build` accepts it, so this is the
/// structured walk.
fn reducible_ir() -> FunctionIr {
    FunctionIr {
        function_id: 2001,
        name: "structuredEffects".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x0", 0x100c)], vec![1, 2]),
            blk(
                1,
                0x1004,
                vec![
                    instr(0x1004, IROp::Other, "mov x2, x3", ""),
                    instr(0x1008, IROp::Jump, "b #0x1000", "#0x1000"),
                ],
                vec![0],
            ),
            blk(2, 0x100c, vec![cbz(0x100c, "x1", 0x1018)], vec![3, 4]),
            blk(
                3,
                0x1010,
                vec![
                    instr(0x1010, IROp::Other, "ldur x16, [x24, #7]", ""),
                    indirect_branch(0x1014),
                ],
                Vec::new(),
            ),
            blk(4, 0x1018, vec![trap(0x1018)], Vec::new()),
        ],
    }
}

/// Irreducible: two entries into the 1 <-> 2 cycle, so `Regions::build` returns
/// `None` and emission must take the DFS fallback.
fn irreducible_ir() -> FunctionIr {
    FunctionIr {
        function_id: 2002,
        name: "fallbackEffects".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            blk(1, 0x1004, vec![cbz(0x1004, "x2", 0x3000)], vec![2, 3]),
            blk(2, 0x2000, vec![cbz(0x2000, "x3", 0x1004)], vec![1, 3]),
            blk(3, 0x3000, vec![cbz(0x3000, "x4", 0x4000)], vec![4, 5]),
            blk(
                4,
                0x3004,
                vec![
                    instr(0x3004, IROp::Other, "ldur x16, [x24, #7]", ""),
                    indirect_branch(0x3008),
                ],
                Vec::new(),
            ),
            blk(5, 0x4000, vec![trap(0x4000)], Vec::new()),
        ],
    }
}

#[test]
fn the_structured_emitter_reports_an_indirect_branch_and_a_trap() {
    let ir = reducible_ir();
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let source = &artifact.source;

    assert!(
        source.contains("while (true) {") && !source.contains("// loop back-edges:"),
        "this fixture must be structured, not handed to the fallback:\n{source}"
    );
    assert_reports_both_effects(source);
    assert_no_fabricated_control(source);
    // The structured walk emits every block once, so the counts are exact here.
    assert_eq!(source.matches(INDIRECT_NOTE).count(), 1, "{source}");
    assert_eq!(source.matches(TRAP_NOTE).count(), 1, "{source}");
    assert_eq!(
        artifact.unresolved_cf, 1,
        "the indirect branch is the one unresolved control effect:\n{source}"
    );
}

#[test]
fn the_fallback_emitter_reports_an_indirect_branch_and_a_trap() {
    let ir = irreducible_ir();
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let source = &artifact.source;

    assert!(
        !source.contains("while (true) {"),
        "this fixture must reach the fallback, not the structurer:\n{source}"
    );
    assert_reports_both_effects(source);
    assert_no_fabricated_control(source);
    // The fallback duplicates a block per path that reaches it, so one indirect
    // branch is counted once per emitted copy rather than once per instruction.
    assert_eq!(
        artifact.unresolved_cf,
        source.matches(INDIRECT_NOTE).count(),
        "every emitted indirect branch is counted as unresolved:\n{source}"
    );
    assert!(
        artifact.unresolved_cf > 0,
        "the fallback must count the indirect branch:\n{source}"
    );
}

/// The two walks must not disagree about what an unknown control effect means.
#[test]
fn both_emitters_render_the_same_control_effects() {
    let structured = emit_pseudocode(&reducible_ir(), &HashMap::new()).source;
    let fallback = emit_pseudocode(&irreducible_ir(), &HashMap::new()).source;

    for source in [&structured, &fallback] {
        assert_reports_both_effects(source);
        assert_no_fabricated_control(source);
    }
}
