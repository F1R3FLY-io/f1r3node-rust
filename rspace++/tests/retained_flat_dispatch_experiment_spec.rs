#![cfg(feature = "retained-flat-dispatch-experiment")]

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::candidate_order::deterministic_candidate_hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::{
    RetainedSetAutomaton, flat_dispatch_experiment,
};
use rspace_plus_plus::rspace::rho_path::{
    RhoPathQuery, RhoPathQueryAtom, RhoPathV1, encode_structural_sequence,
};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
enum Pattern {
    #[default]
    Any,
    Exact(Vec<u8>),
    SequenceFirst {
        expected: Vec<u8>,
        first: Vec<u8>,
    },
    SequenceSecond {
        expected: Vec<u8>,
        second: Vec<u8>,
    },
}

#[derive(Clone, Copy)]
struct Matcher {
    flat: bool,
}

const PATHMAP: Matcher = Matcher { flat: false };
const FLAT: Matcher = Matcher { flat: true };

impl Match<Pattern, Vec<u8>, ()> for Matcher {
    fn get(&self, pattern: &Pattern, datum: &Vec<u8>) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Any => Some(datum.clone()),
            Pattern::Exact(expected) if expected == datum => Some(datum.clone()),
            Pattern::SequenceFirst { expected, .. } | Pattern::SequenceSecond { expected, .. }
                if expected == datum =>
            {
                Some(datum.clone())
            }
            Pattern::Exact(_) | Pattern::SequenceFirst { .. } | Pattern::SequenceSecond { .. } => {
                None
            }
        }
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Any => None,
            Pattern::Exact(path) => Some(path.clone()),
            Pattern::SequenceFirst { .. } | Pattern::SequenceSecond { .. } => None,
        }
    }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> { Some(datum.clone()) }

    fn rho_path_query(&self, pattern: &Pattern) -> Option<RhoPathQuery> {
        match pattern {
            Pattern::SequenceFirst { first, .. } => Some(RhoPathQuery::structural_sequence(
                [RhoPathQueryAtom::Exact(first.clone()), RhoPathQueryAtom::Wildcard],
                false,
            )),
            Pattern::SequenceSecond { second, .. } => Some(RhoPathQuery::structural_sequence(
                [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(second.clone())],
                false,
            )),
            Pattern::Any | Pattern::Exact(_) => None,
        }
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }

    fn retained_flat_dispatch_experiment(&self) -> bool { self.flat }
}

fn waiting(patterns: Vec<Pattern>) -> WaitingContinuation<Pattern, ()> {
    WaitingContinuation {
        patterns: Arc::new(patterns),
        ..WaitingContinuation::default()
    }
}

#[derive(Clone, Debug)]
struct Corpus {
    patterns: Vec<Vec<Pattern>>,
    fresh: Vec<u8>,
    resting: Vec<(Vec<u8>, u8)>,
    bind_positions: Vec<usize>,
}

fn byte() -> impl Strategy<Value = u8> { prop::sample::select(vec![0x00, 0x01, b'a', b'b', 0xff]) }

fn path() -> impl Strategy<Value = Vec<u8>> { prop::collection::vec(byte(), 0..6) }

fn structure() -> impl Strategy<Value = Vec<u8>> {
    (path(), path()).prop_map(|(first, second)| {
        encode_structural_sequence([first.as_slice(), second.as_slice()])
    })
}

fn pattern() -> impl Strategy<Value = Pattern> {
    prop_oneof![
        Just(Pattern::Any),
        path().prop_map(Pattern::Exact),
        (path(), path()).prop_map(|(first, second)| Pattern::SequenceFirst {
            expected: encode_structural_sequence([first.as_slice(), second.as_slice()]),
            first,
        }),
        (path(), path()).prop_map(|(first, second)| Pattern::SequenceSecond {
            expected: encode_structural_sequence([first.as_slice(), second.as_slice()]),
            second,
        }),
    ]
}

fn datum() -> impl Strategy<Value = Vec<u8>> { prop_oneof![path(), structure()] }

fn corpus(candidates: Range<usize>) -> impl Strategy<Value = Corpus> {
    (
        prop::collection::vec(prop::collection::vec(pattern(), 1..=3), candidates),
        datum(),
        prop::collection::vec((datum(), 0u8..3), 0..5),
        prop::collection::vec(0usize..4, 0..4),
    )
        .prop_map(|(patterns, fresh, resting, bind_positions)| Corpus {
            patterns,
            fresh,
            resting,
            bind_positions,
        })
}

