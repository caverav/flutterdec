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

#[derive(Debug, Default, Clone, Copy)]
struct SemanticIntentSummary {
    framework: usize,
    stdlib: usize,
    runtime: usize,
    native: usize,
    selector_tagged: usize,
    constructor_calls: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct PoolMetadataStats {
    total_entries: usize,
    with_target_va: usize,
    with_selector: usize,
    with_owner_class: usize,
    with_library_uri: usize,
}

#[derive(Debug, Default, Clone)]
struct SelectorFallbackSummary {
    total: usize,
    unique: usize,
    top: Vec<SelectorFallbackEntry>,
}

#[derive(Debug, Clone)]
struct SelectorFallbackEntry {
    selector: String,
    count: usize,
    sample: String,
}

pub fn run_decompile(
    repo_root: &Path,
    input_path: &Path,
    opt: &DecompileOptions,
) -> Result<QualityReport> {
    let bundle = load_snapshot_bundle(input_path)?;
    let model = load_model(repo_root, &bundle)?;

    if model.arch != "arm64" {
        bail!("model arch {} unsupported in v1", model.arch);
    }

    let disasm = disassemble_program(
        &model,
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
    let class_to_library = build_class_library_lookup(&model);
    let pool_value_hints = build_pool_value_hints(&model);
    let pool_metadata = collect_pool_metadata_stats(&model);
    let pool_semantic_hints = build_pool_semantic_hints(&model, &class_to_library);
    let pool_target_symbols = build_pool_target_symbols(&pool_semantic_hints, &pool_value_hints);

    for f in &model.functions {
        let resolved = canonical_standard_model_name(f, &class_to_library)
            .unwrap_or_else(|| f.name.clone());
        if resolved != f.name {
            standard_model_symbol_count += 1;
        }
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

    let report = quality_from_artifacts(&model, &disasm, &pseudo, opt);
    let semantic_intent = collect_semantic_intent_summary(&pseudo);
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
        "adapter_kind": model.adapter_kind,
        "dart_version": model.dart_version,
        "counts": {
            "libraries": model.libraries.len(),
            "classes": model.classes.len(),
            "functions": model.functions.len(),
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

fn collect_semantic_intent_summary(pseudo: &[PseudocodeArtifact]) -> SemanticIntentSummary {
    let mut out = SemanticIntentSummary::default();
    for artifact in pseudo {
        for line in artifact.source.lines() {
            if line.contains("// framework:") {
                out.framework += 1;
            }
            if line.contains("// stdlib:") {
                out.stdlib += 1;
            }
            if line.contains("// runtime:") {
                out.runtime += 1;
            }
            if line.contains("// native:") {
                out.native += 1;
            }
            if line.contains("[selector]") {
                out.selector_tagged += 1;
            }
            if line.contains("final ")
                && line.contains(".new(")
                && (line.contains("flutter.") || line.contains("dart."))
            {
                out.constructor_calls += 1;
            }
        }
    }
    out
}

fn collect_selector_fallback_summary(pseudo: &[PseudocodeArtifact]) -> SelectorFallbackSummary {
    let mut out = SelectorFallbackSummary::default();
    let mut counts: HashMap<String, (usize, String)> = HashMap::new();
    for artifact in pseudo {
        for line in artifact.source.lines() {
            let Some(start) = line.find("// selector:") else {
                continue;
            };
            let rest = &line[start + "// selector:".len()..];
            let selector = rest.split(',').next().unwrap_or("").trim();
            if selector.is_empty() {
                continue;
            }
            out.total += 1;
            let sample = line
                .trim()
                .replace('\t', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let sample = if sample.len() > 180 {
                let mut truncated = sample[..180].to_string();
                truncated.push_str("...");
                truncated
            } else {
                sample
            };
            counts
                .entry(selector.to_string())
                .and_modify(|(count, _)| *count += 1)
                .or_insert((1, sample));
        }
    }

    let mut ranked = counts
        .into_iter()
        .map(|(selector, (count, sample))| SelectorFallbackEntry {
            selector,
            count,
            sample,
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.selector.cmp(&b.selector))
    });
    out.unique = ranked.len();
    out.top = ranked.into_iter().take(10).collect();
    out
}

pub fn available_adapters(repo_root: &Path) -> Result<Vec<(String, String, String, bool)>> {
    let entries = list_adapters(repo_root)?;
    Ok(entries
        .into_iter()
        .map(|(e, installed)| (e.snapshot_hash, e.version, e.adapter, installed))
        .collect())
}

fn merge_symbol_name(
    symbol_names: &mut HashMap<u64, String>,
    va: u64,
    candidate: String,
    inserted: &mut usize,
    replaced_generic: &mut usize,
    skipped: &mut usize,
) {
    let candidate = normalize_external_symbol_name(&candidate);
    if candidate.is_empty() {
        return;
    }

    match symbol_names.get(&va) {
        None => {
            symbol_names.insert(va, candidate);
            *inserted += 1;
        }
        Some(existing) => {
            if is_generic_symbol_name(existing) && !is_generic_symbol_name(&candidate) {
                symbol_names.insert(va, candidate);
                *replaced_generic += 1;
            } else {
                *skipped += 1;
            }
        }
    }
}

fn is_generic_symbol_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with("sub_") || trimmed.starts_with("fn_0x") || trimmed == "unknown" {
        return true;
    }
    false
}

fn build_class_library_lookup(model: &ProgramModel) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for c in &model.classes {
        out.entry(c.name.clone()).or_insert_with(|| c.library_uri.clone());
    }
    out
}

fn build_pool_value_hints(model: &ProgramModel) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    for e in &model.object_pool {
        let kind = e.kind.to_ascii_lowercase();
        let decoded_kind = e
            .decoded_kind
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let string_like = kind.contains("string")
            || kind.contains("onebyte")
            || kind.contains("twobyte")
            || decoded_kind.contains("string")
            || decoded_kind.contains("selector");
        if !string_like {
            continue;
        }

        let selector = e.selector.as_deref().unwrap_or("").trim();
        if !selector.is_empty() && selector.len() <= 128 {
            out.insert(e.index, selector.to_string());
            continue;
        }

        let trimmed = e.value.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            continue;
        }
        out.insert(e.index, trimmed.to_string());
    }
    out
}

fn collect_pool_metadata_stats(model: &ProgramModel) -> PoolMetadataStats {
    let mut out = PoolMetadataStats::default();
    out.total_entries = model.object_pool.len();
    for e in &model.object_pool {
        if e.target_va.is_some() {
            out.with_target_va += 1;
        }
        if e
            .selector
            .as_deref()
            .map(str::trim)
            .is_some_and(|v| !v.is_empty())
        {
            out.with_selector += 1;
        }
        if e
            .owner_class
            .as_deref()
            .map(str::trim)
            .is_some_and(|v| !v.is_empty())
        {
            out.with_owner_class += 1;
        }
        if e
            .library_uri
            .as_deref()
            .map(str::trim)
            .is_some_and(|v| !v.is_empty())
        {
            out.with_library_uri += 1;
        }
    }
    out
}

fn build_pool_semantic_hints(
    model: &ProgramModel,
    class_to_library: &HashMap<String, String>,
) -> HashMap<u64, PoolSemanticHint> {
    let mut out = HashMap::new();
    let function_meta = build_function_metadata_lookup(model, class_to_library);
    for e in &model.object_pool {
        let fallback = e
            .target_va
            .and_then(|va| function_meta.get(&va))
            .cloned();

        let selector = e
            .selector
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty() && v.len() <= 128)
            .map(str::to_string)
            .or_else(|| {
                fallback
                    .as_ref()
                    .map(|(name, _, _)| name.as_str())
                    .filter(|name| !is_generic_symbol_name(name))
                    .map(str::to_string)
            });
        let owner_class = e
            .owner_class
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty() && v.len() <= 128)
            .map(str::to_string)
            .or_else(|| {
                fallback
                    .as_ref()
                    .map(|(_, owner, _)| owner.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            });
        let library_uri = e
            .library_uri
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty() && v.len() <= 256)
            .map(str::to_string)
            .or_else(|| {
                fallback
                    .as_ref()
                    .map(|(_, _, lib)| lib.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            });
        let target_va = e.target_va;

        if selector.is_none() && owner_class.is_none() && library_uri.is_none() && target_va.is_none() {
            continue;
        }

        out.insert(
            e.index,
            PoolSemanticHint {
                selector,
                owner_class,
                library_uri,
                target_va,
            },
        );
    }
    out
}

