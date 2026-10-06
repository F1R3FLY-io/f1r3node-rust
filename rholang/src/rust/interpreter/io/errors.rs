// String error codes returned to Rholang callers as the second element
// of `[false, code, msg]` responses.
//
// Both the caller-facing `FSERR_*: FserrCode` const and the WAL-wire
// `FSERR_CODE_*: u32` const are declared in a single
// `consensus_error_codes!` invocation below.  The macro emits four
// artifacts from one source of truth:
//
//   1. `pub const FSERR_<NAME>: FserrCode = FserrCode("FSERR_<NAME>");`
//      for caller-facing pattern-match ergonomics.
//   2. `pub const FSERR_CODE_<NAME>: u32 = <code>;` for compact
//      WAL wire encoding.
//   3. `pub fn fserr_to_code(&str) -> u32` — the taxonomy bridge.
//   4. A `const _: () = { ... }` compile-time assertion that the
//      declared codes are contiguous from 1 up to N (with 0
//      reserved as UNKNOWN).
//
// DO NOT reorder or renumber existing codes.  The u32 mapping is a
// consensus surface: a downstream fingerprint fold picks up any drift,
// but the string/int coherence is best pinned at the source.

use std::io;

use paste::paste;

/// Typed FSERR code, a newtype over `&'static str`.  The compiler
/// catches "someone passed a raw string where a canonical error code
/// was expected" at every call site that used to accept any
/// `&'static str`.
///
/// # Invariants
///
/// - The inner `&'static str` must match the identifier name
///   spec-canonical (`"FSERR_BAD_ARG"` for `FSERR_BAD_ARG`).  The
///   `consensus_error_codes!` macro enforces this at declaration
///   time via `stringify!`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FserrCode(pub &'static str);

