#![recursion_limit = "512"]

use anyhow::{bail, Context, Result};
use flutterdec_adapter::model::{
    Capabilities, CapabilityLevel, CodeRange, CompatibilityBinding, Diagnostic, DiagnosticCode,
    DiagnosticSeverity, Domain, Function, FunctionId, InputRegion, InputRegionName, ObjectPool,
    ObservedInput, Producer, ProducerTrust, ProgramModel, Provenance,
};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::protocol::{BackendId, FallbackReason, RequestedBackend};
use flutterdec_adapter::store::{self, StoreEntry};
use flutterdec_adapter::validate;
use flutterdec_adapter::{
    run_adapter, AdapterInput, AdapterRegionInput, ContainmentReport, HostAuthorization, HostError,
    LibappSource, Limits,
};
use flutterdec_decompiler::{emit_program_with_runtime_stubs, PseudocodeArtifact};
use flutterdec_disasm_arm64::{
    disassemble_program_with_priorities_and_package_hints, FunctionDisassembly,
    FunctionPriorityBreakdown, HintKind, HintOrigin, HintProvenance, ProgramHints,
};
use flutterdec_ir::{build_program_ir, FunctionIr};
use flutterdec_loader::dart_profile::{ResolvedDartProfile, SdkAlias};
use flutterdec_loader::identity::IdentityRejection;
use flutterdec_loader::layout::Layout;
use flutterdec_loader::registry::RegistryError;
use flutterdec_loader::{
    load_snapshot_bundle, load_snapshot_bundle_from_apk_session, ApkSession, SnapshotBundle,
};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    /// Wall-clock deadline for one adapter invocation. `None` keeps the host
    /// default.
    pub adapter_timeout_seconds: Option<u64>,
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
    /// Wall-clock deadline for one adapter invocation, applied to each side.
    pub adapter_timeout_seconds: Option<u64>,
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
    /// Legacy display marker (`unverified` when an alias exists); SDK aliases
    /// are provenance only and never compatibility selectors.
    pub dart_version: Option<String>,
    pub dart_aliases: Option<Vec<SdkAlias>>,
    /// Object-header tag encoding for the selected profile.
    pub dart_tag_style: Option<String>,
    /// features string in its header rather than inferred from the code. It
    /// decides the width of a reference field and the value of `kSmiBits`, so it
    /// selects which offset tables apply. `None` means the header did not parse
    /// and nothing may be assumed.
    pub compressed_pointers: Option<bool>,
    /// The snapshot's features string verbatim, when the header parsed.
    pub snapshot_features: Option<String>,
    /// Whether a verified adapter artifact is installed for the selected record.
    pub adapter_installed: bool,
    /// Who produced the model and under what authorization. Always present:
    /// core recovers the program itself when no adapter is authorized, so there
    /// is always a provider to describe.
    pub provider: Option<ProviderReport>,
    /// What the operator asked for.
    pub requested_backend: Option<String>,
    /// What actually answered, as the protocol result reported it.
    pub resolved_backend: Option<String>,
    /// Why the two differ, when the request was `auto`.
    pub backend_fallback_reason: Option<String>,
    pub producer_id: Option<String>,
    pub producer_trust: Option<String>,
    pub compatibility_record_sha256: Option<String>,
    pub registry_record_present: Option<bool>,
    /// Which containment controls were established for the adapter child, as
    /// the child itself reported them. Absent when no adapter ran.
    pub adapter_containment: Option<ContainmentReport>,
    /// Whether the snapshot identity came out of a real header. Replaces the v3
    /// "does the adapter agree about the hash" check, which compared a host fact
    /// against a string the adapter chose.
    pub snapshot_identity_is_exact: Option<bool>,
    /// Why no adapter was selected, when the identity gate refused the snapshot.
    ///
    /// `Some` means no registry record was selected, no executable was
    /// resolved, and no adapter ran.
    pub identity_rejection: Option<String>,
    /// An adapter that was authorized, ran, and failed. Reported rather than
    /// swallowed: `info` used to drop this on the floor and print a report that
    /// looked like a snapshot with nothing in it.
    pub adapter_error: Option<String>,
    /// The stable category of `adapter_error`.
    pub adapter_error_category: Option<String>,
    /// Per-domain capability levels the model reported.
    pub model_capabilities: Option<BTreeMap<String, String>>,
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

