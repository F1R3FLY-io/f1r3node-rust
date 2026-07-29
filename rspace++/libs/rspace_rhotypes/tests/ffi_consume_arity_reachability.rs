//! # ★★ REACHABILITY — a malformed consume arrives from BYTES, and is refused
//!
//! ## What this file adds to `rspace++/tests/consume_arity_refusal.rs`
//!
//! That file proves the refusal **exists**. It builds `channels` and `patterns`
//! three lines above the call, so it cannot say whether anything outside the
//! crate can produce the fault — and reachability is the whole reason the
//! conversion is worth making. This file supplies it.
//!
//! ```text
//!   a foreign host encodes             the FFI decodes                the site
//!   ConsumeParams { channels: 2,  ──►  ConsumeParams::decode  ──►  RSpace::consume
//!                   patterns: 1 }      (lib.rs:254)                (arity guard)
//! ```
//!
//! ## ★ Why the fault is expressible AT ALL
//!
//! `models/src/main/protobuf/RSpacePlusPlusTypes.proto`:
//!
//! ```proto
//! message ConsumeParams {
//!   repeated rhoapi.Par         channels = 1;   // ← independent field
//!   repeated rhoapi.BindPattern patterns = 2;   // ← independent field
//!   rhoapi.TaggedContinuation   continuation = 3;
//!   bool                        persist = 4;
//!   repeated SortedSetElement   peeks = 5;
//! }
//! ```
//!
//! `InstallParams` has the same first two fields. Two independent `repeated`
//! fields cannot be constrained to equal length by protobuf, and a `repeated`
//! field may be absent entirely — so **every one of the three argument shapes
//! the guards discriminate on is a legal encoding**, and `prost` accepts all of
//! them without complaint. Nothing between `decode` and the guard looks at the
//! lengths: `lib.rs` moves `consume_params.channels` and
//! `consume_params.patterns` straight into the call.
//!
//! ## The reachability table, measured across the whole workspace
//!
//! | caller | how it builds the two vectors | can it express the fault? |
//! |---|---|---|
//! | `reduce.rs::consume_inner` (**every deploy**) | `binds.into_iter().unzip()` | **no** — one `unzip` cannot yield unequal halves |
//! | `reduce.rs::eval_receive` → `binds` | `Receive.binds`, and `p_input_normalizer` rejects `receipts.is_empty()` with `BugFoundError("Expected at least one receipt")` | **no** — `binds` is never empty from source |
//! | `rho_runtime::introduce_system_process` | `vec![name]` / `vec![BindPattern{..}]` | **no** — literally 1 and 1 |
//! | `casper::RuntimeOps::consume_system_result` | `vec![channel]` / `vec![pattern]` | **no** — literally 1 and 1 |
//! | `RSpace::restore_installs` | entries this space itself recorded | **no** — checked at install time |
//! | ★ **the FFI** (`consume`, `install`, `replay_consume`, and `rholang`'s `consume_result`) | two independent repeated proto fields | **YES** |
//!
//! ⇒ **the deploy path is closed by construction, and the FFI is the one open
//! door.** Both halves of that are load-bearing: the first says this change
//! cannot alter how any existing block evaluates, and the second says the guard
//! is not dead code.
//!
//! ## ⚠ The residual, stated rather than hidden
//!
//! The FFI does not merely pass the arguments through — it also `.unwrap()`s
//! the `Result` (`lib.rs:268`, `:331`, `:2059`). So a refused consume **still
//! ends the process at the FFI**, on the `unwrap`, not on the guard. That is a
//! *separate*, already-recorded defect: `rholang/tests/par_read_ceiling_site_registry.rs`
//! classes this surface `Asymmetric` and records that its signatures
//! (`-> *const u8`) already spend the null pointer on a legitimate outcome, so
//! making them refuse is an **ABI change and F1r3node's act**, not this
//! change's. That registry also bounds the severity: no in-tree caller of these
//! symbols exists.
//!
//! ★ [`f2_the_real_ffi_symbol_carries_the_refusal`] therefore measures exactly
//! what did change: the abort's **stderr now names `BugFoundError`**, which is
//! only possible if `RSpace::consume` *returned* the refusal instead of
//! panicking inside it. Before this change the same call printed a bare
//! `RUST ERROR: channels.length must equal patterns.length` from `panic!`. The
//! signal is the same; the thing that reaches the FFI is not.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::process::Command;
use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rspace_plus_plus_types::{ConsumeParams, InstallParams};
use models::rust::utils::new_gint_par;
use prost::Message;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

