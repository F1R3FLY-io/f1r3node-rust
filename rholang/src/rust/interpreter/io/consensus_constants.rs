// T-15 (2026-09-08): single-source-of-truth re-export module for
// every consensus-observable constant in the io subsystem.
// Documented in `docs/consensus-invariants.md` § 5 (byte gates) +
// § 3 (WAL entry serialization) + § 9a (composition-time
// constants).
//
// The constants themselves stay in their originating modules
// (`mod.rs`, `handlers.rs`, `wal.rs`, `lock.rs`, `snapshot.rs`)
// so their `register_consensus_constant!` fingerprint fold
// invocations still resolve.  This module is a bag of `pub use`
// aliases whose sole purpose is to make the full set audit-able
// from a single import line:
//
//     use rholang::rust::interpreter::io::consensus_constants::*;
//
// Adding a new consensus constant: (1) add the `pub const` in
// the originating module with its `register_consensus_constant!`
// invocation and any inline floor checks; (2) add the `pub use`
// alias here; (3) confirm the fingerprint golden hex + the
// `consensus_fold_slice_has_expected_entry_count` count assertion
// both roll together.

// Byte gates (§ 5) — path / write / read caps.
pub use super::handlers::{MAX_ENTRIES, MAX_WRITE_BYTES};
pub use super::lock::{LOCK_ID_CEILING, MAX_RANGES_PER_FILE, MAX_WAITERS_PER_FILE};
// WAL entry serialization (§ 3).
pub use super::snapshot::{MANIFEST_FORMAT_VERSION, SNAPSHOT_FORMAT_VERSION};
pub use super::wal::{MAX_WAL_ENTRIES, WAL_OP_VARIANTS, WAL_OUTCOME_VARIANTS};
pub use super::{MAX_CHUNK_ITEMS, MAX_OPEN_FDS, MAX_READ_BYTES, MAX_TRUNCATE_BYTES};

#[cfg(test)]
mod tests {
    use super::super::consensus_fingerprint::CONSENSUS_FOLD;

    /// T-15 review-fix (S3, 2026-09-08): every constant registered
    /// into `CONSENSUS_FOLD` (the fingerprint fold's authoritative
    /// list) MUST also be re-exported here.  A new consensus
    /// constant added via `register_consensus_constant!` without a
    /// companion `pub use` alias would silently drift out of the
    /// audit surface — a `use ...::consensus_constants::*;`
    /// caller would see stale coverage.
    ///
    /// The check is name-based: `stringify!($const_name)` at
    /// register site vs. re-export identifier here.  Adding a
    /// constant is a two-line change (register + re-export); miss
    /// either and this pin fires.
    ///
    /// `MANIFEST_FORMAT_VERSION` is intentionally re-exported here
    /// but NOT in `CONSENSUS_FOLD` (per M-35 / F11 A8 — the
    /// manifest sig format is a distinct wire surface with its own
    /// version); it is excluded from the drift check via a
    /// documented allow-list.
    #[test]
    fn consensus_constants_reexports_every_fold_entry() {
        let fold_names: std::collections::BTreeSet<&'static str> =
            CONSENSUS_FOLD.iter().map(|e| e.name).collect();

        // Names re-exported by this module.  Kept in sync with the
        // `pub use` block above.  A constant added to the `pub use`
        // block but forgotten here fires the "missing from
        // reexport_names" arm; a constant registered in
        // `CONSENSUS_FOLD` but not re-exported fires the
        // "missing from fold" arm.
        let reexport_names: std::collections::BTreeSet<&'static str> = [
            "MAX_WRITE_BYTES",
            "MAX_ENTRIES",
            "MAX_RANGES_PER_FILE",
            "MAX_WAITERS_PER_FILE",
            "LOCK_ID_CEILING",
            "SNAPSHOT_FORMAT_VERSION",
            "MAX_WAL_ENTRIES",
            "MAX_CHUNK_ITEMS",
            "MAX_OPEN_FDS",
            "MAX_READ_BYTES",
            "MAX_TRUNCATE_BYTES",
            // X-1 / CONS-1 (2026-09-11): pin WAL wire encoding
            // variant counts as consensus-observable.
            "WAL_OP_VARIANTS",
            "WAL_OUTCOME_VARIANTS",
        ]
        .into_iter()
        .collect();

        let missing_from_reexport: Vec<&'static str> =
            fold_names.difference(&reexport_names).copied().collect();
        assert!(
            missing_from_reexport.is_empty(),
            "T-15 drift: constants registered in CONSENSUS_FOLD but NOT \
             re-exported from `consensus_constants.rs`: {missing_from_reexport:?}.  \
             Add `pub use super::<origin_module>::<name>;` here alongside \
             the register invocation."
        );
        let missing_from_fold: Vec<&'static str> =
            reexport_names.difference(&fold_names).copied().collect();
        assert!(
            missing_from_fold.is_empty(),
            "T-15 drift: constants re-exported from `consensus_constants.rs` \
             but NOT registered in CONSENSUS_FOLD: {missing_from_fold:?}.  \
             Either register via `register_consensus_constant!(...)` or \
             remove the re-export."
        );
    }
}
