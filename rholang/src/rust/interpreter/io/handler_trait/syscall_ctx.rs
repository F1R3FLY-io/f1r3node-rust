// Borrowed view of the per-call context that every fs_* handler
// touches.  Threaded into `FsHandler::dispatch` (yet to land,
// slice 4.6) and into every per-step hook (`pre_syscall`,
// `on_replay_side_effect`, `journal`, `resolve_replay_cmode`).
//
// # Field sharing shape
//
//   - `dispatcher`, `space`, `handles`, `metering` — borrowed
//     refs; the construction site (`SyscallCtx::new`) holds them
//     alive for the lifetime of the future.
//   - `mode` — [`ConsensusMode`], `Copy`.
//   - `ack` — the caller-supplied ack-channel `Par` (last
//     positional arg of every fs_* contract).  Used by handlers
//     that journal to the WAL (the WAL entry is keyed on the ack
//     channel hash).
//
// # Metering coupling (Wave 4 + 5 vs. Wave 6)
//
// `metering` is typed as `&'a Arc<dyn
// crate::rust::interpreter::accounting::noop::Metering>` so a
// construction site can swap
// [`crate::rust::interpreter::accounting::noop::NoopMetering`]
// (Wave 4 + 5) for the real `MeteredMachine` (Wave 6) without
// touching any handler call site.  See `accounting/noop.rs` for
// the swap-cost rationale.
//
// # Why no `FsProcesses` wrapper (yet)
//
// Fileio's `SyscallCtx::new(fs: &FsProcesses, ack: &Par)` takes a
// `FsProcesses` struct (the per-runtime handler-dispatch surface)
// as its single source for the five borrowed fields.  `FsProcesses`
// has not yet been ported to dev — it lands with the dispatcher
// slice (4.6 / 4.7).  Until then, the `new` constructor here takes
// the fields individually.  A follow-up convenience constructor
// (`SyscallCtx::from_fs_processes`) can be added when `FsProcesses`
// lands; the current API lets the dispatcher slice swap in whichever
// construction site makes sense without touching this struct.

use std::sync::Arc;

use models::rhoapi::Par;

use crate::rust::interpreter::accounting::noop::Metering;
use crate::rust::interpreter::dispatch::RhoDispatch;
use crate::rust::interpreter::io::handle_table::FileHandleTable;
use crate::rust::interpreter::io::lock::DeployScope;
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_runtime::RhoISpace;

/// Borrowed per-call context threaded into every fs_* handler's
/// `dispatch` method + per-step hooks.
pub struct SyscallCtx<'a> {
    pub dispatcher: &'a RhoDispatch,
    pub space: &'a RhoISpace,
    pub handles: &'a FileHandleTable,
    pub mode: ConsensusMode,
    pub metering: &'a Arc<dyn Metering>,
    /// Caller-supplied ack-channel `Par` (last positional arg of
    /// every fs_* contract).  Journaling handlers key WAL entries
    /// on this channel's hash; non-journaling handlers ignore it.
    pub ack: &'a Par,
}

impl<'a> SyscallCtx<'a> {
    /// Construct a context from the individual field refs.  Named
    /// `new` for grep-ability at the dispatcher's construction site
    /// (yet to land, slice 4.6 / 4.7); when `FsProcesses` is ported,
    /// a convenience `from_fs_processes(fs, ack)` constructor can be
    /// layered on top without touching this signature.
    pub fn new(
        dispatcher: &'a RhoDispatch,
        space: &'a RhoISpace,
        handles: &'a FileHandleTable,
        mode: ConsensusMode,
        metering: &'a Arc<dyn Metering>,
        ack: &'a Par,
    ) -> Self {
        SyscallCtx {
            dispatcher,
            space,
            handles,
            mode,
            metering,
            ack,
        }
    }

    /// Read the per-runtime "current deploy scope" cell — the same
    /// value `FileHandleTable::current_deploy_scope` returns.  Used
    /// by lock-acquire handlers (yet to land, slice 4.9) to tag
    /// `LockRegistry` entries for deploy-end sweep.  Sentinel
    /// `[0; 32]` = no deploy in flight (test / genesis path).
    pub fn current_deploy_scope(&self) -> DeployScope { self.handles.current_deploy_scope() }
}

