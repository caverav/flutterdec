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

fn count_dart_files(root: &Path) -> usize {
    let mut count = 0;
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += count_dart_files(&path);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("dart") {
            count += 1;
        }
    }
    count
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
    assert_core_recovery_is_honest(provider, &summary["model"]["capabilities"], "report.json");

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
    let source_files = count_dart_files(&out);
    assert!(source_files > 0, "no source files were written");
    let index = read_json(&out.join("index.json"));
    assert_eq!(
        index["counts"]["source_files"].as_u64().unwrap_or(0),
        source_files as u64
    );
    for entry in index["entries"].as_array().expect("index entries") {
        let path = entry["path"].as_str().expect("entry path");
        assert!(
            out.join(path).is_file(),
            "index entry does not exist: {path}"
        );
    }

    // And no semantic record was fabricated to get there.
    assert_eq!(summary["counts"]["libraries"], Value::from(0));
    assert_eq!(summary["counts"]["classes"], Value::from(0));
    assert_eq!(summary["counts"]["object_pool"], Value::from(0));
    assert_eq!(
        summary["model"]["function_name_provenance"]["named"],
        Value::from(0),
        "core recovery named a function"
    );

    // One diagnostic per domain that stayed unavailable, plus the one that says
    // the code candidates are a scan.
    assert!(
        summary["model"]["diagnostics"].as_u64().unwrap_or(0) > SEMANTIC_DOMAINS.len() as u64,
        "the fallback left domains unavailable without saying why: {summary}"
    );
    // The quality artifact describes the same recovery, and no emitted name
    // claims to come from the snapshot.
    let quality = read_json(&out.join("quality.json"));
    assert_eq!(
        quality["function_count"], summary["counts"]["functions"],
        "quality.json and report.json disagree about what was recovered"
    );
    assert!(quality["disassembled_function_count"].as_u64().unwrap_or(0) > 0);
    assert_eq!(
        summary["name_resolution"]["final_quality"]["exact"],
        Value::from(0),
        "core recovery produced an exact symbol name"
    );

    assert_eq!(
        prefix.spawns(),
        0,
        "an unknown snapshot executed an adapter {} times",
        prefix.spawns()
    );
    assert!(!prefix.marker.exists());

    // Deterministic: the same input through the same command twice reports the
    // same reason, the same counts and the same provider block.
    let second = out_dir(&prefix, "out-again");
    let second_arg = second.to_str().expect("path").to_string();
    let repeat = prefix.run(&decompile_args(&input, &second_arg));
    assert_eq!(code(&repeat), 0, "{}", stderr(&repeat));
    let repeated = read_json(&second.join("report.json"));
    assert_eq!(
        comparable_across_runs(&repeated["adapter_selection"]["provider"]),
        comparable_across_runs(&summary["adapter_selection"]["provider"]),
        "two identical runs described the provider differently"
    );
    assert_eq!(repeated["counts"], summary["counts"]);
    assert_eq!(prefix.spawns(), 0);
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
        comparable_across_runs(&summary["adapter_selection"]["provider"]),
        comparable_across_runs(&provider),
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

/// Explicit internal mode only recovers machine code, even if a parser is installed.
#[test]
fn explicit_internal_mode_bypasses_an_adapter_that_would_have_run() {
    let prefix = Prefix::new();
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
    assert_eq!(provider["builtin_parser"], Value::Null);
    assert_core_recovery_is_honest(provider, &provider["capabilities"], "internal decompile");

    let info = prefix.run(&["info", &input, "--json", "--adapter-backend", "internal"]);
    assert_eq!(code(&info), 0, "{}", stderr(&info));
    assert_eq!(
        json(&info)["provider"]["core_fallback_reason"],
        text("internal_requested")
    );
    assert_eq!(
        prefix.spawns(),
        0,
        "an explicitly internal run executed an adapter"
    );
    assert!(!prefix.marker.exists());
}

