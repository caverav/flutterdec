pub(super) fn infer_call_intent(call_name: &str) -> Option<String> {
    let lower = call_name.to_ascii_lowercase();
    if is_generic_symbol_placeholder(call_name) {
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

    if let Some(tag) = infer_package_intent(call_name) {
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

pub(super) fn is_generic_symbol_placeholder(name: &str) -> bool {
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

fn infer_package_intent(call_name: &str) -> Option<String> {
    if !call_name.to_ascii_lowercase().starts_with("package_") {
        return None;
    }
    let mut parts = call_name.split('_');
    let head = parts.next().unwrap_or_default();
    if !head.eq_ignore_ascii_case("package") {
        return None;
    }
    let pkg = parts.next()?.trim();
    let owner = parts.next()?.trim();
    let method = parts.collect::<Vec<_>>().join("_");
    if pkg.is_empty() || owner.is_empty() || method.is_empty() {
        return None;
    }
    Some(format!(
        "package:{}.{}.{}",
        pkg.to_ascii_lowercase(),
        owner,
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
    let generic = is_generic_symbol_placeholder(call_name);
    let indirect_alias =
        lower == "dispatchtarget" || lower == "cachedtarget" || lower.starts_with("indirecttarget");
    let known_machine_name = lower.starts_with("dart_")
        || lower.starts_with("flutter_")
        || lower.starts_with("package_")
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
