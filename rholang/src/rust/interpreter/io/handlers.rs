// The 22 native filesystem handlers.
//
// Each handler:
//   1. Unapplies the incoming contract call to extract
//      `(produce, is_replay, previous_output, args)`.
//   2. On replay, immediately re-sends `previous_output` — filesystem
//      calls are non-deterministic and must not be re-issued.  Consistent
//      with `gpt4`/`dalle3`/`ollama_chat`: no cost charged on the replay
//      branch (cost already accounted at capture time by the leader).
//   3. Otherwise validates arguments, quarantines any path via
//      `path::safe_descend`, dispatches to the syscall in a
//      `spawn_blocking` task (so long-blocking `fsync`/`copy` never
//      stalls the reactor), and builds the `[true, ...]` /
//      `[false, code, msg]` reply.
//
// Path safety: every path-taking handler takes `(rootCanon, rel)` and
// descends via `openat + O_NOFOLLOW` at each step.  The leaf operation
// is issued as an `*at` syscall against the resolved parent dirfd, so
// the resolution path used for the safety check is the exact same path
// used for the operation — TOCTOU-immune.
//
// Error messages are scrubbed via `io_msg_scrub` — we surface the
// `std::io::ErrorKind` classification but not the free-form message
// (which on some platforms includes the offending path, leaking the
// caller's root prefix).

use std::path::PathBuf;

use models::rhoapi::{ListParWithRandom, Par};
use tokio::task::spawn_blocking;

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::rho_runtime::RhoISpace;
use super::super::rho_type::{RhoBoolean, RhoByteArray, RhoNumber, RhoString};
use super::errors::*;
use super::handle_table::{FileHandle, FileHandleTable};
// C-R1 review fix: `extract_ok_u64` is used from fs_open's is_replay
// branch to reconstruct the leader's returned fd for shadow-handle
// insertion.
use super::lock::{DeployScope, HolderId, LockError, LockId, LockMode};
use super::mode::{fopen_flags, parse_open_mode, AccessMode};
use super::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply, safe_descend_verified, safe_open,
    SafeParent,
};
use super::response::*;
use super::stat::{error_record, stat_record};
use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
use super::{ConsensusMode, CMODE_CONSENSUS_STR, CMODE_ORACULAR_STR};

