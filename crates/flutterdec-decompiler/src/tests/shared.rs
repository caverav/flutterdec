use super::*;
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::fs;
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("golden")
        .join(name)
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var("FLUTTERDEC_UPDATE_GOLDEN")
        .ok()
        .as_deref()
        == Some("1")
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create golden directory");
        }
        fs::write(&path, format!("{}\n", actual.trim_end()))
            .expect("failed to update golden snapshot");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden snapshot at {} ({e})",
            path.display()
        )
    });
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "golden mismatch for {} (set FLUTTERDEC_UPDATE_GOLDEN=1 to refresh)",
        path.display()
    );
}

fn branch_block(id: usize, va: u64, true_va: u64, false_id: usize, true_id: usize) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs: vec![LlirInstr {
            va,
            op: IROp::Branch,
            src: format!("cbz x0, #0x{true_va:x}"),
            target: format!("#0x{true_va:x}"),
        }],
        succs: vec![true_id, false_id],
        preds: Vec::new(),
    }
}

fn jump_block(id: usize, va: u64, to_id: usize, to_va: u64) -> BasicBlock {
    BasicBlock {
        id,
        start_va: va,
        instrs: vec![LlirInstr {
            va,
            op: IROp::Jump,
            src: format!("b #0x{to_va:x}"),
            target: format!("#0x{to_va:x}"),
        }],
        succs: vec![to_id],
        preds: Vec::new(),
    }
}

