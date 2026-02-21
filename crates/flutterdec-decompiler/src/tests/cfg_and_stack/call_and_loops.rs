#[test]
fn emits_dynamic_call_for_indirect_targets() {
    let ir = FunctionIr {
        function_id: 12,
        name: "indirectCall".to_string(),
        entry_va: 0xc000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc000,
            instrs: vec![
                LlirInstr {
                    va: 0xc000,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc004,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                },
            ],
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    };
    let symbols = HashMap::new();
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains("dynamicCall(indirectTarget9"),
        "indirect calls should use named targets:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("invoke(reg9"),
        "legacy invoke label should be absent:\n{}",
        artifact.source
    );
}

#[test]
fn names_direct_call_target_when_symbol_is_known() {
    let ir = FunctionIr {
        function_id: 44,
        name: "namedCall".to_string(),
        entry_va: 0x4010,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x4010,
            instrs: vec![
                LlirInstr {
                    va: 0x4010,
                    op: IROp::Call,
                    src: "bl #0x5000".to_string(),
                    target: "#0x5000".to_string(),
                },
                LlirInstr {
                    va: 0x4014,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                },
            ],
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    };

    let mut symbols = HashMap::new();
    symbols.insert(0x5000, "Flutter_Stdlib_Helper".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains("Flutter_Stdlib_Helper("),
        "direct call should use resolved symbol name:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("fn_0x5000("),
        "fallback raw address name should not appear when symbol is known:\n{}",
        artifact.source
    );
}

#[test]
fn structures_simple_backedge_as_while_loop() {
    let ir = FunctionIr {
        function_id: 13,
        name: "simpleLoop".to_string(),
        entry_va: 0xd000,
        blocks: vec![
            BasicBlock {
                id: 0,
                start_va: 0xd000,
                instrs: vec![LlirInstr {
                    va: 0xd000,
                    op: IROp::Jump,
                    src: "b #0xd004".to_string(),
                    target: "#0xd004".to_string(),
                }],
                succs: vec![1],
                preds: Vec::new(),
            },
            BasicBlock {
                id: 1,
                start_va: 0xd004,
                instrs: vec![
                    LlirInstr {
                        va: 0xd004,
                        op: IROp::Other,
                        src: "add x0, x0, #1".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xd008,
                        op: IROp::Branch,
                        src: "cbnz x0, #0xd010".to_string(),
                        target: "#0xd010".to_string(),
                    },
                ],
                succs: vec![2, 3],
                preds: vec![0, 2],
            },
            BasicBlock {
                id: 2,
                start_va: 0xd00c,
                instrs: vec![LlirInstr {
                    va: 0xd00c,
                    op: IROp::Jump,
                    src: "b #0xd004".to_string(),
                    target: "#0xd004".to_string(),
                }],
                succs: vec![1],
                preds: vec![1],
            },
            BasicBlock {
                id: 3,
                start_va: 0xd010,
                instrs: vec![LlirInstr {
                    va: 0xd010,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                }],
                succs: Vec::new(),
                preds: vec![1],
            },
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains("while (true) {"),
        "simple backedge should become loop:\n{}",
        artifact.source
    );
    assert!(
        artifact.source.contains("continue;"),
        "loop backedge inside loop should use continue:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("loop back-edges:"),
        "loop summary should not be needed for simple structured loop:\n{}",
        artifact.source
    );
}

#[test]
fn normalizes_zero_register_operands() {
    let ir = FunctionIr {
        function_id: 14,
        name: "zeroRegs".to_string(),
        entry_va: 0xe000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xe000,
            instrs: vec![
                LlirInstr {
                    va: 0xe000,
                    op: IROp::Other,
                    src: "mov w1, wzr".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xe004,
                    op: IROp::Other,
                    src: "stur w1, [x29, #-8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xe008,
                    op: IROp::Other,
                    src: "mov x2, xzr".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xe00c,
                    op: IROp::Other,
                    src: "stur x2, [x29, #-16]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xe010,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                },
            ],
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains("= 0;"),
        "zero register should normalize to 0:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("xzr") && !artifact.source.contains("wzr"),
        "raw zero registers should not leak:\n{}",
        artifact.source
    );
}
