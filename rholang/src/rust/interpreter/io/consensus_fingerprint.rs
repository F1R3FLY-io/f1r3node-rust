// Consensus runtime fingerprint — folds every consensus-observable
// runtime constant into a short hex tag appended to the operator's
// `network_id` at boot.
//
// # Threat model
//
// A consensus-observable constant is one whose value materially
// affects deploy outcomes.  A validator running a different value
// than its peers emits `FSERR_QUOTA_EXCEEDED` (or similar) on
// different inputs than its peers and silently forks the
// tuplespace.  Compile-time floor checks + a hard-fork catalog
// catch accidental *lowering* of these constants, but a targeted
// binary patch or a source fork with a different value would still
// build cleanly and start up.
//
// # Mechanism
//
// At boot, node computes a short hex `fingerprint` from every
// consensus-observable constant registered via
// `register_consensus_constant!`.  `linkme` collects the registered
// entries into `CONSENSUS_FOLD` at link time and
// `consensus_runtime_fingerprint()` sorts by declared `order` and
// encodes.  New constants MUST claim the next-highest unused
// `order` — the runtime contiguity check panics on gaps or
// duplicates.  The fingerprint is appended to the operator's
// `network_id` as `<network_id>#cf<hex>` before that value is
// baked into the TLS interceptor.  Peers with different
// fingerprints see a mismatched `network_id` and get refused by
// the existing peering handshake — the same code path that already
// rejects wrong-network peers.
//
// # Trade-offs
//
// - **No protobuf change.**  The `Header.networkId` field is still
//   a `String`; the fingerprint travels inside it as an opaque
//   suffix.  Zero on-wire schema modification.
// - **No new handshake round-trip.**  The check piggy-backs on the
//   first message.
// - **Coordinated upgrade required.**  Once deployed, this node
//   won't peer with un-upgraded peers (their `network_id` lacks
//   the `#cf<hex>` suffix).  Same upgrade profile as any other
//   consensus-critical change.
// - **Weaker than a Genesis parameter.**  The fingerprint isn't
//   committed to on-chain state, so a shard-post-hoc audit can't
//   determine which caps the Genesis block was formed with.  For
//   per-node fleet-drift protection this is acceptable; for
//   hard-cap-was-what state provenance, use a Genesis-parameter
//   design instead.
//
// # Wave 2 initial state
//
// This PR lands the framework alone: `CONSENSUS_FOLD` is empty and
// `EXPECTED_ENTRY_COUNT = 0`.  Every consensus-observable constant
// registered in later Wave 2 PRs (from `wal.rs`, `snapshot.rs`,
// `handlers.rs`, `lock.rs`, `mod.rs`, etc.) must (a) call
// `register_consensus_constant!` at its declaration site AND (b)
// bump `EXPECTED_ENTRY_COUNT` in the same PR, so the count-guard
// stays load-bearing.

use crypto::rust::hash::blake2b256::Blake2b256;
use linkme::distributed_slice;

/// Delimiter separating the operator's `network_id` from the
/// consensus fingerprint.  `#` chosen because it's URL-safe, not
/// in the alphanumeric identifier set operators typically use for
/// network names, and unambiguous in log lines.
///
/// Length of the fingerprint is 16 hex chars (8 bytes) — enough
/// entropy to make accidental collisions astronomically unlikely
/// while keeping the augmented `network_id` short enough to log.
const FINGERPRINT_DELIMITER: &str = "#cf";
const FINGERPRINT_HEX_LEN: usize = 16; // 8 bytes × 2

// Compile-time pin on the delimiter shape.  A future refactor
// renaming the delimiter to `#fp` or changing the hex length would
// pass all runtime tests silently while peers on the network
// rejected the mismatched-shape peering handshake.  Const asserts
// fire at compile time, so a drift is caught before CI.
const _: () = assert!(
    FINGERPRINT_DELIMITER.len() == 3,
    "FINGERPRINT_DELIMITER shape drifted; peers with a mismatched \
     delimiter length reject the peering handshake."
);
const _: () = assert!(
    FINGERPRINT_HEX_LEN == 16,
    "FINGERPRINT_HEX_LEN drifted from 16 (8 bytes × 2); peers \
     expecting the prior length reject the handshake."
);

