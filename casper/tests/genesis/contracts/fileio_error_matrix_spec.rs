//! Phase 10 slice 10b — per-error-code integration coverage.
//!
//! Systematic single-file reference for the `FSERR_*` code surface a
//! user Rholang deploy can provoke.  Each test provokes exactly one
//! error path with a minimal repro and pins the exact (code, message)
//! reply tuple — so any drift in the code name OR the message text
//! fails a targeted regression, and someone reading the source can
//! grep for the code name to find the canonical trigger.
//!
//! ## Coverage matrix
//!
//! | FSERR code             | Test in this file                            | Layer          |
//! |------------------------|----------------------------------------------|----------------|
//! | `FSERR_BAD_ARG`        | readn_negative / seek_non_string_whence      | Rholang / File |
//! | `FSERR_BAD_ARG`        | openfile_non_map_options                     | Rholang / Fs   |
//! | `FSERR_BAD_ARG`        | openfile_on_dir_kind_returns_bad_arg         | Rholang / Fs   |
//! | `FSERR_UNSUPPORTED`    | openfile_unknown_bundle_name                 | Rholang / Fs   |
//! | `FSERR_UNSUPPORTED`    | chmod_on_read_only                           | Rholang / File |
//! | `FSERR_UNSUPPORTED`    | fs_default_arm_returns_unsupported           | Rholang / Fs   |
//! | `FSERR_CLOSED`         | readn_after_close                            | Rholang / File |
//! | `FSERR_CLOSED`         | writen_after_close_returns_closed            | Rholang / File |
//! | `FSERR_BUSY`           | lockrange_conflict                           | Native lock    |
//! | `FSERR_ALREADY_EXISTS` | openfile_exclusive_on_existing               | Native fs_open |
//! | `FSERR_QUOTA_EXCEEDED` | chunk_over_cap_returns_quota_exceeded        | Rholang/Stream |
//!
//! ## Deferred (not clean-testable from a fresh Rholang test)
//!
//! - `FSERR_NOT_FOUND`: fires on OS-level ENOENT, which requires racing
//!   bundle-setup vs. underlying-file `unlink(2)` between the harness
//!   and the Rholang runtime.  Doable with additional synchronization,
//!   not needed for the per-code reference surface.
//! - `FSERR_PERM`: needs a bundled file whose OS permissions are then
//!   stripped via `std::fs::set_permissions`.  Non-portable across CI
//!   platforms + requires teardown ordering.  Covered informally by
//!   the mock-syscall path in `rholang/tests/file_dir_check.rs`.
//! - `FSERR_IO`: fires on internal invariant breaks (fd params
//!   malformed, `spawn_blocking` join failure).  Not user-provocable
//!   from a well-typed deploy — regressions here fail the fs_generator
//!   and file_dir_check suites first.
//! - `FSERR_CANCELLED`: fires on `wait:true` lock acquisition
//!   cancellation.  Needs a concurrent-cancel harness; Phase 9
//!   territory (see plan §Phase 9 for the harness pickup).
//! - `FSERR_CROSS_DEVICE`: fires on `rename(2)`/`link(2)` across
//!   filesystems.  Requires a multi-mount test rig.
//! - `EOS` (end-of-stream sentinel from `Dir.entries()`): reachable
//!   only through the EntryStream materialisation path in Dir.rho.
//!   Deferred until the fs_entries_stream backing lands (currently a
//!   Phase 1 stub); see Dir.rho:117.  The stream builder's EOS arm
//!   is exercised transitively by `foldChunks` walks in the canonical
//!   examples once that native returns non-empty batches.
//! - `FSERR_QUARANTINE`: fires on symlink-in-path or path-escapes-root
//!   during canonicalisation.  Covered by unit tests in
//!   `rholang/src/rust/interpreter/io/path.rs::tests`.
//! - Additional `FSERR_QUOTA_EXCEEDED` sources: fires on WAL quota
//!   exhaustion; covered by `wal_cap_returns_fserr_quota_exceeded_from_rholang`
//!   in `rholang/tests/fs_wal_spec.rs`.  The Stream.chunk cap is
//!   pinned here (Phase 9 slice 9c-i addition) and additionally
//!   boundary-matrixed by `stream_chunk_max_items_cap_boundary` in
//!   `fileio_stream_argvalidation_spec.rs`.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Bundle a File "target" in `mode` over a tempfile pre-populated with
/// `content`.  Mirrors `bundle_file` from `fileio_file_spec.rs`; kept
/// local to this file so each spec can evolve its harness independently.
fn bundle_file(
    content: &[u8],
    mode: &str,
) -> (
    tempfile::TempDir,
    crate::util::genesis_builder::GenesisParameters,
    String,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("target.bin");
    std::fs::write(&file_path, content).expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        mode.to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    (dir, params, fs_uri)
}

