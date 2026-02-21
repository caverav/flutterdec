impl<'a> FuncEmitter<'a> {
    fn normalize_stack_slot_expr(expr: &str) -> Option<String> {
        let trimmed = expr.trim();
        let (base, off) = parse_stack_base_offset(trimmed)?;
        if off == 0 && trimmed == base {
            return None;
        }
        Some(format!("{base}[{}]", fmt_int(off)))
    }

    pub(super) fn field_expr(base: &str, off: i64) -> String {
        if let Some((stack_base, stack_off)) = parse_stack_base_offset(base) {
            return format!("{stack_base}[{}]", fmt_int(stack_off + off));
        }

        let b = if base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            base.to_string()
        } else {
            format!("({base})")
        };

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
        if let Some(stack_slot) = Self::normalize_stack_slot_expr(&s) {
            s = stack_slot;
        }
        s
    }
}
