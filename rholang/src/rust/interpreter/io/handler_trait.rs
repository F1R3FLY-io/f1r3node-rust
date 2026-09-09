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

    /// Handler's cost weight.  Charged at handler entry via
    /// `metering.reserve_primitive`.  Length-parameterized
    /// handlers (read/write/entries) get a post-reply supplement
    /// via a framework hook landing in wave-3 S3.5.
    fn pre_charge_cost() -> Cost;

    /// Syscall + reply generation.  Returns a `HandlerReply`;
    /// the framework handles produce/ack.  The future's lifetime
    /// is bounded by the passed `SyscallCtx<'a>`.
    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: Self::Args,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>>;

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
}

// ------------------------------------------------------------------
// FsHandlerEntry + FS_HANDLERS distributed slice
// ------------------------------------------------------------------

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
///   - ... (see wave-3-plan.md § Sessions).
///   - S3.12: reaches 28, stays there.
pub const EXPECTED_MIGRATED_HANDLER_COUNT: usize = 10;

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
    // Step 1: cost pre-charge — matches current handler discipline
    // (see the header comment in handlers.rs § "Each handler").
    fs.metering.reserve_primitive(H::pre_charge_cost())?;

    // Step 2: unapply.
    let Some((produce, is_replay, previous, args)) = fs.is_contract_call().unapply(contract_args)
    else {
        return Err(illegal_argument_error(H::NAME));
    };

    // Step 3: arity check.  Framework rejects with
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

    // Step 4: is_replay short-circuit (non-verifying only in S3.1).
    if is_replay {
        if H::VERIFYING {
            // Wave-3 S3.5 lands the verifying-replay dispatch (re-
            // execute + `verify_reply_hash_matches_cached`).  Until
            // then, a VERIFYING handler must NOT be registered.  This
            // is a compile-time discipline: only non-verifying
            // handlers migrate in S3.1–S3.4.  If this panic fires,
            // a session skipped ahead.
            unreachable!(
                "S3.1 framework does not yet dispatch VERIFYING \
                 handlers; wave-3-plan.md S3.5 extends this branch.  \
                 Handler `{}` was registered with VERIFYING=true \
                 before the framework supports it.",
                H::NAME
            );
        }
        // Non-verifying replay: run the optional side-effect hook
        // (fs_close's fd release, fs_entries_stream_next's WAL
        // journaling, etc.).  See handlers.rs § "Each handler"
        // step 5.  Hook gets both raw pre-ack args and `previous`
        // via ctx-bound references so it can journal.
        H::on_replay_side_effect(SyscallCtx::new(fs, &ack), &args[..H::ARITY - 1], &previous).await;
        // Post-reply supplement charge based on `previous`'s shape.
        // Length-parameterized non-verifying replays (e.g.,
        // fs_entries_stream_next) use this to charge n=1 or n=0.
        if let Some(supp) = H::post_reply_supplement(&previous) {
            fs.metering.reserve_incremental_primitive(supp)?;
        }
        produce(&previous, &ack).await?;
        return Ok(previous);
    }

    // Step 5: content parse.  Content-level type mismatches (e.g.,
    // integer slot got a string Par) produce a normal reply, not an
    // `illegal_argument_error`.
    let parsed = match H::parse_content(&args[..H::ARITY - 1]) {
        Ok(a) => a,
        Err(boxed_reply) => {
            let out = vec![(*boxed_reply).into_par()];
            produce(&out, &ack).await?;
            return Ok(out);
        }
    };

    // Step 6: dispatch.
    let reply = H::dispatch(SyscallCtx::new(fs, &ack), parsed).await;
    let out = vec![reply.into_par()];

    // Step 7: post-reply cost supplement.  Framework charges
    // `reserve_incremental_primitive` after dispatch and BEFORE
    // produce, so a budget-exceeded supplement fails the deploy
    // without publishing the reply to the caller — same pre-refactor
    // ordering (see e.g. fs_entries_stream_next's inline sequence).
    if let Some(supp) = H::post_reply_supplement(&out) {
        fs.metering.reserve_incremental_primitive(supp)?;
    }

    // Step 8: produce reply to ack.
    produce(&out, &ack).await?;
    Ok(out)
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
            "fs_flush",                // S3.1 (2026-09-08)
            "fs_tell",                 // S3.2 (2026-09-08)
            "fs_close",                // S3.2 (2026-09-08)
            "fs_release_lock",         // S3.2 (2026-09-08)
            "fs_quarantine",           // S3.3 (2026-09-08)
            "fs_entries_stream_close", // S3.3 (2026-09-08)
            "fs_lock_range",           // S3.3 (2026-09-08)
            "fs_lock_sequential",      // S3.3 (2026-09-08)
            "fs_entries_stream_open",  // S3.4 (2026-09-09)
            "fs_entries_stream_next",  // S3.4 (2026-09-09)
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