fn build_pool_target_symbols(
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
    pool_value_hints: &HashMap<u64, String>,
) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    for (idx, hint) in pool_semantic_hints {
        let Some(target_va) = hint.target_va else {
            continue;
        };
        let Some(owner_raw) = hint
            .owner_class
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        let Some(lib_uri) = hint
            .library_uri
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        let selector_raw = hint
            .selector
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .or_else(|| pool_value_hints.get(idx).map(String::as_str))
            .unwrap_or("");
        if selector_raw.is_empty() {
            continue;
        }

        let owner = sanitize_symbol_token_stream(owner_raw);
        if owner.is_empty() {
            continue;
        }
        let selector = sanitize_symbol_token_stream(selector_raw);
        if selector.is_empty() {
            continue;
        }
        let method = if semantic_token_eq(&selector, &owner) {
            "new".to_string()
        } else {
            selector
        };

        let canonical = if let Some(seg) = dart_library_segment(lib_uri) {
            format!("dart_{}_{}_{}", seg, owner, method)
        } else if let Some(seg) = flutter_library_segment(lib_uri) {
            format!("flutter_{}_{}_{}", seg, owner, method)
        } else {
            continue;
        };
        out.entry(target_va).or_insert(canonical);
    }
    out
}

fn build_function_metadata_lookup(
    model: &ProgramModel,
    class_to_library: &HashMap<String, String>,
) -> HashMap<u64, (String, String, String)> {
    let mut out = HashMap::new();
    for f in &model.functions {
        let owner = f.owner_class.trim();
        if owner.is_empty() {
            continue;
        }
        let lib = class_to_library
            .get(&f.owner_class)
            .cloned()
            .unwrap_or_default();
        out.entry(f.entry_va)
            .or_insert_with(|| (f.name.clone(), f.owner_class.clone(), lib));
    }
    out
}

