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
    let mut emission = EmissionReport::default();

    for p in pseudo {
        emission.irreducible += p.emission.cause_count(StructuredDeclineCause::Irreducible);
        emission.unsupported_region += p
            .emission
            .cause_count(StructuredDeclineCause::UnsupportedRegion);
        emission.repeat_budget += p.emission.cause_count(StructuredDeclineCause::RepeatBudget);
        emission.structured_depth_budget += p
            .emission
            .cause_count(StructuredDeclineCause::StructuredDepthBudget);
        emission.coverage_mismatch += p
            .emission
            .cause_count(StructuredDeclineCause::CoverageMismatch);
        emission.dfs_depth_omissions += p
            .emission
            .event_count(TraversalEventKind::DfsDepthOmission);
        emission.dfs_visit_omissions += p
            .emission
            .event_count(TraversalEventKind::DfsVisitOmission);
        emission.helper_cap_omissions += p
            .emission
            .event_count(TraversalEventKind::HelperCapOmission);
        // Derived from the causes above, one function at a time, so the totals
        // are sums of the same primary facts and not a second tally.
        emission.structured_declines += p.emission.structured_declines();
        emission.structured_rollbacks += p.emission.rollbacks();
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
        emission,
    }
}


// The whole-pipeline control-effect ruler. A separate, digest-protected file rather than
// part of `quality_tests` below, because this file is product source that later
// work edits, so a digest over it would fire on legitimate change. This
// declaration is the only thing that compiles that file and cannot be digested
// either, so `scripts/check-oracle-inventory.py` proves it by compilation.
#[cfg(test)]
#[path = "quality/control_effect_tests.rs"]
mod quality_control_effect_tests;

#[cfg(test)]
mod quality_tests {
    use super::*;

    /// A two-block cycle entered from two sides, which the region analysis
    /// refuses, and a diamond, which it does not. One program, one declined
    /// function, so the report's derived counts have something to derive from.
    fn declining_and_structured_artifacts() -> Vec<PseudocodeArtifact> {
        use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};

        let build = |function_id: u64, succs: &[Vec<usize>]| {
            let mut blocks: Vec<BasicBlock> = succs
                .iter()
                .enumerate()
                .map(|(id, succs)| {
                    let start = 0x1000 + 0x10 * id as u64;
                    let mut instrs = Vec::new();
                    match succs.as_slice() {
                        [] => instrs.push(LlirInstr {
                            va: start,
                            op: IROp::Return,
                            src: "ret".to_string(),
                            target: String::new(),
                        }),
                        [only] => instrs.push(LlirInstr {
                            va: start,
                            op: IROp::Jump,
                            src: format!("b #{:#x}", 0x1000 + 0x10 * *only as u64),
                            target: format!("#{:#x}", 0x1000 + 0x10 * *only as u64),
                        }),
                        [_fallthrough, taken] => instrs.push(LlirInstr {
                            va: start,
                            op: IROp::Branch,
                            src: format!("cbz x0, #{:#x}", 0x1000 + 0x10 * *taken as u64),
                            target: format!("#{:#x}", 0x1000 + 0x10 * *taken as u64),
                        }),
                        _ => unreachable!("fixtures branch at most two ways"),
                    }
                    BasicBlock {
                        id,
                        start_va: start,
                        instrs,
                        succs: succs.clone(),
                        preds: Vec::new(),
                    }
                })
                .collect();
            rebuild_edges(&mut blocks);
            FunctionIr {
                function_id,
                name: format!("fixture{function_id}"),
                entry_va: 0x1000,
                blocks,
            }
        };

        let irreducible = build(1, &[vec![1, 2], vec![2, 3], vec![1, 3], Vec::new()]);
        let diamond = build(2, &[vec![1, 2], vec![3], vec![3], Vec::new()]);
        let symbols = HashMap::new();
        vec![
            flutterdec_decompiler::emit_pseudocode(&irreducible, &symbols),
            flutterdec_decompiler::emit_pseudocode(&diamond, &symbols),
        ]
    }

    fn empty_model() -> ProgramModel {
        ProgramModel {
            schema_version: 3,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: String::new(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: Vec::new(),
            pool_geometry: None,
        }
    }

    fn default_options() -> DecompileOptions {
        DecompileOptions {
            out_dir: std::path::PathBuf::new(),
            emit_asm: false,
            emit_asm_opcodes: false,
            emit_ghidra_script: false,
            emit_ida_script: false,
            emit_ir: false,
            split_records: false,
            extra_symbol_elfs: Vec::new(),
            extra_symbol_map_targets: Vec::new(),
            include_nearest_symbol_map: false,
            focus: None,
            function_target: None,
            max_functions: None,
            max_placeholder_ifs: 0,
            max_unresolved_cf: 0,
            max_indirect_call_ratio: 0.30,
            min_disassembly_ratio: 0.80,
            function_scope: FunctionScope::All,
            app_packages: Vec::new(),
            adapter_backend: AdapterBackend::Internal,
            require_snapshot_hash_match: false,
            analysis_profile: DecompileAnalysisProfile::Balanced,
            engine_options: DecompileEngineOptions::for_profile(DecompileAnalysisProfile::Balanced),
        }
    }

    /// The report's generic decline count and rollback count are sums of the
    /// primary causes, never a second tally kept beside them.
    #[test]
    fn the_report_derives_its_decline_counts_from_the_primary_causes() {
        let pseudo = declining_and_structured_artifacts();
        let report = quality_from_artifacts(&empty_model(), &pseudo, &default_options(), 0);
        let emission = &report.emission;

        let per_cause = emission.irreducible
            + emission.unsupported_region
            + emission.repeat_budget
            + emission.structured_depth_budget
            + emission.coverage_mismatch;
        assert_eq!(
            emission.structured_declines, per_cause,
            "the decline count is the sum of the causes"
        );
        assert_eq!(
            emission.structured_rollbacks,
            emission.repeat_budget + emission.structured_depth_budget + emission.coverage_mismatch,
            "only post-mutation causes roll anything back"
        );
        assert_eq!(emission.irreducible, 1, "one fixture is irreducible");
        assert_eq!(
            emission.structured_declines, 1,
            "the other fixture structures"
        );
        assert_eq!(
            emission.structured_rollbacks, 0,
            "an irreducible decline is preflight"
        );

        let events: usize = pseudo.iter().map(|p| p.emission.events().len()).sum();
        assert_eq!(
            emission.dfs_depth_omissions
                + emission.dfs_visit_omissions
                + emission.helper_cap_omissions,
            events,
            "every traversal event is counted under exactly one kind"
        );
    }

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
