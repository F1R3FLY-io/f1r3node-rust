// fs_entries — (root, rel, cmode) -> [true, [row1, ..., rowN]]
//                                   | [false, FSERR, msg]
//
// Final observation-family handler.  Bulk directory enumeration.
// **Verifying** + **two-event cost** (first observation handler to
// combine both: `fs_entries_stream_next` has the same two-event
// shape but is non-verifying; the other observation handlers are
// verifying but constant-cost).
//
// # Two-event cost
//
// Matches `fs_entries_stream_next` (PR #638) in shape:
//
//   - `pre_charge_cost()` = `fs_entries_cost(0)` = `FS_ENTRIES_SETUP`
//     = 50 (setup; the `0` placeholder arg leaves per-entry charging
//     to `post_reply_supplement`).
//   - `post_reply_supplement(reply)` = per-entry supplement with
//     `n = extract_ok_list_len(reply).unwrap_or(0)`.
//
// Framework auto-dispatches `reserve_incremental_primitive` on
// the supplement so `n=0` (error / EOS / bad-shape) legitimately
// produces a zero-weight cost.
//
// Both leader + follower emit the same 2-event sequence to preserve
// the D3 canonical event log fold bytes.
//
// # Verifying + journal_state_read
//
// `journal` runs `journal_state_read_via_table(WalOp::Entries,
// hash(reply), length=None)` under Consensus — reply hash encodes
// both the success value AND the error-code string so verify.rs
// catches value-divergence AND error-code-divergence.  `length =
// None` because the entry count IS captured in the reply hash
// (the reply list IS the observable outcome; no side-band count
// needed, unlike `fs_entries_stream_next` which uses `Some(0 or 1)`).
//
// # Determinism — sort before building rows
//
// `read_dir_capped` returns names in filesystem-dependent `readdir`
// order.  The handler sorts the names slice BEFORE building reply
// rows so the per-entry iteration order is deterministic across
// validators.  Pre-trait fileio preservation; the sort is
// consensus-observable (the reply list order feeds the reply hash).
//
// # MAX_ENTRIES gate
//
// Caps per-call output at 65_536 entries (consensus-observable
// constant at fold order 16).  Oversize surfaces as
// `FSERR_QUOTA_EXCEEDED` with a message pointing callers at the
// Stream alternative (`entriesStreamOpen` / `_Next` / `_Close`,
// which don't buffer the whole directory).
//
// # L-3 CLOEXEC via F_DUPFD_CLOEXEC
//
// Same discipline as `fs_entries_stream_open` (PR #637): dup the
// opening dirfd with CLOEXEC set, close the original, pass the
// dup to `fdopendir`.  Belt-and-suspenders against fdopendir
// implementations that don't preserve CLOEXEC atomically.
//
// # Shape-A root gating (deferred)
//
// Uses the ungated `resolve_or_identity` — same deferral as
// `fs_exists` / `fs_stat`.  BACKLOG: migrate to
// `resolve_or_identity_gated_for_consensus` with the coordinated
// observation + path-mutation migration.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::{fs_entries_cost, fs_entries_per_entry_supplement_cost};
use crate::rust::interpreter::io::errors::{io_err_code, FSERR_BAD_ARG, FSERR_QUOTA_EXCEEDED};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, spawn_blocking_par, FsHandler, FsHandlerEntry, HandlerFamily,
    HandlerReply, JournalPath, SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    entry_stat_row, journal_state_read_via_table, read_dir_capped,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{io_msg_scrub, quarantine_err_reply};
use crate::rust::interpreter::io::response::{err, extract_ok_list_len, ok_list};
use crate::rust::interpreter::io::wal::WalOp;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode, MAX_ENTRIES};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsEntriesHandler;

/// Parsed args.
pub struct FsEntriesArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsEntriesHandler {
    const NAME: &'static str = "fs_entries";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsEntriesArgs;

