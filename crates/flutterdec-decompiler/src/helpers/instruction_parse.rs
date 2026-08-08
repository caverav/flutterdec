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

/// Register-offset memory operand, e.g. `[x1, x2, lsl #3]`.
///
/// `parse_mem_operand` folds these to displacement 0 because the offset is not a
/// literal, so an indexed access rendered as `base.f0` is indistinguishable from
/// a real read of field 0. That is the addressing mode Dart uses for list and
/// array elements: 2094 sites on the sampled 3.12.1 binary once dispatch-table
/// loads and frame accesses are excluded.
pub(super) fn parse_indexed_operand(op: &str) -> Option<(String, String, u32)> {
    let s = op.trim();
    let lb = s.find('[')?;
    let rb = s[lb..].find(']')? + lb;
    let mut parts = s[lb + 1..rb].split(',').map(str::trim);
    let base = canonical_reg(parts.next()?)?;
    let index = canonical_reg(parts.next()?)?;
    let shift = match parts.next() {
        None => 0,
        Some(extend) => {
            let (kind, amount) = extend.split_once('#')?;
            // Only a left shift scales the index; the extends widen it.
            if !matches!(
                kind.trim().to_ascii_lowercase().as_str(),
                "lsl" | "uxtw" | "sxtw" | "sxtx"
            ) {
                return None;
            }
            amount.trim().parse().ok()?
        }
    };
    Some((base, index, shift))
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
