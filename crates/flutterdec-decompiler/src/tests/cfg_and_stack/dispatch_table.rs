fn other(va: u64, src: &str) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Other,
        src: src.to_string(),
        target: String::new(),
    }
}

fn dispatch_call_ir(index_setup: Vec<LlirInstr>) -> FunctionIr {
    let mut instrs = vec![
        // Receiver in x1: load its header, extract the class id.
        other(0xd000, "ldur x0, [x1, #-1]"),
        other(0xd004, "ubfx x0, x0, #0xc, #0x14"),
    ];
    instrs.extend(index_setup);
    instrs.push(other(0xd020, "ldr x30, [x21, x30, lsl #3]"));
    instrs.push(LlirInstr {
        va: 0xd024,
        op: IROp::Call,
        src: "blr x30".to_string(),
        target: "x30".to_string(),
    });
    instrs.push(LlirInstr {
        va: 0xd028,
        op: IROp::Return,
        src: "ret".to_string(),
        target: String::new(),
    });

    FunctionIr {
        function_id: 900,
        name: "dispatches".to_string(),
        entry_va: 0xd000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xd000,
            instrs,
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    }
}

/// `sub` form: `selector_offset = kOriginElement - 0xf5d = 4096 - 3933 = 163`.
#[test]
fn names_dispatch_table_calls_from_the_sub_encoding() {
    let ir = dispatch_call_ir(vec![other(0xd008, "sub x30, x0, #0xf5d")]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains(".sel163("),
        "sub-encoded selector offset should resolve to 163:\n{}",
        artifact.source
    );
    assert_eq!(
        artifact.dispatch_table_calls, 1,
        "recovered selector should be counted as a dispatch call"
    );
}

