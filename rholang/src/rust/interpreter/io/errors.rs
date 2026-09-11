// String error codes returned to Rholang callers as the second element
// of `[false, code, msg]` responses.  Every code is spec-canonical
// (§Errors).
//
// M-34 fix (S-2 from 2026-09-04 review): both the caller-facing
// `FSERR_*: &str` const and the WAL-wire `FSERR_CODE_*: u32` const
// are declared in a single `consensus_error_codes!` invocation
// below.  The macro emits four artifacts from one source of truth:
//
//   1. `pub const FSERR_<NAME>: &str = "FSERR_<NAME>";` for
//      caller-facing pattern-match ergonomics.
//   2. `pub const FSERR_CODE_<NAME>: u32 = <code>;` for compact
//      WAL wire encoding (`WalOutcome::Failure { code: u32 }`).
//   3. `pub fn fserr_to_code(&str) -> u32` — the taxonomy bridge
//      used by every `finalize_failure_journal` site.
//   4. A `const _: () = { ... }` compile-time assertion that the
//      declared codes are contiguous from 1 up to N (with 0
//      reserved as UNKNOWN) — a "forgot to append at the tail"
//      regression fails at compile time rather than runtime.
//
// Prior to this refactor, adding a new FSERR code required editing
// four hand-synced places (two const blocks + the match + a
// downstream fingerprint fold).  A missed edit shipped `FSERR_*` in
// caller-facing replies but journaled `FSERR_CODE_UNKNOWN` on the
// wire — a silent mis-classification the compile-time assertion now
// prevents.
//
// **DO NOT reorder or renumber existing codes.**  The u32 mapping
// is a hard-fork surface (item #9 in the consensus-observable
// catalog).  New codes append at the tail.

use std::io;

use paste::paste;

/// S4.9 (2026-09-11): typed FSERR code, replaces the pre-S4.9
/// `&'static str` shape of each `FSERR_*` constant.  Introducing a
/// newtype lets the compiler catch "someone passed a raw string
/// where a canonical error code was expected" at every call site
/// (`HandlerReply::err(...)`, `err(...)`, `lock_err_reply(...)`,
/// etc.) that used to accept any `&'static str`.
///
/// # Invariants
///
/// - The inner `&'static str` MUST match the identifier name
///   spec-canonical (`"FSERR_BAD_ARG"` for `FSERR_BAD_ARG`, etc.).
///   The `consensus_error_codes!` macro enforces this at
///   declaration time via `stringify!`.
///
/// # Consensus surface
///
/// The inner bytes are consensus-observable (they appear in
/// `[false, code, msg]` Rholang error tuples).  Wrapping in a
/// newtype does NOT change the wire bytes — `as_str()` returns
/// the identical `&'static str` value, so the `RhoString`-encoded
/// Par for an error reply is byte-identical pre/post S4.9.
///
/// # Ergonomics
///
/// - `Copy` so it can be freely passed by value.
/// - No `Deref` to `&str` — that would defeat the purpose by
///   allowing implicit conversion at call sites.  Use `.as_str()`
///   explicitly when converting to a raw string is required (e.g.,
///   `fserr_to_code` bridge, or emitting to a Par via
///   `RhoString::create_par`).
/// - `Display` prints the inner string, so `format!("{code}")`
///   works ergonomically.
/// - `PartialEq<&str>` and `PartialEq<str>` for direct comparison
///   in tests that compare a code to a literal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FserrCode(pub &'static str);

impl FserrCode {
    /// Return the canonical `&'static str` inner value.  Prefer
    /// this over accessing `.0` directly — the wrapper's field is
    /// `pub` for macro construction only.
    pub const fn as_str(&self) -> &'static str { self.0 }
}

impl std::fmt::Display for FserrCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.0) }
}

impl PartialEq<&str> for FserrCode {
    fn eq(&self, other: &&str) -> bool { self.0 == *other }
}

impl PartialEq<str> for FserrCode {
    fn eq(&self, other: &str) -> bool { self.0 == other }
}

