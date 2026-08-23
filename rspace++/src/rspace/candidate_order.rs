//! THE canonical candidate order of the tuplespace.
//!
//! # Why this module exists
//!
//! RSpace picks waiting continuations from vector-backed per-channel-group
//! pools. Which continuation the matcher reaches first is therefore part of
//! the answer to "which COMM fires", and "which COMM fires" is a consensus
//! quantity: every validator replaying a block must select the same rendezvous
//! the proposer selected, or the post-state root diverges.
//!
//! Those vectors come out of the hot store in **insertion order**, which is an
//! artifact of how a particular node interleaved its reductions. Ordering by a
//! hash of each candidate's bytes replaces that artifact with a function of the
//! values. Complete canonical identity bytes resolve a digest collision. The
//! original vector position is consulted only for byte-identical repetitions;
//! it also remains the continuation-removal address, making the order total.
//!
//! Datum candidates no longer use this vector/index representation. Their
//! authoritative pool is a PathMap Bag, their total order is `(hash,
//! canonical datum identity bytes)`, and their removal address remains the
//! exact trie path; see
//! [`crate::rspace::data_index::RSpaceDataIndex::ordered_channel_entries`].
//!
//! ```text
//!   store order          ordering key                      canonical order
//!   ┌───────────┐        ┌───────────────────────┐         ┌───────────┐
//!   │ 0: cont A │  ───▶  │ (H(A), bytes(A), 0)   │  sort   │ 2: cont C │
//!   │ 1: cont B │        │ (H(B), bytes(B), 1)   │  ────▶  │ 0: cont A │
//!   │ 2: cont C │        │ (H(C), bytes(C), 2)   │         │ 1: cont B │
//!   └───────────┘        └───────────────────────┘         └───────────┘
//!             the i32 riding along is the vector store index
//! ```
//!
//! # Why it is shared
//!
//! Before this module the canonical order was a private method of
//! [`crate::rspace::rspace::RSpace`] and the *replay* space used raw store
//! order instead. That asymmetry was survivable only because replay filters its
//! pools down to the produces the recorded COMM actually consumed, which
//! usually leaves one candidate per bind and makes order moot. It stops being
//! moot as soon as the matcher can select a candidate other than the first
//! spatial match — see
//! [`crate::rspace::space_matcher::SpaceMatcher::extract_guarded_data_candidates`].
//! Play and replay therefore share this order for continuations. They share the
//! PathMap-backed candidate-pool construction for data, so the same
//! replay-equivalence argument holds on both axes.
//!
//! # Cost
//!
//! [`order_candidates_with_index`] computes each candidate's hash **once**
//! (decorate-sort-undecorate) rather than twice per comparison. The previous
//! in-comparator form re-serialized and re-hashed both operands on every one of
//! the `$`\Theta(n \log n)`$` comparisons — for a pool of `$`n`$` candidates
//! that is `$`\Theta(n \log n)`$` bincode serializations where `$`n`$` suffice.
//! The resulting key `(hash, canonical identity bytes, store index)` is a total
//! order, so stable and unstable sorts agree on it. Store index affects only
//! byte-identical repetitions.

use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::serializers::serializers::CandidateOrderingBytes;

/// The ordering hash of one candidate.
///
/// Candidates are `Arc`-shaped and non-serde; the ordering bytes come from
/// the serializer twins ([`CandidateOrderingBytes`]), byte-identical to the
/// the earlier value-shaped `bincode::serialize(candidate)` (golden-pinned).
/// Candidate ordering participates in replay-visible COMM selection; the bytes
/// are a consensus surface.
pub fn deterministic_candidate_hash<D>(candidate: &D) -> Blake2b256Hash
where D: CandidateOrderingBytes {
    let bytes = candidate.candidate_ordering_bytes();
    Blake2b256Hash::new(&bytes)
}

/// Pair every vector-backed candidate with its **store index** and return the
/// pool in the canonical order described in the module documentation.
///
/// The `i32` of each pair is the candidate's position in `candidates` as
/// received — currently the index `HotStore::remove_continuation` later
/// addresses — so it must not be recomputed after the sort.
pub fn order_candidates_with_index<D>(candidates: Vec<D>) -> Vec<(D, i32)>
where D: CandidateOrderingBytes {
    order_candidates_with_index_by(candidates, continuation_identity_digest)
}

#[cfg(not(test))]
#[inline(always)]
fn continuation_identity_digest(identity: &[u8]) -> Blake2b256Hash { Blake2b256Hash::new(identity) }

#[cfg(test)]
fn continuation_identity_digest(identity: &[u8]) -> Blake2b256Hash {
    super::candidate_digest_test_hook::digest(identity)
}

fn order_candidates_with_index_by<D>(
    candidates: Vec<D>,
    mut hash_identity: impl FnMut(&[u8]) -> Blake2b256Hash,
) -> Vec<(D, i32)>
where
    D: CandidateOrderingBytes,
{
    // Decorate: canonical identity bytes and their hash are each computed
    // exactly once. The complete bytes are retained only through the sort and
    // make the collision rule representation- and insertion-order independent.
    let mut decorated: Vec<(Blake2b256Hash, Vec<u8>, i32, D)> = candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| {
            let identity = candidate.candidate_ordering_bytes();
            let hash = hash_identity(&identity);
            (hash, identity, index as i32, candidate)
        })
        .collect();

    // Sort: identity bytes resolve a digest collision exactly as the retained
    // PathMap datum catalog does. Store index is consulted only for byte-
    // identical repetitions whose removal address must remain distinct. The
    // resulting key is total, so unstable sorting needs no scratch buffer.
    decorated.sort_unstable_by(
        |(left_hash, left_identity, left_index, _),
         (right_hash, right_identity, right_index, _)| {
            left_hash
                .cmp(right_hash)
                .then_with(|| left_identity.cmp(right_identity))
                .then_with(|| left_index.cmp(right_index))
        },
    );

    // Undecorate, preserving the store index.
    decorated
        .into_iter()
        .map(|(_, _, index, candidate)| (candidate, index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Identity(&'static [u8]);

    impl CandidateOrderingBytes for Identity {
        fn candidate_ordering_bytes(&self) -> Vec<u8> { self.0.to_vec() }
    }

    fn collision(_: &[u8]) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![0xAB; 32]) }

    #[test]
    fn forced_hash_collisions_use_identity_then_store_index() {
        let forwards = order_candidates_with_index_by(
            vec![Identity(b"z"), Identity(b"same"), Identity(b"a"), Identity(b"same")],
            collision,
        );
        let backwards = order_candidates_with_index_by(
            vec![Identity(b"same"), Identity(b"a"), Identity(b"same"), Identity(b"z")],
            collision,
        );

        assert_eq!(
            forwards
                .iter()
                .map(|(value, _)| value.0)
                .collect::<Vec<_>>(),
            vec![b"a".as_slice(), b"same".as_slice(), b"same".as_slice(), b"z".as_slice()]
        );
        assert_eq!(
            backwards
                .iter()
                .map(|(value, _)| value.0)
                .collect::<Vec<_>>(),
            vec![b"a".as_slice(), b"same".as_slice(), b"same".as_slice(), b"z".as_slice()]
        );
        assert_eq!(
            forwards
                .iter()
                .filter(|(value, _)| value.0 == b"same")
                .map(|(_, index)| *index)
                .collect::<Vec<_>>(),
            vec![1, 3],
            "byte-identical repetitions retain their distinct store removal addresses"
        );
    }
}
