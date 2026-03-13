use anyhow::anyhow;
use dalvik::decode;
use dalvik::Instruction;
use dex::jtype::TypeId;
use dex::method::MethodId;
use dex::DexReader;
use flutterdec_loader::{list_apk_entries, read_apk_entry};

const FLUTTER_ACTIVITY_DESC: &str = "Lio/flutter/embedding/android/FlutterActivity;";
const FLUTTER_ACTIVITY_DELEGATE_DESC: &str =
    "Lio/flutter/embedding/android/FlutterActivityAndFragmentDelegate;";
const FLUTTER_ENGINE_DESC: &str = "Lio/flutter/embedding/engine/FlutterEngine;";
const FLUTTER_LOADER_DESC: &str = "Lio/flutter/embedding/engine/loader/FlutterLoader;";
const FLUTTER_JNI_DESC: &str = "Lio/flutter/embedding/engine/FlutterJNI;";
const DART_EXECUTOR_DESC: &str = "Lio/flutter/embedding/engine/dart/DartExecutor;";
const DART_ENTRYPOINT_DESC: &str =
    "Lio/flutter/embedding/engine/dart/DartExecutor$DartEntrypoint;";
const MAX_STARTUP_PARSE_ERRORS: usize = 20;
static DALVIK_DECODE_HOOK_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StartupClassEvidence {
    pub source_dex: String,
    pub class_descriptor: String,
    pub class_name: String,
    pub super_descriptor: Option<String>,
    pub super_class_name: Option<String>,
    pub relation: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StartupMethodEvidence {
    pub source_dex: String,
    pub class_descriptor: String,
    pub class_name: String,
    pub method_name: String,
    pub target_class: String,
    pub target_class_name: String,
    pub target_method: String,
    pub category: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DartEntrypointEvidence {
    pub source_dex: String,
    pub class_descriptor: String,
    pub class_name: String,
    pub method_name: String,
    pub target_method: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JniBootstrapEvidence {
    pub source_dex: String,
    pub class_descriptor: String,
    pub class_name: String,
    pub method_name: String,
    pub target_method: String,
    pub stage: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AndroidStartupEvidence {
    pub present: bool,
    pub confidence: String,
    pub dex_files: Vec<String>,
    pub parse_errors: Vec<String>,
    pub flutter_activity_classes: Vec<StartupClassEvidence>,
    pub startup_methods: Vec<StartupMethodEvidence>,
    pub dart_entrypoints: Vec<DartEntrypointEvidence>,
    pub jni_bootstrap: Vec<JniBootstrapEvidence>,
}

impl Default for AndroidStartupEvidence {
    fn default() -> Self {
        Self {
            present: false,
            confidence: "none".to_string(),
            dex_files: Vec::new(),
            parse_errors: Vec::new(),
            flutter_activity_classes: Vec::new(),
            startup_methods: Vec::new(),
            dart_entrypoints: Vec::new(),
            jni_bootstrap: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct ScannedStartupClass {
    source_dex: String,
    class_descriptor: String,
    class_name: String,
    super_descriptor: Option<String>,
}

#[derive(Debug, Clone)]
struct ScannedStartupMethodRef {
    source_dex: String,
    class_descriptor: String,
    class_name: String,
    method_name: String,
    target_class: String,
    target_class_name: String,
    target_method: String,
}

#[derive(Debug, Clone)]
struct StartupScanResult {
    classes: Vec<ScannedStartupClass>,
    method_refs: Vec<ScannedStartupMethodRef>,
    parse_errors: Vec<String>,
}

fn is_apk_input(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"))
}

fn is_classes_dex_entry(name: &str) -> bool {
    let basename = name.rsplit('/').next().unwrap_or(name);
    if !basename.starts_with("classes") || !basename.ends_with(".dex") {
        return false;
    }
    let suffix = &basename["classes".len()..basename.len() - ".dex".len()];
    suffix.is_empty() || suffix.chars().all(|c| c.is_ascii_digit())
}

fn descriptor_to_java_name(descriptor: &str) -> String {
    if descriptor.starts_with('L') && descriptor.ends_with(';') {
        return descriptor[1..descriptor.len() - 1].replace('/', ".");
    }
    descriptor.to_string()
}

fn is_relevant_startup_class(descriptor: &str) -> bool {
    matches!(
        descriptor,
        FLUTTER_ACTIVITY_DESC
            | FLUTTER_ACTIVITY_DELEGATE_DESC
            | FLUTTER_ENGINE_DESC
            | FLUTTER_LOADER_DESC
            | FLUTTER_JNI_DESC
            | DART_EXECUTOR_DESC
            | DART_ENTRYPOINT_DESC
    )
}

fn classify_startup_method(target_class: &str, target_method: &str) -> (&'static str, &'static str) {
    match (target_class, target_method) {
        (FLUTTER_ACTIVITY_DELEGATE_DESC, "onAttach") => ("delegate_on_attach", "high"),
        (FLUTTER_ENGINE_DESC, "<init>") => ("flutter_engine_ctor", "high"),
        (FLUTTER_LOADER_DESC, "startInitialization") => ("loader_start_initialization", "high"),
        (FLUTTER_LOADER_DESC, "ensureInitializationComplete") => {
            ("loader_ensure_initialization_complete", "high")
        }
        (FLUTTER_JNI_DESC, "loadLibrary") => ("jni_load_library", "high"),
        (FLUTTER_JNI_DESC, "init") => ("jni_init", "high"),
        (FLUTTER_JNI_DESC, "nativeInit") => ("jni_native_init", "high"),
        (FLUTTER_JNI_DESC, "attachToNative") => ("jni_attach_to_native", "high"),
        (FLUTTER_JNI_DESC, "nativeAttach") => ("jni_native_attach", "high"),
        (DART_ENTRYPOINT_DESC, "<init>") => ("dart_entrypoint_ctor", "high"),
        (DART_EXECUTOR_DESC, "executeDartEntrypoint") => ("dart_entrypoint_execute", "high"),
        _ => ("embedding_call", "medium"),
    }
}

fn jni_stage_for(target_class: &str, target_method: &str) -> Option<&'static str> {
    match (target_class, target_method) {
        (FLUTTER_LOADER_DESC, "startInitialization") => Some("loader_start_initialization"),
        (FLUTTER_LOADER_DESC, "ensureInitializationComplete") => {
            Some("loader_ensure_initialization_complete")
        }
        (FLUTTER_JNI_DESC, "loadLibrary") => Some("jni_load_library"),
        (FLUTTER_JNI_DESC, "init") => Some("jni_init"),
        (FLUTTER_JNI_DESC, "nativeInit") => Some("jni_native_init"),
        (FLUTTER_JNI_DESC, "attachToNative") => Some("jni_attach_to_native"),
        (FLUTTER_JNI_DESC, "nativeAttach") => Some("jni_native_attach"),
        _ => None,
    }
}

fn push_parse_error(errors: &mut Vec<String>, message: String) {
    if errors.len() < MAX_STARTUP_PARSE_ERRORS {
        errors.push(message);
    }
}

fn collect_classes_dex_entries(input_path: &Path) -> Result<Vec<String>> {
    let mut entries = list_apk_entries(input_path)?
        .into_iter()
        .filter(|name| is_classes_dex_entry(name))
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn instruction_method_id(instruction: &Instruction) -> Option<u32> {
    match instruction {
        Instruction::InvokeVirtual { method, .. }
        | Instruction::InvokeSuper { method, .. }
        | Instruction::InvokeDirect { method, .. }
        | Instruction::InvokeStatic { method, .. }
        | Instruction::InvokeInterface { method, .. } => Some(u32::from(*method)),
        Instruction::InvokeVirtualRange { method, .. }
        | Instruction::InvokeSuperRange { method, .. }
        | Instruction::InvokeDirectRange { method, .. }
        | Instruction::InvokeStaticRange { method, .. }
        | Instruction::InvokeInterfaceRange { method, .. } => Some(u32::from(*method)),
        _ => None,
    }
}

fn decode_method_refs<B: AsRef<[u8]>>(
    dex: &dex::Dex<B>,
    source_dex: &str,
    class_descriptor: &str,
    class_name: &str,
    method_name: &str,
    insns: &[u16],
    parse_errors: &mut Vec<String>,
) -> Vec<ScannedStartupMethodRef> {
    let mut out = Vec::new();
    let mut remaining = insns;
    while !remaining.is_empty() {
        let decoded = decode_one_silently(&mut remaining);
        match decoded {
            Ok(Ok(instruction)) => {
                let Some(method_id) = instruction_method_id(&instruction) else {
                    continue;
                };
                let method_item = match dex.get_method_item(MethodId::from(u64::from(method_id))) {
                    Ok(item) => item,
                    Err(err) => {
                        push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve method id {}: {}",
                                class_name, method_name, method_id, err
                            ),
                        );
                        continue;
                    }
                };
                let target_class = match dex.get_type(TypeId::from(u32::from(method_item.class_idx()))) {
                    Ok(ty) => ty.type_descriptor().to_string(),
                    Err(err) => {
                        push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve target class for method id {}: {}",
                                class_name, method_name, method_id, err
                            ),
                        );
                        continue;
                    }
                };
                if !is_relevant_startup_class(&target_class) {
                    continue;
                }
                let target_method = match dex.get_string(method_item.name_idx()) {
                    Ok(name) => name.to_string(),
                    Err(err) => {
                        push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve target method for method id {}: {}",
                                class_name, method_name, method_id, err
                            ),
                        );
                        continue;
                    }
                };
                out.push(ScannedStartupMethodRef {
                    source_dex: source_dex.to_string(),
                    class_descriptor: class_descriptor.to_string(),
                    class_name: class_name.to_string(),
                    method_name: method_name.to_string(),
                    target_class_name: descriptor_to_java_name(&target_class),
                    target_class,
                    target_method,
                });
            }
            Ok(Err(decode::Error::Metadata { length })) => {
                if remaining.len() < length {
                    push_parse_error(
                        parse_errors,
                        format!(
                            "{}:{} -> truncated Dalvik metadata payload",
                            class_name, method_name
                        ),
                    );
                    break;
                }
                remaining = &remaining[length..];
            }
            Ok(Err(err)) => {
                push_parse_error(
                    parse_errors,
                    format!("{}:{} -> Dalvik decode error: {:?}", class_name, method_name, err),
                );
                break;
            }
            Err(_) => {
                push_parse_error(
                    parse_errors,
                    format!(
                        "{}:{} -> Dalvik decode panic on inline metadata",
                        class_name, method_name
                    ),
                );
                break;
            }
        }
    }
    out
}

fn decode_one_silently(remaining: &mut &[u16]) -> std::thread::Result<Result<Instruction, decode::Error>> {
    let _hook_guard = DALVIK_DECODE_HOOK_LOCK.lock().ok();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode::decode_one(remaining)
    }));
    std::panic::set_hook(previous_hook);
    decoded
}

