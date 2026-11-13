use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};

use proptest::prelude::*;

use super::*;
use crate::rspace::bag::Multiplicity;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_repository::HistoryRepositoryInstances;
use crate::rspace::internal::ConsumeCandidate;
use crate::rspace::logging::{RSpaceLogger, StepCommObserver};
use crate::rspace::r#match::Match;
use crate::rspace::replay_rspace::ReplayRSpace;
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceResult,
};
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::trace::event::{COMM, Event};

type TestStore = InMemHotStore<String, String, String, String>;
type TestSnapshot = HotStoreState<String, String, String, String>;

struct EmptyHistory;

impl HistoryReaderBase<String, String, String, String> for EmptyHistory {
    fn get_data_canonical(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(
        &self,
        _key: &[String],
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _key: &String) -> Vec<Vec<String>> { Vec::new() }
}

fn stored_continuation(persist: bool) -> WaitingContinuation<String, String> {
    WaitingContinuation {
        patterns: Arc::new(vec!["pattern".to_owned()]),
        continuation: Arc::new("continuation".to_owned()),
        persist,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![7; 32]),
            persistent: persist,
        },
    }
}

struct Fixture {
    store: TestStore,
    channels: Vec<String>,
    continuation: WaitingContinuation<String, String>,
    paths: [Vec<u8>; 5],
    initial_counts: BTreeMap<Vec<u8>, u64>,
}

impl Fixture {
    fn new(left_count: u64, right_count: u64, arrival_count: u64, persist: bool) -> Self {
        let channels = vec!["left".to_owned(), "right".to_owned()];
        let unrelated_channel = "unrelated".to_owned();
        let left = Datum::create(&channels[0], "selected-left".to_owned(), false);
        let right = Datum::create(&channels[1], "selected-right".to_owned(), false);
        let arrival = Datum::create(&channels[1], "arrival".to_owned(), false);
        let witness = Datum::create(&channels[0], "witness".to_owned(), true);
        let unrelated = Datum::create(&unrelated_channel, "unrelated".to_owned(), false);
        let paths = [
            RSpaceDataIndex::datum_path(&channels[0], &left),
            RSpaceDataIndex::datum_path(&channels[1], &right),
            RSpaceDataIndex::datum_path(&channels[1], &arrival),
            RSpaceDataIndex::datum_path(&channels[0], &witness),
            RSpaceDataIndex::datum_path(&unrelated_channel, &unrelated),
        ];
        let continuation = stored_continuation(persist);
        let initial = HotStoreState {
            continuations: HashMap::from([(channels.clone(), vec![continuation.clone()])]),
            installed_continuations: HashMap::new(),
            data: HashMap::new(),
            joins: HashMap::from([
                (channels[0].clone(), vec![channels.clone()]),
                (channels[1].clone(), vec![channels.clone()]),
            ]),
            installed_joins: HashMap::new(),
        };
        let store = TestStore {
            state: RwLock::new(InMemHotStoreState::from_snapshot(initial).unwrap()),
            history_cache: RwLock::new(HistoryStoreCache::default()),
            history_reader_base: Box::new(EmptyHistory),
        };
        {
            let mut state = store.state.write().unwrap();
            let mut left_entries = vec![(witness, Multiplicity::ONE)];
            if left_count != 0 {
                left_entries.push((left, Multiplicity::new(left_count).unwrap()));
            }
            state
                .data
                .set_channel_counts(channels[0].clone(), left_entries)
                .unwrap();
            let mut right_entries = Vec::new();
            if right_count != 0 {
                right_entries.push((right, Multiplicity::new(right_count).unwrap()));
            }
            if arrival_count != 0 {
                right_entries.push((arrival, Multiplicity::new(arrival_count).unwrap()));
            }
            state
                .data
                .set_channel_counts(channels[1].clone(), right_entries)
                .unwrap();
            state
                .data
                .set_channel_counts(unrelated_channel, [(unrelated, Multiplicity::new(2).unwrap())])
                .unwrap();
        }
        let initial_counts = paths
            .iter()
            .cloned()
            .zip([left_count, right_count, arrival_count, 1, 2])
            .collect();
        Self {
            store,
            channels,
            continuation,
            paths,
            initial_counts,
        }
    }

    fn selected_commit(&self) -> StoredContinuationCommit {
        let (version, loaded) = self.store.get_continuations_versioned(&self.channels);
        let matches = loaded
            .iter()
            .enumerate()
            .filter(|(_, continuation)| *continuation == &self.continuation)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        StoredContinuationCommit {
            address: ContinuationAddress::new(version, i32::try_from(matches[0].0).unwrap()),
            source: self.continuation.source.clone(),
            persist: self.continuation.persist,
            peeks: self.continuation.peeks.clone(),
        }
    }

    fn transition(&self, delta: AcceptedDatumChanges) -> AcceptedMatch<String> {
        AcceptedMatch {
            channels: self.channels.clone(),
            continuation: Some(self.selected_commit()),
            datum_delta: delta,
            required_persistent_paths: vec![Arc::from(self.paths[3].clone())],
            join_channels: self.channels.clone(),
        }
    }

    fn counts(&self) -> BTreeMap<Vec<u8>, u64> {
        let state = self.store.state.read().unwrap();
        self.paths
            .iter()
            .map(|path| (path.clone(), state.data.count_path(path)))
            .collect()
    }

    fn boundary(&self, snapshot: bool) -> Boundary {
        let state = self.store.state.read().unwrap();
        let counts: BTreeMap<_, _> = self
            .paths
            .iter()
            .map(|path| (path.clone(), state.data.count_path(path)))
            .collect();
        Boundary {
            exact_known_cardinality: counts.values().map(|count| u128::from(*count)).sum(),
            counts,
            loaded_channels: state.data.loaded_channel_count(),
            distinct: state.data.distinct_len(),
            cardinality: state.data.cardinality(),
            continuations: state
                .continuations
                .loaded_records()
                .into_iter()
                .map(|(channels, records)| (channels.to_vec(), records))
                .collect(),
            installed_continuations: state
                .continuations
                .installed_records()
                .into_iter()
                .map(|(channels, record)| (channels.to_vec(), record))
                .collect(),
            joins: state.joins.loaded_records().into_iter().collect(),
            installed_joins: state.joins.installed_records().into_iter().collect(),
            snapshot: snapshot.then(|| state.snapshot().unwrap()),
            journal: state.pending_journal(),
        }
    }

    fn assert_unchanged(&self, before: &Boundary) {
        let state = self.store.state.read().unwrap();
        let counts: BTreeMap<_, _> = self
            .paths
            .iter()
            .map(|path| (path.clone(), state.data.count_path(path)))
            .collect();
        let exact_known_cardinality: u128 = counts.values().map(|count| u128::from(*count)).sum();
        assert_eq!(counts, before.counts);
        assert_eq!(exact_known_cardinality, before.exact_known_cardinality);
        assert_eq!(state.data.loaded_channel_count(), before.loaded_channels);
        assert_eq!(state.data.distinct_len(), before.distinct);
        assert_eq!(state.data.cardinality(), before.cardinality);
        assert!(state.matches_pending_journal(&before.journal));
        let continuations: BTreeMap<_, _> = state
            .continuations
            .loaded_records()
            .into_iter()
            .map(|(channels, records)| (channels.to_vec(), records))
            .collect();
        let installed_continuations: BTreeMap<_, _> = state
            .continuations
            .installed_records()
            .into_iter()
            .map(|(channels, record)| (channels.to_vec(), record))
            .collect();
        let joins: BTreeMap<_, _> = state.joins.loaded_records().into_iter().collect();
        let installed_joins: BTreeMap<_, _> = state.joins.installed_records().into_iter().collect();
        assert_eq!(continuations, before.continuations);
        assert_eq!(installed_continuations, before.installed_continuations);
        assert_eq!(joins, before.joins);
        assert_eq!(installed_joins, before.installed_joins);
        if let Some(expected) = &before.snapshot {
            let actual = state.snapshot().unwrap();
            assert_eq!(actual.data, expected.data);
            assert_eq!(actual.continuations, expected.continuations);
            assert_eq!(actual.installed_continuations, expected.installed_continuations);
            assert_eq!(actual.joins, expected.joins);
            assert_eq!(actual.installed_joins, expected.installed_joins);
        }
    }
}

struct Boundary {
    counts: BTreeMap<Vec<u8>, u64>,
    exact_known_cardinality: u128,
    loaded_channels: usize,
    distinct: usize,
    cardinality: u64,
    continuations: BTreeMap<Vec<String>, Vec<WaitingContinuation<String, String>>>,
    installed_continuations: BTreeMap<Vec<String>, WaitingContinuation<String, String>>,
    joins: BTreeMap<String, Vec<Vec<String>>>,
    installed_joins: BTreeMap<String, Vec<Vec<String>>>,
    snapshot: Option<TestSnapshot>,
    journal: PendingDeltas,
}

