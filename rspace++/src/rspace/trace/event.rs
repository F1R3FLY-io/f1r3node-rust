use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::mem::size_of;

use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{self, SourceMeter};
use crate::rspace::hashing::stable_hash_provider::{hash, hash_consume, hash_produce, hash_vec};
use crate::rspace::internal::ConsumeCandidate;
use crate::rspace::native_backing;

// See rspace/src/main/scala/coop/rchain/rspace/trace/Event.scala
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Event {
    Comm(COMM),
    IoEvent(IOEvent),
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

    pub fn cost_identity_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Blake2b256Hash, RSpaceError> {
        type ProduceIdentity<'a> = (&'a Blake2b256Hash, &'a Blake2b256Hash, bool, i32);

        native_backing::inspect(&self.consume, meter)?;
        native_backing::inspect(&self.peeks, meter)?;
        native_backing::inspect(&self.times_repeated, meter)?;
        let map_hash_bytes = self
            .times_repeated
            .keys()
            .try_fold(0_usize, |total, produce| {
                total
                    .checked_add(produce.hash.0.len())
                    .ok_or(RSpaceError::HostWorkRejected)
            })?;
        let count = self.produces.len();
        let bytes = count
            .checked_mul(size_of::<ProduceIdentity<'_>>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, bytes, bytes)?;
        let mut produces: Vec<ProduceIdentity<'_>> = Vec::new();
        produces
            .try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for produce in &self.produces {
            let lookup_bytes = self
                .times_repeated
                .len()
                .checked_mul(produce.hash.0.len())
                .and_then(|bytes| bytes.checked_add(map_hash_bytes))
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(
                self.times_repeated
                    .len()
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                lookup_bytes,
                0,
            )?;
            produces.push((
                &produce.channel_hash,
                &produce.hash,
                produce.persistent,
                *self.times_repeated.get(produce).unwrap_or(&0),
            ));
        }
        let move_bytes = bytes.checked_mul(2).ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(count, move_bytes, 0)?;
        shared::rust::fallible_sort::sort(&mut produces, |a, b| {
            let compared_bytes =
                a.0.0
                    .len()
                    .checked_add(a.1.0.len())
                    .and_then(|bytes| bytes.checked_add(b.0.0.len()))
                    .and_then(|bytes| bytes.checked_add(b.1.0.len()))
                    .and_then(|bytes| {
                        size_of::<ProduceIdentity<'_>>()
                            .checked_mul(2)
                            .and_then(|moves| bytes.checked_add(moves))
                    })
                    .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(1, compared_bytes, 0)?;
            Ok::<_, RSpaceError>(a.cmp(b))
        })?;
        let forward = |operations, scanned, backing| meter.reserve(operations, scanned, backing);
        native_source::hash(
            &(
                &self.consume.channel_hashes,
                &self.consume.hash,
                self.consume.persistent,
                produces,
                &self.peeks,
            ),
            &forward,
        )
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
    pub fn create<C: Serialize, A: Serialize>(channel: &C, datum: &A, persistent: bool) -> Produce {
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

    pub fn create_metered<C: Serialize, A: Serialize>(
        channel: &C,
        datum: &A,
        persistent: bool,
        meter: &dyn SourceMeter,
    ) -> Result<Produce, RSpaceError> {
        let reserve = |operations: usize, scanned, backing| {
            meter.reserve(
                operations
                    .checked_mul(2)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                scanned,
                backing,
            )
        };
        native_source::produce(channel, datum, persistent, &reserve)
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
    pub fn create<C: Serialize, P: Serialize, K: Serialize>(
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

pub fn recorded_removal<C: Serialize>(
    channel: &C,
    datum_source: &Produce,
    operation_id: &[u8],
) -> (Consume, COMM) {
    const PRODUCE_DOMAIN: &[u8] = b"f1r3node:rspace:recorded-removal:produce:v1";
    const CONSUME_DOMAIN: &[u8] = b"f1r3node:rspace:recorded-removal:consume:v1";

    let channel_hash = hash(channel);
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
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, Ordering};

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
    fn metered_produce_preserves_source_bytes_and_rejects_every_reservation_cut() {
        let channel = vec![vec![1_u8, 2], vec![3_u8; 32]];
        let datum = vec![9_u8; 77];
        for persistent in [false, true] {
            let calls = Cell::new(0_usize);
            let totals = Cell::new([0_usize; 3]);
            let unlimited = |operations: usize, scanned: usize, backing: usize| {
                calls.set(calls.get() + 1);
                let [old_operations, old_scanned, old_backing] = totals.get();
                totals.set([
                    old_operations + operations,
                    old_scanned + scanned,
                    old_backing + backing,
                ]);
                Ok(())
            };
            let actual = Produce::create_metered(&channel, &datum, persistent, &unlimited).unwrap();
            let expected = Produce::create(&channel, &datum, persistent);
            assert_eq!(
                bincode::serialize(&actual).unwrap(),
                bincode::serialize(&expected).unwrap()
            );
            assert!(calls.get() > 3);
            assert!(totals.get().iter().all(|total| *total > 0));

            for cut in 0..calls.get() {
                let seen = Cell::new(0_usize);
                let reject = |_: usize, _: usize, _: usize| {
                    let current = seen.get();
                    seen.set(current + 1);
                    if current == cut {
                        Err(RSpaceError::HostWorkRejected)
                    } else {
                        Ok(())
                    }
                };
                assert!(matches!(
                    Produce::create_metered(&channel, &datum, persistent, &reject),
                    Err(RSpaceError::HostWorkRejected)
                ));
                assert_eq!(channel, vec![vec![1_u8, 2], vec![3_u8; 32]]);
                assert_eq!(datum, vec![9_u8; 77]);
            }
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

    #[test]
    fn metered_cost_identity_preserves_canonical_bytes() {
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
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        assert_eq!(original.cost_identity_metered(&unlimited).unwrap(), original.cost_identity());
        assert_eq!(reversed.cost_identity_metered(&unlimited).unwrap(), original.cost_identity());
    }

    #[test]
    fn metered_cost_identity_rejects_before_tuple_allocation() {
        let value = comm();
        let rejected = AtomicBool::new(false);
        let reject = |_: usize, _: usize, backing: usize| {
            if backing == size_of::<(&Blake2b256Hash, &Blake2b256Hash, bool, i32)>() {
                rejected.store(true, Ordering::Relaxed);
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        assert!(matches!(value.cost_identity_metered(&reject), Err(RSpaceError::HostWorkRejected)));
        assert!(rejected.load(Ordering::Relaxed));
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
