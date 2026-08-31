#[path = "runners/reporting.rs"]
mod runners_reporting;
use runners_reporting::{
    collect_bootflow_discovery, collect_call_fallback_summary, collect_semantic_intent_summary,
    collect_selector_fallback_summary, BootflowDiscoveryEntry, BootflowDiscoverySummary,
    CallFallbackSummary, SelectorFallbackSummary, SemanticIntentSummary,
};
#[path = "runners/split.rs"]
mod runners_split;
use runners_split::{split_inflated_records, SplitStats};

#[path = "runners/stubs.rs"]
mod runners_stubs;
use runners_stubs::{prune_calls_that_never_return, shared_stub_names};

/// Why the shared-stub naming produced the count it did, for the report.
struct SharedStubNamingSummary {
    status: String,
    named: usize,
    allocation_named: usize,
    scanned: usize,
}
#[path = "runners/manifest.rs"]
mod runners_manifest;
use runners_manifest::{
    collect_manifest_bootflow_hints, inspect_android_manifest,
    inspect_android_manifest_from_apk_session, AndroidManifestSignals,
};
#[path = "runners/symbols.rs"]
mod runners_symbols;
use runners_symbols::{
    build_pool_semantic_hints, build_pool_target_symbols,
    build_pool_value_hints, canonical_standard_model_name, collect_pool_metadata_stats,
    collect_symbol_quality_counts, merge_symbol_name,
    symbol_name_quality_from_provenance, SymbolMergeStats, SymbolNameQuality,
    SymbolQualityCounts,
};
#[cfg(test)]
use runners_symbols::{is_generic_symbol_name, normalize_external_symbol_name};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopedFunctionKind {
    App,
    Framework,
    Stdlib,
    Unknown,
}

#[derive(Debug, Clone)]
struct FunctionScopeStats {
    total_before_filter: usize,
    total_after_filter: usize,
    excluded: usize,
    app: usize,
    framework: usize,
    stdlib: usize,
    unknown: usize,
    excluded_by_app_package: usize,
}

impl FunctionScopeStats {
    fn from_total(total: usize) -> Self {
        Self {
            total_before_filter: total,
            total_after_filter: total,
            excluded: 0,
            app: 0,
            framework: 0,
            stdlib: 0,
            unknown: 0,
            excluded_by_app_package: 0,
        }
    }
}

/// How the model's function names were recovered, counted.
///
/// `unnamed` replaces v3's `placeholder`: a function with no name is now a
/// distinct, countable state instead of one carrying `sub_1234` and a
/// `name_kind` claiming that was a placeholder.
#[derive(Debug, Default, Clone, Copy)]
struct FunctionNameProvenanceStats {
    exact: usize,
    derived: usize,
    heuristic: usize,
    unnamed: usize,
}

impl FunctionNameProvenanceStats {
    fn named(self) -> usize {
        self.exact + self.derived + self.heuristic
    }
}

fn collect_function_name_provenance_stats(
    functions: &[flutterdec_adapter::model::Function],
) -> FunctionNameProvenanceStats {
    let mut stats = FunctionNameProvenanceStats::default();
    for f in functions {
        let Some(name) = f.name.as_ref() else {
            stats.unnamed += 1;
            continue;
        };
        match name.provenance {
            flutterdec_adapter::model::Provenance::Exact => stats.exact += 1,
            flutterdec_adapter::model::Provenance::Derived => stats.derived += 1,
            flutterdec_adapter::model::Provenance::Heuristic => stats.heuristic += 1,
        }
    }
    stats
}

#[derive(Debug, Default, Clone)]
struct EngineFingerprintContext {
    detected: bool,
    source: Option<String>,
    machine: Option<String>,
    machine_id: Option<u16>,
    machine_matches_bundle_arch: Option<bool>,
    build_id: Option<String>,
    candidate_flutter_version: Option<String>,
    candidate_dart_version: Option<String>,
    confidence: Option<String>,
    symbol_count: Option<usize>,
    dyn_symbol_count: Option<usize>,
    exec_section_count: Option<usize>,
    error: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct EngineSymbolIngestion {
    enabled: bool,
    match_kind: Option<String>,
    loaded_paths: Vec<PathBuf>,
    applied_target_count: usize,
    manifest_path: Option<String>,
    error: Option<String>,
}

fn backend_label(value: Option<AdapterBackend>) -> &'static str {
    match value {
        Some(backend) => backend.as_str(),
        None => "unknown",
    }
}

fn format_quality_gate_failure_message(
    report: &QualityReport,
    quality_path: &Path,
    report_path: &Path,
    input_path: &Path,
    resolved_backend: Option<AdapterBackend>,
    symbol_quality_counts: &SymbolQualityCounts,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "quality gate failed after artifact generation. see {} and {}",
        quality_path.display(),
        report_path.display()
    );
    if !report.failures.is_empty() {
        let _ = writeln!(out, "reasons: {}", report.failures.join("; "));
    }
    let _ = writeln!(
        out,
        "summary: placeholder_ifs={} unresolved_cf={} indirect_call_ratio={:.3} disassembly_ratio={:.3}",
        report.placeholder_ifs,
        report.unresolved_cf,
        report.indirect_call_ratio,
        report.disassembly_ratio
    );

    let mut notes: Vec<String> = Vec::new();
    if !is_apk_input_path(input_path) {
        notes.push("input is not an APK, so manifest/startup evidence is unavailable".to_string());
    }
    if resolved_backend == Some(AdapterBackend::Internal) {
        notes.push(
            "resolved backend is internal: no exact names and no ObjectPool index space"
                .to_string(),
        );
    }
    if symbol_quality_counts.placeholder > 0
        && symbol_quality_counts.exact == 0
        && symbol_quality_counts.external == 0
        && symbol_quality_counts.heuristic == 0
    {
        notes.push("all recovered function names are still placeholders".to_string());
    }
    if !notes.is_empty() {
        let _ = writeln!(out, "context: {}", notes.join("; "));
    }

    out.push_str(
        "artifacts were still written. for exploratory runs while flutterdec is still maturing, you can relax the gates, for example:\n",
    );
    out.push_str(
        "  flutterdec decompile <input> -o <out> \\\n    --max-placeholder-ifs 999999 \\\n    --max-unresolved-cf 999999 \\\n    --max-indirect-call-ratio 1.0 \\\n    --min-disassembly-ratio 0.0\n",
    );
    out.push_str(
        "you can also improve recovery by decompiling the APK instead of raw libapp.so, using a stronger backend, or providing matched engine symbols.",
    );
    out
}

fn is_apk_input_path(input_path: &Path) -> bool {
    input_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"))
}

fn open_apk_session_if_input_is_apk(input_path: &Path) -> Result<Option<ApkSession>> {
    if is_apk_input_path(input_path) {
        return ApkSession::open(input_path).map(Some);
    }
    Ok(None)
}

fn load_snapshot_bundle_with_optional_apk_session(
    input_path: &Path,
    apk_session: Option<&ApkSession>,
) -> Result<SnapshotBundle> {
    if let Some(apk) = apk_session {
        return load_snapshot_bundle_from_apk_session(input_path, apk);
    }
    load_snapshot_bundle(input_path)
}

fn bundle_arch_matches_machine_id(bundle_arch: &str, machine_id: u16) -> bool {
    match bundle_arch {
        "arm64" => machine_id == goblin::elf::header::EM_AARCH64,
        _ => false,
    }
}

fn fingerprint_engine_path(path: &Path, bundle_arch: &str, source: String) -> EngineFingerprintContext {
    let mut ctx = EngineFingerprintContext {
        source: Some(source),
        ..EngineFingerprintContext::default()
    };
    match run_engine_fingerprint(
        path,
        &EngineFingerprintOptions {
            out_dir: None,
            max_markers: 24,
        },
    ) {
        Ok(report) => {
            ctx.detected = true;
            ctx.machine_matches_bundle_arch =
                Some(bundle_arch_matches_machine_id(bundle_arch, report.machine_id));
            ctx.machine = Some(report.machine);
            ctx.machine_id = Some(report.machine_id);
            ctx.build_id = report.build_id;
            ctx.candidate_flutter_version = report.candidate_flutter_version;
            ctx.candidate_dart_version = report.candidate_dart_version;
            ctx.confidence = Some(report.confidence);
            ctx.symbol_count = Some(report.symbol_count);
            ctx.dyn_symbol_count = Some(report.dyn_symbol_count);
            ctx.exec_section_count = Some(report.exec_section_count);
        }
        Err(err) => {
            ctx.error = Some(err.to_string());
        }
    }
    ctx
}

fn find_libflutter_in_apk_session(apk: &ApkSession) -> Result<Option<(String, Vec<u8>)>> {
    let preferred = ["lib/arm64-v8a/libflutter.so", "base/lib/arm64-v8a/libflutter.so"];
    for want in preferred {
        if apk.entry_names().iter().any(|name| name == want) {
            let out = apk
                .read_entry(want)
                .context("read preferred libflutter from apk")?;
            return Ok(Some((want.to_string(), out)));
        }
    }

    for name in apk.entry_names() {
        if name.ends_with("/libflutter.so") || name == "libflutter.so" {
            let out = apk
                .read_entry(name)
                .context("read fallback libflutter from apk")?;
            return Ok(Some((name.clone(), out)));
        }
    }

    Ok(None)
}

