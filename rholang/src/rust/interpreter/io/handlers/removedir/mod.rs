// Trait-exempt `fs_remove_dir` handler + its exclusive helpers.
//
// `fs_remove_dir` stays trait-exempt (vs. the uniform 27 handlers
// in sibling families) because it has:
//
//   * Two structurally distinct dispatch modes (recursive vs
//     non-recursive) with divergent WAL shapes.
//   * An inline recursive-walk syscall loop that runs under a
//     `spawn_blocking` closure holding the FsProcesses lock
//     registry clone.
//   * A reply shape carrying an `nDeleted` count field per
//     DD-RemoveDirReplyShape.
//
// These traits make the trait's uniform dispatch shape a poor fit.
//
// # Module layout (ports fileio's `handlers_removedir.rs` as a
//   decomposed submodule tree)
//
//   * `reply.rs` (slice 5.137): reply builders + wire helpers
//     + `RemoveKind::as_wire` + io-error → FSERR code bridge.
//   * `walk.rs` (slice 5.138): recursive-walk syscall primitives
//     (`unlink_manifest_entry`, `collect_recursive_manifest`,
//     `walk_dirfd_recursive`, `remove_dir_recursive`).  Heavy
//     unsafe libc blocks; see each function's inline SAFETY
//     comments.
//   * `journal.rs` (slice 5.140): Consensus-recursive composer
//     `walk_and_unlink_recursive_with_journal` tying the walker
//     to the reply builders + WAL journaling + per-entry ack
//     seeds.
//   * `handler.rs` (slice 5.141): the `impl FsProcesses` block
//     carrying `fs_remove_dir`.  Composes everything above into
//     a dispatchable handler method.  Slice 5.142 swapped the
//     slice-5.44 stub for this handler at the runtime URN
//     registration in `rho_runtime::dispatch_table_creator`;
//     slice 5.143 deleted the stub.  Supporting helpers
//     `finalize_failure_journal` and `journal_path_mutation_single`
//     live in triage as `_via_table` helpers in
//     `handlers::helpers::journal` and are reused directly here.
//
// Each submodule landed as its own reviewed slice.

pub mod handler;
pub mod journal;
pub mod reply;
pub mod walk;
