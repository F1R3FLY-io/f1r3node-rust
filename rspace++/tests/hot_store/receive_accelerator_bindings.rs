use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock};

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::receive_accelerator::{ReceiveKind, ReceiveOccurrences};
use crate::rspace::trace::event::Consume;

type TestStore = InMemHotStore<String, String, String, String>;
type TestState = InMemHotStoreState<String, String, String, String>;
type TestSnapshot = HotStoreState<String, String, String, String>;
type TestContinuation = WaitingContinuation<String, String>;
type TestHandle = ReceiveAcceleratorHandle<String, String, String>;
type TestAnswer = ReceiveAnswer<String, String, String>;
type TestOccurrences = ReceiveOccurrences<String, String, String>;

struct GroupHistory {
    continuations: HashMap<Vec<String>, Vec<TestContinuation>>,
    joins: HashMap<String, Vec<Vec<String>>>,
}

impl HistoryReaderBase<String, String, String, String> for GroupHistory {
    fn get_data_canonical(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(&self, key: &[String]) -> Vec<TestContinuation> {
        self.continuations.get(key).cloned().unwrap_or_default()
    }

    fn get_joins_canonical(&self, key: &String) -> Vec<Vec<String>> {
        self.joins.get(key).cloned().unwrap_or_default()
    }
}

fn group(names: &[&str]) -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() }

fn channel(name: &str) -> String { name.to_owned() }

fn continuation(name: u8, persist: bool) -> TestContinuation {
    WaitingContinuation {
        patterns: Arc::new(vec![format!("pattern-{name}")]),
        continuation: Arc::new(format!("body-{name}")),
        persist,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![name; 32]),
            persistent: persist,
        },
    }
}

fn fixture_snapshot() -> TestSnapshot {
    let shared = group(&["a", "b", "a"]);
    let single = group(&["a"]);
    HotStoreState {
        continuations: HashMap::from([
            (shared.clone(), vec![continuation(1, false), continuation(2, true)]),
            (single.clone(), vec![continuation(3, false), continuation(4, false)]),
        ]),
        installed_continuations: HashMap::from([(single.clone(), continuation(5, true))]),
        data: HashMap::new(),
        joins: HashMap::from([
            (channel("a"), vec![shared.clone(), single]),
            (channel("b"), vec![shared]),
        ]),
        installed_joins: HashMap::new(),
    }
}

fn fixture_history() -> GroupHistory {
    GroupHistory {
        continuations: HashMap::from([(group(&["a", "c"]), vec![continuation(6, false)])]),
        joins: HashMap::from([(channel("c"), vec![group(&["a", "c"])])]),
    }
}

fn store_from(snapshot: TestSnapshot, history: GroupHistory) -> TestStore {
    TestStore {
        state: RwLock::new(
            TestState::from_snapshot(snapshot)
                .expect("the fixture snapshot fits the multiplicity model"),
        ),
        history_cache: RwLock::new(HistoryStoreCache::default()),
        history_reader_base: Box::new(history),
    }
}

fn rebuilt() -> TestStore {
    let store = store_from(fixture_snapshot(), fixture_history());
    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    store
}

fn revision(store: &TestStore) -> ReceiveRevision {
    store
        .receive_revision()
        .expect("the in-memory hot store issues receive revisions")
}

fn handle(store: &TestStore) -> TestHandle {
    store
        .receive_accelerator()
        .expect("a store with a published occurrence index issues accelerator handles")
}

fn exact(answer: TestAnswer) -> TestOccurrences {
    match answer {
        ReceiveAnswer::Exact(occurrences) => occurrences,
        ReceiveAnswer::Fallback(class) => panic!("expected an exact answer, found {class:?}"),
    }
}

fn fallback(answer: TestAnswer) -> ReceiveAcceleratorClass {
    match answer {
        ReceiveAnswer::Exact(occurrences) => panic!(
            "expected a fallback, found an exact answer over {} groups",
            occurrences.groups().len()
        ),
        ReceiveAnswer::Fallback(class) => class,
    }
}

fn prefix(channels: &[String]) -> Vec<u8> { RhoPathV1::continuation_prefix(channels) }

fn prefixes_of(groups: &[Vec<String>]) -> Vec<Vec<u8>> {
    let mut prefixes = Vec::with_capacity(groups.len());
    prefixes.extend(groups.iter().map(|channels| prefix(channels)));
    prefixes.sort_unstable();
    prefixes
}

fn group_prefixes(occurrences: &TestOccurrences) -> Vec<Vec<u8>> {
    occurrences
        .groups()
        .iter()
        .map(|occurrence| occurrence.prefix().to_vec())
        .collect()
}

