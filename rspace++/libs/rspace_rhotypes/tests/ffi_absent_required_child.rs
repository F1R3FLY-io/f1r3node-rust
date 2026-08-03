//! # ★★ #136/#127 — the FFI boundary, measured: a legal wire `Par` ABORTS the process
//!
//! ## Why this file is in `rspace_rhotypes` and not in `rholang`
//!
//! `rspace_plus_plus_rhotypes::spatial_match_result` is **both** of the two
//! surfaces #127 and #136 were filed against, in one function:
//!
//! ```text
//!   #136 ── an FFI read that PANICS instead of returning Err ──┐
//!                                                              ├─ lib.rs:144-148
//!   #127 ── a wire-decoded `Par` handed to `spatial_match`  ───┘
//! ```
//!
//! ```ignore
//! let target  = Par::decode(target_slice).unwrap();     // ← #136's read
//! let pattern = Par::decode(pattern_slice).unwrap();
//! let mut spatial_matcher = SpatialMatcherContext::new();
//! let result_option = spatial_matcher.spatial_match_result(target, pattern);  // ← #127's site
//! ```
//!
//! It is the ONLY production path in the workspace that hands a `Par` decoded
//! from caller-supplied bytes to the spatial matcher with a caller-supplied
//! pattern. Every other route to the matcher meets a pattern fixed by an
//! earlier, honest match (see
//! `rholang/tests/absent_required_child_reachability.rs` for that measurement
//! and its positive control).
//!
//! ## What is measured here
//!
//! | # | claim | result |
//! |---|---|---|
//! | F1 | a well-formed pair through the real FFI symbol returns normally | **green** — the CONTROL |
//! | F2 | a pair whose target has an absent required child **aborts** | **SIGABRT** |
//!
//! ⚠ F2 is a *subprocess* test because the fault is **not catchable**. On this
//! toolchain (`rustc 1.95.0-nightly`) an `extern "C"` function that panics runs
//! the implicit non-unwinding shim: the runtime prints *"thread caused
//! non-unwinding panic. aborting."* and raises `SIGABRT`. It is therefore
//! **not** undefined behaviour — the question #136 asks first — but it is
//! strictly worse than a thread panic: `catch_unwind` cannot intercept it, no
//! host-side handler runs, and the whole process dies, not one worker.
//!
//! ## Anti-vacuity
//!
//! F2 without F1 would be satisfied by an FFI entry point that aborts on
//! *everything* — a broken build, a null pointer, a missing symbol. F1 runs the
//! identical call shape on well-formed bytes in the same process and requires
//! it to return, so the difference between the two cells is exactly the absent
//! child. F2 additionally requires the abort to be `SIGABRT` specifically,
//! because a `SIGSEGV` would be a different defect wearing this one's clothes.

use std::process::Command;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMinus, Expr, Par};
use models::rust::utils::{new_freevar_par, new_gint_par};
use prost::Message;

/// The env var that puts a re-executed test binary into child mode.
const CHILD: &str = "RSPACE_FFI_ABSENT_CHILD";

/// ★ The WIRE construction. `Par.connective_used` is a proto **field**, decoded
/// verbatim; nothing recomputes it on the read path, so a probe that reproduces
/// what arrives over the wire sets it directly rather than deriving it.
fn protobuf_par(instance: ExprInstance, connective_used: bool) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        connective_used,
        ..Default::default()
    }
}

/// `p1` absent. ★ This is not a hand-built Rust `None` smuggled past a
/// constructor: it is what `prost` produces for a `Par` whose `EMinus.p1`
/// submessage is simply not present in the byte string, which is a legal
/// protobuf encoding that `Par::decode` accepts without complaint.
fn target_with_absent_child() -> Vec<u8> {
    protobuf_par(
        ExprInstance::EMinusBody(EMinus {
            p1: None,
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }),
        false,
    )
    .encode_to_vec()
}

fn well_formed_target() -> Vec<u8> {
    protobuf_par(
        ExprInstance::EMinusBody(EMinus {
            p1: Some(new_gint_par(7, Vec::new(), false)),
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }),
        false,
    )
    .encode_to_vec()
}

