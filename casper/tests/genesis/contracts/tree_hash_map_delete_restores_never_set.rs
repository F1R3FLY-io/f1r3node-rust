//! ★★ **`TreeHashMap!("update", …)` after `TreeHashMap!("delete", …)` RESURRECTED the deleted key.**
//!
//! `TreeHashMapTest.rho:385` (`test_update_after_delete`) names the invariant in its own comment:
//!
//! > After delete, update walks the same code path as update-on-never-set.
//!
//! It does not, and the visible symptom — a raise from `Nil + 1` — is only the messenger. The
//! mechanism is a **data-integrity** defect in a contract the genesis ceremony publishes to the
//! registry, and the raise is what happens to a caller whose update body cannot cope with the `Nil`
//! it was never supposed to be handed.
//!
//! # The mechanism, at `file:line`
//!
//! `TreeHashMapUpdater` (`casper/src/main/resources/Registry.rho:288`) decides, at the leaf, whether
//! there is anything to update:
//!
//! ```text
//!   Registry.rho:294    if (n == len) {              ← at the leaf
//!   Registry.rho:296      if (val == 0) {            ⚠ THE DEFECT
//!   Registry.rho:299        ret!(Nil)                   "there's nothing here"
//!                         } else {
//!   Registry.rho:303        for (@val <- @(*bitmaskTag, node, *storeToken)) {   ← lock
//!   Registry.rho:305          update!(val.get(suffix), *resultCh) |
//!   Registry.rho:308          @(…)!(val.set(suffix, newVal))                    ← WRITES THE KEY
//! ```
//!
//! `val` at a leaf is one of two different carriers:
//!
//! | leaf state | `val` | `val == 0` |
//! |---|---|---|
//! | created by `MakeNode`, never written (`Registry.rho:76`) | `0` — an **`Int`** | `true` |
//! | after any `set` (`Registry.rho:183`) | `{suffix: v, …}` — a **`Map`** | `false` |
//!
//! So `val == 0` asks *“has this leaf ever been written?”* when the question the updater has to
//! answer is *“is `suffix` present in this leaf?”*. Those differ in exactly the state `delete`
//! leaves behind: `TreeHashMapDeleter` writes `val.delete(suffix)` at `Registry.rho:393` and leaves
//! the leaf **Map** in place, so `val == 0` is `false`, the updater takes the else branch, and calls
//! `update!(val.get(suffix), …)` = `update!(Nil, …)`. Whatever the update body returns is then
//! written back by `val.set(suffix, newVal)` — **and the deleted key is back**.
//!
//! `TreeHashMapContains` (`Registry.rho:259`) already asks the right question, `val.contains(suffix)`,
//! on the same leaf Map. The repair is to ask it in the updater too.
//!
//! # ⚠ Why the repair is in the UPDATER and not in the DELETER — this was DERIVED, not chosen
//!
//! The tempting fix is to make `delete` restore the never-set state: if the leaf Map is empty after
//! `val.delete(suffix)`, write `0` back instead. [`update_in_a_leaf_a_sibling_still_occupies_does_not_resurrect`]
//! **refutes** that repair. It puts two keys in one leaf *by construction* — `depth = 0` makes
//! `2 * depth == 0`, so `ByteArrayToNybbleList` returns `[]` for every key and every key lives in the
//! single root leaf `(map, [])` — deletes one, and updates it. The leaf is **not** empty, so an
//! empty-leaf prune would not fire, yet the key is resurrected just the same.
//!
//! So the defect is not "delete leaves an empty Map behind"; it is "the updater tests the leaf's
//! CARRIER where it must test the KEY". A deleter-side prune would additionally have to clear the
//! parent's bitmask bit up the whole spine to be complete, which is a far larger change to a blessed
//! contract, and it would still leave the sibling case broken. One fix, at the site that asks the
//! wrong question.
//!
//! # ⚠ Why the update body's INVOCATION is observed, and not only the resulting value
//!
//! A cell that checked only `get` would go green on a repair that let `update` run and then threw its
//! answer away — which would still charge the caller for the call and still hand it a `Nil` it has no
//! contract for. The probe's update body therefore flips a flag on a channel, **inside** the receive
//! that releases the updater, so `retCh!` cannot outrun the flag and "was it called" is race-free.
//! Every cell reports the pair `(invoked, value)`.
//!
//! # RED, verbatim, before the `Registry.rho` repair
//!
//! ```text
//! ★★ update AFTER DELETE invoked the update body and/or changed the stored value.
//!    expected (false, Nil) — the update body must NOT run and `get` must stay Nil, which is
//!    exactly what update-on-never-set does (see the control).
//!    Registry.rho:296's `val == 0` tests the leaf's CARRIER, not the KEY: after
//!    `delete` the leaf is an EMPTY MAP, which is not 0, so the updater falls through to
//!    `update!(val.get(suffix), …)` = `update!(Nil, …)` and then WRITES THE ANSWER BACK.
//!    recorded assertions:
//!     [FAIL] update after delete must neither invoke the update body nor resurrect the key — expected (false, Nil) , actual (true, 99)
//! ```
//!
//! `(true, 99)` is the whole finding in four characters: `true` — the body ran; `99` — the value it
//! returned is now the value of a key that had been deleted.

