// Annotation anchors resolve to the exact line they were rendered on, and every
// candidate ends with an outcome.
//
// Every fixture here plants the same trap: a line somewhere else in the function
// that reads byte for byte like the line an annotation belongs on. Text cannot
// tell those two apart, so a placement that consults text lands on the wrong one.
// The checks are written to fail when that happens rather than to confirm that
// some annotation appeared somewhere - which the identical decoy line would have
// satisfied just as well.

use super::*;
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

/// The line both the decoy and the real site render, before annotation.
const SHARED_LINE: &str = "  reg19.f8 = reg9;";

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

/// Emit one function through the production entry point and hand back the
/// artifact beside the per-site provenance the run produced.
fn emit(ir: &FunctionIr) -> (PseudocodeArtifact, Vec<FunctionProvenance>) {
    crate::emit_one(
        ir,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
}

/// Zero-based indexes of the artifact lines carrying an annotation of `literal`.
fn annotated_lines(source: &str, literal: &AnnotationLiteral) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(literal.open()))
        .map(|(index, _)| index)
        .collect()
}

/// Zero-based indexes of the artifact lines that are the shared text, annotated
/// or not. An annotation is inserted inside the line, so the match is on the
/// text up to the point an annotation can be inserted at.
fn shared_text_lines(source: &str) -> Vec<usize> {
    let stem = SHARED_LINE.trim().trim_end_matches(';');
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with(stem))
        .map(|(index, _)| index)
        .collect()
}

fn site<'a>(provenance: &'a [FunctionProvenance], loss_site: &str) -> &'a FunctionProvenance {
    provenance
        .iter()
        .find(|stream| stream.loss_site == loss_site)
        .expect("every run reports all three sites")
}

