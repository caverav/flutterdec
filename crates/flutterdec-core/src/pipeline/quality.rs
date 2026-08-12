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
        // Join annotations are reader-facing evidence, not emitted code. Strip
        // only their exact spans: all historical comments stay in the ruler, so
        // pre-annotation reports remain bit-for-bit comparable.
        for line in p.source.lines() {
            let code = flutterdec_decompiler::strip_join_annotation_span(line);
            block_helper_refs += code.matches("_block_").count();
            placeholder_cond_markers += code.matches("/* cond */").count();
            omitted_path_markers += code.matches("omitted complex path").count();
            loop_backedge_markers += code.matches("loop back-edges: ").count();
            for n in 0..=7 {
                raw_arg_name_refs += count_ident_token(&code, &format!("arg{n}"));
            }
            for n in 0..=30 {
                // `xN` is the disassembly spelling; the emitter renders an
                // unresolved register through `named_register_alias`, which yields
                // `regN`. Counting only `xN` reported zero on every real binary
                // while thousands of `regN` were being emitted.
                raw_register_name_refs += count_ident_token(&code, &format!("x{n}"));
                raw_register_name_refs += count_ident_token(&code, &format!("reg{n}"));
            }
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
        let source = "sink(reg0 /* = arg3 | _block_7() */); /* cond */ // omitted complex path; loop back-edges: x1";
        let code = flutterdec_decompiler::strip_join_annotation_span(source);
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
}
