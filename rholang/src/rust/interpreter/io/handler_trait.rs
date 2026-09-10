// Wave 3 S3.1 (2026-09-08): typed dispatch infrastructure for the
// 28 native filesystem handlers.  This module defines:
//
//   - `HandlerReply` — typed success/failure discriminator for a
//     handler's reply.  Both variants wrap a `Par`; construction
//     helpers keep the byte-level reply shape identical to the
//     pre-refactor `err(...)` / `ok_*(...)` calls in `response.rs`.
//   - `SyscallCtx<'a>` — borrowed view of `FsProcesses` threaded
//     into every `FsHandler::dispatch` call, replacing the
//     `&self` implicit context.
//   - `FsHandler` trait — one impl per fs_* syscall.  Each impl
//     declares `NAME`, `ARITY`, `VERIFYING`, cost, arg-parse, and
//     the dispatch body.  Handler bodies return `HandlerReply`;
//     the framework wraps into `vec![reply.into_par()]` and
//     produces to the caller's ack channel.
//   - `FsHandlerEntry` + `FS_HANDLERS` — `linkme` distributed
//     slice populated by each migrated handler's registration.
//     The slice is consumed by `rho_runtime.rs` after all 28
//     handlers migrate (wave 3 S3.12) to replace the current
//     28-way `fs_native_def` block.
//   - `dispatch_via_trait<H>` — the generic framework loop.  A
//     migrated handler's `FsProcesses::fs_x` wrapper is one line:
//     `dispatch_via_trait::<FsXHandler>(self, contract_args).await`.
//
// See `FIPS/fileio/under-review/2026-07-24-File-IO/wave-3-plan.md`
// (uncommitted, per `feedback_fips_commit_scope.md`) for the
// per-session migration sequence.  Locked design decisions live
// in that doc's § "Design decisions (LOCKED)".
//
// # Coexistence with the pre-trait pattern (waves S3.1–S3.11)
//
// During the multi-session migration, the old
// `pub async fn fs_x(&self, ...)` methods on `FsProcesses` are
// retained.  Each migrated handler's method body collapses to a
// single `dispatch_via_trait::<H>(...)` call; unmigrated methods
// keep their original inline body.  `rho_runtime.rs` continues to
// dispatch through `sp.fs.fs_x(...).await` for every handler.
// This means until S3.12 the FS_HANDLERS distributed slice is a
// pin-only surface (a compile-time proof that N of 28 handlers
// have completed migration), NOT the live dispatch path.
//
// The pattern change lands atomically at S3.12: rho_runtime.rs
// switches to a `FS_HANDLERS.iter()` loop, and every
// `FsProcesses::fs_x` wrapper is retired.  Session S3.12 also
// extends the `handlers_top_comment_*` pins in `fileio_cost_spec.
// rs` to walk the slice.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::{ListParWithRandom, Par};

use super::super::accounting::costs::Cost;
use super::super::dispatch::RhoDispatch;
use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::metering::MeteredMachine;
use super::super::rho_runtime::RhoISpace;
use super::handle_table::FileHandleTable;
use super::handlers::FsProcesses;
use super::{response, ConsensusMode};

// ------------------------------------------------------------------
// HandlerReply
// ------------------------------------------------------------------

/// Typed success/failure discriminator for a handler's reply.
///
/// Both variants wrap a `Par` produced by the existing helpers in
/// `response.rs` (`ok_bare`, `ok_u64`, `err`, etc.), so the
/// byte-identical reply shape is preserved across the migration —
/// no consensus-observable change.
///
/// The framework layer branches on the variant when it needs to
/// distinguish success from failure without decoding the Par
/// (currently only for review readability; a future session could
/// use it for WAL Failure journaling drift-checks, metrics, etc.).
pub enum HandlerReply {
    Ok(Par),
    Err(Par),
}

impl HandlerReply {
    /// Success reply — the caller supplies the already-built Par
    /// (via `response::ok_bare`, `response::ok_u64`, etc.).
    pub fn ok(p: Par) -> Self { HandlerReply::Ok(p) }

    /// Failure reply built via `response::err`.  Matches the
    /// `[false, code, msg]` shape emitted by the pre-refactor
    /// `err(FSERR_X, "...")` calls.
    pub fn err(code: &'static str, msg: impl Into<String>) -> Self {
        HandlerReply::Err(response::err(code, msg))
    }

    /// `Box::new(HandlerReply::err(...))` in one call.  The
    /// `FsHandler::parse_content` return type
    /// (`Result<Args, Box<HandlerReply>>`) boxes the Err arm to
    /// satisfy `clippy::result_large_err` (Par is ~296 bytes; a
    /// bare `Result<T, HandlerReply>` fires the pedantic lint).
    /// This helper spares each handler from writing
    /// `Err(Box::new(HandlerReply::err(...)))` at every bad-arg
    /// site.
    pub fn boxed_err(code: &'static str, msg: impl Into<String>) -> Box<Self> {
        Box::new(HandlerReply::err(code, msg))
    }

    /// Consume the reply into its inner `Par`.  Framework produces
    /// `vec![reply.into_par()]` to the ack channel.
    pub fn into_par(self) -> Par {
        match self {
            HandlerReply::Ok(p) => p,
            HandlerReply::Err(p) => p,
        }
    }

    /// True for the success variant.  Not used inside this module;
    /// reserved for downstream consumers of `HandlerReply`.
    pub fn is_ok(&self) -> bool { matches!(self, HandlerReply::Ok(_)) }
}

// ------------------------------------------------------------------
// SyscallCtx
// ------------------------------------------------------------------

