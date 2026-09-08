// Consensus re-execute + verify (Phase 1 landed 2026-09-01; Phase
// 5 completed the extension across the full verifying-handler set
// on 2026-09-02).
//
// Follower-side hash comparison for the 15 verifying WAL ops.  Every
// `pub async fn fs_*` handler in `handlers.rs` whose replay branch
// contains `match verify_reply_hash_matches_cached(...)` hands its
// fresh replay reply here for comparison against the leader's cached
// reply.  Coverage today: fs_write, fs_write_at, fs_truncate,
// fs_chmod, fs_remove_file, fs_remove_dir, fs_rename, fs_copy_file,
// fs_read, fs_read_at, fs_stat, fs_entries, fs_size, fs_seek,
// fs_exists (the last lifted from an explicit Consensus ban on
// 2026-09-04, when SNAPSHOT_FORMAT_VERSION bumped 5 → 6).  The pin
// `handlers_top_comment_phase5_verifying_count_matches_actual` in
// `fileio_cost_spec.rs` enforces the count.
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
    let cached_par = previous
        .first()
        .ok_or(DivergenceReason::MissingCachedReply)?;
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

/// F-3 fix (2026-09-04): compile-time enforcement of `fs_tell`'s
/// derivative-safety property.
///
/// `fs_tell`'s Consensus follower branch (see
/// `handlers.rs::fs_tell` around line 2519) tautologically echoes
/// `previous` without re-executing `libc::lseek(SEEK_CUR)`.  This
/// is sound as long as the shadow position it reads is byte-
/// identical between leader and follower — which holds IFF every
/// handler that writes `FileHandle.position` is itself Phase-5
/// verified (i.e., calls `verify_reply_hash_matches_cached` on
/// its fresh reply and trips `FSERR_CONSENSUS_DIVERGENCE` on
/// mismatch).
///
/// This enum is the AUTHORITATIVE, exhaustive list of fd-position
/// mutators.  Every variant here MUST have a Phase-5 verify
/// branch; the `phase_5_verify_site` method's exhaustive match
/// forces a new-variant author to name the verify call site.
///
/// # Non-mutating peers deliberately excluded
///
/// - `libc::pread` (fs_read_at) — POSIX-guaranteed to NOT advance
///   OS-fd position.
/// - `libc::pwrite` (fs_write_at) — same POSIX guarantee.
/// - `libc::ftruncate` (fs_truncate) — does not touch position.
/// - `libc::open` (fs_open) — initializes position to 0 as a
///   constant, not a mutation of prior state.
///
/// # What a new fd-position-mutating handler needs
///
/// Suppose a future slice adds `fs_pread_advance` that couples
/// read-then-seek in one call.  The invariant discipline is:
///
/// 1. The handler MUST have a Consensus follower re-execute +
///    verify branch (mirror `fs_seek` / `fs_read` / `fs_write`).
/// 2. Add a variant `FsPreadAdvance` to `FdPositionMutator`.
/// 3. `phase_5_verify_site`'s match becomes non-exhaustive
///    (compile error E0004) — add the arm pointing at the new
///    handler's verify call site.
/// 4. Add the variant to `ALL` so the `fd_position_mutator_all_
///    is_exhaustive` test passes.
///
/// Skipping any step 1 leaves `fs_tell` unsound.  Skipping step
/// 3 fails to compile.  Skipping step 4 fails the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FdPositionMutator {
    /// `libc::lseek` in `fs_seek` sets OS-fd position.
    /// B1 verified (2026-09-03).
    FsSeek,
    /// `libc::read` in `fs_read` advances OS-fd position by the
    /// count of bytes read.  Phase-2 verified (2026-09-01).
    FsRead,
    /// `libc::write` in `fs_write` advances OS-fd position by the
    /// count of bytes written.  Phase-3 verified (2026-09-01).
    FsWrite,
}

impl FdPositionMutator {
    /// All known fd-position-mutating handlers.  Manually
    /// maintained alongside the enum variants; the
    /// `fd_position_mutator_all_is_exhaustive` test in this
    /// module fires if a variant is added to the enum without
    /// being appended here.
    pub const ALL: &'static [Self] = &[Self::FsSeek, Self::FsRead, Self::FsWrite];

