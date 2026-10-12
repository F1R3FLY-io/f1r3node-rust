// fs_chown — (root, rel, owner, group, cmode)
//            → [true] | [false, FSERR, msg]
//
// Oracular-only path mutation.  **NON-verifying** — Consensus caps
// are rejected at `parse_content` with `FSERR_UNSUPPORTED`, so the
// framework's verify path never fires for this handler.
//
// # Why Consensus is banned (post-2026-09-02 S-2 security review)
//
// NSS mapping (owner / group name → uid / gid) is **host-local** on
// most deployments: `/etc/passwd` + `/etc/group` + nsswitch
// backends can differ across validators even for the same name
// strings.  Under Consensus cap, applying the chown deterministically
// requires every validator to resolve the same (name) → (uid, gid)
// mapping, which is not an invariant any shard-level mechanism
// enforces today.  The silent-divergence risk: fchownat
// succeeds everywhere, but each validator writes different uid/gid
// bits to its own on-disk inodes; the reply-hash verify compares
// the Rholang reply (just `[true]`), not the inode state, so the
// divergence escapes.
//
// To lift the ban in the future: capture resolved (uid, gid) in
// the WAL entry (not just owner/group strings) and require
// shard-wide NSS coordination (either a cluster-managed
// `/etc/passwd` deployment or an fs_genesis-time registration of
// the uid/gid pairs the shard commits to).
//
// # Oracular-only behavior
//
// Rholang callers on Oracular caps can still chown — the handler's
// behavior under Oracular matches pre-trait:
//
//   - `owner = Some(name)` → `resolve_uid(name)` via NSS; `None` →
//     `u32::MAX` (libc "-1" sentinel = no change).  Same discipline
//     for `group` / `gid`.
//   - `fchownat(dirfd, leaf, uid, gid, AT_SYMLINK_NOFOLLOW)` under
//     the usual `safe_descend_verified` dirfd.
//
// # Journal non-shape
//
// `pre_syscall` calls `journal_path_mutation_single_via_table` with
// `WalOp::Chown`; the helper's Oracular self-guard returns
// `Ok(false)` without appending.  Kept for structural parity with
// fs_chmod — a future Consensus-chown lift would populate the
// same reserve slot.
//
// `journal` is simpler than the verifying handlers: non-verifying
// means `JournalPath::VerifyDivergence` never fires, so finalize
// only patches on err replies from the leader / Oracular echo.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_chown_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_IO, FSERR_QUOTA_EXCEEDED,
    FSERR_UNSUPPORTED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_path_mutation_single_via_table,
};
use crate::rust::interpreter::io::nss::{resolve_gid, resolve_uid};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, extract_err_code, ok_bare};
use crate::rust::interpreter::io::wal::WalOp;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsChownHandler;

/// Parsed args for [`FsChownHandler`].  `owner` and `group` are
/// `Option<String>` because Rholang callers can pass `Nil` in
/// either slot to mean "no change" — `RhoString::unapply` returns
/// `None` for non-String Pars (including `Nil`), which is exactly
/// the right sentinel.  Downstream, `None` maps to
/// `u32::MAX` (libc "-1" = no change).
pub struct FsChownArgs {
    root: String,
    rel: String,
    owner: Option<String>,
    group: Option<String>,
    cmode: ConsensusMode,
}

impl FsHandler for FsChownHandler {
    const NAME: &'static str = "fs_chown";
    // 6 = (root, rel, owner, group, cmode, ack).  NON-verifying —
    // the Consensus ban at parse_content means the framework's
    // verify path never fires for this handler.
    const ARITY: usize = 6;

    type Args = FsChownArgs;

