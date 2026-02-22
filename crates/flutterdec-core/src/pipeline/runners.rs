#[path = "runners/reporting.rs"]
mod runners_reporting;
use runners_reporting::{
    collect_call_fallback_summary, collect_semantic_intent_summary,
    collect_selector_fallback_summary, CallFallbackSummary, SelectorFallbackSummary,
    SemanticIntentSummary,
};
#[path = "runners/symbols.rs"]
mod runners_symbols;
use runners_symbols::{
    build_class_library_lookup, build_pool_semantic_hints, build_pool_target_symbols,
    build_pool_value_hints, canonical_standard_model_name, collect_pool_metadata_stats,
    merge_symbol_name,
};
#[cfg(test)]
use runners_symbols::{is_generic_symbol_name, normalize_external_symbol_name};

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

fn include_function_kind(scope: FunctionScope, kind: ScopedFunctionKind) -> bool {
    match scope {
        FunctionScope::All => true,
        FunctionScope::App => kind == ScopedFunctionKind::App,
        FunctionScope::AppUnknown => {
            kind == ScopedFunctionKind::App || kind == ScopedFunctionKind::Unknown
        }
    }
}

fn apply_function_scope_filter(
    model: &ProgramModel,
    scope: FunctionScope,
) -> (ProgramModel, FunctionScopeStats) {
    let mut class_to_library = HashMap::new();
    for c in &model.classes {
        class_to_library
            .entry(c.name.clone())
            .or_insert_with(|| c.library_uri.clone());
    }

    let mut stats = FunctionScopeStats::from_total(model.functions.len());
    let mut filtered_functions = Vec::new();
    for f in &model.functions {
        let kind = function_kind_from_model(f, &class_to_library);
        match kind {
            ScopedFunctionKind::App => stats.app += 1,
            ScopedFunctionKind::Framework => stats.framework += 1,
            ScopedFunctionKind::Stdlib => stats.stdlib += 1,
            ScopedFunctionKind::Unknown => stats.unknown += 1,
        }
        if include_function_kind(scope, kind) {
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
    };

    if adapter_installed {
        if let Ok(model) = load_model(repo_root, &bundle) {
            out.function_count = Some(model.functions.len());
            out.class_count = Some(model.classes.len());
            out.object_pool_count = Some(model.object_pool.len());
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
    let model = load_model(repo_root, &bundle)?;
    let (scoped_model, function_scope_stats) = apply_function_scope_filter(&model, opt.function_scope);

    if scoped_model.arch != "arm64" {
        bail!("model arch {} unsupported in v1", scoped_model.arch);
    }
    if scoped_model.functions.is_empty() {
        bail!(
            "no functions matched --function-scope {}. try --function-scope all",
            opt.function_scope.as_str()
        );
    }

    let disasm = disassemble_program(
        &scoped_model,
        &bundle.isolate_instr,
        bundle.isolate_instr_va,
        opt.focus.as_deref(),
        opt.max_functions,
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
                let mut line = format!("0x{:x}: {}", i.va, i.mnemonic);
                if !i.op_str.is_empty() {
                    line.push(' ');
                    line.push_str(&i.op_str);
                }
                if !i.annotation.is_empty() {
                    line.push_str(" ; ");
                    line.push_str(&i.annotation);
                }
                lines.push(line);
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
        "dart_version": model.dart_version,
        "function_scope": {
            "selected": opt.function_scope.as_str(),
            "total_before_filter": function_scope_stats.total_before_filter,
            "total_after_filter": function_scope_stats.total_after_filter,
            "excluded": function_scope_stats.excluded,
            "categories": {
                "app": function_scope_stats.app,
                "framework": function_scope_stats.framework,
                "stdlib": function_scope_stats.stdlib,
                "unknown": function_scope_stats.unknown
            }
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
