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

/// One immutable edge in the valid graph presented to the emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct BlockEdge {
    pub from: BlockIdentity,
    pub to: BlockIdentity,
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

impl BlockDisposition {
    pub const ALL: [BlockDisposition; 6] = [
        BlockDisposition::StructuredEmitted,
        BlockDisposition::DfsEmitted,
        BlockDisposition::GuardPruned,
        BlockDisposition::NoreturnPruned,
        BlockDisposition::RetainedUnreachable,
        BlockDisposition::ReachableUnemitted,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BlockDispositionRecord {
    pub identity: BlockIdentity,
    pub disposition: BlockDisposition,
}

/// Invalid graphs never enter the block partition. The digest binds the outcome
/// to the raw graph that was rejected rather than to a partially indexed view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidCfgRawInstruction {
    pub va: u64,
    pub op: IROp,
    pub src: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidCfgRawBlock {
    pub id: usize,
    pub start_va: u64,
    pub instrs: Vec<InvalidCfgRawInstruction>,
    pub succs: Vec<usize>,
    pub preds: Vec<usize>,
}

/// Canonically field-ordered copy of the rejected `FunctionIr`. Every field is
/// required because the stable digest predates this witness and covers the full
/// serialized function, including instruction text and raw edge order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidCfgRawGraph {
    pub function_id: u64,
    pub name: String,
    pub entry_va: u64,
    pub blocks: Vec<InvalidCfgRawBlock>,
}

impl From<&FunctionIr> for InvalidCfgRawGraph {
    fn from(ir: &FunctionIr) -> Self {
        Self {
            function_id: ir.function_id,
            name: ir.name.clone(),
            entry_va: ir.entry_va,
            blocks: ir
                .blocks
                .iter()
                .map(|block| InvalidCfgRawBlock {
                    id: block.id,
                    start_va: block.start_va,
                    instrs: block
                        .instrs
                        .iter()
                        .map(|instr| InvalidCfgRawInstruction {
                            va: instr.va,
                            op: instr.op.clone(),
                            src: instr.src.clone(),
                            target: instr.target.clone(),
                        })
                        .collect(),
                    succs: block.succs.clone(),
                    preds: block.preds.clone(),
                })
                .collect(),
        }
    }
}

impl InvalidCfgRawGraph {
    /// Reconstruct the exact raw graph presented to production admission.
    /// Public validation must run that same ruler rather than infer validity
    /// from the witness digest or maintain a second set of graph rules.
    fn to_function_ir(&self) -> FunctionIr {
        FunctionIr {
            function_id: self.function_id,
            name: self.name.clone(),
            entry_va: self.entry_va,
            blocks: self
                .blocks
                .iter()
                .map(|block| BasicBlock {
                    id: block.id,
                    start_va: block.start_va,
                    instrs: block
                        .instrs
                        .iter()
                        .map(|instr| LlirInstr {
                            va: instr.va,
                            op: instr.op.clone(),
                            src: instr.src.clone(),
                            target: instr.target.clone(),
                        })
                        .collect(),
                    succs: block.succs.clone(),
                    preds: block.preds.clone(),
                })
                .collect(),
        }
    }
}

pub(super) fn raw_graph_digest(graph: &InvalidCfgRawGraph) -> String {
    let raw = serde_json::to_vec(graph).expect("raw graph witness serialization cannot fail");
    // FNV-1a is the existing stable content binding. This is an identity
    // digest, not a cryptographic authenticity claim.
    let digest = raw.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{digest:016x}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidCfgRejected {
    pub function_id: u64,
    pub raw_graph_digest: String,
    /// `None` represents a legacy or incomplete outcome and is rejected by
    /// validation. Keeping it optional makes the serialized schema additive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_graph_witness: Option<InvalidCfgRawGraph>,
}

/// Proof that a reachable-unemitted block lies on the path rooted at one keyed
/// traversal event. The event remains a separate fact, never a disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachableUnemittedExplanation {
    pub identity: BlockIdentity,
    pub event_ordinal: usize,
    /// Inclusive path from the cited event target to `identity`.
    pub path: Vec<BlockIdentity>,
}

/// Cross-stage block accounting carried by the IR and pseudocode surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BlockLedger {
    /// Function whose final partition this ledger describes. A Split remap may
    /// name an older function key on its source side.
    pub function_id: u64,
    pub stages: Vec<StageBlock>,
    pub valid_edges: Vec<BlockEdge>,
    pub remaps: Vec<BlockRemap>,
    pub dispositions: Vec<BlockDispositionRecord>,
    pub reachable_unemitted_explanations: Vec<ReachableUnemittedExplanation>,
    pub invalid_cfg_rejected: Option<InvalidCfgRejected>,
}

