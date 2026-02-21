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
fn rewrites_indirect_call_to_semantic_name_when_selector_is_known() {
    let ir = FunctionIr {
        function_id: 46,
        name: "indirectSemanticCall".to_string(),
        entry_va: 0xc200,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc200,
            instrs: vec![
                LlirInstr {
                    va: 0xc200,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc204,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[42]".to_string(),
                },
                LlirInstr {
                    va: 0xc208,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc20c,
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
    let mut pool = HashMap::new();
    pool.insert(42u64, "setState".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.widgets.State.setState(1, \"setState\" /* pool[42] */, param2, param3); // framework:flutter.widgets.State.setState [selector], indirect via: indirectTarget9"
        ),
        "indirect selector call should be rewritten to semantic direct form:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dynamicCall(indirectTarget9"),
        "rewritten semantic indirect call should not keep dynamicCall form:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_pool_mapping_in_indirect_target_comment() {
    let ir = FunctionIr {
        function_id: 47,
        name: "indirectTargetPoolHint".to_string(),
        entry_va: 0xc300,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc300,
            instrs: vec![
                LlirInstr {
                    va: 0xc300,
                    op: IROp::LoadPool,
                    src: "x9".to_string(),
                    target: "pool[40]".to_string(),
                },
                LlirInstr {
                    va: 0xc304,
                    op: IROp::Other,
                    src: "ldr x9, [x9, #7]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc308,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc30c,
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
    let mut pool = HashMap::new();
    pool.insert(40u64, "_offsetInBytes".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact
            .source
            .contains("dispatch.offsetInBytes(receiver, param1, param2, param3); // selector: offsetInBytes, indirect via: indirectTarget9, target: (pool[40 /* \"_offsetInBytes\" */]).f7"),
        "pool mapping should drive dispatch selector rewrite:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_dispatch_selector_when_nonstandard() {
    let ir = FunctionIr {
        function_id: 48,
        name: "indirectDispatchSelector".to_string(),
        entry_va: 0xc400,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc400,
            instrs: vec![
                LlirInstr {
                    va: 0xc400,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[12]".to_string(),
                },
                LlirInstr {
                    va: 0xc404,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc408,
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
    let mut pool = HashMap::new();
    pool.insert(12u64, "customAction@12345".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dispatch.customAction(receiver, \"customAction@12345\" /* pool[12] */, param2, param3); // selector: customAction, indirect via: indirectTarget9"
        ),
        "nonstandard selector should still rewrite indirect call into dispatch.<selector> form:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dynamicCall(indirectTarget9"),
        "dispatch selector rewrite should avoid dynamicCall fallback:\n{}",
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
