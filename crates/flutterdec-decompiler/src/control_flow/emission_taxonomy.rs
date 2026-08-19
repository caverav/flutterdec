// One vocabulary for everything emission declines or omits.
//
// Two different things used to be called "the emitter gave up". A structured
// attempt can decline the whole function, which is a property of the function
// and its region tree; and a traversal can omit one edge, which is a property of
// that walk at that site and says nothing about whether the block it names was
// emitted somewhere else. Reporting both as one generic count made a declined
// function and a bounded omission indistinguishable, and made a lost path look
// like a rendering choice.
//
// So the two are separate types here. A decline carries exactly one primary
// cause and the immutable identity of the block it is attributed to; a traversal
// event carries its own key - function, source block, target, ordinal - and is
// never a block disposition.

/// Why a structured emission attempt declined, as exactly one primary cause.
///
/// The set is closed: every decline site names one of these, no site records a
/// generic "declined", and the generic count is derived by summing them. A cause
/// is either decided before the attempt touches emitter state (preflight) or
/// during it (post-mutation), and that split decides whether a rollback happened,
/// so it is a property of the cause rather than something recorded beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum StructuredDeclineCause {
    /// The region analysis refused the graph: a retreating edge whose target
    /// does not dominate its source, or a graph whose identity does not hold.
    Irreducible,
    /// The region tree is sound but a reachable block's successor set has no
    /// rendering rule, so structuring it would have to drop or invent an edge.
    UnsupportedRegion,
    /// A shared region would have to be repeated past the block or instruction
    /// budget, or repeated across a loop header.
    RepeatBudget,
    /// Region nesting exceeded the structured walk's depth budget.
    StructuredDepthBudget,
    /// The walk finished without emitting every reachable block.
    CoverageMismatch,
}

impl StructuredDeclineCause {
    /// Every primary cause, in declaration order. Iterated by the derived counts
    /// so a new cause cannot be added without being counted.
    pub const ALL: [StructuredDeclineCause; 5] = [
        StructuredDeclineCause::Irreducible,
        StructuredDeclineCause::UnsupportedRegion,
        StructuredDeclineCause::RepeatBudget,
        StructuredDeclineCause::StructuredDepthBudget,
        StructuredDeclineCause::CoverageMismatch,
    ];

    /// Decided from the region analysis alone, before the attempt writes a line,
    /// a counter, a name or a provenance row. Nothing is rolled back for one.
    pub fn is_preflight(self) -> bool {
        matches!(
            self,
            StructuredDeclineCause::Irreducible | StructuredDeclineCause::UnsupportedRegion
        )
    }

    /// Discovered while rendering, so the attempt's writes have to be undone
    /// before the DFS emitter runs. This is the only thing a rollback is counted
    /// for; it is not a cause of its own.
    pub fn is_post_mutation(self) -> bool {
        !self.is_preflight()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StructuredDeclineCause::Irreducible => "irreducible",
            StructuredDeclineCause::UnsupportedRegion => "unsupported-region",
            StructuredDeclineCause::RepeatBudget => "repeat-budget",
            StructuredDeclineCause::StructuredDepthBudget => "structured-depth-budget",
            StructuredDeclineCause::CoverageMismatch => "coverage-mismatch",
        }
    }
}

/// One function's structured decline: its single primary cause, keyed by the
/// immutable identity of the block the cause is attributed to.
///
/// The key is the block's start address, not its index: an index is a position
/// in a list that later passes may rewrite, while `start_va` is what the block
/// is, and `validate_block_identity` already proves it unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StructuredDecline {
    pub cause: StructuredDeclineCause,
    /// `None` when the cause is a property of the whole function rather than of
    /// one block, which is the case for `Irreducible`.
    pub block_start_va: Option<u64>,
}

/// A traversal limit that omitted one edge, at one site, on one walk.
///
/// Not a block disposition: the block a `DfsVisitOmission` names is very often
/// emitted elsewhere in the same artifact. That is why these are keyed by the
/// event rather than by the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum TraversalEventKind {
    /// The DFS walk was already at its depth budget at this edge.
    DfsDepthOmission,
    /// The target block had been emitted as many times as its visit budget
    /// allows.
    DfsVisitOmission,
    /// The helper budget was exhausted, so the omitted block never got a
    /// definition and its call site carries an explicit omission instead.
    HelperCapOmission,
}

impl TraversalEventKind {
    pub const ALL: [TraversalEventKind; 3] = [
        TraversalEventKind::DfsDepthOmission,
        TraversalEventKind::DfsVisitOmission,
        TraversalEventKind::HelperCapOmission,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TraversalEventKind::DfsDepthOmission => "dfs-depth-omission",
            TraversalEventKind::DfsVisitOmission => "dfs-visit-omission",
            TraversalEventKind::HelperCapOmission => "helper-cap-omission",
        }
    }
}

/// What the omitted edge pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum TraversalTarget {
    /// A block, by its immutable start address.
    Block { start_va: u64 },
    /// A helper that was never defined, by the id its call site spells
    /// (`_block_<id>`).
    Helper { id: usize },
}

