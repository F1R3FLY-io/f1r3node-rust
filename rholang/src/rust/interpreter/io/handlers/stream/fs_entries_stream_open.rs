// fs_entries_stream_open — (root, rel, cmode) -> [true, streamFd]
//                                               | [false, FSERR, msg]
//
// Second Stream-family handler.  Non-verifying stream lifecycle.
// Opens a streaming directory enumerator (`fdopendir`-backed
// `DirHandle`) and returns a stream fd Rholang callers advance via
// `fs_entries_stream_next` / release via `fs_entries_stream_close`.
//
// # Phase-2 Consensus ban
//
// Consensus caps are REJECTED at `parse_content` with
// `FSERR_UNSUPPORTED`.  The readdir order is filesystem-dependent
// and not stable across per-validator subdirs — a Consensus stream
// would silently diverge in enumeration order, producing different
// next-entry byte sequences on each validator (observable through
// `fs_entries_stream_next` replies).  Callers on Consensus caps
// use `fs_entries` instead, which sorts entries internally for
// determinism.
//
// A future ban-lift would need sort-stable readdir (e.g., a
// shard-committed collation order) and dropping the per-validator
// subdir discipline — neither is on the roadmap.
//
// # Replay — Phase-2 shadow-insert
//
// `on_replay_side_effect` installs a shadow `DirHandle` at the
// leader's cached fd so downstream replay-branch handlers
// (`stream_next` / `_close`) can look up `(cmode, canon_path)`
// from the DirHandleTable.  Fires only when `previous` carries
// `[true, fd]` — Consensus opens produce `[false, FSERR_UNSUPPORTED,
// ...]` leader-side, so `extract_ok_fd(previous)` returns `None`
// on the replay path and no shadow gets inserted.
//
// # No telemetry hazard
//
// Dispatch uses raw `tokio::task::spawn_blocking` with
// `Result<DirIter, Box<Par>>` inside the closure.  The outer
// async body consumes the Result and emits `HandlerReply::ok` or
// `HandlerReply::Err` directly.  `clippy::result_large_err` is a
// non-issue here because the Err variant is `Box<Par>` (pointer-
// sized) rather than inline `Par`.
//
// # Why F_DUPFD_CLOEXEC on the fdopendir fd
//
// `fdopendir` takes ownership of the fd and (per glibc / musl)
// does NOT set CLOEXEC atomically.  Pre-trait fileio used the
// L-3 pattern: `openat(... | O_CLOEXEC)` for the opening dirfd,
// then `fcntl(F_DUPFD_CLOEXEC, 0)` to produce a sibling fd with
// CLOEXEC set, close the original, and pass the new fd to
// `DirIter::from_dir_fd` (which internally calls `fdopendir`).
// The dup-with-CLOEXEC guarantees child processes can't inherit
// the stream fd across exec().

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_entries_stream_open_cost;
use crate::rust::interpreter::io::dir_handle_table::{DirHandle, DirIter};
use crate::rust::interpreter::io::errors::{
    io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_QUOTA_EXCEEDED, FSERR_UNSUPPORTED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, extract_ok_fd, ok_fd, Fd};
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsEntriesStreamOpenHandler;

/// Parsed args.
pub struct FsEntriesStreamOpenArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsEntriesStreamOpenHandler {
    const NAME: &'static str = "fs_entries_stream_open";
    const ARITY: usize = 4; // (root, rel, cmode, ack)

    type Args = FsEntriesStreamOpenArgs;

