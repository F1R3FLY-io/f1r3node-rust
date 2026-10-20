//! Added by DR-129 (M34): play picks among candidates that match equally at
//! random, as `dev` does (`shuffle_with_index`). F1-F3 show the random pick,
//! and F7 covers the rollback of a join that names one channel twice (risk
//! R3 of the restoration plan). F8 covers the rollback of the order that E1
//! gives such a channel (RT-06, complete guard search, defect D1).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use super::Wildcard;
use crate::rspace::candidate_order::canonical_candidates;
use crate::rspace::internal::Datum;
use crate::rspace::r#match::Match;
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::ISpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::space_matcher::SpaceMatcher;

/// Fresh spaces per test. With three equal candidates, one candidate is
/// missed in every run with probability `3 * (2 / 3)^200`, about `10^-35`
/// (risk R10).
const RUNS: usize = 200;

/// Fresh spaces for F7. Half of the runs try the vetoed continuation first,
/// so all of them miss it with probability `2^-64`.
const ROLLBACK_RUNS: usize = 64;

const CHANNEL: &str = "channel";
const DATA: [&str; 3] = ["one", "two", "three"];
const BODIES: [&str; 3] = ["first", "second", "third"];

struct AnyDatum;

impl Match<Wildcard, String, String> for AnyDatum {
    fn get(&self, _: &Wildcard, datum: &String) -> Option<String> { Some(datum.clone()) }
}

