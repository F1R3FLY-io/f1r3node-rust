//! THE canonical candidate order of the tuplespace.
//!
//! # Why this module exists
//!
//! RSpace picks the data and the waiting continuations a COMM fires on out of
//! per-channel *pools*. Which element of a pool the matcher reaches first is
//! therefore part of the answer to "which COMM fires", and "which COMM fires"
//! is a consensus quantity: every validator replaying a block must select the
//! same rendezvous the proposer selected, or the post-state root diverges.
//!
//! The pools come out of the hot store in **insertion order**, which is an
//! artifact of how a particular node interleaved its reductions rather than a
//! property of the program. Ordering the pool by a hash of the candidate's own
//! bytes replaces that artifact with a function of the candidate values alone.
//! The original store position survives as the tie-breaker (and as the index
//! `remove_datum` later removes at), so the order is a *total* order — no two
//! entries compare equal — and is reproduced exactly by anyone holding the same
//! pool.
//!
//! ```text
//!   store order          ordering key                      canonical order
//!   ┌───────────┐        ┌───────────────────────┐         ┌───────────┐
//!   │ 0: datum A│  ───▶  │ (H(bytes(A)), 0)      │  sort   │ 2: datum C│
//!   │ 1: datum B│        │ (H(bytes(B)), 1)      │  ────▶  │ 0: datum A│
//!   │ 2: datum C│        │ (H(bytes(C)), 2)      │         │ 1: datum B│
//!   └───────────┘        └───────────────────────┘         └───────────┘
//!             the i32 riding along is always the STORE index
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
//! spatial match — see [`crate::rspace::space_matcher::SpaceMatcher::extract_guarded_data_candidates`].
//! Play and replay therefore share this one order, so that the
//! replay-equivalence argument recorded there ("replay's pool is a subsequence
//! of play's pool, so the lexicographically least admissible selection is the
//! same in both") actually holds.
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
/// P4.1: candidates are `Arc`-shaped (non-serde) — the ordering bytes come from
/// the serializer twins ([`CandidateOrderingBytes`]), byte-identical to the
/// pre-P4.1 `bincode::serialize(candidate)` (golden-pinned). Candidate ordering
/// participates in replay-visible COMM selection; the bytes are a consensus
/// surface.
pub fn deterministic_candidate_hash<D>(candidate: &D) -> Blake2b256Hash
where D: CandidateOrderingBytes {
    let bytes = candidate.candidate_ordering_bytes();
    Blake2b256Hash::new(&bytes)
}

/// Pair every candidate with its **store index** and return the pool in the
/// canonical order described in the module documentation.
///
/// The `i32` of each pair is the candidate's position in `candidates` as
/// received — that is the index `HotStore::remove_datum` /
/// `HotStore::remove_continuation` later address, so it must NOT be recomputed
/// after the sort. The fresh datum a `produce` injects into its own pool is
/// given index `-1` by the caller and is inserted ahead of this order (it is
/// not in the store, so it has no store index and nothing removes it).
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
        left_hash.cmp(right_hash).then_with(|| left_index.cmp(right_index))
    });

    // Undecorate, preserving the store index.
    decorated
        .into_iter()
        .map(|(_, index, candidate)| (candidate, index))
        .collect()
}
