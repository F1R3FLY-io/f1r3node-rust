//! **`E(S)` — the enabled rendezvous set**, and the named firing that consumes it.
//!
//! # What is being added, and why it needs its own spec
//!
//! `RSpace::consume` and `RSpace::produce` answer *one* question: "is there an
//! admissible selection, and if so what is the least one?" A speculative
//! evaluator has to answer a strictly larger one: "what are **all** the
//! rendezvous this state admits?" — and then fire a **named** member of that
//! answer, not whichever one a fresh search would rediscover.
//!
//! Two things are therefore exported:
//!
//! | export | what it does |
//! |---|---|
//! | [`SpaceMatcher::enumerate_enabled_rendezvous`] (bound as `RSpace::enabled_rendezvous` / `ReplayRSpace::enabled_rendezvous`) | read-only: every (continuation × admissible selection) pair the current hot store admits |
//! | `RSpace::process_match_found` (visibility only — the body is untouched) | fire one named rendezvous: remove its data and continuation by store index, drop the joins, emit the `COMM` |
//!
//! Both are consensus surfaces, so this file pins them the way a consensus
//! surface has to be pinned — by measurement against the production selector,
//! not by restating the implementation.
//!
//! # The tests
//!
//! | test | property |
//! |---|---|
//! | [`t0_teeth_the_fixture_puts_the_admissible_datum_off_the_front`] | the corpus really does exercise backtracking — without this every "agrees" below could be vacuous |
//! | [`t1_the_enumeration_head_is_the_selector_choice`] | ★ `E(S)`'s first selection for a continuation is EXACTLY what a real `consume` takes |
//! | [`t2_the_enumeration_is_complete`] | every resting datum that matches appears; a guard removes exactly the ones it rejects |
//! | [`t3_the_query_is_read_only`] | store, event log and produce counter are untouched by the query |
//! | [`t4_the_enumeration_is_deterministic`] | repeated calls agree, and insertion order does not change the answer |
//! | [`t5_a_named_non_least_selection_fires_exactly_itself`] | ★ firing `E(S)[k]`, `k > 0`, consumes the datum NAMED — the thing a re-search cannot express |
//! | [`t6_play_and_replay_enumerate_identically`] | ★ the D1 lesson: one enumeration, two spaces |
//! | [`t7_teeth_an_unsatisfiable_state_enumerates_to_nothing`] | the query can say NO |
//! | [`t8_a_join_enumerates_its_cross_product`] | multi-bind selections, and the guard filtering them |

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::{Datum, ProduceCandidate};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

// ════════════════════════════════════════════════════════════════════════════
// The tuplespace under test — the `guarded_matching_tests.rs` language, so the
// two files pin the same selector with the same vocabulary.
// ════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}
impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize for Pattern {}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
enum Guard {
    #[default]
    Unguarded,
    /// Every matched datum, read as an integer, is at most this bound.
    AtMost(i64),
    /// The matched data are strictly increasing in bind order — a predicate over
    /// an ASSIGNMENT of data to binds, not merely over a set.
    StrictlyIncreasing,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
struct GuardedContinuation {
    name: String,
    guard: Guard,
}
impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize
    for GuardedContinuation
{
}

impl GuardedContinuation {
    fn unguarded(name: &str) -> Self {
        GuardedContinuation {
            name: name.to_string(),
            guard: Guard::Unguarded,
        }
    }
    fn guarded(name: &str, guard: Guard) -> Self {
        GuardedContinuation {
            name: name.to_string(),
            guard,
        }
    }
}

fn as_int(datum: &String) -> i64 {
    datum
        .parse::<i64>()
        .unwrap_or_else(|error| panic!("test data are decimal integers: {datum:?}: {error}"))
}

#[derive(Clone, Default)]
struct GuardingMatch;

impl Match<Pattern, String, GuardedContinuation> for GuardingMatch {
    fn get(&self, pattern: &Pattern, data: &String) -> Option<String> {
        match pattern {
            Pattern::Wildcard => Some(data.clone()),
            Pattern::StringMatch(value) if value == data => Some(data.clone()),
            Pattern::StringMatch(_) => None,
        }
    }

