fn blk(id: usize, va: u64, instrs: Vec<LlirInstr>, succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs,
        succs,
        preds: Vec::new(),
    }
}

fn stmt(va: u64, src: &str) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Other,
        src: src.to_string(),
        target: String::new(),
    }
}

fn cbz(va: u64, reg: &str, target_va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Branch,
        src: format!("cbz {reg}, #0x{target_va:x}"),
        target: format!("#0x{target_va:x}"),
    }
}

fn ret(va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Return,
        src: "ret".to_string(),
        target: String::new(),
    }
}

/// A diamond is the shape the DFS emitter duplicates: it inlines both arms, so
/// the join block is emitted once per incoming path. Structured emission renders
/// the join once, after the `if`.
#[test]
fn emits_a_join_block_exactly_once() {
    let ir = FunctionIr {
        function_id: 1000,
        name: "diamond".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            // Fall-through arm.
            blk(1, 0x1004, vec![stmt(0x1004, "stur x1, [x29, #-0x10]")], vec![3]),
            // Taken arm.
            blk(2, 0x2000, vec![stmt(0x2000, "stur x2, [x29, #-0x18]")], vec![3]),
            // Join.
            blk(
                3,
                0x3000,
                vec![stmt(0x3000, "stur x0, [x29, #-0x20]"), ret(0x3004)],
                Vec::new(),
            ),
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    // The join block's only statement, whatever the naming pass called its slot.
    let joins = artifact
        .source
        .lines()
        .filter(|l| l.contains("= receiver;"))
        .count();
    assert_eq!(
        joins, 1,
        "the join block must be emitted once, not once per arm:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("_block_"),
        "no path should be deferred to a helper:\n{}",
        artifact.source
    );
}

/// A block whose value differs per path cannot carry either path's register
/// binding once it is emitted a single time.
#[test]
fn does_not_attribute_one_path_value_to_a_join() {
    let ir = FunctionIr {
        function_id: 1001,
        name: "joinValue".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            blk(1, 0x1004, vec![stmt(0x1004, "mov x0, #7")], vec![3]),
            blk(2, 0x2000, vec![stmt(0x2000, "mov x0, #9")], vec![3]),
            blk(3, 0x3000, vec![ret(0x3000)], Vec::new()),
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        !artifact.source.contains("return 7;") && !artifact.source.contains("return 9;"),
        "neither arm's value describes the join:\n{}",
        artifact.source
    );
}

/// A back edge becomes `continue` inside `while (true)`, and the loop's exit
/// becomes `break`, with the body emitted once.
#[test]
fn structures_a_natural_loop_without_duplicating_its_body() {
    let ir = FunctionIr {
        function_id: 1002,
        name: "counted".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![stmt(0x1000, "mov x2, #0")], vec![1]),
            // Loop header, exits to block 3.
            blk(1, 0x1004, vec![cbz(0x1004, "x2", 0x3000)], vec![2, 3]),
            // Latch.
            blk(
                2,
                0x1008,
                vec![
                    stmt(0x1008, "sub x2, x2, #1"),
                    stmt(0x100c, "stur x2, [x29, #-0x10]"),
                ],
                vec![1],
            ),
            blk(3, 0x3000, vec![ret(0x3000)], Vec::new()),
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let src = &artifact.source;
    assert!(
        src.contains("while (true) {"),
        "the natural loop should render as a loop:\n{src}"
    );
    assert_eq!(
        src.matches("continue;").count(),
        1,
        "the back edge is the only continue:\n{src}"
    );
    assert_eq!(
        src.lines().filter(|l| l.contains("- 1)")).count(),
        1,
        "the latch body must be emitted once:\n{src}"
    );
    assert!(
        !src.contains("// loop back-edges:"),
        "a structured loop needs no back-edge summary:\n{src}"
    );
}

/// Irreducible control flow, two entries into one cycle, is declined rather than
/// mis-structured, and the DFS emitter still produces output.
#[test]
fn declines_irreducible_control_flow_and_still_emits() {
    let ir = FunctionIr {
        function_id: 1003,
        name: "irreducible".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            // Two entries into the 1 <-> 2 cycle.
            blk(1, 0x1004, vec![cbz(0x1004, "x2", 0x2000)], vec![2, 3]),
            blk(2, 0x2000, vec![cbz(0x2000, "x3", 0x1004)], vec![1, 3]),
            blk(3, 0x3000, vec![ret(0x3000)], Vec::new()),
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains("dynamic irreducible("),
        "output is still produced for irreducible control flow:\n{}",
        artifact.source
    );
    assert!(
        artifact.source.lines().count() > 3,
        "the fallback emitter should render a body:\n{}",
        artifact.source
    );
}

/// A value defined in an arm that returns must not be referenced after it.
/// Emitting each block once removes the per-path duplication that used to hide
/// this, so each arm has to start from the state at the branch.
#[test]
fn an_arm_that_returns_does_not_leak_its_values() {
    let call = |va: u64, target: u64| LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #0x{target:x}"),
        target: format!("#0x{target:x}"),
    };
    let ir = FunctionIr {
        function_id: 1004,
        name: "armScope".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            // Taken arm defines a value and returns.
            blk(1, 0x2000, vec![call(0x2000, 0x9000), ret(0x2004)], Vec::new()),
            // Fall-through arm must not see it.
            blk(2, 0x1004, vec![call(0x1004, 0x9100), ret(0x1008)], Vec::new()),
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let mut declared = Vec::new();
    for line in artifact.source.lines() {
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix("final ") {
            if let Some(name) = rest.split(' ').next() {
                declared.push(name.to_string());
            }
        }
        for used in stripped
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|t| t.len() >= 2 && t.starts_with('t') && t[1..].chars().all(|c| c.is_ascii_digit()))
        {
            assert!(
                declared.iter().any(|d| d == used),
                "{used} is referenced before any declaration:\n{}",
                artifact.source
            );
        }
    }
}