fn try_collect_engine_fingerprint_with_apk_session(
    input_path: &Path,
    apk_session: Option<&ApkSession>,
    bundle_arch: &str,
) -> EngineFingerprintContext {
    if is_apk_input_path(input_path) {
        let libflutter = match apk_session {
            Some(apk) => find_libflutter_in_apk_session(apk),
            None => match ApkSession::open(input_path) {
                Ok(apk) => find_libflutter_in_apk_session(&apk),
                Err(err) => {
                    return EngineFingerprintContext {
                        detected: false,
                        source: None,
                        error: Some(format!("open apk session for engine fingerprint: {err}")),
                        ..EngineFingerprintContext::default()
                    };
                }
            },
        };
        match libflutter {
            Ok(Some((entry_name, bytes))) => {
                let tmp = match NamedTempFile::new() {
                    Ok(t) => t,
                    Err(err) => {
                        return EngineFingerprintContext {
                            detected: false,
                            source: Some(format!("apk:{entry_name}")),
                            error: Some(format!("create temp file for engine fingerprint: {err}")),
                            ..EngineFingerprintContext::default()
                        };
                    }
                };
                if let Err(err) = fs::write(tmp.path(), bytes) {
                    return EngineFingerprintContext {
                        detected: false,
                        source: Some(format!("apk:{entry_name}")),
                        error: Some(format!("write temp libflutter for engine fingerprint: {err}")),
                        ..EngineFingerprintContext::default()
                    };
                }
                return fingerprint_engine_path(tmp.path(), bundle_arch, format!("apk:{entry_name}"));
            }
            Ok(None) => {
                return EngineFingerprintContext {
                    detected: false,
                    source: None,
                    error: Some("libflutter.so not found in APK".to_string()),
                    ..EngineFingerprintContext::default()
                };
            }
            Err(err) => {
                return EngineFingerprintContext {
                    detected: false,
                    source: None,
                    error: Some(err.to_string()),
                    ..EngineFingerprintContext::default()
                };
            }
        }
    }

    let mut candidates = Vec::new();
    if let Some(parent) = input_path.parent() {
        candidates.push(parent.join("libflutter.so"));
    }
    candidates.push(input_path.with_file_name("libflutter.so"));

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        return fingerprint_engine_path(
            &candidate,
            bundle_arch,
            format!("filesystem:{}", candidate.display()),
        );
    }

    EngineFingerprintContext {
        detected: false,
        source: None,
        error: Some("libflutter.so not found near input".to_string()),
        ..EngineFingerprintContext::default()
    }
}

#[cfg(test)]
fn try_collect_engine_fingerprint(input_path: &Path, bundle_arch: &str) -> EngineFingerprintContext {
    try_collect_engine_fingerprint_with_apk_session(input_path, None, bundle_arch)
}

fn resolve_local_engine_symbol_targets(
    repo_root: &Path,
    input_path: &Path,
    bundle_arch: &str,
    engine_context: &EngineFingerprintContext,
) -> EngineSymbolIngestion {
    let ext = input_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "apk" || !engine_context.detected {
        return EngineSymbolIngestion::default();
    }

    let build_id = engine_context.build_id.as_deref();
    let flutter_version = if build_id.is_none() {
        engine_context.candidate_flutter_version.as_deref()
    } else {
        None
    };

    match resolve_local_symbol_cache_paths(repo_root, bundle_arch, build_id, flutter_version) {
        Ok(resolution) => EngineSymbolIngestion {
            enabled: true,
            match_kind: resolution.match_kind,
            loaded_paths: resolution.paths,
            applied_target_count: 0,
            manifest_path: resolution
                .manifest_path
                .as_ref()
                .map(|path| path.display().to_string()),
            error: resolution.error,
        },
        Err(err) => EngineSymbolIngestion {
            enabled: true,
            error: Some(err.to_string()),
            ..EngineSymbolIngestion::default()
        },
    }
}

fn classify_library_uri(uri: &str) -> ScopedFunctionKind {
    let t = uri.trim();
    if t.starts_with("package:flutter/") {
        return ScopedFunctionKind::Framework;
    }
    if t.starts_with("dart:") {
        return ScopedFunctionKind::Stdlib;
    }
    if t.starts_with("package:") {
        return ScopedFunctionKind::App;
    }
    ScopedFunctionKind::Unknown
}

fn function_kind_from_model(
    model: &ProgramModel,
    f: &flutterdec_adapter::model::Function,
) -> ScopedFunctionKind {
    let Some(uri) = model.owner_library_uri(f) else {
        return ScopedFunctionKind::Unknown;
    };
    classify_library_uri(uri)
}

fn normalize_package_name(raw: &str) -> Option<String> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }
    let token = token.strip_prefix("package:").unwrap_or(token);
    let name = token.split('/').next().unwrap_or_default().trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

fn package_name_from_library_uri(uri: &str) -> Option<String> {
    let raw = uri.strip_prefix("package:")?;
    let name = raw.split('/').next().unwrap_or_default().trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

fn normalize_package_filters(values: &[String]) -> HashSet<String> {
    values
        .iter()
        .filter_map(|v| normalize_package_name(v))
        .collect()
}

fn is_non_app_manifest_segment(segment: &str) -> bool {
    matches!(
        segment,
        "com"
            | "org"
            | "net"
            | "io"
            | "app"
            | "dev"
            | "android"
            | "androidx"
            | "example"
    )
}

fn normalized_manifest_segment(segment: &str) -> Option<String> {
    let lowered = segment.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }
    let valid = lowered
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return None;
    }
    Some(lowered)
}

fn push_manifest_hint(hints: &mut HashSet<String>, candidate: &str) {
    let Some(normalized) = normalized_manifest_segment(candidate) else {
        return;
    };
    if is_non_app_manifest_segment(&normalized) {
        return;
    }
    hints.insert(normalized.clone());
    for suffix in ["_app", "_flutter"] {
        if let Some(base) = normalized.strip_suffix(suffix) {
            let trimmed = base.trim_matches('_');
            if trimmed.is_empty() || is_non_app_manifest_segment(trimmed) {
                continue;
            }
            hints.insert(trimmed.to_string());
        }
    }
}

fn derive_manifest_package_hints(package_name: Option<&str>) -> Vec<String> {
    let Some(raw) = package_name else {
        return Vec::new();
    };
    let parts = raw
        .trim()
        .to_ascii_lowercase()
        .split('.')
        .filter(|segment| !segment.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Vec::new();
    }

    let mut hints = HashSet::new();
    if let Some(last) = parts.last() {
        push_manifest_hint(&mut hints, last);
        if last == "app" && parts.len() >= 2 {
            let prev = &parts[parts.len() - 2];
            push_manifest_hint(&mut hints, prev);
        }
    }

    let mut out = hints.into_iter().collect::<Vec<_>>();
    out.sort();
    out
}

fn build_startup_manifest_context(signals: &AndroidManifestSignals) -> StartupManifestContext {
    StartupManifestContext {
        package_name: signals.package_name.clone(),
        application_name: signals.application_name.clone(),
        activities: signals.activities.clone(),
        launcher_activities: signals.launcher_activities.clone(),
        deeplink_activities: signals.deeplink_activities.clone(),
    }
}

fn collect_app_package_counts(model: &ProgramModel) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &model.functions {
        let Some(uri) = model.owner_library_uri(f) else {
            continue;
        };
        if classify_library_uri(uri) != ScopedFunctionKind::App {
            continue;
        }
        let Some(name) = package_name_from_library_uri(uri) else {
            continue;
        };
        *counts.entry(name).or_insert(0) += 1;
    }

    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(|(a_name, a_count), (b_name, b_count)| {
        b_count
            .cmp(a_count)
            .then_with(|| a_name.cmp(b_name))
    });
    items
}

fn format_asm_instruction_line(
    instruction: &flutterdec_disasm_arm64::AsmInstruction,
    emit_opcodes: bool,
) -> String {
    let mut line = if emit_opcodes {
        format!(
            "0x{:x}: {:08x} {}",
            instruction.va, instruction.word, instruction.mnemonic
        )
    } else {
        format!("0x{:x}: {}", instruction.va, instruction.mnemonic)
    };
    if !instruction.op_str.is_empty() {
        line.push(' ');
        line.push_str(&instruction.op_str);
    }
    if !instruction.annotation.is_empty() {
        line.push_str(" ; ");
        line.push_str(&instruction.annotation);
    }
    line
}

fn priority_package_from_library_uri(uri: &str) -> String {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }
    if trimmed.starts_with("dart:") {
        return "dart".to_string();
    }
    if let Some(pkg) = package_name_from_library_uri(trimmed) {
        return pkg;
    }
    "unknown".to_string()
}

fn collect_selected_priority_package_counts(
    selected: &[FunctionPriorityBreakdown],
) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for item in selected {
        let key = priority_package_from_library_uri(item.library_uri.as_deref().unwrap_or(""));
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut out = counts.into_iter().collect::<Vec<_>>();
    out.sort_by(|(a_name, a_count), (b_name, b_count)| {
        b_count.cmp(a_count).then_with(|| a_name.cmp(b_name))
    });
    out
}

#[derive(Debug, Clone, Copy, Default)]
struct SelectedPriorityScopeMix {
    app: usize,
    framework: usize,
    stdlib: usize,
    unknown: usize,
}

