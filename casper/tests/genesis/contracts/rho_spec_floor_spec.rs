//! ★★ **The guards for the non-vacuity floor, each watched RED at the value it refuses.**
//!
//! [`crate::helper::rho_spec::RhoSpec::run_tests`] now refuses a run that verified nothing. A
//! refusal mechanism is worth exactly as much as the evidence that it fires, and "the suite still
//! passes" is not that evidence — it is the *absence* of it. This file supplies the evidence: for
//! each of the three gates, the input that must be rejected, the rejection observed as a value,
//! and its text pinned so a future edit that softens the message has to say so.
//!
//! ⚠ **No test here expects a panic.** The floor's decision is a pure function of (registered,
//! reported), so each guard calls that function and inspects the returned [`FloorBreach`]. A
//! `#[should_panic]` guard could not tell *which* panic it caught, and would go green on an
//! unrelated failure in the fifteen seconds of genesis that precede the decision.
//!
//! ★ **The ACCEPT case is a guard too.** A floor that rejects every input is not a floor, it is a
//! wall. [`the_floor_accepts_a_complete_run`] is what makes the four rejections below meaningful.
//!
//! # The end-to-end RED, recorded
//!
//! Gate 1 was also watched fire through the real harness, on a real subject, not only through this
//! file. `TokenMetadataTest.rho` as committed at `719f2432` asserted by sending `test!((…))` on a
//! channel it had `new`'d itself and that nothing ever received — it had never used the RhoSpec
//! protocol at all — so it was the corpus's own instance of the value gate 1 refuses:
//!
//! ```text
//! FAIL [0.022s] casper::mod genesis::contracts::token_metadata_spec::token_metadata_spec
//! panicked at casper/tests/helper/rho_spec.rs:172:13:
//! ★★ NON-VACUITY FLOOR — TokenMetadataTest.rho registers NO tests with RhoSpec, so running it
//! can verify nothing and passing it would mean nothing.
//! ```
//!
//! ★ 0.022 s: gate 1 is checked *before* the fifteen seconds of genesis, because a fixture that
//! registers nothing cannot be salvaged by running it. The fixture is rewritten to register
//! through RhoSpec, which is why that RED is pinned here rather than left as a failing test.

use std::collections::{BTreeSet, HashMap};

use casper::rust::helper::test_result_collector::{RhoTestAssertion, TestResult};

use crate::helper::rho_spec_suite_manifest::{
    check_registration, check_report, registered_test_names, FloorBreach,
};

/// A `TestResult` that reports one `RhoAssertTrue` per name in `reported`.
fn report(reported: &[&str], has_finished: bool) -> TestResult {
    let mut assertions = HashMap::with_capacity(reported.len());
    for name in reported {
        let mut attempts = HashMap::with_capacity(1);
        attempts.insert(1i64, vec![RhoTestAssertion::RhoAssertTrue {
            test_name: (*name).to_string(),
            is_success: true,
            clue: format!("synthetic assertion for {name}"),
        }]);
        assertions.insert((*name).to_string(), attempts);
    }
    TestResult {
        assertions,
        has_finished,
    }
}