/// One traversal event and its whole key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TraversalEvent {
    pub kind: TraversalEventKind,
    pub function_id: u64,
    /// Start address of the block the omitted edge leaves.
    pub source_start_va: u64,
    pub target: TraversalTarget,
    /// Position of this event among the function's traversal events. Two events
    /// with the same source and target are told apart by it.
    pub ordinal: usize,
}

/// Immutable identity of a block across every dense-id rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct BlockIdentity {
    pub function_id: u64,
    pub start_va: u64,
}

/// A named point at which dense block ids were assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum BlockStage {
    Built,
    GuardPruned,
    Split,
    NoreturnPruned,
    Emission,
}

/// One stage-local dense id and the immutable block it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StageBlock {
    pub stage: BlockStage,
    pub dense_id: usize,
    pub identity: BlockIdentity,
}

/// An identity transition. `None` means the block was removed by this stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BlockRemap {
    pub stage: BlockStage,
    pub from: BlockIdentity,
    pub to: Option<BlockIdentity>,
}

/// The one final outcome of a block from a valid source graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum BlockDisposition {
    StructuredEmitted,
    DfsEmitted,
    GuardPruned,
    NoreturnPruned,
    RetainedUnreachable,
    ReachableUnemitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BlockDispositionRecord {
    pub identity: BlockIdentity,
    pub disposition: BlockDisposition,
}

/// Invalid graphs never enter the block partition. The digest binds the outcome
/// to the raw graph that was rejected rather than to a partially indexed view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidCfgRejected {
    pub function_id: u64,
    pub raw_graph_digest: String,
}

/// Proof that a reachable-unemitted block lies on the path rooted at one keyed
/// traversal event. The event remains a separate fact, never a disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReachableUnemittedExplanation {
    pub identity: BlockIdentity,
    pub event_ordinal: usize,
}

/// Cross-stage block accounting carried by the IR and pseudocode surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BlockLedger {
    pub stages: Vec<StageBlock>,
    pub remaps: Vec<BlockRemap>,
    pub dispositions: Vec<BlockDispositionRecord>,
    pub reachable_unemitted_explanations: Vec<ReachableUnemittedExplanation>,
    pub invalid_cfg_rejected: Option<InvalidCfgRejected>,
}

impl BlockLedger {
    /// Check the reconciliation rules without trusting report counters.
    pub fn validate(&self, events: &[TraversalEvent]) -> Result<(), String> {
        if let Some(invalid) = &self.invalid_cfg_rejected {
            if !self.dispositions.is_empty() {
                return Err(format!(
                    "invalid function {} entered the block partition",
                    invalid.function_id
                ));
            }
            return Ok(());
        }

        let mut stage_ids = BTreeSet::new();
        for block in &self.stages {
            if !stage_ids.insert((block.stage, block.dense_id)) {
                return Err(format!(
                    "stage {:?} reused dense id {}",
                    block.stage, block.dense_id
                ));
            }
        }

        let known_identities: BTreeSet<_> = self.stages.iter().map(|block| block.identity).collect();
        for block in self.stages.iter().filter(|block| block.stage != BlockStage::Built) {
            if !self.remaps.iter().any(|remap| {
                remap.stage == block.stage && remap.to == Some(block.identity)
            }) {
                return Err(format!(
                    "stage {:?} identity {:?} has no remap",
                    block.stage, block.identity
                ));
            }
        }
        for remap in &self.remaps {
            if !known_identities.contains(&remap.from) {
                return Err(format!("remap names unknown source {:?}", remap.from));
            }
            if let Some(target) = remap.to {
                if !self
                    .stages
                    .iter()
                    .any(|block| block.stage == remap.stage && block.identity == target)
                {
                    return Err(format!("remap target {target:?} is absent from its stage"));
                }
            }
        }

        let source: BTreeSet<_> = self
            .stages
            .iter()
            .filter(|b| b.stage == BlockStage::Built)
            .map(|b| b.identity)
            .collect();
        let mut disposition_count = BTreeMap::<BlockIdentity, usize>::new();
        for row in &self.dispositions {
            *disposition_count.entry(row.identity).or_default() += 1;
        }
        let mut terminal = BTreeSet::new();
        for identity in &source {
            let mut current = *identity;
            for stage in [
                BlockStage::Split,
                BlockStage::GuardPruned,
                BlockStage::NoreturnPruned,
                BlockStage::Emission,
            ] {
                if let Some(remap) = self
                    .remaps
                    .iter()
                    .find(|remap| remap.stage == stage && remap.from == current)
                {
                    match remap.to {
                        Some(next) => current = next,
                        None => break,
                    }
                }
            }
            terminal.insert(current);
            match disposition_count.get(&current).copied().unwrap_or(0) {
                1 => {}
                n => return Err(format!("identity {current:?} has {n} dispositions")),
            }
        }
        if let Some(extra) = disposition_count.keys().find(|id| !terminal.contains(id)) {
            return Err(format!("disposition names unknown identity {extra:?}"));
        }

        for row in self
            .dispositions
            .iter()
            .filter(|r| r.disposition == BlockDisposition::ReachableUnemitted)
        {
            let explained = self.reachable_unemitted_explanations.iter().any(|explanation| {
                explanation.identity == row.identity
                    && events.iter().any(|event| {
                        event.function_id == row.identity.function_id
                            && event.ordinal == explanation.event_ordinal
                    })
            });
            if !explained {
                return Err(format!(
                    "reachable-unemitted identity {:?} has no traversal event",
                    row.identity
                ));
            }
        }
        Ok(())
    }

