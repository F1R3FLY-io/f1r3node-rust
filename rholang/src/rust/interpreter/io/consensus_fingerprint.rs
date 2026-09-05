// M-8 fix middle option (2026-08-06): consensus runtime fingerprint
// used to augment the operator's `network_id` at boot.
//
// # Threat model
//
// Every constant enumerated in `docs/consensus-invariants.md § 5`
// (byte gates) is consensus-observable — a validator running a
// different value emits `FSERR_QUOTA_EXCEEDED` on different inputs
// than its peers and silently forks the tuplespace.  Same threat
// for `LOCK_ID_CEILING` (per §5 note) and `SNAPSHOT_FORMAT_VERSION`
// (per §3 WAL entry serialization).  Compile-time floor checks +
// the hard-fork catalog catch accidental *lowering* of these
// constants, but a targeted binary patch or a fork of the source
// (with a different constant) would still build cleanly and start
// up.
//
// # Middle-option fix
//
// At boot, node computes a short hex `fingerprint` from every
// consensus-observable runtime constant listed in §5.  Post B2
// expansion (2026-09-03): coverage is 10 constants (MAX_WAL_ENTRIES
// + 7 byte gates + LOCK_ID_CEILING + SNAPSHOT_FORMAT_VERSION).  New
// constants MUST be appended (never inserted mid-list) — see
// `consensus_runtime_fingerprint` docstring.  The fingerprint is
// appended to the operator's `network_id` as `<network_id>#cf<hex>`
// before the value is baked into the TLS interceptor.  Peers with
// different fingerprints see a mismatched `network_id` and get
// refused by the existing `SslSessionServerInterceptor::
// validate_network_id` path — the exact same code path that
// already rejects wrong-network peers.
//
// # Trade-offs
//
// - **No protobuf change.**  The `Header.networkId` field is
//   still a String; the fingerprint travels inside it as an
//   opaque suffix.  Zero on-wire schema modification.
// - **No new handshake round-trip.**  The check piggy-backs on
//   the first message.
// - **Coordinated upgrade required.**  Once deployed, this node
//   won't peer with un-upgraded peers (their `network_id` lacks
//   the `#cf<hex>` suffix).  Same upgrade profile as any other
//   consensus-critical change.
// - **Weaker than a Genesis parameter** (the alternative M-8
//   design).  The fingerprint isn't committed to on-chain state,
//   so a shard-post-hoc audit can't determine which cap the
//   Genesis block was formed with.  For per-node fleet-drift
//   protection, that's acceptable; for hard-cap-was-what state
//   provenance, use the Genesis-parameter design instead.

use crypto::rust::hash::blake2b256::Blake2b256;

use super::handlers::{MAX_ENTRIES, MAX_WRITE_BYTES};
use super::lock::{LOCK_ID_CEILING, MAX_RANGES_PER_FILE, MAX_WAITERS_PER_FILE};
use super::snapshot::SNAPSHOT_FORMAT_VERSION;
use super::wal::MAX_WAL_ENTRIES;
use super::{MAX_CHUNK_ITEMS, MAX_OPEN_FDS, MAX_READ_BYTES, MAX_TRUNCATE_BYTES};

/// Delimiter separating the operator's network_id from the
/// consensus fingerprint.  `#` chosen because it's URL-safe,
/// not in the alphanumeric identifier set operators typically
/// use for network names, and unambiguous in log lines.
///
/// Length of the fingerprint is 16 hex chars (8 bytes) — enough
/// entropy to make accidental collisions astronomically unlikely
/// while keeping the augmented network_id short enough to log.
const FINGERPRINT_DELIMITER: &str = "#cf";
const FINGERPRINT_HEX_LEN: usize = 16; // 8 bytes × 2