#[test]
fn full_jit_identity_rejects_serwalker_before_lookup_but_allows_machine_code_recovery() {
    let prefix = Prefix::new();
    let mut bytes = synthetic_libapp(HASH, FEATURES);
    let header = bytes
        .windows(4)
        .position(|word| word == [0xf5, 0xf5, 0xdc, 0xdc])
        .unwrap();
    bytes[header + 12..header + 20].copy_from_slice(&2i64.to_le_bytes());
    let path = prefix.root().join("full-jit.so");
    fs::write(&path, bytes).unwrap();
    let input = path.to_str().unwrap();
    fs::write(prefix.share().join("adapters/registry.json"), b"not JSON").unwrap();

    for backend in ["internal", "auto", "serwalker"] {
        let info = prefix.run(&["info", input, "--json", "--adapter-backend", backend]);
        let out = out_dir(&prefix, backend);
        let mut args = decompile_args(input, out.to_str().unwrap());
        args.extend(["--adapter-backend", backend, "--max-functions", "1"]);
        let decompile = prefix.run(&args);
        if backend == "serwalker" {
            assert_ne!(code(&info), 0);
            assert_ne!(code(&decompile), 0);
            assert!(
                stderr(&decompile).contains("identity_rejected"),
                "{}",
                stderr(&decompile)
            );
            assert!(!stderr(&decompile).contains("registry_malformed"));
            assert!(!out.join("report.json").exists());
        } else {
            assert_eq!(code(&info), 0, "{}", stderr(&info));
            assert_eq!(code(&decompile), 0, "{}", stderr(&decompile));
            let expected = if backend == "internal" {
                "internal_requested"
            } else {
                "identity_rejected"
            };
            let provider = &json(&info)["provider"];
            assert_eq!(provider["core_fallback_reason"], text(expected));
            assert_eq!(provider["builtin_parser"], Value::Null);
            assert_core_recovery_is_honest(provider, &provider["capabilities"], backend);
            let report = read_json(&out.join("report.json"));
            assert_eq!(
                report["adapter_selection"]["provider"]["core_fallback_reason"],
                text(expected)
            );
        }
    }
    assert_eq!(prefix.spawns(), 0);
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

/// Two records claiming one snapshot fails both commands, before anything runs.
///
/// This is the registry refusal that must not look like the unknown-snapshot
/// case above. That one exits 0 with a heuristic model and says so; this one
/// means the operator's own registry cannot name a parser for a snapshot it may
/// well have one for, so answering it with heuristics would be a wrong answer
/// dressed up as an honest one.
///
/// The same prefix runs successfully first, so the zero exit code, the spawn and
/// the artifacts are all known to be reachable; the second half then asks for
/// exactly none of them.
#[test]
fn an_ambiguous_registry_fails_both_commands_before_any_adapter_runs() {
    let prefix = Prefix::answering();
    assert_eq!(code(&prefix.install()), 0);
    let input = write_libapp(&prefix, "exact.so", HASH);
    let control_out = out_dir(&prefix, "control");
    let control_arg = control_out.to_str().expect("path").to_string();

    // The control: this registry, this snapshot and this producer do work.
    let control = prefix.run(&decompile_args(&input, &control_arg));
    assert_eq!(code(&control), 0, "{}", stderr(&control));
    assert!(control_out.join("report.json").exists());
    assert_eq!(prefix.spawns(), 1);

    // Now the record is written twice, which is the only difference.
    let registry_path = prefix.share().join("adapters/registry.json");
    let mut registry: Value =
        serde_json::from_slice(&fs::read(&registry_path).expect("read registry"))
            .expect("registry JSON");
    let record = registry["records"][0].clone();
    registry["records"]
        .as_array_mut()
        .expect("records array")
        .push(record);
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("serialize registry"),
    )
    .expect("write ambiguous registry");

    let out = out_dir(&prefix, "out");
    let out_arg = out.to_str().expect("path").to_string();
    let decompile = prefix.run(&decompile_args(&input, &out_arg));
    assert_ne!(
        code(&decompile),
        0,
        "an ambiguous registry produced a decompile: {}",
        stderr(&decompile)
    );
    let message = stderr(&decompile);
    assert!(
        message.contains("error category: registry_ambiguous"),
        "the failure category is not the ambiguity: {message}"
    );
    assert!(
        !out.exists(),
        "an ambiguous registry still wrote decompile artifacts"
    );

    let info = prefix.run(&["info", &input, "--json"]);
    assert_ne!(code(&info), 0, "an ambiguous registry exited 0 from info");
    assert_eq!(
        json(&info)["adapter_error_category"],
        text("registry_ambiguous")
    );
    assert_eq!(
        json(&info)["provider"],
        Value::Null,
        "an ambiguous registry still reported a provider"
    );

    // Neither command reached a child, and neither of them fell back: the run
    // count is still the control's single spawn.
    assert_eq!(
        prefix.spawns(),
        1,
        "an ambiguous registry executed an adapter"
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
