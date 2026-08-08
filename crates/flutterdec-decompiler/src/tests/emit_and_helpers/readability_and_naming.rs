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
fn infers_receiver_type_from_package_owner_semantic_comment_path() {
    let ir = FunctionIr {
        function_id: 808,
        name: "typedPackageReceiver".to_string(),
        entry_va: 0x808000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic typedPackageReceiver(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = sub_9300(arg1, arg2, arg3, arg4); // package:spotube.models.connect.load.ConnectService.executeCommandAsync [selector], was: sub_9300".to_string(),
        "  return t1;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedPackageReceiver");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("spotube.models.connect.load.ConnectService param1"),
        "package owner semantic comment should type receiver with package owner path:\n{out}"
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
fn infers_local_type_from_constructor_call_path() {
    let ir = FunctionIr {
        function_id: 806,
        name: "typedCtorLocal".to_string(),
        entry_va: 0x806000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.lines = vec![
        "dynamic typedCtorLocal(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "".to_string(),
        "  local_m8 = dart.async.StreamIterator.new(arg0, arg1, arg2, arg3); // stdlib:dart.async.StreamIterator.new [selector], was: sub_9100".to_string(),
        "  return local_m8;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedCtorLocal");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("dart.async.StreamIterator tmp"),
        "constructor semantic call should infer local constructor type:\n{out}"
    );
}

#[test]
fn infers_local_type_from_constructor_semantic_comment() {
    let ir = FunctionIr {
        function_id: 807,
        name: "typedCtorCommentLocal".to_string(),
        entry_va: 0x807000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.lines = vec![
        "dynamic typedCtorCommentLocal(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "".to_string(),
        "  local_m8 = sub_9200(arg0, arg1, arg2, arg3); // stdlib:dart.async.StreamIterator.new [selector], was: sub_9200".to_string(),
        "  return local_m8;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedCtorCommentLocal");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("dart.async.StreamIterator tmp"),
        "constructor semantic comment should infer local constructor type:\n{out}"
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
fn infers_string_type_for_pool_mapped_literal_assignment() {
    let ir = FunctionIr {
        function_id: 810,
        name: "typedPoolLiteral".to_string(),
        entry_va: 0x80a000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.lines = vec![
        "dynamic typedPoolLiteral(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "".to_string(),
        "  local_m8 = \"setState\" /* pool[42] */;".to_string(),
        "  return local_m8;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedPoolLiteral");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("String tmp"),
        "pool-mapped literal assignment should infer String local type:\n{out}"
    );
}

#[test]
fn infers_bool_types_from_if_condition_context() {
    let ir = FunctionIr {
        function_id: 811,
        name: "typedConditionBools".to_string(),
        entry_va: 0x80b000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.lines = vec![
        "dynamic typedConditionBools(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "".to_string(),
        "  if (arg1 && (local_m8 == true)) {".to_string(),
        "    return arg0;".to_string(),
        "  }".to_string(),
        "  return arg0;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedConditionBools");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("bool param1"),
        "if-condition use should infer bool argument type:\n{out}"
    );
    assert!(
        out.contains("bool tmp"),
        "if-condition bool comparison should infer bool local type:\n{out}"
    );
}

#[test]
fn aliases_repeated_pool_mapped_literals() {
    let ir = FunctionIr {
        function_id: 812,
        name: "poolLiteralAlias".to_string(),
        entry_va: 0x80c000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic poolLiteralAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = dispatch.customAction(arg0, \"setState\" /* pool[42] */, arg2, arg3);".to_string(),
        "  final t2 = dispatch.customAction(arg0, \"setState\" /* pool[42] */, arg2, arg3);".to_string(),
        "  final t3 = dispatch.customAction(arg0, \"setState\" /* pool[42] */, arg2, arg3);".to_string(),
        "  return t3;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("poolLiteralAlias");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("final String poolStr42 = \"setState\" /* pool[42] */;"),
        "repeated pool literal should hoist to String alias:\n{out}"
    );
    assert!(
        out.contains("dispatch.customAction(receiver, poolStr42, param2, param3);"),
        "repeated pool literal callsites should use hoisted alias:\n{out}"
    );
    assert_eq!(
        out.matches("\"setState\" /* pool[42] */").count(),
        1,
        "pool literal should appear only in hoisted alias declaration:\n{out}"
    );
}

#[test]
fn does_not_alias_pool_mapped_literal_when_usage_is_sparse() {
    let ir = FunctionIr {
        function_id: 813,
        name: "poolLiteralNoAlias".to_string(),
        entry_va: 0x80d000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic poolLiteralNoAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = dispatch.customAction(arg0, \"setState\" /* pool[42] */, arg2, arg3);".to_string(),
        "  final t2 = dispatch.customAction(arg0, \"setState\" /* pool[42] */, arg2, arg3);".to_string(),
        "  return t2;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("poolLiteralNoAlias");
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("final String poolStr42"),
        "sparse pool literal usage should not create alias noise:\n{out}"
    );
}

#[test]
fn infers_local_types_from_semantic_return_paths() {
    let ir = FunctionIr {
        function_id: 808,
        name: "typedSemanticReturns".to_string(),
        entry_va: 0x808000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.locals.insert(-16, "local_m16".to_string());
    emitter.locals.insert(-24, "local_m24".to_string());
    emitter.locals.insert(-32, "local_m32".to_string());
    emitter.locals.insert(-40, "local_m40".to_string());
    emitter.locals.insert(-48, "local_m48".to_string());
    emitter.locals.insert(-56, "local_m56".to_string());
    emitter.locals.insert(-64, "local_m64".to_string());
    emitter.lines = vec![
        "dynamic typedSemanticReturns(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "  var local_m16;".to_string(),
        "  var local_m24;".to_string(),
        "  var local_m32;".to_string(),
        "  var local_m40;".to_string(),
        "  var local_m48;".to_string(),
        "  var local_m56;".to_string(),
        "  var local_m64;".to_string(),
        "".to_string(),
        "  local_m8 = sub_1000(arg0, arg1, arg2, arg3); // stdlib:dart.core.String.substring [selector], was: sub_1000".to_string(),
        "  local_m16 = sub_1001(arg0, arg1, arg2, arg3); // stdlib:dart.core.String.startsWith [selector], was: sub_1001".to_string(),
        "  local_m24 = sub_1002(arg0, arg1, arg2, arg3); // stdlib:dart.core.String.indexOf [selector], was: sub_1002".to_string(),
        "  local_m32 = sub_1003(arg0, arg1, arg2, arg3); // stdlib:dart.typed_data.ByteData.getFloat64 [selector], was: sub_1003".to_string(),
        "  local_m40 = sub_1004(arg0, arg1, arg2, arg3); // stdlib:dart.core.Object.runtimeType [selector], was: sub_1004".to_string(),
        "  local_m48 = sub_1005(arg0, arg1, arg2, arg3); // stdlib:dart.async.Future.then [selector], was: sub_1005".to_string(),
        "  local_m56 = sub_1006(arg0, arg1, arg2, arg3); // stdlib:dart.async.Stream.listen [selector], was: sub_1006".to_string(),
        "  local_m64 = dart.typed_data.ByteData.getUint32(arg0, arg1, arg2, arg3);".to_string(),
        "  return local_m8;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedSemanticReturns");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("\n  String "),
        "semantic substring return should infer String local type:\n{out}"
    );
    assert!(
        out.contains("\n  bool "),
        "semantic startsWith return should infer bool local type:\n{out}"
    );
    assert!(
        out.contains("\n  int "),
        "semantic index/getUint32 return should infer int local type:\n{out}"
    );
    assert!(
        out.contains("\n  double "),
        "semantic getFloat64 return should infer double local type:\n{out}"
    );
    assert!(
        out.contains("\n  Type "),
        "semantic runtimeType return should infer Type local type:\n{out}"
    );
    assert!(
        out.contains("\n  dart.async.Future "),
        "semantic Future.then return should infer Future local type:\n{out}"
    );
    assert!(
        out.contains("\n  dart.async.StreamSubscription "),
        "semantic Stream.listen return should infer StreamSubscription local type:\n{out}"
    );
}

#[test]
fn infers_local_type_from_constructor_like_fallback_call_path() {
    let ir = FunctionIr {
        function_id: 809,
        name: "typedFallbackCtorLocal".to_string(),
        entry_va: 0x809000,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.locals.insert(-8, "local_m8".to_string());
    emitter.lines = vec![
        "dynamic typedFallbackCtorLocal(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  var local_m8;".to_string(),
        "".to_string(),
        "  local_m8 = AndroidPermission.new(arg0, arg1, arg2, arg3); // selector: AndroidPermission, heuristic: constructor-like selector".to_string(),
        "  return local_m8;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("typedFallbackCtorLocal");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("AndroidPermission tmp"),
        "constructor-like fallback call path should infer local constructor type:\n{out}"
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


/// Page-based pool loads that the disassembler's register tracker could not follow
/// still reach the decompiler as raw `((pool + <page> /* lsl #N */)).f<off>` text.
/// That text carries a byte displacement, not an entry index, and the decompiler has
/// no pool geometry to convert it, so it must surface the displacement and decline
/// to resolve, rather than divide by the stride and land on a neighbouring slot.
#[test]
fn residual_shifted_pool_syntax_reports_displacement_and_does_not_resolve() {
    let ir = FunctionIr {
        function_id: 215,
        name: "shiftedPoolTarget".to_string(),
        entry_va: 0xf640,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf640,
            instrs: vec![
                LlirInstr {
                    va: 0xf640,
                    op: IROp::Other,
                    src: "mov x21, ((pool + 8 /* lsl #12 */)).f3640".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xf644,
                    op: IROp::Call,
                    src: "blr x21".to_string(),
                    target: "x21".to_string(),
                },
                LlirInstr {
                    va: 0xf648,
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
    symbols.insert(0x9100, "dart_core_print".to_string());
    let pool = HashMap::new();
    let mut semantic = HashMap::new();
    semantic.insert(
        4551u64,
        PoolSemanticHint {
            target_va: Some(0x9100),
            ..PoolSemanticHint::default()
        },
    );

    let artifact = emit_pseudocode_with_pool_context(&ir, &symbols, &pool, &semantic);
    // (8 << 12) + 3640 == 36408 bytes from PP.
    assert!(
        artifact.source.contains("poolOff[36408]"),
        "residual shifted pool access should surface its byte displacement:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("pool[4551]"),
        "displacement 36408 must not be reported as entry index 4551; the real entry \
         index depends on pool geometry the decompiler does not have:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dart.core.print"),
        "an unresolvable pool displacement must not pick up a semantic hint:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("pool + 8 /* lsl #12 */"),
        "normalized output should not leak shifted-pool raw syntax:\n{}",
        artifact.source
    );
}

#[test]
fn aliases_repeated_stack_slot_reads() {
    let ir = FunctionIr {
        function_id: 212,
        name: "stackSlotAlias".to_string(),
        entry_va: 0xf620,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic stackSlotAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = fn_0x10(sp[-0x10], arg1, arg2, arg3);".to_string(),
        "  final t2 = fn_0x10(sp[-0x10], arg1, arg2, arg3);".to_string(),
        "  final t3 = fn_0x10(sp[-0x10], arg1, arg2, arg3);".to_string(),
        "  final t4 = fn_0x10(sp[-8], arg1, arg2, arg3);".to_string(),
        "  return t3;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("stackSlotAlias");
    let out = emitter.lines.join("\n");
    assert!(
        out.contains("final stackSlotNeg0x10 = sp[-0x10];"),
        "repeated stack slot should be aliased into a prelude local:\n{out}"
    );
    assert!(
        out.contains("fn_0x10(stackSlotNeg0x10, param1, param2, param3);"),
        "stack slot call arguments should use alias:\n{out}"
    );
    assert_eq!(
        out.matches("sp[-0x10]").count(),
        1,
        "original stack slot token should remain only in alias declaration:\n{out}"
    );
}

#[test]
fn does_not_alias_non_repeated_stack_slot_reads() {
    let ir = FunctionIr {
        function_id: 213,
        name: "stackSlotNoAlias".to_string(),
        entry_va: 0xf628,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic stackSlotNoAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = fn_0x10(sp[-0x10], arg1, arg2, arg3);".to_string(),
        "  final t2 = fn_0x10(sp[-0x10], arg1, arg2, arg3);".to_string(),
        "  return t2;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("stackSlotNoAlias");
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("final stackSlotNeg0x10 = sp[-0x10];"),
        "stack slot alias should not be emitted below repetition threshold:\n{out}"
    );
    assert!(
        out.matches("sp[-0x10]").count() >= 2,
        "non-aliased stack slot reads should remain inline:\n{out}"
    );
}

#[test]
fn does_not_alias_repeated_stack_slot_when_written() {
    let ir = FunctionIr {
        function_id: 214,
        name: "stackSlotWrittenNoAlias".to_string(),
        entry_va: 0xf630,
        blocks: Vec::new(),
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines = vec![
        "dynamic stackSlotWrittenNoAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
        "  final t1 = fn_0x10(sp[-0x60], arg1, arg2, arg3);".to_string(),
        "  sp[-0x60] = t1;".to_string(),
        "  final t2 = fn_0x10(sp[-0x60], arg1, arg2, arg3);".to_string(),
        "  final t3 = fn_0x10(sp[-0x60], arg1, arg2, arg3);".to_string(),
        "  return t3;".to_string(),
        "}".to_string(),
    ];

    emitter.apply_name_and_type_hints("stackSlotWrittenNoAlias");
    let out = emitter.lines.join("\n");
    assert!(
        !out.contains("stackSlotNeg0x60"),
        "written stack slots should not be aliased into immutable locals:\n{out}"
    );
    assert!(
        out.contains("sp[-0x60] = t1;"),
        "stack write site should remain explicit:\n{out}"
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
            .contains("dart.core.print(); // stdlib:dart.core.print, was: dart_core_print"),
        "missing stdlib call intent annotation:\n{}",
        artifact.source
    );
}

#[test]
fn preserves_dart_patch_library_segments_in_call_intent() {
    let ir = FunctionIr {
        function_id: 220,
        name: "dartPatchCall".to_string(),
        entry_va: 0xf710,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf710,
            instrs: vec![
                LlirInstr {
                    va: 0xf710,
                    op: IROp::Call,
                    src: "bl #0x5100".to_string(),
                    target: "#0x5100".to_string(),
                },
                LlirInstr {
                    va: 0xf714,
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
    symbols.insert(0x5100, "dart_core_patch_bool_patch_fromEnvironment".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains(
            "dart.core_patch.bool_patch.fromEnvironment(); // stdlib:dart.core_patch.bool_patch.fromEnvironment, was: dart_core_patch_bool_patch_fromEnvironment"
        ),
        "dart patch library segment should be preserved in semantic direct-call rewrite:\n{}",
        artifact.source
    );
}

#[test]
fn preserves_dart_owner_segment_in_call_intent() {
    let ir = FunctionIr {
        function_id: 221,
        name: "dartOwnerCall".to_string(),
        entry_va: 0xf718,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf718,
            instrs: vec![
                LlirInstr {
                    va: 0xf718,
                    op: IROp::Call,
                    src: "bl #0x5200".to_string(),
                    target: "#0x5200".to_string(),
                },
                LlirInstr {
                    va: 0xf71c,
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
    symbols.insert(0x5200, "dart_typed_data_TypedData_offsetInBytes".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains(
            "dart.typed_data.TypedData.offsetInBytes(); // stdlib:dart.typed_data.TypedData.offsetInBytes, was: dart_typed_data_TypedData_offsetInBytes"
        ),
        "dart owner segment should be preserved in semantic direct-call rewrite:\n{}",
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
            .contains("dart_vm.invoke(); // runtime:dart_vm.invoke, was: vm_runtime_Invoke"),
        "missing runtime call intent annotation:\n{}",
        artifact.source
    );
    assert!(
        artifact
            .source
            .contains("libc.memcpy(); // native:libc.memcpy, was: native_libc_memcpy"),
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
            .contains("flutter.widgets.State.setState(); // framework:flutter.widgets.State.setState, was: flutter_widgets_State_setState"),
        "missing flutter setState intent annotation:\n{}",
        artifact.source
    );
    assert!(
        artifact
            .source
            .contains("flutter.widgets.StatefulWidget.createState(); // framework:flutter.widgets.StatefulWidget.createState, was: flutter_widgets_StatefulWidget_createState"),
        "missing flutter createState intent annotation:\n{}",
        artifact.source
    );
}

#[test]
fn preserves_flutter_class_and_method_tokens_with_underscores() {
    let ir = FunctionIr {
        function_id: 241,
        name: "frameworkUnderscoreCall".to_string(),
        entry_va: 0xf910,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xf910,
            instrs: vec![
                LlirInstr {
                    va: 0xf910,
                    op: IROp::Call,
                    src: "bl #0x6210".to_string(),
                    target: "#0x6210".to_string(),
                },
                LlirInstr {
                    va: 0xf914,
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
    symbols.insert(0x6210, "flutter_widgets_Render_Flex_perform_layout".to_string());
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains(
            "flutter.widgets.Render_Flex.perform_layout(); // framework:flutter.widgets.Render_Flex.perform_layout, was: flutter_widgets_Render_Flex_perform_layout"
        ),
        "flutter intent parsing should preserve underscore-heavy class/method splits:\n{}",
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
            .contains("flutter.widgets.State.setState(2, \"setState\" /* pool[42] */); // framework:flutter.widgets.State.setState [selector], was: sub_6100"),
        "missing selector-based framework annotation:\n{}",
        artifact.source
    );
}
/// A resolved pool slot is a known string wherever it is used, not only where it
/// happens to land in a call argument. Assignments, comparisons and returns used to
/// print the bare `pool[N]` even with the value in hand.
#[test]
fn pool_values_render_as_literals_outside_call_arguments() {
    let ir = FunctionIr {
        function_id: 900,
        name: "poolValueUses".to_string(),
        entry_va: 0x11000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x11000,
            instrs: vec![
                LlirInstr {
                    va: 0x11000,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[40]".to_string(),
                },
                // store to a frame local
                LlirInstr {
                    va: 0x11004,
                    op: IROp::Other,
                    src: "stur x1, [x29, #-8]".to_string(),
                    target: String::new(),
                },
                // compare against another register
                LlirInstr {
                    va: 0x11008,
                    op: IROp::Other,
                    src: "cmp x2, x1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x1100c,
                    op: IROp::Other,
                    src: "mov x0, x1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x11010,
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
    pool.insert(40u64, "onError".to_string());
    let out = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool).source;

    assert!(
        out.contains("= \"onError\" /* pool[40] */;"),
        "pool value assigned to a local should read as the string:\n{out}"
    );
    assert!(
        out.contains("return \"onError\" /* pool[40] */;"),
        "returned pool value should read as the string:\n{out}"
    );
    assert!(
        !out.contains("= pool[40];"),
        "no use should be left as a bare slot when the value is known:\n{out}"
    );
}

/// Dereferencing a pooled object is not the same as using its value. `pool[40].f7`
/// reads a field of the object in slot 40, so rendering the string there would claim a
/// field access on a literal; the slot keeps its inline mapping instead.
#[test]
fn pool_field_access_keeps_the_slot_rather_than_the_literal() {
    let ir = FunctionIr {
        function_id: 901,
        name: "poolFieldUse".to_string(),
        entry_va: 0x12000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x12000,
            instrs: vec![
                LlirInstr {
                    va: 0x12000,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[40]".to_string(),
                },
                LlirInstr {
                    va: 0x12004,
                    op: IROp::Other,
                    src: "ldur x2, [x1, #7]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x12008,
                    op: IROp::Other,
                    src: "stur x2, [x29, #-8]".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x1200c,
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
    pool.insert(40u64, "onError".to_string());
    let out = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool).source;

    assert!(
        !out.contains("\"onError\" /* pool[40] */.f"),
        "a field read must not be rendered as a field of a string literal:\n{out}"
    );
    assert!(
        out.contains("pool[40 /* \"onError\" */].f8"),
        "the field base stays a slot, and the offset is reported untagged:\n{out}"
    );
}

#[test]
fn annotates_package_call_intents_from_machine_symbol_names() {
    let ir = FunctionIr {
        function_id: 26,
        name: "packageCalls".to_string(),
        entry_va: 0xfb00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfb00,
            instrs: vec![
                LlirInstr {
                    va: 0xfb00,
                    op: IROp::Call,
                    src: "bl #0x6300".to_string(),
                    target: "#0x6300".to_string(),
                },
                LlirInstr {
                    va: 0xfb04,
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
    symbols.insert(
        0x6300,
        "package_spotube_ConnectService_executeCommandAsync".to_string(),
    );
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact
            .source
            .contains("spotube.ConnectService.executeCommandAsync(); // package:spotube.ConnectService.executeCommandAsync, was: package_spotube_ConnectService_executeCommandAsync"),
        "missing package call intent annotation:\n{}",
        artifact.source
    );
}

#[test]
fn preserves_package_owner_and_method_tokens_with_underscores() {
    let ir = FunctionIr {
        function_id: 27,
        name: "packageUnderscoreCall".to_string(),
        entry_va: 0xfb08,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfb08,
            instrs: vec![
                LlirInstr {
                    va: 0xfb08,
                    op: IROp::Call,
                    src: "bl #0x6310".to_string(),
                    target: "#0x6310".to_string(),
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
    symbols.insert(
        0x6310,
        "package_spotube_Foo_Bar_internal_init".to_string(),
    );
    let artifact = emit_pseudocode(&ir, &symbols);
    assert!(
        artifact.source.contains(
            "spotube.Foo_Bar.internal_init(); // package:spotube.Foo_Bar.internal_init, was: package_spotube_Foo_Bar_internal_init"
        ),
        "package intent parsing should preserve underscore-heavy owner/method splits:\n{}",
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
            "flutter.scheduler.SchedulerBinding.addPostFrameCallback(\"addPostFrameCallback\" /* pool[7] */); // framework:flutter.scheduler.SchedulerBinding.addPostFrameCallback [selector], was: sub_7000"
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
            "dart.async.Future.catchError(\"catchError\" /* pool[9] */); // stdlib:dart.async.Future.catchError [selector], was: sub_7100"
        ),
        "missing dart async selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_typed_data_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 28,
        name: "typedDataSelector".to_string(),
        entry_va: 0xfd00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfd00,
            instrs: vec![
                LlirInstr {
                    va: 0xfd00,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfd04,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[13]".to_string(),
                },
                LlirInstr {
                    va: 0xfd08,
                    op: IROp::Call,
                    src: "bl #0x7200".to_string(),
                    target: "#0x7200".to_string(),
                },
                LlirInstr {
                    va: 0xfd0c,
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
    symbols.insert(0x7200, "sub_7200".to_string());
    let mut pool = HashMap::new();
    pool.insert(13u64, "offsetInBytes".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.typed_data.TypedData.offsetInBytes(\"offsetInBytes\" /* pool[13] */); // stdlib:dart.typed_data.TypedData.offsetInBytes [selector], was: sub_7200"
        ),
        "missing typed_data selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_typed_data_native_set_float32x4_internal_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 45,
        name: "typedDataSetFloat32x4Selector".to_string(),
        entry_va: 0xfd40,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfd40,
            instrs: vec![
                LlirInstr {
                    va: 0xfd40,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfd44,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[28]".to_string(),
                },
                LlirInstr {
                    va: 0xfd48,
                    op: IROp::Call,
                    src: "bl #0x7240".to_string(),
                    target: "#0x7240".to_string(),
                },
                LlirInstr {
                    va: 0xfd4c,
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
    symbols.insert(0x7240, "sub_7240".to_string());
    let mut pool = HashMap::new();
    pool.insert(28u64, "_nativeSetFloat32x4".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.typed_data.ByteData.setFloat32x4(\"_nativeSetFloat32x4\" /* pool[28] */); // stdlib:dart.typed_data.ByteData.setFloat32x4 [selector], was: sub_7240"
        ),
        "missing typed_data native setFloat32x4 selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_typed_data_unmodifiable_uint8_array_view_internal_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 46,
        name: "unmodifiableUint8ArrayViewSelector".to_string(),
        entry_va: 0xfd50,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfd50,
            instrs: vec![
                LlirInstr {
                    va: 0xfd50,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfd54,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[29]".to_string(),
                },
                LlirInstr {
                    va: 0xfd58,
                    op: IROp::Call,
                    src: "bl #0x7250".to_string(),
                    target: "#0x7250".to_string(),
                },
                LlirInstr {
                    va: 0xfd5c,
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
    symbols.insert(0x7250, "sub_7250".to_string());
    let mut pool = HashMap::new();
    pool.insert(29u64, "_UnmodifiableUint8ArrayView".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.typed_data._UnmodifiableUint8ArrayView.new(\"_UnmodifiableUint8ArrayView\" /* pool[29] */); // stdlib:dart.typed_data._UnmodifiableUint8ArrayView.new [selector], was: sub_7250"
        ),
        "missing typed_data unmodifiable uint8 array view selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_typed_data_int32_array_view_internal_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 47,
        name: "int32ArrayViewSelector".to_string(),
        entry_va: 0xfd60,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfd60,
            instrs: vec![
                LlirInstr {
                    va: 0xfd60,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfd64,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[30]".to_string(),
                },
                LlirInstr {
                    va: 0xfd68,
                    op: IROp::Call,
                    src: "bl #0x7260".to_string(),
                    target: "#0x7260".to_string(),
                },
                LlirInstr {
                    va: 0xfd6c,
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
    symbols.insert(0x7260, "sub_7260".to_string());
    let mut pool = HashMap::new();
    pool.insert(30u64, "_Int32ArrayView".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.typed_data._Int32ArrayView.new(\"_Int32ArrayView\" /* pool[30] */); // stdlib:dart.typed_data._Int32ArrayView.new [selector], was: sub_7260"
        ),
        "missing typed_data int32 array view selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_core_match_end_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 33,
        name: "matchEndSelector".to_string(),
        entry_va: 0xfd80,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfd80,
            instrs: vec![
                LlirInstr {
                    va: 0xfd80,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfd84,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[21]".to_string(),
                },
                LlirInstr {
                    va: 0xfd88,
                    op: IROp::Call,
                    src: "bl #0x7280".to_string(),
                    target: "#0x7280".to_string(),
                },
                LlirInstr {
                    va: 0xfd8c,
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
    symbols.insert(0x7280, "sub_7280".to_string());
    let mut pool = HashMap::new();
    pool.insert(21u64, "match_end_index".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core.Match.end(\"match_end_index\" /* pool[21] */); // stdlib:dart.core.Match.end [selector], was: sub_7280"
        ),
        "missing dart core match end selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_io_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 29,
        name: "ioSelector".to_string(),
        entry_va: 0xfe00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xfe00,
            instrs: vec![
                LlirInstr {
                    va: 0xfe00,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xfe04,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[14]".to_string(),
                },
                LlirInstr {
                    va: 0xfe08,
                    op: IROp::Call,
                    src: "bl #0x7300".to_string(),
                    target: "#0x7300".to_string(),
                },
                LlirInstr {
                    va: 0xfe0c,
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
    symbols.insert(0x7300, "sub_7300".to_string());
    let mut pool = HashMap::new();
    pool.insert(14u64, "supportsAnsiEscapes".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.io.Stdout.supportsAnsiEscapes(\"supportsAnsiEscapes\" /* pool[14] */); // stdlib:dart.io.Stdout.supportsAnsiEscapes [selector], was: sub_7300"
        ),
        "missing dart:io selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_native_prefixed_typed_data_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 30,
        name: "nativeTypedDataSelector".to_string(),
        entry_va: 0xff00,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xff00,
            instrs: vec![
                LlirInstr {
                    va: 0xff00,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0xff04,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[15]".to_string(),
                },
                LlirInstr {
                    va: 0xff08,
                    op: IROp::Call,
                    src: "bl #0x7400".to_string(),
                    target: "#0x7400".to_string(),
                },
                LlirInstr {
                    va: 0xff0c,
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
    symbols.insert(0x7400, "sub_7400".to_string());
    let mut pool = HashMap::new();
    pool.insert(15u64, "nativeSetFloat32".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.typed_data.ByteData.setFloat32(\"nativeSetFloat32\" /* pool[15] */); // stdlib:dart.typed_data.ByteData.setFloat32 [selector], was: sub_7400"
        ),
        "missing native-prefixed typed_data selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_runtime_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 31,
        name: "runtimeSelector".to_string(),
        entry_va: 0x10000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10000,
            instrs: vec![
                LlirInstr {
                    va: 0x10000,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10004,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[16]".to_string(),
                },
                LlirInstr {
                    va: 0x10008,
                    op: IROp::Call,
                    src: "bl #0x7500".to_string(),
                    target: "#0x7500".to_string(),
                },
                LlirInstr {
                    va: 0x1000c,
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
    symbols.insert(0x7500, "sub_7500".to_string());
    let mut pool = HashMap::new();
    pool.insert(16u64, "yieldStarIterable".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart_vm.yieldStarIterable(\"yieldStarIterable\" /* pool[16] */); // runtime:dart_vm.yieldStarIterable [selector], was: sub_7500"
        ),
        "missing runtime selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_flutter_internal_list_equals_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 41,
        name: "listEqualsSelector".to_string(),
        entry_va: 0x10080,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10080,
            instrs: vec![
                LlirInstr {
                    va: 0x10080,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10084,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[24]".to_string(),
                },
                LlirInstr {
                    va: 0x10088,
                    op: IROp::Call,
                    src: "bl #0x7900".to_string(),
                    target: "#0x7900".to_string(),
                },
                LlirInstr {
                    va: 0x1008c,
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
    symbols.insert(0x7900, "sub_7900".to_string());
    let mut pool = HashMap::new();
    pool.insert(24u64, "_listEquals".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "flutter.foundation.listEquals(\"_listEquals\" /* pool[24] */); // framework:flutter.foundation.listEquals [selector], was: sub_7900"
        ),
        "missing flutter internal listEquals selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_runtime_internal_prepend_type_arguments_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 42,
        name: "prependTypeArgsSelector".to_string(),
        entry_va: 0x10090,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10090,
            instrs: vec![
                LlirInstr {
                    va: 0x10090,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10094,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[25]".to_string(),
                },
                LlirInstr {
                    va: 0x10098,
                    op: IROp::Call,
                    src: "bl #0x7910".to_string(),
                    target: "#0x7910".to_string(),
                },
                LlirInstr {
                    va: 0x1009c,
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
    symbols.insert(0x7910, "sub_7910".to_string());
    let mut pool = HashMap::new();
    pool.insert(25u64, "_prependTypeArguments".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart_vm.prependTypeArguments(\"_prependTypeArguments\" /* pool[25] */); // runtime:dart_vm.prependTypeArguments [selector], was: sub_7910"
        ),
        "missing runtime internal prependTypeArguments selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_async_stream_controller_internal_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 43,
        name: "streamControllerSelector".to_string(),
        entry_va: 0x100a0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x100a0,
            instrs: vec![
                LlirInstr {
                    va: 0x100a0,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x100a4,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[26]".to_string(),
                },
                LlirInstr {
                    va: 0x100a8,
                    op: IROp::Call,
                    src: "bl #0x7920".to_string(),
                    target: "#0x7920".to_string(),
                },
                LlirInstr {
                    va: 0x100ac,
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
    symbols.insert(0x7920, "sub_7920".to_string());
    let mut pool = HashMap::new();
    pool.insert(26u64, "_StreamController".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.async.StreamController.new(\"_StreamController\" /* pool[26] */); // stdlib:dart.async.StreamController.new [selector], was: sub_7920"
        ),
        "missing internal StreamController selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_io_raw_datagram_socket_internal_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 44,
        name: "rawDatagramSocketSelector".to_string(),
        entry_va: 0x100b0,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x100b0,
            instrs: vec![
                LlirInstr {
                    va: 0x100b0,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x100b4,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[27]".to_string(),
                },
                LlirInstr {
                    va: 0x100b8,
                    op: IROp::Call,
                    src: "bl #0x7930".to_string(),
                    target: "#0x7930".to_string(),
                },
                LlirInstr {
                    va: 0x100bc,
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
    symbols.insert(0x7930, "sub_7930".to_string());
    let mut pool = HashMap::new();
    pool.insert(27u64, "_RawDatagramSocket".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.io.RawDatagramSocket.new(\"_RawDatagramSocket\" /* pool[27] */); // stdlib:dart.io.RawDatagramSocket.new [selector], was: sub_7930"
        ),
        "missing internal RawDatagramSocket selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_core_compile_time_error_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 32,
        name: "compileTimeErrorSelector".to_string(),
        entry_va: 0x10100,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10100,
            instrs: vec![
                LlirInstr {
                    va: 0x10100,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10104,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[17]".to_string(),
                },
                LlirInstr {
                    va: 0x10108,
                    op: IROp::Call,
                    src: "bl #0x7600".to_string(),
                    target: "#0x7600".to_string(),
                },
                LlirInstr {
                    va: 0x1010c,
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
    symbols.insert(0x7600, "sub_7600".to_string());
    let mut pool = HashMap::new();
    pool.insert(17u64, "_CompileTimeError".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.core._CompileTimeError.new(\"_CompileTimeError\" /* pool[17] */); // stdlib:dart.core._CompileTimeError.new [selector], was: sub_7600"
        ),
        "missing compile-time-error selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_dart_io_native_socket_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 33,
        name: "nativeSocketSelector".to_string(),
        entry_va: 0x10200,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10200,
            instrs: vec![
                LlirInstr {
                    va: 0x10200,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10204,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[18]".to_string(),
                },
                LlirInstr {
                    va: 0x10208,
                    op: IROp::Call,
                    src: "bl #0x7700".to_string(),
                    target: "#0x7700".to_string(),
                },
                LlirInstr {
                    va: 0x1020c,
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
    symbols.insert(0x7700, "sub_7700".to_string());
    let mut pool = HashMap::new();
    pool.insert(18u64, "_NativeSocket".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart.io._NativeSocket.new(\"_NativeSocket\" /* pool[18] */); // stdlib:dart.io._NativeSocket.new [selector], was: sub_7700"
        ),
        "missing native-socket selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_runtime_closure_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 34,
        name: "closureSelector".to_string(),
        entry_va: 0x10300,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10300,
            instrs: vec![
                LlirInstr {
                    va: 0x10300,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10304,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[19]".to_string(),
                },
                LlirInstr {
                    va: 0x10308,
                    op: IROp::Call,
                    src: "bl #0x7800".to_string(),
                    target: "#0x7800".to_string(),
                },
                LlirInstr {
                    va: 0x1030c,
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
    symbols.insert(0x7800, "sub_7800".to_string());
    let mut pool = HashMap::new();
    pool.insert(19u64, "_Closure".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart_vm.Closure.new(\"_Closure\" /* pool[19] */); // runtime:dart_vm.Closure.new [selector], was: sub_7800"
        ),
        "missing closure runtime selector annotation:\n{}",
        artifact.source
    );
}

#[test]
fn annotates_runtime_type_parameter_selector_from_pool_string() {
    let ir = FunctionIr {
        function_id: 35,
        name: "typeParameterSelector".to_string(),
        entry_va: 0x10400,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x10400,
            instrs: vec![
                LlirInstr {
                    va: 0x10400,
                    op: IROp::Other,
                    src: "mov x0, #1".to_string(),
                    target: String::new(),
                },
                LlirInstr {
                    va: 0x10404,
                    op: IROp::LoadPool,
                    src: "x1".to_string(),
                    target: "pool[20]".to_string(),
                },
                LlirInstr {
                    va: 0x10408,
                    op: IROp::Call,
                    src: "bl #0x7900".to_string(),
                    target: "#0x7900".to_string(),
                },
                LlirInstr {
                    va: 0x1040c,
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
    symbols.insert(0x7900, "sub_7900".to_string());
    let mut pool = HashMap::new();
    pool.insert(20u64, "_TypeParameter".to_string());
    let artifact = emit_pseudocode_with_pool_hints(&ir, &symbols, &pool);
    assert!(
        artifact.source.contains(
            "dart_vm.TypeParameter.new(\"_TypeParameter\" /* pool[20] */); // runtime:dart_vm.TypeParameter.new [selector], was: sub_7900"
        ),
        "missing type-parameter runtime selector annotation:\n{}",
        artifact.source
    );
}
