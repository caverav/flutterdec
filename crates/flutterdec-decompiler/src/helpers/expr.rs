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

/// Recognise `((pool + <page> /* lsl #<shift> */)).f<offset>` and return the
/// PP-relative byte displacement it reads.
///
/// This is the residual path for page-based pool loads the disassembler's register
/// tracker could not follow. It has no pool geometry, so it can only report the
/// displacement, never an entry index.
fn try_parse_shifted_pool_field(bytes: &[u8], start: usize) -> Option<(usize, u64)> {
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

    let page_start = i;
    while i < bytes.len()
        && !bytes[i].is_ascii_whitespace()
        && bytes[i] != b'/'
        && bytes[i] != b')'
    {
        i += 1;
    }
    if i == page_start {
        return None;
    }
    let page_token = std::str::from_utf8(&bytes[page_start..i]).ok()?;
    let page = parse_non_negative_i64_token(page_token)?;

    i = skip_ascii_ws(bytes, i);
    if i + 2 > bytes.len() || &bytes[i..i + 2] != b"/*" {
        return None;
    }
    i += 2;
    i = skip_ascii_ws(bytes, i);
    if i + 3 > bytes.len() || &bytes[i..i + 3] != b"lsl" {
        return None;
    }
    i += 3;
    i = skip_ascii_ws(bytes, i);
    if i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }

    let shift_start = i;
    while i < bytes.len()
        && !bytes[i].is_ascii_whitespace()
        && bytes[i] != b'*'
        && bytes[i] != b'/'
    {
        i += 1;
    }
    if i == shift_start {
        return None;
    }
    let shift_token = std::str::from_utf8(&bytes[shift_start..i]).ok()?;
    let shift = parse_non_negative_i64_token(shift_token)?;
    if shift > 63 {
        return None;
    }

    i = skip_ascii_ws(bytes, i);
    if i + 2 > bytes.len() || &bytes[i..i + 2] != b"*/" {
        return None;
    }
    i += 2;
    i = skip_ascii_ws(bytes, i);

    // Close the shifted page term.
    if i >= bytes.len() || bytes[i] != b')' {
        return None;
    }
    i += 1;
    i = skip_ascii_ws(bytes, i);

    // A second displacement can sit between the page and the field, as in
    // `((pool + 0x2c /* lsl #12 */) + 0xdc8)).f0`, when one instruction builds
    // the page address and the next reads through it with its own offset. Both
    // displacements are known, so the entry is still exact.
    let mut inner = 0u64;
    if i < bytes.len() && bytes[i] == b'+' {
        i += 1;
        i = skip_ascii_ws(bytes, i);
        let disp_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b')' {
            i += 1;
        }
        let disp_token = std::str::from_utf8(&bytes[disp_start..i]).ok()?;
        inner = parse_non_negative_i64_token(disp_token)?;
        i = skip_ascii_ws(bytes, i);
    }

    // Close every parenthesis this match opened, so replacing the span cannot
    // leave an unbalanced one behind.
    for _ in 1..opens {
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
    let offset = std::str::from_utf8(&bytes[off_start..i]).ok()?.parse::<u64>().ok()?;

    let page_bytes = page.checked_shl(shift as u32)?;
    let total = page_bytes.checked_add(inner)?.checked_add(offset)?;
    if !total.is_multiple_of(8) {
        return None;
    }

    Some((i, total))
}

pub(super) fn normalize_pool_page_field_exprs(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some((end, displacement)) = try_parse_shifted_pool_field(bytes, i) {
            out.push_str(&format!("poolOff[{displacement}]"));
            i = end;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
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
