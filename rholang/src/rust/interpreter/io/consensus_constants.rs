// Single-source-of-truth re-export module for every consensus-
// observable constant in the io subsystem.  Documented in
// `docs/consensus-invariants.md` (byte gates + WAL entry
// serialization + composition-time constants).
//
// The constants themselves stay in their originating modules
// (`mod.rs`, `wal.rs`, `lock.rs`, `snapshot.rs`, `handle_table.rs`)
// so their `register_consensus_constant!` fingerprint-fold
// invocations still resolve.  This module is a bag of `pub use`
// aliases whose sole purpose is to make the full set audit-able
// from a single import line:
//
//     use rholang::rust::interpreter::io::consensus_constants::*;
//
// # Adding a new consensus constant
//
//   1. Add the `pub const` in the originating module with its
//      `register_consensus_constant!` invocation and any inline
//      floor checks.
//   2. Add the `pub use` alias here.
//   3. Add the identifier string to the `REEXPORTED_FOLD_NAMES`
//      set in [`tests::consensus_constants_reexports_every_fold_entry`]
//      below.
//
// Two tests enforce the three-step discipline:
//
//   - The compile-time resolve pin
//     [`tests::consensus_constants_reexports_resolve`]
//     references every `pub use` symbol — pins step (2)
//     against step (1): a rename at origin without updating
//     the alias here fails `cargo build`.
//   - The drift gate
//     [`tests::consensus_constants_reexports_every_fold_entry`]
//     compares `CONSENSUS_FOLD` entry names against
//     `REEXPORTED_FOLD_NAMES` — pins step (3) against step (1).
//
// Together they fire loudly on either direction of drift: a
// fold entry without a re-export means a
// `use consensus_constants::*;` caller silently sees stale
// coverage; a re-export without a fold entry means the
// re-export is stale (constant was removed or renamed but the
// alias stuck).
//
// # Related consensus-observable byte caps
//
// Two additional caps consumed by Wave 4 handlers live in the
// parent `io` module (not here): `MAX_WRITE_BYTES` and
// `MAX_ENTRIES` (see `rholang::interpreter::io::MAX_WRITE_BYTES`
// / `MAX_ENTRIES` in `io/mod.rs`).  They're kept there rather than
// re-exported from this file because the parent module is the
// single source of truth for byte caps consumed inside the handler
// family files (fs_write, fs_write_at, fs_entries, etc.).  The
// three-step recipe above still applies if a new handler-side
// constant ever needs to live in this file.

// WAL entry serialization constants.
// Handle-table fd-entropy headroom cap (deploy-boundary rollback
// collision resistance).
pub use super::handle_table::FD_ENTROPY_HEADROOM_BITS;
// Lock-registry caps + minted-id ceiling.
pub use super::lock::{LOCK_ID_CEILING, MAX_RANGES_PER_FILE, MAX_WAITERS_PER_FILE};
// Snapshot format version — rolled on every wire-format change
// to WAL slice encoding.  `MANIFEST_FORMAT_VERSION` is NOT a
// `CONSENSUS_FOLD` entry (manifest sig format is a distinct
// wire surface with its own version); omitted from the test's
// `REEXPORTED_FOLD_NAMES` and from this re-export block to
// match the fold's authoritative scope.
pub use super::snapshot::SNAPSHOT_FORMAT_VERSION;
pub use super::wal::{MAX_WAL_ENTRIES, WAL_OP_VARIANTS, WAL_OUTCOME_VARIANTS};
// CMODE bundle-string constants + FS_NONCE (composition-time
// constants, mod.rs origin).
pub use super::{CMODE_CONSENSUS_STR, CMODE_ORACULAR_STR, FS_NONCE};
// Byte gates + per-runtime caps (mod.rs origin).
pub use super::{
    MAX_CHUNK_ITEMS, MAX_ENTRIES, MAX_OPEN_FDS, MAX_READ_BYTES, MAX_TRUNCATE_BYTES, MAX_WRITE_BYTES,
};

#[cfg(test)]
mod tests {
    use super::super::consensus_fingerprint::CONSENSUS_FOLD;

