use flutterdec_adapter::{ObjectPoolEntry, ProgramModel};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

#[derive(Debug, Clone, Default)]
pub(super) struct AndroidManifestSignals {
    pub(super) package_name: Option<String>,
    pub(super) has_main_launcher: bool,
    pub(super) has_view_browsable: bool,
    pub(super) activities: Vec<String>,
    pub(super) deeplink_entries: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct AndroidManifestInspection {
    pub(super) present: bool,
    pub(super) parse_error: Option<String>,
    pub(super) signals: AndroidManifestSignals,
}

fn normalize_method_selector(name: &str) -> String {
    let tail = name
        .trim()
        .rsplit(['.', ':'])
        .next()
        .unwrap_or(name)
        .trim();
    let cleaned = tail
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .to_ascii_lowercase();
    cleaned
}

fn is_main_like_selector(selector: &str) -> bool {
    selector == "main"
        || selector.ends_with(".main")
        || selector.ends_with("::main")
        || selector.ends_with("_main")
}

fn is_runapp_selector(selector: &str) -> bool {
    selector == "runapp" || selector.ends_with(".runapp")
}

fn is_deeplink_selector(selector: &str) -> bool {
    matches!(
        selector,
        "didpushrouteinformation"
            | "didpushroute"
            | "didpoproute"
            | "setnewroutepath"
            | "parserouteinformation"
            | "ongenerateroute"
            | "onunknownroute"
            | "onnewintent"
            | "handleintent"
    )
}

fn is_activity_handler_selector(selector: &str) -> bool {
    matches!(
        selector,
        "onnewintent"
            | "handleintent"
            | "oncreate"
            | "onstart"
            | "onresume"
            | "onpause"
            | "onstop"
            | "onactivityresult"
    )
}

fn is_bootstrap_selector(selector: &str) -> bool {
    matches!(
        selector,
        "ensureinitialized"
            | "nativeensureinitialized"
            | "startinitialization"
            | "ensureinitializationcomplete"
    )
}

fn owner_is_bootstrap_context(owner_lower: &str) -> bool {
    owner_lower == "global"
        || owner_lower.contains("binding")
        || owner_lower.contains("bootstrap")
        || owner_lower.contains("engine")
        || owner_lower.contains("jni")
}

fn library_is_bootstrap_context(library_lower: &str) -> bool {
    library_lower.starts_with("package:flutter/")
        || library_lower.ends_with("/main.dart")
        || library_lower.contains("/bootstrap")
        || library_lower.contains("/engine")
}

fn normalize_activity_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let tail = trimmed.rsplit('.').next().unwrap_or(trimmed).trim();
    if tail.is_empty() {
        return None;
    }
    Some(tail.to_ascii_lowercase())
}

fn class_matches_manifest_activity(owner_class: &str, activities: &HashSet<String>) -> bool {
    let owner = owner_class.trim();
    if owner.is_empty() {
        return false;
    }
    let tail = owner.rsplit(['.', '$']).next().unwrap_or(owner).to_ascii_lowercase();
    activities.contains(&tail)
}

fn collect_ascii_strings(bytes: &[u8], min_len: usize, max_items: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for b in bytes {
        if (0x20..=0x7e).contains(b) {
            cur.push(*b);
            continue;
        }
        if cur.len() >= min_len {
            out.push(String::from_utf8_lossy(&cur).to_string());
            if out.len() >= max_items {
                return out;
            }
        }
        cur.clear();
    }
    if cur.len() >= min_len && out.len() < max_items {
        out.push(String::from_utf8_lossy(&cur).to_string());
    }
    out
}