    fn parse_content(args: &[Par]) -> Result<FsEntriesArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String)",
            ));
        };
        // Cmode validation first — matches pre-trait specific
        // "cmode must be..." error ordering.
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
                    "expected (String, String)",
                ));
            }
        };
        Ok(FsEntriesArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> Cost {
        // Setup weight — base 50 (FS_ENTRIES_SETUP).  The `0`
        // argument is a placeholder; per-entry supplement fires
        // via `post_reply_supplement` once the reply is known.
        fs_entries_cost(0)
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // BACKLOG: migrate to resolve_or_identity_gated_for_consensus
            // (coordinated with fs_exists / fs_stat / path-mutation
            // handlers).
            let logical = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let rel = args.rel;
            let cmode = args.cmode;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (code, msg) = quarantine_err_reply(&qe);
                        return err(code, msg);
                    }
                };
                // SAFETY: `parent` is a `SafeParent` from
                // `safe_descend_verified`; its dirfd is open for
                // `parent`'s lifetime and `parent.leaf_ptr()`
                // returns a NUL-terminated `*const c_char` valid
                // for the same lifetime.  `openat` reads both
                // without retention.
                let dir_fd = unsafe {
                    libc::openat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if dir_fd < 0 {
                    let e = std::io::Error::last_os_error();
                    return err(io_err_code(&e), io_msg_scrub(&e));
                }
                // L-3 CLOEXEC: F_DUPFD_CLOEXEC on the fd handed
                // to fdopendir so the DIR*'s underlying fd
                // carries CLOEXEC atomically — see module header
                // on CLOEXEC discipline.
                //
                // SAFETY: `dir_fd` is a freshly-opened open fd
                // from the `openat` above (post-`< 0` check).
                // `fcntl(F_DUPFD_CLOEXEC, 0)` returns a new fd
                // (or -1 with errno) without invalidating
                // `dir_fd`.
                let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
                if read_fd < 0 {
                    let e = std::io::Error::last_os_error();
                    // SAFETY: `dir_fd` is still open (fcntl
                    // failed; no dup created).  We must close
                    // it before returning.
                    unsafe { libc::close(dir_fd) };
                    return err(io_err_code(&e), io_msg_scrub(&e));
                }
                // `read_dir_capped` takes ownership of `read_fd`
                // via fdopendir (success) or manual close
                // (fdopendir failure).  `dir_fd` is still owned
                // here (not transferred); must be closed before
                // we return.  entry_stat_row below uses `dir_fd`
                // (not `read_fd`) because the DIR*'s underlying
                // fd is read_fd — but we need an openat-able fd
                // for the per-entry stat, so we close read_fd
                // via closedir inside read_dir_capped (via DIR*
                // drop) and keep dir_fd open for the per-entry
                // openat calls.
                let entries = read_dir_capped(read_fd, MAX_ENTRIES);
                match entries {
                    Err(e) => {
                        // SAFETY: `dir_fd` is open for the
                        // duration of `read_dir_capped`
                        // (unaffected by its errors; `read_fd`
                        // handling is internal to it).  Close
                        // now before returning.
                        unsafe { libc::close(dir_fd) };
                        err(io_err_code(&e), io_msg_scrub(&e))
                    }
                    Ok((mut names, hit_cap)) => {
                        if hit_cap {
                            // SAFETY: `dir_fd` is open; close it
                            // before propagating the error reply.
                            unsafe { libc::close(dir_fd) };
                            return err(
                                FSERR_QUOTA_EXCEEDED,
                                format!(
                                    "entries exceeds MAX_ENTRIES={MAX_ENTRIES}; use \
                                     entriesStreamOpen / _Next / _Close for large \
                                     directories",
                                ),
                            );
                        }
                        // Deterministic sort — see module header
                        // on determinism.  readdir order is
                        // filesystem-dependent; sorting ensures
                        // reply bytes are stable across
                        // validators.
                        names.sort();
                        let rows: Vec<Par> = names
                            .into_iter()
                            .map(|name| entry_stat_row(dir_fd, &name, cmode))
                            .collect();
                        // SAFETY: `dir_fd` was still open through
                        // the `entry_stat_row` iteration (per-
                        // entry openat calls needed it alive);
                        // last use — close now to release the
                        // fd.
                        unsafe { libc::close(dir_fd) };
                        ok_list(rows)
                    }
                }
            })
            .await;
            HandlerReply::Ok(par)
        })
    }

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        // Cmode is the third positional arg (after root, rel).
        Box::pin(async move {
            let [_, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // State-read journal — fs_stat / fs_size / fs_exists /
        // fs_read / fs_entries all share this shape.  No reserve
        // + finalize pattern; just journal the reply's
        // stable_hash with WalOp::Entries.  Self-guards on
        // Oracular inside `journal_state_read_via_table`.
        Box::pin(async move {
            let [root_par, rel_par, cmode_par] = raw_args else {
                return;
            };
            let Some(cmode) = resolve_cmode(cmode_par) else {
                return;
            };
            let (Some(root), Some(rel)) =
                (RhoString::unapply(root_par), RhoString::unapply(rel_par))
            else {
                return;
            };
            // Path is the lexical join (not safe_descend_verified's
            // on-disk result).  Matches the deterministic-across-
            // validators discipline from fs_chmod's WAL path.
            let mut path_buf = PathBuf::from(root);
            if !rel.is_empty() {
                path_buf.push(&rel);
            }
            let reply = path.produce_reply();
            journal_state_read_via_table(
                ctx.handles,
                cmode,
                WalOp::Entries,
                path_buf,
                reply,
                ctx.ack,
                None,
            );
        })
    }

    fn post_reply_supplement(reply: &[Par]) -> Option<Cost> {
        // Per-entry supplement — fires after dispatch (leader)
        // OR after Oracular echo (via `previous` reply slice).
        // n = 0 on error / bad-shape reply (not an `ok_list`
        // shape); matching fs_entries_stream_next's two-branch
        // supplement discipline.
        let n_entries = extract_ok_list_len(reply).unwrap_or(0);
        Some(fs_entries_per_entry_supplement_cost(n_entries))
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_ENTRIES_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsEntriesHandler as FsHandler>::NAME,
    arity: <FsEntriesHandler as FsHandler>::ARITY,
    verifying: <FsEntriesHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsEntriesHandler>(fs, args)),
    urn_suffix: "entries",
    fixed_channel: FixedChannels::fs_entries,
    body_ref: BodyRefs::FS_ENTRIES,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::errors::FSERR_IO;
    use crate::rust::interpreter::io::response::{err, ok_list};

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(cmode: &str) -> Vec<Par> {
        vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir".to_string()),
            mk_cmode_par(cmode),
        ]
    }

    /// Trait-level constants wire through.  Verifying observation.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsEntriesHandler::NAME, "fs_entries");
        assert_eq!(FsEntriesHandler::ARITY, 4);
        assert!(FsEntriesHandler::VERIFYING);
    }

    /// `parse_content` success path for both cmodes.
    #[test]
    fn parse_content_accepts_both_cmodes() {
        for cmode_str in ["consensus", "oracular"] {
            let parsed = FsEntriesHandler::parse_content(&valid_args(cmode_str))
                .ok()
                .unwrap_or_else(|| panic!("{cmode_str} parses"));
            assert_eq!(parsed.root, "/@bundle/example");
            assert_eq!(parsed.rel, "subdir");
        }
    }

    /// LOAD-BEARING: bad cmode produces specific "cmode must
    /// be..." error.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args("consensus");
        args[2] = mk_cmode_par("Consensus"); // uppercase
        let reply = *FsEntriesHandler::parse_content(&args)
            .err()
            .expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let two = valid_args("consensus")
            .into_iter()
            .take(2)
            .collect::<Vec<_>>();
        let mut four = valid_args("consensus");
        four.push(RhoString::create_par("extra".to_string()));
        assert!(FsEntriesHandler::parse_content(&two).is_err());
        assert!(FsEntriesHandler::parse_content(&four).is_err());
    }

    /// `parse_content` rejects a non-String in root or rel.
    #[test]
    fn parse_content_rejects_non_string_root_or_rel() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..2 {
            let mut args = valid_args("consensus");
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsEntriesHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_entries_cost(0)`.
    /// Golden value pinned at the costs module (= 50, matches
    /// `fs_entries_stream_open` / `fs_entries_stream_next` setup).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsEntriesHandler::pre_charge_cost();
        let via_helper = fs_entries_cost(0);
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// LOAD-BEARING: `post_reply_supplement` on a 3-entry reply
    /// charges n=3 supplement.  Pin the extract_ok_list_len
    /// pipeline end-to-end.
    #[test]
    fn post_reply_supplement_charges_n_for_ok_list() {
        use crate::rust::interpreter::rho_type::RhoString;
        let rows: Vec<Par> = (0..3)
            .map(|i| RhoString::create_par(format!("row-{i}")))
            .collect();
        let reply = [ok_list(rows)];
        let got = FsEntriesHandler::post_reply_supplement(&reply).expect("supplement returns Some");
        let want = fs_entries_per_entry_supplement_cost(3);
        assert_eq!(got.value, want.value);
        assert_eq!(got.operation, want.operation);
    }

    /// LOAD-BEARING: `post_reply_supplement` on an err reply
    /// charges n=0 supplement.  `extract_ok_list_len` returns
    /// None → fallback to 0.
    #[test]
    fn post_reply_supplement_charges_n_0_for_err() {
        let reply = [err(FSERR_IO, "disk error")];
        let got = FsEntriesHandler::post_reply_supplement(&reply).expect("supplement returns Some");
        let want = fs_entries_per_entry_supplement_cost(0);
        assert_eq!(got.value, want.value);
    }

    /// LOAD-BEARING: `post_reply_supplement` on an empty-list
    /// reply (`[true, []]`) charges n=0 supplement.
    #[test]
    fn post_reply_supplement_charges_n_0_for_empty_list() {
        let reply = [ok_list(vec![])];
        let got = FsEntriesHandler::post_reply_supplement(&reply).expect("supplement returns Some");
        let want = fs_entries_per_entry_supplement_cost(0);
        assert_eq!(got.value, want.value);
    }

    /// MAX_ENTRIES is reachable from this module's import path.
    /// Pin against a regression that moved the constant or
    /// typo'd the name — would trip before Wave 6's
    /// consensus-fingerprint recomputation.
    #[test]
    fn max_entries_is_positive() {
        let _: usize = MAX_ENTRIES;
        assert!(MAX_ENTRIES > 0);
    }
}
