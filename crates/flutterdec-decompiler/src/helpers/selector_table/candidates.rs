fn selector_candidates(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let normalized = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    push_unique(&mut out, normalized);

    let mut token = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            token.push(c.to_ascii_lowercase());
        } else if !token.is_empty() {
            push_unique(&mut out, token.clone());
            token.clear();
        }
    }
    if !token.is_empty() {
        push_unique(&mut out, token);
    }

    let derived = out.clone();
    for t in derived {
        if let Some(rest) = t.strip_prefix("init") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("get") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("set") {
            push_unique(&mut out, format!("set{}", rest));
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("native") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix('_') {
            push_unique(&mut out, rest.to_string());
        }
    }

    out
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !s.is_empty() && !out.contains(&s) {
        out.push(s);
    }
}
