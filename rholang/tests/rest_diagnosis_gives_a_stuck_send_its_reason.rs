//! ★ A stuck term that carries its reason — and a proof that carrying it does not
//! move the post-state hash.
//!
//! # What is under test, and what is deliberately NOT
//!
//! A send with no matching receive **rests**. That is the rho-calculus, it is
//! correct, and **these tests assert that it still rests**: not one of them
//! expects an error, and the "no resting data" floor below would fail if any
//! program here started failing instead of resting. Turning an arity-mismatched
//! send into an error would change which programs are accepted, which is a
//! consensus break.
//!
//! What is under test is the **reason**, as a value. `RestReason` is a
//! disposition, not an absence — the same move `GuardDisposition` made from
//! `Option<bool>` to `{Admits, Refutes, NotABoolean, Undecidable, Failed}`.
//!
//! # ⚠ The rows assert the VALUE, never the message text
//!
//! Every row matches on the `RestReason` variant and its fields. The rendered
//! message is asserted exactly once, in `the_message_names_both_arities`, and only
//! for the numbers it must contain — so this suite is not a spelling test and a
//! reworded message does not redden it, while a message that stopped naming the
//! arities does.
//!
//! # ★ The consensus row is the point of the design
//!
//! `asking_why_a_term_rests_does_not_move_the_post_state_hash` runs the identical
//! program on two runtimes, asks one of them for the diagnosis, and requires the
//! two checkpoint roots to be **equal**. That is the measurement behind
//! `rest_diagnosis`'s §4 claim, and it is a measurement rather than an argument
//! because "reads only" is exactly the kind of claim that is true until someone
//! adds a cache: the sibling reader `HotStore::get_data` takes a **write** lock
//! and history-fills, so the property is not a property of "reading".

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rest_diagnosis::{self, Admits, RestReason, RestSite};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::storage::storage_printer;
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

/// Evaluate `source`, require it to have rested rather than failed, and return
/// every reason the store can give.
///
/// ⚠ The `errors.is_empty()` assertion is load-bearing in the opposite direction
/// from usual: it is what pins "resting is still resting". If a future change
/// made an arity mismatch an error, every row in this file would fail here first,
/// naming the semantics that moved.
async fn rest_of(runtime: &mut RhoRuntimeImpl, source: &str) -> Vec<RestSite> {
    let result = runtime
        .evaluate_with_term(source)
        .await
        .unwrap_or_else(|e| panic!("{source} must evaluate, got {e:?}"));
    assert!(
        result.errors.is_empty(),
        "★ {source} must REST, not fail — resting is the calculus and this suite does not \
         change it. Errors: {:?}",
        result.errors
    );
    rest_diagnosis::diagnose(&runtime.get_hot_changes().await)
}

/// The single reason the store gives, or a failure naming how many it gave.
fn only(sites: &[RestSite], what: &str) -> RestReason {
    assert_eq!(
        sites.len(),
        1,
        "{what}: expected exactly one resting site, got {}: {:#?}",
        sites.len(),
        sites
    );
    sites[0].reason.clone()
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ WORK ITEM #169 — the arity-mismatched send
// ─────────────────────────────────────────────────────────────────────────────

/// A send of two payloads to a receive that binds three. Neither side can ever
/// change, so no COMM can ever fire — and that is exactly what the reason says.
///
/// ★ This is the shape that hid a genesis contract invocation two parameters out
/// of date for the entire life of the harness.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_arity_mismatched_send_names_the_arity_and_says_it_can_never_fire() {
    with_runtime("rest-diagnosis-arity-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@a, @b, @c <- ch) { Nil } | ch!(1, 2) }"#,
        )
        .await;

        assert_eq!(
            only(&sites, "a 2-payload send to a 3-name receive"),
            RestReason::ArityMismatch {
                sent: 2,
                count: 1,
                admitted: vec![Admits::Exactly(3)],
            }
        );
    })
    .await;
}