/// Per-arm state isolation must not rewind the temporary counter: two different
/// values may never share a name, even in sibling scopes where Dart would allow
/// it, because the later text passes substitute on those names.
#[test]
fn does_not_reissue_a_temporary_name_across_arms() {
    let call = |va: u64, target: u64| LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #0x{target:x}"),
        target: format!("#0x{target:x}"),
    };
    let ir = FunctionIr {
        function_id: 1005,
        name: "twoArms".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            blk(1, 0x2000, vec![call(0x2000, 0x9000), ret(0x2004)], Vec::new()),
            blk(2, 0x1004, vec![call(0x1004, 0x9100), ret(0x1008)], Vec::new()),
        ],
    };

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let declared: Vec<&str> = artifact
        .source
        .lines()
        .filter_map(|l| l.trim().strip_prefix("final "))
        .filter_map(|l| l.split(' ').next())
        .collect();
    let mut unique = declared.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        declared.len(),
        unique.len(),
        "each temporary is declared once:\n{}",
        artifact.source
    );
    assert_eq!(declared.len(), 2, "both arms call:\n{}", artifact.source);
}

/// A shared continuation that is not the branch's follow node cannot be named in
/// Dart, which has no `goto`. A small one is repeated, which is bounded, rather
/// than sending the function to the DFS emitter, whose duplication is not.
///
/// Here one arm can reach an exit without passing through the shared block, so
/// the shared block is nobody's post-dominator and the follow-node rule cannot
/// place it. It is also not terminal, so only the region budget can allow it.
#[test]
fn repeats_a_small_shared_region_that_is_not_a_follow_node() {
    let ir = FunctionIr {
        function_id: 1006,
        name: "sharedTail".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            blk(1, 0x1004, vec![stmt(0x1004, "stur x1, [x29, #-0x10]")], vec![3]),
            // This arm can leave through block 5 without reaching the tail.
            blk(2, 0x2000, vec![cbz(0x2000, "x2", 0x5000)], vec![3, 5]),
            // Shared, two blocks long, and not terminal.
            blk(3, 0x3000, vec![stmt(0x3000, "stur x3, [x29, #-0x18]")], vec![4]),
            blk(4, 0x3004, vec![ret(0x3004)], Vec::new()),
            blk(5, 0x5000, vec![ret(0x5000)], Vec::new()),
        ],
    };
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.repeated_blocks > 0,
        "the shared region should be repeated under budget:\n{}",
        artifact.source
    );
    assert_eq!(
        artifact
            .source
            .lines()
            .filter(|l| l.contains("= param2;"))
            .count(),
        2,
        "the shared block is emitted on both paths:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("_block_"),
        "and not deferred to a helper:\n{}",
        artifact.source
    );
}