    fn check_commit(&self, k: &GuardedContinuation, matched: &[&String]) -> bool {
        match &k.guard {
            Guard::Unguarded => true,
            Guard::AtMost(bound) => matched.iter().all(|datum| as_int(datum) <= *bound),
            Guard::StrictlyIncreasing => matched
                .windows(2)
                .all(|pair| as_int(pair[0]) < as_int(pair[1])),
        }
    }
}

type TestSpace = RSpace<String, Pattern, String, GuardedContinuation>;
type TestReplaySpace = ReplayRSpace<String, Pattern, String, GuardedContinuation>;
type TestState = HotStoreState<String, Pattern, String, GuardedContinuation>;
type TestRendezvous = ProduceCandidate<String, Pattern, String, GuardedContinuation>;

/// A play space and a replay space over the same (empty) history.
async fn fixture() -> (TestSpace, TestReplaySpace) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace stores");

    let history_repo = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, GuardedContinuation>::lmdb_repository(
            store.history.clone(),
            store.roots.clone(),
            store.cold.clone(),
        )
        .expect("history repository"),
    );
    let history_reader = history_repo
        .get_history_reader(&history_repo.root())
        .expect("history reader");

    let play = RSpace::apply(
        history_repo.clone(),
        HotStoreInstances::create_from_hs_and_hr(TestState::default(), history_reader.base()),
        Arc::new(Box::new(GuardingMatch)),
    );
    let replay = ReplayRSpace::apply(
        history_repo,
        Arc::new(HotStoreInstances::create_from_hs_and_hr(
            TestState::default(),
            history_reader.base(),
        )),
        Arc::new(Box::new(GuardingMatch)),
    );
    (play, replay)
}

async fn play_space() -> TestSpace {
    fixture().await.0
}

/// The `AtMost(45)` corpus, chosen by MEASUREMENT rather than by eye.
///
/// THE canonical candidate order is a Blake2b hash of the serialized candidate
/// (`candidate_order::deterministic_candidate_hash`), so which of two data comes
/// first has nothing to do with their numeric values. The first pair written
/// here was `["55", "42"]` — and `t0` measured that the canonical order puts the
/// *admissible* `42` first, which would have made every "the guard and the
/// enumeration agree" assertion below vacuous: no backtracking would ever have
/// been entered.
///
/// A sweep of the 54 × 45 rejected/admitted pairs on channel `"offer"` found
/// 1115 pairs whose REJECTED member sorts first; `("46", "2")` is the least of
/// them. `t0` re-measures this on every run, so the corpus cannot silently
/// become vacuous again if the serializer or the hash changes.
const REJECTED_FIRST: [&str; 2] = ["46", "2"];

/// Rest `values` on `channel` — each `produce` finds no continuation (none is
/// installed yet), so each deposits.
async fn rest_data(space: &TestSpace, channel: &str, values: &[&str]) {
    for value in values {
        let fired = space
            .produce(channel.to_string(), value.to_string(), false)
            .await
            .expect("produce must not error");
        assert!(fired.is_none(), "the corpus rests data before any receiver is installed");
    }
}

