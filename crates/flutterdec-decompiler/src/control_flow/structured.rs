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

/// Exact provenance of one candidate carried from an arm-end snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinCandidateProvenance {
    pub arm: usize,
    pub value: String,
}

/// Keep candidate text and its producing arm together while sorting, so the
/// provenance audit cannot accidentally inspect a value from another snapshot.
pub(crate) fn ordered_join_candidate_provenance(
    values: impl IntoIterator<Item = JoinCandidateProvenance>,
) -> Vec<JoinCandidateProvenance> {
    let mut ordered = values.into_iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.value
            .cmp(&right.value)
            .then_with(|| left.arm.cmp(&right.arm))
    });
    ordered.dedup_by(|left, right| left.value == right.value);
    ordered
}

/// A candidate is useful only when it identifies a field, call result, or
/// literal; another fallback register or opaque temporary would add no evidence.
pub(crate) fn is_informative_join_candidate(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !contains_uninformative_join_token(value)
        && (value.contains(".f") || contains_call_expression(value) || is_numeric_literal(value))
}

fn contains_call_expression(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'(' {
            index += 1;
            continue;
        }
        let mut start = index;
        while start > 0
            && (bytes[start - 1].is_ascii_alphanumeric()
                || bytes[start - 1] == b'_'
                || bytes[start - 1] == b'.')
        {
            start -= 1;
        }
        if start < index
            && !matches!(&value[start..index], "if" | "while" | "for" | "switch" | "catch")
        {
            return true;
        }
        index += 1;
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
fn contains_uninformative_join_token(value: &str) -> bool {
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
        if is_unrecovered_value_spelling(token) || is_opaque_join_temporary(token) {
            return true;
        }
        start = end.saturating_add(1);
    }
    false
}

fn is_unrecovered_value_spelling(token: &str) -> bool {
    (0..=30).any(|index| {
        unrecovered_value_spellings(&format!("x{index}"))
            .iter()
            .any(|spelling| token == spelling)
    })
}