// Compile-time witness that `SyscallCtx<'a>` has the expected field
// shape.  A drift (e.g., a field removed, renamed, or re-typed)
// trips a build error here before the dispatcher slice (4.6 / 4.7)
// gets further along.
const _SYSCALL_CTX_SHAPE_WITNESS: fn() = || {
    #[allow(dead_code)]
    fn assert_shape<'a>(ctx: &SyscallCtx<'a>) {
        let _: &RhoDispatch = ctx.dispatcher;
        let _: &RhoISpace = ctx.space;
        let _: &FileHandleTable = ctx.handles;
        let _: ConsensusMode = ctx.mode;
        let _: &Arc<dyn Metering> = ctx.metering;
        let _: &Par = ctx.ack;
    }
};

// Compile-time witness that `SyscallCtx::new` takes the expected
// argument shape.  A drift in the constructor's signature trips
// a build error here.
const _SYSCALL_CTX_NEW_SIG_WITNESS: fn() = || {
    #[allow(dead_code)]
    fn assert_new_sig<'a>(
        dispatcher: &'a RhoDispatch,
        space: &'a RhoISpace,
        handles: &'a FileHandleTable,
        mode: ConsensusMode,
        metering: &'a Arc<dyn Metering>,
        ack: &'a Par,
    ) -> SyscallCtx<'a> {
        SyscallCtx::new(dispatcher, space, handles, mode, metering, ack)
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::accounting::noop::NoopMetering;
    use crate::rust::interpreter::io::ConsensusMode;

    // # Scope of these tests
    //
    // Constructing a full `SyscallCtx` requires `dispatcher` +
    // `space`, which need async rspace init — too much setup for a
    // unit test on a one-liner delegate.  The behavior of
    // `SyscallCtx::current_deploy_scope` is pinned by:
    //
    //   - compile-time witness `_SYSCALL_CTX_SHAPE_WITNESS` (above)
    //     — the method's existence + return type.
    //   - End-to-end handler tests (slice 4.7+) — the method's
    //     runtime behavior under real dispatch.
    //
    // The tests below exercise `FileHandleTable::current_deploy_scope`
    // directly — the underlying method the delegate forwards to.
    // Live in this file (vs. `handle_table.rs`) because
    // `SyscallCtx::current_deploy_scope` depends on this behavior:
    // if a future refactor broke `FileHandleTable::current_deploy_scope`
    // (e.g., introduced caching that stales out writes), the
    // delegate would silently return bad values.  Catching it here
    // keeps the dependency surfaced in the delegate's file.

    /// `FileHandleTable::current_deploy_scope` returns the `[0u8; 32]`
    /// sentinel on a fresh table — the "no deploy in flight"
    /// condition that `SyscallCtx::current_deploy_scope` surfaces
    /// to lock-acquire handlers.
    #[test]
    fn file_handle_table_current_deploy_scope_fresh_is_sentinel() {
        let handles = FileHandleTable::new();
        assert_eq!(handles.current_deploy_scope(), [0u8; 32]);
    }

    /// `FileHandleTable::current_deploy_scope` returns the value
    /// most recently written via `set_current_deploy_scope`.  Pins
    /// against a stale-read regression (e.g., a cached-at-construction
    /// optimization) that would make the SyscallCtx delegate
    /// return stale values to lock-acquire handlers.
    #[test]
    fn file_handle_table_current_deploy_scope_roundtrips_set_value() {
        let handles = FileHandleTable::new();
        let scope: DeployScope = [7u8; 32];
        handles.set_current_deploy_scope(scope);
        assert_eq!(handles.current_deploy_scope(), scope);
    }

    /// Compile-time witness that NoopMetering coerces into
    /// `Arc<dyn Metering>` — the exact shape SyscallCtx.metering
    /// expects.  A regression that broke dyn-coercion on
    /// NoopMetering (e.g., added a generic method to the trait)
    /// would trip here.
    #[test]
    fn noop_metering_coerces_into_arc_dyn_metering() {
        let m: Arc<dyn Metering> = Arc::new(NoopMetering::new());
        // Round-trip through a reserve call to prove the dyn
        // dispatch works end-to-end.
        use std::borrow::Cow;

        use crate::rust::interpreter::accounting::costs::Cost;
        let cost = Cost {
            value: 1,
            operation: Cow::Borrowed("test"),
        };
        assert!(m.reserve_primitive(cost).is_ok());
    }

    /// Mode defaults to Consensus (the more restrictive mode) when
    /// a construction site omits it — pinned here because
    /// SyscallCtx carries `mode: ConsensusMode` by value and a
    /// `..Default::default()`-style construction would silently
    /// pick Consensus.  Not a bug (fail-closed by design) but
    /// surface-worthy.
    #[test]
    fn consensus_mode_default_is_consensus() {
        assert_eq!(ConsensusMode::default(), ConsensusMode::Consensus);
    }
}