impl BlockLedger {
    /// Check the reconciliation rules without trusting report counters.
    pub fn validate(&self, events: &[TraversalEvent]) -> Result<(), String> {
        if let Some(invalid) = &self.invalid_cfg_rejected {
            if invalid.function_id != self.function_id {
                return Err(format!(
                    "invalid outcome function {} does not match ledger function {}",
                    invalid.function_id, self.function_id
                ));
            }
            let digest = invalid.raw_graph_digest.as_bytes();
            if digest.len() != 24
                || !digest.starts_with(b"fnv1a64:")
                || !digest[8..]
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            {
                return Err(format!(
                    "invalid function {} has malformed raw graph digest",
                    invalid.function_id
                ));
            }
            let witness = invalid.raw_graph_witness.as_ref().ok_or_else(|| {
                format!(
                    "invalid function {} has no raw graph witness",
                    invalid.function_id
                )
            })?;
            if witness.function_id != invalid.function_id {
                return Err(format!(
                    "raw graph witness function {} does not match invalid function {}",
                    witness.function_id, invalid.function_id
                ));
            }
            if raw_graph_digest(witness) != invalid.raw_graph_digest {
                return Err(format!(
                    "invalid function {} raw graph digest does not match its witness",
                    invalid.function_id
                ));
            }
            if validate_block_identity(&witness.to_function_ir()).is_ok() {
                return Err(format!(
                    "invalid function {} raw graph witness is a valid graph",
                    invalid.function_id
                ));
            }
            if !self.stages.is_empty()
                || !self.valid_edges.is_empty()
                || !self.remaps.is_empty()
                || !self.dispositions.is_empty()
                || !self.reachable_unemitted_explanations.is_empty()
                || !events.is_empty()
            {
                return Err(format!(
                    "invalid function {} carries valid-graph accounting",
                    invalid.function_id
                ));
            }
            return Ok(());
        }

        let mut stage_ids = BTreeSet::new();
        let mut stage_identities = BTreeSet::new();
        for block in &self.stages {
            if !stage_ids.insert((block.stage, block.dense_id)) {
                return Err(format!(
                    "stage {:?} reused dense id {}",
                    block.stage, block.dense_id
                ));
            }
            if !stage_identities.insert((block.stage, block.identity)) {
                return Err(format!(
                    "stage {:?} reused identity {:?}",
                    block.stage, block.identity
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
        let mut remap_keys = BTreeSet::new();
        let mut remap_targets = BTreeSet::new();
        for remap in &self.remaps {
            if remap.stage == BlockStage::Built {
                return Err("Built stage cannot contain a remap".to_string());
            }
            if !remap_keys.insert((remap.stage, remap.from)) {
                return Err(format!(
                    "stage {:?} has ambiguous remaps from {:?}",
                    remap.stage, remap.from
                ));
            }
            if !known_identities.contains(&remap.from) {
                return Err(format!("remap names unknown source {:?}", remap.from));
            }
            if let Some(target) = remap.to {
                if !remap_targets.insert((remap.stage, target)) {
                    return Err(format!(
                        "stage {:?} has ambiguous remaps to {target:?}",
                        remap.stage
                    ));
                }
                if !self
                    .stages
                    .iter()
                    .any(|block| block.stage == remap.stage && block.identity == target)
                {
                    return Err(format!("remap target {target:?} is absent from its stage"));
                }
                if remap.from.start_va != target.start_va {
                    return Err(format!(
                        "remap changes immutable address from {:?} to {target:?}",
                        remap.from
                    ));
                }
                if remap.stage != BlockStage::Split
                    && remap.from.function_id != target.function_id
                {
                    return Err(format!(
                        "only Split may change a function key: {:?} to {target:?}",
                        remap.from
                    ));
                }
            } else if !matches!(
                remap.stage,
                BlockStage::GuardPruned | BlockStage::NoreturnPruned
            ) {
                return Err(format!(
                    "stage {:?} cannot remove identity {:?}",
                    remap.stage, remap.from
                ));
            }
        }

        let final_identities: BTreeSet<_> = self
            .stages
            .iter()
            .filter(|block| block.identity.function_id == self.function_id)
            .map(|block| block.identity)
            .collect();
        let mut unique_edges = BTreeSet::new();
        for edge in &self.valid_edges {
            if edge.from.function_id != self.function_id
                || edge.to.function_id != self.function_id
                || !final_identities.contains(&edge.from)
                || !final_identities.contains(&edge.to)
            {
                return Err(format!("valid graph edge has unknown endpoint {edge:?}"));
            }
            if !unique_edges.insert(*edge) {
                return Err(format!("valid graph repeats edge {edge:?}"));
            }
        }
        for (index, event) in events.iter().enumerate() {
            if event.ordinal != index {
                return Err(format!(
                    "traversal event ordinal {} is not its position {index}",
                    event.ordinal
                ));
            }
            if event.function_id != self.function_id {
                return Err(format!(
                    "traversal event {} has function {}, expected {}",
                    event.ordinal, event.function_id, self.function_id
                ));
            }
        }

        let source: BTreeSet<_> = self
            .stages
            .iter()
            .filter(|b| b.stage == BlockStage::Built)
            .map(|b| b.identity)
            .collect();
        if source.is_empty() {
            if self.stages.is_empty()
                && self.valid_edges.is_empty()
                && self.remaps.is_empty()
                && self.dispositions.is_empty()
                && self.reachable_unemitted_explanations.is_empty()
                && events.is_empty()
            {
                return Ok(());
            }
            return Err("valid ledger has accounting without Built identities".to_string());
        }
        let mut disposition_count = BTreeMap::<BlockIdentity, usize>::new();
        for row in &self.dispositions {
            *disposition_count.entry(row.identity).or_default() += 1;
        }
        let mut active: BTreeMap<BlockIdentity, BlockIdentity> =
            source.iter().map(|identity| (*identity, *identity)).collect();
        let mut terminal_by_source: BTreeMap<_, _> =
            source.iter().map(|identity| (*identity, *identity)).collect();
        for stage in [
            BlockStage::Split,
            BlockStage::GuardPruned,
            BlockStage::NoreturnPruned,
            BlockStage::Emission,
        ] {
            let stage_remaps: Vec<_> = self
                .remaps
                .iter()
                .filter(|remap| remap.stage == stage)
                .collect();
            for remap in &stage_remaps {
                if !active.contains_key(&remap.from) {
                    return Err(format!(
                        "stage {stage:?} remap from {:?} does not continue one live terminal chain",
                        remap.from
                    ));
                }
                if let Some(target) = remap.to {
                    if target != remap.from
                        && stage_remaps.iter().any(|other| other.from == target)
                    {
                        return Err(format!(
                            "stage {stage:?} remap target {target:?} is also a source"
                        ));
                    }
                }
            }
            for remap in stage_remaps {
                let origin = active
                    .remove(&remap.from)
                    .expect("stage remap sources were checked against the live set");
                if let Some(target) = remap.to {
                    if active.insert(target, origin).is_some() {
                        return Err(format!(
                            "stage {stage:?} remap converges ambiguously on {target:?}"
                        ));
                    }
                    terminal_by_source.insert(origin, target);
                }
            }
        }
        let terminal: BTreeSet<_> = terminal_by_source.values().copied().collect();
        for current in &terminal {
            if current.function_id != self.function_id {
                return Err(format!(
                    "terminal identity {current:?} does not match ledger function {}",
                    self.function_id
                ));
            }
            match disposition_count.get(current).copied().unwrap_or(0) {
                1 => {}
                n => return Err(format!("identity {current:?} has {n} dispositions")),
            }
        }
        if let Some(extra) = disposition_count.keys().find(|id| !terminal.contains(id)) {
            return Err(format!("disposition names unknown identity {extra:?}"));
        }

        for remap in self.remaps.iter().filter(|remap| remap.to.is_none()) {
            let expected = match remap.stage {
                BlockStage::GuardPruned => BlockDisposition::GuardPruned,
                BlockStage::NoreturnPruned => BlockDisposition::NoreturnPruned,
                _ => unreachable!("removal stages checked above"),
            };
            if !self.dispositions.iter().any(|row| {
                row.identity == remap.from && row.disposition == expected
            }) {
                return Err(format!(
                    "removal of {:?} at {:?} has no matching disposition",
                    remap.from, remap.stage
                ));
            }
        }

        let valid_explanation = |explanation: &ReachableUnemittedExplanation| {
            let Some(event) = events.get(explanation.event_ordinal) else {
                return false;
            };
            if event.ordinal != explanation.event_ordinal
                || event.function_id != explanation.identity.function_id
                || explanation.path.last() != Some(&explanation.identity)
            {
                return false;
            }
            let target = match event.target {
                TraversalTarget::Block { start_va } => Some(BlockIdentity {
                    function_id: event.function_id,
                    start_va,
                }),
                TraversalTarget::Helper { id } => self
                    .stages
                    .iter()
                    .find(|block| {
                        block.stage == BlockStage::Emission
                            && block.dense_id == id
                            && block.identity.function_id == event.function_id
                    })
                    .map(|block| block.identity),
            };
            explanation.path.first().copied() == target
                && explanation.path.windows(2).all(|hop| {
                    self.valid_edges.contains(&BlockEdge {
                        from: hop[0],
                        to: hop[1],
                    })
                })
        };
        for explanation in &self.reachable_unemitted_explanations {
            if !self.dispositions.iter().any(|row| {
                row.identity == explanation.identity
                    && row.disposition == BlockDisposition::ReachableUnemitted
            }) || !valid_explanation(explanation)
            {
                return Err(format!(
                    "reachable-unemitted explanation for {:?} has an invalid traversal path",
                    explanation.identity
                ));
            }
        }

        for row in self
            .dispositions
            .iter()
            .filter(|r| r.disposition == BlockDisposition::ReachableUnemitted)
        {
            let explained = self
                .reachable_unemitted_explanations
                .iter()
                .any(|explanation| explanation.identity == row.identity);
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
    pub fn validate(&self) -> Result<(), String> {
        if self.block_ledger.invalid_cfg_rejected.is_some() && self.decline.is_some() {
            return Err("invalid function carries a structured decline".to_string());
        }
        self.block_ledger.validate(&self.events)
    }
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