fn is_opaque_join_temporary(token: &str) -> bool {
    ["t", "tmp", "objTmp", "intTmp", "resultTmp"]
        .iter()
        .any(|prefix| {
            token.strip_prefix(prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
}

/// Comments pass through brace-sensitive compaction, so a candidate needing
/// escaping is omitted rather than allowed to alter later structural rewrites.
fn is_safe_join_annotation_candidate(value: &str) -> bool {
    !value.contains('{') && !value.contains('}') && !value.contains("*/")
}

fn canonical_join_register_spelling(token: &str) -> Option<String> {
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
            if let Some(reg) = canonical_join_register_spelling(&line[start..index]) {
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
            self.structured_emitted.insert(id);
            if is_join {
                // Emitted once, so no single incoming path describes this
                // block's register state. Anything a predecessor could have
                // redefined is dropped; the rest still holds its entry value.
                let preds: Vec<usize> = (0..self.ir.blocks.len())
                    .filter(|p| regions.successors(*p).contains(&id))
                    .collect();
                let written = self.registers_written_between(&preds, Some(id));
                self.merge_state_at_join(&written);
            }
            let annotation_start = self.lines.len();
            let flow = self.render_block_body(id, indent);
            if is_join {
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
                        self.record_join_candidates(join, &arm_ends);
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
    /// Remember the arm-end values that the generic join merge will drop.
    ///
    /// The upcoming merge owns the exact write-set convention; candidates are
    /// collected from the two arm snapshots before restoring branch state.
    /// Completeness is deliberately checked against every actual predecessor:
    /// a third route means this is evidence, not an exhaustive value claim.
    pub(crate) fn record_join_candidates(&mut self, join: usize, arm_ends: &[(usize, LiftState)]) {
        if arm_ends.is_empty() {
            return;
        }
        let arms = arm_ends
            .iter()
            .map(|(arm, _)| *arm)
            .collect::<Vec<_>>();
        let (preds, written) = {
            let regions = self.regions.as_ref().expect("regions");
            let preds: Vec<usize> = (0..self.ir.blocks.len())
                .filter(|pred| regions.successors(*pred).contains(&join))
                .collect();
            // Use the same arm roots and stop node as the immediately following
            // merge. `registers_written_between` stops before the join, so no
            // successor beyond it contributes an unrelated write.
            let written = self.registers_written_between(&arms, Some(join));
            (preds, written)
        };
        let complete = arms.len() == 2
            && preds.len() == arms.len()
            && arms.iter().all(|arm| preds.contains(arm));
        let mut regs: Vec<String> = written
            .into_iter()
            .filter(|reg| pinned_value(reg).is_none() && reg != "x15")
            .collect();
        regs.sort();
    
        for reg in regs {
            let provenance = ordered_join_candidate_provenance(arm_ends.iter().flat_map(|(arm, state)| {
                state
                    .reg_values
                    .get(&reg)
                    .into_iter()
                    .filter_map(|value| Self::capped_expr(value))
                    .filter(|value| is_safe_join_annotation_candidate(value))
                    .map(move |value| JoinCandidateProvenance {
                        arm: *arm,
                        value: value.to_string(),
                    })
            }));
            debug_assert!(
                provenance.iter().all(|candidate| arm_ends.iter().any(|(arm, state)| {
                    *arm == candidate.arm
                        && state.reg_values.get(&reg).is_some_and(|value| {
                            Self::capped_expr(value)
                                .filter(|value| is_safe_join_annotation_candidate(value))
                                .is_some_and(|value| value == candidate.value)
                        })
                })),
                "join candidate must come from its recorded arm-end snapshot"
            );
            if provenance.is_empty() {
                continue;
            }
            let values = provenance
                .iter()
                .filter(|candidate| is_informative_join_candidate(&candidate.value))
                .map(|candidate| candidate.value.clone())
                .collect::<Vec<_>>();
            if values.is_empty() {
                continue;
            }
            self.join_candidates.insert(
                (join, reg.clone()),
                JoinCandidates {
                    complete: complete && values.len() == provenance.len(),
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

    /// Append evidence for values lost at a join without rebinding the
    /// register. This runs after every analysis and code rewrite.
    pub(crate) fn append_join_annotations(&mut self) {
        let mut inserts: Vec<(usize, usize, usize, String)> = Vec::new();
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
                    let Some(reg) = canonical_join_register_spelling(&line[token_start..index]) else {
                        continue;
                    };
                    if !anchor.candidate_regs.iter().any(|candidate| candidate == &reg) {
                        continue;
                    }
                    let Some(candidates) = self.join_candidates.get(&(anchor.join, reg)) else {
                        continue;
                    };
                    if candidates
                        .values
                        .iter()
                        .any(|value| !is_safe_join_annotation_candidate(value))
                    {
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
                    let prefix = if candidates.complete {
                        " = "
                    } else {
                        " possible (non-exhaustive): "
                    };
                    let annotation = format!(" /*{}{} */", prefix, candidates.values.join(" | "));
                    // At most one annotation per register spelling on a final
                    // line. Repeated structured renderings can map different
                    // joins to the same textual site; retaining all would make
                    // a single `regN` look like a chain of independently valid
                    // values. First anchor wins, in deterministic render order.
                    if inserts.iter().any(|(existing_line, token_at, _, _)| {
                        *existing_line == line_index && *token_at == token_start
                    }) {
                        continue;
                    }
                    let planned = inserts
                        .iter()
                        .filter(|(existing_line, _, _, _)| *existing_line == line_index)
                        .map(|(_, _, _, existing)| existing.len())
                        .sum::<usize>();
                    if annotation.len() <= MAX_JOIN_ANNOTATION
                        && line.len() + planned + annotation.len() <= MAX_JOIN_ANNOTATED_LINE
                    {
                        inserts.push((line_index, token_start, index, annotation));
                    }
                }
            }
        }
        inserts.sort_unstable_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.cmp(&left.1))
        });
        for (line_index, _, at, annotation) in inserts {
            self.lines[line_index].insert_str(at, &annotation);
        }
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
        self.state.last_cmp = None;
        self.state.selector_hints.clear();
    }
}
