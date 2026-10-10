// fs_stat — (root, rel, cmode) -> [true, record] | [false, FSERR_*, msg]
//
// Verifying observation.  Returns a stat record for the leaf
// (name, size, type, mode bits, and under Oracular also mtime /
// ctime / atime / owner / group) via `openat(O_NOFOLLOW)` +
// `File::metadata`.  Fourth verifying handler, second path-based
// (shares the arg-based cmode-resolution pattern with fs_exists).
//
// # Reply shape
//
// `[true, stat_record_par]` on success — the record is a Rholang
// Map built by [`stat_record`](super::super::super::stat::stat_record)
// with cmode-gated field stripping (Consensus strips the
// host-transient fields `mtime` / `ctime` / `atime` / `owner` /
// `group` for peer-parity determinism).
//
// `[false, FSERR_*, msg]` on failure:
//   - safe_descend_verified QuarantineError → `quarantine_err_reply`
//     (FSERR_QUARANTINE / FSERR_BAD_ARG per the variant table).
//   - fstatat_meta IoError → `err(io_err_code, io_msg_scrub)`.
//
// # Difference from fs_exists
//
// fs_exists folds IoError into `ok_bool(false)` — "file doesn't
// exist" is a legitimate observation.  fs_stat surfaces IoError
// as `FSERR_IO` — a caller asking for metadata needs to know
// whether the file is present + readable, so missing / permission
// errors go through the normal FSERR channel.
//
// # Shape-A gated resolver deferred
//
// Same as fs_exists — uses the ungated `resolve_or_identity`
// from slice 4.12.  Shape-A `resolve_or_identity_gated_for_consensus`
// lands later.
//
// # WAL state-read journaling (Wave 6 scope)
//
// Same deferral as fs_size + fs_exists — `journal_state_read_via_table`
// not yet on dev.  No-op `journal` method until the helper slice
// lands.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_stat_cost;
use crate::rust::interpreter::io::errors::{io_err_code, FSERR_BAD_ARG};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, spawn_blocking_par, FsHandler, FsHandlerEntry, HandlerFamily,
    HandlerReply, JournalPath, SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    fstatat_meta, io_msg_scrub, leaf_of, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, ok_par};
use crate::rust::interpreter::io::stat::stat_record;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsStatHandler;

/// Parsed args for [`FsStatHandler`].  `cmode` is used both in
/// dispatch (for the stat record's host-transient field
/// stripping) and by the yet-to-land Shape-A gated resolver.
pub struct FsStatArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsStatHandler {
    const NAME: &'static str = "fs_stat";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsStatArgs;

