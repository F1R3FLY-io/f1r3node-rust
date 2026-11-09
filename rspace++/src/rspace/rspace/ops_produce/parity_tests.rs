//! Added by decision 123 (plan v4, section 10.1, V3-2): the parity of the
//! shared produce search with the base search (D-OBL-4).
//!
//! `base_run_matcher_for_channels` and `base_extract_first_match` are
//! verbatim copies of the base bodies before decision 123. They differ only
//! in their names and in the call of the copied extraction. The test runs
//! both searches on one store, with fixed orders in place of the random
//! shuffle. It compares the found candidates, and it compares the reads and
//! the order calls of each join group.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::rspace::internal::{ConsumeCandidate, Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::metrics_constants::{
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, RSPACE_METRICS_SOURCE,
};
use crate::rspace::operation_context::{ItemAdmission, MatchAdmission};
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::MaybeProduceCandidate;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::event::{Consume, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    /// The base `run_matcher_for_channels`. Only the name and the call of
    /// `base_extract_first_match` differ.
    fn base_run_matcher_for_channels(
        &self,
        grouped_channels: Vec<Vec<C>>,
        fetch_matching_continuations: impl Fn(
            Vec<C>,
        )
            -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        fetch_matching_data: impl Fn(C) -> (C, Vec<(Datum<A>, i32)>),
        admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    ) -> MaybeProduceCandidate<C, P, A, K> {
        for channels in &grouped_channels {
            let t_cont = Instant::now();
            let match_candidates = fetch_matching_continuations(channels.to_vec());
            metrics::counter!("rspace.matcher.fetch_continuations_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t_cont.elapsed().as_nanos() as u64);
            metrics::counter!("rspace.matcher.continuations_returned", "source" => RSPACE_METRICS_SOURCE)
                .increment(match_candidates.len() as u64);

            let t_data = Instant::now();
            let channel_to_indexed_data: HashMap<C, Vec<(Datum<A>, i32)>> = channels
                .iter()
                .map(|c| fetch_matching_data(c.clone()))
                .collect();
            metrics::counter!("rspace.matcher.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t_data.elapsed().as_nanos() as u64);

            let t_match = Instant::now();
            // Changed by G3 (DR-135, S5): with the owner checks.
            // let first_match = self.extract_first_match(
            //     &self.matcher,
            //     channels.to_vec(),
            //     match_candidates,
            //     channel_to_indexed_data,
            // );
            let first_match = self.base_extract_first_match(
                &self.matcher,
                channels.to_vec(),
                match_candidates,
                channel_to_indexed_data,
                admission,
            );
            metrics::counter!("rspace.matcher.extract_first_match_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t_match.elapsed().as_nanos() as u64);

            if let Some(produce_candidate) = first_match {
                return Some(produce_candidate);
            }
        }
        None
    }

    /// The base `SpaceMatcher::extract_first_match`. Only the name differs.
    fn base_extract_first_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channels: Vec<C>,
        match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        mut channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
        admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    ) -> Option<ProduceCandidate<C, P, A, K>> {
        metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        for (cont, index) in &match_candidates {
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            let __pair_start = std::time::Instant::now();
            let channel_pattern_pairs: Vec<(C, P)> = channels
                .iter()
                .cloned()
                .zip(cont.patterns.iter().cloned())
                .collect();
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(__pair_start.elapsed().as_nanos() as u64);

            let mut rollback = Vec::new();
            // Added by G3 (DR-135, S5): the item test of this continuation
            // (R5).
            let item = admission.map(|admission| ItemAdmission {
                admission,
                continuation: &cont.continuation,
                persistent: cont.persist,
                channel_count: channels.len(),
            });
            // Added by G3 (DR-135, S5): a produce keeps its own datum (R6).
            // On a channel that the join names once, the pattern takes only
            // the incoming datum (index -1). The full list goes into
            // `rollback` first, so the reverse restore writes it back last.
            let own_channel = admission.and_then(|admission| admission.own_datum_channel());
            if let Some(own) = own_channel {
                if channels.iter().filter(|channel| *channel == own).count() == 1 {
                    if let Some(data) = channel_to_index_data.get_mut(own) {
                        if data.iter().any(|(_, index)| *index != -1) {
                            rollback.push((own.clone(), data.clone()));
                            data.retain(|(_, index)| *index == -1);
                        }
                    }
                }
            }
            // Changed by G3 (DR-135, S5): the extraction runs the item test.
            // let data_candidates = self.extract_data_candidates_rollback(
            //     matcher,
            //     &channel_pattern_pairs,
            //     &mut channel_to_index_data,
            //     &mut rollback,
            // );
            let data_candidates = self.extract_data_candidates_rollback(
                matcher,
                &channel_pattern_pairs,
                &mut channel_to_index_data,
                &mut rollback,
                item,
            );
            // Added by G3 (DR-135, S5): on a repeated channel the tuple must
            // hold the incoming datum (R6).
            let holds_own_datum = own_channel.is_none() ||
                data_candidates
                    .iter()
                    .flatten()
                    .any(|candidate| candidate.datum_index == -1);

            // Changed by G3 (DR-135, S5): the tuple must hold the incoming
            // datum under owner funding (R6).
            // if data_candidates.iter().all(|x| x.is_some()) {
            if data_candidates.iter().all(|x| x.is_some()) && holds_own_datum {
                // Cross-channel commit hook: gives the matcher a chance
                // to veto a commit even after every spatial bind has
                // matched. Used for `where`-clause guards that mention
                // bindings from multiple channels (plan §7.12).
                let matched_data: Vec<A> = data_candidates
                    .iter()
                    .map(|c| c.as_ref().unwrap().datum.a.clone())
                    .collect();
                if !matcher.check_commit(&cont.continuation, &matched_data) {
                    // Guard rejected: roll back and try the next
                    // waiting continuation, just like a spatial miss.
                    // Changed by DR-129 (R3): a channel that the join names
                    // more than once is saved once for each match. The
                    // restore runs in reverse, so the first saved list, the
                    // whole one, is the last one written back.
                    // for (ch, original) in rollback {
                    for (ch, original) in rollback.into_iter().rev() {
                        channel_to_index_data.insert(ch, original);
                    }
                    continue;
                }
                // Added by G3 (DR-135, S5): the tuple test (R5). The owners of
                // the tuple pay its COMM, or the tuple is refused: it rolls
                // back, and the next continuation is tried, as on a veto.
                let candidates: Vec<ConsumeCandidate<C, A>> =
                    data_candidates.into_iter().flatten().collect();
                if let Some(admission) = admission {
                    if !admission.admit_tuple(cont, &candidates) {
                        for (ch, original) in rollback.into_iter().rev() {
                            channel_to_index_data.insert(ch, original);
                        }
                        continue;
                    }
                }
                metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                // Changed by G3 (DR-135, S5): the candidates moved out for the
                // tuple test.
                // data_candidates: data_candidates.into_iter().flatten().collect(),
                return Some(ProduceCandidate {
                    channels,
                    // Deref-clone the inner continuation only on a successful
                    // match — failed probes never deep-clone.
                    continuation: (**cont).clone(),
                    continuation_index: *index,
                    data_candidates: candidates,
                });
            }

            // Restore only the channels that were actually mutated
            // Changed by DR-129 (R3): in reverse, as above.
            // for (ch, original) in rollback {
            for (ch, original) in rollback.into_iter().rev() {
                channel_to_index_data.insert(ch, original);
            }
        }
        None
    }
}

/// A datum matches each pattern that is a prefix of it. The commit check
/// vetoes each continuation whose name starts with "veto".
struct PrefixWithVeto;

impl Match<String, String, String> for PrefixWithVeto {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        datum.starts_with(pattern.as_str()).then(|| datum.clone())
    }

    fn check_commit(&self, continuation: &String, _: &[String]) -> bool {
        !continuation.starts_with("veto")
    }

    fn commit_may_veto(&self, continuation: &String) -> bool { continuation.starts_with("veto") }
}

/// The owner checks of a search: an unfunded datum fails its item test, a
/// refused tuple fails the tuple test, and `own` is the channel of the
/// incoming datum (R5, R6).
#[derive(Default)]
struct Owners {
    own: Option<String>,
    unfunded: BTreeSet<String>,
    refused: BTreeSet<Vec<String>>,
}

impl MatchAdmission<String, String, String, String> for Owners {
    fn admit_item(&self, _: &String, _: bool, _: usize, datum: &String, _: bool) -> bool {
        !self.unfunded.contains(datum)
    }

    fn own_datum_channel(&self) -> Option<&String> { self.own.as_ref() }

    fn admit_tuple(
        &self,
        _: &WaitingContinuation<String, String>,
        candidates: &[ConsumeCandidate<String, String>],
    ) -> bool {
        let mut tuple: Vec<String> = candidates
            .iter()
            .map(|candidate| candidate.datum.a.clone())
            .collect();
        tuple.sort();
        !self.refused.contains(&tuple)
    }
}

/// One produce search. `groups` are the join groups of the produce, and
/// `incoming` is its channel and datum. A stored continuation is a join, its
/// patterns and its name, and a stored datum is a channel and a value.
/// `visited` is the number of groups that the search reads. `matches` tells
/// whether the search matches in the stored order and in the reversed order.
struct Scenario {
    name: &'static str,
    groups: Vec<Vec<String>>,
    continuations: Vec<(Vec<String>, Vec<String>, String)>,
    data: Vec<(String, String)>,
    incoming: (String, String),
    owners: Option<Owners>,
    visited: usize,
    matches: [bool; 2],
}

fn strings(items: &[&str]) -> Vec<String> { items.iter().map(|item| item.to_string()).collect() }

fn scenario(
    name: &'static str,
    groups: &[&[&str]],
    continuations: &[(&[&str], &[&str], &str)],
    data: &[(&str, &str)],
    incoming: (&str, &str),
    owners: Option<Owners>,
    visited: usize,
    matches: [bool; 2],
) -> Scenario {
    Scenario {
        name,
        groups: groups.iter().map(|group| strings(group)).collect(),
        continuations: continuations
            .iter()
            .map(|(channels, patterns, name)| {
                (strings(channels), strings(patterns), name.to_string())
            })
            .collect(),
        data: data
            .iter()
            .map(|(channel, value)| (channel.to_string(), value.to_string()))
            .collect(),
        incoming: (incoming.0.to_string(), incoming.1.to_string()),
        owners,
        visited,
        matches,
    }
}

/// Owner checks whose own datum is on `a`, with the unfunded data `unfunded`
/// and the refused tuples `refused`.
fn owners_on_a(unfunded: &[&str], refused: &[&[&str]]) -> Option<Owners> {
    Some(Owners {
        own: Some("a".to_string()),
        unfunded: unfunded.iter().map(|datum| datum.to_string()).collect(),
        refused: refused.iter().map(|tuple| strings(tuple)).collect(),
    })
}

fn scenarios() -> Vec<Scenario> {
    vec![
        // The first group fails on `b`, the second group matches, and the
        // third group is not read.
        scenario(
            "a later group is read only after the earlier groups fail",
            &[&["a", "b"], &["a"], &["a", "c"]],
            &[
                (&["a", "b"], &["x", "y"], "k0"),
                (&["a"], &["x"], "k1"),
                (&["a", "c"], &["x", "w"], "k2"),
            ],
            &[("a", "x2"), ("b", "z1"), ("c", "w1")],
            ("a", "x1"),
            None,
            2,
            [true, true],
        ),
        // In the stored order the veto restores the data of the vetoed tuple
        // on a repeated channel, and the next continuation takes them.
        scenario(
            "a veto restores the data that the vetoed tuple took",
            &[&["a", "a"]],
            &[(&["a", "a"], &["x", "x"], "veto0"), (&["a", "a"], &["x", "x"], "k1")],
            &[("a", "x2"), ("a", "x3")],
            ("a", "x1"),
            None,
            1,
            [true, true],
        ),
        scenario(
            "the search reads every group when no group matches",
            &[&["a"], &["b"]],
            &[(&["a"], &["y"], "k0"), (&["b"], &["y"], "k1")],
            &[("a", "x2"), ("b", "x3")],
            ("a", "x1"),
            None,
            2,
            [false, false],
        ),
        // `a` keeps only the incoming datum (R6). In the stored order `y2`
        // fails its item test, and both continuations take the refused tuple
        // of `x1` and `y1`. In the reversed order `y3` comes first.
        scenario(
            "the owner checks skip an unfunded datum and refuse a tuple",
            &[&["a", "b"]],
            &[(&["a", "b"], &["x", "y"], "k0"), (&["a", "b"], &["x", "y"], "k1")],
            &[("a", "x2"), ("b", "y2"), ("b", "y1"), ("b", "y3")],
            ("a", "x1"),
            owners_on_a(&["y2"], &[&["x1", "y1"]]),
            1,
            [false, true],
        ),
        // The group names `a` twice, so the list of `a` stays whole, and a
        // tuple must hold the incoming datum (R6).
        scenario(
            "a repeated own channel without the incoming datum does not match",
            &[&["a", "a"]],
            &[(&["a", "a"], &["x", "x"], "k0")],
            &[("a", "x2"), ("a", "x3")],
            ("a", "z1"),
            owners_on_a(&[], &[]),
            1,
            [false, false],
        ),
        scenario(
            "a repeated own channel with the incoming datum matches",
            &[&["a", "a"]],
            &[(&["a", "a"], &["x", "x"], "k0")],
            &[("a", "x2"), ("a", "x3")],
            ("a", "x1"),
            owners_on_a(&[], &[]),
            1,
            [true, true],
        ),
    ]
}

/// A space whose store holds the continuations and the data of `scenario`.
/// The store puts each item first in its list, so the items go in reversed,
/// and each list keeps the order of the scenario.
async fn space(scenario: &Scenario) -> RSpace<String, String, String, String> {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let space = RSpace::create(stores, Arc::new(Box::new(PrefixWithVeto))).expect("an RSpace");
    let store = space.get_store();
    for (channels, patterns, name) in scenario.continuations.iter().rev() {
        store.put_continuation(channels, WaitingContinuation {
            patterns: patterns.clone(),
            continuation: name.clone(),
            persist: false,
            peeks: BTreeSet::new(),
            source: Consume::create(channels, patterns, name, false),
        });
    }
    for (channel, value) in scenario.data.iter().rev() {
        store.put_datum(channel, Datum {
            a: value.clone(),
            persist: false,
            source: Produce::create(channel, value, false),
        });
    }
    space
}

/// The base search or the shared search.
#[derive(Clone, Copy)]
enum Body {
    Base,
    Shared,
}

/// One step of a search: a store read of the continuations of a group or of
/// the data of a channel, or one call of the order with the length of its
/// list. The fixed order stands for the shuffle of the base fetch closures.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Continuations(Vec<String>),
    Data(String),
    Order(usize),
}