/// Slice 30c H-R3 integration: compute the ack channel's Blake2b256
/// hash the same way rspace computes `channel_hash` for a produce
/// event.  The result is the sidecar key on `Wal::append_with_ack`;
/// the same hash appears in the deploy_log's `ProduceEvent::channels_hash`
/// when the handler publishes its reply, so the log-order drain
/// can match them.
fn ack_channel_hash(ack: &Par) -> [u8; 32] {
    let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(ack).bytes();
    // M-9 fix (2026-08-06): fail-hard in release, not just debug.
    // Pre-fix used `debug_assert_eq!` + `.min(32)` which would
    // silently zero-pad in release if a future Blake2b256
    // provider swap produced shorter output — hash collisions on
    // the placeholder sentinel `[0u8; 32]` would misroute
    // log-order drain.  Panicking loudly at the mismatch site
    // surfaces the misconfiguration at first call rather than
    // as a downstream consensus divergence.
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — the WAL ack sidecar \
         hard-depends on a fixed 32-byte hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Cap on `fs_entries` output size — prevents a malicious caller pointing
/// the native at a million-entry directory and OOMing the node.
pub const MAX_ENTRIES: usize = 65_536;

/// Cap on `fs_write` payload — symmetric with `MAX_READ_BYTES`.
pub const MAX_WRITE_BYTES: u64 = 64 * 1024 * 1024;

/// Shared per-runtime state for the fs native handlers.  Cloned into
/// each handler closure via `ProcessContext`.
#[derive(Clone)]
pub struct FsProcesses {
    pub dispatcher: RhoDispatch,
    pub space: RhoISpace,
    pub handles: FileHandleTable,
    pub mode: ConsensusMode,
}

impl FsProcesses {
    pub fn new(
        dispatcher: RhoDispatch,
        space: RhoISpace,
        handles: FileHandleTable,
        mode: ConsensusMode,
    ) -> Self {
        FsProcesses {
            dispatcher,
            space,
            handles,
            mode,
        }
    }

    fn is_contract_call(&self) -> ContractCall {
        ContractCall {
            space: self.space.clone(),
            dispatcher: self.dispatcher.clone(),
        }
    }

    /// Redesign helper: journal a Write / WriteAt to the WAL from
    /// data fully derivable from args (fd + bytes + offset).  Called
    /// from `fs_write` and `fs_write_at` BEFORE the `is_replay`
    /// short-circuit so leader and follower populate identical WALs
    /// (C-29-F1 review fix).
    ///
    /// Returns:
    ///   * `Ok(true)`  — fd is a Consensus cap and entry was appended.
    ///   * `Ok(false)` — fd is not Consensus (Oracular or unknown), no-op.
    ///   * `Err(())`   — WAL is at cap (`MAX_WAL_ENTRIES`); caller must
    ///     translate to `FSERR_QUOTA_EXCEEDED` and NOT proceed with the
    ///     syscall so leader/follower stay symmetric (both hit the same
    ///     cap moment).
    ///
    /// Note on partial writes (M-29-3 trade-off): we record the
    /// REQUESTED byte length + a hash of the REQUESTED payload.  On a
    /// partial-write the actual on-disk state is n<len; the FIP
    /// documents this as a caller-responsibility retry pattern.
    /// Recording requested-bytes keeps the WAL fully derivable from
    /// contract args, which is what makes leader/follower symmetric
    /// on the `is_replay` short-circuit path (the follower does NOT
    /// re-issue the syscall and therefore does not know `n`).
    async fn journal_write(
        &self,
        fd: u64,
        bytes: &[u8],
        offset: Option<u64>,
        ack: &Par,
    ) -> Result<bool, ()> {
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path)) => {
                let op = if offset.is_some() {
                    WalOp::WriteAt
                } else {
                    WalOp::Write
                };
                self.handles
                    .wal
                    .append_with_ack(
                        WalEntry {
                            op,
                            path: canon_path,
                            extra_path: None,
                            offset,
                            length: Some(bytes.len() as u64),
                            payload_ref: Some(PayloadRef::hash(bytes)),
                            mode_bits: None,
                            owner: None,
                            group: None,
                            // H-6 fix (2026-08-06): optimistic
                            // Success placeholder; the leader's
                            // finalize_failure_journal below
                            // updates to Failure on syscall error.
                            outcome: WalOutcome::Success,
                        },
                        ack_channel_hash(ack),
                    )
                    .map(|()| true)
            }
            _ => Ok(false),
        }
    }

    /// Slice 32 (PB-M-14 read-hash): journal a Read/ReadAt to the WAL.
    /// Called AFTER a successful read (leader path) OR from the
    /// `is_replay` branch after extracting the cached bytes (follower
    /// path) — both sides append the SAME entry (same op, path,
    /// offset, length, and `Hash(bytes)` payload) so the WAL is
    /// byte-identical across leader and follower.
    ///
    /// The hash is over the RETURNED bytes (post-truncate to the
    /// actual read length), not the requested length — mirrors how
    /// fs_read's reply carries `ok_bytes(bytes)` with the actual
    /// truncated length after `buf.truncate(got as usize)`.
    ///
    /// Under PB-M-14 semantics, a joining validator replaying the
    /// deploy against reconstructed state must observe the same
    /// bytes on `fs_read`.  A mismatch (hash of freshly-read bytes
    /// != WAL entry's hash) indicates disk state divergence between
    /// leader and follower — the read-verify path (implemented
    /// symmetrically via `journal_read` on both sides) catches this
    /// at WAL-root-comparison time rather than as a silent tuplespace
    /// fork downstream.
    async fn journal_read(
        &self,
        fd: u64,
        bytes: &[u8],
        offset: Option<u64>,
        ack: &Par,
    ) -> Result<bool, ()> {
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path)) => {
                let op = if offset.is_some() {
                    WalOp::ReadAt
                } else {
                    WalOp::Read
                };
                self.handles
                    .wal
                    .append_with_ack(
                        WalEntry {
                            op,
                            path: canon_path,
                            extra_path: None,
                            offset,
                            length: Some(bytes.len() as u64),
                            payload_ref: Some(PayloadRef::hash(bytes)),
                            mode_bits: None,
                            owner: None,
                            group: None,
                            // Reads are journaled AFTER a successful
                            // syscall (see docstring above); the
                            // outcome is always Success.  Failed
                            // reads short-circuit before this call.
                            outcome: WalOutcome::Success,
                        },
                        ack_channel_hash(ack),
                    )
                    .map(|()| true)
            }
            _ => Ok(false),
        }
    }

    /// Redesign helper: journal a Truncate to the WAL from data
    /// fully derivable from args (fd + n).  Called from `fs_truncate`
    /// BEFORE the `is_replay` short-circuit (C-29-F1 review fix).
    /// Return semantics identical to `journal_write`.
    /// Slice 30c M-29-3 fix: finalize a previously-reserved write
    /// WAL entry with the ACTUAL bytes written.  Called by
    /// `fs_write` / `fs_write_at` on partial writes (n < requested).
    ///
    /// Semantics:
    /// - Locates the entry with matching ack_hash (the placeholder
    ///   appended by `journal_write` pre-syscall).
    /// - Updates only `length` and `payload_ref` — preserves
    ///   `op`, `path`, `offset`, `outcome` from the placeholder.
    /// - On leader: `n` comes from the syscall reply.
    /// - On follower: `n` comes from the cached `previous` reply.
    /// Both sides derive the same `n` from `bytes` (same args, same
    /// deterministic reply) and produce a byte-identical final entry.
    ///
    /// M-7 fix (2026-08-06): removed the fd-relookup path that
    /// silently no-op'd if the fd was closed between
    /// `journal_write` and this call.  The placeholder is keyed
    /// by `ack_hash` (a fresh unforgeable, unique per syscall)
    /// which cannot be aliased away — the placeholder was just
    /// appended in the same handler.  The WAL method
    /// (`update_partial_write_by_ack_hash`) is a no-op if no
    /// entry matches, which is the correct behavior for
    /// non-Consensus caps (they never appended a placeholder).
    ///
    /// Full-length writes (n == requested) don't call this — the
    /// pre-syscall placeholder already has the correct content.
    /// Failed writes (error reply) go through
    /// `finalize_failure_journal` instead (H-6).
    fn finalize_write_journal(&self, requested_bytes: &[u8], actual_n: u64, ack: &Par) {
        let n = (actual_n as usize).min(requested_bytes.len());
        let actual_slice = &requested_bytes[..n];
        let _ = self
            .handles
            .wal
            .update_partial_write_by_ack_hash(ack_channel_hash(ack), actual_slice);
    }

    /// H-6 fix (2026-08-06): flip a reserved WAL entry's outcome
    /// to `Failure { code }` when the leader's syscall reply
    /// carries an error.  Symmetric across leader (`code` from
    /// the error reply the syscall just returned) and follower
    /// (`code` extracted from the cached `previous` reply).
    ///
    /// All other fields (op, path, offset, length, payload_ref)
    /// are preserved so replay consumers can see WHAT the leader
    /// asked for and WHY they should skip it.
    ///
    /// Followers reading a `Failure` entry MUST NOT apply the
    /// mutation to reconstructed state — the leader never wrote
    /// anything, so the follower's reconstructed state stays
    /// consistent by also not writing.
    ///
    /// No-op if no entry matches `ack_hash` (e.g., the syscall
    /// was on a non-Consensus cap, so `journal_write` /
    /// `journal_truncate` returned early with no reservation).
    fn finalize_failure_journal(&self, code: u32, ack: &Par) {
        let _ = self
            .handles
            .wal
            .update_outcome_by_ack_hash(ack_channel_hash(ack), WalOutcome::Failure { code });
    }

    async fn journal_truncate(&self, fd: u64, n: u64, ack: &Par) -> Result<bool, ()> {
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path)) => self
                .handles
                .wal
                .append_with_ack(
                    WalEntry {
                        op: WalOp::Truncate,
                        path: canon_path,
                        extra_path: None,
                        offset: Some(n),
                        length: None,
                        payload_ref: None,
                        mode_bits: None,
                        owner: None,
                        group: None,
                        // H-6: optimistic placeholder; the
                        // leader's finalize_failure_journal
                        // updates to Failure on syscall error.
                        outcome: WalOutcome::Success,
                    },
                    ack_channel_hash(ack),
                )
                .map(|()| true),
            _ => Ok(false),
        }
    }

    /// M-5 fix (2026-08-06): journal a state-read reply on a
    /// Consensus cap.  Called AFTER the syscall completes (both
    /// leader and follower paths — the follower extracts the
    /// same reply from the cached `previous`, hashes it, and
    /// journals identical bytes).
    ///
    /// - `op`: `WalOp::Stat` / `WalOp::Entries` / `WalOp::Size`.
    /// - `path`: the canonical target (root + rel joined for
    ///   path-based ops; canon_path from FileHandle for fd-based).
    /// - `reply`: the just-produced reply Par.  Hashed via
    ///   `stable_hash_provider::hash` for a canonical Blake2b256.
    /// - Outcome derived from the reply: `[true, ...]` → Success,
    ///   `[false, code, ...]` → Failure { code = fserr_to_code(code) }.
    ///
    /// A no-op if `cmode != Consensus`.  fs_stat / fs_entries
    /// take cmode as an arg; fs_size looks up the FileHandle's
    /// cmode via the fd.  fs_exists deliberately excluded —
    /// its arity (3) has no cmode signal; a follow-up would
    /// require an arity bump.
    fn journal_state_read(
        &self,
        cmode: ConsensusMode,
        op: WalOp,
        path: std::path::PathBuf,
        reply: &Par,
        ack: &Par,
        length: Option<u64>,
    ) {
        if cmode != ConsensusMode::Consensus {
            return;
        }
        let reply_hash: [u8; 32] = {
            let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(reply).bytes();
            assert_eq!(
                h.len(),
                32,
                "M-5: stable_hash must produce a 32-byte Blake2b256"
            );
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&h);
            buf
        };
        let outcome = if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
            WalOutcome::Failure {
                code: fserr_to_code(&code_str),
            }
        } else {
            WalOutcome::Success
        };
        let _ = self.handles.wal.append_with_ack(
            WalEntry {
                op,
                path,
                extra_path: None,
                offset: None,
                length,
                payload_ref: Some(PayloadRef::Hash(reply_hash)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome,
            },
            ack_channel_hash(ack),
        );
    }

    // -------------------------------------------------------------------
    // open — (rootCanon, rel, mode) -> [true, fd] | [false, code, msg]
    // -------------------------------------------------------------------
    pub async fn fs_open(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_open"));
        };
        // Slice 29 (PB-M-14): `(root, rel, mode, cmode, ack)` — the
        // cmode arg is stashed in the returned `FileHandle` so
        // subsequent mutating handlers can journal to the consensus
        // WAL when the cap is `Consensus`.  Same fail-closed
        // semantics as the slice-26 cmode threading: invalid cmode
        // → `FSERR_BAD_ARG`.
        let [root_par, rel_par, mode_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_open"));
        };
        if is_replay {
            // C-R1 review fix (slice 29 round 2): the leader's fs_open
            // populates a FileHandle with (cmode, canon_path); on the
            // follower we must populate a *shadow* handle at the same
            // fd so subsequent replay-branch mutating handlers'
            // `journal_write` / `journal_truncate` can look up
            // `(cmode, canon_path)` and append an identical WAL entry.
            // Pre-fix: follower's fs_open short-circuited without
            // inserting into the fd table → follower's journal_write
            // no-op'd on unknown fd → leader/follower WAL divergence.
            if let Some(fd) = extract_ok_u64(&previous) {
                // Only Consensus caps need shadow handles for
                // journaling — Oracular writes don't append to the
                // WAL either way.  But we insert regardless so any
                // future handler consulting FileHandle metadata on
                // the replay branch sees consistent state.
                if let (Some(root), Some(rel), Some(mode_str)) = (
                    RhoString::unapply(root_par),
                    RhoString::unapply(rel_par),
                    RhoString::unapply(mode_par),
                ) {
                    // resolve_cmode: fail-closed on bogus values, but a
                    // bogus cmode with a `[true, fd]` cached reply is
                    // definitionally a bug (leader wouldn't have
                    // returned success on a rejected cmode).  Fall
                    // back to `Consensus` (most restrictive) rather
                    // than skip the insert so any divergence surfaces
                    // via WAL comparison rather than silently masking.
                    let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
                    let intent = parse_open_mode(&mode_str);
                    // Same canon_path derivation as `open_impl`
                    // (C-29-1 fix) — must be byte-identical so WAL
                    // paths match across leader/follower.
                    let shadow = FileHandle {
                        file: None,
                        // M-R2: same lexical normalization as the leader's
                        // open_impl so follower's WAL entries match.
                        canon_path: canonicalize_lexical(&root, &rel),
                        mode: intent.map(|i| i.mode).unwrap_or(AccessMode::Read),
                        cmode,
                    };
                    // Ignore the return value: if the slot is already
                    // occupied (shouldn't happen on a fresh follower),
                    // the pre-existing handle wins.  Any real
                    // divergence surfaces later via WAL mismatch.
                    let _ = self.handles.insert_at(fd, shadow).await;
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoString::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(mode_str)) => {
                self.open_impl(root, rel, mode_str, cmode).await
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn open_impl(
        &self,
        root: String,
        rel: String,
        mode: String,
        cmode: ConsensusMode,
    ) -> Par {
        let intent = match parse_open_mode(&mode) {
            Some(i) => i,
            None => return err(FSERR_BAD_ARG, format!("unknown fopen mode {mode:?}")),
        };
        let root_pb = PathBuf::from(&root);
        let intent_copy = intent;
        // C-29-1 review fix: keep `rel` accessible for canon_path
        // construction below.  Clone into the blocking closure and
        // retain the original for later use.
        let rel_for_open = rel.clone();
        // openat descent + safe_open in a blocking task — sync fs.
        let opened = spawn_blocking(move || {
            let (flags, mode_bits) = fopen_flags(intent_copy);
            super::path::safe_open(&root_pb, &rel_for_open, flags, mode_bits)
        })
        .await;
        let file = match opened {
            Err(_join_err) => return err(FSERR_IO, "spawn_blocking task failed"),
            Ok(Err(qe)) => {
                let (code, msg) = quarantine_err_reply(&qe);
                return err(code, msg);
            }
            Ok(Ok(f)) => f,
        };
        // Reject non-regular files via fstat on the opened fd.  Because
        // we already have the fd (opened with O_NOFOLLOW), there's no
        // TOCTOU here.
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(e) => return err(io_err_code(&e), io_msg_scrub(&e)),
        };
        if !meta.file_type().is_file() {
            return err(FSERR_UNSUPPORTED, "not a regular file");
        }
        // C-29-1 review fix: include the resolved `rel` in the
        // canonical path so WAL entries can distinguish files under
        // the same canonRoot.  Pre-fix the `.join("")` no-op dropped
        // `rel` entirely, causing every WAL entry to record only the
        // canonRoot — replay had no way to tell which file to apply
        // the payload to.
        let handle = FileHandle {
            file: Some(file),
            // M-R2 round-2 fix: lexically normalize so `a/b.txt` and
            // `./a/b.txt` produce byte-identical canon_paths, keeping
            // WAL entries stable across equivalent rel forms.
            canon_path: canonicalize_lexical(&root, &rel),
            mode: intent.mode,
            cmode,
        };
        match self.handles.insert(handle).await {
            Ok(fd) => ok_u64(fd),
            Err(()) => err(FSERR_QUOTA_EXCEEDED, "per-runtime fd cap reached"),
        }
    }

    // -------------------------------------------------------------------
    // close — (fd) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_close(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_close"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_close"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                // Slice 28 (post-2026-08-06 CRIT-2 fix): fds are
                // hash-derived u64 bit-patterns; the sign bit
                // carries information, so we reinterpret the GInt
                // via `fd as u64` rather than gating on `fd >= 0`.
                self.handles.remove(fd as u64).await;
                ok_bare()
            }
            _ => err(FSERR_BAD_ARG, "expected GInt fd"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // read — (fd, n) -> [true, bytes]
    //
    // Slice 32 (PB-M-14 read-hash): on Consensus caps, `journal_read`
    // appends a `Read` WAL entry whose `payload_ref = Hash(bytes)`
    // AFTER the leader's syscall completes successfully.  The
    // `is_replay = true` follower branch symmetrically re-extracts
    // the bytes from `previous` and appends the SAME entry, so the
    // per-deploy WAL is byte-identical across leader and follower.
    // The follower does NOT re-execute the syscall — the tuplespace
    // `previous` cache already supplies the correct return value.
    // Non-Consensus caps skip journaling on both sides.
    // -------------------------------------------------------------------
    pub async fn fs_read(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_read"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_read"));
        };
        if is_replay {
            // Follower: hash the leader's cached bytes and append a
            // matching WAL entry.  Fd is derivable from the arg;
            // journal_read is a no-op on non-Consensus caps and on
            // reply shapes without a bytes payload (error replies).
            if let (Some(fd), Some(bytes)) =
                (RhoNumber::unapply(fd_par), extract_ok_bytes(&previous))
            {
                // fd is a u64 bit-pattern via GInt; reinterpret
                // unsigned.  n is a legitimate length so no
                // sign-guard removal there.
                let _ = self.journal_read(fd as u64, &bytes, None, ack).await;
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => {
                let r = self.read_impl(fd as u64, n as u64, None).await;
                // Journal on success.  extract_ok_bytes returns None
                // for error replies, so this is a clean guard.
                if let Some(bytes) = extract_ok_bytes(std::slice::from_ref(&r)) {
                    let _ = self.journal_read(fd as u64, &bytes, None, ack).await;
                }
                r
            }
            _ => err(FSERR_BAD_ARG, "expected (fd:GInt, n:GInt>=0)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // readAt — (fd, offset, n) -> [true, bytes]
    //
    // Slice 32 (PB-M-14 read-hash): see `fs_read` docstring.  Same
    // leader/follower journal_read pattern, with offset populated.
    // -------------------------------------------------------------------
    pub async fn fs_read_at(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_read_at"));
        };
        let [fd_par, off_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_read_at"));
        };
        if is_replay {
            if let (Some(fd), Some(off), Some(bytes)) = (
                RhoNumber::unapply(fd_par),
                RhoNumber::unapply(off_par),
                extract_ok_bytes(&previous),
            ) {
                if off >= 0 {
                    let _ = self
                        .journal_read(fd as u64, &bytes, Some(off as u64), ack)
                        .await;
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(n_par),
        ) {
            (Some(fd), Some(off), Some(n)) if off >= 0 && n >= 0 => {
                let r = self.read_impl(fd as u64, n as u64, Some(off as u64)).await;
                if let Some(bytes) = extract_ok_bytes(std::slice::from_ref(&r)) {
                    let _ = self
                        .journal_read(fd as u64, &bytes, Some(off as u64), ack)
                        .await;
                }
                r
            }
            _ => err(FSERR_BAD_ARG, "expected (fd:GInt, off:GInt>=0, n:GInt>=0)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn read_impl(&self, fd: u64, n: u64, offset: Option<u64>) -> Par {
        if n > super::MAX_READ_BYTES {
            return err(
                FSERR_QUOTA_EXCEEDED,
                format!("read {n} exceeds MAX_READ_BYTES"),
            );
        }
        // We can't move a `&mut FileHandle` into spawn_blocking (it lives
        // behind an RwLock owned by `handles`).  Instead: take the fd's
        // raw fd, do the syscall on a blocking task, and let `File` be
        // reconstructed from the handle table on the next call.  We use
        // libc::pread directly so we don't need &mut File.
        let raw_fd = match self.handles.raw_fd(fd).await {
            Some(rfd) => rfd,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        let result = spawn_blocking(move || {
            let mut buf = vec![0u8; n as usize];
            let got = unsafe {
                if let Some(off) = offset {
                    libc::pread(raw_fd, buf.as_mut_ptr() as *mut _, n as usize, off as i64)
                } else {
                    libc::read(raw_fd, buf.as_mut_ptr() as *mut _, n as usize)
                }
            };
            if got < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                buf.truncate(got as usize);
                Ok(buf)
            }
        })
        .await;
        match result {
            Err(_join_err) => err(FSERR_IO, "spawn_blocking task failed"),
            Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
            Ok(Ok(bytes)) => ok_bytes(bytes),
        }
    }

    // -------------------------------------------------------------------
    // write — (fd, bytes) -> [true, nWritten]
    // -------------------------------------------------------------------
    pub async fn fs_write(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_write"));
        };
        let [fd_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_write"));
        };
        // Parse args deterministically for both leader and follower.
        let parsed = match (RhoNumber::unapply(fd_par), RhoByteArray::unapply(bytes_par)) {
            (Some(fd), Some(bytes)) => Some((fd as u64, bytes)),
            _ => None,
        };
        // M-R3 review fix (round 2): enforce MAX_WRITE_BYTES BEFORE
        // journaling so an oversized write cannot consume a WAL slot
        // for a call that will error out.  Mirror fs_truncate's
        // pre-check-then-journal ordering (which was already correct).
        if let Some((_, bytes)) = &parsed {
            if bytes.len() as u64 > MAX_WRITE_BYTES {
                let out = vec![err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // C-29-F1 review fix: journal to WAL on BOTH leader and
        // follower — before the `is_replay` short-circuit — so both
        // sides populate identical WALs.  Populated purely from
        // args (path via fd lookup, payload hash of requested
        // bytes).  A cap-exceeded return here is deterministic
        // (both sides hit the same cap moment) and produces a
        // symmetric FSERR_QUOTA_EXCEEDED reply that is cached in
        // `previous` on the leader and replayed on the follower.
        if let Some((fd, bytes)) = &parsed {
            if self.journal_write(*fd, bytes, None, ack).await.is_err() {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        if is_replay {
            // Slice 30c M-29-3: on the follower, extract the
            // leader's cached `n` and finalize the WAL entry with
            // the actual bytes on partial writes.  Full-length
            // writes leave the pre-syscall placeholder in place
            // (already correct).
            if let (Some((_fd, bytes)), Some(n)) = (&parsed, extract_ok_u64(&previous)) {
                if n < bytes.len() as u64 {
                    self.finalize_write_journal(bytes, n, ack);
                }
            }
            // H-6 fix (2026-08-06): if the leader's cached reply
            // was an error, follower flips the placeholder to
            // Failure { code }.  Symmetric with the leader-path
            // finalize below: same code → same WAL entry.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match parsed.clone() {
            Some((fd, bytes)) => self.write_impl(fd, bytes, None).await,
            None => err(FSERR_BAD_ARG, "expected (u64, ByteArray)"),
        };
        // Slice 30c M-29-3: on the leader, extract the actual `n`
        // from the reply and finalize the WAL entry with the
        // actual bytes on partial writes.
        if let (Some((fd, bytes)), Some(n)) =
            (&parsed, extract_ok_u64(std::slice::from_ref(&reply)))
        {
            if n < bytes.len() as u64 {
                tracing::warn!(
                    target: "f1r3fly.fs_wal",
                    fd = fd,
                    requested = bytes.len(),
                    actual = n,
                    "partial write on Consensus cap; finalizing WAL entry with actual bytes"
                );
                self.finalize_write_journal(bytes, n, ack);
            }
        }
        // H-6 fix (2026-08-06): if the syscall returned an error
        // reply, flip the WAL placeholder to Failure { code }.
        // Followers reading a Failure entry MUST NOT apply the
        // write to reconstructed state — the leader never wrote
        // anything to disk.  Without this the follower would
        // apply a Write against the failed leader's state and
        // diverge.
        if let Some(code_str) = extract_err_code(std::slice::from_ref(&reply)) {
            self.finalize_failure_journal(fserr_to_code(&code_str), ack);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // writeAt — (fd, offset, bytes) -> [true, nWritten]
    // -------------------------------------------------------------------
    pub async fn fs_write_at(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_write_at"));
        };
        let [fd_par, off_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_write_at"));
        };
        let parsed = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoByteArray::unapply(bytes_par),
        ) {
            (Some(fd), Some(off), Some(bytes)) if off >= 0 => Some((fd as u64, off as u64, bytes)),
            _ => None,
        };
        // M-R3 review fix (round 2): MAX_WRITE_BYTES check BEFORE
        // journaling; see fs_write for rationale.
        if let Some((_, _, bytes)) = &parsed {
            if bytes.len() as u64 > MAX_WRITE_BYTES {
                let out = vec![err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // C-29-F1 review fix: journal to WAL on both leader and follower
        // before the `is_replay` short-circuit (see `fs_write` for the
        // full rationale).
        if let Some((fd, off, bytes)) = &parsed {
            if self
                .journal_write(*fd, bytes, Some(*off), ack)
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        if is_replay {
            // Slice 30c M-29-3: follower finalize on partial write.
            if let (Some((_fd, _off, bytes)), Some(n)) = (&parsed, extract_ok_u64(&previous)) {
                if n < bytes.len() as u64 {
                    self.finalize_write_journal(bytes, n, ack);
                }
            }
            // H-6 fix (2026-08-06): follower failure-finalize
            // mirror of the leader path below.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match parsed.clone() {
            Some((fd, off, bytes)) => self.write_impl(fd, bytes, Some(off)).await,
            None => err(FSERR_BAD_ARG, "expected (u64, u64, ByteArray)"),
        };
        // Slice 30c M-29-3: leader finalize on partial write.
        if let (Some((fd, off, bytes)), Some(n)) =
            (&parsed, extract_ok_u64(std::slice::from_ref(&reply)))
        {
            if n < bytes.len() as u64 {
                tracing::warn!(
                    target: "f1r3fly.fs_wal",
                    fd = fd,
                    offset = off,
                    requested = bytes.len(),
                    actual = n,
                    "partial write_at on Consensus cap; finalizing WAL entry"
                );
                self.finalize_write_journal(bytes, n, ack);
            }
        }
        // H-6 fix (2026-08-06): flip placeholder to Failure on
        // syscall error (EIO/ENOSPC/EROFS/etc.) so followers
        // don't replay a write the leader never actually
        // committed.
        if let Some(code_str) = extract_err_code(std::slice::from_ref(&reply)) {
            self.finalize_failure_journal(fserr_to_code(&code_str), ack);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn write_impl(&self, fd: u64, bytes: Vec<u8>, offset: Option<u64>) -> Par {
        if bytes.len() as u64 > MAX_WRITE_BYTES {
            return err(
                FSERR_QUOTA_EXCEEDED,
                format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
            );
        }
        let raw_fd = match self.handles.raw_fd(fd).await {
            Some(rfd) => rfd,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        // Redesign note: WAL journaling for Consensus caps happens in
        // `fs_write` / `fs_write_at` BEFORE this function is called,
        // so both leader and follower populate identical WALs
        // (C-29-F1 review fix).  Do NOT append here.
        let result = spawn_blocking(move || {
            let n = unsafe {
                if let Some(off) = offset {
                    libc::pwrite(raw_fd, bytes.as_ptr() as *const _, bytes.len(), off as i64)
                } else {
                    libc::write(raw_fd, bytes.as_ptr() as *const _, bytes.len())
                }
            };
            if n < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as u64)
            }
        })
        .await;
        match result {
            Err(_join_err) => err(FSERR_IO, "spawn_blocking task failed"),
            Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
            Ok(Ok(n)) => ok_u64(n),
        }
    }

    // -------------------------------------------------------------------
    // seek — (fd, offset, whence) -> [true, newPos]
    // -------------------------------------------------------------------
    pub async fn fs_seek(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_seek"));
        };
        let [fd_par, off_par, whence_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_seek"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoString::unapply(whence_par),
        ) {
            (Some(fd), Some(off), Some(w)) => {
                let whence_code = match w.as_str() {
                    "set" if off >= 0 => Some(libc::SEEK_SET),
                    "cur" => Some(libc::SEEK_CUR),
                    "end" => Some(libc::SEEK_END),
                    _ => None,
                };
                match whence_code {
                    None => err(FSERR_BAD_ARG, "expected whence in {set,cur,end}"),
                    Some(whence) => match self.handles.raw_fd(fd as u64).await {
                        None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                        Some(raw_fd) => {
                            let r = spawn_blocking(move || unsafe {
                                let pos = libc::lseek(raw_fd, off, whence);
                                if pos < 0 {
                                    Err(std::io::Error::last_os_error())
                                } else {
                                    Ok(pos as u64)
                                }
                            })
                            .await;
                            match r {
                                Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                                Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                                Ok(Ok(pos)) => ok_u64(pos),
                            }
                        }
                    },
                }
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, i64, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // tell — (fd) -> [true, pos]
    // -------------------------------------------------------------------
    pub async fn fs_tell(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_tell"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_tell"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    let pos = libc::lseek(raw_fd, 0, libc::SEEK_CUR);
                    if pos < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(pos as u64)
                    }
                })
                .await;
                match r {
                    Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(pos)) => ok_u64(pos),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // size — (fd) -> [true, nBytes]
    // -------------------------------------------------------------------
    pub async fn fs_size(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_size"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_size"));
        };
        // M-5: look up FileHandle cmode + canon_path so both
        // is_replay and leader paths can journal symmetrically.
        // If the fd isn't valid at lookup time, the syscall
        // will produce FSERR_CLOSED and journal-cmode is None
        // (no journaling happens).
        let (jmode, jpath): (Option<ConsensusMode>, Option<PathBuf>) =
            match RhoNumber::unapply(fd_par) {
                Some(fd) => {
                    let meta = self
                        .handles
                        .with_mut(fd as u64, |h| (h.cmode, h.canon_path.clone()))
                        .await;
                    match meta {
                        Some((c, p)) => (Some(c), Some(p)),
                        None => (None, None),
                    }
                }
                None => (None, None),
            };
        if is_replay {
            if let (Some(mode), Some(p)) = (jmode, jpath.clone()) {
                if let Some(reply_par) = previous.first() {
                    self.journal_state_read(mode, WalOp::Size, p, reply_par, ack, None);
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    let mut sb: libc::stat = std::mem::zeroed();
                    if libc::fstat(raw_fd, &mut sb) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(sb.st_size as u64)
                    }
                })
                .await;
                match r {
                    Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(n)) => ok_u64(n),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        // M-5: leader journals from fresh reply.
        if let (Some(mode), Some(p)) = (jmode, jpath) {
            self.journal_state_read(mode, WalOp::Size, p, &reply, ack, None);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // truncate — (fd, n) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_truncate(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_truncate"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_truncate"));
        };
        let parsed = match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => Some((fd as u64, n as u64)),
            _ => None,
        };
        // C-29-F1 review fix: journal to WAL on both leader and
        // follower before the `is_replay` short-circuit.  We do the
        // MAX_TRUNCATE_BYTES check first so an oversize truncate does
        // not consume a WAL slot for a call that will error out.
        if let Some((fd, n)) = &parsed {
            if *n <= super::MAX_TRUNCATE_BYTES && self.journal_truncate(*fd, *n, ack).await.is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        if is_replay {
            // H-6 fix (2026-08-06): follower failure-finalize
            // mirror of the leader path below.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // H-6 refactor: compute the reply without any early-return
        // so failure-finalize gets a single call site below.  The
        // FSERR_CLOSED "unknown fd" branch that previously returned
        // eagerly now folds into `reply` naturally.
        let reply = match parsed {
            Some((fd, n)) => {
                if n > super::MAX_TRUNCATE_BYTES {
                    err(
                        FSERR_QUOTA_EXCEEDED,
                        format!("truncate {n} exceeds MAX_TRUNCATE_BYTES"),
                    )
                } else {
                    match self.handles.raw_fd(fd).await {
                        Some(raw_fd) => {
                            let r = spawn_blocking(move || unsafe {
                                if libc::ftruncate(raw_fd, n as i64) < 0 {
                                    Err(std::io::Error::last_os_error())
                                } else {
                                    Ok(())
                                }
                            })
                            .await;
                            match r {
                                Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                                Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                                Ok(Ok(())) => ok_bare(),
                            }
                        }
                        None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                    }
                }
            }
            None => err(FSERR_BAD_ARG, "expected (u64, u64)"),
        };
        // H-6 fix (2026-08-06): flip placeholder to Failure on
        // syscall error so followers don't replay a truncate the
        // leader never actually committed.
        if let Some(code_str) = extract_err_code(std::slice::from_ref(&reply)) {
            self.finalize_failure_journal(fserr_to_code(&code_str), ack);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // flush — (fd) -> [true]  (fsync: data + metadata)
    // -------------------------------------------------------------------
    pub async fn fs_flush(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_flush"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_flush"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    if libc::fsync(raw_fd) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                })
                .await;
                match r {
                    Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(())) => ok_bare(),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // stat — (rootCanon, rel) -> [true, record]
    // Uses fstatat(AT_SYMLINK_NOFOLLOW) via safe descent.
    // -------------------------------------------------------------------
    pub async fn fs_stat(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_stat"));
        };
        // Slice 26: `(root, rel, cmode, ack)`.  `cmode` is
        // `"oracular"` / `"consensus"` and controls whether the
        // record omits host-transient fields.  On an unrecognized
        // cmode string we fall back to `self.mode` (Oracular by
        // default) — this preserves behavior for any caller that
        // hasn't been updated yet.
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_stat"));
        };
        // M-5 fix (2026-08-06): resolve cmode BEFORE the is_replay
        // short-circuit so both leader + follower can journal
        // symmetrically.  A bad cmode still short-circuits with
        // FSERR_BAD_ARG the same way as before (that path
        // doesn't journal — it's a parse error, not a filesystem
        // observation).
        let mode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        // Precompute journal path (used by both branches).
        let journal_path: Option<PathBuf> =
            match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
                (Some(root), Some(rel)) => {
                    let mut p = PathBuf::from(root);
                    if !rel.is_empty() {
                        p.push(&rel);
                    }
                    Some(p)
                }
                _ => None,
            };
        if is_replay {
            // M-5: follower journals from cached `previous` so
            // WAL is byte-identical with the leader.
            if let Some(p) = journal_path.clone() {
                if let Some(reply_par) = previous.first() {
                    self.journal_state_read(mode, WalOp::Stat, p, reply_par, ack, None);
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let leaf_name = leaf_of(&rel);
                let root_pb = PathBuf::from(root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (code, msg) = quarantine_err_reply(&qe);
                            return err(code, msg);
                        }
                    };
                    match fstatat_meta(&parent) {
                        Ok(m) => ok_par(stat_record(&leaf_name, &m, mode)),
                        Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, String)"),
        };
        // M-5: leader journals from fresh syscall reply.
        if let Some(p) = journal_path {
            self.journal_state_read(mode, WalOp::Stat, p, &reply, ack, None);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // exists — (rootCanon, rel) -> [true, Bool]
    // -------------------------------------------------------------------
    pub async fn fs_exists(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_exists"));
        };
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_exists"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            // Not-found or symlink → exists is false.  A
                            // quarantine failure (escape/absolute/etc.)
                            // is a caller error, surface as bad arg.
                            use super::path::QuarantineError::*;
                            return match qe {
                                EscapesRoot | SymlinkComponent | RootIdentityChanged => {
                                    let (c, m) = quarantine_err_reply(&qe);
                                    err(c, m)
                                }
                                Empty | RootSelf => {
                                    let (c, m) = quarantine_err_reply(&qe);
                                    err(c, m)
                                }
                                IoError(_) => ok_bool(false),
                            };
                        }
                    };
                    let ok = fstatat_meta(&parent).is_ok();
                    ok_bool(ok)
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entries — (rootCanon, rel) -> [true, [record, ...]]
    // Sorted lex by name; capped at MAX_ENTRIES; per-entry stat error
    // becomes a row with an `error` field.
    // -------------------------------------------------------------------
    pub async fn fs_entries(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries"));
        };
        // Slice 26: `(root, rel, cmode, ack)`.
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries"));
        };
        // M-5: resolve cmode before is_replay so both leader
        // and follower journal symmetrically.
        let mode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let journal_path: Option<PathBuf> =
            match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
                (Some(root), Some(rel)) => {
                    let mut p = PathBuf::from(root);
                    if !rel.is_empty() {
                        p.push(&rel);
                    }
                    Some(p)
                }
                _ => None,
            };
        if is_replay {
            if let Some(p) = journal_path.clone() {
                if let Some(reply_par) = previous.first() {
                    self.journal_state_read(mode, WalOp::Entries, p, reply_par, ack, None);
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (code, msg) = quarantine_err_reply(&qe);
                            return err(code, msg);
                        }
                    };
                    // Open the target directory (safely, via openat +
                    // O_NOFOLLOW|O_DIRECTORY off the parent dirfd).
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
                    // L-3 fix (2026-08-06): use F_DUPFD_CLOEXEC so the
                    // duplicated fd carries FD_CLOEXEC atomically.  Plain
                    // libc::dup would produce a fd WITHOUT CLOEXEC; a
                    // concurrent exec (from any thread) would leak the
                    // dir fd into the child process.  F_DUPFD_CLOEXEC
                    // closes the race by setting CLOEXEC in the same
                    // syscall.
                    let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
                    if read_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        unsafe { libc::close(dir_fd) };
                        return err(io_err_code(&e), io_msg_scrub(&e));
                    }
                    let entries = read_dir_capped(read_fd, MAX_ENTRIES);
                    match entries {
                        Err(e) => {
                            unsafe { libc::close(dir_fd) };
                            err(io_err_code(&e), io_msg_scrub(&e))
                        }
                        Ok((mut names, hit_cap)) => {
                            if hit_cap {
                                unsafe { libc::close(dir_fd) };
                                return err(
                                    FSERR_QUOTA_EXCEEDED,
                                    format!(
                                        "entries exceeds MAX_ENTRIES={MAX_ENTRIES}; use \
                                         entriesStream for large directories",
                                    ),
                                );
                            }
                            names.sort();
                            let rows: Vec<Par> = names
                                .into_iter()
                                .map(|name| entry_stat_row(dir_fd, &name, mode))
                                .collect();
                            unsafe { libc::close(dir_fd) };
                            ok_list(rows)
                        }
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        // M-5: leader journals from fresh reply.
        if let Some(p) = journal_path {
            self.journal_state_read(mode, WalOp::Entries, p, &reply, ack, None);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // rename — (fromRootCanon, fromRel, toRootCanon, toRel) -> [true]
    // Uses renameat between two safely-descended parents.
    // -------------------------------------------------------------------
    pub async fn fs_rename(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_rename"));
        };
        // C-R2 review fix: `(from_root, from_rel, to_root, to_rel, cmode, ack)`.
        // Consensus cap short-circuits — see fs_chmod for rationale.
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, cmode_par, ack] =
            args.as_slice()
        else {
            return Err(illegal_argument_error("fs_rename"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "rename unavailable in consensus mode",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        let reply = match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => {
                let from_root_pb = PathBuf::from(from_root);
                let to_root_pb = PathBuf::from(to_root);
                let from_expected_id = self.handles.root_registry.get(&from_root_pb);
                let to_expected_id = self.handles.root_registry.get(&to_root_pb);
                spawn_blocking(move || -> Par {
                    let from_parent =
                        match safe_descend_verified(&from_root_pb, &from_rel, from_expected_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return err(c, m);
                            }
                        };
                    let to_parent =
                        match safe_descend_verified(&to_root_pb, &to_rel, to_expected_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return err(c, m);
                            }
                        };
                    let rc = unsafe {
                        libc::renameat(
                            from_parent.as_raw_fd(),
                            from_parent.leaf_ptr(),
                            to_parent.as_raw_fd(),
                            to_parent.leaf_ptr(),
                        )
                    };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        let code = if e.raw_os_error() == Some(libc::EXDEV) {
                            FSERR_CROSS_DEVICE
                        } else {
                            io_err_code(&e)
                        };
                        err(code, io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected 4 String args + cmode"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // copyFile — (fromRootCanon, fromRel, toRootCanon, toRel) -> [true, nBytes]
    // Uses safe_open on both sides + std::io::copy on File objects.
    // -------------------------------------------------------------------
    pub async fn fs_copy_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        // C-R2 review fix: `(from_root, from_rel, to_root, to_rel, cmode, ack)`.
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, cmode_par, ack] =
            args.as_slice()
        else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "copyFile unavailable in consensus mode",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        let reply = match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => {
                let from_pb = PathBuf::from(from_root);
                let to_pb = PathBuf::from(to_root);
                spawn_blocking(move || -> Par {
                    let mut src = match safe_open(&from_pb, &from_rel, libc::O_RDONLY, 0) {
                        Ok(f) => f,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let mut dst = match safe_open(
                        &to_pb,
                        &to_rel,
                        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                        0o644,
                    ) {
                        Ok(f) => f,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    match std::io::copy(&mut src, &mut dst) {
                        Ok(n) => ok_u64(n),
                        Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected 4 String args + cmode"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // removeFile — (rootCanon, rel) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_remove_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        // C-R2 review fix: `(root, rel, cmode, ack)`.
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "removeFile unavailable in consensus mode",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let rc = unsafe { libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), 0) };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        err(io_err_code(&e), io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // removeDir — (rootCanon, rel, recursive: Bool) -> [true]
    // Non-recursive: unlinkat(AT_REMOVEDIR).
    // Recursive: descend into the target and unlinkat every entry (safe).
    // -------------------------------------------------------------------
    pub async fn fs_remove_dir(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        // C-R2 review fix: `(root, rel, recursive, cmode, ack)`.
        let [root_par, rel_par, recursive_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "removeDir unavailable in consensus mode",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoBoolean::unapply(recursive_par),
        ) {
            (Some(root), Some(rel), Some(recursive)) => {
                let root_pb = PathBuf::from(root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    if recursive {
                        if let Err(e) = remove_dir_recursive(parent.as_raw_fd(), parent.leaf_ptr())
                        {
                            return err(io_err_code(&e), io_msg_scrub(&e));
                        }
                        ok_bare()
                    } else {
                        let rc = unsafe {
                            libc::unlinkat(
                                parent.as_raw_fd(),
                                parent.leaf_ptr(),
                                libc::AT_REMOVEDIR,
                            )
                        };
                        if rc == 0 {
                            ok_bare()
                        } else {
                            let e = std::io::Error::last_os_error();
                            err(io_err_code(&e), io_msg_scrub(&e))
                        }
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, Bool, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chmod — (rootCanon, rel, modeBits) -> [true]
    // fchmodat(AT_SYMLINK_NOFOLLOW) — spec-mandated symlink safety.
    // -------------------------------------------------------------------
    pub async fn fs_chmod(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        // C-R2 review fix (slice 29 round 2): `(root, rel, bits, cmode, ack)`.
        // A Consensus cap short-circuits with FSERR_UNSUPPORTED — path-based
        // mutations are not journaled to the WAL in slice 29, so allowing
        // them on Consensus would silently diverge leader/follower state.
        // Mirrors slice-26 fs_chown pattern; defense-in-depth beneath the
        // Rholang-side H-29-3 guards in File.rho / Dir.rho.
        let [root_par, rel_par, mode_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "chmod unavailable in consensus mode",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoNumber::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(bits)) if (0..=0o7777).contains(&bits) => {
                let root_pb = PathBuf::from(root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                let bits = bits as libc::mode_t;
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let rc = unsafe {
                        libc::fchmodat(
                            parent.as_raw_fd(),
                            parent.leaf_ptr(),
                            bits,
                            libc::AT_SYMLINK_NOFOLLOW,
                        )
                    };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        // ENOTSUP means the platform doesn't honor
                        // AT_SYMLINK_NOFOLLOW on chmod (Linux, some
                        // filesystems).  In that case there's no
                        // symlink-safe chmod primitive; report
                        // UNSUPPORTED so the caller sees the failure
                        // rather than silently following.
                        let code = if e.raw_os_error() == Some(libc::ENOTSUP)
                            || e.raw_os_error() == Some(libc::EOPNOTSUPP)
                        {
                            FSERR_UNSUPPORTED
                        } else {
                            io_err_code(&e)
                        };
                        err(code, io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(
                FSERR_BAD_ARG,
                "expected (String, String, u64<=0o7777, String)",
            ),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chown — (rootCanon, rel, owner, group) -> [true]
    // Consensus mode: returns FSERR_UNSUPPORTED.
    // Oracular: fchownat(AT_SYMLINK_NOFOLLOW).
    // -------------------------------------------------------------------
    pub async fn fs_chown(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_chown"));
        };
        // Slice 26: `(root, rel, owner, group, cmode, ack)`.  A
        // `Consensus` cmode short-circuits with FSERR_UNSUPPORTED
        // without touching the host filesystem, matching plan §369
        // and spec §Storage cases 2 / 4 / 6.
        let [root_par, rel_par, owner_par, group_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chown"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // C-26-F1 review fix: fail-closed on unrecognized cmode.  This
        // check comes BEFORE the Consensus short-circuit so a
        // malformed cmode never silently reaches the fallback.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let reply = if cmode == ConsensusMode::Consensus {
            err(FSERR_UNSUPPORTED, "chown unavailable in consensus mode")
        } else {
            match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
                (Some(root), Some(rel)) => {
                    let owner_opt = RhoString::unapply(owner_par);
                    let group_opt = RhoString::unapply(group_par);
                    let root_pb = PathBuf::from(root);
                    let expected_root_id = self.handles.root_registry.get(&root_pb);
                    chown_impl(&root_pb, rel, owner_opt, group_opt, expected_root_id).await
                }
                _ => err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String|Nil, String|Nil, String)",
                ),
            }
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // quarantine — (rootCanon, rel) -> [true, canonPath]
    // Standalone safety check that also returns a diagnostic display path
    // (procfs magic-link resolution of the parent dirfd + leaf).  Note:
    // the returned canonPath echoes back the (already caller-known)
    // resolved path; other handlers do NOT accept caller-supplied
    // canonPaths.
    // -------------------------------------------------------------------
    pub async fn fs_quarantine(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_quarantine"));
        };
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_quarantine"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(&root);
                let expected_root_id = self.handles.root_registry.get(&root_pb);
                spawn_blocking(move || -> Par {
                    match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(_) => {
                            // Return the caller-supplied joined path;
                            // safe_descend already verified it doesn't
                            // escape.  This is deterministic (no
                            // canonicalize call, so no host drift).
                            ok_string(root_pb.join(&rel).to_string_lossy().into_owned())
                        }
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            err(c, m)
                        }
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entriesStream — (rootCanon, rel) -> [true, streamFd]
    // Placeholder: returns FSERR_UNSUPPORTED.  The backing streaming
    // primitive (a per-runtime dir-handle table analogous to
    // FileHandleTable, with `next(fd)` / `close(fd)` operators) is
    // scoped for Phase 1 tail-end but not yet implemented; Phase 4
    // wires the agent-side EntryStream on top of it.
    // -------------------------------------------------------------------
    pub async fn fs_entries_stream(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        let [_root, _rel, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = err(
            FSERR_UNSUPPORTED,
            "entriesStream backing not yet implemented (Phase 1 tail-end)",
        );
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // Phase 8 slice 8a — range-lock natives.
    //
    // These acquire and release entries in `self.handles.lock_registry`,
    // the RuntimeManager-broadcast `LockRegistry` (X-1 design memo).
    // The lock table is keyed on `(dev, inode)` so cross-cap
    // coordination on the same physical file collapses to one entry
    // regardless of which fresh-mint `File` cap holds it (slice 27).
    //
    // Wait:false only for MVP.  Blocking acquisition (wait:true) is
    // slice 8b via Rig-protocol; every acquire here returns immediately
    // with either `[true, lock_id]` or `[false, FSERR_BUSY, ...]`.
    //
    // WAL journaling of `LockAcquire` / `LockRelease` entries is step 4
    // of slice 8a — deferred here.  The natives resolve the acquire
    // outcome but do not yet append WAL entries.  Under consensus mode
    // they will need to (per X-1 §4); under oracular they will not
    // (per §Mode-differentiated invariants — oracular locks are
    // in-process hints, not consensus state).
    //
    // Deploy-end auto-release (MUST per X-4 / spec §Explicit locks)
    // is step 6 — wired at the WalDeployScope::end hook site in the
    // casper crate.  For now handlers pass `DeployScope::default()`
    // as a placeholder; step 6 threads the real per-deploy identity.
    // Callers holding `LockToken`s must explicitly `release` in the
    // meantime — this is safe for tests + the eventual deploy-end
    // sweep will supersede the placeholder deploy id uniformly.
    //
    // Replay semantics: on `is_replay = true` these natives echo
    // `previous` and do NOT touch `LockRegistry`.  Follower registry
    // state diverges from the leader's, but that divergence is never
    // consensus-observable because every reply is captured — no
    // consensus-observable code path consults `LockRegistry` outside
    // the replay-cached natives.  When step 4 adds WAL journaling of
    // `LockAcquire` / `LockRelease` entries, the follower's state
    // MUST be reconstituted from the WAL during replay (mirror slice
    // 29's `journal_write` / `finalize_write_journal` pattern) so
    // that step 7's consensus-mode unlink gate (`is_locked` in
    // `fs_remove_file` / `fs_remove_dir`) sees the same state on
    // leader and follower.  Under oracular mode the LockRegistry is
    // best-effort per §Mode-differentiated invariants, so follower
    // state doesn't matter there either way.
    // -------------------------------------------------------------------

    /// Acquire a positional range lock on the file behind `fd`.
    ///
    /// Args: `(fd: u64, offset: u64, length: u64, mode: String("r"|"w"),
    /// holder: Par, cmode: String, ack)`
    ///
    /// **Fd-based keying — critical for oracular correctness.**  The
    /// lock keys on `fstat(fd).(st_dev, st_ino)` — the physical file
    /// `fd` points at — NOT on any current-path resolution.  Under
    /// oracular mode a caller's fd may point at an inode different
    /// from whatever's currently at the original path (external
    /// process could have `mv`d the file); keying on the fd's inode
    /// keeps the lock consistent with the reads/writes it protects
    /// (which also operate through `fd`).  Under consensus mode the
    /// path is stable so either keying would be correct — but fd is
    /// uniformly right.
    ///
    /// - `holder` is opaque Par (typically the caller-cap's `stateP`
    ///   GPrivate name) hashed to a stable 32-byte `HolderId` — used
    ///   by `File.close`'s `release_all_for_holder` sweep.
    /// - `cmode` is validated but not currently branched on at acquire
    ///   time; step 4 (WAL) and step 7 (unlink gate) will.
    ///
    /// Reply: `[true, lock_id: Int]` on success, `[false, code, msg]`
    /// on error (FSERR_CLOSED if fd unknown or shadow handle,
    /// FSERR_BUSY, FSERR_BAD_ARG, FSERR_QUOTA_EXCEEDED, FSERR_IO on
    /// fstat failure).
    pub async fn fs_lock_range(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_lock_range"));
        };
        let [fd_par, off_par, len_par, mode_par, holder_par, cmode_par, ack] = args.as_slice()
        else {
            return Err(illegal_argument_error("fs_lock_range"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // cmode validation: rejects bad shapes even though acquire
        // outcome doesn't currently branch on it — step 4 (WAL) and
        // step 7 (unlink gate) will.  Fail-closed matches the pattern
        // of every other cmode-taking native.
        let _cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(len_par),
            RhoString::unapply(mode_par),
            resolve_lock_mode(mode_par),
        ) {
            (Some(fd), Some(off), Some(len), Some(_), Some(lm))
                if fd >= 0 && off >= 0 && len > 0 =>
            {
                match self.dev_inode_from_fd(fd as u64).await {
                    Ok(dev_inode) => {
                        let holder = holder_id_of(holder_par);
                        let deploy = DeployScope::default(); // step 6 threads real id
                        match self.handles.lock_registry.try_acquire_range(
                            dev_inode, off as u64, len as u64, lm, holder, deploy,
                        ) {
                            Ok(id) => ok_u64(id.0),
                            Err(le) => lock_err_reply(le),
                        }
                    }
                    Err((code, msg)) => err(code, msg),
                }
            }
            _ => err(
                FSERR_BAD_ARG,
                "expected (u64, u64, u64>0, String\"r|w\", Par, String)",
            ),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// Acquire the whole-file sequential lock on the file behind `fd`.
    ///
    /// Called by sequential-stream constructors (`chars`, `bytes`,
    /// `lines`, `readLine`, `writeChars`, `writeBytes`, `writeLine`,
    /// `writeLines`, `writeString`, `writeByteArray`) before they
    /// begin producing.  Enforces "one active sequential stream per
    /// File" (FIP §1132) at the physical file level rather than at
    /// the cap level, so cross-cap sequential streams on the same
    /// inode also conflict per §1182 — using fd-based keying (see
    /// `fs_lock_range` docstring for the oracular-correctness
    /// rationale).
    ///
    /// Args: `(fd: u64, holder: Par, cmode: String, ack)`
    /// Reply: `[true, lock_id: Int]` or `[false, code, msg]`.
    pub async fn fs_lock_sequential(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_lock_sequential"));
        };
        let [fd_par, holder_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_lock_sequential"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let _cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) if fd >= 0 => match self.dev_inode_from_fd(fd as u64).await {
                Ok(dev_inode) => {
                    let holder = holder_id_of(holder_par);
                    let deploy = DeployScope::default(); // step 6
                    match self
                        .handles
                        .lock_registry
                        .try_acquire_sequential(dev_inode, holder, deploy)
                    {
                        Ok(id) => ok_u64(id.0),
                        Err(le) => lock_err_reply(le),
                    }
                }
                Err((code, msg)) => err(code, msg),
            },
            _ => err(FSERR_BAD_ARG, "expected (u64, Par, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// Resolve a Rholang-supplied fd to its `(st_dev, st_ino)` pair
    /// via `fstat(2)`.  Common core of `fs_lock_range` /
    /// `fs_lock_sequential` (post-review-2 fix, 2026-08-12): both
    /// natives key their LockRegistry entry on the physical file the
    /// fd points at, not on any current-path resolution, so cross-cap
    /// coordination is oracular-correct.
    ///
    /// Returns `Err((FSERR_CLOSED, ...))` if the fd is unknown to the
    /// handle table or is a shadow handle (replay-only), and
    /// `Err((FSERR_IO, ...))` on fstat failure.
    ///
    /// # Concurrent close race — documented as caller-bug behavior
    ///
    /// `raw_fd` returns the raw OS fd as a bare `i32` and drops the
    /// handle-table lock.  Between the return and the `fstat` below,
    /// a concurrent `fs_close(fd)` on the SAME cap could drop the
    /// tokio `File`, close the OS fd, and let a subsequent `open` in
    /// another handler reuse that fd number for a different inode —
    /// making `fstat` stat the wrong file.
    ///
    /// This race is only exploitable by a caller doing incoherent
    /// concurrent close-and-lock on their OWN cap: under fresh-mint
    /// semantics each cap owns its own fd; other caps have distinct
    /// fds and can't close it.  Under H-7's play/replay isolation
    /// each has its own `FileHandleTable`, so no cross-runtime race
    /// either.  Under D3's `FuturesUnordered` two Par branches of a
    /// single deploy sharing the same cap could race — but "close
    /// while locking" is user-code incoherence, not a security
    /// vulnerability.  Outcome: either `FSERR_CLOSED` (if raw_fd
    /// missed too) or a `(dev, ino)` for whatever file happens to
    /// hold that raw fd at the moment of fstat.  Neither leaks
    /// authority — the caller closed their own fd; if the lock
    /// registry ends up keyed on a different inode, only that
    /// caller sees the confusion, and their subsequent reads/writes
    /// on the (now closed) fd will fail with `FSERR_CLOSED` anyway.
    async fn dev_inode_from_fd(&self, fd: u64) -> Result<(u64, u64), (&'static str, String)> {
        let Some(raw) = self.handles.raw_fd(fd).await else {
            return Err((FSERR_CLOSED, "fd unknown or shadow handle".to_string()));
        };
        #[cfg(unix)]
        {
            unsafe {
                let mut st: libc::stat = std::mem::zeroed();
                if libc::fstat(raw, &mut st) < 0 {
                    let e = std::io::Error::last_os_error();
                    return Err((FSERR_IO, io_msg_scrub(&e)));
                }
                #[allow(clippy::unnecessary_cast)]
                Ok((st.st_dev as u64, st.st_ino as u64))
            }
        }
        #[cfg(not(unix))]
        {
            let _ = raw;
            Ok((0, 0))
        }
    }

    /// Release a previously-acquired lock by id.  Both positional
    /// (`fs_lock_range`) and sequential (`fs_lock_sequential`) locks
    /// release through this native — the id space is unified.
    ///
    /// Args: `(lock_id: u64, ack)`
    /// Reply: `[true]` on success, `[false, FSERR_CLOSED, msg]` if
    /// the id isn't held (double release, cap-close swept it first,
    /// etc.  Idempotent behavior mirrors `File.close` /
    /// `Stream.close`).
    pub async fn fs_release_lock(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_release_lock"));
        };
        let [id_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_release_lock"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(id_par) {
            Some(n) if n >= 0 => match self.handles.lock_registry.release(LockId(n as u64)) {
                Ok(()) => ok_bare(),
                Err(le) => lock_err_reply(le),
            },
            _ => err(FSERR_BAD_ARG, "expected (u64)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }
}

// ---------------------------------------------------------------------
// Helpers — pure fns (no self) called from spawn_blocking closures.
// ---------------------------------------------------------------------

/// Resolve the Rholang-supplied cmode string into a `ConsensusMode`.
/// Slice 26 + C-26-F1 review fix: `fs_stat` / `fs_entries` / `fs_chown`
/// now take the per-cap consensus mode as a REQUIRED positional arg
/// (the library agents peek their `cmodeP` cell and forward the exact
/// string).  Any Par shape other than `String("oracular")` /
/// `String("consensus")` returns `None`; callers surface
/// `FSERR_BAD_ARG` and refuse to proceed.
///
/// Fail-closed rationale: the pre-fix behavior silently defaulted to
/// `self.mode` (Oracular by default), which is a downgrade for
/// Consensus caps.  Under the URN-filter gap (MVP #5 / slice 31) a
/// user deploy could omit the cmode entirely and get chown / host
/// metadata for free.  Under FIPS threat modeling, a per-cap mode arg
/// that fails to parse must reject the call, not silently pick the
/// weaker mode.
///
/// String constants live in `io/mod.rs` (`CMODE_ORACULAR_STR` /
/// `CMODE_CONSENSUS_STR`) and are re-exported by
/// `casper::genesis::contracts::fs_genesis::BundleConsensusMode` so the
/// composer and the resolver agree byte-for-byte.  A drift-assertion
/// test in `fs_genesis.rs` pins the pair.
fn resolve_cmode(par: &Par) -> Option<ConsensusMode> {
    match RhoString::unapply(par).as_deref() {
        Some(s) if s == CMODE_CONSENSUS_STR => Some(ConsensusMode::Consensus),
        Some(s) if s == CMODE_ORACULAR_STR => Some(ConsensusMode::Oracular),
        _ => None,
    }
}

/// Phase 8 slice 8a — parse the Rholang-supplied lock-mode string
/// into a `LockMode`.  Accepts exactly `"r"` and `"w"` per FIP
/// §Explicit locks; any other shape returns `None` and the caller
/// surfaces `FSERR_BAD_ARG`.  Fail-closed mirrors `resolve_cmode`.
fn resolve_lock_mode(par: &Par) -> Option<LockMode> {
    match RhoString::unapply(par).as_deref() {
        Some("r") => Some(LockMode::Read),
        Some("w") => Some(LockMode::Write),
        _ => None,
    }
}

/// Phase 8 slice 8a — derive a stable 32-byte `HolderId` from an
/// opaque Rholang Par (typically the caller-cap's `stateP` GPrivate
/// name).  Uses the same Blake2b256 stable-hash provider that rspace
/// uses for channel identity, so equal-Par callers hash to the same
/// bytes across runtimes deterministically.
fn holder_id_of(par: &Par) -> HolderId {
    let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(par).bytes();
    // Same 32-byte hard-dep as `ack_channel_hash`.  A Blake2b256
    // provider swap producing shorter output would silently
    // collide HolderIds; fail loudly at first call instead.
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — HolderId hard-depends on \
         a fixed 32-byte hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    HolderId::from_bytes(out)
}

/// Phase 8 slice 8a — map a `LockError` from the LockRegistry to
/// the FSERR reply shape.
fn lock_err_reply(le: LockError) -> Par {
    match le {
        LockError::Busy => err(FSERR_BUSY, "range lock unavailable"),
        LockError::Closed => err(FSERR_CLOSED, "lock id not held"),
        LockError::BadArg => err(FSERR_BAD_ARG, "invalid lock argument"),
        LockError::QuotaExceeded => err(
            FSERR_QUOTA_EXCEEDED,
            "range-lock cap exceeded for this file",
        ),
    }
}

fn leaf_of(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_string())
}

/// Fetch a `std::fs::Metadata` for the leaf named by `parent`.  Opens
/// the leaf via openat + O_NOFOLLOW off the parent dirfd, then reads
/// metadata.  A symlink leaf yields `ELOOP` — the caller decides how to
/// surface that.
fn fstatat_meta(parent: &SafeParent) -> std::io::Result<std::fs::Metadata> {
    use std::os::fd::FromRawFd;
    unsafe {
        let fd = libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let file = std::fs::File::from_raw_fd(fd);
        file.metadata()
    }
}

/// Build a stat/error record for one entry inside `dir_fd`.  Opens the
/// entry via openat + O_NOFOLLOW; regular/directory entries produce a
/// full `stat_record`, symlinks and unreadable entries produce an
/// `error_record` (spec §Dir.entries: per-entry error becomes a row).
fn entry_stat_row(dir_fd: libc::c_int, name: &std::ffi::OsStr, mode: ConsensusMode) -> Par {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    let display = name.to_string_lossy().into_owned();
    let cname = match std::ffi::CString::new(name.as_bytes()) {
        Ok(c) => c,
        Err(_) => return error_record(&display, "invalid filename"),
    };
    unsafe {
        let fd = libc::openat(
            dir_fd,
            cname.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return error_record(&display, &io_msg_scrub(&e));
        }
        let file = std::fs::File::from_raw_fd(fd);
        match file.metadata() {
            Ok(m) => stat_record(&display, &m, mode),
            Err(e) => error_record(&display, &io_msg_scrub(&e)),
        }
    }
}

#[allow(clippy::too_many_lines)]
fn read_dir_capped(
    dir_fd: libc::c_int,
    max: usize,
) -> std::io::Result<(Vec<std::ffi::OsString>, bool)> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    unsafe {
        let dir = libc::fdopendir(dir_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err(e);
        }
        let mut names: Vec<OsString> = Vec::new();
        let mut hit_cap = false;
        loop {
            // Reset errno; readdir returns NULL on both EOF and error.
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break; // Clean EOF.
                }
                libc::closedir(dir);
                return Err(e);
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            if names.len() >= max {
                hit_cap = true;
                break;
            }
            names.push(OsString::from_vec(name_bytes.to_vec()));
        }
        libc::closedir(dir);
        Ok((names, hit_cap))
    }
}

/// Recursive symlink-safe rmdir.  Descends from `parent` into `leaf`
/// (must be a directory; ELOOP if symlink), unlinks every entry, then
/// removes the directory itself.
fn remove_dir_recursive(parent_fd: libc::c_int, leaf: *const libc::c_char) -> std::io::Result<()> {
    unsafe {
        let dir_fd = libc::openat(
            parent_fd,
            leaf,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if dir_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Dup dir_fd so we can readdir on one copy and use the other for
        // unlinkat.  L-3 fix (2026-08-06): F_DUPFD_CLOEXEC — see the
        // fs_entries site for rationale.
        let dup_fd = libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0);
        if dup_fd < 0 {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err(e);
        }
        let dir = libc::fdopendir(dup_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            libc::close(dup_fd);
            return Err(e);
        }
        loop {
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break;
                }
                libc::closedir(dir);
                libc::close(dir_fd);
                return Err(e);
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            // Try file first; if it's a directory, recurse.
            let file_rc = libc::unlinkat(dir_fd, name_ptr, 0);
            if file_rc == 0 {
                continue;
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EISDIR) || e.raw_os_error() == Some(libc::EPERM) {
                if let Err(inner) = remove_dir_recursive(dir_fd, name_ptr) {
                    libc::closedir(dir);
                    libc::close(dir_fd);
                    return Err(inner);
                }
                continue;
            }
            libc::closedir(dir);
            libc::close(dir_fd);
            return Err(e);
        }
        libc::closedir(dir);
        libc::close(dir_fd);
        // Finally remove the directory itself.
        if libc::unlinkat(parent_fd, leaf, libc::AT_REMOVEDIR) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

async fn chown_impl(
    root: &std::path::Path,
    rel: String,
    owner: Option<String>,
    group: Option<String>,
    // H-5 fix (2026-08-06): expected (dev, inode) for the root
    // path — plumbed from the caller via
    // `self.handles.root_registry.get(&root_pb)`.  `None` skips
    // identity verification (used by test/fixture paths without
    // a boot-populated registry).
    expected_root_id: Option<(u64, u64)>,
) -> Par {
    use super::nss::{resolve_gid, resolve_uid};

    let uid = match owner {
        None => u32::MAX, // libc: -1 means "no change"
        Some(name) => match resolve_uid(&name) {
            Ok(Some(u)) => u,
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown user {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let gid = match group {
        None => u32::MAX,
        Some(name) => match resolve_gid(&name) {
            Ok(Some(g)) => g,
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown group {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let root_pb = root.to_path_buf();
    spawn_blocking(move || -> Par {
        let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
            Ok(p) => p,
            Err(qe) => {
                let (c, m) = quarantine_err_reply(&qe);
                return err(c, m);
            }
        };
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
            ok_bare()
        } else {
            let e = std::io::Error::last_os_error();
            err(io_err_code(&e), io_msg_scrub(&e))
        }
    })
    .await
    .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
}

/// Silence unused-import warning on AccessMode (used in Phase 5 by the
/// File-agent wiring).
#[allow(dead_code)]
fn _use_access_mode(_a: AccessMode) {}

/// Portable errno reset.  `readdir` returns NULL on both EOF and error;
/// distinguishing them requires clearing errno beforehand and checking
/// it after.  errno lives at platform-specific TLS addresses.
#[cfg(target_os = "macos")]
unsafe fn errno_reset() { *libc::__error() = 0; }

#[cfg(target_os = "linux")]
unsafe fn errno_reset() { *libc::__errno_location() = 0; }

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
unsafe fn errno_reset() {
    compile_error!("Unsupported platform for File I/O FIP native primitives");
}

// ---------------------------------------------------------------------
// Slice 26 review-fix tests: `resolve_cmode` (MT-26-1, ST-26-2).
// ---------------------------------------------------------------------

#[cfg(test)]
mod cmode_tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;
    use models::rust::utils::{new_boundvar_par, new_gbool_par, new_gint_par, new_gstring_par};

    use super::*;

    fn s(v: &str) -> Par { new_gstring_par(v.to_string(), Vec::new(), false) }
    fn nil() -> Par { Par::default() }
    fn i(n: i64) -> Par { new_gint_par(n, Vec::new(), false) }
    fn b(x: bool) -> Par { new_gbool_par(x, Vec::new(), false) }
    fn bytes() -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(vec![0u8, 1, 2])),
        }])
    }

    #[test]
    fn resolve_cmode_accepts_oracular_lowercase() {
        assert_eq!(resolve_cmode(&s("oracular")), Some(ConsensusMode::Oracular));
    }

    #[test]
    fn resolve_cmode_accepts_consensus_lowercase() {
        assert_eq!(
            resolve_cmode(&s("consensus")),
            Some(ConsensusMode::Consensus)
        );
    }

    // ST-26-2: case-sensitivity — capitalized / uppercase forms must
    // NOT be accepted.  A caller passing `"Consensus"` (mismatched
    // convention) MUST get rejected, not silently downgraded.
    #[test]
    fn resolve_cmode_rejects_capitalized() {
        assert_eq!(resolve_cmode(&s("Oracular")), None);
        assert_eq!(resolve_cmode(&s("Consensus")), None);
        assert_eq!(resolve_cmode(&s("CONSENSUS")), None);
    }

    // ST-26-2: whitespace-padded / trailing-space variants also
    // rejected.
    #[test]
    fn resolve_cmode_rejects_whitespace() {
        assert_eq!(resolve_cmode(&s(" oracular")), None);
        assert_eq!(resolve_cmode(&s("consensus ")), None);
        assert_eq!(resolve_cmode(&s("\toracular")), None);
        assert_eq!(resolve_cmode(&s("consensus\n")), None);
    }

    // MT-26-1: non-String Par shapes must all fall to None so the
    // handler surfaces FSERR_BAD_ARG.  Under the pre-fix fallback
    // behavior each of these would have silently defaulted to
    // Oracular.
    #[test]
    fn resolve_cmode_rejects_nil() {
        assert_eq!(resolve_cmode(&nil()), None);
    }

    #[test]
    fn resolve_cmode_rejects_int() {
        assert_eq!(resolve_cmode(&i(0)), None);
        assert_eq!(resolve_cmode(&i(1)), None);
    }

    #[test]
    fn resolve_cmode_rejects_bool() {
        assert_eq!(resolve_cmode(&b(true)), None);
        assert_eq!(resolve_cmode(&b(false)), None);
    }

    #[test]
    fn resolve_cmode_rejects_bytearray() {
        assert_eq!(resolve_cmode(&bytes()), None);
    }

    #[test]
    fn resolve_cmode_rejects_empty_string() {
        assert_eq!(resolve_cmode(&s("")), None);
    }

    #[test]
    fn resolve_cmode_rejects_unknown_string() {
        assert_eq!(resolve_cmode(&s("bogus")), None);
        assert_eq!(resolve_cmode(&s("oracle")), None);
        assert_eq!(resolve_cmode(&s("cons")), None);
    }

    #[test]
    fn resolve_cmode_rejects_boundvar_par() {
        // BoundVar par (an unbound Rholang variable position) must
        // also fall to None — RhoString::unapply returns None for it.
        let bv = new_boundvar_par(0, Vec::new(), false);
        assert_eq!(resolve_cmode(&bv), None);
    }

    // NT-26-3: pin `ConsensusMode::default()` so a future refactor
    // flipping the default would trip a test rather than silently
    // change every fallback direction.
    #[test]
    fn consensus_mode_default_is_consensus() {
        assert_eq!(ConsensusMode::default(), ConsensusMode::Consensus);
    }

    // Drift assertion for the shared constants (M-26-3): the
    // handler-side and composer-side constants MUST match byte-for-
    // byte or the composed source becomes unroutable.
    #[test]
    fn cmode_string_constants_are_stable() {
        assert_eq!(CMODE_ORACULAR_STR, "oracular");
        assert_eq!(CMODE_CONSENSUS_STR, "consensus");
    }

    // -- Phase 8 slice 8a — lock-native helpers -------------------------

    #[test]
    fn resolve_lock_mode_accepts_r_and_w() {
        assert_eq!(resolve_lock_mode(&s("r")), Some(LockMode::Read));
        assert_eq!(resolve_lock_mode(&s("w")), Some(LockMode::Write));
    }

    #[test]
    fn resolve_lock_mode_rejects_capitalized_and_padded() {
        assert_eq!(resolve_lock_mode(&s("R")), None);
        assert_eq!(resolve_lock_mode(&s("W")), None);
        assert_eq!(resolve_lock_mode(&s(" r")), None);
        assert_eq!(resolve_lock_mode(&s("r ")), None);
    }

    #[test]
    fn resolve_lock_mode_rejects_other_strings() {
        assert_eq!(resolve_lock_mode(&s("")), None);
        assert_eq!(resolve_lock_mode(&s("rw")), None);
        assert_eq!(resolve_lock_mode(&s("read")), None);
        assert_eq!(resolve_lock_mode(&s("write")), None);
        assert_eq!(resolve_lock_mode(&s("x")), None);
    }

    #[test]
    fn resolve_lock_mode_rejects_non_string_par() {
        // Fail-closed on any Par shape other than a String.  Mirrors
        // `resolve_cmode`'s discipline — a caller passing an Int or
        // Bool must not be silently downgraded to a default mode.
        assert_eq!(resolve_lock_mode(&nil()), None);
        assert_eq!(resolve_lock_mode(&i(0)), None);
        assert_eq!(resolve_lock_mode(&b(true)), None);
        assert_eq!(resolve_lock_mode(&bytes()), None);
    }

    #[test]
    fn holder_id_of_is_deterministic() {
        // Equal Pars must hash to the same HolderId across calls —
        // this is what makes `release_all_for_holder` work across
        // deploys.  A drift here would break File.close's ability to
        // release the specific cap's locks.
        let a = s("holder-1");
        let b = s("holder-1");
        assert_eq!(holder_id_of(&a), holder_id_of(&b));
    }

    #[test]
    fn holder_id_of_distinguishes_different_pars() {
        assert_ne!(holder_id_of(&s("holder-1")), holder_id_of(&s("holder-2")));
        assert_ne!(holder_id_of(&s("holder")), holder_id_of(&nil()));
        assert_ne!(holder_id_of(&i(1)), holder_id_of(&i(2)));
    }

    #[test]
    fn holder_id_of_hash_width_contract() {
        // The module docstring + runtime assertion require Blake2b256
        // → 32 bytes.  Pinned here so a provider swap producing a
        // shorter digest is caught at test time rather than at first
        // runtime call.
        let h = holder_id_of(&s("any-par"));
        assert_eq!(h.bytes.len(), 32);
    }
}
