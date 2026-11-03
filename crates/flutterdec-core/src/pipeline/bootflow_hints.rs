// Selector classification shared by every enrichment pass.
//
// These predicates say what a selector *looks* like. They do not say what it
// is, and nothing here writes into the adapter's model: the callers turn a
// match into a [`Hint`], which lives in its own record space with its own
// provenance. The previous version of this file appended synthetic
// `ObjectPoolEntry` records at `index = object_pool.len()`, which is how
// derived guesses ended up sharing an index space with hardware pool slots.
use flutterdec_disasm_arm64::Hint;

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
    owner_lower.contains("binding")
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

/// Everything one enrichment pass knows about a candidate before deciding
/// whether it is worth a hint.
pub(crate) struct HintCandidate<'a> {
    pub(crate) origin: HintOrigin,
    pub(crate) provenance: HintProvenance,
    pub(crate) selector: &'a str,
    pub(crate) target_va: Option<u64>,
    pub(crate) owner_class: Option<&'a str>,
    pub(crate) library_uri: Option<&'a str>,
    pub(crate) detail: &'a str,
}

pub(crate) fn push_hint(hints: &mut ProgramHints, kind: HintKind, c: &HintCandidate<'_>) -> bool {
    hints.push(Hint {
        kind,
        origin: c.origin,
        provenance: c.provenance,
        selector: c.selector.to_string(),
        target_va: c.target_va,
        owner_class: c.owner_class.map(str::to_string),
        library_uri: c.library_uri.map(str::to_string),
        detail: c.detail.to_string(),
    })
}

/// The hint kinds a selector's shape supports, in the context it was found in.
///
/// Returns every kind that applies rather than the first: `onNewIntent` on an
/// activity is both a deep-link handler and an activity callback, and collapsing
/// that to one loses a seed category.
pub(crate) fn hint_kinds_for_selector(
    raw_selector: &str,
    owner_class: Option<&str>,
    library_uri: Option<&str>,
) -> Vec<HintKind> {
    let selector = normalize_method_selector(raw_selector);
    if selector.is_empty() {
        return Vec::new();
    }
    let owner_lower = owner_class.unwrap_or("").to_ascii_lowercase();
    let library_lower = library_uri.unwrap_or("").to_ascii_lowercase();

    let mut out = Vec::new();
    if is_main_like_selector(&selector) {
        out.push(HintKind::EntryPoint);
        out.push(HintKind::BootMain);
    }
    if is_runapp_selector(&selector) {
        out.push(HintKind::EntryPoint);
        out.push(HintKind::BootRunApp);
    }
    if is_deeplink_selector(&selector) {
        out.push(HintKind::DeepLinkHandler);
    }
    if is_activity_handler_selector(&selector) {
        // A lifecycle name only means an activity callback in an activity-shaped
        // context. `onResume` on a plain Dart object is not one.
        let activity_context = matches!(selector.as_str(), "onnewintent" | "handleintent")
            || owner_lower.contains("activity")
            || owner_lower.contains("flutterjni")
            || library_lower.contains("activity")
            || library_lower.contains("android")
            || library_lower.starts_with("package:flutter/src/embedding");
        if activity_context {
            out.push(HintKind::ActivityHandler);
        }
    }
    if is_bootstrap_selector(&selector) {
        let bootstrap_context = selector != "ensureinitialized"
            || owner_is_bootstrap_context(&owner_lower)
            || library_is_bootstrap_context(&library_lower);
        if bootstrap_context {
            out.push(HintKind::BootstrapInit);
        }
    }
    out.dedup();
    out
}

/// Hints derived from the names the adapter already recovered.
///
/// This is the replacement for the producer-side "bootflow candidate" pool
/// entries: the same pattern matching, run by the host, over model records, into
/// records that are explicitly heuristic and explicitly not pool entries.
pub(crate) fn collect_model_name_hints(
    model: &flutterdec_adapter::model::ProgramModel,
    hints: &mut ProgramHints,
) -> usize {
    let mut added = 0;
    for function in &model.functions {
        let Some(name) = function.name_text() else {
            continue;
        };
        let owner = model.owner_name(function);
        let library = model.owner_library_uri(function);
        for kind in hint_kinds_for_selector(name, owner, library) {
            let candidate = HintCandidate {
                origin: HintOrigin::ModelNamePattern,
                provenance: HintProvenance::Heuristic,
                selector: name,
                target_va: Some(function.code.start_va),
                owner_class: owner,
                library_uri: library,
                detail: "selector shape of a recovered function name",
            };
            if push_hint(hints, kind, &candidate) {
                added += 1;
            }
        }
    }
    added
}