fn transition_delta(
    paths: &[Vec<u8>; 5],
    left: u64,
    right: u64,
    arrival: u64,
) -> AcceptedDatumChanges {
    let mut delta = BagDelta::new();
    delta
        .record_removal(&paths[0], Multiplicity::new(left).unwrap())
        .unwrap();
    delta
        .record_removal(&paths[1], Multiplicity::new(right).unwrap())
        .unwrap();
    let mut delta = AcceptedDatumChanges::from_removals(delta).unwrap();
    if arrival != 0 {
        delta
            .record_addition(&paths[2], Multiplicity::new(arrival).unwrap())
            .unwrap();
    }
    delta
}

enum OracleOutcome {
    Committed(BTreeMap<Vec<u8>, u64>),
    Stale,
    Overflow,
}

fn sparse_count_oracle(initial: &BTreeMap<Vec<u8>, u64>, delta: &BagDelta) -> OracleOutcome {
    for (path, amount) in delta.removals().iter() {
        if initial.get(&path).copied().unwrap_or(0) < amount.get() {
            return OracleOutcome::Stale;
        }
    }
    for (path, amount) in delta.additions().iter() {
        if initial
            .get(&path)
            .copied()
            .unwrap_or(0)
            .checked_add(amount.get())
            .is_none()
        {
            return OracleOutcome::Overflow;
        }
    }
    let mut next = initial.clone();
    for (path, amount) in delta.removals().iter() {
        *next.entry(path).or_default() -= amount.get();
    }
    for (path, amount) in delta.additions().iter() {
        *next.entry(path).or_default() += amount.get();
    }
    OracleOutcome::Committed(next)
}

