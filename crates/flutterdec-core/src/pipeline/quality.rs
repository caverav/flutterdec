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
        arg_refs += flutterdec_decompiler::count_code_identifier_tokens(&code, &format!("arg{n}"));
    }
    let mut register_refs = 0usize;
    for n in 0..=30 {
        // `xN` is the disassembly spelling; the emitter renders an unresolved
        // register through `named_register_alias`, which yields `regN`. Counting
        // only `xN` reported zero on every real binary while thousands of `regN`
        // were being emitted.
        register_refs +=
            flutterdec_decompiler::count_code_identifier_tokens(&code, &format!("x{n}"));
        register_refs +=
            flutterdec_decompiler::count_code_identifier_tokens(&code, &format!("reg{n}"));
    }
    [
        flutterdec_decompiler::count_code_matches(&code, "_block_"),
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

    use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};

    /// Five instruction slots per block, so a fixture can pad one without its
    /// instructions landing on the next block's address.
    fn block_va(id: usize) -> u64 {
        0x1000 + 0x20 * id as u64
    }

    /// A graph from successor lists alone: no successors is a return, one is a
    /// jump, two is `cbz x0` whose taken edge is the second entry.
    fn graph(function_id: u64, succs: &[Vec<usize>]) -> FunctionIr {
        let mut blocks: Vec<BasicBlock> = succs
            .iter()
            .enumerate()
            .map(|(id, succs)| {
                let start = block_va(id);
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
                        src: format!("b #{:#x}", block_va(*only)),
                        target: format!("#{:#x}", block_va(*only)),
                    }),
                    [_fallthrough, taken] => instrs.push(LlirInstr {
                        va: start,
                        op: IROp::Branch,
                        src: format!("cbz x0, #{:#x}", block_va(*taken)),
                        target: format!("#{:#x}", block_va(*taken)),
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
            entry_va: block_va(0),
            blocks,
        }
    }

    /// A spine whose every block branches to one shared sink.
    ///
    /// The sink is nobody's follow node, so structuring declines and the DFS
    /// walk runs. That walk exceeds its depth budget on the spine and its visit
    /// budget on the sink, and cuts the rest of the spine into helper bodies, so
    /// the block count is how this fixture chooses which traversal limits it
    /// reaches.
    fn fan_in(function_id: u64, n: usize) -> FunctionIr {
        let sink = n - 1;
        let succs: Vec<Vec<usize>> = (0..n)
            .map(|id| {
                if id == sink {
                    Vec::new()
                } else if id + 1 == sink {
                    vec![sink]
                } else {
                    vec![id + 1, sink]
                }
            })
            .collect();
        graph(function_id, &succs)
    }

    /// A two-block cycle entered from two sides, which no dominator makes a loop.
    fn irreducible_fixture(function_id: u64) -> FunctionIr {
        graph(function_id, &[vec![1, 2], vec![2, 3], vec![1, 3], Vec::new()])
    }

    /// A reachable block with three successors: the walk renders a taken arm and
    /// one not-taken arm, so the third edge has no rendering at all.
    fn unsupported_region_fixture(function_id: u64) -> FunctionIr {
        let mut ir = graph(function_id, &[vec![1, 2], Vec::new(), Vec::new()]);
        if let Some(entry) = ir.blocks.iter_mut().find(|b| b.id == 0) {
            entry.succs = vec![1, 2, 3];
        }
        ir.blocks.push(BasicBlock {
            id: 3,
            start_va: block_va(3),
            instrs: vec![LlirInstr {
                va: block_va(3),
                op: IROp::Return,
                src: "ret".to_string(),
                target: String::new(),
            }],
            succs: Vec::new(),
            preds: vec![0],
        });
        ir
    }

    /// A spine whose blocks all branch into one shared region larger than the
    /// repeat budget, and whose last block returns without entering it, so the
    /// region is nobody's follow node and would have to be repeated.
    fn repeat_budget_fixture(function_id: u64) -> FunctionIr {
        let spine = 6usize;
        let region = 17usize;
        let mut succs: Vec<Vec<usize>> = Vec::new();
        for i in 0..spine {
            if i + 1 < spine {
                succs.push(vec![i + 1, spine]);
            } else {
                succs.push(Vec::new());
            }
        }
        for r in 0..region {
            if r + 1 < region {
                succs.push(vec![spine + r + 1]);
            } else {
                succs.push(Vec::new());
            }
        }
        graph(function_id, &succs)
    }

    /// A chain of conditionals whose arms never rejoin, so every one of them
    /// nests inside the last: region depth grows by one per block.
    ///
    /// The spine is longer than the structured walk's depth budget, which is
    /// crate-private to the decompiler. The cause assertion below is what pins
    /// the number: a budget change fails this fixture rather than quietly
    /// re-scoping it.
    fn depth_budget_fixture(function_id: u64) -> FunctionIr {
        let spine = 70usize;
        let mut succs: Vec<Vec<usize>> = Vec::new();
        for i in 0..spine {
            succs.push(vec![i + 1, spine + i]);
        }
        succs.push(Vec::new());
        for _ in 0..spine {
            succs.push(Vec::new());
        }
        graph(function_id, &succs)
    }

    /// A complete binary tree whose leaves all reach one shared sink, with two
    /// leaves entering a two-block cycle from different sides so the graph is
    /// irreducible and the DFS walk is the one that runs.
    ///
    /// The sink is padded past the short-tail case, so its visit budget is the
    /// 24 of a shared block rather than the 48 of a short tail, and the walk
    /// reaches it more often than that.
    fn visit_omission_fixture(function_id: u64) -> FunctionIr {
        let sink = 63usize;
        let (left, right) = (64usize, 65usize);
        let mut succs: Vec<Vec<usize>> = Vec::new();
        for id in 0..sink {
            if id < 31 {
                succs.push(vec![2 * id + 1, 2 * id + 2]);
            } else if id == 31 {
                succs.push(vec![left]);
            } else if id == 32 {
                succs.push(vec![right]);
            } else {
                succs.push(vec![sink]);
            }
        }
        succs.push(Vec::new());
        succs.push(vec![sink, right]);
        succs.push(vec![sink, left]);
        let mut ir = graph(function_id, &succs);
        let block = ir.blocks.iter_mut().find(|b| b.id == sink).expect("sink");
        let mut ret = block.instrs.pop().expect("terminator");
        for offset in 0..3u64 {
            block.instrs.push(LlirInstr {
                va: block_va(sink) + offset * 4,
                op: IROp::RuntimeCheck,
                src: "cmp x0, #0".to_string(),
                target: String::new(),
            });
        }
        ret.va = block_va(sink) + 12;
        block.instrs.push(ret);
        ir
    }

    /// A jump that leaves the function while the graph still records a
    /// successor: the walk ends there, so a reachable block is never emitted.
    fn coverage_mismatch_fixture(function_id: u64) -> FunctionIr {
        let mut ir = graph(function_id, &[vec![1], Vec::new()]);
        if let Some(entry) = ir.blocks.iter_mut().find(|b| b.id == 0) {
            if let Some(terminator) = entry.instrs.last_mut() {
                terminator.src = "b #0x50000".to_string();
                terminator.target = "#0x50000".to_string();
            }
        }
        ir
    }

    /// Blocks past the helper definition budget, so the budget refuses one and
    /// a `HelperCapOmission` is recorded. Mirrors the decompiler's own boundary
    /// fixture; the event assertion below is what keeps it honest.
    const BLOCKS_PAST_HELPER_BUDGET: usize = 784;

    /// One program carrying a fixture for every primary decline cause and every
    /// traversal event family.
    ///
    /// The report derives its counts by summing over artifacts, so a derivation
    /// is only checked by a program that actually produces the thing being
    /// summed. With two fixtures, four cause counters and two event counters
    /// were zero on both sides of every assertion, and a report that dropped
    /// them entirely kept the whole suite green.
    fn taxonomy_artifacts() -> Vec<PseudocodeArtifact> {
        let symbols = HashMap::new();
        [
            irreducible_fixture(1),
            unsupported_region_fixture(2),
            repeat_budget_fixture(3),
            depth_budget_fixture(4),
            coverage_mismatch_fixture(5),
            // A plain diamond, which structures: the control for the causes.
            graph(6, &[vec![1, 2], vec![3], vec![3], Vec::new()]),
            // Depth omissions.
            fan_in(7, 128),
            // Visit omissions.
            visit_omission_fixture(9),
            // Helper cap omissions.
            fan_in(8, BLOCKS_PAST_HELPER_BUDGET),
        ]
        .iter()
        .map(|ir| flutterdec_decompiler::emit_pseudocode(ir, &symbols))
        .collect()
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

    /// Every primary cause and every traversal event family reaches the report,
    /// so no counter is checked against a zero it would hold anyway.
    ///
    /// Each cause is asserted against the fixture that produces it, which is
    /// also what pins the fixtures: a fixture that stopped reaching its cause
    /// fails here rather than turning its counter into a silent zero.
    #[test]
    fn the_report_counts_every_primary_cause_and_every_traversal_event() {
        let pseudo = taxonomy_artifacts();
        let report = quality_from_artifacts(&empty_model(), &pseudo, &default_options(), 0);
        let emission = &report.emission;

        // Stated per cause, not derived: a fixture that stopped reaching its
        // cause has to fail here rather than turn its counter into a zero that
        // the assertion still agrees with. The three event fixtures decline as
        // well on their way to the traversal limits they exist for - the two
        // fan-ins past the structured depth budget, the visit fixture on its
        // irreducible cycle - so two counters are above one.
        for (label, counted, expected) in [
            ("irreducible", emission.irreducible, 2),
            ("unsupported region", emission.unsupported_region, 1),
            ("repeat budget", emission.repeat_budget, 1),
            ("structured depth budget", emission.structured_depth_budget, 3),
            ("coverage mismatch", emission.coverage_mismatch, 1),
        ] {
            assert_eq!(
                counted, expected,
                "the report must count {expected} {label} declines"
            );
        }
        for (label, counted) in [
            ("dfs depth", emission.dfs_depth_omissions),
            ("dfs visit", emission.dfs_visit_omissions),
            ("helper cap", emission.helper_cap_omissions),
        ] {
            assert!(
                counted > 0,
                "no fixture reaches a {label} omission, so its counter proves nothing"
            );
        }

        // Each counter is the sum of that same fact over the artifacts, so a
        // report that kept its own tally, or dropped a cause, disagrees here.
        let sum_cause = |cause| {
            pseudo
                .iter()
                .map(|p| p.emission.cause_count(cause))
                .sum::<usize>()
        };
        let sum_event = |kind| {
            pseudo
                .iter()
                .map(|p| p.emission.event_count(kind))
                .sum::<usize>()
        };
        assert_eq!(
            [
                emission.irreducible,
                emission.unsupported_region,
                emission.repeat_budget,
                emission.structured_depth_budget,
                emission.coverage_mismatch,
            ],
            StructuredDeclineCause::ALL.map(sum_cause),
            "each cause counter is that cause, summed over the artifacts"
        );
        assert_eq!(
            [
                emission.dfs_depth_omissions,
                emission.dfs_visit_omissions,
                emission.helper_cap_omissions,
            ],
            TraversalEventKind::ALL.map(sum_event),
            "each event counter is that event kind, summed over the artifacts"
        );
    }

    /// The report's generic decline count and rollback count are sums of the
    /// primary causes, never a second tally kept beside them.
    #[test]
    fn the_report_derives_its_decline_counts_from_the_primary_causes() {
        let pseudo = taxonomy_artifacts();
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
        // Stated rather than derived, so a fixture that stopped declining, or a
        // sixth one that started, moves these instead of agreeing with itself.
        assert_eq!(emission.structured_declines, 8, "eight fixtures decline");
        assert_eq!(
            emission.structured_rollbacks, 5,
            "the three preflight declines roll nothing back"
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
        assert_eq!(flutterdec_decompiler::count_code_identifier_tokens(&code, "reg0"), 1);
        assert_eq!(flutterdec_decompiler::count_code_identifier_tokens(&code, "arg3"), 0);
        assert_eq!(flutterdec_decompiler::count_code_matches(&code, "_block_"), 0);
        assert_eq!(code.matches("/* cond */").count(), 1);
        assert_eq!(code.matches("omitted complex path").count(), 1);
        assert_eq!(code.matches("loop back-edges: ").count(), 1);
    }

    #[test]
    fn recovered_data_does_not_contribute_to_raw_counters() {
        let source = r#"sink(reg19, reg8Minus1, \"arg0 reg28 _block_999\"); // target: reg1"#;
        assert_eq!(source_text_counters(source)[0..3], [0, 0, 1]);
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
    /// all six counters must read exactly what the un-annotated code reads: a
    /// delta in either direction is contamination. Comment spellings are not
    /// source tokens and therefore do not contribute to the expected counts.
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
            [0, 0, 1, 1, 1, 1],
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
