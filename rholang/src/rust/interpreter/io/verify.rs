// Phase 1 (Consensus re-execute + verify, 2026-09-01).
//
// Follower-side hash comparison for observation-only WAL ops
// (Stat first; Read / ReadAt / Entries / Size / EntriesStreamNext
// follow in Phase 2).
//
// Contract with the caller (typically a handler's is_replay = true
// Consensus branch): the handler re-executes the same syscall
// against its own filesystem, builds the reply Par (via the same
// helper the leader used — e.g., `stat_record`), and hands both the
// fresh reply and the cached-from-RSpace `previous` slice here.
//
// This module DOES NOT read the WAL directly.  It compares hashes
// of two Rholang reply Pars: the follower's fresh re-executed reply
// and the leader's cached reply as replayed via RSpace (`previous`).
// `stable_hash(previous.first())` is byte-identical to what the
// leader's `journal_state_read` wrote into
// `PayloadRef::Hash(reply_hash)` at play time — RSpace guarantees
// the produce content is byte-preserved across leader → follower —
// so comparing hash-of-fresh vs hash-of-cached IS the leader-vs-
// follower verification the design intends.
//
// See auto-memory `fileio_wal_replay_verification_gap.md` for the
// full design authority, and `fileio_observation_wal_semantics.md`
// for why observation-op hashes are verification targets (NEVER
// peer-fetch targets).

use models::rhoapi::Par;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;

/// Why the follower's fresh re-executed reply diverges from the
/// leader's cached reply.  Kept small on purpose: additional
/// variants (e.g., `StatFieldSkew` for a future field-strip drift
/// diagnostic) can be added append-only when the mechanism is
/// generalized past hash equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DivergenceReason {
    /// The fresh re-executed reply's stable_hash does not match the
    /// leader's cached reply hash extracted from `previous`.  Hex-
    /// rendered in `Display` for reply / log ergonomics; the raw
    /// bytes stay available for downstream analysis.
    HashMismatch { fresh: [u8; 32], cached: [u8; 32] },
    /// `previous` was empty — the follower had no cached leader
    /// reply Par to compare against.  Indicates an upstream RSpace-
    /// log gap (the leader produced but the log lost it, or the
    /// follower is running against a truncated log).  Rare enough
    /// in practice that it surfaces as a distinct variant for
    /// clarity rather than a special `HashMismatch` shape.
    MissingCachedReply,
}

impl std::fmt::Display for DivergenceReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HashMismatch { fresh, cached } => write!(
                f,
                "hash mismatch (fresh={}, cached={})",
                hex::encode(fresh),
                hex::encode(cached),
            ),
            Self::MissingCachedReply => f.write_str("no cached reply Par in `previous`"),
        }
    }
}

/// Compare `stable_hash(fresh)` against `stable_hash(previous.first())`.
/// Returns `Ok(())` iff the two hashes are byte-equal (the follower's
/// re-executed reply is byte-identical to the leader's cached reply
/// under the canonical Rholang stable-hash provider).
///
/// The 32-byte assertion mirrors `journal_state_read`'s equivalent
/// check in `handlers.rs`; if the stable-hash provider ever returns
/// a differently-sized digest, both sites must panic loudly at first
/// call rather than silently truncating.
pub fn verify_reply_hash_matches_cached(
    fresh: &Par,
    previous: &[Par],
) -> Result<(), DivergenceReason> {
    let cached_par = previous.first().ok_or(DivergenceReason::MissingCachedReply)?;
    let fresh_hash = par_stable_hash(fresh);
    let cached_hash = par_stable_hash(cached_par);
    if fresh_hash == cached_hash {
        Ok(())
    } else {
        Err(DivergenceReason::HashMismatch {
            fresh: fresh_hash,
            cached: cached_hash,
        })
    }
}

fn par_stable_hash(par: &Par) -> [u8; 32] {
    let h = stable_hash_provider::hash(par).bytes();
    assert_eq!(
        h.len(),
        32,
        "stable_hash_provider must produce 32-byte Blake2b256; got {}",
        h.len()
    );
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&h);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::response::{err, ok_par};
    use crate::rust::interpreter::io::stat::stat_record;
    use crate::rust::interpreter::io::ConsensusMode;
    use std::fs;

    /// Baseline: identical reply Pars hash equal → verify returns Ok.
    #[test]
    fn verify_matches_when_pars_are_bytewise_identical() {
        let a = ok_par(models::rhoapi::Par::default());
        let b = ok_par(models::rhoapi::Par::default());
        assert!(verify_reply_hash_matches_cached(&a, &[b]).is_ok());
    }

    /// Divergence: two clearly-different error replies hash unequal.
    /// Confirms the comparator actually reads the byte content.
    #[test]
    fn verify_returns_hash_mismatch_on_distinct_replies() {
        let fresh = err("FSERR_NOT_FOUND", "cake was a lie");
        let cached = err("FSERR_IO", "disk on fire");
        match verify_reply_hash_matches_cached(&fresh, &[cached]) {
            Err(DivergenceReason::HashMismatch { fresh: f, cached: c }) => {
                assert_ne!(f, c, "mismatched replies must hash to distinct digests");
            }
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }

    /// Empty `previous` slice → MissingCachedReply (distinct from
    /// HashMismatch).  Callers can use this to distinguish an
    /// upstream RSpace-log gap from a genuine leader/follower
    /// content divergence.
    #[test]
    fn verify_returns_missing_when_previous_is_empty() {
        let fresh = ok_par(models::rhoapi::Par::default());
        assert_eq!(
            verify_reply_hash_matches_cached(&fresh, &[]),
            Err(DivergenceReason::MissingCachedReply)
        );
    }

    /// End-to-end stat_record shape: two files with identical
    /// permission bits + size but different names produce distinct
    /// Consensus-mode stat_record hashes.  This is the property
    /// that PB-M-14 leader/follower verification actually leans on
    /// under Phase 1 — the follower's fs_stat re-execute produces
    /// the same Consensus-mode stat_record as the leader iff the
    /// underlying file state agrees.
    #[test]
    fn verify_uses_content_bits_of_stat_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"same content").expect("write a");
        fs::write(&b, b"same content").expect("write b");
        let meta_a = fs::metadata(&a).expect("meta a");
        let meta_b = fs::metadata(&b).expect("meta b");
        // Same name arg → same record → hashes match.
        let rec_a1 = ok_par(stat_record("target", &meta_a, ConsensusMode::Consensus));
        let rec_a2 = ok_par(stat_record("target", &meta_a, ConsensusMode::Consensus));
        assert!(verify_reply_hash_matches_cached(&rec_a1, &[rec_a2]).is_ok());
        // Different name arg on the SAME file → distinct record → mismatch.
        let rec_named_b = ok_par(stat_record("other", &meta_b, ConsensusMode::Consensus));
        assert!(matches!(
            verify_reply_hash_matches_cached(&rec_a1, &[rec_named_b]),
            Err(DivergenceReason::HashMismatch { .. })
        ));
    }
}
