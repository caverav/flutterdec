#![recursion_limit = "512"]

use anyhow::{bail, Context, Result};
use flutterdec_adapter::{
    list_adapters, resolve_adapter_exec, run_adapter, AdapterInput, ProgramModel,
};
use flutterdec_decompiler::{emit_program_with_runtime_stubs, PseudocodeArtifact};
use flutterdec_disasm_arm64::{
    disassemble_program_with_priorities_and_package_hints, FunctionDisassembly,
    FunctionPriorityBreakdown,
};
use flutterdec_ir::{build_program_ir, FunctionIr};
use flutterdec_loader::{
    load_snapshot_bundle, load_snapshot_bundle_from_apk_session, ApkSession, SnapshotBundle,
};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DecompileOptions {
    pub out_dir: PathBuf,
    pub emit_asm: bool,
    pub emit_asm_opcodes: bool,
    pub emit_ghidra_script: bool,
    pub emit_ida_script: bool,
    pub emit_ir: bool,
    /// Split a function record that spans more than one real function.
    /// Opt-in: it multiplies the emitted function count, which moves every
    /// absolute quality counter and makes the model-derived disassembly ratio
    /// compare unlike things.
    pub split_records: bool,
    pub extra_symbol_elfs: Vec<PathBuf>,
    pub extra_symbol_map_targets: Vec<PathBuf>,
    pub include_nearest_symbol_map: bool,
    pub focus: Option<String>,
    pub function_target: Option<FunctionTarget>,
    pub max_functions: Option<usize>,
    pub max_placeholder_ifs: usize,
    pub max_unresolved_cf: usize,
    pub max_indirect_call_ratio: f64,
    pub min_disassembly_ratio: f64,
    pub function_scope: FunctionScope,
    pub app_packages: Vec<String>,
    pub adapter_backend: AdapterBackend,
    pub require_snapshot_hash_match: bool,
    pub analysis_profile: DecompileAnalysisProfile,
    pub engine_options: DecompileEngineOptions,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum FunctionTarget {
    FunctionId(u64),
    EntryVa(u64),
    Any(u64),
}

impl FunctionTarget {
    pub fn kind(self) -> &'static str {
        match self {
            Self::FunctionId(_) => "function-id",
            Self::EntryVa(_) => "entry-va",
            Self::Any(_) => "any",
        }
    }

    pub fn value(self) -> u64 {
        match self {
            Self::FunctionId(v) => v,
            Self::EntryVa(v) => v,
            Self::Any(v) => v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiffOptions {
    pub out_dir: PathBuf,
    pub adapter_backend: AdapterBackend,
    pub function_scope: FunctionScope,
    pub app_packages: Vec<String>,
    pub require_snapshot_hash_match: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum FunctionScope {
    AppUnknown,
    App,
    All,
}

impl FunctionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppUnknown => "app-unknown",
            Self::App => "app",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum DecompileAnalysisProfile {
    Light,
    Balanced,
}

impl DecompileAnalysisProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Balanced => "balanced",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum AdapterBackend {
    /// Try each snapshot-aware backend in turn, then fall back to the internal one.
    Auto,
    /// String carving plus prologue scanning. No real names, no real ObjectPool.
    Internal,
    Blutter,
    /// `r2flutter` (MIT, radareorg): deserializes the AOT snapshot, so it is the only
    /// backend that supplies exact names and an authoritative ObjectPool index space.
    R2Flutter,
}

impl AdapterBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Internal => "internal",
            Self::Blutter => "blutter",
            Self::R2Flutter => "r2flutter",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DecompileEngineOptionOverrides {
    pub canonical_model_symbols: Option<bool>,
    pub pool_value_hints: Option<bool>,
    pub pool_semantic_hints: Option<bool>,
    pub semantic_reporting: Option<bool>,
    pub bootflow_category_seeds: Option<bool>,
    pub apk_startup_analysis: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecompileEngineOptions {
    pub canonical_model_symbols: bool,
    pub pool_value_hints: bool,
    pub pool_semantic_hints: bool,
    pub semantic_reporting: bool,
    pub bootflow_category_seeds: bool,
    pub apk_startup_analysis: bool,
}

impl DecompileEngineOptions {
    pub fn for_profile(profile: DecompileAnalysisProfile) -> Self {
        match profile {
            DecompileAnalysisProfile::Light => Self {
                canonical_model_symbols: false,
                pool_value_hints: false,
                pool_semantic_hints: false,
                semantic_reporting: false,
                bootflow_category_seeds: false,
                apk_startup_analysis: false,
            },
            DecompileAnalysisProfile::Balanced => Self {
                canonical_model_symbols: true,
                pool_value_hints: true,
                pool_semantic_hints: true,
                semantic_reporting: true,
                bootflow_category_seeds: true,
                apk_startup_analysis: true,
            },
        }
    }

    pub fn with_overrides(mut self, overrides: &DecompileEngineOptionOverrides) -> Self {
        if let Some(v) = overrides.canonical_model_symbols {
            self.canonical_model_symbols = v;
        }
        if let Some(v) = overrides.pool_value_hints {
            self.pool_value_hints = v;
        }
        if let Some(v) = overrides.pool_semantic_hints {
            self.pool_semantic_hints = v;
        }
        if let Some(v) = overrides.semantic_reporting {
            self.semantic_reporting = v;
        }
        if let Some(v) = overrides.bootflow_category_seeds {
            self.bootflow_category_seeds = v;
        }
        if let Some(v) = overrides.apk_startup_analysis {
            self.apk_startup_analysis = v;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InfoOutput {
    pub input_path: String,
    pub libapp_path: String,
    pub arch: String,
    pub snapshot_hash: String,
    /// Dart SDK version behind `snapshot_hash`, when the hash is tabulated.
    pub dart_version: Option<String>,
    /// Object-header tag encoding for that version (`CID_INT32`, `CID_SHIFT1`,
    /// `OBJECT_HEADER`); the layout dimension most likely to break a parser.
    pub dart_tag_style: Option<String>,
    /// Whether the snapshot was built with compressed pointers, read from the
    /// features string in its header rather than inferred from the code. It
    /// decides the width of a reference field and the value of `kSmiBits`, so it
    /// selects which offset tables apply. `None` means the header did not parse
    /// and nothing may be assumed.
    pub compressed_pointers: Option<bool>,
    /// The snapshot's features string verbatim, when the header parsed.
    pub snapshot_features: Option<String>,
    pub adapter_installed: bool,
    pub adapter_kind: Option<String>,
    pub manifest_entry_present: Option<bool>,
    pub adapter_snapshot_hash_match: Option<bool>,
    pub compatibility_warnings: Option<Vec<String>>,
    pub function_count: Option<usize>,
    pub class_count: Option<usize>,
    pub object_pool_count: Option<usize>,
    pub app_package_count_total: Option<usize>,
    pub app_package_counts_top: Option<Vec<PackageCount>>,
    pub android_startup_present: Option<bool>,
    pub android_startup_confidence: Option<String>,
    pub android_startup_entrypoint_count: Option<usize>,
    pub android_startup_flutter_activity_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageCount {
    pub package: String,
    pub functions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityReport {
    pub mode: String,
    pub passed: bool,
    pub failures: Vec<String>,
    pub function_count: usize,
    pub disassembled_function_count: usize,
    pub disassembly_ratio: f64,
    pub total_calls: usize,
    pub indirect_calls: usize,
    pub indirect_call_ratio: f64,
    pub placeholder_ifs: usize,
    pub unresolved_cf: usize,
    pub raw_register_calls: usize,
    pub semantic_direct_calls: usize,
    pub semantic_indirect_calls: usize,
    pub dispatch_selector_calls: usize,
    pub dispatch_table_calls: usize,
    pub repeated_blocks: usize,
    pub unlifted_instructions: usize,
    pub target_va_symbol_calls: usize,
    pub block_helper_refs: usize,
    pub raw_arg_name_refs: usize,
    pub raw_register_name_refs: usize,
    pub placeholder_cond_markers: usize,
    pub omitted_path_markers: usize,
    pub loop_backedge_markers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    pub old_input_path: String,
    pub new_input_path: String,
    pub old_snapshot_hash: String,
    pub new_snapshot_hash: String,
    pub old_snapshot_hash_match: bool,
    pub new_snapshot_hash_match: bool,
    pub require_snapshot_hash_match: bool,
    pub old_dart_version: String,
    pub new_dart_version: String,
    pub function_scope: String,
    pub app_packages: Vec<String>,
    pub old_function_count: usize,
    pub new_function_count: usize,
    pub common_function_count: usize,
    pub added_function_count: usize,
    pub removed_function_count: usize,
    pub added_functions_top: Vec<String>,
    pub removed_functions_top: Vec<String>,
    pub added_packages_top: Vec<PackageCount>,
    pub removed_packages_top: Vec<PackageCount>,
    pub report_path: String,
}

include!("pipeline/helpers.rs");
include!("pipeline/model.rs");
include!("pipeline/quality.rs");
include!("pipeline/bootflow_hints.rs");
include!("pipeline/apk_startup.rs");
include!("pipeline/runners_scripts.rs");
include!("pipeline/runners_diff.rs");
include!("pipeline/runners.rs");
include!("pipeline/symbol_map.rs");
include!("pipeline/engine_fingerprint.rs");

/// Serialization-span entry point for the phase benchmark.
///
/// The harness times what the decompile runner serializes out of finished
/// artifacts, so this calls that code rather than restating it: a harness with
/// its own copy of the serialization rules would keep reporting the same number
/// after the pipeline moved. Compiled out without the `bench-spans` feature.
#[cfg(feature = "bench-spans")]
pub mod bench_spans {
    use super::runners_reporting::{
        collect_call_fallback_summary, collect_selector_fallback_summary,
        collect_semantic_intent_summary,
    };
    use super::{
        quality_from_artifacts, terminated, AdapterBackend, DecompileAnalysisProfile,
        DecompileEngineOptions, DecompileOptions, FunctionScope,
    };
    use flutterdec_adapter::ProgramModel;
    use flutterdec_decompiler::PseudocodeArtifact;
    use flutterdec_ir::FunctionIr;
    use serde_json::json;
    use std::path::PathBuf;

    /// A model carrying the one field the serialized quality report reads out of
    /// it: the function count its disassembly ratio divides by. Built once,
    /// outside every measured span, because fixture construction is excluded.
    pub fn synthetic_model(function_count: usize) -> ProgramModel {
        ProgramModel {
            schema_version: 1,
            adapter_kind: "bench".to_string(),
            dart_version: "bench".to_string(),
            snapshot_hash: String::new(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: (0..function_count)
                .map(|i| flutterdec_adapter::FunctionInfo {
                    id: i as u64,
                    name: format!("bench{i}"),
                    owner_class: "Global".to_string(),
                    entry_va: 0,
                    size: 0,
                    code_section_va: 0,
                    name_kind: None,
                })
                .collect(),
            object_pool: Vec::new(),
            pool_geometry: None,
        }
    }

    /// The gate thresholds and engine switches a default `decompile` run uses,
    /// so the timed report is the one the CLI would produce. `balanced` is the
    /// profile that turns semantic reporting on; under `light` the report's
    /// artifact-derived section is skipped entirely and the span would measure
    /// almost nothing.
    pub fn balanced_options() -> DecompileOptions {
        DecompileOptions {
            out_dir: PathBuf::new(),
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

    /// Everything the runner turns finished artifacts into: pseudocode text,
    /// emitted IR JSON, the quality report JSON, and the artifact-derived
    /// section of `report.json`. Disk IO is excluded, so each `fs::write` in the
    /// runner becomes the byte vector that write would have taken.
    ///
    /// Returns the total byte count. Without a returned value the whole span is
    /// dead code and a release build is free to delete it.
    pub fn serialize_artifacts(
        ir: &[FunctionIr],
        pseudo: &[PseudocodeArtifact],
        model: &ProgramModel,
        opt: &DecompileOptions,
        decoded_records: usize,
    ) -> usize {
        let mut bytes = 0usize;

        for artifact in pseudo {
            bytes += terminated(&artifact.source).len();
        }

        for function in ir {
            let mut body = serde_json::to_vec_pretty(function).expect("ir is serializable");
            body.push(b'\n');
            bytes += body.len();
        }

        let report = quality_from_artifacts(model, pseudo, opt, decoded_records);
        bytes += serde_json::to_vec_pretty(&report)
            .expect("quality report is serializable")
            .len();

        let semantic_intent = collect_semantic_intent_summary(pseudo);
        let call_fallback = collect_call_fallback_summary(pseudo);
        let selector_fallback = collect_selector_fallback_summary(pseudo);
        let selector_fallback_top = selector_fallback
            .top
            .iter()
            .map(|entry| {
                json!({
                    "selector": entry.selector,
                    "count": entry.count,
                    "sample": entry.sample
                })
            })
            .collect::<Vec<_>>();
        let semantic_total = report.semantic_direct_calls
            + report.semantic_indirect_calls
            + report.dispatch_selector_calls;
        let semantic_ratio = if report.total_calls == 0 {
            0.0
        } else {
            semantic_total as f64 / report.total_calls as f64
        };
        let summary = json!({
            "semantic_intent": {
                "framework": semantic_intent.framework,
                "stdlib": semantic_intent.stdlib,
                "runtime": semantic_intent.runtime,
                "native": semantic_intent.native,
                "selector_tagged": semantic_intent.selector_tagged,
                "constructor_calls": semantic_intent.constructor_calls
            },
            "selector_fallback": {
                "total": selector_fallback.total,
                "unique": selector_fallback.unique,
                "top": selector_fallback_top
            },
            "call_fallback": {
                "dynamic_call": call_fallback.dynamic_call,
                "dispatch_invoke": call_fallback.dispatch_invoke,
                "dispatch_target_invoke": call_fallback.dispatch_target_invoke,
                "generic_invoke": call_fallback.generic_invoke
            },
            "semantic_total": semantic_total,
            "semantic_ratio": semantic_ratio
        });
        bytes += serde_json::to_vec_pretty(&summary)
            .expect("report summary is serializable")
            .len();

        bytes
    }
}
