fn normalize_file_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn count_ident_token(hay: &str, token: &str) -> usize {
    if token.is_empty() {
        return 0;
    }

    let mut count = 0usize;
    let bytes = hay.as_bytes();
    let mut i = 0usize;
    while i + token.len() <= hay.len() {
        if hay[i..].starts_with(token) {
            let prev_ok = if i == 0 {
                true
            } else {
                !is_ident_char(bytes[i - 1] as char)
            };
            let next_i = i + token.len();
            let next_ok = if next_i >= hay.len() {
                true
            } else {
                !is_ident_char(bytes[next_i] as char)
            };
            if prev_ok && next_ok {
                count += 1;
                i = next_i;
                continue;
            }
        }
        i += 1;
    }
    count
}
