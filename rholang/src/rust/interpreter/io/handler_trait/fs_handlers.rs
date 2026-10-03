// Distributed-slice registry of migrated fs_* handlers.  Populated
// at link time by `#[distributed_slice(FS_HANDLERS)] static X_ENTRY`
// declarations in the per-family handler modules (yet to land —
// slices 4.12+).  Consumed by `rho_runtime.rs` wiring at the
// yet-to-land handler-registration slice to replace the pre-trait
// 28-way `fs_native_def(...)` block with a `FS_HANDLERS.iter()`
// loop.
//
// # Status
//
// Zero migrated handlers at Wave 4 slice 4.10.  The slice is
// declared empty; `EXPECTED_MIGRATED_HANDLER_COUNT = 0`.  Each
// per-family handler slice (4.12+) adds its handlers' entries and
// bumps the count by the family size.  At migration-complete,
// count reaches 27 (fs_remove_dir trait-exempt — see
// `handler_trait::fs_handler` module docstring).
//
// # linkme truncation guard
//
// The `fs_handlers_count_matches_migrated_pinned` runtime pin
// guards against `linkme::distributed_slice` truncation under
// cdylib / LTO / release linkage.  A wrong-but-well-formed empty
// slice would silently disable every migrated handler's
// registration when `rho_runtime.rs` walks `FS_HANDLERS`.  Panic
// loud at the count-check instead.
//
// The pattern matches `consensus_fingerprint.rs`'s existing
// `CONSENSUS_FOLD` registry on dev — both use `linkme` with a
// count-pin test to catch cdylib truncation.
//
// # Wave 6 (cost-accounted-rho) coupling
//
// `FsHandlerEntry.verifying` carries each handler's `VERIFYING`
// const (from the `FsHandler` trait, slice 4.6).  Under Wave 6
// the per-family count pin + the `fs_handlers_family_counts_
// match_pinned` runtime pin (yet to land) catches a drop in
// `const VERIFYING` — the hazard class flagged in
// `FsHandler::VERIFYING`'s docstring.
//
// # Entry fn-pointer shape
//
// `dispatch` is a `fn(FsProcesses, ...) -> Pin<Box<...>>` (not an
// `async fn` — Rust fn pointers can't carry async directly).
// Each handler's registration site coerces a non-capturing
// `|fs, args| Box::pin(...)` closure into this pointer type.  The
// closure calls `dispatch_via_trait_owned::<H>(fs, args)` (yet to
// land, slice 4.11) which internally awaits the handler's async
// `dispatch` method and returns the resulting
// `Result<Vec<Par>, InterpreterError>`.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::{ListParWithRandom, Par};

use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::io::handler_trait::family::HandlerFamily;
use crate::rust::interpreter::io::handler_trait::fs_processes::FsProcesses;
use crate::rust::interpreter::system_processes::{BodyRef, Name};

/// One entry per migrated fs_* handler.  Each handler registers
/// via `#[distributed_slice(FS_HANDLERS)] static X_ENTRY:
/// FsHandlerEntry = FsHandlerEntry { ... };` in its family module.
pub struct FsHandlerEntry {
    /// Handler name (`fs_flush`, `fs_read`, etc.) — same as
    /// `<H as FsHandler>::NAME`.  Stored explicitly (not derived
    /// from the fn pointer) so the pin can walk the slice without
    /// evaluating any of the fn pointers.  Also surfaces in
    /// consensus-divergence replies via
    /// [`consensus_divergence_reply`](super::consensus_divergence_reply).
    pub name: &'static str,

    /// Total arity including the trailing ack channel — same as
    /// `<H as FsHandler>::ARITY`.  Used by the yet-to-land
    /// handler-registration site to construct the
    /// `fs_native_def(..., arity, ...)` call from the slice entry.
    pub arity: usize,

    /// Verifying flag — same as `<H as FsHandler>::VERIFYING`.
    /// `true` = re-execute + verify reply hash on Consensus-cmode
    /// replay.  Used by the yet-to-land per-family verifying-count
    /// pin to guard the 15/27 verify-matrix invariant (14 verifying
    /// in the trait-registered slice + 1 verifying in the trait-
    /// exempt fs_remove_dir = 15 total; see fileio's
    /// `handler_trait::fs_handler` docstring).
    pub verifying: bool,

