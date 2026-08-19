/// Longest sanitized function name we put in an artifact file name.
///
/// Recovered Dart names routinely exceed the 255-byte `NAME_MAX` on their own
/// (mangled generics, `@`-suffixed private names, deep owner chains), which used to
/// abort a whole run with `File name too long`. Artifact names are already prefixed
/// with the unique function id, so truncating the stem cannot collide.
const MAX_FILE_NAME_STEM: usize = 160;

/// Give a text artifact the trailing newline a POSIX text file is supposed to have.
///
/// Emitted bodies are built by joining lines, so they ended at the last character with
/// no terminator: 20,890 of 22,102 and 27,236 of 28,753 pseudocode files on the two
/// reference samples. That silently corrupts any corpus-wide scan, because
/// `cat dir/* | wc -l` splices the last line of one file onto the first of the next and
/// undercounts by about 2.4%. It already caused one published denominator to disagree
/// with another for reasons that took a separate investigation to explain.
///
/// Empty bodies are left empty rather than becoming a lone newline, so a function that
/// emits nothing still produces a zero-length file.
fn terminated(body: &str) -> String {
    if body.is_empty() || body.ends_with('\n') {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len() + 1);
    out.push_str(body);
    out.push('\n');
    out
}

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

    #[test]
    fn terminated_lets_a_corpus_be_concatenated_without_splicing() {
        // The defect: joined bodies end mid-line, so concatenating two files merges the
        // last line of one into the first of the next. Two one-line files must read as
        // two lines after concatenation, not one.
        let a = terminated("dynamic sub_1() {}");
        let b = terminated("dynamic sub_2() {}");
        let corpus = format!("{a}{b}");
        assert_eq!(
            corpus.lines().count(),
            2,
            "concatenating terminated files must not splice line boundaries"
        );
        assert!(a.ends_with('\n'));

        // Idempotent, so a body that already ends correctly is untouched.
        assert_eq!(terminated("already\n"), "already\n");
        // An empty body stays empty rather than becoming a lone newline.
        assert_eq!(terminated(""), "");
    }
}
