use super::*;
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::fs;
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("golden")
        .join(name)
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var("FLUTTERDEC_UPDATE_GOLDEN")
        .ok()
        .as_deref()
        == Some("1")
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create golden directory");
        }
        fs::write(&path, format!("{}\n", actual.trim_end()))
            .expect("failed to update golden snapshot");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden snapshot at {} ({e})",
            path.display()
        )
    });
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "golden mismatch for {} (set FLUTTERDEC_UPDATE_GOLDEN=1 to refresh)",
        path.display()
    );
}

fn branch_block(id: usize, va: u64, true_va: u64, false_id: usize, true_id: usize) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs: vec![LlirInstr {
            va,
            op: IROp::Branch,
            src: format!("cbz x0, #0x{true_va:x}"),
            target: format!("#0x{true_va:x}"),
        }],
        succs: vec![true_id, false_id],
        preds: Vec::new(),
    }
}

fn jump_block(id: usize, va: u64, to_id: usize, to_va: u64) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs: vec![LlirInstr {
            va,
            op: IROp::Jump,
            src: format!("b #0x{to_va:x}"),
            target: format!("#0x{to_va:x}"),
        }],
        succs: vec![to_id],
        preds: Vec::new(),
    }
}

#[test]
fn emits_helper_bodies_for_omitted_paths() {
    let va = |id: usize| 0x1000 + (id as u64) * 4;
    let mut blocks = vec![
        branch_block(0, va(0), va(1), 2, 1),
        branch_block(1, va(1), va(3), 4, 3),
        branch_block(2, va(2), va(5), 6, 5),
        branch_block(3, va(3), va(7), 8, 7),
        branch_block(4, va(4), va(9), 10, 9),
        branch_block(5, va(5), va(11), 12, 11),
        branch_block(6, va(6), va(13), 14, 13),
        jump_block(7, va(7), 15, va(15)),
        jump_block(8, va(8), 15, va(15)),
        jump_block(9, va(9), 15, va(15)),
        jump_block(10, va(10), 15, va(15)),
        jump_block(11, va(11), 15, va(15)),
        jump_block(12, va(12), 15, va(15)),
        jump_block(13, va(13), 15, va(15)),
        jump_block(14, va(14), 15, va(15)),
        BasicBlock {
            id: 15,
            start_va: va(15),
            instrs: vec![LlirInstr {
                va: va(15),
                op: IROp::Return,
                src: "ret".to_string(),
                target: String::new(),
            }],
            succs: Vec::new(),
            preds: vec![7, 8, 9, 10, 11, 12, 13, 14],
        },
    ];

    for b in &mut blocks {
        b.preds.clear();
    }
    for idx in 0..blocks.len() {
        let pred = blocks[idx].id;
        let succs = blocks[idx].succs.clone();
        for succ in succs {
            if let Some(target) = blocks.iter_mut().find(|b| b.id == succ) {
                target.preds.push(pred);
            }
        }
    }

    let ir = FunctionIr {
        function_id: 1,
        name: "testFunc".to_string(),
        entry_va: va(0),
        blocks,
    };
    let symbols = HashMap::new();
    let artifact = emit_pseudocode(&ir, &symbols);

    assert!(
        !artifact.source.contains("path omitted"),
        "unexpected placeholder stub:\n{}",
        artifact.source
    );
    if artifact.source.contains("return _block_15();") {
        assert!(artifact.source.contains("dynamic _block_15() {"));
    }
}

#[test]
fn inlines_trivial_return_helpers() {
    let mut blocks = Vec::new();
    for id in 0..12usize {
        blocks.push(jump_block(
            id,
            0x2000 + (id as u64) * 4,
            id + 1,
            0x2000 + ((id + 1) as u64) * 4,
        ));
    }
    blocks.push(BasicBlock {
        id: 12,
        start_va: 0x2000 + 12 * 4,
        instrs: vec![LlirInstr {
            va: 0x2000 + 12 * 4,
            op: IROp::Return,
            src: "ret".to_string(),
            target: String::new(),
        }],
        succs: Vec::new(),
        preds: vec![11],
    });

    for b in &mut blocks {
        b.preds.clear();
    }
    for idx in 0..blocks.len() {
        let pred = blocks[idx].id;
        let succs = blocks[idx].succs.clone();
        for succ in succs {
            if let Some(target) = blocks.iter_mut().find(|b| b.id == succ) {
                target.preds.push(pred);
            }
        }
    }

    let ir = FunctionIr {
        function_id: 2,
        name: "deepChain".to_string(),
        entry_va: 0x2000,
        blocks,
    };
    let symbols = HashMap::new();
    let artifact = emit_pseudocode(&ir, &symbols);

    assert!(
        !artifact.source.contains("return _block_12();"),
        "trivial helper call should be inlined:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dynamic _block_12()"),
        "trivial helper should be removed:\n{}",
        artifact.source
    );
}

