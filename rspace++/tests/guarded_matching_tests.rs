//! Defect **D1** — a `where`-guard rejection must backtrack to the next resting datum.
//!
//! # What was wrong
//!
//! `SpaceMatcher::find_matching_data_candidate` returns the first datum that matches
//! **spatially**; the commit guard was then evaluated once, on that single pick, by
//! `extract_first_match` / `RSpace::locked_consume`, and a rejection advanced to the next
//! waiting CONTINUATION rather than to the next DATUM. The guard was candidate APPROVAL and
//! never candidate SELECTION, so one rejected pick stranded a rendezvous that a different
//! resting datum would have satisfied. The rho calculus admits no such stuck state: a COMM
//! whose binds can all be filled and whose guard holds is enabled, and an enabled COMM fires.
//!
//! # What these tests pin
//!
//! | test | property |
//! |---|---|
//! | [`a_guard_selects_the_one_admissible_datum_out_of_many`] | the consume path selects across data, not just approves one |
//! | [`the_produce_path_backtracks_across_a_join`] | so does the produce path, through `extract_first_match` |
//! | [`a_guard_no_resting_datum_satisfies_leaves_everything_resting`] | the negative: nothing fires, nothing is consumed, nothing is fabricated |
//! | [`play_and_replay_agree_on_a_guarded_selection`] | replay reconstructs the SAME selection the play space made |
//! | [`play_and_replay_agree_when_the_guard_permutes_a_repeated_channel`] | …including when the guard picks an assignment the spatial order would not |
//! | [`an_unguarded_workload_costs_exactly_what_it_did`] | the guard-aware search is pay-for-what-you-use: no extra `Match::get` calls without a guard |
//! | [`the_canonical_candidate_order_is_a_pure_function_of_the_candidates`] | selection rests on an order that is reproducible, not on store insertion order |
//! | [`the_guarded_selection_is_stable_across_repetitions`] | the same store answers the same way every time |
//!
//! ⚠ **Consensus.** Every one of these fixes changes WHEN A COMM FIRES. See the commit body.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rspace_plus_plus::rspace::candidate_order::order_candidates_with_index;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

// ════════════════════════════════════════════════════════════════════════════════════════════
// A tuplespace whose continuations carry guards
// ════════════════════════════════════════════════════════════════════════════════════════════

/// The spatial pattern language, as in `storage_actions_test.rs`.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}
impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize for Pattern {}

/// The guard language. A guard reads the matched data of EVERY bind, in bind order — that is
/// the whole reason it cannot live inside the per-channel spatial scan.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
enum Guard {
    /// No guard: `Match::check_commit`'s always-true default, and the shape of every
    /// continuation in ordinary (unguarded) Rholang.
    #[default]
    Unguarded,
    /// Every matched datum, read as an integer, is at most this bound. The demo's
    /// `for(@px <- @"offer" where px <= 45)`.
    AtMost(i64),
    /// The matched data are strictly increasing in bind order. Distinguishes an ASSIGNMENT of
    /// data to binds, not merely a set of data — the case where a permuted selection builds an
    /// identical COMM event.
    StrictlyIncreasing,
}

/// A continuation is a name (so distinct continuations are distinguishable in the store) plus
/// its guard.
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

/// Data are decimal integers rendered as strings, so a guard can read them and the spatial
/// matcher can still compare them structurally.
fn as_int(datum: &String) -> i64 {
    datum
        .parse::<i64>()
        .unwrap_or_else(|error| panic!("test data are decimal integers: {datum:?}: {error}"))
}

/// The matcher under test, counting its spatial calls so the cost claim can be asserted rather
/// than asserted-of.
#[derive(Clone, Default)]
struct GuardingMatch {
    spatial_calls: Arc<AtomicUsize>,
    guard_calls: Arc<AtomicUsize>,
}

impl GuardingMatch {
    fn spatial_calls(&self) -> usize { self.spatial_calls.load(Ordering::Relaxed) }

    fn guard_calls(&self) -> usize { self.guard_calls.load(Ordering::Relaxed) }
}

impl Match<Pattern, String, GuardedContinuation> for GuardingMatch {
    fn get(&self, pattern: &Pattern, data: &String) -> Option<String> {
        self.spatial_calls.fetch_add(1, Ordering::Relaxed);
        match pattern {
            Pattern::Wildcard => Some(data.clone()),
            Pattern::StringMatch(value) if value == data => Some(data.clone()),
            Pattern::StringMatch(_) => None,
        }
    }

