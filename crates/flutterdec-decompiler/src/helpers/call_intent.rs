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