/// Everything an operator needs to know about who produced a model.
///
/// One struct rather than one set of fields per command, because `info`,
/// `decompile` and each side of a `diff` all have to answer the same questions
/// and used to answer them in three different shapes. Every field here is a
/// host fact or a protocol fact; none of it is read out of a filename or a
/// substring of adapter output.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderReport {
    /// What the operator asked for.
    pub requested_backend: String,
    /// What produced the model, as the protocol result named it, or `internal`
    /// when core recovered the program itself.
    pub resolved_backend: String,
    /// Set when a pinned request was answered by something else. Always `false`
    /// for `auto`, which pins nothing.
    pub backend_mismatch: bool,
    /// Why a producer used a backend other than the one it prefers.
    pub backend_fallback_reason: Option<String>,
    /// Why no adapter was executed at all. `Some` means zero adapter processes
    /// existed for this input.
    pub core_fallback_reason: Option<String>,
    /// The condition behind `core_fallback_reason`, verbatim.
    pub core_fallback_detail: Option<String>,
    /// The stable sentence explaining what that fallback costs.
    pub core_fallback_effect: Option<String>,
    pub adapter_executed: bool,
    pub adapter_exec_path: Option<String>,
    pub producer_id: String,
    pub producer_version: String,
    pub producer_artifact_sha256: String,
    pub producer_trust: String,
    pub registry_record_present: bool,
    pub compatibility_record_sha256: Option<String>,
    pub parser_family_id: Option<String>,
    pub profile_id: Option<String>,
    pub profile_sha256: Option<String>,
    pub artifact_id: Option<String>,
    pub artifact_sha256: Option<String>,
    /// The machine this ran on, which is not the machine the snapshot targets.
    pub host_os: String,
    pub host_arch: String,
    /// The architecture the snapshot's code was generated for, from the ELF
    /// container rather than from adapter output.
    pub target_arch: String,
    pub snapshot_identity_is_exact: bool,
    /// Why the identity may not authorize an adapter, when it may not.
    pub identity_rejection: Option<String>,
    /// Per-domain capability levels the model reported.
    pub capabilities: BTreeMap<String, String>,
    /// Which containment controls were established for the adapter child, as
    /// the child itself reported them. Absent when no adapter ran.
    pub containment: Option<ContainmentReport>,
    pub warnings: Vec<String>,
}

/// A stable token for what went wrong, for operators matching on outcomes.
///
/// The message is for humans and may change. This may not: it is the difference
/// between a script retrying a timeout and a script reporting a corrupt store.
pub fn error_category(error: &anyhow::Error) -> &'static str {
    for cause in error.chain() {
        if let Some(host) = cause.downcast_ref::<HostError>() {
            return host_error_category(host);
        }
        if let Some(registry) = cause.downcast_ref::<RegistryError>() {
            return registry_error_category(registry);
        }
        if cause.downcast_ref::<IdentityRejection>().is_some() {
            return "identity_rejected";
        }
    }
    "unclassified"
}