/// The env var that puts a re-executed test binary into child mode — the same
/// shape `ffi_absent_required_child.rs` established in this crate.
const CHILD: &str = "RSPACE_FFI_CONSUME_ARITY_CHILD";

/// The exact space the FFI's `Space` wraps. Built here over the in-memory store
/// manager so the W-cells need no filesystem; the F-cell uses the real
/// `space_new` and therefore does.
type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

async fn rho_space() -> RhoSpace {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm
        .r_space_stores()
        .await
        .expect("the in-memory store manager yields rspace stores");
    RSpace::create(store, Arc::new(Box::new(Matcher)))
        .expect("an RSpace over the Rho types is constructible")
}

// ═══════════════════════════════════════════════════════════════════════════
// The wire constructions
//
// ★ Built as PROTOBUF MESSAGES and round-tripped through `encode_to_vec` /
// `decode`, not as Rust values handed straight to the call. The round trip is
// the claim: these are byte strings a foreign host can actually send, and
// `prost` accepts every one of them.
// ═══════════════════════════════════════════════════════════════════════════

fn channel(n: i64) -> Par { new_gint_par(n, Vec::new(), false) }

fn wildcard_pattern() -> BindPattern {
    BindPattern {
        patterns: vec![Par::default()],
        remainder: None,
        free_count: 0,
    }
}

/// `channel_count` channels and `pattern_count` patterns — the two counts are
/// independent PARAMETERS here for the same reason they are independent FIELDS
/// on the wire.
fn consume_params_bytes(channel_count: usize, pattern_count: usize) -> Vec<u8> {
    let mut channels = Vec::with_capacity(channel_count);
    for i in 0..channel_count {
        channels.push(channel(i as i64));
    }
    let mut patterns = Vec::with_capacity(pattern_count);
    for _ in 0..pattern_count {
        patterns.push(wildcard_pattern());
    }

    ConsumeParams {
        channels,
        patterns,
        continuation: Some(TaggedContinuation::default()),
        persist: false,
        peeks: Vec::new(),
    }
    .encode_to_vec()
}

fn install_params_bytes(channel_count: usize, pattern_count: usize) -> Vec<u8> {
    let mut channels = Vec::with_capacity(channel_count);
    for i in 0..channel_count {
        channels.push(channel(i as i64));
    }
    let mut patterns = Vec::with_capacity(pattern_count);
    for _ in 0..pattern_count {
        patterns.push(wildcard_pattern());
    }

    InstallParams {
        channels,
        patterns,
        continuation: Some(TaggedContinuation::default()),
    }
    .encode_to_vec()
}