impl PartialEq<FserrCode> for &str {
    fn eq(&self, other: &FserrCode) -> bool { *self == other.0 }
}

impl PartialEq<FserrCode> for str {
    fn eq(&self, other: &FserrCode) -> bool { self == other.0 }
}

impl PartialEq<String> for FserrCode {
    fn eq(&self, other: &String) -> bool { self.0 == other.as_str() }
}

impl PartialEq<FserrCode> for String {
    fn eq(&self, other: &FserrCode) -> bool { self.as_str() == other.0 }
}

/// Reserved as "unknown" so an in-code error slipping through the
/// mapping still round-trips deterministically rather than silently
/// mis-classifying.  See `fserr_to_code`.
pub const FSERR_CODE_UNKNOWN: u32 = 0;

/// M-34 (2026-09-04, S-2 fix): single-source-of-truth macro for
/// the FSERR taxonomy.
///
/// Syntax:
/// ```ignore
/// consensus_error_codes! {
///     (BAD_ARG, 1),
///     (IO, 2),
///     // ...
/// }
/// ```
///
/// Emits `pub const FSERR_BAD_ARG: &str = "FSERR_BAD_ARG";` and
/// `pub const FSERR_CODE_BAD_ARG: u32 = 1;` for each entry, plus
/// the `fserr_to_code` bridge and a compile-time contiguity check.
macro_rules! consensus_error_codes {
    ( $( ( $name:ident, $code:expr ) ),+ $(,)? ) => {
        paste! {
            $(
                // S4.9 (2026-09-11): FSERR constants are `FserrCode`
                // (newtype over `&'static str`).  The inner value is
                // spec-canonical (`stringify!` enforces identifier-
                // matches-value at compile time).  Consensus surface
                // unchanged — `.as_str()` returns the same bytes the
                // pre-S4.9 `&str` version would have produced.
                pub const [<FSERR_ $name>]: FserrCode =
                    FserrCode(stringify!([<FSERR_ $name>]));
                pub const [<FSERR_CODE_ $name>]: u32 = $code;
            )+

            /// Map a spec-canonical FSERR string to its stable u32 code
            /// for on-wire encoding in the WAL.  Unknown / non-canonical
            /// inputs return `FSERR_CODE_UNKNOWN` (never panics — a
            /// hostile or out-of-band error string still round-trips
            /// deterministically).
            ///
            /// Takes `&str` (not `FserrCode`) because WAL decode paths
            /// receive arbitrary strings — they may not have originated
            /// from a canonical `FSERR_*` constant, and mapping them to
            /// UNKNOWN is the safe fallback.
            pub fn fserr_to_code(s: &str) -> u32 {
                match s {
                    $(
                        s if s == [<FSERR_ $name>].as_str() => [<FSERR_CODE_ $name>],
                    )+
                    _ => FSERR_CODE_UNKNOWN,
                }
            }

            /// Compile-time contiguity check: the declared codes must
            /// be exactly `[1, N]` (with 0 reserved as UNKNOWN).  A
            /// gap or duplicate fires here at const-eval rather than
            /// shipping a WAL with silently-mis-encoded codes.
            const _: () = {
                let codes: &[u32] = &[$([<FSERR_CODE_ $name>]),+];
                let n = codes.len();
                let mut i = 0;
                while i < n {
                    let expected = (i as u32) + 1;
                    assert!(
                        codes[i] == expected,
                        "M-34: FSERR codes must be contiguous 1..N with no \
                         gaps or duplicates; a mis-numbered code was declared \
                         in `consensus_error_codes!`",
                    );
                    i += 1;
                }
            };
        }
    };
}

