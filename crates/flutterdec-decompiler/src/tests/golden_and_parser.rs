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
