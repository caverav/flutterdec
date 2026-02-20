impl<'a> FuncEmitter<'a> {
    pub(super) fn is_terminal_statement(line_trim: &str) -> bool {
        line_trim == "continue;"
            || line_trim == "break;"
            || (line_trim.starts_with("return ") && line_trim.ends_with(';'))
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

}
