// Capture and attribution at a join with more than two incoming paths, and the
// audit rows that prove where each rendered value came from.
//
// Every fixture here drives the real emitter - `try_emit_structured` for the
// capture state, `emit_pseudocode` for the rendered text - rather than handing
// `record_join_candidates` a fabricated snapshot list. The defect these replace
// was invisible to a hand-built list: capture derived its candidates from the
// branch's two arms, so a third incoming path was skipped whole and no list built
// from two arms could show it.

/// Predecessors of `block`, reconstructed from the IR's own successor edges.
///
/// Deliberately not `Regions::predecessors`, which is what capture itself uses: an
/// oracle sharing the traversal it checks agrees with the implementation by
/// construction, including when both are wrong.
fn predecessors_from_ir(ir: &FunctionIr, block: usize) -> Vec<usize> {
    let mut preds: Vec<usize> = ir
        .blocks
        .iter()
        .filter(|candidate| candidate.succs.contains(&block))
        .map(|candidate| candidate.id)
        .collect();
    preds.sort_unstable();
    preds
}

/// A diamond with a third route into the join: block 1 branches straight to it as
/// well as through block 2, so blocks 1, 2 and 3 all reach block 4.
///
/// `third` is the value block 2 leaves in `x0`, which is what separates the
/// all-covered case from the duplicate-value one.
fn three_predecessor_ir(function_id: u64, name: &str, third: &str) -> FunctionIr {
    FunctionIr {
        function_id,
        name: name.to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x1030)], vec![1, 3]),
            blk(
                1,
                0x1010,
                vec![stmt(0x1010, "mov x0, #7"), cbz(0x1014, "x2", 0x1040)],
                vec![2, 4],
            ),
            blk(2, 0x1020, vec![stmt(0x1020, third)], vec![4]),
            blk(3, 0x1030, vec![stmt(0x1030, "mov x0, #9")], vec![4]),
            blk(
                4,
                0x1040,
                vec![stmt(0x1040, "stur x0, [x29, #-0x10]"), ret(0x1044)],
                Vec::new(),
            ),
        ],
    }
}

/// The capture state of a structured render, without the surrounding artifact.
fn captured(ir: &FunctionIr) -> FuncEmitter<'_> {
    let symbols = Box::leak(Box::new(HashMap::new()));
    let mut emitter = FuncEmitter::new(ir, symbols);
    assert!(
        emitter.try_emit_structured(),
        "the fixture must structure, or capture never runs"
    );
    emitter
}

#[test]
fn captures_a_candidate_from_every_predecessor_of_a_three_predecessor_join() {
    let ir = three_predecessor_ir(1040, "threeWayJoin", "mov x0, #8");
    let preds = predecessors_from_ir(&ir, 4);
    assert_eq!(
        preds,
        vec![1, 2, 3],
        "the fixture's join must have three incoming paths"
    );

    let emitter = captured(&ir);
    let candidates = emitter
        .join_candidates
        .get(&(4, "x0".to_string()))
        .expect("a register the join drops must be captured");

    let attributed: Vec<usize> = candidates
        .provenance
        .iter()
        .map(|candidate| candidate.pred)
        .collect();
    assert_eq!(
        attributed, preds,
        "every predecessor that carried a value must appear as an attributed candidate"
    );
    assert_eq!(
        candidates
            .provenance
            .iter()
            .map(|candidate| candidate.value.as_str())
            .collect::<Vec<_>>(),
        vec!["7", "8", "9"],
        "each candidate must carry its own predecessor's value, in ascending predecessor id"
    );
    assert!(
        candidates
            .provenance
            .iter()
            .all(|candidate| !candidate.snapshot_id.is_empty()),
        "no candidate may be recorded without the snapshot it came from"
    );
    assert!(
        candidates.complete,
        "every predecessor contributed, so the claim is exhaustive"
    );

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains(&EXHAUSTIVE_JOIN_ANNOTATION.render(&["7", "8", "9"])),
        "a fully covered three-predecessor join renders three values through the exhaustive literal:\n{}",
        artifact.source
    );
}

