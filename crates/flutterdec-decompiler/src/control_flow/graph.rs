/// What a `brk` renders as. A trap is not a return and not a throw the source
/// program wrote, so it is reported as the effect it is rather than as any
/// source construct.
pub(super) const TRAP_NOTE: &str = "trap: control does not continue";

/// What a `br Xn` renders as, naming the value control left through when the
/// operand survived. Deliberately not a `return`, a `goto` or a `tailCall_`:
/// every one of those names a destination that was never recovered.
pub(super) fn indirect_branch_note(target: &str) -> String {
    let via = target.trim();
    if via.is_empty() {
        "indirect branch: target not recovered".to_string()
    } else {
        format!("indirect branch through {via}: target not recovered")
    }
}

/// What an edge into a block the walk already rendered says.
///
/// The DFS fallback emits a block once per path that reaches it, and Dart has no
/// `goto`, so an edge back into a block already written above cannot be rendered
/// where it occurs. Naming the block keeps the edge in the artifact; emitting
/// nothing dropped it, and a `return` or a `tailCall_` there would state an exit
/// the graph does not contain.
pub(super) fn rejoin_note(id: usize) -> String {
    format!("control rejoins block {id}: already emitted above")
}

/// How deep the DFS walk nests before it stops inlining successors.
pub(super) const DFS_MAX_DEPTH: usize = 12;

/// Why the DFS walk will not inline a successor at this site.
///
/// Split out of `can_inline` rather than re-derived beside it: the omission
/// event has to name the budget that actually refused, and a second copy of the
/// same conditions is a copy that can drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineRefusal {
    /// Already at the depth budget.
    Depth,
    /// The block is being emitted further up this path.
    Active,
    /// The block has been emitted as often as its visit budget allows.
    VisitBudget,
    /// The edge names a block the function does not contain. The identity gate
    /// at the public entry points refuses such a graph before emission, so this
    /// is defensive.
    UnknownBlock,
}

impl<'a> FuncEmitter<'a> {
    /// The one rendering of a successor this walk cannot emit at this site.
    ///
    /// Every reason is stated rather than skipped: a back edge to the loop being
    /// rendered as `continue;`, a back edge to any other active block as a
    /// recorded back edge plus a rejoin note, a block not yet emitted as its
    /// omission helper, and a block already emitted as a rejoin note. Falling
    /// through with no statement is what silently dropped the edge.
    ///
    /// `depth` is the depth the successor would have been emitted at, which is
    /// what decides whether the depth budget or the visit budget refused it.
    pub(super) fn emit_unrenderable_successor(&mut self, indent: usize, id: usize, depth: usize) {
        if self.loop_context.contains(&id) {
            self.push_line(indent, "continue;");
            return;
        }
        if self.active_stack.contains(&id) {
            // A back edge, not a budget: the walk is already inside this block.
            self.loop_back_edges.insert(id);
        } else if let Some(kind) = self.budget_refusal(id, depth) {
            let source = self.current_source_block();
            let target = TraversalTarget::Block {
                start_va: self.block_start_va(id),
            };
            self.record_traversal_event(kind, source, target);
        }
        if self.emitted.contains(&id) {
            self.push_line(indent, &format!("// {}", rejoin_note(id)));
        } else {
            self.emit_omitted_path(indent, Some(id));
        }
    }

    /// Which budget refused this edge, if a budget is what refused it.
    ///
    /// A block already emitted elsewhere is rendered as a rejoin note rather
    /// than as a helper, and that is still an edge this walk did not follow: the
    /// event says which budget stopped it, and says nothing about whether the
    /// block was emitted. Being inside the block is not a budget, so it gets no
    /// event.
    pub(super) fn budget_refusal(&self, to: usize, depth: usize) -> Option<TraversalEventKind> {
        match self.inline_refusal(to, depth) {
            Some(InlineRefusal::Depth) => Some(TraversalEventKind::DfsDepthOmission),
            Some(InlineRefusal::VisitBudget) => Some(TraversalEventKind::DfsVisitOmission),
            Some(InlineRefusal::Active) | Some(InlineRefusal::UnknownBlock) | None => None,
        }
    }

