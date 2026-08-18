//! Regression guards for MakeMint's disjoint decrement and logging protocols.
//!
//! The legacy decrement receive used three arguments with a free first pattern, so it could consume
//! a three-argument `setLog` send. Decrement now has the literal `"decr"` selector and four
//! arguments; `setLog` remains three-argument and literal-tagged. The receive languages therefore
//! have an empty intersection by both arity and first-pattern value.

use casper::rust::genesis::contracts::embedded_rho;

use crate::genesis::contracts::rho_spec_probe::{
    describe, one_test_suite, run_suite_detailed, SuiteOutcome,
};

#[test]
fn decrement_and_setlog_have_disjoint_blessed_dispatch_shapes() {
    let source = embedded_rho::MAKE_MINT;
    let tagged_listener = r#"contract thisPurse(@"decr", @providedDecr, @amount, return)"#;
    let tagged_call = r#"@src!("decr", bundle0{bundle0{src}|*decr}, amount, *result)"#;

    assert_eq!(source.matches(tagged_listener).count(), 1);
    assert_eq!(source.matches(tagged_call).count(), 1);
    assert_eq!(
        source
            .matches(r#"contract thisPurse(@"setLog", logCh, ack)"#)
            .count(),
        1,
    );
    assert!(source.contains("match providedDecr {\n                      =*thisDecr =>"));
    assert!(!source.contains("contract thisPurse(@providedDecr, @amount, return)"));
}

fn on_fresh_purse(probe: &str) -> String {
    const TEMPLATE: &str = r#"    new MakeMintCh, mintCh, purseCh in {
      rl!(`rho:system:makeMint`, *MakeMintCh) |
      for (@(_, MakeMint) <- MakeMintCh) {
        @MakeMint!(*mintCh) |
        for (@mint <- mintCh) {
          @mint!("makePurse", 0, *purseCh) |
          for (@purse <- purseCh) {
            __PROBE__
          }
        }
      }
    }"#;
    TEMPLATE.replace("__PROBE__", probe)
}

#[tokio::test]
async fn one_setlog_send_on_a_fresh_purse() {
    let body = on_fresh_purse(
        r#"new logSink, setAckCh in {
              @purse!("setLog", *logSink, *setAckCh) |
              rhoSpec!("assert", (Nil, "== <-", *setAckCh),
                "setLog is answered by its unique tagged listener",
                *ackCh)
            }"#,
    );
    let source = one_test_suite("the probe", r#"    retCh!(Nil)"#, &body);
    let (outcome, records) = run_suite_detailed(&source).await;

    match &outcome {
        SuiteOutcome::Raised(_) => panic!(
            "one setLog send on a fresh purse raised: {}\n{}",
            outcome.raise_text().expect("matched Raised"),
            describe(&records),
        ),
        SuiteOutcome::Blocked { reported } => panic!(
            "one setLog send blocked; reported: {reported:?}\n{}",
            describe(&records),
        ),
        SuiteOutcome::Completed { .. } => {}
    }
    assert!(!records.is_empty(), "the probe recorded no assertion");
    assert!(
        records.iter().all(|record| record.passed),
        "setLog did not acknowledge with Nil:\n{}",
        describe(&records),
    );
}

#[tokio::test]
async fn tagged_decrement_rejects_the_wrong_capability_with_false() {
    let body = on_fresh_purse(
        r#"new decrAckCh in {
              @purse!("decr", "not-a-capability", 1, *decrAckCh) |
              rhoSpec!("assert", (false, "== <-", *decrAckCh),
                "the tagged decrement listener enforces its exact capability",
                *ackCh)
            }"#,
    );
    let source = one_test_suite("the probe", r#"    retCh!(Nil)"#, &body);
    let (outcome, records) = run_suite_detailed(&source).await;

    assert!(
        outcome.is_completed(),
        "the tagged decrement refusal must complete, got {outcome:?}\n{}",
        describe(&records),
    );
    assert!(
        !records.is_empty(),
        "the refusal probe recorded no assertion"
    );
    assert!(
        records.iter().all(|record| record.passed),
        "the wrong capability was not rejected with false:\n{}",
        describe(&records),
    );
}
