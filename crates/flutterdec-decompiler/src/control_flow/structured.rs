// Renders a function by walking its region structure, emitting every reachable
// basic block exactly once.
//
// The emit-once invariant is checked rather than assumed: reaching a block that
// has already been emitted, and that is neither the enclosing loop's header nor
// its exit, is a structural failure. The whole function then falls back to the
// DFS emitter, so this pass can only improve output, never truncate it.

/// Quality counters, saved and restored around a structuring attempt.
pub(super) struct Counters {
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
}

/// What a block's terminator does, once its body has been emitted.
enum Flow {
    /// Control leaves the function.
    Ends,
    /// Straight-line continuation.
    Goto(usize),
    /// Two-way branch with a rendered condition.
    Branch {
        condition: String,
        taken: Option<usize>,
        not_taken: Option<usize>,
        raw_target: String,
    },
}

// The side table is lookup-only; candidate text is sorted before output, so no map or
// set iteration reaches output.

/// The site tag every key this site writes carries. One constant, so the loss
/// site's name and its key space cannot drift apart, and the three key spaces
/// stay disjoint by construction rather than by hoping block-id ranges never meet
/// an instruction address.
pub(crate) const JOIN_LOSS_SITE: &str = "join";

/// The tag of a candidate's `path_key`: an incoming path at a join is one
/// predecessor block. Deliberately not the site's own tag - a predecessor is not
/// a join, and labelling it one would claim a construct that is not there.
const JOIN_PATH_KIND: &str = "block";

/// The loop-header site's name in the coverage ledger. A loop header reached
/// from several arms is also a join, and loop semantics win: the back-edge value
/// is never rendered at the header, so join semantics would eventually claim a
/// value that is not there.
pub(crate) const LOOP_LOSS_SITE: &str = "loop_entry";

/// The site tag of a loop-entry `site_key`, which is `("loop", header)`. Distinct
/// from `JOIN_LOSS_SITE` so a block that is both a header and a join lands in one
/// key space rather than being claimed twice at one output coordinate.
const LOOP_SITE_TAG: &str = "loop";

/// Whether the audit rows are worth building at all.
///
/// A run that did not ask for an audit builds none: the rows are kept per
/// function until the artifact is final, and the corpus run that measures emitted
/// output must not carry instrumentation it never reads. Test builds always
/// build them, so the rows themselves are assertable without an audit directory
/// and without a process-wide environment variable that whichever test ran first
/// would have decided for everyone.
fn annotation_provenance_wanted() -> bool {
    audit_enabled() || cfg!(test)
}

/// One annotation the emitter has decided to insert: where it goes, what it
/// says, and the site it came from. The site travels with the insertion so the
/// audit cannot derive it a second time and disagree.
struct PlannedJoinAnnotation {
    line: usize,
    /// Byte offset the annotation is inserted at, which is the end of the
    /// register token it describes.
    at: usize,
    text: String,
    join: usize,
    register: String,
}

/// One annotation the insertion path decided not to insert, before the site it
/// belongs to is looked up.
///
/// The block id is kept rather than the site tag: a loop header is also a join
/// by predecessor count, and the classification made at capture is the single
/// reader of that difference everywhere else in this file.
struct PlannedCapOmission {
    join: usize,
    register: String,
    rendered: String,
    reason: &'static str,
    line_len: usize,
    planned_len: usize,
}

/// Exact provenance of one candidate: the predecessor whose end snapshot held
/// it, and the id of that snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinCandidateProvenance {
    /// The predecessor block this value came from. Not an arm root: an arm root
    /// is not the block whose end state produced the value, and a join can have
    /// predecessors that are no branch arm at all.
    pub pred: usize,
    pub value: String,
    pub snapshot_id: String,
}

/// The canonical candidate order: ascending predecessor id.
///
/// One order shared by the audit array and the rendered list is what makes the
/// two comparable. Each deduplicates by first occurrence over it, so without a
/// shared order they would dedup independently and disagree while both being
/// stable across runs - which a cross-run byte-identity check cannot catch.
/// Value breaks the tie so the order is total whatever the input order was.
pub(crate) fn ordered_join_candidate_provenance(
    values: impl IntoIterator<Item = JoinCandidateProvenance>,
) -> Vec<JoinCandidateProvenance> {
    let mut ordered = values.into_iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.pred
            .cmp(&right.pred)
            .then_with(|| left.value.cmp(&right.value))
    });
    ordered
}

/// The rendered value list: first occurrence over the canonical order, with
/// equal values collapsed. Every attribution stays in the provenance, including
/// the duplicates collapsed here - two predecessors carrying one value are one
/// rendered value and two attributions, and the audit is where both survive.
pub(crate) fn rendered_candidate_values(provenance: &[JoinCandidateProvenance]) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for candidate in provenance {
        if !values.iter().any(|value| value == &candidate.value) {
            values.push(candidate.value.clone());
        }
    }
    values
}

/// The three forms a candidate may take. There is no fourth: the rule is a
/// whitelist, so a spelling nobody anticipated is rejected by default rather
/// than emitted until someone thinks to name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateForm {
    /// `0`, `0x3b`, `-1`.
    Literal,
    /// A dotted chain: `arg0.f16`, `thread.f104.f1968`.
    FieldAccess,
    /// A call spanning the whole value: `smiTag(local_m16)`.
    Call,
}

/// Which allowed form `value` is, or `None` when it is none of them.
///
/// Classification is over the **whole** value, not over a substring of it. A
/// containment test accepts anything with an allowed form buried in it -
/// `(thread.f80 + 1)` contains a field access and is not one - and that is the
/// gap a positively stated rule closes. Truncated and malformed text falls out
/// of the same test rather than needing its own: an unbalanced or unterminated
/// value matches no form.
///
/// Every loss site - join, loop entry and pre-call - classifies through this one
/// function, and the atom-level rejection below comes from
/// `unrecovered_value_spellings` alone. A site with its own list is a partial
/// subset of that set, which is how four defects on this branch produced a
/// convincing false pass.
pub(crate) fn candidate_form(value: &str) -> Option<CandidateForm> {
    // The value is classified exactly as it will be rendered, surrounding
    // whitespace included. Trimming here would accept a candidate the reader is
    // shown untrimmed, and an independent scan of the emitted text would then
    // disagree with the filter that emitted it.
    if value.is_empty() || value.trim() != value || contains_uninformative_token(value) {
        return None;
    }
    if is_numeric_literal(value) {
        return Some(CandidateForm::Literal);
    }
    if is_field_access(value) {
        return Some(CandidateForm::FieldAccess);
    }
    if is_call_shaped(value) {
        return Some(CandidateForm::Call);
    }
    None
}

pub(crate) fn is_informative_annotation_candidate(value: &str) -> bool {
    candidate_form(value).is_some()
}