/// FSERR_BAD_ARG: `File.readN(-1)` returns exactly
/// `[false, "FSERR_BAD_ARG", "n must be non-negative"]`.
/// Pinned as the canonical repro for the negative-integer BAD_ARG
/// path in `File.rho`'s readN method (§Non-normative extension).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn readn_negative_returns_bad_arg() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_readn_negative
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("readN(-1) returns FSERR_BAD_ARG with exact message",
        *test_readn_negative)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_readn_negative(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@r <- @file!?("readN", -1)) {{
          rhoSpec!("assert",
            (r, "==", [false, "FSERR_BAD_ARG", "n must be non-negative"]),
            "readN(-1) → FSERR_BAD_ARG", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixReadnNegativeSpec".to_string(),
    )
    .expect("compile readn_negative spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("readn_negative spec failed");
}

/// FSERR_BAD_ARG: `File.seek(0, 42)` (non-String whence) returns
/// exactly `[false, "FSERR_BAD_ARG", "whence must be a String"]`.
/// Pinned as the type-guard repro at File.rho:371 — the outer
/// `match whence` catches non-String values before dispatching to
/// the native seek.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seek_non_string_whence_returns_bad_arg() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_seek_bad_whence
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("seek(0, Int) rejects non-String whence with FSERR_BAD_ARG",
        *test_seek_bad_whence)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_seek_bad_whence(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@r <- @file!?("seek", 0, 42)) {{
          rhoSpec!("assert",
            (r, "==", [false, "FSERR_BAD_ARG", "whence must be a String"]),
            "seek(0, 42) → FSERR_BAD_ARG", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixSeekBadWhenceSpec".to_string(),
    )
    .expect("compile seek_bad_whence spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("seek_bad_whence spec failed");
}

/// FSERR_BAD_ARG: `Fs.openFile("target", "bogus")` (non-Map options)
/// returns exactly `[false, "FSERR_BAD_ARG", "options must be a Map"]`.
/// Pinned as the Fs-layer options-shape guard at Fs.rho:254-255 —
/// the `_ => modeCh!(Nil)` fall-through catches non-Map options and
/// the subsequent `Nil` arm fires BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_non_map_options_returns_bad_arg() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_bad_options
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile(name, non-Map) rejects with FSERR_BAD_ARG",
        *test_openfile_bad_options)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_openfile_bad_options(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("openFile", "target", "bogus")) {{
        rhoSpec!("assert",
          (r, "==", [false, "FSERR_BAD_ARG", "options must be a Map"]),
          "openFile(target, \"bogus\") → FSERR_BAD_ARG", *ackCh)
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixOpenfileBadOptionsSpec".to_string(),
    )
    .expect("compile openfile_bad_options spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile_bad_options spec failed");
}

/// FSERR_UNSUPPORTED: `Fs.openFile("no-such-name", {})` returns
/// exactly `[false, "FSERR_UNSUPPORTED", "logical name not in static bundle"]`.
/// Pinned as the bundle-miss repro at Fs.rho:259-261 — the bundle
/// map's `contains(n) == false` arm.  This is distinct from OS-level
/// ENOENT (which would map to FSERR_NOT_FOUND); the bundle-miss check
/// fires in Rholang before any native path is attempted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_unknown_bundle_name_returns_unsupported() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_unknown
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile(unknown-bundle-name, {{}}) → FSERR_UNSUPPORTED",
        *test_openfile_unknown)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_openfile_unknown(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("openFile", "no-such-name", {{}})) {{
        rhoSpec!("assert",
          (r, "==",
           [false, "FSERR_UNSUPPORTED", "logical name not in static bundle"]),
          "openFile(no-such-name, {{}}) → FSERR_UNSUPPORTED", *ackCh)
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixOpenfileUnknownSpec".to_string(),
    )
    .expect("compile openfile_unknown spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile_unknown spec failed");
}

/// FSERR_UNSUPPORTED: `File.chmod("...")` on an r-mode cap returns
/// exactly `[false, "FSERR_UNSUPPORTED", "chmod requires a write-capable mode"]`.
/// Pinned as the write-capable-mode gate repro at File.rho:652 —
/// mutation methods on r-mode caps short-circuit with UNSUPPORTED
/// before any native dispatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chmod_on_read_only_returns_unsupported() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_chmod_read_only
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("chmod on r-mode file → FSERR_UNSUPPORTED",
        *test_chmod_read_only)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_chmod_read_only(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@r <- @file!?("chmod", "rw-r--r--")) {{
          rhoSpec!("assert",
            (r, "==",
             [false, "FSERR_UNSUPPORTED", "chmod requires a write-capable mode"]),
            "chmod on r-mode → FSERR_UNSUPPORTED", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixChmodReadOnlySpec".to_string(),
    )
    .expect("compile chmod_read_only spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("chmod_read_only spec failed");
}

/// FSERR_CLOSED: `File.readN(4)` after `close()` returns exactly
/// `[false, "FSERR_CLOSED", "file is closed"]`.  Pinned as the
/// state-gate repro (File.rho:295-298) — every File method reads
/// the `stateP` cell first and short-circuits on "closed" without
/// touching the fd.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn readn_after_close_returns_closed() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_readn_after_close
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("readN after close → FSERR_CLOSED",
        *test_readn_after_close)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_readn_after_close(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true] <- @file!?("close")) {{
          for(@r <- @file!?("readN", 4)) {{
            rhoSpec!("assert",
              (r, "==", [false, "FSERR_CLOSED", "file is closed"]),
              "readN after close → FSERR_CLOSED", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixReadnAfterCloseSpec".to_string(),
    )
    .expect("compile readn_after_close spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("readn_after_close spec failed");
}

/// FSERR_BUSY: a second `lockRange(0, 100, "w")` on the same file
/// via a distinct File cap (fresh mint) returns exactly
/// `[false, "FSERR_BUSY", "range lock unavailable"]` while the first
/// cap still holds the lock.  Pinned as the cross-cap lock-conflict
/// repro (native `fs_lock_range` → `lock_err_reply(LockError::Busy)`
/// at handlers.rs:2859).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lockrange_conflict_returns_busy() {
    // 512 bytes so the [0, 100) range is well within EOF.
    let (_dir, params, fs_uri) = bundle_file(&vec![0u8; 512], "rw");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_lockrange_busy
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("cross-cap lockRange conflict → FSERR_BUSY",
        *test_lockrange_busy)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_lockrange_busy(rhoSpec, _, ackCh) = {{
      for(@[true, cap1] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
        for(@[true, cap2] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
          for(@[true, _tok1] <- @cap1!?("lockRange", 0, 100, "w")) {{
            for(@r <- @cap2!?("lockRange", 0, 100, "w")) {{
              rhoSpec!("assert",
                (r, "==",
                 [false, "FSERR_BUSY", "range lock unavailable"]),
                "cross-cap lockRange conflict → FSERR_BUSY", *ackCh)
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixLockrangeBusySpec".to_string(),
    )
    .expect("compile lockrange_busy spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("lockrange_busy spec failed");
}

/// FSERR_ALREADY_EXISTS: `Fs.openFile("target", {"mode":"wx"})` on a
/// bundle whose backing file already exists on disk returns exactly
/// `[false, "FSERR_ALREADY_EXISTS", <native msg>]`.  Pinned as the
/// `O_CREAT | O_EXCL` collision repro through the Fs → openFileImpl →
/// fs_open chain.  Bundle provisioning always seeds an on-disk file
/// (H-P7-8), so this path fires deterministically.  Prior docstring
/// deferral ("blocked on Dir-mutations slice") was over-conservative:
/// the code is directly reachable through Fs.openFile with an
/// exclusive-create mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_exclusive_on_existing_returns_already_exists() {
    // "rw" provisioning is required for the Fs mode-attenuation gate
    // to admit "wx"; the underlying file is seeded by bundle_file.
    let (_dir, params, fs_uri) = bundle_file(b"payload", "rw");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_exclusive
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile(target, {{mode:wx}}) on existing → FSERR_ALREADY_EXISTS",
        *test_openfile_exclusive)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_openfile_exclusive(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("openFile", "target", {{"mode": "wx"}})) {{
        match r {{
          [false, "FSERR_ALREADY_EXISTS", _] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile(wx) on existing → FSERR_ALREADY_EXISTS", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert",
              (r, "==", "[false, FSERR_ALREADY_EXISTS, _]"),
              "openFile(wx) on existing → FSERR_ALREADY_EXISTS", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixOpenfileExclusiveSpec".to_string(),
    )
    .expect("compile openfile_exclusive spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile_exclusive spec failed");
}

/// FSERR_QUOTA_EXCEEDED: `Stream.chunk(65537)` on a ByteStream returns
/// exactly `[false, "FSERR_QUOTA_EXCEEDED",
/// "chunk n exceeds MAX_CHUNK_ITEMS=65536"]`.  Pinned as the canonical
/// per-error-code reference for the Phase 9 slice 9c-i cap (added on
/// `Stream.rho::method chunk(@n)` — see MAX_CHUNK_ITEMS).  A separate
/// three-arm boundary matrix (65536 / 65537 / 1_000_000) lives in
/// `fileio_stream_argvalidation_spec.rs::stream_chunk_max_items_cap_boundary`;
/// this test is intentionally single-value to keep the code-to-repro
/// index dense.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunk_over_cap_returns_quota_exceeded() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_chunk_over_cap
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Stream.chunk(65537) → FSERR_QUOTA_EXCEEDED",
        *test_chunk_over_cap)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_chunk_over_cap(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@r <- @byteStream!?("chunk", 65537)) {{
            rhoSpec!("assert",
              (r, "==",
               [false, "FSERR_QUOTA_EXCEEDED",
                "chunk n exceeds MAX_CHUNK_ITEMS=65536"]),
              "chunk(65537) → FSERR_QUOTA_EXCEEDED", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixChunkOverCapSpec".to_string(),
    )
    .expect("compile chunk_over_cap spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("chunk_over_cap spec failed");
}

/// FSERR_UNSUPPORTED: `Fs.default(...)` — any unknown Fs method
/// returns exactly
/// `[false, "FSERR_UNSUPPORTED", "unknown method or not implemented in this slice"]`.
/// Pinned as the Fs-level default-arm repro (Fs.rho:387-391); if this
/// arm drops or renames the code/message, a caller that guards on
/// FSERR_UNSUPPORTED for feature detection would silently break.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_default_arm_returns_unsupported() {
    let (_dir, params, fs_uri) = bundle_file(b"payload", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_fs_default
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Fs.default arm returns FSERR_UNSUPPORTED",
        *test_fs_default)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_fs_default(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("noSuchMethod", 1, 2, 3)) {{
        rhoSpec!("assert",
          (r, "==",
           [false, "FSERR_UNSUPPORTED",
            "unknown method or not implemented in this slice"]),
          "Fs unknown-method → FSERR_UNSUPPORTED", *ackCh)
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixFsDefaultSpec".to_string(),
    )
    .expect("compile fs_default spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("fs_default spec failed");
}

/// FSERR_CLOSED: `File.writeByteArray(bytes)` after `close()` returns
/// exactly `[false, "FSERR_CLOSED", "file is closed"]`.  Complements
/// `readn_after_close_returns_closed` — the closed-state gate is
/// duplicated across every File method (File.rho grep shows ~20
/// sites), so pinning both a read and a write path guards against a
/// partial refactor that skips the gate on mutators.  The write path
/// is more consequential because a post-close write on a stale fd
/// could touch a distinct file if the fd was recycled by the runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writen_after_close_returns_closed() {
    // Bundle in "rw" so writeByteArray isn't gated by mode before it
    // reaches the closed-state check.
    let (_dir, params, fs_uri) = bundle_file(b"payload", "rw");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_writen_after_close
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("writeByteArray after close → FSERR_CLOSED",
        *test_writen_after_close)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_writen_after_close(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
        for(@[true] <- @file!?("close")) {{
          for(@r <- @file!?("writeByteArray", "AA".hexToBytes())) {{
            rhoSpec!("assert",
              (r, "==", [false, "FSERR_CLOSED", "file is closed"]),
              "writeByteArray after close → FSERR_CLOSED", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixWriteNAfterCloseSpec".to_string(),
    )
    .expect("compile writen_after_close spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("writen_after_close spec failed");
}

/// FSERR_BAD_ARG: `Fs.openFile("shareddir", {})` where "shareddir"
/// resolves to a Dir bundle entry (kind "dir") returns exactly
/// `[false, "FSERR_BAD_ARG", "logical name is a directory; use openDir"]`.
/// Pinned as the cross-kind gate at Fs.rho:299-301 — a caller that
/// requests a file operation on a directory-bundled name is rejected
/// at the Rholang layer before any native dispatch, preserving the
/// bundle-kind invariant.  Symmetric to the openDir-on-file arm
/// (Fs.rho:364-366; not yet pinned).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_on_dir_kind_returns_bad_arg() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    // Seed one file so the bundled dir is non-empty; not strictly
    // required for this test (the gate fires before the syscall), but
    // matches the on-disk shape used elsewhere.
    std::fs::write(root.join("child.txt"), b"unused").expect("seed child");

    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root,
        BundleEntryKind::Dir,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle dir entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_on_dir_kind
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile(dir-kind-name, {{}}) → FSERR_BAD_ARG",
        *test_openfile_on_dir_kind)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_openfile_on_dir_kind(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("openFile", "shareddir", {{}})) {{
        rhoSpec!("assert",
          (r, "==",
           [false, "FSERR_BAD_ARG",
            "logical name is a directory; use openDir"]),
          "openFile on dir bundle → FSERR_BAD_ARG", *ackCh)
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ErrMatrixOpenfileOnDirKindSpec".to_string(),
    )
    .expect("compile openfile_on_dir_kind spec");

    // Keep tempdir alive for the duration of the test.
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile_on_dir_kind spec failed");
    drop(dir);
}
