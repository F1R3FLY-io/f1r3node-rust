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
//   - `lifecycle`: fs_quarantine registered (slice 4.12).
//     fs_open + fs_close yet to land.
//   - `observation`: yet to land.
//   - `mutation`: yet to land.
//   - `stream`: yet to land.
//   - `lock`: yet to land.
//   - `removedir`: trait-exempt (yet to land as inline handler).

pub mod lifecycle;
