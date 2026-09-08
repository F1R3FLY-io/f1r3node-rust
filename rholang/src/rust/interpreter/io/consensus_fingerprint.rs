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
// consensus-observable runtime constant listed in §5.  Post M-35
// (2026-09-08): coverage is 11 constants, each registered from its
// declaration site via `register_consensus_constant!` — `linkme`
// collects the entries into `CONSENSUS_FOLD` at link time and
// `consensus_runtime_fingerprint()` sorts by declared `order` and
// encodes.  New constants MUST claim the next-highest unused order
// — the runtime contiguity check panics on gaps or duplicates.
// The fingerprint is
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
use linkme::distributed_slice;

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

/// M-35 review fix (S1/C1, 2026-09-08): expected count of registered
/// `ConsensusFoldEntry` records.  Guards against silent
/// `linkme::distributed_slice` truncation under cdylib / LTO /
/// release builds — the fingerprint function's contiguity check
/// (a `for` loop over `entries`) is vacuously true on an empty
/// slice, so without this guard a downstream consumer that links
/// `rholang` as a cdylib might see zero entries and compute the
/// hash of an empty buffer as its "fingerprint" — silently
/// producing a wrong-but-well-formed fingerprint that force-splits
/// peering.  With this guard, that failure mode panics loudly at
/// boot.
///
/// When adding a consensus constant: register with the next-highest
/// `order` AND bump this count.  Both must move together; the
/// golden-hex pin (in `tests` below) already forces a coordinated
/// change of the encoded fingerprint too.
const EXPECTED_ENTRY_COUNT: usize = 11;

/// M-35 (2026-09-08, A4-S-3): a single consensus-observable
/// constant's contribution to the fingerprint fold.
///
/// Every constant enumerated in `docs/consensus-invariants.md § 5`
/// (byte gates) plus `LOCK_ID_CEILING` and `SNAPSHOT_FORMAT_VERSION`
/// registers ONE `ConsensusFoldEntry` into the `CONSENSUS_FOLD`
/// distributed slice at its declaration site.  `consensus_runtime_
/// fingerprint()` collects the slice, sorts by `order`, and appends
/// each entry's encoded bytes into the pre-hash buffer.
///
/// The `order` field is a hard-fork surface — reordering flips the
/// fingerprint of an unchanged fleet and force-splits peering.
/// New consensus-observable constants MUST be appended at the tail
/// (order = next-highest); the `order_contiguity` compile-time-ish
/// check in `consensus_runtime_fingerprint` fires at runtime on a
/// gap or duplicate.
///
/// The `encode` field is a plain `fn(&mut Vec<u8>)` (not a
/// closure) — non-capturing so it coerces from a lambda at the
/// declaration site.  The two encoding shapes in use today:
///
///   `|buf| buf.extend_from_slice(&(CONST as u64).to_be_bytes())`
///     for u64 / usize constants — 8 BE bytes.
///   `|buf| buf.push(CONST)`
///     for the u8 `SNAPSHOT_FORMAT_VERSION` — 1 byte.
///
/// A new encoding shape (e.g., u32 BE for a future `Version`
/// constant) would require adding a helper here — the current
/// two-shape design is what the fingerprint has ever encoded.
#[derive(Debug, Clone, Copy)]
pub struct ConsensusFoldEntry {
    /// Position in the fold sequence.  Must be contiguous 1..=N
    /// across all registered entries.
    pub order: u32,
    /// Human-readable constant name (for error messages when the
    /// contiguity check fails).
    pub name: &'static str,
    /// Encode this constant's value into the fold buffer.  Called
    /// once per `consensus_runtime_fingerprint()` invocation.
    pub encode: fn(&mut Vec<u8>),
}

/// Distributed slice of every consensus-observable constant's fold
/// entry.  Contributors register from anywhere in the `rholang`
/// crate via the `register_consensus_constant!` macro.  The `linkme`
/// crate collects them into a linker section and exposes the
/// resulting slice at compile time — no manual list to maintain,
/// no "did you forget to append?" hazard.
#[distributed_slice]
pub static CONSENSUS_FOLD: [ConsensusFoldEntry];

