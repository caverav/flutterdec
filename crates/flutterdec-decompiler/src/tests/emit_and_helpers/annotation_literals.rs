// Drift harness for the two shared authorities: the rejected-spelling set in
// `helpers/naming.rs` and the annotation literals in `helpers/annotation.rs`.
//
// Both exist because a consumer hand-rolling its own copy of a shared fact is
// this branch's most repeated defect. These checks are mechanical on purpose: a
// prose convention did not stop it four times, and a partial copy of either
// authority produces a convincing false pass rather than a visible failure.

/// Every non-test source of this crate, as (path, text). Read from disk rather
/// than listed, so a new file cannot escape the checks below by not being
/// mentioned in them.
fn crate_sources_outside_tests() -> Vec<(PathBuf, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("readable source directory") {
            let path = entry.expect("readable directory entry").path();
            if path.is_dir() {
                if path != root.join("tests") {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().is_some_and(|ext| ext == "rs")
                && path != root.join("tests.rs")
            {
                let text = fs::read_to_string(&path).expect("readable source file");
                sources.push((path, text));
            }
        }
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(
        sources.len() > 10,
        "the source scan found {} files, so it is not scanning the crate",
        sources.len()
    );
    sources
}

fn source_of(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("readable {}: {e}", path.display()))
}

/// The body of the function whose signature line starts with `signature`,
/// brace-matched. The three annotation consumers contain no brace inside a
/// string literal, so counting braces is enough and stays readable.
fn function_body(source: &str, signature: &str) -> String {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("signature not found: {signature}"));
    let open = start
        + source[start..]
            .find('{')
            .expect("function signature is followed by a body");
    let mut depth = 0usize;
    for (offset, byte) in source[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return source[open..open + offset + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced body for {signature}");
}

/// The three delimiters, derived from a literal's own rendering rather than
/// spelled here: a check that hard-codes the text it is protecting drifts with
/// nothing to notice.
fn rendered_delimiters() -> (String, String) {
    let rendered = EXHAUSTIVE_JOIN_ANNOTATION.render(&["A", "B"]);
    let (_, tail) = rendered.split_once('A').expect("first candidate rendered");
    let (separator, close) = tail.split_once('B').expect("second candidate rendered");
    (separator.to_string(), close.to_string())
}

#[test]
fn each_annotation_literal_has_exactly_one_definition() {
    let sources = crate_sources_outside_tests();
    let definition = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("helpers/annotation.rs");
    for literal in ANNOTATION_LITERALS {
        let hits: Vec<(&PathBuf, usize)> = sources
            .iter()
            .map(|(path, text)| (path, text.matches(literal.open()).count()))
            .filter(|(_, count)| *count > 0)
            .collect();
        assert_eq!(
            hits,
            vec![(&definition, 1usize)],
            "`{}` must be defined once, in helpers/annotation.rs, delimiters included",
            literal.open()
        );
    }
}

/// The emitter, the strip parser and the code-span accessor are the three
/// consumers of every literal. None of them may spell a delimiter itself.
///
/// A label-only constant would satisfy "one definition" while both sides still
/// hand-rolled ` /* ` and ` */` - that was the defect - and a hand-written
/// separator inside a fixed-slot template puts the exactly-two-arms assumption
/// back as a third drift axis.
#[test]
fn no_annotation_consumer_hand_rolls_a_delimiter() {
    let (separator, close) = rendered_delimiters();
    let opener_lead = " /* ";
    let structured = source_of("control_flow/structured.rs");
    let emit = source_of("control_flow/emit.rs");
    let lib = source_of("lib.rs");
    let consumers = [
        (
            "join emitter",
            function_body(&structured, "pub(crate) fn append_join_annotations"),
            ".render(",
        ),
        (
            "pre-call emitter",
            function_body(&emit, "pub(super) fn append_call_annotations"),
            ".render(",
        ),
        (
            "strip parser",
            function_body(&lib, "pub fn strip_join_annotation_span"),
            "annotation_at(",
        ),
        (
            "code-span accessor",
            function_body(&lib, "pub(crate) fn code_before_annotation"),
            "annotation_at(",
        ),
    ];
    for (name, body, shared_call) in consumers {
        for delimiter in [opener_lead, separator.as_str(), close.as_str()] {
            assert!(
                !body.contains(delimiter),
                "the {name} spells `{delimiter}` itself instead of using the shared literal"
            );
        }
        assert!(
            body.contains(shared_call),
            "the {name} must reach the literals through `{shared_call}`"
        );
    }
}

/// Live drift case for the two join literals, whose emitter exists. The
/// expected line is built by the constant, so a divergent opener, separator or
/// terminator on either the emitting or the parsing side fails here; running
/// arities one, two and three fails a fixed slot count as well.
///
/// The loop-entry literal has its own case below, now that its emitter exists.
/// The pre-call literal has one too, for the same reason.
#[test]
fn join_annotation_literals_round_trip_through_emitter_and_parsers() {
    let candidates = ["arg0.f8", "arg1.f16()", "7"];
    let emitted_candidates = ["slot0.f8", "slot1.f16()", "7"];
    for (complete, literal) in [
        (true, &EXHAUSTIVE_JOIN_ANNOTATION),
        (false, &NON_EXHAUSTIVE_JOIN_ANNOTATION),
    ] {
        for arity in 1..=candidates.len() {
            let values: Vec<String> = candidates[..arity]
                .iter()
                .map(|value| (*value).to_string())
                .collect();
            let ir = FunctionIr {
                function_id: 1030 + arity as u64,
                name: "annotationDrift".to_string(),
                entry_va: 0x1000,
                blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
            };
            let symbols = HashMap::new();
            let mut emitter = FuncEmitter::new(&ir, &symbols);
            emitter.lines.push(format!(
                "dynamic {}(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5) {{",
                ir.name
            ));
            emitter.lines.push("  sink(reg0);".to_string());
            emitter.join_candidates.insert(
                (0, "x0".to_string()),
                JoinCandidates {
                    complete,
                    values: values.clone(),
                    provenance: values
                        .iter()
                        .enumerate()
                        .map(|(pred, value)| crate::control_flow::JoinCandidateProvenance {
                            pred,
                            value: value.clone(),
                            snapshot_id: String::new(),
                        })
                        .collect(),
                },
            );
            emitter.join_annotation_anchors.push(JoinAnnotationAnchor {
                join: 0,
                candidate_regs: vec!["x0".to_string()],
                lines: emitter.lines.clone(),
            });
            emitter.apply_name_and_type_hints(&ir.name);
            emitter.append_join_annotations();

            let line = emitter.lines.last().expect("annotated line").clone();
            assert_eq!(
                line,
                format!(
                    "  sink(reg0{});",
                    literal.render(&emitted_candidates[..arity])
                ),
                "the emitter must render {arity} candidates through the shared literal"
            );
            assert_eq!(
                crate::strip_join_annotation_span(&line),
                "  sink(reg0);",
                "the strip parser must remove the whole span, delimiters included"
            );
            assert_eq!(
                crate::code_before_annotation(&line),
                "  sink(reg0",
                "the code-span accessor must cut exactly at the opener"
            );
        }
    }
}

/// Live drift case for the loop-entry literal, through the same emitter and the
/// same two parsers. Without it `VAL-BOUNDARY-005` would be satisfied vacuously
/// on exactly the literal this task introduced an emitter for: the corpus would
/// carry annotations no case had ever round-tripped.
///
/// The expected text is built by the constant, so rewording the opener, the
/// separator or the terminator moves emitter and parsers together or fails here.
/// Arities one, two and three run because a loop header can have any number of
/// entry arms, and a fixed slot count would pass at arity one.
#[test]
fn the_loop_entry_annotation_literal_round_trips_through_emitter_and_parsers() {
    let candidates = ["arg0.f8", "arg1.f16()", "7"];
    let emitted_candidates = ["slot0.f8", "slot1.f16()", "7"];
    for arity in 1..=candidates.len() {
        let values: Vec<String> = candidates[..arity]
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        let ir = FunctionIr {
            function_id: 1040 + arity as u64,
            name: "loopEntryDrift".to_string(),
            entry_va: 0x1000,
            blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines.push(format!(
            "dynamic {}(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5) {{",
            ir.name
        ));
        emitter.lines.push("  sink(reg0);".to_string());
        emitter.join_candidates.insert(
            (0, "x0".to_string()),
            JoinCandidates {
                // Never the exhaustive form at this site, whatever the flag says:
                // the site classification below is what selects the literal, so a
                // `complete` list cannot smuggle the join form into a loop header.
                complete: true,
                values: values.clone(),
                provenance: values
                    .iter()
                    .enumerate()
                    .map(|(pred, value)| crate::control_flow::JoinCandidateProvenance {
                        pred,
                        value: value.clone(),
                        snapshot_id: String::new(),
                    })
                    .collect(),
            },
        );
        emitter.loop_annotation_sites.insert(0);
        emitter.join_annotation_anchors.push(JoinAnnotationAnchor {
            join: 0,
            candidate_regs: vec!["x0".to_string()],
            lines: emitter.lines.clone(),
        });
        emitter.apply_name_and_type_hints(&ir.name);
        emitter.append_join_annotations();

        let line = emitter.lines.last().expect("annotated line").clone();
        assert_eq!(
            line,
            format!(
                "  sink(reg0{});",
                LOOP_ENTRY_ANNOTATION.render(&emitted_candidates[..arity])
            ),
            "the emitter must render {arity} entry values through the shared literal"
        );
        assert_eq!(
            crate::strip_join_annotation_span(&line),
            "  sink(reg0);",
            "the strip parser must remove the whole span, delimiters included"
        );
        assert_eq!(
            crate::code_before_annotation(&line),
            "  sink(reg0",
            "the code-span accessor must cut exactly at the opener"
        );
    }
}

/// The strip parser and the code-span accessor must know every literal, not the
/// two that happen to have an emitter today: an unrecognised annotation reaches
/// the quality counters as if it were code.
#[test]
fn both_parsers_recognise_every_annotation_literal() {
    for literal in ANNOTATION_LITERALS {
        let line = format!("  sink(reg0{}); // tail", literal.render(&["obj1.f8", "7"]));
        assert_eq!(
            crate::strip_join_annotation_span(&line),
            "  sink(reg0); // tail",
            "the strip parser does not know `{}`",
            literal.open()
        );
        assert_eq!(
            crate::code_before_annotation(&line),
            "  sink(reg0",
            "the code-span accessor does not know `{}`",
            literal.open()
        );
    }
}

/// The audit and corpus-scan scripts hold copies of the openers too, and a copy
/// *there* is the one that fails silently: a checker whose opener no longer
/// matches finds zero annotations in the corpus, reports zero violations, and
/// reads as a pass. Two of the scripts already assert their own copy from their
/// own integration test; this covers every script at once, including the ones no
/// Rust test drives, and it keeps covering a script added later.
///
/// The scan keys on the convention those scripts already follow - a module-level
/// `UPPER_CASE = "..."` constant - so prose in a docstring is not mistaken for a
/// copy.
#[test]
fn every_script_copy_of_an_annotation_delimiter_matches_its_literal() {
    let (separator, close) = rendered_delimiters();
    let (separator, close) = (separator.as_str(), close.as_str());
    let openers: Vec<&str> = ANNOTATION_LITERALS.iter().map(|l| l.open()).collect();
    let scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts")
        .canonicalize()
        .expect("the scripts directory is where the audit tooling lives");
    let mut checked = 0usize;
    for entry in fs::read_dir(&scripts).expect("readable scripts directory") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|ext| ext != "py") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("readable script");
        for line in text.lines() {
            let Some((name, value)) = line.split_once(" = \"") else {
                continue;
            };
            if !name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
                || name.is_empty()
            {
                continue;
            }
            let Some(value) = value.strip_suffix('"') else {
                continue;
            };
            let expected = if value.starts_with(" /*") {
                Some(&openers[..])
            } else if name.contains("CLOSE") {
                Some(std::slice::from_ref(&close))
            } else if name.contains("SEPARATOR") {
                Some(std::slice::from_ref(&separator))
            } else {
                None
            };
            let Some(expected) = expected else { continue };
            assert!(
                expected.contains(&value),
                "{} spells `{name} = {value:?}`, which is not one of the shared literals",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 4,
        "the scan matched {checked} script constants, so it is not scanning the tooling"
    );
}

/// The fourth counter sentinel, `/* cond */`, is the one that cannot ride
/// *inside* an annotation the way `arg0`, `_block_` and a bare register
/// spelling do, and that is structural rather than an omission in the fixture.
/// The text carries the annotation terminator, so a candidate spelling it would
/// close the span early and leave the tail of its own value in the code the
/// counters read - `placeholder_cond_markers` would then move on annotation
/// content, which this boundary forbids.
///
/// The shared recordable filter rejects such a value at capture, at every loss
/// site, which is why the counter fixture places `/* cond */` outside the span.
/// The delimiters come from the literal's own rendering, so this case follows
/// the constant rather than restating it.
#[test]
fn no_candidate_value_may_carry_an_annotation_delimiter() {
    let (separator, close) = rendered_delimiters();
    assert!(
        crate::control_flow::is_recordable_annotation_candidate("obj1.f8"),
        "the filter must accept an ordinary value, or it rejects everything and proves nothing"
    );
    for value in [
        "/* cond */".to_string(),
        format!("smiTag(x0){close}"),
        format!("obj1.f8{separator}7"),
    ] {
        assert!(
            !crate::control_flow::is_recordable_annotation_candidate(&value),
            "`{value}` would break the annotation span open and must never be recorded"
        );
    }

    // And the reason, demonstrated rather than asserted: were such a value to
    // reach the emitter, the span would no longer strip back to the bare line.
    let leaked = format!("  sink(reg0{});", EXHAUSTIVE_JOIN_ANNOTATION.render(&["/* cond */"]));
    assert_ne!(
        crate::strip_join_annotation_span(&leaked),
        "  sink(reg0);",
        "if a delimiter-bearing value stripped cleanly this filter would be unnecessary"
    );
}

/// Mechanical source check for the rejected-spelling authority: inside the
/// capture module no site names a register spelling of its own, and every
/// spelling test routes through `unrecovered_value_spellings` or one of the two
/// recognisers built on it. The three capture paths - join, loop entry and
/// pre-call - share those recognisers, so a site cannot hold a partial subset.
#[test]
fn the_capture_module_gets_every_spelling_from_the_shared_helper() {
    let structured = source_of("control_flow/structured.rs");
    for alias in (0..=30)
        .map(named_register_alias)
        .chain((0..=30).map(|index| named_indirect_target(&format!("x{index}"))))
    {
        assert!(
            !structured.contains(&format!("\"{alias}\"")),
            "the capture module spells `{alias}` itself instead of asking the helper"
        );
    }

    let authority = "unrecovered_value_spellings";
    let recognisers = [
        "fn is_unrecovered_value_spelling",
        "fn canonical_register_spelling",
    ];
    for recogniser in recognisers {
        let body = function_body(&structured, recogniser);
        assert!(
            body.contains(authority),
            "`{recogniser}` must obtain its spellings from `{authority}`"
        );
    }

    // Any other place that builds a canonical spelling must consult the
    // authority in the same function, which is what stops a site from growing a
    // list of its own next to it.
    for (offset, _) in structured.match_indices("format!(\"x{") {
        let function_start = structured[..offset]
            .rfind("\nfn ")
            .max(structured[..offset].rfind(" fn "))
            .expect("a canonical spelling is built inside a function");
        let body = &structured[function_start..offset];
        assert!(
            body.contains(authority) || structured[offset..].starts_with("format!(\"x{index}\")"),
            "a canonical spelling is built at byte {offset} without consulting `{authority}`"
        );
    }
}

/// The surviving vector, one spelling family at a time. Asserting the vector
/// rather than membership is what catches a spelling the filter forgot: an
/// `assert!(!survivors.contains(..))` passes just as happily when the filter
/// rejects everything, and a membership check on the kept value passes when it
/// rejects nothing.
fn survivors_of(candidates: &[String]) -> Vec<String> {
    candidates
        .iter()
        .filter(|value| crate::control_flow::is_informative_annotation_candidate(value))
        .cloned()
        .collect()
}

#[test]
fn no_canonical_register_spelling_survives_the_shared_filter() {
    let mut candidates: Vec<String> = (0..=30).map(|index| format!("x{index}")).collect();
    candidates.push("obj1.f8".to_string());
    assert_eq!(survivors_of(&candidates), vec!["obj1.f8".to_string()]);
}

#[test]
fn no_named_register_alias_survives_the_shared_filter() {
    let mut candidates: Vec<String> = (0..=30).map(named_register_alias).collect();
    candidates.push("obj1.f8".to_string());
    assert_eq!(survivors_of(&candidates), vec!["obj1.f8".to_string()]);
}

#[test]
fn no_named_indirect_target_survives_the_shared_filter() {
    let mut candidates: Vec<String> = (0..=30)
        .map(|index| named_indirect_target(&format!("x{index}")))
        .collect();
    candidates.push("obj1.f8".to_string());
    assert_eq!(survivors_of(&candidates), vec!["obj1.f8".to_string()]);
}

/// `argN` is deliberately not in the rejected set: candidates captured before
/// `apply_name_and_type_hints` can carry it, then insertion replays the naming
/// map to `slotN` before output. The filter still rejects bare `argN` as
/// uninformative on its own - it names no field, call or literal - which is a
/// different reason and is recorded here so it is not mistaken for membership in
/// the spelling set.
#[test]
fn the_spelling_set_excludes_arg_names() {
    for index in 0..=7 {
        let arg = format!("arg{index}");
        assert!(
            unrecovered_value_spellings(&arg).is_empty(),
            "`{arg}` must not be a register spelling"
        );
        assert!(
            !unrecovered_value_spellings("x0").contains(&arg),
            "`x0` must not claim `{arg}` as one of its spellings"
        );
    }
}

/// Live drift case for the pre-call literal, driven through the real insertion
/// pass rather than through a rendered string.
///
/// The expected line is built by the constant, so changing the opener, the
/// terminator or the separator moves the emitter and both parsers together or
/// fails here. This is what stops `VAL-BOUNDARY-005` from being satisfied
/// vacuously on the one literal this task introduced.
#[test]
fn the_pre_call_annotation_literal_round_trips_through_emitter_and_parsers() {
    let ir = FunctionIr {
        function_id: 1040,
        name: "preCallDrift".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    // The rendered line carries the raw spelling and the finished one the
    // renamed spelling, which is the real shape: the rename pass runs between
    // them, and the alignment has to see through it.
    let signature = format!(
        "dynamic {}(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5) {{",
        ir.name
    );
    emitter.render_lines.push(signature.clone());
    emitter.render_lines.push("  sink(x9);".to_string());
    emitter.lines.push(signature);
    emitter.lines.push("  sink(reg9);".to_string());
    emitter.call_annotation_anchors.push(CallAnnotationAnchor {
        call_va: 0x1004,
        register: "x9".to_string(),
        value: "arg0.f8".to_string(),
        snapshot_id: "1040:0".to_string(),
        line_index: 1,
    });
    emitter.apply_name_and_type_hints(&ir.name);
    emitter.append_call_annotations();

    let line = emitter.lines.last().expect("annotated line").clone();
    assert_eq!(
        line,
        format!("  sink(reg9{});", PRE_CALL_ANNOTATION.render(&["slot0.f8"])),
        "the emitter must render the pre-call value through the shared literal"
    );
    assert_eq!(
        crate::strip_join_annotation_span(&line),
        "  sink(reg9);",
        "the strip parser must remove the whole span, delimiters included"
    );
    assert_eq!(
        crate::code_before_annotation(&line),
        "  sink(reg9",
        "the code-span accessor must cut exactly at the opener"
    );
}

#[test]
fn a_candidate_naming_a_dead_local_is_not_annotated() {
    let ir = FunctionIr {
        function_id: 1041,
        name: "deadCandidate".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(0, 0x1000, vec![ret(0x1000)], Vec::new())],
    };
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines.push(format!(
        "dynamic {}(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5) {{",
        ir.name
    ));
    emitter.lines.push("  sink(reg0);".to_string());
    let dead_local = "deadLocal.f8";
    assert!(
        crate::control_flow::is_recordable_annotation_candidate(dead_local)
            && crate::control_flow::is_informative_annotation_candidate(dead_local),
        "the candidate must reach the liveness gate"
    );
    emitter.join_candidates.insert(
        (0, "x0".to_string()),
        JoinCandidates {
            complete: true,
            values: vec![dead_local.to_string()],
            provenance: vec![crate::control_flow::JoinCandidateProvenance {
                pred: 0,
                value: dead_local.to_string(),
                snapshot_id: String::new(),
            }],
        },
    );
    emitter.join_annotation_anchors.push(JoinAnnotationAnchor {
        join: 0,
        candidate_regs: vec!["x0".to_string()],
        lines: emitter.lines.clone(),
    });
    emitter.apply_name_and_type_hints(&ir.name);
    assert!(
        emitter.lines.iter().all(|line| !line.contains("deadLocal")),
        "the candidate names no identifier in the emitted body"
    );
    emitter.append_join_annotations();

    assert_eq!(
        emitter.lines.last().map(String::as_str),
        Some("  sink(reg0);"),
        "a candidate naming a dead local must not reach an annotation"
    );
}
