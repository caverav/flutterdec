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

    /// Whether the parenthesis at `i` opens an argument list rather than a
    /// grouping.
    ///
    /// `f(a).b` has the same shape as `(a).b` to a scanner that only looks
    /// forward, and unwrapping it produced `fa.b`: the call is gone and an
    /// identifier the body never had is in its place. What separates the two is
    /// the byte before the parenthesis - a name, or the end of one.
    fn opens_an_argument_list(bytes: &[u8], i: usize) -> bool {
        let Some(previous) = i.checked_sub(1).and_then(|prev| bytes.get(prev)) else {
            return false;
        };
        Self::is_ident_char(*previous as char) || *previous == b')' || *previous == b']'
    }

    fn simplify_parenthesized_member_access_once(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'(' && !Self::opens_an_argument_list(bytes, i) {
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

    /// The single top-level equality in `inner`, as its byte offset and the
    /// operator that negates it.
    ///
    /// Top level, because splitting on the first ` != ` in the text took one out
    /// of a nested operand: `!((x != y) != (z != w))` became
    /// `((x == y) != (z != w))`, which negates the wrong comparison and is a
    /// different value, not a different spelling.
    ///
    /// Exactly one, with no top-level `&&`, `||` or conditional, because swapping
    /// the operator is only the negation when the comparison is the whole
    /// expression. `!(x || y != z)` is not `(x || y == z)`: the negation
    /// distributes over the looser operator, so there is nothing to rewrite and
    /// the explicit `!` stays.
    fn top_level_equality(inner: &str) -> Option<(usize, &'static str)> {
        let bytes = inner.as_bytes();
        let mut depth = 0i32;
        let mut found: Option<(usize, &'static str)> = None;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                _ if depth == 0 => {
                    if bytes[i..].starts_with(b" != ") || bytes[i..].starts_with(b" == ") {
                        if found.is_some() {
                            return None;
                        }
                        let negated = if bytes[i + 1] == b'!' { " == " } else { " != " };
                        found = Some((i, negated));
                        i += 4;
                        continue;
                    }
                    if bytes[i..].starts_with(b"&&") || bytes[i..].starts_with(b"||") {
                        return None;
                    }
                    if bytes[i] == b'?' {
                        return None;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        found
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
                            // `!((a != b))` wraps its comparison twice, so the
                            // redundant layers come off before asking what the
                            // top level holds.
                            let mut inner = inner;
                            while let Some(stripped) = Self::strip_outer_parens_once(inner) {
                                inner = stripped;
                            }
                            if let Some((at, negated)) = Self::top_level_equality(inner) {
                                out.push(b'(');
                                out.extend_from_slice(inner[..at].trim().as_bytes());
                                out.extend_from_slice(negated.as_bytes());
                                out.extend_from_slice(inner[at + 4..].trim().as_bytes());
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

    /// Text with every `needle` removed, unless the byte after a match continues
    /// an identifier.
    ///
    /// ` + x28` must not eat the head of ` + x280`, and a plain `replace` has no
    /// notion of where a token ends.
    fn strip_token(text: &str, needle: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(needle) {
            let after = rest[at + needle.len()..].chars().next();
            out.push_str(&rest[..at]);
            if after.is_some_and(Self::is_ident_char) {
                out.push_str(needle);
            }
            rest = &rest[at + needle.len()..];
        }
        out.push_str(rest);
        out
    }

    pub(super) fn clean_expr(expr: String) -> String {
        // The compressed-pointer strip is the one pattern that deliberately spans
        // a comment the emitter rendered itself, so it runs where a comment is
        // ordinary text. A string literal is still off limits: a recovered string
        // reading `... + x28 ...` is program data.
        let mut s = rewrite_outside_string_literals(&expr, |code| {
            let stripped = Self::strip_token(code, " + x28 /* lsl #32 */");
            Self::strip_token(&stripped, " + x28")
        });
        // The scanning rewrites see code only. Each one looks for punctuation and
        // call shapes that occur in recovered strings and in the emitter's own
        // comments as often as in code, and each is byte-preserving, so a pattern
        // that straddles a boundary simply stops matching.
        s = rewrite_code_spans(&s, |code| {
            let mut c = Self::rewrite_negated_comparisons(code);
            c = Self::rewrite_bitfield_classid(&c);
            c = normalize_pool_page_field_exprs(&c);
            Self::simplify_wrapped_member_accesses(&c)
        });
        // Whole-expression forms, matched by their exact shape rather than
        // scanned for: an `if` line whose condition is wrapped, and a bare stack
        // slot. Neither can fire on a fragment of a line that carries a literal
        // or a comment, because neither shape survives the extra text.
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

/// The boundaries `clean_expr` may not cross, and the grouping it may not change.
///
/// These go straight at `clean_expr` because the shapes are not all reachable from
/// a synthetic instruction stream: a line comment quoting an expression and a
/// negation over two nested comparisons both come out of real bodies, and neither
/// can be planted through the IR.
#[cfg(test)]
mod rewrite_boundary_tests {
    use super::*;

    /// A comment is the emitter's own statement about the line, so a cleanup
    /// rewrite may read it but never edit it.
    #[test]
    fn cleanup_rewrites_code_and_leaves_comments_alone() {
        let cases = [
            // line comment
            (
                "  x = ((a)).b; // ((c)).d",
                "  x = a.b; // ((c)).d",
            ),
            // block comment
            (
                "  x = ((a)).b /* ((c)).d */;",
                "  x = a.b /* ((c)).d */;",
            ),
            // the class-id rewrite, in code and quoted in a comment
            (
                "  y = bitField(obj._tag, 0xc, 0x14); // bitField(a, 0xc, 0x14)",
                "  y = classId(obj); // bitField(a, 0xc, 0x14)",
            ),
            // a negated comparison quoted in a comment
            // The condition unwrapper needs the line to end with `) {`, so a
            // trailing comment leaves the redundant parentheses in place. That
            // is the conservative direction and the negation is still correct.
            (
                "  if (!((a != b))) { // !((c != d))",
                "  if ((a == b)) { // !((c != d))",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                FuncEmitter::clean_expr(input.to_string()),
                expected,
                "cleanup crossed a comment boundary: {input}"
            );
        }
    }

    /// A recovered string is program data, escapes included: a `\"` inside it does
    /// not end the literal, so a scanner that mishandles it edits the tail of the
    /// string as if it were code.
    #[test]
    fn cleanup_leaves_string_literals_alone_including_escaped_quotes() {
        for input in [
            r#"  x = "a \" ((y)).z";"#,
            r#"  x = "bitField(a, 0xc, 0x14)";"#,
            r#"  x = "!((p != q))";"#,
            r#"  x = "p + x28";"#,
            r#"  x = "sp[0x10] (value3 - 1)";"#,
        ] {
            assert_eq!(
                FuncEmitter::clean_expr(input.to_string()),
                input,
                "cleanup edited a recovered string: {input}"
            );
        }
    }

    /// The compressed-pointer strip removes a whole token, and the modifier
    /// comment is part of its pattern because the emitter rendered it.
    #[test]
    fn the_compressed_pointer_strip_stops_at_the_token_boundary() {
        assert_eq!(FuncEmitter::clean_expr("(p + x28)".to_string()), "(p)");
        assert_eq!(
            FuncEmitter::clean_expr("(p + x28 /* lsl #32 */)".to_string()),
            "(p)"
        );
        // Not the register: a longer name only starts with it.
        assert_eq!(
            FuncEmitter::clean_expr("(p + x281)".to_string()),
            "(p + x281)"
        );
        // Inside a recovered string it is text, not an addition.
        assert_eq!(
            FuncEmitter::clean_expr(r#"  x = "p + x28";"#.to_string()),
            r#"  x = "p + x28";"#
        );
    }

    /// Negating a comparison is swapping its operator only when the comparison is
    /// the whole expression. The nested case used to split on the first ` != ` in
    /// the text, which sat inside the left operand, so the wrong comparison was
    /// negated and the value changed.
    #[test]
    fn negation_rewrites_the_top_level_comparison_or_nothing() {
        let cases = [
            // An `if` line has its redundant condition parentheses stripped by
            // the same pass, so the negation shows up unwrapped here.
            ("if (!((a != b))) {", "if (a == b) {"),
            ("if (!((a == b))) {", "if (a != b) {"),
            // Two comparisons, one negation: only the outer one is negated.
            (
                "x = !((a != b) != (c != d));",
                "x = ((a != b) == (c != d));",
            ),
            (
                "x = !((a == b) == (c == d));",
                "x = ((a == b) != (c == d));",
            ),
            // A looser operator at the top level: the negation does not
            // distribute into one comparison, so the explicit `!` stays.
            (
                "x = !((a || b != c));",
                "x = !((a || b != c));",
            ),
            (
                "x = !((a && b == c));",
                "x = !((a && b == c));",
            ),
            // A conditional value is not a comparison either.
            (
                "x = !((a ? b : c != d));",
                "x = !((a ? b : c != d));",
            ),
            // Shifted operands keep their own parentheses on both sides.
            (
                "x = !(((a >> 1) != (b >>> 2)));",
                "x = ((a >> 1) == (b >>> 2));",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                FuncEmitter::clean_expr(input.to_string()),
                expected,
                "wrong negation for: {input}"
            );
        }
    }

    /// A member access is only unwrapped when the parentheses are redundant, so
    /// an operator inside them keeps them.
    #[test]
    fn required_parentheses_are_kept() {
        for (input, expected) in [
            ("((arg0 + 1)).f7", "((arg0 + 1)).f7"),
            ("((obj.f15)).f7", "obj.f15.f7"),
            // A call is not a grouping: its argument list keeps its parentheses.
            ("((f(a))).b", "f(a).b"),
            ("(smiUntag(x)).f8", "smiUntag(x).f8"),
            ("f(a).b", "f(a).b"),
            ("((a & 0xff)).f8", "((a & 0xff)).f8"),
        ] {
            assert_eq!(
                FuncEmitter::clean_expr(input.to_string()),
                expected,
                "wrong parenthesisation for: {input}"
            );
        }
    }
}
