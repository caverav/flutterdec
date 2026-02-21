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
fn infers_receiver_type_from_semantic_call_path() {
    let ir = FunctionIr {
        function_id: 801,
        name: "typedReceiver".to_string(),
        entry_va: 0x801000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic typedReceiver(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = flutter.widgets.State.setState(arg0, arg1, arg2, arg3); // framework:flutter.widgets.State.setState".to_string(),
        "  return t1;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedReceiver");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("flutter.widgets.State receiver"),
        "semantic call path should type receiver as flutter State:\n{out}"
    );
}

#[test]
fn infers_receiver_type_from_semantic_comment_path() {
    let ir = FunctionIr {
        function_id: 802,
        name: "typedFuture".to_string(),
        entry_va: 0x802000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic typedFuture(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = customFutureCall(arg1, arg2, arg3, arg4); // stdlib:dart.async.Future.then [selector], was: sub_7000".to_string(),
        "  return t1;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedFuture");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("dart.async.Future param1"),
        "semantic intent comment should type Future receiver:\n{out}"
    );
}

#[test]
fn infers_receiver_type_from_classid_receiver_pattern() {
    let ir = FunctionIr {
        function_id: 804,
        name: "typedClassIdReceiver".to_string(),
        entry_va: 0x804000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic typedClassIdReceiver(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = flutter.widgets.State.dispose(classId(arg0), arg0, \"_dispose\" /* pool[42] */, arg3); // framework:flutter.widgets.State.dispose [selector], indirect via: dispatchTarget".to_string(),
        "  return t1;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedClassIdReceiver");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("flutter.widgets.State receiver"),
        "classId(receiver), receiver pattern should infer typed receiver:\n{out}"
    );
}

#[test]
fn does_not_infer_receiver_type_from_constructor_semantic_path() {
    let ir = FunctionIr {
        function_id: 805,
        name: "ctorNoReceiverType".to_string(),
        entry_va: 0x805000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic ctorNoReceiverType(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = dart.typed_data.Int64List.new(arg0, arg1, arg2, arg3); // stdlib:dart.typed_data.Int64List.new [selector], was: sub_9000".to_string(),
        "  return t1;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("ctorNoReceiverType");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("dynamic receiver"),
        "constructor semantic paths should not force receiver typing:\n{out}"
    );
    assert!(
        !out.contains("dart.typed_data.Int64List receiver"),
        "constructor semantic paths should not be treated as instance receiver calls:\n{out}"
    );
}

#[test]
fn infers_string_and_bool_types_for_literal_assigned_locals() {
    let ir = FunctionIr {
        function_id: 803,
        name: "typedLiterals".to_string(),
        entry_va: 0x803000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.locals.insert(-16, "local_m16".to_string());
    emitter.lines = vec![
        "dynamic typedLiterals(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "  var local_m16;".to_string(),
        "".to_string(),
        "  local_m8 = \"setState\";".to_string(),
        "  local_m16 = true;".to_string(),
        "  return local_m8;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedLiterals");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("String tmp"),
        "string literal assignment should infer String local type:\n{out}"
    );
    assert!(
        out.contains("bool "),
        "bool literal assignment should infer bool local type:\n{out}"
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
fn annotates_stdlib_call_intent_when_symbol_is_named() {
    let ir = FunctionIr {
        function_id: 22,
        name: "stdlibCall".to_string(),
        entry_va: 0xf700,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf700,
            instrs: vec![
                LlirInstr {
                    va: 0xf700,
                    op: IROp::Call,
                    src: "bl #0x5000".to_string(),
                    target: "#0x5000".to_string(),
                },
                LlirInstr {
                    va: 0xf704,
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
    symbols.insert(0x5000, "dart_core_print".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact
            .source
            .contains("final t1 = dart.core.print(receiver, param1, param2, param3); // stdlib:dart.core.print, was: dart_core_print"),
        "missing stdlib call intent annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_runtime_and_native_call_intents() {
    let ir = FunctionIr {
        function_id: 23,
        name: "runtimeNativeCalls".to_string(),
        entry_va: 0xf800,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf800,
            instrs: vec![
                LlirInstr {
                    va: 0xf800,
                    op: IROp::Call,
                    src: "bl #0x6000".to_string(),
                    target: "#0x6000".to_string(),
                },
                LlirInstr {
                    va: 0xf804,
                    op: IROp::Call,
                    src: "bl #0x7000".to_string(),
                    target: "#0x7000".to_string(),
                },
                LlirInstr {
                    va: 0xf808,
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
    symbols.insert(0x6000, "vm_runtime_Invoke".to_string());
    symbols.insert(0x7000, "native_libc_memcpy".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact
            .source
            .contains("dart_vm.invoke(receiver, param1, param2, param3); // runtime:dart_vm.invoke, was: vm_runtime_Invoke"),
        "missing runtime call intent annotation:\n{}",
        artifact.source
    );
    assert!(
        artifact
            .source
            .contains("libc.memcpy(t1, param1, param2, param3); // native:libc.memcpy, was: native_libc_memcpy"),
        "missing native call intent annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_flutter_framework_call_intents() {
    let ir = FunctionIr {
        function_id: 24,
        name: "frameworkCalls".to_string(),
        entry_va: 0xf900,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf900,
            instrs: vec![
                LlirInstr {
                    va: 0xf900,
                    op: IROp::Call,
                    src: "bl #0x6100".to_string(),
                    target: "#0x6100".to_string(),
                },
                LlirInstr {
                    va: 0xf904,
                    op: IROp::Call,
                    src: "bl #0x6200".to_string(),
                    target: "#0x6200".to_string(),
                },
                LlirInstr {
                    va: 0xf908,
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
    symbols.insert(0x6100, "flutter_widgets_State_setState".to_string());
    symbols.insert(0x6200, "flutter_widgets_StatefulWidget_createState".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact
            .source
            .contains("flutter.widgets.State.setState(receiver, param1, param2, param3); // framework:flutter.widgets.State.setState, was: flutter_widgets_State_setState"),
        "missing flutter setState intent annotation:\n{}",
        artifact.source
    );
    assert!(
        artifact
            .source
            .contains("flutter.widgets.StatefulWidget.createState(t1, param1, param2, param3); // framework:flutter.widgets.StatefulWidget.createState, was: flutter_widgets_StatefulWidget_createState"),
        "missing flutter createState intent annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_framework_from_pool_selector_when_call_name_is_generic() {
    let ir = FunctionIr {
        function_id: 25,
        name: "selectorCall".to_string(),
        entry_va: 0xfa00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfa00,
            instrs: vec![
                LlirInstr {
                    va: 0xfa00,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfa04,
                    op: IROp::Other,
                    src: "mov x1, #2".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfa08,
                    op: IROp::LoadPool,
                    src: "x2".to_string(),
                    target: "pool[42]".to_string(),
                },
                LlirInstr {
                    va: 0xfa0c,
                    op: IROp::Call,
                    src: "bl #0x6100".to_string(),
                    target: "#0x6100".to_string(),
                },
                LlirInstr {
                    va: 0xfa10,
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
    symbols.insert(0x6100, "sub_6100".to_string());
    let mut pool = HashMap::new();
    pool.insert(42u64, "setState".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact
            .source
            .contains("flutter.widgets.State.setState(1, 2, \"setState\" /* pool[42] */, param3); // framework:flutter.widgets.State.setState [selector], was: sub_6100"),
        "missing selector-based framework annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_flutter_scheduler_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 26,
        name: "schedulerSelector".to_string(),
        entry_va: 0xfb00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfb00,
            instrs: vec![
                LlirInstr {
                    va: 0xfb00,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfb04,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[7]".to_string(),
                },
                LlirInstr {
                    va: 0xfb08,
                    op: IROp::Call,
                    src: "bl #0x7000".to_string(),
                    target: "#0x7000".to_string(),
                },
                LlirInstr {
                    va: 0xfb0c,
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
    symbols.insert(0x7000, "sub_7000".to_string());
    let mut pool = HashMap::new();
    pool.insert(7u64, "addPostFrameCallback".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.scheduler.SchedulerBinding.addPostFrameCallback(1, \"addPostFrameCallback\" /* pool[7] */, param2, param3); // framework:flutter.scheduler.SchedulerBinding.addPostFrameCallback [selector], was: sub_7000"
        ),
        "missing scheduler selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_async_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 27,
        name: "asyncSelector".to_string(),
        entry_va: 0xfc00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfc00,
            instrs: vec![
                LlirInstr {
                    va: 0xfc00,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfc04,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[9]".to_string(),
                },
                LlirInstr {
                    va: 0xfc08,
                    op: IROp::Call,
                    src: "bl #0x7100".to_string(),
                    target: "#0x7100".to_string(),
                },
                LlirInstr {
                    va: 0xfc0c,
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
    symbols.insert(0x7100, "sub_7100".to_string());
    let mut pool = HashMap::new();
    pool.insert(9u64, "catchError".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.async.Future.catchError(1, \"catchError\" /* pool[9] */, param2, param3); // stdlib:dart.async.Future.catchError [selector], was: sub_7100"
        ),
        "missing dart async selector annotation:\n{}",
        artifact.source
    );
}
