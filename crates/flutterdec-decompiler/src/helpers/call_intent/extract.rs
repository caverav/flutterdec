fn extract_pool_indices(s: &str) -> Vec<u64> {
    collect_pool_indices(s)
}

fn extract_string_literals(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut cur = String::new();
        while j < bytes.len() {
            let b = bytes[j];
            if b == b'\\' && j + 1 < bytes.len() {
                let next = bytes[j + 1] as char;
                cur.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    _ => next,
                });
                j += 2;
                continue;
            }
            if b == b'"' {
                out.push(cur);
                i = j + 1;
                break;
            }
            cur.push(b as char);
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
    }
    out
}

fn extract_selector_name(raw: &str) -> Option<String> {
    let raw_trim = raw.trim();
    if raw_trim.is_empty() {
        return None;
    }
    let raw_lower = raw_trim.to_ascii_lowercase();
    if raw_lower.contains(".dart") || raw_lower.contains('/') || raw_lower.contains('\\') {
        return None;
    }
    if raw_trim.contains("://") {
        return None;
    }
    if raw_trim.contains(' ') {
        return None;
    }

    let mut t = raw_trim;
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        t = inner.trim();
    }
    if let Some((prefix, _)) = t.split_once("/* pool[") {
        t = prefix.trim();
    }
    if let Some((before, _)) = t.split_once('@') {
        t = before.trim();
    }
    if let Some((_, after)) = t.split_once(':') {
        t = after.trim();
    }
    while let Some(rest) = t.strip_prefix('_') {
        t = rest;
    }
    if let Some(rest) = t.strip_prefix("init") {
        t = rest.trim();
    }
    if t.is_empty() || t.len() > 96 {
        return None;
    }

    let mut out = String::new();
    for c in t.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            out.push(c);
        } else if (c == '.' || c == '-' || c == '/' || c == ' ') && !out.ends_with('_') {
            out.push('_');
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        return None;
    }
    let first = out.chars().next().unwrap_or('_');
    if (!first.is_ascii_alphabetic() && first != '_') || out.starts_with("dart_") {
        return None;
    }
    Some(out)
}
