// Shared helpers used across the fs_* handler modules.  Lives
// here (not in `io/path/` or `io/mod.rs`) because these helpers
// are specifically for handler-level operations: fd lookup +
// syscall dispatch + reply construction + WAL journaling.  Pure
// path primitives (safe_descend_verified, canonicalize_lexical,
// fstatat_meta, leaf_of) live in `io/path/mod.rs`.
//
// # Scope boundary
//
// This module corresponds to fileio's `handlers_helpers.rs` (1,345
// LOC on fileio).  Each handler-helpers slice adds the specific
// helpers that slice's handlers need.  Current contents:
//
//   - `read_impl_via_table` — sequential + positional read.
//     Added by slice 4.20 (fs_read).
//   - `ack_channel_hash` — WAL sidecar key derivation.  Added by
//     slice 4.23 (WAL journal helpers).
//   - `finalize_failure_journal_via_table` + three reserve
//     helpers (truncate, path_mutation_single, path_mutation_two)
//     — mutation handlers' pre_syscall + journal hooks.  Added by
//     slice 4.23.
//   - `RemoveKind` + `target_dev_inode_at` + `unlink_leaf_via_dirfd`
//     — unlink primitives for fs_remove_file (and fs_remove_dir
//     when it lands).  Added by slice 4.28.
//
// Yet to land (listed roughly in handler-migration order):
//
//   - `journal_state_read_via_table` — WAL state-read entry
//     (fs_stat / fs_exists / fs_size / fs_read / fs_entries;
//     currently stubbed at every call site).
//   - `journal_read_via_table`, `journal_read_divergence_via_table`
//     — WAL read entry with divergence discriminator (fs_read).
//   - `finalize_write_journal_via_table` — write-specific finalize
//     with byte-count fixup.
//   - `holder_id_of`, `resolve_lock_mode` — lock-handler helpers.
//   - `per_entry_ack_seed` — fs_remove_dir manifest per-entry ack.

pub mod ack_hash;
pub mod journal;
pub mod read_impl;
pub mod unlink;

pub use ack_hash::ack_channel_hash;
pub use journal::{
    finalize_failure_journal_via_table, journal_path_mutation_single_via_table,
    journal_path_mutation_two_via_table, journal_truncate_via_table,
};
pub use read_impl::read_impl_via_table;
pub use unlink::{target_dev_inode_at, unlink_leaf_via_dirfd, RemoveKind};