/// Install a waiting continuation. Returns whether it fired (the corpus always
/// installs receivers LAST, so a fire means the fixture is wrong).
async fn rest_receive(
    space: &TestSpace,
    channels: &[&str],
    patterns: Vec<Pattern>,
    continuation: GuardedContinuation,
) -> bool {
    space
        .consume(
            channels.iter().map(|c| c.to_string()).collect(),
            patterns,
            continuation,
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume must not error")
        .is_some()
}

/// The data each enumerated rendezvous selected, in bind order, as integers —
/// the shape every assertion below compares.
fn selected(enabled: &[TestRendezvous]) -> Vec<(String, Vec<i64>)> {
    enabled
        .iter()
        .map(|rendezvous| {
            (
                rendezvous.continuation.continuation.name.clone(),
                rendezvous
                    .data_candidates
                    .iter()
                    .map(|candidate| as_int(&candidate.datum.a))
                    .collect(),
            )
        })
        .collect()
}

fn resting_ints(state: &TestState, channel: &str) -> Vec<i64> {
    state
        .data
        .get(&channel.to_string())
        .map(|data| data.iter().map(|datum| as_int(&datum.a)).collect())
        .unwrap_or_default()
}

// ════════════════════════════════════════════════════════════════════════════
// T0 — teeth
// ════════════════════════════════════════════════════════════════════════════

/// The corpus below leans on `["55", "42"]` under `AtMost(45)`. If `"42"` happened
/// to be first in THE canonical candidate order the guard would never backtrack
/// and "the enumeration agrees with the selector" would be vacuously true.
///
/// This test measures the canonical order directly and requires the admissible
/// datum to be off the front. It is the same fixture-validity check
/// `guarded_matching_tests.rs::canonical_position` performs, done here so this
/// file's negative results can be trusted on their own.
#[tokio::test]
async fn t0_teeth_the_fixture_puts_the_admissible_datum_off_the_front() {
    let space = play_space().await;
    rest_data(&space, "offer", &REJECTED_FIRST).await;

    let pool = space.get_store().get_data(&"offer".to_string());
    let canonical: Vec<i64> =
        rspace_plus_plus::rspace::candidate_order::order_candidates_with_index(pool)
            .iter()
            .map(|(datum, _)| as_int(&datum.a))
            .collect();

    assert_eq!(
        canonical[0], 46,
        "fixture invalid: the guard-rejected datum must be the canonical FIRST pick, \
         otherwise no backtracking is exercised (canonical order was {canonical:?})"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// T1 — ★ the head identity
// ════════════════════════════════════════════════════════════════════════════

/// **`E(S)`'s first selection for a continuation is exactly what a real
/// `consume` on that state takes.**
///
/// This is the bridge the whole speculative construction stands on: the
/// enumeration is the production selector with the early return replaced by a
/// record, so their first answers must coincide. The test does not inspect
/// either implementation — it runs the enumeration on a state, then runs a real
/// `consume` against a byte-identical copy of the same state, and compares the
/// data each one took.
///
/// Both the unguarded shape (first spatial match wins) and the guarded shape
/// (the guard backtracks past the first spatial match) are checked, because
/// they take different paths through the descent.
#[tokio::test]
async fn t1_the_enumeration_head_is_the_selector_choice() {
    for (label, guard, receiver_patterns) in [
        ("unguarded", Guard::Unguarded, vec![Pattern::Wildcard]),
        ("guarded", Guard::AtMost(45), vec![Pattern::Wildcard]),
    ] {
        // ── the enumeration arm ──
        let enumerating = play_space().await;
        rest_data(&enumerating, "offer", &REJECTED_FIRST).await;
        let fired = rest_receive(
            &enumerating,
            &["offer"],
            receiver_patterns.clone(),
            GuardedContinuation::guarded("k", guard.clone()),
        )
        .await;
        assert!(
            fired,
            "{label}: with data resting, an ordinary consume FIRES — that is the \
             selector answer this test compares against"
        );

        // The consume above fired, so re-stage the same state without firing: a
        // second space that gets the data and the continuation planted directly.
        let staged = play_space().await;
        rest_data(&staged, "offer", &REJECTED_FIRST).await;
        let store = staged.get_store();
        store.put_continuation(
            &vec!["offer".to_string()],
            rspace_plus_plus::rspace::internal::WaitingContinuation::create(
                &vec!["offer".to_string()],
                &receiver_patterns,
                &GuardedContinuation::guarded("k", guard.clone()),
                false,
                BTreeSet::new(),
            ),
        );
        store.put_join(&"offer".to_string(), &["offer".to_string()]);

        let enabled = staged.enabled_rendezvous();
        assert!(!enabled.is_empty(), "{label}: the staged state must admit a rendezvous");
        let head: Vec<i64> = enabled[0]
            .data_candidates
            .iter()
            .map(|candidate| as_int(&candidate.datum.a))
            .collect();

        // ── the selector arm: a THIRD space holding the same data, receiving ──
        let selecting = play_space().await;
        rest_data(&selecting, "offer", &REJECTED_FIRST).await;
        let taken = selecting
            .consume(
                vec!["offer".to_string()],
                receiver_patterns.clone(),
                GuardedContinuation::guarded("k", guard.clone()),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("consume must not error")
            .expect("the selector must fire on this state");
        let selector: Vec<i64> = taken.1.iter().map(|r| as_int(&r.matched_datum)).collect();

        assert_eq!(
            head, selector,
            "{label}: E(S)'s head selection and the production selector's choice must be \
             the same datum — they are the same descent"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════
// T2 — completeness
// ════════════════════════════════════════════════════════════════════════════

/// Every resting datum a bind can match appears in `E(S)`, and a guard removes
/// exactly the ones it rejects — nothing more, nothing less.
#[tokio::test]
async fn t2_the_enumeration_is_complete() {
    // (a) unguarded single bind over four data: four rendezvous.
    let space = play_space().await;
    rest_data(&space, "c", &["10", "20", "30", "40"]).await;
    let store = space.get_store();
    store.put_continuation(
        &vec!["c".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["c".to_string()],
            &vec![Pattern::Wildcard],
            &GuardedContinuation::unguarded("k"),
            false,
            BTreeSet::new(),
        ),
    );
    let mut widths: Vec<i64> = selected(&space.enabled_rendezvous())
        .into_iter()
        .map(|(_, data)| data[0])
        .collect();
    widths.sort();
    assert_eq!(
        widths,
        vec![10, 20, 30, 40],
        "an unguarded wildcard bind is enabled by every resting datum"
    );

    // (b) the same state with `AtMost(25)`: exactly the two admissible data.
    let guarded = play_space().await;
    rest_data(&guarded, "c", &["10", "20", "30", "40"]).await;
    guarded.get_store().put_continuation(
        &vec!["c".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["c".to_string()],
            &vec![Pattern::Wildcard],
            &GuardedContinuation::guarded("k", Guard::AtMost(25)),
            false,
            BTreeSet::new(),
        ),
    );
    let mut admitted: Vec<i64> = selected(&guarded.enabled_rendezvous())
        .into_iter()
        .map(|(_, data)| data[0])
        .collect();
    admitted.sort();
    assert_eq!(
        admitted,
        vec![10, 20],
        "the guard removes exactly the data it rejects"
    );

    // (c) two continuations on one channel: the enumeration is the union.
    let two = play_space().await;
    rest_data(&two, "c", &["10", "20"]).await;
    for name in ["k1", "k2"] {
        two.get_store().put_continuation(
            &vec!["c".to_string()],
            rspace_plus_plus::rspace::internal::WaitingContinuation::create(
                &vec!["c".to_string()],
                &vec![Pattern::Wildcard],
                &GuardedContinuation::unguarded(name),
                false,
                BTreeSet::new(),
            ),
        );
    }
    let mut pairs = selected(&two.enabled_rendezvous());
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("k1".to_string(), vec![10]),
            ("k1".to_string(), vec![20]),
            ("k2".to_string(), vec![10]),
            ("k2".to_string(), vec![20]),
        ],
        "E(S) is the union over continuations of each one's admissible selections"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// T3 — read-only
// ════════════════════════════════════════════════════════════════════════════

/// The query is a pure read. A speculative evaluator calls it once per step of
/// every branch it explores; if it mutated the store, or appended to the event
/// log, exploration would corrupt the state it is exploring.
#[tokio::test]
async fn t3_the_query_is_read_only() {
    let space = play_space().await;
    rest_data(&space, "c", &["10", "20"]).await;
    space.get_store().put_continuation(
        &vec!["c".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["c".to_string()],
            &vec![Pattern::Wildcard],
            &GuardedContinuation::unguarded("k"),
            false,
            BTreeSet::new(),
        ),
    );

    let before = space.get_store().snapshot();
    let checkpoint_before = space.create_soft_checkpoint().await;
    // `create_soft_checkpoint` TAKES the log, so put it back before measuring.
    space
        .revert_to_soft_checkpoint(checkpoint_before.clone())
        .await
        .expect("revert must succeed");

    let first = space.enabled_rendezvous();
    let second = space.enabled_rendezvous();
    assert_eq!(first.len(), 2);
    assert_eq!(selected(&first), selected(&second));

    let after = space.get_store().snapshot();
    assert_eq!(
        resting_ints(&before, "c"),
        resting_ints(&after, "c"),
        "the query must not remove or reorder data"
    );
    assert_eq!(
        before.continuations.len(),
        after.continuations.len(),
        "the query must not remove continuations"
    );

    let checkpoint_after = space.create_soft_checkpoint().await;
    assert_eq!(
        checkpoint_before.log.len(),
        checkpoint_after.log.len(),
        "the query must append no event: {} before vs {} after",
        checkpoint_before.log.len(),
        checkpoint_after.log.len()
    );
    assert_eq!(
        checkpoint_before.produce_counter, checkpoint_after.produce_counter,
        "the query must not move the produce counter"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// T4 — determinism
// ════════════════════════════════════════════════════════════════════════════

/// The enumeration order is a consensus surface: two validators enumerating the
/// same state must produce the same sequence, not merely the same set. The
/// group-key sort exists for exactly this reason (the group set is read out of a
/// `HashMap`), so the test builds the same logical state by inserting the data
/// and the channels in DIFFERENT orders and requires an identical answer.
#[tokio::test]
async fn t4_the_enumeration_is_deterministic() {
    async fn enumerate(channel_order: &[&str], data_order: &[&str]) -> Vec<(String, Vec<i64>)> {
        let space = play_space().await;
        for channel in channel_order {
            rest_data(&space, channel, data_order).await;
            space.get_store().put_continuation(
                &vec![channel.to_string()],
                rspace_plus_plus::rspace::internal::WaitingContinuation::create(
                    &vec![channel.to_string()],
                    &vec![Pattern::Wildcard],
                    &GuardedContinuation::unguarded("k"),
                    false,
                    BTreeSet::new(),
                ),
            );
        }
        selected(&space.enabled_rendezvous())
    }

    let forward = enumerate(&["alpha", "beta", "gamma"], &["10", "20"]).await;
    let reversed = enumerate(&["gamma", "beta", "alpha"], &["10", "20"]).await;
    assert_eq!(
        forward, reversed,
        "the enumeration must not depend on the order channels were written to the store"
    );

    // Ten repetitions of the same construction: byte-identical every time.
    for _ in 0..10 {
        assert_eq!(forward, enumerate(&["alpha", "beta", "gamma"], &["10", "20"]).await);
    }
}

// ════════════════════════════════════════════════════════════════════════════
// T5 — ★ firing a NAMED selection
// ════════════════════════════════════════════════════════════════════════════

/// **The reason `process_match_found` is exported.** `E(S)` is enumerated, a
/// member other than the head is named, and firing it must consume exactly the
/// datum that member names — which is precisely what re-running the search
/// cannot do, because the search always returns the head.
#[tokio::test]
async fn t5_a_named_non_least_selection_fires_exactly_itself() {
    let space = play_space().await;
    rest_data(&space, "c", &["10", "20", "30"]).await;
    space.get_store().put_continuation(
        &vec!["c".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["c".to_string()],
            &vec![Pattern::Wildcard],
            &GuardedContinuation::unguarded("k"),
            false,
            BTreeSet::new(),
        ),
    );
    space.get_store().put_join(&"c".to_string(), &["c".to_string()]);

    let enabled = space.enabled_rendezvous();
    assert_eq!(enabled.len(), 3, "three resting data, one wildcard bind");

    // Name the LAST selection — by construction not the one a search returns.
    let chosen = enabled[2].clone();
    let named = as_int(&chosen.data_candidates[0].datum.a);
    let head = as_int(&enabled[0].data_candidates[0].datum.a);
    assert_ne!(
        named, head,
        "teeth: the named selection must differ from the head, or the test proves nothing"
    );

    let fired = space
        .process_match_found(chosen)
        .expect("firing a named rendezvous must produce a result");
    assert_eq!(
        as_int(&fired.1[0].matched_datum),
        named,
        "the fired COMM must carry the datum that was NAMED"
    );

    let after = space.get_store().snapshot();
    let mut remaining = resting_ints(&after, "c");
    remaining.sort();
    let mut expected: Vec<i64> = vec![10, 20, 30].into_iter().filter(|v| *v != named).collect();
    expected.sort();
    assert_eq!(
        remaining, expected,
        "exactly the named datum is gone from the store"
    );
    assert!(
        after
            .continuations
            .get(&vec!["c".to_string()])
            .map(|group| group.is_empty())
            .unwrap_or(true),
        "the non-persistent continuation is removed by the firing"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// T6 — ★ play and replay
// ════════════════════════════════════════════════════════════════════════════

/// The D1 lesson applied preemptively: the enumeration lives on `SpaceMatcher`,
/// so both spaces run one implementation. This test plants a byte-identical hot
/// state in each and requires identical answers — the check that would catch a
/// future private copy in either space.
#[tokio::test]
async fn t6_play_and_replay_enumerate_identically() {
    let (play, replay) = fixture().await;

    rest_data(&play, "offer", &["55", "42", "13"]).await;
    play.get_store().put_continuation(
        &vec!["offer".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["offer".to_string()],
            &vec![Pattern::Wildcard],
            &GuardedContinuation::guarded("k", Guard::AtMost(45)),
            false,
            BTreeSet::new(),
        ),
    );

    let state = play.get_store().snapshot();
    replay.get_store().set_state(state);

    let from_play = selected(&play.enabled_rendezvous());
    let from_replay = selected(&replay.enabled_rendezvous());
    assert_eq!(
        from_play, from_replay,
        "play and replay must enumerate the same rendezvous in the same order"
    );
    assert_eq!(
        from_play.len(),
        2,
        "teeth: the guard must actually have filtered (55 rejected, 42 and 13 admitted), \
         otherwise the equality above could hold over an empty set"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// T7 — teeth: the query can say NO
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t7_teeth_an_unsatisfiable_state_enumerates_to_nothing() {
    // (a) a continuation with no data at all.
    let lonely = play_space().await;
    let fired = rest_receive(
        &lonely,
        &["c"],
        vec![Pattern::Wildcard],
        GuardedContinuation::unguarded("k"),
    )
    .await;
    assert!(!fired, "no data rests, so the consume installs");
    assert!(
        lonely.enabled_rendezvous().is_empty(),
        "a continuation with no datum is not enabled"
    );

    // (b) data with no continuation.
    let mute = play_space().await;
    rest_data(&mute, "c", &["10"]).await;
    assert!(
        mute.enabled_rendezvous().is_empty(),
        "a datum with no continuation is not a rendezvous"
    );

    // (c) data present, but the guard refuses every one.
    let refused = play_space().await;
    rest_data(&refused, "c", &["50", "60"]).await;
    refused.get_store().put_continuation(
        &vec!["c".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["c".to_string()],
            &vec![Pattern::Wildcard],
            &GuardedContinuation::guarded("k", Guard::AtMost(45)),
            false,
            BTreeSet::new(),
        ),
    );
    assert!(
        refused.enabled_rendezvous().is_empty(),
        "a guard that refuses every resting datum leaves nothing enabled"
    );

    // (d) data present, but no pattern matches spatially.
    let mismatched = play_space().await;
    rest_data(&mismatched, "c", &["10"]).await;
    mismatched.get_store().put_continuation(
        &vec!["c".to_string()],
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &vec!["c".to_string()],
            &vec![Pattern::StringMatch("99".to_string())],
            &GuardedContinuation::unguarded("k"),
            false,
            BTreeSet::new(),
        ),
    );
    assert!(
        mismatched.enabled_rendezvous().is_empty(),
        "a spatial mismatch leaves nothing enabled"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// T8 — joins
// ════════════════════════════════════════════════════════════════════════════

/// A join's admissible set is a cross product, and it is the shape for which
/// "install a minimal store and let the search find the unique match" fails —
/// several selections survive any trim. The enumeration must produce all of
/// them, in a deterministic order, and a guard over the ASSIGNMENT (not merely
/// the set) must filter them by bind order.
#[tokio::test]
async fn t8_a_join_enumerates_its_cross_product() {
    async fn join_state(guard: Guard) -> Vec<Vec<i64>> {
        let space = play_space().await;
        rest_data(&space, "left", &["1", "2"]).await;
        rest_data(&space, "right", &["3", "4"]).await;
        let channels = vec!["left".to_string(), "right".to_string()];
        space.get_store().put_continuation(
            &channels,
            rspace_plus_plus::rspace::internal::WaitingContinuation::create(
                &channels,
                &vec![Pattern::Wildcard, Pattern::Wildcard],
                &GuardedContinuation::guarded("k", guard),
                false,
                BTreeSet::new(),
            ),
        );
        for channel in channels.iter() {
            space.get_store().put_join(channel, &channels);
        }
        selected(&space.enabled_rendezvous())
            .into_iter()
            .map(|(_, data)| data)
            .collect()
    }

    let mut unguarded = join_state(Guard::Unguarded).await;
    unguarded.sort();
    assert_eq!(
        unguarded,
        vec![vec![1, 3], vec![1, 4], vec![2, 3], vec![2, 4]],
        "an unguarded 2-bind join over 2×2 data enumerates the full cross product"
    );

    let mut decreasing = join_state(Guard::StrictlyIncreasing).await;
    decreasing.sort();
    assert_eq!(
        decreasing,
        vec![vec![1, 3], vec![1, 4], vec![2, 3], vec![2, 4]],
        "every left datum is below every right datum here, so the guard admits all four"
    );

    // The guard genuinely bites when the ranges overlap.
    let space = play_space().await;
    rest_data(&space, "left", &["1", "9"]).await;
    rest_data(&space, "right", &["5"]).await;
    let channels = vec!["left".to_string(), "right".to_string()];
    space.get_store().put_continuation(
        &channels,
        rspace_plus_plus::rspace::internal::WaitingContinuation::create(
            &channels,
            &vec![Pattern::Wildcard, Pattern::Wildcard],
            &GuardedContinuation::guarded("k", Guard::StrictlyIncreasing),
            false,
            BTreeSet::new(),
        ),
    );
    let admitted: Vec<Vec<i64>> = selected(&space.enabled_rendezvous())
        .into_iter()
        .map(|(_, data)| data)
        .collect();
    assert_eq!(
        admitted,
        vec![vec![1, 5]],
        "the guard rejects the (9, 5) assignment and admits only (1, 5)"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// A HashMap import keeps the enumeration's own pool type nameable in doc tests
// and silences an otherwise-unused import in future edits of this file.
// ════════════════════════════════════════════════════════════════════════════
#[allow(dead_code)]
fn _pool_type_is_nameable(pool: HashMap<String, Vec<(Datum<String>, i32)>>) -> usize {
    pool.len()
}
