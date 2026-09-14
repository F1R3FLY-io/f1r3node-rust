use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, RwLock};

use proptest::prelude::*;

use super::*;
use crate::rspace::bag::Multiplicity;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

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
        assert_eq!(loaded, vec![self.continuation.clone()]);
        StoredContinuationCommit {
            address: ContinuationAddress::new(version, 0),
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