fn collect_utf16le_ascii_like_strings(bytes: &[u8], min_len: usize, max_items: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let mut j = i;
        let mut cur = Vec::new();
        while j + 1 < bytes.len() {
            let lo = bytes[j];
            let hi = bytes[j + 1];
            if hi != 0 || !(0x20..=0x7e).contains(&lo) {
                break;
            }
            cur.push(lo);
            j += 2;
        }
        if cur.len() >= min_len {
            out.push(String::from_utf8_lossy(&cur).to_string());
            if out.len() >= max_items {
                return out;
            }
            i = j + 2;
        } else {
            i += 1;
        }
    }
    out
}

fn collect_manifest_strings(bytes: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for s in collect_ascii_strings(bytes, 4, 30_000) {
        let t = s.trim();
        if !t.is_empty() {
            out.insert(t.to_string());
        }
    }
    for s in collect_utf16le_ascii_like_strings(bytes, 4, 30_000) {
        let t = s.trim();
        if !t.is_empty() {
            out.insert(t.to_string());
        }
    }
    out
}

fn sanitize_manifest_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start = trimmed
        .find(|c: char| c.is_ascii_alphabetic() || c == '.')
        .unwrap_or(trimmed.len());
    if start >= trimmed.len() {
        return None;
    }
    let candidate = &trimmed[start..];
    let candidate = candidate.trim_matches(|c: char| {
        !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '$' && c != ':'
    });
    if candidate.len() < 3 || candidate.contains(' ') {
        return None;
    }
    let mut cleaned = candidate.to_string();
    if cleaned.len() > 4 {
        let tail = cleaned.get(1..).unwrap_or_default();
        let noisy_prefixed = tail.starts_with("com.")
            || tail.starts_with("org.")
            || tail.starts_with("io.")
            || tail.starts_with("androidx.")
            || tail.starts_with("net.")
            || tail.starts_with("app.")
            || tail.starts_with("dev.")
            || tail.starts_with("me.")
            || tail.starts_with("oss.");
        if noisy_prefixed {
            cleaned = tail.to_string();
        }
    }
    Some(cleaned)
}

fn infer_package_name(strings: &BTreeSet<String>, activities: &[String]) -> Option<String> {
    for activity in activities {
        let t = activity.trim();
        if t.starts_with('.') {
            continue;
        }
        if !t.ends_with("MainActivity") {
            continue;
        }
        let parts = t.split('.').collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let package = parts[..parts.len().saturating_sub(1)].join(".");
        let package_lower = package.to_ascii_lowercase();
        if package_lower.starts_with("android.")
            || package_lower.starts_with("io.flutter.")
            || package_lower.starts_with("androidx.")
        {
            continue;
        }
        return Some(package);
    }

    strings.iter().find_map(|s| {
        let token = sanitize_manifest_token(s)?;
        let is_lower_package = token
            .split('.')
            .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        if !is_lower_package {
            return None;
        }
        if token.split('.').count() < 3 {
            return None;
        }
        if token.split('.').count() > 4 {
            return None;
        }
        let looks_pkg = token.contains('.') && !token.contains('/') && !token.contains(' ');
        if !looks_pkg {
            return None;
        }
        let lower = token.to_ascii_lowercase();
        if lower.starts_with("android.")
            || lower.starts_with("kotlin.")
            || lower.starts_with("java.")
            || lower.starts_with("com.google.")
            || lower.starts_with("io.flutter.")
            || lower.starts_with("android.intent.")
            || lower.contains("intent.action")
            || lower.contains("intent.category")
        {
            return None;
        }
        Some(token)
    })
}

