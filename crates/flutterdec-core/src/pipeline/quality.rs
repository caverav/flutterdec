fn quality_from_artifacts(
    model: &ProgramModel,
    disasm: &[FunctionDisassembly],
    pseudo: &[PseudocodeArtifact],
    opt: &DecompileOptions,
) -> QualityReport {
    let function_count = model.functions.len();
    let disassembled_function_count = disasm.len();

    let mut total_calls = 0usize;
    let mut indirect_calls = 0usize;
    let mut placeholder_ifs = 0usize;
    let mut unresolved_cf = 0usize;
    let mut raw_register_calls = 0usize;
    let mut semantic_direct_calls = 0usize;
    let mut semantic_indirect_calls = 0usize;
    let mut dispatch_selector_calls = 0usize;
    let mut dispatch_table_calls = 0usize;
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
        target_va_symbol_calls += p.target_va_symbol_calls;
        block_helper_refs += p.source.matches("_block_").count();
        placeholder_cond_markers += p.source.matches("/* cond */").count();
        omitted_path_markers += p.source.matches("omitted complex path").count();
        loop_backedge_markers += p.source.matches("loop back-edges: ").count();
        for n in 0..=7 {
            raw_arg_name_refs += count_ident_token(&p.source, &format!("arg{n}"));
        }
        for n in 0..=30 {
            raw_register_name_refs += count_ident_token(&p.source, &format!("x{n}"));
        }
    }

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
        target_va_symbol_calls,
        block_helper_refs,
        raw_arg_name_refs,
        raw_register_name_refs,
        placeholder_cond_markers,
        omitted_path_markers,
        loop_backedge_markers,
    }
}
