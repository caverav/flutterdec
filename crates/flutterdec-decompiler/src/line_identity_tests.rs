use super::*;
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn ir() -> FunctionIr {
    FunctionIr {
        function_id: 0x9100,
        name: "lineIdentity".to_string(),
        entry_va: 0x1000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x1000,
            instrs: vec![LlirInstr {
                va: 0x1000,
                op: IROp::Return,
                src: "ret".to_string(),
                target: String::new(),
            }],
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    }
}

fn three_twins() -> (FunctionIr, HashMap<u64, String>) {
    (ir(), HashMap::new())
}

fn assert_middle_survives(mutate: impl FnOnce(&mut FuncEmitter<'_>), expected: Option<usize>) {
    let (ir, symbols) = three_twins();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    for _ in 0..3 {
        emitter.push_body_line("  identical();".to_string());
    }
    emitter.render_lines = emitter.lines.clone();
    emitter.render_line_ids = emitter.line_ids.clone();
    mutate(&mut emitter);
    let placed = emitter.finished_line_positions();
    assert_eq!(emitter.finished_line_of_render(1, &placed), expected);
}

#[test]
fn three_identical_lines_keep_the_exact_surviving_anchor_through_mutations() {
    assert_middle_survives(
        |emitter| emitter.insert_body_line(0, "  inserted();".to_string()),
        Some(2),
    );
    assert_middle_survives(
        |emitter| {
            emitter.splice_body_lines(0, vec!["  first();".to_string(), "  second();".to_string()]);
        },
        Some(3),
    );
    assert_middle_survives(
        |emitter| {
            emitter.replace_body_line(0, vec!["  first();".to_string(), "  second();".to_string()]);
        },
        Some(2),
    );
    assert_middle_survives(|emitter| emitter.drain_body_lines(0..=0), Some(0));
    assert_middle_survives(
        |emitter| emitter.replace_body_line(1, vec!["  replacement();".to_string()]),
        None,
    );
    assert_middle_survives(|emitter| emitter.drain_body_lines(1..=1), None);
}

fn partial_mismatch_panics(mutate: impl FnOnce(&mut FuncEmitter<'_>)) {
    let ir = ir();
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.push_body_line("first".to_string());
    emitter.push_body_line("second".to_string());
    emitter.line_ids.pop();
    assert!(
        catch_unwind(AssertUnwindSafe(|| mutate(&mut emitter))).is_err(),
        "a nonempty partial identity mismatch must fail loudly"
    );
}

#[test]
fn every_length_changing_helper_rejects_a_partial_identity_mismatch() {
    partial_mismatch_panics(|emitter| emitter.insert_body_line(0, "new".to_string()));
    partial_mismatch_panics(|emitter| emitter.splice_body_lines(0, vec!["new".to_string()]));
    partial_mismatch_panics(|emitter| emitter.replace_body_line(0, vec!["new".to_string()]));
    partial_mismatch_panics(|emitter| emitter.drain_body_lines(0..=0));
    partial_mismatch_panics(|emitter| emitter.sync_line_ids());
}

#[test]
fn an_identity_free_fixture_stays_explicit_until_synchronization() {
    let ir = ir();
    let symbols = HashMap::new();
    let mut emitter = FuncEmitter::new(&ir, &symbols);
    emitter.lines.push("first".to_string());
    emitter.insert_body_line(0, "inserted".to_string());
    emitter.splice_body_lines(2, vec!["last".to_string()]);
    assert!(emitter.line_ids.is_empty());

    emitter.sync_line_ids();
    assert_eq!(emitter.line_ids.len(), emitter.lines.len());
}