#[test]
fn inlines_linear_helper_body_at_call_site() {
    let ir = FunctionIr {
        function_id: 3,
        name: "manualInline".to_string(),
        entry_va: 0x3000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic manualInline(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_1();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_1() {".to_string(),
            "  final t1 = fn_0x1(arg0, arg1, arg2, arg3);".to_string(),
            "  return t1;".to_string(),
            "}".to_string(),
        ];

    emitter.inline_trivial_helpers();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("return _block_1();"),
        "call should be inlined:\n{out}"
    );
    assert!(
        !out.contains("dynamic _block_1()"),
        "unused helper should be removed:\n{out}"
    );
    assert!(
        out.contains("final t1 = fn_0x1(arg0, arg1, arg2, arg3);"),
        "linear helper body should be inserted:\n{out}"
    );
}

#[test]
fn inlines_branch_helper_body_with_null_fallback() {
    let ir = FunctionIr {
        function_id: 4,
        name: "manualBranchInline".to_string(),
        entry_va: 0x4000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic manualBranchInline(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_9();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_9() {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    final t1 = fn_0x2(arg0, arg1, arg2, arg3);".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    return arg0;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.inline_trivial_helpers();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("return _block_9();"),
        "call should be inlined:\n{out}"
    );
    assert!(
        !out.contains("dynamic _block_9()"),
        "unused helper should be removed:\n{out}"
    );
    assert!(
        out.contains("if (arg0 == null) {"),
        "branch helper body should be inserted:\n{out}"
    );
    assert!(
        out.contains("return null;"),
        "non-total branch helper should append null fallback:\n{out}"
    );
}

#[test]
fn inlines_placeholder_cond_helper_body() {
    let ir = FunctionIr {
        function_id: 5,
        name: "manualCondInline".to_string(),
        entry_va: 0x5000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic manualCondInline(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_3();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_3() {".to_string(),
            "  if (/* cond */) {".to_string(),
            "    return null;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    final t1 = fn_0x3(arg0, arg1, arg2, arg3);".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.inline_trivial_helpers();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("return _block_3();"),
        "call should be inlined:\n{out}"
    );
    assert!(
        !out.contains("dynamic _block_3()"),
        "unused helper should be removed:\n{out}"
    );
    assert!(
        out.contains("if (/* cond */) {"),
        "placeholder condition helper should be inlined:\n{out}"
    );
}

#[test]
fn compacts_empty_else_and_duplicate_null_returns() {
    let ir = FunctionIr {
        function_id: 6,
        name: "manualCompact".to_string(),
        entry_va: 0x6000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic manualCompact(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return null;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "  }".to_string(),
            "  return null;".to_string(),
            "  return null;".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("else {\n  }"),
        "empty else should be removed:\n{out}"
    );
    assert!(
        !out.contains("return null;\n  return null;"),
        "duplicate null returns should collapse:\n{out}"
    );
}

#[test]
fn emits_flag_predicate_when_cmp_is_missing() {
    let ir = FunctionIr {
        function_id: 7,
        name: "flagFallback".to_string(),
        entry_va: 0x7000,
        blocks: vec![
            BasicBlock {
                id: 0,
                start_va: 0x7000,
                instrs: vec![LlirInstr {
                    va: 0x7000,
                    op: IROp::Branch,
                    src: "b.eq #0x7008".to_string(),
                    target: "#0x7008".to_string(),
                }],
                succs: vec![1, 2],
                preds: Vec::new(),
            },
            BasicBlock {
                id: 1,
                start_va: 0x7008,
                instrs: vec![LlirInstr {
                    va: 0x7008,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                }],
                succs: Vec::new(),
                preds: vec![0],
            },
            BasicBlock {
                id: 2,
                start_va: 0x7004,
                instrs: vec![LlirInstr {
                    va: 0x7004,
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                }],
                succs: Vec::new(),
                preds: vec![0],
            },
        ],
    };
    let symbols = HashMap::new();
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains("if (flags.b_eq) {"),
        "missing flag predicate fallback:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("/* cond */"),
        "placeholder cond should not be emitted:\n{}",
        artifact.source
    );
}

#[test]
fn infers_local_names_and_int_types() {
    let ir = FunctionIr {
        function_id: 8,
        name: "manualHints".to_string(),
        entry_va: 0x8000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.locals.insert(-16, "local_m16".to_string());
    emitter.lines = vec![
            "dynamic manualHints(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  var local_m8;".to_string(),
            "  var local_m16;".to_string(),
            "".to_string(),
            "  local_m8 = (arg2 + 1);".to_string(),
            "  local_m8 = (local_m8 + 2);".to_string(),
            "  local_m8 = (local_m8 << 1);".to_string(),
            "  local_m16 = pool[42];".to_string(),
            "  if (local_m16.f7 == null) {".to_string(),
            "    return local_m8;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.apply_name_and_type_hints("manualHints");
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("local_m8"),
        "stack local should be renamed:\n{out}"
    );
    assert!(
        !out.contains("local_m16"),
        "stack local should be renamed:\n{out}"
    );
    assert!(
        out.contains("int intTmp"),
        "arithmetic local should get int type:\n{out}"
    );
    assert!(
        out.contains("dynamic poolVal"),
        "pool-assigned local should get poolVal naming:\n{out}"
    );
}

#[test]
fn renames_receiver_argument_from_field_usage() {
    let ir = FunctionIr {
        function_id: 9,
        name: "receiverHints".to_string(),
        entry_va: 0x9000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic receiverHints(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0.f7 == null) {".to_string(),
            "    return arg0;".to_string(),
            "  }".to_string(),
            "  return arg0.f11;".to_string(),
            "}".to_string(),
        ];

    emitter.apply_name_and_type_hints("receiverHints");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("dynamic receiver"),
        "arg0 should be renamed to receiver:\n{out}"
    );
    assert!(
        !out.contains("arg0.f"),
        "field access should use receiver:\n{out}"
    );
}

#[test]
fn renames_receiver_argument_without_field_usage() {
    let ir = FunctionIr {
        function_id: 10,
        name: "receiverDefault".to_string(),
        entry_va: 0xa000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic receiverDefault(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  final t1 = fn_0x10(arg0, arg1, arg2, arg3);".to_string(),
            "  return t1;".to_string(),
            "}".to_string(),
        ];

    emitter.apply_name_and_type_hints("receiverDefault");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("dynamic receiver"),
        "arg0 should default to receiver:\n{out}"
    );
    assert!(!out.contains("arg0"), "arg0 should be replaced:\n{out}");
    assert!(
        out.contains("dynamic param1"),
        "non-inferred args should use param naming:\n{out}"
    );
}

#[test]
fn aliases_raw_register_names_after_hinting() {
    let ir = FunctionIr {
        function_id: 11,
        name: "regAlias".to_string(),
        entry_va: 0xb000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic regAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  final t1 = invoke(x2, [arg0, arg1, arg2, arg3]);".to_string(),
            "  final t2 = invoke(x30, [arg0, arg1, arg2, arg3]);".to_string(),
            "  return t2;".to_string(),
            "}".to_string(),
        ];

    emitter.apply_name_and_type_hints("regAlias");
    let out = emitter.lines.join("\n");
    assert!(!out.contains("x2"), "x2 should be aliased:\n{out}");
    assert!(!out.contains("x30"), "x30 should be aliased:\n{out}");
    assert!(out.contains("reg2"), "reg2 alias missing:\n{out}");
    assert!(
        out.contains("returnAddress"),
        "x30 should map to returnAddress:\n{out}"
    );
}

#[test]
fn aliases_frame_and_return_registers_with_semantic_names() {
    let ir = FunctionIr {
        function_id: 21,
        name: "frameRegs".to_string(),
        entry_va: 0xf600,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic frameRegs(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  final t1 = x29;".to_string(),
            "  final t2 = x30;".to_string(),
            "  return t2;".to_string(),
            "}".to_string(),
        ];

    emitter.apply_name_and_type_hints("frameRegs");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("framePointer") && out.contains("returnAddress"),
        "x29/x30 should use semantic aliases:\n{out}"
    );
}

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
fn rewrites_empty_then_else_to_negated_if() {
    let ir = FunctionIr {
        function_id: 22,
        name: "emptyThen".to_string(),
        entry_va: 0xf700,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic emptyThen(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("if (!(arg0 == null)) {"),
        "empty then/else should be rewritten:\n{out}"
    );
    assert!(
        !out.contains("else {"),
        "else branch should be absorbed:\n{out}"
    );
}

#[test]
fn collapses_if_else_with_identical_returns() {
    let ir = FunctionIr {
        function_id: 25,
        name: "sameReturn".to_string(),
        entry_va: 0xfa00,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic sameReturn(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("if (arg0 == null) {"),
        "identical return branches should collapse:\n{out}"
    );
    assert_eq!(
        out.matches("return arg1;").count(),
        1,
        "collapsed output should keep one return:\n{out}"
    );
}

#[test]
fn collapses_if_then_return_followed_by_same_return() {
    let ir = FunctionIr {
        function_id: 34,
        name: "sameReturnNoElse".to_string(),
        entry_va: 0x13000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic sameReturnNoElse(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  return arg1;".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("if (arg0 == null) {"),
        "redundant guarded return should collapse:\n{out}"
    );
    assert_eq!(
        out.matches("return arg1;").count(),
        1,
        "collapsed output should keep one return:\n{out}"
    );
}

#[test]
fn hoists_else_when_then_terminates() {
    let ir = FunctionIr {
        function_id: 26,
        name: "hoistElse".to_string(),
        entry_va: 0xfb00,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic hoistElse(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    final t1 = fn_0x1(arg0, arg1, arg2, arg3);".to_string(),
            "    return t1;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("if (arg0 == null) {\n    return arg1;\n  }\n  final t1 = fn_0x1"),
        "else body should be hoisted after terminating then-branch:\n{out}"
    );
    assert!(!out.contains("else {"), "else should be removed:\n{out}");
}

#[test]
fn merges_nested_single_if_guards() {
    let ir = FunctionIr {
        function_id: 29,
        name: "mergeNestedIf".to_string(),
        entry_va: 0xfe00,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic mergeNestedIf(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 != null) {".to_string(),
            "    if (arg1 != null) {".to_string(),
            "      return arg2;".to_string(),
            "    }".to_string(),
            "  }".to_string(),
            "  return null;".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("if ((arg0 != null) && (arg1 != null)) {"),
        "nested if guards should merge:\n{out}"
    );
    assert!(
        !out.contains("if (arg0 != null) {\n    if (arg1 != null) {"),
        "legacy nested guard shape should be removed:\n{out}"
    );
}

#[test]
fn removes_redundant_null_check_after_terminating_guard() {
    let ir = FunctionIr {
        function_id: 27,
        name: "redundantNull".to_string(),
        entry_va: 0xfc00,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic redundantNull(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    final t1 = fn_0x1(arg0, arg1, arg2, arg3);".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    return t1;".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("if (arg0 == null) {\n      continue;"),
        "redundant second null-check should be removed:\n{out}"
    );
    assert!(
        !out.contains("while (true) {"),
        "removing synthetic continue should allow wrapper unwrap:\n{out}"
    );
}

#[test]
fn keeps_null_check_when_identifier_is_reassigned() {
    let ir = FunctionIr {
        function_id: 28,
        name: "reassignedNull".to_string(),
        entry_va: 0xfd00,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic reassignedNull(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  arg0 = arg1;".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg2;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.matches("if (arg0 == null) {").count() >= 2,
        "second null-check must stay when variable is reassigned:\n{out}"
    );
}

#[test]
fn unwraps_single_iteration_while_without_continue() {
    let ir = FunctionIr {
        function_id: 23,
        name: "loopWrapper".to_string(),
        entry_va: 0xf800,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic loopWrapper(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("while (true) {"),
        "single-iteration wrappers should be removed:\n{out}"
    );
    assert!(
        out.contains("if (arg0 == null) {"),
        "body should remain after unwrap:\n{out}"
    );
}

#[test]
fn keeps_while_wrapper_when_continue_exists() {
    let ir = FunctionIr {
        function_id: 24,
        name: "loopContinue".to_string(),
        entry_va: 0xf900,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic loopContinue(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("while (true) {"),
        "real loop control flow should keep wrapper:\n{out}"
    );
    assert!(out.contains("continue;"), "continue should remain:\n{out}");
}

#[test]
fn rewrites_multi_continue_loop_as_retry_condition() {
    let ir = FunctionIr {
        function_id: 30,
        name: "retryLoop".to_string(),
        entry_va: 0xff00,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic retryLoop(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    if (arg1 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    return arg2;".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("bool retryLoop1 = true;") && out.contains("while (retryLoop1) {"),
        "multi-continue loop should get retry condition:\n{out}"
    );
    assert!(
        out.contains("retryLoop1 = false;"),
        "retry fall-through update should be emitted:\n{out}"
    );
    assert!(
        !out.contains("while (true) {"),
        "generic while(true) should be removed for multi-continue loops:\n{out}"
    );
}

#[test]
fn merges_consecutive_continue_guards() {
    let ir = FunctionIr {
        function_id: 33,
        name: "continueGuards".to_string(),
        entry_va: 0x12000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic continueGuards(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == 0x85) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    if (arg0 == 0xa0) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    return arg1;".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("if ((arg0 == 0x85) || (arg0 == 0xa0)) {"),
        "continue guards should merge:\n{out}"
    );
    assert_eq!(
        out.matches("continue;").count(),
        1,
        "merged guard should keep one continue:\n{out}"
    );
}

#[test]
fn rewrites_return_then_continue_range_pattern() {
    let ir = FunctionIr {
        function_id: 35,
        name: "rangeContinue".to_string(),
        entry_va: 0x14000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic rangeContinue(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 > 0xd) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  if (arg0 >= 9) {".to_string(),
            "    continue;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("if ((arg0 >= 9) && (arg0 <= 0xd)) {"),
        "range continue guard should be emitted:\n{out}"
    );
    assert!(
        out.contains("if (arg0 > 0xd) {\n    return arg1;\n  }"),
        "upper tail return branch should remain:\n{out}"
    );
}

#[test]
fn unwraps_retry_loop_when_no_retry_paths_remain() {
    let ir = FunctionIr {
        function_id: 31,
        name: "retryCleanup".to_string(),
        entry_va: 0x10000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic retryCleanup(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  bool retryLoop1 = true;".to_string(),
            "  while (retryLoop1) {".to_string(),
            "    retryLoop1 = false;".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    return arg2;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("retryLoop1"),
        "one-shot retry wrappers should collapse:\n{out}"
    );
    assert!(
        out.contains("if (arg0 == null) {"),
        "loop body should remain:\n{out}"
    );
}

#[test]
fn collapses_nested_guarded_returns_inside_if_body() {
    let ir = FunctionIr {
        function_id: 36,
        name: "nestedReturnGuards".to_string(),
        entry_va: 0x15000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic nestedReturnGuards(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 > 0x20) {".to_string(),
            "    if (arg0 == 0x2028) {".to_string(),
            "      return null;".to_string(),
            "    }".to_string(),
            "    return null;".to_string(),
            "  }".to_string(),
            "  return arg1;".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("if (arg0 > 0x20) {\n    return null;\n  }"),
        "nested redundant guarded return should collapse:\n{out}"
    );
    assert!(
        !out.contains("if (arg0 == 0x2028) {"),
        "inner guard should be removed:\n{out}"
    );
}

#[test]
fn extracts_repeated_minus_one_expression_alias() {
    let ir = FunctionIr {
        function_id: 37,
        name: "minusOneAlias".to_string(),
        entry_va: 0x16000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic minusOneAlias(dynamic receiver, dynamic param1, dynamic param2, int value3, dynamic param4, dynamic param5, dynamic param6, dynamic param7) {".to_string(),
            "  if ((value3 - 1) > 0x20) {".to_string(),
            "    return (value3 - 1);".to_string(),
            "  }".to_string(),
            "  if ((value3 - 1) == 0x20) {".to_string(),
            "    return (value3 - 1);".to_string(),
            "  }".to_string(),
            "  return (value3 - 1);".to_string(),
            "}".to_string(),
        ];

    emitter.extract_minus_one_aliases();
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("final int codePoint = (value3 - 1);"),
        "repeated minus-one expression should be aliased:\n{out}"
    );
    assert_eq!(
        out.matches("(value3 - 1)").count(),
        1,
        "all repeated occurrences should use alias after declaration:\n{out}"
    );
}

#[test]
fn collapses_trailing_null_return_guards_after_continue_branches() {
    let ir = FunctionIr {
        function_id: 38,
        name: "nullGuardsAfterContinue".to_string(),
        entry_va: 0x17000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
            "dynamic nullGuardsAfterContinue(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 > 0x20) {".to_string(),
            "    if (arg0 < 0x85) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    if ((arg0 == 0x85) || (arg0 == 0xa0)) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    if (arg0 > 0x200a) {".to_string(),
            "      if (arg0 == 0x2028) {".to_string(),
            "        return null;".to_string(),
            "      }".to_string(),
            "      return null;".to_string(),
            "    }".to_string(),
            "    if (arg0 == 0x1680) {".to_string(),
            "      return null;".to_string(),
            "    }".to_string(),
            "    return null;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("if (arg0 == 0x2028) {"),
        "nested redundant null guard should be removed:\n{out}"
    );
    assert!(
        !out.contains("if (arg0 == 0x1680) {"),
        "trailing redundant null guard should be removed:\n{out}"
    );
}

#[test]
fn rewrites_negated_not_equal_comparisons() {
    let line = "if (!((classId(arg1) << 1) != 0xbc)) {".to_string();
    let got = FuncEmitter::clean_expr(line);
    assert_eq!(got, "if ((classId(arg1) << 1) == 0xbc) {");
}

#[test]
fn simplifies_redundant_wrapped_if_conditions() {
    let line = "  if (((arg0 == 1))) {".to_string();
    let got = FuncEmitter::clean_expr(line);
    assert_eq!(got, "  if (arg0 == 1) {");
}

#[test]
fn golden_retry_loop_compaction_snapshot() {
    let ir = FunctionIr {
        function_id: 901,
        name: "goldenRetryLoop".to_string(),
        entry_va: 0x20100,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic goldenRetryLoop(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  while (true) {".to_string(),
        "    if (arg0 == null) {".to_string(),
        "      continue;".to_string(),
        "    }".to_string(),
        "    if (arg1 == null) {".to_string(),
        "      continue;".to_string(),
        "    }".to_string(),
        "    if (arg2 > 0xd) {".to_string(),
        "      return arg3;".to_string(),
        "    }".to_string(),
        "    if (arg2 >= 9) {".to_string(),
        "      continue;".to_string(),
        "    }".to_string(),
        "    return arg4;".to_string(),
        "    break;".to_string(),
        "  }".to_string(),
        "}".to_string(),
    ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert_golden("retry_loop_compaction.dartpseudo", &out);
}

#[test]
fn golden_null_guard_compaction_snapshot() {
    let ir = FunctionIr {
        function_id: 902,
        name: "goldenNullGuard".to_string(),
        entry_va: 0x20200,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic goldenNullGuard(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  if (arg0 > 0x20) {".to_string(),
        "    if (arg0 < 0x85) {".to_string(),
        "      return arg1;".to_string(),
        "    }".to_string(),
        "    if ((arg0 == 0x85) || (arg0 == 0xa0)) {".to_string(),
        "      continue;".to_string(),
        "    }".to_string(),
        "    if (arg0 > 0x200a) {".to_string(),
        "      if (arg0 == 0x2028) {".to_string(),
        "        return null;".to_string(),
        "      }".to_string(),
        "      return null;".to_string(),
        "    }".to_string(),
        "    if (arg0 == 0x1680) {".to_string(),
        "      return null;".to_string(),
        "    }".to_string(),
        "    return null;".to_string(),
        "  }".to_string(),
        "}".to_string(),
    ];

    emitter.compact_lines();
    let out = emitter.lines.join("\n");
    assert_golden("null_guard_compaction.dartpseudo", &out);
}

#[test]
fn golden_structured_loop_emit_snapshot() {
    let ir = FunctionIr {
        function_id: 903,
        name: "goldenSimpleLoop".to_string(),
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

    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert_golden("structured_loop_emit.dartpseudo", &out);
}

#[test]
fn normalize_target_prefers_last_hex_operand() {
    let got = normalize_target("x0, #0x3f, #0x2008");
    assert_eq!(got, "0x2008");
}
