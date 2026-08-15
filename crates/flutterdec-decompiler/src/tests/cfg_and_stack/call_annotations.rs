// The ordinary-call loss site: a volatile register a call clobbers keeps its
// `regN` spelling and carries, as a comment, the value it held immediately
// before that call.
//
// The grammar half of this is cheap to get right and worthless on its own. A
// well-formed annotation carrying a stale, post-call or invented value is worse
// than no annotation, because nothing in the output tells the reader which one
// they are looking at. So the fixtures below are built around values that are
// distinguishable on purpose: a pre-call value the assertion names, a later
// value the assertion forbids, and an unresolved read between them.

/// A call at `va` into `target`, in the shape the lifter recognises.
fn call_to(va: u64, target: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #0x{target:x}"),
        target: format!("#0x{target:x}"),
    }
}

fn emitted(ir: &FunctionIr, stubs: &HashMap<u64, RuntimeStubEffect>) -> String {
    emit_program_with_runtime_stubs(
        std::slice::from_ref(ir),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        stubs,
    )
    .remove(0)
    .source
}

/// Every pre-call annotation span in `source`, in order.
fn pre_call_annotations(source: &str) -> Vec<String> {
    let mut spans = Vec::new();
    for line in source.lines() {
        let bytes = line.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            if !line[index..].starts_with(PRE_CALL_ANNOTATION.open()) {
                index += 1;
                continue;
            }
            let Some(span) = PRE_CALL_ANNOTATION.span_len(&bytes[index..]) else {
                break;
            };
            spans.push(line[index..index + span].to_string());
            index += span;
        }
    }
    spans
}

/// The value the register held immediately before the call, and never a value
/// it held after one.
///
/// The fixture puts two distinguishable values through x9 in sequence:
/// `arg0.f8` before the first call, and `arg1.f16.f8` before the second. Each
/// unresolved read must carry the value that was live at *its own* call, so
/// asserting the first annotation alone would pass for an emitter that recorded
/// the first value it ever saw and never updated it.
#[test]
fn a_call_clobber_annotates_the_value_held_immediately_before_that_call() {
    let ir = FunctionIr {
        function_id: 0x2100,
        name: "callClobber".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                // Parked in an ABI-preserved register, so it is still readable
                // after the first call and can be the source of a second,
                // distinguishable value.
                stmt(0x1000, "ldur x20, [x2, #15]"),
                // A: the value x9 holds when the first call is made.
                stmt(0x1004, "ldur x9, [x1, #7]"),
                call_to(0x1008, 0x9000),
                // The read the annotation attaches to: x9 is gone, so this
                // renders the unresolved spelling.
                stmt(0x100c, "stur x9, [x19, #7]"),
                // B: live only after the first call, so an emitter that
                // recorded the first value it saw would still be printing A.
                stmt(0x1010, "ldur x9, [x20, #7]"),
                stmt(0x1014, "stur x9, [x23, #7]"),
                call_to(0x1018, 0x9000),
                // The second read must carry B, which is what the second call
                // took.
                stmt(0x101c, "stur x9, [x24, #7]"),
                ret(0x1020),
            ],
            vec![],
        )],
    };
    let out = emitted(&ir, &HashMap::new());

    assert_eq!(
        pre_call_annotations(&out),
        vec![
            PRE_CALL_ANNOTATION.render(&["slot0.f8"]),
            PRE_CALL_ANNOTATION.render(&["slot1.f16.f8"]),
        ],
        "each unresolved read carries the value its own call dropped, never a \
         later one and never the first one twice:\n{out}"
    );
    // Stated separately from the vector above, which would still pass if the
    // annotation had landed on some other line entirely.
    assert!(
        out.contains(&format!(
            "reg19.f8 = reg9{};",
            PRE_CALL_ANNOTATION.render(&["slot0.f8"])
        )),
        "the annotation sits on the unresolved read, beside the register:\n{out}"
    );
    // The resolved read between the two calls keeps its value and gains
    // nothing: there was no loss there to report. Asserted on the absence of an
    // annotation rather than on the rendered value, because the rename pass
    // chooses that identifier from usage and it is not what this is about.
    let resolved = out
        .lines()
        .find(|line| line.contains("reg23.f8 ="))
        .expect("the resolved read is emitted");
    assert!(
        !resolved.contains(PRE_CALL_ANNOTATION.open()) && !resolved.contains("reg9"),
        "a read that resolved is not a loss site: {resolved}"
    );
    // The register keeps its own spelling: this site annotates, it does not bind.
    assert!(
        out.contains("reg9"),
        "annotating must not rebind the register:\n{out}"
    );
}

