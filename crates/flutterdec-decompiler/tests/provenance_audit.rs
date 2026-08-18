//! The pre-call provenance audit, end to end, plus the demonstration that its
//! checker detects a real violation.
//!
//! This lives outside the unit tests on purpose. The audit path is read once per
//! process, so a test that sets it has to own the process; a `#[test]` inside the
//! library would race whichever unit test emitted first and silently observe no
//! audit at all.
//!
//! Only one test here emits: the audit file is append-only and shared, so two
//! tests writing it concurrently would each see the other's records. The loader
//! guard below is safe to sit alongside it because it emits nothing, sets no
//! environment variable, and only reads the source tree.

use flutterdec_decompiler::{
    emit_program_with_runtime_stubs, RuntimeStubEffect, PRE_CALL_ANNOTATION,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

fn stmt(va: u64, src: &str) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Other,
        src: src.to_string(),
        target: String::new(),
    }
}

fn call_to(va: u64, target: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #0x{target:x}"),
        target: format!("#0x{target:x}"),
    }
}

fn ret(va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Return,
        src: "ret".to_string(),
        target: String::new(),
    }
}

/// Two calls, each clobbering x9 while it holds a different value, and an
/// unresolved read after each. Two annotations, two snapshots, and the values
/// are distinguishable, so a record citing the wrong snapshot is visible rather
/// than merely wrong.
fn fixture() -> FunctionIr {
    FunctionIr {
        function_id: 0x4242,
        name: "auditedClobber".to_string(),
        entry_va: 0x1000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x1000,
            instrs: vec![
                stmt(0x1000, "ldur x20, [x2, #15]"),
                stmt(0x1004, "ldur x9, [x1, #7]"),
                call_to(0x1008, 0x9000),
                stmt(0x100c, "stur x9, [x19, #7]"),
                stmt(0x1010, "ldur x9, [x20, #7]"),
                stmt(0x1014, "stur x9, [x23, #7]"),
                call_to(0x1018, 0x9000),
                stmt(0x101c, "stur x9, [x24, #7]"),
                ret(0x1020),
            ],
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    }
}

fn checker() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/check-annotation-provenance.py")
}

/// A crude field read, adequate because every value here is emitter-generated
/// and contains no escape or brace.
fn field<'a>(row: &'a str, name: &str) -> &'a str {
    let key = format!("\"{name}\":");
    let start = row.find(&key).expect("field present") + key.len();
    let rest = &row[start..];
    if let Some(quoted) = rest.strip_prefix('"') {
        &quoted[..quoted.find('"').expect("terminated string")]
    } else {
        let end = rest.find([',', '}']).expect("terminated value");
        &rest[..end]
    }
}

/// The five `include!` lines in `src/tests.rs` are the only thing pulling the
/// five protected in-crate oracle files into the compiled unit-test target, and
/// `#[cfg(test)] mod tests;` in `src/lib.rs` is the only thing pulling
/// `src/tests.rs` in. Delete either level and the unit-test binary still prints
/// `test result: ok`, with fewer tests and a whole protected oracle silenced.
///
/// This assertion lives in an integration test on purpose: it compiles as its own
/// crate, so it cannot be silenced by the loader it protects. A `#[test]` inside
/// the library would disappear along with everything else the moment `mod tests;`
/// went away.
///
/// `src/lib.rs` is deliberately absent from the protocol's protected digest table
/// because it is product source that later work must edit, so its one loader line
/// is protected by this assertion rather than by a whole-file digest.
#[test]
fn the_protected_oracle_loader_chain_is_intact() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let lib_path = crate_root.join("src/lib.rs");
    let lib = std::fs::read_to_string(&lib_path).expect("the crate root source is readable");
    assert!(
        lib.contains("#[cfg(test)]\nmod tests;"),
        "{} must keep the unit-test loader hook `#[cfg(test)] mod tests;` verbatim, \
         or every in-crate oracle is silenced while its digest still matches",
        lib_path.display()
    );

    let loader_path = crate_root.join("src/tests.rs");
    let loader = std::fs::read_to_string(&loader_path).expect("the loader source is readable");
    for included in [
        "tests/shared.rs",
        "tests/emit_and_helpers.rs",
        "tests/cfg_and_stack.rs",
        "tests/compaction_and_aliasing.rs",
        "tests/golden_and_parser.rs",
    ] {
        let line = format!("include!(\"{included}\");");
        assert!(
            loader.contains(&line),
            "{} must keep `{line}`, or that protected oracle file is never compiled",
            loader_path.display()
        );
    }
    assert_eq!(
        loader.matches("include!").count(),
        5,
        "the loader is exactly the five protected includes, nothing else:\n{loader}"
    );
}

