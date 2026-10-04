// fs_quarantine — (rootCanon, rel) -> [true, canonPath] | [false, code, msg]
//
// Non-verifying lifecycle helper.  Runs `safe_descend_verified`
// inside a `spawn_blocking_par` task, echoes the caller-supplied
// joined path on success (no `canonicalize` call → no host drift).
//
// # Why first
//
// Simplest of the 3 lifecycle handlers (no shadow-fd side effects,
// no WAL journaling, no cmode gating).  Serves as the end-to-end
// shape pin for the dispatcher framework (slice 4.11) before the
// more intricate `fs_open` + `fs_close` handlers land.
//
// # Replay behavior
//
// Non-verifying — `on_replay_side_effect` is a no-op (default from
// the trait).  On `is_replay = true`, the dispatcher tautologically
// echoes `previous` (the leader's cached reply).  No handler-local
// state advances on replay.
//
// # Dispatch
//
// 1. Resolve the caller's logical root through the
//    [`RootIdentityRegistry`](super::super::super::path::identity::RootIdentityRegistry)
//    via `resolve_or_identity`.  Returns `(on_disk_root,
//    expected_root_id)` — pre-Shape-A unregistered roots fall
//    through as `(logical, None)` so Oracular callers over arbitrary
//    host paths work.
// 2. `spawn_blocking_par` wraps the TOCTOU-immune
//    [`safe_descend_verified`](super::super::super::path::descend::safe_descend_verified)
//    call.  On success, build `ok_string(root.join(rel))`.  On
//    failure, map the `QuarantineError` to its FSERR via
//    [`quarantine_err_reply`](super::super::super::path::quarantine_err_reply).
//
// # HandlerReply ok/err wrapping — telemetry hazard
//
// The handler returns [`HandlerReply::ok`] regardless of whether
// the closure produced a success or an error Par.  Rholang callers
// pattern-match the Par's `[true, ...]` / `[false, ...]` head, not
// the Rust wrapper variant, so this is consensus-correct.  BUT:
//
// If a future consumer of `HandlerReply::is_ok` (reserved for
// metrics / WAL-Failure journaling per the trait's reply.rs
// docstring) materializes, this handler would misreport error
// paths as successes.  The structural fix would require threading
// `HandlerReply` through `spawn_blocking_par`'s closure (changing
// its signature from `FnOnce() -> Par` to
// `FnOnce() -> HandlerReply`), which is a cross-slice change
// touching every handler + the spawn wrapper.
//
// Future handlers (fs_open, fs_close, fs_read, etc.) that follow
// the `spawn_blocking_par(...) -> Par -> HandlerReply::ok(par)`
// pattern inherit the same hazard.  If / when the first
// is_ok consumer lands, revisit the wrapper signature + migrate
// all handlers at that point.  Flagging here so the pattern
// doesn't propagate blindly.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_quarantine_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, spawn_blocking_par, FsHandler, FsHandlerEntry, HandlerFamily,
    HandlerReply, SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::quarantine_err_reply;
use crate::rust::interpreter::io::response::{err, ok_string};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.  All associated-item / method logic
/// lives on the [`FsHandler`] impl.
pub struct FsQuarantineHandler;

/// Parsed args for [`FsQuarantineHandler`].
///
/// `root` is the caller's canonicalized logical root (e.g.,
/// `/@bundle/example` under Shape A, or any host-filesystem path
/// under Oracular).  `rel` is the relative path to descend from
/// `root`.
pub struct FsQuarantineArgs {
    root: String,
    rel: String,
}

impl FsHandler for FsQuarantineHandler {
    const NAME: &'static str = "fs_quarantine";
    const ARITY: usize = 3; // (rootCanon, rel, ack)

    type Args = FsQuarantineArgs;

