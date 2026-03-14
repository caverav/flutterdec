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
    pub function_name: Option<String>,
    pub library_uri: Option<String>,
    pub initial_route: Option<String>,
    pub app_bundle_path: Option<String>,
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
pub struct BootstrapChainSource {
    pub source_dex: String,
    pub class_descriptor: String,
    pub class_name: String,
    pub method_name: String,
    pub owner_kind: String,
    pub stages: Vec<String>,
    pub complete: bool,
    pub missing_steps: Vec<String>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BootstrapChainPathStep {
    pub class_descriptor: String,
    pub class_name: String,
    pub method_name: String,
    pub owner_kind: String,
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BootstrapChainPath {
    pub source_dex: String,
    pub entry_class_descriptor: String,
    pub entry_class_name: String,
    pub entry_method_name: String,
    pub owner_kind: String,
    pub call_chain: Vec<BootstrapChainPathStep>,
    pub stages: Vec<String>,
    pub complete: bool,
    pub missing_steps: Vec<String>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BootstrapChainEvidence {
    pub complete: bool,
    pub missing_steps: Vec<String>,
    pub sources: Vec<BootstrapChainSource>,
    pub paths: Vec<BootstrapChainPath>,
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
    pub bootstrap_chain: BootstrapChainEvidence,
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
            bootstrap_chain: BootstrapChainEvidence {
                complete: false,
                missing_steps: Vec::new(),
                sources: Vec::new(),
                paths: Vec::new(),
            },
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
    method_defs: Vec<ScannedMethodDef>,
    method_refs: Vec<ScannedStartupMethodRef>,
    app_method_invokes: Vec<ScannedAppMethodInvoke>,
    dart_entrypoints: Vec<ScannedDartEntrypoint>,
    parse_errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct TrackedDartEntrypointValue {
    function_name: Option<String>,
    library_uri: Option<String>,
    app_bundle_path: Option<String>,
}

#[derive(Debug, Clone)]
enum TrackedRegisterValue {
    StringLiteral(String),
    DartEntrypoint(TrackedDartEntrypointValue),
}

#[derive(Debug, Clone)]
struct ScannedDartEntrypoint {
    source_dex: String,
    class_descriptor: String,
    class_name: String,
    method_name: String,
    target_method: String,
    function_name: Option<String>,
    library_uri: Option<String>,
    app_bundle_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScannedMethodKey {
    source_dex: String,
    class_descriptor: String,
    class_name: String,
    method_name: String,
}

impl ScannedMethodKey {
    fn new(source_dex: &str, class_descriptor: &str, class_name: &str, method_name: &str) -> Self {
        Self {
            source_dex: source_dex.to_string(),
            class_descriptor: class_descriptor.to_string(),
            class_name: class_name.to_string(),
            method_name: method_name.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct ScannedMethodDef {
    key: ScannedMethodKey,
}

#[derive(Debug, Clone)]
struct ScannedAppMethodInvoke {
    source: ScannedMethodKey,
    target_class: String,
    target_method: String,
}

#[derive(Debug, Clone)]
struct BootstrapStageCall {
    target_class: String,
    target_class_name: String,
    target_method: String,
    stage: &'static str,
}

#[derive(Debug, Clone, Default)]
struct BootstrapChainSourceBuilder {
    source_dex: String,
    class_descriptor: String,
    class_name: String,
    method_name: String,
    owner_kind: String,
    stages: Vec<String>,
    seen: std::collections::HashSet<&'static str>,
}

impl BootstrapChainSourceBuilder {
    fn push_stage(&mut self, stage: &'static str) {
        if self.seen.insert(stage) {
            self.stages.push(stage.to_string());
        }
    }
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
        (FLUTTER_JNI_DESC, "runBundleAndSnapshotFromLibrary") => ("dart_entrypoint_execute", "high"),
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

const BOOTSTRAP_CHAIN_STAGE_ORDER: [&str; 7] = [
    "activity_on_create",
    "delegate_on_attach",
    "flutter_engine_ctor",
    "loader_start_initialization",
    "loader_ensure_initialization_complete",
    "jni_attach",
    "dart_entrypoint_execute",
];

fn bootstrap_chain_stage_for_method(target_class: &str, target_method: &str) -> Option<&'static str> {
    match (target_class, target_method) {
        (FLUTTER_ACTIVITY_DELEGATE_DESC, "onAttach") => Some("delegate_on_attach"),
        (FLUTTER_ENGINE_DESC, "<init>") => Some("flutter_engine_ctor"),
        (FLUTTER_LOADER_DESC, "startInitialization") => Some("loader_start_initialization"),
        (FLUTTER_LOADER_DESC, "ensureInitializationComplete") => {
            Some("loader_ensure_initialization_complete")
        }
        (FLUTTER_JNI_DESC, "attachToNative") | (FLUTTER_JNI_DESC, "nativeAttach") => {
            Some("jni_attach")
        }
        (FLUTTER_JNI_DESC, "runBundleAndSnapshotFromLibrary")
        | (DART_EXECUTOR_DESC, "executeDartEntrypoint") => Some("dart_entrypoint_execute"),
        _ => None,
    }
}

fn startup_owner_kind(class_descriptor: &str) -> &'static str {
    if class_descriptor.starts_with("Lio/flutter/") {
        "framework"
    } else {
        "app"
    }
}

fn bootstrap_chain_source_confidence(stages: &std::collections::HashSet<&'static str>) -> &'static str {
    if stages.contains("dart_entrypoint_execute")
        || stages.contains("jni_attach")
        || stages.len() >= 4
    {
        "high"
    } else if stages.len() >= 2 {
        "medium"
    } else {
        "low"
    }
}

fn ordered_bootstrap_stages(
    stages: &std::collections::HashSet<&'static str>,
) -> Vec<String> {
    BOOTSTRAP_CHAIN_STAGE_ORDER
        .iter()
        .filter(|stage| stages.contains(**stage))
        .map(|stage| (*stage).to_string())
        .collect()
}

fn bootstrap_missing_steps(
    stages: &std::collections::HashSet<&'static str>,
) -> Vec<String> {
    BOOTSTRAP_CHAIN_STAGE_ORDER
        .iter()
        .filter(|stage| !stages.contains(**stage))
        .map(|stage| (*stage).to_string())
        .collect()
}

fn stage_sort_key(stage: &str) -> usize {
    BOOTSTRAP_CHAIN_STAGE_ORDER
        .iter()
        .position(|candidate| *candidate == stage)
        .unwrap_or(BOOTSTRAP_CHAIN_STAGE_ORDER.len())
}

fn bootstrap_chain_path_confidence(
    stages: &std::collections::HashSet<&'static str>,
    method_chain_len: usize,
) -> &'static str {
    if stages.contains("dart_entrypoint_execute")
        || (stages.contains("jni_attach") && method_chain_len >= 2)
        || stages.len() >= 4
    {
        "high"
    } else if method_chain_len >= 2 || stages.len() >= 2 {
        "medium"
    } else {
        "low"
    }
}

fn is_startup_entry_selector(method_name: &str) -> bool {
    matches!(
        method_name.to_ascii_lowercase().as_str(),
        "oncreate"
            | "onnewintent"
            | "configureflutterengine"
            | "provideflutterengine"
            | "getdartentrypointfunctionname"
            | "getappbundlepath"
            | "getinitialroute"
            | "onstart"
            | "onresume"
            | "onattach"
            | "attachbasecontext"
    )
}

fn is_application_like_class(class_name: &str) -> bool {
    class_name
        .rsplit('.')
        .next()
        .is_some_and(|short| short.ends_with("Application"))
}

fn is_startup_entry_method(
    method: &ScannedMethodKey,
    class_supers: &HashMap<String, Option<String>>,
    direct_stage: bool,
) -> bool {
    let selector = method.method_name.to_ascii_lowercase();
    if !is_startup_entry_selector(&selector) {
        return direct_stage
            && has_super_class(&method.class_descriptor, FLUTTER_ACTIVITY_DESC, class_supers);
    }
    match selector.as_str() {
        "oncreate" | "onstart" | "onresume" | "onattach" | "attachbasecontext" => {
            has_super_class(&method.class_descriptor, FLUTTER_ACTIVITY_DESC, class_supers)
                || is_application_like_class(&method.class_name)
                || direct_stage
        }
        _ => true,
    }
}

fn method_key_from_startup_ref(method: &ScannedStartupMethodRef) -> ScannedMethodKey {
    ScannedMethodKey::new(
        &method.source_dex,
        &method.class_descriptor,
        &method.class_name,
        &method.method_name,
    )
}

fn collect_bootstrap_stage_calls(
    scanned_methods: &[ScannedStartupMethodRef],
) -> HashMap<ScannedMethodKey, Vec<BootstrapStageCall>> {
    let mut out: HashMap<ScannedMethodKey, Vec<BootstrapStageCall>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();

    for method in scanned_methods {
        let Some(stage) = bootstrap_chain_stage_for_method(&method.target_class, &method.target_method)
        else {
            continue;
        };
        let key = method_key_from_startup_ref(method);
        let seen_key = format!(
            "{}|{}|{}|{}|{}",
            key.source_dex, key.class_descriptor, key.method_name, method.target_class, method.target_method
        );
        if !seen.insert(seen_key) {
            continue;
        }
        out.entry(key).or_default().push(BootstrapStageCall {
            target_class: method.target_class.clone(),
            target_class_name: method.target_class_name.clone(),
            target_method: method.target_method.clone(),
            stage,
        });
    }

    for calls in out.values_mut() {
        calls.sort_by(|a, b| {
            stage_sort_key(a.stage)
                .cmp(&stage_sort_key(b.stage))
                .then_with(|| a.target_class_name.cmp(&b.target_class_name))
                .then_with(|| a.target_method.cmp(&b.target_method))
        });
    }

    out
}

fn collect_bootstrap_chain_paths(
    class_supers: &HashMap<String, Option<String>>,
    method_defs: &[ScannedMethodDef],
    app_method_invokes: &[ScannedAppMethodInvoke],
    stage_calls: &HashMap<ScannedMethodKey, Vec<BootstrapStageCall>>,
) -> Vec<BootstrapChainPath> {
    if stage_calls.is_empty() {
        return Vec::new();
    }

    let method_catalog = method_defs
        .iter()
        .map(|method| method.key.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut class_lookup = HashMap::new();
    for method in method_defs {
        class_lookup
            .entry(method.key.class_descriptor.clone())
            .or_insert_with(|| (method.key.source_dex.clone(), method.key.class_name.clone()));
    }

    let mut reverse_edges: HashMap<ScannedMethodKey, Vec<ScannedMethodKey>> = HashMap::new();
    let mut edge_seen = std::collections::HashSet::new();
    for invoke in app_method_invokes {
        let Some((target_source_dex, target_class_name)) = class_lookup.get(&invoke.target_class) else {
            continue;
        };
        let target_key = ScannedMethodKey::new(
            target_source_dex,
            &invoke.target_class,
            target_class_name,
            &invoke.target_method,
        );
        if !method_catalog.contains(&target_key) {
            continue;
        }
        let edge_key = format!(
            "{}|{}|{}|{}|{}|{}",
            invoke.source.source_dex,
            invoke.source.class_descriptor,
            invoke.source.method_name,
            target_key.source_dex,
            target_key.class_descriptor,
            target_key.method_name
        );
        if !edge_seen.insert(edge_key) {
            continue;
        }
        reverse_edges
            .entry(target_key)
            .or_default()
            .push(invoke.source.clone());
    }
    for predecessors in reverse_edges.values_mut() {
        predecessors.sort_by(|a, b| {
            a.source_dex
                .cmp(&b.source_dex)
                .then_with(|| a.class_name.cmp(&b.class_name))
                .then_with(|| a.method_name.cmp(&b.method_name))
        });
    }

    let direct_stage_methods = stage_calls
        .keys()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let mut terminals = direct_stage_methods.iter().cloned().collect::<Vec<_>>();
    terminals.sort_by(|a, b| {
        a.source_dex
            .cmp(&b.source_dex)
            .then_with(|| a.class_name.cmp(&b.class_name))
            .then_with(|| a.method_name.cmp(&b.method_name))
    });

    let mut paths = Vec::new();
    let mut path_seen = std::collections::HashSet::new();
    const MAX_STARTUP_REVERSE_DEPTH: usize = 6;

    for terminal in terminals {
        let mut queue = std::collections::VecDeque::from([(terminal.clone(), vec![terminal.clone()])]);
        let mut visited = std::collections::HashSet::from([terminal.clone()]);
        let mut found = None;
        while let Some((current, path)) = queue.pop_front() {
            if path.len() > MAX_STARTUP_REVERSE_DEPTH {
                continue;
            }
            if current != terminal
                && is_startup_entry_method(
                    &current,
                    class_supers,
                    direct_stage_methods.contains(&current),
                )
            {
                let mut forward = path;
                forward.reverse();
                found = Some(forward);
                break;
            }
            let Some(predecessors) = reverse_edges.get(&current) else {
                continue;
            };
            for predecessor in predecessors {
                if !visited.insert(predecessor.clone()) {
                    continue;
                }
                let mut next_path = path.clone();
                next_path.push(predecessor.clone());
                queue.push_back((predecessor.clone(), next_path));
            }
        }
        let path_keys = found.unwrap_or_else(|| vec![terminal.clone()]);

        let mut seen_stages = std::collections::HashSet::new();
        let mut call_chain = Vec::new();
        for method in &path_keys {
            let is_activity_entry = method.method_name.eq_ignore_ascii_case("onCreate")
                && has_super_class(&method.class_descriptor, FLUTTER_ACTIVITY_DESC, class_supers);
            if is_activity_entry {
                seen_stages.insert("activity_on_create");
            }
            call_chain.push(BootstrapChainPathStep {
                class_descriptor: method.class_descriptor.clone(),
                class_name: method.class_name.clone(),
                method_name: method.method_name.clone(),
                owner_kind: startup_owner_kind(&method.class_descriptor).to_string(),
                stage: is_activity_entry.then_some("activity_on_create".to_string()),
            });

            let Some(calls) = stage_calls.get(method) else {
                continue;
            };
            for call in calls {
                seen_stages.insert(call.stage);
                call_chain.push(BootstrapChainPathStep {
                    class_descriptor: call.target_class.clone(),
                    class_name: call.target_class_name.clone(),
                    method_name: call.target_method.clone(),
                    owner_kind: startup_owner_kind(&call.target_class).to_string(),
                    stage: Some(call.stage.to_string()),
                });
            }
        }

        if seen_stages.is_empty() {
            continue;
        }
        let stages = ordered_bootstrap_stages(&seen_stages);
        let missing_steps = bootstrap_missing_steps(&seen_stages);
        let entry = path_keys.first().expect("startup path has at least one method");
        let terminal = path_keys.last().expect("startup path has at least one method");
        let dedupe_key = format!(
            "{}|{}|{}|{}|{}|{}",
            entry.source_dex,
            entry.class_descriptor,
            entry.method_name,
            terminal.class_descriptor,
            terminal.method_name,
            stages.join(">")
        );
        if !path_seen.insert(dedupe_key) {
            continue;
        }
        paths.push(BootstrapChainPath {
            source_dex: entry.source_dex.clone(),
            entry_class_descriptor: entry.class_descriptor.clone(),
            entry_class_name: entry.class_name.clone(),
            entry_method_name: entry.method_name.clone(),
            owner_kind: startup_owner_kind(&entry.class_descriptor).to_string(),
            call_chain,
            stages,
            complete: missing_steps.is_empty(),
            missing_steps,
            confidence: bootstrap_chain_path_confidence(&seen_stages, path_keys.len()).to_string(),
        });
    }

    paths.sort_by(|a, b| {
        b.complete
            .cmp(&a.complete)
            .then_with(|| a.missing_steps.len().cmp(&b.missing_steps.len()))
            .then_with(|| {
                if a.owner_kind == b.owner_kind {
                    std::cmp::Ordering::Equal
                } else if a.owner_kind == "app" {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            })
            .then_with(|| b.stages.len().cmp(&a.stages.len()))
            .then_with(|| b.call_chain.len().cmp(&a.call_chain.len()))
            .then_with(|| a.entry_class_name.cmp(&b.entry_class_name))
            .then_with(|| a.entry_method_name.cmp(&b.entry_method_name))
    });

    let app_stage_coverage = paths
        .iter()
        .filter(|path| path.owner_kind == "app")
        .flat_map(|path| path.stages.iter().cloned())
        .collect::<std::collections::HashSet<_>>();
    if !app_stage_coverage.is_empty() {
        paths.retain(|path| {
            path.owner_kind == "app"
                || path
                    .stages
                    .iter()
                    .any(|stage| !app_stage_coverage.contains(stage))
        });
    }

    paths
}

fn build_bootstrap_chain_evidence(
    class_supers: &HashMap<String, Option<String>>,
    method_defs: &[ScannedMethodDef],
    scanned_methods: &[ScannedStartupMethodRef],
    app_method_invokes: &[ScannedAppMethodInvoke],
) -> BootstrapChainEvidence {
    let mut sources: HashMap<String, BootstrapChainSourceBuilder> = HashMap::new();
    let stage_calls = collect_bootstrap_stage_calls(scanned_methods);

    for method in scanned_methods {
        let Some(stage) = bootstrap_chain_stage_for_method(&method.target_class, &method.target_method)
        else {
            continue;
        };
        let key = format!(
            "{}|{}|{}",
            method.source_dex, method.class_descriptor, method.method_name
        );
        let builder = sources.entry(key).or_insert_with(|| BootstrapChainSourceBuilder {
            source_dex: method.source_dex.clone(),
            class_descriptor: method.class_descriptor.clone(),
            class_name: method.class_name.clone(),
            method_name: method.method_name.clone(),
            owner_kind: startup_owner_kind(&method.class_descriptor).to_string(),
            stages: Vec::new(),
            seen: std::collections::HashSet::new(),
        });
        if method.method_name.eq_ignore_ascii_case("onCreate")
            && has_super_class(&method.class_descriptor, FLUTTER_ACTIVITY_DESC, class_supers)
        {
            builder.push_stage("activity_on_create");
        }
        builder.push_stage(stage);
    }

    let mut source_entries = sources
        .into_values()
        .filter_map(|builder| {
            if builder.stages.is_empty() {
                return None;
            }
            let confidence = bootstrap_chain_source_confidence(&builder.seen).to_string();
            let missing_steps = bootstrap_missing_steps(&builder.seen);
            Some(BootstrapChainSource {
                source_dex: builder.source_dex,
                class_descriptor: builder.class_descriptor,
                class_name: builder.class_name,
                method_name: builder.method_name,
                owner_kind: builder.owner_kind,
                stages: builder.stages,
                complete: missing_steps.is_empty(),
                missing_steps,
                confidence,
            })
        })
        .collect::<Vec<_>>();

    source_entries.sort_by(|a, b| {
        b.complete
            .cmp(&a.complete)
            .then_with(|| a.missing_steps.len().cmp(&b.missing_steps.len()))
            .then_with(|| {
                if a.owner_kind == b.owner_kind {
                    std::cmp::Ordering::Equal
                } else if a.owner_kind == "app" {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            })
            .then_with(|| b.stages.len().cmp(&a.stages.len()))
            .then_with(|| a.source_dex.cmp(&b.source_dex))
            .then_with(|| a.class_name.cmp(&b.class_name))
            .then_with(|| a.method_name.cmp(&b.method_name))
    });

    let paths = collect_bootstrap_chain_paths(
        class_supers,
        method_defs,
        app_method_invokes,
        &stage_calls,
    );
    let (complete, missing_steps) = paths
        .first()
        .map(|path| (path.complete, path.missing_steps.clone()))
        .or_else(|| {
            source_entries
                .first()
                .map(|source| (source.complete, source.missing_steps.clone()))
        })
        .unwrap_or_else(|| (false, Vec::new()));

    BootstrapChainEvidence {
        complete,
        missing_steps,
        sources: source_entries,
        paths,
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

fn invoke_arg_registers(instruction: &Instruction) -> Option<Vec<u16>> {
    match instruction {
        Instruction::InvokeVirtual { nargs, args, .. }
        | Instruction::InvokeSuper { nargs, args, .. }
        | Instruction::InvokeDirect { nargs, args, .. }
        | Instruction::InvokeStatic { nargs, args, .. }
        | Instruction::InvokeInterface { nargs, args, .. } => Some(
            args.iter()
                .take(usize::from(*nargs))
                .map(|reg| u16::from(*reg))
                .collect(),
        ),
        Instruction::InvokeVirtualRange { args, .. }
        | Instruction::InvokeSuperRange { args, .. }
        | Instruction::InvokeDirectRange { args, .. }
        | Instruction::InvokeStaticRange { args, .. }
        | Instruction::InvokeInterfaceRange { args, .. } => Some(args.clone()),
        _ => None,
    }
}

fn tracked_string(
    tracked: &std::collections::HashMap<u16, TrackedRegisterValue>,
    reg: u16,
) -> Option<String> {
    match tracked.get(&reg) {
        Some(TrackedRegisterValue::StringLiteral(value)) => Some(value.clone()),
        _ => None,
    }
}

fn build_tracked_dart_entrypoint(
    args: &[u16],
    tracked: &std::collections::HashMap<u16, TrackedRegisterValue>,
) -> TrackedDartEntrypointValue {
    let mut value = TrackedDartEntrypointValue::default();
    if args.len() >= 3 {
        value.app_bundle_path = tracked_string(tracked, args[1]);
    }
    if args.len() >= 4 {
        value.library_uri = tracked_string(tracked, args[2]);
        value.function_name = tracked_string(tracked, args[3]);
    } else if args.len() >= 3 {
        value.function_name = tracked_string(tracked, args[2]);
    }
    value
}

fn push_scanned_dart_entrypoint(
    out: &mut Vec<ScannedDartEntrypoint>,
    source_dex: &str,
    class_descriptor: &str,
    class_name: &str,
    method_name: &str,
    target_method: &str,
    value: &TrackedDartEntrypointValue,
) {
    out.push(ScannedDartEntrypoint {
        source_dex: source_dex.to_string(),
        class_descriptor: class_descriptor.to_string(),
        class_name: class_name.to_string(),
        method_name: method_name.to_string(),
        target_method: target_method.to_string(),
        function_name: value.function_name.clone(),
        library_uri: value.library_uri.clone(),
        app_bundle_path: value.app_bundle_path.clone(),
    });
}

fn build_flutterjni_entrypoint(
    args: &[u16],
    tracked: &std::collections::HashMap<u16, TrackedRegisterValue>,
) -> TrackedDartEntrypointValue {
    let mut value = TrackedDartEntrypointValue::default();
    if args.len() >= 2 {
        value.app_bundle_path = tracked_string(tracked, args[1]);
    }
    if args.len() >= 3 {
        value.function_name = tracked_string(tracked, args[2]);
    }
    if args.len() >= 4 {
        value.library_uri = tracked_string(tracked, args[3]);
    }
    value
}

fn decode_method_refs<B: AsRef<[u8]>>(
    dex: &dex::Dex<B>,
    method: &ScannedMethodKey,
    app_class_descriptors: &std::collections::HashSet<String>,
    insns: &[u16],
    parse_errors: &mut Vec<String>,
) -> (
    Vec<ScannedStartupMethodRef>,
    Vec<ScannedDartEntrypoint>,
    Vec<ScannedAppMethodInvoke>,
) {
    let mut out = Vec::new();
    let mut dart_entrypoints = Vec::new();
    let mut app_method_invokes = Vec::new();
    let mut remaining = insns;
    let mut tracked = std::collections::HashMap::<u16, TrackedRegisterValue>::new();
    let mut last_invoke_result: Option<TrackedRegisterValue> = None;
    let source_key = method.clone();
    while !remaining.is_empty() {
        let decoded = decode_one_silently(&mut remaining);
        match decoded {
            Ok(Ok(instruction)) => {
                let pending_invoke_result = last_invoke_result.take();
                match &instruction {
                    Instruction::MoveObject(dst, src) => {
                        if let Some(value) = tracked.get(&u16::from(*src)).cloned() {
                            tracked.insert(u16::from(*dst), value);
                        } else {
                            tracked.remove(&u16::from(*dst));
                        }
                    }
                    Instruction::MoveObjectFrom16(dst, src) => {
                        if let Some(value) = tracked.get(src).cloned() {
                            tracked.insert(u16::from(*dst), value);
                        } else {
                            tracked.remove(&u16::from(*dst));
                        }
                    }
                    Instruction::MoveObject16(dst, src) => {
                        if let Some(value) = tracked.get(src).cloned() {
                            tracked.insert(*dst, value);
                        } else {
                            tracked.remove(dst);
                        }
                    }
                    Instruction::MoveResultObject(dst) => {
                        if let Some(value) = pending_invoke_result {
                            tracked.insert(u16::from(*dst), value);
                        } else {
                            tracked.remove(&u16::from(*dst));
                        }
                    }
                    Instruction::ConstString(dst, idx) => match dex.get_string((*idx).into()) {
                        Ok(value) => {
                            tracked.insert(
                                u16::from(*dst),
                                TrackedRegisterValue::StringLiteral(value.to_string()),
                            );
                        }
                        Err(err) => push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve string {}: {}",
                                method.class_name, method.method_name, idx, err
                            ),
                        ),
                    },
                    Instruction::ConstStringJumbo(dst, idx) => match dex.get_string(*idx) {
                        Ok(value) => {
                            tracked.insert(
                                u16::from(*dst),
                                TrackedRegisterValue::StringLiteral(value.to_string()),
                            );
                        }
                        Err(err) => push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve jumbo string {}: {}",
                                method.class_name, method.method_name, idx, err
                            ),
                        ),
                    },
                    Instruction::NewInstance(dst, ty) => match dex.get_type(TypeId::from(u32::from(*ty))) {
                        Ok(resolved_type) => {
                            if resolved_type.type_descriptor() == DART_ENTRYPOINT_DESC {
                                tracked.insert(
                                    u16::from(*dst),
                                    TrackedRegisterValue::DartEntrypoint(
                                        TrackedDartEntrypointValue::default(),
                                    ),
                                );
                            } else {
                                tracked.remove(&u16::from(*dst));
                            }
                        }
                        Err(err) => push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve new-instance type {}: {}",
                                method.class_name, method.method_name, ty, err
                            ),
                        ),
                    },
                    Instruction::SGetObject(dst, _) => {
                        tracked.remove(&u16::from(*dst));
                    }
                    _ => {}
                }

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
                                method.class_name, method.method_name, method_id, err
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
                                method.class_name, method.method_name, method_id, err
                            ),
                        );
                        continue;
                    }
                };
                let target_method = match dex.get_string(method_item.name_idx()) {
                    Ok(name) => name.to_string(),
                    Err(err) => {
                        push_parse_error(
                            parse_errors,
                            format!(
                                "{}:{} -> resolve target method for method id {}: {}",
                                method.class_name, method.method_name, method_id, err
                            ),
                        );
                        continue;
                    }
                };
                if app_class_descriptors.contains(&target_class)
                    && !target_method.starts_with('<')
                {
                    app_method_invokes.push(ScannedAppMethodInvoke {
                        source: source_key.clone(),
                        target_class: target_class.clone(),
                        target_method: target_method.clone(),
                    });
                }
                let is_dart_entrypoint_ctor =
                    target_class == DART_ENTRYPOINT_DESC && target_method == "<init>";
                let is_execute_dart_entrypoint =
                    target_class == DART_EXECUTOR_DESC && target_method == "executeDartEntrypoint";
                let is_flutterjni_entrypoint =
                    target_class == FLUTTER_JNI_DESC
                        && target_method == "runBundleAndSnapshotFromLibrary";
                if !is_relevant_startup_class(&target_class) {
                    continue;
                }
                out.push(ScannedStartupMethodRef {
                    source_dex: method.source_dex.clone(),
                    class_descriptor: method.class_descriptor.clone(),
                    class_name: method.class_name.clone(),
                    method_name: method.method_name.clone(),
                    target_class_name: descriptor_to_java_name(&target_class),
                    target_class,
                    target_method,
                });

                if let Some(args) = invoke_arg_registers(&instruction) {
                    if is_dart_entrypoint_ctor {
                        if let Some(receiver) = args.first() {
                            let tracked_value = build_tracked_dart_entrypoint(&args, &tracked);
                            tracked.insert(
                                *receiver,
                                TrackedRegisterValue::DartEntrypoint(tracked_value.clone()),
                            );
                            if tracked_value.function_name.is_some()
                                || tracked_value.library_uri.is_some()
                                || tracked_value.app_bundle_path.is_some()
                            {
                                push_scanned_dart_entrypoint(
                                    &mut dart_entrypoints,
                                    &method.source_dex,
                                    &method.class_descriptor,
                                    &method.class_name,
                                    &method.method_name,
                                    "<init>",
                                    &tracked_value,
                                );
                            }
                        }
                    } else if is_execute_dart_entrypoint {
                        if let Some(value) = args.iter().skip(1).find_map(|reg| match tracked.get(reg) {
                            Some(TrackedRegisterValue::DartEntrypoint(value)) => Some(value.clone()),
                            _ => None,
                        }) {
                            push_scanned_dart_entrypoint(
                                &mut dart_entrypoints,
                                &method.source_dex,
                                &method.class_descriptor,
                                &method.class_name,
                                &method.method_name,
                                "executeDartEntrypoint",
                                &value,
                            );
                        }
                    } else if is_flutterjni_entrypoint {
                        let value = build_flutterjni_entrypoint(&args, &tracked);
                        if value.function_name.is_some()
                            || value.library_uri.is_some()
                            || value.app_bundle_path.is_some()
                        {
                            push_scanned_dart_entrypoint(
                                &mut dart_entrypoints,
                                &method.source_dex,
                                &method.class_descriptor,
                                &method.class_name,
                                &method.method_name,
                                "runBundleAndSnapshotFromLibrary",
                                &value,
                            );
                        }
                    }
                }
            }
            Ok(Err(decode::Error::Metadata { length })) => {
                if remaining.len() < length {
                    push_parse_error(
                        parse_errors,
                        format!(
                            "{}:{} -> truncated Dalvik metadata payload",
                            method.class_name, method.method_name
                        ),
                    );
                    break;
                }
                remaining = &remaining[length..];
            }
            Ok(Err(err)) => {
                push_parse_error(
                    parse_errors,
                    format!(
                        "{}:{} -> Dalvik decode error: {:?}",
                        method.class_name, method.method_name, err
                    ),
                );
                break;
            }
            Err(_) => {
                push_parse_error(
                    parse_errors,
                    format!(
                        "{}:{} -> Dalvik decode panic on inline metadata",
                        method.class_name, method.method_name
                    ),
                );
                break;
            }
        }
    }
    (out, dart_entrypoints, app_method_invokes)
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
    let mut method_defs = Vec::new();
    let mut method_refs = Vec::new();
    let mut app_method_invokes = Vec::new();
    let mut dart_entrypoints = Vec::new();
    let mut parse_errors = Vec::new();
    let mut app_class_descriptors = std::collections::HashSet::new();

    for class in dex.classes() {
        let class = class.map_err(|err| anyhow!("parse class in {}: {}", source_dex, err))?;
        let class_descriptor = class.jtype().type_descriptor().to_string();
        let class_name = class.jtype().to_java_type();
        app_class_descriptors.insert(class_descriptor.clone());
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
    }

    for class in dex.classes() {
        let class = class.map_err(|err| anyhow!("parse class in {}: {}", source_dex, err))?;
        let class_descriptor = class.jtype().type_descriptor().to_string();
        let class_name = class.jtype().to_java_type();

        for method in class.methods() {
            let Some(code) = method.code() else {
                continue;
            };
            let method_name = method.name().to_string();
            let method_key =
                ScannedMethodKey::new(source_dex, &class_descriptor, &class_name, &method_name);
            method_defs.push(ScannedMethodDef {
                key: method_key.clone(),
            });
            let (refs, entrypoints, invokes) = decode_method_refs(
                &dex,
                &method_key,
                &app_class_descriptors,
                code.insns(),
                &mut parse_errors,
            );
            method_refs.extend(refs);
            dart_entrypoints.extend(entrypoints);
            app_method_invokes.extend(invokes);
        }
    }

    Ok(StartupScanResult {
        classes,
        method_defs,
        method_refs,
        app_method_invokes,
        dart_entrypoints,
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
    let mut scanned_method_defs = Vec::new();
    let mut scanned_methods = Vec::new();
    let mut scanned_app_method_invokes = Vec::new();
    let mut scanned_entrypoints = Vec::new();
    for result in scan_results {
        for err in result.parse_errors {
            push_parse_error(&mut parse_errors, err);
        }
        scanned_classes.extend(result.classes);
        scanned_method_defs.extend(result.method_defs);
        scanned_methods.extend(result.method_refs);
        scanned_app_method_invokes.extend(result.app_method_invokes);
        scanned_entrypoints.extend(result.dart_entrypoints);
    }
    for class in &scanned_classes {
        class_supers.insert(class.class_descriptor.clone(), class.super_descriptor.clone());
    }
    let bootstrap_chain = build_bootstrap_chain_evidence(
        &class_supers,
        &scanned_method_defs,
        &scanned_methods,
        &scanned_app_method_invokes,
    );

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
    let mut entrypoint_method_seen = std::collections::HashSet::new();
    let mut jni_bootstrap = Vec::new();
    let mut jni_seen = std::collections::HashSet::new();
    for entrypoint in &scanned_entrypoints {
        let key = format!(
            "{}|{}|{}|{}|{:?}|{:?}|{:?}",
            entrypoint.source_dex,
            entrypoint.class_descriptor,
            entrypoint.method_name,
            entrypoint.target_method,
            entrypoint.function_name,
            entrypoint.library_uri,
            entrypoint.app_bundle_path
        );
        let method_key = format!(
            "{}|{}|{}|{}",
            entrypoint.source_dex,
            entrypoint.class_descriptor,
            entrypoint.method_name,
            entrypoint.target_method
        );
        if entrypoint_seen.insert(key) {
            entrypoint_method_seen.insert(method_key);
            let confidence = if entrypoint.function_name.is_some()
                || entrypoint.library_uri.is_some()
                || entrypoint.app_bundle_path.is_some()
            {
                "high"
            } else {
                "medium"
            };
            dart_entrypoints.push(DartEntrypointEvidence {
                source_dex: entrypoint.source_dex.clone(),
                class_descriptor: entrypoint.class_descriptor.clone(),
                class_name: entrypoint.class_name.clone(),
                method_name: entrypoint.method_name.clone(),
                target_method: entrypoint.target_method.clone(),
                function_name: entrypoint.function_name.clone(),
                library_uri: entrypoint.library_uri.clone(),
                initial_route: None,
                app_bundle_path: entrypoint.app_bundle_path.clone(),
                confidence: confidence.to_string(),
            });
        }
    }

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
            let method_key = format!(
                "{}|{}|{}|{}",
                method.source_dex, method.class_descriptor, method.method_name, method.target_method
            );
            if entrypoint_method_seen.insert(method_key) {
                dart_entrypoints.push(DartEntrypointEvidence {
                    source_dex: method.source_dex.clone(),
                    class_descriptor: method.class_descriptor.clone(),
                    class_name: method.class_name.clone(),
                    method_name: method.method_name.clone(),
                    target_method: method.target_method.clone(),
                    function_name: None,
                    library_uri: None,
                    initial_route: None,
                    app_bundle_path: None,
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
            .then(a.function_name.cmp(&b.function_name))
            .then(a.library_uri.cmp(&b.library_uri))
            .then(a.app_bundle_path.cmp(&b.app_bundle_path))
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
        || !jni_bootstrap.is_empty()
        || !bootstrap_chain.sources.is_empty();
    let confidence = if !present {
        "none"
    } else if flutter_activity_classes
        .iter()
        .any(|item| item.confidence == "high")
        || startup_methods.iter().any(|item| item.confidence == "high")
        || !dart_entrypoints.is_empty()
        || !jni_bootstrap.is_empty()
        || bootstrap_chain.complete
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
        bootstrap_chain,
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
        AndroidStartupEvidence, ScannedAppMethodInvoke, ScannedDartEntrypoint, ScannedMethodDef,
        ScannedMethodKey, ScannedStartupClass, ScannedStartupMethodRef, StartupScanResult,
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
            method_defs: vec![
                ScannedMethodDef {
                    key: ScannedMethodKey::new(
                        "classes.dex",
                        "Lcom/example/MainActivity;",
                        "com.example.MainActivity",
                        "onCreate",
                    ),
                },
                ScannedMethodDef {
                    key: ScannedMethodKey::new(
                        "classes.dex",
                        "Lcom/example/MainActivity;",
                        "com.example.MainActivity",
                        "configureFlutterEngine",
                    ),
                },
            ],
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
                ScannedStartupMethodRef {
                    source_dex: "classes.dex".to_string(),
                    class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    class_name: "com.example.MainActivity".to_string(),
                    method_name: "configureFlutterEngine".to_string(),
                    target_class: DART_EXECUTOR_DESC.to_string(),
                    target_class_name: "io.flutter.embedding.engine.dart.DartExecutor"
                        .to_string(),
                    target_method: "executeDartEntrypoint".to_string(),
                },
            ],
            app_method_invokes: vec![ScannedAppMethodInvoke {
                source: ScannedMethodKey::new(
                    "classes.dex",
                    "Lcom/example/MainActivity;",
                    "com.example.MainActivity",
                    "onCreate",
                ),
                target_class: "Lcom/example/MainActivity;".to_string(),
                target_method: "configureFlutterEngine".to_string(),
            }],
            dart_entrypoints: vec![ScannedDartEntrypoint {
                source_dex: "classes.dex".to_string(),
                class_descriptor: "Lcom/example/MainActivity;".to_string(),
                class_name: "com.example.MainActivity".to_string(),
                method_name: "configureFlutterEngine".to_string(),
                target_method: "executeDartEntrypoint".to_string(),
                function_name: Some("main".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
                app_bundle_path: Some("flutter_assets".to_string()),
            }],
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
        assert_eq!(evidence.dart_entrypoints.len(), 2);
        assert_eq!(evidence.jni_bootstrap.len(), 1);
        assert_eq!(evidence.startup_methods.len(), 3);
        assert_eq!(evidence.bootstrap_chain.sources.len(), 2);
        assert_eq!(evidence.bootstrap_chain.paths.len(), 2);
        assert!(!evidence.bootstrap_chain.complete);
        assert_eq!(
            evidence.bootstrap_chain.sources[0].stages,
            vec![
                "activity_on_create".to_string(),
                "loader_start_initialization".to_string(),
            ]
        );
        assert!(evidence.bootstrap_chain.sources[0]
            .missing_steps
            .contains(&"dart_entrypoint_execute".to_string()));
        let correlated = evidence
            .bootstrap_chain
            .paths
            .iter()
            .find(|path| path.call_chain.len() >= 3)
            .expect("correlated startup path");
        assert_eq!(correlated.entry_method_name, "onCreate");
        assert!(correlated
            .stages
            .contains(&"dart_entrypoint_execute".to_string()));
        assert!(correlated.call_chain.iter().any(|step| {
            step.class_name == "io.flutter.embedding.engine.dart.DartExecutor"
                && step.method_name == "executeDartEntrypoint"
        }));
        assert!(evidence
            .dart_entrypoints
            .iter()
            .any(|entry| entry.function_name.as_deref() == Some("main")));
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
                function_name: Some("main".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
                initial_route: None,
                app_bundle_path: Some("flutter_assets".to_string()),
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
            bootstrap_chain: super::BootstrapChainEvidence {
                complete: false,
                missing_steps: vec!["jni_attach".to_string()],
                sources: vec![super::BootstrapChainSource {
                    source_dex: "classes.dex".to_string(),
                    class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    class_name: "com.example.MainActivity".to_string(),
                    method_name: "onCreate".to_string(),
                    owner_kind: "app".to_string(),
                    stages: vec![
                        "activity_on_create".to_string(),
                        "flutter_engine_ctor".to_string(),
                    ],
                    complete: false,
                    missing_steps: vec![
                        "delegate_on_attach".to_string(),
                        "loader_start_initialization".to_string(),
                        "loader_ensure_initialization_complete".to_string(),
                        "jni_attach".to_string(),
                        "dart_entrypoint_execute".to_string(),
                    ],
                    confidence: "medium".to_string(),
                }],
                paths: vec![super::BootstrapChainPath {
                    source_dex: "classes.dex".to_string(),
                    entry_class_descriptor: "Lcom/example/MainActivity;".to_string(),
                    entry_class_name: "com.example.MainActivity".to_string(),
                    entry_method_name: "onCreate".to_string(),
                    owner_kind: "app".to_string(),
                    call_chain: vec![
                        super::BootstrapChainPathStep {
                            class_descriptor: "Lcom/example/MainActivity;".to_string(),
                            class_name: "com.example.MainActivity".to_string(),
                            method_name: "onCreate".to_string(),
                            owner_kind: "app".to_string(),
                            stage: Some("activity_on_create".to_string()),
                        },
                        super::BootstrapChainPathStep {
                            class_descriptor: FLUTTER_ENGINE_DESC.to_string(),
                            class_name: "io.flutter.embedding.engine.FlutterEngine".to_string(),
                            method_name: "<init>".to_string(),
                            owner_kind: "framework".to_string(),
                            stage: Some("flutter_engine_ctor".to_string()),
                        },
                    ],
                    stages: vec![
                        "activity_on_create".to_string(),
                        "flutter_engine_ctor".to_string(),
                    ],
                    complete: false,
                    missing_steps: vec![
                        "delegate_on_attach".to_string(),
                        "loader_start_initialization".to_string(),
                        "loader_ensure_initialization_complete".to_string(),
                        "jni_attach".to_string(),
                        "dart_entrypoint_execute".to_string(),
                    ],
                    confidence: "medium".to_string(),
                }],
            },
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
