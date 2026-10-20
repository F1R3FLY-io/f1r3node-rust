//! Canonical order of RSpace match candidates (I1, DR-75).
//!
//! The order is lexicographic in (source hash, full digest, store index).
//! Phase one sorts by (source hash, index) and reads only the precomputed
//! `Produce`/`Consume` hashes. Phase two re-sorts each run of two or more equal
//! source hashes by (digest, index), so a digest is computed only for a member
//! of such a run.
//!
//! Changed by DR-129: restored dev's randomized candidate order (user decision
//! 2026-10-12). Play shuffles its candidates at random again
//! (`RSpace::shuffle_with_index`), and replay follows the recorded pick. This
//! order is now the internal order of replay only: directive replay
//! (`native_candidate.rs`) and native replay (`native_candidate/selection.rs`),
//! whose candidate choice must agree from replay to replay. Before DR-129,
//! play, directive replay and native replay shared this function, so they
//! chose the same candidate. A play join that names one channel twice orders
//! that channel by source hash (E1 of DR-129, `space_matcher.rs`), which
//! agrees with this order.
//!
//! Formal model:
//! `formal/rocq/cost_accounted_rho/theories/CandidateSourceOrder.v`
//! (`lazy_two_phase_is_canonical`, `phase_one_ignores_digests`,
//! `digests_only_for_ties`, `candidate_order_insertion_independent`,
//! `filter_commutes_with_canonical_sort`) and
//! `formal/tlaplus/cost_accounted_rho/NativeCandidateOrder.tla`.

use std::sync::Arc;

use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, WaitingContinuation};
use super::serializers::serializers::CandidateOrderingBytes;

pub trait CandidateSource {
    fn source_hash(&self) -> &Blake2b256Hash;
}

impl<A: Clone> CandidateSource for Datum<A> {
    fn source_hash(&self) -> &Blake2b256Hash { &self.source.hash }
}

impl<P: Clone, K: Clone> CandidateSource for WaitingContinuation<P, K> {
    fn source_hash(&self) -> &Blake2b256Hash { &self.source.hash }
}

impl<T: CandidateSource + Clone> CandidateSource for std::borrow::Cow<'_, T> {
    fn source_hash(&self) -> &Blake2b256Hash { self.as_ref().source_hash() }
}

impl<T: CandidateSource + ?Sized> CandidateSource for Arc<T> {
    fn source_hash(&self) -> &Blake2b256Hash { (**self).source_hash() }
}

struct Entry<D> {
    value: D,
    index: i32,
    digest: Option<Blake2b256Hash>,
}

/// Changed by DR-131 S6: the canonical order with the digest function as a
/// parameter. It replaces the `OrderWork` trait, whose only other
/// implementation reserved host work before each step. The tests pass a
/// digest function that counts its calls.
fn canonical_order<D: CandidateSource>(
    values: Vec<D>,
    digest: &impl Fn(&D) -> Blake2b256Hash,
) -> Vec<(D, i32)> {
    let count =
        i32::try_from(values.len()).expect("an RSpace candidate list fits the i32 store index");
    if values.len() <= 1 {
        let mut ordered = Vec::with_capacity(values.len());
        ordered.extend((0..count).zip(values).map(|(index, value)| (value, index)));
        return ordered;
    }
    let mut entries = Vec::with_capacity(values.len());
    entries.extend((0..count).zip(values).map(|(index, value)| Entry {
        value,
        index,
        digest: None,
    }));
    entries.sort_unstable_by(|left, right| {
        left.value
            .source_hash()
            .cmp(right.value.source_hash())
            .then_with(|| left.index.cmp(&right.index))
    });
    let mut start = 0;
    while start < entries.len() {
        let mut end = start + 1;
        while end < entries.len() &&
            entries[end - 1].value.source_hash() == entries[end].value.source_hash()
        {
            end += 1;
        }
        if end - start >= 2 {
            settle_tie_run(&mut entries[start..end], digest);
        }
        start = end;
    }
    let mut ordered = Vec::with_capacity(entries.len());
    ordered.extend(entries.into_iter().map(|entry| (entry.value, entry.index)));
    ordered
}

