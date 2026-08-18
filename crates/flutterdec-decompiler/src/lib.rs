use flutterdec_ir::{validate_block_identity, BasicBlock, CfgDefect, FunctionIr, IROp};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct PseudocodeArtifact {
    pub function_id: u64,
    pub function_name: String,
    pub source: String,
    pub placeholder_ifs: usize,
    pub unresolved_cf: usize,
    pub raw_register_calls: usize,
    pub total_calls: usize,
    pub indirect_calls: usize,
    pub semantic_direct_calls: usize,
    pub semantic_indirect_calls: usize,
    pub dispatch_selector_calls: usize,
    /// Calls named from a recovered dispatch-table selector offset, as opposed to
    /// from pool metadata. Provable from the instruction stream alone.
    pub dispatch_table_calls: usize,
    /// Blocks emitted more than once because a shared continuation could not be
    /// named. Bounded by budget; the DFS fallback's duplication is not.
    pub repeated_blocks: usize,
    /// Instructions on a branch arm that the lifter does not model, counted where
    /// that would otherwise have let the branch be elided as having no effect.
    pub unlifted_instructions: usize,
    pub target_va_symbol_calls: usize,
    /// Why structured emission declined for this function, if it did, and every
    /// traversal limit that omitted an edge. Both are primary facts; the generic
    /// decline count and the rollback count are derived from them.
    pub emission: EmissionAccounting,
}

#[derive(Debug, Clone, Default)]
pub struct PoolSemanticHint {
    pub selector: Option<String>,
    pub owner_class: Option<String>,
    pub library_uri: Option<String>,
    pub target_va: Option<u64>,
}

#[derive(Debug, Default, Clone)]
struct LiftState {
    reg_values: HashMap<String, String>,
    selector_hints: HashMap<String, String>,
    last_cmp: Option<(String, String)>,
    /// Per register, the value the most recent call dropped from it, and the
    /// snapshot that value was read out of.
    ///
    /// This lives on `LiftState` rather than beside it because the emitter
    /// clones and restores path state around branch arms and loop bodies. A
    /// side table would keep an arm's clobber after the arm was rolled back and
    /// annotate the fall-through path with a value that path never held.
    call_clobbers: HashMap<String, CallClobber>,
}

/// What one call took from one register.
#[derive(Debug, Clone)]
struct CallClobber {
    /// Address of the clobbering instruction, which is this site's key.
    call_va: u64,
    /// The value held immediately before the call, never after it.
    value: String,
    snapshot_id: String,
}

#[derive(Debug, Clone)]
struct JoinCandidates {
    /// The rendered list: the candidate values deduplicated by first occurrence
    /// over `provenance`'s canonical order.
    values: Vec<String>,
    /// Every actual predecessor contributed a usable candidate, so the rendered
    /// list is an exhaustive claim rather than evidence. Rendered arity does not
    /// decide this: dedup can collapse three covered predecessors to one value.
    complete: bool,
    /// One attribution per emitted candidate, in ascending predecessor id, with
    /// duplicates retained - two predecessors carrying the same value are one
    /// rendered value and two attributions. This is audit data; output only reads
    /// `values`.
    provenance: Vec<control_flow::JoinCandidateProvenance>,
}

/// The register state a block ended with, kept until the annotation pass can
/// record the snapshot its candidates cite. Its index in `block_snapshots` is its
/// identity, which is what the audit's `snapshot_id` is built from.
///
/// Every snapshot is retained rather than one per block: a block emitted twice
/// would otherwise overwrite the state a recorded candidate was read from, and
/// the audit would cite a snapshot whose contents no longer exist.
#[derive(Debug, Clone)]
struct BlockSnapshot {
    block: usize,
    reg_values: HashMap<String, String>,
}

/// A rendered join body and the candidate key it owns. This Vec follows the
/// structured render order; it is never derived by iterating the lookup HashMap.
#[derive(Debug, Clone)]
struct JoinAnnotationAnchor {
    join: usize,
    /// Canonical registers with recorded candidates, sorted before output.
    candidate_regs: Vec<String>,
    /// Render-time lines. Later passes only modify or add surrounding lines;
    /// final insertion finds surviving body lines in their render order.
    lines: Vec<String>,
}

