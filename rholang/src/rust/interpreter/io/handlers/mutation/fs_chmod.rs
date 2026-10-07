// fs_chmod — (root, rel, bits, cmode) -> [true] | [false, FSERR, msg]
//
// First path-mutation handler on dev.  Verifying, constant cost.
// Cmode arg-based (not fd-based — path mutations don't have a
// shadow handle to look up).
//
// # Reserve + finalize (H-6 pattern)
//
//   - `pre_syscall`: reserve a `WalOp::Chmod` entry (with
//     `mode_bits = Some(args.bits)`) under Consensus; Oracular
//     self-guard inside `journal_path_mutation_single_via_table`.
//     Lexical quarantine failure (`canonicalize_lexical` returns
//     Err — e.g., parent-dir reference) is a deterministic no-op
//     — `canonicalize_lexical` is a path-string operation (no
//     filesystem access), so leader + follower see the same
//     outcome and both skip the reserve; the dispatch body then
//     produces the quarantine err reply.
//   - `dispatch`: resolve logical root → `safe_descend_verified` →
//     `fchmodat(dirfd, leaf, bits, AT_SYMLINK_NOFOLLOW)`.
//     ENOTSUP / EOPNOTSUPP from `AT_SYMLINK_NOFOLLOW` on filesystems
//     that don't honor the flag maps to `FSERR_UNSUPPORTED`.
//   - `journal`: on error / verify-divergence, patch via
//     `finalize_failure_journal_via_table` (same path as fs_truncate).
//
// # Shape-A root gating (deferred)
//
// Uses the ungated `root_registry.resolve_or_identity` — same as
// `fs_exists` / `fs_stat` on dev.  The gated version
// (`resolve_or_identity_gated_for_consensus`) that rejects
// Consensus caps against unregistered logical roots is a deferred
// follow-up (BACKLOG — needs `test_permissive` + registry-gating
// support in `path/identity.rs`).  Once ported, this handler + the
// two observation siblings migrate together to the gated API.
//
// # No telemetry hazard
//
// Uses raw `tokio::task::spawn_blocking` returning
// `Result<Par, Par>` where `Ok(par)` is the success Par and
// `Err(par)` is the error Par.  The outer `match` discriminates
// into `HandlerReply::ok` / `HandlerReply::err`, so
// `HandlerReply::is_ok()` correctly reflects the handler's
// semantic outcome.  Follows the pattern established by
// `fs_truncate` (slice 4.24) and avoids the
// `spawn_blocking_par(FnOnce() -> Par) -> HandlerReply::ok(par)`
// chain that `fs_quarantine` / `fs_exists` / `fs_stat` inherit.
//
// # fchmodat flag choice
//
// `AT_SYMLINK_NOFOLLOW` prevents a symlink-leaf chmod from
// following to the target.  Combined with `safe_descend_verified`
// (which enforces O_NOFOLLOW at every intermediate component),
// the leaf-level NOFOLLOW keeps the H-5 defense complete across
// the full path.  On filesystems that don't honor AT_SYMLINK_NOFOLLOW
// (Linux + ext4 is the primary case), the kernel returns ENOTSUP
// — we surface that as `FSERR_UNSUPPORTED` rather than silently
// following, matching pre-trait behavior.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_chmod_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CODE_CONSENSUS_DIVERGENCE,
    FSERR_QUOTA_EXCEEDED, FSERR_UNSUPPORTED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_path_mutation_single_via_table,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, extract_err_code, ok_bare};
use crate::rust::interpreter::io::wal::WalOp;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsChmodHandler;

/// Parsed args for [`FsChmodHandler`].  Bits range-checked to
/// `0..=0o7777` (12 bits — the POSIX mode space including suid /
/// sgid / sticky).  Narrower-than-u32 would reject legitimate
/// sticky-bit setups; wider would accept bits the chmod syscall
/// silently ignores on Linux (making consensus replay compare the
/// post-mask bits across follower kernels — risky).
pub struct FsChmodArgs {
    root: String,
    rel: String,
    bits: u32,
    cmode: ConsensusMode,
}

impl FsHandler for FsChmodHandler {
    const NAME: &'static str = "fs_chmod";
    const ARITY: usize = 5; // (root, rel, bits, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsChmodArgs;

