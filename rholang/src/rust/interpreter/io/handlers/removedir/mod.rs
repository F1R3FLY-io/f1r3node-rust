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
//   * `walk.rs` (slice 5.138, pending): recursive-walk functions
//     (`collect_recursive_manifest`, `walk_dirfd_recursive`,
//     `walk_and_unlink_recursive_with_journal`,
//     `remove_dir_recursive`).
//   * `handler.rs` (future slice): the `impl FsProcesses` block
//     carrying `fs_remove_dir` + `finalize_failure_journal` +
//     `journal_path_mutation_single`.
//
// Each submodule lands as its own reviewed slice.  Until the
// handler-side slice lands, the trait-exempt stub
// `SystemProcesses::fs_remove_dir_stub` (which replies
// `FSERR_UNSUPPORTED`) continues to serve runtime dispatch — this
// module's items are reachable but no production code calls them
// yet.

pub mod reply;