    pub fn disposition_count(&self, disposition: BlockDisposition) -> usize {
        self.dispositions
            .iter()
            .filter(|row| row.disposition == disposition)
            .count()
    }
}

/// Everything one function's emission declined or omitted.
///
/// Only primary facts are stored. The generic decline count and the rollback
/// count are derived from the cause, so neither can drift from it, and neither
/// can be recorded as a cause in its own right.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EmissionAccounting {
    decline: Option<StructuredDecline>,
    events: Vec<TraversalEvent>,
    block_ledger: BlockLedger,
}

impl EmissionAccounting {
    /// Fail closed when auditing a serialized artifact whose enum vocabulary
    /// has not yet been decoded into the closed Rust types.
    pub fn validate_serialized_vocabulary(value: &serde_json::Value) -> Result<(), String> {
        if let Some(cause) = value
            .pointer("/decline/cause")
            .and_then(serde_json::Value::as_str)
        {
            let known = [
                "Irreducible",
                "UnsupportedRegion",
                "RepeatBudget",
                "StructuredDepthBudget",
                "CoverageMismatch",
            ];
            if !known.contains(&cause) {
                return Err(format!("unknown structured decline cause {cause}"));
            }
        }
        if let Some(events) = value.get("events").and_then(serde_json::Value::as_array) {
            for event in events {
                let Some(kind) = event.get("kind").and_then(serde_json::Value::as_str) else {
                    return Err("traversal event has no kind".to_string());
                };
                if ![
                    "DfsDepthOmission",
                    "DfsVisitOmission",
                    "HelperCapOmission",
                ]
                .contains(&kind)
                {
                    return Err(format!("unknown traversal event kind {kind}"));
                }
            }
        }
        Ok(())
    }

    pub fn decline(&self) -> Option<StructuredDecline> {
        self.decline
    }

    /// 1 when this function declined with `cause`, else 0. A function has at
    /// most one primary cause, so this is what the program-level sums add up.
    pub fn cause_count(&self, cause: StructuredDeclineCause) -> usize {
        usize::from(self.decline.is_some_and(|d| d.cause == cause))
    }

    /// The generic structured-decline count, derived as the sum of the primary
    /// causes rather than stored beside them.
    pub fn structured_declines(&self) -> usize {
        StructuredDeclineCause::ALL
            .iter()
            .map(|cause| self.cause_count(*cause))
            .sum()
    }

    /// Derived from the post-mutation causes alone. A preflight decline never
    /// mutated anything, so there is nothing for it to have rolled back.
    pub fn rollbacks(&self) -> usize {
        StructuredDeclineCause::ALL
            .iter()
            .filter(|cause| cause.is_post_mutation())
            .map(|cause| self.cause_count(*cause))
            .sum()
    }

    pub fn events(&self) -> &[TraversalEvent] {
        &self.events
    }

    pub fn block_ledger(&self) -> &BlockLedger {
        &self.block_ledger
    }

    pub fn block_ledger_mut(&mut self) -> &mut BlockLedger {
        &mut self.block_ledger
    }

    pub fn event_count(&self, kind: TraversalEventKind) -> usize {
        self.events.iter().filter(|e| e.kind == kind).count()
    }

    /// Record the function's one primary cause.
    ///
    /// A second call is a bug in the caller: the first cause is the one that
    /// stopped the attempt and anything after it is a consequence. Keeping the
    /// first is what makes the causes disjoint.
    pub(crate) fn record_decline(&mut self, decline: StructuredDecline) {
        debug_assert!(
            self.decline.is_none(),
            "a second structured decline cause was recorded: {:?} after {:?}",
            decline,
            self.decline
        );
        if self.decline.is_none() {
            self.decline = Some(decline);
        }
    }

    /// Number of events recorded so far, which is also the next ordinal. A
    /// rollback uses it to drop the events an abandoned attempt recorded.
    pub(crate) fn event_len(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn truncate_events(&mut self, len: usize) {
        self.events.truncate(len);
    }

    /// Append an event, stamping it with the next ordinal.
    pub(crate) fn record_event(
        &mut self,
        kind: TraversalEventKind,
        function_id: u64,
        source_start_va: u64,
        target: TraversalTarget,
    ) {
        let ordinal = self.events.len();
        self.events.push(TraversalEvent {
            kind,
            function_id,
            source_start_va,
            target,
            ordinal,
        });
    }

}

// The omission fixtures for everything above. A test-only file rather than an
// inline module, so the oracle inventory can name it and product source stays
// free of test bodies.
#[cfg(test)]
#[path = "emission_taxonomy_tests.rs"]
mod emission_taxonomy_tests;
