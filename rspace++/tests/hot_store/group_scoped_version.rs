use std::collections::{BTreeSet, HashMap};

use super::*;
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::trace::event::Consume;

struct EmptyHistory;

impl HistoryReaderBase<String, String, String, String> for EmptyHistory {
    fn get_data_proj(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_proj(
        &self,
        _key: &Vec<String>,
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_proj(&self, _key: &String) -> Vec<Vec<String>> { Vec::new() }
}

fn continuation(source: u8, name: &str) -> WaitingContinuation<String, String> {
    WaitingContinuation {
        patterns: Arc::new(vec![format!("pattern-{name}")]),
        continuation: Arc::new(format!("continuation-{name}")),
        persist: false,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![source; 32]),
            persistent: false,
        },
    }
}

fn assert_same_state(
    left: &HotStoreState<String, String, String, String>,
    right: &HotStoreState<String, String, String, String>,
) {
    assert_eq!(left.continuations, right.continuations);
    assert_eq!(left.installed_continuations, right.installed_continuations);
    assert_eq!(left.data, right.data);
    assert_eq!(left.joins, right.joins);
    assert_eq!(left.installed_joins, right.installed_joins);
}

#[test]
fn a_cross_group_continuation_capability_is_stale_before_any_state_change() {
    let left_channels = vec!["left".to_owned()];
    let right_channels = vec!["right".to_owned()];
    let left = continuation(1, "left");
    let right = continuation(2, "right");
    let initial = HotStoreState {
        continuations: HashMap::from([
            (left_channels.clone(), vec![left.clone()]),
            (right_channels.clone(), vec![right]),
        ]),
        installed_continuations: HashMap::new(),
        data: HashMap::new(),
        joins: HashMap::new(),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (left_version, loaded) = store.get_continuations_versioned(&left_channels);
    assert_eq!(loaded, vec![left.clone()]);
    let before_state = store.snapshot();
    let before_checkpoint = store.checkpoint_changes().expect("checkpoint projection");

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: right_channels,
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(left_version, 0),
                source: left.source,
                persist: left.persist,
                peeks: left.peeks,
            }),
            datum_delta: BagDelta::new(),
            required_persistent_paths: Vec::new(),
            join_channels: Vec::new(),
        })
        .expect("stale proposals are ordinary outcomes");

    assert!(matches!(outcome, AcceptedMatchOutcome::Stale));
    assert_same_state(&before_state, &store.snapshot());
    assert_eq!(before_checkpoint, store.checkpoint_changes().expect("checkpoint projection"));
}

#[test]
fn an_accepted_match_atomically_removes_the_selected_datum_continuation_and_join() {
    let channels = vec!["channel".to_owned()];
    let datum = Datum::create(&channels[0], "payload".to_owned(), false);
    let continuation = continuation(3, "selected");
    let initial = HotStoreState {
        continuations: HashMap::from([(channels.clone(), vec![continuation.clone()])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(channels[0].clone(), vec![datum.clone()])]),
        joins: HashMap::from([(channels[0].clone(), vec![channels.clone()])]),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (version, loaded) = store.get_continuations_versioned(&channels);
    assert_eq!(loaded, vec![continuation.clone()]);

    let datum_path = RSpaceDataIndex::datum_path(&channels[0], &datum);
    let mut datum_delta = BagDelta::new();
    datum_delta
        .record_removal(&datum_path, super::super::bag::Multiplicity::ONE)
        .expect("fixture multiplicity fits u64");
    let before = store.snapshot();

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, 0),
                source: continuation.source.clone(),
                persist: continuation.persist,
                peeks: continuation.peeks.clone(),
            }),
            datum_delta,
            required_persistent_paths: Vec::new(),
            join_channels: channels.clone(),
        })
        .expect("the accepted transition is valid");

    assert!(matches!(outcome, AcceptedMatchOutcome::Committed { continuation: Some(_) }));
    let after = store.snapshot();
    assert_ne!(before.data, after.data);
    assert_eq!(
        after.data.values().map(Vec::len).sum::<usize>(),
        0,
        "the selected datum must be consumed"
    );
    assert_eq!(
        after.continuations.values().map(Vec::len).sum::<usize>(),
        0,
        "the one-shot continuation must be consumed"
    );
    assert_eq!(
        after.joins.values().map(Vec::len).sum::<usize>(),
        0,
        "the now-empty join group must be removed"
    );
    assert!(after.installed_continuations.is_empty());
    assert!(after.installed_joins.is_empty());
}