consensus_error_codes! {
    (BAD_ARG,               1),
    (IO,                    2),
    (NOT_FOUND,             3),
    (ALREADY_EXISTS,        4),
    (PERM,                  5),
    (UNSUPPORTED,           6),
    (QUARANTINE,            7),
    (CLOSED,                8),
    (BUSY,                  9),
    (QUOTA_EXCEEDED,       10),
    (CROSS_DEVICE,         11),
    // Slice-8b `wait: true` cancellation.  Distinct from BUSY so
    // callers can tell "conflict at request time" apart from "was
    // in the queue but got cancelled" (via `LockRegistry::cancel_
    // wait` or the deploy-end sweep in `WalDeployScope::drop`).
    (CANCELLED,            12),
    // Phase 1 (Consensus re-execute + verify, 2026-09-01): the
    // follower's re-executed syscall reply hash does NOT match the
    // leader's cached reply hash extracted from `previous`.
    // Surfaces on Consensus-cap observation ops.  Per D1 = Option A:
    // the divergent DEPLOY fails; the block still proceeds.  See
    // auto-memory `fileio_wal_replay_verification_gap.md`.
    (CONSENSUS_DIVERGENCE, 13),
    // NB-7 cross-deploy mutual-wait deadlock detection (2026-09-02):
    // a `wait: true` acquire that would close a cycle in the
    // cross-deploy wait-for graph is refused eagerly at enqueue time.
    // See `LockRegistry::would_close_cycle` and
    // `docs/consensus-invariants.md § 8`.
    (DEADLOCK,             14),
    // `Fs.revoke()` ambient-authority off-switch (2026-09-03).
    // After `Fs.revoke()` has been called on any Fs instance, every
    // subsequent `openFile` / `openDir` / `stdin` / `stdout` /
    // `stderr` returns `[false, FSERR_REVOKED, ...]`.  Previously-
    // minted caps are unaffected.  See spec §Revocation +
    // design-decisions.md DD-Revoke + `docs/consensus-invariants.md § 4`.
    (REVOKED,              15),
}

/// Map a `std::io::Error` kind to a stable FSERR code.  Callers
/// invoke this at the boundary between kernel errors and Rholang
/// reply Pars.  Not derived from the macro because the mapping is
/// FROM a foreign taxonomy (`io::ErrorKind`) TO ours; a macro-
/// derived version would be less readable.
///
/// S4.9 (2026-09-11): returns `FserrCode` (was `&'static str`) so
/// the compiler enforces canonical-code discipline at every call
/// site that passes the result into `HandlerReply::err` / `err`.
pub fn io_err_code(e: &io::Error) -> FserrCode {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => FSERR_NOT_FOUND,
        PermissionDenied => FSERR_PERM,
        AlreadyExists => FSERR_ALREADY_EXISTS,
        InvalidInput | InvalidData => FSERR_BAD_ARG,
        Unsupported => FSERR_UNSUPPORTED,
        _ => FSERR_IO,
    }
}

/// T-13 (2026-09-11, wave-4 Phase 3, DD-FailClosedOnInvariantBreak):
/// unified poison-abort helper.  Replaces the pre-hardening scattered
/// pattern `.expect("... poisoned")` with a single well-known
/// well-messaged panic that the operational log scan can grep by
/// its canonical prefix (`POISON_ABORT_PREFIX` below).
///
/// A poisoned lock means a thread panicked while holding the write
/// guard — the protected invariant may be half-updated.  Per
/// DD-FailClosedOnInvariantBreak, we deliberately do NOT accept the
/// poisoned inner via `.into_inner()` (option (a) accept-and-log)
/// nor return a soft error (option (b) FSERR_IO); we panic (option
/// (c) deploy abort).  The panic unwinds to the deploy scope, which
/// rejects the block containing the deploy.
///
/// The `slot` argument is a short human-readable identifier for the
/// lock ("Wal.entries", "LockRegistry.inner", etc.) — appears in the
/// panic message for operator triage.  Keep it stable across
/// releases so log-scan alerting doesn't false-negative on cosmetic
/// renames.
///
/// # Consensus surface
///
/// None.  Both the pre- and post-hardening shapes panic on poison
/// with a `String` message; the panic bytes travel via `PoisonError`
/// but are consumed at the deploy layer without touching WAL bytes,
/// reply Pars, or fingerprint inputs.  Free per
/// `f1r3node_no_running_network`.
#[track_caller]
pub fn poison_abort<T>(result: std::sync::LockResult<T>, slot: &str) -> T {
    result.unwrap_or_else(|_| {
        panic!(
            "{POISON_ABORT_PREFIX}: {slot} — a thread panicked while \
             holding this lock; the protected invariant may be \
             half-updated.  Aborting per DD-FailClosedOnInvariantBreak."
        );
    })
}

