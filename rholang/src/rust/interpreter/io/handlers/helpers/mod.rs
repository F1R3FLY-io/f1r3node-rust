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
//   - `errno_reset` + `readdir_one_entry` + `entry_stat_row` +
//     `reply_is_ok` — `readdir`-backed primitives for the Stream
//     + Observation families.  Added by slice 4.30.
//   - `journal_state_read_via_table` — WAL state-read entry
//     with reply-hash capture (fs_entries_stream_next and future
//     fs_stat / fs_exists / fs_size / fs_read / fs_entries
//     migrations to full journaling).  Added by slice 4.30.
//   - `journal_write_via_table` + `finalize_write_journal_via_table`
//     + `write_impl_via_table` — fs_write / fs_write_at reserve +
//     finalize + syscall shim.  Added by slice 4.38.
//
// Yet to land (listed roughly in handler-migration order):
//
//   - `journal_read_via_table`, `journal_read_divergence_via_table`
//     — WAL read entry with divergence discriminator (fs_read).

pub mod ack_hash;
pub mod journal;
pub mod lock_helpers;
pub mod open_impl;
pub mod read_impl;
pub mod readdir;
pub mod unlink;
pub mod write_impl;

pub use ack_hash::{ack_channel_hash, per_entry_ack_seed, MAX_RECURSION_DEPTH};
pub use journal::{
    finalize_failure_journal_via_table, finalize_write_journal_via_table,
    journal_path_mutation_single_via_table, journal_path_mutation_two_via_table,
    journal_state_read_via_table, journal_truncate_via_table, journal_write_via_table,
};
pub use lock_helpers::{
    dev_inode_from_fd_via_table, holder_id_of, lock_err_reply, resolve_lock_mode,
};
pub use open_impl::open_impl_via_table;
pub use read_impl::read_impl_via_table;
pub use readdir::{entry_stat_row, errno_reset, read_dir_capped, readdir_one_entry, reply_is_ok};
pub use unlink::{target_dev_inode_at, unlink_leaf_via_dirfd, RemoveKind};
pub use write_impl::write_impl_via_table;