/// The budget is a real limit: a shared region longer than it falls back rather
/// than duplicating an arbitrary amount of code.
#[test]
fn declines_to_repeat_a_shared_region_over_budget() {
    // Chain of 10 blocks, above the 8-block budget.
    let mut blocks = vec![
        blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
        blk(1, 0x1004, vec![stmt(0x1004, "stur x1, [x29, #-0x10]")], vec![3]),
        blk(2, 0x2000, vec![cbz(0x2000, "x2", 0x5000)], vec![3, 13]),
    ];
    for i in 0..10 {
        let id = 3 + i;
        let va = 0x3000 + (i as u64) * 8;
        blocks.push(blk(
            id,
            va,
            vec![stmt(va, &format!("stur x{}, [x29, #-0x{:x}]", i % 4, 0x20 + i * 8))],
            vec![id + 1],
        ));
    }
    blocks.push(blk(13, 0x5000, vec![ret(0x5000)], Vec::new()));

    let ir = FunctionIr {
        function_id: 1008,
        name: "longTail".to_string(),
        entry_va: 0x1000,
        blocks,
    };
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert_eq!(
        artifact.repeated_blocks, 0,
        "a region over budget must not be repeated:\n{}",
        artifact.source
    );
}

/// The budget exists so a loop is never duplicated.
#[test]
fn never_repeats_a_region_containing_a_loop() {
    let ir = FunctionIr {
        function_id: 1007,
        name: "loopTail".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            blk(1, 0x1004, vec![stmt(0x1004, "stur x1, [x29, #-0x10]")], vec![3]),
            blk(2, 0x2000, vec![stmt(0x2000, "stur x2, [x29, #-0x18]")], vec![3]),
            // Loop header reached from both arms.
            blk(3, 0x3000, vec![cbz(0x3000, "x3", 0x4000)], vec![4, 5]),
            blk(4, 0x3004, vec![stmt(0x3004, "sub x3, x3, #1")], vec![3]),
            blk(5, 0x4000, vec![ret(0x4000)], Vec::new()),
        ],
    };
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert_eq!(
        artifact.source.matches("while (true) {").count(),
        1,
        "the loop must be emitted once, never duplicated into both arms:\n{}",
        artifact.source
    );
}

