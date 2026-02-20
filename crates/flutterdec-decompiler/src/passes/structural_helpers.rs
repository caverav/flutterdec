impl<'a> FuncEmitter<'a> {
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

}
