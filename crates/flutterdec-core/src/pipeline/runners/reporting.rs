use flutterdec_decompiler::PseudocodeArtifact;
use std::collections::{HashMap, HashSet};
use flutterdec_disasm_arm64::{HintKind, ProgramHints};

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
    /// The typed hint kind, not a free-text label a producer could pick.
    pub(super) kind: String,
    /// Which host artifact the hint came from.
    pub(super) source: String,
    /// How well it is known. Always `derived` or `heuristic`, never exact.
    pub(super) provenance: String,
    pub(super) selector: String,
    pub(super) target_va: Option<u64>,
    pub(super) owner_class: Option<String>,
    pub(super) library_uri: Option<String>,
    pub(super) detail: String,
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

fn normalize_bootflow_entries(entries: &mut Vec<BootflowDiscoveryEntry>) {
    entries.sort_by(|a, b| {
        a.target_va
            .unwrap_or(0)
            .cmp(&b.target_va.unwrap_or(0))
            .then_with(|| a.selector.cmp(&b.selector))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    entries.truncate(20);
}

/// Report the boot-flow hints the host derived, grouped by category.
///
/// Reads [`ProgramHints`] and nothing else. v3 read this back out of the model's
/// object pool, which meant the report could only describe what enrichment had
/// already written into the adapter's own records.
pub(super) fn collect_bootflow_discovery(hints: &ProgramHints) -> BootflowDiscoverySummary {
    let mut out = BootflowDiscoverySummary::default();
    // Two hint kinds can share a category: `EntryPoint` and `BootMain` both mean
    // "main". Reporting the same address twice under one category would inflate
    // the discovery counts a reader uses to judge coverage.
    let mut seen: HashSet<(String, Option<u64>, String, String)> = HashSet::new();
    for hint in hints.iter() {
        let entry = BootflowDiscoveryEntry {
            kind: hint.kind.as_str().to_string(),
            source: hint.origin.as_str().to_string(),
            provenance: hint.provenance.as_str().to_string(),
            selector: hint.selector.clone(),
            target_va: hint.target_va,
            owner_class: hint.owner_class.clone(),
            library_uri: hint.library_uri.clone(),
            detail: hint.detail.clone(),
        };
        let (category, bucket) = match hint.kind {
            HintKind::EntryPoint | HintKind::BootMain => ("main", &mut out.main),
            HintKind::BootRunApp => ("runapp", &mut out.runapp),
            HintKind::DeepLinkHandler => ("deeplink", &mut out.deeplink),
            HintKind::ActivityHandler => ("activity", &mut out.activity),
            HintKind::BootstrapInit => ("bootstrap", &mut out.bootstrap),
        };
        let key = (
            category.to_string(),
            hint.target_va,
            hint.selector.to_ascii_lowercase(),
            entry.source.clone(),
        );
        if !seen.insert(key) {
            continue;
        }
        bucket.push(entry);
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