/// Expected count of registered `ConsensusFoldEntry` records.
/// Guards against silent `linkme::distributed_slice` truncation
/// under cdylib / LTO / release builds — the fingerprint
/// function's contiguity check (a `for` loop over `entries`) is
/// vacuously true on an empty slice, so without this guard a
/// downstream consumer that links `rholang` as a cdylib might see
/// zero entries and compute the hash of an empty buffer as its
/// "fingerprint" — silently producing a wrong-but-well-formed
/// fingerprint that force-splits peering.  With this guard, that
/// failure mode panics loudly at boot.
///
/// When adding a consensus constant: register with the next-
/// highest `order` AND bump this count.  Both must move together;
/// the golden-hex pin in `tests` below also forces a coordinated
/// change of the encoded fingerprint.
///
/// Currently 15 — additions so far:
///   PR 2.3 (`wal` types):
///     order 1  — `MAX_WAL_ENTRIES`          (u64_be)
///     order 2  — `WAL_OUTCOME_VARIANTS`     (u64_be)
///     order 3  — `WAL_OP_VARIANTS`          (u64_be)
///   PR 2.4 (`mod` constants):
///     order 4  — `MAX_READ_BYTES`           (u64_be)
///     order 5  — `MAX_TRUNCATE_BYTES`       (u64_be)
///     order 6  — `MAX_OPEN_FDS`             (u64_be)
///     order 7  — `MAX_CHUNK_ITEMS`          (u64_be)
///     order 8  — `CMODE_ORACULAR_STR`       (str_bytes)
///     order 9  — `CMODE_CONSENSUS_STR`      (str_bytes)
///     order 10 — `FS_NONCE`                 (i64_be)
///   PR 2.5 (`lock` types):
///     order 11 — `MAX_RANGES_PER_FILE`      (u64_be)
///     order 12 — `MAX_WAITERS_PER_FILE`     (u64_be)
///     order 13 — `LOCK_ID_CEILING`          (u64_be)
///   PR 2.16 (`handle_table` soft-checkpoint):
///     order 14 — `FD_ENTROPY_HEADROOM_BITS` (u64_be)
///   PR 2.17 (`snapshot` encoder — first `u8_raw`):
///     order 15 — `SNAPSHOT_FORMAT_VERSION`  (u8_raw)
const EXPECTED_ENTRY_COUNT: usize = 17;

/// A single consensus-observable constant's contribution to the
/// fingerprint fold.
///
/// Every consensus-observable constant registers ONE
/// `ConsensusFoldEntry` into the `CONSENSUS_FOLD` distributed
/// slice at its declaration site.  `consensus_runtime_fingerprint`
/// collects the slice, sorts by `order`, and appends each entry's
/// encoded bytes into the pre-hash buffer.
///
/// The `order` field is a hard-fork surface — reordering flips
/// the fingerprint of an unchanged fleet and force-splits peering.
/// New consensus-observable constants MUST be appended at the tail
/// (order = next-highest); the `order_contiguity` check in
/// `consensus_runtime_fingerprint` fires at runtime on a gap or
/// duplicate.
///
/// The `encode` field is a plain `fn(&mut Vec<u8>)` (not a
/// closure) — non-capturing, so it coerces from a lambda at the
/// declaration site.  The encoding shapes supported by
/// `register_consensus_constant!`:
///
///   `u64_be`   — for `u64` / `usize` constants (8 BE bytes).
///   `u8_raw`   — for `u8` constants (single byte push).
///   `str_bytes` — for `&'static str` constants (u32-BE length
///                 prefix + UTF-8 bytes; a rename that preserves
///                 length still produces a distinct fingerprint).
///   `i64_be`   — for signed 64-bit constants (two's-complement
///                BE bytes; distinguishable from `u64_be` for
///                values above `i64::MAX`).
#[derive(Debug, Clone, Copy)]
pub struct ConsensusFoldEntry {
    /// Position in the fold sequence.  Must be contiguous 1..=N
    /// across all registered entries.
    pub order: u32,
    /// Human-readable constant name (for error messages when the
    /// contiguity check fails).
    pub name: &'static str,
    /// Encode this constant's value into the fold buffer.  Called
    /// once per `consensus_runtime_fingerprint` invocation.
    pub encode: fn(&mut Vec<u8>),
}

/// Distributed slice of every consensus-observable constant's fold
/// entry.  Contributors register from anywhere in the `rholang`
/// crate via `register_consensus_constant!`.  The `linkme` crate
/// collects them into a linker section and exposes the resulting
/// slice at compile time — no manual list to maintain, no "did
/// you forget to append?" hazard.
#[distributed_slice]
pub static CONSENSUS_FOLD: [ConsensusFoldEntry];

