// VAL-USEFUL-007: the candidate filter is a whitelist of three forms.
//
// The rule is stated positively - a literal, a field access, or a call-shaped
// expression - so a spelling nobody anticipated is rejected by default. The
// tests below therefore assert the *classification*, not mere rejection: a
// filter that rejects everything passes every `assert!(!accepted)` there is,
// and this branch has already shipped one convincing false pass built that way.

use crate::control_flow::CandidateForm;
use crate::control_flow::candidate_form;
use crate::control_flow::is_informative_annotation_candidate;

fn forms_of(candidates: &[&str]) -> Vec<Option<CandidateForm>> {
    candidates
        .iter()
        .map(|value| candidate_form(value))
        .collect()
}

#[test]
fn each_allowed_form_is_accepted_as_that_form() {
    assert_eq!(
        forms_of(&[
            "0",
            "-1",
            "0x3b",
            "0X3B",
            "arg0.f16",
            "thread.f104.f1968",
            "arg0._tag",
            "smiTag(local_m16)",
            "bitField(arg0._tag, 0xc, 0x14)",
            "smiTag(poolOff[20680].f8)",
            "obj1.method(arg2)",
            "allocate()",
        ]),
        vec![
            Some(CandidateForm::Literal),
            Some(CandidateForm::Literal),
            Some(CandidateForm::Literal),
            Some(CandidateForm::Literal),
            Some(CandidateForm::FieldAccess),
            Some(CandidateForm::FieldAccess),
            Some(CandidateForm::FieldAccess),
            Some(CandidateForm::Call),
            Some(CandidateForm::Call),
            Some(CandidateForm::Call),
            Some(CandidateForm::Call),
            Some(CandidateForm::Call),
        ]
    );
}

/// The hex digits of `0x14` spell `x14`, which is a register spelling. A token
/// scan that starts mid-number rejects every call carrying a hex argument, and
/// `bitField(_, 0xc, 0x14)` is the most common call shape in either corpus - so
/// this is the difference between a working whitelist and one that empties the
/// call form. The independent oracle had exactly this defect on its first run.
#[test]
fn a_hex_argument_is_not_read_as_a_register_spelling() {
    assert_eq!(
        candidate_form("bitField(arg0._tag, 0xc, 0x14)"),
        Some(CandidateForm::Call)
    );
    assert_eq!(candidate_form("smiTag(x14)"), None);
}

/// Everything outside the three forms, class by class. The comparison is
/// against a full vector of `None` so a case that starts being *accepted* fails
/// here rather than passing silently under an any-of assertion.
#[test]
fn every_forbidden_class_is_rejected() {
    let rejected = [
        // Opaque synthesised temporaries, every prefix the emitter and the
        // naming pass mint, bare and as part of a larger form.
        "t7",
        "tmp3",
        "objTmp2",
        "intTmp11",
        "resultTmp0",
        "t7.f8",
        "smiTag(tmp3)",
        // Unrecovered register spellings, likewise bare and embedded.
        "x0",
        "reg5",
        "framePointer",
        "returnAddress",
        "dispatchTarget.f8",
        // Bare identifiers: informative-looking, but they name nothing the
        // reader could not already see.
        "obj1",
        "thread",
        "arg0",
        // Compound expressions. Each contains an allowed form without being
        // one, which is precisely what a containment test could not tell apart:
        // `(thread.f80 + 1)` alone was 1,089 of 5,271 emitted candidates.
        "(thread.f80 + 1)",
        "arg0.f8 + 1",
        "smiTag(arg0) + 1",
        "(arg0.f8)",
        "(smiUntag(local_m40) >> 1)",
        "local_m8.f12[0x107]",
        // Malformed or truncated text.
        "smiTag(arg0",
        "smiTag(arg0))",
        "arg0.",
        ".f8",
        "0x",
        "",
        "   ",
        " arg0.f8",
    ];
    assert_eq!(
        forms_of(&rejected),
        vec![None; rejected.len()],
        "a forbidden class was classified as an allowed form"
    );
}

/// The three loss sites classify through this one function. A site holding its
/// own test is a partial subset of the rule, which is how four defects on this
/// branch produced a convincing false pass - so the uniformity is asserted
/// mechanically rather than trusted.
#[test]
fn every_loss_site_filters_through_the_one_whitelist() {
    let structured = source_of("control_flow/structured.rs");
    let emit = source_of("control_flow/emit.rs");
    let filter = "is_informative_annotation_candidate";

    for (site, body) in [
        ("join", function_body(&structured, "fn record_join_candidates")),
        (
            "loop entry",
            function_body(&structured, "fn record_loop_entry_candidates"),
        ),
        (
            "pre-call",
            function_body(&emit, "fn record_pre_call_snapshot"),
        ),
    ] {
        assert!(
            body.contains(filter),
            "the {site} capture path does not classify through `{filter}`"
        );
    }

    // And the whitelist itself is stated once. A second definition is a second
    // rule, whatever it happens to agree with today.
    assert_eq!(
        structured.matches("fn candidate_form").count(),
        1,
        "the whitelist must have exactly one definition"
    );
    assert_eq!(
        structured.matches(&format!("fn {filter}")).count(),
        1,
        "`{filter}` must have exactly one definition"
    );
    assert!(
        is_informative_annotation_candidate("arg0.f16")
            && !is_informative_annotation_candidate("(thread.f80 + 1)"),
        "the filter the sites call must be the whitelist, not a wrapper that lost it"
    );
}
