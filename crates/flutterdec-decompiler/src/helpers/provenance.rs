// The provenance audit for value annotations: one JSON record per emitted
// annotation, carrying the per-candidate attributions that prove where each
// rendered value came from.
//
// This is a file rather than a `debug_assert` on purpose. The corpus is
// produced by a release build, which compiles every `debug_assert` out, so an
// assertion proves nothing about the measured output. Everything here runs in
// release and is gated only on the environment.
//
// Four records live in the stream, told apart by `record`:
//
//   snapshot      - the register state a loss site dropped, keyed by `snapshot_id`
//   annotation    - one emitted annotation, with `candidates[]` attributing each
//                   rendered value to a `path_key` and the `snapshot_id` it came
//                   from
//   cap_omission  - one annotation dropped whole at insertion, with the reason
//                   and the arithmetic that decided it. It has no coordinate,
//                   because it is not in the artifact
//   cap_summary   - that site's running total for the function, counted at the
//                   drop rather than derived from the rows above
//
// The nesting is load-bearing. Completeness matches records to annotations 1:1
// by `(function_id, output_line, output_col)`, so a three-predecessor join must
// be one record with three attributions rather than three records at one
// coordinate.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Bumped whenever a field is added, removed, or re-interpreted.
pub(super) const PROVENANCE_SCHEMA_VERSION: u32 = 1;

/// The tag of the ordinary-call loss site, in both `loss_site` and the key
/// spaces. One definition, so the emitter and the audit cannot spell it apart.
pub(super) const CALL_LOSS_SITE: &str = "call";

/// A site tag plus its address or block id. Serialises as `["call", 4100]`:
/// tagged, so the three key spaces are disjoint by construction rather than by
/// hoping block-id ranges never meet an instruction address.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(super) struct SiteKey(pub(super) &'static str, pub(super) u64);

/// The register state one loss site dropped, captured immediately before the
/// drop. Every recordable value is kept, not only the ones that go on to be
/// rendered, so the checker can tell "this value was never there" apart from
/// "this value was there but not shown".
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ValueSnapshot {
    pub(super) snapshot_id: String,
    pub(super) site_key: SiteKey,
    /// Canonical register name to the value bound at capture time.
    pub(super) registers: Vec<(String, String)>,
}

/// One rendered value and where it came from.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct CandidateAttribution {
    /// The incoming path this one value came from. At a call site there is
    /// exactly one, and it names the clobbering call.
    pub(super) path_key: SiteKey,
    pub(super) value: String,
    pub(super) snapshot_id: String,
}

/// The reasons the insertion path drops a whole annotation, as they are spelled
/// in the audit and in the ledger built from it. The first two are the budgets;
/// the third is the structural gate, which no candidate filtered upstream can
/// reach and which is recorded rather than dropped silently precisely because a
/// gate that cannot fire is the one nobody notices firing.
pub(super) const ANNOTATION_BUDGET: &str = "annotation";
pub(super) const LINE_BUDGET: &str = "line";
pub(super) const UNSAFE_SPAN: &str = "unsafe";

/// One annotation dropped whole because inserting it would breach a budget.
///
/// A cap that drops evidence silently turns into invisible coverage loss, so
/// every drop leaves a row naming the site, the register, which budget it
/// breached and the arithmetic that decided it. `rendered` is the span that was
/// *not* inserted, kept whole so a corpus scan can prove no part of it - not
/// even a prefix - reached the artifact.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct CapOmission {
    pub(super) loss_site: &'static str,
    pub(super) site_key: SiteKey,
    pub(super) register: String,
    pub(super) budget: &'static str,
    pub(super) annotation_len: usize,
    pub(super) line_len: usize,
    pub(super) planned_len: usize,
    pub(super) rendered: String,
}

/// One emitted annotation, before its output coordinate is known.
///
/// The coordinate is deliberately not filled in by the emitter: a later
/// program-level rewrite can still move text on the line, so it is derived from
/// the finished artifact by locating the annotation span itself.
#[derive(Debug, Clone)]
pub(super) struct PendingAnnotationRecord {
    pub(super) loss_site: &'static str,
    pub(super) site_key: SiteKey,
    /// The rendering anchor this annotation was inserted from, named in terms
    /// the emitted IR can resolve on its own: `["block", id]` for a merge,
    /// `["call", va]` for a clobber.
    ///
    /// `site_key` is read off this same anchor, so the two cannot disagree -
    /// that is the point. Recording it anyway is what lets an external reader
    /// check the claim instead of taking it on trust: with the IR in hand it can
    /// ask what kind of construct the anchor actually is and whether the label
    /// agrees.
    pub(super) anchor: SiteKey,
    pub(super) register: String,
    /// The exact rendered span this record describes, used to pair the record
    /// with its coordinate in the finished source.
    pub(super) rendered: String,
    pub(super) candidates: Vec<CandidateAttribution>,
}