/// Compute the hex fingerprint of all consensus-observable runtime
/// constants.  Coverage: every constant registered via
/// `register_consensus_constant!`.
///
/// Fold order is derived from the `CONSENSUS_FOLD` distributed
/// slice at runtime — sorted by declared `order` field, then
/// encoded in sequence.  Adding a new constant is a single-file
/// change: declare with `register_consensus_constant!(order = N,
/// ...)` and pick the next-highest N.  Contiguity is checked on
/// every call; a gap or duplicate panics with the offending names.
///
/// Returns 16-char lowercase hex (first 8 bytes of Blake2b256).
pub fn consensus_runtime_fingerprint() -> String {
    let mut entries: Vec<&ConsensusFoldEntry> = CONSENSUS_FOLD.iter().collect();
    entries.sort_by_key(|e| e.order);

    // Count-guard against silent linkme truncation OR an accidental
    // extra registration.  With `EXPECTED_ENTRY_COUNT = 0` this
    // fires on any accidental non-zero registration; once Wave 2
    // PRs start bumping the count, it fires on truncation too.
    assert_eq!(
        entries.len(),
        EXPECTED_ENTRY_COUNT,
        "CONSENSUS_FOLD has {} entries but expected {}.  Either \
         (a) `linkme::distributed_slice` truncation under cdylib / \
         LTO / release linkage (production bug — expect shard \
         split), or (b) a `register_consensus_constant!` invocation \
         was added/removed without updating EXPECTED_ENTRY_COUNT \
         (developer error — fix the count).",
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
            "CONSENSUS_FOLD orders must be contiguous 1..=N; \
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

/// Register a consensus-observable constant into `CONSENSUS_FOLD`.
/// Declare AT the site where the constant lives (same module) —
/// the macro emits a `#[distributed_slice(CONSENSUS_FOLD)] static`
/// alongside the existing `pub const`.
///
/// Encoding shapes:
///   - `u64_be`    — for `u64` / `usize` constants (8 BE bytes).
///     The `as u64` cast is emitted by the macro so portability
///     across 32/64-bit builds is automatic.
///   - `u8_raw`    — for `u8` constants (single byte push).
///   - `str_bytes` — for `&'static str` constants (u32-BE length
///     prefix + UTF-8 bytes).
///   - `i64_be`    — for signed 64-bit constants (two's-complement
///     BE bytes).
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
    (order = $order:literal, name = $const_name:ident, str_bytes) => {
        paste::paste! {
            #[linkme::distributed_slice($crate::rust::interpreter::io::consensus_fingerprint::CONSENSUS_FOLD)]
            #[allow(non_upper_case_globals)]
            static [<CONSENSUS_FOLD_ $const_name>]:
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry =
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry {
                    order: $order,
                    name: stringify!($const_name),
                    encode: |buf: &mut Vec<u8>| {
                        let bytes = $const_name.as_bytes();
                        buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                        buf.extend_from_slice(bytes);
                    },
                };
        }
    };
    (order = $order:literal, name = $const_name:ident, i64_be) => {
        paste::paste! {
            #[linkme::distributed_slice($crate::rust::interpreter::io::consensus_fingerprint::CONSENSUS_FOLD)]
            #[allow(non_upper_case_globals)]
            static [<CONSENSUS_FOLD_ $const_name>]:
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry =
                $crate::rust::interpreter::io::consensus_fingerprint::ConsensusFoldEntry {
                    order: $order,
                    name: stringify!($const_name),
                    encode: |buf: &mut Vec<u8>| {
                        buf.extend_from_slice(&($const_name as i64).to_be_bytes());
                    },
                };
        }
    };
}

/// Append the consensus fingerprint to the operator's `network_id`.
/// A no-op if the `network_id` already carries a `#cf` suffix
/// (idempotent — safe to call twice).
///
/// Returns the augmented `network_id`: `<network_id>#cf<hex>`.
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

    /// Runtime pin on the exact delimiter literal.  The compile-
    /// time `const _: () = assert!(...)` checks length; this test
    /// asserts the exact bytes.  A rename from `#cf` to (say) `#fp`
    /// would preserve length but split peering — this pin catches
    /// that.
    #[test]
    fn fingerprint_delimiter_pinned_to_hash_cf() {
        assert_eq!(
            FINGERPRINT_DELIMITER, "#cf",
            "FINGERPRINT_DELIMITER drifted from `#cf`.  Peers on \
             the network use the delimiter to split the augmented \
             `network_id`; a mismatched literal fails the peering \
             handshake with a wrong-network message."
        );
    }

    /// Determinism: two calls in the same process produce the same
    /// fingerprint.  If not, the fold or the hash function is
    /// being read non-deterministically.
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

    /// Golden-hex pin: current consensus-observable constant set
    /// yields a specific fingerprint.  Wave 2 initial state is 0
    /// registered constants, so the fold hashes an empty buffer.
    ///
    /// If this test fires, either (a) a Wave 2 PR added a new
    /// `register_consensus_constant!` invocation and forgot to
    /// bump the pin, or (b) an existing constant's value changed.
    /// Both are coordinated shard-wide events; regenerate the pin
    /// only when the change is intentional.
    #[test]
    fn fingerprint_pinned_for_current_consensus_constants() {
        let fp = consensus_runtime_fingerprint();
        assert_eq!(
            fp.len(),
            FINGERPRINT_HEX_LEN,
            "fingerprint length locked at {FINGERPRINT_HEX_LEN} chars"
        );
        // Regenerate via
        //   cargo test -p rholang --lib -- \
        //     fingerprint_pinned_for_current_consensus_constants --nocapture
        // ONLY when intentionally rolling the consensus surface.
        //
        // Initial anchor (Wave 2 PR 2.2, empty fold): first 8 bytes
        // of Blake2b256(empty) — `0e5751c026e543b2`.
        // Wave 2 PR 2.3 (wal types) → `a98e6ea54e4efb84`.
        // Wave 2 PR 2.4 (mod constants) → `b2024bf3be489a44`.
        //   Adds orders 4-10 to the fold:
        //     4  MAX_READ_BYTES         = 64 * 1024 * 1024   (u64_be)
        //     5  MAX_TRUNCATE_BYTES     = 16 * 1024^3        (u64_be)
        //     6  MAX_OPEN_FDS           = 1024               (u64_be)
        //     7  MAX_CHUNK_ITEMS        = 65_536             (u64_be)
        //     8  CMODE_ORACULAR_STR     = "oracular"         (str_bytes)
        //     9  CMODE_CONSENSUS_STR    = "consensus"        (str_bytes)
        //    10  FS_NONCE               = i64::MAX           (i64_be)
        // Wave 2 PR 2.5 (lock types) → `b49df15c4b948c53`.
        //   Adds orders 11-13 to the fold:
        //    11  MAX_RANGES_PER_FILE    = 1024               (u64_be)
        //    12  MAX_WAITERS_PER_FILE   = 1024               (u64_be)
        //    13  LOCK_ID_CEILING        = i64::MAX - 2^16    (u64_be)
        // (LOCK_ID_CEILING was tightened from `u64::MAX - 2^16` to
        //  `i64::MAX - 2^16` per the PR 2.5 review — closes the
        //  Rholang-i64 wire-truncation footgun by aligning the
        //  ceiling with `LockId::try_from(u64)`'s wire-safety
        //  check.  Rolls this pin one extra time within PR 2.5.)
        // Wave 2 PR 2.16 (`handle_table` soft-checkpoint) →
        // `b271a6804c526e1c`.  Adds order 14:
        //    14  FD_ENTROPY_HEADROOM_BITS = 20               (u64_be)
        // (Consensus-observable per the docstring: fd values live
        //  in Rholang tuplespace state, so the state-hash → fd-
        //  watermark derivation is an implicit consensus
        //  commitment.  Registering closes the partial-upgrade
        //  silent-fork hazard by advertising the constant to the
        //  peering-handshake `network_id` check.)
        // Wave 2 PR 2.17 (`snapshot` encoder — first `u8_raw`
        // fingerprint entry) → `5491728f5f89dc4f`.  Adds order 15:
        //    15  SNAPSHOT_FORMAT_VERSION  = 6                (u8_raw)
        // (Consensus-observable: the encoded WAL slice's
        //  Blake2b256 root is the on-chain commitment consumed by
        //  `WalSnapshotWrite` — a validator running a different
        //  version byte produces different root bytes for
        //  identical WAL contents and silently forks.)
        // Wave 4 PR 4.31 (fs_entries migration) → `b77e82a7474172bb`.
        // Adds order 16:
        //    16  MAX_ENTRIES              = 65_536           (u64_be)
        // (Consensus-observable: divergent caps fork at the
        //  `FSERR_QUOTA_EXCEEDED` boundary — a validator with a
        //  lower cap rejects a 50_000-entry directory call that
        //  another validator accepts, producing different Rholang
        //  reply bytes for identical inputs.)
        // Wave 4 PR 4.38 (fs_write migration) → `7133cf8a9b60f538`.
        // Adds order 17:
        //    17  MAX_WRITE_BYTES          = 64 * 1024 * 1024 (u64_be)
        // (Consensus-observable: divergent caps fork at the
        //  `FSERR_QUOTA_EXCEEDED` boundary — a validator with a
        //  lower cap rejects a 32-MiB write that another accepts.)
        const EXPECTED_FOR_CURRENT: &str = "dd40a754542ccc54";
        assert_eq!(
            fp, EXPECTED_FOR_CURRENT,
            "fingerprint changed — a `register_consensus_constant!` \
             invocation was added/removed or an existing constant's \
             value changed.  That is a coordinated peer-upgrade \
             event.  Update this pin and confirm every peer in the \
             fleet is rebuilt."
        );
        println!("consensus_runtime_fingerprint = {fp}");
    }

    /// Explicit assertion that `CONSENSUS_FOLD` has exactly
    /// `EXPECTED_ENTRY_COUNT` entries.  Redundant with the golden-
    /// hex pin (a count mismatch would also flip the hash), but
    /// makes the invariant explicit and gives a more actionable
    /// panic message when it fails.
    #[test]
    fn consensus_fold_slice_has_expected_entry_count() {
        assert_eq!(
            CONSENSUS_FOLD.len(),
            EXPECTED_ENTRY_COUNT,
            "CONSENSUS_FOLD entry count changed — either a new \
             `register_consensus_constant!` was added (bump \
             EXPECTED_ENTRY_COUNT + regenerate golden hex), one \
             was removed (same), or linkme is not populating the \
             slice (production hazard — investigate before shipping)."
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

    /// Empty `network_id`: the augmenter still appends the
    /// `#cf<hex>` suffix.  Peers would see `#cf<hex>` as the full
    /// `network_id` — unusual but not broken.
    #[test]
    fn augment_network_id_on_empty_string_returns_bare_suffix() {
        let augmented = augment_network_id("");
        assert_eq!(
            augmented.len(),
            FINGERPRINT_DELIMITER.len() + FINGERPRINT_HEX_LEN
        );
        assert!(augmented.starts_with("#cf"));
    }

    /// If the operator provides a `network_id` that already
    /// contains more than one `#cf` occurrence (unlikely but
    /// syntactically possible), the idempotence check treats the
    /// input as "already augmented" and returns it unchanged.
    /// Pinning this so a future refactor doesn't silently start
    /// appending extra suffixes.
    #[test]
    fn augment_network_id_preserves_multiple_pre_existing_suffixes() {
        let weird = "shard#cfdeadbeefdeadbeef#cfaaaaaaaaaaaaaaaa";
        let augmented = augment_network_id(weird);
        assert_eq!(augmented, weird);
    }

    /// Byte-level pin on each `encode` shape.  The golden-hex pin
    /// catches encoding regressions at the whole-fold level, but
    /// this test localizes the failure to a specific shape so a
    /// regression in, say, `str_bytes` doesn't just show up as
    /// "golden hex flipped" during a later PR that also adds a
    /// new constant.
    #[test]
    fn encoding_shapes_produce_expected_bytes() {
        // u64_be — 8 BE bytes.
        const U64_CONST: u64 = 1;
        let entry_u64 = ConsensusFoldEntry {
            order: 1,
            name: "U64_CONST",
            encode: |buf| buf.extend_from_slice(&U64_CONST.to_be_bytes()),
        };
        let mut buf = Vec::new();
        (entry_u64.encode)(&mut buf);
        assert_eq!(buf, vec![0, 0, 0, 0, 0, 0, 0, 1]);

        // u8_raw — single byte push.
        const U8_CONST: u8 = 0x42;
        let entry_u8 = ConsensusFoldEntry {
            order: 1,
            name: "U8_CONST",
            encode: |buf| buf.push(U8_CONST),
        };
        let mut buf = Vec::new();
        (entry_u8.encode)(&mut buf);
        assert_eq!(buf, vec![0x42]);

        // str_bytes — u32-BE length prefix + UTF-8 bytes.
        const STR_CONST: &str = "ab";
        let entry_str = ConsensusFoldEntry {
            order: 1,
            name: "STR_CONST",
            encode: |buf| {
                let bytes = STR_CONST.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            },
        };
        let mut buf = Vec::new();
        (entry_str.encode)(&mut buf);
        assert_eq!(buf, vec![0, 0, 0, 2, b'a', b'b']);

        // i64_be — two's-complement 8 BE bytes; check the sign bit.
        const I64_CONST: i64 = -1;
        let entry_i64 = ConsensusFoldEntry {
            order: 1,
            name: "I64_CONST",
            encode: |buf| buf.extend_from_slice(&I64_CONST.to_be_bytes()),
        };
        let mut buf = Vec::new();
        (entry_i64.encode)(&mut buf);
        assert_eq!(buf, vec![0xFF; 8]);
    }

    /// Source-scan pin: no `register_consensus_constant!`
    /// invocation — nor its target `const` declaration — may sit
    /// inside a platform-`#[cfg(...)]` gate.  A platform-gated
    /// entry (e.g., `#[cfg(target_os = "linux")]
    /// register_consensus_constant!(...)`) would vanish from
    /// `CONSENSUS_FOLD` on the excluded platform, producing a
    /// different fingerprint per OS → shard splits by validator
    /// platform.
    ///
    /// The runtime count guard
    /// (`consensus_fold_slice_has_expected_entry_count`) catches
    /// truncation at boot on the excluded platform.  This test
    /// catches the same class of drift at *test* time on the
    /// platform CI runs on, so a regression doesn't need to wait
    /// for a cross-platform smoke to surface.
    ///
    /// # Detection scope
    ///
    /// The scan walks `io/**/*.rs` recursively (so registrations in
    /// submodule trees like `io/path/`, `io/wal/`, `io/snapshot/`
    /// are covered).  It flags any `#[cfg(...)]`, `#[cfg_attr(...)]`,
    /// or `cfg!(...)` predicate containing `target_os`,
    /// `target_arch`, `target_family`, `target_pointer_width`,
    /// `unix`, or `windows` within K=15 lines *up to and including*
    /// the `register_consensus_constant!` invocation or its
    /// `name = FOO` const declaration.  The inclusive upper bound
    /// catches same-line-attribute shapes like
    /// `#[cfg(unix)] register_consensus_constant!(...)`.
    /// `#[cfg(test)]` and `cfg!(test)` are allowed (they don't
    /// affect release builds).
    ///
    /// # Known blind spot: mod-level gates
    ///
    /// A gate on a `mod` declaration in a parent file
    /// (`#[cfg(unix)] mod handlers;` in `io/mod.rs`) is *not*
    /// detected — the platform predicate lives in a different file
    /// than the registration.  The runtime count guard
    /// (`consensus_fold_slice_has_expected_entry_count`) catches
    /// this on the excluded-platform build, so CI matrices that
    /// exercise both `target_os = "linux"` and `target_os = "macos"`
    /// backstop the source-scan blind spot.
    ///
    /// Wave 2 initial state: 0 registered constants, so the scan
    /// finds 0 sites and passes vacuously.  As soon as any Wave 2
    /// PR registers a constant, the scan starts guarding it.
    #[test]
    fn no_consensus_constant_sits_inside_platform_cfg_gate() {
        const WINDOW: usize = 15;

        fn is_platform_cfg_line(line: &str) -> bool {
            let l = line.trim();
            if l.starts_with("//") || l.starts_with("///") || l.starts_with("*") {
                return false;
            }
            if l.contains("cfg(test)")
                || l.contains("cfg!(test)")
                || l.contains("cfg(all(test")
                || l.contains("cfg_attr(test")
            {
                return false;
            }
            let has_attr = l.contains("#[cfg(") || l.contains("#[cfg_attr(") || l.contains("cfg!(");
            let has_platform_pred = l.contains("target_os")
                || l.contains("target_arch")
                || l.contains("target_family")
                || l.contains("target_pointer_width")
                || l.contains("cfg(unix)")
                || l.contains("cfg(windows)")
                || l.contains("cfg!(unix)")
                || l.contains("cfg!(windows)");
            has_attr && has_platform_pred
        }

        fn extract_const_name(macro_line: &str) -> Option<String> {
            let name_idx = macro_line.find("name = ")?;
            let after = &macro_line[name_idx + "name = ".len()..];
            let end = after
                .find(',')
                .or_else(|| after.find(')'))
                .unwrap_or(after.len());
            let name = after[..end].trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        }

        fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("io/ subtree readable at {}: {e}", dir.display()));
            for entry in entries {
                let entry = entry.expect("readable entry");
                let path = entry.path();
                if path.is_dir() {
                    collect_rs_files(&path, out);
                } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }

        let io_dir = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/rust/interpreter/io"
        ));
        let mut rs_files = Vec::new();
        collect_rs_files(&io_dir, &mut rs_files);
        // Sanity check: recursion must reach at least one file inside
        // a subdirectory (`io/path/*.rs` exists post-Wave 1).  A
        // regression to non-recursive `read_dir` would find only
        // top-level `io/*.rs` and pass a naive count check — this
        // subdir-presence check catches that.
        let has_subdir_file = rs_files.iter().any(|p| {
            p.strip_prefix(&io_dir)
                .map(|rel| rel.components().count() > 1)
                .unwrap_or(false)
        });
        assert!(
            has_subdir_file,
            "recursive scan found {} .rs files under {} but none in \
             a subdirectory — recursion is broken (post-Wave 1 the \
             tree contains `io/path/*.rs`).",
            rs_files.len(),
            io_dir.display(),
        );

        let mut offending: Vec<String> = Vec::new();
        let mut sites_scanned = 0usize;

        for path in &rs_files {
            let content = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("readable .rs file {}: {e}", path.display()));
            let lines: Vec<&str> = content.lines().collect();
            // Display path relative to io/ for readable diagnostics.
            let display_path = path
                .strip_prefix(&io_dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());

            for (idx, line) in lines.iter().enumerate() {
                if !line.contains("register_consensus_constant!") {
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("*")
                    || trimmed.starts_with("macro_rules!")
                {
                    continue;
                }
                // Skip string-literal shapes — the detector self-
                // test includes synthetic `register_consensus_constant!`
                // strings that MUST NOT be treated as real
                // registrations.  A real invocation is always at
                // item level and never starts with a quote.
                if trimmed.starts_with('"')
                    || trimmed.starts_with("r\"")
                    || trimmed.starts_with("r#\"")
                    || trimmed.starts_with("b\"")
                {
                    continue;
                }
                sites_scanned += 1;
                let const_name = extract_const_name(line);

                let start = idx.saturating_sub(WINDOW);
                // Inclusive upper bound so a same-line attribute
                // (`#[cfg(unix)] register_consensus_constant!(...)`)
                // is caught.
                for (li, l) in lines[start..=idx].iter().enumerate() {
                    if is_platform_cfg_line(l) {
                        offending.push(format!(
                            "{}:{}: register_consensus_constant! (name = {:?}) \
                             preceded by platform-cfg at line {}: `{}`",
                            display_path,
                            idx + 1,
                            const_name.as_deref().unwrap_or("?"),
                            start + li + 1,
                            l.trim(),
                        ));
                    }
                }

                if let Some(name) = &const_name {
                    let const_pat_pub = format!("pub const {}:", name);
                    let const_pat_bare = format!("const {}:", name);
                    for (cidx, cline) in lines.iter().enumerate() {
                        if cline.contains(&const_pat_pub) || cline.contains(&const_pat_bare) {
                            let cstart = cidx.saturating_sub(WINDOW);
                            // Inclusive upper bound — same reason as
                            // the register-macro range above.
                            for (li, l) in lines[cstart..=cidx].iter().enumerate() {
                                if is_platform_cfg_line(l) {
                                    offending.push(format!(
                                        "{}:{}: const `{}` declaration preceded \
                                         by platform-cfg at line {}: `{}`",
                                        display_path,
                                        cidx + 1,
                                        name,
                                        cstart + li + 1,
                                        l.trim(),
                                    ));
                                }
                            }
                            let cend = (cidx + 6).min(lines.len());
                            for (li, l) in lines[cidx..cend].iter().enumerate() {
                                if l.contains("cfg!(") && is_platform_cfg_line(l) {
                                    offending.push(format!(
                                        "{}:{}: const `{}` value uses platform \
                                         `cfg!(...)` at line {}: `{}`",
                                        display_path,
                                        cidx + 1,
                                        name,
                                        cidx + li + 1,
                                        l.trim(),
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }

        // Self-check that the file iteration actually scanned every
        // register site.  With `EXPECTED_ENTRY_COUNT = 0` (Wave 2
        // initial state) this comparison is trivially true and
        // clippy flags it — but the check becomes load-bearing as
        // soon as later Wave 2 PRs start bumping the count.
        #[allow(clippy::absurd_extreme_comparisons)]
        {
            assert!(
                sites_scanned >= EXPECTED_ENTRY_COUNT,
                "self-check: expected to scan at least {} \
                 `register_consensus_constant!` invocations \
                 (matching `EXPECTED_ENTRY_COUNT`) but found {}.  \
                 The source-scan pin's file iteration is broken.",
                EXPECTED_ENTRY_COUNT,
                sites_scanned,
            );
        }
        assert!(
            offending.is_empty(),
            "consensus-observable constants MUST NOT be platform-\
             gated (would produce different fingerprints on \
             different validator OSes → shard split by platform).  \
             Offending sites:\n  - {}",
            offending.join("\n  - "),
        );
    }

    /// Companion self-test: verify the platform-cfg detector
    /// actually FIRES on synthetic violations.  A silent false-
    /// negative in `is_platform_cfg_line` would defeat the whole
    /// point of the source-scan pin — the parent test would pass
    /// on a compromised source with no signal that the detector is
    /// broken.  This test exercises every predicate variant the
    /// parent test relies on.
    #[test]
    fn platform_cfg_detector_fires_on_synthetic_variants() {
        // Repeats the parent's helper — kept local because it's a
        // private inner fn.  Any drift here MUST match the parent.
        fn is_platform_cfg_line(line: &str) -> bool {
            let l = line.trim();
            if l.starts_with("//") || l.starts_with("///") || l.starts_with("*") {
                return false;
            }
            if l.contains("cfg(test)")
                || l.contains("cfg!(test)")
                || l.contains("cfg(all(test")
                || l.contains("cfg_attr(test")
            {
                return false;
            }
            let has_attr = l.contains("#[cfg(") || l.contains("#[cfg_attr(") || l.contains("cfg!(");
            let has_platform_pred = l.contains("target_os")
                || l.contains("target_arch")
                || l.contains("target_family")
                || l.contains("target_pointer_width")
                || l.contains("cfg(unix)")
                || l.contains("cfg(windows)")
                || l.contains("cfg!(unix)")
                || l.contains("cfg!(windows)");
            has_attr && has_platform_pred
        }

        // MUST FIRE — every shape a real violation could take.
        let violations = [
            r#"#[cfg(target_os = "linux")]"#,
            r#"#[cfg(target_os = "macos")]"#,
            r#"#[cfg(target_arch = "x86_64")]"#,
            r#"#[cfg(target_family = "unix")]"#,
            r#"#[cfg(target_pointer_width = "64")]"#,
            r#"#[cfg(unix)]"#,
            r#"#[cfg(windows)]"#,
            r#"#[cfg_attr(target_os = "linux", allow(dead_code))]"#,
            r#"    #[cfg(any(target_os = "linux", target_os = "macos"))]"#,
            r#"let x = if cfg!(target_os = "linux") { 100 } else { 200 };"#,
            r#"cfg!(unix)"#,
            r#"cfg!(windows)"#,
            // Same-line-attribute shape — the parent scan uses an
            // inclusive `..=idx` range to catch this; the detector
            // itself must recognize the shape as a violation.
            r#"#[cfg(unix)] register_consensus_constant!(order = 1, name = FOO, u64_be);"#,
        ];
        for v in &violations {
            assert!(
                is_platform_cfg_line(v),
                "detector MUST fire on `{v}` — a real platform-cfg \
                 variant that would silently break fingerprint parity"
            );
        }

        // MUST NOT FIRE — false-positive shapes that would break the
        // parent test on legitimate code.
        let allowed = [
            "// #[cfg(target_os = \"linux\")]",           // comment
            "/// #[cfg(target_os = \"linux\")]",          // doc comment
            "* #[cfg(target_os = \"linux\")]",            // block-comment line
            "#[cfg(test)]",                               // test-only allowed
            "#[cfg(all(test, target_os = \"linux\"))]",   // test-scoped
            "#[cfg_attr(test, allow(dead_code))]",        // test-only attr
            "cfg!(test)",                                 // runtime test check
            "let x = 100;",                               // unrelated code
            "#[cfg(feature = \"foo\")]",                  // feature-gated, not platform
            "pub const MAX_WAL_ENTRIES: usize = 65_536;", // plain const
        ];
        for a in &allowed {
            assert!(
                !is_platform_cfg_line(a),
                "detector MUST NOT fire on `{a}` — legitimate code \
                 that would produce false positives on the parent \
                 test"
            );
        }
    }
}
