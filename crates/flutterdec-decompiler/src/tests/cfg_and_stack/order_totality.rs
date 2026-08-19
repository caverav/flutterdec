// Order totality for the value-annotation work: every ordering it introduces is
// total over a named input, and every test here is the mutation evidence that
// the tie-break carrying it is load-bearing.
//
// Two cold runs agreeing is corroboration, not proof. R24 measured that an
// unfixed partial order over a hash-derived sequence passes a single A/B
// comparison about two thirds of the time on this branch, so a byte-identity
// pair cannot settle the question on its own. What settles it is whether the
// comparator is total over the sequence it sorts, which is what these vary.
//
// The input that varies in production is `HashSet`/`HashMap` iteration order,
// seeded per process. It is also seeded per *instance* - `RandomState::new`
// advances a thread-local counter for every map built - so emitting one
// function repeatedly in a single process reproduces exactly the variation two
// cold processes see. `assert_hash_order_varies` states that premise as an
// assertion rather than relying on it, because a repetition test over a
// sequence that happened not to vary would pass while proving nothing.

/// Fail unless fresh `HashSet`s over `regs` really do iterate differently within
/// this process, which is the premise every repetition test below rests on.
fn assert_hash_order_varies(regs: &[&str], rounds: usize) {
    let orders: Vec<Vec<String>> = (0..rounds)
        .map(|_| {
            regs.iter()
                .map(|reg| (*reg).to_string())
                .collect::<HashSet<String>>()
                .into_iter()
                .collect()
        })
        .collect();
    assert!(
        orders.iter().any(|order| order != &orders[0]),
        "the premise failed: {rounds} fresh HashSets over {regs:?} all iterated \
         identically, so a repetition test proves nothing about hash order"
    );
}

/// A join whose incoming paths cover *different* registers, so the order the
/// dropped registers are visited in is observable.
///
/// Disjoint coverage is the point. If both predecessors carried every register,
/// the first register visited would record both snapshots and the rest would
/// find them already present, so any visit order would produce the same audit.
/// Here block 1 carries x0 and x2 and block 2 carries x3 and x5, so the audit's
/// snapshot order is decided by which register is visited first.
///
/// `stur x0, [x2, #0x10]` is deliberate as well: it reads two dropped registers
/// on one rendered line, which is the only shape in which the insertion sort's
/// offset component can matter.
fn disjoint_coverage_join_ir(function_id: u64) -> FunctionIr {
    FunctionIr {
        function_id,
        name: "disjointCoverageJoin".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x1", 0x1020)], vec![1, 2]),
            blk(
                1,
                0x1010,
                vec![stmt(0x1010, "mov x0, #7"), stmt(0x1014, "mov x2, #8")],
                vec![3],
            ),
            blk(
                2,
                0x1020,
                vec![stmt(0x1020, "mov x3, #9"), stmt(0x1024, "mov x5, #11")],
                vec![3],
            ),
            blk(
                3,
                0x1030,
                vec![
                    stmt(0x1030, "stur x0, [x2, #0x10]"),
                    stmt(0x1034, "stur x3, [x29, #-0x18]"),
                    stmt(0x1038, "stur x5, [x29, #-0x20]"),
                    ret(0x103c),
                ],
                Vec::new(),
            ),
        ],
    }
}

/// A loop header reached from two entry arms that carry *different* registers,
/// plus a body that rewrites all four so the header drops them.
///
/// Two entry arms rather than one, for the same reason the join fixture uses
/// disjoint coverage: with a single entry predecessor every candidate cites the
/// same snapshot, so no register visit order can be told from another and the
/// fixture would pass with the sort deleted. Block 1 carries x0 and x2 and
/// block 2 carries x3 and x5, so which register is visited first decides which
/// snapshot the audit records first.
///
/// The header is a join by predecessor count as well, and the join capture
/// declines it, so what is exercised here is the loop site's own capture.
fn loop_entry_multi_register_ir(function_id: u64) -> FunctionIr {
    FunctionIr {
        function_id,
        name: "loopEntryMultiRegister".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            blk(0, 0x1000, vec![cbz(0x1000, "x6", 0x1020)], vec![1, 2]),
            blk(
                1,
                0x1010,
                vec![stmt(0x1010, "mov x0, #7"), stmt(0x1014, "mov x2, #8")],
                vec![3],
            ),
            blk(
                2,
                0x1020,
                vec![stmt(0x1020, "mov x3, #9"), stmt(0x1024, "mov x5, #11")],
                vec![3],
            ),
            blk(
                3,
                0x1030,
                vec![
                    stmt(0x1030, "stur x0, [x29, #-0x10]"),
                    stmt(0x1034, "stur x2, [x29, #-0x18]"),
                    stmt(0x1038, "stur x3, [x29, #-0x20]"),
                    stmt(0x103c, "stur x5, [x29, #-0x28]"),
                    cbz(0x1040, "x7", 0x1060),
                ],
                vec![4, 5],
            ),
            blk(
                4,
                0x1050,
                vec![
                    stmt(0x1050, "mov x0, #21"),
                    stmt(0x1054, "mov x2, #22"),
                    stmt(0x1058, "mov x3, #23"),
                    stmt(0x105c, "mov x5, #24"),
                ],
                vec![3],
            ),
            blk(5, 0x1060, vec![ret(0x1060)], Vec::new()),
        ],
    }
}

