/// Length of the string literal or comment starting at `i`, if one starts there.
///
/// A recovered pool string is program data: it routinely contains the same
/// punctuation, identifiers and operators the rewrites look for, and editing it
/// silently changes what the binary said. A comment is the emitter's own
/// statement about a line, and rewriting inside one edits a claim rather than
/// code. An unterminated span runs to the end of the text, so a stray quote or
/// `/*` protects the rest of the line instead of exposing it.
fn non_code_span_len(bytes: &[u8], i: usize, comments_too: bool) -> Option<usize> {
    match bytes.get(i)? {
        b'"' => Some(string_literal_len(bytes, i)),
        b'/' if comments_too && bytes.get(i + 1) == Some(&b'*') => {
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                // A recovered value is rendered inside a comment as a quoted
                // literal (`pool[7 /* "..." */]`), so its bytes are the one
                // place a `*/` can appear that the emitter did not write.
                // Skipping the literal keeps the comment ending where the
                // emitter ended it, instead of letting recovered data spill the
                // rest of the annotation onto the line as code.
                if bytes[j] == b'"' {
                    j += string_literal_len(bytes, j);
                    continue;
                }
                if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                    return Some(j + 2 - i);
                }
                j += 1;
            }
            Some(bytes.len() - i)
        }
        // Lines are rewritten one at a time, so a line comment runs to the end.
        b'/' if comments_too && bytes.get(i + 1) == Some(&b'/') => Some(bytes.len() - i),
        _ => None,
    }
}

/// Length of the string literal starting at `i`, escapes honoured. An
/// unterminated one runs to the end of the text.
fn string_literal_len(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b'"' => return (j + 1).min(bytes.len()) - i,
            _ => j += 1,
        }
    }
    bytes.len() - i
}

fn rewrite_spans(text: &str, comments_too: bool, rewrite: impl Fn(&str) -> String) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut code_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(span) = non_code_span_len(bytes, i, comments_too) else {
            i += 1;
            continue;
        };
        out.push_str(&rewrite(&text[code_start..i]));
        let end = (i + span).min(bytes.len());
        out.push_str(&text[i..end]);
        code_start = end;
        i = end;
    }
    out.push_str(&rewrite(&text[code_start..]));
    out
}

/// Apply `rewrite` to the code of `text`, copying every string literal and every
/// comment byte for byte.
///
/// A rewrite runs per code span rather than once over the whole text, so a
/// pattern that straddles a literal or a comment simply does not match. That is
/// the conservative direction: the text is left as the emitter wrote it.
pub(super) fn rewrite_code_spans(text: &str, rewrite: impl Fn(&str) -> String) -> String {
    rewrite_spans(text, true, rewrite)
}

/// As `rewrite_code_spans`, but comments are ordinary text.
///
/// For the rewrites whose pattern deliberately includes a comment the emitter
/// itself rendered, where splitting at the comment would stop them matching at
/// all.
pub(super) fn rewrite_outside_string_literals(
    text: &str,
    rewrite: impl Fn(&str) -> String,
) -> String {
    rewrite_spans(text, false, rewrite)
}

/// Pass each run of code in `text` to `f`, skipping every string literal and
/// every comment.
///
/// The read-only half of `rewrite_code_spans`, for the analyses that read
/// structure instead of editing it. A recovered pool string is program data and
/// a comment is the emitter's own prose, so a brace in either closes no block
/// and an identifier in either names no helper. Runs per code span rather than
/// once over the whole text, so a token straddling a literal does not match.
fn for_each_code_span_at(text: &str, mut f: impl FnMut(usize, &str)) {
    let bytes = text.as_bytes();
    let mut code_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(span) = non_code_span_len(bytes, i, true) else {
            i += 1;
            continue;
        };
        f(code_start, &text[code_start..i]);
        let end = (i + span).min(bytes.len());
        code_start = end;
        i = end;
    }
    f(code_start, &text[code_start..]);
}

