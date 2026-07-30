//! ★★ **`MakeMint.rho`'s `setLog` send has TWO matching listeners — a latent dispatch ambiguity,
//! DERIVED and NOT witnessed.**
//!
//! [`super::genesis_contract_failure_mechanisms::makemint_test_log_set_is_nondeterministic_where_test_deposit_is_not`]
//! measured that `MakeMintTest.rho`'s `test_log_set` is a **race** — same tree, same command, FAIL
//! then PASS — and labelled its mechanism a *hypothesis*: a shared `bdCh` across the
//! `if (Nil != *logCh)` branches. This file tested a **competing** hypothesis. ⚠ **It did not
//! confirm it**, and that negative result is the file's main content, because the alternative was
//! plausible enough that a future reader will propose it again.
//!
//! # The structural fact — DERIVED, and it stands regardless
//!
//! `thisPurse` carries two persistent **three-argument** listeners:
//!
//! ```text
//!   MakeMint.rho:43    contract thisPurse(@providedDecr, @amount, return)   ⚠ FIRST PATTERN IS FREE
//!   MakeMint.rho:102   contract thisPurse(@"setLog",     logCh,   ack)
//! ```
//!
//! `@providedDecr` is a free pattern variable, so it matches **every** first argument — including
//! the string `"setLog"`. A `thisPurse!("setLog", *log, *ack)` send therefore has **two matching
//! continuations**, and RSpace's choice between two matching continuations is not specified by the
//! contract. ★ `setLog` is the ONLY selector with this exposure, and that is an enumeration, not an
//! impression: `"decr"` / `"getBalance"` / `"sprout"` are 2-ary and the only 2-ary listener is
//! `@"decr"`, whose literal cannot match the other two; `"deposit"` is 4-ary and alone at that
//! arity. Count: **1 on the axis of selectors sent to `thisPurse` whose arity is shared with a
//! free-first-pattern listener.**
//!
//! # ⚠ What was MEASURED, including the part that refutes the hypothesis
//!
//! The two candidate continuations answer differently, which makes the ambiguity **witnessable**
//! rather than merely arguable:
//!
//! | consumed by | replies | value on the ack channel |
//! |---|---|---|
//! | `contract thisPurse(@"setLog", logCh, ack)` (`:103`) | `ack!(Nil)` | `Nil` |
//! | `contract thisPurse(@providedDecr, @amount, return)` (`:81-91`, the `_` arm) | `return!(false)` | `false` |
//!
//! `"setLog"` cannot match the `*thisDecr` arm — `thisDecr` is `bundle0{bundle0{*thisPurse}|*decr}`
//! — so a stolen send must land in the `_` arm and answer `false`.
//!
//! | probe | result | reading |
//! |---|---|---|
//! | 12 sequential `setLog` sends in a recursive `contract loop(@n)` | **RAISED**, `parallel or non expression found where expression expected` | looked like a reproduction of `test_log_set` |
//! | the same, `ATTEMPTS = 1` | **RAISED**, identical error | so it did not need repetition |
//! | [`one_setlog_send_on_a_fresh_purse`] — one send, no loop, no accumulator | **PASS**, ack `Nil` | ⚠ **the raise was the LOOP's, not `MakeMint`'s** |
//!
//! ★★ **The third row retracts the first two.** The bisection removed the probe's own machinery —
//! the recursive `contract loop`, the `match answer` classification, the `stealsCh` accumulator —
//! and the raise went with it. So the 12-send and 1-send observations are **measurements of the
//! probe**, not of the contract, and they are recorded here as a failed attempt precisely so the
//! next reader does not re-derive them as evidence. This is the same trap the sibling cell already
//! documented: *"two hand-built reconstructions were measured and neither reproduced"* — the trap
//! being that a reconstruction can raise for its own reasons and look like success.
//!
//! ⇒ **`test_log_set`'s race is NOT explained by the arity collision on the evidence available**,
//! and the sibling cell's `bdCh` hypothesis is not displaced. What this file establishes is that
//! the collision is real in the source, that it has a decisive one-observation witness if it ever
//! fires, and that [`one_setlog_send_on_a_fresh_purse`] is now standing watch for it.
//!
//! # Why `MakeMint.rho` is NOT edited here
//!
//! An unwitnessed dispatch ambiguity does not justify a blessed-contract change. Both plausible
//! repairs — giving the wildcard listener a distinguishing arity, or moving `setLog` off the shared
//! channel — change the contract's **public dispatch**, so every caller is affected, and they
//! differ in how. That needs an owner ruling, and the ruling needs this measurement first. Filed as
//! a finding; the guard below is what makes the finding falsifiable in the meantime.

