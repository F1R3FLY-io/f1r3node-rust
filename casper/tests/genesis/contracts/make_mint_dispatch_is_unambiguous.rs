//! ★★ **`MakeMint.rho` dispatches decrement authority and `setLog` through disjoint shapes.**
//!
//! The former decrement listener had three arguments and a free first pattern:
//!
//! ```text
//! contract thisPurse(@providedDecr, @amount, return)
//! ```
//!
//! It therefore matched the three-argument `thisPurse!("setLog", logCh, ack)` protocol as well as
//! genuine decrement requests. RSpace had two admissible persistent continuations, and the loser
//! was selected by runtime matching order rather than by the purse protocol. The repair gives the
//! decrement operation the same literal method-tag discipline as every other public purse method:
//!
//! ```text
//! contract thisPurse(@"decr", @providedDecr, @amount, return)
//! @src!("decr", providedDecr, amount, return)
//! ```
//!
//! The listener is now four-argument and begins with the literal `"decr"`; `setLog` remains
//! three-argument and begins with the literal `"setLog"`. The two candidate languages have empty
//! intersection by both arity and first-pattern value. Inside the tagged listener,
//! `=*thisDecr` is a VarRef pattern: it compares with the already-bound capability instead of
//! shadowing it with a new binder.
//!
//! # Why this is the principled repair
//!
//! R1 preserves the existing purse and logging channels, changes one in-contract production call,
//! and makes decrement uniform with `deposit`, `split`, `sprout`, `getBalance`, `setLog`, and the
//! two-argument decrement-capability query. Moving `setLog` to another channel would require a new
//! capability-distribution protocol. Keeping the ambiguity would retain known technical debt. A
//! guarded persistent receive would add guard-evaluator behavior and change invalid-capability
//! liveness, while a global literal-pattern preference would alter RSpace semantics for unrelated
//! contracts.
//!
//! # The retracted probes remain retracted
//!
//! Earlier recursive 12-send and one-send loop probes raised `parallel or non expression found
//! where expression expected`. Bisection removed the loop and the raise disappeared; those probes
//! measured their own machinery, not `MakeMint`. They are not evidence for this repair. The tests
//! below instead pin the dispatch shapes in the blessed source, execute a minimal `setLog`, and
//! exercise the tagged listener's exact-capability refusal.

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

/// ★★ **ONE `setLog` send, on a purse that has done nothing else.**
///
/// The four-argument tagged decrement listener cannot consume this three-argument send. Completion
/// with `Nil` therefore checks the public logging path directly rather than sampling a race.
#[tokio::test]
async fn one_setlog_send_on_a_fresh_purse() {
    let body = on_fresh_purse(
        r#"new logSink, setAckCh in {
              @purse!("setLog", *logSink, *setAckCh) |
              // `Nil` is what the unique three-argument `setLog` listener answers.
              rhoSpec!("assert", (Nil, "== <-", *setAckCh),
                "setLog is answered by its unique tagged listener",
                *ackCh)
            }"#,
    );
    let source = one_test_suite("the probe", r#"    retCh!(Nil)"#, &body);
    let (outcome, records) = run_suite_detailed(&source).await;

    println!("one setLog send: {outcome:?}\n{}", describe(&records));

    match &outcome {
        SuiteOutcome::Raised(_) => panic!(
            "★★ ONE `setLog` send on a fresh purse RAISED: {}\n\
             Everything else in this program is exercised without raising by `test_deposit`, so the \
             `setLog` send is the attributable difference.\nRecorded:\n{}",
            outcome.raise_text().expect("matched Raised"),
            describe(&records),
        ),
        SuiteOutcome::Blocked { reported } => panic!(
            "★★ ONE `setLog` send BLOCKED — the unique tagged listener did not answer `Nil`. \
             reported: {reported:?}\nRecorded:\n{}",
            describe(&records),
        ),
        SuiteOutcome::Completed { .. } => {}
    }

    assert!(
        !records.is_empty(),
        "★ NON-VACUITY: completed with no recorded assertion",
    );
    for record in &records {
        println!("★ OBSERVED: {record}");
    }
    assert!(
        records.iter().all(|r| r.passed),
        "★★ `setLog`'s unique three-argument listener did not acknowledge with `Nil`.\n{}",
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
        "the wrong capability was not rejected with `false`:\n{}",
        describe(&records),
    );
}