#[test]
fn an_accepted_multichannel_match_preserves_unconsumed_datum_multiplicity() {
    let channels = vec!["left".to_owned(), "right".to_owned()];
    let left = Datum::create(&channels[0], "repeated".to_owned(), false);
    let right = Datum::create(&channels[1], "single".to_owned(), false);
    let selected = continuation(4, "multichannel");
    let initial = HotStoreState {
        continuations: HashMap::from([(channels.clone(), vec![selected.clone()])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([
            (channels[0].clone(), vec![left.clone(), left.clone()]),
            (channels[1].clone(), vec![right.clone()]),
        ]),
        joins: HashMap::from([
            (channels[0].clone(), vec![channels.clone()]),
            (channels[1].clone(), vec![channels.clone()]),
        ]),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (version, loaded) = store.get_continuations_versioned(&channels);
    assert_eq!(loaded, vec![selected.clone()]);

    let mut datum_delta = BagDelta::new();
    for (channel, datum) in [(&channels[0], &left), (&channels[1], &right)] {
        datum_delta
            .record_removal(
                &RSpaceDataIndex::datum_path(channel, datum),
                super::super::bag::Multiplicity::ONE,
            )
            .expect("fixture multiplicities fit u64");
    }
    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, 0),
                source: selected.source.clone(),
                persist: selected.persist,
                peeks: selected.peeks.clone(),
            }),
            datum_delta,
            required_persistent_paths: Vec::new(),
            join_channels: channels.clone(),
        })
        .expect("the multichannel transition is valid");

    assert!(matches!(outcome, AcceptedMatchOutcome::Committed { continuation: Some(_) }));
    let after = store.snapshot();
    assert_eq!(after.data.get(&channels[0]).map(Vec::len), Some(1));
    assert_eq!(after.data.get(&channels[1]).map(Vec::len), Some(0));
    assert_eq!(after.continuations.values().map(Vec::len).sum::<usize>(), 0);
    assert_eq!(after.joins.values().map(Vec::len).sum::<usize>(), 0);
}

#[test]
fn an_insufficient_datum_delta_is_stale_and_failure_atomic() {
    let channels = vec!["channel".to_owned()];
    let datum = Datum::create(&channels[0], "payload".to_owned(), false);
    let selected = continuation(5, "underflow");
    let initial = HotStoreState {
        continuations: HashMap::from([(channels.clone(), vec![selected.clone()])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(channels[0].clone(), vec![datum.clone()])]),
        joins: HashMap::new(),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (version, _) = store.get_continuations_versioned(&channels);
    let datum_path = RSpaceDataIndex::datum_path(&channels[0], &datum);
    let mut datum_delta = BagDelta::new();
    datum_delta
        .record_removal(
            &datum_path,
            super::super::bag::Multiplicity::new(2).expect("nonzero fixture multiplicity"),
        )
        .expect("fixture multiplicity fits u64");
    let before = store.snapshot();
    let before_checkpoint = store.checkpoint_changes().expect("checkpoint projection");

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, 0),
                source: selected.source.clone(),
                persist: selected.persist,
                peeks: selected.peeks.clone(),
            }),
            datum_delta,
            required_persistent_paths: Vec::new(),
            join_channels: Vec::new(),
        })
        .expect("underflow is an ordinary stale outcome");

    assert!(matches!(outcome, AcceptedMatchOutcome::Stale));
    assert_same_state(&before, &store.snapshot());
    assert_eq!(before_checkpoint, store.checkpoint_changes().expect("checkpoint projection"));
}

#[test]
fn a_missing_required_persistent_path_is_stale_before_any_mutation() {
    let channels = vec!["channel".to_owned()];
    let datum = Datum::create(&channels[0], "payload".to_owned(), false);
    let selected = continuation(6, "missing-witness");
    let initial = HotStoreState {
        continuations: HashMap::from([(channels.clone(), vec![selected.clone()])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(channels[0].clone(), vec![datum.clone()])]),
        joins: HashMap::new(),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (version, _) = store.get_continuations_versioned(&channels);
    let mut datum_delta = BagDelta::new();
    datum_delta
        .record_removal(
            &RSpaceDataIndex::datum_path(&channels[0], &datum),
            super::super::bag::Multiplicity::ONE,
        )
        .expect("fixture multiplicity fits u64");
    let before = store.snapshot();
    let before_checkpoint = store.checkpoint_changes().expect("checkpoint projection");

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, 0),
                source: selected.source.clone(),
                persist: selected.persist,
                peeks: selected.peeks.clone(),
            }),
            datum_delta,
            required_persistent_paths: vec![Arc::from([0xFF_u8, 0x00_u8])],
            join_channels: Vec::new(),
        })
        .expect("missing witness is an ordinary stale outcome");

    assert!(matches!(outcome, AcceptedMatchOutcome::Stale));
    assert_same_state(&before, &store.snapshot());
    assert_eq!(before_checkpoint, store.checkpoint_changes().expect("checkpoint projection"));
}