/// One unresolved read of a register an ordinary call clobbered.
///
/// Captured when the line is rendered, because that is the only moment the
/// register is known to be both unbound and unbound *by this call*. The site key
/// the audit reports is read straight off this anchor, so a record cannot claim
/// a site other than the one that produced its annotation.
#[derive(Debug, Clone)]
struct CallAnnotationAnchor {
    call_va: u64,
    /// Canonical name, e.g. `x9`.
    register: String,
    value: String,
    snapshot_id: String,
    /// Index of the line carrying the read, in the body as it was rendered. The
    /// finished line is found by aligning that body against the finished one,
    /// never by searching the finished text for something that looks like it.
    line_index: usize,
}

struct FuncEmitter<'a> {
    ir: &'a FunctionIr,
    symbol_names: &'a HashMap<u64, String>,
    pool_value_hints: HashMap<u64, String>,
    pool_semantic_hints: HashMap<u64, PoolSemanticHint>,
    /// Per-target facts for calls into known runtime stubs; empty when the SDK
    /// table did not apply, in which case every call is modelled as a Dart call.
    runtime_stubs: HashMap<u64, RuntimeStubEffect>,
    locals: BTreeMap<i64, String>,
    block_by_id: HashMap<usize, &'a BasicBlock>,
    va_to_id: HashMap<u64, usize>,
    dispatch_calls: HashMap<u64, DispatchCall>,
    regions: Option<Regions>,
    /// Monotonic across the whole function. Kept off `LiftState` so restoring a
    /// saved state cannot re-issue a name that already denotes another value.
    call_index: usize,
    structured_emitted: HashSet<usize>,
    loop_stack: Vec<(usize, Option<usize>)>,
    /// Arm-end values for a structured branch's follow block. This stays
    /// outside `LiftState`: restoring path state and merging registers must not
    /// erase an annotation that describes the values just discarded.
    join_candidates: HashMap<(usize, String), JoinCandidates>,
    /// Candidate keys captured with a branch, sorted before its join renders.
    /// This keeps the lookup table out of every output-affecting iteration.
    join_candidate_regs: HashMap<usize, Vec<String>>,
    /// Join bodies in deterministic render order. Final insertion uses their
    /// pre-sorted candidate keys to avoid any side-table iteration.
    join_annotation_anchors: Vec<JoinAnnotationAnchor>,
    /// One per unresolved read of a call-clobbered register, in render order.
    /// The rendering anchor is the only source of this site's key, so the
    /// audit cannot name a site the annotation was not emitted at.
    call_annotation_anchors: Vec<CallAnnotationAnchor>,
    /// The body exactly as it was rendered, kept so a call anchor's line can be
    /// followed through the rewrites that come after. Taken once, when emission
    /// ends.
    render_lines: Vec<String>,
    /// Monotonic within the function; names one pre-call snapshot.
    snapshot_index: usize,
    /// Set while a call statement is being rendered, so its own line is not
    /// annotated with an earlier call's value under the words "this call".
    rendering_call: bool,
    call_provenance: FunctionProvenance,
    /// End states of blocks that precede an annotatable join, in capture order.
    /// A join is emitted after its predecessors, and the merge there drops the
    /// bindings, so the state has to be kept rather than recomputed.
    block_snapshots: Vec<BlockSnapshot>,
    /// Audit rows the join annotations owe, kept per loss site so each site's
    /// records stay in their own output order. Empty unless the run asked for an
    /// audit.
    join_provenance: FunctionProvenance,
    /// Loop headers whose candidates were captured as a loop-entry site.
    ///
    /// Recorded at capture, not re-derived when the annotation is inserted: a
    /// loop header with several predecessors is also a join, so the site is not a
    /// property of the block id alone, and one classification with two readers
    /// cannot disagree with itself the way two derivations can.
    loop_annotation_sites: HashSet<usize>,
    /// Audit rows the loop-entry annotations owe. Its own stream, because
    /// `write_function_provenance` walks a stream in output order and a
    /// concatenation of two sites' rows is not in output order.
    loop_provenance: FunctionProvenance,
    emitted: HashSet<usize>,
    active_stack: Vec<usize>,
    inline_visits: HashMap<usize, usize>,
    omitted_blocks: BTreeSet<usize>,
    /// For each omitted block, the block whose edge first asked for it. The
    /// helper budget is spent long after that edge was walked, so without this
    /// the omission event for a block the budget refused would have no source to
    /// name.
    omission_sources: BTreeMap<usize, usize>,
    /// Omitted blocks the helper budget refused to define. Their call sites
    /// carry an explicit omission instead of a call to a definition that is not
    /// there.
    helper_cap_omitted: BTreeSet<usize>,
    loop_back_edges: BTreeSet<usize>,
    loop_context: Vec<usize>,
    /// Built once on first use by the DFS emitter, which has no `Regions` to ask.
    /// Predecessors, per-block written registers, and which blocks have more
    /// than one predecessor, for merging state where paths converge.
    dfs_preds: Option<HashMap<usize, Vec<usize>>>,
    dfs_block_writes: HashMap<usize, HashSet<String>>,
    lines: Vec<String>,
    /// Identifier renames the naming pass applied to the body, longest key first.
    ///
    /// Annotation candidates are captured while a line is rendered, which is
    /// *before* `apply_name_and_type_hints` runs, so a captured value spells its
    /// locals `argN` / `local_mN` / `local_pN` - names that no longer exist by the
    /// time the annotation is inserted. Replaying this map onto the candidate is
    /// what keeps an annotation referring to an identifier the reader can find.
    identifier_renames: Vec<(String, String)>,

    state: LiftState,
    placeholder_ifs: usize,
    unresolved_cf: usize,
    raw_register_calls: usize,
    total_calls: usize,
    indirect_calls: usize,
    semantic_direct_calls: usize,
    semantic_indirect_calls: usize,
    dispatch_selector_calls: usize,
    dispatch_table_calls: usize,
    repeated_blocks: usize,
    unlifted_instructions: usize,
    target_va_symbol_calls: usize,
    accounting: EmissionAccounting,
    /// The primary cause the structured walk stopped at, set by the first site
    /// that refused. Later refusals are consequences of it, so the first wins.
    decline_site: Option<StructuredDecline>,
}

