//! Whole-pipeline control-effect assertions: the serialized IR that `--emit-ir`
//! writes and any later consumer reads, and the quality report an unrecovered
//! control effect has to surface in.
//!
//! Test-only, and protected by digest in section 7 of
//! `docs/oracle-protocol-ir-cfg-emitter.md`. It is deliberately not part of
//! `quality_tests` in `quality.rs`: that file is product source which later work
//! edits, so it cannot carry a digest.

use super::*;

fn ins(va: u64, mnemonic: &str, op_str: &str) -> flutterdec_disasm_arm64::AsmInstruction {
    flutterdec_disasm_arm64::AsmInstruction {
        va,
        word: 0,
        mnemonic: mnemonic.to_string(),
        op_str: op_str.to_string(),
        annotation: String::new(),
    }
}

/// One record carrying every ARM64 control effect that has to survive the
/// pipeline: a conditional branch with both its edges, a call that keeps its
/// fallthrough, a return, an indirect branch, and a trap.
fn control_effect_record() -> flutterdec_disasm_arm64::FunctionDisassembly {
    flutterdec_disasm_arm64::FunctionDisassembly {
        function_id: 0,
        function_name: "effects".to_string(),
        owner_class: "Global".to_string(),
        entry_va: 0x1000,
        size: 0x20,
        instructions: vec![
            ins(0x1000, "cbz", "x0, #0x1010"),
            ins(0x1004, "bl", "#0x8000"),
            ins(0x1008, "cbz", "x1, #0x1018"),
            ins(0x100c, "ret", ""),
            ins(0x1010, "ldur", "x16, [x24, #7]"),
            ins(0x1014, "br", "x16"),
            ins(0x1018, "brk", "#0x1"),
        ],
    }
}

fn one_function_model() -> ProgramModel {
    ProgramModel {
        schema_version: 3,
        adapter_kind: "test".to_string(),
        dart_version: "unknown".to_string(),
        snapshot_hash: String::new(),
        arch: "arm64".to_string(),
        libraries: Vec::new(),
        classes: Vec::new(),
        functions: vec![flutterdec_adapter::FunctionInfo {
            id: 0,
            name: "effects".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 0x20,
            code_section_va: 0x1000,
            name_kind: None,
        }],
        object_pool: Vec::new(),
        pool_geometry: None,
    }
}

fn options(max_unresolved_cf: usize) -> DecompileOptions {
    DecompileOptions {
        out_dir: std::path::PathBuf::new(),
        emit_asm: false,
        emit_asm_opcodes: false,
        emit_ghidra_script: false,
        emit_ida_script: false,
        emit_ir: true,
        split_records: false,
        extra_symbol_elfs: Vec::new(),
        extra_symbol_map_targets: Vec::new(),
        include_nearest_symbol_map: false,
        focus: None,
        function_target: None,
        max_functions: None,
        max_placeholder_ifs: 0,
        max_unresolved_cf,
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

/// The serialized IR is what `--emit-ir` writes and what any later consumer
/// reads, so a control effect that is right in memory and wrong on disk is
/// still wrong. Operations and edges are both asserted: the operation names
/// the effect, the edge list is what a fabricated fallthrough would show up
/// in.
#[test]
fn serialized_ir_states_every_control_effect_and_its_edges() {
    let ir = flutterdec_ir::build_program_ir(&[control_effect_record()]);
    let json = serde_json::to_value(&ir[0]).expect("function ir serializes");
    let blocks = json["blocks"].as_array().expect("blocks are an array");

    let by_start = |start: u64| {
        blocks
            .iter()
            .find(|b| b["start_va"].as_u64() == Some(start))
            .unwrap_or_else(|| panic!("no block starts at {start:#x}: {json}"))
    };
    let ops = |start: u64| {
        by_start(start)["instrs"]
            .as_array()
            .expect("instructions are an array")
            .iter()
            .map(|i| i["op"].as_str().expect("an op name").to_string())
            .collect::<Vec<_>>()
    };
    let succs = |start: u64| {
        by_start(start)["succs"]
            .as_array()
            .expect("successors are an array")
            .iter()
            .map(|s| s.as_u64().expect("a block id"))
            .collect::<Vec<_>>()
    };
    let id_of = |start: u64| by_start(start)["id"].as_u64().expect("a block id");

    assert_eq!(blocks.len(), 5, "one block per control effect: {json}");
    assert_eq!(ops(0x1000), vec!["Branch"]);
    assert_eq!(
        succs(0x1000),
        vec![id_of(0x1004), id_of(0x1010)],
        "a conditional branch keeps its target and its fallthrough"
    );
    assert_eq!(
        ops(0x1004),
        vec!["Call", "Branch"],
        "a call does not end its block"
    );
    assert_eq!(
        succs(0x1004),
        vec![id_of(0x100c), id_of(0x1018)],
        "the call's fallthrough is the next instruction, and no edge names the callee"
    );
    assert_eq!(ops(0x100c), vec!["Return"]);
    assert!(succs(0x100c).is_empty(), "a return leaves the function");
    assert_eq!(ops(0x1010), vec!["Other", "IndirectBranch"]);
    assert!(
        succs(0x1010).is_empty(),
        "an indirect branch serializes with no edge: {json}"
    );
    assert_eq!(ops(0x1018), vec!["Trap"]);
    assert!(
        succs(0x1018).is_empty(),
        "a trap serializes with no edge: {json}"
    );
}

/// The whole pipeline on the same record: disassembly to IR, IR to
/// pseudocode, artifacts to the quality report. An unknown control effect
/// has to be visible at the end of it, in the counter that exists to say so.
#[test]
fn the_pipeline_reports_an_indirect_branch_as_unresolved_control_flow() {
    let ir = flutterdec_ir::build_program_ir(&[control_effect_record()]);
    let pseudo = flutterdec_decompiler::emit_program(&ir, &HashMap::new());
    let source = &pseudo[0].source;

    assert!(
        source.contains("// indirect branch through reg16: target not recovered"),
        "the artifact must state the indirect branch:\n{source}"
    );
    assert!(
        source.contains("// trap: control does not continue"),
        "the artifact must state the trap:\n{source}"
    );
    assert!(
        !source.contains("tailCall_"),
        "an unrecovered target is not a tail call:\n{source}"
    );
    assert_eq!(
        source.matches("return ").count(),
        1,
        "the one `ret` is the only return in the artifact:\n{source}"
    );

    let model = one_function_model();
    let strict = quality_from_artifacts(&model, &pseudo, &options(0), 1);
    assert_eq!(
        strict.unresolved_cf, 1,
        "the indirect branch is counted once: {strict:?}"
    );
    assert_eq!(strict.total_calls, 1, "the `bl` is the only call");
    assert_eq!(strict.indirect_calls, 0, "a `br` is not a call");
    assert!(
        strict
            .failures
            .contains(&"unresolved control-flow count exceeded threshold".to_string()),
        "an unknown control effect fails a zero-tolerance gate: {strict:?}"
    );
    assert!(!strict.passed);

    let tolerant = quality_from_artifacts(&model, &pseudo, &options(1), 1);
    assert!(
        tolerant.passed,
        "nothing else in this record fails a gate: {tolerant:?}"
    );
}
