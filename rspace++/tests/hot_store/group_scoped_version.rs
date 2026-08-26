use std::collections::{BTreeSet, HashMap};

use super::*;
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