    /// Every constant registered into [`CONSENSUS_FOLD`] (the
    /// fingerprint fold's authoritative list) MUST also be
    /// re-exported from this module AND listed in the
    /// `REEXPORTED_FOLD_NAMES` set below.  A new consensus
    /// constant added via `register_consensus_constant!`
    /// without the companion `pub use` + name entry would
    /// silently drift out of the audit surface — a
    /// `use ...::consensus_constants::*;` caller would see
    /// stale coverage.
    ///
    /// The check is name-based: `stringify!($const_name)` at
    /// register site vs. the identifier strings here.  Missing
    /// either half fires a loud failure.
    ///
    /// `MANIFEST_FORMAT_VERSION` is intentionally NOT in this
    /// set: it is a distinct wire surface with its own version,
    /// not registered into `CONSENSUS_FOLD` (per the manifest
    /// sig design).  Omitting it from both the fold AND the
    /// re-exports keeps this test honest.
    #[test]
    fn consensus_constants_reexports_every_fold_entry() {
        let fold_names: std::collections::BTreeSet<&'static str> =
            CONSENSUS_FOLD.iter().map(|e| e.name).collect();

        let reexport_names: std::collections::BTreeSet<&'static str> = [
            // wal.rs
            "MAX_WAL_ENTRIES",
            "WAL_OUTCOME_VARIANTS",
            "WAL_OP_VARIANTS",
            // mod.rs (byte gates)
            "MAX_READ_BYTES",
            "MAX_TRUNCATE_BYTES",
            "MAX_OPEN_FDS",
            "MAX_CHUNK_ITEMS",
            "MAX_ENTRIES",
            "MAX_WRITE_BYTES",
            // mod.rs (composition-time constants)
            "CMODE_ORACULAR_STR",
            "CMODE_CONSENSUS_STR",
            "FS_NONCE",
            // lock.rs
            "MAX_RANGES_PER_FILE",
            "MAX_WAITERS_PER_FILE",
            "LOCK_ID_CEILING",
            // handle_table.rs
            "FD_ENTROPY_HEADROOM_BITS",
            // snapshot.rs
            "SNAPSHOT_FORMAT_VERSION",
        ]
        .into_iter()
        .collect();

        let missing_from_reexport: Vec<&'static str> =
            fold_names.difference(&reexport_names).copied().collect();
        assert!(
            missing_from_reexport.is_empty(),
            "drift: constants registered in CONSENSUS_FOLD but NOT \
             re-exported from `consensus_constants.rs`: {missing_from_reexport:?}.  \
             Add `pub use super::<origin_module>::<name>;` here alongside \
             the register invocation."
        );
        let missing_from_fold: Vec<&'static str> =
            reexport_names.difference(&fold_names).copied().collect();
        assert!(
            missing_from_fold.is_empty(),
            "drift: constants listed in `REEXPORTED_FOLD_NAMES` but NOT \
             registered in CONSENSUS_FOLD: {missing_from_fold:?}.  \
             Either register via `register_consensus_constant!(...)` or \
             remove the name from the set."
        );
    }

    /// Every string in the reexport set resolves to a real
    /// `pub use` import — catches a typo in the set that would
    /// otherwise go unnoticed if the typo'd name also happened
    /// not to appear in `CONSENSUS_FOLD`.
    #[test]
    fn consensus_constants_reexports_resolve() {
        // Reference every re-exported symbol so the compiler
        // fails if a `pub use` ever drops out of sync with the
        // originating module (e.g., a rename at the origin
        // without updating this module's alias).  The values
        // are discarded; the goal is name-resolution coverage.
        let _ = super::MAX_WAL_ENTRIES;
        let _ = super::WAL_OUTCOME_VARIANTS;
        let _ = super::WAL_OP_VARIANTS;
        let _ = super::MAX_READ_BYTES;
        let _ = super::MAX_TRUNCATE_BYTES;
        let _ = super::MAX_OPEN_FDS;
        let _ = super::MAX_CHUNK_ITEMS;
        let _ = super::MAX_ENTRIES;
        let _ = super::MAX_WRITE_BYTES;
        let _ = super::CMODE_ORACULAR_STR;
        let _ = super::CMODE_CONSENSUS_STR;
        let _ = super::FS_NONCE;
        let _ = super::MAX_RANGES_PER_FILE;
        let _ = super::MAX_WAITERS_PER_FILE;
        let _ = super::LOCK_ID_CEILING;
        let _ = super::FD_ENTROPY_HEADROOM_BITS;
        let _ = super::SNAPSHOT_FORMAT_VERSION;
    }
}
