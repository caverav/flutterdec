use flutterdec_decompiler::{
    emit_pseudocode, BlockDisposition, BlockDispositionRecord, BlockEdge, BlockIdentity,
    BlockLedger, BlockRemap, BlockStage, InvalidCfgRawGraph, InvalidCfgRejected,
    ReachableUnemittedExplanation, StageBlock, TraversalEvent, TraversalEventKind, TraversalTarget,
};
use flutterdec_ir::{validate_block_identity, BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

fn identity(n: u64) -> BlockIdentity {
    BlockIdentity {
        function_id: 7,
        start_va: 0x1000 + n * 0x10,
    }
}

fn raw_graph_digest(graph: &InvalidCfgRawGraph) -> String {
    let raw = serde_json::to_vec(graph).unwrap();
    let digest = raw.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{digest:016x}")
}

fn rejected_witness_ledger(ir: &FunctionIr) -> BlockLedger {
    let witness = InvalidCfgRawGraph::from(ir);
    BlockLedger {
        function_id: ir.function_id,
        invalid_cfg_rejected: Some(InvalidCfgRejected {
            function_id: ir.function_id,
            raw_graph_digest: raw_graph_digest(&witness),
            raw_graph_witness: Some(witness),
        }),
        ..BlockLedger::default()
    }
}

fn block(id: usize, start_va: u64, succs: Vec<usize>, preds: Vec<usize>) -> BasicBlock {
    let op = match succs.len() {
        0 => IROp::Return,
        1 => IROp::Jump,
        _ => IROp::Branch,
    };
    BasicBlock {
        id,
        start_va,
        instrs: vec![LlirInstr {
            va: start_va,
            op,
            src: "control".to_string(),
            target: String::new(),
        }],
        succs,
        preds,
    }
}

fn complete_ledger() -> (BlockLedger, Vec<TraversalEvent>) {
    let dispositions = [
        BlockDisposition::StructuredEmitted,
        BlockDisposition::DfsEmitted,
        BlockDisposition::GuardPruned,
        BlockDisposition::NoreturnPruned,
        BlockDisposition::RetainedUnreachable,
        BlockDisposition::ReachableUnemitted,
    ];
    let mut stages: Vec<_> = dispositions
        .iter()
        .enumerate()
        .map(|(dense_id, _)| StageBlock {
            stage: BlockStage::Built,
            dense_id,
            identity: identity(dense_id as u64),
        })
        .collect();
    stages.extend([0usize, 1, 4, 5].map(|dense_id| StageBlock {
        stage: BlockStage::Emission,
        dense_id,
        identity: identity(dense_id as u64),
    }));
    let ledger = BlockLedger {
        function_id: 7,
        stages,
        valid_edges: vec![BlockEdge {
            from: identity(0),
            to: identity(5),
        }],
        remaps: vec![
            BlockRemap {
                stage: BlockStage::GuardPruned,
                from: identity(2),
                to: None,
            },
            BlockRemap {
                stage: BlockStage::NoreturnPruned,
                from: identity(3),
                to: None,
            },
            BlockRemap {
                stage: BlockStage::Emission,
                from: identity(0),
                to: Some(identity(0)),
            },
            BlockRemap {
                stage: BlockStage::Emission,
                from: identity(1),
                to: Some(identity(1)),
            },
            BlockRemap {
                stage: BlockStage::Emission,
                from: identity(4),
                to: Some(identity(4)),
            },
            BlockRemap {
                stage: BlockStage::Emission,
                from: identity(5),
                to: Some(identity(5)),
            },
        ],
        dispositions: dispositions
            .into_iter()
            .enumerate()
            .map(|(n, disposition)| BlockDispositionRecord {
                identity: identity(n as u64),
                disposition,
            })
            .collect(),
        reachable_unemitted_explanations: vec![ReachableUnemittedExplanation {
            identity: identity(5),
            event_ordinal: 0,
            path: vec![identity(5)],
        }],
        invalid_cfg_rejected: None,
    };
    let events = vec![TraversalEvent {
        kind: TraversalEventKind::HelperCapOmission,
        function_id: 7,
        source_start_va: identity(0).start_va,
        target: TraversalTarget::Block {
            start_va: identity(5).start_va,
        },
        ordinal: 0,
    }];
    (ledger, events)
}

#[test]
fn complete_partition_reconciles_and_plants_fail_closed() {
    let (ledger, events) = complete_ledger();
    assert_eq!(ledger.validate(&events), Ok(()));

    let mut reused_id = ledger.clone();
    reused_id.stages[1].dense_id = reused_id.stages[0].dense_id;
    assert!(reused_id
        .validate(&events)
        .unwrap_err()
        .contains("reused dense id"));

    let mut missing = ledger.clone();
    missing.dispositions.pop();
    assert!(missing
        .validate(&events)
        .unwrap_err()
        .contains("0 dispositions"));

    let mut double = ledger.clone();
    double.dispositions.push(double.dispositions[0]);
    assert!(double
        .validate(&events)
        .unwrap_err()
        .contains("2 dispositions"));

    let mut orphan = ledger.clone();
    orphan.reachable_unemitted_explanations.clear();
    assert!(orphan
        .validate(&events)
        .unwrap_err()
        .contains("no traversal event"));

    let mut missing_remap = ledger.clone();
    missing_remap.stages.push(StageBlock {
        stage: BlockStage::Split,
        dense_id: 0,
        identity: identity(0),
    });
    assert!(missing_remap
        .validate(&events)
        .unwrap_err()
        .contains("has no remap"));

    let mut unrelated = ledger.clone();
    unrelated.valid_edges.clear();
    unrelated.reachable_unemitted_explanations[0].path = vec![identity(0), identity(5)];
    assert!(unrelated
        .validate(&events)
        .unwrap_err()
        .contains("invalid traversal path"));

    let mut wrong_endpoint = ledger.clone();
    wrong_endpoint.reachable_unemitted_explanations[0].path = vec![identity(0)];
    assert!(wrong_endpoint
        .validate(&events)
        .unwrap_err()
        .contains("invalid traversal path"));

    let mut wrong_function = events.clone();
    wrong_function[0].function_id = 8;
    assert!(ledger
        .validate(&wrong_function)
        .unwrap_err()
        .contains("expected 7"));

    let mut duplicate_remap = ledger.clone();
    duplicate_remap.remaps.push(BlockRemap {
        stage: BlockStage::GuardPruned,
        from: identity(2),
        to: Some(identity(2)),
    });
    assert!(duplicate_remap
        .validate(&events)
        .unwrap_err()
        .contains("ambiguous remaps"));

    for reverse in [false, true] {
        let mut removal_and_emission = ledger.clone();
        let removal = BlockRemap {
            stage: BlockStage::GuardPruned,
            from: identity(0),
            to: None,
        };
        if reverse {
            removal_and_emission.remaps.push(removal);
        } else {
            removal_and_emission.remaps.insert(0, removal);
        }
        removal_and_emission.dispositions[0].disposition = BlockDisposition::GuardPruned;
        assert!(
            removal_and_emission
                .validate(&events)
                .unwrap_err()
                .contains("live terminal chain"),
            "GuardPruned removal plus Emission retention was accepted (reverse={reverse})"
        );
    }
}

#[test]
fn split_remap_rekeys_identity_without_reusing_dense_id() {
    let before = identity(0);
    let after = BlockIdentity {
        function_id: 8,
        start_va: before.start_va,
    };
    let ledger = BlockLedger {
        function_id: 8,
        stages: vec![
            StageBlock {
                stage: BlockStage::Built,
                dense_id: 0,
                identity: before,
            },
            StageBlock {
                stage: BlockStage::Split,
                dense_id: 0,
                identity: after,
            },
            StageBlock {
                stage: BlockStage::Emission,
                dense_id: 0,
                identity: after,
            },
        ],
        valid_edges: Vec::new(),
        remaps: vec![
            BlockRemap {
                stage: BlockStage::Split,
                from: before,
                to: Some(after),
            },
            BlockRemap {
                stage: BlockStage::Emission,
                from: after,
                to: Some(after),
            },
        ],
        dispositions: vec![BlockDispositionRecord {
            identity: after,
            disposition: BlockDisposition::DfsEmitted,
        }],
        reachable_unemitted_explanations: Vec::new(),
        invalid_cfg_rejected: None,
    };

    assert_eq!(ledger.validate(&[]), Ok(()));
}

#[test]
fn self_certified_valid_graphs_are_not_invalid_outcomes() {
    let cases = [
        FunctionIr {
            function_id: 81,
            name: "one_block".to_string(),
            entry_va: 0x1000,
            blocks: vec![block(0, 0x1000, vec![], vec![])],
        },
        FunctionIr {
            function_id: 82,
            name: "linear".to_string(),
            entry_va: 0x2000,
            blocks: vec![
                block(0, 0x2000, vec![1], vec![]),
                block(1, 0x2010, vec![], vec![0]),
            ],
        },
        FunctionIr {
            function_id: 83,
            name: "branch".to_string(),
            entry_va: 0x3000,
            blocks: vec![
                block(0, 0x3000, vec![1, 2], vec![]),
                block(1, 0x3010, vec![], vec![0]),
                block(2, 0x3020, vec![], vec![0]),
            ],
        },
        FunctionIr {
            function_id: 84,
            name: "loop".to_string(),
            entry_va: 0x4000,
            blocks: vec![
                block(0, 0x4000, vec![1], vec![1]),
                block(1, 0x4010, vec![0], vec![0]),
            ],
        },
    ];

    for ir in cases {
        assert_eq!(validate_block_identity(&ir), Ok(()), "{}", ir.name);
        assert!(
            emit_pseudocode(&ir, &HashMap::new())
                .emission
                .block_ledger()
                .invalid_cfg_rejected
                .is_none(),
            "production rejected valid {}",
            ir.name
        );
        assert!(
            rejected_witness_ledger(&ir)
                .validate(&[])
                .unwrap_err()
                .contains("raw graph witness is a valid graph"),
            "public validation accepted self-certified valid {}",
            ir.name
        );
    }
}

#[test]
fn every_production_admission_defect_is_a_valid_invalid_witness() {
    let valid = FunctionIr {
        function_id: 90,
        name: "admission_defect".to_string(),
        entry_va: 0x5000,
        blocks: vec![
            block(0, 0x5000, vec![1, 2], vec![]),
            block(1, 0x5010, vec![], vec![0]),
            block(2, 0x5020, vec![], vec![0]),
        ],
    };
    let mut cases = Vec::new();

    let mut duplicate_id = valid.clone();
    duplicate_id.blocks[2].id = 1;
    cases.push(("duplicate id", duplicate_id));

    let mut missing_entry = valid.clone();
    for block in &mut missing_entry.blocks {
        block.id += 1;
    }
    cases.push(("missing entry", missing_entry));

    let mut non_dense = valid.clone();
    non_dense.blocks[2].id = 9;
    cases.push(("non-dense ordering", non_dense));

    let mut duplicate_start = valid.clone();
    duplicate_start.blocks[2].start_va = duplicate_start.blocks[1].start_va;
    cases.push(("duplicate start", duplicate_start));

    let mut missing_successor = valid.clone();
    missing_successor.blocks[0].succs.push(9);
    cases.push(("missing successor", missing_successor));

    let mut bad_predecessor_range = valid;
    bad_predecessor_range.blocks[1].preds.push(9);
    cases.push(("bad predecessor range", bad_predecessor_range));

    for (label, ir) in cases {
        assert!(
            validate_block_identity(&ir).is_err(),
            "canonical admission accepted {label}"
        );
        let artifact = emit_pseudocode(&ir, &HashMap::new());
        let ledger = artifact.emission.block_ledger();
        assert!(
            ledger.invalid_cfg_rejected.is_some(),
            "production did not reject {label}"
        );
        assert_eq!(
            ledger.validate(artifact.emission.events()),
            Ok(()),
            "public validation disagreed with production for {label}"
        );
    }
}

#[test]
fn invalid_graph_has_one_digest_outcome_and_no_partition() {
    let invalid = FunctionIr {
        function_id: 91,
        name: "invalid".to_string(),
        entry_va: 0x1000,
        blocks: vec![
            BasicBlock {
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
            },
            BasicBlock {
                id: 0,
                start_va: 0x1010,
                instrs: Vec::new(),
                succs: Vec::new(),
                preds: Vec::new(),
            },
        ],
    };
    let first = emit_pseudocode(&invalid, &HashMap::new());
    let second = emit_pseudocode(&invalid, &HashMap::new());
    let outcome = first
        .emission
        .block_ledger()
        .invalid_cfg_rejected
        .as_ref()
        .expect("invalid outcome");
    assert_eq!(outcome.function_id, 91);
    assert!(outcome.raw_graph_digest.starts_with("fnv1a64:"));
    let legacy_raw = serde_json::to_vec(&invalid).unwrap();
    let legacy_digest = legacy_raw.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    assert_eq!(
        outcome.raw_graph_digest,
        format!("fnv1a64:{legacy_digest:016x}"),
        "the additive witness must preserve the pre-existing content digest"
    );
    assert_eq!(
        first.emission.block_ledger(),
        second.emission.block_ledger(),
        "digest is stable"
    );
    assert!(first.emission.block_ledger().dispositions.is_empty());
    assert_eq!(
        first
            .emission
            .block_ledger()
            .validate(first.emission.events()),
        Ok(())
    );

    let mut invalid_partition = first.emission.block_ledger().clone();
    invalid_partition.dispositions.push(BlockDispositionRecord {
        identity: BlockIdentity {
            function_id: 91,
            start_va: 0x1000,
        },
        disposition: BlockDisposition::DfsEmitted,
    });
    assert!(invalid_partition
        .validate(first.emission.events())
        .unwrap_err()
        .contains("valid-graph accounting"));

    let mut empty_digest = first.emission.block_ledger().clone();
    empty_digest
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .raw_graph_digest
        .clear();
    assert!(empty_digest
        .validate(first.emission.events())
        .unwrap_err()
        .contains("malformed raw graph digest"));

    let mut wrong_function = first.emission.block_ledger().clone();
    wrong_function
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .function_id = 92;
    assert!(wrong_function
        .validate(first.emission.events())
        .unwrap_err()
        .contains("does not match ledger function"));

    let mut stale_digest = first.emission.block_ledger().clone();
    let digest = &mut stale_digest
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .raw_graph_digest;
    let replacement = if digest.ends_with('0') { '1' } else { '0' };
    digest.pop();
    digest.push(replacement);
    assert!(stale_digest
        .validate(first.emission.events())
        .unwrap_err()
        .contains("does not match its witness"));

    let mut missing_witness = first.emission.block_ledger().clone();
    missing_witness
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .raw_graph_witness = None;
    assert!(missing_witness
        .validate(first.emission.events())
        .unwrap_err()
        .contains("no raw graph witness"));

    let mut mutated_witness = first.emission.block_ledger().clone();
    mutated_witness
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .raw_graph_witness
        .as_mut()
        .unwrap()
        .blocks[0]
        .instrs[0]
        .src
        .push('x');
    assert!(mutated_witness
        .validate(first.emission.events())
        .unwrap_err()
        .contains("does not match its witness"));

    let mut reordered_witness = first.emission.block_ledger().clone();
    reordered_witness
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .raw_graph_witness
        .as_mut()
        .unwrap()
        .blocks
        .swap(0, 1);
    assert!(reordered_witness
        .validate(first.emission.events())
        .unwrap_err()
        .contains("does not match its witness"));

    let mut cross_function_witness = first.emission.block_ledger().clone();
    cross_function_witness
        .invalid_cfg_rejected
        .as_mut()
        .unwrap()
        .raw_graph_witness
        .as_mut()
        .unwrap()
        .function_id = 92;
    assert!(cross_function_witness
        .validate(first.emission.events())
        .unwrap_err()
        .contains("witness function 92 does not match invalid function 91"));

    let dirty = ["stage row", "valid edge", "remap", "explanation"];
    for field in dirty {
        let mut ledger = first.emission.block_ledger().clone();
        match field {
            "stage row" => ledger.stages.push(StageBlock {
                stage: BlockStage::Built,
                dense_id: 0,
                identity: identity(0),
            }),
            "valid edge" => ledger.valid_edges.push(BlockEdge {
                from: identity(0),
                to: identity(1),
            }),
            "remap" => ledger.remaps.push(BlockRemap {
                stage: BlockStage::GuardPruned,
                from: identity(0),
                to: None,
            }),
            "explanation" => {
                ledger
                    .reachable_unemitted_explanations
                    .push(ReachableUnemittedExplanation {
                        identity: identity(0),
                        event_ordinal: 0,
                        path: vec![identity(0)],
                    })
            }
            _ => unreachable!(),
        }
        assert!(
            ledger
                .validate(&[])
                .unwrap_err()
                .contains("valid-graph accounting"),
            "invalid outcome accepted {field}"
        );
    }

    let event = TraversalEvent {
        kind: TraversalEventKind::DfsDepthOmission,
        function_id: 91,
        source_start_va: 0x1000,
        target: TraversalTarget::Block { start_va: 0x1010 },
        ordinal: 0,
    };
    assert!(first
        .emission
        .block_ledger()
        .validate(&[event])
        .unwrap_err()
        .contains("valid-graph accounting"));
}

#[test]
fn cause_and_event_vocabularies_are_closed() {
    let causes = serde_json::to_value(flutterdec_decompiler::StructuredDeclineCause::ALL).unwrap();
    let text = causes.to_string();
    for known in [
        "Irreducible",
        "UnsupportedRegion",
        "RepeatBudget",
        "StructuredDepthBudget",
        "CoverageMismatch",
    ] {
        assert!(text.contains(known));
    }
    assert!(!text.contains("GenericDecline"));
    assert!(!text.contains("Rollback"));
    assert_eq!(TraversalEventKind::ALL.len(), 3);

    let unknown_cause = serde_json::json!({
        "decline": { "cause": "GenericDecline", "block_start_va": null },
        "events": []
    });
    assert!(
        flutterdec_decompiler::EmissionAccounting::validate_serialized_vocabulary(&unknown_cause)
            .unwrap_err()
            .contains("unknown structured decline cause")
    );

    let hidden_kind = serde_json::json!({
        "decline": null,
        "events": [{ "kind": "HiddenOmission" }]
    });
    assert!(
        flutterdec_decompiler::EmissionAccounting::validate_serialized_vocabulary(&hidden_kind)
            .unwrap_err()
            .contains("unknown traversal event kind")
    );
}