#[test]
fn dedupes_a_repeated_value_in_output_and_keeps_both_attributions() {
    // Blocks 2 and 3 leave the same value, so the rendered list is shorter than
    // the attribution list.
    let ir = three_predecessor_ir(1041, "duplicateJoinValue", "mov x0, #9");
    assert_eq!(predecessors_from_ir(&ir, 4), vec![1, 2, 3]);

    let emitter = captured(&ir);
    let candidates = emitter
        .join_candidates
        .get(&(4, "x0".to_string()))
        .expect("a register the join drops must be captured");

    assert_eq!(
        candidates
            .provenance
            .iter()
            .map(|candidate| (candidate.pred, candidate.value.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "7"), (2, "9"), (3, "9")],
        "both predecessors carrying the repeated value keep their own attribution"
    );
    assert_eq!(
        candidates.values,
        vec!["7".to_string(), "9".to_string()],
        "the rendered list collapses equal values by first occurrence"
    );
    assert!(
        candidates.complete,
        "coverage decides the form, not rendered arity: three covered predecessors stay exhaustive"
    );

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact.source.contains(&EXHAUSTIVE_JOIN_ANNOTATION.render(&["7", "9"])),
        "the collapsed list still renders through the exhaustive literal:\n{}",
        artifact.source
    );
}

#[test]
fn renders_non_exhaustive_when_one_predecessor_contributed_nothing_usable() {
    // Block 2 overwrites `x0` with an unrecovered register value, which is
    // rejected as evidence, so that path contributes no usable candidate while
    // still being a real predecessor.
    let ir = three_predecessor_ir(1042, "uncoveredJoinPath", "mov x0, x9");
    assert_eq!(predecessors_from_ir(&ir, 4), vec![1, 2, 3]);

    let emitter = captured(&ir);
    let candidates = emitter
        .join_candidates
        .get(&(4, "x0".to_string()))
        .expect("the remaining paths still carry evidence");
    assert_eq!(
        candidates
            .provenance
            .iter()
            .map(|candidate| candidate.pred)
            .collect::<Vec<_>>(),
        vec![1, 3],
        "the uncovered predecessor contributes no attribution"
    );
    assert!(
        !candidates.complete,
        "one uncovered predecessor makes the list evidence, not an exhaustive claim"
    );

    let artifact = emit_pseudocode(&ir, &HashMap::new());
    assert!(
        artifact
            .source
            .contains(&NON_EXHAUSTIVE_JOIN_ANNOTATION.render(&["7", "9"])),
        "an uncovered predecessor must render the non-exhaustive literal:\n{}",
        artifact.source
    );
    assert!(
        !artifact
            .source
            .contains(EXHAUSTIVE_JOIN_ANNOTATION.open()),
        "no exhaustive claim may survive an uncovered predecessor:\n{}",
        artifact.source
    );
}

#[test]
fn records_one_audit_row_per_annotation_with_a_resolvable_snapshot_per_candidate() {
    let ir = three_predecessor_ir(1043, "auditedJoin", "mov x0, #8");
    let emitter = captured(&ir);
    let mut emitter = emitter;
    emitter.append_join_annotations();

    let records = &emitter.join_provenance.records;
    assert_eq!(
        records.len(),
        1,
        "one row per emitted annotation, not one per candidate: three rows at one \
         output coordinate is a double claim"
    );
    let record = &records[0];
    assert_eq!(record.loss_site, "join");
    assert_eq!(
        record.site_key,
        SiteKey("join", 4),
        "the site key is annotation-level and tagged, and carries no predecessor id"
    );
    assert_eq!(
        record.anchor,
        SiteKey("block", 4),
        "and the rendering anchor it was read off is recorded in IR terms, so a \
         reader can resolve the block itself instead of trusting the label"
    );
    assert_eq!(record.register, "x0");
    assert_eq!(
        record
            .candidates
            .iter()
            .map(|candidate| (candidate.path_key.clone(), candidate.value.clone()))
            .collect::<Vec<_>>(),
        vec![
            (SiteKey("block", 1), "7".to_string()),
            (SiteKey("block", 2), "8".to_string()),
            (SiteKey("block", 3), "9".to_string()),
        ],
        "each candidate names the incoming path its own value came from"
    );

    // The audit's own falsifier: every attribution must be findable in the
    // snapshot it cites, under its own register.
    for candidate in &record.candidates {
        let snapshot = emitter
            .join_provenance
            .snapshots
            .iter()
            .find(|snapshot| snapshot.snapshot_id == candidate.snapshot_id)
            .unwrap_or_else(|| panic!("cited snapshot {} is recorded", candidate.snapshot_id));
        assert_eq!(
            snapshot.site_key, candidate.path_key,
            "a snapshot must name the predecessor path its values were read from, \
             or a value borrowed from a sibling path is checked against a key that \
             agrees with itself"
        );
        assert!(
            snapshot
                .registers
                .iter()
                .any(|(reg, value)| reg == &record.register && value == &candidate.value),
            "{} claims a value its own snapshot does not hold",
            candidate.value
        );
    }
}