/// Canonical prefix for `poison_abort` panic messages.  Kept as a
/// public constant so tests can assert the panic shape without
/// hard-coding the full message text (which may evolve for clarity).
pub const POISON_ABORT_PREFIX: &str = "io/ lock poisoned";

/// T-20 (2026-09-11, wave-4 Phase 3, DD-FailClosedOnInvariantBreak):
/// unified `JoinError`-abort helper.  Replaces the pre-hardening
/// pattern `Err(_je) => HandlerReply::err(FSERR_IO, "spawn_blocking
/// task failed")` with a canonical panic that aborts the deploy.
///
/// A `spawn_blocking` task that panicked (or was cancelled by the
/// runtime shutting down) is an invariant break — the syscall body
/// failed unrecoverably.  Per DD-FailClosedOnInvariantBreak we
/// abort the deploy rather than mask the failure as an FSERR_IO
/// reply that burns budget but hides the underlying bug.
///
/// The panic propagates the original panic payload (message,
/// downcastable) as part of the abort message so operators see the
/// underlying cause alongside the abort banner.  The canonical
/// prefix `JOIN_ERR_ABORT_PREFIX` lets operational log scanning
/// grep for this specific hazard class independent of the payload.
///
/// # Consensus surface
///
/// Zero.  Both pre- and post-hardening shapes fail the deploy on a
/// task panic (the pre-hardening version produced a soft FSERR_IO
/// reply that burned budget; the post-hardening version aborts).
/// No WAL bytes, reply Pars, or fingerprint inputs change; the
/// difference is at the deploy-outcome layer.  Free per
/// `f1r3node_no_running_network`.
#[track_caller]
pub fn join_err_abort(je: tokio::task::JoinError) -> ! {
    let cause: String = if je.is_panic() {
        let payload = je.into_panic();
        if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else {
            "<non-string panic payload>".to_string()
        }
    } else {
        "spawn_blocking task cancelled (runtime shutdown?)".to_string()
    };
    panic!(
        "{JOIN_ERR_ABORT_PREFIX}: {cause}.  Aborting per \
         DD-FailClosedOnInvariantBreak."
    );
}

/// Canonical prefix for `join_err_abort` panic messages.  Kept as
/// a public constant so tests can assert the panic shape without
/// hard-coding the full message text.
pub const JOIN_ERR_ABORT_PREFIX: &str = "io/ handler JoinError";

#[cfg(test)]
mod tests {
    use super::*;