fn is_identifier(token: &str) -> bool {
    let mut bytes = token.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// An identifier followed by at least one `.field`. The base is an identifier
/// too, so an indexed or parenthesised base - `local_m8.f12[0x107]`,
/// `((arg0.f24 + 1)).f24` - is not a field access.
fn is_field_access(value: &str) -> bool {
    let mut segments = value.split('.');
    let Some(base) = segments.next() else {
        return false;
    };
    if !is_identifier(base) {
        return false;
    }
    let mut fields = 0usize;
    for segment in segments {
        if !is_identifier(segment) {
            return false;
        }
        fields += 1;
    }
    fields > 0
}

/// A callee - identifier or field chain - and one argument list closing on the
/// last byte. `foo(1) + 2` and `(a + b)(c)` are not calls; neither is a value
/// whose brackets do not balance.
fn is_call_shaped(value: &str) -> bool {
    let Some(open) = value.find('(') else {
        return false;
    };
    let callee = &value[..open];
    if !(is_identifier(callee) || is_field_access(callee)) {
        return false;
    }
    let rest = &value[open..];
    let mut parens = 0isize;
    let mut brackets = 0isize;
    for (index, byte) in rest.bytes().enumerate() {
        match byte {
            b'(' => parens += 1,
            b')' => {
                parens -= 1;
                if parens < 0 {
                    return false;
                }
                if parens == 0 {
                    return index + 1 == rest.len() && brackets == 0;
                }
            }
            b'[' => brackets += 1,
            b']' => {
                brackets -= 1;
                if brackets < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    false
}

fn is_numeric_literal(value: &str) -> bool {
    let value = value.trim().strip_prefix('-').unwrap_or(value.trim());
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
            |hex| !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        )
}

/// A candidate containing a fallback register or local temporary would merely
/// decorate one admitted gap with another, so omit the whole candidate.
fn contains_uninformative_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut start = 0usize;
    while start < bytes.len() {
        while start < bytes.len()
            && !bytes[start].is_ascii_alphanumeric()
            && bytes[start] != b'_'
        {
            start += 1;
        }
        let end = start
            + bytes[start..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                .count();
        let token = &value[start..end];
        if is_unrecovered_value_spelling(token) || is_opaque_temporary(token) {
            return true;
        }
        start = end.saturating_add(1);
    }
    false
}

/// Replay the body's identifier renames onto a candidate captured before them.
///
/// `arg0` becomes `slot0`, which is live in the signature line, so `slot0.f8` stays
/// a field access on an identifier the reader can find - the annotation was only
/// ever wrong about the spelling. `local_m32` usually becomes `tmpN`, which
/// `candidate_form` then rejects as one gap decorating another; that rejection is
/// correct and was previously hidden by the stale spelling.
///
/// Applied longest-key-first by `sort_rename_pairs`, so `local_m8` cannot corrupt
/// `local_m88`, and token-anchored by `replace_identifier_token`, so it cannot
/// rewrite a substring of a longer name.
pub(crate) fn replay_identifier_renames(value: &str, renames: &[(String, String)]) -> String {
    let mut out = value.to_string();
    for (from, to) in renames {
        out = FuncEmitter::replace_identifier_token(&out, from, to);
    }
    out
}

/// Every identifier token in the body, snapshotted before any annotation is
/// inserted.
///
/// Built once rather than scanning `self.lines` per token for two reasons. The
/// insertion loop mutates those lines as it goes, so a token could otherwise count
/// as live because an *earlier annotation* mentioned it rather than because the code
/// does - annotations validating each other. And the scan was
/// O(candidates x tokens x lines x line length) on functions running to hundreds of
/// lines, where this is a hash lookup. Being order-independent by construction also
/// keeps it out of `VAL-DETERM-011`'s way.
fn live_identifier_tokens(body: &[String]) -> HashSet<String> {
    let mut live = HashSet::new();
    for line in body {
        let bytes = line.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            if !(bytes[index].is_ascii_alphabetic() || bytes[index] == b'_') {
                index += 1;
                continue;
            }
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            live.insert(line[start..index].to_string());
        }
    }
    live
}

/// Whether every identifier a candidate names still exists in the emitted body.
///
/// A candidate is captured while its line is rendered, which is *before*
/// `apply_name_and_type_hints` runs, so it spells locals `argN`, `local_mN` and
/// `local_pN`. Those names are gone by insertion time: the body now says `slotN`,
/// `tmpN`, `poolValN` or `resultTmpN`. An annotation that survives with the old
/// spelling names an identifier that appears nowhere in the file, which is the
/// feature failing at its only job - telling a reader which value a register held.
///
/// Rejecting is the honest outcome rather than replaying the rename map, and two
/// facts decided it. A rename cannot fix every case: when a value was captured from
/// dataflow state that was never rendered into a line there is no rename entry, so
/// the annotation would still dangle. And where a rename does exist it usually maps
/// to `tmpN`, which `contains_uninformative_token` already rejects - `candidate_form`
/// promises the value "is classified exactly as it will be rendered", so emitting
/// text the filter would refuse would break that contract and fail an independent
/// scan of the emitted output.
///
/// The rule is **structural, not a spelling list**. A list of naming families is
/// fail-open: it passes silently the next time the naming pass gains a family, which
/// is how this defect and a stale provenance fixture both got here. So every
/// identifier must be present in the body unless its position proves it is not a
/// local: followed by `(` it is a callee (`smiTag`, `bitField`, `classId`), preceded
/// by `.` it is a field (`f8`, `_tag`), and `RESERVED_EMITTER_IDENTIFIERS` covers the
/// globals the emitter renders without ever renaming. Numeric literals name nothing.
fn candidate_names_only_live_locals(value: &str, live: &HashSet<String>) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if !(bytes[index].is_ascii_alphabetic() || bytes[index] == b'_') {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
            index += 1;
        }
        let token = &value[start..index];
        // Preceded by `.`: a field name, not an identifier in scope.
        if start > 0 && bytes[start - 1] == b'.' {
            continue;
        }
        // Followed by `(`: a callee. Emitted helpers are not locals.
        if bytes.get(index) == Some(&b'(') {
            continue;
        }
        if RESERVED_EMITTER_IDENTIFIERS.contains(&token) {
            continue;
        }
        if !live.contains(token) {
            return false;
        }
    }
    true
}

fn is_unrecovered_value_spelling(token: &str) -> bool {
    (0..=30).any(|index| {
        unrecovered_value_spellings(&format!("x{index}"))
            .iter()
            .any(|spelling| token == spelling)
    })
}