fn collect_selected_priority_scope_mix(
    selected: &[FunctionPriorityBreakdown],
) -> SelectedPriorityScopeMix {
    let mut mix = SelectedPriorityScopeMix::default();
    for item in selected {
        match classify_library_uri(item.library_uri.as_deref().unwrap_or("")) {
            ScopedFunctionKind::App => mix.app += 1,
            ScopedFunctionKind::Framework => mix.framework += 1,
            ScopedFunctionKind::Stdlib => mix.stdlib += 1,
            ScopedFunctionKind::Unknown => mix.unknown += 1,
        }
    }
    mix
}

#[derive(Debug, Clone, Copy, Default)]
struct SelectedPreferredPackageStats {
    preferred_app: usize,
    other_app: usize,
}

fn collect_selected_preferred_package_stats(
    selected: &[FunctionPriorityBreakdown],
    preferred_packages: &HashSet<String>,
) -> SelectedPreferredPackageStats {
    let mut out = SelectedPreferredPackageStats::default();
    for item in selected {
        let Some(uri) = item.library_uri.as_deref() else {
            continue;
        };
        if classify_library_uri(uri) != ScopedFunctionKind::App {
            continue;
        }
        let Some(pkg) = package_name_from_library_uri(uri) else {
            continue;
        };
        if preferred_packages.contains(&pkg) {
            out.preferred_app += 1;
        } else {
            out.other_app += 1;
        }
    }
    out
}

fn collect_selected_priority_component_totals(
    selected: &[FunctionPriorityBreakdown],
) -> Vec<(String, usize, i64)> {
    let mut totals: HashMap<String, (usize, i64)> = HashMap::new();
    for item in selected {
        for component in &item.components {
            let slot = totals.entry(component.name.clone()).or_insert((0, 0));
            slot.0 += 1;
            slot.1 += component.score as i64;
        }
    }
    let mut out = totals
        .into_iter()
        .map(|(name, (occurrences, total_score))| (name, occurrences, total_score))
        .collect::<Vec<_>>();
    out.sort_by(|(a_name, a_occ, a_total), (b_name, b_occ, b_total)| {
        b_total
            .abs()
            .cmp(&a_total.abs())
            .then_with(|| b_occ.cmp(a_occ))
            .then_with(|| a_name.cmp(b_name))
    });
    out
}

#[derive(Debug, Default, Clone, Copy)]
struct SelectedBootflowCategoryStats {
    discovered: usize,
    selected: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct SelectedBootflowStats {
    main: SelectedBootflowCategoryStats,
    runapp: SelectedBootflowCategoryStats,
    deeplink: SelectedBootflowCategoryStats,
    activity: SelectedBootflowCategoryStats,
    bootstrap: SelectedBootflowCategoryStats,
    any: SelectedBootflowCategoryStats,
}

#[derive(Debug, Clone)]
struct SelectedBootflowHit {
    category: String,
    hint_kind: String,
    source: String,
    provenance: String,
    selector: String,
    target_va: u64,
    function_name: Option<String>,
    owner_class: Option<String>,
    library_uri: Option<String>,
    total_score: i32,
}

fn collect_selected_bootflow_category_hits(
    category: &str,
    entries: &[BootflowDiscoveryEntry],
    selected_by_entry_va: &HashMap<u64, &FunctionPriorityBreakdown>,
    any_discovered: &mut HashSet<u64>,
    any_selected: &mut HashSet<u64>,
    hits: &mut Vec<SelectedBootflowHit>,
) -> SelectedBootflowCategoryStats {
    let mut category_discovered = HashSet::new();
    let mut category_selected = HashSet::new();
    let mut seen_hit_keys = HashSet::new();
    for entry in entries {
        let Some(target_va) = entry.target_va else {
            continue;
        };
        category_discovered.insert(target_va);
        any_discovered.insert(target_va);
        let Some(selected) = selected_by_entry_va.get(&target_va) else {
            continue;
        };
        category_selected.insert(target_va);
        any_selected.insert(target_va);
        let hit_key = format!(
            "{}|0x{:x}|{}|{}",
            category,
            target_va,
            entry.selector.to_ascii_lowercase(),
            entry.source.to_ascii_lowercase()
        );
        if !seen_hit_keys.insert(hit_key) {
            continue;
        }
        hits.push(SelectedBootflowHit {
            category: category.to_string(),
            hint_kind: entry.kind.clone(),
            source: entry.source.clone(),
            provenance: entry.provenance.clone(),
            selector: entry.selector.clone(),
            target_va,
            function_name: selected.function_name.clone(),
            owner_class: selected.owner_class.clone(),
            library_uri: selected.library_uri.clone(),
            total_score: selected.total_score,
        });
    }
    SelectedBootflowCategoryStats {
        discovered: category_discovered.len(),
        selected: category_selected.len(),
    }
}

fn collect_selected_bootflow_hits(
    selected: &[FunctionPriorityBreakdown],
    bootflow: &BootflowDiscoverySummary,
) -> (SelectedBootflowStats, Vec<SelectedBootflowHit>) {
    let selected_by_entry_va = selected
        .iter()
        .map(|item| (item.entry_va, item))
        .collect::<HashMap<_, _>>();

    let mut any_discovered = HashSet::new();
    let mut any_selected = HashSet::new();
    let mut hits = Vec::new();

    let main = collect_selected_bootflow_category_hits(
        "main",
        &bootflow.main,
        &selected_by_entry_va,
        &mut any_discovered,
        &mut any_selected,
        &mut hits,
    );
    let runapp = collect_selected_bootflow_category_hits(
        "runapp",
        &bootflow.runapp,
        &selected_by_entry_va,
        &mut any_discovered,
        &mut any_selected,
        &mut hits,
    );
    let deeplink = collect_selected_bootflow_category_hits(
        "deeplink",
        &bootflow.deeplink,
        &selected_by_entry_va,
        &mut any_discovered,
        &mut any_selected,
        &mut hits,
    );
    let activity = collect_selected_bootflow_category_hits(
        "activity",
        &bootflow.activity,
        &selected_by_entry_va,
        &mut any_discovered,
        &mut any_selected,
        &mut hits,
    );
    let bootstrap = collect_selected_bootflow_category_hits(
        "bootstrap",
        &bootflow.bootstrap,
        &selected_by_entry_va,
        &mut any_discovered,
        &mut any_selected,
        &mut hits,
    );

    hits.sort_by(|a, b| {
        b.total_score
            .cmp(&a.total_score)
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.target_va.cmp(&b.target_va))
            .then_with(|| a.selector.cmp(&b.selector))
    });

    let stats = SelectedBootflowStats {
        main,
        runapp,
        deeplink,
        activity,
        bootstrap,
        any: SelectedBootflowCategoryStats {
            discovered: any_discovered.len(),
            selected: any_selected.len(),
        },
    };
    (stats, hits)
}

fn selected_bootflow_coverage_ratio(stats: SelectedBootflowCategoryStats) -> f64 {
    if stats.discovered == 0 {
        return 0.0;
    }
    stats.selected as f64 / stats.discovered as f64
}

fn include_function_kind(scope: FunctionScope, kind: ScopedFunctionKind) -> bool {
    match scope {
        FunctionScope::All => true,
        FunctionScope::App => kind == ScopedFunctionKind::App,
        FunctionScope::AppUnknown => {
            kind == ScopedFunctionKind::App || kind == ScopedFunctionKind::Unknown
        }
    }
}

fn include_by_package_filter(
    kind: ScopedFunctionKind,
    package_name: Option<&str>,
    package_filters: &HashSet<String>,
    stats: &mut FunctionScopeStats,
) -> bool {
    if package_filters.is_empty() {
        return true;
    }
    if kind != ScopedFunctionKind::App {
        stats.excluded_by_app_package += 1;
        return false;
    }
    let Some(name) = package_name else {
        stats.excluded_by_app_package += 1;
        return false;
    };
    let included = package_filters.contains(name);
    if !included {
        stats.excluded_by_app_package += 1;
    }
    included
}

fn apply_function_scope_filter(
    model: &ProgramModel,
    scope: FunctionScope,
    app_packages: &[String],
) -> (ProgramModel, FunctionScopeStats) {
    let package_filters = normalize_package_filters(app_packages);

    let mut stats = FunctionScopeStats::from_total(model.functions.len());
    let mut filtered_functions = Vec::new();
    for f in &model.functions {
        let kind = function_kind_from_model(model, f);
        let package_name = model
            .owner_library_uri(f)
            .and_then(package_name_from_library_uri);
        match kind {
            ScopedFunctionKind::App => stats.app += 1,
            ScopedFunctionKind::Framework => stats.framework += 1,
            ScopedFunctionKind::Stdlib => stats.stdlib += 1,
            ScopedFunctionKind::Unknown => stats.unknown += 1,
        }
        if include_function_kind(scope, kind)
            && include_by_package_filter(
                kind,
                package_name.as_deref(),
                &package_filters,
                &mut stats,
            )
        {
            filtered_functions.push(f.clone());
        }
    }
    stats.total_after_filter = filtered_functions.len();
    stats.excluded = stats.total_before_filter.saturating_sub(stats.total_after_filter);

    let mut scoped = model.clone();
    scoped.functions = filtered_functions;
    (scoped, stats)
}