fn scan_dex_bytes(source_dex: &str, bytes: Vec<u8>) -> Result<StartupScanResult> {
    let dex = DexReader::from_vec(bytes)
        .map_err(|err| anyhow!("parse {} as dex: {}", source_dex, err))?;
    let mut classes = Vec::new();
    let mut method_refs = Vec::new();
    let mut parse_errors = Vec::new();

    for class in dex.classes() {
        let class = class.map_err(|err| anyhow!("parse class in {}: {}", source_dex, err))?;
        let class_descriptor = class.jtype().type_descriptor().to_string();
        let class_name = class.jtype().to_java_type();
        let super_descriptor = class
            .super_class()
            .map(|id| dex.get_type(id))
            .transpose()
            .map_err(|err| anyhow!("resolve super class for {} in {}: {}", class_name, source_dex, err))?
            .map(|ty| ty.type_descriptor().to_string());
        classes.push(ScannedStartupClass {
            source_dex: source_dex.to_string(),
            class_descriptor: class_descriptor.clone(),
            class_name: class_name.clone(),
            super_descriptor,
        });

        for method in class.methods() {
            let Some(code) = method.code() else {
                continue;
            };
            let refs = decode_method_refs(
                &dex,
                source_dex,
                &class_descriptor,
                &class_name,
                &method.name().to_string(),
                code.insns(),
                &mut parse_errors,
            );
            method_refs.extend(refs);
        }
    }

    Ok(StartupScanResult {
        classes,
        method_refs,
        parse_errors,
    })
}

