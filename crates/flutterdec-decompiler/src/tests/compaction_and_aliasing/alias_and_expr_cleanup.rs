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
fn simplifies_wrapped_field_access_chains() {
    let line = "((((obj1.f15)).f7)).f23".to_string();
    let got = FuncEmitter::clean_expr(line);
    assert_eq!(got, "obj1.f15.f7.f23");
}

#[test]
fn keeps_parentheses_for_non_member_field_base() {
    let line = "((arg0 + 1)).f7".to_string();
    let got = FuncEmitter::clean_expr(line);
    assert_eq!(got, "((arg0 + 1)).f7");
}
/// Recovered pool strings are program data and routinely contain the punctuation the
/// expression rewrites look for. A real one from a Flutter engine build reads
/// "... has been collected (nullptr). This is ...", which the member-access
/// simplifier used to shorten to "collected nullptr." inside the quotes.
#[test]
fn clean_expr_does_not_rewrite_inside_string_literals() {
    let line =
        "\"a native peer has been collected (nullptr). This is usually a bug\"".to_string();
    assert_eq!(FuncEmitter::clean_expr(line.clone()), line);
}

#[test]
fn clean_expr_still_simplifies_around_a_string_literal() {
    let line = "f(\"(x).y\", ((arg0)).f7)".to_string();
    let got = FuncEmitter::clean_expr(line);
    assert!(
        got.contains("\"(x).y\""),
        "literal must survive verbatim: {got}"
    );
    assert!(
        got.contains("arg0.f7"),
        "code outside the literal should still simplify: {got}"
    );
}
