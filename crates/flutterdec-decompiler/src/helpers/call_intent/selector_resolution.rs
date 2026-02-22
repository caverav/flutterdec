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
    if let Some(v) = infer_selector_name_from_pool_metadata(args, pool_value_hints, pool_semantic_hints)
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

fn selector_from_pool_hint(
    hint: &crate::PoolSemanticHint,
    fallback_value: Option<&String>,
) -> Option<String> {
    if let Some(sel) = hint.selector.as_deref().and_then(extract_selector_name) {
        return Some(sel);
    }
    fallback_value.map(String::as_str).and_then(extract_selector_name)
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
    const INTERNAL_SELECTOR_MAP: &[(&str, &str)] = &[
        ("_current", "stdlib:dart.core.Iterator.current [selector]"),
        (
            "_equivalentYear",
            "stdlib:dart.core.DateTime.equivalentYear [selector]",
        ),
        ("_listEquals", "framework:flutter.foundation.listEquals [selector]"),
        (
            "_prependTypeArguments",
            "runtime:dart_vm.prependTypeArguments [selector]",
        ),
        (
            "_StreamController",
            "stdlib:dart.async.StreamController.new [selector]",
        ),
        (
            "_RawDatagramSocket",
            "stdlib:dart.io.RawDatagramSocket.new [selector]",
        ),
        (
            "_nativeSetFloat32x4",
            "stdlib:dart.typed_data.ByteData.setFloat32x4 [selector]",
        ),
        (
            "_UnmodifiableUint8ArrayView",
            "stdlib:dart.typed_data._UnmodifiableUint8ArrayView.new [selector]",
        ),
        (
            "_Int32ArrayView",
            "stdlib:dart.typed_data._Int32ArrayView.new [selector]",
        ),
    ];
    for (selector, tag) in INTERNAL_SELECTOR_MAP {
        if t.eq_ignore_ascii_case(selector) {
            return Some((*tag).to_string());
        }
    }
    None
}