fn semantic_token_eq(lhs: &str, rhs: &str) -> bool {
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    };
    normalize(lhs) == normalize(rhs)
}

fn canonical_standard_model_name(
    f: &flutterdec_adapter::FunctionInfo,
    class_to_library: &HashMap<String, String>,
) -> Option<String> {
    if is_generic_symbol_name(&f.name) {
        return None;
    }
    let method = sanitize_symbol_token_stream(&f.name);
    if method.is_empty() || is_generic_symbol_name(&method) {
        return None;
    }

    let lib_uri = class_to_library.get(&f.owner_class)?;
    if let Some(dart_lib) = dart_library_segment(lib_uri) {
        return Some(format!("dart_{}_{}", dart_lib, method));
    }
    if let Some(flutter_seg) = flutter_library_segment(lib_uri) {
        let class_name = sanitize_symbol_token_stream(&f.owner_class);
        if class_name.is_empty() {
            return None;
        }
        return Some(format!("flutter_{}_{}_{}", flutter_seg, class_name, method));
    }
    None
}

fn dart_library_segment(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("dart:")?;
    let seg = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split('.')
        .next()
        .unwrap_or("");
    let seg = sanitize_symbol_token_stream(seg);
    if seg.is_empty() {
        None
    } else {
        Some(seg)
    }
}

