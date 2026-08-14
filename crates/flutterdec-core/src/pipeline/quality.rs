/// The six counters read out of one line of emitted source, in the fixed order
/// `[block_helper_refs, raw_arg_name_refs, raw_register_name_refs,
/// placeholder_cond_markers, omitted_path_markers, loop_backedge_markers]`.
///
/// Extracted so a fixture can score an annotated line through exactly the code
/// the pipeline runs. A fixture with its own copy of the counting rules proves
/// only that the copy agrees with itself.
fn source_text_counters(line: &str) -> [usize; 6] {
    // Value annotations are reader-facing evidence, not emitted code. Strip
    // only their exact spans: all historical comments stay in the ruler, so
    // pre-annotation reports remain bit-for-bit comparable.
    let code = flutterdec_decompiler::strip_join_annotation_span(line);
    let mut arg_refs = 0usize;
    for n in 0..=7 {
        arg_refs += count_ident_token(&code, &format!("arg{n}"));
    }
    let mut register_refs = 0usize;
    for n in 0..=30 {
        // `xN` is the disassembly spelling; the emitter renders an unresolved
        // register through `named_register_alias`, which yields `regN`. Counting
        // only `xN` reported zero on every real binary while thousands of `regN`
        // were being emitted.
        register_refs += count_ident_token(&code, &format!("x{n}"));
        register_refs += count_ident_token(&code, &format!("reg{n}"));
    }
    [
        code.matches("_block_").count(),
        arg_refs,
        register_refs,
        code.matches("/* cond */").count(),
        code.matches("omitted complex path").count(),
        code.matches("loop back-edges: ").count(),
    ]
}

fn quality_from_artifacts(
    model: &ProgramModel,
    pseudo: &[PseudocodeArtifact],
    opt: &DecompileOptions,
    // Records decoded, before any record split. The ratio's denominator is the
    // model's function list, so counting split pieces in the numerator would
    // compare unlike things and report a fraction above one.
    decoded_records: usize,
) -> QualityReport {
    let function_count = model.functions.len();
    let disassembled_function_count = decoded_records;

    let mut total_calls = 0usize;
    let mut indirect_calls = 0usize;
    let mut placeholder_ifs = 0usize;
    let mut unresolved_cf = 0usize;
    let mut raw_register_calls = 0usize;
    let mut semantic_direct_calls = 0usize;
    let mut semantic_indirect_calls = 0usize;
    let mut dispatch_selector_calls = 0usize;
    let mut dispatch_table_calls = 0usize;
    let mut repeated_blocks = 0usize;
    let mut unlifted_instructions = 0usize;
    let mut target_va_symbol_calls = 0usize;
    let mut block_helper_refs = 0usize;
    let mut raw_arg_name_refs = 0usize;
    let mut raw_register_name_refs = 0usize;
    let mut placeholder_cond_markers = 0usize;
    let mut omitted_path_markers = 0usize;
    let mut loop_backedge_markers = 0usize;

    for p in pseudo {
        total_calls += p.total_calls;
        indirect_calls += p.indirect_calls;
        placeholder_ifs += p.placeholder_ifs;
        unresolved_cf += p.unresolved_cf;
        raw_register_calls += p.raw_register_calls;
        semantic_direct_calls += p.semantic_direct_calls;
        semantic_indirect_calls += p.semantic_indirect_calls;
        dispatch_selector_calls += p.dispatch_selector_calls;
        dispatch_table_calls += p.dispatch_table_calls;
        repeated_blocks += p.repeated_blocks;
        unlifted_instructions += p.unlifted_instructions;
        target_va_symbol_calls += p.target_va_symbol_calls;
        for line in p.source.lines() {
            let [helpers, args, registers, conds, omitted, backedges] =
                source_text_counters(line);
            block_helper_refs += helpers;
            raw_arg_name_refs += args;
            raw_register_name_refs += registers;
            placeholder_cond_markers += conds;
            omitted_path_markers += omitted;
            loop_backedge_markers += backedges;
        }
    }

    // Deliberately `decoded_records`, not `disasm.len()`: with a record split the
    // latter counts recovered functions the model never declared, which would push
    // this above 1.0 and turn a minimum gate into a meaningless one.
    let disassembly_ratio = if function_count == 0 {
        0.0
    } else {
        disassembled_function_count as f64 / function_count as f64
    };
    let indirect_call_ratio = if total_calls == 0 {
        0.0
    } else {
        indirect_calls as f64 / total_calls as f64
    };

    let mut failures = Vec::new();
    if placeholder_ifs > opt.max_placeholder_ifs {
        failures.push("placeholder if-count exceeded threshold".to_string());
    }
    if unresolved_cf > opt.max_unresolved_cf {
        failures.push("unresolved control-flow count exceeded threshold".to_string());
    }
    if indirect_call_ratio > opt.max_indirect_call_ratio {
        failures.push("indirect call ratio exceeded threshold".to_string());
    }
    if disassembly_ratio < opt.min_disassembly_ratio {
        failures.push("disassembly ratio below threshold".to_string());
    }

    QualityReport {
        mode: "strict".to_string(),
        passed: failures.is_empty(),
        failures,
        function_count,
        disassembled_function_count,
        disassembly_ratio,
        total_calls,
        indirect_calls,
        indirect_call_ratio,
        placeholder_ifs,
        unresolved_cf,
        raw_register_calls,
        semantic_direct_calls,
        semantic_indirect_calls,
        dispatch_selector_calls,
        dispatch_table_calls,
        repeated_blocks,
        unlifted_instructions,
        target_va_symbol_calls,
        block_helper_refs,
        raw_arg_name_refs,
        raw_register_name_refs,
        placeholder_cond_markers,
        omitted_path_markers,
        loop_backedge_markers,
    }
}


