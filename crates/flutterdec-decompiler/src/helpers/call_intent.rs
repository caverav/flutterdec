use std::collections::HashMap;

pub(super) fn infer_call_intent(call_name: &str) -> Option<String> {
    let lower = call_name.to_ascii_lowercase();
    if lower.starts_with("fn_0x") || lower.starts_with("sub_") {
        return None;
    }

    if let Some(rest) = lower.strip_prefix("dart_") {
        let parts: Vec<&str> = rest.split('_').filter(|p| !p.is_empty()).collect();
        if let Some(lib) = parts.first() {
            let known_lib = matches!(
                *lib,
                "core"
                    | "async"
                    | "collection"
                    | "convert"
                    | "io"
                    | "isolate"
                    | "math"
                    | "typed_data"
                    | "ffi"
                    | "developer"
            );
            if known_lib {
                if let Some(method) = parts.last() {
                    return Some(format!("stdlib:dart.{}.{}", lib, method));
                }
                return Some(format!("stdlib:dart.{}", lib));
            }
        }
    }

    if let Some(tag) = infer_flutter_framework_intent(call_name) {
        return Some(tag);
    }

    if lower.starts_with("vm_runtime_") {
        let name = lower.trim_start_matches("vm_runtime_");
        if !name.is_empty() {
            return Some(format!("runtime:dart_vm.{}", name));
        }
        return Some("runtime:dart_vm".to_string());
    }

    if lower.starts_with("native_libc_") {
        let name = lower.trim_start_matches("native_libc_");
        if !name.is_empty() {
            return Some(format!("native:libc.{}", name));
        }
        return Some("native:libc".to_string());
    }

    if lower.starts_with("native_android_log_") {
        let name = lower.trim_start_matches("native_android_log_");
        if !name.is_empty() {
            return Some(format!("native:android.log.{}", name));
        }
        return Some("native:android.log".to_string());
    }

    None
}

pub(super) fn fallback_call_name_from_selector(selector: &str) -> (String, bool) {
    let normalized = sanitize_name(selector);
    if looks_constructor_like_selector(selector) {
        return (format!("{}.new", normalized), true);
    }
    (format!("dispatch.{}", normalized), false)
}

fn infer_flutter_framework_intent(call_name: &str) -> Option<String> {
    if !call_name.to_ascii_lowercase().starts_with("flutter_") {
        return None;
    }
    let mut parts = call_name.split('_');
    let head = parts.next().unwrap_or_default();
    if !head.eq_ignore_ascii_case("flutter") {
        return None;
    }
    let lib = parts.next()?;
    let class = parts.next()?;
    let method = parts.collect::<Vec<_>>().join("_");
    if lib.is_empty() || class.is_empty() || method.is_empty() {
        return None;
    }
    Some(format!(
        "framework:flutter.{}.{}.{}",
        lib.to_ascii_lowercase(),
        class,
        method
    ))
}

pub(super) fn readable_call_name_from_intent(
    call_name: &str,
    intent: Option<&str>,
) -> Option<String> {
    let intent = intent?;
    let display = intent_display_path(intent)?;
    if display == call_name {
        return None;
    }

    let lower = call_name.to_ascii_lowercase();
    let generic = lower.starts_with("fn_0x") || lower.starts_with("sub_");
    let indirect_alias = lower == "dispatchtarget"
        || lower == "cachedtarget"
        || lower.starts_with("indirecttarget");
    let known_machine_name = lower.starts_with("dart_")
        || lower.starts_with("flutter_")
        || lower.starts_with("vm_runtime_")
        || lower.starts_with("native_libc_")
        || lower.starts_with("native_android_log_");
    if generic || known_machine_name || indirect_alias {
        return Some(display);
    }
    None
}

