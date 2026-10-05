//! Canonical order of RSpace match candidates (I1, DR-75).
//!
//! The order is lexicographic in (source hash, full digest, store index).
//! Phase one sorts by (source hash, index) and reads only the precomputed
//! `Produce`/`Consume` hashes. Phase two re-sorts each run of two or more equal
//! source hashes by (digest, index), so a digest is computed only for a member
//! of such a run. Play, directive replay and metered native replay share this
//! function, so they choose the same candidate.
//!
//! Formal model:
//! `formal/rocq/cost_accounted_rho/theories/CandidateSourceOrder.v`
//! (`lazy_two_phase_is_canonical`, `phase_one_ignores_digests`,
//! `digests_only_for_ties`, `candidate_order_insertion_independent`,
//! `filter_commutes_with_canonical_sort`) and
//! `formal/tlaplus/cost_accounted_rho/NativeCandidateOrder.tla`.

use std::cmp::Ordering;
use std::convert::Infallible;
use std::sync::Arc;

use serde::Serialize;

use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, WaitingContinuation};

pub(crate) trait CandidateSource {
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

/// The primitive work of the canonical order. [`Unmetered`] serves play and
/// directive replay. Metered native replay reserves host work before each
/// primitive.
pub(crate) trait OrderWork<D> {
    type Error;

    fn index_bound(&self, length: usize) -> Result<i32, Self::Error>;

    fn buffer<T>(&self, length: usize) -> Result<Vec<T>, Self::Error>;

    fn sort<T>(
        &self,
        values: &mut [T],
        compare: impl Fn(&T, &T) -> Ordering,
        scanned: impl Fn(&T) -> usize,
    ) -> Result<(), Self::Error>;

    fn scan(&self, left: &Blake2b256Hash, right: &Blake2b256Hash) -> Result<(), Self::Error>;

    fn digest(&self, value: &D) -> Result<Blake2b256Hash, Self::Error>;
}

struct Entry<D> {
    value: D,
    index: i32,
    digest: Option<Blake2b256Hash>,
}

pub(crate) fn canonical_order<D: CandidateSource, W: OrderWork<D>>(
    values: Vec<D>,
    work: &W,
) -> Result<Vec<(D, i32)>, W::Error> {
    let count = work.index_bound(values.len())?;
    if values.len() <= 1 {
        let mut ordered = work.buffer(values.len())?;
        ordered.extend((0..count).zip(values).map(|(index, value)| (value, index)));
        return Ok(ordered);
    }
    let mut entries = work.buffer(values.len())?;
    entries.extend((0..count).zip(values).map(|(index, value)| Entry {
        value,
        index,
        digest: None,
    }));
    work.sort(
        &mut entries,
        |left, right| {
            left.value
                .source_hash()
                .cmp(right.value.source_hash())
                .then_with(|| left.index.cmp(&right.index))
        },
        |entry| entry.value.source_hash().0.len(),
    )?;
    let mut start = 0;
    while start < entries.len() {
        let mut end = start + 1;
        while end < entries.len() {
            let previous = entries[end - 1].value.source_hash();
            let next = entries[end].value.source_hash();
            work.scan(previous, next)?;
            if previous != next {
                break;
            }
            end += 1;
        }
        if end - start >= 2 {
            settle_tie_run(&mut entries[start..end], work)?;
        }
        start = end;
    }
    let mut ordered = work.buffer(entries.len())?;
    ordered.extend(entries.into_iter().map(|entry| (entry.value, entry.index)));
    Ok(ordered)
}

fn settle_tie_run<D, W: OrderWork<D>>(run: &mut [Entry<D>], work: &W) -> Result<(), W::Error> {
    for entry in run.iter_mut() {
        entry.digest = Some(work.digest(&entry.value)?);
    }
    work.sort(
        run,
        |left, right| {
            left.digest
                .cmp(&right.digest)
                .then_with(|| left.index.cmp(&right.index))
        },
        |entry| entry.digest.as_ref().map_or(0, |digest| digest.0.len()),
    )
}

pub(crate) struct Unmetered;

impl<D: Serialize> OrderWork<D> for Unmetered {
    type Error = Infallible;