pub fn run_info(
    repo_root: &Path,
    input_path: &Path,
    adapter_backend: AdapterBackend,
) -> Result<InfoOutput> {
    let apk_session = open_apk_session_if_input_is_apk(input_path)?;
    let bundle = load_snapshot_bundle_with_optional_apk_session(input_path, apk_session.as_ref())?;
    // `info` reports rather than fails, but it still may not look an adapter up
    // for a snapshot that could never authorize one: the filesystem probe is
    // downstream of the gate, not a way around it.
    let identity_rejection = bundle.identity.exact_selection_key().err();
    let adapter_installed = identity_rejection.is_none()
        && resolve_adapter_exec(repo_root, &bundle.snapshot_hash).is_ok();
    let manifest_inspection = if let Some(apk) = apk_session.as_ref() {
        inspect_android_manifest_from_apk_session(apk)
    } else {
        inspect_android_manifest(input_path)
    };
    let startup_evidence = if let Some(apk) = apk_session.as_ref() {
        analyze_android_startup_with_manifest_from_apk_session(
            apk,
            &build_startup_manifest_context(&manifest_inspection.signals),
        )
    } else {
        analyze_android_startup_with_manifest(
            input_path,
            &build_startup_manifest_context(&manifest_inspection.signals),
        )
    };

    let mut out = InfoOutput {
        input_path: bundle.input_path.display().to_string(),
        libapp_path: bundle.libapp_path.display().to_string(),
        arch: bundle.arch.clone(),
        snapshot_hash: bundle.snapshot_hash.clone(),
        dart_version: bundle
            .dart_profile
            .as_ref()
            .map(|p| p.dart_version.clone()),
        dart_tag_style: bundle
            .dart_profile
            .as_ref()
            .map(|p| p.profile.tag_style.as_str().to_string()),
        compressed_pointers: bundle.compressed_pointers,
        snapshot_features: bundle.snapshot_features.clone(),
        adapter_installed,
        requested_backend: Some(adapter_backend.as_str().to_string()),
        resolved_backend: None,
        backend_fallback_reason: None,
        producer_id: None,
        producer_trust: None,
        compatibility_record_sha256: None,
        manifest_entry_present: None,
        snapshot_identity_is_exact: Some(bundle.identity.is_exact()),
        identity_rejection: identity_rejection.as_ref().map(ToString::to_string),
        model_capabilities: None,
        compatibility_warnings: None,
        function_count: None,
        class_count: None,
        object_pool_count: None,
        app_package_count_total: None,
        app_package_counts_top: None,
        android_startup_present: Some(startup_evidence.present),
        android_startup_confidence: Some(startup_evidence.confidence.clone()),
        android_startup_entrypoint_count: Some(startup_evidence.dart_entrypoints.len()),
        android_startup_flutter_activity_count: Some(startup_evidence.flutter_activity_classes.len()),
    };

    if adapter_installed {
        if let Ok(loaded) = load_model(repo_root, &bundle, adapter_backend) {
            let manifest_entry_present = loaded.manifest_entry_adapter.is_some();
            let model = loaded.model;
            // The model was validated against the host identity before it got
            // here, so it describes this snapshot by construction. What is worth
            // reporting is whether that identity was header-derived at all.
            let identity_is_exact = bundle.identity.is_exact();
            let resolved_backend = backend_from_id(loaded.resolved_backend);
            let backend_mismatch = match adapter_backend {
                AdapterBackend::Auto => false,
                _ => resolved_backend != adapter_backend,
            };
            let warnings = collect_compatibility_warnings(
                manifest_entry_present,
                identity_is_exact,
                backend_mismatch,
            );
            out.requested_backend = Some(adapter_backend.as_str().to_string());
            out.resolved_backend = Some(resolved_backend.as_str().to_string());
            out.backend_fallback_reason = loaded
                .fallback_reason
                .map(|reason| reason.as_str().to_string());
            out.producer_id = Some(loaded.producer.id.clone());
            out.producer_trust = Some(producer_trust_label(loaded.producer.trust).to_string());
            out.compatibility_record_sha256 =
                Some(loaded.compatibility.record_sha256.to_string());
            out.manifest_entry_present = Some(manifest_entry_present);
            out.snapshot_identity_is_exact = Some(identity_is_exact);
            out.compatibility_warnings = Some(warnings);
            out.model_capabilities = Some(capability_map(&model.capabilities));
            out.function_count = Some(model.functions.len());
            out.class_count = Some(model.classes.len());
            out.object_pool_count = Some(model.object_pool.entries.len());
            let app_package_counts = collect_app_package_counts(&model);
            out.app_package_count_total = Some(app_package_counts.len());
            out.app_package_counts_top = Some(
                app_package_counts
                    .into_iter()
                    .take(20)
                    .map(|(package, functions)| PackageCount { package, functions })
                    .collect::<Vec<_>>(),
            );
        }
    }

    Ok(out)
}

fn collect_compatibility_warnings(
    manifest_entry_present: bool,
    identity_is_exact: bool,
    backend_mismatch: bool,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !manifest_entry_present {
        warnings.push("adapter manifest entry missing for this snapshot hash".to_string());
    }
    if !identity_is_exact {
        warnings.push(
            "snapshot identity is not header-derived, so no exact parser could be authorized"
                .to_string(),
        );
    }
    if backend_mismatch {
        warnings.push("resolved adapter backend differs from requested backend".to_string());
    }
    warnings
}

fn producer_trust_label(trust: ProducerTrust) -> &'static str {
    match trust {
        ProducerTrust::Registered => "registered",
        ProducerTrust::Local => "local",
        ProducerTrust::Untrusted => "untrusted",
    }
}

