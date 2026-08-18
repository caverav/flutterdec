//! The ARM64 control-effect table of section 3 of
//! `docs/oracle-protocol-ir-cfg-emitter.md`, and the block-construction
//! assertions that read it.
//!
//! Test-only, and protected by digest in section 7 of that protocol, so no row
//! of the table can be weakened or removed while every other digest still
//! matches. It is deliberately not part of `mod tests` in `lib.rs`: that module
//! is ordinary test space which later work edits, and a digest over a file later
//! work must edit fires on legitimate change and is worthless as a ruler.

use super::tests::ins;
use super::*;

/// One row of the ARM64 control-effect table in section 3 of
/// `docs/oracle-protocol-ir-cfg-emitter.md`, written from `DDI 0487` C6.2.
struct ControlEffect {
    mnemonic: &'static str,
    op_str: &'static str,
    op: IROp,
    /// Whether the instruction that follows must become a leader.
    ends_block: bool,
    /// Start addresses of the successors of the block this instruction
    /// ends, ascending. Empty means no edge at all, which is what makes an
    /// invented fallthrough or a guessed target visible.
    succ_starts: &'static [u64],
}

/// The whole table, so a new control effect cannot be added without a row.
/// `0x1008` is the fallthrough block and `0x2000` the branch target block in
/// the fixture below.
const CONTROL_EFFECTS: &[ControlEffect] = &[
    ControlEffect {
        mnemonic: "b",
        op_str: "#0x2000",
        op: IROp::Jump,
        ends_block: true,
        succ_starts: &[0x2000],
    },
    ControlEffect {
        mnemonic: "br",
        op_str: "x16",
        op: IROp::IndirectBranch,
        ends_block: true,
        succ_starts: &[],
    },
    ControlEffect {
        mnemonic: "ret",
        op_str: "",
        op: IROp::Return,
        ends_block: true,
        succ_starts: &[],
    },
    ControlEffect {
        mnemonic: "brk",
        op_str: "#0x1",
        op: IROp::Trap,
        ends_block: true,
        succ_starts: &[],
    },
    ControlEffect {
        mnemonic: "b.eq",
        op_str: "#0x2000",
        op: IROp::Branch,
        ends_block: true,
        succ_starts: &[0x1008, 0x2000],
    },
    ControlEffect {
        mnemonic: "cbz",
        op_str: "x0, #0x2000",
        op: IROp::Branch,
        ends_block: true,
        succ_starts: &[0x1008, 0x2000],
    },
    ControlEffect {
        mnemonic: "cbnz",
        op_str: "x0, #0x2000",
        op: IROp::Branch,
        ends_block: true,
        succ_starts: &[0x1008, 0x2000],
    },
    ControlEffect {
        mnemonic: "tbz",
        op_str: "x0, #0x3, #0x2000",
        op: IROp::Branch,
        ends_block: true,
        succ_starts: &[0x1008, 0x2000],
    },
    ControlEffect {
        mnemonic: "tbnz",
        op_str: "x0, #0x3, #0x2000",
        op: IROp::Branch,
        ends_block: true,
        succ_starts: &[0x1008, 0x2000],
    },
    // A call returns, so it keeps its fallthrough and takes no edge to the
    // callee even when the callee is a block of this same function.
    ControlEffect {
        mnemonic: "bl",
        op_str: "#0x2000",
        op: IROp::Call,
        ends_block: false,
        succ_starts: &[0x1008],
    },
    ControlEffect {
        mnemonic: "blr",
        op_str: "x16",
        op: IROp::Call,
        ends_block: false,
        succ_starts: &[0x1008],
    },
    // Control row: an ordinary instruction is the only thing that may take
    // the fallthrough by default.
    ControlEffect {
        mnemonic: "mov",
        op_str: "x0, x1",
        op: IROp::Other,
        ends_block: false,
        succ_starts: &[0x1008],
    },
];

/// The instruction under test at `0x1004`, with instructions following it,
/// reached from a block that also makes `0x1008` a leader. The successor set
/// of the middle block is then exactly the instruction's control effect.
fn control_effect_fixture(row: &ControlEffect) -> FunctionDisassembly {
    FunctionDisassembly {
        function_id: 42,
        function_name: "effect".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: 0x1010,
        instructions: vec![
            ins(0x1000, "cbz", "x9, #0x1008"),
            ins(0x1004, row.mnemonic, row.op_str),
            ins(0x1008, "mov", "x0, x1"),
            ins(0x100c, "b", "#0x2000"),
            ins(0x2000, "ret", ""),
        ],
    }
}