    fn parse_content(args: &[Par]) -> Result<FsEntriesStreamOpenArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String root, String rel)",
            ));
        };
        // Cmode validation first — matches pre-trait error-message
        // ordering (cmode check runs after arity+cmode-Par
        // extraction).
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        // Phase-2 ban (2026-09-01): Consensus + entriesStream* is
        // UNSUPPORTED.  Reject at open time with a specific message
        // pointing callers at `fs_entries` for sorted-deterministic
        // enumeration.
        if cmode == ConsensusMode::Consensus {
            return Err(HandlerReply::boxed_err(
                FSERR_UNSUPPORTED,
                "entriesStream* is not supported on Consensus caps — readdir order \
                 is fs-dependent and not stable across per-validator subdirs.  Use \
                 `fs_entries` (sorted, deterministic across validators) instead.",
            ));
        }
        // Post-cmode String extraction — a bogus cmode with valid
        // root/rel produces the "cmode must be..." error, not
        // "expected (String root, String rel)".
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String root, String rel)",
                ));
            }
        };
        Ok(FsEntriesStreamOpenArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> Cost { fs_entries_stream_open_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesStreamOpenArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // BACKLOG: migrate to resolve_or_identity_gated_for_consensus
            // when the gated API lands.  Note: entries_stream_open
            // Consensus is already banned at parse_content, so the
            // gated-vs-ungated choice is moot here; kept for
            // structural parity with path-mutation handlers.
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel_for_open = args.rel.clone();
            // safe_descend + openat + fdopendir in a blocking task.
            // Err returns a Box<Par> so the Result stays pointer-
            // sized (sidesteps the clippy::result_large_err issue
            // that fs_chmod handled via #[allow]).
            let opened = spawn_blocking(move || -> Result<DirIter, Box<Par>> {
                let parent = match safe_descend_verified(&root_pb, &rel_for_open, expected_root_id)
                {
                    Ok(p) => p,
                    Err(qe) => {
                        let (code, msg) = quarantine_err_reply(&qe);
                        return Err(Box::new(err(code, msg)));
                    }
                };
                // SAFETY: `parent` is a `SafeParent` from
                // `safe_descend_verified`; the dirfd is open for
                // `parent`'s lifetime and `parent.leaf_ptr()`
                // returns a NUL-terminated `*const c_char` valid
                // for the same lifetime.  `openat` does not retain
                // either past the call.
                let dir_fd = unsafe {
                    libc::openat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if dir_fd < 0 {
                    let e = std::io::Error::last_os_error();
                    return Err(Box::new(err(io_err_code(&e), io_msg_scrub(&e))));
                }
                // L-3 pattern: F_DUPFD_CLOEXEC on the fd handed to
                // fdopendir so the DIR*'s underlying fd carries
                // CLOEXEC atomically (fdopendir doesn't set it on
                // its own).  See module header on CLOEXEC
                // discipline.
                //
                // SAFETY: `dir_fd` is a freshly-opened open fd from
                // the `openat` above (post-`< 0` check).  fcntl
                // reads the fd flags and returns a new fd (or -1
                // on failure).
                let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
                // SAFETY: same `dir_fd` from `openat` above; still
                // open (we didn't transfer-close via fcntl's dup).
                // Close is fire-and-forget — even on error, the fd
                // is gone.
                unsafe { libc::close(dir_fd) };
                if read_fd < 0 {
                    let e = std::io::Error::last_os_error();
                    return Err(Box::new(err(io_err_code(&e), io_msg_scrub(&e))));
                }
                DirIter::from_dir_fd(read_fd)
                    .map_err(|e| Box::new(err(io_err_code(&e), io_msg_scrub(&e))))
            })
            .await;
            let opened = match opened {
                Ok(r) => r,
                Err(je) => join_err_abort(je),
            };
            match opened {
                Ok(iter) => {
                    // `canonicalize_lexical` returns a Result on
                    // dev.  Err arm shouldn't fire here because
                    // `safe_descend_verified` succeeded (same path-
                    // string, same quarantine checks); fallback to
                    // PathBuf::from join for Err to avoid an unwrap
                    // panic.
                    let canon_path =
                        canonicalize_lexical(&args.root, &args.rel).unwrap_or_else(|_| {
                            let mut p = PathBuf::from(&args.root);
                            if !args.rel.is_empty() {
                                p.push(&args.rel);
                            }
                            p
                        });
                    let deploy = ctx.current_deploy_scope();
                    let handle = DirHandle::new(iter, canon_path, args.cmode, deploy);
                    match ctx.handles.dir_handles.insert(handle).await {
                        Ok(fd) => match Fd::try_from(fd) {
                            Ok(fd) => HandlerReply::ok(ok_fd(fd)),
                            // Allocator contract: FileHandleTable /
                            // DirHandleTable produce values in
                            // [0, i64::MAX].  If this ever triggers,
                            // the allocator has a bug — surface it
                            // as an FSERR_QUOTA_EXCEEDED rather
                            // than a panic.
                            Err(_) => HandlerReply::err(
                                FSERR_QUOTA_EXCEEDED,
                                "allocator produced out-of-range stream fd",
                            ),
                        },
                        Err(()) => HandlerReply::err(
                            FSERR_QUOTA_EXCEEDED,
                            "per-runtime dir-stream fd cap reached",
                        ),
                    }
                }
                Err(e_par) => HandlerReply::Err(*e_par),
            }
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Shadow-insert at the leader's fd so subsequent replay-
        // branch handlers (stream_next / _close) can look up
        // (cmode, canon_path).  Only fires on `[true, fd]` leader
        // replies — Consensus opens were rejected leader-side so
        // `extract_ok_fd(previous)` returns None and no shadow
        // gets inserted.
        Box::pin(async move {
            let Some(fd) = extract_ok_fd(previous) else {
                return;
            };
            let [root_par, rel_par, cmode_par, ..] = raw_args else {
                return;
            };
            let (Some(root), Some(rel)) =
                (RhoString::unapply(root_par), RhoString::unapply(rel_par))
            else {
                return;
            };
            // Fall back to Consensus on bogus cmode — matches
            // fs_open's C-R1 fallback rationale (a bogus cmode
            // with a [true, fd] cached reply is definitionally a
            // bug; fail-closed to the more restrictive mode).
            let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
            let canon_path = canonicalize_lexical(&root, &rel).unwrap_or_else(|_| {
                let mut p = PathBuf::from(&root);
                if !rel.is_empty() {
                    p.push(&rel);
                }
                p
            });
            let deploy = ctx.current_deploy_scope();
            let shadow = DirHandle::shadow(canon_path, cmode, deploy);
            let _ = ctx.handles.dir_handles.insert_at(fd.as_u64(), shadow).await;
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_ENTRIES_STREAM_OPEN_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsEntriesStreamOpenHandler as FsHandler>::NAME,
    arity: <FsEntriesStreamOpenHandler as FsHandler>::ARITY,
    verifying: <FsEntriesStreamOpenHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| {
        Box::pin(dispatch_via_trait_owned::<FsEntriesStreamOpenHandler>(
            fs, args,
        ))
    },
    urn_suffix: "entriesStreamOpen",
    fixed_channel: FixedChannels::fs_entries_stream_open,
    body_ref: BodyRefs::FS_ENTRIES_STREAM_OPEN,
    family: HandlerFamily::Stream,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(cmode: &str) -> Vec<Par> {
        vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir".to_string()),
            mk_cmode_par(cmode),
        ]
    }

    /// Trait-level constants wire through.  Non-verifying,
    /// ARITY = 4 (root + rel + cmode + ack).
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsEntriesStreamOpenHandler::NAME, "fs_entries_stream_open");
        assert_eq!(FsEntriesStreamOpenHandler::ARITY, 4);
        assert!(!FsEntriesStreamOpenHandler::VERIFYING);
    }

    /// `parse_content` success path with Oracular cmode.
    #[test]
    fn parse_content_accepts_oracular_tuple() {
        let parsed = FsEntriesStreamOpenHandler::parse_content(&valid_args("oracular"))
            .ok()
            .expect("oracular parses");
        assert_eq!(parsed.root, "/@bundle/example");
        assert_eq!(parsed.rel, "subdir");
        assert_eq!(parsed.cmode, ConsensusMode::Oracular);
    }

    /// LOAD-BEARING Phase-2 ban: Consensus cmode is REJECTED at
    /// parse_content with `FSERR_UNSUPPORTED` + the specific
    /// "readdir order is fs-dependent..." message pointing
    /// callers at `fs_entries` as the deterministic alternative.
    /// This is the entire security premise of the handler — a
    /// future refactor that accidentally accepted Consensus
    /// streams would produce silent cross-validator divergence
    /// through per-entry replies.
    #[test]
    fn parse_content_rejects_consensus_cmode() {
        let reply = *FsEntriesStreamOpenHandler::parse_content(&valid_args("consensus"))
            .err()
            .expect("consensus rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// LOAD-BEARING: bad cmode (non-canonical) produces the
    /// specific "cmode must be..." error, distinct from the
    /// Phase-2 UNSUPPORTED message.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args("oracular");
        args[2] = mk_cmode_par("Oracular"); // uppercase
        let reply = *FsEntriesStreamOpenHandler::parse_content(&args)
            .err()
            .expect("bad cmode rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// `parse_content` rejects wrong arg count (2 or 4+).  The
    /// parser requires exactly 3 pre-ack args.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let two = valid_args("oracular")
            .into_iter()
            .take(2)
            .collect::<Vec<_>>();
        let mut four = valid_args("oracular");
        four.push(RhoString::create_par("extra".to_string()));
        assert!(FsEntriesStreamOpenHandler::parse_content(&two).is_err());
        assert!(FsEntriesStreamOpenHandler::parse_content(&four).is_err());
    }

    /// `parse_content` rejects a non-String in root or rel.
    #[test]
    fn parse_content_rejects_non_string_root_or_rel() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..2 {
            let mut args = valid_args("oracular");
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsEntriesStreamOpenHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_entries_stream_open_cost`.
    /// Golden value pinned at the costs module (= 50, same as
    /// `fs_entries` setup).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsEntriesStreamOpenHandler::pre_charge_cost();
        let via_helper = fs_entries_stream_open_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
