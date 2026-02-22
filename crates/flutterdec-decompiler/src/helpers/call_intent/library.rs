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
            if let Some(uri) = pool_value_hints.get(&idx).and_then(|v| normalize_library_uri(v)) {
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
