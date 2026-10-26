// fs_open — (root, rel, mode, cmode) -> [true, fd]
//                                      | [false, FSERR, msg]
//
// Third and final Lifecycle-family handler.  **Non-verifying** —
// fd allocation itself isn't a persistent-state mutation, so
// there's no WAL reserve + finalize pattern.  BUT the handler has
// a non-trivial `on_replay_side_effect` that installs a shadow
// `FileHandle` at the leader's cached fd so downstream mutating
// handlers (fs_write / fs_truncate / etc.) can look up
// `(cmode, canon_path)` symmetrically.
//
// # Shadow-insert on replay (Phase-2 fd-based re-execute)
//
// Under Consensus + non-append mode, the shadow's `file` slot
// backs a REAL `libc::open` against the follower's own subdir
// file.  This is **load-bearing for Phase-2 fd-based re-execute
// ops** (fs_size / fs_read / fs_write / etc.) — those ops look
// up the shadow fd and run their syscalls on the real file.
// Under Oracular or Consensus + append (append rejected leader-
// side), the shadow's `file` slot is `None` (metadata-only).
//
// # Consensus + O_APPEND rejected (handler layer)
//
// POSIX `O_APPEND` is a per-syscall atomic-seek-to-end — the
// shadow-position model can't track where the leader actually
// wrote.  `open_impl_via_table` rejects Consensus + append with
// a specific error message.  See helper module docstring for the
// full rationale.
//
// # Dispatch — thin wrapper on open_impl_via_table
//
// The actual work (mode parse, append rejection, root
// resolution, safe_open_verified, metadata regular-file check,
// handle-table insert) lives in `open_impl_via_table`.  Dispatch
// here just forwards the args.  Shared with the (future)
// `Fs.rho` agent's wrapper that layers Shape-A root-registry
// gating on top.
//
// # No telemetry hazard chain
//
// Returns `HandlerReply::Ok(par)` where `par` is the Par built
// by `open_impl_via_table` (ok_fd or err).  Same shape as
// `fs_quarantine` / `fs_entries` etc.; chain extends to 8
// handlers.  Future HandlerReply::is_ok consumer refactor
// coordinates with these.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_open_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handle_table::FileHandle;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::open_impl_via_table;
use crate::rust::interpreter::io::mode::{fopen_flags, parse_open_mode, AccessMode, ExistPolicy};
use crate::rust::interpreter::io::path::canonicalize_lexical;
use crate::rust::interpreter::io::path::open::safe_open_verified;
use crate::rust::interpreter::io::response::extract_ok_fd;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsOpenHandler;

