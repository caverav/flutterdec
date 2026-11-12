impl<'a> FuncEmitter<'a> {
    pub(super) fn insert_loop_summary_comment(&mut self) {
        if self.loop_back_edges.is_empty() || self.lines.is_empty() {
            return;
        }

        let details = self
            .loop_back_edges
            .iter()
            .map(|id| format!("block {}", id))
            .collect::<Vec<_>>()
            .join(", ");
        let summary = format!("  // loop back-edges: {}", details);

        let mut insert_idx = 1usize;
        while insert_idx < self.lines.len() {
            let t = self.lines[insert_idx].trim_start();
            if t.starts_with("var ") || t.starts_with("int ") || t.starts_with("dynamic ") {
                insert_idx += 1;
                continue;
            }
            if self.lines[insert_idx].trim().is_empty() {
                insert_idx += 1;
            }
            break;
        }
        self.insert_body_line(insert_idx, summary);
    }

    pub(super) fn visit_limit(&self, id: usize) -> usize {
        if let Some(block) = self.block_by_id.get(&id) {
            let tail = block.instrs.last().map(|i| &i.op);
            if block.instrs.len() <= 3 && matches!(tail, Some(IROp::Jump | IROp::Return)) {
                return 48;
            }
            if block.preds.len() > 1 {
                return 24;
            }
        }
        14
    }

    pub(super) fn append_helper_functions(&mut self) {
        let mut generated = BTreeSet::new();
        let mut queue: Vec<usize> = self.omitted_blocks.iter().copied().collect();
        let mut queued: HashSet<usize> = queue.iter().copied().collect();

        while let Some(id) = queue.pop() {
            queued.remove(&id);
            if generated.contains(&id) {
                continue;
            }
            // Past the budget the queue is drained rather than abandoned: every
            // block that will not get a definition has to be named, or its call
            // site would be the only trace of it and there would be nothing to
            // attribute the omission to.
            if generated.len() >= HELPER_DEFINITION_BUDGET {
                self.helper_cap_omitted.insert(id);
                continue;
            }
            generated.insert(id);

            let mut helper = FuncEmitter::new(self.ir, self.symbol_names);
            // Helper bodies are copied verbatim into the caller by
            // `inline_helper_calls`, so their temporaries share the caller's
            // namespace and the counter has to continue rather than restart.
            helper.call_index = self.call_index;
            helper.pool_value_hints = self.pool_value_hints.clone();
            helper.pool_semantic_hints = self.pool_semantic_hints.clone();
            // Helper bodies contain the shared slow paths, so this is where most
            // of the runtime-stub calls end up: without it 535 error-stub sites
            // on one sample keep the Dart-call model and bind a throw.
            helper.runtime_stubs = self.runtime_stubs.clone();
            helper.emit_block(id, 1, 0);
            // A helper is part of this function's final artifact. Preserve the
            // blocks its nested walk emitted so the final disposition ledger
            // does not mistake helper-rendered blocks for omissions.
            self.emitted.extend(helper.emitted.iter().copied());
            self.call_index = helper.call_index;
            let has_terminator = helper.lines.iter().any(|line| {
                let t = line.trim_start();
                t.starts_with("return ") || t == "continue;"
            });
            let fallback_return = helper
                .capped_reg_value("x0")
                .unwrap_or_else(|| "null".to_string());

            self.push_body_line(format!("dynamic _block_{}() {{", id));
            // A helper body is rendered by its own emitter, so these lines are
            // new to this body and carry new identities. No anchor of this
            // function's render was ever on one of them.
            for (line, line_id) in helper.lines.into_iter().zip(helper.line_ids) {
                let call_kind = helper.rendered_call_kinds.get(&line_id).copied();
                self.push_body_line(line);
                if let Some(call_kind) = call_kind {
                    self.rendered_call_kinds
                        .insert(*self.line_ids.last().expect("helper line has an identity"), call_kind);
                }
            }
            if !has_terminator {
                self.push_line(1, &format!("return {};", fallback_return));
            }
            self.push_body_line("}".to_string());

            // The helper walked its own edges, so its omissions are this
            // function's omissions: they name the same blocks and their events
            // belong in the same stream.
            for event in helper.accounting.events() {
                self.accounting.record_event(
                    event.kind,
                    event.function_id,
                    event.source_start_va,
                    event.target,
                );
            }
            for (target, source) in helper.omission_sources {
                self.omission_sources.entry(target).or_insert(source);
            }
            for next in helper.omitted_blocks {
                if !generated.contains(&next) && !queued.contains(&next) {
                    queue.push(next);
                    queued.insert(next);
                }
            }
        }
    }
}
