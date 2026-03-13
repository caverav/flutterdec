struct SyntheticHintInput<'a> {
    decoded_kind: &'a str,
    selector: &'a str,
    target_va: Option<u64>,
    owner_class: &'a str,
    library_uri: &'a str,
    value: &'a str,
    confidence: Option<f64>,
    source: Option<&'a str>,
}

fn normalize_method_selector(name: &str) -> String {
    let tail = name
        .trim()
        .rsplit(['.', ':'])
        .next()
        .unwrap_or(name)
        .trim();
    tail.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .to_ascii_lowercase()
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

fn synthetic_hint_key(
    decoded_kind: &str,
    selector: &str,
    target_va: Option<u64>,
    owner_class: &str,
    library_uri: &str,
    value: &str,
) -> String {
    let target = target_va
        .map(|va| format!("0x{va:x}"))
        .unwrap_or_else(|| "none".to_string());
    format!(
        "{}|{}|{}|{}|{}|{}",
        decoded_kind.to_ascii_lowercase(),
        selector.to_ascii_lowercase(),
        target,
        owner_class.to_ascii_lowercase(),
        library_uri.to_ascii_lowercase(),
        value.to_ascii_lowercase()
    )
}

fn collect_existing_bootflow_hint_keys(
    model: &flutterdec_adapter::ProgramModel,
) -> std::collections::HashSet<String> {
    model.object_pool
        .iter()
        .filter_map(|entry| {
            Some(synthetic_hint_key(
                entry.decoded_kind.as_deref()?,
                entry.selector.as_deref()?,
                entry.target_va,
                entry.owner_class.as_deref().unwrap_or(""),
                entry.library_uri.as_deref().unwrap_or(""),
                &entry.value,
            ))
        })
        .collect()
}

fn push_synthetic_hint(
    model: &mut flutterdec_adapter::ProgramModel,
    seen: &mut std::collections::HashSet<String>,
    hint: &SyntheticHintInput<'_>,
) -> bool {
    let key = synthetic_hint_key(
        hint.decoded_kind,
        hint.selector,
        hint.target_va,
        hint.owner_class,
        hint.library_uri,
        hint.value,
    );
    if seen.contains(&key) {
        return false;
    }
    seen.insert(key);
    let next_index = model.object_pool.len() as u64;
    model.object_pool.push(flutterdec_adapter::ObjectPoolEntry {
        index: next_index,
        kind: "String".to_string(),
        value: hint.value.to_string(),
        decoded_kind: Some(hint.decoded_kind.to_string()),
        selector: Some(hint.selector.to_string()),
        target_va: hint.target_va,
        owner_class: Some(hint.owner_class.to_string()),
        library_uri: Some(hint.library_uri.to_string()),
        confidence: hint.confidence,
        source: hint.source.map(str::to_string),
    });
    true
}