/// Borrowed view of the FsProcesses fields a handler body touches:
/// dispatcher, space, handles, mode, metering — plus the caller's
/// ack channel (needed by handlers that journal to the WAL, which
/// keys entries on the ack channel hash).  Constructed by the
/// framework via `SyscallCtx::new(fs, ack)` and passed to
/// `FsHandler::dispatch`.
///
/// Sharing shape:
///   - `dispatcher`, `space`, `metering` — currently the `.clone()`
///     hop lives inside the framework (or the handler body); the
///     borrow here is by-ref because the wrapping `dispatch_via_
///     trait` future already keeps `&FsProcesses` alive across
///     `.await` boundaries.
///   - `handles` — `FileHandleTable` is `Clone` internally
///     (Arc-based); a handler that needs to move it into a
///     `spawn_blocking` closure clones the field.
///   - `mode` — `Copy`.
///   - `ack` — the caller-supplied ack channel Par (last positional
///     arg).  Used by handlers that journal (WAL keys on
///     `ack_channel_hash(ack)`).  Non-journaling handlers ignore it.
pub struct SyscallCtx<'a> {
    pub dispatcher: &'a RhoDispatch,
    pub space: &'a RhoISpace,
    pub handles: &'a FileHandleTable,
    pub mode: ConsensusMode,
    pub metering: &'a MeteredMachine,
    pub ack: &'a Par,
}

impl<'a> SyscallCtx<'a> {
    /// Framework construction site.  Wave-3 S3.4 (2026-09-08)
    /// replaced the pre-existing `From<&FsProcesses>` because
    /// SyscallCtx now also carries the caller's ack channel Par
    /// (WAL journaling needs it).  The `ack` reference borrows into
    /// the caller's owned `Par` stashed inside `dispatch_via_trait`.
    pub fn new(fs: &'a FsProcesses, ack: &'a Par) -> Self {
        SyscallCtx {
            dispatcher: &fs.dispatcher,
            space: &fs.space,
            handles: &fs.handles,
            mode: fs.mode,
            metering: &fs.metering,
            ack,
        }
    }

    /// Read the per-runtime "current deploy scope" cell — the same
    /// value `FsProcesses::current_deploy_scope` returns.  Used by
    /// lock-acquire handlers to tag `LockRegistry` entries for
    /// deploy-end sweep.  Sentinel `[0; 32]` = no deploy in flight
    /// (test / genesis path).
    pub fn current_deploy_scope(&self) -> [u8; 32] {
        *self
            .handles
            .current_deploy_scope
            .read()
            .expect("current_deploy_scope RwLock poisoned")
    }
}

// ------------------------------------------------------------------
// FsHandler trait
// ------------------------------------------------------------------

/// One impl per fs_* syscall.  Framework calls (in order):
///
///   1. `pre_charge_cost()` — reserved via
///      `metering.reserve_primitive`.  Charged BEFORE unapply so
///      an argument-shape rejection still costs the caller.
///   2. `is_contract_call().unapply(contract_args)` — the
///      framework layer.  Returns `illegal_argument_error(NAME)`
///      on shape mismatch.
///   3. Arity check — `args.len() == ARITY`, else
///      `illegal_argument_error(NAME)`.  Ack is the last Par.
///   4. Non-verifying is_replay tautology (VERIFYING = false):
///      produce `previous` to ack, return `Ok(previous)`.
///      Verifying handlers hook the replay branch through a
///      framework extension landing in wave-3 S3.5.
///   5. `parse_content(&args[..ARITY-1])` — extract typed args
///      from the non-ack slots.  Returns
///      `Err(HandlerReply::err(...))` on content mismatch (framework
///      produces the reply and returns `Ok(vec![reply])`).
///   6. `dispatch(ctx, args)` — the syscall body.  Returns a
///      `HandlerReply`.  Framework produces
///      `vec![reply.into_par()]` to ack.
///
/// A migrated handler's `FsProcesses::fs_x` wrapper is one line:
///
/// ```ignore
/// pub async fn fs_flush(
///     &self,
///     contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
/// ) -> Result<Vec<Par>, InterpreterError> {
///     dispatch_via_trait::<FsFlushHandler>(self, contract_args).await
/// }
/// ```
pub trait FsHandler {
    /// Human-readable handler name.  Passed to
    /// `illegal_argument_error(NAME)` on framework-level shape
    /// mismatch and stored as `FsHandlerEntry.name`.
    const NAME: &'static str;

    /// Total argument count including the trailing ack channel.
    /// The pre-ack args are `args[..ARITY-1]`; ack is `args[ARITY-1]`.
    const ARITY: usize;

    /// `false` = non-verifying (tautological echo on `is_replay`).
    /// `true` = re-execute + verify reply hash against leader's
    /// cached reply.  15 of 28 handlers are verifying; see
    /// `docs/consensus-invariants.md § Per-op re-execute behavior`.
    ///
    /// The framework's `is_replay` branch currently only handles
    /// non-verifying dispatch (wave-3 S3.1).  Verifying dispatch
    /// lands in wave-3 S3.5; registering a `VERIFYING = true`
    /// handler before then fires an `unreachable!` guard in
    /// `dispatch_via_trait`.
    const VERIFYING: bool = false;

    /// Parsed content-arg type produced by `parse_content` and
    /// consumed by `dispatch`.  `Send + 'static` so it can move
    /// into the async block.
    type Args: Send + 'static;

    /// Extract typed args from `args[..ARITY-1]` (the framework
    /// has already sliced off ack).  Returns
    /// `Err(HandlerReply::boxed_err(FSERR_BAD_ARG, ...))` on
    /// type-level mismatch (e.g., an integer slot got a string
    /// Par); the framework produces the reply to ack and returns
    /// `Ok(vec![reply])`.
    ///
    /// The Err arm is boxed because `HandlerReply` wraps a Par
    /// (~296 bytes); `Result<T, HandlerReply>` fires
    /// `clippy::result_large_err` at the pedantic threshold.
    /// Handlers use `HandlerReply::boxed_err(code, msg)` for the
    /// one-line construction.
    fn parse_content(args: &[Par]) -> Result<Self::Args, Box<HandlerReply>>;

    /// Handler's constant-work cost weight.  Charged via
    /// `metering.reserve_primitive` when
    /// `pre_charge_incremental` returns None (default).
    ///
    /// Wave-3 S3.6 (2026-09-09): the framework moved the pre-charge
    /// call from PRE-unapply to POST-unapply-POST-arity — matches
    /// pre-refactor `fs_read` / `fs_read_at` which post-unapply
    /// charge a length-parameterized cost.  For constant-work
    /// handlers the ordering shift is unobservable under
    /// determinism (a caller-misuse `contract_args.0.len() != 1`
    /// or `args.len() != ARITY` is unreachable via the
    /// deterministic ContractCall dispatcher).
    fn pre_charge_cost() -> Cost;

