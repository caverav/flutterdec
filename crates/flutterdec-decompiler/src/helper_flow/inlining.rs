impl<'a> FuncEmitter<'a> {
    pub(super) fn helper_inline_lines(meta: &HelperMeta) -> Option<InlineHelperPlan> {
        let non_empty: Vec<&String> = meta
            .body_lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .collect();
        if non_empty.is_empty() || non_empty.len() > 28 {
            return None;
        }

        // Every structural read below is of the emitter's own code. A helper
        // body carries recovered pool strings and emitter comments, and a brace
        // or a `_block_` spelling inside one is data being quoted, not a nested
        // block or a nested helper call.
        for line in &non_empty {
            if code_contains(line, "_block_") {
                return None;
            }
        }

        let last = non_empty.last()?.trim();
        let linear_last_return = last.starts_with("return ") && last.ends_with(';');
        let linear_no_braces = non_empty
            .iter()
            .all(|l| !code_contains(l, "{") && !code_contains(l, "}"));
        if linear_last_return && linear_no_braces {
            return Some(InlineHelperPlan {
                lines: meta.body_lines.clone(),
                append_null_return: false,
            });
        }

        // Single top-level if/else helper:
        // if (...) { ... } else { ... }
        let trimmed: Vec<&str> = non_empty.iter().map(|l| l.trim()).collect();
        if trimmed
            .first()
            .is_some_and(|l| l.starts_with("if (") && l.ends_with('{'))
        {
            let mut depth = 0i32;
            let mut if_end = None;
            for (idx, line) in trimmed.iter().enumerate() {
                depth += code_brace_delta(line);
                if depth == 0 {
                    if_end = Some(idx);
                    break;
                }
            }
            if let Some(if_end) = if_end {
                if if_end + 1 < trimmed.len() && trimmed[if_end + 1].starts_with("else {") {
                    depth = 0;
                    let mut else_end = None;
                    for (idx, line) in trimmed.iter().enumerate().skip(if_end + 1) {
                        depth += code_brace_delta(line);
                        if depth == 0 {
                            else_end = Some(idx);
                            break;
                        }
                    }
                    if let Some(else_end) = else_end {
                        if else_end == trimmed.len() - 1 {
                            let has_return_if = trimmed
                                .iter()
                                .take(if_end)
                                .skip(1)
                                .any(|l| l.starts_with("return ") && l.ends_with(';'));
                            let has_return_else = trimmed
                                .iter()
                                .take(else_end)
                                .skip(if_end + 2)
                                .any(|l| l.starts_with("return ") && l.ends_with(';'));

                            return Some(InlineHelperPlan {
                                lines: meta.body_lines.clone(),
                                append_null_return: !(has_return_if && has_return_else),
                            });
                        }
                    }
                }
            }
        }

        // Fallback: inline small mixed helpers (setup + branch) without nested _block calls.
        let mut depth = 0i32;
        let mut balanced = true;
        for line in &trimmed {
            depth += code_brace_delta(line);
            if depth < 0 {
                balanced = false;
                break;
            }
        }
        if balanced && depth == 0 {
            return Some(InlineHelperPlan {
                lines: meta.body_lines.clone(),
                append_null_return: true,
            });
        }

        None
    }

    pub(super) fn inline_helper_calls(&mut self, helper_id: usize, plan: &InlineHelperPlan) {
        let call = format!("return _block_{}();", helper_id);

        let mut i = 0usize;
        while i < self.lines.len() {
            if self.lines[i].trim() != call {
                i += 1;
                continue;
            }

            let call_indent = Self::leading_spaces(&self.lines[i]);
            let base_indent = plan
                .lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| Self::leading_spaces(l))
                .min()
                .unwrap_or(0);

            let mut replacement = Vec::new();
            for line in &plan.lines {
                if line.trim().is_empty() {
                    continue;
                }
                let rel = Self::leading_spaces(line).saturating_sub(base_indent);
                replacement.push(format!(
                    "{}{}",
                    " ".repeat(call_indent + rel),
                    line.trim_start()
                ));
            }
            if replacement.is_empty() {
                replacement.push(format!("{}return null;", " ".repeat(call_indent)));
            }
            if plan.append_null_return {
                replacement.push(format!("{}return null;", " ".repeat(call_indent)));
            }

            self.replace_body_line(i, replacement.clone());
            i += replacement.len();
        }
    }

    pub(super) fn inline_trivial_helpers(&mut self) {
        let first_pass = Self::scan_helpers(&self.lines);
        if first_pass.is_empty() {
            return;
        }

        for h in &first_pass {
            if let Some(expr) = &h.return_expr {
                let call = format!("return _block_{}();", h.id);
                let repl = format!("return {};", expr);
                for line in &mut self.lines {
                    if line.trim() == call {
                        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
                        *line = format!("{}{}", " ".repeat(indent), repl);
                    }
                }
            }
        }

        let second_pass = Self::scan_helpers(&self.lines);
        for h in &second_pass {
            let Some(plan) = Self::helper_inline_lines(h) else {
                continue;
            };
            self.inline_helper_calls(h.id, &plan);
        }

        let final_helpers = Self::scan_helpers(&self.lines);
        let mut remove_ranges = Vec::new();
        for h in &final_helpers {
            let token = format!("_block_{}(", h.id);
            if Self::token_count(&self.lines, &token) <= 1 {
                remove_ranges.push((h.start, h.end));
            }
        }
        remove_ranges.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (start, end) in remove_ranges {
            self.drain_body_lines(start..=end);
        }
    }

    /// Make every surviving `_block_N()` call resolve, and state the ones that
    /// cannot.
    ///
    /// A call whose helper was defined keeps both: rewriting it dropped the
    /// block's whole body from the artifact, and rewriting it to `return null;`
    /// in particular claimed the function returns there, which is an exit the
    /// graph does not contain. Only a call the helper budget refused to define
    /// is rewritten, and then into an explicit omission that names the block
    /// rather than into a fabricated return.
    ///
    /// Definitions nothing calls are dropped afterwards, so the call set and the
    /// definition set are equal in the finished artifact.
    pub(super) fn resolve_remaining_helpers(&mut self) {
        let defined: HashSet<usize> = Self::scan_helpers(&self.lines)
            .iter()
            .map(|h| h.id)
            .collect();

        let mut omitted_ids = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut rewritten = Vec::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            let Some(id) = Self::parse_helper_call(&self.lines[i]) else {
                i += 1;
                continue;
            };
            if defined.contains(&id) {
                i += 1;
                continue;
            }

            debug_assert!(
                self.helper_cap_omitted.contains(&id),
                "a helper call with no definition that the budget never refused: block {id}"
            );
            if seen_ids.insert(id) {
                omitted_ids.push(id);
            }
            let indent = Self::leading_spaces(&self.lines[i]);
            self.lines[i] = format!("{}// {}", " ".repeat(indent), helper_cap_note(id));
            rewritten.push(id);
            i += 1;
        }

        for id in rewritten {
            let source = self
                .omission_sources
                .get(&id)
                .copied()
                .unwrap_or_else(|| self.current_source_block());
            self.record_traversal_event(
                TraversalEventKind::HelperCapOmission,
                source,
                TraversalTarget::Helper { id },
            );
        }

        if !omitted_ids.is_empty() {
            omitted_ids.sort_unstable();
            omitted_ids.dedup();
            let details = omitted_ids
                .iter()
                .map(|id| format!("block {}", id))
                .collect::<Vec<_>>()
                .join(", ");
            let summary = format!("  // omitted complex paths: {}", details);
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

        self.drop_unreferenced_helpers();
        debug_assert_eq!(
            Self::helper_call_ids(&self.lines),
            Self::helper_definition_ids(&self.lines),
            "every helper call must resolve to exactly one definition"
        );
    }

    /// Remove helper definitions nothing calls, to a fixpoint.
    ///
    /// Removing one deletes the calls inside it, which can leave another helper
    /// reachable from nothing, so a single pass would leave definitions behind
    /// that the artifact never reaches. Only definitions are removed here, never
    /// calls, so no path can be lost by this.
    pub(super) fn drop_unreferenced_helpers(&mut self) {
        loop {
            let helpers = Self::scan_helpers(&self.lines);
            if helpers.is_empty() {
                return;
            }
            let called = Self::helper_call_ids(&self.lines);
            let mut remove_ranges: Vec<(usize, usize)> = helpers
                .into_iter()
                .filter(|h| !called.contains(&h.id))
                .map(|h| (h.start, h.end))
                .collect();
            if remove_ranges.is_empty() {
                return;
            }
            remove_ranges.sort_unstable_by(|a, b| b.0.cmp(&a.0));
            for (start, end) in remove_ranges {
                self.drain_body_lines(start..=end);
            }
        }
    }

    pub(super) fn helper_call_ids(lines: &[String]) -> BTreeSet<usize> {
        lines.iter().filter_map(|l| Self::parse_helper_call(l)).collect()
    }

    pub(super) fn helper_definition_ids(lines: &[String]) -> BTreeSet<usize> {
        Self::scan_helpers(lines).iter().map(|h| h.id).collect()
    }
}

/// How many helper definitions one function may carry.
pub(super) const HELPER_DEFINITION_BUDGET: usize = 64;

/// What a call the helper budget refused to define renders as.
///
/// Deliberately not `return null;`: the call site is an edge into a block that
/// exists and was not emitted, and a return there states an exit the graph does
/// not contain. The note names the block so the omission is attributable.
pub(super) fn helper_cap_note(id: usize) -> String {
    format!("omitted path to block {id}: helper budget exhausted, block not emitted")
}
