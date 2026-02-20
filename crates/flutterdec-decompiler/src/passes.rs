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
                                if !has_continue {
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

                        if break_at_top_level && !has_continue {
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
                                    {
                                        if Self::null_checked_ident(t).as_deref() == Some(&id) {
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
                                                        Self::find_block_end(
                                                            &self.lines,
                                                            second_else,
                                                        )
                                                    {
                                                        for idx in second_else + 1..second_else_end
                                                        {
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

                out.push(cur.clone());
                i += 1;
            }

            self.lines = out;
            if !changed {
                break;
            }
        }
    }

    pub(super) fn is_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    pub(super) fn dedent_once(line: &str) -> String {
        line.strip_prefix("  ").unwrap_or(line).to_string()
    }

    pub(super) fn leading_indent(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }

    pub(super) fn find_block_end(lines: &[String], start: usize) -> Option<usize> {
        let mut depth = 0i32;
        for (idx, line) in lines.iter().enumerate().skip(start) {
            depth += line.chars().filter(|&c| c == '{').count() as i32;
            depth -= line.chars().filter(|&c| c == '}').count() as i32;
            if depth == 0 {
                return Some(idx);
            }
        }
        None
    }

    pub(super) fn if_condition(line_trim: &str) -> Option<&str> {
        Some(
            line_trim
                .strip_prefix("if (")
                .and_then(|s| s.strip_suffix(") {"))?
                .trim(),
        )
    }

    pub(super) fn parse_simple_cmp(cond: &str) -> Option<(String, String, String)> {
        let c = cond.trim();
        if c.contains("||") || c.contains("&&") {
            return None;
        }
        for op in [">=", "<=", "==", "!=", ">", "<"] {
            if let Some((lhs, rhs)) = c.split_once(op) {
                return Some((
                    lhs.trim().to_string(),
                    op.to_string(),
                    rhs.trim().to_string(),
                ));
            }
        }
        None
    }

    pub(super) fn parse_int_literal(s: &str) -> Option<i64> {
        let t = s.trim().trim_start_matches('#');
        if let Some(hex) = t.strip_prefix("0x") {
            return i64::from_str_radix(hex, 16).ok();
        }
        t.parse::<i64>().ok()
    }

    pub(super) fn retry_decl_var(line_trim: &str) -> Option<String> {
        let rest = line_trim.strip_prefix("bool ")?;
        let var = rest.strip_suffix(" = true;")?.trim();
        if var.is_empty() || !var.chars().all(Self::is_ident_char) {
            return None;
        }
        Some(var.to_string())
    }

    pub(super) fn while_var(line_trim: &str) -> Option<String> {
        let var = line_trim
            .strip_prefix("while (")
            .and_then(|s| s.strip_suffix(") {"))?
            .trim();
        if var.is_empty() || !var.chars().all(Self::is_ident_char) {
            return None;
        }
        Some(var.to_string())
    }

    pub(super) fn redundant_guarded_return_chain(
        lines: &[String],
        start: usize,
        indent: usize,
    ) -> Option<(String, usize)> {
        if start >= lines.len() {
            return None;
        }
        let mut idx = start;
        let mut expected_ret: Option<String> = None;

        loop {
            if idx >= lines.len() {
                return None;
            }
            let line = &lines[idx];
            let t = line.trim();
            if Self::leading_indent(line) != indent || !t.starts_with("if (") || !t.ends_with(") {")
            {
                return None;
            }
            let cond = Self::if_condition(t)?;
            if cond.contains("flags.") || cond.contains("/* cond */") {
                return None;
            }

            let then_end = Self::find_block_end(lines, idx)?;
            let mut else_start = then_end + 1;
            while else_start < lines.len() && lines[else_start].trim().is_empty() {
                else_start += 1;
            }
            if else_start < lines.len() && lines[else_start].trim() == "else {" {
                return None;
            }

            let then_ret = Self::single_top_level_return(lines, idx + 1, then_end)?;
            if let Some(existing) = &expected_ret {
                if existing != &then_ret {
                    return None;
                }
            } else {
                expected_ret = Some(then_ret);
            }

            idx = then_end + 1;
            while idx < lines.len() && lines[idx].trim().is_empty() {
                idx += 1;
            }
            if idx >= lines.len() {
                return None;
            }
            if Self::leading_indent(&lines[idx]) != indent {
                return None;
            }

            let t = lines[idx].trim();
            if Some(t) == expected_ret.as_deref() {
                return Some((expected_ret.unwrap_or_default(), idx));
            }
            if t.starts_with("if (") && t.ends_with(") {") {
                continue;
            }
            return None;
        }
    }

    pub(super) fn collapse_guarded_returns_inside_if(
        lines: &[String],
        start: usize,
    ) -> Option<(String, usize)> {
        if start >= lines.len() {
            return None;
        }
        let start_trim = lines[start].trim();
        if !start_trim.starts_with("if (") || !start_trim.ends_with(") {") {
            return None;
        }
        let then_end = Self::find_block_end(lines, start)?;

        let mut else_start = then_end + 1;
        while else_start < lines.len() && lines[else_start].trim().is_empty() {
            else_start += 1;
        }
        if else_start < lines.len() && lines[else_start].trim() == "else {" {
            return None;
        }

        #[derive(Debug)]
        enum TopStmt {
            IfRet(String),
            Ret(String),
        }

        let mut stmts = Vec::new();
        let mut idx = start + 1;
        while idx < then_end {
            let t = lines[idx].trim();
            if t.is_empty() {
                idx += 1;
                continue;
            }

            if t.starts_with("if (") && t.ends_with(") {") {
                let nested_end = Self::find_block_end(lines, idx)?;
                if nested_end >= then_end {
                    return None;
                }
                let mut nested_else = nested_end + 1;
                while nested_else < then_end && lines[nested_else].trim().is_empty() {
                    nested_else += 1;
                }
                if nested_else < then_end && lines[nested_else].trim() == "else {" {
                    return None;
                }
                let ret = Self::single_top_level_return(lines, idx + 1, nested_end)?;
                stmts.push(TopStmt::IfRet(ret));
                idx = nested_end + 1;
                continue;
            }

            if t.starts_with("return ") {
                stmts.push(TopStmt::Ret(t.to_string()));
                idx += 1;
                continue;
            }
            return None;
        }

        if stmts.len() < 2 {
            return None;
        }
        let final_ret = match stmts.last()? {
            TopStmt::Ret(r) => r.clone(),
            TopStmt::IfRet(_) => return None,
        };
        for stmt in &stmts[..stmts.len() - 1] {
            let TopStmt::IfRet(r) = stmt else {
                return None;
            };
            if *r != final_ret {
                return None;
            }
        }

        Some((final_ret, then_end))
    }

    pub(super) fn null_checked_ident(line_trim: &str) -> Option<String> {
        let cond = Self::if_condition(line_trim)?;
        let (lhs, rhs) = cond.split_once("==")?;
        let lhs = lhs.trim();
        let rhs = rhs.trim();
        let ident = if rhs == "null" {
            lhs
        } else if lhs == "null" {
            rhs
        } else {
            return None;
        };
        if ident.is_empty() || !ident.chars().all(Self::is_ident_char) {
            return None;
        }
        Some(ident.to_string())
    }

    pub(super) fn assigns_ident(line: &str, ident: &str) -> bool {
        let t = line.trim();
        if t.starts_with("if (") {
            return false;
        }
        let mut i = 0usize;
        let bytes = t.as_bytes();
        while i + ident.len() <= t.len() {
            if t[i..].starts_with(ident) {
                let prev_ok = if i == 0 {
                    true
                } else {
                    !Self::is_ident_char(bytes[i - 1] as char)
                };
                let next_i = i + ident.len();
                let next_ok = if next_i >= t.len() {
                    true
                } else {
                    !Self::is_ident_char(bytes[next_i] as char)
                };
                if prev_ok && next_ok {
                    let rest = t[next_i..].trim_start();
                    if rest.starts_with('=') && !rest.starts_with("==") {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    pub(super) fn block_terminates_at_top_level(
        lines: &[String],
        start: usize,
        end: usize,
    ) -> bool {
        if start >= end || end > lines.len() {
            return false;
        }

        let mut rel_depth = 1i32;
        let mut last_top_level_stmt = None;
        for line in lines.iter().take(end).skip(start) {
            let t = line.trim();
            if rel_depth == 1 && !t.is_empty() {
                last_top_level_stmt = Some(t.to_string());
            }
            rel_depth += line.chars().filter(|&c| c == '{').count() as i32;
            rel_depth -= line.chars().filter(|&c| c == '}').count() as i32;
        }

        let Some(stmt) = last_top_level_stmt else {
            return false;
        };
        stmt.starts_with("return ") || stmt == "continue;" || stmt == "break;"
    }

    pub(super) fn single_top_level_return(
        lines: &[String],
        start: usize,
        end: usize,
    ) -> Option<String> {
        let only = Self::single_top_level_stmt(lines, start, end)?;
        if only.starts_with("return ") {
            Some(only)
        } else {
            None
        }
    }

    pub(super) fn single_top_level_stmt(
        lines: &[String],
        start: usize,
        end: usize,
    ) -> Option<String> {
        if start >= end || end > lines.len() {
            return None;
        }

        let mut rel_depth = 1i32;
        let mut top_level: Vec<String> = Vec::new();
        for line in lines.iter().take(end).skip(start) {
            let t = line.trim();
            if rel_depth == 1 && !t.is_empty() {
                top_level.push(t.to_string());
            }
            rel_depth += line.chars().filter(|&c| c == '{').count() as i32;
            rel_depth -= line.chars().filter(|&c| c == '}').count() as i32;
        }

        if top_level.len() != 1 {
            return None;
        }
        Some(top_level.remove(0))
    }

    pub(super) fn replace_identifier_token(line: &str, from: &str, to: &str) -> String {
        if from.is_empty() || from == to {
            return line.to_string();
        }

        let mut out = String::with_capacity(line.len());
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < line.len() {
            if line[i..].starts_with(from) {
                let prev_ok = if i == 0 {
                    true
                } else {
                    !Self::is_ident_char(bytes[i - 1] as char)
                };
                let next_i = i + from.len();
                let next_ok = if next_i >= line.len() {
                    true
                } else {
                    !Self::is_ident_char(bytes[next_i] as char)
                };
                if prev_ok && next_ok {
                    out.push_str(to);
                    i += from.len();
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    pub(super) fn collect_ident_stats(lines: &[String], id: &str) -> IdentStats {
        let mut s = IdentStats::default();
        let field_pat = format!("{id}.");
        let null_eq_1 = format!("{id} == null");
        let null_eq_2 = format!("null == {id}");
        let null_ne_1 = format!("{id} != null");
        let null_ne_2 = format!("null != {id}");
        let call_assign = format!("{id} = t");

        for line in lines {
            let t = line.trim();
            s.field_access += t.matches(&field_pat).count();
            s.arith_ops += t.matches(&format!("{id} +")).count();
            s.arith_ops += t.matches(&format!("{id} -")).count();
            s.arith_ops += t.matches(&format!("{id} <<")).count();
            s.arith_ops += t.matches(&format!("{id} >>")).count();
            s.arith_ops += t.matches(&format!("{id} &")).count();
            s.arith_ops += t.matches(&format!("{id} |")).count();
            s.arith_ops += t.matches(&format!("{id} ^")).count();
            s.null_cmp += t.matches(&null_eq_1).count();
            s.null_cmp += t.matches(&null_eq_2).count();
            s.null_cmp += t.matches(&null_ne_1).count();
            s.null_cmp += t.matches(&null_ne_2).count();

            if t.starts_with(&format!("{id} = pool["))
                || t.contains(&format!("{id} = (pool["))
                || t.contains(&format!("{id} = ((pool["))
            {
                s.pool_assign += 1;
            }
            if t.starts_with(&call_assign) {
                s.call_assign += 1;
            }
        }
        s
    }

    pub(super) fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
        if !used.contains(base) {
            used.insert(base.to_string());
            return base.to_string();
        }
        let mut i = 2usize;
        loop {
            let candidate = format!("{base}{i}");
            if !used.contains(&candidate) {
                used.insert(candidate.clone());
                return candidate;
            }
            i += 1;
        }
    }

    pub(super) fn is_local_decl_line(t: &str) -> bool {
        if !(t.starts_with("int ") || t.starts_with("dynamic ")) {
            return false;
        }
        if !t.ends_with(';') || t.contains('=') {
            return false;
        }
        !t.contains('(')
    }

    pub(super) fn prelude_insert_index(lines: &[String]) -> usize {
        let mut idx = 1usize;
        while idx < lines.len() {
            let t = lines[idx].trim();
            if t.is_empty() || t.starts_with("//") || Self::is_local_decl_line(t) {
                idx += 1;
                continue;
            }
            break;
        }
        idx
    }

    pub(super) fn minus_one_idents(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut start = 0usize;
        while let Some(rel) = line[start..].find(" - 1)") {
            let idx = start + rel;
            let prefix = &line[..idx];
            if let Some(lp) = prefix.rfind('(') {
                let ident = prefix[lp + 1..].trim();
                if !ident.is_empty()
                    && ident.chars().all(Self::is_ident_char)
                    && ident
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                {
                    out.push(ident.to_string());
                }
            }
            start = idx + " - 1)".len();
        }
        out
    }

    pub(super) fn name_taken(lines: &[String], name: &str) -> bool {
        lines.iter().any(|l| l.contains(name))
    }

    pub(super) fn identifier_assigned(lines: &[String], ident: &str) -> bool {
        lines.iter().any(|l| Self::assigns_ident(l, ident))
    }

    pub(super) fn extract_minus_one_aliases(&mut self) {
        if self.lines.len() < 3 {
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in &self.lines {
            for ident in Self::minus_one_idents(line) {
                *counts.entry(ident).or_insert(0) += 1;
            }
        }

        let mut candidates: Vec<(String, usize)> = counts
            .into_iter()
            .filter(|(_, count)| *count >= 4)
            .collect();
        candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        if candidates.is_empty() {
            return;
        }

        let insert_idx = Self::prelude_insert_index(&self.lines);
        let mut inserts = Vec::new();
        for (ident, _) in candidates {
            if Self::identifier_assigned(&self.lines, &ident) {
                continue;
            }
            let pattern = format!("({ident} - 1)");
            if !self.lines.iter().any(|l| l.contains(&pattern)) {
                continue;
            }

            let base = if ident.starts_with("value") {
                "codePoint".to_string()
            } else {
                format!("{ident}Minus1")
            };
            let mut alias = base.clone();
            let mut n = 2usize;
            while Self::name_taken(&self.lines, &alias)
                || inserts.iter().any(|l: &String| l.contains(&alias))
            {
                alias = format!("{base}{n}");
                n += 1;
            }

            let mut replaced = false;
            for line in &mut self.lines {
                if line.contains(&pattern) {
                    *line = line.replace(&pattern, &alias);
                    replaced = true;
                }
            }
            if replaced {
                inserts.push(format!("  final int {alias} = ({ident} - 1);"));
            }
        }

        if !inserts.is_empty() {
            self.lines.splice(insert_idx..insert_idx, inserts);
        }
    }

    pub(super) fn apply_name_and_type_hints(&mut self, fn_name: &str) {
        if self.lines.is_empty() {
            return;
        }

        let arg_ids: Vec<String> = (0..8).map(|i| format!("arg{i}")).collect();
        let local_ids: Vec<String> = self.locals.values().cloned().collect();
        let mut used = HashSet::new();
        used.insert("thread".to_string());
        used.insert("pool".to_string());
        used.insert("sp".to_string());
        used.insert("null".to_string());
        used.insert("flags".to_string());
        used.insert("dynamic".to_string());

        let mut renames: HashMap<String, String> = HashMap::new();
        let mut arg_types: HashMap<String, String> = HashMap::new();
        let mut local_types: HashMap<String, String> = HashMap::new();

        for arg in &arg_ids {
            let stats = Self::collect_ident_stats(&self.lines, arg);
            let idx = arg.trim_start_matches("arg").parse::<usize>().unwrap_or(0);
            let base = if idx == 0 {
                "receiver".to_string()
            } else if stats.field_access >= 1 {
                format!("obj{idx}")
            } else if stats.arith_ops >= 2 && stats.field_access == 0 {
                format!("value{idx}")
            } else {
                format!("param{idx}")
            };
            let name = Self::unique_name(&base, &mut used);
            if name != *arg {
                renames.insert(arg.clone(), name);
            }
            let ty = if stats.arith_ops >= 2 && stats.field_access == 0 {
                "int"
            } else {
                "dynamic"
            };
            arg_types.insert(arg.clone(), ty.to_string());
        }

        let mut pool_i = 1usize;
        let mut obj_i = 1usize;
        let mut int_i = 1usize;
        let mut tmp_i = 1usize;
        for local in &local_ids {
            let stats = Self::collect_ident_stats(&self.lines, local);
            let base = if stats.pool_assign > 0 {
                let n = pool_i;
                pool_i += 1;
                format!("poolVal{n}")
            } else if stats.field_access >= 2 {
                let n = obj_i;
                obj_i += 1;
                format!("objTmp{n}")
            } else if stats.arith_ops >= 2 && stats.field_access == 0 {
                let n = int_i;
                int_i += 1;
                format!("intTmp{n}")
            } else if stats.call_assign > 0 {
                let n = tmp_i;
                tmp_i += 1;
                format!("resultTmp{n}")
            } else {
                let n = tmp_i;
                tmp_i += 1;
                format!("tmp{n}")
            };
            let name = Self::unique_name(&base, &mut used);
            if name != *local {
                renames.insert(local.clone(), name);
            }
            let ty = if stats.arith_ops >= 2 && stats.field_access == 0 {
                "int"
            } else {
                "dynamic"
            };
            local_types.insert(local.clone(), ty.to_string());
        }

        let mut rename_pairs: Vec<(String, String)> = renames.into_iter().collect();
        rename_pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for line in &mut self.lines {
            let mut cur = line.clone();
            for (from, to) in &rename_pairs {
                cur = Self::replace_identifier_token(&cur, from, to);
            }
            *line = cur;
        }

        let args_sig = arg_ids
            .iter()
            .map(|arg| {
                let name = rename_pairs
                    .iter()
                    .find_map(|(from, to)| if from == arg { Some(to.clone()) } else { None })
                    .unwrap_or_else(|| arg.clone());
                let ty = arg_types
                    .get(arg)
                    .cloned()
                    .unwrap_or_else(|| "dynamic".to_string());
                format!("{ty} {name}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.lines[0] = format!("dynamic {}({}) {{", fn_name, args_sig);

        let mut local_type_by_name: HashMap<String, String> = HashMap::new();
        for local in &local_ids {
            let name = rename_pairs
                .iter()
                .find_map(|(from, to)| {
                    if from == local {
                        Some(to.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| local.clone());
            let ty = local_types
                .get(local)
                .cloned()
                .unwrap_or_else(|| "dynamic".to_string());
            local_type_by_name.insert(name, ty);
        }

        for line in &mut self.lines {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("var ") {
                if let Some(name) = rest.strip_suffix(';') {
                    if let Some(ty) = local_type_by_name.get(name.trim()) {
                        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
                        *line = format!("{}{} {};", " ".repeat(indent), ty, name.trim());
                    }
                }
            }
        }

        for line in &mut self.lines {
            let mut cur = line.clone();
            for n in 0..=30 {
                let from = format!("x{n}");
                let to = named_register_alias(n);
                cur = Self::replace_identifier_token(&cur, &from, &to);
            }
            *line = cur;
        }
    }

    pub(super) fn field_expr(base: &str, off: i64) -> String {
        let b = if base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            base.to_string()
        } else {
            format!("({base})")
        };

        if b == "sp" || b == "stack" {
            return format!("{b}[{}]", fmt_int(off));
        }

        if off == -1 {
            format!("{b}._tag")
        } else if off >= 0 {
            format!("{b}.f{off}")
        } else {
            format!("{b}.m{}", -off)
        }
    }

    pub(super) fn rewrite_bitfield_classid(input: &str) -> String {
        let mut out = String::new();
        let bytes = input.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            if input[i..].starts_with("bitField(") {
                let start = i + "bitField(".len();
                let mut j = start;
                let mut depth = 1i32;
                while j < bytes.len() {
                    let c = bytes[j] as char;
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < bytes.len() {
                    let inside = input[start..j].trim();
                    if let Some(prefix) = inside.strip_suffix(", 0xc, 0x14") {
                        let base = prefix.trim().strip_suffix("._tag").unwrap_or(prefix.trim());
                        out.push_str(&format!("classId({})", base));
                        i = j + 1;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    pub(super) fn rewrite_negated_comparisons(input: &str) -> String {
        let mut out = String::new();
        let bytes = input.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            if input[i..].starts_with("!((") {
                let mut depth = 0i32;
                let mut end = None;
                let mut j = i + 1;
                while j < bytes.len() {
                    let c = bytes[j] as char;
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(j);
                            break;
                        }
                    }
                    j += 1;
                }

                if let Some(end_idx) = end {
                    let wrapped = &input[i + 1..=end_idx];
                    if let Some(inner) = wrapped.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
                    {
                        if let Some((lhs, rhs)) = inner.split_once(" != ") {
                            out.push('(');
                            out.push_str(lhs.trim());
                            out.push_str(" == ");
                            out.push_str(rhs.trim());
                            out.push(')');
                            i = end_idx + 1;
                            continue;
                        }
                        if let Some((lhs, rhs)) = inner.split_once(" == ") {
                            out.push('(');
                            out.push_str(lhs.trim());
                            out.push_str(" != ");
                            out.push_str(rhs.trim());
                            out.push(')');
                            i = end_idx + 1;
                            continue;
                        }
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }

        out
    }

    pub(super) fn strip_outer_parens_once(expr: &str) -> Option<&str> {
        let t = expr.trim();
        if t.len() < 2 || !t.starts_with('(') || !t.ends_with(')') {
            return None;
        }
        let mut depth = 0i32;
        for (idx, c) in t.char_indices() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 && idx + c.len_utf8() != t.len() {
                    return None;
                }
            }
            if depth < 0 {
                return None;
            }
        }
        if depth != 0 {
            return None;
        }
        Some(&t[1..t.len() - 1])
    }

    pub(super) fn simplify_wrapped_if_condition(line: &str) -> String {
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let t = line.trim();
        let Some(cond) = t.strip_prefix("if (").and_then(|s| s.strip_suffix(") {")) else {
            return line.to_string();
        };

        let mut cur = cond.trim().to_string();
        while let Some(inner) = Self::strip_outer_parens_once(&cur) {
            cur = inner.trim().to_string();
        }
        format!("{}if ({}) {{", " ".repeat(indent), cur)
    }

    pub(super) fn clean_expr(expr: String) -> String {
        let mut s = expr;
        s = s.replace(" + x28 /* lsl #32 */", "");
        s = s.replace(" + x28", "");
        s = Self::rewrite_negated_comparisons(&s);
        s = Self::rewrite_bitfield_classid(&s);
        s = Self::simplify_wrapped_if_condition(&s);
        s
    }
}