use crate::genesis::contracts::rho_spec_probe::{
    describe, one_test_suite, only, run_suite_detailed, AssertionRecord, SuiteOutcome, PROBE_TEST,
};

/// What the probe's update body returns. Any value distinguishable from `Nil` would do; a number is
/// used so a resurrected key is unmistakable in the RED text.
const RESURRECTED: i64 = 99;

/// The probe's `setup`. `TreeHashMap` is reached through the registry, not through a private copy, so
/// the cells measure the contract the genesis ceremony actually published.
const SETUP: &str = r#"    retCh!(Nil)"#;

/// Build a probe body around `prepare`, which must end by leaving the map in the state under test and
/// then must NOT itself call `"update"`.
///
/// `prepare` is spliced with `thm` (the map), `TreeHashMap` (the contract, bound as a name exactly as
/// `TreeHashMapTest.rho:70` binds it) and `updateFn` in scope, and must send on `readyCh` when the
/// preparation has been acknowledged.
fn probe_body(
    depth: i64,
    key: &str,
    prepare: &str,
    observe: &str,
    expected: &str,
    clue: &str,
) -> String {
    format!(
        r#"    new TreeHashMapCh, invokedCh, updateFn, initCh, readyCh, updCh, getCh, observedCh in {{
      rl!(`rho:lang:treeHashMap`, *TreeHashMapCh) |

      // `false` is the sentinel meaning "the update body has not run". It is replaced INSIDE the
      // receive that also sends `retCh`, because `retCh` is what releases the updater: written in
      // parallel, the flag could still be in flight when the observation reads it.
      invokedCh!(false) |
      contract updateFn(@val, retCh) = {{
        for (_ <- invokedCh) {{ invokedCh!(true) | retCh!({RESURRECTED}) }}
      }} |

      for (TreeHashMap <- TreeHashMapCh) {{
        TreeHashMap!("init", {depth}, *initCh) |
        for (@thm <- initCh) {{
{prepare}
          |
          for (_ <- readyCh) {{
            TreeHashMap!("update", thm, {key}, *updateFn, *updCh) |
            for (_ <- updCh) {{
              TreeHashMap!("get", thm, {key}, *getCh) |
              for (@got <- getCh) {{
                // The flag is PEEKED, so this observation does not consume it.
                for (@invoked <<- invokedCh) {{
{observe}
                }}
              }}
            }}
          }}
        }}
      }} |

      rhoSpec!("assert", ({expected}, "== <-", *observedCh), "{clue}", *ackCh)
    }}"#
    )
}

/// The common shape: report `(invoked, value)`.
const OBSERVE_PAIR: &str = r#"                  observedCh!((invoked, got))"#;

