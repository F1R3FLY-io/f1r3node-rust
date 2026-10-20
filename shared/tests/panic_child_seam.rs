//! # Self-tests of the child-process seam in `support/panic_child.rs`
//!
//! Added by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
//! policy test). The seam decides whether a rewritten test passes, so each refusal rule
//! has a test here that makes the seam refuse. A seam that accepted every child would
//! make every rewritten test pass with no evidence.
//!
//! | test | rule |
//! |---|---|
//! | [`a_panic_after_the_marker_is_accepted`] | rule 4, by message and by location, and the reports of an unwind |
//! | [`a_subject_that_returns_is_refused`] | rule 2 |
//! | [`a_panic_in_the_setup_is_refused`] | rule 3 |
//! | [`a_panic_with_other_text_is_refused`] | rule 4 |
//! | [`an_exit_after_the_marker_is_refused`] | rule 4, when no panic follows the marker |
//! | [`a_name_that_resolves_to_no_test_is_refused`] | rule 1 |
//! | [`a_child_cannot_start_a_child`] | the recursion guard |
//! | [`an_empty_expectation_is_refused`] | the expectation must narrow the panic |
//! | [`the_parser_reads_both_header_forms`] | the parser of panic blocks |

#[path = "support/panic_child.rs"]
mod panic_child;

use panic_child::{panic_blocks, Expected, PanicBlock, PanicChild, DEFAULT_CASE};

fn seam(function: &str) -> PanicChild { PanicChild::new(module_path!(), function) }

#[track_caller]
fn refusal(child: &PanicChild, expected: Expected<'_>) -> String {
    match child.try_expect_panic_in_case(DEFAULT_CASE, expected) {
        Ok(report) => panic!("the seam accepted a child that it must refuse: {report:?}"),
        Err(reason) => reason,
    }
}

struct Reporter<'a>(&'a PanicChild);

impl Drop for Reporter<'_> {
    fn drop(&mut self) { self.0.report("state", "after\nthe unwind"); }
}

#[test]
fn a_panic_after_the_marker_is_accepted() {
    let child = seam("a_panic_after_the_marker_is_accepted");
    if let Some(case) = child.case() {
        let _reporter = Reporter(&child);
        child.subject_reached();
        panic!("the expected seam text in case {case}");
    }
    let report = child.expect_panic_in_case("message", "the expected seam text in case message");
    assert!(
        report
            .location()
            .starts_with("shared/tests/panic_child_seam.rs:"),
        "the accepted panic must carry the location of the subject, not {}",
        report.location()
    );
    assert_eq!(
        report.reported("state"),
        Some("after\nthe unwind"),
        "a report that a destructor writes during the unwind must reach the parent"
    );
    let report = child.expect_panic_at_in_case("location", "shared/tests/panic_child_seam.rs:");
    assert_eq!(report.message(), "the expected seam text in case location");
}

#[test]
fn a_subject_that_returns_is_refused() {
    let child = seam("a_subject_that_returns_is_refused");
    if child.is_child() {
        child.subject_reached();
        return;
    }
    let reason = refusal(&child, Expected::Message("any text"));
    assert!(
        reason.contains("the subject returned without a panic"),
        "{reason}"
    );
}

#[test]
fn a_panic_in_the_setup_is_refused() {
    let child = seam("a_panic_in_the_setup_is_refused");
    if child.is_child() {
        panic!("the setup failed before the subject");
    }
    let reason = refusal(&child, Expected::Message("the setup failed"));
    assert!(reason.contains("never reached its subject"), "{reason}");
}

#[test]
fn a_panic_with_other_text_is_refused() {
    let child = seam("a_panic_with_other_text_is_refused");
    if child.is_child() {
        child.subject_reached();
        panic!("a different message");
    }
    let reason = refusal(&child, Expected::Message("the expected message"));
    assert!(
        reason.contains("no panic after the subject meets"),
        "{reason}"
    );
}

#[test]
fn an_exit_after_the_marker_is_refused() {
    let child = seam("an_exit_after_the_marker_is_refused");
    if child.is_child() {
        child.subject_reached();
        std::process::exit(3);
    }
    let reason = refusal(&child, Expected::Message("any text"));
    assert!(
        reason.contains("without a panic after its subject started"),
        "{reason}"
    );
}

#[test]
fn a_name_that_resolves_to_no_test_is_refused() {
    let child = seam("no_test_has_this_name");
    let reason = refusal(&child, Expected::Message("any text"));
    assert!(reason.contains("did not run exactly one test"), "{reason}");
}

#[test]
fn a_child_cannot_start_a_child() {
    let child = seam("a_child_cannot_start_a_child");
    if child.is_child() {
        child.subject_reached();
        let reason = refusal(&child, Expected::Message("any text"));
        panic!("the inner start was refused: {reason}");
    }
    child.expect_panic("the inner start was refused: a child never starts another child");
}

#[test]
fn an_empty_expectation_is_refused() {
    let child = seam("an_empty_expectation_is_refused");
    let reason = refusal(&child, Expected::Message(""));
    assert!(reason.contains("the expectation is empty"), "{reason}");
}

#[test]
fn the_parser_reads_both_header_forms() {
    let stderr = "thread 'main' panicked at src/a.rs:1:2:\n\
                  first line\n\
                  second line\n\
                  note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\
                  thread 'worker' (4242) panicked at src/b.rs:3:4:\n\
                  other\n\
                  PANIC-CHILD report key=value\n";
    assert_eq!(panic_blocks(stderr), vec![
        PanicBlock {
            location: "src/a.rs:1:2".to_string(),
            message: "first line\nsecond line".to_string(),
        },
        PanicBlock {
            location: "src/b.rs:3:4".to_string(),
            message: "other".to_string()
        },
    ]);
}
