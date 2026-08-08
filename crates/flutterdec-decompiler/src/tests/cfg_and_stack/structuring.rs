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