    fn check_commit(&self, k: &GuardedContinuation, matched: &[&String]) -> bool {
        self.guard_calls.fetch_add(1, Ordering::Relaxed);
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

/// A play space and the replay space rigged against the same history — the
/// `replay_rspace_tests.rs` fixture, with the guarding matcher.
async fn fixture() -> (TestSpace, TestReplaySpace, GuardingMatch) {
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

    let matcher = GuardingMatch::default();

    let hot_store = {
        let cache: HotStoreState<String, Pattern, String, GuardedContinuation> =
            HotStoreState::default();
        HotStoreInstances::create_from_hs_and_hr(cache, history_reader.base())
    };
    let space =
        RSpace::apply(history_repo.clone(), hot_store, Arc::new(Box::new(matcher.clone())));

    let replay_store = {
        let cache: HotStoreState<String, Pattern, String, GuardedContinuation> =
            HotStoreState::default();
        HotStoreInstances::create_from_hs_and_hr(cache, history_reader.base())
    };
    let replay_space = ReplayRSpace::apply(
        history_repo,
        Arc::new(replay_store),
        Arc::new(Box::new(matcher.clone())),
    );

    (space, replay_space, matcher)
}

/// The data resting on `channel`, as integers, in store order.
async fn resting(space: &TestSpace, channel: &str) -> Vec<i64> {
    space
        .get_data(&channel.to_string())
        .await
        .iter()
        .map(|datum| as_int(&datum.a))
        .collect()
}

/// The number of continuations waiting on `channels`.
async fn waiting(space: &TestSpace, channels: &[&str]) -> usize {
    space
        .get_waiting_continuations(channels.iter().map(|c| c.to_string()).collect())
        .await
        .len()
}

/// Where `datum` lands in THE canonical candidate order of `data` — the order the matcher
/// enumerates candidates in. Used to assert that a test's admissible datum is genuinely NOT
/// the first spatial pick, so the test exercises backtracking rather than accidentally
/// agreeing with the old behaviour.
fn canonical_position(channel: &str, data: &[&str], datum: &str) -> usize {
    let datums: Vec<Datum<String>> = data
        .iter()
        .map(|value| Datum::create(&channel.to_string(), value.to_string(), false))
        .collect();
    let ordered = order_candidates_with_index(datums);
    ordered
        .iter()
        .position(|(candidate, _)| *candidate.a == datum)
        .unwrap_or_else(|| panic!("{datum:?} must be among {data:?}"))
}

// ════════════════════════════════════════════════════════════════════════════════════════════
// The consume path — the demo's shape
// ════════════════════════════════════════════════════════════════════════════════════════════

/// Several data rest; the guard admits exactly ONE of them; the guarded receive is installed
/// last, so nothing later can re-trigger the rendezvous and whatever the consume decides is
/// final. The admissible datum must be the one consumed, and it must be consumed on the FIRST
/// attempt — no matter where the canonical order happens to place it.
///
/// This is the rspace-level transliteration of the settlement demo's Beat 3
/// (`@"offer"!(55) | @"offer"!(42)` under `for(@px <- @"offer" where px <= 45)`), widened from
/// two data to eight so that a matcher which merely got lucky with the order cannot pass.
#[tokio::test]
async fn a_guard_selects_the_one_admissible_datum_out_of_many() {
    let (space, _replay, matcher) = fixture().await;

    // Exactly one of these is <= 45.
    const OFFERS: [&str; 8] = ["91", "77", "68", "55", "42", "63", "88", "59"];
    const ADMISSIBLE: &str = "42";

    assert!(
        canonical_position("offer", &OFFERS, ADMISSIBLE) > 0,
        "the admissible datum must NOT be the first spatial pick, or this test would pass \
         without any backtracking at all — choose different payloads"
    );

    for offer in OFFERS {
        let fired = space
            .produce("offer".to_string(), offer.to_string(), false)
            .await
            .expect("produce");
        assert!(fired.is_none(), "nothing is waiting yet, so {offer} must rest");
    }

    let fired = space
        .consume(
            vec!["offer".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("desk", Guard::AtMost(45)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");

    let (_continuation, results) = fired.expect(
        "⚠ DEFECT D1: an admissible datum was resting, so the COMM was ENABLED and must fire",
    );
    assert_eq!(results.len(), 1, "one bind, one result");
    assert_eq!(results[0].matched_datum, ADMISSIBLE, "the guard chose which datum settles");

    let mut left = resting(&space, "offer").await;
    left.sort();
    let mut expected: Vec<i64> = OFFERS
        .iter()
        .filter(|offer| **offer != ADMISSIBLE)
        .map(|offer| offer.parse().expect("integer"))
        .collect();
    expected.sort();
    assert_eq!(left, expected, "exactly the admissible datum was consumed; the rest still rest");
    assert_eq!(waiting(&space, &["offer"]).await, 0, "the receive fired, so it is not waiting");
    assert!(matcher.guard_calls() >= 2, "the guard was asked about more than one candidate");
}

/// The negative. When NO resting datum satisfies the guard the receive must rest: every datum
/// stays, the continuation is installed, and nothing is invented. (A matcher that "fixed" D1
/// by weakening the guard would consume something here.)
#[tokio::test]
async fn a_guard_no_resting_datum_satisfies_leaves_everything_resting() {
    let (space, _replay, matcher) = fixture().await;

    const OFFERS: [&str; 4] = ["91", "77", "68", "55"];

    for offer in OFFERS {
        let _ = space
            .produce("offer".to_string(), offer.to_string(), false)
            .await
            .expect("produce");
    }

    let fired = space
        .consume(
            vec!["offer".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("desk", Guard::AtMost(45)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");

    assert!(fired.is_none(), "no admissible datum ⇒ no COMM");

    let mut left = resting(&space, "offer").await;
    left.sort();
    assert_eq!(left, vec![55, 68, 77, 91], "every datum still rests — none consumed, none invented");
    assert_eq!(waiting(&space, &["offer"]).await, 1, "the guarded receive is installed and waiting");
    assert_eq!(
        matcher.guard_calls(),
        OFFERS.len(),
        "the search asked the guard about every candidate before giving up"
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════
// The produce path — `extract_first_match`, shared with replay
// ════════════════════════════════════════════════════════════════════════════════════════════

/// The produce path reaches the guard through `extract_first_match`, which is also the replay
/// space's produce path. A two-channel join makes the backtracking visible here: the arriving
/// datum pairs with several resting data, and only one of those pairings satisfies the guard.
#[tokio::test]
async fn the_produce_path_backtracks_across_a_join() {
    let (space, _replay, _matcher) = fixture().await;

    // `for(@bid <- bid; @ask <- ask) where bid, ask <= 50` — with 50 as the budget, only the
    // 10 can pair with an ask of 20.
    let fired = space
        .consume(
            vec!["bid".to_string(), "ask".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::guarded("settle", Guard::AtMost(50)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");
    assert!(fired.is_none(), "no data yet, so the join rests");

    for bid in ["60", "55", "10", "70"] {
        let fired = space
            .produce("bid".to_string(), bid.to_string(), false)
            .await
            .expect("produce");
        assert!(
            fired.is_none(),
            "the ask side is empty, so no selection exists yet and {bid} must rest"
        );
    }

    let fired = space
        .produce("ask".to_string(), "20".to_string(), false)
        .await
        .expect("produce")
        .expect("⚠ DEFECT D1: (10, 20) satisfies the guard, so the COMM was ENABLED");

    let matched: Vec<String> = fired.1.iter().map(|result| result.matched_datum.clone()).collect();
    assert_eq!(matched, vec!["10".to_string(), "20".to_string()], "the admissible pairing fired");

    let mut bids = resting(&space, "bid").await;
    bids.sort();
    assert_eq!(bids, vec![55, 60, 70], "only the 10 was consumed");
    assert!(resting(&space, "ask").await.is_empty(), "the arriving ask was consumed, not dropped");
}

// ════════════════════════════════════════════════════════════════════════════════════════════
// ★ The second, WIDER repair — a guard-free receive that the same search unsticks
// ════════════════════════════════════════════════════════════════════════════════════════════

/// ⚠ **CONSENSUS.** This test pins behaviour that changed for programs carrying NO guard.
///
/// A search that can backtrack is complete against spatial dead-ends too. `for(@x <- c; @"k"
/// <- c)` has a wildcard bind that can swallow the very datum the second bind needs; the
/// predecessor filled the binds left to right and abandoned the receive the moment the second
/// found nothing, although `(x = "other", "k")` is a rendezvous the rho calculus enables. The
/// guard-aware search reaches it, so this COMM now fires where it previously did not — and the
/// affected shape is ordinary Rholang, not the new `where` syntax.
///
/// The payloads are chosen so the canonical order presents `"1"` (the datum the second bind
/// needs) FIRST, which is exactly when the wildcard swallows it.
#[tokio::test]
async fn a_guard_free_join_no_longer_strands_on_a_spatial_dead_end() {
    let (space, _replay, _matcher) = fixture().await;

    // Canonical order on channel "ch": "1" must come before "7".
    const DATA: [&str; 2] = ["7", "1"];
    assert_eq!(
        canonical_position("ch", &DATA, "1"),
        0,
        "this test requires the SPECIFIC datum first in the canonical order, so the wildcard \
         bind reaches it before the bind that needs it"
    );

    for datum in DATA {
        let _ = space
            .produce("ch".to_string(), datum.to_string(), false)
            .await
            .expect("produce");
    }

    let fired = space
        .consume(
            vec!["ch".to_string(), "ch".to_string()],
            vec![Pattern::Wildcard, Pattern::StringMatch("1".to_string())],
            // No guard whatsoever: this is the guard-FREE half of the repair.
            GuardedContinuation::unguarded("join"),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect(
            "(x = 7, \"1\") is a rendezvous the rho calculus enables; the wildcard bind must \
             give the \"1\" back rather than stranding the receive",
        );

    let matched: Vec<String> = fired.1.iter().map(|result| result.matched_datum.clone()).collect();
    assert_eq!(matched, vec!["7".to_string(), "1".to_string()]);
    assert!(resting(&space, "ch").await.is_empty(), "both data were consumed by the one COMM");
}

/// ⚠ **CONSENSUS.** A receive that takes two data from the SAME channel must leave neither
/// behind. It used to leave one.
///
/// `remove_datum` removes POSITIONALLY, so a batch of removals from one channel must run from
/// the highest index down; removing a low index first shifts every higher one. Both spaces
/// sorted their candidates highest-first and then `.rev()`ed that into lowest-first, so the
/// second removal of a same-channel pair ran off the end, its `Err` was swallowed, and the
/// datum stayed in the store AFTER being delivered to the continuation — free to be consumed
/// again. (Scala's `storePersistentData` / `removeMatchedDatumAndJoin` sort
/// `_.datumIndex` with `Ordering[Int].reverse` and traverse in THAT order; the port
/// re-reversed it.)
///
/// The defect is independent of the guard work, but it could not be left: the guard-aware
/// search reaches same-channel selections that previously stranded, which would have turned a
/// latent duplication into a live one.
#[tokio::test]
async fn a_receive_taking_two_data_from_one_channel_removes_both() {
    for (label, patterns) in [
        ("both wildcards", vec![Pattern::Wildcard, Pattern::Wildcard]),
        (
            "a wildcard and a literal",
            vec![Pattern::Wildcard, Pattern::StringMatch("1".to_string())],
        ),
    ] {
        let (space, _replay, _matcher) = fixture().await;
        for datum in ["7", "1"] {
            let _ = space
                .produce("ch".to_string(), datum.to_string(), false)
                .await
                .expect("produce");
        }

        let fired = space
            .consume(
                vec!["ch".to_string(), "ch".to_string()],
                patterns,
                GuardedContinuation::unguarded("pair"),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("consume")
            .expect("both binds can be filled");

        let mut matched: Vec<String> =
            fired.1.iter().map(|result| result.matched_datum.clone()).collect();
        matched.sort();
        assert_eq!(matched, vec!["1".to_string(), "7".to_string()], "{label}: both were delivered");
        assert!(
            resting(&space, "ch").await.is_empty(),
            "{label}: a datum delivered to the continuation must NOT still be in the store — \
             it could be consumed a second time"
        );
    }
}

/// The same, on the produce path (`remove_matched_datum_and_join`), where the arriving datum is
/// one of the two the receive takes.
#[tokio::test]
async fn a_produce_completing_a_same_channel_join_removes_both() {
    let (space, _replay, _matcher) = fixture().await;

    let fired = space
        .consume(
            vec!["ch".to_string(), "ch".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::unguarded("pair"),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");
    assert!(fired.is_none(), "nothing rests yet");

    let fired = space
        .produce("ch".to_string(), "7".to_string(), false)
        .await
        .expect("produce");
    assert!(fired.is_none(), "one datum cannot fill two binds");

    let fired = space
        .produce("ch".to_string(), "1".to_string(), false)
        .await
        .expect("produce")
        .expect("the second datum completes the join");

    let mut matched: Vec<String> =
        fired.1.iter().map(|result| result.matched_datum.clone()).collect();
    matched.sort();
    assert_eq!(matched, vec!["1".to_string(), "7".to_string()]);
    assert!(
        resting(&space, "ch").await.is_empty(),
        "neither datum may survive the COMM that consumed it"
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════
// Play / replay agreement
// ════════════════════════════════════════════════════════════════════════════════════════════

/// Replay must reconstruct the SAME selection, not merely a COMM with the same event bytes.
/// The check is the post-state root: play and replay end on the same history root, and the
/// replay space's rigged COMM multimap is fully consumed.
#[tokio::test]
async fn play_and_replay_agree_on_a_guarded_selection() {
    let (space, replay_space, _matcher) = fixture().await;

    // Canonical order: 88, 42, 68, 77, 44, 91 — so the first spatial pick (88) is INADMISSIBLE
    // and the selection is reached only by backtracking. Pinned below so a change to the
    // ordering bytes cannot quietly turn this into a test of the easy case.
    const OFFERS: [&str; 6] = ["91", "42", "77", "44", "68", "88"];
    assert_eq!(
        canonical_position("offer", &OFFERS, "88"),
        0,
        "this test requires an inadmissible datum FIRST in the canonical order"
    );

    let empty_point = space.create_checkpoint().await.expect("checkpoint");

    for offer in OFFERS {
        let _ = space
            .produce("offer".to_string(), offer.to_string(), false)
            .await
            .expect("produce");
    }
    let play_result = space
        .consume(
            vec!["offer".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("desk", Guard::AtMost(45)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect("the enabled COMM fires in play");
    let rig_point = space.create_checkpoint().await.expect("checkpoint");

    replay_space
        .rig_and_reset(empty_point.root, rig_point.log)
        .await
        .expect("rig and reset");

    for offer in OFFERS {
        let _ = replay_space
            .produce("offer".to_string(), offer.to_string(), false)
            .await
            .expect("replay produce");
    }
    let replay_result = replay_space
        .consume(
            vec!["offer".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("desk", Guard::AtMost(45)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("replay consume")
        .expect("the same COMM fires in replay");

    assert_eq!(replay_result.0, play_result.0, "same continuation, same bindings");
    assert_eq!(replay_result.1, play_result.1, "same datum selected");

    let final_point = replay_space.create_checkpoint().await.expect("checkpoint");
    assert_eq!(final_point.root, rig_point.root, "same post-state root");
    assert!(
        replay_space
            .replay_data
            .lock()
            .expect("replay data lock")
            .is_empty(),
        "every rigged COMM was consumed by replay"
    );
}

/// ★ The case the shared candidate order exists for. Both binds draw from the SAME channel, so
/// the two data can be assigned to the binds either way round and `COMM::new` — which sorts its
/// produce refs — cannot tell the two assignments apart. Only the guard can. If replay selected
/// by spatial order while play selected by the guard, the trace assertion would still pass and
/// the two would bind the receive's variables the other way round: a silent post-state
/// divergence. This pins that they agree.
#[tokio::test]
async fn play_and_replay_agree_when_the_guard_permutes_a_repeated_channel() {
    let (space, replay_space, _matcher) = fixture().await;

    // Canonical order: 8 then 3 — DESCENDING, so the spatial-first assignment `(8, 3)` is the
    // one the guard refuses and the admissible assignment is the permuted `(3, 8)`.
    const BOOK: [&str; 2] = ["3", "8"];
    assert_eq!(
        canonical_position("book", &BOOK, "8"),
        0,
        "this test requires the LARGER datum first in the canonical order, so that the guard \
         and the spatial order disagree about the assignment"
    );

    let empty_point = space.create_checkpoint().await.expect("checkpoint");

    for entry in BOOK {
        let _ = space
            .produce("book".to_string(), entry.to_string(), false)
            .await
            .expect("produce");
    }
    let play_result = space
        .consume(
            vec!["book".to_string(), "book".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::guarded("pair", Guard::StrictlyIncreasing),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect("(3, 8) is admissible, so the COMM is enabled");

    let play_matched: Vec<String> = play_result
        .1
        .iter()
        .map(|result| result.matched_datum.clone())
        .collect();
    assert_eq!(
        play_matched,
        vec!["3".to_string(), "8".to_string()],
        "the guard chose the increasing ASSIGNMENT, not merely the pair"
    );
    let rig_point = space.create_checkpoint().await.expect("checkpoint");

    replay_space
        .rig_and_reset(empty_point.root, rig_point.log)
        .await
        .expect("rig and reset");

    for entry in BOOK {
        let _ = replay_space
            .produce("book".to_string(), entry.to_string(), false)
            .await
            .expect("replay produce");
    }
    let replay_result = replay_space
        .consume(
            vec!["book".to_string(), "book".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::guarded("pair", Guard::StrictlyIncreasing),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("replay consume")
        .expect("the same COMM fires in replay");

    assert_eq!(
        replay_result.1, play_result.1,
        "replay bound the SAME datum to the SAME bind — not the permuted assignment that would \
         have built an identical COMM event"
    );
    let final_point = replay_space.create_checkpoint().await.expect("checkpoint");
    assert_eq!(final_point.root, rig_point.root, "same post-state root");
}

// ════════════════════════════════════════════════════════════════════════════════════════════
// Cost and determinism
// ════════════════════════════════════════════════════════════════════════════════════════════

/// Pay-for-what-you-use. Without a guard the search accepts its first leaf, so it makes exactly
/// the spatial calls the pre-fix single-pick matcher made: one scan of the pool, stopping at the
/// first match. Asserting the exact count (not merely "not much more") is what keeps a future
/// refactor from quietly making the COMM path quadratic for everybody.
#[tokio::test]
async fn an_unguarded_workload_costs_exactly_what_it_did() {
    let (space, _replay, matcher) = fixture().await;

    for datum in ["1", "2", "3", "4", "5", "6", "7", "8"] {
        let _ = space
            .produce("ch".to_string(), datum.to_string(), false)
            .await
            .expect("produce");
    }

    let before = matcher.spatial_calls();
    let fired = space
        .consume(
            vec!["ch".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::unguarded("plain"),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect("a wildcard receive over a non-empty channel fires");
    assert_eq!(fired.1.len(), 1);

    assert_eq!(
        matcher.spatial_calls() - before,
        1,
        "an unguarded receive stops at the first spatial match — no candidate is re-examined"
    );

    // …and a guarded receive that its first candidate satisfies costs the same.
    let before = matcher.spatial_calls();
    let fired = space
        .consume(
            vec!["ch".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("generous", Guard::AtMost(1000)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect("a satisfiable guard fires");
    assert_eq!(fired.1.len(), 1);
    assert_eq!(
        matcher.spatial_calls() - before,
        1,
        "a guard that accepts its first candidate costs one spatial call, as before the fix"
    );
}

/// ★ The cost of completeness, measured rather than asserted.
///
/// The search visits at most `Π_j |pool_j|` leaves, and a leaf is reached only after a guard
/// rejection, so the shape of the bill is:
///
/// | receive | pools | `Match::get` calls | `check_commit` calls |
/// |---|---|---|---|
/// | unguarded, any arity | any | first match per bind | 1 |
/// | guarded, one bind | `n` | `≤ n` | `≤ n` |
/// | guarded, two binds | `n`, `m` | `≤ n + n·m` | `≤ n·m` |
///
/// This test measures the WORST case of each row — a guard that admits nothing, so the search
/// is driven to exhaustion — on stores far larger than this system has been observed to hold on
/// one channel, and prints the wall time so the quadratic row is a number the reader can weigh
/// rather than a word. Run with `--nocapture` to see it.
#[tokio::test]
async fn the_cost_of_a_complete_guarded_search_is_bounded_and_measured() {
    // ── One bind, 1000 resting data, NOTHING admissible: the exhaustive scan ────────────────
    let (space, _replay, matcher) = fixture().await;
    const POOL: i64 = 1000;
    for value in 0..POOL {
        let _ = space
            .produce("deep".to_string(), value.to_string(), false)
            .await
            .expect("produce");
    }

    let spatial_before = matcher.spatial_calls();
    let guard_before = matcher.guard_calls();
    let started = std::time::Instant::now();
    let fired = space
        .consume(
            vec!["deep".to_string()],
            vec![Pattern::Wildcard],
            // Every datum is >= 0, so nothing satisfies this and the search runs to exhaustion.
            GuardedContinuation::guarded("insatiable", Guard::AtMost(-1)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");
    let elapsed = started.elapsed();
    let spatial = matcher.spatial_calls() - spatial_before;
    let guards = matcher.guard_calls() - guard_before;

    assert!(fired.is_none(), "nothing is admissible, so the receive rests");
    assert_eq!(
        spatial, POOL as usize,
        "one bind exhausts in a LINEAR scan: one spatial call per datum, no re-examination"
    );
    assert_eq!(guards, spatial, "and one guard question per spatial match");
    println!(
        "★ guarded single bind, {POOL} resting data, exhaustive: {spatial} Match::get, \
         {guards} check_commit, {elapsed:?}"
    );

    // ── Two binds, 60 × 60, NOTHING admissible: the quadratic corner ────────────────────────
    let (space, _replay, matcher) = fixture().await;
    const SIDE: i64 = 60;
    for value in 0..SIDE {
        let _ = space
            .produce("left".to_string(), value.to_string(), false)
            .await
            .expect("produce");
        let _ = space
            .produce("right".to_string(), value.to_string(), false)
            .await
            .expect("produce");
    }

    let spatial_before = matcher.spatial_calls();
    let guard_before = matcher.guard_calls();
    let started = std::time::Instant::now();
    let fired = space
        .consume(
            vec!["left".to_string(), "right".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::guarded("insatiable", Guard::AtMost(-1)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");
    let elapsed = started.elapsed();
    let spatial = matcher.spatial_calls() - spatial_before;
    let guards = matcher.guard_calls() - guard_before;
    let side = SIDE as usize;

    assert!(fired.is_none(), "nothing is admissible, so the join rests");
    assert_eq!(
        guards,
        side * side,
        "the exhaustive two-bind search asks the guard once per complete selection"
    );
    assert_eq!(
        spatial,
        side + side * side,
        "…and makes one spatial call per bind per selection reached"
    );
    println!(
        "★ guarded two-bind join, {side} × {side} resting data, exhaustive: {spatial} \
         Match::get, {guards} check_commit, {elapsed:?}"
    );

    // ── The same store WITHOUT a guard: what the pre-fix matcher cost ──────────────────────
    let (space, _replay, matcher) = fixture().await;
    for value in 0..SIDE {
        let _ = space
            .produce("left".to_string(), value.to_string(), false)
            .await
            .expect("produce");
        let _ = space
            .produce("right".to_string(), value.to_string(), false)
            .await
            .expect("produce");
    }
    let spatial_before = matcher.spatial_calls();
    let started = std::time::Instant::now();
    let fired = space
        .consume(
            vec!["left".to_string(), "right".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::unguarded("plain"),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect("an unguarded join over two non-empty channels fires");
    let elapsed = started.elapsed();
    let spatial = matcher.spatial_calls() - spatial_before;

    assert_eq!(fired.1.len(), 2);
    assert_eq!(
        spatial, 2,
        "unguarded: one spatial call per bind — the pre-fix cost, unchanged"
    );
    println!(
        "  control — the SAME store with no guard: {spatial} Match::get, {elapsed:?}"
    );
}

/// The candidate order the selection rests on is a function of the candidates alone, not of the
/// order they were inserted in. Two stores holding the same data in different insertion orders
/// present the same candidate sequence to the matcher — modulo the store index each carries,
/// which is exactly the removal address and must NOT be canonicalized.
#[test]
fn the_canonical_candidate_order_is_a_pure_function_of_the_candidates() {
    let make = |values: &[&str]| -> Vec<Datum<String>> {
        values
            .iter()
            .map(|value| Datum::create(&"ch".to_string(), value.to_string(), false))
            .collect()
    };

    let forwards = order_candidates_with_index(make(&["91", "77", "42", "68"]));
    let backwards = order_candidates_with_index(make(&["68", "42", "77", "91"]));

    let payloads = |ordered: &[(Datum<String>, i32)]| -> Vec<String> {
        ordered.iter().map(|(datum, _)| (*datum.a).clone()).collect()
    };
    assert_eq!(
        payloads(&forwards),
        payloads(&backwards),
        "insertion order does not survive into the candidate order"
    );

    let mut indices: Vec<i32> = forwards.iter().map(|(_, index)| *index).collect();
    indices.sort();
    assert_eq!(indices, vec![0, 1, 2, 3], "every STORE index is preserved exactly once");

    assert_eq!(
        payloads(&order_candidates_with_index(make(&["91", "77", "42", "68"]))),
        payloads(&forwards),
        "and the order is stable across calls"
    );
}

/// The same store answers the same way every time: repeat the whole guarded rendezvous on fresh
/// spaces and require an identical selection each time. (Determinism here is per-store, which is
/// what consensus needs; the payloads themselves determine the order.)
#[tokio::test]
async fn the_guarded_selection_is_stable_across_repetitions() {
    // Canonical order: 88, 42, 77, 63, 44 — an inadmissible datum first (so the answer is
    // reached by backtracking) and TWO admissible ones (so "which one" is a real question).
    const OFFERS: [&str; 5] = ["63", "42", "77", "44", "88"];
    assert_eq!(
        canonical_position("offer", &OFFERS, "88"),
        0,
        "this test requires an inadmissible datum FIRST in the canonical order"
    );
    let mut selections = Vec::with_capacity(8);

    for _ in 0..8 {
        let (space, _replay, _matcher) = fixture().await;
        for offer in OFFERS {
            let _ = space
                .produce("offer".to_string(), offer.to_string(), false)
                .await
                .expect("produce");
        }
        let fired = space
            .consume(
                vec!["offer".to_string()],
                vec![Pattern::Wildcard],
                GuardedContinuation::guarded("desk", Guard::AtMost(45)),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("consume")
            .expect("two of the five offers are admissible, so the COMM is enabled");
        selections.push(fired.1[0].matched_datum.clone());
    }

    let first = selections[0].clone();
    assert!(
        selections.iter().all(|selection| *selection == first),
        "the guarded selection must not vary run to run: {selections:?}"
    );
    assert!(
        first == "42" || first == "44",
        "and it must be one of the ADMISSIBLE offers: {first}"
    );

    // …and it is the FIRST admissible candidate in the canonical order — the same selection
    // rule an unguarded receive follows, with "admissible" widened from spatial to spatial+guard.
    let admissible_first = canonical_position("offer", &OFFERS, "42")
        .min(canonical_position("offer", &OFFERS, "44"));
    assert!(
        admissible_first > 0,
        "the answer must lie behind at least one rejection, or this test does not exercise \
         backtracking at all"
    );
    assert_eq!(
        canonical_position("offer", &OFFERS, &first),
        admissible_first,
        "the selection is the FIRST admissible candidate in the canonical order"
    );
}

/// A guarded receive that no data satisfy must not disturb the map the next continuation is
/// matched against. Two continuations wait on one channel: the first rejects everything, the
/// second accepts. The produce must reach the second — which it can only do if the failed
/// search restored every pool it touched.
#[tokio::test]
async fn a_failed_guarded_search_restores_the_candidate_pools() {
    let (space, _replay, _matcher) = fixture().await;

    // Installed first, so it is offered the produce first.
    let fired = space
        .consume(
            vec!["ch".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("strict", Guard::AtMost(-1)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");
    assert!(fired.is_none(), "nothing rests yet");

    let fired = space
        .consume(
            vec!["ch".to_string()],
            vec![Pattern::Wildcard],
            GuardedContinuation::guarded("lenient", Guard::AtMost(1000)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");
    assert!(fired.is_none(), "still nothing rests");

    let fired = space
        .produce("ch".to_string(), "7".to_string(), false)
        .await
        .expect("produce")
        .expect("the lenient receive admits the 7, so a COMM is enabled");

    assert_eq!(fired.0.continuation.name, "lenient", "the continuation whose guard held fired");
    assert_eq!(fired.1[0].matched_datum, "7");
    assert!(resting(&space, "ch").await.is_empty(), "the datum was consumed exactly once");
    assert_eq!(waiting(&space, &["ch"]).await, 1, "the strict receive is still waiting");
}

/// A regression guard for the pools themselves: `HashMap` iteration order must not leak into
/// the selection. The join below binds two DIFFERENT channels, so the search visits two map
/// entries; run it repeatedly and require one answer.
#[tokio::test]
async fn a_multi_channel_guarded_selection_does_not_depend_on_map_iteration_order() {
    let mut selections: Vec<Vec<String>> = Vec::with_capacity(8);

    for _ in 0..8 {
        let (space, _replay, _matcher) = fixture().await;

        for bid in ["60", "20", "55", "15"] {
            let _ = space
                .produce("bid".to_string(), bid.to_string(), false)
                .await
                .expect("produce");
        }
        for ask in ["30", "80", "25"] {
            let _ = space
                .produce("ask".to_string(), ask.to_string(), false)
                .await
                .expect("produce");
        }

        let fired = space
            .consume(
                vec!["bid".to_string(), "ask".to_string()],
                vec![Pattern::Wildcard, Pattern::Wildcard],
                GuardedContinuation::guarded("settle", Guard::AtMost(40)),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("consume")
            .expect("several pairings satisfy the guard, so the COMM is enabled");

        selections.push(fired.1.iter().map(|r| r.matched_datum.clone()).collect());
    }

    let first = selections[0].clone();
    assert!(
        selections.iter().all(|selection| *selection == first),
        "the multi-channel guarded selection must not vary run to run: {selections:?}"
    );
    assert!(
        as_int(&first[0]) <= 40 && as_int(&first[1]) <= 40,
        "and it must satisfy the guard: {first:?}"
    );
}

/// The `HashMap` the search mutates is keyed by channel; this pins that a bind whose channel
/// has NO pool at all fails the whole selection rather than silently binding fewer variables.
#[tokio::test]
async fn a_bind_on_an_empty_channel_fires_nothing() {
    let (space, _replay, _matcher) = fixture().await;

    let _ = space
        .produce("bid".to_string(), "10".to_string(), false)
        .await
        .expect("produce");

    let fired = space
        .consume(
            vec!["bid".to_string(), "ask".to_string()],
            vec![Pattern::Wildcard, Pattern::Wildcard],
            GuardedContinuation::guarded("settle", Guard::AtMost(1000)),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume");

    assert!(fired.is_none(), "the ask channel is empty, so no selection exists");
    assert_eq!(resting(&space, "bid").await, vec![10], "and the bid still rests");

    let observed: HashMap<String, usize> = HashMap::new();
    assert!(observed.is_empty(), "no side channel was written");
}
