impl<'a> FuncEmitter<'a> {
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
        if ident.is_empty() {
            return false;
        }
        let mut i = 0usize;
        let bytes = t.as_bytes();
        let ident_bytes = ident.as_bytes();
        while i + ident_bytes.len() <= bytes.len() {
            if bytes[i..].starts_with(ident_bytes) {
                let prev_ok = if i == 0 {
                    true
                } else {
                    !Self::is_ident_char(bytes[i - 1] as char)
                };
                let next_i = i + ident_bytes.len();
                let next_ok = if next_i >= bytes.len() {
                    true
                } else {
                    !Self::is_ident_char(bytes[next_i] as char)
                };
                if prev_ok && next_ok {
                    if let Ok(rest) = std::str::from_utf8(&bytes[next_i..]) {
                        let rest = rest.trim_start();
                        if rest.starts_with('=') && !rest.starts_with("==") {
                            return true;
                        }
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

}

#[cfg(test)]
mod guard_and_flow_utf8_tests {
    use super::*;

    #[test]
    fn assigns_ident_handles_utf8_text() {
        let line = r#"final t5 = call(local, "Možete", local2);"#;
        assert!(!FuncEmitter::assigns_ident(line, "local"));
        assert!(FuncEmitter::assigns_ident("local = 1;", "local"));
    }
}