pub(super) fn for_each_code_span(text: &str, mut f: impl FnMut(&str)) {
    for_each_code_span_at(text, |_, code| f(code));
}

/// Find `needle` only where it is wholly contained in emitter-owned code.
pub(super) fn find_in_code(text: &str, needle: &str) -> Option<usize> {
    let mut found = None;
    for_each_code_span_at(text, |offset, code| {
        if found.is_none() {
            found = code.find(needle).map(|index| offset + index);
        }
    });
    found
}

/// The rest of the code span containing `start`, or `None` for recovered data.
pub(super) fn code_span_from(text: &str, start: usize) -> Option<&str> {
    let mut span = None;
    for_each_code_span_at(text, |offset, code| {
        let end = offset + code.len();
        if span.is_none() && start >= offset && start <= end {
            span = text.get(start..end);
        }
    });
    span
}

/// Start of the first line comment outside literals and block comments.
pub(super) fn line_comment_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'/') {
            return Some(i);
        }
        if let Some(span) = non_code_span_len(bytes, i, true) {
            i += span;
        } else {
            i += 1;
        }
    }
    None
}

/// How many `{` and how many `}` the code of `text` carries.
pub(super) fn code_brace_counts(text: &str) -> (i32, i32) {
    let (mut opens, mut closes) = (0i32, 0i32);
    for_each_code_span(text, |code| {
        for byte in code.bytes() {
            match byte {
                b'{' => opens += 1,
                b'}' => closes += 1,
                _ => {}
            }
        }
    });
    (opens, closes)
}

/// How much the code of `text` opens minus how much it closes.
pub(super) fn code_brace_delta(text: &str) -> i32 {
    let (opens, closes) = code_brace_counts(text);
    opens - closes
}

/// Whether `needle` occurs in the code of `text`.
pub(super) fn code_contains(text: &str, needle: &str) -> bool {
    let mut found = false;
    for_each_code_span(text, |code| found |= code.contains(needle));
    found
}

pub(super) fn parse_int(token: &str) -> Option<i64> {
    let t = token.trim().trim_start_matches('#');
    if let Some(hex) = t.strip_prefix("-0x") {
        return i64::from_str_radix(hex, 16).ok().map(|v| -v);
    }
    if let Some(hex) = t.strip_prefix("0x") {
        return i64::from_str_radix(hex, 16).ok();
    }
    t.parse::<i64>().ok()
}

pub(super) fn parse_expr_int(expr: &str) -> Option<i64> {
    let t = expr.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        return parse_expr_int(inner);
    }
    parse_int(t)
}

pub(super) fn fmt_int(v: i64) -> String {
    if v < 0 {
        // `-v` overflows for `i64::MIN`, which is reachable: a `movk` into the
        // top lane produces exactly that value, and a panic in a formatter is a
        // worse failure than a wide literal.
        let mag = v.unsigned_abs();
        if mag >= 10 {
            format!("-0x{mag:x}")
        } else {
            format!("-{mag}")
        }
    } else if v >= 10 {
        format!("0x{v:x}")
    } else {
        v.to_string()
    }
}

pub(super) fn parse_base_offset_expr(expr: &str) -> Option<(String, i64)> {
    let mut t = expr.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        t = inner.trim();
    }

    let bytes = t.as_bytes();
    let mut depth = 0i32;
    let mut op_idx = None;
    let mut op_ch = '+';
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        let c = bytes[i] as char;
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
        } else if depth == 0
            && bytes[i] == b' '
            && (bytes[i + 1] == b'+' || bytes[i + 1] == b'-')
            && bytes[i + 2] == b' '
        {
            op_idx = Some(i);
            op_ch = bytes[i + 1] as char;
            break;
        }
        i += 1;
    }

    let idx = op_idx?;
    let lhs = t[..idx].trim();
    let rhs = t[idx + 3..].trim();
    if lhs.is_empty() {
        return None;
    }
    let off = parse_expr_int(rhs)?;
    let signed_off = if op_ch == '-' { -off } else { off };
    Some((lhs.to_string(), signed_off))
}