fn has_super_class(
    class_descriptor: &str,
    expected_super: &str,
    class_supers: &HashMap<String, Option<String>>,
) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut current = class_descriptor;
    while let Some(Some(next)) = class_supers.get(current) {
        if next == expected_super {
            return true;
        }
        if !seen.insert(next.clone()) {
            break;
        }
        current = next;
    }
    false
}

fn finalize_android_startup_evidence(
    dex_files: Vec<String>,
    mut parse_errors: Vec<String>,
    scan_results: Vec<StartupScanResult>,
) -> AndroidStartupEvidence {
    let mut class_supers = HashMap::new();
    let mut scanned_classes = Vec::new();
    let mut scanned_methods = Vec::new();
    for result in scan_results {
        for err in result.parse_errors {
            push_parse_error(&mut parse_errors, err);
        }
        scanned_classes.extend(result.classes);
        scanned_methods.extend(result.method_refs);
    }
    for class in &scanned_classes {
        class_supers.insert(class.class_descriptor.clone(), class.super_descriptor.clone());
    }

    let mut flutter_activity_classes = Vec::new();
    let mut class_seen = std::collections::HashSet::new();
    for class in &scanned_classes {
        if !has_super_class(&class.class_descriptor, FLUTTER_ACTIVITY_DESC, &class_supers) {
            continue;
        }
        let key = format!("{}|{}", class.source_dex, class.class_descriptor);
        if !class_seen.insert(key) {
            continue;
        }
        flutter_activity_classes.push(StartupClassEvidence {
            source_dex: class.source_dex.clone(),
            class_descriptor: class.class_descriptor.clone(),
            class_name: class.class_name.clone(),
            super_class_name: class
                .super_descriptor
                .as_deref()
                .map(descriptor_to_java_name),
            super_descriptor: class.super_descriptor.clone(),
            relation: "subclass".to_string(),
            confidence: "high".to_string(),
        });
    }

    let mut startup_methods = Vec::new();
    let mut startup_seen = std::collections::HashSet::new();
    let mut dart_entrypoints = Vec::new();
    let mut entrypoint_seen = std::collections::HashSet::new();
    let mut jni_bootstrap = Vec::new();
    let mut jni_seen = std::collections::HashSet::new();
    for method in &scanned_methods {
        let (category, confidence) = classify_startup_method(&method.target_class, &method.target_method);
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            method.source_dex,
            method.class_descriptor,
            method.method_name,
            method.target_class,
            method.target_method,
            category
        );
        if startup_seen.insert(key) {
            startup_methods.push(StartupMethodEvidence {
                source_dex: method.source_dex.clone(),
                class_descriptor: method.class_descriptor.clone(),
                class_name: method.class_name.clone(),
                method_name: method.method_name.clone(),
                target_class: method.target_class.clone(),
                target_class_name: method.target_class_name.clone(),
                target_method: method.target_method.clone(),
                category: category.to_string(),
                confidence: confidence.to_string(),
            });
        }

        if matches!(
            (method.target_class.as_str(), method.target_method.as_str()),
            (DART_ENTRYPOINT_DESC, "<init>") | (DART_EXECUTOR_DESC, "executeDartEntrypoint")
        ) {
            let key = format!(
                "{}|{}|{}|{}",
                method.source_dex, method.class_descriptor, method.method_name, method.target_method
            );
            if entrypoint_seen.insert(key) {
                dart_entrypoints.push(DartEntrypointEvidence {
                    source_dex: method.source_dex.clone(),
                    class_descriptor: method.class_descriptor.clone(),
                    class_name: method.class_name.clone(),
                    method_name: method.method_name.clone(),
                    target_method: method.target_method.clone(),
                    confidence: confidence.to_string(),
                });
            }
        }

        if let Some(stage) = jni_stage_for(&method.target_class, &method.target_method) {
            let key = format!(
                "{}|{}|{}|{}",
                method.source_dex, method.class_descriptor, method.method_name, stage
            );
            if jni_seen.insert(key) {
                jni_bootstrap.push(JniBootstrapEvidence {
                    source_dex: method.source_dex.clone(),
                    class_descriptor: method.class_descriptor.clone(),
                    class_name: method.class_name.clone(),
                    method_name: method.method_name.clone(),
                    target_method: method.target_method.clone(),
                    stage: stage.to_string(),
                    confidence: confidence.to_string(),
                });
            }
        }
    }

    flutter_activity_classes.sort_by(|a, b| {
        a.source_dex
            .cmp(&b.source_dex)
            .then(a.class_name.cmp(&b.class_name))
    });
    startup_methods.sort_by(|a, b| {
        a.source_dex
            .cmp(&b.source_dex)
            .then(a.class_name.cmp(&b.class_name))
            .then(a.method_name.cmp(&b.method_name))
            .then(a.category.cmp(&b.category))
            .then(a.target_method.cmp(&b.target_method))
    });
    dart_entrypoints.sort_by(|a, b| {
        a.source_dex
            .cmp(&b.source_dex)
            .then(a.class_name.cmp(&b.class_name))
            .then(a.method_name.cmp(&b.method_name))
            .then(a.target_method.cmp(&b.target_method))
    });
    jni_bootstrap.sort_by(|a, b| {
        a.source_dex
            .cmp(&b.source_dex)
            .then(a.class_name.cmp(&b.class_name))
            .then(a.method_name.cmp(&b.method_name))
            .then(a.stage.cmp(&b.stage))
    });

    let present = !flutter_activity_classes.is_empty()
        || !startup_methods.is_empty()
        || !dart_entrypoints.is_empty()
        || !jni_bootstrap.is_empty();
    let confidence = if !present {
        "none"
    } else if flutter_activity_classes
        .iter()
        .any(|item| item.confidence == "high")
        || startup_methods.iter().any(|item| item.confidence == "high")
        || !dart_entrypoints.is_empty()
        || !jni_bootstrap.is_empty()
    {
        "high"
    } else {
        "medium"
    };

    AndroidStartupEvidence {
        present,
        confidence: confidence.to_string(),
        dex_files,
        parse_errors,
        flutter_activity_classes,
        startup_methods,
        dart_entrypoints,
        jni_bootstrap,
    }
}

