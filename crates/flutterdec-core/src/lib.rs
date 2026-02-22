use anyhow::{bail, Context, Result};
use flutterdec_adapter::{
    list_adapters, resolve_adapter_exec, run_adapter, AdapterInput, ProgramModel,
};
use flutterdec_decompiler::{emit_program_with_pool_context, PseudocodeArtifact};
use flutterdec_disasm_arm64::{disassemble_program, FunctionDisassembly};
use flutterdec_ir::{build_program_ir, FunctionIr};
use flutterdec_loader::{load_snapshot_bundle, SnapshotBundle};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DecompileOptions {
    pub out_dir: PathBuf,
    pub emit_asm: bool,
    pub emit_ir: bool,
    pub extra_symbol_elfs: Vec<PathBuf>,
    pub extra_symbol_map_targets: Vec<PathBuf>,
    pub include_nearest_symbol_map: bool,
    pub focus: Option<String>,
    pub max_functions: Option<usize>,
    pub max_placeholder_ifs: usize,
    pub max_unresolved_cf: usize,
    pub max_indirect_call_ratio: f64,
    pub min_disassembly_ratio: f64,
    pub function_scope: FunctionScope,
    pub app_packages: Vec<String>,
    pub analysis_profile: DecompileAnalysisProfile,
    pub engine_options: DecompileEngineOptions,
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

#[derive(Debug, Clone, Default)]
pub struct DecompileEngineOptionOverrides {
    pub canonical_model_symbols: Option<bool>,
    pub pool_value_hints: Option<bool>,
    pub pool_semantic_hints: Option<bool>,
    pub semantic_reporting: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecompileEngineOptions {
    pub canonical_model_symbols: bool,
    pub pool_value_hints: bool,
    pub pool_semantic_hints: bool,
    pub semantic_reporting: bool,
}

impl DecompileEngineOptions {
    pub fn for_profile(profile: DecompileAnalysisProfile) -> Self {
        match profile {
            DecompileAnalysisProfile::Light => Self {
                canonical_model_symbols: false,
                pool_value_hints: false,
                pool_semantic_hints: false,
                semantic_reporting: false,
            },
            DecompileAnalysisProfile::Balanced => Self {
                canonical_model_symbols: true,
                pool_value_hints: true,
                pool_semantic_hints: true,
                semantic_reporting: true,
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
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InfoOutput {
    pub input_path: String,
    pub libapp_path: String,
    pub arch: String,
    pub snapshot_hash: String,
    pub adapter_installed: bool,
    pub function_count: Option<usize>,
    pub class_count: Option<usize>,
    pub object_pool_count: Option<usize>,
    pub app_package_count_total: Option<usize>,
    pub app_package_counts_top: Option<Vec<PackageCount>>,
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
    pub target_va_symbol_calls: usize,
    pub block_helper_refs: usize,
    pub raw_arg_name_refs: usize,
    pub raw_register_name_refs: usize,
    pub placeholder_cond_markers: usize,
    pub omitted_path_markers: usize,
    pub loop_backedge_markers: usize,
}

include!("pipeline/helpers.rs");
include!("pipeline/model.rs");
include!("pipeline/quality.rs");
include!("pipeline/runners.rs");
include!("pipeline/symbol_map.rs");
include!("pipeline/engine_fingerprint.rs");