fn candidates(patterns: &[Vec<Pattern>]) -> Vec<WaitingContinuation<Pattern, ()>> {
    patterns.iter().cloned().map(waiting).collect()
}

fn bag(resting: &[(Vec<u8>, u8)]) -> Bag {
    let mut bag = Bag::new();
    for (datum, identity) in resting {
        bag.insert(&RhoPathV1::relative_datum_key(datum, &[*identity]))
            .expect("generated resting occurrence");
    }
    bag
}

#[test]
fn generated_flat_tables_select_and_count_exactly_as_the_pathmap_index() {
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let mut runner = TestRunner::new(config);
    runner
        .run(&corpus(0..16), |corpus| {
            let candidates = candidates(&corpus.patterns);
            let pathmap = RetainedSetAutomaton::compile(&PATHMAP, &candidates);
            let flat = RetainedSetAutomaton::compile(&FLAT, &candidates);
            prop_assert!(flat_dispatch_experiment::flat_tables(&pathmap).is_none());
            prop_assert!(flat_dispatch_experiment::flat_tables(&flat).is_some());
            prop_assert_eq!(flat.stats(), pathmap.stats());
            prop_assert_eq!(flat.candidate_count(), pathmap.candidate_count());
            let empty = Bag::new();
            let singleton = bag(&corpus.resting[..corpus.resting.len().min(1)]);
            let several = bag(&corpus.resting);
            for resting in [None, Some(&empty), Some(&singleton), Some(&several)] {
                prop_assert_eq!(
                    flat.matching_candidates_with_resting(
                        &corpus.fresh,
                        &corpus.bind_positions,
                        resting
                    ),
                    pathmap.matching_candidates_with_resting(
                        &corpus.fresh,
                        &corpus.bind_positions,
                        resting
                    )
                );
            }
            Ok(())
        })
        .expect("generated flat-dispatch selections and statistics equal the PathMap index");
}

#[test]
fn deep_wide_and_long_shared_inputs_are_stack_safe_on_a_tiny_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let deep = vec![0x5a; 1_000_000];
            let deep_candidates = vec![waiting(vec![Pattern::Exact(deep.clone())])];
            let deep_automaton = RetainedSetAutomaton::compile(&FLAT, &deep_candidates);
            assert_eq!(deep_automaton.matching_candidates(&deep, &[0]), vec![true]);
            assert!(deep_automaton.stats().maximum_terminal_depth >= deep.len());
            drop(deep_automaton);

            let wide_candidates = (0..20_000)
                .map(|_| waiting(vec![Pattern::Any]))
                .collect::<Vec<_>>();
            let wide_automaton = RetainedSetAutomaton::compile(&FLAT, &wide_candidates);
            let selected = wide_automaton.matching_candidates(b"datum", &[0]);
            assert_eq!(selected.len(), wide_candidates.len());
            assert!(selected.into_iter().all(|candidate| candidate));
            assert_eq!(wide_automaton.stats().distinct_terminal_path_count, 1);
            assert_eq!(wide_automaton.stats().shared_terminal_count, 19_999);
            drop(wide_automaton);
        })
        .expect("tiny-stack flat automaton worker")
        .join()
        .expect("deep and wide flat tables compile, select and drop on a tiny stack");

    let shared = vec![0x5a; 99_000];
    let mut first = shared.clone();
    first.resize(100_000, 0x41);
    let mut second = shared.clone();
    second.resize(100_000, 0x42);
    let mut absent = shared.clone();
    absent.resize(100_000, 0x43);
    let mut resting = Bag::new();
    resting
        .insert(&RhoPathV1::relative_datum_key(&first, b"identity/first"))
        .expect("first long resting key");
    resting
        .insert(&RhoPathV1::relative_datum_key(&second, b"identity/second"))
        .expect("second long resting key");
    let candidates = vec![
        waiting(vec![Pattern::Exact(shared[..50_000].to_vec())]),
        waiting(vec![Pattern::Exact(shared.clone())]),
        waiting(vec![Pattern::Exact(second)]),
        waiting(vec![Pattern::Exact(absent)]),
    ];
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn_scoped(scope, || {
                for matcher in [PATHMAP, FLAT] {
                    let automaton = RetainedSetAutomaton::compile(&matcher, &candidates);
                    let selected =
                        automaton.matching_candidates_with_resting(b"fresh", &[0], Some(&resting));
                    assert_eq!(selected, vec![true, true, true, false]);
                    drop(automaton);
                }
            })
            .expect("tiny-stack product worker")
            .join()
            .expect("both product walks cross the long shared Bag keys on a tiny stack");
    });
}

