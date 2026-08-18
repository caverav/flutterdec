use super::*;

impl<'a> FuncEmitter<'a> {
    pub(super) fn compact_lines(&mut self) {
        for _pass in 0..16 {
            let mut changed = false;
            let mut out = Vec::new();
            let mut i = 0usize;
            let mut retry_loop_id = 1usize;

            while i < self.lines.len() {
                let cur = &self.lines[i];
                let cur_trim = cur.trim();

                if let Some(var) = Self::retry_decl_var(cur_trim) {
                    if i + 1 < self.lines.len() {
                        let next_trim = self.lines[i + 1].trim();
                        if Self::while_var(next_trim).as_deref() == Some(var.as_str()) {
                            if let Some(loop_end) = Self::find_block_end(&self.lines, i + 1) {
                                let has_continue = (i + 2..loop_end)
                                    .any(|idx| self.lines[idx].trim() == "continue;");
                                // Unwrapping dedents the body, so a `break;` that
                                // binds to this loop would end up outside every
                                // loop and stop naming the edge it stands for.
                                let bound_control = Self::loop_bound_control_lines(
                                    &self.lines,
                                    i + 2,
                                    loop_end,
                                );
                                if !has_continue && bound_control.is_empty() {
                                    for idx in i + 2..loop_end {
                                        let t = self.lines[idx].trim();
                                        if t == format!("{var} = false;")
                                            || t == format!("{var} = true;")
                                        {
                                            continue;
                                        }
                                        out.push(Self::dedent_once(&self.lines[idx]));
                                    }
                                    i = loop_end + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }
                }

                if cur_trim == "while (true) {" {
                    if let Some(j) = Self::find_block_end(&self.lines, i) {
                        let mut rel_depth = 1i32;
                        let mut has_continue = false;
                        let mut continue_count = 0usize;
                        let mut last_non_empty = None;
                        let mut break_at_top_level = false;

                        for idx in i + 1..j {
                            let t = self.lines[idx].trim();
                            if !t.is_empty() {
                                last_non_empty = Some(idx);
                                if t == "continue;" {
                                    has_continue = true;
                                    continue_count += 1;
                                }
                            }
                            rel_depth +=
                                self.lines[idx].chars().filter(|&c| c == '{').count() as i32;
                            rel_depth -=
                                self.lines[idx].chars().filter(|&c| c == '}').count() as i32;
                        }

                        if let Some(last_idx) = last_non_empty {
                            break_at_top_level =
                                self.lines[last_idx].trim() == "break;" && rel_depth == 1;
                            if self.lines[last_idx].trim() == "break;" {
                                let mut depth_at_break = 1i32;
                                for idx in i + 1..last_idx {
                                    depth_at_break +=
                                        self.lines[idx].chars().filter(|&c| c == '{').count()
                                            as i32;
                                    depth_at_break -=
                                        self.lines[idx].chars().filter(|&c| c == '}').count()
                                            as i32;
                                }
                                break_at_top_level = depth_at_break == 1;
                            }
                        }

                        // Every `break;` and `continue;` in the body that binds to
                        // this loop. Dedenting the body leaves each of them
                        // outside every loop, where the statement is not Dart and
                        // the edge it was emitted for is gone from the artifact,
                        // so the wrapper stays unless the body owns none of them -
                        // or, below, unless the only one is the wrapper's own
                        // trailing `break;`, which is removed with it.
                        let bound_control = Self::loop_bound_control_lines(&self.lines, i + 1, j);

                        if bound_control.is_empty()
                            && !has_continue
                            && Self::block_terminates_at_top_level(&self.lines, i + 1, j)
                        {
                            for idx in i + 1..j {
                                out.push(Self::dedent_once(&self.lines[idx]));
                            }
                            i = j + 1;
                            changed = true;
                            continue;
                        }

                        if break_at_top_level
                            && !has_continue
                            && bound_control.len() == 1
                            && Some(bound_control[0]) == last_non_empty
                        {
                            for idx in i + 1..j {
                                if Some(idx) == last_non_empty && self.lines[idx].trim() == "break;"
                                {
                                    continue;
                                }
                                out.push(Self::dedent_once(&self.lines[idx]));
                            }
                            i = j + 1;
                            changed = true;
                            continue;
                        }

                        if break_at_top_level && continue_count >= 2 {
                            let indent = Self::leading_indent(cur);
                            let retry_var = format!("retryLoop{retry_loop_id}");
                            retry_loop_id += 1;

                            out.push(format!("{}bool {} = true;", " ".repeat(indent), retry_var));
                            out.push(format!("{}while ({}) {{", " ".repeat(indent), retry_var));

                            for idx in i + 1..j {
                                if Some(idx) == last_non_empty && self.lines[idx].trim() == "break;"
                                {
                                    continue;
                                }
                                out.push(self.lines[idx].clone());
                            }
                            out.push(format!("{}{} = false;", " ".repeat(indent + 2), retry_var));

                            out.push(self.lines[j].clone());
                            i = j + 1;
                            changed = true;
                            continue;
                        }
                    }
                }

                if cur_trim.starts_with("if (") && cur_trim.ends_with(") {") {
                    let indent = Self::leading_indent(cur);
                    if let Some((ret_stmt, final_ret_idx)) =
                        Self::redundant_guarded_return_chain(&self.lines, i, indent)
                    {
                        out.push(format!("{}{}", " ".repeat(indent), ret_stmt));
                        i = final_ret_idx + 1;
                        changed = true;
                        continue;
                    }
                    if let Some((ret_stmt, then_end)) =
                        Self::collapse_guarded_returns_inside_if(&self.lines, i)
                    {
                        out.push(cur.clone());
                        out.push(format!("{}{}", " ".repeat(indent + 2), ret_stmt));
                        out.push(self.lines[then_end].clone());
                        i = then_end + 1;
                        changed = true;
                        continue;
                    }
                }

                if cur_trim.starts_with("if (") && cur_trim.ends_with(") {") {
                    let cond = Self::if_condition(cur_trim).unwrap_or("");
                    if !cond.contains("flags.") && !cond.contains("/* cond */") {
                        if let Some(first_end) = Self::find_block_end(&self.lines, i) {
                            let mut first_else = first_end + 1;
                            while first_else < self.lines.len()
                                && self.lines[first_else].trim().is_empty()
                            {
                                first_else += 1;
                            }
                            let first_has_else = first_else < self.lines.len()
                                && self.lines[first_else].trim() == "else {";
                            if !first_has_else {
                                if let Some(then_ret) =
                                    Self::single_top_level_return(&self.lines, i + 1, first_end)
                                {
                                    let indent =
                                        cur.chars().take_while(|c| c.is_whitespace()).count();
                                    let mut next = first_end + 1;
                                    while next < self.lines.len()
                                        && self.lines[next].trim().is_empty()
                                    {
                                        next += 1;
                                    }
                                    if next < self.lines.len() {
                                        let next_line = &self.lines[next];
                                        let next_trim = next_line.trim();
                                        if Self::leading_indent(next_line) == indent
                                            && next_trim.starts_with("if (")
                                            && next_trim.ends_with(") {")
                                        {
                                            if let Some(next_end) =
                                                Self::find_block_end(&self.lines, next)
                                            {
                                                let mut next_else = next_end + 1;
                                                while next_else < self.lines.len()
                                                    && self.lines[next_else].trim().is_empty()
                                                {
                                                    next_else += 1;
                                                }
                                                let next_has_else = next_else < self.lines.len()
                                                    && self.lines[next_else].trim() == "else {";
                                                if !next_has_else
                                                    && Self::single_top_level_stmt(
                                                        &self.lines,
                                                        next + 1,
                                                        next_end,
                                                    )
                                                    .as_deref()
                                                        == Some("continue;")
                                                {
                                                    if let Some((lhs1, op1, rhs1)) =
                                                        Self::parse_simple_cmp(cond)
                                                    {
                                                        if let Some((lhs2, op2, rhs2)) =
                                                            Self::parse_simple_cmp(
                                                                Self::if_condition(next_trim)
                                                                    .unwrap_or(""),
                                                            )
                                                        {
                                                            if lhs1 == lhs2
                                                                && op1 == ">"
                                                                && op2 == ">="
                                                            {
                                                                if let (Some(k), Some(l)) = (
                                                                    Self::parse_int_literal(&rhs1),
                                                                    Self::parse_int_literal(&rhs2),
                                                                ) {
                                                                    if l <= k {
                                                                        out.push(format!(
                                                                            "{}if (({} >= {}) && ({} <= {})) {{",
                                                                            " ".repeat(indent),
                                                                            lhs2,
                                                                            rhs2,
                                                                            lhs2,
                                                                            rhs1
                                                                        ));
                                                                        out.push(format!(
                                                                            "{}continue;",
                                                                            " ".repeat(indent + 2)
                                                                        ));
                                                                        out.push(format!(
                                                                            "{}}}",
                                                                            " ".repeat(indent)
                                                                        ));
                                                                        out.push(cur.clone());
                                                                        out.push(format!(
                                                                            "{}{}",
                                                                            " ".repeat(indent + 2),
                                                                            then_ret
                                                                        ));
                                                                        out.push(
                                                                            self.lines[first_end]
                                                                                .clone(),
                                                                        );
                                                                        i = next_end + 1;
                                                                        changed = true;
                                                                        continue;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(then_end) = Self::find_block_end(&self.lines, i) {
                            let mut then_else = then_end + 1;
                            while then_else < self.lines.len()
                                && self.lines[then_else].trim().is_empty()
                            {
                                then_else += 1;
                            }
                            let has_else = then_else < self.lines.len()
                                && self.lines[then_else].trim() == "else {";
                            if !has_else {
                                if let Some(then_ret) =
                                    Self::single_top_level_return(&self.lines, i + 1, then_end)
                                {
                                    let mut next = then_end + 1;
                                    while next < self.lines.len()
                                        && self.lines[next].trim().is_empty()
                                    {
                                        next += 1;
                                    }
                                    if next < self.lines.len()
                                        && self.lines[next].trim() == then_ret
                                    {
                                        let indent =
                                            cur.chars().take_while(|c| c.is_whitespace()).count();
                                        out.push(format!("{}{}", " ".repeat(indent), then_ret));
                                        i = next + 1;
                                        changed = true;
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(first_cond) = Self::if_condition(cur_trim) {
                        if let Some(first_end) = Self::find_block_end(&self.lines, i) {
                            let mut first_else = first_end + 1;
                            while first_else < self.lines.len()
                                && self.lines[first_else].trim().is_empty()
                            {
                                first_else += 1;
                            }
                            let first_has_else = first_else < self.lines.len()
                                && self.lines[first_else].trim() == "else {";
                            let mut conds = Vec::new();
                            if !first_has_else
                                && Self::single_top_level_stmt(&self.lines, i + 1, first_end)
                                    .as_deref()
                                    == Some("continue;")
                            {
                                conds.push(first_cond.to_string());
                                let indent = Self::leading_indent(cur);
                                let mut end = first_end;
                                loop {
                                    let mut next = end + 1;
                                    while next < self.lines.len()
                                        && self.lines[next].trim().is_empty()
                                    {
                                        next += 1;
                                    }
                                    if next >= self.lines.len() {
                                        break;
                                    }
                                    if Self::leading_indent(&self.lines[next]) != indent {
                                        break;
                                    }
                                    let next_trim = self.lines[next].trim();
                                    let Some(next_cond) = Self::if_condition(next_trim) else {
                                        break;
                                    };
                                    let Some(next_end) = Self::find_block_end(&self.lines, next)
                                    else {
                                        break;
                                    };
                                    let mut next_else = next_end + 1;
                                    while next_else < self.lines.len()
                                        && self.lines[next_else].trim().is_empty()
                                    {
                                        next_else += 1;
                                    }
                                    let next_has_else = next_else < self.lines.len()
                                        && self.lines[next_else].trim() == "else {";
                                    if next_has_else {
                                        break;
                                    }
                                    if Self::single_top_level_stmt(&self.lines, next + 1, next_end)
                                        .as_deref()
                                        != Some("continue;")
                                    {
                                        break;
                                    }
                                    conds.push(next_cond.to_string());
                                    end = next_end;
                                }

                                if conds.len() >= 2 {
                                    out.push(format!(
                                        "{}if ({}) {{",
                                        " ".repeat(indent),
                                        conds
                                            .iter()
                                            .map(|c| format!("({})", c))
                                            .collect::<Vec<_>>()
                                            .join(" || ")
                                    ));
                                    out.push(format!("{}continue;", " ".repeat(indent + 2)));
                                    out.push(format!("{}}}", " ".repeat(indent)));
                                    i = end + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(outer_cond) = Self::if_condition(cur_trim) {
                        if let Some(outer_end) = Self::find_block_end(&self.lines, i) {
                            let mut inner_start = None;
                            for idx in i + 1..outer_end {
                                if !self.lines[idx].trim().is_empty() {
                                    inner_start = Some(idx);
                                    break;
                                }
                            }

                            if let Some(inner_start) = inner_start {
                                let inner_trim = self.lines[inner_start].trim();
                                if Self::leading_indent(&self.lines[inner_start])
                                    == Self::leading_indent(cur) + 2
                                    && inner_trim.starts_with("if (")
                                    && inner_trim.ends_with(") {")
                                {
                                    if let Some(inner_end) =
                                        Self::find_block_end(&self.lines, inner_start)
                                    {
                                        if inner_end < outer_end {
                                            let mut only_inner = true;
                                            for idx in i + 1..outer_end {
                                                if idx >= inner_start && idx <= inner_end {
                                                    continue;
                                                }
                                                if !self.lines[idx].trim().is_empty() {
                                                    only_inner = false;
                                                    break;
                                                }
                                            }
                                            if only_inner {
                                                if let Some(inner_cond) =
                                                    Self::if_condition(inner_trim)
                                                {
                                                    let indent = Self::leading_indent(cur);
                                                    out.push(format!(
                                                        "{}if (({}) && ({})) {{",
                                                        " ".repeat(indent),
                                                        outer_cond,
                                                        inner_cond
                                                    ));
                                                    for idx in inner_start + 1..inner_end {
                                                        out.push(Self::dedent_once(
                                                            &self.lines[idx],
                                                        ));
                                                    }
                                                    out.push(self.lines[outer_end].clone());
                                                    i = outer_end + 1;
                                                    changed = true;
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let cur_indent = Self::leading_indent(cur);
                    if let Some(id) = Self::null_checked_ident(cur_trim) {
                        if let Some(first_end) = Self::find_block_end(&self.lines, i) {
                            let mut first_else = first_end + 1;
                            while first_else < self.lines.len()
                                && self.lines[first_else].trim().is_empty()
                            {
                                first_else += 1;
                            }
                            let first_has_else = first_else < self.lines.len()
                                && self.lines[first_else].trim() == "else {";
                            if !first_has_else
                                && Self::block_terminates_at_top_level(
                                    &self.lines,
                                    i + 1,
                                    first_end,
                                )
                            {
                                let mut rewritten = false;
                                let mut scan = first_end + 1;
                                while scan < self.lines.len() {
                                    let line = &self.lines[scan];
                                    let t = line.trim();
                                    if t.is_empty() {
                                        scan += 1;
                                        continue;
                                    }

                                    let indent = Self::leading_indent(line);
                                    if indent < cur_indent {
                                        break;
                                    }

                                    if Self::assigns_ident(line, &id) {
                                        break;
                                    }

                                    if indent == cur_indent
                                        && t.starts_with("if (")
                                        && t.ends_with(") {")
                                        && Self::null_checked_ident(t).as_deref() == Some(&id)
                                    {
                                        if let Some(second_end) =
                                            Self::find_block_end(&self.lines, scan)
                                        {
                                            for idx in i..scan {
                                                out.push(self.lines[idx].clone());
                                            }

                                            let mut second_else = second_end + 1;
                                            while second_else < self.lines.len()
                                                && self.lines[second_else].trim().is_empty()
                                            {
                                                second_else += 1;
                                            }
                                            if second_else < self.lines.len()
                                                && self.lines[second_else].trim() == "else {"
                                            {
                                                if let Some(second_else_end) =
                                                    Self::find_block_end(&self.lines, second_else)
                                                {
                                                    for idx in second_else + 1..second_else_end {
                                                        out.push(Self::dedent_once(
                                                            &self.lines[idx],
                                                        ));
                                                    }
                                                    i = second_else_end + 1;
                                                } else {
                                                    i = second_end + 1;
                                                }
                                            } else {
                                                i = second_end + 1;
                                            }
                                            changed = true;
                                            rewritten = true;
                                            break;
                                        }
                                    }

                                    scan += 1;
                                }
                                if rewritten {
                                    continue;
                                }
                            }
                        }
                    }

                    let cond = cur_trim
                        .strip_prefix("if (")
                        .and_then(|s| s.strip_suffix(") {"))
                        .unwrap_or("");

                    if !cond.contains("flags.") && !cond.contains("/* cond */") {
                        if let Some(then_end) = Self::find_block_end(&self.lines, i) {
                            if let Some(then_ret) =
                                Self::single_top_level_return(&self.lines, i + 1, then_end)
                            {
                                let mut next = then_end + 1;
                                while next < self.lines.len() && self.lines[next].trim().is_empty()
                                {
                                    next += 1;
                                }
                                if next < self.lines.len() && self.lines[next].trim() == then_ret {
                                    let indent =
                                        cur.chars().take_while(|c| c.is_whitespace()).count();
                                    out.push(format!("{}{}", " ".repeat(indent), then_ret));
                                    i = next + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(then_end) = Self::find_block_end(&self.lines, i) {
                        let mut else_start = then_end + 1;
                        while else_start < self.lines.len()
                            && self.lines[else_start].trim().is_empty()
                        {
                            else_start += 1;
                        }
                        if else_start < self.lines.len()
                            && self.lines[else_start].trim() == "else {"
                        {
                            if let Some(else_end) = Self::find_block_end(&self.lines, else_start) {
                                if Self::block_terminates_at_top_level(&self.lines, i + 1, then_end)
                                {
                                    for idx in i..=then_end {
                                        out.push(self.lines[idx].clone());
                                    }
                                    for idx in else_start + 1..else_end {
                                        out.push(Self::dedent_once(&self.lines[idx]));
                                    }
                                    i = else_end + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }

                    if !cond.contains("flags.") && !cond.contains("/* cond */") {
                        let mut j = i + 1;
                        while j < self.lines.len() && self.lines[j].trim().is_empty() {
                            j += 1;
                        }
                        if j < self.lines.len() && self.lines[j].trim().starts_with("return ") {
                            let then_ret = self.lines[j].trim().to_string();
                            let mut k = j + 1;
                            while k < self.lines.len() && self.lines[k].trim().is_empty() {
                                k += 1;
                            }
                            if k < self.lines.len() && self.lines[k].trim() == "}" {
                                let mut l = k + 1;
                                while l < self.lines.len() && self.lines[l].trim().is_empty() {
                                    l += 1;
                                }
                                if l < self.lines.len() && self.lines[l].trim() == "else {" {
                                    let mut m = l + 1;
                                    while m < self.lines.len() && self.lines[m].trim().is_empty() {
                                        m += 1;
                                    }
                                    if m < self.lines.len() && self.lines[m].trim() == then_ret {
                                        let mut n = m + 1;
                                        while n < self.lines.len()
                                            && self.lines[n].trim().is_empty()
                                        {
                                            n += 1;
                                        }
                                        if n < self.lines.len() && self.lines[n].trim() == "}" {
                                            let indent = cur
                                                .chars()
                                                .take_while(|c| c.is_whitespace())
                                                .count();
                                            out.push(format!("{}{}", " ".repeat(indent), then_ret));
                                            i = n + 1;
                                            changed = true;
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let mut j = i + 1;
                    while j < self.lines.len() && self.lines[j].trim().is_empty() {
                        j += 1;
                    }
                    if j < self.lines.len() && self.lines[j].trim() == "}" {
                        let mut k = j + 1;
                        while k < self.lines.len() && self.lines[k].trim().is_empty() {
                            k += 1;
                        }
                        if k < self.lines.len() && self.lines[k].trim() == "else {" {
                            let mut depth = 0i32;
                            let mut m = None;
                            for idx in k..self.lines.len() {
                                let line = &self.lines[idx];
                                depth += line.chars().filter(|&c| c == '{').count() as i32;
                                depth -= line.chars().filter(|&c| c == '}').count() as i32;
                                if depth == 0 {
                                    m = Some(idx);
                                    break;
                                }
                            }
                            if let Some(m) = m {
                                if let Some(cond) = cur_trim
                                    .strip_prefix("if (")
                                    .and_then(|s| s.strip_suffix(") {"))
                                {
                                    let indent =
                                        cur.chars().take_while(|c| c.is_whitespace()).count();
                                    out.push(format!("{}if (!({})) {{", " ".repeat(indent), cond));
                                    for line in &self.lines[k + 1..m] {
                                        out.push(line.clone());
                                    }
                                    out.push(self.lines[m].clone());
                                    i = m + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }
                }

                if cur_trim == "else {" {
                    let mut j = i + 1;
                    while j < self.lines.len() && self.lines[j].trim().is_empty() {
                        j += 1;
                    }
                    if j < self.lines.len() && self.lines[j].trim() == "}" {
                        i = j + 1;
                        changed = true;
                        continue;
                    }
                }

                if cur_trim == "return null;"
                    && out
                        .last()
                        .is_some_and(|p: &String| p.trim() == "return null;")
                {
                    i += 1;
                    changed = true;
                    continue;
                }

                if Self::is_terminal_statement(cur_trim) {
                    let cur_indent = Self::leading_indent(cur);
                    let mut j = i + 1;
                    let mut skipped_any = false;
                    while j < self.lines.len() {
                        let next = &self.lines[j];
                        let next_trim = next.trim();
                        if next_trim.is_empty() {
                            j += 1;
                            skipped_any = true;
                            continue;
                        }
                        let next_indent = Self::leading_indent(next);
                        if next_trim.starts_with('}') && next_indent <= cur_indent {
                            break;
                        }
                        j += 1;
                        skipped_any = true;
                    }
                    if skipped_any {
                        out.push(cur.clone());
                        i = j;
                        changed = true;
                        continue;
                    }
                }

                out.push(cur.clone());
                i += 1;
            }

            self.lines = out;
            if !changed {
                break;
            }
        }
    }
}