/// `list` indexed as the shuffle indexes it, in the stored order or in the
/// reversed order.
fn fixed_order<D>(list: Vec<D>, reversed: bool, steps: &RefCell<Vec<Step>>) -> Vec<(D, i32)> {
    steps.borrow_mut().push(Step::Order(list.len()));
    let mut indexed: Vec<(D, i32)> = list
        .into_iter()
        .enumerate()
        .map(|(index, item)| (item, index as i32))
        .collect();
    if reversed {
        indexed.reverse();
    }
    indexed
}

/// A found candidate: its channels, its continuation and store index, and
/// each matched channel, datum, removed value and datum index.
type Found = Option<(
    Vec<String>,
    WaitingContinuation<String, String>,
    i32,
    Vec<(String, Datum<String>, String, i32)>,
)>;

fn found(candidate: MaybeProduceCandidate<String, String, String, String>) -> Found {
    candidate.map(|candidate| {
        (
            candidate.channels,
            candidate.continuation,
            candidate.continuation_index,
            candidate
                .data_candidates
                .into_iter()
                .map(|data| (data.channel, data.datum, data.removed_datum, data.datum_index))
                .collect(),
        )
    })
}

/// Runs one search of `scenario` with the fetch closures of the base
/// produce, in the fixed order. It returns the found candidate and the steps
/// of the search.
fn search(
    space: &RSpace<String, String, String, String>,
    scenario: &Scenario,
    reversed: bool,
    body: Body,
) -> (Found, Vec<Step>) {
    let steps = RefCell::new(Vec::new());
    let store = space.get_store();
    let (bat_channel, value) = &scenario.incoming;
    let incoming = Datum {
        a: value.clone(),
        persist: false,
        source: Produce::create(bat_channel, value, false),
    };
    let fetch_matching_continuations = |channels: Vec<String>| {
        steps
            .borrow_mut()
            .push(Step::Continuations(channels.clone()));
        fixed_order(store.get_continuations_arc(&channels), reversed, &steps)
    };
    let fetch_matching_data = |channel: String| {
        steps.borrow_mut().push(Step::Data(channel.clone()));
        let mut ordered = fixed_order(store.get_data(&channel), reversed, &steps);
        if channel == *bat_channel {
            ordered.insert(0, (incoming.clone(), -1));
        }
        (channel, ordered)
    };
    let admission = scenario
        .owners
        .as_ref()
        .map(|owners| owners as &dyn MatchAdmission<String, String, String, String>);
    let groups = scenario.groups.clone();
    let candidate = match body {
        Body::Base => space.base_run_matcher_for_channels(
            groups,
            fetch_matching_continuations,
            fetch_matching_data,
            admission,
        ),
        Body::Shared => space.run_matcher_for_channels(
            groups,
            fetch_matching_continuations,
            fetch_matching_data,
            admission,
        ),
    };
    (found(candidate), steps.into_inner())
}

