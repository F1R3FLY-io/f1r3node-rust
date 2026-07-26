// See rspace/src/main/scala/coop/rchain/rspace/internal.scala

use std::collections::BTreeSet;
use std::hash::Hash;
use std::sync::Arc;

use counter::Counter;
use dashmap::DashMap;
use proptest_derive::Arbitrary;
use serde::Serialize;

use super::hashing::stable_hash_provider::StableHashSerialize;

use super::trace::event::{Consume, Produce};

// EPathMap fix P4.1 — reference-shaped RSpace transport (plan v1 §1-P4,
// sub-commit 1 of 3): the PAYLOAD fields of the hot-store records are
// `Arc`-shaped, so every store→matcher→result hop is a refcount bump instead
// of a deep copy (the per-consume-attempt `get_data` clone, the per-produce
// `get_continuations` clone, the candidate/`check_commit`/`wrap_result`
// copies — plan §0.C). The struct NAMES and every generic signature that
// mentions them are unchanged; only construction sites and owned-payload
// consumers adapt.
//
// `Serialize`/`Deserialize` are deliberately DROPPED (the workspace requests
// serde's "derive" feature only — no "rc" — so an `Arc` field reaching
// `bincode::serialize` is a COMPILE ERROR): every serializer site is thereby
// fail-closed enumerated and must MATERIALIZE explicitly through the
// borrowed twins in `serializers.rs` (cold-store leaves, byte-golden-pinned)
// or `CandidateOrderingBytes` below (the deterministic candidate-ordering
// hash). The remaining derives keep their exact semantics: `Arc<T>`
// delegates `PartialEq`/`Eq`/`Hash`/`Debug`/`Default` to `T`, so equality,
// hot-store identity strings, and proptest generation are unchanged.

// The 'Arbitrary' macro is needed here for proptest in hot_store_spec.rs
// The 'Default' macro is needed here for hot_store_spec.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash, Arbitrary, Default)]
pub struct Datum<A: Clone> {
    /// The stored payload, shared by `Arc` (P4.1): cloning a `Datum` — per
    /// consume attempt, per candidate, per history-cache fill — bumps a
    /// refcount instead of deep-copying the (potentially EPathMap-heavy)
    /// payload. Read sites deref-coerce (`&datum.a` → `&A`); the few
    /// owned-payload boundaries materialize with `(*datum.a).clone()`.
    pub a: Arc<A>,
    pub persist: bool,
    pub source: Produce,
}

impl<A> Datum<A>
where A: Clone + StableHashSerialize
{
    pub fn create<C: Serialize>(channel: &C, a: A, persist: bool) -> Datum<A> {
        let source = Produce::create(channel, &a, persist);
        Datum {
            a: Arc::new(a),
            persist,
            source,
        }
    }
}

// The 'Arbitrary' macro is needed here for proptest in hot_store_spec.rs
// The 'Default' macro is needed here for hot_store_spec.rs
#[derive(Clone, Debug, Arbitrary, Default, PartialEq, Eq, Hash)]
pub struct WaitingContinuation<P: Clone, K: Clone> {
    /// Patterns shared by `Arc` (P4.1): `get_continuations` clones the whole
    /// waiting-continuation vector per produce attempt — the pattern list now
    /// travels by refcount. The matcher borrows individual patterns
    /// (sub-commit 2); `ContResult` materializes once per fired COMM.
    pub patterns: Arc<Vec<P>>,
    /// The continuation body shared by `Arc` (P4.1) — the largest payload of
    /// the record (`TaggedContinuation` carrying the receive body).
    pub continuation: Arc<K>,
    pub persist: bool,
    pub peeks: BTreeSet<i32>,
    pub source: Consume,
}

