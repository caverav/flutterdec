// Renders a function by walking its region structure, emitting every reachable
// basic block exactly once.
//
// The emit-once invariant is checked rather than assumed: reaching a block that
// has already been emitted, and that is neither the enclosing loop's header nor
// its exit, is a structural failure. The whole function then falls back to the
// DFS emitter, so this pass can only improve output, never truncate it.

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
        self.regions = None;
        false
    }

    fn counter_snapshot(&self) -> [usize; 11] {
        [
            self.placeholder_ifs,
            self.unresolved_cf,
            self.raw_register_calls,
            self.total_calls,
            self.indirect_calls,
            self.semantic_direct_calls,
            self.semantic_indirect_calls,
            self.dispatch_selector_calls,
            self.dispatch_table_calls,
            self.repeated_blocks,
            self.target_va_symbol_calls,
        ]
    }

    fn restore_counters(&mut self, c: [usize; 11]) {
        self.placeholder_ifs = c[0];
        self.unresolved_cf = c[1];
        self.raw_register_calls = c[2];
        self.total_calls = c[3];
        self.indirect_calls = c[4];
        self.semantic_direct_calls = c[5];
        self.semantic_indirect_calls = c[6];
        self.dispatch_selector_calls = c[7];
        self.dispatch_table_calls = c[9];
        self.repeated_blocks = c[10];
        self.target_va_symbol_calls = c[8];
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

            match self.render_block_body(id, indent) {
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
                    let arms: Vec<usize> = [taken, not_taken].into_iter().flatten().collect();

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
                    let taken_lines: Vec<String> = self.lines.split_off(buffer_start);

                    self.state = state_at_branch.clone();
                    if let Some(f) = not_taken {
                        if Some(f) != region_follow
                            && !self.render_sequence(f, region_follow, indent + 1, depth + 1)
                        {
                            return false;
                        }
                    }
                    let else_lines: Vec<String> = self.lines.split_off(buffer_start);

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
                        .state
                        .reg_values
                        .get("x0")
                        .cloned()
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
                                self.push_line(indent, &format!("return tailCall_{};", normalized));
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
    /// A region containing a loop is never repeated, and the budget was chosen
    /// from the measured distribution: at 8 blocks and 48 instructions, 85% of
    /// reducible functions structure with 1.09x duplication inside them, against
    /// 75% at 1.02x for terminal blocks only and 92% at 1.45x if the budget is
    /// raised eightfold.
    fn is_repeatable_region(&self, id: usize, follow: Option<usize>) -> bool {
        const MAX_REPEATED_BLOCKS: usize = 8;
        const MAX_REPEATED_INSTRUCTIONS: usize = 48;
        let Some(regions) = self.regions.as_ref() else {
            return false;
        };

        let mut seen: HashSet<usize> = HashSet::new();
        let mut instructions = 0usize;
        let mut stack = vec![id];
        while let Some(block) = stack.pop() {
            if Some(block) == follow || !seen.insert(block) {
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

    /// Registers written by any block reachable from `roots` before `stop`.
    ///
    /// A binding survives a merge only if no path into it redefines the
    /// register, which is exactly this set's complement.
    fn registers_written_between(&self, roots: &[usize], stop: Option<usize>) -> HashSet<String> {
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
                        // A call clobbers the volatile registers.
                        written.extend((0..18).map(|r| format!("x{r}")));
                        written.insert("x30".to_string());
                    }
                }
            }
            stack.extend(regions.successors(id).iter().copied());
        }
        written
    }

    /// Drop register bindings that a merge cannot attribute to one path. The ABI
    /// registers hold the same value everywhere, and a register no path in the
    /// merged region writes still holds whatever it held on entry.
    fn merge_state_at_join(&mut self, written: &HashSet<String>) {
        self.state.reg_values.retain(|reg, _| {
            matches!(reg.as_str(), "x15" | "x22" | "x26" | "x27") || !written.contains(reg)
        });
        self.state.last_cmp = None;
        self.state.selector_hints.clear();
    }
}