/// Wide offsets are materialised `movz`+`movk` then added, so both halves must
/// be folded: `0x9624 | (1 << 16) = 0x19624`, plus the origin element.
#[test]
fn folds_movk_halves_into_the_selector_offset() {
    let ir = dispatch_call_ir(vec![
        other(0xd008, "mov x17, #0x9624"),
        other(0xd00c, "movk x17, #1, lsl #16"),
        other(0xd010, "add x30, x0, x17"),
    ]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let expected = 0x19624 + 4096;
    assert!(
        artifact.source.contains(&format!(".sel{expected}(")),
        "movk half must be folded, expected sel{expected}:\n{}",
        artifact.source
    );
}

/// Offsets that are a multiple of 4096 encode as a shifted immediate:
/// `sub x30, x0, #1, lsl #12` is -4096, so the selector offset is 0.
#[test]
fn folds_shifted_immediates_into_the_selector_offset() {
    let ir = dispatch_call_ir(vec![other(0xd008, "sub x30, x0, #1, lsl #12")]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains(".sel0("),
        "shifted immediate must scale by its lsl amount, expected sel0:\n{}",
        artifact.source
    );
}

/// A zero offset degenerates to a register move, so the class id indexes the
/// table directly and the selector offset is the origin element.
#[test]
fn recovers_the_zero_offset_register_move_encoding() {
    let ir = dispatch_call_ir(vec![other(0xd008, "mov x30, x0")]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains(".sel4096("),
        "a bare register move is a zero offset, expected sel4096:\n{}",
        artifact.source
    );
}

/// The receiver is the object whose header supplied the class id, rendered as
/// the callee's target and never repeated in the argument list.
#[test]
fn renders_the_receiver_and_omits_it_from_the_arguments() {
    let ir = dispatch_call_ir(vec![
        other(0xd008, "mov x2, x10"),
        // x4 is ARGS_DESC_REG, never an argument.
        other(0xd00c, "mov x4, x11"),
        other(0xd010, "sub x30, x0, #0xf5d"),
    ]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let call = artifact
        .source
        .lines()
        .find(|l| l.contains(".sel163("))
        .expect("dispatch call line");
    let (head, rest) = call.split_once(".sel163(").expect("selector call");
    let receiver = head.rsplit([' ', '=']).next().unwrap_or_default();
    let args = rest.split(')').next().unwrap_or_default();
    assert!(
        !receiver.is_empty() && receiver != "dispatch",
        "receiver register x1 should resolve to a named value: {call}"
    );
    assert_eq!(
        args.split(", ").count(),
        1,
        "only x2 is an argument: x1 is the receiver and x4 is the arguments descriptor: {call}"
    );
    assert!(
        !args.split(", ").any(|a| a == receiver),
        "receiver {receiver} must not also appear as an argument: {call}"
    );
}

/// Redefining the receiver register between the header load and the call makes
/// the receiver unknown. Rendering the new occupant would be a wrong receiver.
///
/// The intent has always been to degrade, but the spelling used to be a literal
/// `dispatch` in the receiver position, which reads as an object the call was made
/// on rather than as an admission. The selector now stands alone and the comment
/// says the receiver was not recovered.
#[test]
fn drops_a_receiver_whose_register_was_redefined() {
    let ir = dispatch_call_ir(vec![
        other(0xd008, "mov x1, x9"),
        other(0xd00c, "sub x30, x0, #0xf5d"),
    ]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains("receiver: unrecovered"),
        "a clobbered receiver must be reported as unrecovered:\n{}",
        artifact.source
    );
    assert!(
        !artifact.source.contains("dispatch.sel"),
        "an unknown receiver must not be spelled as an object:\n{}",
        artifact.source
    );
}

/// A stale table entry must not name an unrelated indirect call. Overwriting the
/// target register between the load and the call kills the binding.
#[test]
fn does_not_name_an_unrelated_indirect_call() {
    let mut ir = dispatch_call_ir(vec![other(0xd008, "sub x30, x0, #0xf5d")]);
    let instrs = &mut ir.blocks[0].instrs;
    // Redefine the call target after the dispatch table load.
    instrs.insert(instrs.len() - 2, other(0xd022, "mov x30, x5"));
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        !artifact.source.contains(".sel163("),
        "a redefined target must not inherit the dispatch selector:\n{}",
        artifact.source
    );
    assert_eq!(artifact.dispatch_table_calls, 0);
}

/// The class-id bitfield position has moved between SDK versions, so recovery
/// must key on the header load rather than on the immediates.
#[test]
fn recovers_the_receiver_under_a_shifted_class_id_bitfield() {
    let mut instrs = vec![
        other(0xd000, "ldur x0, [x1, #-1]"),
        // Not the 3.9/3.11 position; a later layout change would look like this.
        other(0xd004, "ubfx x0, x0, #0xd, #0x14"),
        other(0xd008, "sub x30, x0, #0xf5d"),
        other(0xd020, "ldr x30, [x21, x30, lsl #3]"),
    ];
    instrs.push(LlirInstr {
        va: 0xd024,
        op: IROp::Call,
        src: "blr x30".to_string(),
        target: "x30".to_string(),
    });
    instrs.push(LlirInstr {
        va: 0xd028,
        op: IROp::Return,
        src: "ret".to_string(),
        target: String::new(),
    });
    let ir = FunctionIr {
        function_id: 901,
        name: "shiftedCid".to_string(),
        entry_va: 0xd000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0xd000,
            instrs,
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    };
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let call = artifact
        .source
        .lines()
        .find(|l| l.contains(".sel163("))
        .expect("dispatch call line");
    // Anchored on the receiver actually resolving, not on the absence of the old
    // `dispatch.` marker: that marker can no longer be emitted at all, so a
    // negative assertion against it would pass without testing anything.
    assert!(
        !call.contains("receiver: unrecovered"),
        "receiver must still resolve when the bitfield position differs: {call}"
    );
    assert!(
        call.contains("receiver.sel163(") || call.contains("param1.sel163("),
        "the resolved receiver must appear in the callee position: {call}"
    );
}

/// An empty argument list must not read as a recovered zero-arity method.
#[test]
fn labels_an_empty_argument_list_as_unknown() {
    let ir = dispatch_call_ir(vec![other(0xd008, "sub x30, x0, #0xf5d")]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains(".sel163(); // dispatch table")
            && artifact.source.contains("args: unknown"),
        "no defined argument register, so the arity is unknown, not zero:\n{}",
        artifact.source
    );
}

/// Arguments the call site defines are reported, in convention order, and
/// labelled as a lower bound rather than a signature.
#[test]
fn reports_defined_argument_registers_as_a_lower_bound() {
    let ir = dispatch_call_ir(vec![
        other(0xd008, "mov x2, x9"),
        other(0xd00c, "mov x5, x10"),
        other(0xd010, "sub x30, x0, #0xf5d"),
    ]);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let call = artifact
        .source
        .lines()
        .find(|l| l.contains(".sel163("))
        .expect("dispatch call line");
    assert!(
        call.contains("args: lower bound"),
        "a partial argument list must say so: {call}"
    );
    let args = call
        .split_once(".sel163(")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(args, _)| args)
        .unwrap_or_default();
    assert_eq!(
        args.split(", ").count(),
        2,
        "x2 and x5 were defined, x4 is the arguments descriptor and is never an argument: {call}"
    );
}
