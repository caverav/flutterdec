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

    /// Length of the string literal starting at `i`, including both quotes.
    ///
    /// Recovered pool strings are real program data and frequently contain the same
    /// punctuation these rewrites look for. `"... collected (nullptr). This is ..."` reads
    /// as a parenthesised member access to a byte scanner, and simplifying it silently
    /// edits a string that came out of the binary. Scanners copy literals verbatim.
    fn string_literal_len(bytes: &[u8], i: usize) -> Option<usize> {
        if bytes.get(i) != Some(&b'"') {
            return None;
        }
        let mut j = i + 1;
        while j < bytes.len() {
            match bytes[j] {
                b'\\' => j += 2,
                b'"' => return Some(j + 1 - i),
                _ => j += 1,
            }
        }
        None
    }

    fn simplify_wrapped_member_access_once(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(len) = Self::string_literal_len(bytes, i) {
                out.extend_from_slice(&bytes[i..i + len]);
                i += len;
                continue;
            }
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
            if let Some(len) = Self::string_literal_len(bytes, i) {
                out.extend_from_slice(&bytes[i..i + len]);
                i += len;
                continue;
            }
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
            return format!("{b}._tag");
        }
        if off < 0 {
            return format!("{b}.m{}", -off);
        }
        // Dart object pointers carry `kHeapObjectTag`, so a field load reads
        // `[obj + offset - 1]`. Field offsets are 4-aligned, so a displacement of
        // 3 mod 4 is exactly a tag-adjusted one and identifies itself: no
        // knowledge of the base is needed, and THR or pool displacements, which
        // are aligned and untagged, never match. Measured on a real binary,
        // 262439 of 272805 object-base displacements are 3 or 7 mod 8, and every
        // THR displacement is 0 mod 8.
        //
        // Reporting the real offset makes the number readable without knowing the
        // tagging scheme, and joinable by equality to a recovered class field
        // table, which is keyed the same way.
        if off % 4 == 3 {
            format!("{b}.f{}", off + 1)
        } else {
            format!("{b}.f{off}")
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
        s = normalize_pool_page_field_exprs(&s);
        s = Self::simplify_wrapped_member_accesses(&s);
        s = Self::simplify_wrapped_if_condition(&s);
        if let Some(stack_slot) = Self::normalize_stack_slot_expr(&s) {
            s = stack_slot;
        }
        s
    }
}

#[cfg(test)]
mod expr_cleanup_utf8_tests {
    use super::*;

    #[test]
    fn rewrite_negated_comparisons_handles_utf8_text() {
        let input = r#"final s = "Možete"; if (!((a != b))) { return s; }"#;
        let out = FuncEmitter::rewrite_negated_comparisons(input);
        assert!(out.contains(r#""Možete""#));
        assert!(out.contains("(a == b)"));
    }

    #[test]
    fn rewrite_bitfield_classid_handles_utf8_text() {
        let input = r#"final s = "pronaći"; final x = bitField(obj._tag, 0xc, 0x14);"#;
        let out = FuncEmitter::rewrite_bitfield_classid(input);
        assert!(out.contains(r#""pronaći""#));
        assert!(out.contains("classId(obj)"));
    }

    #[test]
    fn clean_expr_normalizes_shifted_pool_field_access() {
        // The shift folds in the simplifier now, so this is the shape the emitter
        // produces: `add rD, pool, #0x8, lsl #12` reaches here already added.
        let input = "((pool + 0x8000)).f3640".to_string();
        let out = FuncEmitter::clean_expr(input);
        // (8 << 12) + 3640 == 36408 bytes from PP. Converting that to an entry index
        // needs the pool's entries_offset/word_size, which this layer does not have.
        assert_eq!(out, "poolOff[36408]");
    }

    #[test]
    fn clean_expr_normalizes_nested_shifted_pool_field_access() {
        let input = "((((pool + 0x8000)).f816).f7)".to_string();
        let out = FuncEmitter::clean_expr(input);
        assert_eq!(out, "(poolOff[33584].f7)");
    }

    /// A Dart object pointer carries `kHeapObjectTag`, so a field load reads one
    /// byte below the field. Field offsets are 4-aligned, so a displacement of 3
    /// mod 4 is exactly a tag-adjusted one and identifies itself; everything else
    /// is already in the untagged space and must not shift.
    #[test]
    fn field_offsets_are_reported_untagged() {
        // Tagged: the load was `[obj + 0x10 - 1]`.
        assert_eq!(FuncEmitter::field_expr("obj", 15), "obj.f16");
        assert_eq!(FuncEmitter::field_expr("obj", 7), "obj.f8");
        // Compressed slots are 4 bytes, so 3 mod 4 also covers `[obj + 4 - 1]`.
        assert_eq!(FuncEmitter::field_expr("obj", 3), "obj.f4");
        // Already aligned, so untagged: an object pool or THR displacement.
        assert_eq!(FuncEmitter::field_expr("obj", 16), "obj.f16");
        assert_eq!(FuncEmitter::field_expr("thread", 72), "thread.f72");
        assert_eq!(FuncEmitter::field_expr("thread", 0x38), "thread.f56");
        // The header sits below the tag and is named, not numbered.
        assert_eq!(FuncEmitter::field_expr("obj", -1), "obj._tag");
        assert_eq!(FuncEmitter::field_expr("obj", -8), "obj.m8");
    }
}
