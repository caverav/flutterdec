/// Longest sanitized function name we put in an artifact file name.
///
/// Recovered Dart names routinely exceed the 255-byte `NAME_MAX` on their own
/// (mangled generics, `@`-suffixed private names, deep owner chains), which used to
/// abort a whole run with `File name too long`. Artifact names are already prefixed
/// with the unique function id, so truncating the stem cannot collide.
const MAX_FILE_NAME_STEM: usize = 160;

fn normalize_file_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if out.len() >= MAX_FILE_NAME_STEM {
            break;
        }
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
    let token_bytes = token.as_bytes();
    let mut i = 0usize;
    while i + token_bytes.len() <= bytes.len() {
        if bytes[i..].starts_with(token_bytes) {
            let prev_ok = if i == 0 {
                true
            } else {
                !is_ident_char(bytes[i - 1] as char)
            };
            let next_i = i + token_bytes.len();
            let next_ok = if next_i >= bytes.len() {
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

#[cfg(test)]
mod helpers_tests {
    use super::*;

    #[test]
    fn count_ident_token_handles_utf8_text() {
        let hay = r#"dynamic x = local; final s = "Možete"; local = 2;"#;
        assert_eq!(count_ident_token(hay, "local"), 2);
    }

    #[test]
    fn normalize_file_name_caps_long_recovered_dart_names() {
        let long = format!("method_{}_deserialize", "Isar_CollectionSchema".repeat(40));
        let out = normalize_file_name(&long);
        assert_eq!(out.len(), MAX_FILE_NAME_STEM);
        assert!(out.starts_with("method_Isar_CollectionSchema"));
        // Leaves room for the `{id:05}_` prefix and the longest extension we emit.
        assert!("00000_".len() + out.len() + ".dartpseudo".len() < 255);
    }

    #[test]
    fn normalize_file_name_keeps_short_names_intact() {
        assert_eq!(normalize_file_name("sub_652b98"), "sub_652b98");
        assert_eq!(normalize_file_name("method.Duration.dyn:_"), "method_Duration_dyn__");
    }
}