    /// Length-parameterized cost override.  Returns
    /// `Some(cost)` for handlers whose cost depends on caller-
    /// supplied args (fs_read / fs_read_at / fs_write / fs_write_
    /// at / fs_entries etc.).  Framework then uses
    /// `metering.reserve_incremental_primitive(cost)` (which allows
    /// zero-weight, unlike `reserve_primitive`).  Called with the
    /// raw pre-ack args.  Default: None — framework falls back to
    /// `pre_charge_cost()`.
    ///
    /// This is a SINGLE-EVENT charge, not a base + supplement.
    /// Pre-refactor `fs_read` emits ONE `BillableTokenEvent::
    /// Primitive` per invocation; splitting into two events would
    /// change the authority_cost_witness fold bytes and split
    /// peering.
    fn pre_charge_incremental(_raw_args: &[Par]) -> Option<Cost> { None }

    /// Syscall + reply generation.  Returns a `HandlerReply`;
    /// the framework handles produce/ack.  The future's lifetime
    /// is bounded by the passed `SyscallCtx<'a>`.
    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: Self::Args,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>>;

    /// Optional pre-syscall hook — runs AFTER `parse_content` but
    /// BEFORE `dispatch` on the leader path + the Consensus-follower
    /// verifying-replay fall-through path.  Framework SKIPS the hook
    /// on the Oracular tautological echo path (parse_content isn't
    /// called on that path either).
    ///
    /// Wave-3 S3.7 (2026-09-09) introduced this hook for the
    /// path-mutation family (fs_chmod / fs_chown / fs_truncate /
    /// fs_write / fs_rename / fs_copy_file / fs_remove_file /
    /// fs_remove_dir) — these handlers pre-append a WAL entry with
    /// a `WalOutcome::Success` placeholder BEFORE running the
    /// syscall; the placeholder is patched to
    /// `WalOutcome::Failure { code }` by `journal` after the outcome
    /// is known (via `finalize_failure_journal_via_table`).
    ///
    /// Returns `Err(boxed_reply)` to produce the reply and return
    /// early (used for `FSERR_QUOTA_EXCEEDED` when the WAL is at
    /// cap).  Default: no-op `Ok(())`.
    ///
    /// Oracular-path skip: pre-refactor Oracular followers DID call
    /// the pre-syscall journal helpers, but those helpers self-guard
    /// on Consensus (`_ => Ok(false)`).  Framework skipping the hook
    /// on Oracular echo preserves byte-identity — the only
    /// observable difference is fewer fn calls.
    fn pre_syscall<'a>(
        _ctx: SyscallCtx<'a>,
        _args: &'a Self::Args,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    /// Optional side effect on the non-verifying `is_replay` branch
    /// BEFORE the tautological echo of `previous` to ack.  Default:
    /// no-op.
    ///
    /// Callers:
    /// - `fs_close`, `fs_entries_stream_close`: Phase-2 fd-release
    ///   (2026-09-01, in `docs/consensus-invariants.md`).  Follower
    ///   removes its shadow fd on replay, matching the leader's
    ///   post-close state.
    /// - `fs_entries_stream_open`: shadow-fd INSERT on replay so
    ///   downstream replay-branch handlers (stream_next / _close)
    ///   can look up `(cmode, canon_path)` from the leader's cached
    ///   `[true, fd]` reply.  Uses `previous` (the leader's cached
    ///   reply, containing the fd) — hence this hook takes both
    ///   `raw_args` and `previous`.
    /// - `fs_entries_stream_next`: per-entry cost supplement + WAL
    ///   journaling on replay, driven by the shape of `previous`.
    ///
    /// Takes the raw pre-ack Par slice (`args[..ARITY-1]`) rather
    /// than a parsed `Self::Args`.  Rationale: matches the
    /// pre-refactor `is_replay` branch of the handler bodies,
    /// which parsed args OPPORTUNISTICALLY on replay (`if let
    /// Some(fd) = RhoNumber::unapply(fd_par)`) so a follower
    /// seeing an unparseable arg still echoes `previous` (the
    /// leader's cached reply) rather than surfacing an
    /// `FSERR_BAD_ARG` reply that would diverge from what the
    /// leader produced.  Running parse_content before the
    /// is_replay short-circuit would flip that behavior — a
    /// consensus regression.
    ///
    /// Ack is available on `ctx.ack` if the hook needs to journal.
    fn on_replay_side_effect<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }

    /// Optional post-reply cost supplement — for length-parameterized
    /// handlers (fs_entries, fs_entries_stream_next, and later
    /// fs_read/fs_write/etc. in S3.5+).  Default: no supplement.
    ///
    /// Called by the framework AFTER `dispatch` (or after the
    /// `is_replay` echo, using `previous` as the reply slice).
    /// Returns `Some(cost)` to charge via
    /// `metering.reserve_incremental_primitive`; a failure to
    /// reserve propagates as `InterpreterError` from
    /// `dispatch_via_trait`.
    ///
    /// `reply` is the caller-facing reply slice — a 1-element
    /// `&[Par]` containing the reply Par.  Handlers inspect its
    /// shape (via `reply_is_ok` or similar) to compute the byte /
    /// entry count that drives the supplement weight.
    fn post_reply_supplement(_reply: &[Par]) -> Option<Cost> { None }

    /// For verifying handlers (`VERIFYING = true`): determine the
    /// caller-supplied ConsensusMode from the raw pre-ack args or
    /// a context lookup (fs_size reads the fd's shadow).  Called
    /// by the framework's is_replay branch to decide between the
    /// Oracular tautological echo path and the Consensus
    /// re-execute + verify path.
    ///
    /// Returns None if cmode can't be determined (e.g., bad cmode
    /// string on fs_stat, unknown fd on fs_size).  In that case
    /// the framework takes the Oracular path — matches pre-refactor
    /// behavior where an unresolved cmode on replay tautologically
    /// echoed `previous`.
    ///
    /// Non-verifying handlers do NOT override this; the framework
    /// skips the call.
    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        Box::pin(async { None })
    }

    /// For verifying handlers that journal to the WAL (fs_stat,
    /// fs_size, fs_exists, fs_read, fs_write, fs_entries, etc.):
    /// journal the reply to the WAL and perform any handler-
    /// specific state advance (shadow position for fs_read /
    /// fs_write).  Framework calls this at 4 semantic sites —
    /// discriminated by `JournalPath`:
    ///   - `Leader`: post-dispatch on the leader path.
    ///   - `VerifySuccess`: post-verify on Consensus follower.
    ///   - `VerifyDivergence`: post-verify-failure on Consensus
    ///     follower.  Carries BOTH the fresh syscall reply (for
    ///     state advance — fs_read advances shadow position by
    ///     the actually-read bytes even though the produce Par
    ///     is the divergence-err) AND the framework-built
    ///     `consensus_divergence_reply` (for journal payload).
    ///   - `OracularEcho`: post-arity on the non-verifying /
    ///     Oracular-cmode replay path.  Carries only the
    ///     cached leader reply from `previous.first()`.
    ///
    /// Default: no-op.  Non-journaling handlers skip.
    fn journal<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
}

