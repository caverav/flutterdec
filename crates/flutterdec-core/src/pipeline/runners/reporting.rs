use flutterdec_decompiler::PseudocodeArtifact;
use std::collections::HashMap;

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
                && (callee.ends_with(".invoke")
                    || (!line.contains("[selector]")
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

fn extract_assignment_callee(line: &str) -> Option<&str> {
    let eq_idx = line.find("= ")?;
    let rhs = line.get(eq_idx + 2..)?.trim();
    let open_idx = rhs.find('(')?;
    rhs.get(..open_idx).map(str::trim)
}