fn analyze_android_startup(input_path: &Path) -> AndroidStartupEvidence {
    if !is_apk_input(input_path) {
        return AndroidStartupEvidence::default();
    }

    let dex_files = match collect_classes_dex_entries(input_path) {
        Ok(entries) => entries,
        Err(err) => {
            return AndroidStartupEvidence {
                parse_errors: vec![format!("list classes.dex entries: {}", err)],
                ..AndroidStartupEvidence::default()
            };
        }
    };

    if dex_files.is_empty() {
        return AndroidStartupEvidence {
            dex_files,
            ..AndroidStartupEvidence::default()
        };
    }

    let mut parse_errors = Vec::new();
    let mut results = Vec::new();
    for dex_file in &dex_files {
        match read_apk_entry(input_path, dex_file)
            .and_then(|bytes| scan_dex_bytes(dex_file, bytes))
        {
            Ok(result) => results.push(result),
            Err(err) => push_parse_error(
                &mut parse_errors,
                format!("{}: {}", dex_file, err),
            ),
        }
    }

    finalize_android_startup_evidence(dex_files, parse_errors, results)
}

fn startup_method_names(startup: &AndroidStartupEvidence) -> std::collections::HashSet<String> {
    startup
        .startup_methods
        .iter()
        .map(|method| normalize_method_selector(&method.method_name))
        .filter(|selector| !selector.is_empty())
        .collect()
}