fn settle_tie_run<D>(run: &mut [Entry<D>], digest: &impl Fn(&D) -> Blake2b256Hash) {
    for entry in run.iter_mut() {
        entry.digest = Some(digest(&entry.value));
    }
    run.sort_unstable_by(|left, right| {
        left.digest
            .cmp(&right.digest)
            .then_with(|| left.index.cmp(&right.index))
    });
}

/// The digest that settles a tie of source hashes: Blake2b-256 of the
/// candidate's bincode encoding.
fn candidate_digest<D: CandidateOrderingBytes>(value: &D) -> Blake2b256Hash {
    Blake2b256Hash::new(&value.candidate_ordering_bytes())
}

pub(crate) fn canonical_candidates<D: CandidateSource + CandidateOrderingBytes>(
    values: Vec<D>,
) -> Vec<(D, i32)> {
    canonical_order(values, &candidate_digest)
}

/// Compatibility spelling for the stack-safe matcher and replay walkers.
/// There is only one consensus ordering implementation: the source-hash-first
/// canonical order above. No caller may fall back to the old digest-first sort.
pub fn order_candidates_with_index<D: CandidateSource + CandidateOrderingBytes>(
    values: Vec<D>,
) -> Vec<(D, i32)> {
    canonical_candidates(values)
}

#[cfg(test)]
pub(crate) fn datum_with_source<A: Clone>(a: A, source: u8, persist: bool) -> Datum<A> {
    use super::trace::event::Produce;
    Datum {
        a: Arc::new(a),
        persist,
        source: Produce::new(
            Blake2b256Hash(vec![0; 32]),
            Blake2b256Hash(vec![source; 32]),
            persist,
        ),
    }
}

/// Candidates over a small source domain, so equal source hashes (tie runs)
/// occur often. Ties differ in payload, persistence, or the metadata that
/// `Produce.hash` omits.
#[cfg(test)]
pub(crate) fn candidate_strategy() -> impl proptest::strategy::Strategy<Value = Vec<Datum<Vec<u8>>>>
{
    use proptest::prelude::*;
    prop::collection::vec(
        (0u8..3, any::<bool>(), prop::collection::vec(any::<u8>(), 0..6), any::<bool>()),
        0..24,
    )
    .prop_map(|rows| {
        rows.into_iter()
            .map(|(source, persist, payload, nondeterministic)| {
                let mut datum = datum_with_source(payload.clone(), source, persist);
                if nondeterministic {
                    datum.source = datum.source.mark_as_non_deterministic(vec![payload]);
                }
                datum
            })
            .collect()
    })
}

// Moved by DR-129 from `space_matcher.rs` (a `dev` file), with no change in
// behavior, so that the dev file keeps only dev's code and E1.
// Legacy (digest, index) candidate order. Production now uses the canonical
// (source hash, digest, index) order in `candidate_order::canonical_candidates`
// (I1, DR-75), which digests only tie runs. This body stays as the test oracle
// that `NativeCandidateDigestCache.v` and
// `hash_once_order_equals_per_comparison_hash_order` cover.
#[cfg(test)]
pub(crate) fn deterministic_candidates<D: serde::Serialize>(data: Vec<D>) -> Vec<(D, i32)> {
    if data.len() <= 1 {
        return data.into_iter().map(|datum| (datum, 0)).collect();
    }
    use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    let hash =
        |candidate: &D| Blake2b256Hash::new(&bincode::serialize(candidate).unwrap_or_default());
    let mut indexed: Vec<_> = data
        .into_iter()
        .enumerate()
        .map(|(index, datum)| {
            let digest = hash(&datum);
            (datum, index as i32, digest)
        })
        .collect();
    indexed.sort_by(|(_, left_index, left_hash), (_, right_index, right_hash)| {
        left_hash
            .cmp(right_hash)
            .then_with(|| left_index.cmp(right_index))
    });
    indexed
        .into_iter()
        .map(|(datum, index, _)| (datum, index))
        .collect()
}

// Disabled by the merge of feature/cost-accounted-rho (DR-131): only the
// removed metered candidate tests (`native_candidate/metered/tests.rs`) called
// this helper.
// #[cfg(test)]
// pub(crate) fn encoded_order<D: CandidateOrderingBytes>(
//     ordered: &[(D, i32)],
// ) -> Vec<(Vec<u8>, i32)> {
//     ordered
//         .iter()
//         .map(|(value, index)| (value.candidate_ordering_bytes(), *index))
//         .collect()
// }