impl FserrCode {
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

/// Single-source-of-truth macro for the FSERR taxonomy.
///
/// Emits `pub const FSERR_BAD_ARG: FserrCode = FserrCode("FSERR_BAD_ARG");`
/// and `pub const FSERR_CODE_BAD_ARG: u32 = 1;` for each entry, plus
/// the `fserr_to_code` bridge and a compile-time contiguity check.
macro_rules! consensus_error_codes {
    ( $( ( $name:ident, $code:expr ) ),+ $(,)? ) => {
        paste! {
            $(
                pub const [<FSERR_ $name>]: FserrCode =
                    FserrCode(stringify!([<FSERR_ $name>]));
                pub const [<FSERR_CODE_ $name>]: u32 = $code;
            )+

            /// Map a spec-canonical FSERR string to its stable u32 code
            /// for on-wire encoding.  Unknown / non-canonical inputs
            /// return `FSERR_CODE_UNKNOWN` (never panics).
            pub fn fserr_to_code(s: &str) -> u32 {
                match s {
                    $(
                        s if s == [<FSERR_ $name>].as_str() => [<FSERR_CODE_ $name>],
                    )+
                    _ => FSERR_CODE_UNKNOWN,
                }
            }

            /// Compile-time contiguity check: declared codes must be
            /// exactly `[1, N]` (with 0 reserved as UNKNOWN).
            const _: () = {
                let codes: &[u32] = &[$([<FSERR_CODE_ $name>]),+];
                let n = codes.len();
                let mut i = 0;
                while i < n {
                    let expected = (i as u32) + 1;
                    assert!(
                        codes[i] == expected,
                        "FSERR codes must be contiguous 1..N with no \
                         gaps or duplicates; a mis-numbered code was \
                         declared in `consensus_error_codes!`",
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
    (CANCELLED,            12),
    (CONSENSUS_DIVERGENCE, 13),
    (DEADLOCK,             14),
    (REVOKED,              15),
}

/// Map a `std::io::Error` kind to a stable FSERR code.  Callers invoke
/// this at the boundary between kernel errors and Rholang reply Pars.
/// Not derived from the macro because the mapping is FROM a foreign
/// taxonomy (`io::ErrorKind`) TO ours.
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

/// Canonical prefix for `poison_abort` panic messages.  Kept as a
/// public constant so tests can assert the panic shape without
/// hard-coding the full message text (which may evolve for
/// clarity), and so operational log-scan alerting can grep for
/// this exact prefix.  Keep it stable across releases.
pub const POISON_ABORT_PREFIX: &str = "io/ lock poisoned";

/// Fail-closed lock-poison handler per **DD-FailClosedOnInvariantBreak**.
///
/// A poisoned lock means a thread panicked while holding it —
/// the protected invariant may be half-updated, and any consumer
/// that reads the guard back could observe inconsistent state.
/// Two failure-mode options exist for the caller:
///
///   (a) `.unwrap_or_else(|e| e.into_inner())` — accept the
///       potentially-broken state and continue.  Rejected by
///       DD-FailClosedOnInvariantBreak because a follower silently
///       applying a corrupt state derivative to a consensus reply
///       could produce a tuplespace fork that never surfaces at
///       the boundary.
///   (b) `.expect("lock poisoned")` — panic with a bespoke
///       message.  Correct in spirit but the message varies per
///       call site, defeating log-scan alerting.
///
/// This helper picks (b) with a **canonical** message prefix
/// (`POISON_ABORT_PREFIX`) plus the caller-supplied slot name for
/// operator triage.  Every lock-guard acquisition in `io/*.rs`
/// MUST route through here — enforced by the
/// `no_raw_poison_expect_or_into_inner_outside_poison_abort_helper`
/// source-scan test below.
///
/// The `slot` parameter is a human-readable identifier for the
/// specific lock (`"Wal.entries"`, `"LockRegistry.inner"`, etc.) —
/// appears in the panic message.  Keep slot names stable across
/// releases so log-scan alerting doesn't false-negative on
/// cosmetic renames.
///
/// # Consensus surface
///
/// None.  The panic bytes travel via `PoisonError` but are
/// consumed at the deploy layer without touching WAL bytes, reply
/// Pars, or fingerprint inputs.
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

/// Canonical prefix for `join_err_abort` panic messages.  Kept as
/// a public constant so tests can assert the panic shape and
/// operational log scanning can grep this specific hazard class.
pub const JOIN_ERR_ABORT_PREFIX: &str = "io/ handler JoinError";

/// Fail-closed `tokio::task::JoinError` handler per
/// **DD-FailClosedOnInvariantBreak**.
///
/// A `spawn_blocking` task that panicked (or was cancelled by the
/// runtime shutting down) is an invariant break — the syscall body
/// failed unrecoverably.  The pre-hardening pattern
/// `Err(_je) => HandlerReply::err(FSERR_IO, "spawn_blocking task
/// failed")` produced a soft FSERR_IO reply that burned budget
/// but hid the underlying bug from the deploy-outcome layer.
///
/// This helper replaces that pattern with a canonical
/// `panic!` — the underlying panic payload (message,
/// downcastable) is preserved in the abort banner so operators
/// see the underlying cause alongside the abort prefix.
///
/// # Panic propagation across the async runtime
///
/// This function `panic!`s from within the return value of a fn
/// invoked inside an `async fn`.  For that panic to reach the
/// deploy-outcome layer (and be treated as an invariant break
/// per DD-FailClosedOnInvariantBreak), the async runtime MUST
/// unwind the panic across the `await` boundary.  Tokio does this
/// by default via `catch_unwind`.  If a future runtime swap
/// changes this default, `join_err_abort` needs re-plumbing (an
/// explicit deploy-scope cell that gets flipped to "aborted" and
/// consulted at deploy end).
///
/// # Consensus surface
///
/// None.  Pre- and post-hardening shapes both fail the deploy on
/// a task panic (the pre-hardening version produced a soft
/// FSERR_IO reply that burned budget; the post-hardening version
/// aborts).  No WAL bytes, reply Pars, or fingerprint inputs
/// change — the difference is at the deploy-outcome layer.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip pin: every declared FSERR_* string maps to the
    /// corresponding FSERR_CODE_* u32 under `fserr_to_code`.
    #[test]
    fn fserr_string_to_code_round_trip_pins_every_declared_code() {
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
            assert_eq!(fserr_to_code(s), *code);
            let expected_str = format!("FSERR_{}", &s[6..]);
            assert_eq!(s, expected_str);
        }
        assert_eq!(fserr_to_code("bogus"), FSERR_CODE_UNKNOWN);
        assert_eq!(fserr_to_code(""), FSERR_CODE_UNKNOWN);
    }

    /// Pin the `FserrCode` newtype behavior.
    #[test]
    fn fserr_code_newtype_shape_pins() {
        let code = FSERR_BAD_ARG;
        assert_eq!(code.as_str(), "FSERR_BAD_ARG");
        assert_eq!(code, "FSERR_BAD_ARG");
        assert_eq!("FSERR_BAD_ARG", code);
        assert_eq!(format!("{code}"), "FSERR_BAD_ARG");
        assert_ne!(FSERR_BAD_ARG, FSERR_IO);
        assert_ne!(FSERR_BAD_ARG.as_str(), FSERR_IO.as_str());
    }

    /// Pin the current-slice count of FSERR codes at 15.  Adding a new
    /// code is a consensus surface change and requires a coordinated
    /// peer upgrade — bumping this pin is the flag for a reviewer to
    /// verify the code was appended (not inserted) at the tail.
    #[test]
    fn fserr_code_count_is_pinned() {
        assert_eq!(FSERR_CODE_REVOKED, 15);
    }

    // --- DD-FailClosedOnInvariantBreak: poison_abort ---------------

    /// Extract a `catch_unwind` payload as a `String` — covers both
    /// `String` and `&'static str` panic shapes.  Non-string
    /// payloads (`panic_any(u32)`, etc.) return an empty string,
    /// which the caller can then treat as a distinct failure mode.
    /// Hoisted so the panic-shape tests don't repeat the downcast
    /// chain.
    fn panic_msg(payload: &Box<dyn std::any::Any + Send>) -> String {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&'static str>()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default()
    }

    /// A non-poisoned `Ok` guard passes through unchanged; no
    /// panic.
    #[test]
    fn poison_abort_passes_through_unpoisoned_guard() {
        let m = std::sync::Mutex::new(42u32);
        let guard = poison_abort(m.lock(), "test.slot");
        assert_eq!(*guard, 42);
    }

    /// A poisoned lock triggers a panic whose message starts with
    /// `POISON_ABORT_PREFIX` and includes both the caller-supplied
    /// slot and the DD citation.  The prefix is load-bearing for
    /// operational log-scan alerting on this specific hazard class
    /// — keep it stable.
    #[test]
    fn poison_abort_panics_with_canonical_prefix_on_poison() {
        use std::sync::{Arc, Mutex};
        let m = Arc::new(Mutex::new(0u32));
        let m2 = Arc::clone(&m);
        // Poison the mutex: spawn a thread that panics while
        // holding the guard.
        let handle = std::thread::spawn(move || {
            let _guard = m2.lock().unwrap();
            panic!("intentional test poison");
        });
        let _ = handle.join(); // Absorb the thread's panic.
        assert!(m.is_poisoned(), "test setup: mutex must be poisoned");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            poison_abort(m.lock(), "MySlot")
        }));
        let payload = result.expect_err("poison_abort MUST panic on a poisoned lock, not return");
        let msg = panic_msg(&payload);
        assert!(
            msg.starts_with(POISON_ABORT_PREFIX),
            "poison_abort panic message must start with \
             POISON_ABORT_PREFIX = {POISON_ABORT_PREFIX:?} for \
             log-scan alerting; got {msg:?}"
        );
        assert!(
            msg.contains("MySlot"),
            "poison_abort message must include the caller-supplied \
             slot for triage; got {msg:?}"
        );
        assert!(
            msg.contains("DD-FailClosedOnInvariantBreak"),
            "message must cite the DD for operator traceability; \
             got {msg:?}"
        );
    }

    // --- DD-FailClosedOnInvariantBreak: join_err_abort -------------

    /// A task-panic `JoinError` triggers a panic whose message
    /// starts with `JOIN_ERR_ABORT_PREFIX`, preserves the
    /// underlying panic payload, and cites the DD.
    #[tokio::test]
    async fn join_err_abort_panics_with_canonical_prefix_on_task_panic() {
        let je = tokio::task::spawn(async { panic!("underlying task panic") })
            .await
            .expect_err("spawned task should surface a JoinError");
        assert!(je.is_panic(), "test setup: JoinError must carry a panic");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| join_err_abort(je)));
        let payload = result.expect_err("join_err_abort MUST panic on a task-panic JoinError");
        let msg = panic_msg(&payload);
        assert!(
            msg.starts_with(JOIN_ERR_ABORT_PREFIX),
            "join_err_abort panic must start with \
             JOIN_ERR_ABORT_PREFIX = {JOIN_ERR_ABORT_PREFIX:?} for \
             log-scan alerting; got {msg:?}"
        );
        assert!(
            msg.contains("underlying task panic"),
            "original panic payload must be preserved in the abort \
             banner for operator triage; got {msg:?}"
        );
        assert!(
            msg.contains("DD-FailClosedOnInvariantBreak"),
            "message must cite the DD for operator traceability; \
             got {msg:?}"
        );
    }

    /// A cancellation `JoinError` (task aborted via `handle.abort()`)
    /// exercises the `!je.is_panic()` branch — the abort banner
    /// includes the "cancelled (runtime shutdown?)" cause string
    /// instead of a downcasted panic payload.  Guards against a
    /// future refactor that special-cases only the panic branch.
    #[tokio::test]
    async fn join_err_abort_panics_with_canonical_prefix_on_task_cancel() {
        // Spawn a long-sleep task then abort it — produces a
        // JoinError with `is_cancelled() == true` (the branch the
        // panic-only test above skips).
        let handle = tokio::task::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await
        });
        handle.abort();
        let je = handle
            .await
            .expect_err("aborted task should surface a JoinError");
        assert!(
            !je.is_panic(),
            "test setup: cancellation JoinError must NOT be a panic variant"
        );

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| join_err_abort(je)));
        let payload = result.expect_err("join_err_abort MUST panic on any JoinError variant");
        let msg = panic_msg(&payload);
        assert!(
            msg.starts_with(JOIN_ERR_ABORT_PREFIX),
            "cancellation-branch panic must also start with \
             JOIN_ERR_ABORT_PREFIX; got {msg:?}"
        );
        assert!(
            msg.contains("cancelled"),
            "cancellation-branch cause string must be included in \
             the abort banner; got {msg:?}"
        );
        assert!(
            msg.contains("DD-FailClosedOnInvariantBreak"),
            "cancellation-branch message must cite the DD too; \
             got {msg:?}"
        );
    }

    /// A `panic_any(u32)` task produces a `JoinError` whose panic
    /// payload is neither `String` nor `&'static str` — the
    /// downcast falls through to the `<non-string panic payload>`
    /// fallback.  Guards the payload-extraction chain against
    /// silently dropping to a bare prefix when a downstream
    /// caller uses a structured panic type.
    #[tokio::test]
    async fn join_err_abort_falls_back_on_non_string_panic_payload() {
        let je = tokio::task::spawn(async { std::panic::panic_any(42u32) })
            .await
            .expect_err("panic_any task should surface a JoinError");
        assert!(je.is_panic(), "test setup: JoinError must carry a panic");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| join_err_abort(je)));
        let payload = result.expect_err("join_err_abort MUST panic on any panic variant");
        let msg = panic_msg(&payload);
        assert!(
            msg.starts_with(JOIN_ERR_ABORT_PREFIX),
            "non-string-payload branch must still start with the \
             canonical prefix; got {msg:?}"
        );
        assert!(
            msg.contains("<non-string panic payload>"),
            "non-string payload must surface the sentinel string so \
             operators know the underlying panic type wasn't a \
             String / &str; got {msg:?}"
        );
    }

    // --- Source-scan discipline ------------------------------------

    /// Recursive collector — matches the pattern the
    /// `consensus_fingerprint` source-scan uses so subdirectory
    /// modules (like `path/`) are covered.
    fn collect_io_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("io/ subtree readable at {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("readable entry");
            let path = entry.path();
            if path.is_dir() {
                collect_io_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    /// Scan `io/**/*.rs` (recursive) for raw `.expect(...poisoned...)`
    /// and `.into_inner()` on a `PoisonError` — the two shortcuts
    /// DD-FailClosedOnInvariantBreak rejects.  Every lock-guard
    /// acquisition site in `io/` MUST route through
    /// `poison_abort(lock.read(), "MyType.field")` or
    /// `poison_abort(lock.write(), ...)`.
    ///
    /// `errors.rs` is exempt — the strings appear in the docstring
    /// examples above and the panic message inside `poison_abort`
    /// itself, which is the source of truth.
    ///
    /// # Scope: regression-only
    ///
    /// This is a substring-match on trimmed source lines, not a
    /// Rust parser.  It catches the *typical* bypass shapes (a
    /// literal `.expect("<something> poisoned")` on one line, or
    /// `.into_inner()` co-located with `PoisonError` /
    /// `unwrap_or_else` on the same line) but not adversarial
    /// obfuscation: a two-line `match` that pulls `e.into_inner()`
    /// out of an `Err` arm several lines below the guard call would
    /// slip past, as would a rename of the panic message wording.
    /// Code review is the primary enforcement; this scan is the
    /// automated tripwire that catches good-faith regressions.
    #[test]
    fn no_raw_poison_expect_or_into_inner_outside_poison_abort_helper() {
        let io_dir = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/rust/interpreter/io"
        ));
        let mut rs_files = Vec::new();
        collect_io_rs_files(&io_dir, &mut rs_files);

        // Sanity check: recursion must reach at least one file inside
        // a subdirectory (`io/path/*.rs` exists post-Wave 1).
        let has_subdir_file = rs_files.iter().any(|p| {
            p.strip_prefix(&io_dir)
                .map(|rel| rel.components().count() > 1)
                .unwrap_or(false)
        });
        assert!(
            has_subdir_file,
            "recursion is broken — expected at least one .rs file in \
             an io/ subdirectory (e.g., `io/path/*.rs`), found none"
        );

        let mut offending: Vec<String> = Vec::new();
        for path in &rs_files {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            // errors.rs is the helper's home — exempt.
            if file_name == "errors.rs" {
                continue;
            }
            let content = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("readable .rs file {}: {e}", path.display()));
            let display_path = path
                .strip_prefix(&io_dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("*")
                {
                    continue;
                }
                let lower = line.to_lowercase();
                if lower.contains(".expect(") && lower.contains("poisoned") {
                    offending.push(format!(
                        "{display_path}:{}: raw `.expect(...poisoned...)` \
                         — replace with `poison_abort(...)` (see \
                         DD-FailClosedOnInvariantBreak): `{}`",
                        idx + 1,
                        line.trim(),
                    ));
                }
                if lower.contains(".into_inner()")
                    && (lower.contains("poisonerror") || lower.contains("unwrap_or_else"))
                {
                    offending.push(format!(
                        "{display_path}:{}: `.into_inner()` on a \
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
            "raw poison-shortcut patterns found in io/*.rs — all \
             lock-guard acquisitions MUST route through \
             `poison_abort(...)`:\n  - {}",
            offending.join("\n  - "),
        );
    }

    /// Scan `io/**/*.rs` (recursive) for the pre-hardening pattern
    /// `Err(...) => ... "spawn_blocking task failed"` soft reply —
    /// replaced by `join_err_abort(je)`.  A regression that
    /// reintroduces the soft-reply pattern masks the panic as
    /// FSERR_IO, which DD-FailClosedOnInvariantBreak rejects.
    ///
    /// # Scope: regression-only
    ///
    /// Same limitation as
    /// `no_raw_poison_expect_or_into_inner_outside_poison_abort_helper`
    /// above: substring match on the exact `"spawn_blocking task
    /// failed"` literal, one line at a time.  A cosmetic reword
    /// (`"spawn_blocking failure"`) or a two-line `Err(je) => { ...
    /// "spawn_blocking task failed" ... }` split slips past.  Code
    /// review is primary; the scan catches the copy-paste
    /// regression the pattern was named for.
    #[test]
    fn no_raw_spawn_blocking_join_err_soft_reply_outside_helper() {
        let io_dir = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/rust/interpreter/io"
        ));
        let mut rs_files = Vec::new();
        collect_io_rs_files(&io_dir, &mut rs_files);

        let mut offending: Vec<String> = Vec::new();
        for path in &rs_files {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            if file_name == "errors.rs" {
                continue;
            }
            let content = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("readable .rs file {}: {e}", path.display()));
            let display_path = path
                .strip_prefix(&io_dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
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
                        "{display_path}:{}: raw `Err(...) => ... \
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
            "raw spawn_blocking soft-reply patterns found in io/*.rs \
             — all `JoinError` handling MUST route through \
             `join_err_abort(je)`:\n  - {}",
            offending.join("\n  - "),
        );
    }
}