fn startup_has_bootstrap_signal(startup: &AndroidStartupEvidence) -> bool {
    !startup.jni_bootstrap.is_empty()
        || startup.startup_methods.iter().any(|method| {
            matches!(
                method.category.as_str(),
                "delegate_on_attach"
                    | "flutter_engine_ctor"
                    | "loader_start_initialization"
                    | "loader_ensure_initialization_complete"
                    | "jni_load_library"
                    | "jni_init"
                    | "jni_native_init"
                    | "jni_attach_to_native"
                    | "jni_native_attach"
            )
        })
}

fn startup_has_entrypoint_signal(startup: &AndroidStartupEvidence) -> bool {
    !startup.dart_entrypoints.is_empty()
        || startup.startup_methods.iter().any(|method| {
            matches!(
                method.category.as_str(),
                "dart_entrypoint_ctor" | "dart_entrypoint_execute"
            )
        })
}

fn build_startup_class_library_lookup(
    classes: &[flutterdec_adapter::ClassInfo],
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for class in classes {
        out.entry(class.name.clone())
            .or_insert_with(|| class.library_uri.clone());
    }
    out
}

fn startup_hint_confidence(explicit_signal: bool) -> f64 {
    if explicit_signal {
        0.9
    } else {
        0.8
    }
}

fn first_startup_method_selector(
    startup: &AndroidStartupEvidence,
    predicate: impl Fn(&str) -> bool,
) -> Option<String> {
    startup
        .startup_methods
        .iter()
        .map(|method| normalize_method_selector(&method.method_name))
        .find(|selector| predicate(selector))
}

fn first_startup_owner_and_library(startup: &AndroidStartupEvidence) -> (&str, String) {
    if let Some(entrypoint) = startup.dart_entrypoints.first() {
        return (
            entrypoint.class_name.as_str(),
            format!("apk:{}", entrypoint.source_dex),
        );
    }
    if let Some(method) = startup.startup_methods.first() {
        return (method.class_name.as_str(), format!("apk:{}", method.source_dex));
    }
    if let Some(class) = startup.flutter_activity_classes.first() {
        return (class.class_name.as_str(), format!("apk:{}", class.source_dex));
    }
    ("AndroidStartup", "apk:classes.dex".to_string())
}