fn infer_activity_names(strings: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in strings {
        let Some(t) = sanitize_manifest_token(s) else {
            continue;
        };
        if t.is_empty() || t.contains('/') || t.contains(' ') {
            continue;
        }
        if !t.contains("Activity") {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("android.") || lower.starts_with("io.flutter.") {
            continue;
        }
        out.push(t);
    }
    out.sort();
    out.dedup();
    out.truncate(30);
    out
}

fn infer_deeplink_entries(strings: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in strings {
        let Some(t) = sanitize_manifest_token(s) else {
            continue;
        };
        if t.is_empty() || t.len() > 180 {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.contains("schemas.android.com/apk/res/android") {
            continue;
        }
        if lower.contains("://") || lower.starts_with("android.intent.action.view") {
            out.push(t);
        }
    }
    out.sort();
    out.dedup();
    out.truncate(30);
    out
}

fn looks_non_app_package(package: &str) -> bool {
    let lower = package.to_ascii_lowercase();
    lower.starts_with("android.")
        || lower.starts_with("androidx.")
        || lower.starts_with("com.google.")
        || lower.starts_with("io.flutter.")
        || lower.starts_with("com.pichillilorenzo.")
        || lower.starts_with("com.ryanheise.")
}

fn analyze_manifest_bytes(bytes: &[u8]) -> AndroidManifestSignals {
    let strings = collect_manifest_strings(bytes);
    let lower = strings
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let has_main_launcher = lower.contains("android.intent.action.main")
        && lower.contains("android.intent.category.launcher");
    let deeplink_entries = infer_deeplink_entries(&strings);
    let has_custom_deeplink = deeplink_entries
        .iter()
        .any(|v| v.contains("://") && !v.to_ascii_lowercase().starts_with("http"));
    let has_view_browsable = lower.contains("android.intent.action.view")
        && (lower.contains("android.intent.category.browsable") || has_custom_deeplink);
    let activities = infer_activity_names(&strings);
    let package_name = infer_package_name(&strings, &activities)
        .filter(|pkg| !looks_non_app_package(pkg));

    AndroidManifestSignals {
        package_name,
        has_main_launcher,
        has_view_browsable,
        activities,
        deeplink_entries,
    }
}

fn read_android_manifest_from_apk(input_path: &Path) -> Result<Option<Vec<u8>>, String> {
    let f = fs::File::open(input_path).map_err(|e| format!("open apk: {e}"))?;
    let mut zip = ZipArchive::new(f).map_err(|e| format!("parse apk zip: {e}"))?;
    for path in ["AndroidManifest.xml", "base/AndroidManifest.xml"] {
        if let Ok(mut entry) = zip.by_name(path) {
            let mut out = Vec::new();
            entry
                .read_to_end(&mut out)
                .map_err(|e| format!("read manifest entry {path}: {e}"))?;
            return Ok(Some(out));
        }
    }
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else {
            continue;
        };
        if !entry.name().ends_with("/AndroidManifest.xml") {
            continue;
        }
        let mut out = Vec::new();
        entry
            .read_to_end(&mut out)
            .map_err(|e| format!("read manifest entry by index: {e}"))?;
        return Ok(Some(out));
    }
    Ok(None)
}

pub(super) fn inspect_android_manifest(input_path: &Path) -> AndroidManifestInspection {
    let is_apk = input_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"));
    if !is_apk {
        return AndroidManifestInspection::default();
    }
    match read_android_manifest_from_apk(input_path) {
        Ok(Some(bytes)) => AndroidManifestInspection {
            present: true,
            parse_error: None,
            signals: analyze_manifest_bytes(&bytes),
        },
        Ok(None) => AndroidManifestInspection {
            present: false,
            parse_error: None,
            signals: AndroidManifestSignals::default(),
        },
        Err(err) => AndroidManifestInspection {
            present: false,
            parse_error: Some(err),
            signals: AndroidManifestSignals::default(),
        },
    }
}

fn push_synthetic_hint(
    model: &mut ProgramModel,
    seen: &mut HashSet<String>,
    decoded_kind: &str,
    selector: &str,
    target_va: u64,
    owner_class: &str,
    library_uri: &str,
    value: &str,
) -> bool {
    let key = format!(
        "{}|{}|0x{:x}",
        decoded_kind.to_ascii_lowercase(),
        selector.to_ascii_lowercase(),
        target_va
    );
    if seen.contains(&key) {
        return false;
    }
    seen.insert(key);
    let next_index = model.object_pool.len() as u64;
    model.object_pool.push(ObjectPoolEntry {
        index: next_index,
        kind: "String".to_string(),
        value: value.to_string(),
        decoded_kind: Some(decoded_kind.to_string()),
        selector: Some(selector.to_string()),
        target_va: Some(target_va),
        owner_class: Some(owner_class.to_string()),
        library_uri: Some(library_uri.to_string()),
    });
    true
}

pub(super) fn enrich_model_with_manifest_bootflow_hints(
    model: &ProgramModel,
    signals: &AndroidManifestSignals,
) -> (ProgramModel, usize) {
    let mut enriched = model.clone();
    let mut inserted = 0usize;
    let mut class_library = HashMap::new();
    for class in &enriched.classes {
        class_library
            .entry(class.name.clone())
            .or_insert_with(|| class.library_uri.clone());
    }

    let mut seen = enriched
        .object_pool
        .iter()
        .filter_map(|entry| {
            Some(format!(
                "{}|{}|0x{:x}",
                entry.decoded_kind.as_deref()?.to_ascii_lowercase(),
                entry.selector.as_deref()?.to_ascii_lowercase(),
                entry.target_va?
            ))
        })
        .collect::<HashSet<_>>();

    let activity_set = signals
        .activities
        .iter()
        .filter_map(|name| normalize_activity_name(name))
        .collect::<HashSet<_>>();
    let has_deeplink_signal = signals.has_view_browsable || !signals.deeplink_entries.is_empty();

    let functions = enriched.functions.clone();
    for function in functions {
        let selector = normalize_method_selector(&function.name);
        if selector.is_empty() {
            continue;
        }
        let owner = function.owner_class.trim();
        let owner_lower = owner.to_ascii_lowercase();
        let library_uri = class_library
            .get(&function.owner_class)
            .cloned()
            .unwrap_or_default();
        let library_lower = library_uri.to_ascii_lowercase();

        if signals.has_main_launcher && is_main_like_selector(&selector) {
            if push_synthetic_hint(
                &mut enriched,
                &mut seen,
                "ManifestMainCandidate",
                &selector,
                function.entry_va,
                owner,
                &library_uri,
                "manifest:main-launcher",
            ) {
                inserted += 1;
            }
        }
        if signals.has_main_launcher && is_runapp_selector(&selector) {
            if push_synthetic_hint(
                &mut enriched,
                &mut seen,
                "ManifestRunAppCandidate",
                &selector,
                function.entry_va,
                owner,
                &library_uri,
                "manifest:runapp",
            ) {
                inserted += 1;
            }
        }
        if has_deeplink_signal && is_deeplink_selector(&selector) {
            if push_synthetic_hint(
                &mut enriched,
                &mut seen,
                "ManifestDeepLinkCandidate",
                &selector,
                function.entry_va,
                owner,
                &library_uri,
                "manifest:deeplink",
            ) {
                inserted += 1;
            }
        }
        if has_deeplink_signal
            && class_matches_manifest_activity(owner, &activity_set)
            && is_activity_handler_selector(&selector)
        {
            if push_synthetic_hint(
                &mut enriched,
                &mut seen,
                "ManifestActivityCandidate",
                &selector,
                function.entry_va,
                owner,
                &library_uri,
                "manifest:activity",
            ) {
                inserted += 1;
            }
        }
        if signals.has_main_launcher
            && is_bootstrap_selector(&selector)
            && (owner_is_bootstrap_context(&owner_lower)
                || library_is_bootstrap_context(&library_lower))
        {
            if push_synthetic_hint(
                &mut enriched,
                &mut seen,
                "ManifestBootstrapCandidate",
                &selector,
                function.entry_va,
                owner,
                &library_uri,
                "manifest:bootstrap",
            ) {
                inserted += 1;
            }
        }
    }

    (enriched, inserted)
}