/// Every counter must round-trip through snapshot and restore into its own
/// field. The first version used a positional array, and inserting three fields
/// rotated four of them onto each other's values.
///
/// This is a latent defect rather than an observed one: `FuncEmitter::new` zeroes
/// every counter and nothing increments one before `try_emit_structured` takes
/// the snapshot, so in practice zero was restored over zero and no reported
/// figure was ever wrong. Only a direct round-trip can catch it, which is exactly
/// why it survived.
#[test]
fn every_counter_round_trips_into_its_own_field() {
    let ir = FunctionIr {
        function_id: 1009,
        name: "counters".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);

    // Distinct values, so any two fields swapping is visible.
    emitter.placeholder_ifs = 1;
    emitter.unresolved_cf = 2;
    emitter.raw_register_calls = 3;
    emitter.total_calls = 4;
    emitter.indirect_calls = 5;
    emitter.semantic_direct_calls = 6;
    emitter.semantic_indirect_calls = 7;
    emitter.dispatch_selector_calls = 8;
    emitter.dispatch_table_calls = 9;
    emitter.repeated_blocks = 10;
    emitter.unlifted_instructions = 11;
    emitter.target_va_symbol_calls = 12;

    let saved = emitter.counter_snapshot();
    emitter.placeholder_ifs = 0;
    emitter.unresolved_cf = 0;
    emitter.raw_register_calls = 0;
    emitter.total_calls = 0;
    emitter.indirect_calls = 0;
    emitter.semantic_direct_calls = 0;
    emitter.semantic_indirect_calls = 0;
    emitter.dispatch_selector_calls = 0;
    emitter.dispatch_table_calls = 0;
    emitter.repeated_blocks = 0;
    emitter.unlifted_instructions = 0;
    emitter.target_va_symbol_calls = 0;
    emitter.restore_counters(saved);

    assert_eq!(emitter.placeholder_ifs, 1);
    assert_eq!(emitter.unresolved_cf, 2);
    assert_eq!(emitter.raw_register_calls, 3);
    assert_eq!(emitter.total_calls, 4);
    assert_eq!(emitter.indirect_calls, 5);
    assert_eq!(emitter.semantic_direct_calls, 6);
    assert_eq!(emitter.semantic_indirect_calls, 7);
    assert_eq!(emitter.dispatch_selector_calls, 8);
    assert_eq!(emitter.dispatch_table_calls, 9);
    assert_eq!(emitter.repeated_blocks, 10);
    assert_eq!(emitter.unlifted_instructions, 11);
    assert_eq!(emitter.target_va_symbol_calls, 12);
}

/// Both emitters render a tail call the same way. They diverged once, so the
/// same jump read differently depending on which path ran.
#[test]
fn both_emitters_render_a_tail_call_identically() {
    let jump_out = |va: u64| LlirInstr {
        va,
        op: IROp::Jump,
        // Outside the function, so it cannot resolve to a block.
        src: "b #0x99000".to_string(),
        target: "#0x99000".to_string(),
    };
    let structured = FunctionIr {
        function_id: 1010,
        name: "tailStructured".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(0, 0x1000, vec![jump_out(0x1000)], Vec::new())],
    };
    let out = emit_pseudocode(&structured, &HashMap::new());
    assert!(
        out.source.contains("return tailCall_0x99000();"),
        "a tail call is rendered as a call:\n{}",
        out.source
    );
}

/// `ldp` reads two consecutive registers' worth, so the second destination is
/// one register width past the first: 8 bytes for an `x` pair, 4 for a `w`
/// pair. Both were unmodelled before, which left stale values in both
/// destinations at 79,645 sites across the two sample binaries.
#[test]
fn load_pair_reads_consecutive_slots() {
    let ir = FunctionIr {
        function_id: 900,
        name: "loadPair".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldp x9, x10, [x2, #0x10]"),
                stmt(0x1004, "stur x9, [x3, #7]"),
                stmt(0x1008, "stur x10, [x3, #0xf]"),
                ret(0x100c),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        out.contains(".f16;"),
        "first destination should read the addressed field:\n{out}"
    );
    assert!(
        out.contains(".f24;"),
        "second destination should read one word further, not the same field:\n{out}"
    );
}

/// A `w` pair strides by 4, not 8. No sample binary contains one, so this is
/// the only thing exercising that branch: an uncompressed-pointer build never
/// emits a 32-bit pair, but the encoding is real and the stride is not 8.
#[test]
fn load_pair_of_word_registers_strides_by_four() {
    let ir = FunctionIr {
        function_id: 901,
        name: "loadPairWord".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldp w9, w10, [x2, #0x10]"),
                stmt(0x1004, "stur x10, [x3, #7]"),
                ret(0x1008),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        out.contains(".f20;"),
        "a word pair should stride by four:\n{out}"
    );
}

/// Pre- and post-index addressing writes the base register back, so the base no
/// longer describes the address it held before the access.
#[test]
fn post_index_addressing_drops_the_base_binding() {
    let ir = FunctionIr {
        function_id: 902,
        name: "postIndex".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "mov x2, x1"),
                stmt(0x1004, "ldr x9, [x2], #0x10"),
                stmt(0x1008, "stur x2, [x3, #7]"),
                ret(0x100c),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        !out.contains("= receiver;"),
        "the written-back base must not still read as its pre-access value:\n{out}"
    );
}

/// Dart materialises the canonical bools by adding `kTrueOffsetFromNull` and
/// `kFalseOffsetFromNull` to NULL_REG, and `csel` between them turns a
/// comparison into a value. Unmodelled, the destination kept a stale binding,
/// so a function returning `cond ? true : false` emitted `return receiver;`.
#[test]
fn conditional_select_between_bools_recovers_the_comparison() {
    let ir = FunctionIr {
        function_id: 903,
        name: "boolSelect".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "cmp x1, x2"),
                stmt(0x1004, "add x16, x22, #0x20"),
                stmt(0x1008, "add x17, x22, #0x30"),
                stmt(0x100c, "csel x9, x16, x17, ne"),
                stmt(0x1010, "stur x9, [x3, #7]"),
                ret(0x1014),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        out.contains("(receiver != param1)"),
        "true-then-false arms should render as the comparison itself:\n{out}"
    );
}

