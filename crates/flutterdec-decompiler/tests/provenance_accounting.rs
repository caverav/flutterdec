use flutterdec_decompiler::emit_program;
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use serde_json::Value;
use std::collections::HashMap;

fn stmt(va: u64, src: &str) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Other,
        src: src.to_string(),
        target: String::new(),
    }
}

fn call(va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Call,
        src: "bl #0x9000".to_string(),
        target: "#0x9000".to_string(),
    }
}

fn branch(va: u64, target: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Branch,
        src: format!("cbz x9, #0x{target:x}"),
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

fn block(id: usize, va: u64, instrs: Vec<LlirInstr>, succs: Vec<usize>) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs,
        succs,
        preds: Vec::new(),
    }
}

fn accepted() -> FunctionIr {
    FunctionIr {
        function_id: 0x9200,
        name: "acceptedAccounting".to_string(),
        entry_va: 0x1000,
        blocks: vec![block(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldur x9, [x1, #7]"),
                call(0x1004),
                stmt(0x1008, "stur x9, [x19, #7]"),
                ret(0x100c),
            ],
            Vec::new(),
        )],
    }
}

fn rejection_only() -> FunctionIr {
    FunctionIr {
        function_id: 0x9201,
        name: "rejectionOnlyAccounting".to_string(),
        entry_va: 0x2000,
        blocks: vec![
            block(
                0,
                0x2000,
                vec![
                    stmt(0x2000, "ldur x9, [x1, #7]"),
                    call(0x2004),
                    branch(0x2008, 0x3000),
                ],
                vec![1, 2],
            ),
            block(1, 0x200c, vec![ret(0x200c)], Vec::new()),
            block(2, 0x3000, vec![ret(0x3000)], Vec::new()),
        ],
    }
}

fn number(row: &Value, field: &str) -> u64 {
    row[field]
        .as_u64()
        .unwrap_or_else(|| panic!("{field} must be an unsigned count in {row}"))
}

#[test]
fn release_audit_accounts_for_accepted_and_rejection_only_streams() {
    let dir = std::env::temp_dir().join(format!(
        "flutterdec-provenance-accounting-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let audit = dir.join("audit.jsonl");
    std::env::set_var("FLUTTERDEC_PROV_AUDIT", &audit);
    std::env::set_var("FLUTTERDEC_PROV_SAMPLE", "release-accounting");

    emit_program(&[accepted(), rejection_only()], &HashMap::new());

    let rows: Vec<Value> = std::fs::read_to_string(&audit)
        .expect("release audit exists")
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid JSONL row"))
        .collect();
    assert!(rows.iter().all(|row| row["schema_version"] == 2));

    for function_id in [0x9200, 0x9201] {
        assert!(rows.iter().any(|row| {
            row["record"] == "snapshot" && number(row, "function_id") == function_id
        }));
    }

    let accepted_row = rows
        .iter()
        .find(|row| row["record"] == "annotation" && number(row, "function_id") == 0x9200)
        .expect("the accepted-only stream is represented by its accepted row");
    assert_eq!(number(accepted_row, "candidates_considered"), 1);
    assert_eq!(number(accepted_row, "accepted"), 1);
    assert_eq!(number(accepted_row, "rejected"), 0);
    assert_eq!(accepted_row["unaccounted_candidates"], 0);

    let summary = rows
        .iter()
        .find(|row| {
            row["record"] == "cap_summary"
                && number(row, "function_id") == 0x9201
                && row["loss_site"] == "call"
        })
        .expect("the rejection-only function must publish its summary");
    assert_eq!(number(summary, "candidates_considered"), 1);
    assert_eq!(number(summary, "accepted"), 0);
    assert_eq!(number(summary, "rejected"), 1);
    assert_eq!(
        number(summary, "accepted") + number(summary, "rejected"),
        number(summary, "candidates_considered")
    );
    assert_eq!(summary["unaccounted_candidates"], 0);

    let rejections: Vec<&Value> = rows
        .iter()
        .filter(|row| row["record"] == "filter_rejection")
        .collect();
    assert_eq!(rejections.len(), 1);
    assert_eq!(number(rejections[0], "function_id"), 0x9201);
    assert_eq!(rejections[0]["reason"], "anchor_line_dropped");
    assert!(rejections[0]["reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty()));
}