    /// Grep-verifiable pointer to the
    /// `verify_reply_hash_matches_cached` call site for this
    /// handler.  A regression that reverted the verify branch to
    /// a Phase-0 tautological pass-through would leave this
    /// string pointing at code that no longer verifies — a code-
    /// review-catchable smell.  The exhaustive `match` forces
    /// every new variant to name its verify site.
    pub const fn phase_5_verify_site(self) -> &'static str {
        match self {
            Self::FsSeek => {
                "rholang/src/rust/interpreter/io/handlers.rs::fs_seek — \
                 Consensus is_replay branch (search: \
                 `verify_reply_hash_matches_cached(&fresh_reply` in fs_seek)"
            }
            Self::FsRead => {
                "rholang/src/rust/interpreter/io/handlers.rs::fs_read — \
                 Consensus is_replay branch (search: \
                 `verify_reply_hash_matches_cached(&fresh_reply` in fs_read)"
            }
            Self::FsWrite => {
                "rholang/src/rust/interpreter/io/handlers.rs::fs_write — \
                 Consensus is_replay branch (search: \
                 `verify_reply_hash_matches_cached(&fresh_reply` in fs_write)"
            }
        }
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

/// Const-eval guard: every variant in `FdPositionMutator::ALL`
/// must return a non-empty `phase_5_verify_site` string.  A
/// regression that returned `""` for a variant (or added a new
/// variant to `ALL` with a placeholder string) fires here at
/// compile time rather than at runtime.  Complements the
/// `fd_position_mutator_all_is_exhaustive` test's exhaustive-
/// variant coverage.
const _: () = {
    let mut i = 0;
    while i < FdPositionMutator::ALL.len() {
        let site = FdPositionMutator::ALL[i].phase_5_verify_site();
        assert!(
            !site.is_empty(),
            "F-3: FdPositionMutator variant has empty phase_5_verify_site — \
             fs_tell's tautological pass-through is unsound"
        );
        i += 1;
    }
};

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::rust::interpreter::io::response::{err, ok_par};
    use crate::rust::interpreter::io::stat::stat_record;
    use crate::rust::interpreter::io::ConsensusMode;

    /// F-3 pin (2026-09-04): `FdPositionMutator::ALL` must be
    /// exhaustive with respect to the enum's variants.  The
    /// pattern-match arm below fails to compile (E0004
    /// non-exhaustive) if a new variant is added.  The runtime
    /// `assert!(ALL.contains(&m))` then fails if the new variant
    /// wasn't appended to `ALL`.
    ///
    /// Together with the const-eval guard above, this pins:
    ///   - Every enum variant is reachable through `ALL`.
    ///   - Every enum variant has a non-empty verify-site pointer.
    ///   - Adding a new variant forces the new-variant author to
    ///     touch this test AND `phase_5_verify_site`, giving
    ///     three compile-time reminders that fs_tell's tautology
    ///     depends on Phase-5 verify being in place.
    #[test]
    fn fd_position_mutator_all_is_exhaustive() {
        // Any variant addition without a match arm here fails to
        // compile.
        fn tag(m: FdPositionMutator) -> &'static str {
            match m {
                FdPositionMutator::FsSeek => "seek",
                FdPositionMutator::FsRead => "read",
                FdPositionMutator::FsWrite => "write",
            }
        }
        // Every variant must ALSO be in `ALL` — the const array is
        // manually maintained and this is where discipline gets
        // enforced at test time.
        for m in [
            FdPositionMutator::FsSeek,
            FdPositionMutator::FsRead,
            FdPositionMutator::FsWrite,
        ] {
            assert!(
                FdPositionMutator::ALL.contains(&m),
                "F-3 discipline: {} is a FdPositionMutator variant but \
                 missing from ALL — fs_tell's tautological pass-through \
                 is unsound",
                tag(m),
            );
        }
        // Cross-check ALL doesn't carry duplicates.
        let mut seen: Vec<FdPositionMutator> = Vec::new();
        for m in FdPositionMutator::ALL {
            assert!(
                !seen.contains(m),
                "F-3 discipline: {} appears twice in ALL",
                tag(*m),
            );
            seen.push(*m);
        }
    }

    /// Belt-and-suspenders: pin the current-slice contents of
    /// `ALL` at 3 variants (fs_seek / fs_read / fs_write).  A
    /// future slice that adds an fd-position-mutating handler
    /// must intentionally bump this pin, which is a hook for a
    /// reviewer to check that the new handler ALSO added a
    /// Phase-5 verify branch — the compile-time enforcement
    /// above handles the enum-level discipline, but the human-
    /// review step is what catches a bogus verify site.
    #[test]
    fn fd_position_mutator_all_size_is_pinned() {
        assert_eq!(
            FdPositionMutator::ALL.len(),
            3,
            "F-3 pin: adding an fd-position mutator?  Confirm the new \
             handler has a Consensus follower verify_reply_hash_matches_\
             cached branch, then bump this pin to reflect the new count."
        );
    }

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
            Err(DivergenceReason::HashMismatch {
                fresh: f,
                cached: c,
            }) => {
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
