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
pub use super::wal::MAX_WAL_ENTRIES;
pub use super::{MAX_CHUNK_ITEMS, MAX_OPEN_FDS, MAX_READ_BYTES, MAX_TRUNCATE_BYTES};