#[test]
fn two_compilations_of_one_generated_corpus_build_equal_flat_tables() {
    let mut runner = TestRunner::deterministic();
    let corpus = corpus(48..64)
        .new_tree(&mut runner)
        .expect("one generated corpus")
        .current();
    let candidates = candidates(&corpus.patterns);
    let first = RetainedSetAutomaton::compile(&FLAT, &candidates);
    let second = RetainedSetAutomaton::compile(&FLAT, &candidates);
    let first_tables =
        flat_dispatch_experiment::flat_tables(&first).expect("the first compilation is flat");
    let second_tables =
        flat_dispatch_experiment::flat_tables(&second).expect("the second compilation is flat");

    assert_eq!(format!("{first_tables:?}"), format!("{second_tables:?}"));
    let resting = bag(&corpus.resting);
    for positions in [vec![0], vec![1], vec![2], vec![0, 1, 2]] {
        for resting in [None, Some(&resting)] {
            assert_eq!(
                first.matching_candidates_with_resting(&corpus.fresh, &positions, resting),
                second.matching_candidates_with_resting(&corpus.fresh, &positions, resting)
            );
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum SpacePattern {
    #[default]
    Wildcard,
    SequenceSecond {
        expected: String,
        second: Vec<u8>,
    },
}

impl StableHashSerialize for SpacePattern {}

impl ColdStoreDecode for SpacePattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

#[derive(Clone)]
struct CountingMatcher {
    flat: bool,
    enabled: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

impl Match<SpacePattern, String, String> for CountingMatcher {
    fn get(&self, pattern: &SpacePattern, datum: &String) -> Option<String> {
        if !self.enabled.load(Ordering::Relaxed) {
            return None;
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        match pattern {
            SpacePattern::Wildcard => Some(datum.clone()),
            SpacePattern::SequenceSecond { expected, .. } if expected == datum => {
                Some(datum.clone())
            }
            SpacePattern::SequenceSecond { .. } => None,
        }
    }

    fn retained_datum_path(&self, datum: &String) -> Option<Vec<u8>> {
        Some(datum.as_bytes().to_vec())
    }

    fn rho_path_query(&self, pattern: &SpacePattern) -> Option<RhoPathQuery> {
        match pattern {
            SpacePattern::SequenceSecond { second, .. } => Some(RhoPathQuery::structural_sequence(
                [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(second.clone())],
                false,
            )),
            SpacePattern::Wildcard => None,
        }
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }

    fn retained_flat_dispatch_experiment(&self) -> bool { self.flat }
}

type TestSpace = RSpace<String, SpacePattern, String, String>;

async fn space(flat: bool, enabled: Arc<AtomicBool>) -> (TestSpace, Arc<AtomicUsize>) {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let calls = Arc::new(AtomicUsize::new(0));
    let matcher = CountingMatcher {
        flat,
        enabled,
        calls: Arc::clone(&calls),
    };
    let space = RSpace::create(stores, Arc::new(Box::new(matcher))).expect("RSpace");
    (space, calls)
}

fn sequence_value(first: &[u8], second: &[u8]) -> String {
    String::from_utf8(encode_structural_sequence([first, second]))
        .expect("ASCII structural sequence")
}

async fn datum_first_receives_agree() {
    let channel = "flat-dispatch-retained-query-automaton".to_string();
    let channels = vec![channel.clone()];
    let mut receives = (0..256)
        .map(|index| {
            let first = format!("record/{index:08x}");
            let second = format!("class/{index:08x}").into_bytes();
            let value = sequence_value(first.as_bytes(), &second);
            let pattern = SpacePattern::SequenceSecond {
                expected: value.clone(),
                second,
            };
            let continuation = format!("continuation/{index:08x}");
            let patterns = vec![pattern.clone()];
            let waiting = WaitingContinuation::create(
                &channels,
                &patterns,
                &continuation,
                true,
                BTreeSet::new(),
            );
            (deterministic_candidate_hash(&waiting), index, value, pattern, continuation)
        })
        .collect::<Vec<_>>();
    let target = receives
        .iter()
        .max_by_key(|(hash, index, ..)| (hash.clone(), *index))
        .expect("nonempty corpus")
        .2
        .clone();

    let enabled = Arc::new(AtomicBool::new(true));
    let (pathmap, pathmap_calls) = space(false, Arc::clone(&enabled)).await;
    let (flat, flat_calls) = space(true, enabled).await;
    for (_, _, _, pattern, continuation) in receives.drain(..) {
        for space in [&pathmap, &flat] {
            assert!(
                space
                    .consume(
                        channels.clone(),
                        vec![pattern.clone()],
                        continuation.clone(),
                        true,
                        BTreeSet::new(),
                    )
                    .await
                    .expect("consume")
                    .is_none()
            );
        }
    }
    drop(pathmap.take_event_log().await);
    drop(flat.take_event_log().await);
    pathmap_calls.store(0, Ordering::Relaxed);
    flat_calls.store(0, Ordering::Relaxed);

    let pathmap_result = pathmap
        .produce(channel.clone(), target.clone(), false)
        .await
        .expect("PathMap produce")
        .expect("PathMap COMM");
    let flat_result = flat
        .produce(channel, target, false)
        .await
        .expect("flat produce")
        .expect("flat COMM");

    assert_eq!(flat_result, pathmap_result);
    assert_eq!(pathmap_calls.load(Ordering::Relaxed), 1);
    assert_eq!(flat_calls.load(Ordering::Relaxed), pathmap_calls.load(Ordering::Relaxed));
    assert_eq!(flat.take_event_log().await, pathmap.take_event_log().await);
    assert_eq!(flat.to_map().await, pathmap.to_map().await);
}

async fn resting_only_comm_agrees() {
    let channel = "flat-dispatch-retained-resting-product".to_string();
    let channels = vec![channel.clone()];
    let resting_second = b"resting/class".to_vec();
    let resting = sequence_value(b"resting/head", &resting_second);
    let fresh = sequence_value(b"fresh/head", b"fresh/class");
    let pattern = SpacePattern::SequenceSecond {
        expected: resting.clone(),
        second: resting_second,
    };

    let enabled = Arc::new(AtomicBool::new(false));
    let (pathmap, pathmap_calls) = space(false, Arc::clone(&enabled)).await;
    let (flat, flat_calls) = space(true, Arc::clone(&enabled)).await;
    for space in [&pathmap, &flat] {
        assert!(
            space
                .install(
                    channels.clone(),
                    vec![pattern.clone()],
                    "installed/continuation".to_string(),
                )
                .await
                .expect("install")
                .is_none()
        );
        assert!(
            space
                .produce(channel.clone(), resting.clone(), false)
                .await
                .expect("resting produce")
                .is_none()
        );
        drop(space.take_event_log().await);
    }
    enabled.store(true, Ordering::Relaxed);
    pathmap_calls.store(0, Ordering::Relaxed);
    flat_calls.store(0, Ordering::Relaxed);

    let pathmap_result = pathmap
        .produce(channel.clone(), fresh.clone(), false)
        .await
        .expect("PathMap fresh produce")
        .expect("the installed continuation matches resting data");
    let flat_result = flat
        .produce(channel, fresh, false)
        .await
        .expect("flat fresh produce")
        .expect("the flat product preserves the resting-only COMM");

    assert_eq!(flat_result, pathmap_result);
    assert_eq!(pathmap_calls.load(Ordering::Relaxed), 1);
    assert_eq!(flat_calls.load(Ordering::Relaxed), pathmap_calls.load(Ordering::Relaxed));
    assert_eq!(flat.take_event_log().await, pathmap.take_event_log().await);
    assert_eq!(flat.to_map().await, pathmap.to_map().await);
}

#[tokio::test]
async fn rspace_differentials_agree_between_the_pathmap_and_flat_matchers() {
    datum_first_receives_agree().await;
    resting_only_comm_agrees().await;
}