fn enrich_model_with_apk_startup_bootflow_hints(
    model: &flutterdec_adapter::ProgramModel,
    startup: &AndroidStartupEvidence,
) -> (flutterdec_adapter::ProgramModel, usize) {
    if !startup.present {
        return (model.clone(), 0);
    }

    let observed_method_names = startup_method_names(startup);
    let has_entrypoint_signal = startup_has_entrypoint_signal(startup);
    let has_bootstrap_signal = startup_has_bootstrap_signal(startup);
    let has_deeplink_signal = observed_method_names.iter().any(|name| is_deeplink_selector(name));
    let has_activity_signal = !startup.flutter_activity_classes.is_empty()
        || observed_method_names
            .iter()
            .any(|name| is_activity_handler_selector(name));

    if !has_entrypoint_signal && !has_bootstrap_signal && !has_deeplink_signal && !has_activity_signal {
        return (model.clone(), 0);
    }

    let mut enriched = model.clone();
    let mut inserted = 0usize;
    let mut inserted_main = 0usize;
    let mut inserted_runapp = 0usize;
    let mut inserted_deeplink = 0usize;
    let mut inserted_activity = 0usize;
    let mut inserted_bootstrap = 0usize;
    let class_library = build_startup_class_library_lookup(&enriched.classes);
    let mut seen = collect_existing_bootflow_hint_keys(&enriched);
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

        if has_entrypoint_signal
            && is_main_like_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "StartupMainCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "bootflow:main:apk_startup",
                    confidence: Some(startup_hint_confidence(true)),
                    source: Some("apk_startup"),
                },
            )
        {
            inserted += 1;
            inserted_main += 1;
        }

        if has_entrypoint_signal
            && is_runapp_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "StartupRunAppCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "bootflow:runapp:apk_startup",
                    confidence: Some(startup_hint_confidence(true)),
                    source: Some("apk_startup"),
                },
            )
        {
            inserted += 1;
            inserted_runapp += 1;
        }

        if has_deeplink_signal
            && is_deeplink_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "StartupDeepLinkCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "bootflow:deeplink:apk_startup",
                    confidence: Some(startup_hint_confidence(false)),
                    source: Some("apk_startup"),
                },
            )
        {
            inserted += 1;
            inserted_deeplink += 1;
        }

        if has_activity_signal
            && is_activity_handler_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "StartupActivityCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "bootflow:activity:apk_startup",
                    confidence: Some(startup_hint_confidence(false)),
                    source: Some("apk_startup"),
                },
            )
        {
            inserted += 1;
            inserted_activity += 1;
        }

        if has_bootstrap_signal
            && is_bootstrap_selector(&selector)
            && (owner_is_bootstrap_context(&owner_lower)
                || library_is_bootstrap_context(&library_lower))
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "StartupBootstrapCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "bootflow:init:apk_startup",
                    confidence: Some(startup_hint_confidence(false)),
                    source: Some("apk_startup"),
                },
            )
        {
            inserted += 1;
            inserted_bootstrap += 1;
        }
    }

    let (startup_owner, startup_library_uri) = first_startup_owner_and_library(startup);
    let startup_library_uri = startup_library_uri.as_str();

    if has_entrypoint_signal
        && inserted_main == 0
        && push_synthetic_hint(
            &mut enriched,
            &mut seen,
            &SyntheticHintInput {
                decoded_kind: "StartupMainCandidate",
                selector: "main",
                target_va: None,
                owner_class: startup_owner,
                library_uri: startup_library_uri,
                value: "bootflow:main:apk_startup",
                confidence: Some(startup_hint_confidence(true)),
                source: Some("apk_startup"),
            },
        )
    {
        inserted += 1;
    }

    if has_entrypoint_signal
        && inserted_runapp == 0
        && push_synthetic_hint(
            &mut enriched,
            &mut seen,
            &SyntheticHintInput {
                decoded_kind: "StartupRunAppCandidate",
                selector: "runApp",
                target_va: None,
                owner_class: startup_owner,
                library_uri: startup_library_uri,
                value: "bootflow:runapp:apk_startup",
                confidence: Some(startup_hint_confidence(true)),
                source: Some("apk_startup"),
            },
        )
    {
        inserted += 1;
    }

    if has_deeplink_signal && inserted_deeplink == 0 {
        let selector = first_startup_method_selector(startup, is_deeplink_selector)
            .unwrap_or_else(|| "onNewIntent".to_string());
        if push_synthetic_hint(
            &mut enriched,
            &mut seen,
            &SyntheticHintInput {
                decoded_kind: "StartupDeepLinkCandidate",
                selector: &selector,
                target_va: None,
                owner_class: startup_owner,
                library_uri: startup_library_uri,
                value: "bootflow:deeplink:apk_startup",
                confidence: Some(startup_hint_confidence(false)),
                source: Some("apk_startup"),
            },
        ) {
            inserted += 1;
        }
    }

    if has_activity_signal && inserted_activity == 0 {
        let selector = first_startup_method_selector(startup, is_activity_handler_selector)
            .unwrap_or_else(|| "onCreate".to_string());
        if push_synthetic_hint(
            &mut enriched,
            &mut seen,
            &SyntheticHintInput {
                decoded_kind: "StartupActivityCandidate",
                selector: &selector,
                target_va: None,
                owner_class: startup_owner,
                library_uri: startup_library_uri,
                value: "bootflow:activity:apk_startup",
                confidence: Some(startup_hint_confidence(false)),
                source: Some("apk_startup"),
            },
        ) {
            inserted += 1;
        }
    }

    if has_bootstrap_signal && inserted_bootstrap == 0 {
        let selector = startup
            .jni_bootstrap
            .first()
            .map(|item| item.target_method.clone())
            .or_else(|| {
                startup
                    .startup_methods
                    .iter()
                    .find(|method| method.category.contains("initialization") || method.category.contains("jni"))
                    .map(|method| method.target_method.clone())
            })
            .unwrap_or_else(|| "attachToNative".to_string());
        if push_synthetic_hint(
            &mut enriched,
            &mut seen,
            &SyntheticHintInput {
                decoded_kind: "StartupBootstrapCandidate",
                selector: &selector,
                target_va: None,
                owner_class: startup_owner,
                library_uri: startup_library_uri,
                value: "bootflow:init:apk_startup",
                confidence: Some(startup_hint_confidence(false)),
                source: Some("apk_startup"),
            },
        ) {
            inserted += 1;
        }
    }

    (enriched, inserted)
}

