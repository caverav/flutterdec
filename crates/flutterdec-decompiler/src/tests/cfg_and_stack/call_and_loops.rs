#[test]
fn emits_callable_style_for_generic_indirect_targets() {
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
        artifact
            .source
            .contains("indirectTarget9(receiver, param1, param2, param3); // indirect via: indirectTarget9"),
        "generic indirect calls should render callable style:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dynamicCall(indirectTarget9"),
        "generic indirect calls should avoid dynamicCall text:\n{}",
        artifact.source
    );
}

#[test]
fn propagates_resolved_generic_direct_symbol_names_across_program() {
    let ir = vec![
        FunctionIr {
            function_id: 501,
            name: "producer".to_string(),
            entry_va: 0x1500,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0x1500,
                instrs: vec![
                    LlirInstr {
                        va: 0x1500,
                        op: IROp::LoadPool,
                        src: "x1".to_string(),
                        target: "pool[64]".to_string(),
                    },
                    LlirInstr {
                        va: 0x1504,
                        op: IROp::Call,
                        src: "bl #0x9000".to_string(),
                        target: "0x9000".to_string(),
                    },
                    LlirInstr {
                        va: 0x1508,
                        op: IROp::LoadPool,
                        src: "x1".to_string(),
                        target: "pool[64]".to_string(),
                    },
                    LlirInstr {
                        va: 0x150c,
                        op: IROp::Call,
                        src: "bl #0x9000".to_string(),
                        target: "0x9000".to_string(),
                    },
                    LlirInstr {
                        va: 0x1510,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        },
        FunctionIr {
            function_id: 502,
            name: "consumer".to_string(),
            entry_va: 0x1600,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0x1600,
                instrs: vec![
                    LlirInstr {
                        va: 0x1600,
                        op: IROp::Call,
                        src: "bl #0x9000".to_string(),
                        target: "0x9000".to_string(),
                    },
                    LlirInstr {
                        va: 0x1604,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        },
    ];
    let mut symbols = HashMap::new();
    symbols.insert(0x9000u64, "sub_9000".to_string());
    let mut pool = HashMap::new();
    pool.insert(64u64, "_CompileTimeError".to_string());
    let artifacts = emit_program_with_pool_hints(&ir, &symbols, &pool);
    let producer = artifacts
        .iter()
        .find(|a| a.function_id == 501)
        .expect("producer artifact");
    let consumer = artifacts
        .iter()
        .find(|a| a.function_id == 502)
        .expect("consumer artifact");
    assert!(
        producer
            .source
            .contains("dart.core._CompileTimeError.new("),
        "producer should resolve generic call from selector evidence:\n{}",
        producer.source
    );
    assert!(
        consumer
            .source
            .contains("dart.core._CompileTimeError.new("),
        "consumer should reuse resolved generic symbol alias from program context:\n{}",
        consumer.source
    );
    assert!(
        consumer
            .source
            .contains("inferred from: sub_9000"),
        "program-level rewrite should preserve traceability:\n{}",
        consumer.source
    );
}

#[test]
fn does_not_propagate_generic_alias_when_evidence_is_sparse() {
    let ir = vec![
        FunctionIr {
            function_id: 601,
            name: "producerSparse".to_string(),
            entry_va: 0x1a00,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0x1a00,
                instrs: vec![
                    LlirInstr {
                        va: 0x1a00,
                        op: IROp::LoadPool,
                        src: "x1".to_string(),
                        target: "pool[64]".to_string(),
                    },
                    LlirInstr {
                        va: 0x1a04,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1a08,
                        op: IROp::LoadPool,
                        src: "x1".to_string(),
                        target: "pool[64]".to_string(),
                    },
                    LlirInstr {
                        va: 0x1a0c,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1a10,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        },
        FunctionIr {
            function_id: 602,
            name: "consumerSparse".to_string(),
            entry_va: 0x1b00,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0x1b00,
                instrs: vec![
                    LlirInstr {
                        va: 0x1b00,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1b04,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1b08,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1b0c,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1b10,
                        op: IROp::Call,
                        src: "bl #0x9003".to_string(),
                        target: "0x9003".to_string(),
                    },
                    LlirInstr {
                        va: 0x1b14,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        },
    ];
    let mut symbols = HashMap::new();
    symbols.insert(0x9003u64, "sub_9003".to_string());
    let mut pool = HashMap::new();
    pool.insert(64u64, "_CompileTimeError".to_string());
    let artifacts = emit_program_with_pool_hints(&ir, &symbols, &pool);
    let producer = artifacts
        .iter()
        .find(|a| a.function_id == 601)
        .expect("producerSparse artifact");
    let consumer = artifacts
        .iter()
        .find(|a| a.function_id == 602)
        .expect("consumerSparse artifact");
    assert!(
        producer
            .source
            .contains("dart.core._CompileTimeError.new("),
        "producer with selector evidence should still rewrite:\n{}",
        producer.source
    );
    assert!(
        consumer.source.contains("sub_9003("),
        "consumer should keep generic call when alias evidence is sparse:\n{}",
        consumer.source
    );
    assert!(
        !consumer.source.contains("inferred from: sub_9003"),
        "sparse alias evidence should not trigger program-level inferred rewrite:\n{}",
        consumer.source
    );
}

#[test]
fn ignores_long_sentence_like_owner_markers() {
    let ir = FunctionIr {
        function_id: 503,
        name: "ownerSentenceFilter".to_string(),
        entry_va: 0x1700,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x1700,
            instrs: vec![
                LlirInstr {
                    va: 0x1700,
                    op: IROp::LoadPool,
                    src: "x3".to_string(),
                    target: "pool[2488]".to_string(),
                },
                LlirInstr {
                    va: 0x1704,
                    op: IROp::Call,
                    src: "bl #0x9001".to_string(),
                    target: "0x9001".to_string(),
                },
                LlirInstr {
                    va: 0x1708,
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
    symbols.insert(0x9001u64, "sub_9001".to_string());
    let mut pool = HashMap::new();
    pool.insert(
        2488u64,
        "To resolve this error, maintain at least one listener on the stream, or create an ImageStreamCompleterHandle from the keepAlive method, or create a new stream for the image.".to_string(),
    );
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains("sub_9001("),
        "sentence-like marker should not become synthetic owner.invoke path:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains(".invoke("),
        "sentence-like marker should not produce owner invoke rewrite:\n{}",
        artifact.source
    );
}

#[test]
fn emits_callable_style_for_noncanonical_indirect_targets() {
    let ir = FunctionIr {
        function_id: 121,
        name: "indirectCallNoncanonical".to_string(),
        entry_va: 0xc020,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc020,
            instrs: vec![
                LlirInstr {
                    va: 0xc020,
                    op: IROp::Call,
                    src: "blr xzr".to_string(),
                    target: "xzr".to_string(),
                },
                LlirInstr {
                    va: 0xc024,
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
        artifact
            .source
            .contains("xzr(receiver, param1, param2, param3); // indirect via: xzr"),
        "noncanonical indirect targets should still prefer callable fallback style:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dynamicCall(xzr"),
        "noncanonical indirect targets should avoid dynamicCall when callable fallback is safe:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_dispatch_target_fallback_to_dispatch_invoke() {
    let ir = FunctionIr {
        function_id: 45,
        name: "dispatchInvokeFallback".to_string(),
        entry_va: 0xc100,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc100,
            instrs: vec![
                LlirInstr {
                    va: 0xc100,
                    op: IROp::Call,
                    src: "blr x30".to_string(),
                    target: "x30".to_string(),
                },
                LlirInstr {
                    va: 0xc104,
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
        artifact
            .source
            .contains("dispatch.invoke(receiver, param1, param2, param3); // indirect via: dispatchTarget"),
        "dispatchTarget fallback should use dispatch.invoke form:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dynamicCall(dispatchTarget"),
        "dispatchTarget fallback should avoid dynamicCall form:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_dispatch_target_fallback_to_resolved_target_invoke() {
    let ir = FunctionIr {
        function_id: 453,
        name: "dispatchResolvedTargetInvokeFallback".to_string(),
        entry_va: 0xc108,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc108,
            instrs: vec![
                LlirInstr {
                    va: 0xc108,
                    op: IROp::Other,
                    src: "mov x30, x1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc10c,
                    op: IROp::Call,
                    src: "blr x30".to_string(),
                    target: "x30".to_string(),
                },
                LlirInstr {
                    va: 0xc110,
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
        artifact
            .source
            .contains("param1(receiver, param1, param2, param3); // indirect via: dispatchTarget"),
        "resolved dispatch target should render as target(...) fallback:\n{}",
        artifact.source
    );
    assert!(
        !artifact
            .source
            .contains("dispatch.invoke(receiver, param1, param2, param3)"),
        "resolved dispatch target should avoid plain dispatch.invoke fallback:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_dispatch_target_fallback_to_library_invoke_when_uri_is_known() {
    let ir = FunctionIr {
        function_id: 451,
        name: "dispatchLibraryInvokeFallback".to_string(),
        entry_va: 0xc110,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc110,
            instrs: vec![
                LlirInstr {
                    va: 0xc110,
                    op: IROp::LoadPool,
                    src: "x2".to_string(),
                    target: "pool[44]".to_string(),
                },
                LlirInstr {
                    va: 0xc114,
                    op: IROp::Call,
                    src: "blr x30".to_string(),
                    target: "x30".to_string(),
                },
                LlirInstr {
                    va: 0xc118,
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
    pool.insert(44u64, "package:flutter/src/widgets/heroes.dart".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.widgets.invoke(receiver, param1, \"package:flutter/src/widgets/heroes.dart\" /* pool[44] */, param3); // framework:flutter.widgets.invoke [library], indirect via: dispatchTarget"
        ),
        "known library URI should rewrite dispatch fallback to semantic library invoke:\n{}",
        artifact.source
    );
    assert!(
        !artifact
            .source
            .contains("dispatch.invoke(receiver, param1, \"package:flutter/src/widgets/heroes.dart\""),
        "library-aware fallback should avoid plain dispatch.invoke:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_generic_direct_call_to_owner_invoke_when_library_and_owner_markers_exist() {
    let ir = FunctionIr {
        function_id: 455,
        name: "directOwnerInvokeRewrite".to_string(),
        entry_va: 0xc113,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc113,
            instrs: vec![
                LlirInstr {
                    va: 0xc113,
                    op: IROp::LoadPool,
                    src: "x2".to_string(),
                    target: "pool[44]".to_string(),
                },
                LlirInstr {
                    va: 0xc117,
                    op: IROp::LoadPool,
                    src: "x3".to_string(),
                    target: "pool[45]".to_string(),
                },
                LlirInstr {
                    va: 0xc11b,
                    op: IROp::Call,
                    src: "bl #0x6141f8".to_string(),
                    target: "0x6141f8".to_string(),
                },
                LlirInstr {
                    va: 0xc11f,
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
    symbols.insert(0x6141f8u64, "sub_6141f8".to_string());
    let mut pool = HashMap::new();
    pool.insert(44u64, "package:flutter/src/widgets/heroes.dart".to_string());
    pool.insert(45u64, "RenderErrorBox.".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.widgets.RenderErrorBox.invoke(receiver, param1, \"package:flutter/src/widgets/heroes.dart\" /* pool[44] */, \"RenderErrorBox.\" /* pool[45] */); // framework:flutter.widgets.RenderErrorBox.invoke, was: sub_6141f8"
        ),
        "generic direct calls with owner/library markers should rewrite to semantic owner invoke:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("sub_6141f8("),
        "rewritten semantic owner call should not keep generic sub_* call name:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_generic_direct_call_to_library_invoke_when_only_library_marker_exists() {
    let ir = FunctionIr {
        function_id: 456,
        name: "directLibraryInvokeRewrite".to_string(),
        entry_va: 0xc130,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc130,
            instrs: vec![
                LlirInstr {
                    va: 0xc130,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[4656]".to_string(),
                },
                LlirInstr {
                    va: 0xc134,
                    op: IROp::Call,
                    src: "bl #0x9002".to_string(),
                    target: "0x9002".to_string(),
                },
                LlirInstr {
                    va: 0xc138,
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
    symbols.insert(0x9002u64, "sub_9002".to_string());
    let mut pool = HashMap::new();
    pool.insert(4656u64, "dart:core-patch/bool_patch.dart".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core_patch.bool_patch.invoke(receiver, \"dart:core-patch/bool_patch.dart\" /* pool[4656] */, param2, param3); // stdlib:dart.core_patch.bool_patch.invoke [library], was: sub_9002"
        ),
        "generic direct calls with only library marker should rewrite to semantic library invoke:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("sub_9002("),
        "rewritten semantic library call should not keep generic sub_* call name:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_dispatch_target_library_comment_target_to_dispatch_alias() {
    let ir = FunctionIr {
        function_id: 454,
        name: "dispatchLibraryTargetCommentAlias".to_string(),
        entry_va: 0xc112,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc112,
            instrs: vec![
                LlirInstr {
                    va: 0xc112,
                    op: IROp::Other,
                    src: "mov x30, x21".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc116,
                    op: IROp::Other,
                    src: "ldr x30, [x30, #0]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc11a,
                    op: IROp::LoadPool,
                    src: "x2".to_string(),
                    target: "pool[44]".to_string(),
                },
                LlirInstr {
                    va: 0xc11e,
                    op: IROp::Call,
                    src: "blr x30".to_string(),
                    target: "x30".to_string(),
                },
                LlirInstr {
                    va: 0xc122,
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
    pool.insert(44u64, "package:flutter/src/widgets/heroes.dart".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "framework:flutter.widgets.invoke [library], indirect via: dispatchTarget, target: dispatchTargetFn"
        ),
        "dispatch target comment should use alias instead of raw slot expression:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("target: reg21.f0"),
        "raw dispatch slot target comment should be hidden behind alias:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_dispatch_target_fallback_to_package_invoke_when_uri_is_known() {
    let ir = FunctionIr {
        function_id: 452,
        name: "dispatchPackageInvokeFallback".to_string(),
        entry_va: 0xc120,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc120,
            instrs: vec![
                LlirInstr {
                    va: 0xc120,
                    op: IROp::LoadPool,
                    src: "x2".to_string(),
                    target: "pool[45]".to_string(),
                },
                LlirInstr {
                    va: 0xc124,
                    op: IROp::Call,
                    src: "blr x30".to_string(),
                    target: "x30".to_string(),
                },
                LlirInstr {
                    va: 0xc128,
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
    pool.insert(45u64, "package:spotube/models/connect/load.dart".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "spotube.models.connect.load.invoke(receiver, param1, \"package:spotube/models/connect/load.dart\" /* pool[45] */, param3); // package:spotube.models.connect.load.invoke [library], indirect via: dispatchTarget"
        ),
        "package URI should rewrite dispatch fallback to semantic package invoke:\n{}",
        artifact.source
    );
    assert!(
        !artifact
            .source
            .contains("dispatch.invoke(receiver, param1, \"package:spotube/models/connect/load.dart\""),
        "package-aware fallback should avoid plain dispatch.invoke:\n{}",
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
fn rewrites_indirect_call_to_stdlib_list_removeat_when_selector_is_known() {
    let ir = FunctionIr {
        function_id: 460,
        name: "indirectStdlibRemoveAt".to_string(),
        entry_va: 0xc210,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc210,
            instrs: vec![
                LlirInstr {
                    va: 0xc210,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc214,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[52]".to_string(),
                },
                LlirInstr {
                    va: 0xc218,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc21c,
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
    pool.insert(52u64, "removeAt".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core.List.removeAt(1, \"removeAt\" /* pool[52] */, param2, param3); // stdlib:dart.core.List.removeAt [selector], indirect via: indirectTarget9"
        ),
        "known removeAt selector should rewrite to stdlib List.removeAt call:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_stdlib_match_end_when_selector_is_known() {
    let ir = FunctionIr {
        function_id: 472,
        name: "indirectStdlibMatchEnd".to_string(),
        entry_va: 0xc2c0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc2c0,
            instrs: vec![
                LlirInstr {
                    va: 0xc2c0,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc2c4,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[93]".to_string(),
                },
                LlirInstr {
                    va: 0xc2c8,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc2cc,
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
    pool.insert(93u64, "match_end_index".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core.Match.end(1, \"match_end_index\" /* pool[93] */, param2, param3); // stdlib:dart.core.Match.end [selector], indirect via: indirectTarget9"
        ),
        "known match_end_index selector should rewrite to stdlib Match.end call:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dispatch.match_end_index("),
        "known match_end_index selector should avoid dispatch fallback call:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_stdlib_iterator_current_when_internal_selector_is_known() {
    let ir = FunctionIr {
        function_id: 473,
        name: "indirectStdlibIteratorCurrent".to_string(),
        entry_va: 0xc2d0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc2d0,
            instrs: vec![
                LlirInstr {
                    va: 0xc2d0,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc2d4,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[94]".to_string(),
                },
                LlirInstr {
                    va: 0xc2d8,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc2dc,
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
    pool.insert(94u64, "_current".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core.Iterator.current(1, \"_current\" /* pool[94] */, param2, param3); // stdlib:dart.core.Iterator.current [selector], indirect via: indirectTarget9"
        ),
        "known internal _current selector should rewrite to stdlib Iterator.current call:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dispatch.current("),
        "known internal _current selector should avoid dispatch fallback call:\n{}",
        artifact.source
    );
}

#[test]
fn keeps_dispatch_fallback_for_plain_current_selector_name() {
    let ir = FunctionIr {
        function_id: 474,
        name: "indirectPlainCurrentSelector".to_string(),
        entry_va: 0xc2e0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc2e0,
            instrs: vec![
                LlirInstr {
                    va: 0xc2e0,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc2e4,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[95]".to_string(),
                },
                LlirInstr {
                    va: 0xc2e8,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc2ec,
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
    pool.insert(95u64, "current".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains("dispatch.current(1, \"current\" /* pool[95] */, param2, param3); // selector: current, indirect via: indirectTarget9"),
        "plain current selector should stay dispatch fallback to avoid false positives:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dart.core.Iterator.current("),
        "plain current selector should not force stdlib Iterator.current mapping:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_stdlib_datetime_equivalent_year_when_internal_selector_is_known() {
    let ir = FunctionIr {
        function_id: 475,
        name: "indirectStdlibDateTimeEquivalentYear".to_string(),
        entry_va: 0xc2f0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc2f0,
            instrs: vec![
                LlirInstr {
                    va: 0xc2f0,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc2f4,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[96]".to_string(),
                },
                LlirInstr {
                    va: 0xc2f8,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc2fc,
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
    pool.insert(96u64, "_equivalentYear".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core.DateTime.equivalentYear(1, \"_equivalentYear\" /* pool[96] */, param2, param3); // stdlib:dart.core.DateTime.equivalentYear [selector], indirect via: indirectTarget9"
        ),
        "known internal _equivalentYear selector should rewrite to stdlib DateTime.equivalentYear call:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dispatch.equivalentYear("),
        "known internal _equivalentYear selector should avoid dispatch fallback call:\n{}",
        artifact.source
    );
}

#[test]
fn keeps_dispatch_fallback_for_plain_equivalent_year_selector_name() {
    let ir = FunctionIr {
        function_id: 476,
        name: "indirectPlainEquivalentYearSelector".to_string(),
        entry_va: 0xc300,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc300,
            instrs: vec![
                LlirInstr {
                    va: 0xc300,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc304,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[97]".to_string(),
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
    pool.insert(97u64, "equivalentYear".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains("dispatch.equivalentYear(1, \"equivalentYear\" /* pool[97] */, param2, param3); // selector: equivalentYear, indirect via: indirectTarget9"),
        "plain equivalentYear selector should stay dispatch fallback to avoid false positives:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dart.core.DateTime.equivalentYear("),
        "plain equivalentYear selector should not force stdlib DateTime.equivalentYear mapping:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_framework_navigator_pushnamed_when_selector_is_known() {
    let ir = FunctionIr {
        function_id: 461,
        name: "indirectNavigatorPushNamed".to_string(),
        entry_va: 0xc220,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc220,
            instrs: vec![
                LlirInstr {
                    va: 0xc220,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc224,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[53]".to_string(),
                },
                LlirInstr {
                    va: 0xc228,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc22c,
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
    pool.insert(53u64, "pushNamed".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.widgets.Navigator.pushNamed(1, \"pushNamed\" /* pool[53] */, param2, param3); // framework:flutter.widgets.Navigator.pushNamed [selector], indirect via: indirectTarget9"
        ),
        "known pushNamed selector should rewrite to framework Navigator.pushNamed call:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_framework_constructor_when_selector_is_class_name() {
    let ir = FunctionIr {
        function_id: 462,
        name: "indirectKeyedSubtreeCtor".to_string(),
        entry_va: 0xc230,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc230,
            instrs: vec![
                LlirInstr {
                    va: 0xc230,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc234,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[60]".to_string(),
                },
                LlirInstr {
                    va: 0xc238,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc23c,
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
    pool.insert(60u64, "KeyedSubtree".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.widgets.KeyedSubtree.new(1, \"KeyedSubtree\" /* pool[60] */, param2, param3); // framework:flutter.widgets.KeyedSubtree.new [selector], indirect via: indirectTarget9"
        ),
        "class selector should rewrite to framework constructor-style semantic path:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_from_pool_metadata_semantic_owner() {
    let ir = FunctionIr {
        function_id: 463,
        name: "indirectMetadataSemantic".to_string(),
        entry_va: 0xc240,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc240,
            instrs: vec![
                LlirInstr {
                    va: 0xc240,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc244,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[88]".to_string(),
                },
                LlirInstr {
                    va: 0xc248,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc24c,
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
    pool.insert(88u64, "opaqueSelector".to_string());
    let mut metadata = HashMap::new();
    metadata.insert(
        88u64,
        PoolSemanticHint {
            selector: Some("didChangeMetrics".to_string()),
            owner_class: Some("WidgetsBindingObserver".to_string()),
            library_uri: Some("package:flutter/src/widgets/binding.dart".to_string()),
            target_va: None,
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &metadata);
    assert!(
        artifact.source.contains(
            "flutter.widgets.WidgetsBindingObserver.didChangeMetrics(1, \"opaqueSelector\" /* pool[88] */, param2, param3); // framework:flutter.widgets.WidgetsBindingObserver.didChangeMetrics [selector], indirect via: indirectTarget9"
        ),
        "pool metadata should drive deterministic semantic owner path rewrite:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_dispatch_selector_from_metadata_without_pool_string() {
    let ir = FunctionIr {
        function_id: 464,
        name: "indirectMetadataDispatch".to_string(),
        entry_va: 0xc250,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc250,
            instrs: vec![
                LlirInstr {
                    va: 0xc250,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[89]".to_string(),
                },
                LlirInstr {
                    va: 0xc254,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc258,
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
    let pool = HashMap::new();
    let mut metadata = HashMap::new();
    metadata.insert(
        89u64,
        PoolSemanticHint {
            selector: Some("customDispatch42".to_string()),
            owner_class: None,
            library_uri: None,
            target_va: None,
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &metadata);
    assert!(
        artifact.source.contains(
            "dispatch.customDispatch42(receiver, pool[89], param2, param3); // selector: customDispatch42, indirect via: indirectTarget9"
        ),
        "selector metadata should provide dispatch fallback names without string pool hint:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_owner_selector_when_library_uri_missing() {
    let ir = FunctionIr {
        function_id: 470,
        name: "indirectMetadataOwnerNoLibrary".to_string(),
        entry_va: 0xc2b0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc2b0,
            instrs: vec![
                LlirInstr {
                    va: 0xc2b0,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[99]".to_string(),
                },
                LlirInstr {
                    va: 0xc2b4,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc2b8,
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
    pool.insert(99u64, "opaqueSelector".to_string());
    let mut metadata = HashMap::new();
    metadata.insert(
        99u64,
        PoolSemanticHint {
            selector: Some("didChangeMetrics".to_string()),
            owner_class: Some("WidgetsBindingObserver".to_string()),
            library_uri: None,
            target_va: None,
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &metadata);
    assert!(
        artifact.source.contains(
            "WidgetsBindingObserver.didChangeMetrics(receiver, \"opaqueSelector\" /* pool[99] */, param2, param3); // owner:WidgetsBindingObserver.didChangeMetrics [selector], indirect via: indirectTarget9"
        ),
        "owner metadata should rewrite selector fallback even when library URI is missing:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dispatch.didChangeMetrics("),
        "owner selector rewrite should avoid dispatch fallback call:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_to_package_owner_selector_from_metadata() {
    let ir = FunctionIr {
        function_id: 469,
        name: "indirectMetadataPackageOwner".to_string(),
        entry_va: 0xc2a0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc2a0,
            instrs: vec![
                LlirInstr {
                    va: 0xc2a0,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[98]".to_string(),
                },
                LlirInstr {
                    va: 0xc2a4,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc2a8,
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
    pool.insert(98u64, "opaqueSelector".to_string());
    let mut metadata = HashMap::new();
    metadata.insert(
        98u64,
        PoolSemanticHint {
            selector: Some("executeCommandAsync".to_string()),
            owner_class: Some("ConnectService".to_string()),
            library_uri: Some("package:spotube/models/connect/load.dart".to_string()),
            target_va: None,
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &metadata);
    assert!(
        artifact.source.contains("spotube.models.connect.load.ConnectService.executeCommandAsync("),
        "package metadata owner selector should rewrite to package-qualified path:\n{}",
        artifact.source
    );
    assert!(
        artifact
            .source
            .contains("package:spotube.models.connect.load.ConnectService.executeCommandAsync [selector]"),
        "package metadata owner selector should include package semantic intent:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dispatch.executeCommandAsync("),
        "package metadata owner selector rewrite should avoid dispatch fallback call:\n{}",
        artifact.source
    );
}

#[test]
fn rewrites_indirect_call_from_target_va_metadata_using_symbol_name() {
    let ir = FunctionIr {
        function_id: 465,
        name: "indirectMetadataTargetVa".to_string(),
        entry_va: 0xc260,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc260,
            instrs: vec![
                LlirInstr {
                    va: 0xc260,
                    op: IROp::LoadPool,
                    src: "x9".to_string(),
                    target: "pool[90]".to_string(),
                },
                LlirInstr {
                    va: 0xc264,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc268,
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
    symbols.insert(0x5000u64, "dart_core_print".to_string());
    let pool = HashMap::new();
    let mut metadata = HashMap::new();
    metadata.insert(
        90u64,
        PoolSemanticHint {
            selector: None,
            owner_class: None,
            library_uri: None,
            target_va: Some(0x5000),
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &metadata);
    assert!(
        artifact.source.contains(
            "dart.core.print(receiver, param1, param2, param3); // stdlib:dart.core.print, indirect via: indirectTarget9, target: pool[90], target_va: 0x5000, was: dart_core_print"
        ),
        "target_va metadata should rewrite indirect call using resolved symbol name:\n{}",
        artifact.source
    );
    assert_eq!(
        artifact.target_va_symbol_calls, 1,
        "target_va rewrite counter should increment for resolved symbol rewrites:\n{}",
        artifact.source
    );
}

#[test]
fn does_not_rewrite_indirect_call_from_target_va_when_symbol_is_generic() {
    let ir = FunctionIr {
        function_id: 466,
        name: "indirectMetadataTargetVaGeneric".to_string(),
        entry_va: 0xc270,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc270,
            instrs: vec![
                LlirInstr {
                    va: 0xc270,
                    op: IROp::LoadPool,
                    src: "x9".to_string(),
                    target: "pool[91]".to_string(),
                },
                LlirInstr {
                    va: 0xc274,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc278,
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
    symbols.insert(0x5001u64, "sub_5001".to_string());
    let pool = HashMap::new();
    let mut metadata = HashMap::new();
    metadata.insert(
        91u64,
        PoolSemanticHint {
            selector: None,
            owner_class: None,
            library_uri: None,
            target_va: Some(0x5001),
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &metadata);
    assert!(
        artifact
            .source
            .contains("indirectTarget9(receiver, param1, param2, param3)"),
        "generic target symbols should not force semantic rewrite:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dart.core.print("),
        "generic target symbols should not invent semantic target names:\n{}",
        artifact.source
    );
    assert_eq!(
        artifact.target_va_symbol_calls, 0,
        "target_va rewrite counter should remain zero for generic symbols:\n{}",
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
            .contains("dart.typed_data.TypedData.offsetInBytes(receiver, param1, param2, param3); // stdlib:dart.typed_data.TypedData.offsetInBytes [selector], indirect via: indirectTarget9, target: pool[40 /* \"_offsetInBytes\" */].f7"),
        "pool mapping should drive typed_data semantic selector rewrite:\n{}",
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
fn rewrites_constructor_like_nonstandard_selector_to_new_call() {
    let ir = FunctionIr {
        function_id: 481,
        name: "indirectCtorLikeSelector".to_string(),
        entry_va: 0xc410,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc410,
            instrs: vec![
                LlirInstr {
                    va: 0xc410,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[13]".to_string(),
                },
                LlirInstr {
                    va: 0xc414,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc418,
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
    pool.insert(13u64, "AndroidPermission".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "AndroidPermission.new(receiver, \"AndroidPermission\" /* pool[13] */, param2, param3); // selector: AndroidPermission, heuristic: constructor-like selector, indirect via: indirectTarget9"
        ),
        "constructor-like selector should render as .new fallback call:\n{}",
        artifact.source
    );
}

#[test]
fn keeps_dispatch_fallback_for_acronym_like_selector_names() {
    let ir = FunctionIr {
        function_id: 482,
        name: "indirectAcronymSelector".to_string(),
        entry_va: 0xc420,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc420,
            instrs: vec![
                LlirInstr {
                    va: 0xc420,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[14]".to_string(),
                },
                LlirInstr {
                    va: 0xc424,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc428,
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
    pool.insert(14u64, "TORRENT".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dispatch.TORRENT(receiver, \"TORRENT\" /* pool[14] */, param2, param3); // selector: TORRENT, indirect via: indirectTarget9"
        ),
        "acronym-like selectors should keep dispatch fallback form:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("TORRENT.new("),
        "acronym-like selectors should not be rewritten as constructors:\n{}",
        artifact.source
    );
}

#[test]
fn keeps_dispatch_fallback_for_builtin_type_selector_names() {
    let ir = FunctionIr {
        function_id: 483,
        name: "indirectBuiltinTypeSelector".to_string(),
        entry_va: 0xc430,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc430,
            instrs: vec![
                LlirInstr {
                    va: 0xc430,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[15]".to_string(),
                },
                LlirInstr {
                    va: 0xc434,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc438,
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
    pool.insert(15u64, "Function".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dispatch.Function(receiver, \"Function\" /* pool[15] */, param2, param3); // selector: Function, indirect via: indirectTarget9"
        ),
        "builtin type selectors should keep dispatch fallback form:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("Function.new("),
        "builtin type selectors should not be rewritten as constructors:\n{}",
        artifact.source
    );
}

#[test]
fn keeps_dynamic_call_when_target_selector_is_file_path_like() {
    let ir = FunctionIr {
        function_id: 49,
        name: "indirectFileLikeSelector".to_string(),
        entry_va: 0xc500,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xc500,
            instrs: vec![
                LlirInstr {
                    va: 0xc500,
                    op: IROp::LoadPool,
                    src: "x9".to_string(),
                    target: "pool[77]".to_string(),
                },
                LlirInstr {
                    va: 0xc504,
                    op: IROp::Other,
                    src: "ldr x9, [x9, #7]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xc508,
                    op: IROp::Call,
                    src: "blr x9".to_string(),
                    target: "x9".to_string(),
                },
                LlirInstr {
                    va: 0xc50c,
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
    pool.insert(77u64, "dart_mappablesrcmapper_utils.dart".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains("indirectTarget9("),
        "file-like selector hints should not force semantic rewrite:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dart.core.map(") && !artifact.source.contains("dispatch."),
        "file-like selector hints should avoid false positive standard/dispatch names:\n{}",
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
