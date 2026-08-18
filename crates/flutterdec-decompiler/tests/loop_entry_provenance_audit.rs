//! The loop-entry provenance audit, end to end, plus the demonstration that its
//! checker detects a real violation.
//!
//! Outside the unit tests for the same reason the pre-call audit is: the audit
//! path is read once per process, so a test that sets it has to own the process,
//! and the file is append-only, so two tests writing it would each see the
//! other's rows. One test per integration binary, and this is a separate binary
//! from the pre-call one.
//!
//! The audit is what a `debug_assert` cannot be. Assertions are compiled out of
//! the release build that produces the measured corpus, so this path is exercised
//! here in the same shape a corpus run uses it: environment variable, real
//! emitter, file on disk, unmodified checker.

use flutterdec_decompiler::{
    emit_program_with_runtime_stubs, RuntimeStubEffect, LOOP_ENTRY_ANNOTATION,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

fn blk(id: usize, va: u64, instrs: Vec<LlirInstr>, succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs,
        succs,
        preds: Vec::new(),
    }
}

fn stmt(va: u64, src: &str) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Other,
        src: src.to_string(),
        target: String::new(),
    }
}

fn cbz(va: u64, reg: &str, target_va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Branch,
        src: format!("cbz {reg}, #0x{target_va:x}"),
        target: format!("#0x{target_va:x}"),
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

/// A loop header reached from two entry arms holding different values, with the
/// loop body rebinding the same register to a third. The two entry values are
/// distinguishable, so a candidate attributed to the wrong arm is visible rather
/// than merely wrong - which is what makes the planted violation below a real one.
fn fixture() -> FunctionIr {
    FunctionIr {
        function_id: 0x5151,
        name: "auditedLoopEntry".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x2000)], vec![1, 2]),
            blk(1, 0x1004, vec![stmt(0x1004, "mov x0, #7")], vec![3]),
            blk(2, 0x2000, vec![stmt(0x2000, "mov x0, #9")], vec![3]),
            blk(
                3,
                0x3000,
                vec![
                    stmt(0x3000, "stur x0, [x29, #-0x10]"),
                    cbz(0x3004, "x3", 0x5000),
                ],
                vec![4, 5],
            ),
            blk(
                4,
                0x3008,
                vec![stmt(0x3008, "mov x0, #11"), stmt(0x300c, "sub x3, x3, #1")],
                vec![3],
            ),
            blk(5, 0x5000, vec![ret(0x5000)], Vec::new()),
        ],
    }
}

fn checker() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/check-annotation-provenance.py")
}

/// A crude field read, adequate because every value here is emitter-generated and
/// contains no escape or brace.
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