/// Discriminates the 4 framework paths at which `FsHandler::journal`
/// fires.  See `FsHandler::journal` doc-comment for the full site
/// enumeration.  Handlers pattern-match on the path to select the
/// correct WAL-entry shape (fs_read's success-vs-divergence entries
/// differ in payload_ref) and the correct state-advance source
/// (fs_read advances shadow by the actually-read bytes even on
/// verify-divergence, when the produced reply is the divergence-err).
pub enum JournalPath<'a> {
    /// `is_replay=false` — leader path after successful dispatch.
    /// `fresh_reply` is what will be produced to ack.
    Leader { fresh_reply: &'a Par },
    /// `is_replay=true` + `VERIFYING=true` + Consensus cmode +
    /// verify_reply_hash_matches_cached succeeded.  `fresh_reply`
    /// is what will be produced.
    VerifySuccess { fresh_reply: &'a Par },
    /// `is_replay=true` + `VERIFYING=true` + Consensus cmode +
    /// verify_reply_hash_matches_cached failed.  `divergence_reply`
    /// is what will be produced; `fresh_reply` is the actual
    /// syscall outcome for state-advance purposes.
    VerifyDivergence {
        fresh_reply: &'a Par,
        divergence_reply: &'a Par,
    },
    /// Oracular tautological echo — either non-verifying handler
    /// on `is_replay=true`, or verifying handler with Oracular
    /// cmode.  `previous_reply` is `previous.first()` (the leader's
    /// cached play-time reply).
    OracularEcho { previous_reply: &'a Par },
}

impl<'a> JournalPath<'a> {
    /// The reply Par that will be produced to ack.  Handlers that
    /// journal the produced reply (fs_stat / fs_size / fs_exists)
    /// use this.
    pub fn produce_reply(&self) -> &'a Par {
        match *self {
            JournalPath::Leader { fresh_reply } => fresh_reply,
            JournalPath::VerifySuccess { fresh_reply } => fresh_reply,
            JournalPath::VerifyDivergence {
                divergence_reply, ..
            } => divergence_reply,
            JournalPath::OracularEcho { previous_reply } => previous_reply,
        }
    }

    /// The reply Par whose bytes/count drive shadow state advance.
    /// Same as `produce_reply` EXCEPT on `VerifyDivergence`, where
    /// it's the FRESH syscall reply (actual bytes read/written)
    /// rather than the divergence-err reply.
    pub fn state_source_reply(&self) -> &'a Par {
        match *self {
            JournalPath::Leader { fresh_reply } => fresh_reply,
            JournalPath::VerifySuccess { fresh_reply } => fresh_reply,
            JournalPath::VerifyDivergence { fresh_reply, .. } => fresh_reply,
            JournalPath::OracularEcho { previous_reply } => previous_reply,
        }
    }

    /// True iff this path is a verify-failure.  Handlers whose
    /// divergence-journal shape differs from their success-journal
    /// shape (fs_read: `journal_read_divergence` vs `journal_read`)
    /// use this.
    pub fn is_divergence(&self) -> bool { matches!(self, JournalPath::VerifyDivergence { .. }) }
}

// ------------------------------------------------------------------
// FsHandlerEntry + FS_HANDLERS distributed slice
// ------------------------------------------------------------------

/// Handler family — used by the per-family count pin in
/// `fileio_cost_spec.rs` and by future `handlers_{family}.rs` file
/// splits (wave-3 S3.13).  Grouped by the shape of the syscall's
/// effect on state and on the WAL:
///
///   - `Mutation`: writes state.  8 handlers migrated + 1 exempt
///     (fs_remove_dir).  Includes fs_write, fs_write_at, fs_truncate,
///     fs_chmod, fs_chown, fs_remove_file, fs_rename, fs_copy_file.
///     WAL-journaling by default; some (fs_chown under Consensus,
///     fs_chmod / fs_rename / fs_copy_file / fs_truncate / fs_write*)
///     verify replies across leader / follower.
///
///   - `Observation`: reads state without mutating.  9 handlers.
///     Includes fs_read, fs_read_at, fs_stat, fs_entries, fs_size,
///     fs_seek, fs_exists, fs_flush, fs_tell.  Some (fs_stat,
///     fs_entries, fs_exists, fs_size) verify replies; fs_read,
///     fs_read_at, fs_seek are shape-observation-only and don't
///     verify.
///
///   - `Stream`: per-fd directory-entries streaming primitives.
///     3 handlers: fs_entries_stream_open / _next / _close.
///     fs_entries_stream_next is the only verifying streaming
///     handler (per-Next reply verified).
///
///   - `Lock`: byte-range and sequential lock helpers.  4
///     handlers: fs_lock_range, fs_lock_sequential, fs_release_lock,
///     fs_release_all_for_holder.  Non-verifying (LockRegistry is
///     host-local).
///
///   - `Lifecycle`: file / cap creation + retirement.  3 handlers:
///     fs_open, fs_close, fs_quarantine.  Non-verifying; fs_open
///     has `on_replay_side_effect` for shadow-fd installation.
///
/// Total: 8+9+3+4+3 = 27 migrated FS_HANDLERS + 1 trait-exempt
/// (fs_remove_dir, mutation family) = 28.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HandlerFamily {
    Mutation,
    Observation,
    Stream,
    Lock,
    Lifecycle,
}

