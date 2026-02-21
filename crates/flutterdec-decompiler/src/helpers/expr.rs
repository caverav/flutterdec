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
        let mag = -v;
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
            if lt == "null" && r_int.is_some() {
                return fmt_int(r_int.unwrap());
            }
            if rt == "null" && l_int.is_some() {
                return fmt_int(l_int.unwrap());
            }
            if l_int == Some(0) {
                return rt.to_string();
            }
            if r_int == Some(0) {
                return lt.to_string();
            }
            if let (Some(a), Some(b)) = (l_int, r_int) {
                return fmt_int(a + b);
            }
            if let (Some((base, off)), Some(delta)) = (parse_base_offset_expr(lt), r_int) {
                let sum = off + delta;
                if sum == 0 {
                    return base;
                }
                if sum > 0 {
                    return format!("({base} + {})", fmt_int(sum));
                }
                return format!("({base} - {})", fmt_int(-sum));
            }
        }
        "-" => {
            if r_int == Some(0) {
                return lt.to_string();
            }
            if let (Some(a), Some(b)) = (l_int, r_int) {
                return fmt_int(a - b);
            }
            if let (Some((base, off)), Some(delta)) = (parse_base_offset_expr(lt), r_int) {
                let sum = off - delta;
                if sum == 0 {
                    return base;
                }
                if sum > 0 {
                    return format!("({base} + {})", fmt_int(sum));
                }
                return format!("({base} - {})", fmt_int(-sum));
            }
        }
        _ => {}
    }

    format!("({lt} {op} {rt})")
}
