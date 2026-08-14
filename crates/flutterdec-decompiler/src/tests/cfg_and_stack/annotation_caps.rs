// The budgets on the annotation insertion path, and the rule that a budget is
// never met by cutting a span down to size.
//
// A truncated annotation leaves an opener with no terminator, so every consumer
// that parses comments reads the rest of the file as one comment. The emitted
// artifact is therefore worse than the one that omitted the evidence entirely,
// which is why both budgets drop the whole span and why each drop leaves a row.
//
// Every fixture here drives the real insertion pass. The candidate lists are
// controlled - a value long enough to breach the per-annotation budget does not
// occur naturally, and a forbidden sequence is rejected at capture, so neither
// can be reached from an IR fixture - but the pass, the budgets and the ledger
// rows are the shipped ones.

use crate::control_flow::{MAX_JOIN_ANNOTATED_LINE, MAX_JOIN_ANNOTATION};

/// An emitter holding `line`, with `regs` captured as one-value join candidates
/// at block 0 and an anchor over that line.
///
/// The same shape the literal drift cases use: this file is about what the
/// insertion pass does with a candidate list, not about how capture built one.
fn emitter_with_candidates<'a>(
    ir: &'a FunctionIr,
    symbols: &'a HashMap<u64, String>,
    line: &str,
    regs: &[(&str, &str)],
) -> FuncEmitter<'a> {
    let mut emitter = FuncEmitter::new(ir, symbols);
    emitter.lines.push(line.to_string());
    for (reg, value) in regs {
        emitter.join_candidates.insert(
            (0, (*reg).to_string()),
            JoinCandidates {
                complete: true,
                values: vec![(*value).to_string()],
                provenance: vec![crate::control_flow::JoinCandidateProvenance {
                    pred: 1,
                    value: (*value).to_string(),
                    snapshot_id: String::new(),
                }],
            },
        );
    }
    emitter.join_annotation_anchors.push(JoinAnnotationAnchor {
        join: 0,
        candidate_regs: regs.iter().map(|(reg, _)| (*reg).to_string()).collect(),
        lines: emitter.lines.clone(),
    });
    emitter
}

fn cap_fixture_ir(function_id: u64, name: &str) -> FunctionIr {
    FunctionIr {
        function_id,
        name: name.to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
    }
}

/// The whole ledger contribution of one run, as the audit publishes it: the
/// per-site rows and the total counted at the drop.
fn omission_rows(emitter: &FuncEmitter<'_>) -> Vec<(&'static str, &'static str, usize)> {
    let mut rows: Vec<(&'static str, &'static str, usize)> = Vec::new();
    for stream in [
        &emitter.join_provenance,
        &emitter.loop_provenance,
        &emitter.call_provenance,
    ] {
        assert_eq!(
            stream.omitted_at_insertion,
            stream.cap_omissions.len(),
            "the counted total and the detail rows must agree for {}",
            stream.loss_site
        );
        for omission in &stream.cap_omissions {
            rows.push((
                omission.loss_site,
                omission.budget,
                omission.annotation_len,
            ));
        }
    }
    rows
}

/// A value that renders through `literal` to exactly `target` bytes.
fn value_rendering_to(literal: &AnnotationLiteral, target: usize) -> String {
    let overhead = literal.render(&[""]).len();
    assert!(
        target >= overhead,
        "a {target}-byte span cannot hold this literal's {overhead} bytes of delimiters"
    );
    "v".repeat(target - overhead)
}

#[test]
fn omits_the_whole_annotation_when_it_exceeds_the_per_annotation_budget() {
    let ir = cap_fixture_ir(2100, "perAnnotationBudget");
    let symbols = HashMap::new();
    let code = "  sink(reg0);";

    // One byte under the budget goes in; one byte over it takes the whole span
    // with it, and the line is left exactly as it was.
    for (span, fits) in [(MAX_JOIN_ANNOTATION, true), (MAX_JOIN_ANNOTATION + 1, false)] {
        let value = value_rendering_to(&EXHAUSTIVE_JOIN_ANNOTATION, span);
        let mut emitter =
            emitter_with_candidates(&ir, &symbols, code, &[("x0", value.as_str())]);
        emitter.append_join_annotations();

        let line = emitter.lines[0].clone();
        if fits {
            assert_eq!(
                line,
                format!("  sink(reg0{});", EXHAUSTIVE_JOIN_ANNOTATION.render(&[&value])),
                "a span of exactly {span} bytes is inside the budget"
            );
            assert!(
                omission_rows(&emitter).is_empty(),
                "an annotation that was inserted may not be counted as omitted"
            );
            continue;
        }
        assert_eq!(
            line, code,
            "a span of {span} bytes is over the budget, so nothing at all is inserted"
        );
        assert!(
            !line.contains(EXHAUSTIVE_JOIN_ANNOTATION.open()),
            "an over-budget span must leave no opener behind: {line}"
        );
        assert_eq!(
            omission_rows(&emitter),
            vec![("join", "annotation", span)],
            "the drop must be counted against the join site's per-annotation budget"
        );
    }
}