    /// The type-erased dispatch fn.  Each per-handler registration
    /// site coerces a non-capturing `|fs, args| Box::pin(...)`
    /// closure into this fn-pointer type.  The closure body is
    /// one line:
    /// `dispatch_via_trait_owned::<H>(fs, args)` — the generic
    /// framework loop (yet to land, slice 4.11).
    ///
    /// The tuple parameter is `(args, is_replay, previous)` —
    /// matches [`ContractCall::unapply`](crate::rust::interpreter::contract_call::ContractCall::unapply)
    /// result shape.  Handler authors don't destructure it at
    /// registration sites; `dispatch_via_trait_owned` absorbs the
    /// destructuring internally at step 1.
    ///
    /// # Why `fn` (not `Fn`)?
    ///
    /// A `fn` pointer is `Copy + Send + Sync` by construction and
    /// stores in a static without needing an outer `Box` /
    /// `Arc`.  A `Fn` closure trait object (`Box<dyn Fn(...)>`)
    /// would work but requires heap allocation per entry.  Non-
    /// capturing closures coerce to `fn` pointers for free, so the
    /// registration shape is just as ergonomic.
    pub dispatch: fn(
        FsProcesses,
        (Vec<ListParWithRandom>, bool, Vec<Par>),
    )
        -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>,

    /// URN suffix appended to `"rho:io:fs:native:1.0.0/"` when
    /// registering the handler at genesis.  E.g., `"open"`,
    /// `"readAt"`, `"removeFile"`.  camelCase per the pre-trait
    /// `fs_native_def` call sites in `rho_runtime.rs`; the
    /// composed source at `fs_genesis.rs` (yet to land, Wave 5)
    /// expects the exact same string.
    ///
    /// A regression that drifted this field from the pre-trait
    /// hard-coded string in `rho_runtime.rs` would silently rename
    /// the URN, breaking URN-map lookups from `Fs.rho`.  Will be
    /// guarded by the handler-registration slice's URN-suffix pin.
    pub urn_suffix: &'static str,

    /// Fixed-channel constructor: `FixedChannels::fs_x` fn pointer.
    /// The yet-to-land handler-registration loop calls
    /// `(entry.fixed_channel)()` to obtain the `Par` byte-name
    /// used as the rendezvous channel.
    pub fixed_channel: fn() -> Name,

    /// `BodyRefs::FS_X` constant.  Rholang's built-in dispatch
    /// table keys handler resolution on this `i64`.
    pub body_ref: BodyRef,

    /// Handler family.  Groups handlers by effect shape; used by
    /// the yet-to-land per-family count pin + by the per-family
    /// handler file split.  See
    /// [`HandlerFamily`](super::family::HandlerFamily) for the
    /// taxonomy.
    pub family: HandlerFamily,
}

/// Distributed slice of every migrated handler's
/// [`FsHandlerEntry`].  Populated at link time by
/// `#[distributed_slice(FS_HANDLERS)] static ...` declarations
/// in per-family handler modules (yet to land, slices 4.12+).
#[distributed_slice]
pub static FS_HANDLERS: [FsHandlerEntry] = [..];