/// Two arms binding `x9` to different values and a join that reads it, behind an
/// entry read of `x9` that renders the identical line.
fn join_with_a_duplicate_line() -> FunctionIr {
    FunctionIr {
        function_id: 0x7001,
        name: "joinDuplicate".to_string(),
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
fn loop_entry_with_a_duplicate_line() -> FunctionIr {
    FunctionIr {
        function_id: 0x7002,
        name: "loopEntryDuplicate".to_string(),
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

/// A call clobbering `x9` while it holds a recorded value and an unresolved read
/// after it, behind an identical earlier line whose `x9` no call had touched.
fn call_clobber_with_a_duplicate_line() -> FunctionIr {
    FunctionIr {
        function_id: 0x7003,
        name: "callDuplicate".to_string(),
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

/// A read of a clobbered `x9` on a line compaction removes: both arms of the
/// guard return the same statement, so the guard and its arms collapse into one
/// return and the line the anchor was captured on is gone from the artifact.
fn call_clobber_on_a_line_a_rewrite_removes() -> FunctionIr {
    FunctionIr {
        function_id: 0x7004,
        name: "droppedAnchor".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(
                0,
                0x1000,
                vec![
                    stmt(0x1000, "ldur x9, [x1, #7]"),
                    call_to(0x1004, 0x9000),
                    cbz(0x1008, "x9", 0x2000),
                ],
                vec![1, 2],
            ),
            blk(1, 0x100c, vec![ret(0x100c)], Vec::new()),
            blk(2, 0x2000, vec![ret(0x2000)], Vec::new()),
        ],
    }
}

/// Two calls clobbering `x9` while it holds the same value, each followed by an
/// identical unresolved read: two annotations whose text is byte-identical, on
/// two lines that are byte-identical.
fn two_calls_producing_identical_annotations() -> FunctionIr {
    FunctionIr {
        function_id: 0x7005,
        name: "twinAnnotations".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "mov x9, #5"),
                call_to(0x1004, 0x9000),
                stmt(0x1008, "stur x9, [x19, #7]"),
                stmt(0x100c, "mov x9, #5"),
                call_to(0x1010, 0x9000),
                stmt(0x1014, "stur x9, [x19, #7]"),
                ret(0x1018),
            ],
            Vec::new(),
        )],
    }
}

/// The join's own line carries the annotation; the identical line the entry
/// block rendered does not.
///
/// The decoy is the first of the two, so a placement that searches the finished
/// body from the top takes it - which is what the join site did, and what no
/// audit check could see: a record naming block 3 sat on a coordinate inside
/// block 0 while every field in the record was true of block 3.
#[test]
fn a_join_annotation_lands_in_its_own_block_and_not_on_an_identical_earlier_line() {
    let (artifact, provenance) = emit(&join_with_a_duplicate_line());
    let shared = shared_text_lines(&artifact.source);
    assert_eq!(
        shared.len(),
        2,
        "the fixture must render the decoy and the join line identically:\n{}",
        artifact.source
    );
    assert_eq!(
        annotated_lines(&artifact.source, &EXHAUSTIVE_JOIN_ANNOTATION),
        vec![shared[1]],
        "the annotation belongs to the join block's line, the second of the two:\n{}",
        artifact.source
    );

    let join = site(&provenance, JOIN_LOSS_SITE);
    assert_eq!(join.records.len(), 1, "one record per emitted annotation");
    assert_eq!(
        join.records[0].output_line, shared[1],
        "the record must name the line its annotation is on:\n{}",
        artifact.source
    );
    assert_eq!(join.records[0].site_key, SiteKey(JOIN_LOSS_SITE, 3));
}

/// The loop header's own line carries the annotation, not the identical line
/// before the loop.
#[test]
fn a_loop_entry_annotation_lands_on_the_header_and_not_on_an_identical_earlier_line() {
    let (artifact, provenance) = emit(&loop_entry_with_a_duplicate_line());
    let shared = shared_text_lines(&artifact.source);
    assert_eq!(
        shared.len(),
        2,
        "the fixture must render the decoy and the header line identically:\n{}",
        artifact.source
    );
    let annotated = annotated_lines(&artifact.source, &LOOP_ENTRY_ANNOTATION);
    assert_eq!(
        annotated,
        vec![shared[1]],
        "the annotation belongs to the loop header's line:\n{}",
        artifact.source
    );
    // The decoy is outside the loop and the header's line is inside it, so a
    // redirect also moves the annotation out of the loop it describes.
    let opener = artifact
        .source
        .lines()
        .position(|line| line.trim() == "while (true) {")
        .expect("the fixture emits a loop");
    assert!(
        annotated[0] > opener && shared[0] < opener,
        "the decoy precedes the loop and the annotated line is inside it:\n{}",
        artifact.source
    );

    let loops = site(&provenance, LOOP_LOSS_SITE);
    assert_eq!(loops.records.len(), 1, "one record per emitted annotation");
    assert_eq!(
        loops.records[0].output_line, annotated[0],
        "the record must name the line its annotation is on:\n{}",
        artifact.source
    );
}

/// The read after the call carries the annotation; the identical read before it,
/// which no call had clobbered, does not.
#[test]
fn a_call_annotation_lands_on_the_read_after_its_own_call() {
    let (artifact, _) = emit(&call_clobber_with_a_duplicate_line());
    let shared = shared_text_lines(&artifact.source);
    assert_eq!(
        shared.len(),
        2,
        "the fixture must render the decoy and the clobbered read identically:\n{}",
        artifact.source
    );
    let call_line = artifact
        .source
        .lines()
        .position(|line| line.contains("fn_0x9000()"))
        .expect("the fixture emits the call");
    let annotated = annotated_lines(&artifact.source, &PRE_CALL_ANNOTATION);
    assert_eq!(
        annotated,
        vec![shared[1]],
        "the annotation belongs to the read after the call:\n{}",
        artifact.source
    );
    assert!(
        shared[0] < call_line && annotated[0] > call_line,
        "the decoy read precedes the call and the annotated read follows it:\n{}",
        artifact.source
    );
}

/// Two byte-identical annotations on two byte-identical lines stay on their own
/// lines, so neither can be read as a second value for the other's register.
#[test]
fn identical_annotations_on_identical_lines_keep_separate_lines() {
    let (artifact, _) = emit(&two_calls_producing_identical_annotations());
    let annotated = annotated_lines(&artifact.source, &PRE_CALL_ANNOTATION);
    assert_eq!(
        annotated.len(),
        2,
        "each clobbered read is annotated:\n{}",
        artifact.source
    );
    let first = artifact.source.lines().nth(annotated[0]).expect("line");
    let second = artifact.source.lines().nth(annotated[1]).expect("line");
    assert_eq!(
        first, second,
        "this is only a test of identity if the two lines read the same:\n{}",
        artifact.source
    );
}

/// A candidate whose line no pass left intact is rejected with a reason, and
/// annotates nothing.
///
/// Both halves matter. Without the row the candidate leaves the ledger with no
/// outcome at all; without the empty-artifact check, "rejected" could still mean
/// the value was written somewhere else first.
#[test]
fn a_candidate_whose_line_a_rewrite_removed_is_rejected_rather_than_moved() {
    let (artifact, provenance) = emit(&call_clobber_on_a_line_a_rewrite_removes());
    assert!(
        annotated_lines(&artifact.source, &PRE_CALL_ANNOTATION).is_empty(),
        "the read the value described is gone, so nothing may carry it:\n{}",
        artifact.source
    );
    let calls = site(&provenance, CALL_LOSS_SITE);
    assert_eq!(
        calls.candidates_considered, 1,
        "the fixture must reach the annotation pass with one candidate"
    );
    assert_eq!(
        calls
            .filter_rejections
            .iter()
            .map(|rejection| rejection.reason)
            .collect::<Vec<_>>(),
        vec!["anchor_line_dropped"],
        "the drop is recorded with the reason it happened for"
    );
    assert_eq!(calls.unaccounted_candidates(), 0);
}

/// Every candidate every site considered ends as an emitted annotation, a
/// recorded rejection, or a recorded budget drop.
///
/// Non-vacuous by construction: a site that considered nothing reconciles
/// trivially, so each fixture also asserts that its own site was reached.
#[test]
fn every_candidate_ends_with_a_recorded_outcome() {
    let cases: Vec<(&str, FunctionIr, &str)> = vec![
        ("join", join_with_a_duplicate_line(), JOIN_LOSS_SITE),
        ("loop", loop_entry_with_a_duplicate_line(), LOOP_LOSS_SITE),
        ("call", call_clobber_with_a_duplicate_line(), CALL_LOSS_SITE),
        (
            "dropped",
            call_clobber_on_a_line_a_rewrite_removes(),
            CALL_LOSS_SITE,
        ),
        (
            "twins",
            two_calls_producing_identical_annotations(),
            CALL_LOSS_SITE,
        ),
    ];
    for (name, ir, expected_site) in cases {
        let (_, provenance) = emit(&ir);
        assert!(
            site(&provenance, expected_site).candidates_considered > 0,
            "{name}: the fixture must reach the {expected_site} site's gates"
        );
        for stream in &provenance {
            assert_eq!(
                stream.unaccounted_candidates(),
                0,
                "{name}: {} considered {} candidates and accounted for {}",
                stream.loss_site,
                stream.candidates_considered,
                stream.accounted_candidates()
            );
        }
    }
}

/// Every record's line index is a line of the artifact, and that line carries
/// the record's own annotation text.
#[test]
fn every_record_names_a_line_that_carries_its_annotation() {
    let mut records = 0usize;
    for ir in [
        join_with_a_duplicate_line(),
        loop_entry_with_a_duplicate_line(),
    ] {
        let (artifact, provenance) = emit(&ir);
        let lines: Vec<&str> = artifact.source.lines().collect();
        for stream in &provenance {
            assert_eq!(
                stream.records.len(),
                stream.annotations_emitted,
                "{}: one record per emitted annotation, no more and no fewer",
                stream.loss_site
            );
            for record in &stream.records {
                records += 1;
                let line = lines
                    .get(record.output_line)
                    .unwrap_or_else(|| panic!("record names line {}", record.output_line));
                assert!(
                    line.contains(&record.rendered),
                    "the record's line must carry its annotation: {line:?} against {:?}",
                    record.rendered
                );
            }
        }
    }
    assert!(records > 0, "the fixtures must produce records");
}