pub(super) fn parse_stack_base_offset(expr: &str) -> Option<(String, i64)> {
    let t = expr.trim();
    if t.is_empty() {
        return None;
    }
    if t == "sp" || t == "stack" {
        return Some((t.to_string(), 0));
    }
    if let Some(inner) = t.strip_suffix(']') {
        if let Some((base, index)) = inner.split_once('[') {
            let base = base.trim();
            if (base == "sp" || base == "stack") && !index.trim().is_empty() {
                if let Some(off) = parse_expr_int(index) {
                    return Some((base.to_string(), off));
                }
            }
        }
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if let Some((base, off)) = parse_stack_base_offset(inner) {
            return Some((base, off));
        }
    }
    if let Some((base, off)) = parse_base_offset_expr(t) {
        if let Some((stack_base, stack_off)) = parse_stack_base_offset(&base) {
            return Some((stack_base, stack_off + off));
        }
    }
    None
}

pub(super) fn simplify_bin_expr(lhs: String, op: &str, rhs: String) -> String {
    let lt = lhs.trim();
    let rt = rhs.trim();
    let l_int = parse_expr_int(lt);
    let r_int = parse_expr_int(rt);

    match op {
        "+" => {
            if lt == "null" {
                if let Some(v) = r_int {
                    return fmt_int(v);
                }
            }
            if rt == "null" {
                if let Some(v) = l_int {
                    return fmt_int(v);
                }
            }
            if l_int == Some(0) {
                return rt.to_string();
            }
            if r_int == Some(0) {
                return lt.to_string();
            }
            // ARM64 arithmetic wraps, so folding wraps too. Plain `+` panics in
            // debug on overflow, which `movk` made reachable by binding full
            // 64-bit constants, and a checked fold that bailed would silently
            // drop a constant that is genuinely known.
            if let (Some(a), Some(b)) = (l_int, r_int) {
                return fmt_int(a.wrapping_add(b));
            }
            if let (Some((stack_base, stack_off)), Some(delta)) =
                (parse_stack_base_offset(lt), r_int)
            {
                return format!("{stack_base}[{}]", fmt_int(stack_off.wrapping_add(delta)));
            }
            if let (Some((base, off)), Some(delta)) = (parse_base_offset_expr(lt), r_int) {
                let sum = off.wrapping_add(delta);
                if sum == 0 {
                    return base;
                }
                if sum > 0 {
                    return format!("({base} + {})", fmt_int(sum));
                }
                return format!("({base} - {})", fmt_int(sum.wrapping_neg()));
            }
        }
        "-" => {
            if r_int == Some(0) {
                return lt.to_string();
            }
            if let (Some(a), Some(b)) = (l_int, r_int) {
                return fmt_int(a.wrapping_sub(b));
            }
            if let (Some((stack_base, stack_off)), Some(delta)) =
                (parse_stack_base_offset(lt), r_int)
            {
                return format!("{stack_base}[{}]", fmt_int(stack_off.wrapping_sub(delta)));
            }
            if let (Some((base, off)), Some(delta)) = (parse_base_offset_expr(lt), r_int) {
                let sum = off.wrapping_sub(delta);
                if sum == 0 {
                    return base;
                }
                if sum > 0 {
                    return format!("({base} + {})", fmt_int(sum));
                }
                return format!("({base} - {})", fmt_int(sum.wrapping_neg()));
            }
        }
        // A shift of a literal by a literal is a literal. Folding it here lets a
        // shifted-immediate address like `add rd, pool, #0x2c, lsl #12` become
        // `(pool + 0x2c000)` through the `+` arm above, which is the same shape
        // an unshifted pool address already takes. Without this the shifted form
        // needs its own recogniser downstream.
        "<<" => {
            if let (Some(a), Some(b)) = (l_int, r_int) {
                if let Some(v) = u32::try_from(b).ok().filter(|b| *b < 64).and_then(|b| {
                    // Shifting out of range is not a fold, it is a misparse.
                    a.checked_shl(b)
                }) {
                    return fmt_int(v);
                }
            }
        }
        _ => {}
    }

    format!("({lt} {op} {rt})")
}