/// Everything one function contributed to the audit.
#[derive(Debug, Clone, Default)]
pub(super) struct FunctionProvenance {
    pub(super) function_id: u64,
    /// The loss site this stream belongs to. Set where the stream is built, so
    /// the omission summary names its site without reading the rows it is meant
    /// to be checked against.
    pub(super) loss_site: &'static str,
    pub(super) snapshots: Vec<ValueSnapshot>,
    pub(super) records: Vec<PendingAnnotationRecord>,
    /// One row per annotation this site dropped for a budget.
    pub(super) cap_omissions: Vec<CapOmission>,
    /// The same drops as a running total, kept beside the rows rather than
    /// derived from them: the ledger splits the rows by reason and a scan counts
    /// them, so a row lost between the emitter and the audit file shows up as a
    /// disagreement instead of as a smaller, still plausible total.
    pub(super) omitted_at_insertion: usize,
}

#[derive(serde::Serialize)]
struct SnapshotLine<'a> {
    schema_version: u32,
    record: &'static str,
    sample: &'a str,
    candidate_sha256: &'a str,
    function_id: u64,
    snapshot_id: &'a str,
    site_key: &'a SiteKey,
    registers: &'a [(String, String)],
}

#[derive(serde::Serialize)]
struct AnnotationLine<'a> {
    schema_version: u32,
    record: &'static str,
    sample: &'a str,
    candidate_sha256: &'a str,
    function_id: u64,
    output_line: usize,
    output_col: usize,
    loss_site: &'static str,
    site_key: &'a SiteKey,
    anchor: &'a SiteKey,
    register: &'a str,
    candidates: &'a [CandidateAttribution],
}

#[derive(serde::Serialize)]
struct CapOmissionLine<'a> {
    schema_version: u32,
    record: &'static str,
    sample: &'a str,
    candidate_sha256: &'a str,
    function_id: u64,
    loss_site: &'static str,
    site_key: &'a SiteKey,
    register: &'a str,
    budget: &'a str,
    annotation_len: usize,
    line_len: usize,
    planned_len: usize,
    rendered: &'a str,
}

#[derive(serde::Serialize)]
struct CapSummaryLine<'a> {
    schema_version: u32,
    record: &'static str,
    sample: &'a str,
    candidate_sha256: &'a str,
    function_id: u64,
    loss_site: &'a str,
    omitted_at_insertion: usize,
}

/// The facts of one dropped annotation, bundled so the recorder stays a single
/// call rather than a wide argument list.
///
/// `annotation_len` is deliberately absent: it is derived from `rendered` inside
/// `record_cap_omission`, so no caller can compute it differently.
pub(super) struct CapOmissionFacts {
    pub(super) loss_site: &'static str,
    pub(super) site_key: SiteKey,
    pub(super) register: String,
    pub(super) rendered: String,
    pub(super) budget: &'static str,
    pub(super) line_len: usize,
    pub(super) planned_len: usize,
}

/// Note one annotation dropped whole by a budget.
///
/// The only place a drop is counted, so the count and the detail row cannot be
/// added at different sites and disagree about what a drop is.
///
/// Unlike the snapshots this is *not* gated on the audit being on. A drop is
/// rare and the row costs one allocation when it happens, and a counter that
/// only exists under an environment variable is one a fixture cannot assert on -
/// which is how a cap becomes silent again. Only the write to the audit file is
/// gated; the emitted bytes are identical either way.
pub(super) fn record_cap_omission(
    provenance: &mut FunctionProvenance,
    facts: CapOmissionFacts,
) {
    provenance.omitted_at_insertion += 1;
    provenance.cap_omissions.push(CapOmission {
        loss_site: facts.loss_site,
        site_key: facts.site_key,
        register: facts.register,
        budget: facts.budget,
        annotation_len: facts.rendered.len(),
        line_len: facts.line_len,
        planned_len: facts.planned_len,
        rendered: facts.rendered,
    });
}

/// The audit output path, or `None` when the audit is off.
///
/// Off is the default: the corpus run that measures emitted output must not pay
/// for instrumentation it does not read.
pub(super) fn audit_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("FLUTTERDEC_PROV_AUDIT")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
    .as_ref()
}

pub(super) fn audit_enabled() -> bool {
    audit_path().is_some()
}

fn sample_name() -> &'static str {
    static SAMPLE: OnceLock<String> = OnceLock::new();
    SAMPLE.get_or_init(|| std::env::var("FLUTTERDEC_PROV_SAMPLE").unwrap_or_default())
}

/// The digest of the binary that produced these records.
///
/// Read from the running executable rather than from an environment variable so
/// the record identifies the build that actually emitted it. A validator still
/// checks the field against its own `sha256sum`; this only removes the chance of
/// labelling one build with another's digest.
fn candidate_sha256() -> &'static str {
    static DIGEST: OnceLock<String> = OnceLock::new();
    DIGEST.get_or_init(|| {
        let Ok(exe) = std::env::current_exe() else {
            return "unavailable".to_string();
        };
        let Ok(output) = std::process::Command::new("sha256sum").arg(&exe).output() else {
            return "unavailable".to_string();
        };
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("unavailable")
            .to_string()
    })
}

