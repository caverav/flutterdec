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