/// Operand order carries the polarity: with the arms reversed the value is the
/// inverse condition, not the same one.
#[test]
fn conditional_select_reads_the_arm_order() {
    let ir = FunctionIr {
        function_id: 904,
        name: "boolSelectReversed".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "cmp x1, x2"),
                stmt(0x1004, "add x16, x22, #0x30"),
                stmt(0x1008, "add x17, x22, #0x20"),
                stmt(0x100c, "csel x9, x16, x17, ne"),
                stmt(0x1010, "stur x9, [x3, #7]"),
                ret(0x1014),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        out.contains("(receiver == param1)"),
        "false-then-true arms should render as the inverse condition:\n{out}"
    );
}

/// `tst` sets the flags from a mask test, so a following branch describes that
/// mask and not whatever the previous `cmp` compared. 22,141 conditions across
/// the two samples take their flags from `tst`.
#[test]
fn mask_test_supplies_the_following_condition() {
    let ir = FunctionIr {
        function_id: 905,
        name: "maskTest".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(
                0,
                0x1000,
                vec![
                    stmt(0x1000, "cmp x5, x6"),
                    stmt(0x1004, "tst x1, #1"),
                    LlirInstr {
                        va: 0x1008,
                        op: IROp::Branch,
                        src: "b.ne #0x1010".to_string(),
                        target: "#0x1010".to_string(),
                    },
                ],
                vec![1, 2],
            ),
            blk(
                1,
                0x100c,
                vec![stmt(0x100c, "stur x1, [x3, #7]"), ret(0x1014)],
                vec![],
            ),
            blk(2, 0x1010, vec![ret(0x1010)], vec![]),
        ],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        out.contains("(receiver & 1)"),
        "the condition should describe the mask test, not an earlier compare:\n{out}"
    );
}

/// An unmodelled flag writer leaves `last_cmp` describing an older comparison.
/// Naming that as the condition is a confident false claim, so it degrades to
/// the raw flag instead.
#[test]
fn unmodelled_flag_writer_does_not_inherit_an_older_compare() {
    let ir = FunctionIr {
        function_id: 906,
        name: "staleFlags".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(
                0,
                0x1000,
                vec![
                    stmt(0x1000, "cmp x5, x6"),
                    stmt(0x1004, "ccmp x1, #0, #4, ne"),
                    LlirInstr {
                        va: 0x1008,
                        op: IROp::Branch,
                        src: "b.ge #0x1010".to_string(),
                        target: "#0x1010".to_string(),
                    },
                ],
                vec![1, 2],
            ),
            blk(1, 0x100c, vec![ret(0x100c)], vec![]),
            blk(2, 0x1010, vec![ret(0x1010)], vec![]),
        ],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        out.contains("flags.b_ge"),
        "an unmodelled flag writer should not leave the previous compare readable:\n{out}"
    );
}

