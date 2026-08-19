use flutterdec_decompiler::{
    emit_pseudocode, BlockDisposition, BlockDispositionRecord, BlockIdentity, BlockLedger,
    BlockRemap, BlockStage, ReachableUnemittedExplanation, StageBlock, TraversalEvent,
    TraversalEventKind, TraversalTarget,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::HashMap;

fn identity(n: u64) -> BlockIdentity {
    BlockIdentity {
        function_id: 7,
        start_va: 0x1000 + n * 0x10,
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
    let stages = dispositions
        .iter()
        .enumerate()
        .map(|(dense_id, _)| StageBlock {
            stage: BlockStage::Built,
            dense_id,
            identity: identity(dense_id as u64),
        })
        .collect();
    let ledger = BlockLedger {
        stages,
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
}

#[test]
fn split_remap_rekeys_identity_without_reusing_dense_id() {
    let before = identity(0);
    let after = BlockIdentity {
        function_id: 8,
        start_va: before.start_va,
    };
    let ledger = BlockLedger {
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
        .contains("entered the block partition"));
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