#[cfg(test)]
mod deterministic_candidates_tests {
    use super::deterministic_candidates;
    use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    fn per_comparison_hash_order<D: serde::Serialize>(data: Vec<D>) -> Vec<(D, i32)> {
        let hash = |candidate: &D| {
            Blake2b256Hash::new(&bincode::serialize(candidate).expect("test candidate serializes"))
        };
        let mut indexed: Vec<(D, i32)> = data
            .into_iter()
            .enumerate()
            .map(|(index, datum)| (datum, index as i32))
            .collect();
        indexed.sort_by(|(left, left_index), (right, right_index)| {
            hash(left)
                .cmp(&hash(right))
                .then_with(|| left_index.cmp(right_index))
        });
        indexed
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

        /// Extracted from `NativeCandidateDigestCache.v`:
        /// `cached_comparison_is_original_comparison` and
        /// `caching_preserves_candidate_order`. Hashing each candidate once yields
        /// the same order as hashing inside every comparison, including duplicate
        /// candidates (index tie-break) and the `n <= 1` fast path.
        #[test]
        fn hash_once_order_equals_per_comparison_hash_order(
            data in proptest::collection::vec((0u8..4, "[a-c]{0,3}"), 0..12),
        ) {
            proptest::prop_assert_eq!(
                deterministic_candidates(data.clone()),
                per_comparison_hash_order(data)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::{BTreeSet, HashMap};

    use proptest::prelude::*;
    use serde::Serialize;

    use super::*;

    /// Independent value-shaped oracle for the pre-Arc consensus wire layout.
    #[derive(Serialize)]
    struct LegacyDatum {
        a: Vec<u8>,
        persist: bool,
        source: super::super::trace::event::Produce,
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn arc_backed_source_order_matches_value_shaped_oracle(values in candidate_strategy()) {
            let mut oracle = Vec::with_capacity(values.len());
            for (index, datum) in values.iter().enumerate() {
                let bytes = bincode::serialize(&LegacyDatum {
                    a: datum.a.as_ref().clone(),
                    persist: datum.persist,
                    source: datum.source.clone(),
                }).unwrap();
                prop_assert_eq!(datum.candidate_ordering_bytes(), bytes.clone());
                oracle.push((datum.source.hash.clone(), Blake2b256Hash::new(&bytes), index as i32));
            }
            oracle.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
            let expected: Vec<_> = oracle.into_iter().map(|(_, _, index)| index).collect();
            let actual: Vec<_> = canonical_candidates(values).into_iter().map(|(_, index)| index).collect();
            prop_assert_eq!(actual, expected);
        }
    }

    fn counted<D: CandidateSource + CandidateOrderingBytes>(
        values: Vec<D>,
    ) -> (Vec<(D, i32)>, usize) {
        let digests = Cell::new(0);
        let ordered = canonical_order(values, &|value: &D| {
            digests.set(digests.get() + 1);
            candidate_digest(value)
        });
        (ordered, digests.get())
    }

    /// Reference: sort by (source hash, full digest, index), digesting every
    /// candidate. `CandidateSourceOrder.canonical_sort`.
    fn reference_order<D: CandidateSource + CandidateOrderingBytes>(
        values: Vec<D>,
    ) -> Vec<(D, i32)> {
        let mut keyed: Vec<(Blake2b256Hash, Blake2b256Hash, i32, D)> =
            Vec::with_capacity(values.len());
        for (index, value) in values.into_iter().enumerate() {
            let digest = Blake2b256Hash::new(&value.candidate_ordering_bytes());
            keyed.push((
                value.source_hash().clone(),
                digest,
                i32::try_from(index).expect("test index fits i32"),
                value,
            ));
        }
        keyed.sort_by(|left, right| (&left.0, &left.1, left.2).cmp(&(&right.0, &right.1, right.2)));
        keyed
            .into_iter()
            .map(|(_, _, index, value)| (value, index))
            .collect()
    }

    fn encoded<D: CandidateOrderingBytes>(ordered: &[(D, i32)]) -> Vec<Vec<u8>> {
        ordered
            .iter()
            .map(|(value, _)| value.candidate_ordering_bytes())
            .collect()
    }

    fn candidates() -> impl Strategy<Value = Vec<Datum<Vec<u8>>>> { candidate_strategy() }

    fn tie_members<D: CandidateSource>(values: &[D]) -> usize {
        let mut counts: HashMap<&Blake2b256Hash, usize> = HashMap::with_capacity(values.len());
        for value in values {
            *counts.entry(value.source_hash()).or_insert(0) += 1;
        }
        counts.values().filter(|count| **count >= 2).sum()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// `two_phase_is_canonical` / `lazy_two_phase_is_canonical`.
        #[test]
        fn canonical_order_equals_reference_with_generated_collisions(values in candidates()) {
            let expected = reference_order(values.clone());
            let actual = canonical_candidates(values);
            prop_assert_eq!(encoded(&actual), encoded(&expected));
            prop_assert_eq!(
                actual.iter().map(|(_, index)| *index).collect::<Vec<_>>(),
                expected.iter().map(|(_, index)| *index).collect::<Vec<_>>()
            );
        }

        /// `candidate_order_insertion_independent`.
        #[test]
        fn canonical_order_is_insertion_independent(
            (values, permuted) in candidates().prop_flat_map(|values| {
                let shuffled = Just(values.clone()).prop_shuffle();
                (Just(values), shuffled)
            }),
        ) {
            prop_assert_eq!(
                encoded(&canonical_candidates(values)),
                encoded(&canonical_candidates(permuted))
            );
        }

        /// `filter_commutes_with_canonical_sort`: survivors keep their relative
        /// order whether the filter runs before or after the sort.
        #[test]
        fn filter_then_sort_equals_sort_then_filter(
            values in candidates(),
            keep in prop::collection::vec(any::<bool>(), 24),
        ) {
            let sorted_then_filtered: Vec<i32> = canonical_candidates(values.clone())
                .into_iter()
                .filter(|(_, index)| keep[*index as usize])
                .map(|(_, index)| index)
                .collect();
            let survivors: Vec<(usize, Datum<Vec<u8>>)> = values
                .into_iter()
                .enumerate()
                .filter(|(index, _)| keep[*index])
                .collect();
            let original: Vec<i32> = survivors
                .iter()
                .map(|(index, _)| i32::try_from(*index).expect("test index fits i32"))
                .collect();
            let filtered_then_sorted: Vec<i32> = canonical_candidates(
                survivors.into_iter().map(|(_, value)| value).collect(),
            )
            .into_iter()
            .map(|(_, position)| original[position as usize])
            .collect();
            prop_assert_eq!(sorted_then_filtered, filtered_then_sorted);
        }

        /// `digests_only_for_ties` and `distinct_sources_need_no_digest`: the
        /// order digests exactly the members of runs of two or more equal
        /// source hashes.
        #[test]
        fn ties_digest_only_run_members(values in candidates()) {
            let expected = tie_members(&values);
            let (_, digests) = counted(values);
            prop_assert_eq!(digests, expected);
        }
    }

    /// Sentinel (register I1): a system-contract channel holds many methods.
    /// Twenty-six continuations with distinct sources compute no digest.
    #[test]
    fn twenty_six_distinct_continuations_compute_zero_digests() {
        let continuations: Vec<Arc<WaitingContinuation<u8, u32>>> = (0..26u32)
            .map(|body| {
                Arc::new(WaitingContinuation::create(
                    &vec![7u8],
                    &vec![0u8],
                    &body,
                    true,
                    BTreeSet::new(),
                ))
            })
            .collect();
        let expected = reference_order(continuations.clone());
        let (ordered, digests) = counted(continuations);
        assert_eq!(digests, 0);
        assert_eq!(encoded(&ordered), encoded(&expected));
    }

    #[test]
    fn empty_and_single_candidate_lists_keep_their_index() {
        let (empty, digests) = counted(Vec::<Datum<Vec<u8>>>::new());
        assert!(empty.is_empty());
        assert_eq!(digests, 0);
        let (single, digests) = counted(vec![datum_with_source(vec![1u8], 2, false)]);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].1, 0);
        assert_eq!(digests, 0);
    }
}