use crate::genesis::contracts::rho_spec_probe::{
    describe, one_test_suite, run_suite_detailed, SuiteOutcome,
};

/// ★★ **ONE `setLog` send, on a purse that has done nothing else.**
///
/// This is the minimal form, reached by bisection from a 12-iteration loop (see the module header's
/// measurement table). Everything before the `setLog` — the registry lookup, `MakeMint(*mintCh)`,
/// `makePurse` — is exercised without raising by `MakeMintTest.rho`'s `test_deposit`, which
/// COMPLETES. So the `setLog` send is the only new thing this program does, and the outcome is
/// attributable to it.
///
/// Three outcomes, each with a distinct reading, and the cell reports which occurred:
///
/// | observation | reading |
/// |---|---|
/// | ack is `Nil` | the `setLog` contract answered; the collision did not fire on this draw |
/// | ack is `false` | ★ **positive witness** — the 3-ary wildcard consumed a `setLog` send |
/// | raise | the dispatch reached code that neither listener's body accounts for |
#[tokio::test]
async fn one_setlog_send_on_a_fresh_purse() {
    const BODY: &str = r#"    new MakeMintCh, mintCh, purseCh, logSink, setAckCh in {
      rl!(`rho:system:makeMint`, *MakeMintCh) |
      for (@(_, MakeMint) <- MakeMintCh) {
        @MakeMint!(*mintCh) |
        for (@mint <- mintCh) {
          @mint!("makePurse", 0, *purseCh) |
          for (@purse <- purseCh) {
            @purse!("setLog", *logSink, *setAckCh) |
            // `Nil` is what `contract thisPurse(@"setLog", logCh, ack)` answers (MakeMint.rho:103).
            // `false` is what the 3-ary wildcard's `_` arm answers (MakeMint.rho:83).
            rhoSpec!("assert", (Nil, "== <-", *setAckCh),
              "setLog is answered by the setLog contract, not by the 3-ary wildcard",
              *ackCh)
          }
        }
      }
    }"#;

    let source = one_test_suite("the probe", r#"    retCh!(Nil)"#, BODY);
    let (outcome, records) = run_suite_detailed(&source).await;

    println!("one setLog send: {outcome:?}\n{}", describe(&records));

    match &outcome {
        SuiteOutcome::Raised(_) => panic!(
            "★★ ONE `setLog` send on a fresh purse RAISED: {}\n\
             Everything else in this program is exercised without raising by `test_deposit`, so the \
             `setLog` send is the attributable difference. `MakeMint.rho`'s logging path has never \
             been evaluated by any passing test in this repository.\nRecorded:\n{}",
            outcome.raise_text().expect("matched Raised"),
            describe(&records),
        ),
        SuiteOutcome::Blocked { reported } => panic!(
            "★★ ONE `setLog` send BLOCKED — neither listener rested on it, so `setAckCh` should have \
             carried `Nil` or `false`. reported: {reported:?}\nRecorded:\n{}",
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
        "★★ `setLog`'s ack was NOT `Nil`. An ack of `false` is a POSITIVE WITNESS that the 3-ary \
         wildcard `contract thisPurse(@providedDecr, @amount, return)` (`MakeMint.rho:43`) consumed \
         a send meant for `contract thisPurse(@\"setLog\", logCh, ack)` (`MakeMint.rho:102`) — the \
         two listeners have the same arity and the first pattern is free.\n{}",
        describe(&records),
    );
}
