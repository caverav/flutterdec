#[test]
fn collapses_helper_calls_into_omitted_path_comments() {
    let ir = FunctionIr {
        function_id: 15,
        name: "helperCollapse".to_string(),
        entry_va: 0xf000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic helperCollapse(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_3();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_3() {".to_string(),
            "  final t1 = fn_0x3(arg0, arg1, arg2, arg3);".to_string(),
            "  return t1;".to_string(),
            "}".to_string(),
        ];

    emitter.collapse_remaining_helpers();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("_block_"),
        "helper scaffolding should be removed:\n{out}"
    );
    assert!(
        out.contains("omitted complex paths: block 3"),
        "function should include omitted-path summary:\n{out}"
    );
    assert!(
        out.contains("return null;"),
        "call should get a safe fallback return:\n{out}"
    );
}

#[test]
fn summarizes_duplicate_omitted_blocks_once() {
    let ir = FunctionIr {
        function_id: 16,
        name: "helperCollapseDedup".to_string(),
        entry_va: 0xf100,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic helperCollapseDedup(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return _block_9();".to_string(),
            "  }".to_string(),
            "  return _block_9();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_9() {".to_string(),
            "  return arg0;".to_string(),
            "}".to_string(),
        ];

    emitter.collapse_remaining_helpers();
    let out = emitter.lines.join("\n");
    assert_eq!(
        out.matches("omitted complex paths: block 9").count(),
        1,
        "duplicate omitted blocks should be summarized once:\n{out}"
    );
    assert_eq!(
        out.matches("return null;").count(),
        2,
        "each omitted callsite should become return null:\n{out}"
    );
}

#[test]
fn simplifies_null_base_add_immediate() {
    let ir = FunctionIr {
        function_id: 17,
        name: "nullAdd".to_string(),
        entry_va: 0xf200,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf200,
            instrs: vec![
                LlirInstr {
                    va: 0xf200,
                    op: IROp::Other,
                    src: "add x1, x22, #0x20".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf204,
                    op: IROp::Other,
                    src: "stur x1, [x29, #-8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf208,
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
        artifact.source.contains("= 0x20;"),
        "null-based add should collapse to literal:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("(null + 0x20)"),
        "legacy null arithmetic should be removed:\n{}",
        artifact.source
    );
}

#[test]
fn folds_nested_stack_offset_arithmetic() {
    let ir = FunctionIr {
        function_id: 18,
        name: "stackOffsetFold".to_string(),
        entry_va: 0xf300,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf300,
            instrs: vec![
                LlirInstr {
                    va: 0xf300,
                    op: IROp::Other,
                    src: "sub x1, x15, #0x20".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf304,
                    op: IROp::Other,
                    src: "add x2, x1, #0x10".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf308,
                    op: IROp::Other,
                    src: "stur x2, [x29, #-8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf30c,
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
        artifact.source.contains("(sp - 0x10)"),
        "stack-offset expression should fold:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("((sp - 0x20) + 0x10)"),
        "unfolded nested arithmetic should not remain:\n{}",
        artifact.source
    );
}

#[test]
fn does_not_inject_alternative_path_comment() {
    let ir = FunctionIr {
        function_id: 19,
        name: "noAlternativePath".to_string(),
        entry_va: 0xf400,
        blocks: vec![
            BasicBlock {
                id: 0,
                start_va: 0xf400,
                instrs: vec![LlirInstr {
                    va: 0xf400,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                }],
                succs: Vec::new(),
                preds: Vec::new(),
            },
            BasicBlock {
                id: 1,
                start_va: 0xf404,
                instrs: vec![LlirInstr {
                    va: 0xf404,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                }],
                succs: Vec::new(),
                preds: Vec::new(),
            },
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        !artifact.source.contains("alternative path"),
        "synthetic alternative-path comment should not be emitted:\n{}",
        artifact.source
    );
}

#[test]
fn renders_stack_access_as_indexed_slot() {
    let ir = FunctionIr {
        function_id: 20,
        name: "stackSlots".to_string(),
        entry_va: 0xf500,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf500,
            instrs: vec![
                LlirInstr {
                    va: 0xf500,
                    op: IROp::Other,
                    src: "ldr x1, [x15, #8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf504,
                    op: IROp::Other,
                    src: "str x1, [x15, #-8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf508,
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
        artifact.source.contains("sp[8]"),
        "positive stack slots should use index notation:\n{}",
        artifact.source
    );
    assert!(
        artifact.source.contains("sp[-8]"),
        "negative stack slots should use index notation:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("sp.f8") && !artifact.source.contains("sp.m8"),
        "legacy stack field notation should not remain:\n{}",
        artifact.source
    );
}

#[test]
fn collapses_stack_pointer_offset_base_into_indexed_slot() {
    let ir = FunctionIr {
        function_id: 21,
        name: "stackBaseCollapse".to_string(),
        entry_va: 0xf600,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf600,
            instrs: vec![
                LlirInstr {
                    va: 0xf600,
                    op: IROp::Other,
                    src: "sub x1, x15, #0x30".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf604,
                    op: IROp::Other,
                    src: "ldr x2, [x1]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf608,
                    op: IROp::Other,
                    src: "str x2, [x15, #8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf60c,
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
        artifact.source.contains("sp[-0x30]"),
        "derived stack base should collapse to indexed slot:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("((sp - 0x30)).f0"),
        "legacy synthetic field form should not remain:\n{}",
        artifact.source
    );
}
