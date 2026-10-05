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
//
// Yet to land (listed roughly in handler-migration order):
//
//   - `fs_chown` — owner/group mutation.  Consensus caps rejected
//     (host uid/gid state is not deterministic across validators).
//   - `fs_write` / `fs_write_at` — byte-payload mutation; length-
//     parameterized cost via `post_reply_supplement`.
//   - `fs_copy_file` — two-endpoint mutation with byte-count reply.
//   - `fs_remove_file` — single-path mutation with lock-registry
//     gate.
//
// Family: [`HandlerFamily::Mutation`](super::super::handler_trait::family::HandlerFamily::Mutation).

pub mod fs_chmod;
pub mod fs_rename;
pub mod fs_truncate;