/// The rendered source and the audit state of one structured emission, as one
/// comparable string.
///
/// Both halves are needed. The register visit order is invisible in the
/// pseudocode - annotations are planned by walking the rendered line, not the
/// register list - and shows up only in the order the audit records its
/// snapshots, which is a file a validator diffs across runs.
fn emission_fingerprint(ir: &FunctionIr) -> String {
    let symbols = Box::leak(Box::new(HashMap::new()));
    let mut emitter = FuncEmitter::new(ir, symbols);
    assert!(
        emitter.try_emit_structured(),
        "the fixture must structure, or capture never runs"
    );
    emitter.append_join_annotations();
    format!(
        "{}\n--\n{:?}\n--\n{:?}",
        emitter.lines.join("\n"),
        emitter.join_provenance,
        emitter.loop_provenance
    )
}

/// Ordering 1: the shared candidate order, `structured.rs::ordered_join_candidate_provenance`.
///
/// Its input is a `filter_map` over `Regions::predecessors`, which is a `Vec`
/// and not hash-derived, so its own order is already fixed. The tie-break on
/// value is what makes the comparator total for *any* input, and this drives it
/// with the one input the real capture cannot produce: two candidates sharing a
/// predecessor. Without the tie-break the stable sort simply preserves whatever
/// order it was handed, so the six permutations disagree.
#[test]
fn candidate_order_is_total_over_every_permutation_of_its_input() {
    let candidate = |pred: usize, value: &str, snapshot: &str| crate::control_flow::JoinCandidateProvenance {
        pred,
        value: value.to_string(),
        snapshot_id: snapshot.to_string(),
    };
    let a = candidate(1, "arg0.f8", "join:4:pred:1:0");
    let b = candidate(1, "arg1.f16", "join:4:pred:1:0");
    let c = candidate(3, "9", "join:4:pred:3:0");

    let permutations = [
        vec![a.clone(), b.clone(), c.clone()],
        vec![a.clone(), c.clone(), b.clone()],
        vec![b.clone(), a.clone(), c.clone()],
        vec![b.clone(), c.clone(), a.clone()],
        vec![c.clone(), a.clone(), b.clone()],
        vec![c.clone(), b.clone(), a.clone()],
    ];
    let expected = vec![a.clone(), b.clone(), c.clone()];
    for permutation in permutations {
        let ordered = crate::control_flow::ordered_join_candidate_provenance(permutation.clone());
        assert_eq!(
            ordered, expected,
            "the candidate order must not depend on the order the candidates \
             arrived in, and this permutation moved it: {permutation:?}"
        );
        assert_eq!(
            crate::control_flow::rendered_candidate_values(&ordered),
            vec![
                "arg0.f8".to_string(),
                "arg1.f16".to_string(),
                "9".to_string()
            ],
            "the rendered list dedups by first occurrence over that order, so an \
             unstable order moves the emitted text too"
        );
    }
}

/// Ordering 2: `regs.sort()` in `record_join_candidates`, over the `HashSet`
/// `registers_written_between` returns.
///
/// The set is hash-derived, which is exactly the case R24 names as the hazard.
/// Twenty-four emissions in one process see twenty-four hash seeds, and every
/// one must produce the same bytes and the same audit.
#[test]
fn a_join_emits_one_fingerprint_under_every_hash_seed() {
    assert_hash_order_varies(&["x0", "x2", "x3", "x5"], 24);
    let ir = disjoint_coverage_join_ir(0x3100);
    let first = emission_fingerprint(&ir);
    assert!(
        first.contains(NON_EXHAUSTIVE_JOIN_ANNOTATION.open()),
        "the fixture must actually annotate, or this compares two empty runs:\n{first}"
    );
    for round in 1..24 {
        assert_eq!(
            emission_fingerprint(&ir),
            first,
            "emission {round} differs from the first under a different hash seed"
        );
    }
}