    fn index_bound(&self, length: usize) -> Result<i32, Infallible> {
        Ok(i32::try_from(length).expect("an RSpace candidate list fits the i32 store index"))
    }

    fn buffer<T>(&self, length: usize) -> Result<Vec<T>, Infallible> {
        Ok(Vec::with_capacity(length))
    }

    fn sort<T>(
        &self,
        values: &mut [T],
        compare: impl Fn(&T, &T) -> Ordering,
        _scanned: impl Fn(&T) -> usize,
    ) -> Result<(), Infallible> {
        values.sort_unstable_by(compare);
        Ok(())
    }

    fn scan(&self, _left: &Blake2b256Hash, _right: &Blake2b256Hash) -> Result<(), Infallible> {
        Ok(())
    }

    fn digest(&self, value: &D) -> Result<Blake2b256Hash, Infallible> {
        Ok(Blake2b256Hash::new(&bincode::serialize(value).unwrap_or_default()))
    }
}

pub(crate) fn canonical_candidates<D: CandidateSource + Serialize>(
    values: Vec<D>,
) -> Vec<(D, i32)> {
    match canonical_order(values, &Unmetered) {
        Ok(ordered) => ordered,
        Err(never) => match never {},
    }
}

#[cfg(test)]
pub(crate) fn datum_with_source<A: Clone>(a: A, source: u8, persist: bool) -> Datum<A> {
    use super::trace::event::Produce;
    Datum {
        a,
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

#[cfg(test)]
pub(crate) fn encoded_order<D: Serialize>(ordered: &[(D, i32)]) -> Vec<(Vec<u8>, i32)> {
    ordered
        .iter()
        .map(|(value, index)| {
            (bincode::serialize(value).expect("test candidate serializes"), *index)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::{BTreeSet, HashMap};

    use proptest::prelude::*;

    use super::*;

    struct CountingWork {
        digests: Cell<usize>,
    }

    impl<D: Serialize> OrderWork<D> for CountingWork {
        type Error = Infallible;

        fn index_bound(&self, length: usize) -> Result<i32, Infallible> {
            <Unmetered as OrderWork<D>>::index_bound(&Unmetered, length)
        }

        fn buffer<T>(&self, length: usize) -> Result<Vec<T>, Infallible> {
            <Unmetered as OrderWork<D>>::buffer(&Unmetered, length)
        }

        fn sort<T>(
            &self,
            values: &mut [T],
            compare: impl Fn(&T, &T) -> Ordering,
            scanned: impl Fn(&T) -> usize,
        ) -> Result<(), Infallible> {
            <Unmetered as OrderWork<D>>::sort(&Unmetered, values, compare, scanned)
        }

        fn scan(&self, left: &Blake2b256Hash, right: &Blake2b256Hash) -> Result<(), Infallible> {
            <Unmetered as OrderWork<D>>::scan(&Unmetered, left, right)
        }

        fn digest(&self, value: &D) -> Result<Blake2b256Hash, Infallible> {
            self.digests.set(self.digests.get() + 1);
            Unmetered.digest(value)
        }
    }

    fn counted<D: CandidateSource + Serialize>(values: Vec<D>) -> (Vec<(D, i32)>, usize) {
        let work = CountingWork {
            digests: Cell::new(0),
        };
        let ordered = match canonical_order(values, &work) {
            Ok(ordered) => ordered,
            Err(never) => match never {},
        };
        (ordered, work.digests.get())
    }

    /// Reference: sort by (source hash, full digest, index), digesting every
    /// candidate. `CandidateSourceOrder.canonical_sort`.
    fn reference_order<D: CandidateSource + Serialize>(values: Vec<D>) -> Vec<(D, i32)> {
        let mut keyed: Vec<(Blake2b256Hash, Blake2b256Hash, i32, D)> =
            Vec::with_capacity(values.len());
        for (index, value) in values.into_iter().enumerate() {
            let digest = Blake2b256Hash::new(
                &bincode::serialize(&value).expect("test candidate serializes"),
            );
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

    fn encoded<D: Serialize>(ordered: &[(D, i32)]) -> Vec<Vec<u8>> {
        ordered
            .iter()
            .map(|(value, _)| bincode::serialize(value).expect("test candidate serializes"))
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
