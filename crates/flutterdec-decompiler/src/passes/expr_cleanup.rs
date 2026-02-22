impl<'a> FuncEmitter<'a> {
    fn is_simple_member_expr(expr: &str) -> bool {
        let t = expr.trim();
        !t.is_empty()
            && t.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || c == '_'
                    || c == '$'
                    || c == '.'
                    || c == '['
                    || c == ']'
                    || c == '('
                    || c == ')'
            })
    }

    fn simplify_wrapped_member_access_once(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if i + 3 < bytes.len() && bytes[i] == b'(' && bytes[i + 1] == b'(' {
                let mut depth = 0i32;
                let mut j = i;
                while j < bytes.len() {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < bytes.len()
                    && j + 1 < bytes.len()
                    && bytes[j + 1] == b'.'
                    && j >= i + 3
                    && bytes[j - 1] == b')'
                {
                    if let Ok(inner_raw) = std::str::from_utf8(&bytes[i + 2..j - 1]) {
                        let inner = inner_raw.trim();
                        if Self::is_simple_member_expr(inner) {
                            out.push(b'(');
                            out.extend_from_slice(inner.as_bytes());
                            out.push(b')');
                            i = j + 1;
                            continue;
                        }
                    }
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).unwrap_or_else(|_| input.to_string())
    }

    fn simplify_parenthesized_member_access_once(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'(' {
                let mut depth = 0i32;
                let mut j = i;
                while j < bytes.len() {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < bytes.len() && j + 1 < bytes.len() && bytes[j + 1] == b'.' {
                    if let Ok(inner_raw) = std::str::from_utf8(&bytes[i + 1..j]) {
                        let inner = inner_raw.trim();
                        if Self::is_simple_member_expr(inner) {
                            out.extend_from_slice(inner.as_bytes());
                            i = j + 1;
                            continue;
                        }
                    }
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).unwrap_or_else(|_| input.to_string())
    }

    fn simplify_wrapped_member_accesses(input: &str) -> String {
        let mut cur = input.to_string();
        for _ in 0..8 {
            let next = Self::simplify_parenthesized_member_access_once(
                &Self::simplify_wrapped_member_access_once(&cur),
            );
            if next == cur {
                break;
            }
            cur = next;
        }
        cur
    }

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
        let bytes = input.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;

        while i < bytes.len() {
            if bytes[i..].starts_with(b"bitField(") {
                let start = i + "bitField(".len();
                let mut j = start;
                let mut depth = 1i32;
                while j < bytes.len() {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < bytes.len() {
                    if let Ok(inside_raw) = std::str::from_utf8(&bytes[start..j]) {
                        let inside = inside_raw.trim();
                        if let Some(prefix) = inside.strip_suffix(", 0xc, 0x14") {
                            let base = prefix.trim().strip_suffix("._tag").unwrap_or(prefix.trim());
                            out.extend_from_slice(format!("classId({})", base).as_bytes());
                            i = j + 1;
                            continue;
                        }
                    }
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).unwrap_or_else(|_| input.to_string())
    }

    pub(super) fn rewrite_negated_comparisons(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;

        while i < bytes.len() {
            if bytes[i..].starts_with(b"!((") {
                let mut depth = 0i32;
                let mut end = None;
                let mut j = i + 1;
                while j < bytes.len() {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(j);
                            break;
                        }
                    }
                    j += 1;
                }

                if let Some(end_idx) = end {
                    if let Ok(wrapped) = std::str::from_utf8(&bytes[i + 1..=end_idx]) {
                        if let Some(inner) =
                            wrapped.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
                        {
                            if let Some((lhs, rhs)) = inner.split_once(" != ") {
                                out.push(b'(');
                                out.extend_from_slice(lhs.trim().as_bytes());
                                out.extend_from_slice(b" == ");
                                out.extend_from_slice(rhs.trim().as_bytes());
                                out.push(b')');
                                i = end_idx + 1;
                                continue;
                            }
                            if let Some((lhs, rhs)) = inner.split_once(" == ") {
                                out.push(b'(');
                                out.extend_from_slice(lhs.trim().as_bytes());
                                out.extend_from_slice(b" != ");
                                out.extend_from_slice(rhs.trim().as_bytes());
                                out.push(b')');
                                i = end_idx + 1;
                                continue;
                            }
                        }
                    }
                }
            }
            out.push(bytes[i]);
            i += 1;
        }

        String::from_utf8(out).unwrap_or_else(|_| input.to_string())
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
        s = Self::simplify_wrapped_member_accesses(&s);
        s = Self::simplify_wrapped_if_condition(&s);
        if let Some(stack_slot) = Self::normalize_stack_slot_expr(&s) {
            s = stack_slot;
        }
        s
    }
}