/// Which emitter runs.
///
/// `Auto` is what every public entry point uses: structured emission first, the
/// DFS walk when it declines. `DirectDfs` runs the DFS walk without the attempt,
/// so a declined function can be compared with what it is supposed to equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmissionPlan {
    Auto,
    DirectDfs,
}

#[derive(Debug, Clone)]
struct HelperMeta {
    id: usize,
    start: usize,
    end: usize,
    body_lines: Vec<String>,
    return_expr: Option<String>,
}

#[derive(Debug, Clone)]
struct InlineHelperPlan {
    lines: Vec<String>,
    append_null_return: bool,
}

#[derive(Debug, Default, Clone)]
struct IdentStats {
    field_access: usize,
    arith_ops: usize,
    pool_assign: usize,
    null_cmp: usize,
    call_assign: usize,
}

mod control_flow;
mod helper_flow;
mod helpers;
mod passes;

use control_flow::Regions;
use control_flow::{JOIN_LOSS_SITE, LOOP_LOSS_SITE};
use helpers::*;

pub use control_flow::{
    EmissionAccounting, StructuredDecline, StructuredDeclineCause, TraversalEvent,
    TraversalEventKind, TraversalTarget,
};

pub use helpers::{
    AnnotationLiteral, ANNOTATION_LITERALS, EXHAUSTIVE_JOIN_ANNOTATION, LOOP_ENTRY_ANNOTATION,
    NON_EXHAUSTIVE_JOIN_ANNOTATION, PRE_CALL_ANNOTATION,
};

/// Remove every value annotation from a source line, whichever loss site
/// emitted it. Other emitter comments remain observable to preserve the
/// historical quality ruler.
pub fn strip_join_annotation_span(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let mut end = index + 1;
            while end < bytes.len() {
                match bytes[end] {
                    b'\\' => end += 2,
                    b'"' => {
                        end += 1;
                        break;
                    }
                    _ => end += 1,
                }
            }
            out.push_str(&line[index..end.min(bytes.len())]);
            index = end.min(bytes.len());
            continue;
        }
        let Some(literal) = annotation_at(&bytes[index..]) else {
            out.push(bytes[index] as char);
            index += 1;
            continue;
        };
        let Some(span) = literal.span_len(&bytes[index..]) else {
            out.push_str(&line[index..]);
            break;
        };
        index += span;
    }
    out
}

/// Return the code span before a value annotation, whichever loss site emitted
/// it. Analyses use this borrowed prefix; rewrites deliberately keep operating
/// on complete lines.
pub(crate) fn code_before_annotation(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            index += 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' => index += 2,
                    b'"' => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            continue;
        }
        if annotation_at(&bytes[index..]).is_some() {
            return &line[..index];
        }
        index += 1;
    }
    line
}

