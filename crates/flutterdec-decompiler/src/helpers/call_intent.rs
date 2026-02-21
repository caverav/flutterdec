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

pub(super) fn infer_call_intent_with_context(
    call_name: &str,
    args: &[String],
    pool_value_hints: &HashMap<u64, String>,
) -> Option<String> {
    if let Some(v) = infer_call_intent(call_name) {
        return Some(v);
    }
    infer_selector_intent_from_pool(args, pool_value_hints)
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
            if let Some(tag) = classify_standard_selector(v) {
                return Some(tag);
            }
        }
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

fn classify_standard_selector(raw: &str) -> Option<String> {
    let normalized = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let flutter = [
        ("setstate", "framework:flutter.widgets.State.setState"),
        (
            "createstate",
            "framework:flutter.widgets.StatefulWidget.createState",
        ),
        ("build", "framework:flutter.widgets.Widget.build"),
        ("initstate", "framework:flutter.widgets.State.initState"),
        ("dispose", "framework:flutter.widgets.State.dispose"),
        (
            "didupdatewidget",
            "framework:flutter.widgets.State.didUpdateWidget",
        ),
        (
            "didchangedependencies",
            "framework:flutter.widgets.State.didChangeDependencies",
        ),
    ];
    for (needle, tag) in flutter {
        if normalized == needle || normalized.contains(needle) {
            return Some(format!("{} [selector]", tag));
        }
    }

    let dart_core = [
        ("print", "stdlib:dart.core.print"),
        ("tostring", "stdlib:dart.core.toString"),
        ("hashcode", "stdlib:dart.core.hashCode"),
        ("compareto", "stdlib:dart.core.compareTo"),
        ("contains", "stdlib:dart.core.contains"),
        ("map", "stdlib:dart.core.map"),
        ("where", "stdlib:dart.core.where"),
    ];
    for (needle, tag) in dart_core {
        if normalized == needle || normalized.contains(needle) {
            return Some(format!("{} [selector]", tag));
        }
    }

    None
}