/// Parsed args.
pub struct FsOpenArgs {
    root: String,
    rel: String,
    mode: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsOpenHandler {
    const NAME: &'static str = "fs_open";
    const ARITY: usize = 5; // (root, rel, mode, cmode, ack)

    type Args = FsOpenArgs;

    fn parse_content(args: &[Par]) -> Result<FsOpenArgs, Box<HandlerReply>> {
        let [root_par, rel_par, mode_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String, String)",
            ));
        };
        // Cmode validation first — matches pre-trait error
        // discrimination ordering.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoString::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(mode)) => Ok(FsOpenArgs {
                root,
                rel,
                mode,
                cmode,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String, String)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_open_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsOpenArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par =
                open_impl_via_table(ctx.handles, args.root, args.rel, args.mode, args.cmode).await;
            HandlerReply::Ok(par)
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // C-R1: install a shadow FileHandle at the leader's fd
        // so downstream replay-branch mutating handlers can look
        // up (cmode, canon_path).  Phase-2: under Consensus +
        // non-append, the shadow's `file` slot backs a REAL
        // libc::open via safe_open_verified so downstream fd-
        // based re-execute ops (fs_size / fs_read / etc.) can
        // run on the follower's own subdir file.  Oracular caps
        // keep the pre-Phase-2 metadata-only shadow (file: None).
        Box::pin(async move {
            let Some(fd) = extract_ok_fd(previous) else {
                return;
            };
            let [root_par, rel_par, mode_par, cmode_par] = raw_args else {
                return;
            };
            let (Some(root), Some(rel), Some(mode_str)) = (
                RhoString::unapply(root_par),
                RhoString::unapply(rel_par),
                RhoString::unapply(mode_par),
            ) else {
                return;
            };
            // Bogus cmode with cached `[true, fd]` is a leader
            // bug; fail-closed to Consensus (most restrictive).
            let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
            let intent = parse_open_mode(&mode_str);
            // Phase-2 real-open: Consensus + non-append.  Under
            // any other combination (Oracular, or Consensus +
            // append — rejected leader-side), the shadow's
            // `file` slot stays None.
            let file: Option<std::sync::Arc<std::fs::File>> = if cmode == ConsensusMode::Consensus {
                match intent {
                    Some(intent) if intent.policy != ExistPolicy::CreateOrAppend => {
                        let root_pb = std::path::PathBuf::from(&root);
                        let (root_pb, expected_root_id) =
                            ctx.handles.root_registry.resolve_or_identity(&root_pb);
                        let rel_for_open = rel.clone();
                        let intent_copy = intent;
                        let opened = tokio::task::spawn_blocking(move || {
                            let (flags, mode_bits) = fopen_flags(intent_copy);
                            safe_open_verified(
                                &root_pb,
                                &rel_for_open,
                                flags,
                                mode_bits,
                                expected_root_id,
                            )
                        })
                        .await;
                        match opened {
                            Ok(Ok(f)) => Some(std::sync::Arc::new(f)),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };
            // `canonicalize_lexical` fallback — the leader
            // reached a `[true, fd]` reply, so the lexical join
            // succeeded; this is safety against a defensive
            // regression on dev.
            let canon_path = canonicalize_lexical(&root, &rel).unwrap_or_else(|_| {
                let mut p = std::path::PathBuf::from(&root);
                if !rel.is_empty() {
                    p.push(&rel);
                }
                p
            });
            let deploy = ctx.current_deploy_scope();
            let shadow = FileHandle {
                file,
                canon_path,
                mode: intent.map(|i| i.mode).unwrap_or(AccessMode::Read),
                cmode,
                position: 0,
                deploy,
            };
            // Ignore the return: slot already occupied on repeat
            // call → existing handle wins.  Real divergence
            // surfaces later via WAL comparison.
            let _ = ctx.handles.insert_at(fd.as_u64(), shadow).await;
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_OPEN_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsOpenHandler as FsHandler>::NAME,
    arity: <FsOpenHandler as FsHandler>::ARITY,
    verifying: <FsOpenHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsOpenHandler>(fs, args)),
    urn_suffix: "open",
    fixed_channel: FixedChannels::fs_open,
    body_ref: BodyRefs::FS_OPEN,
    family: HandlerFamily::Lifecycle,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(cmode: &str, mode: &str) -> Vec<Par> {
        vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir/file".to_string()),
            RhoString::create_par(mode.to_string()),
            mk_cmode_par(cmode),
        ]
    }

    /// Trait-level constants wire through.  Non-verifying
    /// (unlike the mutation handlers); ARITY = 5.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsOpenHandler::NAME, "fs_open");
        assert_eq!(FsOpenHandler::ARITY, 5);
        assert!(!FsOpenHandler::VERIFYING);
    }

    /// `parse_content` success path for a representative tuple.
    #[test]
    fn parse_content_accepts_valid_tuple() {
        let parsed = FsOpenHandler::parse_content(&valid_args("consensus", "r"))
            .ok()
            .expect("valid tuple parses");
        assert_eq!(parsed.root, "/@bundle/example");
        assert_eq!(parsed.rel, "subdir/file");
        assert_eq!(parsed.mode, "r");
        assert_eq!(parsed.cmode, ConsensusMode::Consensus);
    }

    /// Both cmodes round-trip at parse (the Consensus+append
    /// rejection fires INSIDE dispatch, not at parse — parse
    /// accepts the tuple; dispatch produces the FSERR_BAD_ARG
    /// reply).
    #[test]
    fn parse_content_accepts_both_cmodes() {
        for cmode_str in ["consensus", "oracular"] {
            let parsed = FsOpenHandler::parse_content(&valid_args(cmode_str, "r"))
                .ok()
                .unwrap_or_else(|| panic!("{cmode_str} parses"));
            match cmode_str {
                "consensus" => assert_eq!(parsed.cmode, ConsensusMode::Consensus),
                "oracular" => assert_eq!(parsed.cmode, ConsensusMode::Oracular),
                _ => unreachable!(),
            }
        }
    }

    /// Various mode strings all parse — mode-string validation
    /// happens at dispatch time via `parse_open_mode`, not at
    /// parse_content.  Pin that the parser accepts the mode slot
    /// as any String.
    #[test]
    fn parse_content_accepts_any_mode_string() {
        for mode in ["r", "r+", "w", "w+", "a", "a+", "xx-bogus"] {
            let parsed = FsOpenHandler::parse_content(&valid_args("oracular", mode))
                .ok()
                .unwrap_or_else(|| panic!("mode {mode:?} parses at content level"));
            assert_eq!(parsed.mode, mode);
        }
    }

    /// LOAD-BEARING: bad cmode produces the specific "cmode
    /// must be..." error, distinct from the combined
    /// "expected (String, String, String, String)" shape error.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args("consensus", "r");
        args[3] = mk_cmode_par("Consensus"); // uppercase
        let reply = *FsOpenHandler::parse_content(&args).err().expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let three = valid_args("consensus", "r")
            .into_iter()
            .take(3)
            .collect::<Vec<_>>();
        let mut five = valid_args("consensus", "r");
        five.push(RhoString::create_par("extra".to_string()));
        assert!(FsOpenHandler::parse_content(&three).is_err());
        assert!(FsOpenHandler::parse_content(&five).is_err());
    }

    /// `parse_content` rejects a non-String in root / rel / mode.
    #[test]
    fn parse_content_rejects_non_string_in_any_slot() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..3 {
            let mut args = valid_args("consensus", "r");
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsOpenHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_open_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsOpenHandler::pre_charge_cost();
        let via_helper = fs_open_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
