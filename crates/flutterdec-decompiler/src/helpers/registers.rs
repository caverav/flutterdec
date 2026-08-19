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

/// Whether the token names a 32-bit view of a register. `canonical_reg` folds
/// `w3` and `x3` onto one key because they are one machine register, but the
/// width still decides what the register holds: a `w` form computes in 32 bits
/// and zero-extends, so the high half is cleared rather than carried.
pub(super) fn is_w_register(token: &str) -> bool {
    let t = token.trim().trim_end_matches('!').to_ascii_lowercase();
    let Some(rest) = t.strip_prefix('w') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// Whether an operand modifier is an extend rather than a shift.
///
/// An extend names the width it reads - `sxtw` sign-extends 32 bits, `uxtb`
/// takes 8 - and the renderer states that width in the expression it builds, so
/// the operand behind it is read whole. A kind missing here is read at the
/// width it is spelled at, which is the conservative direction: an unresolved
/// read rather than a value the access does not produce.
pub(super) fn is_extend_modifier(kind: &str) -> bool {
    matches!(
        kind,
        "sxtb" | "sxth" | "sxtw" | "sxtx" | "uxtb" | "uxth" | "uxtw" | "uxtx"
    )
}
