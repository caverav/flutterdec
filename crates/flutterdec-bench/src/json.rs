//! Just enough JSON to emit a raw result document without a dependency.
//!
//! Output only. The harness never reads its own JSON back; the aggregator works
//! from the tab-separated sample stream written beside it, which is produced
//! from the same in-memory values, so the two cannot disagree and no parser is
//! needed here.

use std::fmt::Write as _;

pub enum Json {
    Null,
    Bool(bool),
    U(u64),
    F(f64),
    S(String),
    A(Vec<Json>),
    O(Vec<(String, Json)>),
}

impl Json {
    pub fn s(value: impl Into<String>) -> Self {
        Json::S(value.into())
    }

    pub fn o(fields: Vec<(&str, Json)>) -> Self {
        Json::O(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        let inner_pad = "  ".repeat(indent + 1);
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            Json::U(v) => {
                let _ = write!(out, "{v}");
            }
            Json::F(v) => {
                // JSON has no NaN and no infinity. Writing a bare `NaN` would
                // produce a document no reader accepts, which is worse than a
                // null the reader can see and reject.
                if v.is_finite() {
                    let _ = write!(out, "{v:.6}");
                } else {
                    out.push_str("null");
                }
            }
            Json::S(v) => write_string(out, v),
            Json::A(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&inner_pad);
                    item.write(out, indent + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad);
                out.push(']');
            }
            Json::O(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (key, value)) in fields.iter().enumerate() {
                    out.push_str(&inner_pad);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    if i + 1 < fields.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad);
                out.push('}');
            }
        }
    }
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character class that would otherwise end the string early or leave
    /// a raw control byte in the document.
    #[test]
    fn escapes_what_would_otherwise_break_the_document() {
        let mut probe = String::from("quote:");
        probe.push('"');
        probe.push_str(" backslash:");
        probe.push('\\');
        probe.push_str(" newline:");
        probe.push('\n');
        probe.push_str(" tab:");
        probe.push('\t');
        probe.push_str(" control:");
        probe.push('\u{1}');

        let rendered = Json::s(probe).to_pretty();
        assert!(rendered.contains("quote:\\\""), "{rendered}");
        assert!(rendered.contains("backslash:\\\\"), "{rendered}");
        assert!(rendered.contains("newline:\\n"), "{rendered}");
        assert!(rendered.contains("tab:\\t"), "{rendered}");
        assert!(rendered.contains("control:\\u0001"), "{rendered}");
        assert!(
            !rendered.contains('\u{1}'),
            "no raw control byte may survive"
        );
    }

    #[test]
    fn non_finite_numbers_do_not_produce_an_unparseable_document() {
        assert_eq!(Json::F(f64::NAN).to_pretty().trim(), "null");
        assert_eq!(Json::F(f64::INFINITY).to_pretty().trim(), "null");
        assert_eq!(Json::F(0.5).to_pretty().trim(), "0.500000");
    }

    #[test]
    fn empty_containers_stay_on_one_line() {
        let value = Json::o(vec![("a", Json::A(vec![])), ("b", Json::O(vec![]))]);
        let rendered = value.to_pretty();
        assert!(rendered.contains("\"a\": []"), "{rendered}");
        assert!(rendered.contains("\"b\": {}"), "{rendered}");
    }
}