fn refusal_text<T: std::fmt::Debug>(outcome: &Result<T, RSpaceError>, what: &str) -> String {
    match outcome {
        Err(RSpaceError::BugFoundError(msg)) => msg.clone(),
        Err(other) => panic!("★ {what} refused with the wrong variant: {other:?}"),
        Ok(value) => panic!(
            "★★ {what} did NOT refuse — it returned Ok({value:?}). A byte string a foreign host \
             can encode reached the guard and was accepted."
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// W — from the wire, in process
//
// Each cell performs the FFI's OWN first line (`…Params::decode`) on bytes, then
// hands the decoded fields to the same method the FFI hands them to. What it
// does NOT do is re-raise the `Err` — which is the only difference from the
// real symbol, and is the residual documented in this file's header.
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn w1_wire_encoded_consume_shapes_are_refused_and_the_control_is_accepted() {
    let space = rho_space().await;

    // (channel_count, pattern_count, expected refusal or None for the control)
    let cases: [(usize, usize, Option<&str>); 4] = [
        (1, 1, None),
        (2, 2, None),
        (2, 1, Some("RUST ERROR: channels.length must equal patterns.length")),
        (0, 0, Some("RUST ERROR: channels can't be empty")),
    ];

    for (channel_count, pattern_count, expected) in cases {
        let bytes = consume_params_bytes(channel_count, pattern_count);

        // ★ THE FFI'S OWN LINE, reproduced: `lib.rs:254`.
        let params = ConsumeParams::decode(bytes.as_slice()).unwrap_or_else(|e| {
            panic!(
                "★★ prost REFUSED a {channel_count}-channel/{pattern_count}-pattern \
                 ConsumeParams: {e}. If the wire format could reject this shape the guard would \
                 be unreachable and this whole file would be vacuous — it cannot, and this line \
                 is what says so."
            )
        });
        assert_eq!(
            (params.channels.len(), params.patterns.len()),
            (channel_count, pattern_count),
            "★★ the decode did not preserve the two lengths, so the bytes are not carrying the \
             fault this cell claims they carry"
        );

        let outcome = space
            .consume(
                params.channels,
                params.patterns,
                params.continuation.expect("the encoded continuation is present"),
                params.persist,
                BTreeSet::new(),
            )
            .await;

        match expected {
            // ★ THE CONTROLS. Two of them, at both arities the REDs use, so
            // neither "2 channels" nor "consume at all" can explain a refusal.
            None => {
                assert!(
                    outcome
                        .unwrap_or_else(|e| panic!(
                            "★ CONTROL {channel_count}/{pattern_count} was refused with {e:?}"
                        ))
                        .is_none(),
                    "the control consume found no resting data, so it must return None"
                );
            }
            Some(message) => assert_eq!(
                refusal_text(&outcome, &format!("consume {channel_count}/{pattern_count}")),
                message
            ),
        }
    }
}

#[tokio::test]
async fn w2_wire_encoded_install_arity_mismatch_is_refused_and_the_control_is_accepted() {
    let space = rho_space().await;

    let control_bytes = install_params_bytes(1, 1);
    let control = InstallParams::decode(control_bytes.as_slice())
        .expect("a 1/1 InstallParams is a legal encoding");
    assert!(
        space
            .install(control.channels, control.patterns, control.continuation.expect("present"))
            .await
            .expect("★ CONTROL: a 1-channel/1-pattern install from the wire must succeed")
            .is_none(),
        "the control install found no resting data, so it must return None"
    );

    let red_bytes = install_params_bytes(2, 1);
    let red = InstallParams::decode(red_bytes.as_slice())
        .expect("★★ a 2-channel/1-pattern InstallParams is ALSO a legal encoding");
    assert_eq!((red.channels.len(), red.patterns.len()), (2, 1));

    let outcome = space
        .install(red.channels, red.patterns, red.continuation.expect("present"))
        .await;
    assert_eq!(
        refusal_text(&outcome, "install 2/1 from the wire"),
        "RUST ERROR: channels.length must equal patterns.length"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// F — the REAL exported symbol
// ═══════════════════════════════════════════════════════════════════════════

/// F1 — the CONTROL through the real `extern "C"` symbol, in this process.
///
/// It must return, or F2's abort is attributable to calling the entry point at
/// all rather than to the malformed arity.
#[test]
fn f1_control_a_wellformed_consume_returns_through_the_real_ffi_symbol() {
    let dir = temp_space_dir("control");
    let space = new_space(&dir);
    let payload = consume_params_bytes(1, 1);

    let result = rspace_plus_plus_rhotypes::consume(space, payload.as_ptr(), payload.len());
    assert!(
        result.is_null(),
        "a consume that finds no resting data returns the null pointer for 'no match'; a non-null \
         result here would mean the control matched something, and the space is fresh"
    );

    println!("  F1 control: a 1/1 ConsumeParams returned through the real FFI symbol");
    let _ = std::fs::remove_dir_all(&dir);
}

/// ★★ F2 — the same symbol, `channels.len() != patterns.len()`, in a child.
///
/// ⚠ THIS CELL DOES NOT ASSERT THAT THE PROCESS SURVIVES, and it is not
/// expecting a panic as a *result*: it is measuring a **process outcome** that
/// `catch_unwind` provably cannot observe (an `extern "C"` frame runs the
/// non-unwinding shim), which is why it runs out of process — pattern 3 of
/// `shared/tests/panic_expectation_gate.rs`.
///
/// What it measures is the CONTENT of the abort. The refusal `RSpace::consume`
/// now returns travels out of the guard, out of `block_on`, and is re-raised by
/// the FFI's own `.unwrap()` — so `BugFoundError` appears in stderr. A `panic!`
/// inside the guard could never put that string there.
#[test]
fn f2_the_real_ffi_symbol_carries_the_refusal() {
    if std::env::var(CHILD).is_ok() {
        // ── child ───────────────────────────────────────────────────────────
        // The control runs FIRST and in the same process, so if the child dies
        // for an unrelated reason (a missing symbol, an unopenable store) the
        // parent reports that instead of a bare "wrong exit code".
        let dir = temp_space_dir("child");
        let space = new_space(&dir);

        let control = consume_params_bytes(1, 1);
        let control_result =
            rspace_plus_plus_rhotypes::consume(space, control.as_ptr(), control.len());
        println!("CHILD_CONTROL_RETURNED={}", control_result.is_null());

        let malformed = consume_params_bytes(2, 1);
        let survived =
            rspace_plus_plus_rhotypes::consume(space, malformed.as_ptr(), malformed.len());
        println!("CHILD_SURVIVED_MALFORMED=true ptr_null={}", survived.is_null());
        return;
    }

    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = Command::new(exe)
        .env(CHILD, "1")
        .args([
            "--exact",
            "f2_the_real_ffi_symbol_carries_the_refusal",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .expect("the child test process spawns");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.contains("CHILD_CONTROL_RETURNED=true"),
        "★★ the child did not get as far as its own control, so it is not measuring the arity \
         mismatch at all.\n--- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );

    // ★★ THE CELL. `BugFoundError` in stderr means the guard RETURNED its
    // refusal and something upstream re-raised it. Before the conversion the
    // guard called `panic!` directly and this string did not exist.
    assert!(
        stderr.contains("BugFoundError")
            && stderr.contains("RUST ERROR: channels.length must equal patterns.length"),
        "★★ the child died, but stderr does not carry the REFUSAL. Either the guard is panicking \
         again instead of returning `Err`, or the message drifted from the one the other three \
         sites use.\n--- child stderr ---\n{stderr}"
    );

    // The residual, pinned rather than hidden: the FFI's own `.unwrap()` still
    // ends the process. Fixing THAT is an ABI change (`-> *const u8` already
    // spends null on 'no match') and F1r3node's act — see this file's header
    // and `rholang/tests/par_read_ceiling_site_registry.rs`.
    assert!(
        !stdout.contains("CHILD_SURVIVED_MALFORMED"),
        "★ the FFI entry point RETURNED on a malformed `ConsumeParams`. That is BETTER than what \
         is recorded here, but it means the residual documented in this file's header has been \
         fixed and the disposition in `par_read_ceiling_site_registry.rs` is now stale. \
         Re-derive it.\n--- child stdout ---\n{stdout}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            output.status.signal(),
            Some(6),
            "★ the child died, but not with SIGABRT (signal {:?}). A SIGSEGV would be a \
             stack-depth fault wearing this defect's clothes.\n--- child stderr ---\n{stderr}",
            output.status.signal()
        );
    }

    println!(
        "  ⇒ F2: a {}-byte `ConsumeParams` that `prost` ACCEPTS reaches `RSpace::consume`, is \
         REFUSED there, and the refusal reaches stderr through the FFI's own unwrap",
        consume_params_bytes(2, 1).len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// F-cell plumbing
// ═══════════════════════════════════════════════════════════════════════════

/// A private directory for one `space_new`. `space_new` opens an LMDB
/// environment under `<dir>/rspace++/`, so two spaces must not share a path.
fn temp_space_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rspace-ffi-consume-arity-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("the temporary space directory is creatable");
    dir
}

fn new_space(dir: &std::path::Path) -> *mut rspace_plus_plus_rhotypes::Space {
    let path = CString::new(dir.to_str().expect("the temporary path is UTF-8"))
        .expect("the temporary path has no interior NUL");
    rspace_plus_plus_rhotypes::space_new(path.as_ptr())
}
