use super::*;

impl<'a> FuncEmitter<'a> {
    pub(super) fn parse_helper_header(line: &str) -> Option<usize> {
        let t = line.trim();
        if !t.starts_with("dynamic _block_") || !t.ends_with("() {") {
            return None;
        }
        let rest = t.strip_prefix("dynamic _block_")?;
        let id_s = rest.strip_suffix("() {")?;
        id_s.parse::<usize>().ok()
    }

    pub(super) fn parse_helper_call(line: &str) -> Option<usize> {
        let t = line.trim();
        if !t.starts_with("return _block_") || !t.ends_with("();") {
            return None;
        }
        let rest = t.strip_prefix("return _block_")?;
        let id_s = rest.strip_suffix("();")?;
        id_s.parse::<usize>().ok()
    }

    pub(super) fn scan_helpers(lines: &[String]) -> Vec<HelperMeta> {
        let mut out = Vec::new();
        let mut i = 0usize;

        while i < lines.len() {
            let Some(id) = Self::parse_helper_header(&lines[i]) else {
                i += 1;
                continue;
            };

            let mut depth = 0i32;
            let mut j = i;
            while j < lines.len() {
                let line = &lines[j];
                depth += line.chars().filter(|&c| c == '{').count() as i32;
                depth -= line.chars().filter(|&c| c == '}').count() as i32;
                if depth == 0 {
                    break;
                }
                j += 1;
            }
            if j >= lines.len() {
                break;
            }

            let mut body_lines = Vec::new();
            for line in &lines[i + 1..j] {
                body_lines.push(line.clone());
            }

            let mut statements = Vec::new();
            for line in &lines[i + 1..j] {
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                statements.push(t.to_string());
            }
            let return_expr = if statements.len() == 1 {
                let stmt = &statements[0];
                if stmt.starts_with("return ") && stmt.ends_with(';') {
                    Some(
                        stmt.trim_start_matches("return ")
                            .trim_end_matches(';')
                            .trim()
                            .to_string(),
                    )
                } else {
                    None
                }
            } else {
                None
            };

            out.push(HelperMeta {
                id,
                start: i,
                end: j,
                body_lines,
                return_expr,
            });
            i = j + 1;
        }

        out
    }

    pub(super) fn token_count(lines: &[String], token: &str) -> usize {
        lines.iter().map(|l| l.matches(token).count()).sum()
    }

    pub(super) fn leading_spaces(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }

    pub(super) fn helper_inline_lines(meta: &HelperMeta) -> Option<InlineHelperPlan> {
        let non_empty: Vec<&String> = meta
            .body_lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .collect();
        if non_empty.is_empty() || non_empty.len() > 28 {
            return None;
        }

        for line in &non_empty {
            let t = line.trim();
            if t.contains("_block_") {
                return None;
            }
        }

        let last = non_empty.last()?.trim();
        let linear_last_return = last.starts_with("return ") && last.ends_with(';');
        let linear_no_braces = non_empty.iter().all(|l| {
            let t = l.trim();
            !t.contains('{') && !t.contains('}')
        });
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
                depth += line.chars().filter(|&c| c == '{').count() as i32;
                depth -= line.chars().filter(|&c| c == '}').count() as i32;
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
                        depth += line.chars().filter(|&c| c == '{').count() as i32;
                        depth -= line.chars().filter(|&c| c == '}').count() as i32;
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
            depth += line.chars().filter(|&c| c == '{').count() as i32;
            depth -= line.chars().filter(|&c| c == '}').count() as i32;
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

            self.lines.splice(i..=i, replacement.clone());
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
            self.lines.drain(start..=end);
        }
    }

    pub(super) fn collapse_remaining_helpers(&mut self) {
        let mut omitted_ids = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            let Some(id) = Self::parse_helper_call(&self.lines[i]) else {
                i += 1;
                continue;
            };

            if seen_ids.insert(id) {
                omitted_ids.push(id);
            }
            let indent = Self::leading_spaces(&self.lines[i]);
            let replacement = vec![format!("{}return null;", " ".repeat(indent))];
            self.lines.splice(i..=i, replacement.clone());
            i += replacement.len();
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
            self.lines.insert(insert_idx, summary);
        }

        let helpers = Self::scan_helpers(&self.lines);
        if helpers.is_empty() {
            return;
        }

        let mut remove_ranges: Vec<(usize, usize)> =
            helpers.into_iter().map(|h| (h.start, h.end)).collect();
        remove_ranges.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (start, end) in remove_ranges {
            self.lines.drain(start..=end);
        }
    }

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
            helper.emit_block(id, 1, 0);
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
