//! What `info`, `decompile` and `diff` tell an operator, per scenario.
//!
//! Every case runs the packaged binary against a real snapshot fixture and a
//! real fixture producer, and reads the artifacts the command wrote. The
//! scenarios are the ones an operator actually hits: an exact match that runs,
//! an unknown snapshot that nothing is authorized to parse, an adapter that
//! answers about the wrong snapshot, one that never finishes, one that writes a
//! corrupt model, and a two-sided diff whose sides were not produced the same
//! way.
//!
//! The rig's producers all append to a spawn log, so "no adapter executed" is
//! backed by a count rather than by the absence of an assertion. Each
//! zero-spawn case has a sibling that shows the same rig producing spawns.

mod support;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use support::*;

/// A snapshot whose header parses and whose hash no record covers.
const UNKNOWN_HASH: &str = "ace654289f5abc240509fc941453ebc5";

fn write_libapp(prefix: &Prefix, name: &str, hash: &str) -> String {
    let path = prefix.root().join(name);
    fs::write(&path, synthetic_libapp(hash, FEATURES)).expect("write libapp");
    path.to_str().expect("path").to_string()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read artifact")).expect("artifact JSON")
}

/// `decompile` with the gates relaxed, so the assertion is about what was
/// reported rather than about whether a heuristic model clears a quality bar.
fn decompile_args<'a>(input: &'a str, out: &'a str) -> Vec<&'a str> {
    vec![
        "decompile",
        input,
        "-o",
        out,
        "--function-scope",
        "all",
        "--min-disassembly-ratio",
        "0.0",
        "--max-placeholder-ifs",
        "999999",
        "--max-unresolved-cf",
        "999999",
        "--max-indirect-call-ratio",
        "1.0",
    ]
}

fn out_dir(prefix: &Prefix, name: &str) -> PathBuf {
    prefix.root().join(name)
}

/// Every domain a snapshot parser would supply, and none of which instruction
/// bytes can.
const SEMANTIC_DOMAINS: &[&str] = &[
    "libraries",
    "classes",
    "class_relationships",
    "function_names",
    "object_pool",
    "pool_index_space",
];

fn assert_core_recovery_is_honest(provider: &Value, capabilities: &Value, source: &str) {
    assert_eq!(
        provider["adapter_executed"],
        Value::Bool(false),
        "{source} claims an adapter ran"
    );
    assert_eq!(
        provider["resolved_backend"],
        Value::String("internal".to_string()),
        "{source} named a backend core cannot be"
    );
    assert_eq!(
        provider["adapter_exec_path"],
        Value::Null,
        "{source} names an executable for a run that had none"
    );
    assert_eq!(
        provider["containment"],
        Value::Null,
        "{source} reports containment for a child that never existed"
    );
    assert_eq!(
        provider["compatibility_record_sha256"],
        Value::Null,
        "{source} bound a core-recovered model to a compatibility record"
    );
    assert!(
        provider["core_fallback_effect"]
            .as_str()
            .is_some_and(|effect| effect.contains("no function names")
                && effect.contains("no authoritative ObjectPool index space")),
        "{source} does not say what the fallback costs: {provider}"
    );
    for domain in SEMANTIC_DOMAINS {
        assert_eq!(
            capabilities[*domain],
            Value::String("unavailable".to_string()),
            "{source} claims {domain} without a snapshot parser"
        );
    }
}

