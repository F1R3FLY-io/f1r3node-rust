// Shared helpers used across the fs_* handler modules.  Lives
// here (not in `io/path/` or `io/mod.rs`) because these helpers
// are specifically for handler-level operations: fd lookup +
// syscall dispatch + reply construction.  Pure path primitives
// (safe_descend_verified, canonicalize_lexical, fstatat_meta,
// leaf_of) live in `io/path/mod.rs`.
//
// # Scope boundary
//
// This module corresponds to fileio's `handlers_helpers.rs` (1,345
// LOC on fileio).  Each handler-helpers slice adds the specific
// helpers that slice's handlers need.  Current contents:
//
//   - `read_impl_via_table` — sequential + positional read.
//     Added by slice 4.20 (fs_read).
//
// Yet to land (listed roughly in handler-migration order):
//
//   - `journal_state_read_via_table` — WAL state-read entry
//     (fs_stat / fs_exists / fs_size / fs_read; currently stubbed
//     at every call site).
//   - `journal_read_via_table`, `journal_read_divergence_via_table`
//     — WAL read entry with divergence discriminator (fs_read).
//   - `journal_write_via_table`, `journal_truncate_via_table`,
//     etc. — WAL mutation journaling (fs_write, fs_truncate, ...).
//   - `holder_id_of`, `resolve_lock_mode` — lock-handler helpers.

pub mod read_impl;

pub use read_impl::read_impl_via_table;