/// A natural loop header reached from both arms of a branch is a join by
/// predecessor count and a loop site by semantics. Capturing it here would put a
/// join-tagged row at a coordinate the loop site also claims.
#[test]
fn declines_a_loop_header_that_is_also_a_join() {
    let ir = FunctionIr {
        function_id: 1044,
        name: "loopHeaderJoin".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x1020)], vec![1, 2]),
            blk(1, 0x1010, vec![stmt(0x1010, "mov x0, #7")], vec![3]),
            blk(2, 0x1020, vec![stmt(0x1020, "mov x0, #9")], vec![3]),
            // Loop header: both arms plus the back edge from block 4.
            blk(
                3,
                0x1030,
                vec![
                    stmt(0x1030, "stur x0, [x29, #-0x10]"),
                    cbz(0x1034, "x2", 0x1050),
                ],
                vec![4, 5],
            ),
            blk(4, 0x1040, vec![stmt(0x1040, "mov x0, #11")], vec![3]),
            blk(5, 0x1050, vec![ret(0x1050)], Vec::new()),
        ],
    };
    assert_eq!(
        predecessors_from_ir(&ir, 3),
        vec![1, 2, 4],
        "the fixture's header must be reached from both arms and its back edge"
    );

    let mut emitter = captured(&ir);
    let regions = emitter.regions.as_ref().expect("regions");
    assert!(regions.is_loop_header(3), "block 3 must be a natural loop header");
    assert!(regions.is_join(3), "and a join by predecessor count");

    emitter.append_join_annotations();
    assert!(
        emitter
            .join_provenance
            .records
            .iter()
            .all(|record| record.site_key != SiteKey("join", 3)),
        "a loop header belongs to the loop site, so no join-tagged row may claim it"
    );
    assert!(
        emitter.join_provenance.snapshots.is_empty(),
        "and the join stream records no snapshot for it either"
    );
}

/// A candidate whose own text contains the separator is rejected before it can be
/// rendered.
///
/// Not a style rule: `smiTag((arg0 | 1))` is a real value on this corpus, and one
/// candidate spelling it renders to the same bytes as two candidates `smiTag((arg0`
/// and `1))`. Every check that compares rendered values against recorded ones then
/// has to guess the arity, and so does the reader.
#[test]
fn rejects_a_candidate_that_contains_the_separator() {
    let ambiguous = "smiTag((arg0 | 1))";
    assert!(
        !crate::control_flow::is_recordable_annotation_candidate(ambiguous),
        "a value containing the separator must not be recordable"
    );
    assert_eq!(
        EXHAUSTIVE_JOIN_ANNOTATION.render(&[ambiguous]),
        EXHAUSTIVE_JOIN_ANNOTATION.render(&["smiTag((arg0", "1))"]),
        "one such value and two values are the same bytes, which is why it is rejected"
    );
    assert!(
        crate::control_flow::is_recordable_annotation_candidate("smiTag((arg0 + 1))"),
        "an ordinary expression is still recordable"
    );
}
