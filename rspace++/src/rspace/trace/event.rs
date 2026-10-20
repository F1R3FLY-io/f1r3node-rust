use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::{
    EventCodecEpoch, StableHashSerialize, channel_hash_for_epoch, channel_hashes_for_epoch, hash,
    hash_consume, hash_consume_for_epoch, hash_produce, hash_produce_for_epoch, hash_vec,
};
use crate::rspace::internal::ConsumeCandidate;

// See rspace/src/main/scala/coop/rchain/rspace/trace/Event.scala
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Event {
    Comm(COMM),
    IoEvent(IOEvent),
    RestingComm(Arc<RestingCOMM>),
}

/// A commit of data and a continuation that were both already resting in the
/// store. Unlike `COMM`, this event has no arriving Produce or Consume IO leg.
/// Bind order and store indices are part of its identity: sorting the sources
/// would alias distinct repeated-channel selections.
#[derive(Debug, Clone)]
pub struct RestingCOMM {
    pub consume: Consume,
    pub continuation_index: i32,
    pub ordered_produces: Vec<Produce>,
    pub ordered_data_indices: Vec<i32>,
    pub peeks: BTreeSet<i32>,
}

impl PartialEq for RestingCOMM {
    fn eq(&self, other: &Self) -> bool {
        self.consume == other.consume &&
            self.continuation_index == other.continuation_index &&
            self.ordered_data_indices == other.ordered_data_indices &&
            self.peeks == other.peeks &&
            self.ordered_produces.len() == other.ordered_produces.len() &&
            self.ordered_produces
                .iter()
                .zip(&other.ordered_produces)
                .all(|(left, right)| {
                    left.channel_hash == right.channel_hash &&
                        left.hash == right.hash &&
                        left.persistent == right.persistent
                })
    }
}

impl Eq for RestingCOMM {}

impl Hash for RestingCOMM {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.consume.hash(state);
        self.continuation_index.hash(state);
        self.ordered_data_indices.hash(state);
        self.peeks.hash(state);
        self.ordered_produces.len().hash(state);
        for produce in &self.ordered_produces {
            produce.channel_hash.hash(state);
            produce.hash.hash(state);
            produce.persistent.hash(state);
        }
    }
}