    pub(super) fn branch_condition(&self, mnemonic: &str, ops: &[String]) -> Option<String> {
        if mnemonic.starts_with("b.") {
            if let Some(cmp) = &self.state.last_cmp {
                return cond_from_cmp(mnemonic, cmp);
            }
            return Some(format!("flags.{}", mnemonic.replace('.', "_")));
        }

        if mnemonic == "cbz" && !ops.is_empty() {
            let v = self.operand_expr(&ops[0]);
            return Some(format!("{} == 0", v));
        }
        if mnemonic == "cbnz" && !ops.is_empty() {
            let v = self.operand_expr(&ops[0]);
            return Some(format!("{} != 0", v));
        }
        if mnemonic == "tbz" && ops.len() >= 2 {
            let v = self.operand_expr(&ops[0]);
            let bit = self.lookup_reg(&ops[1]);
            return Some(format!("(({} >> {}) & 1) == 0", v, bit));
        }
        if mnemonic == "tbnz" && ops.len() >= 2 {
            let v = self.operand_expr(&ops[0]);
            let bit = self.lookup_reg(&ops[1]);
            return Some(format!("(({} >> {}) & 1) != 0", v, bit));
        }

        None
    }

    pub(super) fn branch_target_block(&self, target: &str) -> Option<usize> {
        let normalized = normalize_target(target);
        let va = normalized.strip_prefix("0x")?;
        let parsed = u64::from_str_radix(va, 16).ok()?;
        self.va_to_id.get(&parsed).copied()
    }

    pub(super) fn inline_refusal(&self, to: usize, depth: usize) -> Option<InlineRefusal> {
        if depth >= DFS_MAX_DEPTH {
            return Some(InlineRefusal::Depth);
        }
        if self.active_stack.contains(&to) {
            return Some(InlineRefusal::Active);
        }
        if self.inline_visits.get(&to).copied().unwrap_or(0) >= self.visit_limit(to) {
            return Some(InlineRefusal::VisitBudget);
        }
        if !self.block_by_id.contains_key(&to) {
            return Some(InlineRefusal::UnknownBlock);
        }
        None
    }

    pub(super) fn can_inline(&self, to: usize, depth: usize) -> bool {
        self.inline_refusal(to, depth).is_none()
    }

    pub(super) fn has_backedge_pred(&self, id: usize) -> bool {
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        for pred in &block.preds {
            if let Some(pb) = self.block_by_id.get(pred) {
                if pb.succs.contains(&id) && pb.start_va >= block.start_va {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn has_forward_pred(&self, id: usize) -> bool {
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        for pred in &block.preds {
            if let Some(pb) = self.block_by_id.get(pred) {
                if pb.succs.contains(&id) && pb.start_va < block.start_va {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn should_wrap_loop_header(&self, id: usize, depth: usize) -> bool {
        if depth >= 10 {
            return false;
        }
        if !self.loop_context.is_empty() {
            return false;
        }
        if self.loop_context.contains(&id) {
            return false;
        }
        if self.active_stack.contains(&id) {
            return false;
        }
        if self.inline_visits.get(&id).copied().unwrap_or(0) >= self.visit_limit(id) {
            return false;
        }
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        let tail = block.instrs.last().map(|i| &i.op);
        if !matches!(tail, Some(IROp::Branch)) {
            return false;
        }
        if block.succs.len() < 2 {
            return false;
        }
        self.has_backedge_pred(id) && self.has_forward_pred(id)
    }

    pub(super) fn emit_wrapped_loop(&mut self, id: usize, indent: usize, depth: usize) {
        self.loop_context.push(id);
        self.push_line(indent, "while (true) {");
        self.emit_block(id, indent + 1, depth + 1);
        self.push_line(indent + 1, "break;");
        self.push_line(indent, "}");
        self.loop_context.pop();
    }
}
