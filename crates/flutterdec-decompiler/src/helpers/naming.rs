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

/// Every spelling that denotes a register whose value was not recovered. The
/// order is fixed and this list is for membership checks, never emitted directly.
pub(super) fn unrecovered_value_spellings(canonical: &str) -> Vec<String> {
    let Some(reg) = canonical_reg(canonical) else {
        return Vec::new();
    };
    let Some(id) = reg.strip_prefix('x').and_then(|id| id.parse::<usize>().ok()) else {
        return Vec::new();
    };
    if id > 30 {
        return Vec::new();
    }
    let mut spellings = vec![
        reg,
        named_register_alias(id),
        named_indirect_target(canonical),
    ];
    spellings.sort();
    spellings.dedup();
    spellings
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
