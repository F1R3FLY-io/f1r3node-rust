// Typed dispatch infrastructure for the fs_* native filesystem
// handlers.  Wave 4 slice 4.4 ports the self-contained data types
// of this module:
//
//   - [`HandlerReply`] — typed success/failure discriminator for a
//     handler's reply.  Both variants wrap a `Par`; construction
//     helpers keep the byte-level reply shape identical to the
//     `response::err` / `response::ok_*` helpers.
//   - [`JournalPath`] — discriminates the four framework paths at
//     which a WAL-journaling handler's `journal` method fires
//     (leader, verify-success, verify-divergence, oracular-echo).
//   - [`HandlerFamily`] — groups handlers by effect shape
//     (mutation, observation, stream, lock, lifecycle).  Used by
//     the per-family count pin in the yet-to-land handler-dispatcher
//     slice and by future per-family file splits.
//
// # Deferred to subsequent Wave 4 slices
//
// The remaining pieces of fileio's `handler_trait.rs` depend on
// infrastructure that has not yet landed on dev:
//
//   - `SyscallCtx<'a>` — borrowed view of `FsProcesses` (that
//     struct lands with the handler-dispatcher slice).
//   - `FsHandler` trait — one impl per fs_* syscall.
//   - `FsHandlerEntry` + `FS_HANDLERS` distributed-slice registry.
//   - `dispatch_via_trait<H>` — the generic framework loop.
//
// Those pieces will land in slices 4.5 (trait + context) and 4.6
// (dispatcher + registry).  The data types in this slice have no
// forward dependency on them.
//
// # Wave 6 (cost-accounted-rho) coupling
//
// Under Wave 4 + 5, handlers dispatch through
// [`crate::rust::interpreter::accounting::noop::NoopMetering`] —
// cost reservations are no-ops.  Under Wave 6, the construction
// site swaps `NoopMetering` for `MeteredMachine` and the reserve
// calls light up.  The data types ported in this slice are
// consensus-observable through the `JournalPath` discriminator
// (determines which WAL entry a handler writes) and the
// `HandlerReply` wire shape (byte-identical to `response::err` /
// `response::ok_*`).  A silent drift at either surface produces
// different follower / leader traces under Wave 6 — the regression
// pins in each submodule's test block catch it at Wave 4.

pub mod family;
pub mod fs_handler;
pub mod journal_path;
pub mod reply;
pub mod spawn_blocking;
pub mod syscall_ctx;

pub use family::HandlerFamily;
pub use fs_handler::FsHandler;
pub use journal_path::JournalPath;
pub use reply::HandlerReply;
pub use spawn_blocking::spawn_blocking_par;
pub use syscall_ctx::SyscallCtx;