fn host_error_category(error: &HostError) -> &'static str {
    match error {
        HostError::IdentityRejected(_) => "identity_rejected",
        HostError::RecordInvalid(_) => "record_invalid",
        HostError::RecordDigestMismatch { .. } => "record_digest_mismatch",
        HostError::UnsupportedMajors { .. } => "unsupported_majors",
        HostError::IdentityRecordMismatch { .. } => "identity_record_mismatch",
        HostError::TargetMismatch { .. } => "target_mismatch",
        HostError::FeatureMismatch { .. } => "feature_mismatch",
        HostError::HostVariantMismatch { .. } => "host_variant_mismatch",
        HostError::VariantNotInRecord { .. } => "variant_not_in_record",
        HostError::ArtifactPathRejected(_) => "artifact_path_rejected",
        HostError::ArtifactNotExecutable(_) => "artifact_not_executable",
        HostError::ArtifactDigestMismatch { .. } => "artifact_digest_mismatch",
        HostError::ProfileRejected(_) => "profile_rejected",
        HostError::ProducerMismatch(_) => "producer_mismatch",
        HostError::BindingMismatch(_) => "binding_mismatch",
        HostError::InputRejected(_) => "input_rejected",
        HostError::RequestRejected(_) => "request_rejected",
        HostError::OutputHandleRejected(_) => "output_handle_rejected",
        HostError::ImageNotSealed(_) => "image_not_sealed",
        HostError::Workspace(_) => "workspace_failed",
        HostError::Spawn(_) => "spawn_failed",
        HostError::Timeout { .. } => "adapter_timeout",
        HostError::OutputLimitExceeded { .. } => "adapter_output_limit_exceeded",
        HostError::Crashed { .. } => "adapter_crashed",
        HostError::NoResult { .. } => "adapter_no_result",
        HostError::DocumentTooLarge { .. } => "adapter_document_too_large",
        HostError::MalformedDocument { .. } => "adapter_malformed_document",
        HostError::ResultMismatch(_) => "adapter_result_mismatch",
        HostError::ModelPathMismatch { .. } => "adapter_model_path_mismatch",
        HostError::AdapterFailed { .. } => "adapter_reported_failure",
        HostError::ModelRejected(_) => "adapter_model_rejected",
        HostError::ContainmentUnreported => "containment_unreported",
        HostError::Io(_) => "adapter_io",
    }
}

fn registry_error_category(error: &RegistryError) -> &'static str {
    match error {
        RegistryError::Malformed(_) => "registry_malformed",
        RegistryError::UnsupportedVersion(_) => "registry_unsupported_version",
        RegistryError::Identity(_) => "identity_rejected",
        RegistryError::NoRecord(_) => "registry_no_record",
        RegistryError::TargetMismatch { .. } => "registry_target_mismatch",
        RegistryError::FeatureMismatch { .. } => "registry_feature_mismatch",
        RegistryError::Ambiguous(_) => "registry_ambiguous",
        RegistryError::InvalidRecord(_) => "registry_invalid_record",
        RegistryError::Profile(_) => "registry_profile_rejected",
        RegistryError::ArtifactAbsent(_) => "registry_artifact_absent",
        RegistryError::Artifact(_) => "registry_artifact_rejected",
    }
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
    pub old_dart_aliases: Vec<SdkAlias>,
    pub new_dart_aliases: Vec<SdkAlias>,
    /// Who produced each side. Reported per side rather than once, because the
    /// two sides are selected independently and a diff between an adapter model
    /// and a core-recovered one compares unlike things.
    pub old_provider: ProviderReport,
    pub new_provider: ProviderReport,
    /// Set when the two sides were not produced the same way, which is the one
    /// condition that makes the counts below misleading rather than merely
    /// incomplete.
    pub provider_mismatch: bool,
    /// Functions with no name, owner or library on each side. An address alone
    /// is not stable across builds, so these are counted and excluded rather
    /// than collapsed into one descriptor that reads as "unchanged".
    pub old_uncomparable_function_count: usize,
    pub new_uncomparable_function_count: usize,
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
include!("pipeline/fallback.rs");
include!("pipeline/model.rs");
include!("pipeline/quality.rs");
include!("pipeline/bootflow_hints.rs");
include!("pipeline/apk_startup.rs");
include!("pipeline/runners_scripts.rs");
include!("pipeline/runners_diff.rs");
include!("pipeline/runners.rs");
include!("pipeline/symbol_map.rs");
include!("pipeline/engine_fingerprint.rs");
