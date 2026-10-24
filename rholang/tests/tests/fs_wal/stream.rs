// Stream family — per-fd directory-entries streaming primitives.
//
// 2 tests exercising the Phase-2 Consensus ban + Oracular-does-not-
// journal invariants.  See parent module `fs_wal_spec::tests` for
// shared setup (create_runtime, rand).
//
// Wave-3 S3.14 (2026-09-10) — split out of `fs_wal_spec.rs` via
// `#[path]` submodule.  All tests still run as part of the single
// `fs_wal_spec` integration binary.

use super::*;

/// Phase 2 ban pin (Consensus re-execute + verify, 2026-09-01):
/// **`entriesStreamOpen` with cmode="consensus" MUST reject
/// with `FSERR_UNSUPPORTED`.**  See handlers.rs's
/// `fs_entries_stream_open` for the design rationale: readdir
/// order is fs-dependent and not stable across D3 per-validator
/// subdirs, so a Consensus-cap stream would trip spurious
/// CONSENSUS_DIVERGENCE on any two validators with independently-
/// created copies of the same logical directory.  Users are
/// directed to bulk `fs_entries` (sorted, deterministic) instead.
///
/// A regression that dropped the ban would let Consensus stream
/// opens through; downstream `entriesStreamNext` would then
/// exercise the Phase-0 tautological cached-reply consumption
/// path (removed as dead code by the ban commit) and mask real
/// divergences.  This pin makes the ban load-bearing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn entries_stream_open_rejects_consensus_with_fserr_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let runtime = create_runtime().await;

    let term = format!(
        r#"
        new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
          fsOpen!("{root}", "sub", "consensus", *o) |
          for (@reply <- o) {{
            @"result"!(reply)
          }}
        }}
        "#,
        root = dir.path().display(),
    );
    runtime
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            rand(),
        )
        .await
        .expect("evaluate Consensus entriesStreamOpen");

    // Capture the reply from @"result" and assert it's specifically
    // FSERR_UNSUPPORTED — not some other early-return error code.
    // A regression that swapped the ban for a BAD_ARG / IO / etc.
    // would still produce no WAL entry (and the assertion below
    // would still pass), so we need the code-slot check to lock the
    // specific FSERR down.
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;
    use rholang::rust::interpreter::io::errors::FSERR_UNSUPPORTED;
    use rholang::rust::interpreter::io::response::extract_err_code;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    let result_channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString("result".to_string())),
    }]);
    let datums = runtime.get_data(&result_channel).await;
    let reply_par = datums
        .first()
        .and_then(|d| d.a.pars.first())
        .cloned()
        .expect(
            "no reply on @\"result\" — the ban's early-return produce didn't \
             land, or the term shape changed",
        );
    let code = extract_err_code(std::slice::from_ref(&reply_par)).expect(
        "reply must be an [false, code, msg] error shape from the ban's \
         early-return; got a non-error reply",
    );
    assert_eq!(
        code, FSERR_UNSUPPORTED,
        "Consensus entriesStreamOpen rejection must use FSERR_UNSUPPORTED \
         specifically (see handlers.rs::fs_entries_stream_open ban comment). \
         A regression that returned FSERR_BAD_ARG or FSERR_IO would still \
         produce no WAL entry so the wal.is_empty() check below wouldn't \
         catch it — this assertion is the load-bearing pin for the \
         specific ban code.  Got code: {code}"
    );

    // No WAL entry should be journaled since the open was rejected
    // before any fd allocation.
    assert!(
        runtime.fs_handles.wal.is_empty(),
        "Consensus entriesStreamOpen rejection must NOT journal — the \
         leader errored out before any fd was created, so there's no \
         stream state to journal.  Got WAL: {:?}",
        runtime.fs_handles.wal.snapshot()
    );
}

/// Streaming-backing slice Step 3 (2026-08-25): Oracular
/// entriesStreamNext MUST NOT journal — same cross-cap isolation
/// invariant as fs_stat / fs_entries oracular pins.  A regression
/// that ignored the cap's cmode and journaled unconditionally
/// would surface Oracular reads in the Consensus WAL and diverge
/// across validators with different local fs state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn entries_stream_next_oracular_does_not_journal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/x"), b"x").unwrap();
    let runtime = create_runtime().await;

    let open_term = format!(
        r#"
        new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
          fsOpen!("{root}", "sub", "oracular", *o) |
          for (@[true, _fd] <- o) {{ Nil }}
        }}
        "#,
        root = dir.path().display(),
    );
    runtime
        .evaluate(
            &open_term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            rand(),
        )
        .await
        .expect("evaluate open");

    // 2 Next calls (1 yield + 1 EOS).
    for _ in 0..2 {
        let term = r#"
            new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {
              fsNext!(1, *r) |
              for (@_reply <- r) { Nil }
            }
            "#
        .to_string();
        runtime
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate next");
    }

    assert!(
        runtime.fs_handles.wal.is_empty(),
        "Oracular entriesStreamNext MUST NOT journal; got {} entries",
        runtime.fs_handles.wal.len()
    );
}