    /// M-34 (2026-09-04, S-2 fix): round-trip pin — every declared
    /// FSERR_* string must map to the corresponding FSERR_CODE_*
    /// u32 under `fserr_to_code`.  Pre-macro, this had to be
    /// asserted per-code by hand; post-macro, a regression that
    /// mistyped one of the four sync sites would surface here even
    /// if the compile-time contiguity check missed it.
    #[test]
    fn fserr_string_to_code_round_trip_pins_every_declared_code() {
        // S4.9 (2026-09-11): pairs are now `(FserrCode, u32)`.  The
        // round-trip goes through `.as_str()` to feed the string-
        // taking `fserr_to_code`.
        let pairs: &[(FserrCode, u32)] = &[
            (FSERR_BAD_ARG, FSERR_CODE_BAD_ARG),
            (FSERR_IO, FSERR_CODE_IO),
            (FSERR_NOT_FOUND, FSERR_CODE_NOT_FOUND),
            (FSERR_ALREADY_EXISTS, FSERR_CODE_ALREADY_EXISTS),
            (FSERR_PERM, FSERR_CODE_PERM),
            (FSERR_UNSUPPORTED, FSERR_CODE_UNSUPPORTED),
            (FSERR_QUARANTINE, FSERR_CODE_QUARANTINE),
            (FSERR_CLOSED, FSERR_CODE_CLOSED),
            (FSERR_BUSY, FSERR_CODE_BUSY),
            (FSERR_QUOTA_EXCEEDED, FSERR_CODE_QUOTA_EXCEEDED),
            (FSERR_CROSS_DEVICE, FSERR_CODE_CROSS_DEVICE),
            (FSERR_CANCELLED, FSERR_CODE_CANCELLED),
            (FSERR_CONSENSUS_DIVERGENCE, FSERR_CODE_CONSENSUS_DIVERGENCE),
            (FSERR_DEADLOCK, FSERR_CODE_DEADLOCK),
            (FSERR_REVOKED, FSERR_CODE_REVOKED),
        ];
        for (c, code) in pairs {
            let s = c.as_str();
            assert_eq!(
                fserr_to_code(s),
                *code,
                "M-34: fserr_to_code({s:?}) must map to {code} — a \
                 mismatch here means the `consensus_error_codes!` \
                 macro's inputs are out of sync with the manual \
                 pair-table below."
            );
            // The const's inner string value must equal its
            // identifier name (spec-canonical); `stringify!` in the
            // macro enforces this at compile time.
            let expected_str = format!("FSERR_{}", &s[6..]);
            assert_eq!(
                s, expected_str,
                "M-34: the const's string value must equal its \
                 identifier name (spec-canonical)."
            );
        }
        // Unknown / non-canonical strings must map to
        // FSERR_CODE_UNKNOWN rather than panic.
        assert_eq!(fserr_to_code("bogus"), FSERR_CODE_UNKNOWN);
        assert_eq!(fserr_to_code(""), FSERR_CODE_UNKNOWN);
    }

    /// S4.9 (2026-09-11): pin the `FserrCode` newtype behavior.
    /// Round-trip through `as_str()` must preserve the canonical
    /// string, equality with `&str` must work both directions, and
    /// `Display` must emit the bare code string (no wrapper prefix).
    #[test]
    fn fserr_code_newtype_shape_pins() {
        let code = FSERR_BAD_ARG;
        assert_eq!(code.as_str(), "FSERR_BAD_ARG");
        assert_eq!(code, "FSERR_BAD_ARG");
        assert_eq!("FSERR_BAD_ARG", code);
        assert_eq!(format!("{code}"), "FSERR_BAD_ARG");
        // Distinct codes are inequal at both the newtype and the
        // string level.
        assert_ne!(FSERR_BAD_ARG, FSERR_IO);
        assert_ne!(FSERR_BAD_ARG.as_str(), FSERR_IO.as_str());
    }

    /// M-34 (2026-09-04, S-2 fix): pin the current-slice count of
    /// FSERR codes at 15.  Adding a new code is a hard-fork surface
    /// change (item #9 in the consensus-observable catalog) and
    /// requires a coordinated peer upgrade — bumping this pin is
    /// the flag for a reviewer to also verify the code was
    /// appended (not inserted) and that the consensus fingerprint
    /// downstream picks it up.
    #[test]
    fn fserr_code_count_is_pinned() {
        // Highest code is 15 (FSERR_REVOKED).  If a new code is
        // added, bump this to N and confirm all four artifacts
        // (const &str, const u32, fserr_to_code arm, round-trip
        // pair-table above) were updated in the same slice.
        assert_eq!(
            FSERR_CODE_REVOKED, 15,
            "M-34: FSERR_CODE_REVOKED is the current-tail code; a \
             change here means a new code was appended (bump the \
             constant) or an existing code was renumbered \
             (hard-fork surface violation)."
        );
    }

    /// T-13 (2026-09-11, DD-FailClosedOnInvariantBreak): `poison_abort`
    /// happy path — a non-poisoned `Ok` guard passes through
    /// unchanged, no panic.
    #[test]
    fn poison_abort_passes_through_unpoisoned_guard() {
        let m = std::sync::Mutex::new(42u32);
        let guard = poison_abort(m.lock(), "test.slot");
        assert_eq!(*guard, 42);
    }

