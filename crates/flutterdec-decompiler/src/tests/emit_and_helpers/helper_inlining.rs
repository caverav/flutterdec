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

/// A pre- or post-indexed access writes its base register back, so the base is a
/// destination even for a store, which otherwise writes nothing at all.
///
/// This feeds the join merge through `dfs_block_writes`, so a missed base write
/// left that register's binding alive across a join that had redefined it. 2,346
/// and 1,394 such instructions on the two sample binaries have a base outside the
/// pinned set, where the omission is observable.
#[test]
fn a_writeback_access_writes_its_base_register() {
    let cases = [
        // pre-indexed: offset inside the brackets, writeback marked with `!`
        ("str x1, [x0, #8]!", vec!["x0"]),
        ("stp x2, x3, [x15, #-0x10]!", vec!["x15"]),
        // post-indexed: brackets close first, offset is the next operand
        ("ldr x1, [x0], #8", vec!["x0", "x1"]),
        ("ldp x1, x2, [x0], #16", vec!["x0", "x1", "x2"]),
        // no writeback: a plain store writes no register, a plain load its dest
        ("str x1, [x0, #8]", vec![]),
        ("ldr x1, [x0, #8]", vec!["x1"]),
        ("stp x2, x3, [x15, #16]", vec![]),
    ];
    for (src, expected) in cases {
        let (mnemonic, ops) = split_instruction(src);
        let mut got = written_registers(&mnemonic, &ops);
        got.sort();
        got.dedup();
        let mut want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(got, want, "written registers for `{src}`");
    }
}