#[test]
fn an_accepted_match_preserves_a_persistent_continuation() {
    let channels = vec!["persistent".to_owned()];
    let datum = Datum::create(&channels[0], "payload".to_owned(), false);
    let selected = WaitingContinuation {
        patterns: Arc::new(vec!["pattern-persistent".to_owned()]),
        continuation: Arc::new("continuation-persistent".to_owned()),
        persist: true,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![7; 32]),
            persistent: true,
        },
    };
    let initial = HotStoreState {
        continuations: HashMap::from([(channels.clone(), vec![selected.clone()])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(channels[0].clone(), vec![datum.clone()])]),
        joins: HashMap::from([(channels[0].clone(), vec![channels.clone()])]),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (version, loaded) = store.get_continuations_versioned(&channels);
    assert_eq!(loaded, vec![selected.clone()]);
    let mut datum_delta = BagDelta::new();
    datum_delta
        .record_removal(
            &RSpaceDataIndex::datum_path(&channels[0], &datum),
            super::super::bag::Multiplicity::ONE,
        )
        .expect("fixture multiplicity fits u64");

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, 0),
                source: selected.source.clone(),
                persist: selected.persist,
                peeks: selected.peeks.clone(),
            }),
            datum_delta,
            required_persistent_paths: Vec::new(),
            join_channels: channels.clone(),
        })
        .expect("the persistent transition is valid");

    assert!(matches!(outcome, AcceptedMatchOutcome::Committed { continuation: Some(_) }));
    let after = store.snapshot();
    assert_eq!(after.data.values().map(Vec::len).sum::<usize>(), 0);
    assert_eq!(after.continuations.values().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(after.joins.values().map(Vec::len).sum::<usize>(), 1);
}

#[test]
fn an_accepted_match_can_remove_the_selected_datum_and_publish_a_new_arrival() {
    let channels = vec!["arrival".to_owned()];
    let selected_datum = Datum::create(&channels[0], "selected".to_owned(), false);
    let arrival = Datum::create(&channels[0], "arrival".to_owned(), false);
    let selected = continuation(8, "arrival");
    let initial = HotStoreState {
        continuations: HashMap::from([(channels.clone(), vec![selected.clone()])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(channels[0].clone(), vec![selected_datum.clone()])]),
        joins: HashMap::new(),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let (version, _) = store.get_continuations_versioned(&channels);
    let mut datum_delta = BagDelta::new();
    datum_delta
        .record_removal(
            &RSpaceDataIndex::datum_path(&channels[0], &selected_datum),
            super::super::bag::Multiplicity::ONE,
        )
        .expect("fixture removal fits u64");
    datum_delta
        .record_addition(
            &RSpaceDataIndex::datum_path(&channels[0], &arrival),
            super::super::bag::Multiplicity::ONE,
        )
        .expect("fixture addition fits u64");

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, 0),
                source: selected.source.clone(),
                persist: selected.persist,
                peeks: selected.peeks.clone(),
            }),
            datum_delta,
            required_persistent_paths: Vec::new(),
            join_channels: Vec::new(),
        })
        .expect("the arrival transition is valid");

    assert!(matches!(outcome, AcceptedMatchOutcome::Committed { continuation: Some(_) }));
    let after = store.snapshot();
    assert_eq!(after.data.get(&channels[0]), Some(&vec![arrival]));
    assert_eq!(after.continuations.values().map(Vec::len).sum::<usize>(), 0);
}

#[test]
fn a_datum_only_accepted_match_commits_without_a_continuation_capability() {
    let channels = vec!["datum-only".to_owned()];
    let datum = Datum::create(&channels[0], "payload".to_owned(), false);
    let initial = HotStoreState {
        continuations: HashMap::new(),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(channels[0].clone(), vec![datum.clone()])]),
        joins: HashMap::new(),
        installed_joins: HashMap::new(),
    };
    let store = HotStoreInstances::create_from_hs_and_hr(initial, Box::new(EmptyHistory));
    let mut datum_delta = BagDelta::new();
    datum_delta
        .record_removal(
            &RSpaceDataIndex::datum_path(&channels[0], &datum),
            super::super::bag::Multiplicity::ONE,
        )
        .expect("fixture removal fits u64");

    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels,
            continuation: None,
            datum_delta,
            required_persistent_paths: Vec::new(),
            join_channels: Vec::new(),
        })
        .expect("datum-only transition is valid");

    assert!(matches!(outcome, AcceptedMatchOutcome::Committed { continuation: None }));
    assert_eq!(store.snapshot().data.values().map(Vec::len).sum::<usize>(), 0);
}
