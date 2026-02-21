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
    let trimmed = intent.trim().trim_end_matches(" [selector]");
    for prefix in ["framework:", "stdlib:", "runtime:", "native:"] {
        if let Some(path) = trimmed.strip_prefix(prefix) {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
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
    let lib_uri = hint.library_uri.as_deref()?.trim();
    let method = semantic_method_from_selector(selector, &owner);
    if method.is_empty() {
        return None;
    }

    if let Some(seg) = dart_library_segment(lib_uri) {
        return Some(format!("stdlib:dart.{}.{}.{}", seg, owner, method));
    }
    if let Some(seg) = flutter_library_segment(lib_uri) {
        return Some(format!("framework:flutter.{}.{}.{}", seg, owner, method));
    }
    None
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
            let Some(sel) = extract_selector_name(v) else {
                continue;
            };
            if let Some(tag) = classify_standard_selector(&sel) {
                return Some(tag);
            }
        }
        for lit in extract_string_literals(arg) {
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

fn classify_standard_selector(raw: &str) -> Option<String> {
    let flutter = [
        ("setstate", "framework:flutter.widgets.State.setState"),
        (
            "createstate",
            "framework:flutter.widgets.StatefulWidget.createState",
        ),
        ("build", "framework:flutter.widgets.Widget.build"),
        ("initstate", "framework:flutter.widgets.State.initState"),
        ("dispose", "framework:flutter.widgets.State.dispose"),
        ("activate", "framework:flutter.widgets.State.activate"),
        ("deactivate", "framework:flutter.widgets.State.deactivate"),
        ("reassemble", "framework:flutter.widgets.State.reassemble"),
        (
            "didupdatewidget",
            "framework:flutter.widgets.State.didUpdateWidget",
        ),
        (
            "didchangedependencies",
            "framework:flutter.widgets.State.didChangeDependencies",
        ),
        (
            "addlistener",
            "framework:flutter.foundation.ChangeNotifier.addListener",
        ),
        (
            "removelistener",
            "framework:flutter.foundation.ChangeNotifier.removeListener",
        ),
        (
            "notifylisteners",
            "framework:flutter.foundation.ChangeNotifier.notifyListeners",
        ),
        (
            "addpostframecallback",
            "framework:flutter.scheduler.SchedulerBinding.addPostFrameCallback",
        ),
        (
            "markneedsbuild",
            "framework:flutter.widgets.Element.markNeedsBuild",
        ),
        (
            "findrenderobject",
            "framework:flutter.widgets.BuildContext.findRenderObject",
        ),
        (
            "createrenderobject",
            "framework:flutter.rendering.RenderObjectWidget.createRenderObject",
        ),
        (
            "updaterenderobject",
            "framework:flutter.rendering.RenderObjectWidget.updateRenderObject",
        ),
        (
            "keyedsubtree",
            "framework:flutter.widgets.KeyedSubtree.new",
        ),
        (
            "parentdatawidget",
            "framework:flutter.widgets.ParentDataWidget.new",
        ),
        (
            "slivergridparentdata",
            "framework:flutter.rendering.SliverGridParentData.new",
        ),
        (
            "didchangeapplifecyclestate",
            "framework:flutter.widgets.WidgetsBindingObserver.didChangeAppLifecycleState",
        ),
        (
            "didchangemetrics",
            "framework:flutter.widgets.WidgetsBindingObserver.didChangeMetrics",
        ),
        (
            "didchangelocales",
            "framework:flutter.widgets.WidgetsBindingObserver.didChangeLocales",
        ),
        (
            "didchangeplatformbrightness",
            "framework:flutter.widgets.WidgetsBindingObserver.didChangePlatformBrightness",
        ),
        (
            "didchangetextscalefactor",
            "framework:flutter.widgets.WidgetsBindingObserver.didChangeTextScaleFactor",
        ),
        (
            "didchangeaccessibilityfeatures",
            "framework:flutter.widgets.WidgetsBindingObserver.didChangeAccessibilityFeatures",
        ),
        (
            "didhavememorypressure",
            "framework:flutter.widgets.WidgetsBindingObserver.didHaveMemoryPressure",
        ),
        (
            "addobserver",
            "framework:flutter.widgets.WidgetsBinding.addObserver",
        ),
        (
            "removeobserver",
            "framework:flutter.widgets.WidgetsBinding.removeObserver",
        ),
        (
            "pushnamedandremoveuntil",
            "framework:flutter.widgets.Navigator.pushNamedAndRemoveUntil",
        ),
        (
            "pushreplacementnamed",
            "framework:flutter.widgets.Navigator.pushReplacementNamed",
        ),
        ("pushnamed", "framework:flutter.widgets.Navigator.pushNamed"),
        ("popuntil", "framework:flutter.widgets.Navigator.popUntil"),
        ("push", "framework:flutter.widgets.Navigator.push"),
        ("pop", "framework:flutter.widgets.Navigator.pop"),
        (
            "showsnackbar",
            "framework:flutter.material.ScaffoldMessengerState.showSnackBar",
        ),
        (
            "hidecurrentsnackbar",
            "framework:flutter.material.ScaffoldMessengerState.hideCurrentSnackBar",
        ),
        (
            "removecurrentsnackbar",
            "framework:flutter.material.ScaffoldMessengerState.removeCurrentSnackBar",
        ),
    ];
    let dart_async = [
        ("then", "stdlib:dart.async.Future.then"),
        ("catcherror", "stdlib:dart.async.Future.catchError"),
        ("whencomplete", "stdlib:dart.async.Future.whenComplete"),
        ("listen", "stdlib:dart.async.Stream.listen"),
        ("streamiterator", "stdlib:dart.async.StreamIterator.new"),
        ("wait", "stdlib:dart.async.Future.wait"),
        ("delayed", "stdlib:dart.async.Future.delayed"),
        ("timeout", "stdlib:dart.async.Future.timeout"),
        ("asstream", "stdlib:dart.async.Future.asStream"),
        ("transform", "stdlib:dart.async.Stream.transform"),
        ("distinct", "stdlib:dart.async.Stream.distinct"),
        ("takewhile", "stdlib:dart.async.Stream.takeWhile"),
        ("skipwhile", "stdlib:dart.async.Stream.skipWhile"),
    ];
    let dart_core = [
        ("print", "stdlib:dart.core.print"),
        ("compiletimeerror", "stdlib:dart.core._CompileTimeError.new"),
        ("tostring", "stdlib:dart.core.toString"),
        ("hashcode", "stdlib:dart.core.hashCode"),
        ("compareto", "stdlib:dart.core.compareTo"),
        ("contains", "stdlib:dart.core.contains"),
        ("containskey", "stdlib:dart.core.Map.containsKey"),
        ("putifabsent", "stdlib:dart.core.Map.putIfAbsent"),
        ("firstwhere", "stdlib:dart.core.Iterable.firstWhere"),
        ("singlewhere", "stdlib:dart.core.Iterable.singleWhere"),
        ("map", "stdlib:dart.core.map"),
        ("where", "stdlib:dart.core.where"),
        ("join", "stdlib:dart.core.String.join"),
        ("split", "stdlib:dart.core.String.split"),
        ("substring", "stdlib:dart.core.String.substring"),
        ("startswith", "stdlib:dart.core.String.startsWith"),
        ("endswith", "stdlib:dart.core.String.endsWith"),
        ("replaceall", "stdlib:dart.core.String.replaceAll"),
        ("tolowercase", "stdlib:dart.core.String.toLowerCase"),
        ("touppercase", "stdlib:dart.core.String.toUpperCase"),
        ("removeat", "stdlib:dart.core.List.removeAt"),
        ("removewhere", "stdlib:dart.core.List.removeWhere"),
        ("addall", "stdlib:dart.core.List.addAll"),
        ("putall", "stdlib:dart.core.Map.addAll"),
        ("tolist", "stdlib:dart.core.Iterable.toList"),
        ("toset", "stdlib:dart.core.Iterable.toSet"),
        ("foreach", "stdlib:dart.core.Iterable.forEach"),
        ("indexof", "stdlib:dart.core.String.indexOf"),
        ("lastindexof", "stdlib:dart.core.String.lastIndexOf"),
        ("trimleft", "stdlib:dart.core.String.trimLeft"),
        ("trimright", "stdlib:dart.core.String.trimRight"),
        ("trim", "stdlib:dart.core.String.trim"),
        ("codeunitat", "stdlib:dart.core.String.codeUnitAt"),
    ];
    let dart_io = [
        ("supportsansiescapes", "stdlib:dart.io.Stdout.supportsAnsiEscapes"),
        ("websocketimpl", "stdlib:dart.io.WebSocketImpl.new"),
        ("nativesocket", "stdlib:dart.io._NativeSocket.new"),
    ];
    let dart_vm_runtime = [
        ("yieldstariterable", "runtime:dart_vm.yieldStarIterable"),
        ("closure", "runtime:dart_vm.Closure.new"),
        ("typeparameter", "runtime:dart_vm.TypeParameter.new"),
    ];
    let dart_typed_data = [
        ("float32x4list", "stdlib:dart.typed_data.Float32x4List.new"),
        ("int64list", "stdlib:dart.typed_data.Int64List.new"),
        ("offsetinbytes", "stdlib:dart.typed_data.TypedData.offsetInBytes"),
        (
            "lengthinbytes",
            "stdlib:dart.typed_data.TypedData.lengthInBytes",
        ),
        (
            "elementsizeinbytes",
            "stdlib:dart.typed_data.TypedData.elementSizeInBytes",
        ),
        ("setfloat32", "stdlib:dart.typed_data.ByteData.setFloat32"),
        ("setfloat64", "stdlib:dart.typed_data.ByteData.setFloat64"),
        ("setint32", "stdlib:dart.typed_data.ByteData.setInt32"),
        ("setuint32", "stdlib:dart.typed_data.ByteData.setUint32"),
        ("getfloat32", "stdlib:dart.typed_data.ByteData.getFloat32"),
        ("getfloat64", "stdlib:dart.typed_data.ByteData.getFloat64"),
        ("getint32", "stdlib:dart.typed_data.ByteData.getInt32"),
        ("getuint32", "stdlib:dart.typed_data.ByteData.getUint32"),
    ];

    for candidate in selector_candidates(raw) {
        for (needle, tag) in flutter {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_async {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_core {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_io {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_vm_runtime {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_typed_data {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
    }

    None
}

fn selector_candidates(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let normalized = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    push_unique(&mut out, normalized);

    let mut token = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            token.push(c.to_ascii_lowercase());
        } else if !token.is_empty() {
            push_unique(&mut out, token.clone());
            token.clear();
        }
    }
    if !token.is_empty() {
        push_unique(&mut out, token);
    }

    let derived = out.clone();
    for t in derived {
        if let Some(rest) = t.strip_prefix("init") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("get") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("set") {
            push_unique(&mut out, format!("set{}", rest));
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("native") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix('_') {
            push_unique(&mut out, rest.to_string());
        }
    }

    out
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !s.is_empty() && !out.contains(&s) {
        out.push(s);
    }
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