fn names(of: &[&str]) -> BTreeSet<String> {
    of.iter().map(|n| (*n).to_string()).collect()
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// Gate 1 — a fixture that registers nothing
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★ RED, gate 1. The exact shape `TokenMetadataTest.rho` had: a fixture that asserts by sending
/// on a channel of its own, so `RhoSpec` never hears of a test.
#[test]
fn the_floor_refuses_a_fixture_that_registers_no_tests() {
    const NO_REGISTRATION: &str = r#"
new rl(`rho:registry:lookup`), subjectCh, resultCh, test in {
  rl!(`rho:system:tokenMetadata`, *subjectCh) |
  for (@(_, Subject) <- subjectCh) {
    @Subject!("name", *resultCh) |
    for (@name <- resultCh) {
      test!((name == "F1R3CAP"))
    }
  }
}
"#;

    let registered = registered_test_names(NO_REGISTRATION)
        .expect("the probe source must normalize; it is ordinary Rholang");
    assert!(
        registered.is_empty(),
        "★ FLOOR for this guard: the probe must genuinely register nothing, or the rejection \
         below is testing something else; got {registered:?}",
    );

    let breach = check_registration(&registered)
        .expect_err("★★ a fixture that registers no test must be REFUSED, not passed");
    assert_eq!(breach, FloorBreach::NoRegisteredTests);
    assert!(
        breach.to_string().starts_with("registers NO tests with RhoSpec"),
        "the refusal must name its reason first; got {breach}",
    );
}

/// ★ GREEN companion. Gate 1 must not refuse a fixture that *does* register — otherwise the guard
/// above proves only that the gate is stuck closed.
#[test]
fn the_floor_admits_a_fixture_that_registers_tests() {
    const REGISTRATION: &str = r#"
new rl(`rho:registry:lookup`), RhoSpecCh, test_first, test_second in {
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for (@(_, RhoSpec) <- RhoSpecCh) {
    @RhoSpec!("testSuite",
      [
        ("the first thing", *test_first),
        ("the second thing", *test_second),
      ])
  } |
  contract test_first(rhoSpec, _, ackCh) = { ackCh!(true) } |
  contract test_second(rhoSpec, _, ackCh) = { ackCh!(true) }
}
"#;

    let registered =
        registered_test_names(REGISTRATION).expect("the probe source must normalize");
    assert_eq!(
        registered,
        names(&["the first thing", "the second thing"]),
        "★ the extractor must read the registration out of the normalized term",
    );
    assert_eq!(check_registration(&registered), Ok(()));
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// Gate 2 — `has_finished`, recorded since the harness was written and read by nothing
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★ RED, gate 2, at exactly `has_finished == false`.
///
/// The input is otherwise perfect: every registered test reported, and every assertion succeeded.
/// The *only* thing wrong is the completion bit — which is precisely the subject, and which the
/// old `run_tests` would have passed without a word.
#[test]
fn the_floor_refuses_a_suite_that_did_not_run_to_completion() {
    let registered = names(&["a", "b"]);

    let unfinished = report(&["a", "b"], false);
    let breach = check_report(&registered, &unfinished)
        .expect_err("★★ has_finished == false must FAIL the suite");
    assert_eq!(breach, FloorBreach::DidNotFinish {
        registered: registered.clone(),
        reported: names(&["a", "b"]),
    });
    assert!(
        breach
            .to_string()
            .starts_with("did not run to completion: `testSuiteCompleted` never fired."),
        "the refusal must name its reason first; got {breach}",
    );

    // ★ THE CONTROLLED COMPARISON: flip that one bit and nothing else, and the floor admits it.
    // This is what shows the rejection above is attributable to `has_finished` and to nothing
    // else in the input.
    assert_eq!(
        check_report(&registered, &report(&["a", "b"], true)),
        Ok(()),
        "★ `has_finished` must be the ONLY difference between the refusal above and acceptance",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// Gate 3 — registered ═ reported, in both directions
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★ RED, gate 3 (`⊇`). "A suite that registers 5 tests and reports 2 fails."
#[test]
fn the_floor_refuses_a_registered_test_that_reported_no_assertion() {
    let registered = names(&["a", "b", "c", "d", "e"]);
    let partial = report(&["a", "b"], true);

    let breach = check_report(&registered, &partial).expect_err(
        "★★ a suite that registers five tests and reports two must FAIL — those three were \
         never checked",
    );
    assert_eq!(breach, FloorBreach::RegisteredButSilent {
        silent: names(&["c", "d", "e"]),
        registered: registered.clone(),
    });
    assert!(
        breach
            .to_string()
            .starts_with("registered 5 test(s) and 3 of them reported NO assertion"),
        "the refusal must count what went unchecked; got {breach}",
    );

    // ★ The same registration, fully reported, is admitted.
    assert_eq!(
        check_report(&registered, &report(&["a", "b", "c", "d", "e"], true)),
        Ok(())
    );
}

/// ★ RED, gate 3 (`⊆`) — the direction that guards the EXTRACTOR rather than the fixture.
///
/// If [`registered_test_names`] ever misses a registration, the suite's own assertions arrive
/// under a name the extractor did not predict. Without this direction the floor would silently
/// shrink to whatever the extractor happened to find, which is the failure mode a derived floor
/// exists to avoid.
#[test]
fn the_floor_refuses_a_report_from_a_name_it_never_saw_registered() {
    let registered = names(&["a"]);
    let surprising = report(&["a", "b"], true);

    let breach = check_report(&registered, &surprising).expect_err(
        "★★ an assertion under an unregistered name means the extractor under-reported; that \
         must FAIL rather than narrow the floor",
    );
    assert_eq!(breach, FloorBreach::ReportedButUnregistered {
        unregistered: names(&["b"]),
        registered: registered.clone(),
    });
    assert!(
        breach
            .to_string()
            .starts_with("reported assertions under 1 name(s) it never registered"),
        "the refusal must name the surprise; got {breach}",
    );
}

/// ★★ THE ACCEPT CASE. Without it the four refusals above are consistent with a floor that
/// refuses everything, which would be indistinguishable from a broken harness.
#[test]
fn the_floor_accepts_a_complete_run() {
    let registered = names(&["ListOps.sum works", "ListOps.filter works"]);
    let complete = report(&["ListOps.sum works", "ListOps.filter works"], true);
    assert_eq!(
        check_report(&registered, &complete),
        Ok(()),
        "★★ a run in which every registered test reported and the suite finished must PASS",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The extractor's own floor
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ THE NON-VACUITY FLOOR FOR THE EXTRACTOR. A walker that returned the empty set for
/// everything would make gate 1 refuse every suite and gate 3 vacuous; a walker that skipped one
/// syntactic nesting would silently shrink the floor for whichever fixture used it.
///
/// The fixture below buries a registration under **every** container a process can sit inside and
/// that a Rholang fixture can actually spell — `new`, a receive body, a contract body (a
/// persistent receive), both arms of a `match`, both branches of an `if`, a bundle, an element of
/// a list, a map value, a tuple component, and a send's data — and the assertion is on the exact
/// set, so a missed container shows up as a missing name and a spurious traversal as an extra one.
///
/// The operator arms (`EPlus`, `EEq`, `EPercentPercent`, …) are not probed here because a `Send`
/// in an operand position is not a term the normalizer will build; their coverage is guaranteed
/// instead by the **exhaustive** `match` in `push_expr_children`, which the compiler enforces.
/// That is the stronger of the two guarantees, since it holds for variants that do not yet exist.
#[test]
fn the_extractor_finds_a_registration_under_every_nesting_a_process_can_take() {
    const NESTED: &str = r#"
new RhoSpec, outer, inner, t in {
  RhoSpec!("testSuite", [("bare", *t)]) |

  new deeper in {
    RhoSpec!("testSuite", [("under new", *t)])
  } |

  for (@x <- outer) {
    RhoSpec!("testSuite", [("under for", *t)])
  } |

  contract inner(@y) = {
    RhoSpec!("testSuite", [("under contract", *t)])
  } |

  match 1 {
    1 => { RhoSpec!("testSuite", [("under match case", *t)]) }
    _ => { RhoSpec!("testSuite", [("under match default", *t)]) }
  } |

  if (true) {
    RhoSpec!("testSuite", [("under if true", *t)])
  } else {
    RhoSpec!("testSuite", [("under if false", *t)])
  } |

  outer!(bundle+{ RhoSpec!("testSuite", [("under bundle", *t)]) }) |

  outer!([ RhoSpec!("testSuite", [("under list", *t)]) ]) |

  outer!({ "k": RhoSpec!("testSuite", [("under map value", *t)]) }) |

  outer!(( RhoSpec!("testSuite", [("under tuple", *t)]), 2 )) |

  outer!(RhoSpec!("testSuite", [("under send data", *t)])) |

  RhoSpec!("testSuite", *outer, [("with setup", *t)]) |
  RhoSpec!("testSuite", *outer, *inner, [("with setup and teardown", *t)])
}
"#;

    let found = registered_test_names(NESTED).expect("the probe source must normalize");
    assert_eq!(
        found,
        names(&[
            "bare",
            "under new",
            "under for",
            "under contract",
            "under match case",
            "under match default",
            "under if true",
            "under if false",
            "under bundle",
            "under list",
            "under map value",
            "under tuple",
            "under send data",
            "with setup",
            "with setup and teardown",
        ]),
        "★★ the extractor must reach a `testSuite` registration through every container a process \
         can occur in, and must read all three arities of the registration",
    );
}

/// ★ The extractor must not invent registrations. A `"testSuite"` string that is not a selector,
/// and a registration list whose leading component is not a string literal, are both ignored.
#[test]
fn the_extractor_reads_registrations_and_not_look_alikes() {
    const LOOK_ALIKES: &str = r#"
new RhoSpec, notASuite, t, dynamicName in {
  notASuite!("testSuite") |
  notASuite!("a string mentioning testSuite in passing") |
  RhoSpec!("testSuite", [(*dynamicName, *t)]) |
  RhoSpec!("testSuite", [(42, *t)]) |
  RhoSpec!("testSuite", [("the only real one", *t)])
}
"#;

    let found = registered_test_names(LOOK_ALIKES).expect("the probe source must normalize");
    assert_eq!(
        found,
        names(&["the only real one"]),
        "★ only a `(\"literal name\", …)` tuple in a `\"testSuite\"` send is a registration",
    );
}

/// ★★ THE COMMITTED CORPUS, cross-checked without a hand-maintained list.
///
/// Every `.rho` under `casper/src/test/resources/` whose source spells the **string literal**
/// `"testSuite"` must yield a non-empty registration set, and every one that does not must yield
/// an empty one. That is set equality between two independently-computed families — a textual
/// occurrence and a normalized-term extraction — so neither side is a mirror of the other, and
/// neither is written down here.
///
/// ⚠ The textual side reads `"testSuite"` **with its quotes**, not the bare word. Measured while
/// writing this: the bare word also occurs inside the URI `` `rho:test:testSuiteCompleted` ``,
/// which put `FailingResultCollectorTest.rho` — a fixture that registers nothing and is *meant*
/// to — on the textual side and made this guard red for a reason that had nothing to do with the
/// extractor. The selector is a string literal in every registration, so quoting the needle is
/// both tighter and more faithful to what a registration is.
#[test]
fn every_corpus_fixture_that_mentions_testsuite_yields_a_registration() {
    let directory = ["casper/src/test/resources", "src/test/resources"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|candidate| candidate.is_dir())
        .expect(
            "the casper test-resource directory must be reachable from the test's working \
             directory",
        );

    let mut mentions: BTreeSet<String> = BTreeSet::new();
    let mut extracts: BTreeSet<String> = BTreeSet::new();
    let mut examined = 0usize;

    for entry in std::fs::read_dir(&directory).expect("the resource directory must be readable") {
        let path = entry.expect("a readable directory yields readable entries").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rho") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a `.rho` resource has a UTF-8 file name")
            .to_string();
        let source = std::fs::read_to_string(&path).expect("a `.rho` resource must be readable");
        examined += 1;

        if source.contains("\"testSuite\"") {
            mentions.insert(name.clone());
        }

        // `RhoSpecContract.rho` is the harness, not a fixture: it *receives* `"testSuite"` in a
        // `contract` head and forwards it, and its own `RhoSpec!("testSuite", …)` re-dispatches
        // the caller's `tests` variable rather than a literal list. It therefore mentions the
        // selector and registers no literal name — the one file for which the two families
        // legitimately differ, and it is identified by its role, not by being on a list.
        if name == "RhoSpecContract.rho" {
            mentions.remove(&name);
            continue;
        }

        let registered = registered_test_names(&source)
            .unwrap_or_else(|e| panic!("{name} must normalize: {e:?}"));
        if !registered.is_empty() {
            extracts.insert(name);
        }
    }

    assert!(
        examined >= 20,
        "★ FLOOR: this check must actually have read the corpus; it examined {examined} `.rho` \
         file(s) in {}",
        directory.display(),
    );
    assert!(
        !extracts.is_empty(),
        "★ FLOOR: the extractor found a registration in NO corpus fixture, which would make \
         every gate below it meaningless",
    );
    assert_eq!(
        mentions,
        extracts,
        "★★ a fixture that mentions `testSuite` must yield a registration and one that does not \
         must not.\n\
         mentions it but yields nothing: {:?}\n\
         yields a registration without mentioning it: {:?}",
        mentions.difference(&extracts).collect::<Vec<_>>(),
        extracts.difference(&mentions).collect::<Vec<_>>(),
    );
}