#[test]
fn the_loop_entry_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path() {
    let dir = std::env::temp_dir().join("flutterdec-loop-prov-audit-test");
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
        1,
        "one record per emitted annotation, not one per candidate:\n{source}\n{text}"
    );
    assert_eq!(
        snapshots.len(),
        2,
        "one snapshot per cited entry predecessor:\n{text}"
    );
    let record = annotations[0];
    // Tagged, and the loop key space rather than the join one, at a block that is
    // both a header and a join.
    assert!(
        record.contains("\"site_key\":[\"loop\",3]"),
        "the site key must name the loop header, tagged:\n{record}"
    );
    assert!(
        record.contains("\"loss_site\":\"loop_entry\"") && record.contains("\"schema_version\":"),
        "the record must name its loss site and schema:\n{record}"
    );
    assert!(
        !text.contains("\"site_key\":[\"join\",3]"),
        "a loop header must not also be claimed as a join site:\n{text}"
    );
    // One attribution per entry predecessor, each naming its own path.
    assert!(
        record.contains("\"path_key\":[\"block\",1],\"value\":\"7\""),
        "the first entry arm's value must be attributed to block 1:\n{record}"
    );
    assert!(
        record.contains("\"path_key\":[\"block\",2],\"value\":\"9\""),
        "the second entry arm's value must be attributed to block 2:\n{record}"
    );
    assert!(
        !record.contains("\"value\":\"11\""),
        "11 is the back-edge value, which the header does not render:\n{record}"
    );

    // The coordinate is checked against the emitted text, not taken on trust.
    let line: usize = field(record, "output_line").parse().expect("line number");
    let column: usize = field(record, "output_col").parse().expect("column number");
    let text_line = source.lines().nth(line - 1).expect("line exists");
    assert!(
        text_line[column - 1..].starts_with(LOOP_ENTRY_ANNOTATION.open()),
        "the record's coordinate must land on its own annotation: {text_line:?} at {column}"
    );
    assert!(
        text_line.contains(&format!(
            "reg0{}",
            LOOP_ENTRY_ANNOTATION.render(&["7", "9"])
        )),
        "the annotation sits beside the register it describes:\n{text_line}"
    );

    // The emitted pseudocode and IR the checker resolves against, in the layout a
    // corpus run writes them. Passing both is what exercises the site-resolution
    // and output-anchor checks rather than leaving them unrun.
    let pseudocode_dir = dir.join("pseudocode");
    let ir_dir = dir.join("ir");
    std::fs::create_dir_all(&pseudocode_dir).expect("pseudocode directory");
    std::fs::create_dir_all(&ir_dir).expect("ir directory");
    std::fs::write(
        pseudocode_dir.join(format!("{:05}_auditedLoopEntry.dartpseudo", ir.function_id)),
        format!("{source}\n"),
    )
    .expect("emitted pseudocode");
    std::fs::write(
        ir_dir.join(format!("{:05}_auditedLoopEntry.json", ir.function_id)),
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
    let clean_report = String::from_utf8_lossy(&clean.stdout).to_string();
    assert!(
        clean.status.success(),
        "the honest audit must pass the checker:\n{clean_report}"
    );
    assert!(
        clean_report.contains("violations loop_ir   0")
            && clean_report.contains("violations loop_anchor 0"),
        "both loop checks must have run, not been skipped:\n{clean_report}"
    );

    // A real violation, planted: the record keeps its own site, its own register
    // and its own snapshot ids, and one candidate takes its value from the other
    // arm's snapshot. Everything stays internally plausible and only the
    // attribution is wrong, which is exactly what a self-consistent emitter would
    // produce and what no check reading the record alone could see.
    let planted_path = dir.join("planted.jsonl");
    let planted = text.replacen(
        "\"path_key\":[\"block\",1],\"value\":\"7\"",
        "\"path_key\":[\"block\",1],\"value\":\"9\"",
        1,
    );
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

    // The other end of the same binding: a genuine value attributed to the
    // snapshot it did not come from. Counted per candidate element, so a record
    // with one good attribution and one invented one is not a satisfied row.
    let swapped_path = dir.join("swapped.jsonl");
    let first = field(snapshots[0], "snapshot_id").to_string();
    let second = field(snapshots[1], "snapshot_id").to_string();
    let swapped = text.replacen(
        &format!("\"snapshot_id\":\"{first}\""),
        &format!("\"snapshot_id\":\"{second}\""),
        1,
    );
    assert_ne!(swapped, text, "the second plant must change the audit");
    std::fs::write(&swapped_path, &swapped).expect("swapped audit");
    let swapped_run = Command::new("python3")
        .arg(&unmodified)
        .arg(&swapped_path)
        .output()
        .expect("python3 available");
    let swapped_report = String::from_utf8_lossy(&swapped_run.stdout).to_string();
    assert!(
        !swapped_run.status.success(),
        "a candidate citing a sibling arm's snapshot must be rejected:\n{swapped_report}"
    );

    // A wrong site, at a block that is a real join with a real predecessor and a
    // real drop of the same register: the failure the IR resolution exists for.
    let mislabelled_path = dir.join("mislabelled.jsonl");
    let mislabelled = text.replace("\"site_key\":[\"loop\",3]", "\"site_key\":[\"loop\",0]");
    assert_ne!(mislabelled, text, "the third plant must change the audit");
    std::fs::write(&mislabelled_path, &mislabelled).expect("mislabelled audit");
    let mislabelled_run = Command::new("python3")
        .arg(&unmodified)
        .arg(&mislabelled_path)
        .arg("--ir-dir")
        .arg(&ir_dir)
        .arg("--pseudocode-dir")
        .arg(&pseudocode_dir)
        .output()
        .expect("python3 available");
    let mislabelled_report = String::from_utf8_lossy(&mislabelled_run.stdout).to_string();
    assert!(
        !mislabelled_run.status.success(),
        "a record naming a block that is not the loop header must be rejected:\n{mislabelled_report}"
    );

    // The checker's copy of the literal and the emitter's constant are the same
    // bytes. Without this the corpus scan would go quietly vacuous the day the
    // literal is reworded: zero loop annotations found, zero violations reported.
    let script = std::fs::read_to_string(&unmodified).expect("readable checker");
    assert!(
        script.contains(LOOP_ENTRY_ANNOTATION.open()),
        "the checker must recognise the emitter's own loop-entry literal"
    );
}