fn flutter_library_segment(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("package:flutter/")?;
    let rest = rest.strip_prefix("src/").unwrap_or(rest);
    let seg = rest.split('/').next().unwrap_or("").trim_end_matches(".dart");
    let seg = sanitize_symbol_token_stream(seg);
    if seg.is_empty() {
        None
    } else {
        Some(seg)
    }
}

fn normalize_external_symbol_name(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return s;
    }
    if is_generic_symbol_name(&s) {
        return s;
    }

    if let Some((base, _)) = s.split_once('@') {
        s = base.to_string();
    }
    if let Some(demangled) = try_demangle_cpp(&s) {
        s = demangled;
    }

    if let Some(name) = canonicalize_known_symbol(&s) {
        return name;
    }

    sanitize_symbol_token_stream(&s)
}

fn try_demangle_cpp(name: &str) -> Option<String> {
    if !name.starts_with("_Z") {
        return None;
    }
    let symbol = cpp_demangle::Symbol::new(name).ok()?;
    symbol
        .demangle(&cpp_demangle::DemangleOptions::default())
        .ok()
}

fn canonicalize_known_symbol(symbol: &str) -> Option<String> {
    let lower = symbol.to_ascii_lowercase();

    if let Some(lib) = extract_dart_library(symbol) {
        let ids = extract_symbol_identifiers(symbol);
        let mut out = vec!["dart".to_string(), lib.clone()];
        if let Some(class) = ids
            .iter()
            .rev()
            .nth(1)
            .and_then(|v| normalize_symbol_piece(v))
            .filter(|v| !v.is_empty() && *v != lib && *v != "dart")
        {
            out.push(class);
        }
        if let Some(method) = ids
            .last()
            .and_then(|v| normalize_symbol_piece(v))
            .filter(|v| !v.is_empty())
        {
            out.push(method);
        }
        return Some(out.join("_"));
    }

    if let Some(rest) = symbol.strip_prefix("Dart_") {
        return Some(format!("vm_runtime_{}", sanitize_symbol_token_stream(rest)));
    }

    if let Some(rest) = symbol.strip_prefix("__android_log_") {
        return Some(format!(
            "native_android_log_{}",
            sanitize_symbol_token_stream(rest)
        ));
    }

    let libc_funcs = [
        "printf",
        "puts",
        "putchar",
        "write",
        "fwrite",
        "memcpy",
        "memmove",
        "memcmp",
        "memchr",
        "strlen",
        "strcpy",
        "strcmp",
        "strstr",
        "snprintf",
        "open",
        "close",
        "read",
        "abort",
        "malloc",
        "calloc",
        "realloc",
        "free",
    ];
    if libc_funcs.iter().any(|f| *f == lower) {
        return Some(format!("native_libc_{}", lower));
    }

    None
}

fn extract_dart_library(symbol: &str) -> Option<String> {
    let lower = symbol.to_ascii_lowercase();
    if let Some(pos) = lower.find("dart:") {
        let mut lib = String::new();
        for c in lower[pos + 5..].chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                lib.push(c);
            } else {
                break;
            }
        }
        if !lib.is_empty() {
            return Some(lib);
        }
    }

    if let Some(pos) = lower.find("dart::") {
        let mut lib = String::new();
        for c in lower[pos + 6..].chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                lib.push(c);
            } else {
                break;
            }
        }
        if !lib.is_empty() {
            return Some(lib);
        }
    }

    None
}

