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
}
