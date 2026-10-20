// Ack-channel hashing used as the sidecar key for WAL entries.
//
// The hash is derived the same way rspace computes `channel_hash`
// for a produce event, via `stable_hash_provider::hash`.  The
// resulting 32-byte digest serves two roles:
//
//   1. Sidecar key on [`Wal::append_with_ack`] — downstream
//      finalize_failure / finalize_write hooks look the entry up by
//      ack hash to patch its [`WalOutcome`] after the syscall
//      returns.
//   2. Match point for the deploy_log's `ProduceEvent::channels_hash`
//      — the same hash appears when the handler publishes its reply,
//      so the log-order drain can pair WAL entries with their
//      reply emissions.
//
// # Why fail-hard on digest-length mismatch
//
// Pre-fix (fileio M-9, 2026-08-06) the length check was a
// `debug_assert_eq!` + `.min(32)` silent truncation.  Under release
// builds a Blake2b256 provider swap producing shorter output would
// silently zero-pad — hash collisions on the sentinel `[0u8; 32]`
// would misroute log-order drain and ultimately diverge replay.
// `assert_eq!` surfaces the misconfiguration at first call rather
// than as a downstream consensus divergence.

use models::rhoapi::Par;

/// Compute the ack channel's 32-byte Blake2b256 digest.
///
/// Panics (release-mode) if the underlying `stable_hash_provider`
/// returns a digest ≠ 32 bytes — the WAL ack sidecar hard-depends
/// on a fixed 32-byte width.
pub fn ack_channel_hash(ack: &Par) -> [u8; 32] {
    let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(ack).bytes();
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — the WAL ack sidecar \
         hard-depends on a fixed 32-byte hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Derive a unique per-entry ack hash for a recursive-removeDir
/// manifest entry.  Rholang callers see one ack channel for the
/// whole `fsRemoveDir!(...)` call, but the leader and follower
/// each append MANY WAL entries (one per tree leaf).  To keep the
/// WAL's `ack_hashes` sidecar meaningful (log-order drain,
/// per-entry outcome finalize), each entry needs its own sidecar
/// key.  Both sides derive the same key from the shared ack
/// channel + entry's canonical path:
///
/// ```text
/// Blake2b256(ack_channel_hash(ack) || 0xFE || path_bytes)
/// ```
///
/// The `0xFE` separator prevents accidental collision with the
/// standard [`ack_channel_hash`] used for single-entry ops (which
/// never sees a path-suffix domain byte).  Determinism is
/// symmetric: leader and follower see the same ack Par (via rig
/// replay) and the same canonical path (via the reply-manifest
/// lookup + `canonicalize_lexical`).
pub fn per_entry_ack_seed(ack: &Par, path: &std::path::Path) -> [u8; 32] {
    let base = ack_channel_hash(ack);
    let path_bytes = path.as_os_str().as_encoded_bytes();
    let mut buf = Vec::with_capacity(base.len() + 1 + path_bytes.len());
    buf.extend_from_slice(&base);
    buf.push(0xFE);
    buf.extend_from_slice(path_bytes);
    let h = crypto::rust::hash::blake2b256::Blake2b256::hash(buf);
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — \
         per-entry ack sidecar hard-depends on a fixed 32-byte \
         hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// X-3 / SEC-Mi-03: cap for `walk_dirfd_recursive`'s descent-depth
/// defense against pathological deeply-nested directory trees
/// that could exhaust the OS stack.  Linux ulimit for stack is
/// typically ~2 MiB → ~1000+ frames at the walker's ~2 KiB/frame
/// local allocations.
///
/// Consensus mode is protected by the per-FIP threat model
/// (consensus-managed trees have no adversarial writer); Oracular
/// mode is the surface this cap defends.
pub const MAX_RECURSION_DEPTH: usize = 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};

    /// Hashing two different Pars produces two different digests —
    /// the ack hash discriminates between different ack channels.
    /// Pins against a regression that returns a constant digest.
    #[test]
    fn distinct_pars_hash_to_distinct_digests() {
        let a = RhoString::create_par("ack-a".to_string());
        let b = RhoString::create_par("ack-b".to_string());
        assert_ne!(ack_channel_hash(&a), ack_channel_hash(&b));
    }

    /// Hashing the same Par twice is deterministic — the ack hash
    /// is reproducible across calls (leader + follower see the same
    /// hash from the same ack channel).
    #[test]
    fn hash_is_deterministic_across_calls() {
        let p = RhoString::create_par("stable-ack".to_string());
        assert_eq!(ack_channel_hash(&p), ack_channel_hash(&p));
    }

    /// Digest is always 32 bytes.  Compile-time witness is baked
    /// into the return type (`[u8; 32]`); this runtime check pins
    /// against a type-level accident that changed the length.
    #[test]
    fn digest_length_is_32() {
        let p = RhoNumber::create_par(42);
        let d = ack_channel_hash(&p);
        assert_eq!(d.len(), 32);
    }

    /// Digest doesn't collapse to the all-zero sentinel for a
    /// non-trivial Par.  `[0u8; 32]` is the "no ack" sentinel used
    /// by `Wal::append` (ack-less); a hash that collided to it
    /// would misroute `update_outcome_by_ack_hash` lookups.
    #[test]
    fn digest_is_not_zero_sentinel_for_non_trivial_par() {
        let p = RhoString::create_par("real-ack-channel".to_string());
        assert_ne!(ack_channel_hash(&p), [0u8; 32]);
    }

    // --- per_entry_ack_seed --------------------------------------

    #[test]
    fn per_entry_seed_distinct_from_ack_channel_hash() {
        // LOAD-BEARING: the 0xFE separator prevents accidental
        // collision between a single-entry op's `ack_channel_hash(ack)`
        // and a per-entry recursive-manifest hash.  Even with an
        // empty path, the seed differs because the separator byte
        // alone changes the digest.
        let ack = RhoString::create_par("ack-chan".to_string());
        let base = ack_channel_hash(&ack);
        let per_entry = per_entry_ack_seed(&ack, std::path::Path::new(""));
        assert_ne!(base, per_entry);
    }

    #[test]
    fn per_entry_seed_differs_across_paths() {
        // LOAD-BEARING: each tree leaf needs its own sidecar key.
        // Same ack + different paths must produce different seeds.
        let ack = RhoString::create_par("ack-chan".to_string());
        let a = per_entry_ack_seed(&ack, std::path::Path::new("/a/leaf.bin"));
        let b = per_entry_ack_seed(&ack, std::path::Path::new("/b/leaf.bin"));
        assert_ne!(a, b);
    }

    #[test]
    fn per_entry_seed_differs_across_acks() {
        // Different ack channels for the same path produce
        // different per-entry seeds.  Follows from
        // `ack_channel_hash`'s determinism-over-ack.
        let ack_a = RhoString::create_par("ack-a".to_string());
        let ack_b = RhoString::create_par("ack-b".to_string());
        let path = std::path::Path::new("/tree/leaf.bin");
        assert_ne!(
            per_entry_ack_seed(&ack_a, path),
            per_entry_ack_seed(&ack_b, path)
        );
    }

    #[test]
    fn per_entry_seed_is_deterministic() {
        // Leader + follower see the same ack + same canonical
        // path → same seed.  Two calls at the leader side also
        // produce the same seed (needed for finalize_write hooks
        // that recompute the seed from the manifest entry).
        let ack = RhoString::create_par("ack-chan".to_string());
        let path = std::path::Path::new("/stable/leaf.bin");
        assert_eq!(
            per_entry_ack_seed(&ack, path),
            per_entry_ack_seed(&ack, path)
        );
    }

    #[test]
    fn per_entry_seed_digest_length_is_32() {
        let ack = RhoString::create_par("ack".to_string());
        let seed = per_entry_ack_seed(&ack, std::path::Path::new("/some/path"));
        assert_eq!(seed.len(), 32);
    }

    #[test]
    fn max_recursion_depth_const_pin() {
        // Pins the consensus-flavored cap.  A future refactor can't
        // silently shift the OS-stack defense threshold.
        assert_eq!(MAX_RECURSION_DEPTH, 1024);
    }
}