fn extract_symbol_identifiers(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(cur.clone());
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn normalize_symbol_piece(piece: &str) -> Option<String> {
    let cleaned = sanitize_symbol_token_stream(piece);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn sanitize_symbol_token_stream(input: &str) -> String {
    let mut out = String::new();
    let mut prev_us = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod runners_tests {
    use super::*;

    #[test]
    fn merge_symbol_name_replaces_generic_only() {
        let mut map = HashMap::new();
        map.insert(0x1000, "sub_1000".to_string());
        map.insert(0x2000, "StrongName".to_string());

        let mut inserted = 0usize;
        let mut replaced = 0usize;
        let mut skipped = 0usize;

        merge_symbol_name(
            &mut map,
            0x1000,
            "RealSymbol".to_string(),
            &mut inserted,
            &mut replaced,
            &mut skipped,
        );
        merge_symbol_name(
            &mut map,
            0x2000,
            "OtherSymbol".to_string(),
            &mut inserted,
            &mut replaced,
            &mut skipped,
        );
        merge_symbol_name(
            &mut map,
            0x3000,
            "InsertedSymbol".to_string(),
            &mut inserted,
            &mut replaced,
            &mut skipped,
        );

        assert_eq!(map.get(&0x1000).map(String::as_str), Some("RealSymbol"));
        assert_eq!(map.get(&0x2000).map(String::as_str), Some("StrongName"));
        assert_eq!(map.get(&0x3000).map(String::as_str), Some("InsertedSymbol"));
        assert_eq!(inserted, 1);
        assert_eq!(replaced, 1);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn generic_name_detection_is_strict() {
        assert!(is_generic_symbol_name("sub_1234"));
        assert!(is_generic_symbol_name("fn_0x55"));
        assert!(is_generic_symbol_name("unknown"));
        assert!(!is_generic_symbol_name("Dart_Invoke"));
    }

    #[test]
    fn normalizes_known_external_symbols() {
        assert_eq!(
            normalize_external_symbol_name("Dart_Invoke"),
            "vm_runtime_Invoke"
        );
        assert_eq!(
            normalize_external_symbol_name("memcpy@LIBC"),
            "native_libc_memcpy"
        );
        assert_eq!(
            normalize_external_symbol_name("__android_log_print"),
            "native_android_log_print"
        );
        assert_eq!(
            normalize_external_symbol_name("dart:core::print"),
            "dart_core_print"
        );
    }

    #[test]
    fn canonicalizes_standard_model_function_names() {
        let mut class_lib = HashMap::new();
        class_lib.insert("_StringBase".to_string(), "dart:core".to_string());
        class_lib.insert(
            "State".to_string(),
            "package:flutter/src/widgets/framework.dart".to_string(),
        );
        class_lib.insert(
            "RenderObject".to_string(),
            "package:flutter/src/rendering/object.dart".to_string(),
        );

        let dart_fn = flutterdec_adapter::FunctionInfo {
            id: 1,
            name: "toString".to_string(),
            owner_class: "_StringBase".to_string(),
            entry_va: 0x1000,
            size: 4,
            code_section_va: 0x1000,
        };
        assert_eq!(
            canonical_standard_model_name(&dart_fn, &class_lib).as_deref(),
            Some("dart_core_toString")
        );

        let flutter_fn = flutterdec_adapter::FunctionInfo {
            id: 2,
            name: "setState".to_string(),
            owner_class: "State".to_string(),
            entry_va: 0x2000,
            size: 4,
            code_section_va: 0x2000,
        };
        assert_eq!(
            canonical_standard_model_name(&flutter_fn, &class_lib).as_deref(),
            Some("flutter_widgets_State_setState")
        );

        let render_fn = flutterdec_adapter::FunctionInfo {
            id: 3,
            name: "layout".to_string(),
            owner_class: "RenderObject".to_string(),
            entry_va: 0x3000,
            size: 4,
            code_section_va: 0x3000,
        };
        assert_eq!(
            canonical_standard_model_name(&render_fn, &class_lib).as_deref(),
            Some("flutter_rendering_RenderObject_layout")
        );

        let generic_fn = flutterdec_adapter::FunctionInfo {
            id: 4,
            name: "sub_1234".to_string(),
            owner_class: "State".to_string(),
            entry_va: 0x4000,
            size: 4,
            code_section_va: 0x4000,
        };
        assert!(canonical_standard_model_name(&generic_fn, &class_lib).is_none());
    }

    #[test]
    fn aggregates_semantic_intent_counts_from_pseudocode() {
        let pseudo = vec![
            PseudocodeArtifact {
                function_id: 1,
                function_name: "f1".to_string(),
                source: r#"dynamic f1(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = flutter.widgets.KeyedSubtree.new(arg0, arg1, arg2, arg3); // framework:flutter.widgets.KeyedSubtree.new [selector]
  final t2 = dart.core.List.removeAt(arg0, arg1, arg2, arg3); // stdlib:dart.core.List.removeAt [selector]
  final t3 = vm_runtime_Invoke(arg0, arg1, arg2, arg3); // runtime:dart_vm.invoke
  final t4 = native_libc_memcpy(arg0, arg1, arg2, arg3); // native:libc.memcpy
  return t4;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 4,
                indirect_calls: 0,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 0,
                target_va_symbol_calls: 0,
            },
            PseudocodeArtifact {
                function_id: 2,
                function_name: "f2".to_string(),
                source: r#"dynamic f2(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.invoke(arg0, arg1, arg2, arg3); // indirect via: dispatchTarget
  return t1;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 1,
                indirect_calls: 1,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 0,
                target_va_symbol_calls: 0,
            },
        ];

        let summary = collect_semantic_intent_summary(&pseudo);
        assert_eq!(summary.framework, 1);
        assert_eq!(summary.stdlib, 1);
        assert_eq!(summary.runtime, 1);
        assert_eq!(summary.native, 1);
        assert_eq!(summary.selector_tagged, 2);
        assert_eq!(summary.constructor_calls, 1);
    }

    #[test]
    fn summarizes_selector_fallback_counts_from_pseudocode() {
        let pseudo = vec![
            PseudocodeArtifact {
                function_id: 1,
                function_name: "f1".to_string(),
                source: r#"dynamic f1(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.current(arg0, arg1, arg2, arg3); // selector: current, indirect via: dispatchTarget
  final t2 = dispatch.current(arg0, arg1, arg2, arg3); // selector: current, indirect via: dispatchTarget
  return t2;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 2,
                indirect_calls: 2,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 2,
                target_va_symbol_calls: 0,
            },
            PseudocodeArtifact {
                function_id: 2,
                function_name: "f2".to_string(),
                source: r#"dynamic f2(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.customAction(arg0, arg1, arg2, arg3); // selector: customAction, indirect via: indirectTarget9
  return t1;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 1,
                indirect_calls: 1,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 1,
                target_va_symbol_calls: 0,
            },
        ];

        let summary = collect_selector_fallback_summary(&pseudo);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.unique, 2);
        assert_eq!(summary.top.first().map(|v| v.selector.as_str()), Some("current"));
        assert_eq!(summary.top.first().map(|v| v.count), Some(2));
        assert!(
            summary
                .top
                .first()
                .map(|v| v.sample.contains("dispatch.current("))
                .unwrap_or(false)
        );
        assert_eq!(
            summary.top.get(1).map(|v| v.selector.as_str()),
            Some("customAction")
        );
        assert_eq!(summary.top.get(1).map(|v| v.count), Some(1));
    }

    #[test]
    fn builds_pool_semantic_hints_from_adapter_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 7,
                    kind: "String".to_string(),
                    value: "didChangeMetrics".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("didChangeMetrics".to_string()),
                    target_va: Some(0x1234),
                    owner_class: Some("WidgetsBindingObserver".to_string()),
                    library_uri: Some("package:flutter/src/widgets/binding.dart".to_string()),
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 8,
                    kind: "Smi".to_string(),
                    value: "42".to_string(),
                    decoded_kind: None,
                    selector: None,
                    target_va: None,
                    owner_class: None,
                    library_uri: None,
                },
            ],
        };

        let class_to_library = build_class_library_lookup(&model);
        let hints = build_pool_semantic_hints(&model, &class_to_library);
        assert_eq!(hints.len(), 1);
        let h = hints.get(&7).expect("missing semantic hint entry");
        assert_eq!(h.selector.as_deref(), Some("didChangeMetrics"));
        assert_eq!(h.owner_class.as_deref(), Some("WidgetsBindingObserver"));
        assert_eq!(
            h.library_uri.as_deref(),
            Some("package:flutter/src/widgets/binding.dart")
        );
        assert_eq!(h.target_va, Some(0x1234));
    }

    #[test]
    fn collects_pool_metadata_coverage_stats() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 1,
                    kind: "String".to_string(),
                    value: "a".to_string(),
                    decoded_kind: None,
                    selector: Some("setState".to_string()),
                    target_va: Some(0x1000),
                    owner_class: Some("State".to_string()),
                    library_uri: Some("package:flutter/src/widgets/framework.dart".to_string()),
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 2,
                    kind: "Smi".to_string(),
                    value: "42".to_string(),
                    decoded_kind: None,
                    selector: None,
                    target_va: None,
                    owner_class: None,
                    library_uri: None,
                },
            ],
        };

        let stats = collect_pool_metadata_stats(&model);
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.with_target_va, 1);
        assert_eq!(stats.with_selector, 1);
        assert_eq!(stats.with_owner_class, 1);
        assert_eq!(stats.with_library_uri, 1);
    }

    #[test]
    fn builds_pool_target_symbols_from_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 7,
                    kind: "String".to_string(),
                    value: "didChangeMetrics".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("didChangeMetrics".to_string()),
                    target_va: Some(0x1234),
                    owner_class: Some("WidgetsBindingObserver".to_string()),
                    library_uri: Some("package:flutter/src/widgets/binding.dart".to_string()),
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 8,
                    kind: "String".to_string(),
                    value: "Int64List".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("Int64List".to_string()),
                    target_va: Some(0x2234),
                    owner_class: Some("Int64List".to_string()),
                    library_uri: Some("dart:typed_data".to_string()),
                },
            ],
        };

        let class_to_library = build_class_library_lookup(&model);
        let hints = build_pool_semantic_hints(&model, &class_to_library);
        let values = build_pool_value_hints(&model);
        let map = build_pool_target_symbols(&hints, &values);
        assert_eq!(
            map.get(&0x1234).map(String::as_str),
            Some("flutter_widgets_WidgetsBindingObserver_didChangeMetrics")
        );
        assert_eq!(
            map.get(&0x2234).map(String::as_str),
            Some("dart_typed_data_Int64List_new")
        );
    }

    #[test]
    fn enriches_pool_semantic_hints_from_function_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 1,
                name: "State".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:flutter/src/widgets/framework.dart".to_string(),
            }],
            functions: vec![flutterdec_adapter::FunctionInfo {
                id: 11,
                name: "setState".to_string(),
                owner_class: "State".to_string(),
                entry_va: 0x4000,
                size: 4,
                code_section_va: 0x4000,
            }],
            object_pool: vec![flutterdec_adapter::ObjectPoolEntry {
                index: 21,
                kind: "Closure".to_string(),
                value: "opaque".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: Some(0x4000),
                owner_class: None,
                library_uri: None,
            }],
        };

        let class_to_library = build_class_library_lookup(&model);
        let hints = build_pool_semantic_hints(&model, &class_to_library);
        let h = hints.get(&21).expect("missing enriched semantic hint");
        assert_eq!(h.selector.as_deref(), Some("setState"));
        assert_eq!(h.owner_class.as_deref(), Some("State"));
        assert_eq!(
            h.library_uri.as_deref(),
            Some("package:flutter/src/widgets/framework.dart")
        );
        assert_eq!(h.target_va, Some(0x4000));
    }
}