    fn parse_content(args: &[Par]) -> Result<FsQuarantineArgs, Box<HandlerReply>> {
        let [root_par, rel_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String)",
            ));
        };
        match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => Ok(FsQuarantineArgs { root, rel }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_quarantine_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsQuarantineArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Shape-A root resolution: look up the caller's logical
            // root in the registry.  Unregistered logicals fall
            // through to `(logical, None)` — Oracular callers over
            // arbitrary host paths still work.
            let logical = PathBuf::from(&args.root);
            let (on_disk_root, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let rel = args.rel;

            // spawn_blocking_par runs the TOCTOU-immune descend off
            // the tokio reactor.  Panics in the closure become
            // deploy aborts via JOIN_ERR_ABORT_PREFIX — the
            // fail-closed discipline from spawn_blocking.rs.
            let par = spawn_blocking_par(move || -> Par {
                match safe_descend_verified(&on_disk_root, &rel, expected_root_id) {
                    Ok(_safe_parent) => {
                        // Echo the joined path — no canonicalize
                        // call → no host drift between leader /
                        // follower on paths that resolve to the
                        // same bytes.
                        ok_string(on_disk_root.join(&rel).to_string_lossy().into_owned())
                    }
                    Err(qe) => {
                        let (code, msg) = quarantine_err_reply(&qe);
                        err(code, msg)
                    }
                }
            })
            .await;
            // Both success + failure replies are already fully-formed
            // Pars from `ok_string` / `err`.  Wrap into `HandlerReply`
            // through the Ok constructor regardless — the Rholang
            // caller pattern-matches the Par's `[true, ...]` /
            // `[false, ...]` head, not the Rust wrapper variant.
            // See `HandlerReply` module docstring.
            HandlerReply::ok(par)
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration — populated at
/// link time, consumed by the yet-to-land `rho_runtime.rs` wiring
/// that walks `FS_HANDLERS.iter()`.  The dispatcher fn-pointer
/// forwards through
/// [`dispatch_via_trait_owned`](super::super::super::handler_trait::dispatch::dispatch_via_trait_owned).
#[distributed_slice(FS_HANDLERS)]
static FS_QUARANTINE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsQuarantineHandler as FsHandler>::NAME,
    arity: <FsQuarantineHandler as FsHandler>::ARITY,
    verifying: <FsQuarantineHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsQuarantineHandler>(fs, args)),
    urn_suffix: "quarantine",
    fixed_channel: FixedChannels::fs_quarantine,
    body_ref: BodyRefs::FS_QUARANTINE,
    family: HandlerFamily::Lifecycle,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoNumber;

    // Runtime dispatch testing of fs_quarantine requires a tempdir
    // fixture (safe_descend_verified touches the filesystem) + a
    // SyscallCtx with real dispatcher / space (needed by the
    // dispatcher framework's step-1 unapply).  The handler's
    // isolated surface is tested here:
    //
    //   - parse_content success + failure paths.
    //   - pre_charge_cost delegates to costs::fs_quarantine_cost.
    //   - trait-level constants (NAME, ARITY, VERIFYING).
    //
    // The FS_HANDLERS entry registration is pinned by the
    // registry-count test in `handler_trait::fs_handlers::tests`;
    // that pin trips if this entry fails to link.
    //
    // End-to-end dispatch (through the framework's 7-step pipeline,
    // off a real FsProcesses) lands with the handler-registration
    // slice that wires the dispatcher into rho_runtime.rs.

    /// Trait-level constants wire through to the declared values.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsQuarantineHandler::NAME, "fs_quarantine");
        assert_eq!(FsQuarantineHandler::ARITY, 3);
        assert!(!FsQuarantineHandler::VERIFYING);
    }

    /// `parse_content` success path returns `(root, rel)`.
    #[test]
    fn parse_content_accepts_two_strings() {
        let args = vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir/file".to_string()),
        ];
        let parsed = FsQuarantineHandler::parse_content(&args)
            .ok()
            .expect("two strings parse");
        assert_eq!(parsed.root, "/@bundle/example");
        assert_eq!(parsed.rel, "subdir/file");
    }

    /// `parse_content` rejects wrong arg count (empty, 1, 3+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let one = vec![RhoString::create_par("root".to_string())];
        let three = vec![
            RhoString::create_par("root".to_string()),
            RhoString::create_par("rel".to_string()),
            RhoString::create_par("extra".to_string()),
        ];
        assert!(FsQuarantineHandler::parse_content(&empty).is_err());
        assert!(FsQuarantineHandler::parse_content(&one).is_err());
        assert!(FsQuarantineHandler::parse_content(&three).is_err());
    }

    /// `parse_content` rejects a non-string in either slot.
    #[test]
    fn parse_content_rejects_non_string_args() {
        let non_string_root = vec![
            RhoNumber::create_par(42),
            RhoString::create_par("rel".to_string()),
        ];
        let non_string_rel = vec![
            RhoString::create_par("root".to_string()),
            RhoNumber::create_par(42),
        ];
        assert!(FsQuarantineHandler::parse_content(&non_string_root).is_err());
        assert!(FsQuarantineHandler::parse_content(&non_string_rel).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_quarantine_cost`.
    /// The golden-value weight is pinned at the costs module
    /// (`fs_quarantine_cost_pin` in `io/costs.rs` tests, slice 4.2);
    /// here we just prove the handler reads through to the helper.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsQuarantineHandler::pre_charge_cost();
        let via_helper = fs_quarantine_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// Shape pin on the Err reply built from a sample
    /// `QuarantineError` — ensures the handler's error path builds a
    /// well-formed `[false, code, msg]` reply via
    /// `quarantine_err_reply`.  The actual error-code mapping is
    /// pinned at `path::quarantine_err_reply` (existing dev tests).
    #[test]
    fn quarantine_err_reply_builds_well_formed_par() {
        use crate::rust::interpreter::io::path::QuarantineError;
        let qe = QuarantineError::Empty;
        let (code, msg) = quarantine_err_reply(&qe);
        let reply = err(code, msg);
        // Not vacuous: a non-default Par.
        assert_ne!(
            prost::Message::encode_to_vec(&reply),
            prost::Message::encode_to_vec(&Par::default())
        );
    }
}