/// Assert that the one recorded assertion of a probe suite passed, or fail with the observation.
fn expect_probe_pass(outcome: &SuiteOutcome, records: &[AssertionRecord], headline: &str) {
    assert!(
        outcome.is_completed(),
        "★ the probe suite must reach `testSuiteCompleted` — a block means the map operations never \
         finished and nothing below is a measurement of the contract. Got {outcome:?}. Recorded:\n{}",
        describe(records),
    );
    assert_eq!(
        outcome
            .reported_set()
            .expect("completed suites carry a reported set"),
        only(PROBE_TEST),
        "★ exactly the registered probe must have reported",
    );
    assert!(
        !records.is_empty(),
        "★ NON-VACUITY: the suite completed but recorded NO assertion, so it proves nothing. \
         {headline}",
    );
    assert!(
        records.iter().all(|r| r.passed),
        "{headline}\n   recorded assertions:\n{}",
        describe(records),
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The control, and the floor that keeps the control honest
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★ **THE CONTROL: `update` on a never-set key does not invoke the update body.**
///
/// This is the behaviour `test_update_after_delete` says update-after-delete should match, and it is
/// reached by a different branch entirely: the key's path does not exist, so the *interior* test at
/// `Registry.rho:318` returns `Nil` at `:321` and the leaf is never consulted. It passes before the
/// repair as well as after — it is here to establish that the expected pair `(false, Nil)` is the
/// contract's own answer somewhere, and not a value this file invented.
#[tokio::test]
async fn update_on_a_never_set_key_does_not_invoke_the_update_body() {
    let source = one_test_suite(
        PROBE_TEST,
        SETUP,
        &probe_body(
            3,
            r#""never set""#,
            r#"          readyCh!(Nil)"#,
            OBSERVE_PAIR,
            "(false, Nil)",
            "update on a never-set key must not invoke the update body",
        ),
    );

    let (outcome, records) = run_suite_detailed(&source).await;
    println!("never-set control: {outcome:?}\n{}", describe(&records));
    expect_probe_pass(
        &outcome,
        &records,
        "★ CONTROL: `update` on a key that was never set must answer without invoking the update \
         body — `TreeHashMapUpdater` returns at `Registry.rho:321` because the interior bitmask bit \
         is clear. If THIS is failing, the expectation `(false, Nil)` used by the subjects is wrong \
         and this whole file needs re-deriving.",
    );
}

/// ★★ **THE NON-VACUITY FLOOR: the instrument can SEE an invocation.**
///
/// Every other cell in this file asserts that the update body did *not* run. Without this cell they
/// would all pass against a probe whose flag never flips — a broken `invokedCh`, an `updateFn` the
/// updater cannot reach, a `depth` at which nothing is stored. Here the key IS present, so the body
/// must run, must be handed the stored value, and its answer must become the new value.
#[tokio::test]
async fn update_on_a_present_key_invokes_the_update_body_and_stores_its_answer() {
    let source = one_test_suite(
        PROBE_TEST,
        SETUP,
        &probe_body(
            3,
            r#""k""#,
            r#"          TreeHashMap!("set", thm, "k", 50, *readyCh)"#,
            OBSERVE_PAIR,
            &format!("(true, {RESURRECTED})"),
            "update on a present key must invoke the update body and store its answer",
        ),
    );

    let (outcome, records) = run_suite_detailed(&source).await;
    println!("present-key floor: {outcome:?}\n{}", describe(&records));
    expect_probe_pass(
        &outcome,
        &records,
        "★★ FLOOR: with the key PRESENT the update body must run and its answer must be stored. If \
         this fails, the probe cannot observe an invocation at all and every \"the body did not run\" \
         assertion in this file is vacuous.",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The subjects
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`set` then `delete` then `update`: the update body must not run, and the key must stay gone.**
///
/// This is `TreeHashMapTest.rho:385`'s scenario with the raise factored out — the probe's update body
/// returns a constant instead of `val + 1`, so the cell measures the update path rather than
/// `Nil + 1`'s refusal. The leaf here is an **empty** Map: one key was set, and it was deleted.
///
/// Watched RED before the `Registry.rho:296` repair; the RED text is quoted in this module's header.
#[tokio::test]
async fn update_after_delete_does_not_invoke_the_update_body_nor_resurrect_the_key() {
    let source = one_test_suite(
        PROBE_TEST,
        SETUP,
        &probe_body(
            3,
            r#""k""#,
            r#"          new setCh, delCh in {
            TreeHashMap!("set", thm, "k", 50, *setCh) |
            for (_ <- setCh) {
              TreeHashMap!("delete", thm, "k", *delCh) |
              for (_ <- delCh) { readyCh!(Nil) }
            }
          }"#,
            OBSERVE_PAIR,
            "(false, Nil)",
            "update after delete must neither invoke the update body nor resurrect the key",
        ),
    );

    let (outcome, records) = run_suite_detailed(&source).await;
    println!("update-after-delete: {outcome:?}\n{}", describe(&records));
    expect_probe_pass(
        &outcome,
        &records,
        "★★ update AFTER DELETE invoked the update body and/or changed the stored value.\n   \
         expected (false, Nil) — the update body must NOT run and `get` must stay Nil, which is \
         exactly what update-on-never-set does (see the control).\n   \
         Registry.rho:296's `val == 0` tests the leaf's CARRIER, not the KEY: after `delete` the \
         leaf is an EMPTY MAP, which is not 0, so the updater falls through to \
         `update!(val.get(suffix), …)` = `update!(Nil, …)` and then WRITES THE ANSWER BACK.",
    );
}

/// ★★ **The GENERALISATION, and the refutation of the deleter-side repair: the leaf is NOT empty.**
///
/// `depth = 0` makes `2 * depth == 0`, so `ByteArrayToNybbleList!(hash, 0, 0, [], …)` returns `[]`
/// for **every** key and every key lives in the single root leaf `(map, [])` created by
/// `MakeNode!(0, (*map, []))` at `Registry.rho:95`. Two keys are set, one is deleted, and the deleted
/// one is updated. The leaf still holds the sibling, so:
///
/// * an "if the leaf is empty, write `0` back" repair in `TreeHashMapDeleter` would **not fire**, and
/// * the key is resurrected anyway.
///
/// Which is why the repair is at the updater's test and not at the deleter's write. The sibling's
/// value is carried in the observation too, so a repair that fixed the deleted key by damaging its
/// neighbour cannot pass.
#[tokio::test]
async fn update_in_a_leaf_a_sibling_still_occupies_does_not_resurrect() {
    let source = one_test_suite(
        PROBE_TEST,
        SETUP,
        &probe_body(
            0,
            r#""k""#,
            r#"          new setKCh, setSibCh, delCh in {
            TreeHashMap!("set", thm, "k", 50, *setKCh) |
            for (_ <- setKCh) {
              // Set the sibling AFTER "k" so both writes are ordered and the leaf Map is known to
              // hold two keys before the delete.
              TreeHashMap!("set", thm, "sibling", 7, *setSibCh) |
              for (_ <- setSibCh) {
                TreeHashMap!("delete", thm, "k", *delCh) |
                for (_ <- delCh) { readyCh!(Nil) }
              }
            }
          }"#,
            r#"                  new sibCh in {
                    TreeHashMap!("get", thm, "sibling", *sibCh) |
                    for (@sib <- sibCh) {
                      observedCh!((invoked, got, sib))
                    }
                  }"#,
            "(false, Nil, 7)",
            "a sibling in the same leaf must neither hide the defect nor be damaged by its repair",
        ),
    );

    let (outcome, records) = run_suite_detailed(&source).await;
    println!("sibling-occupied leaf: {outcome:?}\n{}", describe(&records));
    expect_probe_pass(
        &outcome,
        &records,
        "★★ update AFTER DELETE resurrected the key even though the leaf was NOT empty (a sibling \
         key still occupies it), or the repair damaged the sibling.\n   \
         expected (false, Nil, 7) = (body did not run, deleted key still absent, sibling intact).\n   \
         ⚠ THE NON-EMPTY LEAF IS THE POINT: it refutes any repair that prunes empty leaves in \
         `TreeHashMapDeleter` (`Registry.rho:377`). The wrong question is asked at \
         `Registry.rho:296`, and that is where it must be fixed.",
    );
}