impl<P, K> WaitingContinuation<P, K>
where
    P: Clone + StableHashSerialize,
    K: Clone + StableHashSerialize,
{
    pub fn create<C: Clone + Serialize>(
        channels: &Vec<C>,
        patterns: &Vec<P>,
        continuation: &K,
        persist: bool,
        peeks: BTreeSet<i32>,
    ) -> WaitingContinuation<P, K> {
        let source = Consume::create(channels, patterns, &continuation, persist);
        WaitingContinuation {
            patterns: Arc::new(patterns.to_vec()),
            continuation: Arc::new(continuation.clone()),
            persist,
            peeks,
            source,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConsumeCandidate<C, A: Clone> {
    pub channel: C,
    pub datum: Datum<A>,
    /// The datum as it sat in the store (pre-match), shared by `Arc` (P4.1):
    /// referenced by peek re-produces and `wrap_result`; previously a deep
    /// copy per candidate.
    pub removed_datum: Arc<A>,
    pub datum_index: i32,
}

/// `Clone` mirrors [`ConsumeCandidate`]'s, and is what lets a caller HOLD an
/// enumerated rendezvous (see
/// [`crate::rspace::space_matcher::SpaceMatcher::enumerate_enabled_rendezvous`])
/// while deciding whether to fire it: every field was already `Clone`, and
/// `process_match_found` consumes its argument by value, so without this a
/// caller could only fire the rendezvous it happened to own outright.
#[derive(Clone, Debug)]
pub struct ProduceCandidate<C, P: Clone, A: Clone, K: Clone> {
    pub channels: Vec<C>,
    pub continuation: WaitingContinuation<P, K>,
    pub continuation_index: i32,
    pub data_candidates: Vec<ConsumeCandidate<C, A>>,
}

// Eq and PartialEq is needed here for reduce_spec tests
#[derive(Debug, Eq, PartialEq)]
pub struct Row<P: Clone, A: Clone, K: Clone> {
    pub data: Vec<Datum<A>>,
    pub wks: Vec<WaitingContinuation<P, K>>,
}

#[derive(Clone, Debug)]
pub struct Install<P, K> {
    pub patterns: Vec<P>,
    pub continuation: K,
}

#[derive(Clone, Debug)]
pub struct MultisetMultiMap<K: Hash + Eq, V: Hash + Eq> {
    pub map: DashMap<K, Counter<V>>,
}

impl<K, V> MultisetMultiMap<K, V>
where
    K: Eq + Hash,
    V: Eq + Hash,
{
    pub fn empty() -> Self {
        MultisetMultiMap {
            map: DashMap::new(),
        }
    }

    pub fn add_binding(&self, k: K, v: V) {
        match self.map.get_mut(&k) {
            Some(mut current) => match current.get_mut(&v) {
                Some(count) => *count += 1,
                None => {
                    current.insert(v, 1);
                }
            },
            None => {
                let mut ms = Counter::new();
                ms.insert(v, 1);
                self.map.insert(k, ms);
            }
        }
    }

    pub fn clear(&self) { self.map.clear(); }

    pub fn is_empty(&self) -> bool { self.map.is_empty() }
}

impl<K: Hash + Eq, V: Hash + Eq> MultisetMultiMap<K, V> {
    // In-place removal to avoid moving the whole map
    pub fn remove_binding_in_place(&self, k: &K, v: &V) {
        let mut should_remove_key = false;

        if let Some(mut current) = self.map.get_mut(k) {
            let mut should_remove_value = false;
            if let Some(count) = current.get_mut(v) {
                if *count > 1 {
                    *count -= 1;
                } else {
                    should_remove_value = true;
                }
            }

            if should_remove_value {
                current.remove(v);
            }

            if current.is_empty() {
                should_remove_key = true;
            }
        }

        if should_remove_key {
            self.map.remove(k);
        }
    }
}

// This function remains for compatibility but delegates to in-place version and
// returns the same map
pub fn remove_binding<K: Hash + Eq, V: Hash + Eq>(
    ms: MultisetMultiMap<K, V>,
    k: K,
    v: V,
) -> MultisetMultiMap<K, V> {
    ms.remove_binding_in_place(&k, &v);
    ms
}

#[cfg(test)]
mod tests {
    use super::MultisetMultiMap;

    #[test]
    fn multiset_multimap_add_binding_increments_existing_count() {
        let ms = MultisetMultiMap::empty();
        ms.add_binding("k", "v");
        ms.add_binding("k", "v");

        let count = ms
            .map
            .get(&"k")
            .and_then(|counter| counter.get(&"v").copied())
            .unwrap_or(0);
        assert_eq!(count, 2);
    }

    #[test]
    fn multiset_multimap_remove_binding_decrements_before_removing() {
        let ms = MultisetMultiMap::empty();
        ms.add_binding("k", "v");
        ms.add_binding("k", "v");

        ms.remove_binding_in_place(&"k", &"v");
        let count_after_one_remove = ms
            .map
            .get(&"k")
            .and_then(|counter| counter.get(&"v").copied())
            .unwrap_or(0);
        assert_eq!(count_after_one_remove, 1);

        ms.remove_binding_in_place(&"k", &"v");
        assert!(ms.map.get(&"k").is_none());
    }
}