/// The honest-fallback case, end to end: an unknown snapshot still produces
/// useful code and refuses to produce meaning.
#[test]
fn an_unknown_snapshot_is_recovered_by_core_with_no_adapter_execution() {
    let prefix = Prefix::answering();
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "unknown.so", UNKNOWN_HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let info = prefix.run(&["info", &input, "--json"]);
    assert_eq!(code(&info), 0, "{}", stderr(&info));
    let report = json(&info);
    assert_eq!(
        report["provider"]["core_fallback_reason"],
        text("no_compatibility_record")
    );
    assert_eq!(report["registry_record_present"], Value::Bool(false));
    assert_eq!(report["adapter_installed"], Value::Bool(false));
    assert_core_recovery_is_honest(
        &report["provider"],
        &report["provider"]["capabilities"],
        "flutterdec info",
    );

    let decompile = prefix.run(&decompile_args(&input, &out_arg));
    assert_eq!(code(&decompile), 0, "{}", stderr(&decompile));
    let summary = read_json(&out.join("report.json"));
    let provider = &summary["adapter_selection"]["provider"];
    assert_eq!(
        provider["core_fallback_reason"],
        text("no_compatibility_record")
    );
    assert_core_recovery_is_honest(&provider, &summary["model"]["capabilities"], "report.json");

    // Useful code output: candidates recovered, disassembled, and emitted.
    assert!(
        summary["counts"]["functions"].as_u64().unwrap_or(0) > 0,
        "core recovered no function candidates: {summary}"
    );
    assert!(
        summary["counts"]["disassembled_functions"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "no candidate reached the disassembler: {summary}"
    );
    let pseudocode = fs::read_dir(out.join("pseudocode"))
        .expect("pseudocode dir")
        .count();
    assert!(pseudocode > 0, "no pseudocode was written");

    // And no semantic record was fabricated to get there.
    assert_eq!(summary["counts"]["libraries"], Value::from(0));
    assert_eq!(summary["counts"]["classes"], Value::from(0));
    assert_eq!(summary["counts"]["object_pool"], Value::from(0));
    assert_eq!(
        summary["model"]["function_name_provenance"]["named"],
        Value::from(0),
        "core recovery named a function"
    );

    assert_eq!(
        prefix.spawns(),
        0,
        "an unknown snapshot executed an adapter {} times",
        prefix.spawns()
    );
    assert!(!prefix.marker.exists());
}

/// The control for the case above: the same prefix, the same producer, a hash
/// the registry does cover. Without this, zero spawns could mean the fixture
/// producer never works.
#[test]
fn an_exact_snapshot_runs_the_authorized_adapter_and_reports_the_binding() {
    let prefix = Prefix::answering();
    let install = prefix.install();
    assert_eq!(code(&install), 0, "{}", stderr(&install));
    let input = write_libapp(&prefix, "exact.so", HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let info = prefix.run(&["info", &input, "--json"]);
    assert_eq!(code(&info), 0, "{}", stderr(&info));
    let provider = json(&info)["provider"].clone();
    assert_eq!(provider["adapter_executed"], Value::Bool(true));
    assert_eq!(provider["core_fallback_reason"], Value::Null);
    assert_eq!(provider["resolved_backend"], text("internal"));
    assert_eq!(provider["requested_backend"], text("auto"));
    assert_eq!(provider["backend_mismatch"], Value::Bool(false));
    assert_eq!(provider["registry_record_present"], Value::Bool(true));
    assert_eq!(provider["parser_family_id"], text("fixture-family"));
    assert_eq!(provider["profile_id"], text("fixture-profile"));
    assert_eq!(provider["artifact_id"], text("fixture-artifact"));
    assert_eq!(provider["producer_trust"], text("registered"));
    assert_eq!(provider["target_arch"], text("arm64"));
    assert_eq!(provider["host_os"], text(std::env::consts::OS));
    assert_eq!(provider["host_arch"], text(std::env::consts::ARCH));
    assert_eq!(
        provider["snapshot_identity_is_exact"],
        Value::Bool(true),
        "an exact identity was not reported as header-derived"
    );

    // The digests are the ones the install reported, not restatements.
    let installed = json(&install);
    assert_eq!(
        provider["compatibility_record_sha256"],
        installed["record"]["compatibility_record_sha256"]
    );
    assert_eq!(provider["artifact_sha256"], installed["record"]["sha256"]);
    assert_eq!(
        provider["producer_artifact_sha256"], installed["record"]["sha256"],
        "the producer digest is not the digest of the bytes that ran"
    );
    assert_eq!(
        provider["adapter_exec_path"].as_str().map(PathBuf::from),
        Some(prefix.artifact())
    );

    let decompile = prefix.run(&decompile_args(&input, &out_arg));
    assert_eq!(code(&decompile), 0, "{}", stderr(&decompile));
    let summary = read_json(&out.join("report.json"));
    assert_eq!(
        summary["adapter_selection"]["provider"], provider,
        "info and report.json describe the same run differently"
    );
    assert_eq!(
        summary["compatibility"]["core_fallback_reason"],
        Value::Null
    );

    assert_eq!(
        prefix.spawns(),
        2,
        "the two commands did not each run the adapter exactly once"
    );
}

/// A pinned external backend is refused by name rather than answered with
/// prologue scanning, and refused before anything is executed.
#[test]
fn a_pinned_external_backend_is_refused_instead_of_substituted() {
    let prefix = Prefix::answering();
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "unknown.so", UNKNOWN_HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let mut args = decompile_args(&input, &out_arg);
    args.extend(["--adapter-backend", "r2flutter"]);
    let refused = prefix.run(&args);
    assert_ne!(code(&refused), 0, "a pinned backend was silently answered");
    let message = stderr(&refused);
    assert!(
        message.contains("--adapter-backend r2flutter")
            && message.contains("no_compatibility_record"),
        "the refusal does not name the deterministic reason: {message}"
    );
    assert!(
        !out.join("report.json").exists(),
        "a refused run still wrote a report"
    );
    assert_eq!(prefix.spawns(), 0);
}

/// The explicit-internal case reads no registry and executes nothing, on a
/// snapshot that an installed adapter would otherwise have handled.
#[test]
fn explicit_internal_mode_bypasses_an_adapter_that_would_have_run() {
    let prefix = Prefix::answering();
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "exact.so", HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let mut args = decompile_args(&input, &out_arg);
    args.extend(["--adapter-backend", "internal"]);
    let decompile = prefix.run(&args);
    assert_eq!(code(&decompile), 0, "{}", stderr(&decompile));

    let summary = read_json(&out.join("report.json"));
    let provider = &summary["adapter_selection"]["provider"];
    assert_eq!(provider["core_fallback_reason"], text("internal_requested"));
    assert_eq!(provider["requested_backend"], text("internal"));
    assert_core_recovery_is_honest(provider, &summary["model"]["capabilities"], "report.json");
    // A record exists for this snapshot; internal mode never looked for it.
    assert_eq!(provider["registry_record_present"], Value::Bool(false));
    assert_eq!(
        prefix.spawns(),
        0,
        "an explicitly internal run executed an adapter"
    );
    assert!(!prefix.marker.exists());
}

/// An adapter that answers about a snapshot it was not given is rejected, and
/// the rejection is not quietly turned into core recovery.
#[test]
fn an_adapter_that_reports_the_wrong_identity_fails_with_its_own_category() {
    let prefix = Prefix::with_producer(&wrong_identity_producer());
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "exact.so", HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let decompile = prefix.run(&decompile_args(&input, &out_arg));
    assert_ne!(code(&decompile), 0, "a wrong-identity model was accepted");
    let message = stderr(&decompile);
    assert!(
        message.contains("error category: adapter_model_rejected"),
        "the failure category is not the model rejection: {message}"
    );
    assert!(
        !out.join("report.json").exists(),
        "a rejected model still produced a report"
    );
    assert_eq!(prefix.spawns(), 1, "the producer did not run exactly once");

    // `info` reports the same failure rather than printing a report with the
    // model fields missing.
    let info = prefix.run(&["info", &input, "--json"]);
    assert_ne!(code(&info), 0);
    assert_eq!(
        json(&info)["adapter_error_category"],
        text("adapter_model_rejected")
    );
}

/// A producer that never finishes is ended by the host deadline, and the
/// command says so rather than hanging or reporting an empty snapshot.
#[test]
fn an_adapter_that_never_finishes_times_out_with_its_own_category() {
    let prefix = Prefix::with_producer(&sleeping_producer());
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "exact.so", HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let started = std::time::Instant::now();
    let mut args = decompile_args(&input, &out_arg);
    args.extend(["--adapter-timeout", "2"]);
    let decompile = prefix.run(&args);
    let elapsed = started.elapsed();

    assert_ne!(code(&decompile), 0, "a run that never answered exited 0");
    let message = stderr(&decompile);
    assert!(
        message.contains("error category: adapter_timeout"),
        "the failure category is not the timeout: {message}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "the deadline did not bound the run: {elapsed:?}"
    );
    assert!(
        !out.join("report.json").exists(),
        "a timed-out run still produced a report"
    );
    assert_eq!(prefix.spawns(), 1);
}

/// A model that is not JSON is a corrupt answer, not an absent one, and it gets
/// its own category rather than the generic failure.
#[test]
fn an_adapter_that_writes_a_corrupt_model_fails_with_its_own_category() {
    let prefix = Prefix::with_producer(&corrupt_model_producer());
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "exact.so", HASH);
    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();

    let decompile = prefix.run(&decompile_args(&input, &out_arg));
    assert_ne!(code(&decompile), 0, "a corrupt model was accepted");
    let message = stderr(&decompile);
    assert!(
        message.contains("error category: adapter_malformed_document"),
        "the failure category is not the malformed document: {message}"
    );
    assert!(
        !out.join("report.json").exists(),
        "a corrupt model still produced a report"
    );
    assert_eq!(prefix.spawns(), 1);
}

/// A two-sided diff reports each side separately, flags a run whose sides were
/// not produced the same way, and refuses to count address-only candidates as
/// functions that matched.
#[test]
fn a_two_sided_diff_reports_each_side_and_flags_a_mixed_run() {
    let prefix = Prefix::answering();
    assert_eq!(code(&prefix.install()), 0);
    let exact = write_libapp(&prefix, "exact.so", HASH);
    let unknown = write_libapp(&prefix, "unknown.so", UNKNOWN_HASH);
    let out = out_dir(&prefix, "diff-out");
    let out_arg = out.to_str().expect("path").to_string();

    let diff = prefix.run(&[
        "diff", "--old", &exact, "--new", &unknown, "-o", &out_arg, "--json",
    ]);
    assert_eq!(code(&diff), 0, "{}", stderr(&diff));
    let report = json(&diff);

    assert_eq!(
        report["provider_mismatch"],
        Value::Bool(true),
        "a diff between an adapter model and a core-recovered one was not flagged"
    );
    assert_eq!(
        report["old_provider"]["adapter_executed"],
        Value::Bool(true)
    );
    assert_eq!(report["old_provider"]["core_fallback_reason"], Value::Null);
    assert_eq!(
        report["new_provider"]["adapter_executed"],
        Value::Bool(false)
    );
    assert_eq!(
        report["new_provider"]["core_fallback_reason"],
        text("no_compatibility_record")
    );
    assert_eq!(
        report["old_provider"]["parser_family_id"],
        text("fixture-family")
    );
    assert_eq!(report["new_provider"]["parser_family_id"], Value::Null);

    // The core-recovered side is all address-only candidates, so none of them
    // is comparable. Counting them as one matching `::` descriptor would have
    // read as "one function, unchanged".
    assert!(
        report["new_uncomparable_function_count"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "the core-recovered side reported no uncomparable candidates: {report}"
    );
    assert_eq!(
        report["new_function_count"],
        Value::from(0),
        "an address-only candidate was counted as a comparable function"
    );

    let written = read_json(&out.join("diff_report.json"));
    assert_eq!(
        written["provider_mismatch"], report["provider_mismatch"],
        "the printed report and the written one disagree"
    );

    // Exactly one side ran an adapter.
    assert_eq!(prefix.spawns(), 1);
}

/// A diff whose old side fails names the side. Both sides are selected
/// independently, so "the old one" is information the operator does not have.
#[test]
fn a_diff_that_fails_on_one_side_says_which_side() {
    let prefix = Prefix::with_producer(&corrupt_model_producer());
    assert_eq!(code(&prefix.install()), 0);
    let exact = write_libapp(&prefix, "exact.so", HASH);
    let unknown = write_libapp(&prefix, "unknown.so", UNKNOWN_HASH);
    let out = out_dir(&prefix, "diff-out");
    let out_arg = out.to_str().expect("path").to_string();

    let diff = prefix.run(&[
        "diff", "--old", &exact, "--new", &unknown, "-o", &out_arg, "--json",
    ]);
    assert_ne!(code(&diff), 0);
    let message = stderr(&diff);
    assert!(
        message.contains("old input") && message.contains("exact.so"),
        "the failure does not name the failing side: {message}"
    );
    assert!(
        message.contains("error category: adapter_malformed_document"),
        "the failing side lost its category: {message}"
    );
    assert!(!out.join("diff_report.json").exists());

    // And the mirror image: the failing input on the new side, where the old
    // side succeeds first. One spawn on each run, and the second names the
    // other side.
    let mirrored = prefix.run(&[
        "diff", "--old", &unknown, "--new", &exact, "-o", &out_arg, "--json",
    ]);
    assert_ne!(code(&mirrored), 0);
    let message = stderr(&mirrored);
    assert!(
        message.contains("new input") && message.contains("exact.so"),
        "the mirrored failure does not name the failing side: {message}"
    );
    assert_eq!(
        prefix.spawns(),
        2,
        "the unknown side spawned an adapter, or a failing side spawned twice"
    );
}