/// Ordering 3: `regs.sort()` in `record_loop_entry_candidates`, over the same
/// hash-derived write set, plus `registers.sort()` over the snapshot's own
/// `reg_values` map.
#[test]
fn a_loop_header_emits_one_fingerprint_under_every_hash_seed() {
    assert_hash_order_varies(&["x0", "x2", "x3", "x5"], 24);
    let ir = loop_entry_multi_register_ir(0x3101);
    let first = emission_fingerprint(&ir);
    assert!(
        first.contains(LOOP_ENTRY_ANNOTATION.open()),
        "the fixture must actually annotate, or this compares two empty runs:\n{first}"
    );
    for round in 1..24 {
        assert_eq!(
            emission_fingerprint(&ir),
            first,
            "emission {round} differs from the first under a different hash seed"
        );
    }
}

/// The snapshot register list is built by iterating a `HashMap`, so its order is
/// hash-derived and its sort is what fixes it. Asserted on the value rather than
/// on run-to-run equality: an audit row whose registers came out in a different
/// order every run would still be self-consistent, and still not diffable.
#[test]
fn a_recorded_snapshot_lists_its_registers_in_sorted_order() {
    let ir = disjoint_coverage_join_ir(0x3102);
    let emitter = captured(&ir);
    let snapshots = &emitter.join_provenance.snapshots;
    assert!(
        !snapshots.is_empty(),
        "the fixture must record snapshots, or this asserts nothing"
    );
    for snapshot in snapshots {
        let mut sorted = snapshot.registers.clone();
        sorted.sort();
        assert_eq!(
            snapshot.registers, sorted,
            "snapshot {} lists its registers in hash order, which differs per \
             process",
            snapshot.snapshot_id
        );
    }
}

/// Ordering 4: `inserts.sort_unstable_by` in `append_join_annotations`.
///
/// `(line, at)` is unique by construction - a duplicate pair is rejected before
/// the insert is planned - so the comparator has no ties left to break and the
/// unstable sort is total. The offset component is not decoration: inserts are
/// planned in ascending offset while a line is scanned, and applying them in
/// that order would shift every later offset by the length of the annotation
/// already inserted, so the second annotation would land inside the wrong token.
#[test]
fn two_annotations_on_one_line_land_on_their_own_registers() {
    let ir = disjoint_coverage_join_ir(0x3103);
    let artifact = emit_pseudocode(&ir, &HashMap::new());
    let line = artifact
        .source
        .lines()
        .find(|line| line.contains("reg2") && line.contains("reg0"))
        .unwrap_or_else(|| panic!("the two-read line must be emitted:\n{}", artifact.source));
    assert_eq!(
        line.trim(),
        format!(
            "reg2{}.f16 = reg0{};",
            NON_EXHAUSTIVE_JOIN_ANNOTATION.render(&["8"]),
            NON_EXHAUSTIVE_JOIN_ANNOTATION.render(&["7"])
        ),
        "each annotation must sit immediately after its own register token"
    );
}

/// Ordering 5: `inserts.sort_unstable_by` in `append_call_annotations`, the same
/// shape at the call site and the same reason.
#[test]
fn two_call_annotations_on_one_line_land_on_their_own_registers() {
    let ir = FunctionIr {
        function_id: 0x3104,
        name: "twoClobberedReads".to_string(),
        entry_va: 0x1000,
        blocks: vec![blk(
            0,
            0x1000,
            vec![
                stmt(0x1000, "ldur x9, [x1, #7]"),
                stmt(0x1004, "ldur x10, [x1, #15]"),
                call_to(0x1008, 0x9000),
                // Both reads are of registers the call dropped, on one line,
                // and the two pre-call values are deliberately different: with
                // one value the line would look right whichever register each
                // annotation had actually been attached to.
                stmt(0x100c, "stur x9, [x10, #7]"),
                ret(0x1010),
            ],
            Vec::new(),
        )],
    };
    let out = emitted(&ir, &HashMap::new());
    let line = out
        .lines()
        .find(|line| line.contains("reg9") && line.contains("reg10"))
        .unwrap_or_else(|| panic!("the two-read line must be emitted:\n{out}"));
    assert_eq!(
        line.trim(),
        format!(
            "reg10{}.f8 = reg9{};",
            PRE_CALL_ANNOTATION.render(&["slot0.f16"]),
            PRE_CALL_ANNOTATION.render(&["slot0.f8"])
        ),
        "each annotation must sit immediately after its own register token"
    );
}