#[test]
fn the_pre_call_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path() {
    let dir = std::env::temp_dir().join("flutterdec-prov-audit-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let audit = dir.join("audit.jsonl");
    std::env::set_var("FLUTTERDEC_PROV_AUDIT", &audit);
    std::env::set_var("FLUTTERDEC_PROV_SAMPLE", "fixture");

    let ir = fixture();
    let stubs: HashMap<u64, RuntimeStubEffect> = HashMap::new();
    let source = emit_program_with_runtime_stubs(
        std::slice::from_ref(&ir),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &stubs,
    )
    .remove(0)
    .source;

    let text = std::fs::read_to_string(&audit).expect("the audit is written in a release build");
    let rows: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let annotations: Vec<&&str> = rows
        .iter()
        .filter(|row| row.contains("\"record\":\"annotation\""))
        .collect();
    let snapshots: Vec<&&str> = rows
        .iter()
        .filter(|row| row.contains("\"record\":\"snapshot\""))
        .collect();

    assert_eq!(
        annotations.len(),
        2,
        "one record per emitted annotation, no more and no fewer:\n{source}\n{text}"
    );
    assert_eq!(snapshots.len(), 2, "one snapshot per cited call:\n{text}");

    // Tagged keys, the call's own address, and the value each call actually
    // took. Asserting the site keys apart is what distinguishes "the audit
    // names a call" from "the audit names *this* call".
    assert!(
        annotations[0].contains("\"site_key\":[\"call\",4104]"),
        "first annotation must key on 0x1008:\n{}",
        annotations[0]
    );
    assert!(
        annotations[1].contains("\"site_key\":[\"call\",4120]"),
        "second annotation must key on 0x1018:\n{}",
        annotations[1]
    );
    assert!(annotations[0].contains("\"value\":\"slot0.f8\""));
    assert!(annotations[1].contains("\"value\":\"slot1.f16.f8\""));
    assert!(annotations[0].contains("\"loss_site\":\"call\""));
    assert!(annotations[0].contains("\"schema_version\":"));

    // The coordinate is checked against the emitted text, not taken on trust.
    for row in &annotations {
        let line: usize = field(row, "output_line").parse().expect("line number");
        let column: usize = field(row, "output_col").parse().expect("column number");
        let text = source.lines().nth(line - 1).expect("line exists");
        assert!(
            text[column - 1..].starts_with(PRE_CALL_ANNOTATION.open()),
            "the record's coordinate must land on its own annotation: {text:?} at {column}"
        );
    }

    // The emitted pseudocode and IR the checker resolves against, in the layout
    // a corpus run writes them. Passing both is what exercises the checker's
    // site-resolution and output-anchor checks rather than leaving them
    // unrun - and the anchor check reads the annotation text, so the checker's
    // copy of the literal has to agree with the emitter's or this fails.
    let pseudocode_dir = dir.join("pseudocode");
    let ir_dir = dir.join("ir");
    std::fs::create_dir_all(&pseudocode_dir).expect("pseudocode directory");
    std::fs::create_dir_all(&ir_dir).expect("ir directory");
    std::fs::write(
        pseudocode_dir.join(format!("{:05}_auditedClobber.dartpseudo", ir.function_id)),
        format!("{source}\n"),
    )
    .expect("emitted pseudocode");
    std::fs::write(
        ir_dir.join(format!("{:05}_auditedClobber.json", ir.function_id)),
        serde_json::to_vec(&ir).expect("serialisable IR"),
    )
    .expect("emitted IR");

    let unmodified = checker();
    let clean = Command::new("python3")
        .arg(&unmodified)
        .arg(&audit)
        .arg("--ir-dir")
        .arg(&ir_dir)
        .arg("--pseudocode-dir")
        .arg(&pseudocode_dir)
        .output()
        .expect("python3 available");
    assert!(
        clean.status.success(),
        "the honest audit must pass the checker:\n{}",
        String::from_utf8_lossy(&clean.stdout)
    );

    // A real violation, planted: the first annotation keeps its own site, its
    // own register and its own snapshot id, and takes its value from the other
    // call's snapshot. Everything about the record stays internally plausible,
    // and only the attribution is wrong - which is the failure this audit
    // exists to catch and the one a self-consistent emitter would produce.
    let planted_path = dir.join("planted.jsonl");
    let planted = text.replacen("\"value\":\"slot0.f8\"", "\"value\":\"slot1.f16.f8\"", 1);
    assert_ne!(planted, text, "the plant must change the audit");
    std::fs::write(&planted_path, &planted).expect("planted audit");

    let caught = Command::new("python3")
        .arg(&unmodified)
        .arg(&planted_path)
        .output()
        .expect("python3 available");
    let report = String::from_utf8_lossy(&caught.stdout).to_string();
    assert!(
        !caught.status.success(),
        "the unmodified checker must reject a candidate taken from the wrong path:\n{report}"
    );
    assert!(
        report.contains("violations snapshot  1"),
        "the violation must be counted once, against the offending candidate:\n{report}"
    );
    assert!(
        report.contains("violations total     1"),
        "and it must not be double counted by another check:\n{report}"
    );

    // A second plant, at the other end of the same binding: a genuine value
    // attributed to the snapshot it did not come from.
    let swapped_path = dir.join("swapped.jsonl");
    let first_snapshot = field(snapshots[0], "snapshot_id").to_string();
    let second_snapshot = field(snapshots[1], "snapshot_id").to_string();
    let swapped = text.replacen(
        &format!("\"snapshot_id\":\"{first_snapshot}\"}}]"),
        &format!("\"snapshot_id\":\"{second_snapshot}\"}}]"),
        1,
    );
    assert_ne!(swapped, text, "the second plant must change the audit");
    std::fs::write(&swapped_path, &swapped).expect("swapped audit");
    let caught = Command::new("python3")
        .arg(&unmodified)
        .arg(&swapped_path)
        .output()
        .expect("python3 available");
    assert!(
        !caught.status.success(),
        "a candidate citing another call's snapshot must fail:\n{}",
        String::from_utf8_lossy(&caught.stdout)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
