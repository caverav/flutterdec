//! Public CFG behavior for the accepted direct-target operand grammar.

use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly};
use flutterdec_ir::build_function_ir;

fn ins(va: u64, mnemonic: &str, operands: &str) -> AsmInstruction {
    AsmInstruction {
        va,
        word: 0,
        mnemonic: mnemonic.to_string(),
        op_str: operands.to_string(),
        annotation: String::new(),
    }
}

fn branch_fixture(mnemonic: &str, operands: &str, target: u64) -> FunctionDisassembly {
    FunctionDisassembly {
        function_id: 1,
        function_name: "targetRadix".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: target.saturating_sub(0x1000).saturating_add(4),
        instructions: vec![
            ins(0x1000, mnemonic, operands),
            ins(0x1004, "ret", ""),
            ins(target, "ret", ""),
        ],
    }
}

fn successor_starts(disasm: &FunctionDisassembly) -> Vec<u64> {
    let ir = build_function_ir(disasm);
    let entry = ir
        .blocks
        .iter()
        .find(|block| block.start_va == disasm.entry_va)
        .expect("entry block");
    entry
        .succs
        .iter()
        .map(|successor| ir.blocks[*successor].start_va)
        .collect()
}

#[test]
fn public_cfg_distinguishes_decimal_and_explicit_hex_targets() {
    for (spelling, target) in [
        ("8192", 8192),
        ("0008192", 8192),
        ("1000000", 1_000_000),
        ("0x2000", 0x2000),
        ("0X2000", 0x2000),
        ("#0x2000", 0x2000),
        ("#0X2000", 0x2000),
        ("10000a", 0x10000a),
        ("10000A", 0x10000a),
    ] {
        assert_eq!(
            successor_starts(&branch_fixture("b", spelling, target)),
            vec![target],
            "wrong public CFG target for {spelling}"
        );
    }
}

#[test]
fn public_cfg_accepts_target_boundaries_without_radix_guessing() {
    let high = u64::MAX - 1;
    for spelling in [
        high.to_string(),
        format!("#0x{high:x}"),
        format!("{high:x}"),
    ] {
        assert_eq!(
            successor_starts(&branch_fixture("b", &spelling, high)),
            vec![high],
            "wrong public CFG target at the u64 boundary for {spelling}"
        );
    }

    let zero = FunctionDisassembly {
        function_id: 2,
        function_name: "zeroTarget".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0,
        size: 8,
        instructions: vec![ins(0, "b", "0"), ins(4, "ret", "")],
    };
    assert_eq!(successor_starts(&zero), vec![0]);
}

#[test]
fn conditional_targets_keep_their_taken_and_fallthrough_edges() {
    for (mnemonic, operands, target) in [
        ("cbz", "x0, 1000000", 1_000_000),
        ("tbnz", "x0, #0x3f, 10000a", 0x10000a),
    ] {
        assert_eq!(
            successor_starts(&branch_fixture(mnemonic, operands, target)),
            vec![0x1004, target],
            "{mnemonic} lost its taken or fallthrough edge"
        );
    }
}

#[test]
fn direct_calls_keep_fallthrough_and_never_add_a_callee_edge() {
    for target in ["1000000", "10000a", "#0X10000A", "not-an-address"] {
        let disasm = branch_fixture("bl", target, 1_000_000);
        let ir = build_function_ir(&disasm);
        let entry = &ir.blocks[0];
        assert_eq!(
            entry
                .instrs
                .iter()
                .map(|instruction| instruction.va)
                .collect::<Vec<_>>(),
            vec![0x1000, 0x1004],
            "call {target} must fall through inside its block"
        );
        assert!(
            entry.succs.is_empty(),
            "call {target} invented a callee edge"
        );
    }
}

#[test]
fn malformed_or_ambiguous_targets_remain_unknown() {
    for target in [
        "",
        "#",
        "0x",
        "0X",
        "##0x2000",
        "0x2000g",
        "10000g",
        "0x2000 0x3000",
        "#0x2000, #0x3000",
        "18446744073709551616",
        "10000000000000000f",
    ] {
        assert!(
            successor_starts(&branch_fixture("b", target, 0x2000)).is_empty(),
            "malformed or ambiguous target {target:?} invented an edge"
        );
    }
}