#[cfg(test)]
mod quality_tests {
    use super::*;

    #[test]
    fn annotation_span_does_not_contribute_to_source_counters() {
        // Built by the shared literal rather than spelled here: a fixture that
        // hand-writes the span it is testing keeps passing on its own copy after
        // the emitter has moved, which is the drift this whole boundary exists
        // to prevent.
        let annotation = flutterdec_decompiler::EXHAUSTIVE_JOIN_ANNOTATION
            .render(&["arg3", "_block_7()"]);
        let source = format!(
            "sink(reg0{annotation}); /* cond */ // omitted complex path; loop back-edges: x1"
        );
        let code = flutterdec_decompiler::strip_join_annotation_span(&source);
        assert_eq!(count_ident_token(&code, "reg0"), 1);
        assert_eq!(count_ident_token(&code, "arg3"), 0);
        assert_eq!(code.matches("_block_").count(), 0);
        assert_eq!(code.matches("/* cond */").count(), 1);
        assert_eq!(code.matches("omitted complex path").count(), 1);
        assert_eq!(code.matches("loop back-edges: ").count(), 1);
    }

    #[test]
    fn stripping_is_a_noop_for_pre_annotation_source() {
        let source = "sink(reg0 /* cond */); // 3 instructions not lifted: arg2 _block_9";
        assert_eq!(flutterdec_decompiler::strip_join_annotation_span(source), source);
    }

    /// One fixture per annotation literal, driven off the shared table so a
    /// fifth literal cannot be added without being scored here. Each annotation
    /// carries a sentinel for every source counter that reads identifiers -
    /// `arg0`, `_block_`, a bare register spelling - and the line carries an
    /// unrelated `/* cond */` outside the span. Annotation recovers nothing, so
    /// all six counters must read exactly what the un-annotated line reads: a
    /// delta in either direction is contamination.
    #[test]
    fn no_annotation_literal_moves_a_source_counter() {
        let bare = "  sink(reg0); /* cond */ // omitted complex path; loop back-edges: x1";
        let expected = source_text_counters(bare);
        for literal in flutterdec_decompiler::ANNOTATION_LITERALS {
            for values in [
                vec!["arg0"],
                vec!["arg0", "_block_7()"],
                vec!["arg0", "_block_7()", "reg9"],
            ] {
                let annotated = bare.replacen("reg0", &format!("reg0{}", literal.render(&values)), 1);
                assert_eq!(
                    source_text_counters(&annotated),
                    expected,
                    "annotation `{}` moved a source counter in `{annotated}`",
                    literal.open()
                );
            }
        }
        assert_eq!(
            expected,
            [0, 0, 2, 1, 1, 1],
            "the fixture must actually exercise the counters it pins"
        );
    }

    /// An unrelated comment is not evidence and must survive: the strip parser
    /// keys on the annotation openers alone, so the historical ruler still sees
    /// every comment the emitter wrote before annotation existed.
    #[test]
    fn an_unrelated_cond_comment_survives_stripping() {
        let source = "  if (/* cond */) { sink(reg0 /* pool[7] */); } // loop back-edges: x1";
        assert_eq!(
            flutterdec_decompiler::strip_join_annotation_span(source),
            source
        );
        assert_eq!(
            source_text_counters(source)[3],
            1,
            "the cond marker must still count"
        );
    }
}
