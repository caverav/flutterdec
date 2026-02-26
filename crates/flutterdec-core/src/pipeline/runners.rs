#[path = "runners/reporting.rs"]
mod runners_reporting;
use runners_reporting::{
    collect_bootflow_discovery, collect_call_fallback_summary, collect_semantic_intent_summary,
    collect_selector_fallback_summary, BootflowDiscoveryEntry, BootflowDiscoverySummary,
    CallFallbackSummary, SelectorFallbackSummary, SemanticIntentSummary,
};
#[path = "runners/manifest.rs"]
mod runners_manifest;
use runners_manifest::{
    enrich_model_with_manifest_bootflow_hints, inspect_android_manifest,
};
#[cfg(test)]
use runners_manifest::AndroidManifestSignals;
#[path = "runners/symbols.rs"]
mod runners_symbols;
use runners_symbols::{
    build_class_library_lookup, build_pool_semantic_hints, build_pool_target_symbols,
    build_pool_value_hints, canonical_standard_model_name, collect_pool_metadata_stats,
    merge_symbol_name,
};
#[cfg(test)]
use runners_symbols::{is_generic_symbol_name, normalize_external_symbol_name};
use std::collections::HashSet;

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
    f: &flutterdec_adapter::FunctionInfo,
    class_to_library: &HashMap<String, String>,
) -> ScopedFunctionKind {
    let Some(uri) = class_to_library.get(&f.owner_class) else {
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

fn collect_app_package_counts(model: &ProgramModel) -> Vec<(String, usize)> {
    let mut class_to_library = HashMap::new();
    for c in &model.classes {
        class_to_library
            .entry(c.name.clone())
            .or_insert_with(|| c.library_uri.clone());
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &model.functions {
        let Some(uri) = class_to_library.get(&f.owner_class) else {
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
        let key = priority_package_from_library_uri(&item.library_uri);
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
        match classify_library_uri(&item.library_uri) {
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
        if classify_library_uri(&item.library_uri) != ScopedFunctionKind::App {
            continue;
        }
        let Some(pkg) = package_name_from_library_uri(&item.library_uri) else {
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
    decoded_kind: String,
    selector: String,
    target_va: u64,
    function_name: String,
    owner_class: String,
    library_uri: String,
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
        category_discovered.insert(entry.target_va);
        any_discovered.insert(entry.target_va);
        let Some(selected) = selected_by_entry_va.get(&entry.target_va) else {
            continue;
        };
        category_selected.insert(entry.target_va);
        any_selected.insert(entry.target_va);
        let hit_key = format!(
            "{}|0x{:x}|{}",
            category,
            entry.target_va,
            entry.selector.to_ascii_lowercase()
        );
        if !seen_hit_keys.insert(hit_key) {
            continue;
        }
        hits.push(SelectedBootflowHit {
            category: category.to_string(),
            decoded_kind: entry.decoded_kind.clone(),
            selector: entry.selector.clone(),
            target_va: entry.target_va,
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
    let mut class_to_library = HashMap::new();
    for c in &model.classes {
        class_to_library
            .entry(c.name.clone())
            .or_insert_with(|| c.library_uri.clone());
    }
    let package_filters = normalize_package_filters(app_packages);

    let mut stats = FunctionScopeStats::from_total(model.functions.len());
    let mut filtered_functions = Vec::new();
    for f in &model.functions {
        let kind = function_kind_from_model(f, &class_to_library);
        let package_name = class_to_library
            .get(&f.owner_class)
            .and_then(|uri| package_name_from_library_uri(uri));
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

pub fn run_info(repo_root: &Path, input_path: &Path) -> Result<InfoOutput> {
    let bundle = load_snapshot_bundle(input_path)?;
    let adapter_installed = resolve_adapter_exec(repo_root, &bundle.snapshot_hash).is_ok();

    let mut out = InfoOutput {
        input_path: bundle.input_path.display().to_string(),
        libapp_path: bundle.libapp_path.display().to_string(),
        arch: bundle.arch.clone(),
        snapshot_hash: bundle.snapshot_hash.clone(),
        adapter_installed,
        function_count: None,
        class_count: None,
        object_pool_count: None,
        app_package_count_total: None,
        app_package_counts_top: None,
    };

    if adapter_installed {
        if let Ok(model) = load_model(repo_root, &bundle, AdapterBackend::Auto) {
            out.function_count = Some(model.functions.len());
            out.class_count = Some(model.classes.len());
            out.object_pool_count = Some(model.object_pool.len());
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

pub fn run_decompile(
    repo_root: &Path,
    input_path: &Path,
    opt: &DecompileOptions,
) -> Result<QualityReport> {
    let bundle = load_snapshot_bundle(input_path)?;
    let loaded_model = load_model(repo_root, &bundle, opt.adapter_backend)?;
    let manifest_inspection = inspect_android_manifest(input_path);
    let (model, manifest_synthetic_hints) = if manifest_inspection.present {
        enrich_model_with_manifest_bootflow_hints(&loaded_model, &manifest_inspection.signals)
    } else {
        (loaded_model, 0)
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

    if scoped_model.arch != "arm64" {
        bail!("model arch {} unsupported in v1", scoped_model.arch);
    }
    if scoped_model.functions.is_empty() {
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
        &scoped_model,
        &bundle.isolate_instr,
        bundle.isolate_instr_va,
        opt.focus.as_deref(),
        opt.max_functions,
        &priority_package_hints,
        opt.engine_options.bootflow_category_seeds,
    );
    let ir: Vec<FunctionIr> = build_program_ir(&disasm);
    let mut symbol_names: HashMap<u64, String> = HashMap::new();
    let mut symbol_merge_inserted = 0usize;
    let mut symbol_merge_replaced_generic = 0usize;
    let mut symbol_merge_skipped = 0usize;
    let mut standard_model_symbol_count = 0usize;
    let class_to_library = if opt.engine_options.canonical_model_symbols
        || opt.engine_options.pool_semantic_hints
    {
        build_class_library_lookup(&scoped_model)
    } else {
        HashMap::new()
    };
    let pool_value_hints = if opt.engine_options.pool_value_hints
        || opt.engine_options.pool_semantic_hints
    {
        build_pool_value_hints(&model)
    } else {
        HashMap::new()
    };
    let pool_metadata = collect_pool_metadata_stats(&model);
    let function_name_kind_count = model
        .functions
        .iter()
        .filter(|f| {
            f.name_kind
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
        })
        .count();
    let pool_confidence_count = model
        .object_pool
        .iter()
        .filter(|e| e.confidence.is_some())
        .count();
    let pool_source_count = model
        .object_pool
        .iter()
        .filter(|e| {
            e.source
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
        })
        .count();
    let pool_semantic_hints = if opt.engine_options.pool_semantic_hints {
        build_pool_semantic_hints(&model, &class_to_library)
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

    for f in &scoped_model.functions {
        let resolved = if opt.engine_options.canonical_model_symbols {
            let resolved = canonical_standard_model_name(f, &class_to_library)
                .unwrap_or_else(|| f.name.clone());
            if resolved != f.name {
                standard_model_symbol_count += 1;
            }
            resolved
        } else {
            f.name.clone()
        };
        symbol_names.insert(f.entry_va, resolved);
    }
    for f in &disasm {
        symbol_names
            .entry(f.entry_va)
            .or_insert_with(|| f.function_name.clone());
    }
    for (va, name) in &pool_target_symbols {
        merge_symbol_name(
            &mut symbol_names,
            *va,
            name.clone(),
            &mut symbol_merge_inserted,
            &mut symbol_merge_replaced_generic,
            &mut symbol_merge_skipped,
        );
    }
    for elf_path in &opt.extra_symbol_elfs {
        let ext = load_elf_function_symbols(elf_path)
            .with_context(|| format!("load external symbols from {}", elf_path.display()))?;
        for (va, name) in ext {
            merge_symbol_name(
                &mut symbol_names,
                va,
                name,
                &mut symbol_merge_inserted,
                &mut symbol_merge_replaced_generic,
                &mut symbol_merge_skipped,
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
                va,
                name,
                &mut symbol_merge_inserted,
                &mut symbol_merge_replaced_generic,
                &mut symbol_merge_skipped,
            );
        }
    }
    let pseudo = emit_program_with_pool_context(
        &ir,
        &symbol_names,
        &pool_value_hints,
        &pool_semantic_hints,
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
        fs::write(pseudo_dir.join(filename), &p.source)?;
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
                normalize_file_name(&f.function_name)
            );
            fs::write(asm_dir.join(filename), lines.join("\n"))?;
        }
    }

    if opt.emit_ir {
        for f in &ir {
            let filename = format!("{:05}_{}.json", f.function_id, normalize_file_name(&f.name));
            fs::write(ir_dir.join(filename), serde_json::to_vec_pretty(f)?)?;
        }
    }

    let report = quality_from_artifacts(&scoped_model, &disasm, &pseudo, opt);
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
    let bootflow_discovery = collect_bootflow_discovery(&model);
    let (selected_bootflow_stats, selected_bootflow_hits) =
        collect_selected_bootflow_hits(&prioritization_selected, &bootflow_discovery);
    let selected_bootflow_hits_top = selected_bootflow_hits
        .iter()
        .take(20)
        .map(|hit| {
            json!({
                "category": hit.category,
                "decoded_kind": hit.decoded_kind,
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
                "decoded_kind": entry.decoded_kind,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "value": entry.value
            })
        })
        .collect::<Vec<_>>();
    let bootflow_runapp = bootflow_discovery
        .runapp
        .iter()
        .map(|entry| {
            json!({
                "decoded_kind": entry.decoded_kind,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "value": entry.value
            })
        })
        .collect::<Vec<_>>();
    let bootflow_deeplink = bootflow_discovery
        .deeplink
        .iter()
        .map(|entry| {
            json!({
                "decoded_kind": entry.decoded_kind,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "value": entry.value
            })
        })
        .collect::<Vec<_>>();
    let bootflow_activity = bootflow_discovery
        .activity
        .iter()
        .map(|entry| {
            json!({
                "decoded_kind": entry.decoded_kind,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "value": entry.value
            })
        })
        .collect::<Vec<_>>();
    let bootflow_bootstrap = bootflow_discovery
        .bootstrap
        .iter()
        .map(|entry| {
            json!({
                "decoded_kind": entry.decoded_kind,
                "selector": entry.selector,
                "target_va": entry.target_va,
                "owner_class": entry.owner_class,
                "library_uri": entry.library_uri,
                "value": entry.value
            })
        })
        .collect::<Vec<_>>();
    fs::create_dir_all(&opt.out_dir)?;

    let quality_path = opt.out_dir.join("quality.json");
    fs::write(&quality_path, serde_json::to_vec_pretty(&report)?)?;

    let summary = json!({
        "input": bundle.input_path,
        "libapp": bundle.libapp_path,
        "arch": bundle.arch,
        "snapshot_hash": bundle.snapshot_hash,
        "analysis": {
            "profile": opt.analysis_profile.as_str(),
            "engine": &opt.engine_options
        },
        "adapter_kind": model.adapter_kind,
        "adapter_schema": {
            "schema_version": model.schema_version,
            "compatibility_mode": if model.schema_version == 2 { "v2_compat" } else { "native_v3" },
            "function_name_kind_count": function_name_kind_count,
            "pool_confidence_count": pool_confidence_count,
            "pool_source_count": pool_source_count
        },
        "dart_version": model.dart_version,
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
            "main_launcher": manifest_inspection.signals.has_main_launcher,
            "view_browsable": manifest_inspection.signals.has_view_browsable,
            "activity_count": manifest_inspection.signals.activities.len(),
            "activities": manifest_inspection.signals.activities,
            "deeplink_entry_count": manifest_inspection.signals.deeplink_entries.len(),
            "deeplink_entries": manifest_inspection.signals.deeplink_entries,
            "synthetic_bootflow_hints": manifest_synthetic_hints
        },
        "counts": {
            "libraries": model.libraries.len(),
            "classes": model.classes.len(),
            "functions": scoped_model.functions.len(),
            "functions_total": model.functions.len(),
            "object_pool": model.object_pool.len(),
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
        "symbol_merge": {
            "inserted": symbol_merge_inserted,
            "replaced_generic": symbol_merge_replaced_generic,
            "skipped": symbol_merge_skipped
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
            "with_owner_class": pool_metadata.with_owner_class,
            "with_library_uri": pool_metadata.with_library_uri
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
        "prioritization": {
            "enabled": opt.max_functions.is_some(),
            "focus": opt.focus.clone(),
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
        bail!("quality gate failed. see {}", quality_path.display());
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