impl RestingCOMM {
    /// Protocol-7-only identity. Existing `COMM` and IO event hashes remain
    /// byte-for-byte unchanged in both CostV1 and CostV2.
    pub fn cost_v2_identity(&self) -> Result<Blake2b256Hash, RSpaceError> {
        const DOMAIN: &[u8] = b"f1r3node:rspace-resting-comm:v1";
        if self.ordered_produces.len() != self.ordered_data_indices.len() {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let ordered: Vec<_> = self
            .ordered_produces
            .iter()
            .zip(self.ordered_data_indices.iter().copied())
            .map(|(produce, index)| {
                (&produce.channel_hash, &produce.hash, produce.persistent, index)
            })
            .collect();
        let encoded = bincode::serialize(&(
            DOMAIN,
            &self.consume.channel_hashes,
            &self.consume.hash,
            self.consume.persistent,
            self.continuation_index,
            &ordered,
            &self.peeks,
        ))
        .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
        Ok(Blake2b256Hash::new(&encoded))
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum IOEvent {
    Produce(Produce),
    Consume(Consume),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct COMM {
    pub consume: Consume,
    pub produces: Vec<Produce>,
    pub peeks: BTreeSet<i32>,
    pub times_repeated: BTreeMap<Produce, i32>,
}

impl COMM {
    pub fn new<C, A: Clone>(
        data_candidates: &[ConsumeCandidate<C, A>],
        consume_ref: Consume,
        peeks: BTreeSet<i32>,
        produce_counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
    ) -> Self {
        let mut produce_refs: Vec<Produce> = data_candidates
            .iter()
            .map(|candidate| candidate.datum.source.clone())
            .collect();

        // produce_refs.sort_by(|a, b| {
        //     let a_cloned = a.clone();
        //     let b_cloned = b.clone();
        //     (a_cloned.channel_hash, a_cloned.hash, a.persistent).cmp(&(
        //         b_cloned.channel_hash,
        //         b_cloned.hash,
        //         b.persistent,
        //     ))
        // });
        // Note: this sort uses (channel_hash, hash, persistent) for COMM event
        // identity, which differs from Produce::Ord (hash-only). Do not replace
        // with .sort().
        produce_refs.sort_by(|a, b| {
            a.channel_hash
                .cmp(&b.channel_hash)
                .then_with(|| a.hash.cmp(&b.hash))
                .then_with(|| a.persistent.cmp(&b.persistent))
        });
        // produce_refs.sort_by_key(|p| {
        //     let p_cloned = p.clone();
        //     (p_cloned.channel_hash, p_cloned.hash, p.persistent)
        // });

        let times_repeated = produce_counters(&produce_refs);
        COMM {
            consume: consume_ref,
            produces: produce_refs,
            peeks,
            times_repeated,
        }
    }

    pub fn cost_identity(&self) -> Blake2b256Hash {
        let mut produces: Vec<_> = self
            .produces
            .iter()
            .map(|produce| {
                (
                    produce.channel_hash.clone(),
                    produce.hash.clone(),
                    produce.persistent,
                    *self.times_repeated.get(produce).unwrap_or(&0),
                )
            })
            .collect();
        produces.sort();
        let encoded = bincode::serialize(&(
            &self.consume.channel_hashes,
            &self.consume.hash,
            self.consume.persistent,
            produces,
            &self.peeks,
        ))
        .expect("COMM cost identity serialization");
        Blake2b256Hash::new(&encoded)
    }
}

// Needed for 'counter' crate
impl Hash for COMM {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.consume.hash(state);
        self.produces.hash(state);
        self.peeks.hash(state);

        for (key, value) in &self.times_repeated {
            key.hash(state);
            value.hash(state);
        }
    }
}

/// Canonical replay-candidate order.
///
/// A single IO event may be bound to more than one distinct COMM in
/// `MultisetMultiMap<IOEvent, COMM>`. The backing `Counter` is a `HashMap`, so
/// its iteration order cannot decide which viable COMM replay attempts first.
/// This order is deliberately written out instead of derived: adding or moving
/// a struct field must not silently rewrite replay selection. Every component
/// participates in `COMM` identity and already has a deterministic total order;
/// `Produce` compares by its identity hash, including inside `times_repeated`.
impl Ord for COMM {
    fn cmp(&self, other: &Self) -> Ordering {
        self.consume
            .cmp(&other.consume)
            .then_with(|| self.produces.cmp(&other.produces))
            .then_with(|| self.peeks.cmp(&other.peeks))
            .then_with(|| self.times_repeated.cmp(&other.times_repeated))
    }
}

impl PartialOrd for COMM {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

// The 'Arbitrary' macro is needed here for proptest in hot_store_spec.rs
// The 'Default' macro is needed here for hot_store_spec.rs
//
// Custom PartialEq/Eq/Hash/Ord: identity is determined solely by the `hash`
// field (a cryptographic hash of channel + data + persist). Metadata fields
// like `is_deterministic`, `output_value`, and `failed` are set after creation
// (e.g. via mark_as_non_deterministic) and must NOT affect identity.
#[derive(Serialize, Deserialize, Clone, Debug, Arbitrary, Default)]
pub struct Produce {
    pub channel_hash: Blake2b256Hash,
    pub hash: Blake2b256Hash,
    pub persistent: bool,
    pub is_deterministic: bool,
    pub output_value: Vec<Vec<u8>>,
    /// Indicates whether this produce event represents a failed
    /// non-deterministic process. Used for replay safety of external
    /// service calls (OpenAI, Ollama, gRPC).
    pub failed: bool,
}

impl PartialEq for Produce {
    fn eq(&self, other: &Self) -> bool { self.hash == other.hash }
}

impl Eq for Produce {}

impl std::hash::Hash for Produce {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.hash.hash(state); }
}

impl Ord for Produce {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.hash.cmp(&other.hash) }
}

impl PartialOrd for Produce {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Produce {
    /// Explicit-epoch event constructor. Keep the historical `create` path
    /// unchanged until live RSpace, native replay, and history share the same
    /// authenticated session context.
    pub fn create_for_epoch<C: StableHashSerialize, A: StableHashSerialize>(
        epoch: EventCodecEpoch,
        channel: &C,
        datum: &A,
        persistent: bool,
    ) -> Result<Produce, RSpaceError> {
        let channel_hash = channel_hash_for_epoch(epoch, channel)?;
        let hash = hash_produce_for_epoch(epoch, &channel_hash, datum, persistent)?;
        Ok(Produce::new(channel_hash, hash, persistent))
    }

    pub fn create<C: StableHashSerialize, A: StableHashSerialize>(
        channel: &C,
        datum: &A,
        persistent: bool,
    ) -> Produce {
        let channel_hash = hash(channel);
        let hash = hash_produce(channel_hash.bytes(), datum, persistent);
        Produce {
            channel_hash,
            hash,
            persistent,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        }
    }

    pub fn new(channel_hash: Blake2b256Hash, hash: Blake2b256Hash, persistent: bool) -> Produce {
        Produce {
            channel_hash,
            hash,
            persistent,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        }
    }

    pub fn mark_as_non_deterministic(self, previous: Vec<Vec<u8>>) -> Self {
        Produce {
            is_deterministic: false,
            output_value: previous,
            ..self
        }
    }

    /// Mark this produce event as failed, indicating a non-deterministic
    /// process failure. Used to record failures from external service calls
    /// (OpenAI, Ollama, gRPC) so replay can correctly handle them without
    /// re-executing the external call.
    pub fn with_error(&self) -> Self {
        Produce {
            failed: true,
            ..self.clone()
        }
    }
}

// The 'Arbitrary' macro is needed here for proptest in hot_store_spec.rs
// The 'Default' macro is needed here for hot_store_spec.rs
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Arbitrary,
    Hash,
    Default,
    Ord,
    PartialOrd
)]
pub struct Consume {
    pub channel_hashes: Vec<Blake2b256Hash>,
    pub hash: Blake2b256Hash,
    pub persistent: bool,
}

impl Consume {
    /// Explicit-epoch counterpart of `create`. Pattern order is canonicalized
    /// by the hash provider after channel hashes are sorted.
    pub fn create_for_epoch<
        C: StableHashSerialize,
        P: StableHashSerialize,
        K: StableHashSerialize,
    >(
        epoch: EventCodecEpoch,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persistent: bool,
    ) -> Result<Consume, RSpaceError> {
        let channel_hashes = channel_hashes_for_epoch(epoch, channels)?;
        let hash =
            hash_consume_for_epoch(epoch, &channel_hashes, patterns, continuation, persistent)?;
        Ok(Consume {
            channel_hashes,
            hash,
            persistent,
        })
    }