#[test]
fn bounded_multichannel_transition_refines_independent_sparse_count_oracle() {
    let mut committed = 0;
    let mut stale = 0;
    let mut valid_first_stale_second = 0;
    for left_count in 0..=2 {
        for right_count in 0..=2 {
            for left_removal in 1..=2 {
                for right_removal in 1..=2 {
                    for arrival_addition in 0..=2 {
                        for persist in [false, true] {
                            let fixture = Fixture::new(left_count, right_count, 1, persist);
                            let delta = transition_delta(
                                &fixture.paths,
                                left_removal,
                                right_removal,
                                arrival_addition,
                            );
                            let prediction =
                                sparse_count_oracle(&fixture.initial_counts, delta.delta());
                            let transition = fixture.transition(delta.clone());
                            let before = fixture.boundary(true);
                            let result = fixture.store.apply_accepted_match(transition).unwrap();
                            match prediction {
                                OracleOutcome::Stale => {
                                    assert!(matches!(result, AcceptedMatchOutcome::Stale));
                                    fixture.assert_unchanged(&before);
                                    if left_count >= left_removal && right_count < right_removal {
                                        valid_first_stale_second += 1;
                                    }
                                    stale += 1;
                                }
                                OracleOutcome::Committed(expected) => {
                                    let AcceptedMatchOutcome::Committed {
                                        continuation: Some(returned),
                                    } = result
                                    else {
                                        panic!(
                                            "the accepted transition must return its selected \
                                             continuation"
                                        )
                                    };
                                    assert_eq!(returned, fixture.continuation);
                                    assert_eq!(fixture.counts(), expected);
                                    let state = fixture.store.state.read().unwrap();
                                    assert!(state.data.pending_delta() == delta.delta());
                                    assert_eq!(
                                        state.data.distinct_len(),
                                        expected.values().filter(|count| **count != 0).count()
                                    );
                                    let actual = state.snapshot().unwrap();
                                    let expected_resting = usize::from(persist);
                                    assert_eq!(
                                        actual.continuations[&fixture.channels].len(),
                                        expected_resting
                                    );
                                    let expected_joins = if persist { 1 } else { 0 };
                                    for channel in &fixture.channels {
                                        assert_eq!(
                                            actual.joins.get(channel).map_or(0, Vec::len),
                                            expected_joins
                                        );
                                    }
                                    assert_eq!(state.data.count_path(&fixture.paths[4]), 2);
                                    committed += 1;
                                }
                                OracleOutcome::Overflow => unreachable!(),
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(committed > 0);
    assert!(stale > 0);
    assert!(valid_first_stale_second > 0);
    assert_extracted_commit_obligations_have_stable_normal_suite_bindings();
}

fn apply_frozen_split_predecessor(fixture: &Fixture, remove_joins: bool, publish_arrival: bool) {
    let selected = fixture.selected_commit();
    assert_eq!(
        fixture
            .store
            .remove_continuation(&fixture.channels, selected.address.index),
        Some(())
    );
    let removals = transition_delta(&fixture.paths, 1, 1, 0);
    fixture.store.apply_datum_delta(removals.delta()).unwrap();
    if remove_joins {
        for channel in &fixture.channels {
            assert_eq!(fixture.store.remove_join(channel, &fixture.channels), Some(()));
        }
    }
    if publish_arrival {
        let arrival = Datum::create(&fixture.channels[1], "arrival".to_owned(), false);
        fixture.store.put_datum(&fixture.channels[1], arrival);
    }
}

#[test]
fn frozen_split_predecessor_agrees_with_atomic_success_and_mutants_diverge() {
    let mut compared = 0;
    for left_count in 1..=3 {
        for right_count in 1..=3 {
            for arrival_count in 0..=2 {
                let atomic = Fixture::new(left_count, right_count, arrival_count, false);
                let transition = atomic.transition(transition_delta(&atomic.paths, 1, 1, 1));
                let result = atomic.store.apply_accepted_match(transition).unwrap();
                let AcceptedMatchOutcome::Committed {
                    continuation: Some(returned),
                } = result
                else {
                    panic!("the atomic transition must commit its selected continuation")
                };
                assert_eq!(returned, atomic.continuation);

                let predecessor = Fixture::new(left_count, right_count, arrival_count, false);
                apply_frozen_split_predecessor(&predecessor, true, true);
                let atomic_state = atomic.store.snapshot();
                let predecessor_state = predecessor.store.snapshot();
                assert_eq!(atomic_state.data, predecessor_state.data);
                assert_eq!(atomic_state.continuations, predecessor_state.continuations);
                assert_eq!(
                    atomic_state.installed_continuations,
                    predecessor_state.installed_continuations
                );
                assert_eq!(atomic_state.joins, predecessor_state.joins);
                assert_eq!(atomic_state.installed_joins, predecessor_state.installed_joins);
                assert_eq!(atomic.counts(), predecessor.counts());
                let atomic_journal = atomic.store.state.read().unwrap().pending_journal();
                assert!(
                    predecessor
                        .store
                        .state
                        .read()
                        .unwrap()
                        .matches_pending_journal(&atomic_journal),
                    "the frozen predecessor must match all five atomic journal domains"
                );
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 27);

    let without_joins = Fixture::new(1, 1, 0, false);
    apply_frozen_split_predecessor(&without_joins, false, true);
    let atomic_joins = Fixture::new(1, 1, 0, false);
    let transition = atomic_joins.transition(transition_delta(&atomic_joins.paths, 1, 1, 1));
    assert!(matches!(
        atomic_joins.store.apply_accepted_match(transition).unwrap(),
        AcceptedMatchOutcome::Committed { .. }
    ));
    assert_ne!(without_joins.store.snapshot().joins, atomic_joins.store.snapshot().joins);

    let without_arrival = Fixture::new(1, 1, 0, false);
    apply_frozen_split_predecessor(&without_arrival, true, false);
    assert_ne!(without_arrival.counts(), atomic_joins.counts());
}

#[test]
fn another_continuation_keeps_joins_when_atomic_match_commits() {
    fn with_other_continuation() -> Fixture {
        let fixture = Fixture::new(1, 1, 0, false);
        let mut other = stored_continuation(false);
        other.continuation = Arc::new("other-continuation".to_owned());
        other.source.hash = Blake2b256Hash::from_bytes(vec![8; 32]);
        assert_eq!(fixture.store.put_continuation(&fixture.channels, other), Some(true));
        fixture
    }

    let atomic = with_other_continuation();
    let transition = atomic.transition(transition_delta(&atomic.paths, 1, 1, 1));
    assert!(matches!(
        atomic.store.apply_accepted_match(transition).unwrap(),
        AcceptedMatchOutcome::Committed { .. }
    ));
    let atomic_state = atomic.store.snapshot();
    assert_eq!(atomic_state.continuations[&atomic.channels].len(), 1);
    for channel in &atomic.channels {
        assert_eq!(atomic_state.joins[channel], vec![atomic.channels.clone()]);
    }

    let predecessor = with_other_continuation();
    apply_frozen_split_predecessor(&predecessor, true, true);
    let predecessor_state = predecessor.store.snapshot();
    assert_eq!(atomic_state.data, predecessor_state.data);
    assert_eq!(atomic_state.continuations, predecessor_state.continuations);
    assert_eq!(atomic_state.installed_continuations, predecessor_state.installed_continuations);
    assert_eq!(atomic_state.joins, predecessor_state.joins);
    assert_eq!(atomic_state.installed_joins, predecessor_state.installed_joins);
    let atomic_journal = atomic.store.state.read().unwrap().pending_journal();
    let predecessor_journal = predecessor.store.state.read().unwrap().pending_journal();
    assert_eq!(atomic_journal.data, predecessor_journal.data);
    assert_eq!(atomic_journal.continuations, predecessor_journal.continuations);
    assert!(
        atomic_journal
            .continuation_content
            .as_ref()
            .unwrap()
            .iter()
            .eq(predecessor_journal
                .continuation_content
                .as_ref()
                .unwrap()
                .iter())
    );
    assert!(
        atomic_journal
            .continuation_source_bytes
            .as_ref()
            .unwrap()
            .iter()
            .eq(predecessor_journal
                .continuation_source_bytes
                .as_ref()
                .unwrap()
                .iter())
    );
    assert!(atomic_journal.joins.as_ref().unwrap().is_empty());
    assert_eq!(atomic_journal.joins, predecessor_journal.joins);

    assert!(
        predecessor
            .store
            .state
            .read()
            .unwrap()
            .matches_pending_journal(&atomic_journal)
    );

    // Deliberately bypass the historical guard to show that unconditional
    // join removal would be visible even though another continuation remains.
    let unconditional_removal = with_other_continuation();
    apply_frozen_split_predecessor(&unconditional_removal, false, true);
    {
        let mut state = unconditional_removal.store.state.write().unwrap();
        for channel in &unconditional_removal.channels {
            assert!(
                state
                    .joins
                    .remove(channel, &unconditional_removal.channels)
                    .is_some()
            );
        }
        state.advance_journal_version();
    }
    let mutant_state = unconditional_removal.store.snapshot();
    assert_eq!(atomic_state.data, mutant_state.data);
    assert_eq!(atomic_state.continuations, mutant_state.continuations);
    assert_ne!(atomic_state.joins, mutant_state.joins);
    assert!(
        !unconditional_removal
            .store
            .state
            .read()
            .unwrap()
            .matches_pending_journal(&atomic_journal)
    );
}

#[test]
fn frozen_split_delta_refusal_exposes_early_continuation_removal() {
    for (right_count, arrival_count, overflow) in [(0, 1, false), (1, u64::MAX, true)] {
        let atomic = Fixture::new(1, right_count, arrival_count, false);
        let delta = transition_delta(&atomic.paths, 1, 1, 1);
        let before = atomic.boundary(false);
        let result = atomic.store.apply_accepted_match(atomic.transition(delta));
        if overflow {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(AcceptedMatchOutcome::Stale)));
        }
        atomic.assert_unchanged(&before);

        let predecessor = Fixture::new(1, right_count, arrival_count, false);
        let split_before = predecessor.boundary(false);
        let selected = predecessor.selected_commit();
        assert_eq!(
            predecessor
                .store
                .remove_continuation(&predecessor.channels, selected.address.index),
            Some(())
        );
        let split_delta = transition_delta(&predecessor.paths, 1, 1, 1);
        assert!(
            predecessor
                .store
                .apply_datum_delta(split_delta.delta())
                .is_err()
        );
        let split_after = predecessor.boundary(false);
        assert_eq!(split_after.counts, before.counts);
        assert_eq!(split_after.counts, split_before.counts);
        assert_eq!(split_after.loaded_channels, before.loaded_channels);
        assert_eq!(split_after.distinct, before.distinct);
        assert_eq!(split_after.cardinality, before.cardinality);
        assert_ne!(split_after.continuations, before.continuations);
        assert_eq!(split_after.installed_continuations, before.installed_continuations);
        assert_eq!(split_after.joins, before.joins);
        assert_eq!(split_after.installed_joins, before.installed_joins);
        assert_eq!(split_after.journal.data, before.journal.data);
        assert_ne!(split_after.journal.continuations, before.journal.continuations);
        assert_eq!(split_after.journal.joins, before.journal.joins);
        assert!(
            !predecessor
                .store
                .state
                .read()
                .unwrap()
                .matches_pending_journal(&before.journal)
        );

        let without_early_remove = Fixture::new(1, right_count, arrival_count, false);
        let control_before = without_early_remove.boundary(false);
        let control_delta = transition_delta(&without_early_remove.paths, 1, 1, 1);
        assert!(
            without_early_remove
                .store
                .apply_datum_delta(control_delta.delta())
                .is_err()
        );
        without_early_remove.assert_unchanged(&control_before);
    }
}

#[test]
fn exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch() {
    for refusal in 0..3 {
        let fixture = Fixture::new(1, 1, 1, false);
        let delta = transition_delta(&fixture.paths, 1, 1, 1);
        let mut transition = fixture.transition(delta);
        if refusal == 0 {
            transition.continuation.as_mut().unwrap().source.hash =
                Blake2b256Hash::from_bytes(vec![8; 32]);
        } else if refusal == 1 {
            transition.required_persistent_paths = vec![Arc::from([0xff_u8, 0x00])];
        } else {
            let mut replacement = stored_continuation(true);
            replacement.patterns = Arc::new(vec!["replacement-pattern".to_owned()]);
            replacement.continuation = Arc::new("replacement-continuation".to_owned());
            replacement.source.hash = Blake2b256Hash::from_bytes(vec![9; 32]);
            assert_eq!(
                fixture
                    .store
                    .put_continuation(&fixture.channels, replacement),
                Some(true)
            );
        }
        let before = fixture.boundary(true);
        let result = fixture.store.apply_accepted_match(transition).unwrap();
        assert!(matches!(result, AcceptedMatchOutcome::Stale));
        fixture.assert_unchanged(&before);
    }
}

#[test]
fn stale_selected_nonpersistent_or_persistent_path_preserves_all_other_changes() {
    for selected in [0usize, 3] {
        let fixture = Fixture::new(1, 1, 1, false);
        let delta = transition_delta(&fixture.paths, 1, 1, 1);
        let transition = fixture.transition(delta);
        fixture
            .store
            .remove_datum_path(&fixture.paths[selected])
            .unwrap();
        let before = fixture.boundary(true);
        let result = fixture.store.apply_accepted_match(transition).unwrap();
        assert!(matches!(result, AcceptedMatchOutcome::Stale));
        fixture.assert_unchanged(&before);
    }
}

#[test]
fn unrelated_mutation_does_not_invalidate_an_exact_selected_match() {
    let fixture = Fixture::new(1, 1, 1, false);
    let delta = transition_delta(&fixture.paths, 1, 1, 1);
    let transition = fixture.transition(delta);
    let unrelated = Datum::create(&"unrelated".to_owned(), "unrelated".to_owned(), false);
    fixture.store.put_datum(&"unrelated".to_owned(), unrelated);
    assert_eq!(fixture.counts()[&fixture.paths[4]], 3);
    let result = fixture.store.apply_accepted_match(transition).unwrap();
    let AcceptedMatchOutcome::Committed {
        continuation: Some(returned),
    } = result
    else {
        panic!("an unrelated mutation must not invalidate the selected continuation")
    };
    assert_eq!(returned, fixture.continuation);
    assert_eq!(fixture.counts()[&fixture.paths[4]], 3);
    assert_eq!(fixture.counts()[&fixture.paths[0]], 0);
    assert_eq!(fixture.counts()[&fixture.paths[1]], 0);
    assert_eq!(fixture.counts()[&fixture.paths[2]], 2);
}

#[test]
fn arrival_cancellation_does_not_authorize_a_stale_stored_selection() {
    let fixture = Fixture::new(1, 1, 1, false);
    let mut delta = transition_delta(&fixture.paths, 1, 1, 1);
    delta
        .record_addition(&fixture.paths[0], Multiplicity::ONE)
        .unwrap();
    assert_eq!(delta.delta().removals().count(&fixture.paths[0]), 0);
    let transition = fixture.transition(delta);
    fixture.store.remove_datum_path(&fixture.paths[0]).unwrap();
    let before = fixture.boundary(true);
    let result = fixture.store.apply_accepted_match(transition).unwrap();
    assert!(matches!(result, AcceptedMatchOutcome::Stale));
    fixture.assert_unchanged(&before);
}

#[test]
fn cancellation_matrix_checks_original_demand_and_exact_commit_state() {
    let mut refused = 0;
    let mut committed = 0;
    for demand in 1..=3 {
        for current in 0..=4 {
            for arrivals in 0..=4 {
                for persist in [false, true] {
                    let fixture = Fixture::new(demand, 1, 1, persist);
                    let mut changes = transition_delta(&fixture.paths, demand, 1, 1);
                    for _ in 0..arrivals {
                        changes
                            .record_addition(&fixture.paths[0], Multiplicity::ONE)
                            .unwrap();
                    }
                    let transition = fixture.transition(changes.clone());
                    let datum =
                        Datum::create(&fixture.channels[0], "selected-left".to_owned(), false);
                    for _ in current..demand {
                        fixture.store.remove_datum_path(&fixture.paths[0]).unwrap();
                    }
                    for _ in demand..current {
                        fixture.store.put_datum(&fixture.channels[0], datum.clone());
                    }
                    let unrelated =
                        Datum::create(&"unrelated".to_owned(), "unrelated".to_owned(), false);
                    fixture.store.put_datum(&"unrelated".to_owned(), unrelated);
                    let before = fixture.boundary(true);
                    let result = fixture.store.apply_accepted_match(transition).unwrap();
                    if current < demand {
                        assert!(matches!(result, AcceptedMatchOutcome::Stale));
                        fixture.assert_unchanged(&before);
                        refused += 1;
                    } else {
                        let AcceptedMatchOutcome::Committed {
                            continuation: Some(returned),
                        } = result
                        else {
                            panic!("sufficient original demand must commit")
                        };
                        assert_eq!(returned, fixture.continuation);
                        let mut expected = before.counts.clone();
                        expected.insert(fixture.paths[0].clone(), current - demand + arrivals);
                        expected.insert(fixture.paths[1].clone(), 0);
                        expected.insert(fixture.paths[2].clone(), 2);
                        assert_eq!(fixture.counts(), expected);
                        let state = fixture.store.state.read().unwrap();
                        let mut expected_journal = before.journal.data.clone().unwrap();
                        for (path, amount) in changes.delta().removals().iter() {
                            expected_journal.record_removal(&path, amount).unwrap();
                        }
                        for (path, amount) in changes.delta().additions().iter() {
                            expected_journal.record_addition(&path, amount).unwrap();
                        }
                        assert_eq!(state.data.pending_delta(), &expected_journal);
                        assert_eq!(
                            state.data.distinct_len(),
                            expected.values().filter(|n| **n != 0).count()
                        );
                        let actual = state.snapshot().unwrap();
                        let mut expected_snapshot = before.snapshot.unwrap();
                        if !persist {
                            expected_snapshot
                                .continuations
                                .insert(fixture.channels.clone(), vec![]);
                            for channel in &fixture.channels {
                                expected_snapshot.joins.insert(channel.clone(), vec![]);
                            }
                        }
                        assert_eq!(actual.continuations, expected_snapshot.continuations);
                        assert_eq!(
                            actual.installed_continuations,
                            expected_snapshot.installed_continuations
                        );
                        assert_eq!(actual.joins, expected_snapshot.joins);
                        assert_eq!(actual.installed_joins, expected_snapshot.installed_joins);
                        committed += 1;
                    }
                }
            }
        }
    }
    assert_eq!(refused, 60);
    assert_eq!(committed, 90);
}

#[test]
fn uncancelled_changes_need_no_extra_count_queries_and_mixed_input_is_rejected() {
    let mut removals = BagDelta::new();
    removals.record_removal(b"a", Multiplicity::ONE).unwrap();
    let mut changes = AcceptedDatumChanges::from_removals(removals).unwrap();
    changes.record_addition(b"ab", Multiplicity::ONE).unwrap();
    assert!(
        changes.guards_satisfied_by(|_| panic!("uncancelled path must not need a guard lookup"))
    );
    assert!(AcceptedDatumChanges::from_removals(changes.delta().clone()).is_none());
}

#[test]
fn rejected_addition_preserves_normalized_delta_and_cancellation_guard() {
    let mut removals = BagDelta::new();
    removals.record_removal(b"a", Multiplicity::ONE).unwrap();
    let mut changes = AcceptedDatumChanges::from_removals(removals).unwrap();
    changes.record_addition(b"a", Multiplicity::ONE).unwrap();
    changes
        .record_addition(b"a", Multiplicity::new(u64::MAX).unwrap())
        .unwrap();
    let before = changes.delta().clone();
    assert!(changes.record_addition(b"a", Multiplicity::ONE).is_err());
    assert_eq!(changes.delta(), &before);
    assert!(!changes.guards_satisfied_by(|_| 0));
    assert!(changes.guards_satisfied_by(|_| 1));
}

proptest! {
    #[test]
    fn finite_additions_preserve_original_demand_at_exact_endpoints(
        demand in prop::array::uniform4(0_u64..32),
        counts in prop::array::uniform4(0_u64..64),
        additions in prop::collection::vec((0_usize..4, 1_u64..32), 0..64),
    ) {
        let paths: [&[u8]; 4] = [b"", b"a", b"ab", b"b"];
        let mut removals = BagDelta::new();
        for (path, amount) in paths.iter().zip(demand) {
            if let Ok(amount) = Multiplicity::new(amount) {
                removals.record_removal(path, amount).unwrap();
            }
        }
        let mut changes = AcceptedDatumChanges::from_removals(removals).unwrap();
        let mut totals = [0_u64; 4];
        for step in 0..=additions.len() {
            if step != 0 {
                let (index, amount) = additions[step - 1];
                changes.record_addition(paths[index], Multiplicity::new(amount).unwrap()).unwrap();
                totals[index] += amount;
            }
            for index in 0..4 {
                prop_assert_eq!(changes.delta().removals().count(paths[index]), demand[index].saturating_sub(totals[index]));
                prop_assert_eq!(changes.delta().additions().count(paths[index]), totals[index].saturating_sub(demand[index]));
            }
            let mut queried = BTreeSet::new();
            let guards_pass = changes.guards_satisfied_by(|path| {
                let index = paths.iter().position(|key| *key == path).unwrap();
                assert!(demand[index] != 0 && totals[index] != 0);
                assert!(queried.insert(index));
                counts[index]
            });
            let net_pass = (0..4).all(|i| counts[i] >= changes.delta().removals().count(paths[i]));
            prop_assert_eq!(guards_pass && net_pass, (0..4).all(|i| counts[i] >= demand[i]));
            if guards_pass {
                prop_assert_eq!(queried.len(), (0..4).filter(|i| demand[*i] != 0 && totals[*i] != 0).count());
            }
        }
    }
}

#[test]
fn valid_first_path_then_malformed_path_is_failure_atomic() {
    let fixture = Fixture::new(1, 1, 1, false);
    let mut delta = transition_delta(&fixture.paths, 1, 1, 1);
    delta.record_addition(&[0xff], Multiplicity::ONE).unwrap();
    let transition = fixture.transition(delta);
    let before = fixture.boundary(true);
    assert!(fixture.store.apply_accepted_match(transition).is_err());
    fixture.assert_unchanged(&before);
}

#[test]
fn maximal_arrival_overflow_is_failure_atomic_without_multiplicity_projection() {
    let fixture = Fixture::new(1, 1, u64::MAX, false);
    let delta = transition_delta(&fixture.paths, 1, 1, 1);
    assert!(matches!(
        sparse_count_oracle(&fixture.initial_counts, delta.delta()),
        OracleOutcome::Overflow
    ));
    let transition = fixture.transition(delta);
    let before = fixture.boundary(false);
    assert!(fixture.store.apply_accepted_match(transition).is_err());
    fixture.assert_unchanged(&before);
}

#[test]
fn maximal_arrival_boundary_commits_exactly_once_without_projection() {
    let fixture = Fixture::new(1, 1, u64::MAX - 1, false);
    let delta = transition_delta(&fixture.paths, 1, 1, 1);
    let expected = match sparse_count_oracle(&fixture.initial_counts, delta.delta()) {
        OracleOutcome::Committed(counts) => counts,
        _ => panic!("maximal valid arrival should be admitted"),
    };
    let transition = fixture.transition(delta.clone());
    let result = fixture.store.apply_accepted_match(transition).unwrap();
    let AcceptedMatchOutcome::Committed {
        continuation: Some(returned),
    } = result
    else {
        panic!("the maximal valid arrival must return its selected continuation")
    };
    assert_eq!(returned, fixture.continuation);
    assert_eq!(fixture.counts(), expected);
    let state = fixture.store.state.read().unwrap();
    assert!(state.data.pending_delta() == delta.delta());
    assert_eq!(state.data.distinct_len(), expected.values().filter(|count| **count != 0).count());
    let continuations: BTreeMap<_, _> = state
        .continuations
        .loaded_records()
        .into_iter()
        .map(|(channels, records)| (channels.to_vec(), records))
        .collect();
    assert_eq!(continuations.get(&fixture.channels), Some(&Vec::new()));
    assert!(state.continuations.installed_records().is_empty());
    let joins: BTreeMap<_, _> = state.joins.loaded_records().into_iter().collect();
    for channel in &fixture.channels {
        assert_eq!(joins.get(channel), Some(&Vec::new()));
    }
    assert!(state.joins.installed_records().is_empty());
}

type TestHotStore = dyn HotStore<String, String, String, String>;
type SpaceStoreCell = Arc<RwLock<Arc<Box<TestHotStore>>>>;
type ProduceOutcome = MaybeProduceResult<String, String, String, String>;
type ConsumeOutcome = MaybeConsumeResult<String, String, String, String>;
type FiredContinuation = ContResult<String, String, String>;
type ConsumedDatum = RSpaceResult<String, String>;

const RESTING_DATUM: &str = "selected-left";
const ARRIVING_DATUM: &str = "selected-right";
const UNRELATED: &str = "unrelated";
const SELECTED_CONTINUATION: &str = "selected";
const RIVAL_CONTINUATION: &str = "rival";
const WILDCARD_PATTERN: &str = "wildcard";
const BLOCKED_PATTERN: &str = "blocked";

fn rendezvous_group() -> Vec<String> { vec!["left".to_owned(), "right".to_owned()] }

fn waiting_on_group(
    pattern: &str,
    continuation: &str,
    persist: bool,
) -> WaitingContinuation<String, String> {
    WaitingContinuation::create(
        &rendezvous_group(),
        &vec![pattern.to_owned(); 2],
        &continuation.to_owned(),
        persist,
        BTreeSet::new(),
    )
}

#[derive(Debug, PartialEq)]
struct MatchProgress {
    resting_datum: bool,
    stored_arrival: bool,
    selected_continuation: bool,
    join: bool,
}

struct StoreImage {
    state: TestSnapshot,
    journal: PendingDeltas,
}

impl StoreImage {
    fn of(store: &TestHotStore) -> Self {
        Self {
            state: store.snapshot(),
            journal: store
                .pending_deltas()
                .expect("the hot store exposes its pending journals"),
        }
    }

    fn first_difference(&self, other: &Self) -> Option<&'static str> {
        fn same_entries<V>(left: &Option<PathMap<V>>, right: &Option<PathMap<V>>) -> bool
        where V: Clone + PartialEq + Send + Sync + Unpin {
            match (left, right) {
                (Some(left), Some(right)) => left.iter().eq(right.iter()),
                (None, None) => true,
                _ => false,
            }
        }

        let (state, journal) = (&self.state, &self.journal);
        [
            ("data", state.data == other.state.data),
            ("continuations", state.continuations == other.state.continuations),
            (
                "installed continuations",
                state.installed_continuations == other.state.installed_continuations,
            ),
            ("joins", state.joins == other.state.joins),
            ("installed joins", state.installed_joins == other.state.installed_joins),
            ("data journal", journal.data == other.journal.data),
            ("continuation journal", journal.continuations == other.journal.continuations),
            (
                "continuation content journal",
                same_entries(&journal.continuation_content, &other.journal.continuation_content),
            ),
            (
                "continuation source journal",
                same_entries(
                    &journal.continuation_source_bytes,
                    &other.journal.continuation_source_bytes,
                ),
            ),
            ("join journal", journal.joins == other.journal.joins),
        ]
        .into_iter()
        .find_map(|(domain, equal)| (!equal).then_some(domain))
    }

    fn match_progress(&self) -> MatchProgress {
        let group = rendezvous_group();
        MatchProgress {
            resting_datum: self
                .state
                .data
                .get(&group[0])
                .is_some_and(|data| data.iter().any(|datum| *datum.a == RESTING_DATUM)),
            stored_arrival: self
                .state
                .data
                .get(&group[1])
                .is_some_and(|data| !data.is_empty()),
            selected_continuation: self.state.continuations.get(&group).is_some_and(|waiting| {
                waiting
                    .iter()
                    .any(|continuation| *continuation.continuation == SELECTED_CONTINUATION)
            }),
            join: self
                .state
                .joins
                .get(&group[1])
                .is_some_and(|joins| joins.contains(&group)),
        }
    }
}

#[derive(Debug, PartialEq)]
enum EventOrderDefect {
    RefusedCommitPublishedComm,
    RefusedCommitChanged(&'static str),
    CommPrecededCompleteCommit(&'static str),
    AcceptedCommitPublished(usize),
    AcceptedCommitChangedNothing,
}

fn event_order_defect(
    must_commit: bool,
    before: &StoreImage,
    published: &[&StoreImage],
    after: &StoreImage,
) -> Option<EventOrderDefect> {
    match (must_commit, published) {
        (false, []) => before
            .first_difference(after)
            .map(EventOrderDefect::RefusedCommitChanged),
        (false, _) => Some(EventOrderDefect::RefusedCommitPublishedComm),
        (true, [image]) => match (image.first_difference(after), before.first_difference(after)) {
            (Some(domain), _) => Some(EventOrderDefect::CommPrecededCompleteCommit(domain)),
            (None, None) => Some(EventOrderDefect::AcceptedCommitChangedNothing),
            (None, Some(_)) => None,
        },
        (true, images) => Some(
            images
                .iter()
                .find_map(|image| image.first_difference(after))
                .map_or(
                    EventOrderDefect::AcceptedCommitPublished(images.len()),
                    EventOrderDefect::CommPrecededCompleteCommit,
                ),
        ),
    }
}

struct ObservedSelection {
    continuation: String,
    matched: Vec<String>,
    published_before: usize,
    image: StoreImage,
}

struct ObservedPublication {
    comm: COMM,
    image: StoreImage,
}

#[derive(Default)]
struct CommitObservations {
    arriving_rival: Option<WaitingContinuation<String, String>>,
    selections: Vec<ObservedSelection>,
    publications: Vec<ObservedPublication>,
}

#[derive(Default)]
struct CommitProbe {
    store: OnceLock<SpaceStoreCell>,
    observations: Mutex<CommitObservations>,
}

impl CommitProbe {
    fn attach(&self, store: SpaceStoreCell) {
        assert!(self.store.set(store).is_ok(), "a commit probe observes exactly one space");
    }

    fn hot_store(&self) -> Arc<Box<TestHotStore>> {
        self.store
            .get()
            .expect("the commit probe is attached to its space")
            .read()
            .expect("space store lock")
            .clone()
    }

    fn image(&self) -> StoreImage { StoreImage::of(&**self.hot_store()) }

    fn observations(&self) -> MutexGuard<'_, CommitObservations> {
        self.observations.lock().expect("commit observations lock")
    }

    fn publish(&self, comm: &COMM) {
        let image = self.image();
        self.observations().publications.push(ObservedPublication {
            comm: comm.clone(),
            image,
        });
    }
}

struct ProbeHooks(Arc<CommitProbe>);

impl Match<String, String, String> for ProbeHooks {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        (pattern == WILDCARD_PATTERN).then(|| datum.clone())
    }

    fn check_commit(&self, continuation: &String, matched: &[&String]) -> bool {
        let mut observations = self.0.observations();
        if let Some(rival) = observations.arriving_rival.take() {
            assert_eq!(
                self.0
                    .hot_store()
                    .put_continuation(&rendezvous_group(), rival),
                Some(true),
                "the arriving rival is a new continuation of the selected group"
            );
        }
        let published_before = observations.publications.len();
        observations.selections.push(ObservedSelection {
            continuation: continuation.clone(),
            matched: matched.iter().copied().cloned().collect(),
            published_before,
            image: self.0.image(),
        });
        true
    }
}

impl StepCommObserver<String, String, String, String> for ProbeHooks {
    fn observe_comm(
        &self,
        _channels: &[String],
        _consumed: &[String],
        _patterns: &[String],
        _continuation: &String,
        comm: &COMM,
        _label: &str,
    ) {
        self.0.publish(comm);
    }
}

impl RSpaceLogger<String, String, String, String> for ProbeHooks {
    fn log_comm(
        &self,
        _data_candidates: &Vec<ConsumeCandidate<String, String>>,
        _channels: &Vec<String>,
        _wk: WaitingContinuation<String, String>,
        comm: COMM,
        _label: &str,
    ) -> COMM {
        self.0.publish(&comm);
        comm
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rival {
    Absent,
    Resting,
    Arriving,
}

#[derive(Clone, Copy, Debug)]
enum Completion {
    Produce(Rival),
    Consume,
}

#[derive(Clone, Copy, Debug)]
struct MatchScenario {
    persist: bool,
    completion: Completion,
}

const MATCH_SCENARIOS: [MatchScenario; 8] = [
    MatchScenario {
        persist: false,
        completion: Completion::Produce(Rival::Absent),
    },
    MatchScenario {
        persist: false,
        completion: Completion::Produce(Rival::Resting),
    },
    MatchScenario {
        persist: false,
        completion: Completion::Produce(Rival::Arriving),
    },
    MatchScenario {
        persist: true,
        completion: Completion::Produce(Rival::Absent),
    },
    MatchScenario {
        persist: true,
        completion: Completion::Produce(Rival::Resting),
    },
    MatchScenario {
        persist: true,
        completion: Completion::Produce(Rival::Arriving),
    },
    MatchScenario {
        persist: false,
        completion: Completion::Consume,
    },
    MatchScenario {
        persist: true,
        completion: Completion::Consume,
    },
];

#[derive(Debug, PartialEq)]
struct CommittedState {
    left_data: Vec<Datum<String>>,
    right_data: Vec<Datum<String>>,
    unrelated_data: Vec<Datum<String>>,
    continuations: Vec<WaitingContinuation<String, String>>,
    left_joins: Vec<Vec<String>>,
    right_joins: Vec<Vec<String>>,
}

impl MatchScenario {
    fn rival(self) -> Rival {
        match self.completion {
            Completion::Produce(rival) => rival,
            Completion::Consume => Rival::Absent,
        }
    }

    fn attempts(self) -> &'static [bool] {
        match self.rival() {
            Rival::Arriving => &[false, true],
            Rival::Absent | Rival::Resting => &[true],
        }
    }

    fn stores_selected_continuation(self) -> bool {
        match self.completion {
            Completion::Produce(_) => self.persist,
            Completion::Consume => false,
        }
    }

    fn joins_remain(self) -> bool {
        match self.completion {
            Completion::Produce(rival) => self.persist || rival != Rival::Absent,
            Completion::Consume => false,
        }
    }

    fn pending_progress(self) -> MatchProgress {
        match self.completion {
            Completion::Produce(_) => MatchProgress {
                resting_datum: true,
                stored_arrival: false,
                selected_continuation: true,
                join: true,
            },
            Completion::Consume => MatchProgress {
                resting_datum: true,
                stored_arrival: true,
                selected_continuation: false,
                join: false,
            },
        }
    }

    fn committed_progress(self) -> MatchProgress {
        MatchProgress {
            resting_datum: false,
            stored_arrival: false,
            selected_continuation: self.stores_selected_continuation(),
            join: self.joins_remain(),
        }
    }

    fn committed_state(self) -> CommittedState {
        let continuations = [
            (self.rival() != Rival::Absent)
                .then(|| waiting_on_group(BLOCKED_PATTERN, RIVAL_CONTINUATION, false)),
            self.stores_selected_continuation()
                .then(|| waiting_on_group(WILDCARD_PATTERN, SELECTED_CONTINUATION, true)),
        ]
        .into_iter()
        .flatten()
        .collect();
        let joins = if self.joins_remain() {
            vec![rendezvous_group()]
        } else {
            Vec::new()
        };
        CommittedState {
            left_data: Vec::new(),
            right_data: Vec::new(),
            unrelated_data: vec![Datum::create(&UNRELATED.to_owned(), UNRELATED.to_owned(), false)],
            continuations,
            left_joins: joins.clone(),
            right_joins: joins,
        }
    }
}

#[derive(Debug, PartialEq)]
enum CompletionOutcome {
    Produce(ProduceOutcome),
    Consume(ConsumeOutcome),
}

impl CompletionOutcome {
    fn fired(&self) -> Option<(&FiredContinuation, &[ConsumedDatum])> {
        match self {
            Self::Produce(outcome) => outcome
                .as_ref()
                .map(|(fired, consumed, _)| (fired, consumed.as_slice())),
            Self::Consume(outcome) => outcome
                .as_ref()
                .map(|(fired, consumed)| (fired, consumed.as_slice())),
        }
    }
}

struct SideObservation {
    selections: Vec<ObservedSelection>,
    publications: Vec<ObservedPublication>,
    outcome: CompletionOutcome,
    settled: StoreImage,
    committed: CommittedState,
    rows: HashMap<Vec<String>, Row<String, String, String>>,
}

impl SideObservation {
    fn event_order_defect(&self, attempt: usize, must_commit: bool) -> Option<EventOrderDefect> {
        let selection = &self.selections[attempt];
        let (after, published_until) = match self.selections.get(attempt + 1) {
            Some(next) => (&next.image, next.published_before),
            None => (&self.settled, self.publications.len()),
        };
        let published = self.publications[selection.published_before..published_until]
            .iter()
            .map(|publication| &publication.image)
            .collect::<Vec<_>>();
        event_order_defect(must_commit, &selection.image, &published, after)
    }

    fn published_comms(&self) -> Vec<COMM> {
        self.publications
            .iter()
            .map(|publication| publication.comm.clone())
            .collect()
    }
}

struct ScenarioObservation {
    staged_root: Blake2b256Hash,
    play: SideObservation,
    play_log: Vec<Event>,
    play_root: Blake2b256Hash,
    replay: SideObservation,
    replay_data_check: Result<(), RSpaceError>,
    replay_data_exhausted: bool,
    replay_root: Result<Blake2b256Hash, RSpaceError>,
}

async fn observe_match<S>(
    space: &S,
    probe: &CommitProbe,
    scenario: MatchScenario,
) -> SideObservation
where
    S: ISpace<String, String, String, String>,
{
    {
        let mut observations = probe.observations();
        assert!(
            observations.selections.is_empty() && observations.publications.is_empty(),
            "staging reaches no complete selection and publishes no COMM"
        );
        observations.arriving_rival = (scenario.rival() == Rival::Arriving)
            .then(|| waiting_on_group(BLOCKED_PATTERN, RIVAL_CONTINUATION, false));
    }
    let group = rendezvous_group();
    let outcome = match scenario.completion {
        Completion::Produce(_) => CompletionOutcome::Produce(
            space
                .produce(group[1].clone(), ARRIVING_DATUM.to_owned(), false)
                .await
                .expect("the produce that completes the match"),
        ),
        Completion::Consume => CompletionOutcome::Consume(
            space
                .consume(
                    group.clone(),
                    vec![WILDCARD_PATTERN.to_owned(); 2],
                    SELECTED_CONTINUATION.to_owned(),
                    scenario.persist,
                    BTreeSet::new(),
                )
                .await
                .expect("the consume that completes the match"),
        ),
    };
    let settled = probe.image();
    let mut continuations = space.get_waiting_continuations(group.clone()).await;
    continuations.sort_by(|left, right| left.continuation.cmp(&right.continuation));
    let committed = CommittedState {
        left_data: space.get_data(&group[0]).await,
        right_data: space.get_data(&group[1]).await,
        unrelated_data: space.get_data(&UNRELATED.to_owned()).await,
        continuations,
        left_joins: space.get_joins(group[0].clone()).await,
        right_joins: space.get_joins(group[1].clone()).await,
    };
    let rows = space.to_map().await;
    let CommitObservations {
        selections,
        publications,
        ..
    } = std::mem::take(&mut *probe.observations());
    SideObservation {
        selections,
        publications,
        outcome,
        settled,
        committed,
        rows,
    }
}

async fn observe_scenario(scenario: MatchScenario) -> ScenarioObservation {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    let history = Arc::new(
        HistoryRepositoryInstances::<String, String, String, String>::repository(
            stores.authority(),
        )
        .expect("history repository"),
    );
    let reader = history
        .get_history_reader(&history.root())
        .expect("history reader");
    let play_probe = Arc::new(CommitProbe::default());
    let replay_probe = Arc::new(CommitProbe::default());
    let mut play = RSpace::apply(
        history.clone(),
        HotStoreInstances::create_from_hs_and_hr(TestSnapshot::default(), reader.base()),
        Arc::new(Box::new(ProbeHooks(Arc::clone(&play_probe)))),
    );
    play.set_step_observer(Some(Arc::new(ProbeHooks(Arc::clone(&play_probe)))));
    play_probe.attach(Arc::clone(&play.store));
    let replay = ReplayRSpace::apply_with_logger(
        history,
        Arc::new(HotStoreInstances::create_from_hs_and_hr(TestSnapshot::default(), reader.base())),
        Arc::new(Box::new(ProbeHooks(Arc::clone(&replay_probe)))),
        Box::new(ProbeHooks(Arc::clone(&replay_probe))),
    );
    replay_probe.attach(Arc::clone(&replay.store));

    let group = rendezvous_group();
    let (staged_arrival, staged_selection) = match scenario.completion {
        Completion::Produce(_) => {
            (None, Some((WILDCARD_PATTERN, SELECTED_CONTINUATION, scenario.persist)))
        }
        Completion::Consume => (Some((group[1].clone(), ARRIVING_DATUM)), None),
    };
    let staged_data = [
        Some((group[0].clone(), RESTING_DATUM)),
        staged_arrival,
        Some((UNRELATED.to_owned(), UNRELATED)),
    ];
    for (channel, datum) in staged_data.into_iter().flatten() {
        let stored = play
            .produce(channel, datum.to_owned(), false)
            .await
            .expect("stage a datum");
        assert!(stored.is_none(), "a staged datum meets no continuation");
    }
    let staged_continuations = [
        staged_selection,
        (scenario.rival() == Rival::Resting).then_some((
            BLOCKED_PATTERN,
            RIVAL_CONTINUATION,
            false,
        )),
    ];
    for (pattern, continuation, persist) in staged_continuations.into_iter().flatten() {
        let waiting = play
            .consume(
                group.clone(),
                vec![pattern.to_owned(); 2],
                continuation.to_owned(),
                persist,
                BTreeSet::new(),
            )
            .await
            .expect("stage a continuation");
        assert!(waiting.is_none(), "a staged continuation finds no complete match");
    }
    let staged = play
        .create_checkpoint()
        .await
        .expect("checkpoint of the staged state");

    let play_side = observe_match(&play, &play_probe, scenario).await;
    let played = play
        .create_checkpoint()
        .await
        .expect("play checkpoint after the match");

    replay
        .rig_and_reset(staged.root.clone(), played.log.clone())
        .await
        .expect("rig replay with play's log at the staged root");
    let replay_side = observe_match(&replay, &replay_probe, scenario).await;
    let replay_data_check = replay.check_replay_data().await;
    let replay_data_exhausted = replay
        .replay_data
        .lock()
        .expect("replay data lock")
        .is_empty();
    let replay_root = replay
        .create_checkpoint()
        .await
        .map(|checkpoint| checkpoint.root);

    ScenarioObservation {
        staged_root: staged.root,
        play: play_side,
        play_log: played.log,
        play_root: played.root,
        replay: replay_side,
        replay_data_check,
        replay_data_exhausted,
        replay_root,
    }
}

fn run_to_completion<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
        .block_on(future)
}

fn logged_comms(log: &[Event]) -> Vec<COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm.clone()),
            Event::IoEvent(_) => None,
        })
        .collect()
}

enum CommitOrder {
    AtomicCommitThenComm,
    CommThenFrozenSplit,
}

struct FixtureTrace {
    before: StoreImage,
    published: Vec<StoreImage>,
    after: StoreImage,
}

impl FixtureTrace {
    fn event_order_defect(&self, must_commit: bool) -> Option<EventOrderDefect> {
        let published = self.published.iter().collect::<Vec<_>>();
        event_order_defect(must_commit, &self.before, &published, &self.after)
    }
}

fn fixture_commit_trace(order: CommitOrder, stale_selection: bool) -> FixtureTrace {
    let fixture = Fixture::new(1, 1, 0, false);
    let ticket = fixture.transition(transition_delta(&fixture.paths, 1, 1, 0));
    if stale_selection {
        assert_eq!(
            fixture.store.put_continuation(
                &fixture.channels,
                waiting_on_group(BLOCKED_PATTERN, RIVAL_CONTINUATION, false)
            ),
            Some(true),
            "the rival is a new continuation of the selected group"
        );
    }
    let before = StoreImage::of(&fixture.store);
    let published = match order {
        CommitOrder::AtomicCommitThenComm => {
            let outcome = fixture
                .store
                .apply_accepted_match(ticket)
                .expect("the atomic transaction decides the ticket");
            match outcome {
                AcceptedMatchOutcome::Committed { .. } => vec![StoreImage::of(&fixture.store)],
                AcceptedMatchOutcome::Stale => Vec::new(),
            }
        }
        CommitOrder::CommThenFrozenSplit => {
            let published = vec![StoreImage::of(&fixture.store)];
            apply_frozen_split_predecessor(&fixture, true, false);
            published
        }
    };
    FixtureTrace {
        before,
        published,
        after: StoreImage::of(&fixture.store),
    }
}

#[test]
fn comm_event_is_logged_only_after_the_complete_commit() {
    for scenario in MATCH_SCENARIOS {
        let observation = run_to_completion(observe_scenario(scenario));
        for (side, observed) in [("play", &observation.play), ("replay", &observation.replay)] {
            assert_eq!(
                observed.selections.len(),
                scenario.attempts().len(),
                "{side} {scenario:?}: one commit attempt follows each complete selection"
            );
            for (attempt, must_commit) in scenario.attempts().iter().enumerate() {
                assert_eq!(
                    observed.selections[attempt].image.match_progress(),
                    scenario.pending_progress(),
                    "{side} {scenario:?} attempt {attempt}: the store holds the initial state \
                     until the commit is decided"
                );
                assert_eq!(
                    observed.event_order_defect(attempt, *must_commit),
                    None,
                    "{side} {scenario:?} attempt {attempt}: a COMM is published only after a \
                     complete commit"
                );
            }
            let [publication] = observed.publications.as_slice() else {
                panic!(
                    "{side} {scenario:?}: the accepted match publishes one COMM, not {}",
                    observed.publications.len()
                )
            };
            assert_eq!(
                publication.image.match_progress(),
                scenario.committed_progress(),
                "{side} {scenario:?}: the store holds the complete commit when the COMM is \
                 published"
            );
            assert_eq!(
                observed.committed,
                scenario.committed_state(),
                "{side} {scenario:?}: the committed state is the complete proposal"
            );
            let Some((fired, consumed)) = observed.outcome.fired() else {
                panic!("{side} {scenario:?}: the accepted match returns its continuation")
            };
            assert_eq!(
                (fired.continuation.as_str(), fired.persistent),
                (SELECTED_CONTINUATION, scenario.persist),
                "{side} {scenario:?}: the fired continuation is the selected one"
            );
            assert_eq!(
                consumed
                    .iter()
                    .map(|datum| datum.matched_datum.as_str())
                    .collect::<Vec<_>>(),
                [RESTING_DATUM, ARRIVING_DATUM],
                "{side} {scenario:?}: the COMM consumes the selected data"
            );
        }
        assert_eq!(
            logged_comms(&observation.play_log),
            observation.play.published_comms(),
            "{scenario:?}: play's event log holds exactly the published COMM"
        );
    }

    for stale_selection in [false, true] {
        let must_commit = !stale_selection;
        let atomic = fixture_commit_trace(CommitOrder::AtomicCommitThenComm, stale_selection);
        assert_eq!(
            atomic.event_order_defect(must_commit),
            None,
            "the atomic commit publishes a COMM only after a complete commit (stale selection: \
             {stale_selection})"
        );
        let predecessor = fixture_commit_trace(CommitOrder::CommThenFrozenSplit, stale_selection);
        let reported = if stale_selection {
            EventOrderDefect::RefusedCommitPublishedComm
        } else {
            EventOrderDefect::CommPrecededCompleteCommit("data")
        };
        assert_eq!(
            predecessor.event_order_defect(must_commit),
            Some(reported),
            "the frozen split predecessor publishes its COMM before the commit (stale selection: \
             {stale_selection})"
        );
    }
}

#[test]
fn play_and_replay_apply_the_same_ticket_to_the_same_state() {
    for scenario in MATCH_SCENARIOS {
        let observation = run_to_completion(observe_scenario(scenario));
        let (play, replay) = (&observation.play, &observation.replay);
        let attempts = scenario.attempts().len();
        assert_eq!(
            (play.selections.len(), replay.selections.len()),
            (attempts, attempts),
            "{scenario:?}: play and replay make the same commit attempts"
        );
        for (attempt, (played, replayed)) in
            play.selections.iter().zip(&replay.selections).enumerate()
        {
            assert_eq!(
                (&played.continuation, &played.matched, played.published_before),
                (&replayed.continuation, &replayed.matched, replayed.published_before),
                "{scenario:?} attempt {attempt}: play and replay select the same ticket"
            );
            assert_eq!(
                played.image.first_difference(&replayed.image),
                None,
                "{scenario:?} attempt {attempt}: play and replay select from the same state"
            );
        }
        assert!(play.outcome.fired().is_some(), "{scenario:?}: the match fires");
        assert_eq!(
            play.outcome, replay.outcome,
            "{scenario:?}: play and replay return the same result"
        );
        let logged = logged_comms(&observation.play_log);
        assert_eq!(logged.len(), 1, "{scenario:?}: play logs one COMM");
        assert_eq!(play.published_comms(), logged, "{scenario:?}: play publishes the COMM it logs");
        assert_eq!(
            replay.published_comms(),
            logged,
            "{scenario:?}: replay publishes the COMM play logged"
        );
        for (played, replayed) in play.publications.iter().zip(&replay.publications) {
            assert_eq!(
                played.image.first_difference(&replayed.image),
                None,
                "{scenario:?}: play and replay publish the COMM over the same state"
            );
        }
        assert_eq!(
            play.settled.first_difference(&replay.settled),
            None,
            "{scenario:?}: play and replay leave the same state and journals"
        );
        assert!(
            play.settled
                .journal
                .data
                .as_ref()
                .is_some_and(|delta| !delta.is_empty()),
            "{scenario:?}: the compared journals record the commit"
        );
        assert_eq!(
            play.committed, replay.committed,
            "{scenario:?}: play and replay read the same committed state"
        );
        assert_eq!(play.rows, replay.rows, "{scenario:?}: play and replay hold the same rows");
        assert_eq!(
            observation.replay_data_check,
            Ok(()),
            "{scenario:?}: replay consumed every recorded COMM"
        );
        assert!(observation.replay_data_exhausted, "{scenario:?}: the replay data is exhausted");
        assert_eq!(
            observation.replay_root.as_ref(),
            Ok(&observation.play_root),
            "{scenario:?}: play and replay reach the same checkpoint root"
        );
        assert_ne!(
            observation.play_root, observation.staged_root,
            "{scenario:?}: the match changes the checkpoint root"
        );
    }
}

fn assert_extracted_commit_obligations_have_stable_normal_suite_bindings() {
    struct Binding {
        id: &'static str,
        obligation: &'static str,
        // A proof-only row's test is a finite witness, not executable refinement of that
        // obligation.
        proof_only: bool,
        target_name: &'static str,
        target: fn(),
    }

    let rocq = [
        Binding {
            id: "ACM-RC-01",
            obligation: "transition_is_deterministic",
            proof_only: true,
            target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            target: bounded_multichannel_transition_refines_independent_sparse_count_oracle,
        },
        Binding {
            id: "ACM-RC-02",
            obligation: "refusal_is_observably_nonmutating",
            proof_only: false,
            target_name:
                "exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch",
            target: exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch,
        },
        Binding {
            id: "ACM-RC-03",
            obligation: "accepted_state_is_one_complete_proposal",
            proof_only: false,
            target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            target: bounded_multichannel_transition_refines_independent_sparse_count_oracle,
        },
        Binding {
            id: "ACM-RC-04",
            obligation: "stale_epoch_cannot_commit",
            proof_only: false,
            target_name:
                "exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch",
            target: exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch,
        },
        Binding {
            id: "ACM-RC-05",
            obligation: "missing_persistent_witness_cannot_commit",
            proof_only: false,
            target_name:
                "exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch",
            target: exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch,
        },
        Binding {
            id: "ACM-RC-06",
            obligation: "inapplicable_delta_cannot_commit",
            proof_only: false,
            target_name:
                "stale_selected_nonpersistent_or_persistent_path_preserves_all_other_changes",
            target: stale_selected_nonpersistent_or_persistent_path_preserves_all_other_changes,
        },
        Binding {
            id: "ACM-RC-07",
            obligation: "comm_publication_follows_a_complete_commit",
            proof_only: false,
            target_name: "comm_event_is_logged_only_after_the_complete_commit",
            target: comm_event_is_logged_only_after_the_complete_commit,
        },
        Binding {
            id: "ACM-RC-08",
            obligation: "unrelated_group_is_preserved",
            proof_only: false,
            target_name: "unrelated_mutation_does_not_invalidate_an_exact_selected_match",
            target: unrelated_mutation_does_not_invalidate_an_exact_selected_match,
        },
        Binding {
            id: "ACM-RC-09",
            obligation: "play_replay_same_ticket_same_result",
            proof_only: false,
            target_name: "play_and_replay_apply_the_same_ticket_to_the_same_state",
            target: play_and_replay_apply_the_same_ticket_to_the_same_state,
        },
    ];
    let tla = [
        Binding {
            id: "ACM-TLA-01",
            obligation: "TypeInvariant",
            proof_only: true,
            target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            target: bounded_multichannel_transition_refines_independent_sparse_count_oracle,
        },
        Binding {
            id: "ACM-TLA-02",
            obligation: "PlayPublishesOneCompleteState",
            proof_only: false,
            target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            target: bounded_multichannel_transition_refines_independent_sparse_count_oracle,
        },
        Binding {
            id: "ACM-TLA-03",
            obligation: "ReplayPublishesOneCompleteState",
            proof_only: false,
            target_name: "comm_event_is_logged_only_after_the_complete_commit",
            target: comm_event_is_logged_only_after_the_complete_commit,
        },
        Binding {
            id: "ACM-TLA-04",
            obligation: "RejectedTransitionIsNonMutating",
            proof_only: false,
            target_name:
                "exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch",
            target: exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch,
        },
        Binding {
            id: "ACM-TLA-05",
            obligation: "EventPublicationFollowsCommit",
            proof_only: false,
            target_name: "comm_event_is_logged_only_after_the_complete_commit",
            target: comm_event_is_logged_only_after_the_complete_commit,
        },
        Binding {
            id: "ACM-TLA-06",
            obligation: "StaleOrForgedTicketNeverCommits",
            proof_only: false,
            target_name:
                "exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch",
            target: exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch,
        },
        Binding {
            id: "ACM-TLA-07",
            obligation: "PersistentWitnessesAreRequired",
            proof_only: false,
            target_name:
                "exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch",
            target: exact_capabilities_refuse_wrong_source_missing_witness_and_stale_group_epoch,
        },
        Binding {
            id: "ACM-TLA-08",
            obligation: "CommittedMultiplicityAndArrivalAreExact",
            proof_only: false,
            target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            target: bounded_multichannel_transition_refines_independent_sparse_count_oracle,
        },
        Binding {
            id: "ACM-TLA-09",
            obligation: "ContinuationAndJoinCommitTogether",
            proof_only: false,
            target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            target: bounded_multichannel_transition_refines_independent_sparse_count_oracle,
        },
        Binding {
            id: "ACM-TLA-10",
            obligation: "PlayReplayTraceEquivalent",
            proof_only: false,
            target_name: "play_and_replay_apply_the_same_ticket_to_the_same_state",
            target: play_and_replay_apply_the_same_ticket_to_the_same_state,
        },
    ];

    let rocq_source = include_str!("../../../formal/rocq/stack_safe_pda/theories/RSpaceCommit.v");
    let tla_source = include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceBagCommit.tla");
    let tla_config = include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceBagCommit.cfg");
    let normal_suite_source = include_str!("accepted_match_formal_bindings.rs");
    let rocq_declarations: Vec<_> = rocq_source
        .lines()
        .filter_map(|line| line.strip_prefix("Theorem "))
        .map(|line| {
            line.split_whitespace()
                .next()
                .expect("Rocq theorem has a name")
        })
        .collect();
    let tla_declarations: Vec<_> = tla_config
        .lines()
        .filter_map(|line| line.strip_prefix("INVARIANT "))
        .collect();
    let mut sorted_rocq = rocq_declarations.clone();
    sorted_rocq.sort_unstable();
    assert!(
        sorted_rocq.windows(2).all(|pair| pair[0] != pair[1]),
        "duplicate Rocq theorem declaration"
    );
    let mut sorted_tla = tla_declarations.clone();
    sorted_tla.sort_unstable();
    assert!(
        sorted_tla.windows(2).all(|pair| pair[0] != pair[1]),
        "duplicate TLC invariant declaration"
    );
    let extracted_rocq: BTreeSet<_> = rocq_declarations.into_iter().collect();
    let extracted_tla: BTreeSet<_> = tla_declarations.into_iter().collect();
    assert_eq!(extracted_rocq.len(), rocq.len());
    assert_eq!(extracted_tla.len(), tla.len());
    assert!(rocq.iter().any(|binding| binding.proof_only));
    assert!(tla.iter().any(|binding| binding.proof_only));
    let mut ids = BTreeSet::new();
    for (bindings, extracted) in [(&rocq[..], &extracted_rocq), (&tla[..], &extracted_tla)] {
        let mut mapped = BTreeSet::new();
        for binding in bindings {
            assert!(
                ids.insert(binding.id),
                "duplicate accepted-match obligation ID: {}",
                binding.id
            );
            assert!(
                extracted.contains(binding.obligation),
                "unextracted obligation: {}",
                binding.id
            );
            assert!(
                mapped.insert(binding.obligation),
                "duplicate obligation binding: {}",
                binding.obligation
            );
            assert!(
                normal_suite_source.contains(&format!("#[test]\nfn {}(", binding.target_name)),
                "binding target is not a normal-suite test: {}",
                binding.id
            );
            let _executable_target = binding.target;
        }
        assert_eq!(&mapped, extracted, "model obligation has no normal-suite binding");
    }
    for obligation in &extracted_tla {
        assert!(
            tla_source.contains(&format!("{obligation} ==")),
            "TLC invariant has no model definition: {obligation}"
        );
    }
}