/// The message must carry the two numbers a developer needs. Asserted on the
/// numbers, not the prose, so the wording stays free (diagnostics are not a
/// consensus surface) while the content does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_message_names_both_arities() {
    with_runtime("rest-diagnosis-message-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@a, @b, @c <- ch) { Nil } | ch!(1, 2) }"#,
        )
        .await;
        let message = sites
            .first()
            .expect("a 2-payload send to a 3-name receive must produce a reason")
            .describe();
        for needle in ["ARITY MISMATCH", "2 payload(s)", "3 payload(s)", "EVER"] {
            assert!(
                message.contains(needle),
                "the message must name {needle:?}; got: {message}"
            );
        }
    })
    .await;
}

/// A remainder absorbs a **surplus**, so a bind with two explicit patterns and a
/// remainder admits two payloads or more. Three payloads into it is therefore not
/// an arity question at all — and the row proves it by exhibiting a program where
/// the COMM does not fire *for a different reason*: the first pattern is the
/// literal `42`.
///
/// ⚠ The first draft of this row asserted the surplus alone
/// (`ch!(1, 2, 3)` into `@a, @b ... @rest`) and went RED with **zero** resting
/// sites — because that program *fires*, which is the correct answer and the
/// opposite of what the row claimed. The premise was wrong, not the code.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remainder_admits_a_surplus_so_the_refusal_is_not_about_arity() {
    with_runtime("rest-diagnosis-remainder-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@42, @b ... @rest <- ch) { Nil } | ch!(1, 2, 3) }"#,
        )
        .await;
        match only(&sites, "a 3-payload send into a 2-plus-remainder bind") {
            RestReason::ArityMismatch { admitted, .. } => panic!(
                "a remainder bind admits a surplus; reported a mismatch against {admitted:?}"
            ),
            other => assert_eq!(
                other,
                RestReason::ShapeOrGuardRefused {
                    sent: 3,
                    count: 1,
                    admitted: vec![Admits::AtLeast(2)],
                },
                "arity 3 is admissible against `AtLeast(2)`; the literal `42` is what refuses"
            ),
        }
    })
    .await;
}

/// ★ And a remainder does **not** make every arity admissible: it sets a FLOOR.
/// One payload into `@a, @b ... @rest` is a genuine arity mismatch, and the
/// reported bound must be `AtLeast(2)` rather than `Exactly(2)` — so the row pins
/// both halves of [`Admits`] against each other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remainder_still_has_a_floor_and_a_deficit_is_a_mismatch() {
    with_runtime("rest-diagnosis-deficit-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@a, @b ... @rest <- ch) { Nil } | ch!(1) }"#,
        )
        .await;
        assert_eq!(
            only(&sites, "a 1-payload send into a 2-plus-remainder bind"),
            RestReason::ArityMismatch {
                sent: 1,
                count: 1,
                admitted: vec![Admits::AtLeast(2)],
            },
            "a remainder absorbs a surplus, never a deficit"
        );
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE SIBLING — a send nobody reads
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_send_nobody_reads_says_nothing_is_installed_there() {
    with_runtime("rest-diagnosis-unread-", |mut runtime| async move {
        let sites = rest_of(&mut runtime, r#"new ch in { ch!(1) }"#).await;
        assert_eq!(
            only(&sites, "a send with no receive at all"),
            RestReason::NoReader { sent: 1, count: 1 }
        );
    })
    .await;
}

/// Two sends of the same arity on one channel are ONE reason with `count: 2`,
/// not two reasons — so a channel with a thousand resting data does not produce a
/// thousand lines.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn data_of_one_arity_are_grouped() {
    with_runtime("rest-diagnosis-grouped-", |mut runtime| async move {
        let sites = rest_of(&mut runtime, r#"new ch in { ch!(1) | ch!(2) }"#).await;
        assert_eq!(
            only(&sites, "two 1-payload sends on one channel"),
            RestReason::NoReader { sent: 1, count: 2 }
        );
    })
    .await;
}

/// Two sends of DIFFERENT arity on one channel are two reasons, because they are
/// two different questions: one may be reconcilable and the other not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn data_of_different_arities_are_separate_questions() {
    with_runtime("rest-diagnosis-arities-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@a, @b <- ch) { Nil } | ch!(1) | ch!(1, 2, 3) }"#,
        )
        .await;
        let reasons: Vec<RestReason> = sites.iter().map(|s| s.reason.clone()).collect();
        assert_eq!(
            reasons,
            vec![
                RestReason::ArityMismatch {
                    sent: 1,
                    count: 1,
                    admitted: vec![Admits::Exactly(2)],
                },
                RestReason::ArityMismatch {
                    sent: 3,
                    count: 1,
                    admitted: vec![Admits::Exactly(2)],
                },
            ],
            "one reason per resting arity, ascending"
        );
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE FLOORS — a reason that is always "arity" is worthless
// ─────────────────────────────────────────────────────────────────────────────

/// Arities agree and the COMM still does not fire, so the answer must NOT be
/// "arity mismatch". Without this row a diagnosis hard-coded to
/// `ArityMismatch` would pass every row above.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shape_refusal_is_not_reported_as_an_arity_mismatch() {
    with_runtime("rest-diagnosis-shape-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@42 <- ch) { Nil } | ch!(7) }"#,
        )
        .await;
        assert_eq!(
            only(&sites, "7 against the literal pattern 42"),
            RestReason::ShapeOrGuardRefused {
                sent: 1,
                count: 1,
                admitted: vec![Admits::Exactly(1)],
            },
            "the arities agree (1 = 1), so the refusal is in the pattern"
        );
    })
    .await;
}

