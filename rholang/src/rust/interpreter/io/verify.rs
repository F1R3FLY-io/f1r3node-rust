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
    use crate::rust::interpreter::io::stat::{error_record, stat_record};
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
        use super::super::errors::{FSERR_IO, FSERR_NOT_FOUND};
        let fresh = err(FSERR_NOT_FOUND, "cake was a lie");
        let cached = err(FSERR_IO, "disk on fire");
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

    /// X-1 / CONS-3 (2026-09-12, branch-review-2026-09-11.md): pin
    /// the `stable_hash_provider::hash` output for a small set of
    /// well-known Pars.  The reply-hash verify path
    /// (`verify_reply_hash_matches_cached`) compares
    /// `stable_hash(fresh)` against `stable_hash(previous.first())`;
    /// if the hash function's serialization drifts (bincode
    /// version bump, Par's `Serialize` impl change, hash function
    /// substitution) followers would silently accept mismatched
    /// hashes and diverge at the tuplespace level.
    ///
    /// These golden bytes are the equivalent of the fs_genesis
    /// composed-source hashes: any code change here is a hard-fork
    /// event that must roll every validator's cached-reply-hash
    /// expectations.
    ///
    /// # Regeneration
    ///
    /// If a downstream change is intentional (e.g., bincode
    /// version bump coordinated across the network), regenerate
    /// via `--nocapture` and update the constants below.  Anchor
    /// this to a version-bump commit in the log for auditability.
    #[test]
    fn cons3_stable_hash_pinned_for_known_pars() {
        use super::super::errors::{FSERR_BAD_ARG, FSERR_IO};
        use super::super::response::{
            err_eos, ok_bare, ok_bool, ok_bytes, ok_int, ok_list, ok_string,
        };
        // Case 1: ok_par(Par::default()) — the empty-payload
        // success reply.  Every non-verifying handler that returns
        // `[true]` uses this shape; every verifying handler that
        // hits its own tautological echo path returns it under
        // Consensus mode.
        let empty_ok = ok_par(models::rhoapi::Par::default());
        let empty_ok_hash = par_stable_hash(&empty_ok);
        // Pinned bytes (regenerate on intentional roll).
        const EXPECTED_EMPTY_OK: [u8; 32] = [
            89, 167, 148, 142, 26, 75, 85, 14, 41, 59, 13, 160, 239, 236, 58, 176, 116, 38, 180,
            54, 121, 32, 0, 75, 107, 121, 123, 46, 228, 251, 160, 110,
        ];
        assert_eq!(
            empty_ok_hash, EXPECTED_EMPTY_OK,
            "CONS-3: stable_hash of ok_par(Par::default()) drifted. \
             If intentional (bincode version bump, Par Serialize \
             impl change), coordinate across the network + update \
             this pin.  Otherwise revert the drift-causing edit."
        );
        // Case 2: `err(FSERR_BAD_ARG, "example")` — a concrete
        // error reply.  Exercises the FserrCode + msg encoding
        // through stable_hash.
        let bad_arg_reply = err(FSERR_BAD_ARG, "example");
        let bad_arg_hash = par_stable_hash(&bad_arg_reply);
        const EXPECTED_BAD_ARG: [u8; 32] = [
            7, 68, 175, 62, 6, 223, 205, 97, 254, 101, 101, 108, 179, 34, 2, 59, 84, 168, 160, 44,
            237, 45, 237, 179, 38, 147, 155, 123, 196, 89, 14, 129,
        ];
        assert_eq!(
            bad_arg_hash, EXPECTED_BAD_ARG,
            "CONS-3: stable_hash of err(FSERR_BAD_ARG, \"example\") \
             drifted.  Same rationale as above."
        );
        // Case 3: distinct FSERR codes MUST produce distinct
        // digests — sanity check that the code participates in
        // the hash (a bug that ignored FserrCode would silently
        // pass Cases 1+2 but let leader/follower diverge on
        // otherwise-similar replies).
        let io_reply = err(FSERR_IO, "example");
        let io_hash = par_stable_hash(&io_reply);
        assert_ne!(
            bad_arg_hash, io_hash,
            "CONS-3: distinct FSERR codes with identical message \
             MUST hash distinctly — the FserrCode bytes are part \
             of the stable_hash input"
        );

        // X-5b (2026-09-12): expanded coverage.  Each additional
        // reply shape pins the stable_hash encoding for a distinct
        // ExprInstance kind — a serialization drift in any one
        // element type (EList, GBool, GInt, GByteArray, GString,
        // nested EList) surfaces here as a specific pin mismatch
        // rather than as a broad "empty-ok drifted" false narrow.
        //
        // Case 4: `[true]` bare success (ok_bare).  Distinct from
        // Case 1 (which is `[true, ]` with a trailing Par::default()
        // — subtly different wire bytes).
        let bare_ok_hash = par_stable_hash(&ok_bare());
        const EXPECTED_OK_BARE: [u8; 32] = [
            82, 147, 151, 219, 74, 164, 235, 233, 239, 18, 199, 89, 244, 91, 142, 82, 96, 141, 214,
            219, 56, 78, 181, 71, 121, 197, 126, 184, 153, 217, 232, 6,
        ];
        assert_eq!(
            bare_ok_hash, EXPECTED_OK_BARE,
            "CONS-3: stable_hash of ok_bare() drifted"
        );

        // Case 5: `[true, 42]` — the numeric ok shape used by
        // fs_write's reply (bytes-written count), fs_size, fs_seek
        // (position), etc.  Pins the GInt encoding.
        let ok_int_hash = par_stable_hash(&ok_int(42));
        const EXPECTED_OK_INT_42: [u8; 32] = [
            51, 96, 87, 171, 31, 222, 60, 15, 222, 21, 30, 120, 92, 184, 132, 206, 41, 146, 180,
            180, 104, 64, 40, 150, 147, 209, 204, 34, 95, 93, 134, 255,
        ];
        assert_eq!(
            ok_int_hash, EXPECTED_OK_INT_42,
            "CONS-3: stable_hash of ok_int(42) drifted"
        );

        // Case 6: `[true, true]` — the ok_bool shape used by
        // fs_exists.  A subtle regression that made GBool encode
        // identically to GInt(0/1) would surface here vs. Case 5.
        let ok_bool_hash = par_stable_hash(&ok_bool(true));
        const EXPECTED_OK_BOOL_TRUE: [u8; 32] = [
            47, 136, 47, 67, 46, 225, 56, 12, 207, 13, 251, 168, 53, 69, 16, 147, 54, 70, 249, 24,
            232, 200, 240, 73, 236, 26, 198, 53, 94, 146, 237, 108,
        ];
        assert_eq!(
            ok_bool_hash, EXPECTED_OK_BOOL_TRUE,
            "CONS-3: stable_hash of ok_bool(true) drifted"
        );

        // Case 7: `[true, <bytes>]` — the ok_bytes shape used by
        // fs_read's reply.  Pins the GByteArray encoding.
        let ok_bytes_hash = par_stable_hash(&ok_bytes(vec![0xaa, 0xbb, 0xcc]));
        const EXPECTED_OK_BYTES_AABBCC: [u8; 32] = [
            247, 91, 85, 18, 230, 71, 121, 102, 35, 39, 202, 202, 109, 173, 38, 132, 150, 31, 18,
            130, 118, 50, 245, 107, 157, 75, 250, 81, 219, 28, 249, 29,
        ];
        assert_eq!(
            ok_bytes_hash, EXPECTED_OK_BYTES_AABBCC,
            "CONS-3: stable_hash of ok_bytes([0xaa, 0xbb, 0xcc]) drifted"
        );

        // Case 8: `[true, "hello"]` — the ok_string shape used by
        // fs_quarantine's reply.  Pins the GString encoding.
        let ok_string_hash = par_stable_hash(&ok_string("hello".to_string()));
        const EXPECTED_OK_STRING_HELLO: [u8; 32] = [
            175, 78, 235, 95, 251, 71, 13, 48, 55, 45, 152, 93, 156, 127, 199, 206, 96, 216, 5, 75,
            56, 51, 81, 238, 189, 154, 43, 32, 100, 66, 142, 232,
        ];
        assert_eq!(
            ok_string_hash, EXPECTED_OK_STRING_HELLO,
            "CONS-3: stable_hash of ok_string(\"hello\") drifted"
        );

        // Case 9: `[true, [a, b]]` — the ok_list nested-list shape
        // used by fs_entries' reply (list of [path, kind] pairs).
        // Pins nested EList encoding.
        let entry_a = ok_string("file.bin".to_string());
        let entry_b = ok_string("dir".to_string());
        let ok_nested_hash = par_stable_hash(&ok_list(vec![entry_a, entry_b]));
        const EXPECTED_OK_NESTED_LIST: [u8; 32] = [
            100, 4, 18, 166, 58, 200, 157, 131, 71, 140, 90, 225, 181, 138, 116, 166, 190, 138,
            238, 116, 22, 92, 3, 3, 77, 137, 187, 221, 93, 71, 228, 213,
        ];
        assert_eq!(
            ok_nested_hash, EXPECTED_OK_NESTED_LIST,
            "CONS-3: stable_hash of ok_list([<nested>]) drifted"
        );

        // Case 10: `[false, "EOS"]` — the stream-EOS terminator.
        // Distinct from the 3-element error shape (`[false, code,
        // msg]`); a regression that collapsed EOS into a full
        // FSERR shape would hash differently.
        let eos_hash = par_stable_hash(&err_eos());
        const EXPECTED_ERR_EOS: [u8; 32] = [
            148, 21, 6, 250, 199, 107, 135, 241, 69, 250, 125, 251, 136, 233, 51, 174, 251, 53, 83,
            99, 135, 163, 15, 247, 41, 210, 170, 50, 227, 53, 29, 212,
        ];
        assert_eq!(
            eos_hash, EXPECTED_ERR_EOS,
            "CONS-3: stable_hash of err_eos() drifted"
        );

        // Case 11 (X-8, 2026-09-13, branch-review-2026-09-13.md
        // Track D MINOR): pin `ExprInstance::EMapBody` encoding.
        // `stat_record` and `error_record` are the only current
        // producers of EMapBody-typed reply Pars; without a pin
        // here, a Rholang change that reordered EMap fields or
        // altered the KeyValuePair encoding would drift the
        // `stat_record` hash without a single-shape assertion
        // firing — a Consensus-mode fs_stat divergence would
        // surface only as a HashMismatch at replay time.
        //
        // Uses `error_record` because its inputs are pure static
        // strings, so the hash is deterministic across hosts
        // (unlike `stat_record`, which pulls timestamps + mode
        // bits from live Metadata).  Any encoding-drift regression
        // in the shared `EMap { kvs, locally_free,
        // connective_used, remainder }` serialization surfaces on
        // both `stat_record`-in-Consensus and `error_record` output,
        // so `error_record` is a faithful proxy.
        let emap_hash = par_stable_hash(&ok_par(error_record("target", "example error")));
        const EXPECTED_ERR_EMAP: [u8; 32] = [
            230, 153, 196, 238, 244, 219, 255, 88, 230, 152, 182, 167, 24, 235, 206, 201, 13, 18,
            45, 86, 65, 63, 20, 219, 250, 23, 249, 96, 18, 162, 132, 180,
        ];
        assert_eq!(
            emap_hash, EXPECTED_ERR_EMAP,
            "CONS-3 Case 11: stable_hash of ok_par(error_record(\"target\", \
             \"example error\")) drifted.  This pins the EMapBody encoding \
             (fs_stat / fs_entries Consensus-mode replies rely on it)."
        );

        // Cross-shape distinctness: every pinned digest above must
        // be pairwise-distinct.  A bug that made two shapes collide
        // through stable_hash (e.g., ignoring the outer list
        // discriminator) would slip past the per-case pin assertions
        // (they'd all agree on the same wrong value); this
        // pairwise-distinct check catches that class.
        let all_hashes = [
            ("empty_ok", empty_ok_hash),
            ("bad_arg", bad_arg_hash),
            ("io_err", io_hash),
            ("bare_ok", bare_ok_hash),
            ("ok_int_42", ok_int_hash),
            ("ok_bool_true", ok_bool_hash),
            ("ok_bytes_aabbcc", ok_bytes_hash),
            ("ok_string_hello", ok_string_hash),
            ("ok_nested_list", ok_nested_hash),
            ("err_eos", eos_hash),
            ("err_emap", emap_hash),
        ];
        for i in 0..all_hashes.len() {
            for j in (i + 1)..all_hashes.len() {
                assert_ne!(
                    all_hashes[i].1, all_hashes[j].1,
                    "CONS-3: distinct reply shapes {} and {} MUST hash \
                     distinctly — a collision here indicates the \
                     stable_hash input is missing a discriminator",
                    all_hashes[i].0, all_hashes[j].0,
                );
            }
        }
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