fn intent_display_path(intent: &str) -> Option<String> {
    let trimmed = intent
        .trim()
        .trim_end_matches(" [selector]")
        .trim_end_matches(" [library]");
    for prefix in [
        "framework:",
        "stdlib:",
        "runtime:",
        "native:",
        "package:",
        "owner:",
    ] {
        if let Some(path) = trimmed.strip_prefix(prefix) {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

pub(super) fn infer_library_intent_from_context(
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, crate::PoolSemanticHint>,
) -> Option<String> {
    for arg in args {
        for idx in extract_pool_indices(arg) {
            if let Some(uri) = pool_semantic_hints
                .get(&idx)
                .and_then(|hint| hint.library_uri.as_deref())
                .map(str::trim)
            {
                if let Some(tag) = library_intent_from_uri(uri) {
                    return Some(tag);
                }
            }
            if let Some(uri) = pool_value_hints
                .get(&idx)
                .and_then(|v| normalize_library_uri(v))
            {
                if let Some(tag) = library_intent_from_uri(&uri) {
                    return Some(tag);
                }
            }
        }
        for lit in extract_string_literals(arg) {
            if let Some(uri) = normalize_library_uri(&lit) {
                if let Some(tag) = library_intent_from_uri(&uri) {
                    return Some(tag);
                }
            }
        }
    }
    None
}

fn library_intent_from_uri(uri: &str) -> Option<String> {
    if let Some(seg) = dart_library_segment(uri) {
        return Some(format!("stdlib:dart.{}.invoke [library]", seg));
    }
    if let Some(seg) = flutter_library_segment(uri) {
        return Some(format!("framework:flutter.{}.invoke [library]", seg));
    }
    if let Some(seg) = package_library_segment(uri) {
        return Some(format!("package:{}.invoke [library]", seg));
    }
    None
}

fn normalize_library_uri(raw: &str) -> Option<String> {
    let mut t = raw.trim();
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        t = inner.trim();
    }
    if let Some((before, _)) = t.split_once("/* pool[") {
        t = before.trim();
    }
    if t.starts_with("dart:") || t.starts_with("package:") {
        return Some(t.to_string());
    }
    None
}

fn package_library_segment(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("package:")?;
    let mut parts = Vec::new();
    for raw_part in rest.split('/') {
        let trimmed = raw_part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let base = trimmed.strip_suffix(".dart").unwrap_or(trimmed);
        let token = sanitize_semantic_token(base).to_ascii_lowercase();
        if !token.is_empty() {
            parts.push(token);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

pub(super) fn infer_call_intent_with_context(
    call_name: &str,
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, crate::PoolSemanticHint>,
) -> Option<String> {
    if let Some(v) = infer_call_intent(call_name) {
        return Some(v);
    }
    infer_selector_intent_from_context(args, pool_value_hints, pool_semantic_hints)
}

pub(super) fn infer_selector_intent_from_context(
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, crate::PoolSemanticHint>,
) -> Option<String> {
    if let Some(v) =
        infer_selector_intent_from_pool_metadata(args, pool_value_hints, pool_semantic_hints)
    {
        return Some(v);
    }
    infer_selector_intent_from_pool(args, pool_value_hints)
}

pub(super) fn infer_selector_name_from_context(
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, crate::PoolSemanticHint>,
) -> Option<String> {
    if let Some(v) =
        infer_selector_name_from_pool_metadata(args, pool_value_hints, pool_semantic_hints)
    {
        return Some(v);
    }
    for arg in args {
        for idx in extract_pool_indices(arg) {
            let Some(v) = pool_value_hints.get(&idx) else {
                continue;
            };
            if let Some(sel) = extract_selector_name(v) {
                return Some(sel);
            }
        }
        for lit in extract_string_literals(arg) {
            if let Some(sel) = extract_selector_name(&lit) {
                return Some(sel);
            }
        }
    }
    None
}

fn infer_selector_name_from_pool_metadata(
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, crate::PoolSemanticHint>,
) -> Option<String> {
    for arg in args {
        for idx in extract_pool_indices(arg) {
            let Some(hint) = pool_semantic_hints.get(&idx) else {
                continue;
            };
            if let Some(sel) = selector_from_pool_hint(hint, pool_value_hints.get(&idx)) {
                return Some(sel);
            }
        }
    }
    None
}

fn infer_selector_intent_from_pool_metadata(
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, crate::PoolSemanticHint>,
) -> Option<String> {
    for arg in args {
        for idx in extract_pool_indices(arg) {
            let Some(hint) = pool_semantic_hints.get(&idx) else {
                continue;
            };
            let Some(sel) = selector_from_pool_hint(hint, pool_value_hints.get(&idx)) else {
                continue;
            };
            if let Some(tag) = semantic_intent_from_pool_hint(hint, &sel) {
                return Some(format!("{} [selector]", tag));
            }
            if let Some(tag) = classify_standard_selector(&sel) {
                return Some(tag);
            }
        }
    }
    None
}

fn selector_from_pool_hint(hint: &crate::PoolSemanticHint, fallback_value: Option<&String>) -> Option<String> {
    if let Some(sel) = hint
        .selector
        .as_deref()
        .and_then(extract_selector_name)
    {
        return Some(sel);
    }
    fallback_value
        .map(String::as_str)
        .and_then(extract_selector_name)
}

fn semantic_intent_from_pool_hint(hint: &crate::PoolSemanticHint, selector: &str) -> Option<String> {
    let owner = sanitize_semantic_token(hint.owner_class.as_deref()?);
    if owner.is_empty() {
        return None;
    }
    let method = semantic_method_from_selector(selector, &owner);
    if method.is_empty() {
        return None;
    }

    if let Some(lib_uri) = hint.library_uri.as_deref().map(str::trim) {
        if let Some(seg) = dart_library_segment(lib_uri) {
            return Some(format!("stdlib:dart.{}.{}.{}", seg, owner, method));
        }
        if let Some(seg) = flutter_library_segment(lib_uri) {
            return Some(format!("framework:flutter.{}.{}.{}", seg, owner, method));
        }
        if let Some(seg) = package_library_segment(lib_uri) {
            return Some(format!("package:{}.{}.{}", seg, owner, method));
        }
    }

    Some(format!("owner:{}.{}", owner, method))
}

fn semantic_method_from_selector(selector: &str, owner_class: &str) -> String {
    let method = sanitize_semantic_token(selector);
    if method.is_empty() {
        return String::new();
    }
    if constructor_like_selector(&method, owner_class) {
        return "new".to_string();
    }
    method
}

fn constructor_like_selector(selector: &str, owner_class: &str) -> bool {
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let sel = normalize(selector);
    let owner = normalize(owner_class);
    !sel.is_empty() && sel == owner
}

fn looks_constructor_like_selector(selector: &str) -> bool {
    let token = sanitize_semantic_token(selector);
    if token.is_empty() {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "function" | "object" | "type" | "dynamic" | "null" | "never"
    ) {
        return false;
    }
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    if token
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return false;
    }
    true
}

fn sanitize_semantic_token(input: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for c in input.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            out.push(c);
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    while out.starts_with('_') {
        out.remove(0);
    }
    out
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
    let seg = sanitize_semantic_token(seg).to_ascii_lowercase();
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
    let seg = sanitize_semantic_token(seg).to_ascii_lowercase();
    if seg.is_empty() {
        None
    } else {
        Some(seg)
    }
}

fn infer_selector_intent_from_pool(
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
) -> Option<String> {
    for arg in args {
        for idx in extract_pool_indices(arg) {
            let Some(v) = pool_value_hints.get(&idx) else {
                continue;
            };
            if let Some(tag) = classify_internal_standard_selector(v) {
                return Some(tag);
            }
            let Some(sel) = extract_selector_name(v) else {
                continue;
            };
            if let Some(tag) = classify_standard_selector(&sel) {
                return Some(tag);
            }
        }
        for lit in extract_string_literals(arg) {
            if let Some(tag) = classify_internal_standard_selector(&lit) {
                return Some(tag);
            }
            let Some(sel) = extract_selector_name(&lit) else {
                continue;
            };
            if let Some(tag) = classify_standard_selector(&sel) {
                return Some(tag);
            }
        }
    }
    None
}

fn classify_internal_standard_selector(raw: &str) -> Option<String> {
    let mut t = raw.trim();
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        t = inner.trim();
    }
    if let Some((prefix, _)) = t.split_once("/* pool[") {
        t = prefix.trim();
    }
    if t.eq_ignore_ascii_case("_current") {
        return Some("stdlib:dart.core.Iterator.current [selector]".to_string());
    }
    if t.eq_ignore_ascii_case("_equivalentYear") {
        return Some("stdlib:dart.core.DateTime.equivalentYear [selector]".to_string());
    }
    if t.eq_ignore_ascii_case("_listEquals") {
        return Some("framework:flutter.foundation.listEquals [selector]".to_string());
    }
    if t.eq_ignore_ascii_case("_prependTypeArguments") {
        return Some("runtime:dart_vm.prependTypeArguments [selector]".to_string());
    }
    if t.eq_ignore_ascii_case("_StreamController") {
        return Some("stdlib:dart.async.StreamController.new [selector]".to_string());
    }
    if t.eq_ignore_ascii_case("_RawDatagramSocket") {
        return Some("stdlib:dart.io.RawDatagramSocket.new [selector]".to_string());
    }
    None
}

fn extract_pool_indices(s: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 5] == b"pool[" {
            let mut j = i + 5;
            let mut val = 0u64;
            let mut has_digit = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                has_digit = true;
                val = val
                    .saturating_mul(10)
                    .saturating_add((bytes[j] - b'0') as u64);
                j += 1;
            }
            if has_digit && j < bytes.len() && bytes[j] == b']' {
                out.push(val);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn extract_string_literals(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut cur = String::new();
        while j < bytes.len() {
            let b = bytes[j];
            if b == b'\\' && j + 1 < bytes.len() {
                let next = bytes[j + 1] as char;
                cur.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    _ => next,
                });
                j += 2;
                continue;
            }
            if b == b'"' {
                out.push(cur);
                i = j + 1;
                break;
            }
            cur.push(b as char);
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
    }
    out
}

fn extract_selector_name(raw: &str) -> Option<String> {
    let raw_trim = raw.trim();
    if raw_trim.is_empty() {
        return None;
    }
    let raw_lower = raw_trim.to_ascii_lowercase();
    if raw_lower.contains(".dart") || raw_lower.contains('/') || raw_lower.contains('\\') {
        return None;
    }
    if raw_trim.contains("://") {
        return None;
    }
    if raw_trim.contains(' ') {
        return None;
    }

    let mut t = raw_trim;
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        t = inner.trim();
    }
    if let Some((prefix, _)) = t.split_once("/* pool[") {
        t = prefix.trim();
    }
    if let Some((before, _)) = t.split_once('@') {
        t = before.trim();
    }
    if let Some((_, after)) = t.split_once(':') {
        t = after.trim();
    }
    while let Some(rest) = t.strip_prefix('_') {
        t = rest;
    }
    if let Some(rest) = t.strip_prefix("init") {
        t = rest.trim();
    }
    if t.is_empty() || t.len() > 96 {
        return None;
    }

    let mut out = String::new();
    for c in t.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            out.push(c);
        } else if c == '.' || c == '-' || c == '/' || c == ' ' {
            if !out.ends_with('_') {
                out.push('_');
            }
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        return None;
    }
    let first = out.chars().next().unwrap_or('_');
    if (!first.is_ascii_alphabetic() && first != '_') || out.starts_with("dart_") {
        return None;
    }
    Some(out)
}
