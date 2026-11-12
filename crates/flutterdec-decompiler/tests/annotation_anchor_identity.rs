//! Annotation provenance over a corpus whose functions each hold two lines that
//! read the same, end to end and through the unmodified cross-audit reconciler.
//!
//! Outside the unit tests for the reason the other two audits are: the audit path
//! is read once per process and the file is append-only, so a test that sets it
//! owns the process. One emitting test per integration binary, and this is its
//! own binary.
//!
//! What this adds to the two per-site audits is the decoy. Every fixture renders
//! the annotated line twice - once where the value was really lost and once
//! somewhere the value was never lost at all - so a placement that consults text
//! rather than line identity produces an audit in which every field is true and
//! every coordinate is in the wrong place. The reconciler is then run over it,
//! and five one-field corruptions of the honest audit are run past it too, so a
//! pass here is a checker that fires rather than a checker that agrees.

use flutterdec_decompiler::{
    emit_program_with_runtime_stubs, RuntimeStubEffect, EXHAUSTIVE_JOIN_ANNOTATION,
    LOOP_ENTRY_ANNOTATION, PRE_CALL_ANNOTATION,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

fn call_to(va: u64, target: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #0x{target:x}"),
        target: format!("#0x{target:x}"),
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

/// A join reached from two arms with different values for `x9`, behind an entry
/// read of `x9` that renders the identical line.
fn join_fixture() -> FunctionIr {
    FunctionIr {
        function_id: 0x8001,
        name: "joinDecoy".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(
                0,
                0x1000,
                vec![
                    stmt(0x1000, "stur x9, [x19, #7]"),
                    cbz(0x1004, "x1", 0x2000),
                ],
                vec![1, 2],
            ),
            blk(1, 0x1008, vec![stmt(0x1008, "mov x9, #7")], vec![3]),
            blk(2, 0x2000, vec![stmt(0x2000, "mov x9, #9")], vec![3]),
            blk(
                3,
                0x3000,
                vec![stmt(0x3000, "stur x9, [x19, #7]"), ret(0x3004)],
                Vec::new(),
            ),
        ],
    }
}

/// A loop header reached from two arms with different values for `x9`, behind an
/// entry read of `x9` that renders the identical line.
fn loop_fixture() -> FunctionIr {
    FunctionIr {
        function_id: 0x8002,
        name: "loopDecoy".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(
                0,
                0x1000,
                vec![
                    stmt(0x1000, "stur x9, [x19, #7]"),
                    cbz(0x1004, "x1", 0x2000),
                ],
                vec![1, 2],
            ),
            blk(1, 0x1008, vec![stmt(0x1008, "mov x9, #7")], vec![3]),
            blk(2, 0x2000, vec![stmt(0x2000, "mov x9, #9")], vec![3]),
            blk(
                3,
                0x3000,
                vec![
                    stmt(0x3000, "stur x9, [x19, #7]"),
                    cbz(0x3004, "x3", 0x5000),
                ],
                vec![4, 5],
            ),
            blk(
                4,
                0x3008,
                vec![stmt(0x3008, "mov x9, #11"), stmt(0x300c, "sub x3, x3, #1")],
                vec![3],
            ),
            blk(5, 0x5000, vec![ret(0x5000)], Vec::new()),
        ],
    }
}

/// A call clobbering `x9` and an unresolved read after it, behind an identical
/// earlier read no call had clobbered.
fn call_fixture() -> FunctionIr {
    FunctionIr {
        function_id: 0x8003,
        name: "callDecoy".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "stur x9, [x19, #7]"),
                stmt(0x1004, "ldur x9, [x1, #7]"),
                call_to(0x1008, 0x9000),
                stmt(0x100c, "stur x9, [x19, #7]"),
                ret(0x1010),
            ],
            Vec::new(),
        )],
    }
}

fn script(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts")
        .join(name)
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

fn reconcile(audit: &Path, pseudocode: &Path, ir: &Path) -> (bool, String) {
    let run = Command::new("python3")
        .arg(script("prov_cross_audit_reconcile.py"))
        .arg(audit)
        .arg("--pseudocode")
        .arg(pseudocode)
        .arg("--ir")
        .arg(ir)
        .output()
        .expect("python3 available");
    (
        run.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        ),
    )
}

/// Indexes of the lines whose code, annotation aside, is the shared text.
fn decoy_and_site(source: &str) -> (usize, usize) {
    let lines: Vec<usize> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("reg19.f8 = reg9"))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "each fixture renders the decoy and the real site identically:\n{source}"
    );
    (lines[0], lines[1])
}