/// One entry per migrated handler in the `FS_HANDLERS` distributed
/// slice.  Structured to be usable by rho_runtime.rs's future
/// registration walk (wave-3 S3.12).
///
/// `dispatch` takes an owned `FsProcesses` (which is `Clone` via
/// Arc-wrapped fields, cheap) to avoid lifetime complexity in the
/// `fn` pointer signature — matching the shape of `fs_native_def`
/// in rho_runtime.rs which threads `SystemProcesses` by value.
pub struct FsHandlerEntry {
    /// Handler name (`fs_flush`, `fs_read`, etc.) — same as
    /// `<H as FsHandler>::NAME`.  Stored explicitly (not derived
    /// from the fn pointer) so the pin can walk the slice without
    /// evaluating any of the fn pointers.
    pub name: &'static str,

    /// Total arity including ack — same as `<H as FsHandler>::ARITY`.
    /// Used at S3.12 to construct the `fs_native_def(..., arity, ...)`
    /// call from the slice.
    pub arity: usize,

    /// Verifying flag — same as `<H as FsHandler>::VERIFYING`.
    /// Used by the pin
    /// `verifying_handler_count_matches_pinned_15` (landing at
    /// wave-3 S3.11) to guard the 15/28 verify-matrix invariant.
    pub verifying: bool,

    /// The type-erased dispatch fn.  Each per-handler registration
    /// site coerces a non-capturing `|fs, args| Box::pin(...)`
    /// closure into this fn-pointer type.
    pub dispatch: fn(
        FsProcesses,
        (Vec<ListParWithRandom>, bool, Vec<Par>),
    )
        -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>,

    /// URN suffix appended to `"rho:io:fs:native:1.0.0/"` when
    /// registering the handler at genesis (S3.12).  E.g. `"open"`,
    /// `"readAt"`, `"removeFile"`.  camelCase per the pre-S3.12
    /// `fs_native_def` call sites in `rho_runtime.rs`; the composed
    /// source at `fs_genesis.rs` expects the exact same string.
    ///
    /// Pinned across the S3.12 refactor: an entry whose `urn_suffix`
    /// drifts from the pre-S3.12 hard-coded string in
    /// `rho_runtime.rs` would silently rename the URN, breaking
    /// URN-map lookups from `Fs.rho`.  Guarded by the pre-S3.12
    /// URN-suffix pin in `fs_genesis.rs`.
    pub urn_suffix: &'static str,

    /// Fixed-channel constructor: `FixedChannels::fs_x` fn pointer.
    /// Loop body at S3.12 calls `(entry.fixed_channel)()` to obtain
    /// the `Par` byte-name used as the rendezvous channel.
    pub fixed_channel: fn() -> super::super::system_processes::Name,

    /// BodyRef constant: `BodyRefs::FS_X`.  Rholang's built-in
    /// dispatch table keys handler resolution on this i64.
    pub body_ref: super::super::system_processes::BodyRef,

    /// Handler family.  Groups handlers by effect shape; used by
    /// the per-family count pin (`fs_handlers_family_counts_
    /// match_pinned` in fileio_cost_spec.rs) and by the S3.13 file
    /// split into `handlers_{family}.rs`.  See `HandlerFamily`
    /// doc-comment for the taxonomy.
    pub family: HandlerFamily,
}

/// Distributed slice of every migrated handler's `FsHandlerEntry`.
/// Populated at link time by `#[distributed_slice(FS_HANDLERS)]
/// static ...` declarations in `handlers.rs` (or, post-S3.13, in
/// `handlers_{family}.rs`).
///
/// `linkme` truncation guard: see `EXPECTED_MIGRATED_HANDLER_COUNT`
/// below.  During wave 3 the count moves as sessions migrate more
/// handlers; the pin fires if a migrated handler goes missing
/// (e.g., cdylib LTO drops the linker section).  At S3.12 the count
/// reaches 28 and stays there.
#[distributed_slice]
pub static FS_HANDLERS: [FsHandlerEntry] = [..];

/// Count of migrated handlers.  Bump this at every session that
/// migrates a handler; the pin
/// `fs_handlers_count_matches_migrated_pinned` fires on drift.
///
/// Wave-3 progression:
///   - S3.1 (2026-09-08): +1 (fs_flush).  Count = 1.
///   - S3.2 (2026-09-08): +3 (fs_tell, fs_close, fs_release_lock).
///     Count = 4.
///   - S3.3 (2026-09-08): +4 (fs_quarantine, fs_entries_stream_close,
///     fs_lock_range, fs_lock_sequential).  Count = 8.
///   - S3.4 (2026-09-09): +2 (fs_entries_stream_open,
///     fs_entries_stream_next).  Count = 10.  Stream family complete.
///   - S3.5 (2026-09-09): +3 (fs_size, fs_stat, fs_exists).
///     Count = 13.  First verifying handlers; framework extended for
///     the re-execute + verify_reply_hash_matches_cached path.
///   - S3.6 (2026-09-09): +3 (fs_read, fs_read_at, fs_seek).
///     Count = 16.  Verifying observation handlers with length-
///     parameterized cost (fs_read / fs_read_at) or shadow-position
///     state advance (fs_read / fs_read_at / fs_seek).  Framework
///     extended with `pre_charge_incremental` + `JournalPath` enum.
///   - S3.7 (2026-09-09): +3 (fs_chown, fs_chmod, fs_truncate).
///     Count = 19.  Mutation handlers with pre-append WAL journal
///     via new `pre_syscall` framework hook.  fs_chown is
///     non-verifying (Consensus banned at parse); fs_chmod +
///     fs_truncate verify.
///   - S3.8 (2026-09-09): +2 (fs_write, fs_write_at).  Count = 21.
///     Verifying mutation with byte payload; both use length-
///     parameterized cost + pre-append WAL + finalize_write on
///     partial write.  fs_write advances shadow position on all
///     4 paths; fs_write_at doesn't (pwrite semantics).
///   - S3.9 (2026-09-09): +3 (fs_entries, fs_rename, fs_copy_file).
///     Count = 24.  fs_entries: verifying observation with
///     two-event cost (setup + per-entry supplement).  fs_rename +
///     fs_copy_file: verifying mutation with 2-path journal via
///     new `journal_path_mutation_two_via_table` free-fn.
///   - S3.10 (2026-09-09): +2 (fs_open, fs_remove_file).  Count =
///     26.  fs_open: non-verifying lifecycle (most intricate
///     handler) with on_replay_side_effect installing a shadow
///     FileHandle + Phase-2 real-open on Consensus caps.
///     fs_remove_file: verifying path-mutation with lock-registry
///     gate.
///   - S3.11 (2026-09-09): +1 (fs_release_all_for_holder).  Count
///     = 27.  Non-verifying lifecycle (tautological echo on replay);
///     cancel-first / release-second ordering preserved from the
///     WalDeployScope::drop B1 fix.  fs_remove_dir stays trait-
///     exempt — its 4 divergence reply shapes (DD-RemoveDir
///     4-element vs 5-element err_with_manifest) + per-entry WAL
///     journaling inside the recursive Consensus walk don't fit
///     the pre_syscall/journal hook contract without adding a
///     one-off `divergence_reply(args, reason)` trait method used
///     by only this handler.  Documented in wave-3-plan.md § S3.11.
///   - ... (see wave-3-plan.md § Sessions).
///   - S3.12: reaches 27 migrated + fs_remove_dir exempt (28 total).
pub const EXPECTED_MIGRATED_HANDLER_COUNT: usize = 27;

