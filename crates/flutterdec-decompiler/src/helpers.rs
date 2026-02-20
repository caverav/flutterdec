use super::LiftState;
use flutterdec_ir::FunctionIr;
use std::collections::BTreeSet;

pub(super) fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "function".to_string()
    } else {
        out
    }
}

pub(super) fn canonical_reg(token: &str) -> Option<String> {
    let t = token.trim().trim_end_matches('!').to_ascii_lowercase();
    if t.len() < 2 {
        return None;
    }
    let mut chars = t.chars();
    let first = chars.next()?;
    if first != 'x' && first != 'w' {
        return None;
    }
    let rest = chars.as_str();
    if rest.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("x{rest}"));
    }
    None
}

pub(super) fn is_zero_reg(token: &str) -> bool {
    matches!(
        token
            .trim()
            .trim_end_matches('!')
            .to_ascii_lowercase()
            .as_str(),
        "xzr" | "wzr"
    )
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

pub(super) fn split_operands(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;

    for c in s.chars() {
        match c {
            '[' => {
                depth += 1;
                cur.push(c);
            }
            ']' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                let piece = cur.trim();
                if !piece.is_empty() {
                    out.push(piece.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }

    let piece = cur.trim();
    if !piece.is_empty() {
        out.push(piece.to_string());
    }

    out
}

pub(super) fn split_instruction(src: &str) -> (String, Vec<String>) {
    let mut it = src.trim().splitn(2, char::is_whitespace);
    let mnemonic = it.next().unwrap_or("").to_ascii_lowercase();
    let operands = it.next().unwrap_or("").trim();
    let ops = if operands.is_empty() {
        Vec::new()
    } else {
        split_operands(operands)
    };
    (mnemonic, ops)
}

pub(super) fn parse_mem_operand(op: &str) -> Option<(String, i64)> {
    let s = op.trim();
    let lb = s.find('[')?;
    let rb = s[lb..].find(']')? + lb;
    let inside = &s[lb + 1..rb];
    let parts: Vec<&str> = inside.split(',').collect();
    if parts.is_empty() {
        return None;
    }
    let base = parts[0].trim().to_ascii_lowercase();
    let off = if parts.len() >= 2 {
        parse_int(parts[1]).unwrap_or(0)
    } else {
        0
    };
    Some((base, off))
}

pub(super) fn normalize_target(target: &str) -> String {
    let mut last_hex = None;
    for token in target.split(|c: char| c.is_whitespace() || c == ',') {
        let t = token.trim().trim_start_matches('#');
        if let Some(hex) = t.strip_prefix("0x") {
            if let Ok(v) = u64::from_str_radix(hex, 16) {
                last_hex = Some(v);
            }
        }
    }
    if let Some(v) = last_hex {
        return format!("0x{v:x}");
    }
    for token in target.split(|c: char| c.is_whitespace() || c == ',') {
        let t = token.trim().trim_start_matches('#');
        if let Some(reg) = canonical_reg(t) {
            return reg;
        }
    }
    target.trim_start_matches('#').to_string()
}

pub(super) fn named_indirect_target(token: &str) -> String {
    if let Some(reg) = canonical_reg(token) {
        if let Some(id) = reg.strip_prefix('x') {
            if let Ok(n) = id.parse::<usize>() {
                return match n {
                    30 => "dispatchTarget".to_string(),
                    2 => "cachedTarget".to_string(),
                    _ => format!("indirectTarget{n}"),
                };
            }
        }
    }
    token.to_string()
}

pub(super) fn named_register_alias(n: usize) -> String {
    match n {
        30 => "returnAddress".to_string(),
        29 => "framePointer".to_string(),
        _ => format!("reg{n}"),
    }
}

pub(super) fn local_name(off: i64) -> String {
    if off < 0 {
        format!("local_m{}", -off)
    } else if off > 0 {
        format!("local_p{}", off)
    } else {
        "local_0".to_string()
    }
}

pub(super) fn collect_stack_offsets(ir: &FunctionIr) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();

    for block in &ir.blocks {
        for ins in &block.instrs {
            let (mnemonic, ops) = split_instruction(&ins.src);
            if (mnemonic == "ldur" || mnemonic == "ldr" || mnemonic == "stur" || mnemonic == "str")
                && ops.len() >= 2
            {
                if let Some((base, off)) = parse_mem_operand(&ops[1]) {
                    if base == "x29" {
                        out.insert(off);
                    }
                }
            }
        }
    }

    out
}

pub(super) fn init_state() -> LiftState {
    let mut s = LiftState::default();
    for i in 0..8 {
        s.reg_values.insert(format!("x{i}"), format!("arg{i}"));
    }
    s.reg_values.insert("x15".to_string(), "sp".to_string());
    s.reg_values.insert("x22".to_string(), "null".to_string());
    s.reg_values.insert("x26".to_string(), "thread".to_string());
    s.reg_values.insert("x27".to_string(), "pool".to_string());
    s
}

pub(super) fn cond_from_cmp(branch: &str, cmp: &(String, String)) -> Option<String> {
    let op = match branch {
        "b.eq" => "==",
        "b.ne" => "!=",
        "b.lt" => "<",
        "b.le" => "<=",
        "b.gt" => ">",
        "b.ge" => ">=",
        "b.hi" => ">",
        "b.ls" => "<=",
        "b.lo" => "<",
        "b.hs" => ">=",
        _ => return None,
    };
    Some(format!("{} {} {}", cmp.0, op, cmp.1))
}