fn audit_file() -> Option<&'static Mutex<File>> {
    static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = audit_path()?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Locate each record's annotation in the finished source and append the audit
/// lines for one function.
///
/// The coordinate comes from the output itself: the span is searched for in
/// render order and must match the record's own rendered text, so a record whose
/// annotation did not survive to the artifact is dropped rather than given a
/// coordinate it does not occupy.
pub(super) fn write_function_provenance(source: &str, provenance: &FunctionProvenance) {
    if provenance.records.is_empty() && provenance.cap_omissions.is_empty() {
        return;
    }
    let Some(file) = audit_file() else {
        return;
    };

    let mut placed: Vec<(usize, usize, &PendingAnnotationRecord)> = Vec::new();
    let mut cursor = (0usize, 0usize);
    for record in &provenance.records {
        let Some(position) = find_span(source, &record.rendered, cursor) else {
            continue;
        };
        cursor = (position.0, position.1 + 1);
        placed.push((position.0, position.1, record));
    }
    if placed.is_empty() && provenance.cap_omissions.is_empty() {
        return;
    }

    let mut referenced: Vec<&str> = placed
        .iter()
        .flat_map(|(_, _, record)| {
            record
                .candidates
                .iter()
                .map(|candidate| candidate.snapshot_id.as_str())
        })
        .collect();
    referenced.sort_unstable();
    referenced.dedup();

    let mut out = String::new();
    for snapshot in &provenance.snapshots {
        if !referenced.contains(&snapshot.snapshot_id.as_str()) {
            continue;
        }
        let line = SnapshotLine {
            schema_version: PROVENANCE_SCHEMA_VERSION,
            record: "snapshot",
            sample: sample_name(),
            candidate_sha256: candidate_sha256(),
            function_id: provenance.function_id,
            snapshot_id: &snapshot.snapshot_id,
            site_key: &snapshot.site_key,
            registers: &snapshot.registers,
        };
        if let Ok(text) = serde_json::to_string(&line) {
            out.push_str(&text);
            out.push('\n');
        }
    }
    for (output_line, output_col, record) in &placed {
        let line = AnnotationLine {
            schema_version: PROVENANCE_SCHEMA_VERSION,
            record: "annotation",
            sample: sample_name(),
            candidate_sha256: candidate_sha256(),
            function_id: provenance.function_id,
            output_line: *output_line,
            output_col: *output_col,
            loss_site: record.loss_site,
            site_key: &record.site_key,
            anchor: &record.anchor,
            register: &record.register,
            candidates: &record.candidates,
        };
        if let Ok(text) = serde_json::to_string(&line) {
            out.push_str(&text);
            out.push('\n');
        }
    }

    // Omissions are not placed: an annotation that was never inserted has no
    // coordinate in the artifact, and inventing one would make the row look like
    // an emitted annotation to any reader matching by coordinate.
    for omission in &provenance.cap_omissions {
        let line = CapOmissionLine {
            schema_version: PROVENANCE_SCHEMA_VERSION,
            record: "cap_omission",
            sample: sample_name(),
            candidate_sha256: candidate_sha256(),
            function_id: provenance.function_id,
            loss_site: omission.loss_site,
            site_key: &omission.site_key,
            register: &omission.register,
            budget: omission.budget,
            annotation_len: omission.annotation_len,
            line_len: omission.line_len,
            planned_len: omission.planned_len,
            rendered: &omission.rendered,
        };
        if let Ok(text) = serde_json::to_string(&line) {
            out.push_str(&text);
            out.push('\n');
        }
    }
    if provenance.omitted_at_insertion > 0 {
        let line = CapSummaryLine {
            schema_version: PROVENANCE_SCHEMA_VERSION,
            record: "cap_summary",
            sample: sample_name(),
            candidate_sha256: candidate_sha256(),
            function_id: provenance.function_id,
            loss_site: provenance.loss_site,
            omitted_at_insertion: provenance.omitted_at_insertion,
        };
        if let Ok(text) = serde_json::to_string(&line) {
            out.push_str(&text);
            out.push('\n');
        }
    }

    if let Ok(mut handle) = file.lock() {
        let _ = handle.write_all(out.as_bytes());
    }
}

/// The first `(line, column)` of `needle` at or after `from`, both 1-based and
/// counted in bytes, as the emitted file is read.
fn find_span(source: &str, needle: &str, from: (usize, usize)) -> Option<(usize, usize)> {
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        if number < from.0 {
            continue;
        }
        let start = if number == from.0 {
            from.1.saturating_sub(1)
        } else {
            0
        };
        if start > line.len() {
            continue;
        }
        if let Some(offset) = line[start..].find(needle) {
            return Some((number, start + offset + 1));
        }
    }
    None
}