    fn parse_content(args: &[Par]) -> Result<FsChmodArgs, Box<HandlerReply>> {
        let [root_par, rel_par, mode_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, u64<=0o7777, String)",
            ));
        };
        // Cmode validation first — matches pre-trait specific
        // "cmode must be..." error message, which is distinct from
        // the combined "expected (String, String, u64<=0o7777,
        // String)" error on content shape.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        // Combined args parse — pre-trait emits the combined
        // "expected (String, String, u64<=0o7777, String)" on any
        // single failure (string unapply fail OR bits-range fail).
        let combined_err = || {
            HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, u64<=0o7777, String)",
            )
        };
        let root = RhoString::unapply(root_par).ok_or_else(combined_err)?;
        let rel = RhoString::unapply(rel_par).ok_or_else(combined_err)?;
        let bits = RhoNumber::unapply(mode_par).ok_or_else(combined_err)?;
        if !(0..=0o7777).contains(&bits) {
            return Err(combined_err());
        }
        Ok(FsChmodArgs {
            root,
            rel,
            bits: bits as u32,
            cmode,
        })
    }

    fn pre_charge_cost() -> Cost { fs_chmod_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsChmodArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            // Lexical quarantine: `canonicalize_lexical` is
            // path-string-only (no filesystem access), so leader +
            // follower see the same outcome.  Err path = skip
            // reserve; dispatch produces the quarantine reply.
            let canon_path = match canonicalize_lexical(&args.root, &args.rel) {
                Ok(p) => p,
                Err(_) => return Ok(()),
            };
            if journal_path_mutation_single_via_table(
                ctx.handles,
                args.cmode,
                WalOp::Chmod,
                canon_path,
                Some(args.bits),
                None,
                None,
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
        args: FsChmodArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // BACKLOG: migrate to resolve_or_identity_gated_for_consensus
            // once path/identity ports the Shape-A gating.  Dev's
            // sibling fs_exists / fs_stat carry the same note.
            let logical = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let rel = args.rel;
            let bits = args.bits as libc::mode_t;
            // Direct `spawn_blocking` with `Result<Par, Par>` so
            // the outer match discriminates HandlerReply::ok /
            // HandlerReply::err cleanly — see module header on
            // "No telemetry hazard".  `clippy::result_large_err`
            // triggers because `Par` is ~248 bytes; boxing the Err
            // variant would add an allocation per failed syscall
            // without improving the hazard-avoidance semantics.
            // The Result flows one level up into a match, so the
            // "large Err" lives on the stack for one frame.
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
                // `safe_descend_verified`; the enclosed dirfd is
                // open for the lifetime of `parent` (RAII), and
                // `parent.leaf_ptr()` returns a NUL-terminated
                // `*const c_char` valid for the same lifetime.
                // `fchmodat` reads both without retaining either
                // past the call.
                let rc = unsafe {
                    libc::fchmodat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        bits,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if rc == 0 {
                    Ok(ok_bare())
                } else {
                    let e = std::io::Error::last_os_error();
                    // ENOTSUP / EOPNOTSUPP → FSERR_UNSUPPORTED
                    // (Linux + some fs don't honor AT_SYMLINK_NOFOLLOW
                    // on chmod; report UNSUPPORTED rather than
                    // silently following).
                    let code = if e.raw_os_error() == Some(libc::ENOTSUP)
                        || e.raw_os_error() == Some(libc::EOPNOTSUPP)
                    {
                        FSERR_UNSUPPORTED
                    } else {
                        io_err_code(&e)
                    };
                    Err(err(code, io_msg_scrub(&e)))
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

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        // Cmode is the fourth positional arg (after root, rel,
        // bits).  Standard arg-based pattern — path mutations
        // don't have a shadow fd to look it up on.
        Box::pin(async move {
            let [_, _, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Same H-6 finalize shape as fs_truncate.
        Box::pin(async move {
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                let reply = path.produce_reply();
                if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                    finalize_failure_journal_via_table(
                        ctx.handles,
                        fserr_to_code(&code_str),
                        ctx.ack,
                    );
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_CHMOD_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsChmodHandler as FsHandler>::NAME,
    arity: <FsChmodHandler as FsHandler>::ARITY,
    verifying: <FsChmodHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsChmodHandler>(fs, args)),
    urn_suffix: "chmod",
    fixed_channel: FixedChannels::fs_chmod,
    body_ref: BodyRefs::FS_CHMOD,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    /// Trait-level constants wire through.  Second Mutation-family
    /// handler, path-based (ARITY = 5 = root + rel + bits + cmode
    /// + ack).  VERIFYING = true.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsChmodHandler::NAME, "fs_chmod");
        assert_eq!(FsChmodHandler::ARITY, 5);
        assert!(FsChmodHandler::VERIFYING);
    }

    /// `parse_content` success path — standard (root, rel, bits,
    /// cmode) tuple with bits inside the 12-bit POSIX mode space.
    #[test]
    fn parse_content_accepts_valid_tuple() {
        let args = vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir/file".to_string()),
            RhoNumber::create_par(0o644),
            mk_cmode_par("consensus"),
        ];
        let parsed = FsChmodHandler::parse_content(&args)
            .ok()
            .expect("valid tuple parses");
        assert_eq!(parsed.root, "/@bundle/example");
        assert_eq!(parsed.rel, "subdir/file");
        assert_eq!(parsed.bits, 0o644);
        assert_eq!(parsed.cmode, ConsensusMode::Consensus);
    }

    /// Oracular cmode roundtrips.
    #[test]
    fn parse_content_accepts_oracular_cmode() {
        let args = vec![
            RhoString::create_par("/some/root".to_string()),
            RhoString::create_par("f".to_string()),
            RhoNumber::create_par(0o600),
            mk_cmode_par("oracular"),
        ];
        let parsed = FsChmodHandler::parse_content(&args)
            .ok()
            .expect("oracular parses");
        assert_eq!(parsed.cmode, ConsensusMode::Oracular);
    }

    /// LOAD-BEARING: a wrong cmode string (case, extra chars)
    /// produces the specific "cmode must be..." error, distinct
    /// from the combined "expected (...)" error.  Pre-trait
    /// callers depend on this discrimination.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let args = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("f".to_string()),
            RhoNumber::create_par(0o644),
            RhoString::create_par("Consensus".to_string()), // uppercase
        ];
        let reply = *FsChmodHandler::parse_content(&args).err().expect("rejects");
        // The reply is HandlerReply::Err(..) — a Par encoding
        // [false, FSERR_BAD_ARG, "cmode must be...".
        // Pin message presence by matching the Rust-side error
        // variant structure.  Exact-string match is reserved for
        // the dispatcher's wire output.
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// LOAD-BEARING: bits = 0o10000 (above 12-bit range) rejects.
    /// Prevents accepting values the chmod syscall silently masks,
    /// which would make consensus replay compare post-mask bits.
    #[test]
    fn parse_content_rejects_bits_above_12bit_range() {
        let args = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("f".to_string()),
            RhoNumber::create_par(0o10000),
            mk_cmode_par("consensus"),
        ];
        assert!(FsChmodHandler::parse_content(&args).is_err());
    }

    /// LOAD-BEARING: bits = -1 rejects.  RhoNumber::unapply returns
    /// i64; `(0..=0o7777).contains(&-1)` is false.  Prevents a
    /// negative value from reinterpreting through `as u32` into
    /// a huge positive.
    #[test]
    fn parse_content_rejects_negative_bits() {
        let args = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("f".to_string()),
            RhoNumber::create_par(-1),
            mk_cmode_par("consensus"),
        ];
        assert!(FsChmodHandler::parse_content(&args).is_err());
    }

    /// Boundary: bits = 0 (all permissions cleared) and bits =
    /// 0o7777 (max 12-bit value) both parse.
    #[test]
    fn parse_content_accepts_bits_boundaries() {
        for bits in [0, 0o7777] {
            let args = vec![
                RhoString::create_par("r".to_string()),
                RhoString::create_par("f".to_string()),
                RhoNumber::create_par(bits),
                mk_cmode_par("consensus"),
            ];
            let parsed = FsChmodHandler::parse_content(&args)
                .ok()
                .expect("boundary parses");
            assert_eq!(parsed.bits, bits as u32);
        }
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let three = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("f".to_string()),
            RhoNumber::create_par(0o644),
        ];
        let five = vec![
            RhoString::create_par("r".to_string()),
            RhoString::create_par("f".to_string()),
            RhoNumber::create_par(0o644),
            mk_cmode_par("consensus"),
            RhoString::create_par("extra".to_string()),
        ];
        assert!(FsChmodHandler::parse_content(&three).is_err());
        assert!(FsChmodHandler::parse_content(&five).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_chmod_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsChmodHandler::pre_charge_cost();
        let via_helper = fs_chmod_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