#[test]
fn every_arm64_control_effect_has_exactly_the_documented_edges() {
    for row in CONTROL_EFFECTS {
        let ir = build_function_ir(&control_effect_fixture(row));
        let start_of: BTreeMap<usize, u64> = ir.blocks.iter().map(|b| (b.id, b.start_va)).collect();
        let under_test = ir
            .blocks
            .iter()
            .find(|b| b.start_va == 0x1004)
            .unwrap_or_else(|| panic!("{} should start a block of its own", row.mnemonic));

        assert_eq!(
            under_test.instrs.first().map(|i| &i.op),
            Some(&row.op),
            "{} {} should classify as {:?}",
            row.mnemonic,
            row.op_str,
            row.op
        );

        let succs: Vec<u64> = under_test
            .succs
            .iter()
            .map(|s| start_of[s])
            .collect::<BTreeSet<u64>>()
            .into_iter()
            .collect();
        assert_eq!(
            succs,
            row.succ_starts.to_vec(),
            "{} {} has the wrong edges: {:?}",
            row.mnemonic,
            row.op_str,
            under_test.succs
        );

        // I1 through I4: nothing above may be read off a malformed graph.
        for b in &ir.blocks {
            let mut sorted = b.succs.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, b.succs, "successors must be sorted and unique");
            let mut sorted = b.preds.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, b.preds, "predecessors must be sorted and unique");
            for s in &b.succs {
                assert!(
                    ir.blocks[*s].preds.contains(&b.id),
                    "{} {}: edge {} -> {} is missing its predecessor",
                    row.mnemonic,
                    row.op_str,
                    b.id,
                    s
                );
            }
            for p in &b.preds {
                assert!(
                    ir.blocks[*p].succs.contains(&b.id),
                    "{} {}: predecessor {} of {} has no matching edge",
                    row.mnemonic,
                    row.op_str,
                    p,
                    b.id
                );
            }
        }
        for (i, b) in ir.blocks.iter().enumerate() {
            assert_eq!(b.id, i, "block ids are exactly 0..len");
        }
    }
}

#[test]
fn only_a_control_effect_that_ends_a_block_makes_the_next_instruction_a_leader() {
    for row in CONTROL_EFFECTS {
        let d = FunctionDisassembly {
            function_id: 43,
            function_name: "ender".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 12,
            instructions: vec![
                ins(0x1000, row.mnemonic, row.op_str),
                ins(0x1004, "mov", "x0, x1"),
                ins(0x1008, "ret", ""),
            ],
        };
        let ir = build_function_ir(&d);
        let expected_blocks = if row.ends_block { 2 } else { 1 };
        assert_eq!(
            ir.blocks.len(),
            expected_blocks,
            "{} {} should {} end its block: {:?}",
            row.mnemonic,
            row.op_str,
            if row.ends_block { "" } else { "not " },
            ir.blocks
                .iter()
                .map(|b| (b.start_va, b.instrs.len()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            ir.blocks[0].instrs.len(),
            if row.ends_block { 1 } else { 3 },
            "{} {} left the wrong instructions in the entry block",
            row.mnemonic,
            row.op_str
        );
    }
}

/// `br` is how a dispatch stub or a tail call leaves a function. The register
/// it leaves through is kept as provenance, and no edge is derived from it in
/// either direction: not a fallthrough, and not a guessed target.
#[test]
fn an_indirect_branch_keeps_its_register_and_takes_no_edge() {
    let d = FunctionDisassembly {
        function_id: 44,
        function_name: "dispatch".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: 12,
        instructions: vec![
            ins(0x1000, "ldur", "x16, [x24, #7]"),
            ins(0x1004, "br", "x16"),
            // Only reachable because something else branches here, which
            // nothing in this function does.
            ins(0x1008, "ret", ""),
        ],
    };
    let ir = build_function_ir(&d);
    let tail = ir.blocks[0].instrs.last().expect("a tail instruction");
    assert_eq!(tail.op, IROp::IndirectBranch);
    assert_eq!(tail.target, "x16", "the register is kept as provenance");
    assert!(
        ir.blocks[0].succs.is_empty(),
        "an indirect branch invents no edge: {:?}",
        ir.blocks[0].succs
    );
    assert!(
        ir.blocks[1].preds.is_empty(),
        "the instruction after it is not fallen into: {:?}",
        ir.blocks[1].preds
    );
}

/// `brk` raises; control does not continue past it. Dart AOT ends every
/// raising stub with it, so a fabricated fallthrough there attaches the next
/// stub's body to a path that never reaches it.
#[test]
fn a_trap_ends_the_block_with_no_successors() {
    let d = FunctionDisassembly {
        function_id: 45,
        function_name: "raise".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: 12,
        instructions: vec![
            ins(0x1000, "brk", "#0x1"),
            ins(0x1004, "mov", "x0, x1"),
            ins(0x1008, "ret", ""),
        ],
    };
    let ir = build_function_ir(&d);
    assert_eq!(ir.blocks[0].instrs[0].op, IROp::Trap);
    assert!(
        ir.blocks[0].succs.is_empty(),
        "a trap has no successors: {:?}",
        ir.blocks[0].succs
    );
    assert!(
        ir.blocks[1].preds.is_empty(),
        "the code after a trap is not fallen into: {:?}",
        ir.blocks[1].preds
    );
}