impl<'a> FuncEmitter<'a> {
    fn new(ir: &'a FunctionIr, symbol_names: &'a HashMap<u64, String>) -> Self {
        let offsets = collect_stack_offsets(ir);
        let mut locals = BTreeMap::new();
        for off in offsets {
            locals.insert(off, local_name(off));
        }

        let mut block_by_id = HashMap::new();
        let mut va_to_id = HashMap::new();
        for b in &ir.blocks {
            block_by_id.insert(b.id, b);
            va_to_id.insert(b.start_va, b.id);
        }

        Self {
            ir,
            symbol_names,
            pool_value_hints: HashMap::new(),
            pool_semantic_hints: HashMap::new(),
            runtime_stubs: HashMap::new(),
            locals,
            block_by_id,
            va_to_id,
            dispatch_calls: dispatch_table_calls(ir),
            regions: None,
            call_index: 0,
            structured_emitted: HashSet::new(),
            loop_stack: Vec::new(),
            join_candidates: HashMap::new(),
            join_candidate_regs: HashMap::new(),
            join_annotation_anchors: Vec::new(),
            call_annotation_anchors: Vec::new(),
            render_lines: Vec::new(),
            snapshot_index: 0,
            rendering_call: false,
            call_provenance: FunctionProvenance {
                function_id: ir.function_id,
                loss_site: CALL_LOSS_SITE,
                ..FunctionProvenance::default()
            },
            block_snapshots: Vec::new(),
            join_provenance: FunctionProvenance {
                function_id: ir.function_id,
                loss_site: JOIN_LOSS_SITE,
                ..FunctionProvenance::default()
            },
            loop_annotation_sites: HashSet::new(),
            loop_provenance: FunctionProvenance {
                function_id: ir.function_id,
                loss_site: LOOP_LOSS_SITE,
                ..FunctionProvenance::default()
            },
            emitted: HashSet::new(),
            active_stack: Vec::new(),
            inline_visits: HashMap::new(),
            omitted_blocks: BTreeSet::new(),
            omission_sources: BTreeMap::new(),
            helper_cap_omitted: BTreeSet::new(),
            loop_back_edges: BTreeSet::new(),
            loop_context: Vec::new(),
            dfs_preds: None,
            dfs_block_writes: HashMap::new(),
            lines: Vec::new(),
            identifier_renames: Vec::new(),
            state: init_state(),
            placeholder_ifs: 0,
            unresolved_cf: 0,
            raw_register_calls: 0,
            total_calls: 0,
            indirect_calls: 0,
            semantic_direct_calls: 0,
            semantic_indirect_calls: 0,
            dispatch_selector_calls: 0,
            dispatch_table_calls: 0,
            repeated_blocks: 0,
            unlifted_instructions: 0,
            target_va_symbol_calls: 0,
            accounting: EmissionAccounting::default(),
            decline_site: None,
        }
    }

    /// The artifact plus the audit rows its annotations owe, one set per loss
    /// site.
    ///
    /// The rows leave here without an output coordinate. A program-level rewrite
    /// still runs over the finished source and can move text on an annotated
    /// line, so the coordinate is derived from the artifact after that, by
    /// finding the annotation span itself.
    ///
    /// One set per site rather than one merged list, because that search is
    /// monotonic: it resumes where the previous record was found, so identical
    /// spans are told apart by order. Two sites' records interleave in the
    /// output, so merging them into one list would leave the second site's rows
    /// searching from a cursor already past their own annotations.
    fn emit_with_provenance(self) -> (PseudocodeArtifact, Vec<FunctionProvenance>) {
        self.emit_with_plan(EmissionPlan::Auto)
    }