/// Expected count of migrated handlers in [`FS_HANDLERS`].  Bumped
/// at every per-family handler slice as handlers get registered;
/// the pin `fs_handlers_count_matches_migrated_pinned` fires on
/// drift.
///
/// # Wave 4 progression (reference)
///
/// Each per-family handler slice adds its handlers' entries and
/// bumps this count.  At migration-complete (handlers slices
/// finished), count reaches 27 — fs_remove_dir trait-exempt
/// (see `handler_trait::fs_handler` module docstring).
///
/// Current: 4 handlers migrated.
///
/// Wave 4 slice progression:
///   - 4.10: 0 (empty registry infrastructure).
///   - 4.12: +1 (`fs_quarantine`).  Count = 1.
///   - 4.13: +1 (`fs_close`).  Count = 2.
///   - 4.14: +1 (`fs_flush`).  Count = 3.
///   - 4.15: +1 (`fs_tell`).  Count = 4.
pub const EXPECTED_MIGRATED_HANDLER_COUNT: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    /// LOAD-BEARING linkme truncation guard: the registry's actual
    /// length matches the expected count.  A drift here means
    /// either:
    ///
    ///   (a) Production `linkme` truncation under cdylib / LTO /
    ///       release linkage — a real bug.  Expect handler
    ///       dispatch to silently no-op once the dispatcher slice
    ///       (4.11) starts walking `FS_HANDLERS`.
    ///
    ///   (b) A per-family handler slice registered a handler but
    ///       forgot to bump `EXPECTED_MIGRATED_HANDLER_COUNT`
    ///       (or vice versa) — a developer error.
    ///
    /// Matches the pattern used by `CONSENSUS_FOLD`'s
    /// truncation guard in `consensus_fingerprint.rs`.
    #[test]
    fn fs_handlers_count_matches_migrated_pinned() {
        assert_eq!(
            FS_HANDLERS.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT,
            "FS_HANDLERS has {} entries but expected {}.  Either \
             (a) `linkme::distributed_slice` truncation under cdylib \
             / LTO / release linkage (production bug — dispatcher \
             will silently no-op), or (b) a per-family slice \
             migrated a handler without bumping \
             EXPECTED_MIGRATED_HANDLER_COUNT (or vice versa).",
            FS_HANDLERS.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT,
        );
    }

    /// Progression pin — the count moves monotonically as per-
    /// family handler slices (4.12+) land.  Each new handler
    /// registration appends its canonical name to
    /// `EXPECTED_REGISTERED_HANDLER_NAMES` below AND bumps
    /// `EXPECTED_MIGRATED_HANDLER_COUNT` to the new length.  The
    /// test verifies both are in sync + every expected name is
    /// present in the slice.
    ///
    /// Slice-number-stable: future slices extend the array +
    /// bump the constant without touching the test name.
    #[test]
    fn migrated_handlers_match_registration_set() {
        /// Canonical names registered so far.  Append (don't
        /// re-order — the array's ordering is informational, not
        /// consensus-observable) at every per-family handler
        /// slice.  See `handler_trait::fs_handler` docstring for
        /// the migration-complete target (27 handlers, fs_remove_dir
        /// trait-exempt).
        const EXPECTED_REGISTERED_HANDLER_NAMES: &[&str] = &[
            "fs_quarantine", // slice 4.12
            "fs_close",      // slice 4.13
            "fs_flush",      // slice 4.14
            "fs_tell",       // slice 4.15
        ];

        assert_eq!(
            EXPECTED_MIGRATED_HANDLER_COUNT,
            EXPECTED_REGISTERED_HANDLER_NAMES.len(),
            "EXPECTED_MIGRATED_HANDLER_COUNT ({}) must equal the \
             length of EXPECTED_REGISTERED_HANDLER_NAMES ({}).  A \
             per-family slice bumped one but not the other.",
            EXPECTED_MIGRATED_HANDLER_COUNT,
            EXPECTED_REGISTERED_HANDLER_NAMES.len(),
        );

        for name in EXPECTED_REGISTERED_HANDLER_NAMES {
            assert!(
                FS_HANDLERS.iter().any(|h| h.name == *name),
                "FS_HANDLERS entry for `{name}` missing.  A \
                 regression that unregistered the entry would trip \
                 here before any dispatcher invocation."
            );
        }
    }

    /// `FsHandlerEntry` carries primitive-only fields + fn-pointers
    /// — so it's `Sync + Send + 'static` by construction and
    /// `distributed_slice`-safe.  A regression that added a
    /// non-static field (e.g., a `&'a str` or an `Arc<...>`) would
    /// trip here.
    #[test]
    fn fs_handler_entry_is_static_send_sync() {
        fn require_static_send_sync<T: Send + Sync + 'static>() {}
        require_static_send_sync::<FsHandlerEntry>();
    }
}
