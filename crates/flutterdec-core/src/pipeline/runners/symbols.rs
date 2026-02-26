use flutterdec_adapter::{FunctionInfo, ProgramModel};
use flutterdec_decompiler::PoolSemanticHint;
use std::collections::HashMap;

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct PoolMetadataStats {
    pub(super) total_entries: usize,
    pub(super) with_target_va: usize,
    pub(super) with_selector: usize,
    pub(super) with_owner_class: usize,
    pub(super) with_library_uri: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SymbolNameQuality {
    Placeholder,
    Heuristic,
    External,
    Exact,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct SymbolMergeStats {
    pub(super) inserted: usize,
    pub(super) replaced: usize,
    pub(super) skipped: usize,
    pub(super) replaced_to_placeholder: usize,
    pub(super) replaced_to_heuristic: usize,
    pub(super) replaced_to_external: usize,
    pub(super) replaced_to_exact: usize,
}

impl SymbolMergeStats {
    fn record_replacement(&mut self, quality: SymbolNameQuality) {
        self.replaced += 1;
        match quality {
            SymbolNameQuality::Placeholder => self.replaced_to_placeholder += 1,
            SymbolNameQuality::Heuristic => self.replaced_to_heuristic += 1,
            SymbolNameQuality::External => self.replaced_to_external += 1,
            SymbolNameQuality::Exact => self.replaced_to_exact += 1,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct SymbolQualityCounts {
    pub(super) placeholder: usize,
    pub(super) heuristic: usize,
    pub(super) external: usize,
    pub(super) exact: usize,
}

impl SymbolQualityCounts {
    fn bump(&mut self, quality: SymbolNameQuality) {
        match quality {
            SymbolNameQuality::Placeholder => self.placeholder += 1,
            SymbolNameQuality::Heuristic => self.heuristic += 1,
            SymbolNameQuality::External => self.external += 1,
            SymbolNameQuality::Exact => self.exact += 1,
        }
    }
}

pub(super) fn symbol_name_quality_from_name_kind(raw: Option<&str>) -> Option<SymbolNameQuality> {
    let token = raw?.trim().to_ascii_lowercase();
    match token.as_str() {
        "placeholder" => Some(SymbolNameQuality::Placeholder),
        "heuristic" => Some(SymbolNameQuality::Heuristic),
        "external" => Some(SymbolNameQuality::External),
        "exact" => Some(SymbolNameQuality::Exact),
        _ => None,
    }
}

pub(super) fn infer_symbol_name_quality(name: &str) -> SymbolNameQuality {
    if is_generic_symbol_name(name) {
        SymbolNameQuality::Placeholder
    } else if is_heuristic_canonical_symbol_name(name) {
        SymbolNameQuality::Heuristic
    } else {
        SymbolNameQuality::External
    }
}

pub(super) fn collect_symbol_quality_counts(
    symbol_quality: &HashMap<u64, SymbolNameQuality>,
) -> SymbolQualityCounts {
    let mut counts = SymbolQualityCounts::default();
    for quality in symbol_quality.values() {
        counts.bump(*quality);
    }
    counts
}

pub(super) fn merge_symbol_name(
    symbol_names: &mut HashMap<u64, String>,
    symbol_quality: &mut HashMap<u64, SymbolNameQuality>,
    va: u64,
    candidate: String,
    candidate_quality: Option<SymbolNameQuality>,
    stats: &mut SymbolMergeStats,
) {
    let candidate = normalize_external_symbol_name(&candidate);
    if candidate.is_empty() {
        return;
    }
    let candidate_quality =
        candidate_quality.unwrap_or_else(|| infer_symbol_name_quality(&candidate));

    match symbol_names.get(&va).cloned() {
        None => {
            symbol_names.insert(va, candidate);
            symbol_quality.insert(va, candidate_quality);
            stats.inserted += 1;
        }
        Some(existing_name) => {
            let existing_quality = symbol_quality
                .get(&va)
                .copied()
                .unwrap_or_else(|| infer_symbol_name_quality(&existing_name));
            symbol_quality.entry(va).or_insert(existing_quality);
            if candidate_quality > existing_quality {
                symbol_names.insert(va, candidate);
                symbol_quality.insert(va, candidate_quality);
                stats.record_replacement(candidate_quality);
            } else {
                stats.skipped += 1;
            }
        }
    }
}

pub(super) fn is_generic_symbol_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered == "unknown"
        || lowered.starts_with("sub_")
        || lowered.starts_with("fn_0x")
        || lowered.starts_with("nullsub_")
        || lowered.starts_with("loc_")
        || lowered.starts_with("off_")
    {
        return true;
    }
    if let Some(rest) = lowered.strip_prefix("fun_") {
        let token = rest.strip_prefix("0x").unwrap_or(rest).trim_matches('_');
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    false
}

fn is_heuristic_canonical_symbol_name(name: &str) -> bool {
    name.starts_with("dart_") || name.starts_with("flutter_") || name.starts_with("package_")
}

pub(super) fn build_class_library_lookup(model: &ProgramModel) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for c in &model.classes {
        out.entry(c.name.clone()).or_insert_with(|| c.library_uri.clone());
    }
    out
}

pub(super) fn build_pool_value_hints(model: &ProgramModel) -> HashMap<u64, String> {
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

pub(super) fn collect_pool_metadata_stats(model: &ProgramModel) -> PoolMetadataStats {
    let mut out = PoolMetadataStats {
        total_entries: model.object_pool.len(),
        ..PoolMetadataStats::default()
    };
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

pub(super) fn build_pool_semantic_hints(
    model: &ProgramModel,
    class_to_library: &HashMap<String, String>,
) -> HashMap<u64, PoolSemanticHint> {
    let mut out = HashMap::new();
    let function_meta = build_function_metadata_lookup(model, class_to_library);
    for e in &model.object_pool {
        let fallback = e.target_va.and_then(|va| function_meta.get(&va)).cloned();

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

        if selector.is_none() && owner_class.is_none() && library_uri.is_none() && target_va.is_none()
        {
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

pub(super) fn build_pool_target_symbols(
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
        } else if let Some(pkg) = package_library_segment(lib_uri) {
            format!("package_{}_{}_{}", pkg, owner, method)
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

pub(super) fn canonical_standard_model_name(
    f: &FunctionInfo,
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
    let mut head_tail = rest.splitn(2, '/');
    let head_raw = head_tail.next().unwrap_or("");
    let seg = head_raw.split('.').next().unwrap_or("");
    let seg = sanitize_symbol_token_stream(seg);
    if seg.is_empty() {
        return None;
    }

    let mut out = seg;
    let patch_like = head_raw.to_ascii_lowercase().contains("patch");
    if patch_like {
        if let Some(tail) = head_tail.next() {
            let stem = tail
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches(".dart");
            let stem = sanitize_symbol_token_stream(stem);
            if !stem.is_empty() && stem != out {
                out.push('_');
                out.push_str(&stem);
            }
        }
    }

    Some(out)
}

fn flutter_library_segment(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("package:flutter/")?;
    let rest = rest.strip_prefix("src/").unwrap_or(rest);
    let seg = rest.split('/').next().unwrap_or("").trim_end_matches(".dart");
    let seg = sanitize_symbol_token_stream(seg);
    if seg.is_empty() { None } else { Some(seg) }
}

fn package_library_segment(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("package:")?;
    let pkg = rest.split('/').next().unwrap_or("").trim();
    if pkg.is_empty() || pkg.eq_ignore_ascii_case("flutter") {
        return None;
    }
    let seg = sanitize_symbol_token_stream(pkg);
    if seg.is_empty() {
        None
    } else {
        Some(seg.to_ascii_lowercase())
    }
}

pub(super) fn normalize_external_symbol_name(raw: &str) -> String {
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
    symbol.demangle().ok()
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
        "printf", "puts", "putchar", "write", "fwrite", "memcpy", "memmove", "memcmp", "memchr",
        "strlen", "strcpy", "strcmp", "strstr", "snprintf", "open", "close", "read", "abort",
        "malloc", "calloc", "realloc", "free",
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