#[test]
fn annotations_bind_their_own_line_and_the_reconciler_rejects_every_planted_defect() {
    let dir = std::env::temp_dir().join("flutterdec-anchor-identity-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let audit = dir.join("audit.jsonl");
    std::env::set_var("FLUTTERDEC_PROV_AUDIT", &audit);
    std::env::set_var("FLUTTERDEC_PROV_SAMPLE", "decoy");

    let functions = vec![join_fixture(), loop_fixture(), call_fixture()];
    let stubs: HashMap<u64, RuntimeStubEffect> = HashMap::new();
    let artifacts = emit_program_with_runtime_stubs(
        &functions,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &stubs,
    );

    // The three families, each on the second of two identical lines.
    let openers = [
        EXHAUSTIVE_JOIN_ANNOTATION.open(),
        LOOP_ENTRY_ANNOTATION.open(),
        PRE_CALL_ANNOTATION.open(),
    ];
    let mut expected: HashMap<u64, usize> = HashMap::new();
    for (artifact, opener) in artifacts.iter().zip(openers) {
        let (decoy, site) = decoy_and_site(&artifact.source);
        let annotated: Vec<usize> = artifact
            .source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(opener))
            .map(|(index, _)| index)
            .collect();
        assert_eq!(
            annotated,
            vec![site],
            "the annotation belongs to the line the value was lost on, not to the \
             identical line at {decoy}:\n{}",
            artifact.source
        );
        expected.insert(artifact.function_id, site + 1);
    }

    let text = std::fs::read_to_string(&audit).expect("the audit is written in a release build");
    let rows: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let annotations: Vec<&&str> = rows
        .iter()
        .filter(|row| row.contains("\"record\":\"annotation\""))
        .collect();
    assert_eq!(
        annotations.len(),
        3,
        "one record per emitted annotation, one per fixture:\n{text}"
    );

    // Each record's coordinate is checked against the artifact, and against the
    // line the fixture says the value was lost on. The first check alone would
    // pass on a record that took the decoy's coordinate, if the decoy carried an
    // annotation - which is exactly the failure this fixture set exists for.
    for row in &annotations {
        let function_id: u64 = field(row, "function_id").parse().expect("function id");
        let line: usize = field(row, "output_line").parse().expect("line number");
        let column: usize = field(row, "output_col").parse().expect("column number");
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact.function_id == function_id)
            .expect("record names an emitted function");
        let text = artifact.source.lines().nth(line - 1).expect("line exists");
        assert!(
            openers
                .iter()
                .any(|open| text[column - 1..].starts_with(open)),
            "the record's coordinate must land on its own annotation: {text:?} at {column}"
        );
        assert_eq!(
            line, expected[&function_id],
            "the record must name the line the value was really lost on:\n{}",
            artifact.source
        );
    }

    // The corpus layout the reconciler resolves against.
    let pseudocode_dir = dir.join("pseudocode");
    let ir_dir = dir.join("ir");
    std::fs::create_dir_all(&pseudocode_dir).expect("pseudocode directory");
    std::fs::create_dir_all(&ir_dir).expect("ir directory");
    for (artifact, ir) in artifacts.iter().zip(&functions) {
        std::fs::write(
            pseudocode_dir.join(format!(
                "{:05}_{}.dartpseudo",
                artifact.function_id, artifact.function_name
            )),
            format!("{}\n", artifact.source),
        )
        .expect("emitted pseudocode");
        std::fs::write(
            ir_dir.join(format!(
                "{:05}_{}.json",
                artifact.function_id, artifact.function_name
            )),
            serde_json::to_vec(ir).expect("serialisable IR"),
        )
        .expect("emitted IR");
    }

    let (clean, report) = reconcile(&audit, &pseudocode_dir, &ir_dir);
    assert!(clean, "the honest audit must reconcile:\n{report}\n{text}");

    // The join site's own output-anchor checker, unmodified: it re-reads every
    // join record's coordinate out of the artifact and re-derives the rendered
    // candidate list and the predecessor coverage from the IR.
    let anchors = Command::new("python3")
        .arg(script("prov_join_output_anchor_check.py"))
        .arg(&audit)
        .arg("--pseudocode")
        .arg(&pseudocode_dir)
        .arg("--ir")
        .arg(&ir_dir)
        .output()
        .expect("python3 available");
    assert!(
        anchors.status.success(),
        "the honest audit must pass the join anchor check:\n{}{}",
        String::from_utf8_lossy(&anchors.stdout),
        String::from_utf8_lossy(&anchors.stderr)
    );

    // One field wrong per plant, each a defect the contract names. A plant that
    // the reconciler accepted would mean the clean pass above proves nothing.
    let annotation_rows: Vec<String> = rows
        .iter()
        .filter(|row| row.contains("\"record\":\"annotation\""))
        .map(|row| (*row).to_string())
        .collect();
    let join_row = annotation_rows
        .iter()
        .find(|row| row.contains("\"loss_site\":\"join\""))
        .expect("the join fixture produced a record")
        .clone();
    let plants: Vec<(&str, String)> = vec![
        (
            // Wrong path: the value is attributed to a block that is not a
            // predecessor of the join at all, so no path could have carried it.
            "wrong path",
            text.replace(
                &join_row,
                &join_row.replace("\"path_key\":[\"block\",1]", "\"path_key\":[\"block\",0]"),
            ),
        ),
        (
            // Wrong value: the record claims a value the artifact does not show.
            "wrong value",
            text.replace(
                &join_row,
                &join_row.replace("\"value\":\"7\"", "\"value\":\"5\""),
            ),
        ),
        (
            // Wrong site: the record names another block as the site of a loss
            // that happened at this one.
            "wrong site",
            text.replace(
                &join_row,
                &join_row.replace("\"site_key\":[\"join\",3]", "\"site_key\":[\"join\",0]"),
            ),
        ),
        (
            // Duplicate: two records claim one annotation, which is what a
            // placement that can redirect produces when both records find the
            // same span.
            "duplicate record",
            format!("{text}{join_row}\n"),
        ),
        (
            // Dropped anchor: the record keeps its site and its value and points
            // at a line that carries no annotation at all.
            "dropped anchor",
            text.replace(
                &join_row,
                &join_row.replace("\"output_col\":", "\"output_col\":1,\"unused_col\":"),
            ),
        ),
    ];
    for (name, planted) in plants {
        assert_ne!(planted, text, "the {name} plant must change the audit");
        let path = dir.join(format!("planted-{}.jsonl", name.replace(' ', "-")));
        std::fs::write(&path, &planted).expect("planted audit");
        let (accepted, report) = reconcile(&path, &pseudocode_dir, &ir_dir);
        assert!(
            !accepted,
            "the reconciler must reject the {name} plant:\n{report}"
        );
    }
}
