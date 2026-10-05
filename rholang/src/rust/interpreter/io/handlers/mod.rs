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
// # Status (Wave 4)
//
//   - `lifecycle`: fs_quarantine (slice 4.12), fs_close (slice 4.13)
//     registered.  fs_open yet to land.
//   - `observation`: fs_flush (4.14), fs_tell (4.15), fs_seek (4.16),
//     fs_size (4.17), fs_exists (4.18), fs_stat (4.19), fs_read (4.20),
//     fs_read_at (4.21) registered.  fs_entries yet to land.
//   - `mutation`: yet to land.
//   - `stream`: fs_entries_stream_close (slice 4.22) registered.
//     fs_entries_stream_open / _next yet to land.
//   - `lock`: yet to land.
//   - `removedir`: trait-exempt (yet to land as inline handler).

pub mod helpers;
pub mod lifecycle;
pub mod observation;
pub mod stream;
