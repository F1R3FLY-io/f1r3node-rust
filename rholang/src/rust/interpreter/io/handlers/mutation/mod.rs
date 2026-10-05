// Mutation family — path- or fd-based syscalls that mutate FS state.
//
// All verifying handlers.  Each pre-appends a `WalOutcome::Success`
// placeholder WAL entry in `pre_syscall` (via the WAL journal
// helpers from slice 4.23); `journal` finalizes the placeholder to
// `WalOutcome::Failure { code }` on syscall error or verify-
// divergence (the H-6 reserve + finalize pattern).  Under Consensus
// cap, the follower re-executes the syscall from the WAL entry
// and compares the reply to the leader's via `verify.rs`.
//
// # Status (Wave 4)
//
//   - `fs_truncate` — fd + n; libc::ftruncate.  Constant cost.
//     Added by slice 4.24.
//   - `fs_chmod` — path + bits + cmode; safe_descend_verified +
//     fchmodat.  First path-mutation.  Added by slice 4.25.
//   - `fs_rename` — two-endpoint path + cmode; renameat.  First
//     two-endpoint mutation; exercises
//     `journal_path_mutation_two_via_table`.  Added by slice 4.26.
//   - `fs_chown` — path + owner + group + cmode; fchownat.  First
//     NON-verifying mutation (Consensus caps rejected at
//     parse_content for NSS-divergence).  Added by slice 4.27.
//   - `fs_remove_file` — path + cmode; unlinkat under lock-registry
//     unlink gate (Consensus + locked → FSERR_BUSY, Oracular +
//     locked → log-warn + proceed).  Added by slice 4.28.
//   - `fs_copy_file` — two-endpoint byte-count copy via
//     `safe_open_verified` + `std::io::copy`.  Added by slice 4.33.
//   - `fs_write` — fd + ByteArray; libc::write.  First length-
//     parameterized mutation (single-event incremental cost + H-6
//     reserve-then-finalize with partial-write patch via
//     `finalize_write_journal_via_table`).  Added by slice 4.38.
//
// Yet to land:
//
//   - `fs_write_at` — same as fs_write but with offset; libc::pwrite;
//     no shadow position advance (POSIX pwrite semantics).
//
// Family: [`HandlerFamily::Mutation`](super::super::handler_trait::family::HandlerFamily::Mutation).

pub mod fs_chmod;
pub mod fs_chown;
pub mod fs_copy_file;
pub mod fs_remove_file;
pub mod fs_rename;
pub mod fs_truncate;
pub mod fs_write;
