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
//
// Yet to land (listed roughly in handler-migration order):
//
//   - `fs_chmod` — mode-bits mutation; safe_descend_verified + chmod.
//   - `fs_chown` — owner/group mutation.  Consensus caps rejected
//     (host uid/gid state is not deterministic across validators).
//   - `fs_write` / `fs_write_at` — byte-payload mutation; length-
//     parameterized cost via `post_reply_supplement`.
//   - `fs_rename` / `fs_copy_file` — two-endpoint path mutation
//     via `journal_path_mutation_two_via_table`.
//   - `fs_remove_file` — single-path mutation with lock-registry
//     gate.
//
// Family: [`HandlerFamily::Mutation`](super::super::handler_trait::family::HandlerFamily::Mutation).

pub mod fs_truncate;