fn skip_ascii_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn parse_non_negative_i64_token(token: &str) -> Option<u64> {
    let parsed = parse_int(token)?;
    (parsed >= 0).then_some(parsed as u64)
}

/// Recognise `((pool + <displacement>)).f<offset>` and return the PP-relative
/// byte displacement it reads.
///
/// This is the residual path for pool loads the disassembler's register tracker
/// could not follow. It has no pool geometry, so it can only report the
/// displacement, never an entry index.
///
/// A shifted-immediate page address used to reach here as
/// `((pool + <page> /* lsl #<shift> */)).f<offset>`, needing the shift parsed out
/// of a comment. `simplify_bin_expr` folds a literal shift now, so the page
/// arrives already added in and there is one shape rather than three.
fn try_parse_pool_page_field(bytes: &[u8], start: usize) -> Option<(usize, u64)> {
    let mut i = start;
    let mut opens = 0usize;
    while i < bytes.len() && bytes[i] == b'(' {
        opens += 1;
        i += 1;
    }
    if opens < 2 {
        return None;
    }
    i = skip_ascii_ws(bytes, i);
    if i + 4 > bytes.len() || &bytes[i..i + 4] != b"pool" {
        return None;
    }
    i += 4;
    i = skip_ascii_ws(bytes, i);
    if i >= bytes.len() || bytes[i] != b'+' {
        return None;
    }
    i += 1;
    i = skip_ascii_ws(bytes, i);

    let disp_start = i;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b')' {
        i += 1;
    }
    if i == disp_start {
        return None;
    }
    let displacement =
        parse_non_negative_i64_token(std::str::from_utf8(&bytes[disp_start..i]).ok()?)?;
    i = skip_ascii_ws(bytes, i);

    // Close every parenthesis this match opened, so replacing the span cannot
    // leave an unbalanced one behind.
    for _ in 0..opens {
        if i >= bytes.len() || bytes[i] != b')' {
            return None;
        }
        i += 1;
    }
    if i + 2 > bytes.len() || bytes[i] != b'.' || bytes[i + 1] != b'f' {
        return None;
    }
    i += 2;

    let off_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == off_start {
        return None;
    }
    let offset = std::str::from_utf8(&bytes[off_start..i])
        .ok()?
        .parse::<u64>()
        .ok()?;

    let total = displacement.checked_add(offset)?;
    if !total.is_multiple_of(8) {
        return None;
    }

    Some((i, total))
}

pub(super) fn normalize_pool_page_field_exprs(input: &str) -> String {
    let bytes = input.as_bytes();
    // Bytes, not chars: `bytes[i] as char` reinterprets each byte of a multi-byte
    // character as its own code point, so a recovered string carrying one came out
    // mojibake.
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some((end, displacement)) = try_parse_pool_page_field(bytes, i) {
            out.extend_from_slice(format!("poolOff[{displacement}]").as_bytes());
            i = end;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

pub(super) fn collect_pool_indices(expr: &str) -> Vec<u64> {
    let normalized = normalize_pool_page_field_exprs(expr);
    let bytes = normalized.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 5] == b"pool[" {
            let mut j = i + 5;
            let mut idx = 0u64;
            let mut has_digit = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                has_digit = true;
                idx = idx
                    .saturating_mul(10)
                    .saturating_add((bytes[j] - b'0') as u64);
                j += 1;
            }
            if has_digit && j < bytes.len() && bytes[j] == b']' {
                out.push(idx);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}