#[test]
fn annotates_a_line_at_the_three_thousand_character_boundary_and_omits_one_byte_past_it() {
    let ir = cap_fixture_ir(2101, "lineBoundary");
    let symbols = HashMap::new();
    let value = "obj1.f8";
    let span = EXHAUSTIVE_JOIN_ANNOTATION.render(&[value]).len();

    for (final_len, fits) in [
        (MAX_JOIN_ANNOTATED_LINE, true),
        (MAX_JOIN_ANNOTATED_LINE + 1, false),
    ] {
        // Padded on the right of the annotation point, so the only thing the two
        // runs differ in is the one byte the budget is being asked about.
        let head = "  sink(reg0);";
        let code = format!("{head}{}", " ".repeat(final_len - span - head.len()));
        let mut emitter = emitter_with_candidates(&ir, &symbols, &code, &[("x0", value)]);
        emitter.append_join_annotations();

        let line = emitter.lines[0].clone();
        if fits {
            assert_eq!(
                line.len(),
                MAX_JOIN_ANNOTATED_LINE,
                "a line that lands exactly on the cap is emitted annotated"
            );
            assert!(
                line.contains(&EXHAUSTIVE_JOIN_ANNOTATION.render(&[value])),
                "and it carries the whole span"
            );
            assert!(
                omission_rows(&emitter).is_empty(),
                "nothing was dropped, so no row may claim one was"
            );
            continue;
        }
        assert_eq!(
            line, code,
            "one byte past the cap drops the whole annotation, not the byte"
        );
        assert!(
            line.len() < MAX_JOIN_ANNOTATED_LINE,
            "and the emitted line stays under the cap"
        );
        assert_eq!(
            omission_rows(&emitter),
            vec![("join", "line", span)],
            "the drop must be counted against the aggregate line budget"
        );
    }
}

/// Several annotations on one line share one budget, so the one that overruns it
/// is dropped whole while the ones that fit are untouched.
///
/// The aggregate is the case a per-annotation cap alone cannot see: every span
/// here is far inside `MAX_JOIN_ANNOTATION`, and it is only their sum on one
/// physical line that breaches anything.
#[test]
fn omits_the_whole_annotation_when_several_on_one_line_exceed_the_aggregate_budget() {
    let ir = cap_fixture_ir(2102, "aggregateBudget");
    let symbols = HashMap::new();
    let values = [("x0", "obj1.f8"), ("x1", "obj2.f16()"), ("x2", "obj3.f24")];
    let spans: Vec<usize> = values
        .iter()
        .map(|(_, value)| EXHAUSTIVE_JOIN_ANNOTATION.render(&[value]).len())
        .collect();
    for span in &spans {
        assert!(
            *span <= MAX_JOIN_ANNOTATION,
            "no single span here may breach the per-annotation budget, or the \
             aggregate is not what is being tested"
        );
    }

    // Room for the first two spans and one byte short of the third.
    let head = "  sink(reg0, reg1, reg2);";
    let pad = MAX_JOIN_ANNOTATED_LINE - spans.iter().sum::<usize>() + 1 - head.len();
    let code = format!("{head}{}", " ".repeat(pad));
    let mut emitter = emitter_with_candidates(&ir, &symbols, &code, &values);
    emitter.append_join_annotations();

    let line = emitter.lines[0].clone();
    assert!(
        line.len() <= MAX_JOIN_ANNOTATED_LINE,
        "the emitted line must respect the aggregate budget: {} bytes",
        line.len()
    );
    assert_eq!(
        line.matches(EXHAUSTIVE_JOIN_ANNOTATION.open()).count(),
        2,
        "the two spans that fit are kept: {line}"
    );
    for (index, (_, value)) in values.iter().enumerate() {
        let span = EXHAUSTIVE_JOIN_ANNOTATION.render(&[value]);
        assert_eq!(
            line.contains(&span),
            index < 2,
            "span {index} must be present whole or absent whole: {line}"
        );
    }
    assert_eq!(
        omission_rows(&emitter),
        vec![("join", "line", spans[2])],
        "the one dropped span is counted once, against the aggregate budget"
    );
}