/// Prefixes for a value whose name states nothing about where it came from.
/// `objTmp` and `intTmp` were listed here until the naming pass stopped
/// asserting a type from usage counts; both now render as `tmp`, which this
/// already matches, so dropping them changed no candidate's verdict - verified
/// by annotation counts unmoved at 4,369 and 7,246.
///
/// **This is not "every prefix the naming pass emits", and must not become
/// that.** `slotN` and `poolValN` are deliberately absent. A parameter was
/// never opaque - `receiver` and `paramN` were not listed either - and
/// `poolValN` states an observed source, so both are legitimate annotation
/// candidates. Adding either would silently reject a whole class of candidates,
/// moving annotation coverage and the quality counters: a ruler change.
fn is_opaque_temporary(token: &str) -> bool {
    ["t", "tmp", "resultTmp"].iter().any(|prefix| {
        token.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}

/// Comments pass through brace-sensitive compaction, so a candidate needing
/// escaping is omitted rather than allowed to alter later structural rewrites.
///
/// Shared by every loss site, and deliberately separate from
/// `is_informative_annotation_candidate`: capture keeps a safe-but-uninformative
/// candidate in the provenance record, and it is that record which decides
/// whether a site's evidence is exhaustive.
pub(crate) fn is_recordable_annotation_candidate(value: &str) -> bool {
    // The brace and comment-terminator test is asked of the authority that owns
    // the terminator, for the same reason the separator test is: a second
    // spelling here would drift the moment either delimiter is reworded.
    !contains_forbidden_sequence(value)
        // A value containing the separator renders identically to two values, so
        // the list stops being readable back into candidates. Asked of the
        // authority that owns the separator rather than spelled here.
        && !contains_candidate_separator(value)
}

fn canonical_register_spelling(token: &str) -> Option<String> {
    (0..=30).find_map(|index| {
        let canonical = format!("x{index}");
        unrecovered_value_spellings(&canonical)
            .iter()
            .any(|spelling| spelling == token)
            .then_some(canonical)
    })
}

fn contains_identifier_token(line: &str, needle: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = line[offset..].find(needle) {
        let start = offset + found;
        let end = start + needle.len();
        if (start == 0 || !FuncEmitter::is_ident_char(line.as_bytes()[start - 1] as char))
            && (end == line.len() || !FuncEmitter::is_ident_char(line.as_bytes()[end] as char))
        {
            return true;
        }
        offset = end;
    }
    false
}

fn register_tokens(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if !bytes[index].is_ascii_alphabetic() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && FuncEmitter::is_ident_char(bytes[index] as char) {
            index += 1;
        }
        if start == 0 || !FuncEmitter::is_ident_char(bytes[start - 1] as char) {
            if let Some(reg) = canonical_register_spelling(&line[start..index]) {
                tokens.push(reg);
            }
        }
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

impl<'a> FuncEmitter<'a> {
    /// Emit the whole function from its region structure. Returns false when the
    /// function is irreducible or the walk would have to emit a block twice, in
    /// which case nothing has been appended and the caller should use the DFS
    /// emitter instead.
    pub(super) fn try_emit_structured(&mut self) -> bool {
        let Some(regions) = Regions::build(self.ir) else {
            return false;
        };

        let saved_lines = self.lines.len();
        let saved_state = self.state.clone();
        let saved_counters = self.counter_snapshot();
        // A call anchor holds the index of the line it was rendered on, so an
        // abandoned attempt leaves indices pointing into a body that no longer
        // exists. Left in place they resolve against the DFS emitter's lines and
        // annotate whatever happens to be there.
        let saved_call_anchors = self.call_annotation_anchors.len();
        let saved_call_snapshots = self.call_provenance.snapshots.len();

        self.regions = Some(regions);
        let ok = self.render_sequence(0, None, 1, 0);
        let covered = self.structured_emitted.len()
            == self
                .regions
                .as_ref()
                .map(Regions::reachable_count)
                .unwrap_or(0);

        if ok && covered {
            return true;
        }

        self.lines.truncate(saved_lines);
        self.state = saved_state;
        self.restore_counters(saved_counters);
        self.structured_emitted.clear();
        self.loop_stack.clear();
        self.join_candidates.clear();
        self.join_candidate_regs.clear();
        self.join_annotation_anchors.clear();
        self.loop_annotation_sites.clear();
        // The DFS emitter annotates nothing, so a rollback must also drop the
        // snapshots and the audit rows the abandoned structuring captured: a
        // snapshot no surviving annotation cites is a record of a site that is
        // not in the output.
        self.block_snapshots.clear();
        self.join_provenance.snapshots.clear();
        self.call_annotation_anchors.truncate(saved_call_anchors);
        self.call_provenance.snapshots.truncate(saved_call_snapshots);
        self.join_provenance.records.clear();
        self.loop_provenance.snapshots.clear();
        self.loop_provenance.records.clear();
        self.regions = None;
        false
    }

    /// Counters saved before a structuring attempt, so a rollback to the DFS
    /// emitter does not double count. Named rather than positional: the first
    /// version was an array, and inserting three fields silently rotated four of
    /// them onto each other's values.
    pub(super) fn counter_snapshot(&self) -> Counters {
        Counters {
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
        }
    }

    pub(super) fn restore_counters(&mut self, c: Counters) {
        let Counters {
            placeholder_ifs,
            unresolved_cf,
            raw_register_calls,
            total_calls,
            indirect_calls,
            semantic_direct_calls,
            semantic_indirect_calls,
            dispatch_selector_calls,
            dispatch_table_calls,
            repeated_blocks,
            unlifted_instructions,
            target_va_symbol_calls,
        } = c;
        self.placeholder_ifs = placeholder_ifs;
        self.unresolved_cf = unresolved_cf;
        self.raw_register_calls = raw_register_calls;
        self.total_calls = total_calls;
        self.indirect_calls = indirect_calls;
        self.semantic_direct_calls = semantic_direct_calls;
        self.semantic_indirect_calls = semantic_indirect_calls;
        self.dispatch_selector_calls = dispatch_selector_calls;
        self.dispatch_table_calls = dispatch_table_calls;
        self.repeated_blocks = repeated_blocks;
        self.unlifted_instructions = unlifted_instructions;
        self.target_va_symbol_calls = target_va_symbol_calls;
    }

    /// Emit blocks from `start` up to but excluding `follow`, which is the
    /// enclosing region's continuation.
    fn render_sequence(
        &mut self,
        start: usize,
        follow: Option<usize>,
        indent: usize,
        depth: usize,
    ) -> bool {
        if depth > 64 {
            return false;
        }
        let mut cursor = Some(start);

        while let Some(id) = cursor {
            if Some(id) == follow {
                return true;
            }
            if let Some(&(_, loop_follow)) = self.loop_stack.last() {
                if Some(id) == loop_follow {
                    self.push_line(indent, "break;");
                    return true;
                }
            }
            if self.structured_emitted.contains(&id) {
                // A back edge to the innermost enclosing loop. An outer loop
                // would need a labelled `continue`, which is declined for now.
                if self.loop_stack.last().map(|(h, _)| *h) == Some(id) {
                    self.push_line(indent, "continue;");
                    return true;
                }
                if !self.is_repeatable_region(id, follow) {
                    // Neither a back edge nor a small shared region, so the
                    // region tree does not describe this edge.
                    return false;
                }
                self.repeated_blocks += 1;
            }

            let regions = self.regions.as_ref().expect("regions");
            if !regions.is_reachable(id) {
                return false;
            }
            if regions.is_loop_header(id) && !self.loop_stack.iter().any(|(h, _)| *h == id) {
                match self.render_loop(id, indent, depth) {
                    Some(next) => {
                        cursor = next;
                        continue;
                    }
                    None => return false,
                }
            }

            let is_join = regions.is_join(id);
            // A natural loop header can also be a join - reached from two arms
            // plus its back edge. It is a loop site, whatever its predecessor
            // count: the back-edge value is never rendered, so join semantics
            // would eventually claim a value that is not there, and a join-tagged
            // record at a site the loop pass also claims is a double claim at one
            // output coordinate. The merge below is unchanged for it; only capture
            // and annotation decline.
            let annotatable_join = is_join && !regions.is_loop_header(id);
            self.structured_emitted.insert(id);
            if is_join {
                // Emitted once, so no single incoming path describes this
                // block's register state. Anything a predecessor could have
                // redefined is dropped; the rest still holds its entry value.
                let preds = regions.predecessors(id).to_vec();
                let written = self.registers_written_between(&preds, Some(id));
                if annotatable_join {
                    // Before the merge, and from the same write set the merge
                    // uses: a register the merge keeps still holds its value, and
                    // annotating it would report a loss that did not happen.
                    self.record_join_candidates(id, &preds, &written);
                }
                self.merge_state_at_join(&written);
            }
            let annotation_start = self.lines.len();
            let flow = self.render_block_body(id, indent);
            self.snapshot_block_end(id);
            // A loop header carries its own candidates, captured by `render_loop`
            // before the merge that dropped them, so it needs the same anchor a
            // join gets. The site declined the join capture above; declining the
            // anchor too would leave the candidates with nothing to attach to.
            if annotatable_join || self.loop_annotation_sites.contains(&id) {
                let candidate_regs = self.join_candidate_regs.remove(&id).unwrap_or_default();
                self.join_annotation_anchors.push(JoinAnnotationAnchor {
                    join: id,
                    candidate_regs,
                    lines: self.lines[annotation_start..].to_vec(),
                });
            }

            match flow {

                Flow::Ends => return true,
                Flow::Goto(next) => cursor = Some(next),
                Flow::Branch {
                    condition,
                    taken,
                    not_taken,
                    raw_target,
                } => {
                    let regions = self.regions.as_ref().expect("regions");
                    let mut region_follow = regions.follow_of(id);
                    // A follow node outside the enclosing loop is that loop's
                    // exit, which its own `break` check handles.
                    if let Some(&(header, _)) = self.loop_stack.last() {
                        if region_follow.is_some_and(|f| !regions.in_loop(header, f)) {
                            region_follow = None;
                        }
                    }
                    if region_follow == Some(id) {
                        region_follow = None;
                    }

                    // Each arm starts from the state at the branch, and neither
                    // arm's bindings escape it. Without this a value defined in
                    // an arm that returns is still referenced afterwards, which
                    // is how the DFS emitter avoided the problem: by duplicating
                    // the continuation per path instead of merging.
                    let state_at_branch = self.state.clone();
                    let mut arm_ends = Vec::with_capacity(2);
                    

                    // Arms are rendered into buffers so emptiness is decided on
                    // what they actually emit, which includes merge assignments.
                    let buffer_start = self.lines.len();
                    match taken {
                        Some(t) if Some(t) != region_follow => {
                            if !self.render_sequence(t, region_follow, indent + 1, depth + 1) {
                                return false;
                            }
                        }
                        Some(_) => {}
                        None => {
                            if raw_target.starts_with("0x") {
                                self.push_line(indent + 1, "/* external branch */");
                            } else {
                                self.unresolved_cf += 1;
                                self.push_line(indent + 1, "// unresolved branch target");
                            }
                        }
                    }
                    let taken_state = self.state.clone();
                    let taken_lines: Vec<String> = self.lines.split_off(buffer_start);
                    if let Some(arm) = taken {
                        arm_ends.push((arm, taken_state));
                    }

                    self.state = state_at_branch.clone();
                    if let Some(f) = not_taken {
                        if Some(f) != region_follow
                            && !self.render_sequence(f, region_follow, indent + 1, depth + 1)
                        {
                            return false;
                        }
                    }
                    let else_state = self.state.clone();
                    let else_lines: Vec<String> = self.lines.split_off(buffer_start);
                    if let Some(arm) = not_taken {
                        arm_ends.push((arm, else_state));
                    }

                    // An arm can also be empty because the lifter does not model
                    // its instructions. Eliding then deletes real computation, so
                    // an empty arm carrying unmodelled work says so instead.
                    let mut taken_lines = taken_lines;
                    let mut else_lines = else_lines;
                    for (lines, arm) in [(&mut taken_lines, taken), (&mut else_lines, not_taken)] {
                        if !lines.is_empty() {
                            continue;
                        }
                        let unlifted = self.unlifted_on_arm(arm, region_follow);
                        if unlifted > 0 {
                            self.unlifted_instructions += unlifted;
                            lines.push(format!(
                                "{}// {} instructions not lifted",
                                "  ".repeat(indent + 1),
                                unlifted
                            ));
                        }
                    }

                    match (taken_lines.is_empty(), else_lines.is_empty()) {
                        // Both arms only reach the join, so the test decides
                        // nothing that the output records.
                        (true, true) => {}
                        // Only the other arm has content: state it directly
                        // rather than as an empty `if` with an `else`.
                        (true, false) => {
                            self.push_line(indent, &format!("if (!({})) {{", condition));
                            self.lines.extend(else_lines);
                            self.push_line(indent, "}");
                        }
                        (false, true) => {
                            self.push_line(indent, &format!("if ({}) {{", condition));
                            self.lines.extend(taken_lines);
                            self.push_line(indent, "}");
                        }
                        (false, false) if else_lines.is_empty() => {
                            self.push_line(indent, &format!("if ({}) {{", condition));
                            self.lines.extend(taken_lines);
                            self.push_line(indent, "}");
                        }
                        (false, false) => {
                            self.push_line(indent, &format!("if ({}) {{", condition));
                            self.lines.extend(taken_lines);
                            self.push_line(indent, "}");
                            self.push_line(indent, "else {");
                            self.lines.extend(else_lines);
                            self.push_line(indent, "}");
                        }
                    }

                    cursor = region_follow;
                    self.state = state_at_branch;
                    if let Some(join) = cursor {
                        // Reached from both arms, so a binding survives only if
                        // neither arm redefined it.
                        let arms = arm_ends.iter().map(|(arm, _)| *arm).collect::<Vec<_>>();
                        let written = self.registers_written_between(&arms, Some(join));
                        self.merge_state_at_join(&written);
                    }
                }
            }
        }

        true
    }

    /// Emit a natural loop as `while (true) { ... }`. Returns where control
    /// continues afterwards, or `None` if the body could not be structured.
    fn render_loop(&mut self, header: usize, indent: usize, depth: usize) -> Option<Option<usize>> {
        let loop_follow = self.regions.as_ref().expect("regions").loop_follow_of(header);
        self.push_line(indent, "while (true) {");
        self.loop_stack.push((header, loop_follow));
        // The header is re-entered by the back edge, so only bindings the loop
        // body never writes survive into it, and the same holds after the loop.
        let written = self.registers_written_between(&[header], None);
        let state_before = self.state.clone();
        // Before the merge below, and from the same write set it uses: a register
        // the merge keeps still holds its value, so annotating it would report a
        // loss that did not happen.
        self.record_loop_entry_candidates(header, &written);
        self.merge_state_at_join(&written);
        let ok = self.render_sequence(header, loop_follow, indent + 1, depth + 1);
        self.loop_stack.pop();
        self.push_line(indent, "}");
        if !ok {
            return None;
        }
        self.state = state_before;
        self.merge_state_at_join(&written);
        Some(loop_follow)
    }

    /// Emit a block's non-terminator instructions and classify its terminator.
    fn render_block_body(&mut self, id: usize, indent: usize) -> Flow {
        let Some(block) = self.block_by_id.get(&id).copied() else {
            return Flow::Ends;
        };

        for ins in &block.instrs {
            match ins.op {
                IROp::Call => self.emit_call(&ins.target, ins.va, indent),
                IROp::LoadPool => {
                    let ops = split_operands(&ins.src);
                    if let Some(dst) = ops.first().and_then(|o| canonical_reg(o)) {
                        let rhs = if ins.target.is_empty() {
                            "pool[?]".to_string()
                        } else {
                            ins.target.clone()
                        };
                        // A pool load rebinds the register, so whatever a call
                        // took from it earlier no longer describes it.
                        self.state.call_clobbers.remove(&dst);
                        self.state.reg_values.insert(dst, Self::clean_expr(rhs));
                    }
                }
                IROp::RuntimeCheck => {}
                IROp::Other => self.apply_other_lift(&ins.src, indent),
                IROp::Return => {
                    let ret = self
                        .capped_reg_value("x0")
                        .unwrap_or_else(|| "null".to_string());
                    self.push_line(indent, &format!("return {};", ret));
                    return Flow::Ends;
                }
                IROp::Jump => {
                    let target = self.branch_target_block(&ins.target);
                    return match target {
                        Some(t) => Flow::Goto(t),
                        None => {
                            let normalized = normalize_target(&ins.target);
                            if normalized.starts_with("0x") {
                                self.push_line(indent, &format!("return tailCall_{}();", normalized));
                            } else {
                                self.unresolved_cf += 1;
                                self.push_line(indent, "// unresolved jump");
                            }
                            Flow::Ends
                        }
                    };
                }
                IROp::Branch => {
                    let (mnemonic, ops) = split_instruction(&ins.src);
                    let condition = match self.branch_condition(&mnemonic, &ops) {
                        Some(c) => Self::clean_expr(c),
                        None => {
                            self.placeholder_ifs += 1;
                            "/* cond */".to_string()
                        }
                    };
                    let taken = self.branch_target_block(&ins.target);
                    let not_taken = self
                        .regions
                        .as_ref()
                        .expect("regions")
                        .successors(id)
                        .iter()
                        .copied()
                        .find(|s| Some(*s) != taken);
                    return Flow::Branch {
                        condition,
                        taken,
                        not_taken,
                        raw_target: normalize_target(&ins.target),
                    };
                }
            }
        }

        // No terminator: falls through to the single successor.
        match self.regions.as_ref().expect("regions").successors(id) {
            [next] => Flow::Goto(*next),
            _ => Flow::Ends,
        }
    }

    /// Whether a block already emitted may be emitted again, bounding how much
    /// is duplicated.
    ///
    /// Dart has no `goto`, so a shared continuation that is not the follow node
    /// of the branch being structured cannot be named at all: the only choices
    /// are to repeat it, to hoist it into a helper, or to give up on structuring
    /// the function. Giving up means the DFS emitter, whose duplication is
    /// unbounded, so repeating a small region is strictly the smaller cost.
    ///
    /// The commonest instance is Dart's shared non-returning slow path for null,
    /// bounds and type checks: a few instructions ending in a throw or deopt
    /// stub, many predecessors, no successors. It post-dominates nothing, so it
    /// is never a follow node, and it alone accounted for 84% of the fallbacks.
    ///
    /// A repeated region may end at the innermost enclosing loop header, which
    /// renders as `continue;`. Any other loop header is still rejected: entering
    /// one would duplicate a nested loop body or target the wrong `continue`.
    ///
    /// The 16-block, 96-instruction budget bounds the remaining duplication and
    /// stays below the fourfold alternative's pathological tail.
    fn is_repeatable_region(&self, id: usize, follow: Option<usize>) -> bool {
        const MAX_REPEATED_BLOCKS: usize = 16;
        const MAX_REPEATED_INSTRUCTIONS: usize = 96;
        let Some(regions) = self.regions.as_ref() else {
            return false;
        };

        let mut seen: HashSet<usize> = HashSet::new();
        let mut instructions = 0usize;
        let enclosing_loop = self.loop_stack.last().map(|(header, _)| *header);
        let mut stack = vec![id];
        while let Some(block) = stack.pop() {
            if Some(block) == follow
                || Some(block) == enclosing_loop
                || !seen.insert(block)
            {
                continue;
            }
            if regions.is_loop_header(block) || seen.len() > MAX_REPEATED_BLOCKS {
                return false;
            }
            instructions += self.block_by_id.get(&block).map_or(0, |b| b.instrs.len());
            if instructions > MAX_REPEATED_INSTRUCTIONS {
                return false;
            }
            stack.extend(regions.successors(block).iter().copied());
        }
        true
    }

    /// How many instructions on an arm the lifter does not model.
    ///
    /// An arm that emits nothing may simply have no effect, or may be full of
    /// work the lifter cannot express. Treating the two alike would delete real
    /// computation, so the count decides, and it is reported at the site rather
    /// than only in aggregate.
    fn unlifted_on_arm(&self, arm: Option<usize>, stop: Option<usize>) -> usize {
        let Some(arm) = arm else { return 0 };
        if Some(arm) == stop {
            return 0;
        }
        let Some(regions) = self.regions.as_ref() else {
            // The caller adds this to a counter and prints it, so a sentinel
            // would overflow and render as a nonsense instruction count.
            return 0;
        };
        let mut unmodelled = 0usize;
        let mut seen: HashSet<usize> = HashSet::new();
        let mut stack = vec![arm];
        while let Some(id) = stack.pop() {
            if Some(id) == stop || !seen.insert(id) {
                continue;
            }
            if let Some(block) = self.block_by_id.get(&id) {
                for ins in &block.instrs {
                    if !matches!(ins.op, IROp::Other) {
                        continue;
                    }
                    let (mnemonic, _) = split_instruction(&ins.src);
                    if !Self::lifts_mnemonic(&mnemonic) {
                        unmodelled += 1;
                    }
                }
            }
            stack.extend(regions.successors(id).iter().copied());
        }
        unmodelled
    }

    /// Registers written by any block reachable from `roots` before `stop`.
    ///
    /// A binding survives a merge only if no path into it redefines the
    /// register, which is exactly this set's complement.
    pub(crate) fn registers_written_between(&self, roots: &[usize], stop: Option<usize>) -> HashSet<String> {
        let mut written = HashSet::new();
        let Some(regions) = self.regions.as_ref() else {
            return written;
        };
        let mut seen: HashSet<usize> = HashSet::new();
        let mut stack: Vec<usize> = roots.iter().copied().filter(|r| Some(*r) != stop).collect();

        while let Some(id) = stack.pop() {
            if Some(id) == stop || !seen.insert(id) {
                continue;
            }
            if let Some(block) = self.block_by_id.get(&id) {
                for ins in &block.instrs {
                    let (mnemonic, ops) = split_instruction(&ins.src);
                    written.extend(written_registers(&mnemonic, &ops));
                    if matches!(ins.op, IROp::Call) {
                        // The same set the lifter drops at a call, so the two
                        // cannot disagree: `(0..18)` swept in SPREG, which a call
                        // preserves, and omitted x18, which it does not.
                        written.extend(CALL_CLOBBERED_REGISTERS.iter().map(|r| (*r).to_string()));
                    }
                }
            }
            stack.extend(regions.successors(id).iter().copied());
        }
        written
    }
    /// Keep this block's end state when it precedes a join that can be
    /// annotated.
    ///
    /// A candidate may only come from the end state of the predecessor it is
    /// attributed to, and the join is emitted after all of them, so the state has
    /// to be kept: by the time the join renders, its merge has dropped exactly
    /// the bindings the annotation describes. Blocks that precede no annotatable
    /// join are not snapshotted, which keeps this off the hot path for the
    /// majority of blocks.
    fn snapshot_block_end(&mut self, block: usize) {
        let precedes_annotatable_merge = self.regions.as_ref().is_some_and(|regions| {
            regions.successors(block).iter().any(|succ| {
                (regions.is_join(*succ) && !regions.is_loop_header(*succ))
                    // A loop header's entry paths need the same treatment, and
                    // for the same reason: `render_loop` merges before the header
                    // renders, so by then the entry values are gone. The back edge
                    // is not an entry path - its value is never rendered at the
                    // header - so a block inside the loop is not snapshotted for it.
                    || (regions.is_loop_header(*succ) && !regions.in_loop(*succ, block))
            })
        });
        if !precedes_annotatable_merge {
            return;
        }
        self.block_snapshots.push(BlockSnapshot {
            block,
            reg_values: self.state.reg_values.clone(),
        });
    }

    /// The most recent recorded end state of `block`, with its capture index. A
    /// repeated region emits a block more than once, and it is the state the join
    /// actually merged - the last one before the join renders - that the candidate
    /// came from.
    fn latest_block_snapshot(&self, block: usize) -> Option<(usize, &BlockSnapshot)> {
        self.block_snapshots
            .iter()
            .enumerate()
            .rev()
            .find(|(_, snapshot)| snapshot.block == block)
    }

    /// The audit id of one predecessor's end state as cited by one join.
    ///
    /// The capture index is part of it because a repeated region snapshots a
    /// block twice, and the two are different states; the join is part of it
    /// because the record is what that join dropped along that path.
    fn join_snapshot_id(join: usize, pred: usize, capture: usize) -> String {
        format!("join:{}:pred:{}:{}", join, pred, capture)
    }

    /// Record the snapshots one register's candidates cite, once each.
    ///
    /// The whole register map goes in, not only the cited register: a snapshot
    /// listing just the value being claimed is a restatement of the claim rather
    /// than something it can disagree with.
    fn record_join_snapshots(&mut self, join: usize, provenance: &[JoinCandidateProvenance]) {
        if !annotation_provenance_wanted() {
            return;
        }
        for candidate in provenance {
            if self
                .join_provenance
                .snapshots
                .iter()
                .any(|snapshot| snapshot.snapshot_id == candidate.snapshot_id)
            {
                continue;
            }
            let Some(registers) = self.latest_block_snapshot(candidate.pred).and_then(
                |(capture, snapshot)| {
                    // The id is rebuilt rather than trusted: a snapshot recorded
                    // under an id that does not name its own capture is how a
                    // value from a sibling path would pass unnoticed.
                    (Self::join_snapshot_id(join, candidate.pred, capture)
                        == candidate.snapshot_id)
                        .then(|| {
                            let mut registers: Vec<(String, String)> = snapshot
                                .reg_values
                                .iter()
                                .map(|(reg, value)| (reg.clone(), value.clone()))
                                .collect();
                            registers.sort();
                            registers
                        })
                },
            ) else {
                continue;
            };
            self.join_provenance.snapshots.push(ValueSnapshot {
                snapshot_id: candidate.snapshot_id.clone(),
                // The path this snapshot is the end state of, not the join that
                // dropped it - the same key the loop site records, and the one
                // the shared checker pairs against the candidate's own
                // `path_key`. Naming the join here made that pairing agree with
                // itself for a value borrowed from any sibling predecessor.
                site_key: SiteKey(JOIN_PATH_KIND, candidate.pred as u64),
                registers,
            });
        }
    }

    /// Remember the values the join's merge is about to drop, one attribution per
    /// predecessor that carried a usable one.
    ///
    /// The predecessor set is the join's own, not the branch's two arms. A join
    /// with a third incoming path used to be skipped whole, and an arm root is not
    /// the block whose end state produced the value: the arm's last block is, and
    /// it is that block a candidate is attributed to.
    ///
    /// Completeness is coverage of that predecessor set, not rendered arity:
    /// dedup can collapse three covered predecessors into one rendered value and
    /// the claim is still exhaustive.
    pub(crate) fn record_join_candidates(
        &mut self,
        join: usize,
        preds: &[usize],
        written: &HashSet<String>,
    ) {
        if preds.is_empty() {
            return;
        }
        let mut regs: Vec<String> = written
            .iter()
            .filter(|reg| pinned_value(reg).is_none() && *reg != "x15")
            .cloned()
            .collect();
        regs.sort();

        for reg in regs {
            // Ascending predecessor id, which `Regions::predecessors` already is,
            // and re-established by the shared order in case it ever is not.
            let provenance = ordered_join_candidate_provenance(preds.iter().filter_map(|pred| {
                let (capture, snapshot) = self.latest_block_snapshot(*pred)?;
                let value = Self::capped_expr(snapshot.reg_values.get(&reg)?)?;
                // Both filters, here rather than at render time: a candidate that
                // cannot be rendered is not a candidate this predecessor
                // contributed, and counting it as coverage would let an
                // unrenderable value claim the exhaustive form.
                (is_recordable_annotation_candidate(value)
                    && is_informative_annotation_candidate(value))
                .then(|| JoinCandidateProvenance {
                    pred: *pred,
                    value: value.to_string(),
                    snapshot_id: Self::join_snapshot_id(join, *pred, capture),
                })
            }));
            if provenance.is_empty() {
                continue;
            }
            self.record_join_snapshots(join, &provenance);
            let complete = preds
                .iter()
                .all(|pred| provenance.iter().any(|candidate| candidate.pred == *pred));
            let values = rendered_candidate_values(&provenance);
            self.join_candidates.insert(
                (join, reg.clone()),
                JoinCandidates {
                    complete,
                    values,
                    provenance,
                },
            );
            self.join_candidate_regs.entry(join).or_default().push(reg);
        }
        if let Some(regs) = self.join_candidate_regs.get_mut(&join) {
            regs.sort();
            regs.dedup();
        }
    }

    /// The audit id of one entry predecessor's end state as cited by one loop
    /// header. Same shape as the join form and a different prefix, so the two key
    /// spaces stay disjoint at a block that is both.
    fn loop_snapshot_id(header: usize, pred: usize, capture: usize) -> String {
        format!("loop:{}:pred:{}:{}", header, pred, capture)
    }

    /// Record the snapshots one loop-entry candidate list cites, once each.
    ///
    /// The whole register map goes in, not only the cited register: a snapshot
    /// listing just the value being claimed is a restatement of the claim rather
    /// than something it can disagree with.
    fn record_loop_entry_snapshots(&mut self, header: usize, provenance: &[JoinCandidateProvenance]) {
        if !annotation_provenance_wanted() {
            return;
        }
        for candidate in provenance {
            if self
                .loop_provenance
                .snapshots
                .iter()
                .any(|snapshot| snapshot.snapshot_id == candidate.snapshot_id)
            {
                continue;
            }
            let Some(registers) = self.latest_block_snapshot(candidate.pred).and_then(
                |(capture, snapshot)| {
                    // The id is rebuilt rather than trusted: a snapshot recorded
                    // under an id that does not name its own capture is how a
                    // value from a sibling entry path would pass unnoticed.
                    (Self::loop_snapshot_id(header, candidate.pred, capture)
                        == candidate.snapshot_id)
                        .then(|| {
                            let mut registers: Vec<(String, String)> = snapshot
                                .reg_values
                                .iter()
                                .map(|(reg, value)| (reg.clone(), value.clone()))
                                .collect();
                            registers.sort();
                            registers
                        })
                },
            ) else {
                continue;
            };
            self.loop_provenance.snapshots.push(ValueSnapshot {
                snapshot_id: candidate.snapshot_id.clone(),
                // The path this snapshot is the end state of, not the site that
                // dropped it: the checker pairs it against the candidate's own
                // `path_key`, and naming the header here would make that pairing
                // agree with itself for a value from any entry arm.
                site_key: SiteKey(JOIN_PATH_KIND, candidate.pred as u64),
                registers,
            });
        }
    }

    /// Remember the values the loop header's merge is about to drop, one
    /// attribution per non-back-edge predecessor that carried a usable one.
    ///
    /// Capture is per predecessor, and that is the whole difficulty of this site.
    /// `render_loop` merges before the header renders, so at a header reached from
    /// two arms holding 7 and 9 the merged state holds the drop, not both values:
    /// reading it there produces correct-looking output at single-entry headers and
    /// silently nothing at exactly the multi-entry headers this site owns.
    ///
    /// The back edge is excluded by construction. Its value is not rendered at the
    /// header, so it is not an entry value, and the temporal qualifier in the
    /// literal is what keeps the claim honest: only what held on entry.
    ///
    /// Every loop header is this site's, whatever its predecessor count. A header
    /// reached from several arms is also a join, and the join capture declines it.
    pub(crate) fn record_loop_entry_candidates(&mut self, header: usize, written: &HashSet<String>) {
        let entry_preds: Vec<usize> = {
            let Some(regions) = self.regions.as_ref() else {
                return;
            };
            regions
                .predecessors(header)
                .iter()
                .copied()
                .filter(|pred| !regions.in_loop(header, *pred))
                .collect()
        };
        if entry_preds.is_empty() {
            return;
        }
        // Recorded whatever the capture below finds, because it is the site
        // classification the literal and the audit key both read. A header with no
        // usable candidate simply has nothing to annotate.
        self.loop_annotation_sites.insert(header);

        let mut regs: Vec<String> = written
            .iter()
            .filter(|reg| pinned_value(reg).is_none() && *reg != "x15")
            .cloned()
            .collect();
        regs.sort();

        for reg in regs {
            // Ascending predecessor id, which `Regions::predecessors` already is,
            // through the same shared order the join site uses: the rendered list
            // and the audit array deduplicate by first occurrence over it, and two
            // independent orders would disagree while both being stable across runs.
            let provenance =
                ordered_join_candidate_provenance(entry_preds.iter().filter_map(|pred| {
                    let (capture, snapshot) = self.latest_block_snapshot(*pred)?;
                    let value = Self::capped_expr(snapshot.reg_values.get(&reg)?)?;
                    (is_recordable_annotation_candidate(value)
                        && is_informative_annotation_candidate(value))
                    .then(|| JoinCandidateProvenance {
                        pred: *pred,
                        value: value.to_string(),
                        snapshot_id: Self::loop_snapshot_id(header, *pred, capture),
                    })
                }));
            if provenance.is_empty() {
                continue;
            }
            self.record_loop_entry_snapshots(header, &provenance);
            let values = rendered_candidate_values(&provenance);
            self.join_candidates.insert(
                (header, reg.clone()),
                JoinCandidates {
                    // There is one loop form, never the exhaustive one. The
                    // temporal qualifier already scopes the claim to entry, so it
                    // asserts no present value and needs no exhaustiveness marker -
                    // and the back-edge value, which is not rendered here, is
                    // exactly what an exhaustive claim would be wrong about.
                    complete: false,
                    values,
                    provenance,
                },
            );
            self.join_candidate_regs.entry(header).or_default().push(reg);
        }
        if let Some(regs) = self.join_candidate_regs.get_mut(&header) {
            regs.sort();
            regs.dedup();
        }
    }

    /// Append evidence for values lost at a join without rebinding the
    /// register. This runs after every analysis and code rewrite.
    /// Bring every recorded snapshot into the namespace the body ended up in.
    ///
    /// Snapshots are captured at an arm end, before the naming pass, so their
    /// register values spell locals `argN` / `local_mN`. The candidates that cite
    /// them are replayed into `slotN` / `tmpN` at insertion, and
    /// `check_snapshot`'s rule is *audit-internal* - "every candidate's value is in
    /// the snapshot its own id names" - so leaving the two sides in different
    /// namespaces makes a sound emitter report violations.
    ///
    /// Renaming both sides is sound precisely because that rule compares the audit
    /// against itself: it asks whether the value the annotation shows was present in
    /// the state the snapshot recorded, and a consistent renaming of both preserves
    /// exactly that. The rules that do reach outside the audit - `ir` and `loop_ir` -
    /// check site keys, path keys and binding loss, never value spellings, so they
    /// are unaffected.
    pub(crate) fn normalize_provenance_namespace(&mut self) {
        if self.identifier_renames.is_empty() {
            return;
        }
        let renames = self.identifier_renames.clone();
        for stream in [
            &mut self.join_provenance,
            &mut self.loop_provenance,
            &mut self.call_provenance,
        ] {
            for snapshot in &mut stream.snapshots {
                for (_, value) in &mut snapshot.registers {
                    *value = replay_identifier_renames(value, &renames);
                }
            }
        }
    }

    pub(crate) fn append_join_annotations(&mut self) {
        // Before the loop, so a later annotation cannot make an identifier look live.
        let live = live_identifier_tokens(&self.lines);
        let mut inserts: Vec<PlannedJoinAnnotation> = Vec::new();
        let mut omissions: Vec<PlannedCapOmission> = Vec::new();
        for anchor in &self.join_annotation_anchors {
            let mut next_line = 0usize;
            for original in &anchor.lines {
                let original_tokens = register_tokens(original);
                if original_tokens.is_empty() {
                    continue;
                }
                let Some(relative) = self.lines[next_line..].iter().position(|line| {
                    original_tokens.iter().all(|reg| {
                        unrecovered_value_spellings(reg).iter().any(|spelling| {
                            contains_identifier_token(line, spelling)
                        })
                    })
                }) else {
                    continue;
                };
                let line_index = next_line + relative;
                next_line = line_index + 1;
                let line = &self.lines[line_index];
                let bytes = line.as_bytes();
                let mut index = 0usize;
                while index < bytes.len() {
                    if !bytes[index].is_ascii_alphabetic() {
                        index += 1;
                        continue;
                    }
                    let token_start = index;
                    while index < bytes.len() && Self::is_ident_char(bytes[index] as char) {
                        index += 1;
                    }
                    if token_start > 0 && Self::is_ident_char(bytes[token_start - 1] as char) {
                        continue;
                    }
                    let Some(reg) = canonical_register_spelling(&line[token_start..index]) else {
                        continue;
                    };
                    if !anchor.candidate_regs.iter().any(|candidate| candidate == &reg) {
                        continue;
                    }
                    let Some(candidates) = self.join_candidates.get(&(anchor.join, reg.clone()))
                    else {
                        continue;
                    };
                    if let Some(raw) = candidates
                        .values
                        .iter()
                        .find(|value| !is_recordable_annotation_candidate(value))
                    {
                        // Judged on the raw value, before any rename, so this is a
                        // different drop from the gate below and cannot be folded into
                        // it. Recorded so "every rejection on this path is accounted"
                        // is true of the function rather than of one branch of it.
                        // Invariant across the R33 change, so it cannot explain the
                        // reconciliation gap - which is exactly why it is worth having
                        // counted rather than assumed.
                        let rendered = raw.clone();
                        let loop_site = self.loop_annotation_sites.contains(&anchor.join);
                        let (loss_site, site_tag, provenance) = if loop_site {
                            (LOOP_LOSS_SITE, LOOP_SITE_TAG, &mut self.loop_provenance)
                        } else {
                            (JOIN_LOSS_SITE, JOIN_LOSS_SITE, &mut self.join_provenance)
                        };
                        record_filter_rejection(
                            provenance,
                            FilterRejection {
                                loss_site,
                                site_key: SiteKey(site_tag, anchor.join as u64),
                                register: reg.clone(),
                                reason: "not_recordable_raw",
                                rendered,
                            },
                        );
                        continue;
                    }
                    // Captured before the naming pass, inserted after it, so the
                    // spelling must be brought forward before anything judges it.
                    // `arg0.f8` becomes `slot0.f8`, still a field access on an
                    // identifier the reader can find; `local_m32.f8` becomes
                    // `tmp7.f8`, which the filter below then rejects as one gap
                    // decorating another - correctly, and it was only surviving
                    // because the stale spelling hid it.
                    let renamed: Vec<String> = candidates
                        .values
                        .iter()
                        .map(|value| replay_identifier_renames(value, &self.identifier_renames))
                        .collect();
                    // Re-judged on the text that will actually be emitted, which is
                    // what `candidate_form` promises and what an independent scan of
                    // the output checks.
                    //
                    // Every rejection here is recorded. Bringing the spelling forward
                    // is what makes these gates fire at all: `local_m32.f8` passed
                    // capture and becomes `tmp7.f8`, which `is_opaque_temporary` then
                    // rejects. So this is where the annotations lost this round actually
                    // go, and a silent `continue` would have made a drop of roughly two
                    // thousand per sample invisible - the same accounting gap the cap
                    // ledger exists to prevent.
                    let rejection = renamed.iter().find_map(|value| {
                        if !is_recordable_annotation_candidate(value) {
                            Some((value, "not_recordable"))
                        } else if !is_informative_annotation_candidate(value) {
                            Some((value, "opaque_after_rename"))
                        } else if !candidate_names_only_live_locals(value, &live) {
                            Some((value, "names_absent_identifier"))
                        } else {
                            None
                        }
                    });
                    if let Some((value, reason)) = rejection {
                        let rendered = value.clone();
                        let loop_site = self.loop_annotation_sites.contains(&anchor.join);
                        let (loss_site, site_tag, provenance) = if loop_site {
                            (LOOP_LOSS_SITE, LOOP_SITE_TAG, &mut self.loop_provenance)
                        } else {
                            (JOIN_LOSS_SITE, JOIN_LOSS_SITE, &mut self.join_provenance)
                        };
                        record_filter_rejection(
                            provenance,
                            FilterRejection {
                                loss_site,
                                // Declared key space, as in `record_cap_omissions`: tag
                                // `loop`, label `loop_entry`.
                                site_key: SiteKey(site_tag, anchor.join as u64),
                                register: reg.clone(),
                                reason,
                                rendered,
                            },
                        );
                        continue;
                    }
                    debug_assert!(
                        candidates.values.iter().all(|value| {
                            candidates
                                .provenance
                                .iter()
                                .any(|candidate| candidate.value == *value)
                        }),
                        "join annotation candidate must come from an arm-end snapshot"
                    );
                    let literal = if self.loop_annotation_sites.contains(&anchor.join) {
                        &LOOP_ENTRY_ANNOTATION
                    } else if candidates.complete {
                        &EXHAUSTIVE_JOIN_ANNOTATION
                    } else {
                        &NON_EXHAUSTIVE_JOIN_ANNOTATION
                    };
                    let annotation = literal.render(&renamed);
                    // At most one annotation per register spelling on a final
                    // line. Repeated structured renderings can map different
                    // joins to the same textual site; retaining all would make
                    // a single `regN` look like a chain of independently valid
                    // values. First anchor wins, in deterministic render order.
                    if inserts
                        .iter()
                        .any(|existing| existing.line == line_index && existing.at == index)
                    {
                        // Recorded, because this is the last unaccounted drop on this
                        // path and its size is unknown until it is counted. It is a
                        // different fact from the gates above: nothing about this value
                        // is wrong, the coordinate is simply already claimed.
                        let loop_site = self.loop_annotation_sites.contains(&anchor.join);
                        let (loss_site, site_tag, provenance) = if loop_site {
                            (LOOP_LOSS_SITE, LOOP_SITE_TAG, &mut self.loop_provenance)
                        } else {
                            (JOIN_LOSS_SITE, JOIN_LOSS_SITE, &mut self.join_provenance)
                        };
                        record_filter_rejection(
                            provenance,
                            FilterRejection {
                                loss_site,
                                site_key: SiteKey(site_tag, anchor.join as u64),
                                register: reg.clone(),
                                reason: "coordinate_already_claimed",
                                rendered: annotation.clone(),
                            },
                        );
                        continue;
                    }
                    let planned = inserts
                        .iter()
                        .filter(|existing| existing.line == line_index)
                        .map(|existing| existing.text.len())
                        .sum::<usize>();
                    // Whole or not at all. A span cut to fit leaves an unclosed
                    // comment opener, and every consumer that parses comments
                    // then reads the rest of the file as one.
                    let omitted = if !rendered_annotation_is_safe(&annotation) {
                        Some(UNSAFE_SPAN)
                    } else if annotation.len() > MAX_JOIN_ANNOTATION {
                        Some(ANNOTATION_BUDGET)
                    } else if line.len() + planned + annotation.len() > MAX_JOIN_ANNOTATED_LINE {
                        Some(LINE_BUDGET)
                    } else {
                        None
                    };
                    if let Some(reason) = omitted {
                        // Collected here and routed after the loop: the anchors
                        // are borrowed for the whole walk, and a silent drop is
                        // the failure this row exists to make impossible.
                        omissions.push(PlannedCapOmission {
                            join: anchor.join,
                            register: reg,
                            rendered: annotation,
                            reason,
                            line_len: line.len(),
                            planned_len: planned,
                        });
                        continue;
                    }
                    inserts.push(PlannedJoinAnnotation {
                        line: line_index,
                        at: index,
                        text: annotation,
                        join: anchor.join,
                        register: reg,
                    });
                }
            }
        }
        self.record_cap_omissions(omissions);
        inserts.sort_unstable_by(|left, right| {
            right
                .line
                .cmp(&left.line)
                .then_with(|| right.at.cmp(&left.at))
        });
        for planned in &inserts {
            self.lines[planned.line].insert_str(planned.at, &planned.text);
        }
        self.record_join_annotation_provenance(&inserts);
        self.record_loop_entry_annotation_provenance(&inserts);
    }

    /// Route each dropped annotation to its own site's stream.
    ///
    /// Site classification is read from `loop_annotation_sites`, exactly as the
    /// literal choice and the audit key read it, so a loop header cannot be
    /// counted against the join site's ledger row.
    fn record_cap_omissions(&mut self, omissions: Vec<PlannedCapOmission>) {
        for omission in omissions {
            let loop_site = self.loop_annotation_sites.contains(&omission.join);
            let (loss_site, site_tag, provenance) = if loop_site {
                (LOOP_LOSS_SITE, LOOP_SITE_TAG, &mut self.loop_provenance)
            } else {
                (JOIN_LOSS_SITE, JOIN_LOSS_SITE, &mut self.join_provenance)
            };
            record_cap_omission(
                provenance,
                CapOmissionFacts {
                    loss_site,
                    // The key tag, not the loss-site label. The two coincide at the join
                    // site and diverge at the loop site: the declared key space is
                    // `("loop", header)` while the label is `loop_entry`, which is the
                    // pairing `LOSS_SITE_OF_TAG` in the reconciler states and its own
                    // fixture uses. `loop_entry` is not a member of `SITE_TAGS`, so
                    // keying by the label put these rows in no declared space at all.
                    //
                    // It survived because neither validator reads an omission row: the
                    // reconciler filters to `record == "annotation"` and the provenance
                    // checker never mentions them. `annotation_caps.rs` asserts on the
                    // `loss_site` label, not the key, so it stayed green too.
                    site_key: SiteKey(site_tag, omission.join as u64),
                    register: omission.register,
                    rendered: omission.rendered,
                    budget: omission.reason,
                    line_len: omission.line_len,
                    planned_len: omission.planned_len,
                },
            );
        }
    }

    /// One audit row per emitted loop-entry annotation, keyed off the anchor that
    /// produced it.
    ///
    /// Both keys come off the planned insertion and the site classification made
    /// at capture, never off a second walk of the region tree: a key derived on its
    /// own path can name a real loop header with a real entry predecessor and a
    /// real drop of the same register while the annotation it labels was emitted at
    /// another block entirely, and every check that reads only the audit passes.
    ///
    /// The coordinate is deliberately not recorded here, for the same reason the
    /// join rows do not carry one: a program-level rewrite still runs over the
    /// finished source and can move text on an annotated line. Rows go out in
    /// ascending output order, which is what lets that search stay monotonic.
    fn record_loop_entry_annotation_provenance(&mut self, inserts: &[PlannedJoinAnnotation]) {
        if !annotation_provenance_wanted() {
            return;
        }
        // Cloned before the closure: these run on `&mut self` while the
        // attribution mapping needs the map, and a borrow of both at once does not
        // typecheck.
        let renames = self.identifier_renames.clone();
        let mut ordered: Vec<&PlannedJoinAnnotation> = inserts.iter().collect();
        ordered.sort_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.at.cmp(&right.at))
        });
        let records: Vec<PendingAnnotationRecord> = ordered
            .iter()
            .filter_map(|planned| {
                // Only this site's annotations, decided by the classification the
                // literal was chosen from. The join rows exclude the same blocks,
                // so one annotation is claimed once.
                if !self.loop_annotation_sites.contains(&planned.join) {
                    return None;
                }
                let candidates = self
                    .join_candidates
                    .get(&(planned.join, planned.register.clone()))?;
                Some(PendingAnnotationRecord {
                    loss_site: LOOP_LOSS_SITE,
                    site_key: SiteKey(LOOP_SITE_TAG, planned.join as u64),
                    // The block whose rendered body this annotation was placed
                    // in, from the same planned insertion the key above comes
                    // off. An external reader resolves it in the emitted IR and
                    // asks whether a loop-tagged label is what that block earns.
                    anchor: SiteKey(JOIN_PATH_KIND, planned.join as u64),
                    register: planned.register.clone(),
                    rendered: planned.text.clone(),
                    // Every attribution, duplicates included: two entry arms
                    // carrying one value are one rendered value and two rows, and
                    // this is where the second survives.
                    candidates: candidates
                        .provenance
                        .iter()
                        .map(|candidate| CandidateAttribution {
                            path_key: SiteKey(JOIN_PATH_KIND, candidate.pred as u64),
                            // The same replay the rendered span went through, so the
                            // attribution and the emitted text cannot disagree.
                            // `VAL-PROV-COMPLETE-015` count 5 compares them directly.
                            value: replay_identifier_renames(&candidate.value, &renames),
                            snapshot_id: candidate.snapshot_id.clone(),
                        })
                        .collect(),
                })
            })
            .collect();
        self.loop_provenance.records.extend(records);
    }

    /// One audit row per emitted annotation, keyed off the anchor that produced
    /// it.
    ///
    /// The site and the register come from the planned insertion, not from a
    /// second derivation: a key computed on its own path can name a real join
    /// with a real predecessor and a real drop of the same register while the
    /// annotation it labels was emitted somewhere else entirely, and every check
    /// that reads only the audit passes. Taken off the anchor, that mismatch
    /// cannot be expressed.
    ///
    /// The coordinate is deliberately not recorded here. A program-level rewrite
    /// still runs over the finished source and can move text on an annotated
    /// line, so it is derived from the artifact afterwards by locating the
    /// rendered span. Records go out in ascending output order, which is what lets
    /// that search stay monotonic.
    fn record_join_annotation_provenance(&mut self, inserts: &[PlannedJoinAnnotation]) {
        if !annotation_provenance_wanted() {
            return;
        }
        // Cloned before the closure: these run on `&mut self` while the
        // attribution mapping needs the map, and a borrow of both at once does not
        // typecheck.
        let renames = self.identifier_renames.clone();
        let mut ordered: Vec<&PlannedJoinAnnotation> = inserts.iter().collect();
        ordered.sort_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.at.cmp(&right.at))
        });
        let records: Vec<PendingAnnotationRecord> = ordered
            .iter()
            .filter_map(|planned| {
                // A loop header can be a join as well, and its candidates share
                // this anchor table. Its annotation is a loop site's, so claiming
                // it here would put two site-tagged records at one output
                // coordinate - the double claim the site precedence exists to
                // avoid.
                if self
                    .regions
                    .as_ref()
                    .is_some_and(|regions| regions.is_loop_header(planned.join))
                {
                    return None;
                }
                let candidates = self
                    .join_candidates
                    .get(&(planned.join, planned.register.clone()))?;
                Some(PendingAnnotationRecord {
                    loss_site: JOIN_LOSS_SITE,
                    site_key: SiteKey(JOIN_LOSS_SITE, planned.join as u64),
                    // Same anchor the key above is read off, recorded so the
                    // derivation is inspectable: the IR says whether that block
                    // is the multi-predecessor non-header a join label claims.
                    anchor: SiteKey(JOIN_PATH_KIND, planned.join as u64),
                    register: planned.register.clone(),
                    rendered: planned.text.clone(),
                    // Every attribution, duplicates included: two predecessors
                    // carrying one value are one rendered value and two rows, and
                    // this is where the second survives.
                    candidates: candidates
                        .provenance
                        .iter()
                        .map(|candidate| CandidateAttribution {
                            path_key: SiteKey(JOIN_PATH_KIND, candidate.pred as u64),
                            // Same replay as the rendered span, for the same reason
                            // as the loop site above.
                            value: replay_identifier_renames(&candidate.value, &renames),
                            snapshot_id: candidate.snapshot_id.clone(),
                        })
                        .collect(),
                })
            })
            .collect();
        self.join_provenance.records.extend(records);
    }

    /// Drop register bindings that a merge cannot attribute to one path. A
    /// register no path in the merged region writes still holds whatever it held
    /// on entry, and a reserved register holds the same value everywhere.
    ///
    /// SPREG is exempt for a different reason, and the distinction matters. It
    /// is not pinned, because the prologue's `sub x15, x15, #N` genuinely
    /// changes it and the frame offset has to be tracked or slot addresses come
    /// out wrong. But frames are balanced, so every path into a join leaves the
    /// same stack pointer: the write that changes it is in the prologue, which
    /// dominates. Dropping it here instead costs 11,717 stack slot references,
    /// which degrade to `reg15` for no correctness gain.
    fn merge_state_at_join(&mut self, written: &HashSet<String>) {
        self.state.reg_values.retain(|reg, _| {
            pinned_value(reg).is_some() || reg == "x15" || !written.contains(reg)
        });
        // A pre-call value describes one path. Past a merge that any path could
        // have written the register on, it describes no path in particular, and
        // an annotation carrying it would be a claim about the wrong one.
        self.state
            .call_clobbers
            .retain(|reg, _| !written.contains(reg));
        self.state.last_cmp = None;
        self.state.selector_hints.clear();
    }
}
