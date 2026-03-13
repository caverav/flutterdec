use flutterdec_decompiler::PseudocodeArtifact;
use std::collections::{HashMap, HashSet};
use flutterdec_adapter::ProgramModel;

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct SemanticIntentSummary {
    pub(super) framework: usize,
    pub(super) stdlib: usize,
    pub(super) runtime: usize,
    pub(super) native: usize,
    pub(super) selector_tagged: usize,
    pub(super) constructor_calls: usize,
}

#[derive(Debug, Default, Clone)]
pub(super) struct SelectorFallbackSummary {
    pub(super) total: usize,
    pub(super) unique: usize,
    pub(super) top: Vec<SelectorFallbackEntry>,
}

#[derive(Debug, Clone)]
pub(super) struct SelectorFallbackEntry {
    pub(super) selector: String,
    pub(super) count: usize,
    pub(super) sample: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct CallFallbackSummary {
    pub(super) dynamic_call: usize,
    pub(super) dispatch_invoke: usize,
    pub(super) dispatch_target_invoke: usize,
    pub(super) generic_invoke: usize,
}

#[derive(Debug, Clone)]
pub(super) struct BootflowDiscoveryEntry {
    pub(super) decoded_kind: String,
    pub(super) source: String,
    pub(super) selector: String,
    pub(super) target_va: Option<u64>,
    pub(super) owner_class: String,
    pub(super) library_uri: String,
    pub(super) value: String,
}

#[derive(Debug, Default, Clone)]
pub(super) struct BootflowDiscoverySummary {
    pub(super) main: Vec<BootflowDiscoveryEntry>,
    pub(super) runapp: Vec<BootflowDiscoveryEntry>,
    pub(super) deeplink: Vec<BootflowDiscoveryEntry>,
    pub(super) activity: Vec<BootflowDiscoveryEntry>,
    pub(super) bootstrap: Vec<BootflowDiscoveryEntry>,
}

pub(super) fn collect_semantic_intent_summary(
    pseudo: &[PseudocodeArtifact],
) -> SemanticIntentSummary {
    let mut out = SemanticIntentSummary::default();
    for artifact in pseudo {
        for line in artifact.source.lines() {
            if line.contains("// framework:") {
                out.framework += 1;
            }
            if line.contains("// stdlib:") {
                out.stdlib += 1;
            }
            if line.contains("// runtime:") {
                out.runtime += 1;
            }
            if line.contains("// native:") {
                out.native += 1;
            }
            if line.contains("[selector]") {
                out.selector_tagged += 1;
            }
            if line.contains("final ")
                && line.contains(".new(")
                && (line.contains("flutter.") || line.contains("dart."))
            {
                out.constructor_calls += 1;
            }
        }
    }
    out
}

pub(super) fn collect_selector_fallback_summary(
    pseudo: &[PseudocodeArtifact],
) -> SelectorFallbackSummary {
    let mut out = SelectorFallbackSummary::default();
    let mut counts: HashMap<String, (usize, String)> = HashMap::new();
    for artifact in pseudo {
        for line in artifact.source.lines() {
            let Some(start) = line.find("// selector:") else {
                continue;
            };
            let rest = &line[start + "// selector:".len()..];
            let selector = rest.split(',').next().unwrap_or("").trim();
            if selector.is_empty() {
                continue;
            }
            out.total += 1;
            let sample = line
                .trim()
                .replace('\t', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let sample = if sample.len() > 180 {
                let mut truncated = sample[..180].to_string();
                truncated.push_str("...");
                truncated
            } else {
                sample
            };
            counts
                .entry(selector.to_string())
                .and_modify(|(count, _)| *count += 1)
                .or_insert((1, sample));
        }
    }

    let mut ranked = counts
        .into_iter()
        .map(|(selector, (count, sample))| SelectorFallbackEntry {
            selector,
            count,
            sample,
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.selector.cmp(&b.selector))
    });
    out.unique = ranked.len();
    out.top = ranked.into_iter().take(10).collect();
    out
}

pub(super) fn collect_call_fallback_summary(pseudo: &[PseudocodeArtifact]) -> CallFallbackSummary {
    let mut out = CallFallbackSummary::default();
    for artifact in pseudo {
        for line in artifact.source.lines() {
            let callee = extract_assignment_callee(line).unwrap_or("");
            if line.contains("dynamicCall(") {
                out.dynamic_call += 1;
            }
            if line.contains("dispatch.invoke(") {
                out.dispatch_invoke += 1;
            }
            if line.contains("indirect via: dispatchTarget")
                && callee != "dispatch.invoke"
                && !callee.starts_with("dispatch.")
                && (callee.ends_with(".invoke")
                    || (!line.contains("[selector]")
                        && !line.contains("selector:")
                        && !line.contains("target_va:")
                        && !line.contains("framework:")
                        && !line.contains("stdlib:")
                        && !line.contains("runtime:")
                        && !line.contains("native:")
                        && !line.contains("package:")))
            {
                out.dispatch_target_invoke += 1;
            }
            if line.contains("indirect via: indirectTarget") && callee.starts_with("indirectTarget")
            {
                out.generic_invoke += 1;
            }
        }
    }
    out
}

fn is_main_like_selector(selector_lower: &str) -> bool {
    selector_lower == "main"
        || selector_lower.ends_with(".main")
        || selector_lower.ends_with("::main")
        || selector_lower.ends_with("_main")
}

fn is_runapp_selector(selector_lower: &str) -> bool {
    selector_lower == "runapp" || selector_lower.ends_with(".runapp")
}

fn is_deeplink_selector(selector_lower: &str) -> bool {
    matches!(
        selector_lower,
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

fn is_activity_selector(selector_lower: &str) -> bool {
    matches!(
        selector_lower,
        "onnewintent" | "handleintent" | "oncreate" | "onstart" | "onresume" | "onpause" | "onstop"
    )
}

fn is_bootstrap_selector(selector_lower: &str) -> bool {
    matches!(
        selector_lower,
        "ensureinitialized"
            | "nativeensureinitialized"
            | "startinitialization"
            | "ensureinitializationcomplete"
    )
}

struct BootflowEntrySeed<'a> {
    decoded_kind: &'a str,
    source: &'a str,
    selector: &'a str,
    target_va: Option<u64>,
    owner_class: &'a str,
    library_uri: &'a str,
    value: &'a str,
}

fn push_bootflow_entry(
    out: &mut Vec<BootflowDiscoveryEntry>,
    seen: &mut HashSet<String>,
    category: &str,
    seed: &BootflowEntrySeed<'_>,
) {
    let key = format!(
        "{}|{}|{}|{}|{}|{}",
        category,
        seed.target_va
            .map(|va| format!("0x{va:x}"))
            .unwrap_or_else(|| "none".to_string()),
        seed.selector.to_ascii_lowercase(),
        seed.source.to_ascii_lowercase(),
        seed.owner_class.to_ascii_lowercase(),
        seed.library_uri.to_ascii_lowercase()
    );
    if seen.contains(&key) {
        return;
    }
    seen.insert(key);
    out.push(BootflowDiscoveryEntry {
        decoded_kind: seed.decoded_kind.to_string(),
        source: seed.source.to_string(),
        selector: seed.selector.to_string(),
        target_va: seed.target_va,
        owner_class: seed.owner_class.to_string(),
        library_uri: seed.library_uri.to_string(),
        value: seed.value.to_string(),
    });
}

fn normalize_bootflow_entries(entries: &mut Vec<BootflowDiscoveryEntry>) {
    entries.sort_by(|a, b| {
        a.target_va
            .unwrap_or(0)
            .cmp(&b.target_va.unwrap_or(0))
            .then_with(|| a.selector.cmp(&b.selector))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.decoded_kind.cmp(&b.decoded_kind))
    });
    entries.truncate(20);
}

fn bootflow_entry_source(
    entry: &flutterdec_adapter::ObjectPoolEntry,
    decoded_kind_lower: &str,
    value_lower: &str,
) -> &'static str {
    match entry.source.as_deref().map(str::trim).unwrap_or("") {
        "manifest" => "manifest",
        "apk_startup" => "apk_startup",
        "synthetic" | "internal" | "blutter" => "adapter",
        explicit if !explicit.is_empty() => "adapter",
        _ if decoded_kind_lower.starts_with("manifest") || value_lower.starts_with("manifest:") => {
            "manifest"
        }
        _ if decoded_kind_lower.starts_with("startup") || value_lower.ends_with(":apk_startup") => {
            "apk_startup"
        }
        _ => "adapter",
    }
}

