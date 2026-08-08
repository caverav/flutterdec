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
        self.lines.insert(insert_idx, summary);
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
            if !generated.insert(id) {
                continue;
            }
            if generated.len() > 64 {
                break;
            }

            let mut helper = FuncEmitter::new(self.ir, self.symbol_names);
            // Helper bodies are copied verbatim into the caller by
            // `inline_helper_calls`, so their temporaries share the caller's
            // namespace and the counter has to continue rather than restart.
            helper.call_index = self.call_index;
            helper.pool_value_hints = self.pool_value_hints.clone();
            helper.pool_semantic_hints = self.pool_semantic_hints.clone();
            helper.emit_block(id, 1, 0);
            self.call_index = helper.call_index;
            let has_terminator = helper.lines.iter().any(|line| {
                let t = line.trim_start();
                t.starts_with("return ") || t == "continue;"
            });
            let fallback_return = helper
                .state
                .reg_values
                .get("x0")
                .cloned()
                .unwrap_or_else(|| "null".to_string());

            self.lines.push(format!("dynamic _block_{}() {{", id));
            self.lines.extend(helper.lines);
            if !has_terminator {
                self.push_line(1, &format!("return {};", fallback_return));
            }
            self.lines.push("}".to_string());

            for next in helper.omitted_blocks {
                if !generated.contains(&next) && !queued.contains(&next) {
                    queue.push(next);
                    queued.insert(next);
                }
            }
        }
    }
}