/// The steps of a search that reads the first `visited` groups of
/// `scenario`: for each group the continuations and then the data of each
/// channel occurrence, and one order call after each read.
fn expected_steps(
    space: &RSpace<String, String, String, String>,
    scenario: &Scenario,
) -> Vec<Step> {
    let visited = &scenario.groups[..scenario.visited];
    let store = space.get_store();
    let mut steps = Vec::with_capacity(visited.iter().map(|group| 2 + 2 * group.len()).sum());
    for channels in visited {
        steps.push(Step::Continuations(channels.clone()));
        steps.push(Step::Order(store.get_continuations_arc(channels).len()));
        for channel in channels {
            steps.push(Step::Data(channel.clone()));
            steps.push(Step::Order(store.get_data(channel).len()));
        }
    }
    steps
}

/// Added by decision 123 (plan v4, section 10.1, V3-2): on fixed orders the
/// shared search finds the candidate of the base search. Both read the lists
/// of a join group once, when the search reaches the group, in the base
/// order, and each read has one order call. No group is read after a match.
#[tokio::test]
async fn run_matcher_visits_join_groups_in_the_old_order() {
    for scenario in scenarios() {
        let space = space(&scenario).await;
        for (reversed, matches) in [false, true].into_iter().zip(scenario.matches) {
            let (base, base_steps) = search(&space, &scenario, reversed, Body::Base);
            let (shared, shared_steps) = search(&space, &scenario, reversed, Body::Shared);
            let context = format!("{} (reversed: {reversed})", scenario.name);
            assert_eq!(shared, base, "{context}: the found candidate");
            assert_eq!(shared.is_some(), matches, "{context}: the match");
            assert_eq!(shared_steps, base_steps, "{context}: the steps");
            assert_eq!(shared_steps, expected_steps(&space, &scenario), "{context}: the reads");
        }
    }
}