/// Vetoes the commit of one named continuation, as a `where` guard does.
struct VetoNamed(&'static str);

impl Match<Wildcard, String, String> for VetoNamed {
    fn get(&self, _: &Wildcard, datum: &String) -> Option<String> { Some(datum.clone()) }

    fn check_commit(&self, continuation: &String, _: &[&String]) -> bool { continuation != self.0 }

    fn commit_may_veto(&self, continuation: &String) -> bool { continuation == self.0 }
}

async fn space(
    matcher: Box<dyn Match<Wildcard, String, String>>,
) -> RSpace<String, Wildcard, String, String> {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    RSpace::create(stores, Arc::new(matcher)).expect("an RSpace")
}

/// One fresh run: three data on one channel, then one consume. Returns the
/// datum that the consume took.
async fn data_pick() -> String {
    let rspace = space(Box::new(AnyDatum)).await;
    for value in DATA {
        rspace
            .produce(CHANNEL.to_string(), value.to_string(), false)
            .await
            .expect("the produce runs");
    }
    let (_, data) = rspace
        .consume(
            vec![CHANNEL.to_string()],
            vec![Wildcard],
            "body".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("the consume runs")
        .expect("a stored datum matches");
    data[0].matched_datum.clone()
}

/// F1: play picks each of three equally matching data across 200 fresh runs.
#[tokio::test]
async fn play_picks_each_of_three_matching_data() {
    let mut picked = BTreeSet::new();
    for _ in 0..RUNS {
        picked.insert(data_pick().await);
    }
    let expected: BTreeSet<String> = DATA.iter().map(|value| value.to_string()).collect();
    assert_eq!(picked, expected, "play must pick every matching datum in some run");
}

/// F2: play picks each of three equally matching continuations across 200
/// fresh runs.
#[tokio::test]
async fn play_picks_each_of_three_matching_continuations() {
    let mut fired = BTreeSet::new();
    for _ in 0..RUNS {
        let rspace = space(Box::new(AnyDatum)).await;
        for body in BODIES {
            let stored = rspace
                .consume(
                    vec![CHANNEL.to_string()],
                    vec![Wildcard],
                    body.to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("the consume runs");
            assert!(stored.is_none(), "no datum is stored yet");
        }
        let (continuation, _, _) = rspace
            .produce(CHANNEL.to_string(), "datum".to_string(), false)
            .await
            .expect("the produce runs")
            .expect("a waiting continuation matches");
        fired.insert(continuation.continuation);
    }
    let expected: BTreeSet<String> = BODIES.iter().map(|body| body.to_string()).collect();
    assert_eq!(fired, expected, "play must fire every matching continuation in some run");
}

/// F3 (negative control): some run picks a datum other than the first one in
/// the canonical order of DR-75, so play does not use that order.
#[tokio::test]
async fn play_does_not_follow_the_canonical_order() {
    let stored: Vec<Datum<String>> = DATA
        .iter()
        .map(|value| Datum::create(&CHANNEL.to_string(), value.to_string(), false))
        .collect();
    let canonical_first = canonical_candidates(stored)
        .into_iter()
        .next()
        .expect("three candidates")
        .0
        .a;
    let mut other = 0;
    for _ in 0..RUNS {
        if data_pick().await != *canonical_first {
            other += 1;
        }
    }
    assert!(other > 0, "every run picked the canonical first datum {canonical_first:?}");
}

/// F7 (risk R3): a commit veto rolls back a join that names one channel
/// twice. The next continuation of the same produce must see every datum
/// again, so play fires it instead of storing the produce.
#[tokio::test]
async fn a_vetoed_join_on_a_repeated_channel_restores_every_datum() {
    let mut stored_instead = 0;
    for _ in 0..ROLLBACK_RUNS {
        let rspace = space(Box::new(VetoNamed("vetoed"))).await;
        let channel = "x".to_string();
        rspace
            .produce(channel.clone(), "stored".to_string(), false)
            .await
            .expect("the produce runs");
        for body in ["vetoed", "accepted"] {
            let waiting = rspace
                .consume(
                    vec![channel.clone(); 2],
                    vec![Wildcard; 2],
                    body.to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("the consume runs");
            assert!(waiting.is_none(), "one datum cannot satisfy two patterns");
        }
        match rspace
            .produce(channel.clone(), "incoming".to_string(), false)
            .await
            .expect("the produce runs")
        {
            Some((fired, _, _)) => assert_eq!(fired.continuation, "accepted"),
            None => stored_instead += 1,
        }
    }
    assert_eq!(
        stored_instead, 0,
        "after a vetoed join, play stored the produce in {stored_instead} of {ROLLBACK_RUNS} runs"
    );
}

/// F8 (RT-06, complete guard search, defect D1): E1 orders the pool of a
/// channel that the join names twice before the complete search. When the
/// commit vetoes every assignment, the rollback restores the pool in its
/// order before the call. F7 cannot reach this rollback, because play puts
/// the incoming datum first, so the pool of F7 already has the order of E1.
#[tokio::test]
async fn a_vetoed_join_restores_the_order_that_e1_changed() {
    let rspace = space(Box::new(VetoNamed("vetoed"))).await;
    let channel = "x".to_string();
    let mut pool: Vec<(Datum<String>, i32)> = Vec::with_capacity(DATA.len());
    for (index, value) in (0i32..).zip(DATA) {
        pool.push((Datum::create(&channel, value.to_string(), false), index));
    }
    pool.sort_by(|(left, _), (right, _)| right.source.hash.cmp(&left.source.hash));
    assert!(
        pool.windows(2)
            .all(|pair| pair[0].0.source.hash > pair[1].0.source.hash),
        "the pool is in decreasing hash order, so E1 changes it"
    );
    let mut pools = HashMap::from([(channel.clone(), pool.clone())]);
    let wildcard = Wildcard;
    let pairs = [(&channel, &wildcard), (&channel, &wildcard)];
    let matcher: Box<dyn Match<Wildcard, String, String>> = Box::new(VetoNamed("vetoed"));
    let selection = rspace.extract_prepared_data_candidates(
        &matcher,
        &pairs,
        &"vetoed".to_string(),
        &mut pools,
    );
    assert!(selection.is_none(), "the commit vetoes every assignment");
    assert_eq!(
        pools,
        HashMap::from([(channel, pool)]),
        "the rollback restores the pool in its order before the call"
    );
}