    /// The same emission under a chosen plan.
    ///
    /// `Auto` is the production decision and the only one any public entry point
    /// uses. `DirectDfs` enters the fallback without attempting to structure,
    /// which is what a declined function is supposed to be equal to: it is the
    /// reference the decline is compared against, not a second transition.
    fn emit_with_plan(
        mut self,
        plan: EmissionPlan,
    ) -> (PseudocodeArtifact, Vec<FunctionProvenance>) {
        let fn_name = sanitize_name(&self.ir.name);

        // One parameter per register the Dart convention passes an argument in.
        // A lower bound, not a signature: arguments past the sixth are
        // stack-passed and not modelled.
        let params = (0..DART_ARGUMENT_REGISTERS.len())
            .map(|i| format!("dynamic arg{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.lines.push(format!("dynamic {fn_name}({params}) {{"));
        for name in self.locals.values() {
            self.lines.push(format!("  var {};", name));
        }
        if !self.locals.is_empty() {
            self.lines.push(String::new());
        }

        let body_start = self.lines.len();
        // Structured emission first: it emits every reachable block exactly
        // once. It declines on irreducible control flow, and verifies the
        // emit-once invariant rather than assuming it, so a failure rolls back
        // and the DFS emitter runs instead.
        let structured = match plan {
            EmissionPlan::Auto => self.try_emit_structured(),
            EmissionPlan::DirectDfs => false,
        };
        if !structured {
            if let Some(entry) = self.ir.blocks.first() {
                self.emit_block(entry.id, 1, 0);
            }
        }

        // Last resort for a body that came out empty, which means the DFS emitter
        // above declined every path. Gated on the structured attempt having
        // failed: a successful one has already emitted every reachable block, and
        // running this anyway would let both emitters contribute to one body,
        // because `emitted` tracks the DFS emitter alone and is empty after a
        // structured success. Not currently reachable, since no function on either
        // sample has an entry block with no instructions, but the guard states the
        // intent rather than relying on that.
        let body_lines = self.lines.len().saturating_sub(body_start);
        if !structured && body_lines == 0 {
            for b in &self.ir.blocks {
                if self.emitted.contains(&b.id) {
                    continue;
                }
                self.emit_block(b.id, 1, 0);
                break;
            }
        }

        // The rendered body, before a single rewrite touches it. Every call
        // anchor indexes into this.
        self.render_lines = self.lines.clone();

        self.lines.push("}".to_string());
        if !self.omitted_blocks.is_empty() {
            self.lines.push(String::new());
            self.append_helper_functions();
            self.inline_trivial_helpers();
            self.resolve_remaining_helpers();
        }
        self.insert_loop_summary_comment();
        self.compact_lines();
        for line in &mut self.lines {
            *line = Self::clean_expr(line.clone());
        }
        self.apply_name_and_type_hints(&fn_name);
        self.extract_minus_one_aliases();
        // Before the appenders: they replay the renames onto candidates, and the
        // audit's snapshot rule compares a candidate against the snapshot it cites,
        // so both sides have to be in the same namespace.
        self.normalize_provenance_namespace();
        self.append_join_annotations();
        self.append_call_annotations();

        let artifact = PseudocodeArtifact {
            function_id: self.ir.function_id,
            function_name: fn_name,
            source: self.lines.join("\n"),
            placeholder_ifs: self.placeholder_ifs,
            unresolved_cf: self.unresolved_cf,
            raw_register_calls: self.raw_register_calls,
            total_calls: self.total_calls,
            indirect_calls: self.indirect_calls,
            semantic_direct_calls: self.semantic_direct_calls,
            semantic_indirect_calls: self.semantic_indirect_calls,
            dispatch_selector_calls: self.dispatch_selector_calls,
            dispatch_table_calls: self.dispatch_table_calls,
            repeated_blocks: self.repeated_blocks,
            unlifted_instructions: self.unlifted_instructions,
            target_va_symbol_calls: self.target_va_symbol_calls,
            emission: self.accounting,
        };
        (
            artifact,
            vec![
                self.call_provenance,
                self.join_provenance,
                self.loop_provenance,
            ],
        )
    }

    fn push_line(&mut self, indent: usize, line: &str) {
        let line = format!("{}{}", "  ".repeat(indent), line);
        // Before the line is owned by `lines`, because the anchor needs the
        // register state as it stood when the read was rendered: after the push
        // the emitter may bind, drop or restore any of it.
        self.record_call_annotation_anchors(&line);
        self.lines.push(line);
    }

    fn emit_omitted_path(&mut self, indent: usize, block_id: Option<usize>) {
        if let Some(id) = block_id {
            self.omitted_blocks.insert(id);
            let source = self.current_source_block();
            self.omission_sources.entry(id).or_insert(source);
            self.push_line(indent, &format!("return _block_{}();", id));
        } else {
            self.push_line(indent, "/* path omitted */");
        }
    }

    /// The block the walk is inside right now, which is the source of any edge
    /// it declines to render. The entry block when nothing is on the stack,
    /// which is where a helper body starts.
    fn current_source_block(&self) -> usize {
        self.active_stack
            .last()
            .copied()
            .or_else(|| self.ir.blocks.first().map(|b| b.id))
            .unwrap_or(0)
    }

    fn block_start_va(&self, id: usize) -> u64 {
        self.block_by_id
            .get(&id)
            .map(|b| b.start_va)
            .unwrap_or(self.ir.entry_va)
    }

    /// Record one traversal omission, keyed by function, source block, target
    /// and ordinal.
    fn record_traversal_event(
        &mut self,
        kind: TraversalEventKind,
        source: usize,
        target: TraversalTarget,
    ) {
        let function_id = self.ir.function_id;
        let source_start_va = self.block_start_va(source);
        self.accounting
            .record_event(kind, function_id, source_start_va, target);
    }
}

/// What a call to a known runtime stub does, read from the SDK per stub slot.
///
/// The Dart calling convention does not describe a stub's inputs, so a call to
/// one gets no inferred argument list either way.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeStubEffect {
    /// The stub defines a value in `SharedSlowPathStubABI::kResultReg` (`R0`).
    /// When false, binding the call's result claims a value that does not exist.
    pub writes_result: bool,
    /// The stub saves and restores every non-reserved register around its
    /// runtime call, so it clobbers nothing a normal call would.
    pub preserves_registers: bool,
}

/// The marker the one diagnostic of an unusable CFG carries.
///
/// Public so a consumer can recognise the artifact for what it is instead of
/// matching on prose, and so a fixture cannot drift from what the emitter writes.
pub const INVALID_CFG_NOTE: &str = "invalid CFG";

/// The whole artifact for a `FunctionIr` no consumer may index.
///
/// Nothing here reads `blocks`, so no relation is computed off a graph whose
/// identity does not hold and neither emitter runs: a body would have to invent
/// the flow the graph failed to state. The single diagnostic names the defect,
/// and the unresolved-control-flow counter reports the whole body as one
/// unresolved site, which is what it is.
fn invalid_cfg_artifact(ir: &FunctionIr, defect: &CfgDefect) -> PseudocodeArtifact {
    let function_name = sanitize_name(&ir.name);
    let params = (0..DART_ARGUMENT_REGISTERS.len())
        .map(|i| format!("dynamic arg{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    PseudocodeArtifact {
        function_id: ir.function_id,
        source: format!(
            "dynamic {function_name}({params}) {{\n  // {INVALID_CFG_NOTE}: {defect}: control flow not recovered\n}}"
        ),
        function_name,
        placeholder_ifs: 0,
        unresolved_cf: 1,
        raw_register_calls: 0,
        total_calls: 0,
        indirect_calls: 0,
        semantic_direct_calls: 0,
        semantic_indirect_calls: 0,
        dispatch_selector_calls: 0,
        dispatch_table_calls: 0,
        repeated_blocks: 0,
        unlifted_instructions: 0,
        target_va_symbol_calls: 0,
        // Neither emitter ran, so there is no decline to attribute and no
        // traversal to omit anything.
        emission: EmissionAccounting::default(),
    }
}

/// The one entry every public emission function funnels through, so the
/// validation gate cannot be reached around.
///
/// Validation runs before `FuncEmitter::new`, which is where `block_by_id` and
/// `va_to_id` are built: after those maps exist the duplicate the check is
/// looking for has already been collapsed into one entry.
fn emit_one(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
    runtime_stubs: &HashMap<u64, RuntimeStubEffect>,
) -> (PseudocodeArtifact, Vec<FunctionProvenance>) {
    emit_one_with_plan(
        ir,
        symbol_names,
        pool_value_hints,
        pool_semantic_hints,
        runtime_stubs,
        EmissionPlan::Auto,
    )
}

fn emit_one_with_plan(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
    runtime_stubs: &HashMap<u64, RuntimeStubEffect>,
    plan: EmissionPlan,
) -> (PseudocodeArtifact, Vec<FunctionProvenance>) {
    if let Err(defect) = validate_block_identity(ir) {
        return (invalid_cfg_artifact(ir, &defect), Vec::new());
    }
    let mut emitter = FuncEmitter::new(ir, symbol_names);
    emitter.pool_value_hints = pool_value_hints.clone();
    emitter.pool_semantic_hints = pool_semantic_hints.clone();
    emitter.runtime_stubs = runtime_stubs.clone();
    match plan {
        EmissionPlan::Auto => emitter.emit_with_provenance(),
        EmissionPlan::DirectDfs => emitter.emit_with_plan(plan),
    }
}

/// The artifact the DFS walk produces on its own.
///
/// A declined structured attempt must equal this apart from its cause
/// accounting, so this is the reference that comparison is made against, by this
/// crate's own differential and by anything else that wants to see what
/// structuring bought for one function.
pub fn emit_pseudocode_direct_dfs(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
) -> PseudocodeArtifact {
    emit_one_with_plan(
        ir,
        symbol_names,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        EmissionPlan::DirectDfs,
    )
    .0
}

pub fn emit_pseudocode(ir: &FunctionIr, symbol_names: &HashMap<u64, String>) -> PseudocodeArtifact {
    let empty = HashMap::new();
    emit_pseudocode_with_pool_context(ir, symbol_names, &empty, &HashMap::new())
}

pub fn emit_pseudocode_with_pool_hints(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
) -> PseudocodeArtifact {
    let empty = HashMap::new();
    emit_pseudocode_with_pool_context(ir, symbol_names, pool_value_hints, &empty)
}

pub fn emit_pseudocode_with_pool_context(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
) -> PseudocodeArtifact {
    emit_one(
        ir,
        symbol_names,
        pool_value_hints,
        pool_semantic_hints,
        &HashMap::new(),
    )
    .0
}

pub fn emit_program(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
) -> Vec<PseudocodeArtifact> {
    let empty = HashMap::new();
    emit_program_with_pool_hints(ir, symbol_names, &empty)
}

pub fn emit_program_with_pool_hints(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
) -> Vec<PseudocodeArtifact> {
    let empty = HashMap::new();
    emit_program_with_pool_context(ir, symbol_names, pool_value_hints, &empty)
}

pub fn emit_program_with_pool_context(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
) -> Vec<PseudocodeArtifact> {
    let empty = HashMap::new();
    emit_program_with_runtime_stubs(
        ir,
        symbol_names,
        pool_value_hints,
        pool_semantic_hints,
        &empty,
    )
}

/// As `emit_program_with_pool_context`, plus what the SDK says a call to each
/// known runtime stub does. Without the table every call is modelled as a normal
/// Dart call: result bound, caller-saved registers dropped. Both are wrong for a
/// shared stub, and those are the commonest calls in the binary.
pub fn emit_program_with_runtime_stubs(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
    runtime_stubs: &HashMap<u64, RuntimeStubEffect>,
) -> Vec<PseudocodeArtifact> {
    let (mut artifacts, provenance): (Vec<_>, Vec<_>) = ir
        .iter()
        .map(|f| {
            emit_one(
                f,
                symbol_names,
                pool_value_hints,
                pool_semantic_hints,
                runtime_stubs,
            )
        })
        .unzip();
    apply_program_level_generic_call_rewrites(&mut artifacts);
    // After the rewrite, never before: it substitutes a callee name on lines
    // that can also carry an annotation, which moves every column to its right.
    // An audit coordinate taken earlier would point at the wrong byte.
    if audit_enabled() {
        for (artifact, provenance) in artifacts.iter().zip(&provenance) {
            for site in provenance {
                write_function_provenance(&artifact.source, site);
            }
        }
    }
    artifacts
}

fn apply_program_level_generic_call_rewrites(artifacts: &mut [PseudocodeArtifact]) {
    let aliases = collect_generic_symbol_aliases(artifacts);
    if aliases.is_empty() {
        return;
    }
    for artifact in artifacts {
        let mut changed = false;
        let mut lines = Vec::new();
        for line in artifact.source.lines() {
            if let Some(rewritten) = rewrite_generic_call_line(line, &aliases) {
                lines.push(rewritten);
                changed = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if changed {
            artifact.source = lines.join("\n");
        }
    }
}

fn collect_generic_symbol_aliases(artifacts: &[PseudocodeArtifact]) -> HashMap<String, String> {
    let mut candidates: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut generic_reference_counts: HashMap<String, usize> = HashMap::new();
    for artifact in artifacts {
        for line in artifact.source.lines() {
            let Some((callee, original)) = extract_rewrite_evidence(line) else {
                if let Some(callee) = extract_call_callee(line) {
                    if is_generic_call_name(callee) {
                        *generic_reference_counts
                            .entry(callee.to_string())
                            .or_insert(0) += 1;
                    }
                }
                continue;
            };
            *generic_reference_counts
                .entry(original.clone())
                .or_insert(0) += 1;
            let by_name = candidates.entry(original).or_default();
            *by_name.entry(callee).or_insert(0) += 1;
        }
    }

    let mut aliases = HashMap::new();
    for (original, by_name) in candidates {
        let mut ranked = by_name.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|(a_name, a_count), (b_name, b_count)| {
            b_count
                .cmp(a_count)
                .then_with(|| semantic_name_score(b_name).cmp(&semantic_name_score(a_name)))
                .then_with(|| a_name.cmp(b_name))
        });
        let Some((best_name, best_count)) = ranked.first().cloned() else {
            continue;
        };
        if best_count < 2 {
            continue;
        }
        let total_references = generic_reference_counts
            .get(&original)
            .copied()
            .unwrap_or(best_count);
        if best_count * 2 < total_references {
            continue;
        }
        if let Some((_, second_count)) = ranked.get(1) {
            if *second_count * 2 >= best_count {
                continue;
            }
        }
        aliases.insert(original, best_name);
    }
    aliases
}

fn semantic_name_score(name: &str) -> usize {
    if name.starts_with("flutter.") {
        return 5;
    }
    if name.starts_with("package:") {
        return 4;
    }
    if name.starts_with("dart.") {
        return 3;
    }
    if name.starts_with("dart_vm.") {
        return 2;
    }
    1
}

fn extract_rewrite_evidence(line: &str) -> Option<(String, String)> {
    if !line.contains("was: ") {
        return None;
    }
    let eq = line.find("= ")?;
    let call_start = eq + 2;
    let open = line[call_start..].find('(')? + call_start;
    let callee = line[call_start..open].trim().to_string();
    if callee.is_empty() || callee.starts_with("sub_") || callee.starts_with("fn_0x") {
        return None;
    }
    let was_idx = line.find("was: ")? + 5;
    let tail = &line[was_idx..];
    let original = tail
        .split([',', ' ', ')'])
        .find(|s| !s.trim().is_empty())?
        .trim()
        .to_string();
    if !original.starts_with("sub_") && !original.starts_with("fn_0x") {
        return None;
    }
    Some((callee, original))
}

fn rewrite_generic_call_line(line: &str, aliases: &HashMap<String, String>) -> Option<String> {
    if line.contains("was: ") {
        return None;
    }
    let eq = line.find("= ")?;
    let call_start = eq + 2;
    let open = line[call_start..].find('(')? + call_start;
    let original = line[call_start..open].trim();
    if !original.starts_with("sub_") && !original.starts_with("fn_0x") {
        return None;
    }
    let alias = aliases.get(original)?;

    let mut rewritten = String::new();
    rewritten.push_str(&line[..call_start]);
    rewritten.push_str(alias);
    rewritten.push_str(&line[open..]);
    if let Some(comment_idx) = rewritten.find("//") {
        rewritten.insert_str(comment_idx + 2, &format!(" inferred from: {}, ", original));
    } else {
        rewritten.push_str(&format!(" // inferred from: {}", original));
    }
    Some(rewritten)
}

fn extract_call_callee(line: &str) -> Option<&str> {
    let eq = line.find("= ")?;
    let call_start = eq + 2;
    let open = line[call_start..].find('(')? + call_start;
    Some(line[call_start..open].trim())
}

fn is_generic_call_name(name: &str) -> bool {
    name.starts_with("sub_") || name.starts_with("fn_0x")
}

/// Nested-span instrumentation for the phase benchmark.
///
/// Region analysis runs inside emission, so an emitter span that included it
/// would overlap the CFG span. The emitter charges its region-analysis time
/// here and the harness subtracts it, which is the only way the two spans can
/// be disjoint without moving the call out of the emitter.
///
/// Counting is per thread and never synchronised: the harness is single
/// threaded, and an atomic would put a contended read-modify-write inside the
/// span it is supposed to be measuring.
#[cfg(feature = "bench-spans")]
pub mod bench_spans {
    use std::cell::Cell;

    thread_local! {
        static CFG_NANOS: Cell<u64> = const { Cell::new(0) };
    }

    /// Nanoseconds charged to region analysis on this thread since the last
    /// take, clearing the counter. Reading and clearing together means a caller
    /// cannot leave a previous function's time attributed to the next one.
    pub fn take_cfg_nanos() -> u64 {
        CFG_NANOS.with(|c| c.replace(0))
    }

    pub(crate) fn add_cfg_nanos(nanos: u64) {
        CFG_NANOS.with(|c| c.set(c.get().saturating_add(nanos)));
    }
}

#[cfg(test)]
mod tests;
