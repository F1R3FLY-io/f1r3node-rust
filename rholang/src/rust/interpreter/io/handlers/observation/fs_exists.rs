// fs_exists — (root, rel, cmode) -> [true, Bool] | [false, FSERR_BAD_ARG, msg]
//              | [false, FSERR_QUARANTINE, msg]
//
// Verifying observation.  Checks whether a path exists under a
// quarantined root — returns `[true, true]` if `fstatat` succeeds,
// `[true, false]` on IO error (file-not-found + friends), and
// `[false, FSERR_*, msg]` on security-sensitive quarantine
// failures (symlink in path, root-identity drift, escape).
//
// # Quarantine vs. IoError discrimination
//
// `safe_descend_verified` returns `QuarantineError`:
//
//   | Variant                  | Reply shape                             |
//   |--------------------------|-----------------------------------------|
//   | `EscapesRoot`            | `err(FSERR_QUARANTINE, "...")`          |
//   | `SymlinkComponent`       | `err(FSERR_QUARANTINE, "...")`          |
//   | `RootIdentityChanged`    | `err(FSERR_QUARANTINE, "...")`          |
//   | `Empty` / `RootSelf`     | `err(FSERR_BAD_ARG, "...")`             |
//   | `IoError(_, _)`          | `ok_bool(false)` (path missing / etc.)  |
//
// Security-sensitive failures surface as FSERR (consensus-visible
// so a Rholang caller can distinguish from "file doesn't exist").
// IoError folds into `false` — matches "exists check" semantics:
// a missing file is a legitimate `false`, not an error.
//
// # Cmode source — arg-based
//
// `resolve_replay_cmode` reads from `raw_args[2]` (the cmode Par).
// Different shape from fs_seek / fs_size (fd-based cmode via
// shadow lookup).
//
// # Shape-A gated resolver deferred
//
// Fileio's fs_exists uses
// `root_registry.resolve_or_identity_gated_for_consensus(root, cmode)`
// which refuses Consensus caps targeting unregistered logical
// roots.  That gated variant has NOT been ported to dev yet —
// it needs `FSERR_UNREGISTERED_ROOT` + the test-permissive flag
// infrastructure, which land with the Shape-A boot slice.
//
// Under Wave 4 + 5, this handler uses the ungated
// `resolve_or_identity` (slice 4.12) — Consensus caps against
// unregistered logicals fall through to `(logical, None)` the
// same way Oracular caps do.  This is pre-Shape-A behavior.
// Shape-A enforcement + the gated resolver land together in a
// future slice.
//
// # WAL state-read journaling (Wave 6 scope)
//
// Same deferral as fs_size — `journal_state_read_via_table` not
// yet on dev.  No-op `journal` method until the helper slice
// lands.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_exists_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, spawn_blocking_par, FsHandler, FsHandlerEntry, HandlerFamily,
    HandlerReply, JournalPath, SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{fstatat_meta, quarantine_err_reply, QuarantineError};
use crate::rust::interpreter::io::response::{err, ok_bool};
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsExistsHandler;

/// Parsed args for [`FsExistsHandler`].  `cmode` stored on the
/// struct so `dispatch` can pass it to the (yet-to-land) gated
/// resolver — current ungated resolver ignores it but the field
/// is already positioned for the Shape-A gated upgrade.
pub struct FsExistsArgs {
    root: String,
    rel: String,
    #[allow(dead_code)] // Will be used by the gated resolver (yet to land).
    cmode: ConsensusMode,
}

impl FsHandler for FsExistsHandler {
    const NAME: &'static str = "fs_exists";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsExistsArgs;