pub(super) fn collect_bootflow_discovery(model: &ProgramModel) -> BootflowDiscoverySummary {
    let mut out = BootflowDiscoverySummary::default();
    let mut seen = HashSet::new();

    for entry in &model.object_pool {
        let decoded_kind = entry
            .decoded_kind
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        let decoded_kind_lower = decoded_kind.to_ascii_lowercase();
        let selector = entry.selector.as_deref().map(str::trim).unwrap_or("");
        let selector_lower = selector.to_ascii_lowercase();
        let value = entry.value.trim();
        let value_lower = value.to_ascii_lowercase();
        let owner_class = entry.owner_class.as_deref().map(str::trim).unwrap_or("");
        let library_uri = entry.library_uri.as_deref().map(str::trim).unwrap_or("");
        let source = bootflow_entry_source(entry, &decoded_kind_lower, &value_lower);
        let seed = BootflowEntrySeed {
            decoded_kind,
            source,
            selector,
            target_va: entry.target_va,
            owner_class,
            library_uri,
            value,
        };

        if decoded_kind_lower == "bootmaincandidate"
            || decoded_kind_lower == "startupmaincandidate"
            || decoded_kind_lower == "manifestmaincandidate"
            || value_lower.starts_with("bootflow:main:")
            || value_lower.starts_with("manifest:main")
            || (decoded_kind_lower == "entrypointcandidate" && is_main_like_selector(&selector_lower))
        {
            push_bootflow_entry(
                &mut out.main,
                &mut seen,
                "main",
                &seed,
            );
        }

        if decoded_kind_lower == "bootrunappcandidate"
            || decoded_kind_lower == "startuprunappcandidate"
            || decoded_kind_lower == "manifestrunappcandidate"
            || value_lower.starts_with("bootflow:runapp:")
            || value_lower.starts_with("manifest:runapp")
            || (decoded_kind_lower == "entrypointcandidate" && is_runapp_selector(&selector_lower))
        {
            push_bootflow_entry(
                &mut out.runapp,
                &mut seen,
                "runapp",
                &seed,
            );
        }

        if decoded_kind_lower == "deeplinkhandlercandidate"
            || decoded_kind_lower == "startupdeeplinkcandidate"
            || decoded_kind_lower == "manifestdeeplinkcandidate"
            || value_lower.starts_with("bootflow:deeplink:")
            || value_lower.starts_with("manifest:deeplink")
            || is_deeplink_selector(&selector_lower)
        {
            push_bootflow_entry(
                &mut out.deeplink,
                &mut seen,
                "deeplink",
                &seed,
            );
        }

        if decoded_kind_lower == "activityhandlercandidate"
            || decoded_kind_lower == "startupactivitycandidate"
            || decoded_kind_lower == "manifestactivitycandidate"
            || value_lower.starts_with("bootflow:activity:")
            || value_lower.starts_with("manifest:activity")
            || is_activity_selector(&selector_lower)
        {
            push_bootflow_entry(
                &mut out.activity,
                &mut seen,
                "activity",
                &seed,
            );
        }

        if decoded_kind_lower == "bootstrapinitcandidate"
            || decoded_kind_lower == "startupbootstrapcandidate"
            || decoded_kind_lower == "manifestbootstrapcandidate"
            || value_lower.starts_with("bootflow:init:")
            || value_lower.starts_with("manifest:bootstrap")
            || is_bootstrap_selector(&selector_lower)
        {
            push_bootflow_entry(
                &mut out.bootstrap,
                &mut seen,
                "bootstrap",
                &seed,
            );
        }
    }

    normalize_bootflow_entries(&mut out.main);
    normalize_bootflow_entries(&mut out.runapp);
    normalize_bootflow_entries(&mut out.deeplink);
    normalize_bootflow_entries(&mut out.activity);
    normalize_bootflow_entries(&mut out.bootstrap);
    out
}

fn extract_assignment_callee(line: &str) -> Option<&str> {
    let eq_idx = line.find("= ")?;
    let rhs = line.get(eq_idx + 2..)?.trim();
    let open_idx = rhs.find('(')?;
    rhs.get(..open_idx).map(str::trim)
}