/// A pattern that forces the matcher into the `EMinus` arm: same variant, with
/// a free variable in slot 0.
fn pattern_bytes() -> Vec<u8> {
    protobuf_par(
        ExprInstance::EMinusBody(EMinus {
            p1: Some(new_freevar_par(0, Vec::new())),
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }),
        true,
    )
    .encode_to_vec()
}

/// Call the REAL exported symbol, exactly as a foreign host does: one
/// contiguous buffer, split by the two lengths.
fn call_ffi(target: &[u8], pattern: &[u8]) -> *const u8 {
    let mut payload = Vec::with_capacity(target.len() + pattern.len());
    payload.extend_from_slice(target);
    payload.extend_from_slice(pattern);
    rspace_plus_plus_rhotypes::spatial_match_result(payload.as_ptr(), target.len(), pattern.len())
}

/// F1 — the CONTROL. Well-formed bytes through the same entry point return.
#[test]
fn f1_control_a_well_formed_pair_returns_through_the_ffi() {
    let result = call_ffi(&well_formed_target(), &pattern_bytes());
    assert!(
        !result.is_null(),
        "★ F1 CONTROL: a well-formed `EMinus` target and an `EMinus` pattern with a free variable \
         in slot 0 did NOT match through the FFI entry point. Without this cell green, F2's abort \
         is not attributable to the absent child — it would be attributable to calling the entry \
         point at all."
    );
    // The FFI leaks its result buffer by design (`Box::leak`); the host frees it
    // through `deallocate_memory`. Nothing is asserted about the contents here —
    // F1 exists to show the call SHAPE is sound.
    println!("  F1 control: the well-formed pair matched through the real FFI symbol");
}

/// ★★ F2 — the RED. The same call, one absent child, and the process dies.
#[test]
fn f2_an_absent_required_child_aborts_the_process_at_the_ffi_boundary() {
    if std::env::var(CHILD).is_ok() {
        // ── child: make the call that is expected to abort ──────────────────
        //
        // The control runs FIRST, in the same process. If the child somehow
        // survives the malformed call, its stdout says so and the parent fails
        // with that text rather than with a bare "wrong exit code".
        let control = call_ffi(&well_formed_target(), &pattern_bytes());
        println!("CHILD_CONTROL_MATCHED={}", !control.is_null());

        let survived = call_ffi(&target_with_absent_child(), &pattern_bytes());
        println!("CHILD_SURVIVED_MALFORMED=true ptr_null={}", survived.is_null());
        return;
    }

    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = Command::new(exe)
        .env(CHILD, "1")
        .args([
            "--exact",
            "f2_an_absent_required_child_aborts_the_process_at_the_ffi_boundary",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .expect("the child test process spawns");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // ★ The child's OWN control must have passed, or the child died for an
    // unrelated reason and this measurement is of something else.
    assert!(
        stdout.contains("CHILD_CONTROL_MATCHED=true"),
        "★★ the child process did not get as far as its own control. It is not measuring the \
         absent child at all.\n--- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );

    assert!(
        !stdout.contains("CHILD_SURVIVED_MALFORMED"),
        "★★★ the FFI entry point RETURNED on a `Par` with an absent required child. The panic \
         this task is about is gone — which is good news, but it means this guard no longer \
         measures anything and the disposition recorded in `par_read_ceiling_site_registry.rs` is \
         stale. Re-derive it.\n--- child stdout ---\n{stdout}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            output.status.signal(),
            Some(6),
            "★★ the child died, but not with SIGABRT (signal {:?}, status {:?}). This test claims \
             a NON-UNWINDING PANIC at an `extern \"C\"` frame; a SIGSEGV would be a stack-depth \
             fault and a clean exit would be a different failure entirely.\n--- child stderr \
             ---\n{stderr}",
            output.status.signal(),
            output.status
        );
    }

    assert!(
        stderr.contains("non-unwinding panic"),
        "★ the child aborted, but the runtime did not report the non-unwinding shim. The finding \
         is specifically that `catch_unwind` CANNOT intercept this, which is what that message \
         evidences.\n--- child stderr ---\n{stderr}"
    );

    println!(
        "  ⇒ F2: a {}-byte `Par` that `Par::decode` ACCEPTS aborts the process (SIGABRT, \
         non-unwinding) at `spatial_match_result`",
        target_with_absent_child().len()
    );
}