/// The model's per-domain capability levels, for reports.
fn capability_map(caps: &Capabilities) -> BTreeMap<String, String> {
    Domain::ALL
        .iter()
        .map(|domain| {
            (
                domain.as_str().to_string(),
                caps.level(*domain).as_str().to_string(),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct TargetSelectionStats {
    enabled: bool,
    scope_overridden: bool,
    matched_count: usize,
}

fn function_matches_target(
    func: &flutterdec_adapter::model::Function,
    target: FunctionTarget,
) -> bool {
    let id = u64::from(func.id.0);
    match target {
        FunctionTarget::FunctionId(want) => id == want,
        FunctionTarget::EntryVa(entry_va) => func.code.start_va == entry_va,
        FunctionTarget::Any(value) => id == value || func.code.start_va == value,
    }
}

fn target_label(target: FunctionTarget) -> String {
    match target {
        FunctionTarget::FunctionId(id) => format!("id:{id}"),
        FunctionTarget::EntryVa(entry_va) => format!("va:0x{entry_va:x}"),
        FunctionTarget::Any(value) => format!("{value}"),
    }
}

fn apply_target_function_filter(
    full_model: &ProgramModel,
    scoped_model: &ProgramModel,
    target: FunctionTarget,
) -> Result<(ProgramModel, TargetSelectionStats)> {
    let mut selected_functions = scoped_model
        .functions
        .iter()
        .filter(|func| function_matches_target(func, target))
        .cloned()
        .collect::<Vec<_>>();
    let mut scope_overridden = false;

    if selected_functions.is_empty() {
        selected_functions = full_model
            .functions
            .iter()
            .filter(|func| function_matches_target(func, target))
            .cloned()
            .collect::<Vec<_>>();
        scope_overridden = !selected_functions.is_empty();
    }

    if selected_functions.is_empty() {
        bail!(
            "target {} matched no functions; try --function-scope all or remove --app-package filters",
            target_label(target)
        );
    }
    if matches!(target, FunctionTarget::Any(_)) && selected_functions.len() > 1 {
        let preview = selected_functions
            .iter()
            .take(8)
            .map(|func| {
                format!(
                    "id={} va=0x{:x} {}",
                    func.id,
                    func.code.start_va,
                    func.name_text().unwrap_or("<unnamed>")
                )
            })
            .collect::<Vec<_>>();
        bail!(
            "target {} is ambiguous and matched {} functions: {}. use id:<n> or va:0x<addr>",
            target_label(target),
            selected_functions.len(),
            preview.join(", ")
        );
    }

    let mut selected_model = if scope_overridden {
        full_model.clone()
    } else {
        scoped_model.clone()
    };
    selected_model.functions = selected_functions.clone();
    Ok((
        selected_model,
        TargetSelectionStats {
            enabled: true,
            scope_overridden,
            matched_count: selected_functions.len(),
        },
    ))
}

pub fn run_decompile(
    repo_root: &Path,
    input_path: &Path,
    opt: &DecompileOptions,
) -> Result<QualityReport> {
    let apk_session = open_apk_session_if_input_is_apk(input_path)?;
    let bundle = load_snapshot_bundle_with_optional_apk_session(input_path, apk_session.as_ref())?;
    let loaded_model = load_model(repo_root, &bundle, opt.adapter_backend)?;
    let adapter_exec_path = loaded_model.adapter_exec.display().to_string();
    let manifest_entry_version = loaded_model.manifest_entry_version.clone();
    let manifest_entry_adapter = loaded_model.manifest_entry_adapter.clone();
    let requested_backend = opt.adapter_backend;
    // Four distinct typed facts, none of them read out of a name: what the host
    // asked for, what answered, why it differed, and who produced the model.
    let resolved_backend = backend_from_id(loaded_model.resolved_backend);
    let backend_fallback_reason = loaded_model.fallback_reason;
    let producer = loaded_model.producer.clone();
    let compatibility = loaded_model.compatibility.clone();
    let backend_mismatch = match requested_backend {
        AdapterBackend::Auto => false,
        _ => resolved_backend != requested_backend,
    };
    let snapshot_identity_is_exact = bundle.identity.is_exact();
    if opt.require_snapshot_hash_match && !snapshot_identity_is_exact {
        bail!(
            "--require-snapshot-hash-match: decompile input identity is not header-derived: {}",
            bundle
                .identity
                .exact_selection_key()
                .err()
                .map(|rejection| rejection.to_string())
                .unwrap_or_default()
        );
    }
    let engine_context =
        try_collect_engine_fingerprint_with_apk_session(input_path, apk_session.as_ref(), &bundle.arch);
    let mut engine_symbol_ingestion =
        resolve_local_engine_symbol_targets(repo_root, input_path, &bundle.arch, &engine_context);
    let manifest_inspection = if let Some(apk) = apk_session.as_ref() {
        inspect_android_manifest_from_apk_session(apk)
    } else {
        inspect_android_manifest(input_path)
    };
    let startup_evidence = if opt.engine_options.apk_startup_analysis {
        if let Some(apk) = apk_session.as_ref() {
            analyze_android_startup_with_manifest_from_apk_session(
                apk,
                &build_startup_manifest_context(&manifest_inspection.signals),
            )
        } else {
            analyze_android_startup_with_manifest(
                input_path,
                &build_startup_manifest_context(&manifest_inspection.signals),
            )
        }
    } else {
        AndroidStartupEvidence::default()
    };
    // Enrichment produces hints. The model is not rewritten, so the
    // authoritative library/class/function/pool records and the pool index space
    // are exactly what the adapter authored, before and after this point.
    let model = loaded_model.model;
    let mut hints = ProgramHints::new();
    let model_name_hints = collect_model_name_hints(&model, &mut hints);
    let manifest_hint_count = if manifest_inspection.present {
        collect_manifest_bootflow_hints(&model, &manifest_inspection.signals, &mut hints)
    } else {
        0
    };
    let startup_hint_count = if opt.engine_options.apk_startup_analysis {
        collect_apk_startup_bootflow_hints(&model, &startup_evidence, &mut hints)
    } else {
        0
    };
    let app_package_counts = collect_app_package_counts(&model);
    let app_package_counts_top = app_package_counts
        .iter()
        .take(20)
        .map(|(package, functions)| json!({ "package": package, "functions": functions }))
        .collect::<Vec<_>>();
    let normalized_app_packages = normalize_package_filters(&opt.app_packages);
    let mut normalized_app_package_list = normalized_app_packages.iter().cloned().collect::<Vec<_>>();
    normalized_app_package_list.sort();
    let mut priority_package_hints = normalized_app_package_list.clone();
    if priority_package_hints.is_empty() {
        priority_package_hints =
            derive_manifest_package_hints(manifest_inspection.signals.package_name.as_deref());
    }
    let (scoped_model, function_scope_stats) =
        apply_function_scope_filter(&model, opt.function_scope, &opt.app_packages);
    let (selected_model, target_selection_stats) = if let Some(target) = opt.function_target {
        apply_target_function_filter(&model, &scoped_model, target)?
    } else {
        (scoped_model.clone(), TargetSelectionStats::default())
    };

    // Target architecture is a header fact the loader owns, and the model was
    // already checked against it. v3 read `arch` off the adapter's own output,
    // which meant an adapter could declare itself arm64.
    if !matches!(
        bundle.identity.target_arch,
        flutterdec_loader::identity::TargetArch::Arm64
    ) {
        bail!(
            "target architecture {} unsupported in v1",
            bundle.identity.target_arch
        );
    }
    if selected_model.functions.is_empty() {
        let app_package_note = if normalized_app_packages.is_empty() {
            String::new()
        } else {
            let available = app_package_counts
                .iter()
                .take(8)
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if available.is_empty() {
                " and/or selected --app-package filters".to_string()
            } else {
                format!(
                    " and/or selected --app-package filters (available: {})",
                    available.join(", ")
                )
            }
        };
        bail!(
            "no functions matched --function-scope {}{}. try --function-scope all",
            opt.function_scope.as_str(),
            app_package_note
        );
    }

    let (disasm, selected_priorities) = disassemble_program_with_priorities_and_package_hints(
        &selected_model,
        &hints,
        &bundle.isolate_instr,
        bundle.isolate_instr_va,
        if target_selection_stats.enabled {
            None
        } else {
            opt.focus.as_deref()
        },
        if target_selection_stats.enabled {
            None
        } else {
            opt.max_functions
        },
        &priority_package_hints,
        opt.engine_options.bootflow_category_seeds,
    );
    // Records that span several real functions are split before the IR is built, so
    // each piece gets dense block ids and an entry at block 0, which is what
    // `Regions::build` requires. Opt-in, because it multiplies the function count.
    let pre_split_disassembled = disasm.len();
    let (disasm, split_stats) = if opt.split_records {
        split_inflated_records(disasm)
    } else {
        (disasm, SplitStats::default())
    };
    let mut ir: Vec<FunctionIr> = build_program_ir(&disasm);
    let mut symbol_names: HashMap<u64, String> = HashMap::new();
    let mut symbol_quality: HashMap<u64, SymbolNameQuality> = HashMap::new();
    let mut symbol_merge_stats = SymbolMergeStats::default();
    let mut standard_model_symbol_count = 0usize;
    // `pool[N]` in the disassembly is a real ObjectPool entry index only when the
    // model claims a hardware index space. An ordinal pool's indexes are
    // positions in the producer's own list, so joining the two would attach
    // arbitrary values to unrelated slots. The pool-reading helpers enforce this
    // themselves; the flag is kept for the report.
    let pool_metadata = collect_pool_metadata_stats(&model);
    let pool_index_space_authoritative = pool_metadata.addressable;
    let pool_value_hints =
        if opt.engine_options.pool_value_hints || opt.engine_options.pool_semantic_hints {
            build_pool_value_hints(&model)
        } else {
            HashMap::new()
        };
    let function_name_provenance_stats =
        collect_function_name_provenance_stats(&model.functions);
    let pool_semantic_hints = if opt.engine_options.pool_semantic_hints {
        build_pool_semantic_hints(&model, &hints)
    } else {
        HashMap::new()
    };
    let pool_target_symbols = if opt.engine_options.pool_semantic_hints
        && opt.engine_options.canonical_model_symbols
    {
        build_pool_target_symbols(&pool_semantic_hints, &pool_value_hints)
    } else {
        HashMap::new()
    };

    for f in &selected_model.functions {
        // A function with no recovered name contributes no symbol. It gets an
        // address-derived label at emit time instead, so nothing downstream can
        // mistake `fn_0x1000` for something the snapshot said.
        let Some(model_name) = f.name_text() else {
            symbol_quality.insert(f.code.start_va, SymbolNameQuality::Placeholder);
            continue;
        };
        let model_quality = f
            .name
            .as_ref()
            .map(|name| symbol_name_quality_from_provenance(name.provenance))
            .unwrap_or(SymbolNameQuality::Placeholder);
        let (resolved, resolved_quality) = if opt.engine_options.canonical_model_symbols {
            match canonical_standard_model_name(&selected_model, f) {
                Some(canonical) if canonical != model_name => {
                    standard_model_symbol_count += 1;
                    (canonical, SymbolNameQuality::Heuristic)
                }
                _ => (model_name.to_string(), model_quality),
            }
        } else {
            (model_name.to_string(), model_quality)
        };
        symbol_names.insert(f.code.start_va, resolved);
        symbol_quality.insert(f.code.start_va, resolved_quality);
    }
    for f in &disasm {
        let Some(name) = f.function_name.clone() else {
            symbol_quality
                .entry(f.entry_va)
                .or_insert(SymbolNameQuality::Placeholder);
            continue;
        };
        symbol_names.entry(f.entry_va).or_insert(name);
        symbol_quality
            .entry(f.entry_va)
            .or_insert(SymbolNameQuality::Heuristic);
    }
    for (va, name) in &pool_target_symbols {
        merge_symbol_name(
            &mut symbol_names,
            &mut symbol_quality,
            *va,
            name.clone(),
            Some(SymbolNameQuality::Heuristic),
            &mut symbol_merge_stats,
        );
    }
    for elf_path in &opt.extra_symbol_elfs {
        let ext = load_elf_function_symbols(elf_path)
            .with_context(|| format!("load external symbols from {}", elf_path.display()))?;
        for (va, name) in ext {
            merge_symbol_name(
                &mut symbol_names,
                &mut symbol_quality,
                va,
                name,
                Some(SymbolNameQuality::External),
                &mut symbol_merge_stats,
            );
        }
    }
    for map_path in &opt.extra_symbol_map_targets {
        let ext = load_symbol_target_symbols(map_path, opt.include_nearest_symbol_map)
            .with_context(|| {
                format!(
                    "load symbol target map from {}",
                    map_path.display()
                )
            })?;
        for (va, name) in ext {
            merge_symbol_name(
                &mut symbol_names,
                &mut symbol_quality,
                va,
                name,
                Some(SymbolNameQuality::External),
                &mut symbol_merge_stats,
            );
        }
    }
    for map_path in &engine_symbol_ingestion.loaded_paths.clone() {
        if opt.extra_symbol_map_targets.iter().any(|explicit| explicit == map_path) {
            continue;
        }
        let ext = load_symbol_target_symbols(map_path, opt.include_nearest_symbol_map)
            .with_context(|| {
                format!(
                    "load auto-ingested symbol target map from {}",
                    map_path.display()
                )
            })?;
        engine_symbol_ingestion.applied_target_count += ext.len();
        for (va, name) in ext {
            merge_symbol_name(
                &mut symbol_names,
                &mut symbol_quality,
                va,
                name,
                Some(SymbolNameQuality::External),
                &mut symbol_merge_stats,
            );
        }
    }
    // Shared stubs name themselves: each loads its own `Code` object from a
    // fixed `Thread` slot in its prologue, so this is read from the callee
    // rather than inferred from how often it is called. `Exact` for that
    // reason. Gated on a known (version, pointer mode) and cross-checked
    // against the binary's own offset set, so an unknown SDK names nothing
    // instead of naming everything wrong.
    let stub_naming = shared_stub_names(
        &disasm,
        bundle.dart_profile.as_ref().map(|p| p.dart_version.as_str()),
        bundle.compressed_pointers,
    );
    let shared_stub_naming = SharedStubNamingSummary {
        status: stub_naming.status.to_string(),
        named: stub_naming.names.len(),
        allocation_named: stub_naming.allocation_named,
        scanned: stub_naming.scanned,
    };
    // A call that raises has no fall-through, so the edge the disassembler
    // recorded after it is not real. Cut before the emitters see the CFG.
    let noreturn_prune =
        prune_calls_that_never_return(&mut ir, &stub_naming.non_returning);
    for (va, name) in stub_naming.names {
        merge_symbol_name(
            &mut symbol_names,
            &mut symbol_quality,
            va,
            name,
            Some(SymbolNameQuality::Exact),
            &mut symbol_merge_stats,
        );
    }
    let symbol_quality_counts = collect_symbol_quality_counts(&symbol_quality);
    let pseudo = emit_program_with_runtime_stubs(
        &ir,
        &symbol_names,
        &pool_value_hints,
        &pool_semantic_hints,
        &stub_naming.effects,
    );

    let asm_dir = opt.out_dir.join("asm");
    let ir_dir = opt.out_dir.join("ir");
    let pseudo_dir = opt.out_dir.join("pseudocode");
    fs::create_dir_all(&pseudo_dir).context("create pseudocode out dir")?;
    if opt.emit_asm {
        fs::create_dir_all(&asm_dir)?;
    }
    if opt.emit_ir {
        fs::create_dir_all(&ir_dir)?;
    }

    for p in &pseudo {
        let filename = format!(
            "{:05}_{}.dartpseudo",
            p.function_id,
            normalize_file_name(&p.function_name)
        );
        fs::write(pseudo_dir.join(filename), terminated(&p.source))?;
    }

    if opt.emit_asm {
        for f in &disasm {
            let mut lines = Vec::new();
            for i in &f.instructions {
                lines.push(format_asm_instruction_line(i, opt.emit_asm_opcodes));
            }
            let filename = format!(
                "{:05}_{}.s",
                f.function_id,
                normalize_file_name(&f.display_name())
            );
            fs::write(asm_dir.join(filename), terminated(&lines.join("\n")))?;
        }
    }

    if opt.emit_ir {
        for f in &ir {
            let filename = format!("{:05}_{}.json", f.function_id, normalize_file_name(&f.name));
            let mut body = serde_json::to_vec_pretty(f)?;
            body.push(b'\n');
            fs::write(ir_dir.join(filename), body)?;
        }
    }

    let ghidra_script_path = if opt.emit_ghidra_script {
        Some(opt.out_dir.join("ghidra_apply_symbols.py"))
    } else {
        None
    };
    let ida_script_path = if opt.emit_ida_script {
        Some(opt.out_dir.join("ida_apply_symbols.py"))
    } else {
        None
    };
    let script_pool_comments = if opt.emit_ghidra_script || opt.emit_ida_script {
        collect_ghidra_pool_comments(&disasm, &pool_value_hints)
    } else {
        Vec::new()
    };
    let ghidra_script_stats = if let Some(path) = ghidra_script_path.as_ref() {
        Some(write_ghidra_symbol_script(
            path,
            &symbol_names,
            &script_pool_comments,
        )?)
    } else {
        None
    };
    let ida_script_stats = if let Some(path) = ida_script_path.as_ref() {
        Some(write_ida_symbol_script(
            path,
            &symbol_names,
            &script_pool_comments,
        )?)
    } else {
        None
    };

    let report =
        quality_from_artifacts(&selected_model, &pseudo, opt, pre_split_disassembled);
    let (semantic_intent, call_fallback, selector_fallback, selector_fallback_top) =
        if opt.engine_options.semantic_reporting {
            let semantic_intent = collect_semantic_intent_summary(&pseudo);
            let call_fallback = collect_call_fallback_summary(&pseudo);
            let selector_fallback = collect_selector_fallback_summary(&pseudo);
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
            (
                semantic_intent,
                call_fallback,
                selector_fallback,
                selector_fallback_top,
            )
        } else {
            (
                SemanticIntentSummary::default(),
                CallFallbackSummary::default(),
                SelectorFallbackSummary::default(),
                Vec::new(),
            )
        };
    let semantic_total =
        report.semantic_direct_calls + report.semantic_indirect_calls + report.dispatch_selector_calls;
    let semantic_ratio = if report.total_calls == 0 {
        0.0
    } else {
        semantic_total as f64 / report.total_calls as f64
    };
    let indirect_semantic_ratio = if report.indirect_calls == 0 {
        0.0
    } else {
        (report.semantic_indirect_calls + report.dispatch_selector_calls) as f64
            / report.indirect_calls as f64
    };
    let prioritization_selected = selected_priorities
        .iter()
        .take(64)
        .cloned()
        .collect::<Vec<FunctionPriorityBreakdown>>();
    let prioritization_package_counts = collect_selected_priority_package_counts(&prioritization_selected);
    let prioritization_package_counts_top = prioritization_package_counts
        .iter()
        .take(20)
        .map(|(package, functions)| json!({ "package": package, "functions": functions }))
        .collect::<Vec<_>>();
    let prioritization_scope_mix = collect_selected_priority_scope_mix(&prioritization_selected);
    let prioritization_app_like_ratio = if prioritization_selected.is_empty() {
        0.0
    } else {
        prioritization_scope_mix.app as f64 / prioritization_selected.len() as f64
    };
    let mut preferred_priority_packages = normalize_package_filters(&priority_package_hints);
    preferred_priority_packages.insert("app".to_string());
    let preferred_package_stats =
        collect_selected_preferred_package_stats(&prioritization_selected, &preferred_priority_packages);
    let preferred_package_ratio = if preferred_package_stats.preferred_app
        + preferred_package_stats.other_app
        == 0
    {
        0.0
    } else {
        preferred_package_stats.preferred_app as f64
            / (preferred_package_stats.preferred_app + preferred_package_stats.other_app) as f64
    };
    let prioritization_unknown_count = prioritization_package_counts
        .iter()
        .find(|(name, _)| name == "unknown")
        .map(|(_, count)| *count)
        .unwrap_or(0usize);
    let prioritization_component_totals = collect_selected_priority_component_totals(&prioritization_selected);
    let prioritization_component_totals_top = prioritization_component_totals
        .iter()
        .take(30)
        .map(|(component, occurrences, score_total)| {
            let avg = if *occurrences == 0 {
                0.0
            } else {
                *score_total as f64 / *occurrences as f64
            };
            json!({
                "component": component,
                "occurrences": occurrences,
                "score_total": score_total,
                "score_avg": avg
            })
        })
        .collect::<Vec<_>>();
    let bootflow_discovery = collect_bootflow_discovery(&hints);
    let (selected_bootflow_stats, selected_bootflow_hits) =
        collect_selected_bootflow_hits(&prioritization_selected, &bootflow_discovery);
    let selected_bootflow_hits_top = selected_bootflow_hits
        .iter()
        .take(20)
        .map(|hit| {
            json!({
                "category": hit.category,
                "hint_kind": hit.hint_kind,
                "provenance": hit.provenance,
                "source": hit.source,
                "selector": hit.selector,
                "target_va": hit.target_va,
                "function_name": hit.function_name,
                "owner_class": hit.owner_class,
                "library_uri": hit.library_uri,
                "total_score": hit.total_score
            })
        })
        .collect::<Vec<_>>();
    let selected_bootflow_coverage = json!({
        "main": {
            "selected": selected_bootflow_stats.main.selected,
            "discovered": selected_bootflow_stats.main.discovered,
            "coverage": selected_bootflow_coverage_ratio(selected_bootflow_stats.main)
        },
        "runapp": {
            "selected": selected_bootflow_stats.runapp.selected,
            "discovered": selected_bootflow_stats.runapp.discovered,
            "coverage": selected_bootflow_coverage_ratio(selected_bootflow_stats.runapp)
        },
        "deeplink": {
            "selected": selected_bootflow_stats.deeplink.selected,
            "discovered": selected_bootflow_stats.deeplink.discovered,
            "coverage": selected_bootflow_coverage_ratio(selected_bootflow_stats.deeplink)
        },
        "activity": {
            "selected": selected_bootflow_stats.activity.selected,
            "discovered": selected_bootflow_stats.activity.discovered,
            "coverage": selected_bootflow_coverage_ratio(selected_bootflow_stats.activity)
        },
        "bootstrap": {
            "selected": selected_bootflow_stats.bootstrap.selected,
            "discovered": selected_bootflow_stats.bootstrap.discovered,
            "coverage": selected_bootflow_coverage_ratio(selected_bootflow_stats.bootstrap)
        },
        "any": {
            "selected": selected_bootflow_stats.any.selected,
            "discovered": selected_bootflow_stats.any.discovered,
            "coverage": selected_bootflow_coverage_ratio(selected_bootflow_stats.any)
        }
    });
    let bootflow_main = bootflow_discovery
        .main
        .iter()
        .map(|entry| {
            json!({
                "kind": entry.kind,
                "source": entry.source,
                "provenance": entry.provenance,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "detail": entry.detail
            })
        })
        .collect::<Vec<_>>();
    let bootflow_runapp = bootflow_discovery
        .runapp
        .iter()
        .map(|entry| {
            json!({
                "kind": entry.kind,
                "provenance": entry.provenance,
                "source": entry.source,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "detail": entry.detail
            })
        })
        .collect::<Vec<_>>();
    let bootflow_deeplink = bootflow_discovery
        .deeplink
        .iter()
        .map(|entry| {
            json!({
                "kind": entry.kind,
                "provenance": entry.provenance,
                "source": entry.source,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "detail": entry.detail
            })
        })
        .collect::<Vec<_>>();
    let bootflow_activity = bootflow_discovery
        .activity
        .iter()
        .map(|entry| {
            json!({
                "kind": entry.kind,
                "provenance": entry.provenance,
                "source": entry.source,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "detail": entry.detail
            })
        })
        .collect::<Vec<_>>();
    let bootflow_bootstrap = bootflow_discovery
        .bootstrap
        .iter()
        .map(|entry| {
            json!({
                "kind": entry.kind,
                "provenance": entry.provenance,
                "source": entry.source,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "detail": entry.detail
            })
        })
        .collect::<Vec<_>>();
    fs::create_dir_all(&opt.out_dir)?;

    let quality_path = opt.out_dir.join("quality.json");
    fs::write(&quality_path, serde_json::to_vec_pretty(&report)?)?;
    let bundle_snapshot_hash = bundle.snapshot_hash.clone();
    let manifest_entry_present = manifest_entry_adapter.is_some();
    let compatibility_warnings = collect_compatibility_warnings(
        manifest_entry_present,
        snapshot_identity_is_exact,
        backend_mismatch,
    );
    let compatibility_status = if compatibility_warnings.is_empty() {
        "ok"
    } else {
        "warning"
    };

    let summary = json!({
        "input": bundle.input_path,
        "libapp": bundle.libapp_path,
        "arch": bundle.arch,
        "snapshot_hash": bundle_snapshot_hash.clone(),
        "dart_profile": bundle.dart_profile.as_ref().map(|p| json!({
            "dart_version": p.dart_version,
            "profile_version": p.profile_version,
            "tag_style": p.profile.tag_style.as_str(),
            "compressed_word_size": p.profile.compressed_word_size,
            "header_fields": p.profile.header_fields,
            "max_alignment": p.profile.max_alignment
        })),
        "analysis": {
            "profile": opt.analysis_profile.as_str(),
            "engine": &opt.engine_options
        },
        // Four separate typed facts. `resolved_backend` comes from the protocol
        // result, never from a filename or a substring of adapter output.
        "adapter_selection": {
            "requested_backend": requested_backend.as_str(),
            "resolved_backend": backend_label(Some(resolved_backend)),
            "fallback_reason": backend_fallback_reason.map(|reason| reason.as_str()),
            "backend_mismatch": backend_mismatch,
            "require_snapshot_hash_match": opt.require_snapshot_hash_match,
            "adapter_exec_path": adapter_exec_path,
            "manifest_entry_adapter": manifest_entry_adapter,
            "manifest_entry_version": manifest_entry_version,
            "snapshot_identity": {
                "hash": bundle_snapshot_hash,
                "header_derived": snapshot_identity_is_exact
            }
        },
        "producer": {
            "id": producer.id,
            "version": producer.version,
            "artifact_sha256": producer.artifact_sha256.to_string(),
            "trust": producer_trust_label(producer.trust)
        },
        "compatibility": {
            "record_sha256": compatibility.record_sha256.to_string(),
            "parser_family_id": compatibility.parser_family_id,
            "profile_id": compatibility.profile_id,
            "profile_sha256": compatibility.profile_sha256.to_string()
        },
        "model": {
            "model_version": model.model_version,
            "name_pattern_hints": model_name_hints,
            "capabilities": capability_map(&model.capabilities),
            "diagnostics": model.diagnostics.len(),
            "function_name_provenance": {
                "named": function_name_provenance_stats.named(),
                "exact": function_name_provenance_stats.exact,
                "derived": function_name_provenance_stats.derived,
                "heuristic": function_name_provenance_stats.heuristic,
                "unnamed": function_name_provenance_stats.unnamed
            },
            "pool_index_space_addressable": pool_metadata.addressable,
            "pool_heuristic_entries": pool_metadata.heuristic
        },
        "engine_fingerprint_context": {
            "detected": engine_context.detected,
            "source": engine_context.source,
            "machine": engine_context.machine,
            "machine_id": engine_context.machine_id,
            "machine_matches_bundle_arch": engine_context.machine_matches_bundle_arch,
            "build_id": engine_context.build_id,
            "candidate_flutter_version": engine_context.candidate_flutter_version,
            "candidate_dart_version": engine_context.candidate_dart_version,
            "confidence": engine_context.confidence,
            "symbol_count": engine_context.symbol_count,
            "dyn_symbol_count": engine_context.dyn_symbol_count,
            "exec_section_count": engine_context.exec_section_count,
            "error": engine_context.error
        },
        "compatibility": {
            "status": compatibility_status,
            "model": {
                "version": model.model_version,
                "supported_versions": [flutterdec_adapter::model::MODEL_VERSION],
                "supported": model.model_version == flutterdec_adapter::model::MODEL_VERSION
            },
            "snapshot_identity_is_exact": snapshot_identity_is_exact,
            "snapshot_hash_match_required": opt.require_snapshot_hash_match,
            "manifest_entry_present": manifest_entry_present,
            "warnings": compatibility_warnings
        },
        // The Dart version is a host fact resolved from the snapshot hash, not
        // something the adapter reports: a semantic version is an alias of the
        // hash, never a selector.
        "dart_version": bundle
            .dart_profile
            .as_ref()
            .map(|p| p.dart_version.clone()),
        "function_scope": {
            "selected": opt.function_scope.as_str(),
            "total_before_filter": function_scope_stats.total_before_filter,
            "total_after_filter": function_scope_stats.total_after_filter,
            "excluded": function_scope_stats.excluded,
            "excluded_by_app_package": function_scope_stats.excluded_by_app_package,
            "app_packages": normalized_app_package_list,
            "priority_package_hints": priority_package_hints,
            "app_package_count_total": app_package_counts.len(),
            "app_package_counts_top": app_package_counts_top,
            "categories": {
                "app": function_scope_stats.app,
                "framework": function_scope_stats.framework,
                "stdlib": function_scope_stats.stdlib,
                "unknown": function_scope_stats.unknown
            }
        },
        "android_manifest": {
            "present": manifest_inspection.present,
            "parse_mode": manifest_inspection.parse_mode,
            "parse_error": manifest_inspection.parse_error,
            "confidence": {
                "package_name": manifest_inspection.confidence.package_name,
                "launcher": manifest_inspection.confidence.launcher,
                "deeplink": manifest_inspection.confidence.deeplink,
                "activities": manifest_inspection.confidence.activities
            },
            "package_name": manifest_inspection.signals.package_name,
            "application_name": manifest_inspection.signals.application_name,
            "main_launcher": manifest_inspection.signals.has_main_launcher,
            "view_browsable": manifest_inspection.signals.has_view_browsable,
            "activity_count": manifest_inspection.signals.activities.len(),
            "activities": manifest_inspection.signals.activities,
            "launcher_activity_count": manifest_inspection.signals.launcher_activities.len(),
            "launcher_activities": manifest_inspection.signals.launcher_activities,
            "deeplink_activity_count": manifest_inspection.signals.deeplink_activities.len(),
            "deeplink_activities": manifest_inspection.signals.deeplink_activities,
            "deeplink_entry_count": manifest_inspection.signals.deeplink_entries.len(),
            "deeplink_entries": manifest_inspection.signals.deeplink_entries,
            "bootflow_hints": manifest_hint_count
        },
        "android_startup": {
            "enabled": opt.engine_options.apk_startup_analysis,
            "present": startup_evidence.present,
            "confidence": startup_evidence.confidence,
            "dex_file_count": startup_evidence.dex_files.len(),
            "dex_files": startup_evidence.dex_files,
            "parse_error_count": startup_evidence.parse_errors.len(),
            "parse_errors": startup_evidence.parse_errors,
            "flutter_activity_count": startup_evidence.flutter_activity_classes.len(),
            "flutter_activity_classes": startup_evidence.flutter_activity_classes,
            "startup_method_count": startup_evidence.startup_methods.len(),
            "startup_methods": startup_evidence.startup_methods,
            "dart_entrypoint_count": startup_evidence.dart_entrypoints.len(),
            "dart_entrypoints": startup_evidence.dart_entrypoints,
            "jni_bootstrap_count": startup_evidence.jni_bootstrap.len(),
            "jni_bootstrap": startup_evidence.jni_bootstrap,
            "bootstrap_chain": {
                "complete": startup_evidence.bootstrap_chain.complete,
                "missing_steps": startup_evidence.bootstrap_chain.missing_steps,
                "source_count": startup_evidence.bootstrap_chain.sources.len(),
                "sources": startup_evidence.bootstrap_chain.sources,
                "path_count": startup_evidence.bootstrap_chain.paths.len(),
                "paths": startup_evidence.bootstrap_chain.paths
            },
            "bootflow_hints": startup_hint_count
        },
        "counts": {
            "libraries": model.libraries.len(),
            "classes": model.classes.len(),
            "functions": selected_model.functions.len(),
            "functions_total": model.functions.len(),
            "object_pool": model.object_pool.entries.len(),
            "disassembled_functions": disasm.len()
        },
        "quality": report,
        "extra_symbol_elfs": opt
            .extra_symbol_elfs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        "extra_symbol_map_targets": opt
            .extra_symbol_map_targets
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        "include_nearest_symbol_map": opt.include_nearest_symbol_map,
        "engine_symbol_ingestion": {
            "enabled": engine_symbol_ingestion.enabled,
            "match_kind": engine_symbol_ingestion.match_kind,
            "manifest_path": engine_symbol_ingestion.manifest_path,
            "loaded_paths": engine_symbol_ingestion
                .loaded_paths
                .iter()
                .filter(|path| !opt.extra_symbol_map_targets.iter().any(|explicit| explicit == *path))
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "applied_target_count": engine_symbol_ingestion.applied_target_count,
            "error": engine_symbol_ingestion.error
        },
        "ghidra_script": {
            "enabled": opt.emit_ghidra_script,
            "path": ghidra_script_path
                .as_ref()
                .map(|p| p.display().to_string()),
            "symbol_count": ghidra_script_stats
                .as_ref()
                .map(|stats| stats.symbol_count)
                .unwrap_or(0),
            "pool_comment_count": ghidra_script_stats
                .as_ref()
                .map(|stats| stats.comment_count)
                .unwrap_or(0)
        },
        "ida_script": {
            "enabled": opt.emit_ida_script,
            "path": ida_script_path
                .as_ref()
                .map(|p| p.display().to_string()),
            "symbol_count": ida_script_stats
                .as_ref()
                .map(|stats| stats.symbol_count)
                .unwrap_or(0),
            "pool_comment_count": ida_script_stats
                .as_ref()
                .map(|stats| stats.comment_count)
                .unwrap_or(0)
        },
        "name_resolution": {
            "final_quality": {
                "placeholder": symbol_quality_counts.placeholder,
                "heuristic": symbol_quality_counts.heuristic,
                "external": symbol_quality_counts.external,
                "exact": symbol_quality_counts.exact
            },
            "merge": {
                "inserted": symbol_merge_stats.inserted,
                "replaced": symbol_merge_stats.replaced,
                "skipped": symbol_merge_stats.skipped,
                "replaced_to_placeholder": symbol_merge_stats.replaced_to_placeholder,
                "replaced_to_heuristic": symbol_merge_stats.replaced_to_heuristic,
                "replaced_to_external": symbol_merge_stats.replaced_to_external,
                "replaced_to_exact": symbol_merge_stats.replaced_to_exact
            }
        },
        "symbol_merge": {
            "inserted": symbol_merge_stats.inserted,
            "replaced_generic": symbol_merge_stats.replaced,
            "skipped": symbol_merge_stats.skipped
        },
        "standard_model_symbols": standard_model_symbol_count
        ,
        "pool_value_hints": pool_value_hints.len(),
        "pool_semantic_hints": pool_semantic_hints.len(),
        "pool_target_symbols": pool_target_symbols.len(),
        "pool_metadata": {
            "total_entries": pool_metadata.total_entries,
            "with_target_va": pool_metadata.with_target_va,
            "with_selector": pool_metadata.with_selector,
            "with_value": pool_metadata.with_value,
            "heuristic_entries": pool_metadata.heuristic,
            "index_space_authoritative": pool_index_space_authoritative,
            "geometry": model.object_pool.geometry.map(|g| serde_json::json!({
                "entries_offset": g.entries_offset,
                "word_size": g.word_size
            })),
            "hints_suppressed_reason": if pool_index_space_authoritative {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(
                    "the model declares an ordinal pool index space, so pool indexes carry \
                     no address meaning and pool value/semantic hints were not applied"
                        .to_string(),
                )
            }
        },
        "semantic_rewrite": {
            "total": semantic_total,
            "ratio": semantic_ratio,
            "direct": report.semantic_direct_calls,
            "indirect": report.semantic_indirect_calls,
            "dispatch_fallback": report.dispatch_selector_calls,
            "target_va_symbol": report.target_va_symbol_calls,
            "indirect_ratio": indirect_semantic_ratio
        },
        "semantic_intent": {
            "framework": semantic_intent.framework,
            "stdlib": semantic_intent.stdlib,
            "runtime": semantic_intent.runtime,
            "native": semantic_intent.native,
            "selector_tagged": semantic_intent.selector_tagged,
            "constructor_calls": semantic_intent.constructor_calls
        },
        // Reported separately, and never folded into the disassembly ratio: the
        // ratio's denominator is the model's function list, so counting split
        // pieces in its numerator would compare unlike things.
        "record_split": {
            "enabled": opt.split_records,
            "records_declared": pre_split_disassembled,
            "records_split": split_stats.records_split,
            "functions_recovered": split_stats.functions_recovered,
            "rejected_branch_target": split_stats.rejected_branch_target,
            "rejected_not_contained": split_stats.rejected_not_contained,
            "rejected_no_block": split_stats.rejected_no_block
        },
        "selector_fallback": {
            "total": selector_fallback.total,
            "unique": selector_fallback.unique,
            "top": selector_fallback_top
        },
        // Carries the keys the gate actually used, not just the outcome: a
        // `named` status beside an unknown version would leave no way to tell
        // which SDK the names came from, and a zero would be indistinguishable
        // from a feature that never ran.
        "shared_stub_naming": {
            "status": shared_stub_naming.status,
            "named": shared_stub_naming.named,
            "allocation_named": shared_stub_naming.allocation_named,
            "functions_scanned": shared_stub_naming.scanned,
            "noreturn_pruned_functions": noreturn_prune.functions,
            "noreturn_pruned_blocks": noreturn_prune.blocks_cut,
            "noreturn_pruned_instructions": noreturn_prune.instructions_cut,
            "resolved_backend": resolved_backend.as_str(),
            "snapshot_dart_version": bundle.dart_profile.as_ref().map(|p| p.dart_version.clone()),
            "compressed_pointers": bundle.compressed_pointers
        },
        "call_fallback": {
            "dynamic_call": call_fallback.dynamic_call,
            "dispatch_invoke": call_fallback.dispatch_invoke,
            "dispatch_target_invoke": call_fallback.dispatch_target_invoke,
            "generic_invoke": call_fallback.generic_invoke
        },
        "prioritization": {
            "enabled": opt.max_functions.is_some() && !target_selection_stats.enabled,
            "focus": if target_selection_stats.enabled { None } else { opt.focus.clone() },
            "selected_count": selected_priorities.len(),
            "selected_package_count_total": prioritization_package_counts.len(),
            "selected_unknown_library_count": prioritization_unknown_count,
            "selected_package_counts_top": prioritization_package_counts_top,
            "selected_scope_mix": {
                "app": prioritization_scope_mix.app,
                "framework": prioritization_scope_mix.framework,
                "stdlib": prioritization_scope_mix.stdlib,
                "unknown": prioritization_scope_mix.unknown
            },
            "selected_app_like_ratio": prioritization_app_like_ratio,
            "selected_preferred_app_count": preferred_package_stats.preferred_app,
            "selected_other_app_count": preferred_package_stats.other_app,
            "selected_preferred_app_ratio": preferred_package_ratio,
            "selected_component_totals_top": prioritization_component_totals_top,
            "selected_bootflow_coverage": selected_bootflow_coverage,
            "selected_bootflow_hits_top": selected_bootflow_hits_top,
            "selected": prioritization_selected
        },
        "target_selection": {
            "enabled": target_selection_stats.enabled,
            "selector": opt.function_target.map(target_label),
            "kind": opt.function_target.map(|target| target.kind()),
            "value": opt.function_target.map(|target| target.value()),
            "scope_overridden": target_selection_stats.scope_overridden,
            "matched_count": target_selection_stats.matched_count
        },
        "bootflow_discovery": {
            "main_count": bootflow_discovery.main.len(),
            "runapp_count": bootflow_discovery.runapp.len(),
            "deeplink_count": bootflow_discovery.deeplink.len(),
            "activity_count": bootflow_discovery.activity.len(),
            "bootstrap_count": bootflow_discovery.bootstrap.len(),
            "main": bootflow_main,
            "runapp": bootflow_runapp,
            "deeplink": bootflow_deeplink,
            "activity": bootflow_activity,
            "bootstrap": bootflow_bootstrap
        }
    });

    fs::write(
        opt.out_dir.join("report.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;

    if !report.passed {
        let report_path = opt.out_dir.join("report.json");
        bail!(
            "{}",
            format_quality_gate_failure_message(
                &report,
                &quality_path,
                &report_path,
                input_path,
                Some(resolved_backend),
                &symbol_quality_counts,
            )
        );
    }

    Ok(report)
}

pub fn available_adapters(repo_root: &Path) -> Result<Vec<(String, String, String, bool)>> {
    let entries = list_adapters(repo_root)?;
    Ok(entries
        .into_iter()
        .map(|(e, installed)| (e.snapshot_hash, e.version, e.adapter, installed))
        .collect())
}

#[cfg(test)]
#[path = "runners/tests.rs"]
mod runners_tests;