    /// T-13 (2026-09-11, DD-FailClosedOnInvariantBreak): `poison_abort`
    /// on a poisoned lock panics with the canonical prefix.  The
    /// prefix is used by operational log scanning to alert on this
    /// specific hazard class — keep it stable.
    #[test]
    fn poison_abort_panics_with_canonical_prefix_on_poison() {
        use std::sync::{Arc, Mutex};
        let m = Arc::new(Mutex::new(0u32));
        let m2 = Arc::clone(&m);
        // Poison the mutex: spawn a thread that panics while
        // holding the write guard.
        let handle = std::thread::spawn(move || {
            let _guard = m2.lock().unwrap();
            panic!("intentional test poison");
        });
        let _ = handle.join(); // Absorb the thread's panic.
        assert!(m.is_poisoned(), "test setup: mutex must be poisoned");

        // Now the actual test: poison_abort on the poisoned lock
        // must panic with a message starting with POISON_ABORT_PREFIX
        // and mentioning the caller-supplied slot.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            poison_abort(m.lock(), "MySlot")
        }));
        let payload =
            result.expect_err("T-13: poison_abort MUST panic on a poisoned lock, not return");
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&'static str>()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        assert!(
            msg.starts_with(POISON_ABORT_PREFIX),
            "T-13: poison_abort panic message must start with \
             POISON_ABORT_PREFIX = {POISON_ABORT_PREFIX:?} for log-\
             scan alerting; got {msg:?}"
        );
        assert!(
            msg.contains("MySlot"),
            "T-13: poison_abort message must include the caller-\
             supplied slot for triage; got {msg:?}"
        );
        assert!(
            msg.contains("DD-FailClosedOnInvariantBreak"),
            "T-13: message must cite the DD for operator traceability; \
             got {msg:?}"
        );
    }

    /// T-20 (2026-09-11, DD-FailClosedOnInvariantBreak):
    /// `join_err_abort` on a task-panic JoinError panics with the
    /// canonical prefix and preserves the underlying payload.
    #[tokio::test]
    async fn join_err_abort_panics_with_canonical_prefix_on_task_panic() {
        let je = tokio::task::spawn(async { panic!("underlying task panic") })
            .await
            .expect_err("spawned task should surface a JoinError");
        assert!(je.is_panic(), "test setup: JoinError must carry a panic");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| join_err_abort(je)));
        let payload = result.expect_err(
            "T-20: join_err_abort MUST panic on a task-panic JoinError, \
             not return",
        );
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&'static str>()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        assert!(
            msg.starts_with(JOIN_ERR_ABORT_PREFIX),
            "T-20: join_err_abort panic must start with \
             JOIN_ERR_ABORT_PREFIX = {JOIN_ERR_ABORT_PREFIX:?} for \
             log-scan alerting; got {msg:?}"
        );
        assert!(
            msg.contains("underlying task panic"),
            "T-20: the original panic payload must be preserved in \
             the abort banner for operator triage; got {msg:?}"
        );
        assert!(
            msg.contains("DD-FailClosedOnInvariantBreak"),
            "T-20: message must cite the DD for operator \
             traceability; got {msg:?}"
        );
    }

    /// T-20 lint pin (2026-09-11, DD-FailClosedOnInvariantBreak):
    /// scan all `.rs` files under `rholang/src/rust/interpreter/io/`
    /// and assert no raw `Err(_je) => HandlerReply::err(FSERR_IO,
    /// "spawn_blocking task failed")` pattern remains — the T-20
    /// migration replaced all such sites with `join_err_abort(je)`.
    /// A regression that reintroduces the soft-reply pattern
    /// masks the panic (option-(b) FSERR_IO) which DD-
    /// FailClosedOnInvariantBreak explicitly rejects.
    #[test]
    fn no_raw_spawn_blocking_join_err_soft_reply_outside_helper() {
        let io_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rust/interpreter/io");
        let mut offending: Vec<String> = Vec::new();

        for entry in std::fs::read_dir(io_dir).expect("io/ dir readable") {
            let entry = entry.expect("readable entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            // errors.rs is the helper's home — the string appears in
            // doc comments.  Skip it entirely.
            if file_name == "errors.rs" {
                continue;
            }
            let content = std::fs::read_to_string(&path).expect("io/ .rs file readable");
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("*")
                {
                    continue;
                }
                if line.contains("spawn_blocking task failed") && line.contains("Err(") {
                    offending.push(format!(
                        "{file_name}:{}: raw `Err(...) => ... \
                         \"spawn_blocking task failed\"` soft reply \
                         — replace with `Err(je) => \
                         super::errors::join_err_abort(je)` (see \
                         DD-FailClosedOnInvariantBreak): `{}`",
                        idx + 1,
                        line.trim(),
                    ));
                }
            }
        }

        assert!(
            offending.is_empty(),
            "T-20: raw `spawn_blocking task failed` soft reply \
             pattern found outside the `join_err_abort` helper.  Per \
             DD-FailClosedOnInvariantBreak, JoinError arms MUST \
             route through `join_err_abort(je)` for deploy abort:\n  \
             - {}",
            offending.join("\n  - "),
        );
    }

    /// T-13 lint pin (2026-09-11, DD-FailClosedOnInvariantBreak):
    /// scan all `.rs` files under `rholang/src/rust/interpreter/io/`
    /// and assert no raw `.expect("... poisoned")` pattern remains
    /// (all such sites should route through `poison_abort`).  Also
    /// scans for `.unwrap_or_else(|e| e.into_inner())` — the classic
    /// option-(a) accept-and-log shortcut that DD-
    /// FailClosedOnInvariantBreak explicitly rejects.
    ///
    /// If you're adding a new lock-guard acquisition site, use
    /// `poison_abort(lock.read(), "MyType.field")` or
    /// `poison_abort(lock.write(), ...)` from `super::errors`.
    #[test]
    fn no_raw_poison_expect_or_into_inner_outside_poison_abort_helper() {
        let io_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rust/interpreter/io");
        let mut offending: Vec<String> = Vec::new();

        let entries = std::fs::read_dir(io_dir).expect("io/ dir readable");
        for entry in entries {
            let entry = entry.expect("readable entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let content = std::fs::read_to_string(&path).expect("io/ .rs file readable");
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();

            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim_start();
                // Skip comment / doc lines.
                if trimmed.starts_with("//")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("*")
                {
                    continue;
                }
                // errors.rs is the helper's home — the string
                // "poisoned" appears in doc-comment examples and
                // panic-message literals inside `poison_abort` and
                // its test.  Skip errors.rs entirely (this test lives
                // there; the file is the source of truth).
                if file_name == "errors.rs" {
                    continue;
                }
                // Check for raw `.expect(...poisoned...)` — case-
                // insensitive on "poisoned" to catch both variants.
                let lower = line.to_lowercase();
                if lower.contains(".expect(") && lower.contains("poisoned") {
                    offending.push(format!(
                        "{file_name}:{}: raw `.expect(...poisoned...)` \
                         — replace with `poison_abort(...)` (see \
                         DD-FailClosedOnInvariantBreak): `{}`",
                        idx + 1,
                        line.trim(),
                    ));
                }
                // Check for `.into_inner()` on a `PoisonError` — the
                // option-(a) accept-poison shortcut.
                if lower.contains(".into_inner()")
                    && (lower.contains("poisonerror") || lower.contains("unwrap_or_else"))
                {
                    offending.push(format!(
                        "{file_name}:{}: `.into_inner()` on a \
                         PoisonError-shaped result — DD-\
                         FailClosedOnInvariantBreak forbids accept-\
                         and-log on poison; use `poison_abort(...)`: \
                         `{}`",
                        idx + 1,
                        line.trim(),
                    ));
                }
            }
        }

        assert!(
            offending.is_empty(),
            "T-13: raw poison-expect / poison-accept patterns found \
             outside the `poison_abort` helper.  Per \
             DD-FailClosedOnInvariantBreak (design-decisions.md § \
             DD-FailClosedOnInvariantBreak), all lock acquisitions \
             in the io/ tree must route through `poison_abort()`:\n  \
             - {}",
            offending.join("\n  - "),
        );
    }
}