#[cfg(test)]
mod apk_startup_tests {
    use super::{
        analyze_android_startup, classify_startup_method, enrich_model_with_apk_startup_bootflow_hints,
        finalize_android_startup_evidence, has_super_class, is_classes_dex_entry,
        AndroidStartupEvidence, ScannedStartupClass, ScannedStartupMethodRef, StartupScanResult,
        DART_ENTRYPOINT_DESC, DART_EXECUTOR_DESC, FLUTTER_ACTIVITY_DESC, FLUTTER_ENGINE_DESC,
        FLUTTER_JNI_DESC, FLUTTER_LOADER_DESC,
    };
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn matches_classes_dex_entry_names() {
        assert!(is_classes_dex_entry("classes.dex"));
        assert!(is_classes_dex_entry("classes2.dex"));
        assert!(is_classes_dex_entry("nested/classes10.dex"));
        assert!(!is_classes_dex_entry("class.dex"));
        assert!(!is_classes_dex_entry("classesx.dex"));
        assert!(!is_classes_dex_entry("classes.jar"));
    }

    #[test]
    fn resolves_transitive_flutter_activity_inheritance() {
        let mut supers = HashMap::new();
        supers.insert(
            "Lcom/example/MainActivity;".to_string(),
            Some("Lcom/example/BaseActivity;".to_string()),
        );
        supers.insert(
            "Lcom/example/BaseActivity;".to_string(),
            Some(FLUTTER_ACTIVITY_DESC.to_string()),
        );
        supers.insert(FLUTTER_ACTIVITY_DESC.to_string(), Some("Landroid/app/Activity;".to_string()));
        assert!(has_super_class(
            "Lcom/example/MainActivity;",
            FLUTTER_ACTIVITY_DESC,
            &supers
        ));
    }

    #[test]
    fn classifies_known_startup_methods() {
        assert_eq!(
            classify_startup_method(FLUTTER_LOADER_DESC, "startInitialization"),
            ("loader_start_initialization", "high")
        );
        assert_eq!(
            classify_startup_method(DART_EXECUTOR_DESC, "executeDartEntrypoint"),
            ("dart_entrypoint_execute", "high")
        );
        assert_eq!(
            classify_startup_method(FLUTTER_JNI_DESC, "attachToNative"),
            ("jni_attach_to_native", "high")
        );
        assert_eq!(
            classify_startup_method("Lio/flutter/embedding/engine/FlutterEngine;", "spawn"),
            ("embedding_call", "medium")
        );
    }

    #[test]
    fn finalizes_android_startup_evidence_with_entrypoints_and_jni() {
        let scan = StartupScanResult {
            classes: vec![ScannedStartupClass {
                source_dex: "classes.dex".to_string(),
                class_descriptor: "Lcom/example/MainActivity;".to_string(),
                class_name: "com.example.MainActivity".to_string(),
                super_descriptor: Some(FLUTTER_ACTIVITY_DESC.to_string()),
            }],
            method_refs: vec![
                ScannedStartupMethodRef {
                    source_dex: "classes.dex".to_string(),
                    class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    class_name: "com.example.MainActivity".to_string(),
                    method_name: "onCreate".to_string(),
                    target_class: FLUTTER_LOADER_DESC.to_string(),
                    target_class_name: "io.flutter.embedding.engine.loader.FlutterLoader"
                        .to_string(),
                    target_method: "startInitialization".to_string(),
                },
                ScannedStartupMethodRef {
                    source_dex: "classes.dex".to_string(),
                    class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    class_name: "com.example.MainActivity".to_string(),
                    method_name: "onCreate".to_string(),
                    target_class: DART_ENTRYPOINT_DESC.to_string(),
                    target_class_name:
                        "io.flutter.embedding.engine.dart.DartExecutor$DartEntrypoint"
                            .to_string(),
                    target_method: "<init>".to_string(),
                },
            ],
            parse_errors: Vec::new(),
        };

        let evidence = finalize_android_startup_evidence(
            vec!["classes.dex".to_string()],
            Vec::new(),
            vec![scan],
        );

        assert!(evidence.present);
        assert_eq!(evidence.confidence, "high");
        assert_eq!(evidence.flutter_activity_classes.len(), 1);
        assert_eq!(evidence.dart_entrypoints.len(), 1);
        assert_eq!(evidence.jni_bootstrap.len(), 1);
        assert_eq!(evidence.startup_methods.len(), 2);
    }

