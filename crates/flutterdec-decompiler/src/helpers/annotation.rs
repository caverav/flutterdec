// The annotation literals, delimiters included. This file is the only
// definition of each one.
//
// Every literal has three consumers - the emitter that renders it, the strip
// parser that hides it from the quality counters, and the code-span accessor
// that hides it from later analysis - and a consumer hand-rolling its own copy
// of a shared fact is this branch's most repeated defect. The emitter used to
// write the non-exhaustive label with no delimiters while the strip parser
// matched a byte string carrying them: two independent spellings of one literal,
// with nothing tying them together. (The old spellings are not quoted here, on
// purpose - the drift check counts occurrences, and a comment quoting a literal
// is a second copy of it.)
//
// A constant holding only the label would not have fixed that, because both
// sides would still hand-roll ` /* ` and ` */`. So each literal owns *every*
// delimiter, and the separator and terminator - identical across all four - are
// defined once for all of them.

/// Between two candidate values. Defined once, because a hand-written ` | `
/// inside a fixed-slot template reintroduces the exactly-two-arms assumption
/// and leaves the separator as a third undetected drift axis.
const CANDIDATE_SEPARATOR: &str = " | ";

/// The terminator every annotation ends with.
const ANNOTATION_CLOSE: &str = " */";

/// Whether a candidate value contains the separator itself.
///
/// Such a value makes the rendered list impossible to read back: one candidate
/// spelling a bitwise or, and two candidates, render to the same bytes. A reader
/// cannot tell an arity of one from an arity of two, and neither can any check
/// comparing rendered values against recorded ones. Every loss site rejects it,
/// and the test lives here because this is where the separator is defined.
pub fn contains_candidate_separator(value: &str) -> bool {
    value.contains(CANDIDATE_SEPARATOR)
}

/// Whether a value carries a sequence no annotation may contain.
///
/// Braces steer the brace-sensitive compaction pass, so one inside a comment
/// makes a later structural rewrite read a block that is not there. A comment
/// terminator inside the span ends it early and leaves the rest of the
/// annotation on the line as code. The terminator is derived from the one
/// constant rather than spelled again, so a reworded terminator moves this test
/// with it.
pub fn contains_forbidden_sequence(value: &str) -> bool {
    value.contains('{') || value.contains('}') || value.contains(ANNOTATION_CLOSE.trim())
}

/// Whether a fully rendered span is safe to insert into a line.
///
/// The last gate before insertion, asked of the whole span rather than of its
/// candidates: the property the artifact has to hold is about the bytes that
/// reach the line, and a capture path that stops filtering, or a fifth literal
/// with an unlucky label, would otherwise put them there unchecked.
pub fn rendered_annotation_is_safe(annotation: &str) -> bool {
    annotation_at(annotation.as_bytes()).is_some()
        && annotation
            .strip_suffix(ANNOTATION_CLOSE)
            .is_some_and(|body| !contains_forbidden_sequence(body))
}

/// One annotation literal: opener, label, separator and terminator.
///
/// The only interpolation point is the candidate list, and its arity is
/// unbounded - a three-predecessor join renders three values, a loop header
/// with two disagreeing entry arms renders two - so no slot count is baked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationLiteral {
    open: &'static str,
}

impl AnnotationLiteral {
    /// The whole span for `values`, delimiters included.
    pub fn render<S: AsRef<str>>(&self, values: &[S]) -> String {
        let mut out = String::from(self.open);
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                out.push_str(CANDIDATE_SEPARATOR);
            }
            out.push_str(value.as_ref());
        }
        out.push_str(ANNOTATION_CLOSE);
        out
    }

    /// The opener, for consumers that recognise a span rather than render one.
    pub fn open(&self) -> &'static str {
        self.open
    }

    /// Length of the annotation span starting at `rest`, whose first bytes are
    /// this literal's opener. `None` when the span is never terminated: an
    /// unterminated annotation is left alone rather than swallowing the tail of
    /// a line.
    pub fn span_len(&self, rest: &[u8]) -> Option<usize> {
        let close = ANNOTATION_CLOSE.as_bytes();
        let body = rest.get(self.open.len()..)?;
        let end = body
            .windows(close.len())
            .position(|window| window == close)?;
        Some(self.open.len() + end + close.len())
    }
}

/// A join whose every actual predecessor contributed a usable candidate.
pub static EXHAUSTIVE_JOIN_ANNOTATION: AnnotationLiteral = AnnotationLiteral { open: " /* = " };

/// A join that lost at least one predecessor's value, so the rendered list is
/// evidence rather than an exhaustive claim.
pub static NON_EXHAUSTIVE_JOIN_ANNOTATION: AnnotationLiteral = AnnotationLiteral {
    open: " /* possible (non-exhaustive): ",
};

/// A loop header merge, entry value only.
pub static LOOP_ENTRY_ANNOTATION: AnnotationLiteral = AnnotationLiteral {
    open: " /* loop-entry value: ",
};

/// An ordinary call clobber: the value the register held before the call.
pub static PRE_CALL_ANNOTATION: AnnotationLiteral = AnnotationLiteral {
    open: " /* value before this call: ",
};

/// Every annotation literal. Consumers that must recognise all of them iterate
/// this rather than listing a subset, which is how the strip parser and the
/// code-span accessor stay in step with the emitters.
pub static ANNOTATION_LITERALS: [&AnnotationLiteral; 4] = [
    &EXHAUSTIVE_JOIN_ANNOTATION,
    &NON_EXHAUSTIVE_JOIN_ANNOTATION,
    &LOOP_ENTRY_ANNOTATION,
    &PRE_CALL_ANNOTATION,
];

/// The literal whose span starts at the front of `rest`, if any.
pub fn annotation_at(rest: &[u8]) -> Option<&'static AnnotationLiteral> {
    ANNOTATION_LITERALS
        .iter()
        .copied()
        .find(|literal| rest.starts_with(literal.open.as_bytes()))
}