fn positions_in(store: &TestStore, name: &str, channels: &[String]) -> Vec<usize> {
    let answer = exact(store.current_receive_occurrences(&channel(name)));
    let group_prefix = prefix(channels);
    let occurrence = answer
        .groups()
        .iter()
        .find(|candidate| candidate.prefix() == group_prefix.as_slice())
        .expect("the channel occurs in the group");
    assert_eq!(occurrence.channels(), channels);
    occurrence
        .positions()
        .iter()
        .map(|position| position.index())
        .collect()
}

#[test]
fn absent_accelerators_fall_back_without_an_empty_answer() {
    let store = store_from(fixture_snapshot(), fixture_history());
    let receiving = channel("a");
    assert!(store.receive_accelerator().is_none());
    assert_eq!(
        fallback(store.current_receive_occurrences(&receiving)),
        ReceiveAcceleratorClass::Absent
    );
    assert_eq!(
        fallback(store.receive_occurrences(None, &receiving)),
        ReceiveAcceleratorClass::Absent
    );

    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    assert_eq!(
        fallback(store.receive_occurrences(None, &receiving)),
        ReceiveAcceleratorClass::Absent
    );
    let held = handle(&store);
    assert_eq!(
        group_prefixes(&exact(store.receive_occurrences(Some(&held), &receiving))),
        prefixes_of(&[group(&["a", "b", "a"]), group(&["a"])])
    );
}

#[test]
fn held_handles_become_stale_after_a_continuation_change_and_foreign_after_a_reset() {
    let store = rebuilt();
    let receiving = channel("a");
    let held = handle(&store);
    assert_eq!(store.put_continuation(&group(&["a"]), continuation(7, false)), Some(true));
    assert_eq!(
        fallback(store.receive_occurrences(Some(&held), &receiving)),
        ReceiveAcceleratorClass::Stale
    );
    let refreshed = handle(&store);
    assert_eq!(
        exact(store.receive_occurrences(Some(&refreshed), &receiving))
            .groups()
            .len(),
        2
    );

    store.clear();
    assert!(store.receive_accelerator().is_none());
    assert_eq!(
        fallback(store.current_receive_occurrences(&receiving)),
        ReceiveAcceleratorClass::Absent
    );
    for stale in [&held, &refreshed] {
        assert_eq!(
            fallback(store.receive_occurrences(Some(stale), &receiving)),
            ReceiveAcceleratorClass::Foreign
        );
    }

    store.set_state(fixture_snapshot());
    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    for stale in [&held, &refreshed] {
        assert_eq!(
            fallback(store.receive_occurrences(Some(stale), &receiving)),
            ReceiveAcceleratorClass::Foreign
        );
    }
    let restored = handle(&store);
    assert_eq!(
        exact(store.receive_occurrences(Some(&restored), &receiving))
            .groups()
            .len(),
        2
    );
}

#[test]
fn unloaded_joins_make_the_channel_incomplete() {
    let store = rebuilt();
    let receiving = channel("c");
    let joined = group(&["a", "c"]);
    let kept = revision(&store);
    assert_eq!(
        fallback(store.current_receive_occurrences(&receiving)),
        ReceiveAcceleratorClass::Incomplete
    );

    assert_eq!(store.get_joins(&receiving), vec![joined.clone()]);
    assert_eq!(
        fallback(store.current_receive_occurrences(&receiving)),
        ReceiveAcceleratorClass::Incomplete
    );

    assert_eq!(store.get_continuations(&joined), vec![continuation(6, false)]);
    assert_eq!(revision(&store), kept);
    let answer = exact(store.current_receive_occurrences(&receiving));
    assert_eq!(answer.revision(), &kept);
    assert_eq!(group_prefixes(&answer), vec![prefix(&joined)]);
}

#[test]
fn join_groups_outside_the_handle_scope_make_the_channel_incomplete() {
    let store = rebuilt();
    let receiving = channel("a");
    let joined = group(&["a", "c"]);
    let kept = revision(&store);
    assert_eq!(
        exact(store.current_receive_occurrences(&receiving))
            .groups()
            .len(),
        2
    );

    store
        .put_join(&receiving, &joined)
        .expect("the in-memory hot store records every join");
    assert_eq!(revision(&store), kept);
    assert_eq!(
        fallback(store.current_receive_occurrences(&receiving)),
        ReceiveAcceleratorClass::Incomplete
    );

    assert_eq!(store.get_continuations(&joined), vec![continuation(6, false)]);
    assert_eq!(
        group_prefixes(&exact(store.current_receive_occurrences(&receiving))),
        prefixes_of(&[group(&["a", "b", "a"]), group(&["a"]), joined])
    );
}