/// Compute the hex fingerprint of all consensus-observable
/// runtime constants.  Coverage post B2 expansion (2026-09-03) —
/// every constant enumerated in `docs/consensus-invariants.md § 5`
/// (byte gates) plus `LOCK_ID_CEILING` and `SNAPSHOT_FORMAT_VERSION`.
///
/// The append order is FIXED — reordering flips the fingerprint of
/// an unchanged fleet and force-splits peering across a benign
/// refactor.  New consensus-observable constants MUST be appended
/// at the END of this list (never inserted mid-list); each addition
/// is a coordinated peer-upgrade event since it flips the
/// fingerprint.
///
/// Current fold order (do not reorder):
///  1. `MAX_WAL_ENTRIES` (u64 BE)
///  2. `MAX_WRITE_BYTES` (u64 BE)
///  3. `MAX_READ_BYTES` (u64 BE)
///  4. `MAX_TRUNCATE_BYTES` (u64 BE)
///  5. `MAX_ENTRIES` (u64 BE)
///  6. `MAX_OPEN_FDS` (u64 BE)
///  7. `MAX_RANGES_PER_FILE` (u64 BE)
///  8. `MAX_WAITERS_PER_FILE` (u64 BE)
///  9. `LOCK_ID_CEILING` (u64 BE)
/// 10. `SNAPSHOT_FORMAT_VERSION` (u8)
/// 11. `MAX_CHUNK_ITEMS` (u64 BE) — M-19 review follow-up
///     (2026-09-04, Gap 2): appended so a per-validator patch of
///     the `Stream.rho::chunk(@n)` cap doesn't silently peer with
///     divergent nodes.
///
/// Returns 16-char lowercase hex (first 8 bytes of Blake2b256).
pub fn consensus_runtime_fingerprint() -> String {
    let mut buf = Vec::with_capacity(8 * 10 + 1);
    // Cast every usize/u8 to u64/u8 explicitly so the encoding is
    // portable across 32/64-bit builds — a validator with the same
    // constants but a different pointer width must produce the
    // same fingerprint.
    buf.extend_from_slice(&(MAX_WAL_ENTRIES as u64).to_be_bytes());
    buf.extend_from_slice(&MAX_WRITE_BYTES.to_be_bytes());
    buf.extend_from_slice(&MAX_READ_BYTES.to_be_bytes());
    buf.extend_from_slice(&MAX_TRUNCATE_BYTES.to_be_bytes());
    buf.extend_from_slice(&(MAX_ENTRIES as u64).to_be_bytes());
    buf.extend_from_slice(&(MAX_OPEN_FDS as u64).to_be_bytes());
    buf.extend_from_slice(&(MAX_RANGES_PER_FILE as u64).to_be_bytes());
    buf.extend_from_slice(&(MAX_WAITERS_PER_FILE as u64).to_be_bytes());
    buf.extend_from_slice(&LOCK_ID_CEILING.to_be_bytes());
    buf.push(SNAPSHOT_FORMAT_VERSION);
    buf.extend_from_slice(&MAX_CHUNK_ITEMS.to_be_bytes());
    let hash = Blake2b256::hash(buf);
    let mut hex = String::with_capacity(FINGERPRINT_HEX_LEN);
    for b in hash.iter().take(FINGERPRINT_HEX_LEN / 2) {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// Append the consensus fingerprint to the operator's `network_id`.
/// A no-op if the network_id already carries a `#cf` suffix
/// (idempotent — safe to call twice).
///
/// Returns the augmented network_id: `<network_id>#cf<hex>`.
pub fn augment_network_id(network_id: &str) -> String {
    if network_id.contains(FINGERPRINT_DELIMITER) {
        // Already augmented (or the operator manually provided a
        // fingerprint suffix — respect it).
        return network_id.to_string();
    }
    format!(
        "{network_id}{FINGERPRINT_DELIMITER}{fp}",
        fp = consensus_runtime_fingerprint()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Determinism: two calls in the same process produce the same
    /// fingerprint.  If not, MAX_WAL_ENTRIES or the hash function
    /// is being read non-deterministically.
    #[test]
    fn fingerprint_is_deterministic_across_calls() {
        let a = consensus_runtime_fingerprint();
        let b = consensus_runtime_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a.len(), FINGERPRINT_HEX_LEN);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "must be lowercase hex; got {a}"
        );
    }

    /// Golden-hex pin: current consensus-observable constants yield
    /// a specific fingerprint.  Post B2 expansion (2026-09-03) this
    /// covers 10 constants (see `consensus_runtime_fingerprint`
    /// docstring); if ANY of them changes, this test fires — forcing
    /// the maintainer to acknowledge the change breaks peering with
    /// un-upgraded peers.
    ///
    /// Prior anchors (each roll = intentional shard-wide constant
    /// change requiring coordinated peer upgrade):
    ///  - `26681741869115a2` (pre-B2, when fingerprint only folded
    ///    MAX_WAL_ENTRIES).
    #[test]
    fn fingerprint_pinned_for_current_consensus_constants() {
        let fp = consensus_runtime_fingerprint();
        assert_eq!(
            fp.len(),
            FINGERPRINT_HEX_LEN,
            "fingerprint length locked at {FINGERPRINT_HEX_LEN} chars"
        );
        // Pinned value: regenerate deliberately when ANY of the 11
        // consensus constants changes.  Coordinated peer-upgrade
        // required for each roll.
        // Regenerate: cargo test -p rholang --lib -- \
        //   fingerprint_pinned_for_current_consensus_constants --nocapture
        //
        // Prior anchor: 2315df6c0d5b6687 (pre-M-19-Gap-2, 2026-09-04)
        //   — 10 constants (MAX_WAL_ENTRIES through
        //   SNAPSHOT_FORMAT_VERSION).
        // Current anchor: MAX_CHUNK_ITEMS appended at position 11.
        const EXPECTED_FOR_CURRENT: &str = "0982cf37fab162be";
        assert_eq!(
            fp, EXPECTED_FOR_CURRENT,
            "M-8/B2 fingerprint changed — did MAX_WAL_ENTRIES, MAX_WRITE_BYTES, \
             MAX_READ_BYTES, MAX_TRUNCATE_BYTES, MAX_ENTRIES, MAX_OPEN_FDS, \
             MAX_RANGES_PER_FILE, MAX_WAITERS_PER_FILE, LOCK_ID_CEILING, \
             SNAPSHOT_FORMAT_VERSION, or MAX_CHUNK_ITEMS change?  If yes, that \
             is a coordinated peer-upgrade event.  Update this constant + \
             re-verify every peer in the fleet is rebuilt."
        );
        println!("consensus_runtime_fingerprint = {fp}");
    }

    #[test]
    fn augment_network_id_appends_fingerprint() {
        let augmented = augment_network_id("mainnet");
        assert!(augmented.starts_with("mainnet#cf"));
        assert_eq!(
            augmented.len(),
            "mainnet".len() + FINGERPRINT_DELIMITER.len() + FINGERPRINT_HEX_LEN
        );
    }

    #[test]
    fn augment_network_id_is_idempotent() {
        let once = augment_network_id("testnet");
        let twice = augment_network_id(&once);
        assert_eq!(once, twice, "augmentation must be idempotent");
    }

    #[test]
    fn augment_network_id_preserves_operator_prefix() {
        let augmented = augment_network_id("my-corporate-shard");
        assert!(augmented.starts_with("my-corporate-shard#cf"));
    }
}