    fn parse_content(args: &[Par]) -> Result<FsExistsArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String)",
            ));
        };
        // Cmode must parse first — matches fileio's error-message
        // discrimination (distinct message for cmode mismatch vs.
        // root/rel mismatch).
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String)",
                ));
            }
        };
        Ok(FsExistsArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> Cost { fs_exists_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsExistsArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Ungated resolver — Shape-A gated variant deferred
            // (see module header).  Consensus + unregistered
            // logical falls through to `(logical, None)` the same
            // way Oracular does.
            let logical = PathBuf::from(&args.root);
            let (on_disk_root, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let rel = args.rel;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&on_disk_root, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        return match qe {
                            // Security-sensitive quarantine errors
                            // surface as FSERR (not folded into
                            // ok_bool(false)) — the Rholang caller
                            // distinguishes "file missing" from
                            // "someone tried to escape the root".
                            QuarantineError::EscapesRoot
                            | QuarantineError::SymlinkComponent
                            | QuarantineError::RootIdentityChanged => {
                                let (c, m) = quarantine_err_reply(&qe);
                                err(c, m)
                            }
                            // Shape-invalid input (empty rel, root
                            // itself) is also an FSERR_BAD_ARG —
                            // the caller passed a malformed path.
                            QuarantineError::Empty | QuarantineError::RootSelf => {
                                let (c, m) = quarantine_err_reply(&qe);
                                err(c, m)
                            }
                            // IoError (file-not-found + friends)
                            // folds into `false` — "doesn't exist"
                            // is a legitimate observation, not an
                            // error worth surfacing to the caller.
                            QuarantineError::IoError(_, _) => ok_bool(false),
                        };
                    }
                };
                let present = fstatat_meta(&parent).is_ok();
                ok_bool(present)
            })
            .await;
            // Both success + failure replies are already fully-
            // formed Pars from `ok_bool` / `err` — wrap into
            // `HandlerReply::ok` regardless.  The telemetry hazard
            // flagged on fs_quarantine applies here: a future
            // `is_ok()` consumer would misreport err replies as
            // successes.  Pattern inherited from fs_quarantine;
            // see that handler's module docstring for the Wave 6
            // follow-up on threading `HandlerReply` through
            // `spawn_blocking_par`.
            HandlerReply::ok(par)
        })
    }

    /// Verifying-handler cmode resolution — arg-based.  Reads
    /// `raw_args[2]` (the cmode Par) via [`resolve_cmode`].  `None`
    /// → dispatcher's Oracular echo path (matches pre-trait
    /// behavior on unparseable cmode arg).
    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        Box::pin(async move {
            let [_, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    /// WAL state-read journal — Wave 6 scope.  Same deferral as
    /// fs_size.  Will journal `WalOp::Exists` with the composed
    /// `(root ⊕ rel)` path when `journal_state_read_via_table`
    /// lands.
    fn journal<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Wave 4 + 5: no-op.
        // Wave 6: journal_state_read_via_table(ctx.handles, cmode,
        // WalOp::Exists, root.join(rel), path.produce_reply(),
        // ctx.ack, None).
        Box::pin(async {})
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_EXISTS_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsExistsHandler as FsHandler>::NAME,
    arity: <FsExistsHandler as FsHandler>::ARITY,
    verifying: <FsExistsHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsExistsHandler>(fs, args)),
    urn_suffix: "exists",
    fixed_channel: FixedChannels::fs_exists,
    body_ref: BodyRefs::FS_EXISTS,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoNumber;

    /// Trait-level constants wire through — ARITY=4 for the
    /// path-based form, VERIFYING=true for the third verifying
    /// handler.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsExistsHandler::NAME, "fs_exists");
        assert_eq!(FsExistsHandler::ARITY, 4);
        assert!(FsExistsHandler::VERIFYING);
    }

    /// Happy path: all three args parse (both cmode strings).
    #[test]
    fn parse_content_accepts_oracular_cmode() {
        let args = vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir/file".to_string()),
            RhoString::create_par("oracular".to_string()),
        ];
        let parsed = FsExistsHandler::parse_content(&args)
            .ok()
            .expect("oracular parses");
        assert_eq!(parsed.root, "/@bundle/example");
        assert_eq!(parsed.rel, "subdir/file");
        assert_eq!(parsed.cmode, ConsensusMode::Oracular);
    }

    #[test]
    fn parse_content_accepts_consensus_cmode() {
        let args = vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("f".to_string()),
            RhoString::create_par("consensus".to_string()),
        ];
        let parsed = FsExistsHandler::parse_content(&args)
            .ok()
            .expect("consensus parses");
        assert_eq!(parsed.cmode, ConsensusMode::Consensus);
    }

    /// LOAD-BEARING: unknown cmode strings rejected at parse.
    /// Pin against a future refactor that silently defaulted to
    /// Consensus (fail-closed) OR Oracular (fail-open) — either
    /// would be a consensus-observable regression.
    #[test]
    fn parse_content_rejects_unknown_cmode() {
        let args = vec![
            RhoString::create_par("/".to_string()),
            RhoString::create_par("x".to_string()),
            RhoString::create_par("CONSENSUS".to_string()), // wrong case
        ];
        assert!(FsExistsHandler::parse_content(&args).is_err());
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        assert!(FsExistsHandler::parse_content(&[]).is_err());
        assert!(FsExistsHandler::parse_content(&[
            RhoString::create_par("a".to_string()),
            RhoString::create_par("b".to_string()),
        ])
        .is_err());
    }

    #[test]
    fn parse_content_rejects_non_string_root_or_rel() {
        let non_string_root = vec![
            RhoNumber::create_par(42),
            RhoString::create_par("rel".to_string()),
            RhoString::create_par("oracular".to_string()),
        ];
        let non_string_rel = vec![
            RhoString::create_par("root".to_string()),
            RhoNumber::create_par(42),
            RhoString::create_par("oracular".to_string()),
        ];
        assert!(FsExistsHandler::parse_content(&non_string_root).is_err());
        assert!(FsExistsHandler::parse_content(&non_string_rel).is_err());
    }

    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsExistsHandler::pre_charge_cost();
        let via_helper = fs_exists_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `resolve_cmode` from io/mod.rs — a byte-level pin on the
    /// cmode-string acceptance set.  Pin against drift in the
    /// accepted set (e.g., tolerating `"Consensus"` capitalized).
    #[test]
    fn resolve_cmode_accepts_canonical_strings() {
        assert_eq!(
            resolve_cmode(&RhoString::create_par("oracular".to_string())),
            Some(ConsensusMode::Oracular)
        );
        assert_eq!(
            resolve_cmode(&RhoString::create_par("consensus".to_string())),
            Some(ConsensusMode::Consensus)
        );
    }

    #[test]
    fn resolve_cmode_rejects_wrong_case() {
        assert!(resolve_cmode(&RhoString::create_par("Oracular".to_string())).is_none());
        assert!(resolve_cmode(&RhoString::create_par("CONSENSUS".to_string())).is_none());
        assert!(resolve_cmode(&RhoString::create_par("".to_string())).is_none());
        assert!(resolve_cmode(&RhoString::create_par("mixed".to_string())).is_none());
    }

    #[test]
    fn resolve_cmode_rejects_non_string_par() {
        assert!(resolve_cmode(&RhoNumber::create_par(0)).is_none());
    }
}
