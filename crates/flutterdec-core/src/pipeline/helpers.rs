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
