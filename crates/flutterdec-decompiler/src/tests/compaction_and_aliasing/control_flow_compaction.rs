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
        !out.contains("retryLoop1 = false;"),
        "dead retry fall-through update should be removed:\n{out}"
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