/// Compute the hex fingerprint of all consensus-observable
/// runtime constants.  Coverage: every constant registered via
/// `register_consensus_constant!` (see `docs/consensus-invariants.md
/// § 5` for the operator-visible catalog).
///
/// M-35 (2026-09-08, A4-S-3): fold order is derived from the
/// `CONSENSUS_FOLD` distributed slice at runtime — sorted by
/// declared `order` field, then encoded in sequence.  Adding a
/// new constant is a single-file change: declare with
/// `register_consensus_constant!(order = N, ...)` and pick the
/// next-highest N.  Contiguity is checked on every call; a gap
/// or duplicate panics with the offending names.
///
/// Returns 16-char lowercase hex (first 8 bytes of Blake2b256).
pub fn consensus_runtime_fingerprint() -> String {
    let mut entries: Vec<&ConsensusFoldEntry> = CONSENSUS_FOLD.iter().collect();
    entries.sort_by_key(|e| e.order);

    // M-35 review fix (S1, 2026-09-08): guard against silent
    // linkme truncation.  An empty `entries` passes the
    // contiguity loop vacuously and would silently produce
    // BLAKE2b(""), which is a plausible-looking but wrong
    // fingerprint.  Panic loud at boot instead.
    assert_eq!(
        entries.len(),
        EXPECTED_ENTRY_COUNT,
        "M-35: CONSENSUS_FOLD has {} entries but expected {}.  \
         Either (a) `linkme::distributed_slice` truncation under \
         cdylib / LTO / release linkage (production bug — expect \
         shard split), or (b) a `register_consensus_constant!` \
         invocation was added/removed without updating \
         EXPECTED_ENTRY_COUNT (developer error — fix the count).",
        entries.len(),
        EXPECTED_ENTRY_COUNT
    );

    // Contiguity check: orders must be exactly 1..=entries.len().
    // A gap or duplicate is a shard-splitting error that shows up
    // as a wrong fingerprint (peers refuse to peer) — but at that
    // point the diagnostic is far from the cause.  Catch it here
    // where the constant list is defined.
    for (i, e) in entries.iter().enumerate() {
        let expected = (i as u32) + 1;
        assert_eq!(
            e.order, expected,
            "M-35: CONSENSUS_FOLD orders must be contiguous 1..=N; \
             expected order {} but entry `{}` has order {}.  Fix: \
             assign the next-highest available order to the new \
             `register_consensus_constant!` invocation, or find \
             the duplicate.",
            expected, e.name, e.order
        );
    }

    let mut buf = Vec::with_capacity(8 * entries.len() + 1);
    for entry in &entries {
        (entry.encode)(&mut buf);
    }
    let hash = Blake2b256::hash(buf);
    let mut hex = String::with_capacity(FINGERPRINT_HEX_LEN);
    for b in hash.iter().take(FINGERPRINT_HEX_LEN / 2) {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// M-35 (2026-09-08, A4-S-3): register a consensus-observable
/// constant into `CONSENSUS_FOLD`.  Declare AT the site where the
/// constant lives (same module) — the macro emits a
/// `#[distributed_slice(CONSENSUS_FOLD)] static` alongside the
/// existing `pub const`.
///
/// Two encoding shapes:
///   - `u64_be` — for `u64` / `usize` constants (8 BE bytes).  The
///     `as u64` cast is emitted by the macro so portability across
///     32/64-bit builds is automatic.
///   - `u8_raw` — for the lone `u8` `SNAPSHOT_FORMAT_VERSION`
///     (single byte push).
///
/// Example:
/// ```ignore
/// use crate::rust::interpreter::io::consensus_fingerprint::{
///     ConsensusFoldEntry, CONSENSUS_FOLD,
/// };
/// pub const MAX_WAL_ENTRIES: usize = 100_000;
/// register_consensus_constant!(order = 1, name = MAX_WAL_ENTRIES, u64_be);
/// ```
#[macro_export]
macro_rules! register_consensus_constant {
    (order = $order:literal, name = $const_name:ident, u64_be) => {
        paste::paste! {
            #[linkme::distributed_slice($crate::rust::interpreter::io::consensus_fingerprint::CONSENSUS_FOLD)]
            #[allow(non_upper_case_globals)]
            static [<CONSENSUS_FOLD_ $const_name>]:
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry =
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry {
                    order: $order,
                    name: stringify!($const_name),
                    encode: |buf: &mut Vec<u8>| {
                        buf.extend_from_slice(&($const_name as u64).to_be_bytes());
                    },
                };
        }
    };
    (order = $order:literal, name = $const_name:ident, u8_raw) => {
        paste::paste! {
            #[linkme::distributed_slice($crate::rust::interpreter::io::consensus_fingerprint::CONSENSUS_FOLD)]
            #[allow(non_upper_case_globals)]
            static [<CONSENSUS_FOLD_ $const_name>]:
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry =
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry {
                    order: $order,
                    name: stringify!($const_name),
                    encode: |buf: &mut Vec<u8>| {
                        buf.push($const_name);
                    },
                };
        }
    };
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
    /// a specific fingerprint.  Post M-35 (2026-09-08) coverage is
    /// 11 constants collected via `linkme` from their declaration
    /// sites (see `register_consensus_constant!` invocations across
    /// `wal.rs`, `handlers.rs`, `mod.rs`, `lock.rs`, `snapshot.rs`).
    /// If ANY of them changes, this test fires — forcing the
    /// maintainer to acknowledge the change breaks peering with
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

    /// M-35 review fix (C1, 2026-09-08): explicit assertion that
    /// `CONSENSUS_FOLD` has exactly `EXPECTED_ENTRY_COUNT` entries.
    /// Redundant with the golden-hex pin (which would also fire on
    /// a count mismatch, via a wrong hash), but makes the invariant
    /// explicit and gives a more actionable panic message when it
    /// fails.
    #[test]
    fn consensus_fold_slice_has_expected_entry_count() {
        assert_eq!(
            CONSENSUS_FOLD.len(),
            EXPECTED_ENTRY_COUNT,
            "CONSENSUS_FOLD entry count changed — either a new \
             `register_consensus_constant!` was added (bump \
             EXPECTED_ENTRY_COUNT + regenerate golden hex), one was \
             removed (same), or linkme is not populating the slice \
             (production hazard — investigate before shipping)."
        );
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