/// Each forbidden sequence, one controlled candidate at a time.
///
/// A brace steers the brace-sensitive compaction pass into reading a block that
/// is not there; a comment terminator inside the span ends it early and leaves
/// the rest of the annotation on the line as code. Both are rejected at capture
/// and again at insertion, so the artifact holds the property whichever filter a
/// later change moves.
#[test]
fn no_forbidden_sequence_survives_capture_or_insertion() {
    let ir = cap_fixture_ir(2103, "forbiddenSequences");
    let symbols = HashMap::new();
    let terminator = {
        let rendered = EXHAUSTIVE_JOIN_ANNOTATION.render(&["A"]);
        rendered.split_once('A').expect("rendered").1.trim().to_string()
    };
    let forbidden = ["obj1.f{8}", "obj1.f8}", format!("obj1{terminator}f8").as_str()]
        .map(str::to_string);

    for value in forbidden {
        assert!(
            !crate::control_flow::is_recordable_annotation_candidate(&value),
            "`{value}` must not be recordable"
        );
        assert!(
            !rendered_annotation_is_safe(&EXHAUSTIVE_JOIN_ANNOTATION.render(&[&value])),
            "a span built from `{value}` must not pass the insertion gate"
        );

        let code = "  sink(reg0);";
        let mut emitter =
            emitter_with_candidates(&ir, &symbols, code, &[("x0", value.as_str())]);
        emitter.append_join_annotations();
        assert_eq!(
            emitter.lines[0], code,
            "`{value}` must not reach the emitted line"
        );
    }

    assert!(
        rendered_annotation_is_safe(&EXHAUSTIVE_JOIN_ANNOTATION.render(&["obj1.f8", "7"])),
        "an ordinary span must still pass the gate"
    );
}

/// The pre-call site shares the budgets and owes its own ledger row.
///
/// Without this the per-site column would be exercised on two of the three
/// sites, and a call-site drop would be the one that stayed silent.
#[test]
fn omits_a_whole_pre_call_annotation_over_budget_and_counts_it_against_the_call_site() {
    let ir = cap_fixture_ir(2104, "preCallBudget");
    let symbols = HashMap::new();
    let code = "  sink(reg9);";
    let value = value_rendering_to(&PRE_CALL_ANNOTATION, MAX_JOIN_ANNOTATION + 1);

    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.render_lines.push(code.to_string());
    emitter.lines.push(code.to_string());
    emitter.call_annotation_anchors.push(CallAnnotationAnchor {
        call_va: 0x1004,
        register: "x9".to_string(),
        value: value.clone(),
        snapshot_id: "2104:0".to_string(),
        line_index: 0,
    });
    emitter.append_call_annotations();

    assert_eq!(
        emitter.lines[0], code,
        "an over-budget pre-call span is dropped whole"
    );
    assert_eq!(
        omission_rows(&emitter),
        vec![("call", "annotation", MAX_JOIN_ANNOTATION + 1)],
        "and it is counted against the call site, not against a join"
    );
}

/// A loop-header drop belongs to the loop site's ledger row, not the join's.
///
/// A loop header is also a join by predecessor count, so the two sites share a
/// block id space; the classification recorded at capture is the only thing that
/// tells them apart, here as everywhere else.
#[test]
fn counts_a_loop_header_drop_against_the_loop_site() {
    let ir = cap_fixture_ir(2105, "loopHeaderBudget");
    let symbols = HashMap::new();
    let code = "  sink(reg0);";
    let value = value_rendering_to(&LOOP_ENTRY_ANNOTATION, MAX_JOIN_ANNOTATION + 1);

    let mut emitter = emitter_with_candidates(&ir, &symbols, code, &[("x0", value.as_str())]);
    emitter.loop_annotation_sites.insert(0);
    emitter.append_join_annotations();

    assert_eq!(emitter.lines[0], code, "the whole span is dropped");
    assert_eq!(
        omission_rows(&emitter),
        vec![("loop_entry", "annotation", MAX_JOIN_ANNOTATION + 1)],
        "a loop header's drop is the loop site's ledger row"
    );
}