    pub fn create<C: StableHashSerialize, P: StableHashSerialize, K: StableHashSerialize>(
        channels: &Vec<C>,
        patterns: &Vec<P>,
        continuation: &K,
        persistent: bool,
    ) -> Consume {
        let channel_hashes = hash_vec(channels);
        let channels_encoded_sorted: Vec<Vec<u8>> =
            channel_hashes.iter().map(|hash| hash.bytes()).collect();
        let hash = hash_consume(channels_encoded_sorted, patterns, &continuation, persistent);
        Consume {
            channel_hashes,
            hash,
            persistent,
        }
    }
}

pub fn recorded_removal<C: StableHashSerialize>(
    channel: &C,
    datum_source: &Produce,
    operation_id: &[u8],
) -> (Consume, COMM) {
    recorded_removal_with_hash(hash(channel), datum_source, operation_id)
}

pub fn recorded_removal_for_epoch<C: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channel: &C,
    datum_source: &Produce,
    operation_id: &[u8],
) -> Result<(Consume, COMM), RSpaceError> {
    Ok(recorded_removal_with_hash(
        channel_hash_for_epoch(epoch, channel)?,
        datum_source,
        operation_id,
    ))
}

fn recorded_removal_with_hash(
    channel_hash: Blake2b256Hash,
    datum_source: &Produce,
    operation_id: &[u8],
) -> (Consume, COMM) {
    const PRODUCE_DOMAIN: &[u8] = b"f1r3node:rspace:recorded-removal:produce:v1";
    const CONSUME_DOMAIN: &[u8] = b"f1r3node:rspace:recorded-removal:consume:v1";

    let mut produce_identity = Vec::with_capacity(
        PRODUCE_DOMAIN.len() + datum_source.hash.bytes().len() + operation_id.len(),
    );
    produce_identity.extend_from_slice(PRODUCE_DOMAIN);
    produce_identity.extend_from_slice(&datum_source.hash.bytes());
    produce_identity.extend_from_slice(operation_id);
    let logical_produce =
        Produce::new(channel_hash.clone(), Blake2b256Hash::new(&produce_identity), false);

    let mut consume_identity =
        Vec::with_capacity(CONSUME_DOMAIN.len() + logical_produce.hash.bytes().len());
    consume_identity.extend_from_slice(CONSUME_DOMAIN);
    consume_identity.extend_from_slice(&logical_produce.hash.bytes());
    let consume = Consume {
        channel_hashes: vec![channel_hash],
        hash: Blake2b256Hash::new(&consume_identity),
        persistent: false,
    };
    let comm = COMM {
        consume: consume.clone(),
        produces: vec![logical_produce.clone()],
        peeks: BTreeSet::new(),
        times_repeated: BTreeMap::from([(logical_produce, 1)]),
    };
    (consume, comm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comm() -> COMM {
        let produce =
            Produce::new(Blake2b256Hash::new(b"channel"), Blake2b256Hash::new(b"produce"), false);
        COMM {
            consume: Consume {
                channel_hashes: vec![Blake2b256Hash::new(b"channel")],
                hash: Blake2b256Hash::new(b"consume"),
                persistent: false,
            },
            produces: vec![produce.clone()],
            peeks: BTreeSet::new(),
            times_repeated: BTreeMap::from([(produce, 1)]),
        }
    }

    #[test]
    fn cost_identity_ignores_produce_telemetry() {
        let original = comm();
        let mut changed = original.clone();
        changed.produces[0] = changed.produces[0]
            .clone()
            .mark_as_non_deterministic(vec![b"external output".to_vec()])
            .with_error();
        assert_eq!(original.cost_identity(), changed.cost_identity());
    }

    #[test]
    fn cost_identity_commits_repetition_count() {
        let original = comm();
        let mut changed = original.clone();
        changed
            .times_repeated
            .insert(changed.produces[0].clone(), 2);
        assert_ne!(original.cost_identity(), changed.cost_identity());
    }

    proptest::proptest! {
        #[test]
        fn cost_identity_distinguishes_all_completed_persistent_firing_counts(
            first in 0i32..1_000_000,
            second in 0i32..1_000_000,
        ) {
            proptest::prop_assume!(first != second);
            let mut left = comm();
            let mut right = comm();
            left.times_repeated.insert(left.produces[0].clone(), first);
            right.times_repeated.insert(right.produces[0].clone(), second);
            proptest::prop_assert_ne!(left.cost_identity(), right.cost_identity());
        }
    }

    #[test]
    fn cost_identity_canonicalizes_producer_order() {
        let mut original = comm();
        let second = Produce::new(
            Blake2b256Hash::new(b"channel-2"),
            Blake2b256Hash::new(b"produce-2"),
            false,
        );
        original.produces.push(second.clone());
        original.times_repeated.insert(second, 1);
        let mut reversed = original.clone();
        reversed.produces.reverse();
        assert_eq!(original.cost_identity(), reversed.cost_identity());
    }

    /// Changed by DR-131 S6: the metered COMM identity is deleted. The test
    /// keeps its check that the identity of reversed producers, one of them
    /// persistent, with a peek, equals the identity of the original order.
    #[test]
    fn cost_identity_canonicalizes_persistent_producers_and_peeks() {
        let mut original = comm();
        let second = Produce::new(
            Blake2b256Hash::new(b"channel-2"),
            Blake2b256Hash::new(b"produce-2"),
            true,
        );
        original.produces.push(second.clone());
        original.times_repeated.insert(second, 3);
        original.peeks.insert(0);
        let mut reversed = original.clone();
        reversed.produces.reverse();
        assert_eq!(reversed.cost_identity(), original.cost_identity());
    }

    #[test]
    fn resting_comm_identity_is_domain_separated_and_binds_repeated_channel_order() {
        let ordinary = comm();
        let mut resting = RestingCOMM {
            consume: ordinary.consume.clone(),
            continuation_index: 0,
            ordered_produces: vec![ordinary.produces[0].clone(), ordinary.produces[0].clone()],
            ordered_data_indices: vec![0, 1],
            peeks: BTreeSet::new(),
        };
        let identity = resting.cost_v2_identity().unwrap();
        assert_ne!(identity, ordinary.cost_identity());

        resting.ordered_data_indices.reverse();
        assert_ne!(identity, resting.cost_v2_identity().unwrap());
        resting.ordered_data_indices.reverse();
        resting.continuation_index = 1;
        assert_ne!(identity, resting.cost_v2_identity().unwrap());

        resting.continuation_index = 0;
        resting.ordered_produces[0] = resting.ordered_produces[0]
            .clone()
            .mark_as_non_deterministic(vec![b"telemetry".to_vec()]);
        assert_eq!(identity, resting.cost_v2_identity().unwrap());
    }

    #[test]
    fn recorded_removal_identity_distinguishes_linear_occurrences() {
        let source = Produce::create(&"purse", &"stack", false);
        let first = recorded_removal(&"purse", &source, b"first");
        let first_again = recorded_removal(&"purse", &source, b"first");
        let second = recorded_removal(&"purse", &source, b"second");

        assert_eq!(first, first_again);
        assert_ne!(first, second);
        assert_eq!(first.0.channel_hashes, vec![hash(&"purse")]);
        assert!(!first.1.produces[0].persistent);
    }
}
