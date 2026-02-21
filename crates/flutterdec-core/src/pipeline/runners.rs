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
    let pseudo = emit_program_with_pool_hints(&ir, &symbol_names, &pool_value_hints);

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
        "pool_value_hints": pool_value_hints.len()
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
        if !kind.contains("string") && !kind.contains("onebyte") && !kind.contains("twobyte") {
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
}
