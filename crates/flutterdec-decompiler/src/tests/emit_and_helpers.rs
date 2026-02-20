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