    #[test]
    fn invalid_dex_entry_becomes_parse_error_without_panicking() {
        let dir = tempdir().expect("tempdir");
        let apk_path = dir.path().join("invalid.apk");
        let file = File::create(&apk_path).expect("create apk");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("classes.dex", options).expect("classes.dex");
        zip.write_all(b"not-a-dex").expect("write classes.dex");
        zip.finish().expect("finish apk");

        let evidence = analyze_android_startup(&apk_path);
        assert!(!evidence.present);
        assert_eq!(evidence.dex_files, vec!["classes.dex"]);
        assert!(!evidence.parse_errors.is_empty());
    }

    #[test]
    fn non_apk_input_returns_empty_startup_evidence() {
        let evidence = analyze_android_startup(Path::new("/tmp/libapp.so"));
        assert_eq!(evidence, AndroidStartupEvidence::default());
    }

    #[test]
    fn enriches_model_with_apk_startup_synthetic_bootflow_hints() {
        let model = flutterdec_adapter::ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![flutterdec_adapter::LibraryInfo {
                id: 1,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![
                flutterdec_adapter::ClassInfo {
                    id: 1,
                    name: "Global".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 2,
                    name: "WidgetHost".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
            ],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 1,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                    name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 2,
                    name: "runApp".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1004,
                    size: 4,
                    code_section_va: 0x1000,
                    name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 3,
                    name: "onNewIntent".to_string(),
                    owner_class: "WidgetHost".to_string(),
                    entry_va: 0x1008,
                    size: 4,
                    code_section_va: 0x1000,
                    name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 4,
                    name: "ensureInitialized".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x100c,
                    size: 4,
                    code_section_va: 0x1000,
                    name_kind: None,
                },
            ],
            object_pool: Vec::new(),
        };
        let startup = AndroidStartupEvidence {
            present: true,
            confidence: "high".to_string(),
            dex_files: vec!["classes.dex".to_string()],
            parse_errors: Vec::new(),
            flutter_activity_classes: vec![super::StartupClassEvidence {
                source_dex: "classes.dex".to_string(),
                class_descriptor: "Lcom/example/MainActivity;".to_string(),
                class_name: "com.example.MainActivity".to_string(),
                super_descriptor: Some(FLUTTER_ACTIVITY_DESC.to_string()),
                super_class_name: Some("io.flutter.embedding.android.FlutterActivity".to_string()),
                relation: "subclass".to_string(),
                confidence: "high".to_string(),
            }],
            startup_methods: vec![
                super::StartupMethodEvidence {
                    source_dex: "classes.dex".to_string(),
                    class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    class_name: "com.example.MainActivity".to_string(),
                    method_name: "onNewIntent".to_string(),
                    target_class: FLUTTER_ENGINE_DESC.to_string(),
                    target_class_name: "io.flutter.embedding.engine.FlutterEngine".to_string(),
                    target_method: "<init>".to_string(),
                    category: "flutter_engine_ctor".to_string(),
                    confidence: "high".to_string(),
                },
                super::StartupMethodEvidence {
                    source_dex: "classes.dex".to_string(),
                    class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    class_name: "com.example.MainActivity".to_string(),
                    method_name: "configureFlutterEngine".to_string(),
                    target_class: DART_EXECUTOR_DESC.to_string(),
                    target_class_name: "io.flutter.embedding.engine.dart.DartExecutor".to_string(),
                    target_method: "executeDartEntrypoint".to_string(),
                    category: "dart_entrypoint_execute".to_string(),
                    confidence: "high".to_string(),
                },
            ],
            dart_entrypoints: vec![super::DartEntrypointEvidence {
                source_dex: "classes.dex".to_string(),
                class_descriptor: "Lcom/example/MainActivity;".to_string(),
                class_name: "com.example.MainActivity".to_string(),
                method_name: "configureFlutterEngine".to_string(),
                target_method: "executeDartEntrypoint".to_string(),
                confidence: "high".to_string(),
            }],
            jni_bootstrap: vec![super::JniBootstrapEvidence {
                source_dex: "classes.dex".to_string(),
                class_descriptor: "Lcom/example/MainActivity;".to_string(),
                class_name: "com.example.MainActivity".to_string(),
                method_name: "onCreate".to_string(),
                target_method: "attachToNative".to_string(),
                stage: "jni_attach_to_native".to_string(),
                confidence: "high".to_string(),
            }],
        };

        let (enriched, inserted) = enrich_model_with_apk_startup_bootflow_hints(&model, &startup);
        assert!(inserted >= 5);

        let kinds = enriched
            .object_pool
            .iter()
            .filter_map(|entry| entry.decoded_kind.as_deref())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"StartupMainCandidate"));
        assert!(kinds.contains(&"StartupRunAppCandidate"));
        assert!(kinds.contains(&"StartupDeepLinkCandidate"));
        assert!(kinds.contains(&"StartupActivityCandidate"));
        assert!(kinds.contains(&"StartupBootstrapCandidate"));
        assert!(enriched
            .object_pool
            .iter()
            .all(|entry| entry.source.as_deref() == Some("apk_startup")));
    }
}
