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
//! values. The original vector position survives as both tie-breaker and the
//! continuation-removal address, making the order total.
//!
//! Datum candidates no longer use this vector/index representation. Their
//! authoritative pool is a PathMap Bag, their total order is `(hash, trie
//! path)`, and their removal address is the exact trie path; see
//! [`crate::rspace::data_index::RSpaceDataIndex::ordered_channel_entries`].
//!
//! ```text
//!   store order          ordering key                      canonical order
//!   ┌───────────┐        ┌───────────────────────┐         ┌───────────┐
//!   │ 0: cont A │  ───▶  │ (H(bytes(A)), 0)      │  sort   │ 2: cont C │
//!   │ 1: cont B │        │ (H(bytes(B)), 1)      │  ────▶  │ 0: cont A │
//!   │ 2: cont C │        │ (H(bytes(C)), 2)      │         │ 1: cont B │
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
//! the `Θ(n log n)` comparisons — for a pool of `n` candidates that is
//! `Θ(n log n)` bincode serializations where `n` suffice. The resulting
//! permutation is identical: the key `(hash, store_index)` is a total order, so
//! stable and unstable sorts agree on it.

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
    // Decorate: one hash per candidate, computed exactly once.
    let mut decorated: Vec<(Blake2b256Hash, i32, D)> = candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| {
            (deterministic_candidate_hash(&candidate), index as i32, candidate)
        })
        .collect();

    // Sort: `(hash, store_index)` is a TOTAL order (indices are unique), so an
    // unstable sort yields the same permutation a stable one would — without
    // the stable sort's scratch allocation.
    decorated.sort_unstable_by(|(left_hash, left_index, _), (right_hash, right_index, _)| {
        left_hash
            .cmp(right_hash)
            .then_with(|| left_index.cmp(right_index))
    });

    // Undecorate, preserving the store index.
    decorated
        .into_iter()
        .map(|(_, index, candidate)| (candidate, index))
        .collect()
}