    fn parse_content(args: &[Par]) -> Result<FsStatArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String)",
            ));
        };
        // Cmode parses first — specific error message
        // discrimination matches pre-trait fs_stat.
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
        Ok(FsStatArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> Cost { fs_stat_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsStatArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Leaf name extracted BEFORE moving `args.rel` into the
            // closure — `leaf_of` is a lexical string op (no
            // filesystem touch), so this is cheap + keeps the
            // closure's capture set smaller.
            let leaf_name = leaf_of(&args.rel);
            // Ungated resolver — Shape-A gated variant deferred
            // (see module header).
            let logical = PathBuf::from(&args.root);
            let (on_disk_root, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let rel = args.rel;
            let cmode = args.cmode;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&on_disk_root, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (code, msg) = quarantine_err_reply(&qe);
                        return err(code, msg);
                    }
                };
                match fstatat_meta(&parent) {
                    // Success: build the stat record with cmode-
                    // gated field stripping, wrap in ok_par.
                    Ok(m) => ok_par(stat_record(&leaf_name, &m, cmode)),
                    // IoError: surface as FSERR_IO with scrubbed
                    // message.  Different from fs_exists which
                    // folds IoError into ok_bool(false) — stat
                    // callers need to know about missing /
                    // permission-denied as errors.
                    Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                }
            })
            .await;
            // Same HandlerReply::ok wrapping as fs_quarantine /
            // fs_exists — inherits the telemetry hazard (future
            // is_ok() consumer would misreport err Pars).
            // Documented at fs_quarantine's module header.
            HandlerReply::ok(par)
        })
    }

    /// Verifying-handler cmode resolution — arg-based.  Same
    /// shape as fs_exists: reads `raw_args[2]` via
    /// [`resolve_cmode`].  `None` → Oracular echo path.
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
    /// fs_size + fs_exists.  Wave 6 target: `journal_state_read_via_table(
    /// ctx.handles, cmode, WalOp::Stat, root.join(rel),
    /// path.produce_reply(), ctx.ack, None)`.
    fn journal<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Wave 4 + 5: no-op.
        Box::pin(async {})
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_STAT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsStatHandler as FsHandler>::NAME,
    arity: <FsStatHandler as FsHandler>::ARITY,
    verifying: <FsStatHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsStatHandler>(fs, args)),
    urn_suffix: "stat",
    fixed_channel: FixedChannels::fs_stat,
    body_ref: BodyRefs::FS_STAT,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoNumber;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsStatHandler::NAME, "fs_stat");
        assert_eq!(FsStatHandler::ARITY, 4);
        assert!(FsStatHandler::VERIFYING);
    }

    #[test]
    fn parse_content_accepts_oracular_cmode() {
        let args = vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("sub/file".to_string()),
            RhoString::create_par("oracular".to_string()),
        ];
        let parsed = FsStatHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.root, "/@bundle/example");
        assert_eq!(parsed.rel, "sub/file");
        assert_eq!(parsed.cmode, ConsensusMode::Oracular);
    }

    #[test]
    fn parse_content_accepts_consensus_cmode() {
        let args = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("l".to_string()),
            RhoString::create_par("consensus".to_string()),
        ];
        let parsed = FsStatHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.cmode, ConsensusMode::Consensus);
    }

    /// LOAD-BEARING: unknown cmode strings rejected at parse
    /// (same discipline as fs_exists).
    #[test]
    fn parse_content_rejects_unknown_cmode() {
        let args = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("l".to_string()),
            RhoString::create_par("CONSENSUS".to_string()),
        ];
        assert!(FsStatHandler::parse_content(&args).is_err());
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        assert!(FsStatHandler::parse_content(&[]).is_err());
        assert!(FsStatHandler::parse_content(&[
            RhoString::create_par("a".to_string()),
            RhoString::create_par("b".to_string()),
        ])
        .is_err());
    }

    #[test]
    fn parse_content_rejects_non_string_args() {
        let non_string_root = vec![
            RhoNumber::create_par(42),
            RhoString::create_par("l".to_string()),
            RhoString::create_par("oracular".to_string()),
        ];
        assert!(FsStatHandler::parse_content(&non_string_root).is_err());
    }

    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsStatHandler::pre_charge_cost();
        let via_helper = fs_stat_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `leaf_of` extracts the final component.  Tests pin the
    /// pre-trait fallback discipline — `Path::file_name()`
    /// returning `None` yields the full `rel` string.
    #[test]
    fn leaf_of_extracts_final_component() {
        assert_eq!(leaf_of("sub/dir/file.txt"), "file.txt");
        assert_eq!(leaf_of("solo"), "solo");
    }

    /// LOAD-BEARING: `Path::file_name()` returns `None` for a
    /// trailing-slash path (`"dir/"` → `Some("dir")` actually;
    /// `"/"` → `None`).  Pin the None-fallback branch so a future
    /// refactor that panicked on `None` would trip here.
    #[test]
    fn leaf_of_falls_back_to_full_rel_on_none() {
        // `Path::file_name()` returns `None` for the empty path
        // and for paths ending in `..`.  The fallback returns the
        // full `rel` — matches pre-trait behavior.
        assert_eq!(leaf_of(""), "");
        assert_eq!(leaf_of(".."), "..");
    }
}
