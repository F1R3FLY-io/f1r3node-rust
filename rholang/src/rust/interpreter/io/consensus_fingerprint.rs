// M-8 fix middle option (2026-08-06): consensus runtime fingerprint
// used to augment the operator's `network_id` at boot.
//
// # Threat model
//
// `MAX_WAL_ENTRIES` (`wal.rs`) is consensus-observable — a validator
// running a different value would emit `FSERR_QUOTA_EXCEEDED` on
// different inputs than its peers and silently fork the tuplespace.
// The compile-time floor check + hard-fork catalog item #11 catch
// accidental *lowering* of the constant, but a targeted binary patch
// or a fork of the source (with a different constant) would still
// build cleanly and start up.
//
// # Middle-option fix
//
// At boot, node computes a short hex `fingerprint` from every
// consensus-observable runtime constant (currently just
// `MAX_WAL_ENTRIES`; new constants can be folded in without a
// wire-format change).  The fingerprint is appended to the
// operator's `network_id` as `<network_id>#cf<hex>` before the
// value is baked into the TLS interceptor.  Peers with different
// fingerprints see a mismatched `network_id` and get refused by
// the existing `SslSessionServerInterceptor::validate_network_id`
// path — the exact same code path that already rejects wrong-
// network peers.
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

use super::wal::MAX_WAL_ENTRIES;

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
/// runtime constants.  Currently:
///
/// - `MAX_WAL_ENTRIES` (u64, big-endian)
///
/// New consensus-observable constants MUST be appended to this
/// list (never reordered — that would flip the fingerprint of an
/// unchanged fleet).  Any such addition is a coordinated peer-
/// upgrade event; the network partitions until every peer runs
/// the new binary.
///
/// Returns 16-char lowercase hex (first 8 bytes of Blake2b256).
pub fn consensus_runtime_fingerprint() -> String {
    let mut buf = Vec::with_capacity(8);
    // MAX_WAL_ENTRIES is usize on this platform but must be
    // encoded portably.  Cast to u64 explicitly.
    buf.extend_from_slice(&(MAX_WAL_ENTRIES as u64).to_be_bytes());
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

    /// Golden-hex pin: current MAX_WAL_ENTRIES = 65536 yields a
    /// specific fingerprint.  If MAX_WAL_ENTRIES ever changes,
    /// this test fires — forcing the maintainer to acknowledge
    /// the change breaks peering with un-upgraded peers.
    #[test]
    fn fingerprint_pinned_for_current_max_wal_entries() {
        // MAX_WAL_ENTRIES = 65_536 → u64-be = 00 00 00 00 00 01 00 00
        // Blake2b256 of that is deterministic; take the first 8 bytes hex.
        let fp = consensus_runtime_fingerprint();
        assert_eq!(
            fp.len(),
            FINGERPRINT_HEX_LEN,
            "fingerprint length locked at {FINGERPRINT_HEX_LEN} chars"
        );
        // Pinned value: regenerate deliberately when MAX_WAL_ENTRIES
        // changes.  Coordinated peer-upgrade required.
        // Regenerate: cargo test -p rholang --lib -- \
        //   fingerprint_pinned_for_current_max_wal_entries --nocapture
        const EXPECTED_FOR_65536: &str = "26681741869115a2";
        assert_eq!(
            fp, EXPECTED_FOR_65536,
            "M-8 fingerprint changed — did MAX_WAL_ENTRIES change?  If \
             yes, that is a coordinated peer-upgrade event.  Update this \
             constant + re-verify every peer in the fleet is rebuilt."
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