#[test]
fn installed_only_groups_outside_the_handle_scope_make_the_channel_incomplete() {
    let store = rebuilt();
    let receiving = channel("d");
    let installed = group(&["d"]);
    assert_eq!(store.install_continuation(&installed, continuation(8, true)), Some(()));
    assert!(store.get_joins(&receiving).is_empty());
    assert_eq!(
        fallback(store.current_receive_occurrences(&receiving)),
        ReceiveAcceleratorClass::Incomplete
    );

    assert_eq!(store.get_continuations(&installed), vec![continuation(8, true)]);
    let answer = exact(store.current_receive_occurrences(&receiving));
    let [occurrence] = answer.groups() else {
        panic!("the installed group is the channel's only group");
    };
    assert_eq!(occurrence.channels(), installed.as_slice());
    let [target] = occurrence.targets() else {
        panic!("the installed slot is the group's only target");
    };
    assert_eq!(target.kind(), ReceiveKind::Installed);
    assert_eq!(target.continuation(), &continuation(8, true));
}

#[test]
fn held_handles_never_answer_for_groups_loaded_after_they_were_taken() {
    let store = rebuilt();
    let receiving = channel("a");
    let shared = group(&["a", "b", "a"]);
    let joined = group(&["a", "c"]);
    let held = handle(&store);
    store
        .put_join(&receiving, &joined)
        .expect("the in-memory hot store records every join");
    assert_eq!(store.get_continuations(&joined), vec![continuation(6, false)]);
    assert_eq!(&revision(&store), held.revision());

    assert_eq!(
        fallback(store.receive_occurrences(Some(&held), &receiving)),
        ReceiveAcceleratorClass::Incomplete
    );
    assert_eq!(
        group_prefixes(&exact(store.receive_occurrences(Some(&held), &channel("b")))),
        vec![prefix(&shared)]
    );
    let current = handle(&store);
    assert_eq!(
        group_prefixes(&exact(store.receive_occurrences(Some(&current), &receiving))),
        prefixes_of(&[shared, group(&["a"]), joined])
    );
}

#[test]
fn repeated_channels_report_every_position_of_the_channel() {
    let store = rebuilt();
    let shared = group(&["a", "b", "a"]);
    assert_eq!(positions_in(&store, "a", &shared), vec![0, 2]);
    assert_eq!(positions_in(&store, "b", &shared), vec![1]);
    assert_eq!(positions_in(&store, "a", &group(&["a"])), vec![0]);
}

#[test]
fn targets_list_the_installed_slot_then_resting_records_in_get_continuations_order() {
    let store = rebuilt();
    let single = group(&["a"]);
    assert_eq!(store.put_continuation(&single, continuation(9, false)), Some(true));
    let answer = exact(store.current_receive_occurrences(&channel("a")));
    let group_prefix = prefix(&single);
    let occurrence = answer
        .groups()
        .iter()
        .find(|candidate| candidate.prefix() == group_prefix.as_slice())
        .expect("the single-channel group holds the channel");
    let kinds: Vec<ReceiveKind> = occurrence
        .targets()
        .iter()
        .map(|target| target.kind())
        .collect();
    assert_eq!(kinds, vec![
        ReceiveKind::Installed,
        ReceiveKind::Resting,
        ReceiveKind::Resting,
        ReceiveKind::Resting
    ]);
    let continuations: Vec<TestContinuation> = occurrence
        .targets()
        .iter()
        .map(|target| target.continuation().clone())
        .collect();
    assert_eq!(continuations, store.get_continuations(&single));
    assert!(
        occurrence
            .targets()
            .iter()
            .all(|target| target.group_prefix() == group_prefix.as_slice())
    );
}

#[test]
fn answers_carry_the_revision_of_the_handle_that_produced_them() {
    let store = rebuilt();
    let receiving = channel("a");
    let first = handle(&store);
    assert_eq!(first.revision(), &revision(&store));
    assert_eq!(
        exact(store.receive_occurrences(Some(&first), &receiving)).revision(),
        first.revision()
    );

    assert_eq!(store.put_continuation(&group(&["a"]), continuation(9, false)), Some(true));
    let second = handle(&store);
    assert_ne!(second.revision(), first.revision());
    assert!(second.revision().shares_generation(first.revision()));
    assert_eq!(
        exact(store.receive_occurrences(Some(&second), &receiving)).revision(),
        second.revision()
    );
    assert_eq!(exact(store.current_receive_occurrences(&receiving)).revision(), &revision(&store));
}
