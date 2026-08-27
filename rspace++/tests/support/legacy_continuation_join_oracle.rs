//! Frozen HashMap/Vec continuation and join oracle for the PathMap lifecycle
//! gate.
//!
//! This is deliberately a small semantic model, not a second hot store.  It
//! shares frozen record types and serializers with production because those
//! bytes are the compatibility contract under test, but it never calls a
//! PathMap-backed store or cursor.

use std::collections::HashMap;

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hot_store::HotStoreState;
use rspace_plus_plus::rspace::hot_store_action::{
    HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::CandidateOrderingBytes;

pub type TestState = HotStoreState<String, String, String, String>;
pub type TestAction = HotStoreAction<String, String, String, String>;
pub type TestContinuation = WaitingContinuation<String, String>;

#[derive(Clone, Debug, Default)]
pub struct LegacyContinuationJoinOracle {
    pub continuations: HashMap<Vec<String>, Vec<TestContinuation>>,
    pub data: HashMap<String, Vec<Datum<String>>>,
    pub joins: HashMap<String, Vec<Vec<String>>>,
}

impl LegacyContinuationJoinOracle {
    /// Load a legacy snapshot while preserving the first representative of a
    /// source-free continuation identity and every join occurrence.
    pub fn from_snapshot(snapshot: TestState) -> Self {
        let mut continuations = HashMap::with_capacity(snapshot.continuations.len());
        for (channels, values) in snapshot.continuations {
            let mut first_wins = Vec::with_capacity(values.len());
            for value in values {
                if !first_wins
                    .iter()
                    .any(|existing| same_continuation_identity(existing, &value))
                {
                    first_wins.push(value);
                }
            }
            continuations.insert(channels, first_wins);
        }
        Self {
            continuations,
            data: snapshot.data,
            joins: snapshot.joins,
        }
    }

    pub fn snapshot(&self) -> TestState {
        TestState {
            continuations: self.continuations.clone(),
            installed_continuations: HashMap::new(),
            data: self.data.clone(),
            joins: self.joins.clone(),
            installed_joins: HashMap::new(),
        }
    }

    /// Cold-store decoding returns collections in frozen record-byte order.
    pub fn canonical_history_snapshot(&self) -> TestState {
        let mut snapshot = self.snapshot();
        for values in snapshot.continuations.values_mut() {
            values.sort_unstable_by_key(CandidateOrderingBytes::candidate_ordering_bytes);
        }
        for values in snapshot.data.values_mut() {
            values.sort_unstable_by_key(CandidateOrderingBytes::candidate_ordering_bytes);
        }
        for values in snapshot.joins.values_mut() {
            values
                .sort_unstable_by_key(|join| bincode::serialize(join).expect("legacy join bytes"));
        }
        snapshot
    }

    /// Independently reproduce the frozen continuation selection key:
    /// `(BLAKE2b(candidate bytes), candidate bytes, original vector index)`.
    pub fn ordered_continuations(&self, channels: &[String]) -> Vec<(TestContinuation, i32)> {
        let mut decorated = self
            .continuations
            .get(channels)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, continuation)| {
                let bytes = continuation.candidate_ordering_bytes();
                (
                    Blake2b256Hash::new(&bytes),
                    bytes,
                    i32::try_from(index).expect("test continuation index"),
                    continuation,
                )
            })
            .collect::<Vec<_>>();
        decorated.sort_unstable_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        decorated
            .into_iter()
            .map(|(_, _, index, continuation)| (continuation, index))
            .collect()
    }

    /// Remove the named continuation and exactly one occurrence of its receive
    /// group from every participating channel's legacy join vector.
    pub fn apply_selected(&mut self, channels: &[String], store_index: i32) -> TestContinuation {
        let values = self
            .continuations
            .get_mut(channels)
            .expect("oracle continuation group");
        let selected = values.remove(usize::try_from(store_index).expect("nonnegative index"));
        if values.is_empty() {
            for channel in channels {
                if let Some(channel_joins) = self.joins.get_mut(channel) {
                    if let Some(position) = channel_joins.iter().position(|join| join == channels) {
                        channel_joins.remove(position);
                    }
                }
            }
        }
        selected
    }

    /// Typed legacy actions are the independent root oracle.  Empty maps need
    /// no action when reconstructing the content-derived final root from the
    /// empty root.
    pub fn typed_checkpoint_actions(&self) -> Vec<TestAction> {
        let mut actions = Vec::new();

        let mut data_keys = self.data.keys().cloned().collect::<Vec<_>>();
        data_keys.sort();
        for channel in data_keys {
            let values = self.data.get(&channel).expect("collected data key");
            if !values.is_empty() {
                actions.push(HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                    channel,
                    data: values.clone(),
                })));
            }
        }

        let mut continuation_keys = self.continuations.keys().cloned().collect::<Vec<_>>();
        continuation_keys.sort();
        for channels in continuation_keys {
            let values = self
                .continuations
                .get(&channels)
                .expect("collected continuation key");
            if !values.is_empty() {
                actions.push(HotStoreAction::Insert(InsertAction::InsertContinuations(
                    InsertContinuations {
                        channels,
                        continuations: values.clone(),
                    },
                )));
            }
        }

        let mut join_keys = self.joins.keys().cloned().collect::<Vec<_>>();
        join_keys.sort();
        for channel in join_keys {
            let values = self.joins.get(&channel).expect("collected join key");
            if !values.is_empty() {
                actions.push(HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                    channel,
                    joins: values.clone(),
                })));
            }
        }
        actions
    }
}

pub fn same_continuation_identity(left: &TestContinuation, right: &TestContinuation) -> bool {
    left.patterns == right.patterns &&
        left.continuation == right.continuation &&
        left.persist == right.persist &&
        left.peeks == right.peeks
}
