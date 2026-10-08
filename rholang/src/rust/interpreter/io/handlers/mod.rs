// Per-family handler modules.  Each family (lifecycle, observation,
// mutation, stream, lock, removedir) owns the fs_* handlers whose
// effect shape matches that family's category — see
// [`HandlerFamily`](super::handler_trait::family::HandlerFamily) for
// the taxonomy.
//
// Each handler module:
//
//   - Declares a zero-sized `FsXHandler` struct.
//   - Implements [`FsHandler`](super::handler_trait::fs_handler::FsHandler)
//     for it (NAME, ARITY, Args type, parse_content, pre_charge_cost,
//     dispatch, optional per-step hooks).
//   - Registers via `#[distributed_slice(FS_HANDLERS)] static
//     FS_X_ENTRY: FsHandlerEntry = ...;`.
//
// The dispatcher ([`dispatch_via_trait`](super::handler_trait::dispatch::dispatch_via_trait))
// walks the trait through the 7-step pipeline.  Each registration
// bumps [`EXPECTED_MIGRATED_HANDLER_COUNT`](super::handler_trait::fs_handlers::EXPECTED_MIGRATED_HANDLER_COUNT)
// by one.
//
// # Status (Wave 4 complete; removedir trait-exempt wired in Wave 5)
//
// All 27 trait-registered handlers landed; the trait-exempt
// fs_remove_dir has the real DD-RemoveDirReplyShape handler ported
// in slices 5.136-5.141 (decomposed removedir submodule under
// `handlers/removedir/`), URN registration swapped from the slice-
// 5.44 stub in slice 5.142, and the stub deleted in slice 5.143.
//
//   - `lifecycle` (3): fs_quarantine (4.12), fs_close (4.13),
//     fs_open (4.32).
//   - `observation` (9): fs_flush (4.14), fs_tell (4.15),
//     fs_seek (4.16), fs_size (4.17), fs_exists (4.18),
//     fs_stat (4.19), fs_read (4.20), fs_read_at (4.21),
//     fs_entries (4.31).
//   - `mutation` (8): fs_truncate (4.24), fs_chmod (4.25),
//     fs_rename (4.26), fs_chown (4.27), fs_remove_file (4.28),
//     fs_copy_file (4.33), fs_write (4.38), fs_write_at (4.39).
//   - `stream` (3): fs_entries_stream_close (4.22),
//     fs_entries_stream_open (4.29), fs_entries_stream_next (4.30).
//   - `lock` (4): fs_lock_range (4.34), fs_lock_sequential (4.35),
//     fs_release_lock (4.36), fs_release_all_for_holder (4.37).
//   - `removedir`: trait-exempt `FsProcesses::fs_remove_dir` method
//     (slices 5.136–5.141) with the DD-RemoveDirReplyShape 4-shape
//     divergence replies.  URN registration swapped from the slice-
//     5.44 stub to the real handler in slice 5.142; stub deleted in
//     slice 5.143.

pub mod helpers;
pub mod lifecycle;
pub mod lock;
pub mod mutation;
pub mod observation;
pub mod removedir;
pub mod stream;