// ------------------------------------------------------------------
// Framework loop: dispatch_via_trait
// ------------------------------------------------------------------

/// Owned-`FsProcesses` adapter around `dispatch_via_trait`, used as
/// the fn-pointer body in every `#[distributed_slice(FS_HANDLERS)]
/// static FS_X_ENTRY` registration.  `FsHandlerEntry.dispatch` is a
/// plain `fn(FsProcesses, ...) -> Pin<Box<...>>` — no lifetimes on
/// the pointer type — so the closure body owns `fs` for the future's
/// lifetime and borrows into it for the trait dispatch call.
///
/// Non-capturing generic fn (rather than a closure captured in each
/// registration site) so the pointer coercion at the static's init
/// expression is uniform across all 28 handler entries.
pub async fn dispatch_via_trait_owned<H: FsHandler>(
    fs: FsProcesses,
    contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
) -> Result<Vec<Par>, InterpreterError> {
    dispatch_via_trait::<H>(&fs, contract_args).await
}

/// The generic framework loop.  Called by each migrated handler's
/// `FsProcesses::fs_x` wrapper and by every `FS_HANDLERS.dispatch`
/// entry (via `dispatch_via_trait_owned`).  See the `FsHandler`
/// doc-comment for the step-by-step dispatch order.
pub async fn dispatch_via_trait<H: FsHandler>(
    fs: &FsProcesses,
    contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
) -> Result<Vec<Par>, InterpreterError> {
    // Step 1: unapply.
    let Some((produce, is_replay, previous, args)) = fs.is_contract_call().unapply(contract_args)
    else {
        return Err(illegal_argument_error(H::NAME));
    };

    // Step 2: arity check.  Framework rejects with
    // `illegal_argument_error`; matches the current
    // `let [x, y, ack] = args.as_slice() else {
    //     return Err(illegal_argument_error("fs_x")); }` pattern.
    if args.len() != H::ARITY {
        return Err(illegal_argument_error(H::NAME));
    }

    // Ack is the last Par.  Cloned because the produce closure takes
    // `&Par`; the clone cost is a Par-header-plus-Arc bump, not a
    // deep copy.
    let ack = args[H::ARITY - 1].clone();

    let raw_pre_ack: &[Par] = &args[..H::ARITY - 1];

    // Step 3: cost pre-charge — post-unapply-post-arity.  Wave-3
    // S3.6 (2026-09-09) moved this from pre-unapply to match
    // pre-refactor `fs_read` / `fs_read_at` semantics (length-
    // parameterized weights require the args to compute).  For
    // constant-work handlers the ordering shift is unobservable
    // under determinism (the pre-arity `illegal_argument_error`
    // path is unreachable via the deterministic ContractCall
    // dispatcher).  Handlers with a length-parameterized weight
    // override `pre_charge_incremental` to return `Some(cost)` —
    // framework uses `reserve_incremental_primitive` (which
    // allows zero-weight, unlike `reserve_primitive`).  Emits a
    // SINGLE BillableTokenEvent per invocation, matching
    // pre-refactor's single `reserve_incremental_primitive(base
    // + n*per_byte)` call — splitting into two events would
    // change the authority_cost_witness fold bytes.
    match H::pre_charge_incremental(raw_pre_ack) {
        Some(cost) => fs.metering.reserve_incremental_primitive(cost)?,
        None => fs.metering.reserve_primitive(H::pre_charge_cost())?,
    }

    // Step 4: is_replay short-circuit.
    if is_replay {
        // Verifying-follower cmode dispatch (S3.5+, 2026-09-09).
        // The framework resolves the handler's effective cmode from
        // raw args + ctx (per-handler via `resolve_replay_cmode`).
        // Under Oracular / unresolved cmode we take the tautological
        // echo path — matches the pre-refactor
        // `if is_replay && mode != Consensus { echo previous }`
        // short-circuit.  Under Consensus we fall through to
        // re-execute + verify below.
        let verifying_consensus_replay = if H::VERIFYING {
            let replay_cmode =
                H::resolve_replay_cmode(SyscallCtx::new(fs, &ack), raw_pre_ack).await;
            replay_cmode == Some(ConsensusMode::Consensus)
        } else {
            false
        };

        if !verifying_consensus_replay {
            // Non-verifying replay OR verifying Oracular/unresolved
            // replay.  Run the optional side-effect hook, then
            // tautologically echo previous.
            H::on_replay_side_effect(SyscallCtx::new(fs, &ack), raw_pre_ack, &previous).await;
            // For verifying handlers on the Oracular tautological
            // path, journal `previous.first()` — matches pre-refactor
            // structural parity.  journal_state_read self-guards on
            // Consensus so this is a WAL no-op for Oracular caps.
            if H::VERIFYING {
                if let Some(previous_reply) = previous.first() {
                    H::journal(
                        SyscallCtx::new(fs, &ack),
                        raw_pre_ack,
                        JournalPath::OracularEcho { previous_reply },
                    )
                    .await;
                }
            }
            // Post-reply supplement charge based on `previous`'s
            // shape (length-parameterized non-verifying replays like
            // fs_entries_stream_next use this).
            if let Some(supp) = H::post_reply_supplement(&previous) {
                fs.metering.reserve_incremental_primitive(supp)?;
            }
            produce(&previous, &ack).await?;
            return Ok(previous);
        }
        // Verifying + Consensus follower — fall through to
        // re-execute + verify path.
    }

    // Step 5: content parse.  Content-level type mismatches produce
    // a normal reply, not an `illegal_argument_error`.
    let parsed = match H::parse_content(raw_pre_ack) {
        Ok(a) => a,
        Err(boxed_reply) => {
            let out = vec![(*boxed_reply).into_par()];
            produce(&out, &ack).await?;
            return Ok(out);
        }
    };

    // Step 5b: pre-syscall hook.  Wave-3 S3.7 (2026-09-09) — path-
    // mutation handlers pre-append a WAL entry with a Success
    // placeholder before the syscall runs; on WAL-cap exhaustion
    // the handler returns Err(boxed_reply) and the framework
    // produces the reply + early-returns.  Default no-op for
    // handlers that don't pre-append.
    if let Err(early_reply) = H::pre_syscall(SyscallCtx::new(fs, &ack), &parsed).await {
        let out = vec![(*early_reply).into_par()];
        produce(&out, &ack).await?;
        return Ok(out);
    }

    // Step 6: dispatch — runs on the leader path AND on the
    // Consensus-follower verifying-replay path (that's the point of
    // "re-execute + verify").
    let fresh_reply = H::dispatch(SyscallCtx::new(fs, &ack), parsed).await;
    let fresh_par = fresh_reply.into_par();

    // Step 7: verify (Consensus follower only) + journal + produce.
    if is_replay {
        // Consensus follower verify — compare fresh reply's
        // stable_hash against leader's cached reply hash extracted
        // from `previous`.  See verify.rs::verify_reply_hash_matches_
        // cached + auto-memory `fileio_wal_replay_verification_gap.md`.
        // D1 = Option A: divergent DEPLOY fails; block still
        // proceeds.
        match super::verify::verify_reply_hash_matches_cached(&fresh_par, &previous) {
            Ok(()) => {
                H::journal(
                    SyscallCtx::new(fs, &ack),
                    raw_pre_ack,
                    JournalPath::VerifySuccess {
                        fresh_reply: &fresh_par,
                    },
                )
                .await;
                let out = vec![fresh_par];
                if let Some(supp) = H::post_reply_supplement(&out) {
                    fs.metering.reserve_incremental_primitive(supp)?;
                }
                produce(&out, &ack).await?;
                Ok(out)
            }
            Err(reason) => {
                let divergence = super::handlers::consensus_divergence_reply(H::NAME, reason);
                H::journal(
                    SyscallCtx::new(fs, &ack),
                    raw_pre_ack,
                    JournalPath::VerifyDivergence {
                        fresh_reply: &fresh_par,
                        divergence_reply: &divergence,
                    },
                )
                .await;
                // S3.9-F5 (2026-09-09): cost supplement is billed
                // against the FRESH syscall reply (state_source),
                // not the divergence reply produced to ack.
                // Pre-refactor fs_entries computed `n_entries`
                // from `fresh_reply` BEFORE the verify branch and
                // charged that same n regardless of verify Ok/Err.
                // Divergence-path charging against `divergence`
                // (list_len=0 for err payload) would silently drop
                // cost witness bytes vs. pre-refactor.  Handlers
                // that don't override this method (default None)
                // are unaffected.
                if let Some(supp) = H::post_reply_supplement(std::slice::from_ref(&fresh_par)) {
                    fs.metering.reserve_incremental_primitive(supp)?;
                }
                let out = vec![divergence];
                produce(&out, &ack).await?;
                Ok(out)
            }
        }
    } else {
        // Leader path — journal fresh_par, produce it.
        H::journal(
            SyscallCtx::new(fs, &ack),
            raw_pre_ack,
            JournalPath::Leader {
                fresh_reply: &fresh_par,
            },
        )
        .await;
        let out = vec![fresh_par];
        if let Some(supp) = H::post_reply_supplement(&out) {
            fs.metering.reserve_incremental_primitive(supp)?;
        }
        produce(&out, &ack).await?;
        Ok(out)
    }
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// M-35-style pin: guard against `linkme::distributed_slice`
    /// truncation under cdylib / LTO / release linkage.  A
    /// wrong-but-well-formed empty slice would silently disable
    /// every migrated handler's registration when rho_runtime.rs
    /// starts walking `FS_HANDLERS` (wave-3 S3.12).  Panic loud
    /// at the count-check instead.
    ///
    /// The count bumps at every session that migrates a handler.
    /// A drift here (count = declared - migrated) means either:
    ///   (a) production `linkme` truncation (real bug, expect
    ///       handler dispatch to silently no-op post-S3.12), OR
    ///   (b) a session migrated a handler but forgot to bump
    ///       `EXPECTED_MIGRATED_HANDLER_COUNT` (developer error).
    #[test]
    fn fs_handlers_count_matches_migrated_pinned() {
        assert_eq!(
            FS_HANDLERS.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT,
            "FS_HANDLERS has {} entries but expected {}.  Either \
             (a) `linkme::distributed_slice` truncation under cdylib \
             / LTO / release linkage (production bug — post-S3.12 \
             dispatch will silently no-op), or (b) a wave-3 session \
             migrated a handler without bumping \
             EXPECTED_MIGRATED_HANDLER_COUNT (or vice versa).",
            FS_HANDLERS.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT
        );
    }

    /// Wave-3 migration pin: every handler that a wave-3 session
    /// migrated MUST have an `FS_HANDLERS` entry with its
    /// spec-canonical name.  A regression that unregisters an
    /// entry (or renames its `name` field) fires here.  New
    /// migrations extend the table below.
    #[test]
    fn migrated_handlers_are_registered_in_fs_handlers() {
        // Update this table at every session that migrates a
        // handler.  Order matches wave-3-plan.md § Sessions.
        let migrated: &[&str] = &[
            "fs_flush",                  // S3.1 (2026-09-08)
            "fs_tell",                   // S3.2 (2026-09-08)
            "fs_close",                  // S3.2 (2026-09-08)
            "fs_release_lock",           // S3.2 (2026-09-08)
            "fs_quarantine",             // S3.3 (2026-09-08)
            "fs_entries_stream_close",   // S3.3 (2026-09-08)
            "fs_lock_range",             // S3.3 (2026-09-08)
            "fs_lock_sequential",        // S3.3 (2026-09-08)
            "fs_entries_stream_open",    // S3.4 (2026-09-09)
            "fs_entries_stream_next",    // S3.4 (2026-09-09)
            "fs_size",                   // S3.5 (2026-09-09)
            "fs_stat",                   // S3.5 (2026-09-09)
            "fs_exists",                 // S3.5 (2026-09-09)
            "fs_read",                   // S3.6 (2026-09-09)
            "fs_read_at",                // S3.6 (2026-09-09)
            "fs_seek",                   // S3.6 (2026-09-09)
            "fs_chown",                  // S3.7 (2026-09-09)
            "fs_chmod",                  // S3.7 (2026-09-09)
            "fs_truncate",               // S3.7 (2026-09-09)
            "fs_write",                  // S3.8 (2026-09-09)
            "fs_write_at",               // S3.8 (2026-09-09)
            "fs_entries",                // S3.9 (2026-09-09)
            "fs_rename",                 // S3.9 (2026-09-09)
            "fs_copy_file",              // S3.9 (2026-09-09)
            "fs_open",                   // S3.10 (2026-09-09)
            "fs_remove_file",            // S3.10 (2026-09-09)
            "fs_release_all_for_holder", // S3.11 (2026-09-09)
        ];
        for name in migrated {
            let found = FS_HANDLERS.iter().any(|h| h.name == *name);
            assert!(
                found,
                "FS_HANDLERS entry for `{name}` missing.  A wave-3 \
                 session migrated this handler but its \
                 `#[distributed_slice(FS_HANDLERS)] static X_ENTRY` \
                 was removed from handlers.rs (or its `name` field \
                 drifted from the spec-canonical literal)."
            );
        }
        assert_eq!(
            migrated.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT,
            "The migrated-handler table above must have the same \
             length as EXPECTED_MIGRATED_HANDLER_COUNT.  Bump both \
             when a wave-3 session migrates a new handler."
        );
    }

    /// Wave-3 S3.13a (2026-09-09): per-family count pin.  Every
    /// migrated handler declares a `family: HandlerFamily::X` on its
    /// `#[distributed_slice(FS_HANDLERS)] static FS_X_ENTRY`
    /// registration.  This pin locks the family breakdown against
    /// the plan-documented shape:
    ///
    ///   mutation:    8 (fs_remove_dir trait-exempt → not in slice)
    ///   observation: 9
    ///   stream:      3
    ///   lock:        4
    ///   lifecycle:   3
    ///                --
    ///                27 total (matches EXPECTED_MIGRATED_HANDLER_COUNT).
    ///
    /// A regression that miscategorizes a handler (or leaves a new
    /// migration with a stale family field from a copy-paste)
    /// surfaces here.  Prep for S3.13b `handlers_{family}.rs` file
    /// split — the count per family MUST match the per-file
    /// registration count.
    #[test]
    fn fs_handlers_family_counts_match_pinned() {
        let mut counts = [0usize; 5];
        for entry in FS_HANDLERS.iter() {
            let idx = match entry.family {
                HandlerFamily::Mutation => 0,
                HandlerFamily::Observation => 1,
                HandlerFamily::Stream => 2,
                HandlerFamily::Lock => 3,
                HandlerFamily::Lifecycle => 4,
            };
            counts[idx] += 1;
        }
        let expected = [8usize, 9, 3, 4, 3]; // see doc-comment
        assert_eq!(
            counts, expected,
            "FS_HANDLERS family count drift.  Expected \
             [Mutation=8, Observation=9, Stream=3, Lock=4, Lifecycle=3] \
             (fs_remove_dir trait-exempt, so mutation is 8 in the slice; \
             total 27 = EXPECTED_MIGRATED_HANDLER_COUNT).  Got {counts:?}.  \
             Either a handler's `family:` field drifted, or a new handler \
             was added without updating this pin.  See HandlerFamily \
             doc-comment for the family taxonomy."
        );
        let total: usize = counts.iter().sum();
        assert_eq!(
            total, EXPECTED_MIGRATED_HANDLER_COUNT,
            "Per-family count sum ({total}) must equal \
             EXPECTED_MIGRATED_HANDLER_COUNT ({EXPECTED_MIGRATED_HANDLER_COUNT}).  \
             Someone added a HandlerFamily variant but not the row above, or \
             a family field was omitted from a FS_HANDLERS entry."
        );
    }

    /// HandlerReply construction round-trip: a `HandlerReply::err`
    /// call produces a Par byte-identical to the pre-refactor
    /// `response::err(code, msg)` call.  Wave-3 T-04's whole
    /// premise is that reply bytes stay stable across the trait
    /// migration — a divergence here breaks consensus.
    #[test]
    fn handler_reply_err_matches_response_err_bytes() {
        use prost::Message;
        let via_trait = HandlerReply::err("FSERR_BAD_ARG", "sample").into_par();
        let via_response = response::err("FSERR_BAD_ARG", "sample");
        assert_eq!(
            via_trait.encode_to_vec(),
            via_response.encode_to_vec(),
            "HandlerReply::err must produce a Par byte-identical to \
             response::err — the reply shape is consensus-observable \
             (WAL reply-hash verify + Rholang caller pattern-match)."
        );
    }
}