/// A call whose callee the SDK says preserves every register drops nothing, so
/// there is nothing to annotate.
///
/// This is a real negative rather than a restatement: the same instruction
/// sequence annotated twice in the fixture above.
#[test]
fn a_preserving_runtime_stub_emits_no_call_annotation() {
    let ir = FunctionIr {
        function_id: 0x2101,
        name: "preservingStubCaller".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldur x9, [x1, #7]"),
                call_to(0x1004, 0x9000),
                stmt(0x1008, "stur x9, [x20, #7]"),
                ret(0x100c),
            ],
            vec![],
        )],
    };
    let mut stubs = HashMap::new();
    stubs.insert(
        0x9000u64,
        RuntimeStubEffect {
            writes_result: false,
            preserves_registers: true,
        },
    );
    let mut symbols = HashMap::new();
    symbols.insert(0x9000u64, "stackOverflowSharedWithoutFpuRegs".to_string());
    let out = emit_program_with_runtime_stubs(
        std::slice::from_ref(&ir),
        &symbols,
        &HashMap::new(),
        &HashMap::new(),
        &stubs,
    )
    .remove(0)
    .source;

    assert!(
        pre_call_annotations(&out).is_empty(),
        "a preserved register lost nothing, so nothing is annotated:\n{out}"
    );
    assert!(
        !out.contains("reg9"),
        "the binding survives the call outright, so nothing reads as unresolved:\n{out}"
    );

    // The same shape with a clobbering stub does annotate, which is what makes
    // the emptiness above evidence rather than a fixture that renders nothing.
    let mut clobbering = HashMap::new();
    clobbering.insert(
        0x9000u64,
        RuntimeStubEffect {
            writes_result: false,
            preserves_registers: false,
        },
    );
    let clobbered = emit_program_with_runtime_stubs(
        std::slice::from_ref(&ir),
        &symbols,
        &HashMap::new(),
        &HashMap::new(),
        &clobbering,
    )
    .remove(0)
    .source;
    assert_eq!(
        pre_call_annotations(&clobbered),
        vec![PRE_CALL_ANNOTATION.render(&["slot0.f8"])],
        "the identical fixture annotates once the stub stops preserving:\n{clobbered}"
    );
}

/// R19-R28 are `kAbiPreservedCpuRegs` in `runtime/vm/constants_arm64.h`, so an
/// ordinary call does not drop them and there is no loss to report.
///
/// The fixture puts an ABI-preserved and a volatile register through the same
/// call, so the assertion distinguishes "preserved registers are not annotated"
/// from "this function annotates nothing".
#[test]
fn an_abi_preserved_register_emits_no_call_annotation() {
    let ir = FunctionIr {
        function_id: 0x2102,
        name: "abiPreserved".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldur x20, [x1, #7]"),
                stmt(0x1004, "ldur x9, [x2, #15]"),
                call_to(0x1008, 0x9000),
                stmt(0x100c, "stur x20, [x23, #7]"),
                stmt(0x1010, "stur x9, [x24, #7]"),
                ret(0x1014),
            ],
            vec![],
        )],
    };
    let out = emitted(&ir, &HashMap::new());

    assert_eq!(
        pre_call_annotations(&out),
        vec![PRE_CALL_ANNOTATION.render(&["slot1.f16"])],
        "only the volatile register lost a value at this call:\n{out}"
    );
    assert!(
        !out.contains("reg20"),
        "an ABI-preserved register still reads as its value after the call:\n{out}"
    );
    for reg in 19..=28 {
        assert!(
            !out.contains(&format!("reg{reg}{}", PRE_CALL_ANNOTATION.open())),
            "R{reg} is ABI-preserved and must never carry a call-loss annotation:\n{out}"
        );
    }
}

/// Only the volatile registers `constants_arm64.h` names, checked against the
/// header rather than against the emitter's own list.
///
/// `kDartVolatileCpuRegs` is R0-R14, plus TMP/TMP2 (R16/R17), R18 off Fuchsia,
/// and LR, which the call itself writes. R15 is SPREG and R19-R28 are
/// `kAbiPreservedCpuRegs`.
#[test]
fn the_clobbered_set_is_the_abi_volatile_set() {
    let volatile: Vec<String> = (0..=14)
        .chain([16, 17, 18, 30])
        .map(|index| format!("x{index}"))
        .collect();
    assert_eq!(
        CALL_CLOBBERED_REGISTERS
            .iter()
            .map(|reg| (*reg).to_string())
            .collect::<Vec<_>>(),
        volatile,
        "the annotated set must be the ABI volatile set, R19-R28 excluded"
    );
    for reg in 19..=28 {
        assert!(
            !CALL_CLOBBERED_REGISTERS.contains(&format!("x{reg}").as_str()),
            "R{reg} is ABI-preserved"
        );
    }
}