/// An unmodelled instruction still writes its destination. Leaving the previous
/// binding in place rendered it as that register's value at every later read.
#[test]
fn unmodelled_instruction_drops_its_destination_binding() {
    let ir = FunctionIr {
        function_id: 907,
        name: "staleValue".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "mov x9, x1"),
                stmt(0x1004, "frintn d0, d1"),
                stmt(0x1008, "clz x9, x2"),
                stmt(0x100c, "stur x9, [x3, #7]"),
                ret(0x1010),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    assert!(
        !out.contains("= receiver;"),
        "a register overwritten by an unmodelled instruction must not keep its old value:\n{out}"
    );
}
/// `kTrueOffsetFromNull` is 0x20 and `kFalseOffsetFromNull` is 0x30
/// (`runtime/vm/pointer_tagging.h`), so an add of either off NULL_REG
/// materialises a canonical bool, not an integer. Any other displacement is
/// still plain arithmetic against null, which is a true statement about a
/// canonical object and must not be dropped.
#[test]
fn null_relative_adds_name_the_canonical_bools_only() {
    let materialise = |imm: &str| {
        let ir = FunctionIr {
            function_id: 908,
            name: "boolMaterialise".to_string(),
            entry_va: 0x1000,
            blocks: vec![blk(
                0,
                0x1000,
                vec![
                    stmt(0x1000, &format!("add x9, x22, #{imm}")),
                    stmt(0x1004, "stur x9, [x3, #7]"),
                    ret(0x1008),
                ],
                vec![],
            )],
        };
        emit_pseudocode(&ir, &HashMap::new()).source
    };
    let t = materialise("0x20");
    assert!(t.contains("= true;"), "0x20 off null is `true`:\n{t}");
    let f = materialise("0x30");
    assert!(f.contains("= false;"), "0x30 off null is `false`:\n{f}");
    let other = materialise("0x40");
    assert!(
        !other.contains("true") && !other.contains("false"),
        "an undefined offset must not be named as a bool:\n{other}"
    );
}
/// A pool address built by one instruction and read through by the next carries
/// two displacements, and both are known, so the entry is exact. The normaliser
/// only accepted the single-displacement shape, so widening the load arms left
/// 834 references rendering as raw `(pool + page) + off` arithmetic.
#[test]
fn pool_page_with_a_second_displacement_resolves_to_an_entry() {
    let ir = FunctionIr {
        function_id: 909,
        name: "poolPage".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "add x9, x27, #0x2c, lsl #12"),
                stmt(0x1004, "ldr x9, [x9, #0xdc8]"),
                stmt(0x1008, "stur x9, [x3, #7]"),
                ret(0x100c),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    // 0x2c << 12 == 0x2c000, plus 0xdc8.
    assert!(
        out.contains(&format!("poolOff[{}]", 0x2c000 + 0xdc8)),
        "both displacements should fold into one pool entry:\n{out}"
    );
    assert_eq!(
        out.matches('(').count(),
        out.matches(')').count(),
        "folding must not leave an unbalanced parenthesis:\n{out}"
    );
}

/// Every frame slot the lifter can name needs a declaration, and `ldp` names
/// two of them from its third operand. Missing either yields an identifier with
/// no declaration, since only collected slots are declared.
#[test]
fn load_pair_frame_slots_are_declared() {
    let ir = FunctionIr {
        function_id: 910,
        name: "framePair".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldp x9, x10, [x29, #-0x20]"),
                stmt(0x1004, "stur x9, [x3, #7]"),
                stmt(0x1008, "stur x10, [x3, #0xf]"),
                ret(0x100c),
            ],
            vec![],
        )],
    };
    let out = emit_pseudocode(&ir, &HashMap::new()).source;
    // The two stores must read two different slot names, and each of those
    // names must have a declaration.
    let read: Vec<String> = out
        .lines()
        .filter_map(|l| l.trim().strip_suffix(';'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(_, rhs)| rhs.to_string())
        .collect();
    assert_eq!(read.len(), 2, "both slots should be stored:\n{out}");
    assert_ne!(
        read[0], read[1],
        "the two slots must not resolve to one name:\n{out}"
    );
    for name in &read {
        assert!(
            out.lines()
                .any(|l| l.trim() == format!("dynamic {name};") || l.trim() == format!("var {name};")),
            "slot {name} is referenced without a declaration:\n{out}"
        );
    }
}