/// ★ The false-positive floor. A program whose COMM fires leaves nothing resting,
/// so the diagnosis must be EMPTY. A reason-finder that reported every channel
/// would pass every row above and this one is what refuses it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_program_that_fully_reduces_rests_nothing() {
    with_runtime("rest-diagnosis-reduces-", |mut runtime| async move {
        let sites = rest_of(
            &mut runtime,
            r#"new ch in { for (@a <- ch) { Nil } | ch!(1) }"#,
        )
        .await;
        assert!(
            sites.is_empty(),
            "the COMM fires, so nothing rests; got {sites:#?}"
        );
        let report = rest_diagnosis::report(&sites);
        assert!(
            report.contains("No resting data"),
            "the empty report must say so rather than being blank: {report}"
        );
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// ★★ THE CONSENSUS ROW — asking must not change the answer
// ─────────────────────────────────────────────────────────────────────────────

/// The one program the consensus row runs. ⚠ It uses a **quoted** channel and not
/// a `new`-bound one: an unforgeable name is a function of the deploy's random
/// seed, and the seed is the confound named on [`fixed_seed`].
const RESTING_SOURCE: &str = r#"for (@a, @b, @c <- @"ch") { Nil } | @"ch"!(1, 2)"#;

/// ⚠★ **The confound this row had to remove, stated so it is not re-introduced.**
///
/// `RhoRuntime::evaluate_with_term` seeds the deploy with
/// `Blake2b512Random::create_from_length(128)`, whose body is
/// `rand::thread_rng().fill(&mut bytes)` — **nondeterministic**. Two runs of the
/// same source therefore reach two different post-state roots on their own, and a
/// two-arm before/after comparison would report *"the diagnostic moved the state
/// hash"* when nothing of the sort had happened. The first draft of this row did
/// exactly that and went RED with two unequal roots.
///
/// So the row calls `evaluate` directly with a fixed seed **and** carries a
/// control arm, because removing a confound you have found is not the same as
/// showing there is none left.
fn fixed_seed() -> Blake2b512Random {
    Blake2b512Random::create_from_bytes(b"rest-diagnosis: one fixed seed for every arm")
}

/// Evaluate [`RESTING_SOURCE`] on a fresh runtime and return the post-state root.
/// When `ask`, interrogate the diagnosis first — through **both** entry points,
/// the pure analysis and the printer surface that emits through `tracing`.
async fn root_after(prefix: &str, ask: bool) -> Blake2b256Hash {
    with_runtime(prefix, |mut runtime| async move {
        let result = runtime
            .evaluate(
                RESTING_SOURCE,
                Cost::unsafe_max(),
                HashMap::new(),
                fixed_seed(),
            )
            .await
            .expect("the arm must evaluate");
        assert!(
            result.errors.is_empty(),
            "the arm must REST, not fail: {:?}",
            result.errors
        );

        if ask {
            let sites = rest_diagnosis::diagnose(&runtime.get_hot_changes().await);
            assert!(
                !sites.is_empty(),
                "★ FLOOR for this guard: the treatment must actually ASK something. An empty \
                 diagnosis would make the arms agree for the wrong reason."
            );
            let _ = rest_diagnosis::report(&sites);
            let printed = storage_printer::pretty_print_rest_diagnosis(&runtime).await;
            assert!(
                printed.contains("ARITY MISMATCH"),
                "the printer surface must have produced the diagnosis: {printed}"
            );
        }

        runtime.create_checkpoint().await.root
    })
    .await
}

/// ★★ **Three arms.** Two controls that do not ask, and one treatment that does.
///
/// The controls establish that the instrument is deterministic *at all* — which is
/// checked FIRST, so a nondeterministic harness fails as *"the harness"* and never
/// as *"the diagnostic is consensus-visible"*. Only then is the treatment compared.
///
/// This is the measurement behind `rest_diagnosis` §4: the diagnostic may exceed
/// upstream, and the semantics may not diverge. `HotStore::to_map` takes a read
/// lock and clones; its sibling `HotStore::get_data` takes a **write** lock and
/// inserts a history fill that `changes()` would then emit as a store action — so
/// *"it only reads"* is a property of the specific API and not of reading, and it
/// is measured here rather than argued.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn asking_why_a_term_rests_does_not_move_the_post_state_hash() {
    let control_one = root_after("rest-diagnosis-control-one-", false).await;
    let control_two = root_after("rest-diagnosis-control-two-", false).await;
    assert_eq!(
        control_one, control_two,
        "★ CONTROL: two runs that ask nothing must agree, or this guard cannot attribute a \
         difference to the asking. If this row fails, the harness is nondeterministic — see \
         `fixed_seed` — and the treatment row below proves nothing either way."
    );

    let treatment = root_after("rest-diagnosis-treatment-", true).await;
    assert_eq!(
        treatment, control_one,
        "★★ asking why a term rests moved the post-state root. The diagnostic is on the \
         consensus byte path and must not ship in this form."
    );
}

/// And the snapshot itself is unchanged by having been analysed — the finer-grained
/// twin of the row above, which fails closer to the cause if `to_map` ever gains a
/// fill.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn analysing_a_snapshot_does_not_change_the_next_snapshot() {
    with_runtime("rest-diagnosis-snapshot-", |mut runtime| async move {
        runtime
            .evaluate_with_term(r#"new ch in { for (@a, @b, @c <- ch) { Nil } | ch!(1, 2) }"#)
            .await
            .expect("must evaluate");

        let before = runtime.get_hot_changes().await;
        let sites = rest_diagnosis::diagnose(&before);
        assert!(
            !sites.is_empty(),
            "FLOOR: the analysis must have found something"
        );
        let _ = rest_diagnosis::report(&sites);
        let after = runtime.get_hot_changes().await;

        assert_eq!(
            before.keys().len(),
            after.keys().len(),
            "the analysed snapshot gained or lost a channel"
        );
        for (channels, row) in &before {
            let other = after
                .get(channels)
                .expect("a channel present before the analysis must be present after it");
            assert_eq!(
                row.data.len(),
                other.data.len(),
                "the datum count at a channel moved"
            );
            assert_eq!(
                row.wks.len(),
                other.wks.len(),
                "the continuation count at a channel moved"
            );
        }
    })
    .await;
}