/// A value a merge could no longer attribute to one path is dropped rather than
/// carried past it.
///
/// Both arms call, so both drop x9, and the value each dropped describes only
/// its own arm. Annotating the join block's read with either would state the
/// other path's value as fact on a line reached from both.
#[test]
fn a_pre_call_value_does_not_survive_a_merge_that_could_have_rewritten_it() {
    let ir = FunctionIr {
        function_id: 0x2103,
        name: "mergedClobber".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x0", 0x1100)], vec![2, 1]),
            blk(
                1,
                0x1010,
                vec![
                    stmt(0x1010, "ldur x9, [x1, #7]"),
                    call_to(0x1014, 0x9000),
                    stmt(0x1018, "b #0x1200"),
                ],
                vec![3],
            ),
            blk(
                2,
                0x1100,
                vec![
                    stmt(0x1100, "ldur x9, [x2, #15]"),
                    call_to(0x1104, 0x9000),
                    stmt(0x1108, "b #0x1200"),
                ],
                vec![3],
            ),
            blk(
                3,
                0x1200,
                vec![stmt(0x1200, "stur x9, [x23, #7]"), ret(0x1204)],
                vec![],
            ),
        ],
    };
    let out = emitted(&ir, &HashMap::new());

    let after_merge = out
        .lines()
        .filter(|line| line.contains("reg23.f8"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !after_merge.contains(PRE_CALL_ANNOTATION.open()),
        "no single call's value describes a line both arms reach:\n{out}"
    );
}

/// A register written again after the call is no longer describable by what the
/// call took from it.
///
/// This is the case the corpus found. `blr` drops x9, the call binds its result,
/// and an instruction the lifter does not model then writes x9 - so x9 reads
/// unresolved again, for the second reason. The value the call took is a genuine
/// historical fact and a false statement about this read, which is exactly the
/// shape the site is required not to emit.
///
/// Asserted on the unresolved spelling as well as on the annotation: if the
/// write ever becomes modelled the read resolves, and this test must fail loudly
/// rather than pass because there is nothing left to annotate.
#[test]
fn a_value_is_not_annotated_once_something_else_has_written_the_register() {
    let ir = FunctionIr {
        function_id: 0x2104,
        name: "rewrittenAfterCall".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldur x9, [x1, #7]"),
                call_to(0x1004, 0x9000),
                // Unmodelled, and it writes x9: the binding the call dropped is
                // two writes behind by the next line.
                stmt(0x1008, "eor x9, x2, #0x10"),
                stmt(0x100c, "stur x9, [x19, #7]"),
                ret(0x1010),
            ],
            vec![],
        )],
    };
    let out = emitted(&ir, &HashMap::new());

    assert!(
        out.contains("reg19.f8 = reg9;"),
        "the read is still unresolved, so this fixture still exercises the case:\n{out}"
    );
    assert!(
        pre_call_annotations(&out).is_empty(),
        "the call's value does not describe a register something else has since \
         written:\n{out}"
    );
}

/// The annotation goes on the line the read was rendered on, not on the first
/// line that looks like it.
///
/// This is the corpus defect, reduced. The DFS emitter duplicates a shared
/// continuation, so `if (reg2 <= 0) {` can occur six times in one function, and
/// re-finding the anchor by content put a call's value on the first of them -
/// a line whose register was dropped by a merge rather than by that call. The
/// value was genuine, the site was not, and nothing in the output said so.
///
/// The rendered line and the finished line differ here exactly as the rename
/// pass makes them differ, so this also fails if the alignment starts demanding
/// identical text.
#[test]
fn an_annotation_lands_on_its_own_rendered_line_among_identical_ones() {
    let ir = FunctionIr {
        function_id: 0x2105,
        name: "repeatedLines".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    for _ in 0..5 {
        emitter.render_lines.push("  if (x2 <= 0) {".to_string());
        emitter.lines.push("  if (reg2 <= 0) {".to_string());
    }
    emitter.call_annotation_anchors.push(CallAnnotationAnchor {
        call_va: 0x1008,
        register: "x2".to_string(),
        value: "7".to_string(),
        snapshot_id: "8453:0".to_string(),
        line_index: 3,
    });
    emitter.append_call_annotations();

    let annotated: Vec<usize> = emitter
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(PRE_CALL_ANNOTATION.open()))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        annotated,
        vec![3],
        "the annotation must land on line 3, the one the anchor was rendered on:\n{:#?}",
        emitter.lines
    );
}