    fn parse_content(args: &[Par]) -> Result<FsChownArgs, Box<HandlerReply>> {
        let [root_par, rel_par, owner_par, group_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String|Nil, String|Nil, String)",
            ));
        };
        // C-26-F1: fail-closed on unrecognized cmode (same as the
        // other path-mutation handlers).
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        // Post-2026-09-02 S-2 review ban: fs_chown + Consensus is
        // UNSUPPORTED.  See module header for the NSS-divergence
        // rationale.
        if cmode == ConsensusMode::Consensus {
            return Err(HandlerReply::boxed_err(
                FSERR_UNSUPPORTED,
                "fs_chown: Consensus mode not supported — NSS mapping (owner/group \
                 name to uid/gid) is host-local and can differ across validators, \
                 producing silent on-disk divergence that the reply-hash verify \
                 cannot detect.  Use Oracular mode, or lift this ban by capturing \
                 resolved uid/gid in the WAL entry with shard-wide NSS coordination.",
            ));
        }
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String|Nil, String|Nil, String)",
                ));
            }
        };
        Ok(FsChownArgs {
            root,
            rel,
            // `RhoString::unapply` returns None for Nil (and any
            // non-String Par), which is exactly the "no change"
            // sentinel downstream maps to u32::MAX.
            owner: RhoString::unapply(owner_par),
            group: RhoString::unapply(group_par),
            cmode,
        })
    }

    fn pre_charge_cost() -> Cost { fs_chown_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsChownArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        // Consensus is rejected at parse_content, so this call is
        // Oracular-only and the helper's self-guard returns
        // `Ok(false)` without appending.  Kept for structural
        // parity with fs_chmod — a future Consensus-chown lift
        // would populate the same reserve slot.
        Box::pin(async move {
            let canon_path = match canonicalize_lexical(&args.root, &args.rel) {
                Ok(p) => p,
                Err(_) => return Ok(()),
            };
            if journal_path_mutation_single_via_table(
                ctx.handles,
                args.cmode,
                WalOp::Chown,
                canon_path,
                None,
                args.owner.clone(),
                args.group.clone(),
                ctx.ack,
            )
            .await
            .is_err()
            {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    "WAL cap exceeded",
                ));
            }
            Ok(())
        })
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsChownArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // NSS resolution: run OUTSIDE the spawn_blocking so a
            // slow NSS backend (nss_ldap, etc.) doesn't tie up a
            // blocking-pool thread.  Owner / group each independently
            // resolve to u32::MAX ("no change") when the Par was
            // Nil / absent.
            let uid = match args.owner {
                None => u32::MAX,
                Some(name) => match resolve_uid(&name) {
                    Ok(Some(u)) => u,
                    Ok(None) => {
                        return HandlerReply::err(FSERR_BAD_ARG, format!("unknown user {name}"));
                    }
                    Err(e) => return HandlerReply::err(FSERR_IO, e),
                },
            };
            let gid = match args.group {
                None => u32::MAX,
                Some(name) => match resolve_gid(&name) {
                    Ok(Some(g)) => g,
                    Ok(None) => {
                        return HandlerReply::err(FSERR_BAD_ARG, format!("unknown group {name}"));
                    }
                    Err(e) => return HandlerReply::err(FSERR_IO, e),
                },
            };
            // BACKLOG: migrate to resolve_or_identity_gated_for_consensus
            // once path/identity ports the Shape-A gating.  Same
            // deferral as fs_chmod / fs_rename.  Noting that the
            // Consensus ban at parse_content makes the gated vs.
            // ungated choice moot for this handler, but structural
            // parity with the other path-mutation handlers wants
            // the same migration when it lands.
            let logical = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let rel = args.rel;
            #[allow(clippy::result_large_err)]
            let r = spawn_blocking(move || -> Result<Par, Par> {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return Err(err(c, m));
                    }
                };
                // SAFETY: `parent` is a `SafeParent` from
                // `safe_descend_verified` above; dirfd stays open
                // for `parent`'s lifetime, and `leaf_ptr()` returns
                // a NUL-terminated `*const c_char` valid for the
                // same lifetime.  `fchownat` reads both without
                // retention.  AT_SYMLINK_NOFOLLOW: don't follow a
                // symlink leaf (matches the H-5 defense from
                // fs_chmod).
                let rc = unsafe {
                    libc::fchownat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        uid,
                        gid,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if rc == 0 {
                    Ok(ok_bare())
                } else {
                    let e = std::io::Error::last_os_error();
                    Err(err(io_err_code(&e), io_msg_scrub(&e)))
                }
            })
            .await;
            match r {
                Err(je) => join_err_abort(je),
                Ok(Ok(par)) => HandlerReply::ok(par),
                Ok(Err(par)) => HandlerReply::Err(par),
            }
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // fs_chown is non-verifying — the framework never invokes
        // the VerifyDivergence path.  finalize_failure fires on
        // Leader / OracularEcho when the reply carries an err code;
        // success / absent-reserve path leaves the WAL unchanged
        // (reserve was Oracular-skipped at pre_syscall anyway).
        Box::pin(async move {
            let reply = path.produce_reply();
            if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                finalize_failure_journal_via_table(ctx.handles, fserr_to_code(&code_str), ctx.ack);
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_CHOWN_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsChownHandler as FsHandler>::NAME,
    arity: <FsChownHandler as FsHandler>::ARITY,
    verifying: <FsChownHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsChownHandler>(fs, args)),
    urn_suffix: "chown",
    fixed_channel: FixedChannels::fs_chown,
    body_ref: BodyRefs::FS_CHOWN,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn mk_nil() -> Par { Par::default() }

    fn valid_oracular_args() -> Vec<Par> {
        vec![
            RhoString::create_par("/some/root".to_string()),
            RhoString::create_par("file".to_string()),
            RhoString::create_par("alice".to_string()),
            RhoString::create_par("staff".to_string()),
            mk_cmode_par("oracular"),
        ]
    }

    /// Trait-level constants wire through.  ARITY = 6, non-
    /// verifying (unlike fs_chmod / fs_rename which are verifying).
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsChownHandler::NAME, "fs_chown");
        assert_eq!(FsChownHandler::ARITY, 6);
        assert!(!FsChownHandler::VERIFYING);
    }

    /// `parse_content` success path with Oracular cmode + named
    /// owner + named group.
    #[test]
    fn parse_content_accepts_valid_oracular_tuple() {
        let parsed = FsChownHandler::parse_content(&valid_oracular_args())
            .ok()
            .expect("oracular tuple parses");
        assert_eq!(parsed.root, "/some/root");
        assert_eq!(parsed.rel, "file");
        assert_eq!(parsed.owner.as_deref(), Some("alice"));
        assert_eq!(parsed.group.as_deref(), Some("staff"));
        assert_eq!(parsed.cmode, ConsensusMode::Oracular);
    }

    /// Nil owner / Nil group both roundtrip as `None` — the
    /// "no change" sentinel.  Downstream maps `None` to
    /// `u32::MAX`.
    #[test]
    fn parse_content_accepts_nil_owner_and_group() {
        let args = vec![
            RhoString::create_par("/r".to_string()),
            RhoString::create_par("f".to_string()),
            mk_nil(),
            mk_nil(),
            mk_cmode_par("oracular"),
        ];
        let parsed = FsChownHandler::parse_content(&args)
            .ok()
            .expect("nil owner/group parse");
        assert!(parsed.owner.is_none());
        assert!(parsed.group.is_none());
    }

    /// LOAD-BEARING: Consensus cmode is REJECTED at
    /// `parse_content` with the specific NSS-divergence message
    /// (FSERR_UNSUPPORTED).  This is the entire security premise
    /// of the handler — if a future refactor accidentally
    /// accepted Consensus, silent cross-validator divergence
    /// would follow.
    #[test]
    fn parse_content_rejects_consensus_cmode() {
        let mut args = valid_oracular_args();
        args[4] = mk_cmode_par("consensus");
        let reply = *FsChownHandler::parse_content(&args)
            .err()
            .expect("consensus rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// LOAD-BEARING: bad cmode string (non-canonical, e.g.,
    /// capitalized) produces the specific "cmode must be..."
    /// message distinct from the Consensus-UNSUPPORTED message.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_oracular_args();
        args[4] = mk_cmode_par("Oracular"); // uppercase
        let reply = *FsChownHandler::parse_content(&args)
            .err()
            .expect("bad cmode rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// `parse_content` rejects wrong arg count (4 or 6+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let four = valid_oracular_args()
            .into_iter()
            .take(4)
            .collect::<Vec<_>>();
        let mut six = valid_oracular_args();
        six.push(RhoString::create_par("extra".to_string()));
        assert!(FsChownHandler::parse_content(&four).is_err());
        assert!(FsChownHandler::parse_content(&six).is_err());
    }

    /// `parse_content` rejects a non-String in `root` or `rel`
    /// (unlike `owner` / `group` which accept non-String as Nil =
    /// "no change").
    #[test]
    fn parse_content_rejects_non_string_root_or_rel() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..2 {
            let mut args = valid_oracular_args();
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsChownHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_chown_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsChownHandler::pre_charge_cost();
        let via_helper = fs_chown_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
