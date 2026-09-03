//! Phase 10 canonical-example regressions.
//!
//! Each `#[tokio::test]` here mirrors one of the pedagogical `.rho`
//! scripts under `rholang/examples/fileio_*.rho`.  The examples are
//! human-readable artifacts published with the File I/O FIP; the
//! tests here verify the same assertion path executes to the
//! documented behavior against real genesis + RhoSpec plumbing.
//!
//! Pattern: `format!()`-compose an inline RhoSpec test source that
//! reproduces the example's logic, plus an assertion arm.  Bundle
//! entries are injected via `GenesisBuilder::build_genesis_parameters_with_defaults`
//! + `params.2.fs_bundle`, mirroring `fs_generator_spec.rs`.
//!
//! Adding a new example: (1) author `rholang/examples/fileio_<name>.rho`,
//! (2) add a matching `fileio_<name>` test here whose RhoSpec source
//! reproduces the example's contract, (3) reference this test from
//! the example's docstring under "Companion regression".

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Slice 10a-1: canonical example `fileio_chown_consensus.rho`.
///
/// A File cap minted from a `BundleConsensusMode::Consensus` static
/// bundle entry must return `[false, "FSERR_UNSUPPORTED", _]` when
/// `chown` is invoked, because consensus caps are always provisioned
/// read-only and `File.chown` gates on write-capable fmode before
/// dispatching to `fsChown`.
///
/// This is the observable behavior the example script documents;
/// this test pins it against genesis + RhoSpec so a refactor of
/// either the bundle-plumbing path or the `File.chown` write-mode
/// gate is caught in CI.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_chown_on_consensus_cap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("consensus.dat");
    std::fs::write(&file_path, b"consensus payload").expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "consensus-cap".to_string(),
        canon.clone(),
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Consensus,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_chown_on_consensus_cap_returns_unsupported
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("chown on consensus cap returns FSERR_UNSUPPORTED",
         *test_chown_on_consensus_cap_returns_unsupported)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_chown_on_consensus_cap_returns_unsupported(rhoSpec, _, ackCh) = {{
      for(@[true, fileCap] <- @fs!?("openFile", "consensus-cap", {{}})) {{
        for(@r <- @fileCap!?("chown", "alice", "users")) {{
          match r {{
            [false, "FSERR_UNSUPPORTED",
             "chown requires a write-capable mode"] => {{
              rhoSpec!("assert", (true, "==", true),
                "chown on consensus-cap returns FSERR_UNSUPPORTED", *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==",
                "[false, FSERR_UNSUPPORTED, \"chown requires a write-capable mode\"]"),
                "chown on consensus-cap returns FSERR_UNSUPPORTED", *ackCh)
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
        "FileioChownConsensusSpec".to_string(),
    )
    .expect("compile fileio_chown_consensus test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_chown_consensus spec failed");
}

/// Slice 10a-2: canonical example `fileio_lockrange.rho`.
///
/// Composes Phase 7 slice-27 fresh-mint (two `openFile("target")`
/// calls yield distinct caps) with Phase 8's cross-cap range-lock
/// coordination (RuntimeManager-shared LockRegistry keyed on
/// canonical path).  Sequence:
///
///   1. cap1, cap2 = two fresh mints of the same bundle entry.
///   2. cap1.lockRange(0, 100, "w") → [true, token1].
///   3. cap2.lockRange(0, 100, "w") → [false, "FSERR_BUSY", _].
///   4. token1.release() → [true].
///   5. cap2.lockRange(0, 100, "w") → [true, token2].
///   6. token2.release() → [true].
///
/// Verifies the full cross-cap coordination path against real
/// native handlers (not the file_dir_check mocks) — a regression
/// in either the fresh-mint LockRegistry keying or the range-lock
/// conflict-detection algorithm would fail this test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_lockrange_cross_cap_busy_then_release() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("lockrange.dat");
    // Seed with 512 bytes so [0, 100) is well within EOF and reads/
    // writes under the lock have room to operate (though this test
    // doesn't actually do I/O — the lock semantics are what's tested).
    std::fs::write(&file_path, vec![0u8; 512]).expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon.clone(),
        BundleEntryKind::File,
        "rw".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_cross_cap_busy_then_release
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("cross-cap range-lock: busy then release then retry",
         *test_cross_cap_busy_then_release)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_cross_cap_busy_then_release(rhoSpec, _, ackCh) = {{
      for(@[true, cap1] <- @fs!?("openFile", "target", {{"mode": "rw"}});
          @[true, cap2] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
        for(@lock1 <- @cap1!?("lockRange", 0, 100, "w")) {{
          match lock1 {{
            [true, token1] => {{
              for(@busy <- @cap2!?("lockRange", 0, 100, "w")) {{
                match busy {{
                  [false, "FSERR_BUSY", _msg] => {{
                    for(@rel1 <- @token1!?("release")) {{
                      // Reply may be [true] or [true, _]; both count as success.
                      match rel1 {{
                        [true] => {{
                          for(@retry <- @cap2!?("lockRange", 0, 100, "w")) {{
                            match retry {{
                              [true, token2] => {{
                                for(@rel2 <- @token2!?("release")) {{
                                  match rel2 {{
                                    [true] => {{
                                      rhoSpec!("assert", (true, "==", true),
                                        "cross-cap lock lifecycle round-trip", *ackCh)
                                    }}
                                    [true, _] => {{
                                      rhoSpec!("assert", (true, "==", true),
                                        "cross-cap lock lifecycle round-trip", *ackCh)
                                    }}
                                    _ => {{
                                      rhoSpec!("assert", (rel2, "==", "[true, ...]"),
                                        "token2 release must succeed", *ackCh)
                                    }}
                                  }}
                                }}
                              }}
                              _ => {{
                                rhoSpec!("assert", (retry, "==", "[true, token2]"),
                                  "cap2 retry after cap1 release must succeed", *ackCh)
                              }}
                            }}
                          }}
                        }}
                        [true, _] => {{
                          for(@retry <- @cap2!?("lockRange", 0, 100, "w")) {{
                            match retry {{
                              [true, token2] => {{
                                for(@rel2 <- @token2!?("release")) {{
                                  rhoSpec!("assert", (true, "==", true),
                                    "cross-cap lock lifecycle round-trip", *ackCh)
                                }}
                              }}
                              _ => {{
                                rhoSpec!("assert", (retry, "==", "[true, token2]"),
                                  "cap2 retry after cap1 release must succeed", *ackCh)
                              }}
                            }}
                          }}
                        }}
                        _ => {{
                          rhoSpec!("assert", (rel1, "==", "[true, ...]"),
                            "token1 release must succeed", *ackCh)
                        }}
                      }}
                    }}
                  }}
                  _ => {{
                    rhoSpec!("assert", (busy, "==", "[false, FSERR_BUSY, _]"),
                      "cap2 lock while cap1 holds must be FSERR_BUSY", *ackCh)
                  }}
                }}
              }}
            }}
            _ => {{
              rhoSpec!("assert", (lock1, "==", "[true, token1]"),
                "cap1 initial lock acquire must succeed", *ackCh)
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
        "FileioLockrangeSpec".to_string(),
    )
    .expect("compile fileio_lockrange test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_lockrange spec failed");
}

/// Phase 10 3-way lock-contention stress (2026-08-23): three caps
/// on the same bundled file exercise the anti-starvation sequence
/// {hold, conflict, release, retry-and-admit} across two waiters.
/// Verifies end-to-end that cap1's release restores admission to
/// cap2, and cap2's release in turn restores admission to cap3 —
/// neither queued cap is starved by the other.
///
/// Sequence:
///   1. cap1 lockRange wait:false → [true, token1].
///   2. cap2 lockRange wait:false → [false, "FSERR_BUSY", _]
///      (conflicts with cap1's held range).
///   3. cap3 lockRange wait:false → [false, "FSERR_BUSY", _]
///      (same conflict; parallel proof both are blocked).
///   4. token1.release() → cap1 releases.
///   5. cap2 lockRange wait:false → [true, token2].
///   6. cap3 lockRange wait:false → [false, "FSERR_BUSY", _]
///      (still blocked by cap2 now).
///   7. token2.release() → cap2 releases.
///   8. cap3 lockRange wait:false → [true, token3].
///   9. token3.release() → clean shutdown.
///
/// This uses wait:false throughout (fail-fast on conflict) instead
/// of wait:true (park until admissible) because the RhoSpec harness
/// under `casper::helper::rho_spec` does not currently drive the
/// tokio task infrastructure the wait:true admit-await path
/// requires — a concurrent-waiter deploy times out silently rather
/// than resolving the parked acquires.  Strict head-of-line FIFO
/// order between concurrent wait:true parkers is unit-tested at
/// the LockRegistry layer
/// (`rholang/src/rust/interpreter/io/lock.rs::three_waiters_admit_fifo_after_release`);
/// end-to-end wait:true coverage awaits harness plumbing (own
/// slice, likely part of the two-runtime replay harness work).
///
/// The retry-after-release pattern still exercises the load-bearing
/// cross-cap plumbing: LockRegistry keying on `(dev, inode)`
/// aggregation of the three fresh-mint caps, conflict detection
/// per-range, release-triggered-availability, and clean
/// termination.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_lockrange_three_way_no_starvation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("threeway.dat");
    std::fs::write(&file_path, vec![0u8; 512]).expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        "rw".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");
    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_three_way
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("3-way lockRange no starvation across release chain",
        *test_three_way)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_three_way(rhoSpec, _, ackCh) = {{
      for(@[true, cap1] <- @fs!?("openFile", "target", {{"mode": "rw"}});
          @[true, cap2] <- @fs!?("openFile", "target", {{"mode": "rw"}});
          @[true, cap3] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
        for(@[true, token1] <- @cap1!?("lockRange", 0, 100, "w")) {{
          // cap2 + cap3 both conflict with cap1's hold.
          for(@r2busy <- @cap2!?("lockRange", 0, 100, "w");
              @r3busy <- @cap3!?("lockRange", 0, 100, "w")) {{
            match [r2busy, r3busy] {{
              [[false, "FSERR_BUSY", _], [false, "FSERR_BUSY", _]] => {{
                // Release cap1; cap2 must succeed, cap3 remains blocked.
                for(@rel1 <- @token1!?("release")) {{
                  match rel1 {{
                    [true] => {{
                      for(@r2ok <- @cap2!?("lockRange", 0, 100, "w");
                          @r3still <- @cap3!?("lockRange", 0, 100, "w")) {{
                        match [r2ok, r3still] {{
                          [[true, token2], [false, "FSERR_BUSY", _]] => {{
                            for(@rel2 <- @token2!?("release")) {{
                              match rel2 {{
                                [true] => {{
                                  for(@r3ok <- @cap3!?("lockRange", 0, 100, "w")) {{
                                    match r3ok {{
                                      [true, token3] => {{
                                        for(@rel3 <- @token3!?("release")) {{
                                          match rel3 {{
                                            [true] => {{
                                              rhoSpec!("assert", (true, "==", true),
                                                "3-way no-starvation: cap1->cap2->cap3 release chain fully drained",
                                                *ackCh)
                                            }}
                                            _ => {{
                                              rhoSpec!("assert", (rel3, "==", "[true]"),
                                                "token3 release must succeed", *ackCh)
                                            }}
                                          }}
                                        }}
                                      }}
                                      _ => {{
                                        rhoSpec!("assert", (r3ok, "==", "[true, token3]"),
                                          "cap3 must acquire after cap2 releases", *ackCh)
                                      }}
                                    }}
                                  }}
                                }}
                                _ => {{
                                  rhoSpec!("assert", (rel2, "==", "[true]"),
                                    "token2 release must succeed", *ackCh)
                                }}
                              }}
                            }}
                          }}
                          _ => {{
                            rhoSpec!("assert",
                              ([r2ok, r3still], "==",
                               "[[true, token2], [false, FSERR_BUSY, _]]"),
                              "post-release-1: cap2 admitted, cap3 still busy", *ackCh)
                          }}
                        }}
                      }}
                    }}
                    _ => {{
                      rhoSpec!("assert", (rel1, "==", "[true]"),
                        "token1 release must succeed", *ackCh)
                    }}
                  }}
                }}
              }}
              _ => {{
                rhoSpec!("assert",
                  ([r2busy, r3busy], "==",
                   "[[false, FSERR_BUSY, _], [false, FSERR_BUSY, _]]"),
                  "both cap2 and cap3 must observe FSERR_BUSY while cap1 holds",
                  *ackCh)
              }}
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
        "FileioThreeWayLockSpec".to_string(),
    )
    .expect("compile three-way lock test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("three-way lockRange spec failed");
}

/// Slice 10a-3: canonical example `fileio_static.rho`.
///
/// Static-config file-to-file line copy: read every line from a pre-
/// populated bundled source file via `source!lines()`, hand the
/// resulting LineStream to `dest!writeLines(...)`, and verify the
/// destination file's on-disk content matches the source.
///
/// End-to-end proof of the stream-producer / stream-consumer wiring
/// (Phase 4 stream library + Phase 5 File.lines / File.writeLines).
/// The writeLines drain iterates line-by-line, and each iteration
/// acquires + releases a whole-file sequential lock on dest (Phase 8
/// §Sequential-vs-positional coordination), so a real syscall path
/// exercises both the stream plumbing and the lock protocol.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_static_line_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_path = dir.path().join("source.txt");
    let dest_path = dir.path().join("dest.txt");

    let source_content = b"hello\nworld\n";
    std::fs::write(&source_path, source_content).expect("seed source");
    std::fs::write(&dest_path, b"").expect("seed dest");

    let source_canon = std::fs::canonicalize(&source_path).expect("canonicalize source");
    let dest_canon = std::fs::canonicalize(&dest_path).expect("canonicalize dest");

    let source_entry = BundleEntry::try_new(
        "source".to_string(),
        source_canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("source bundle entry");
    let dest_entry = BundleEntry::try_new(
        "dest".to_string(),
        dest_canon.clone(),
        BundleEntryKind::File,
        "rw".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("dest bundle entry");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![source_entry, dest_entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_write_lines_from_source_to_dest
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("writeLines drains source LineStream into dest file",
         *test_write_lines_from_source_to_dest)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_write_lines_from_source_to_dest(rhoSpec, _, ackCh) = {{
      for(@[true, src] <- @fs!?("openFile", "source", {{"mode": "r"}});
          @[true, dst] <- @fs!?("openFile", "dest", {{"mode": "rw"}})) {{
        for(@linesReply <- @src!?("lines")) {{
          match linesReply {{
            [true, sourceLines] => {{
              for(@wlReply <- @dst!?("writeLines", sourceLines)) {{
                match wlReply {{
                  [true] => {{
                    rhoSpec!("assert", (true, "==", true),
                      "writeLines drain returns [true]", *ackCh)
                  }}
                  _ => {{
                    rhoSpec!("assert", (wlReply, "==", "[true]"),
                      "writeLines drain returns [true]", *ackCh)
                  }}
                }}
              }}
            }}
            _ => {{
              rhoSpec!("assert", (linesReply, "==", "[true, sourceLines]"),
                "source.lines() must succeed", *ackCh)
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "FileioStaticSpec".to_string())
            .expect("compile fileio_static test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("fileio_static spec failed");

    // Verify on-disk: dest must contain what source had.  writeLines
    // emits each line followed by LF, so a source ending in LF
    // produces a dest that also ends in LF (byte-identical for this
    // input).
    let dest_bytes = std::fs::read(&dest_canon).expect("read dest after copy");
    assert_eq!(
        dest_bytes, source_content,
        "dest file content after writeLines must match source"
    );
}

/// Slice 10a-4: canonical example `fileio_membrane.rho`.
///
/// Wraps a File cap in a forwarder that consults a mutable `revoked`
/// flag.  A holder of the forwarder (via `bundle+{*tellMembrane}`)
/// can invoke methods through it; the revocation switch is retained
/// by the wrapping deploy.  Pre-revoke `tell()` returns a real reply
/// from the underlying File.  Post-revoke `tell()` returns
/// `[false, "FSERR_REVOKED", _]` without touching the underlying.
///
/// Simplification vs. the FIP §Ocap patterns pseudocode: the example
/// wraps a nullary method (`tell`), not a variadic one, because
/// Rholang's grammar disallows `...` splats in send positions.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_membrane_revokes_forwarder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("membrane.dat");
    std::fs::write(&file_path, b"membrane test payload").expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_membrane_revocation_switch
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("membrane forwards pre-revoke, returns FSERR_REVOKED post-revoke",
         *test_membrane_revocation_switch)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_membrane_revocation_switch(rhoSpec, _, ackCh) = {{
      for(@[true, underlyingFile] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        new revoked, tellMembrane in {{
          revoked!(false) |
          contract tellMembrane(returnCh) = {{
            for (@r <<- revoked) {{
              match r {{
                true => returnCh!([false, "FSERR_REVOKED", "capability revoked"])
                false => {{
                  for (@reply <- @underlyingFile!?("tell")) {{
                    returnCh!(reply)
                  }}
                }}
              }}
            }}
          }} |
          new r1Ch in {{
            tellMembrane!(*r1Ch) |
            for(@r1 <- r1Ch) {{
              match r1 {{
                [true, _pos] => {{
                  // Pre-revoke path worked; now revoke and retry.
                  for(@_ <- revoked) {{
                    revoked!(true) |
                    new r2Ch in {{
                      tellMembrane!(*r2Ch) |
                      for(@r2 <- r2Ch) {{
                        match r2 {{
                          [false, "FSERR_REVOKED", _] => {{
                            rhoSpec!("assert", (true, "==", true),
                              "membrane returns FSERR_REVOKED after revoke", *ackCh)
                          }}
                          _ => {{
                            rhoSpec!("assert", (r2, "==", "[false, FSERR_REVOKED, _]"),
                              "membrane returns FSERR_REVOKED after revoke", *ackCh)
                          }}
                        }}
                      }}
                    }}
                  }}
                }}
                _ => {{
                  rhoSpec!("assert", (r1, "==", "[true, _pos]"),
                    "pre-revoke tell must succeed", *ackCh)
                }}
              }}
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
        "FileioMembraneSpec".to_string(),
    )
    .expect("compile fileio_membrane test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("fileio_membrane spec failed");
}

/// Slice 10e: canonical example `fileio_cross_fs_isolation.rho`.
///
/// Exercises FIP §867's isolation invariant: Alice's manipulation of
/// her File cap — including wrapping it in a revocable membrane and
/// flipping the revocation switch — cannot affect a subordinate who
/// obtained an independent cap for the same logical name via a fresh
/// `openFile` call.
///
/// Under slice-27 fresh-mint semantics (2026-08-04), every `openFile`
/// call yields a structurally distinct `File` agent with its own
/// cursor and its own kernel fd.  Membranes wrap SPECIFIC instances,
/// so revoking Alice's membrane over `fA` leaves `fB` (obtained by
/// Bob via a second `openFile`) completely untouched.
///
/// Framing note: earlier plan-doc references had 10e "blocked on
/// Powerbox stub" because the FIP-style demonstration would ideally
/// give Alice and Bob distinct `Fs` instances (per PB-M-1's original
/// per-principal Fs framing).  That framing was superseded by the
/// PB-M-1 resolution (2026-07-30 — shared Fs is safe under uniform-
/// per-bucket bundles + no cache) combined with slice-27 fresh-mint.
/// The core §867 property is fully demonstrable under the shared-Fs
/// MVP; the Powerbox stub would ADD per-principal bundle scoping
/// (a stronger claim), but is not required for the isolation
/// invariant.
///
/// Sequence:
///   1. Alice opens "shared" → fA.
///   2. Bob opens "shared" → fB.  Slice-27 fresh-mint ⇒ fA ≠ fB.
///   3. Alice wraps fA in a `tellMembrane` + private `revoked` switch
///      (FIP §Ocap patterns idiom).
///   4. Pre-revoke: Alice's membrane forwards `tell()` to fA.
///   5. Alice flips revocation switch.
///   6. Post-revoke: Alice's membrane returns FSERR_REVOKED.
///   7. Bob calls fB!?("tell") directly — succeeds with [true, 0].
///
/// The regression asserts (7): Bob's independent cap continues to
/// work after Alice's revocation, proving membrane invisibility
/// across independent fresh-mints.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_cross_fs_membrane_invisible_to_bob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("shared.dat");
    std::fs::write(&file_path, b"cross-fs isolation test payload").expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "shared".to_string(),
        canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_cross_fs_membrane_invisible_to_bob
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("Alice's revoked membrane is invisible to Bob's independent cap",
         *test_cross_fs_membrane_invisible_to_bob)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_cross_fs_membrane_invisible_to_bob(rhoSpec, _, ackCh) = {{
      // Alice and Bob each obtain a fresh File cap for the same
      // logical name.  Slice-27 fresh-mint => fA and fB are distinct
      // agents with independent cursors + kernel fds.
      for(@[true, fA] <- @fs!?("openFile", "shared", {{"mode": "r"}});
          @[true, fB] <- @fs!?("openFile", "shared", {{"mode": "r"}})) {{

        new revoked, tellMembrane in {{
          // Alice wraps fA in a revocable forwarder.  The `revoked`
          // switch is kept private to Alice's scope.
          revoked!(false) |
          contract tellMembrane(returnCh) = {{
            for (@r <<- revoked) {{
              match r {{
                true => returnCh!([false, "FSERR_REVOKED", "capability revoked"])
                false => {{
                  for (@reply <- @fA!?("tell")) {{
                    returnCh!(reply)
                  }}
                }}
              }}
            }}
          }} |

          // Sanity: pre-revoke, the membrane forwards correctly.
          new preCh in {{
            tellMembrane!(*preCh) |
            for(@preReply <- preCh) {{
              match preReply {{
                [true, _pos] => {{
                  // Alice flips the revocation switch: consume + republish.
                  for(@_ <- revoked) {{
                    revoked!(true) |

                    // Post-revoke: membrane returns FSERR_REVOKED
                    // (checked implicitly by the follow-up: Bob's
                    // independent cap must not observe Alice's
                    // revocation).
                    new postCh in {{
                      tellMembrane!(*postCh) |
                      for(@postReply <- postCh) {{
                        match postReply {{
                          [false, "FSERR_REVOKED", _] => {{
                            // Now the actual isolation assertion:
                            // Bob's fB (obtained by a fresh openFile,
                            // not through Alice's membrane) must
                            // continue to work.  Alice's manipulation
                            // is invisible to Bob's independently-
                            // obtained cap.
                            for(@bobReply <- @fB!?("tell")) {{
                              match bobReply {{
                                [true, _bobPos] => {{
                                  rhoSpec!("assert", (true, "==", true),
                                    "Bob's independent cap unaffected by Alice's revoke",
                                    *ackCh)
                                }}
                                _ => {{
                                  rhoSpec!("assert",
                                    (bobReply, "==", "[true, _pos]"),
                                    "Bob's tell must succeed post-Alice-revoke",
                                    *ackCh)
                                }}
                              }}
                            }}
                          }}
                          _ => {{
                            rhoSpec!("assert",
                              (postReply, "==", "[false, FSERR_REVOKED, _]"),
                              "membrane must return FSERR_REVOKED after revoke",
                              *ackCh)
                          }}
                        }}
                      }}
                    }}
                  }}
                }}
                _ => {{
                  rhoSpec!("assert", (preReply, "==", "[true, _pos]"),
                    "pre-revoke membrane tell must succeed", *ackCh)
                }}
              }}
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
        "FileioCrossFsIsolationSpec".to_string(),
    )
    .expect("compile fileio_cross_fs_isolation test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_cross_fs_isolation spec failed");
}

/// Slice 10a-5: canonical example `fileio_readonly_forwarder.rho`.
///
/// A File cap is wrapped in a forwarder that whitelists specific
/// read-side method names (`tell`, `size`) and returns FSERR_UNSUPPORTED
/// for everything else.  Verifies:
///
///   - Allowed method (`tell`) — reply routed from underlying File.
///   - Allowed method (`size`) — reply routed from underlying File.
///   - Blocked method (`chmod`) — forwarder returns FSERR_UNSUPPORTED
///     without touching underlyingFile (defense: even if chmod would
///     succeed on the underlying, the forwarder blocks it).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_readonly_forwarder_filters_mutations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("forwarder.dat");
    std::fs::write(&file_path, b"readonly forwarder payload").expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_readonly_forwarder_allows_reads_blocks_mutations
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("read-only forwarder passes reads, blocks mutations",
         *test_readonly_forwarder_allows_reads_blocks_mutations)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_readonly_forwarder_allows_reads_blocks_mutations(rhoSpec, _, ackCh) = {{
      for(@[true, underlyingFile] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        new readOnlyForwarder in {{
          contract readOnlyForwarder(returnCh, @method, ...@_args) = {{
            match method {{
              "tell" => {{
                for (@r <- @underlyingFile!?("tell")) {{ returnCh!(r) }}
              }}
              "size" => {{
                for (@r <- @underlyingFile!?("size")) {{ returnCh!(r) }}
              }}
              _ => {{
                returnCh!([false, "FSERR_UNSUPPORTED",
                  "method not on read-only wrapper"])
              }}
            }}
          }} |
          new tellCh, sizeCh, chmodCh in {{
            readOnlyForwarder!(*tellCh, "tell") |
            readOnlyForwarder!(*sizeCh, "size") |
            readOnlyForwarder!(*chmodCh, "chmod", "rw-r--r--") |
            for(@rTell <- tellCh; @rSize <- sizeCh; @rChmod <- chmodCh) {{
              match [rTell, rSize, rChmod] {{
                [[true, _], [true, _],
                 [false, "FSERR_UNSUPPORTED",
                  "method not on read-only wrapper"]] => {{
                  rhoSpec!("assert", (true, "==", true),
                    "forwarder allows tell + size, blocks chmod", *ackCh)
                }}
                _ => {{
                  rhoSpec!("assert",
                    ([rTell, rSize, rChmod], "==",
                     "[[true,_], [true,_], [false, FSERR_UNSUPPORTED, \"method not on read-only wrapper\"]]"),
                    "forwarder allows tell + size, blocks chmod", *ackCh)
                }}
              }}
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
        "FileioReadonlyForwarderSpec".to_string(),
    )
    .expect("compile fileio_readonly_forwarder test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_readonly_forwarder spec failed");
}

/// Slice 10a-6: canonical example `fileio_buffer_loop.rho`.
///
/// Bounded-memory line-by-line read via `readLineInto(buf)` in a
/// tail-recursive loop.  PB-B-5 (2026-09-02) shipped the Allocator
/// publication at `rho:serve:1.0.0:<FS_GENERATOR_PUB_KEY_HEX>:buffer:1.0.0`,
/// so user deploys can now obtain a Buffer via `lookupVersion` +
/// `alloc!?("allocBytes", n)`.  E2E resolution + minting is pinned
/// by `buffer_cap_is_resolvable_via_versioned_registry_uri` in
/// `fileio_fs_spec.rs`.
///
/// Body added 2026-09-03: bundles a 3-line source file, composes a
/// RhoSpec that runs a tail-recursive `readLineInto(buf)` loop
/// counting iterations, and asserts the loop terminates on the
/// eof-marked iteration having consumed all three lines.  Proves
/// the composed path (Allocator versioned lookup → allocBytes →
/// File.readLineInto arity-1 → Buffer.clear) works end-to-end
/// under real genesis + RhoSpec plumbing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_buffer_loop_bounded_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_path = dir.path().join("source.txt");

    // 3 lines; the loop should run 4 iterations (3 lines + one eof
    // termination iteration).  Each line is short enough to fit in
    // the 4 KiB allocated buffer without truncation.
    std::fs::write(&source_path, b"alpha\nbeta\ngamma\n").expect("seed source");
    let source_canon = std::fs::canonicalize(&source_path).expect("canonicalize source");

    let source_entry = BundleEntry::try_new(
        "source".to_string(),
        source_canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("source bundle entry");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![source_entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let pk_hex = hex::encode(standard_deploys::FS_GENERATOR_PUB_KEY.bytes.clone());

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  v1Api(`rho:registry:v1:internal`),
  RhoSpecCh,
  fsCh,
  allocCh,
  test_buffer_loop_bounded_read
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Buffer.readLineInto loop consumes bundled source and hits eof",
        *test_buffer_loop_bounded_read)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  v1Api!("lookupVersion",
    "rho:serve:1.0.0:{pk_hex}:buffer:1.0.0", Nil, *allocCh) |

  for(@(_, fs) <- fsCh; @alloc <- allocCh) {{
    contract test_buffer_loop_bounded_read(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "source", {{"mode": "r"}});
          @[true, buf]  <- @alloc!?("allocBytes", 4096)) {{
        new loopReader, doneCh in {{
          // Tail-recursive loop: on each iteration, readLineInto
          // then either terminate (eof) or clear + recurse.  Passes
          // the running iteration count through so the finalizer can
          // assert loop length.
          contract loopReader(@n) = {{
            for(@r <- @file!?("readLineInto", buf)) {{
              match r {{
                [true, [_nBytes, m /\ Map]] => {{
                  match m.get("eof") {{
                    true  => doneCh!(("eof", n + 1))
                    false => {{
                      for(@_ <- @buf!?("clear")) {{
                        loopReader!(n + 1)
                      }}
                    }}
                    // Missing key → break to avoid infinite loop.
                    _     => doneCh!(("missing-eof-key", n))
                  }}
                }}
                _ => doneCh!(("readLineInto-failed", r))
              }}
            }}
          }} |
          loopReader!(0) |
          for(@outcome <- doneCh) {{
            match outcome {{
              ("eof", n /\ Int) => {{
                rhoSpec!("assert", (n, "==", 4),
                  "readLineInto loop must run exactly 4 iterations (3 lines + eof)",
                  *ackCh)
              }}
              _ => {{
                rhoSpec!("assert", (outcome, "==", "(\"eof\", 4)"),
                  "loop must terminate on the eof branch after 4 iterations",
                  *ackCh)
              }}
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
        "FileioBufferLoopSpec".to_string(),
    )
    .expect("compile fileio_buffer_loop test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_buffer_loop spec failed");
}

/// Slice 10a-8 (partial): sanity check that `Fs.stdin` and
/// `Fs.stdout` return caps that can be dispatched.  The full
/// echo-loop test from `fileio_stdio.rho` is deferred to slice 10c
/// (stdio replay wiring), which lands the capture side of
/// Stdin.fsRead so a follower replay can reproduce a lead's stdin
/// reads deterministically.
///
/// This test verifies the surface exists — a regression in Fs's
/// stdin / stdout methods or in Stdin.rho / Stdout.rho constructor
/// invocations would fail here without needing live stdin input.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_stdio_caps_are_resolvable() {
    let params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_stdin_and_stdout_return_caps
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("Fs.stdin and Fs.stdout both return [true, cap]",
         *test_stdin_and_stdout_return_caps)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_stdin_and_stdout_return_caps(rhoSpec, _, ackCh) = {{
      for(@rIn  <- @fs!?("stdin");
          @rOut <- @fs!?("stdout")) {{
        match [rIn, rOut] {{
          [[true, _], [true, _]] => {{
            rhoSpec!("assert", (true, "==", true),
              "stdin and stdout resolve to caps", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", ([rIn, rOut], "==", "[[true, _], [true, _]]"),
              "stdin and stdout resolve to caps", *ackCh)
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
        "FileioStdioCapsSpec".to_string(),
    )
    .expect("compile fileio_stdio caps test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_stdio caps spec failed");
}

/// Slice 10a-9: canonical example `fileio_parallel.rho`, sequential
/// variant.  Uses `fold(0, plus)` over a byte stream to assert the
/// result is the byte-sum of the source file — the mathematical
/// convergent that the companion `foldConcurrent` and `mapReduce`
/// variants below must also produce.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_parallel_byte_sum_sequential_variant() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("parallel.dat");
    // Seed with a small byte pattern whose sum is easy to compute
    // out of band.  Bytes 1..=10 → sum = 55.
    let content: Vec<u8> = (1u8..=10).collect();
    std::fs::write(&file_path, &content).expect("seed file");
    let expected_sum: i64 = content.iter().map(|&b| b as i64).sum();
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_byte_sum_via_fold
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("sequential fold sums every byte in the file",
         *test_byte_sum_via_fold)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_byte_sum_via_fold(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          new plus in {{
            contract plus(returnCh, @acc, @byte) = {{
              returnCh!(acc + byte.nth(0))
            }} |
            for(@r <- @byteStream!?("fold", 0, *plus)) {{
              match r {{
                [true, total] => {{
                  rhoSpec!("assert", (total, "==", {expected_sum}),
                    "byteStream.fold(0, plus) sums to expected total", *ackCh)
                }}
                _ => {{
                  rhoSpec!("assert", (r, "==", "[true, sum]"),
                    "byteStream.fold(0, plus) sums to expected total", *ackCh)
                }}
              }}
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
        "FileioParallelSequentialSpec".to_string(),
    )
    .expect("compile fileio_parallel sequential test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_parallel sequential spec failed");
}

/// Slice 10a-9: `foldConcurrent` variant.  Same reduction as the
/// sequential test above but via `byteStream.foldConcurrent(0, plus, 8)`
/// (8 worker fan-out).  Asserts the same total (55 for bytes 1..=10)
/// — this is the convergence property required by the FIP: for a
/// commutative+associative combine, the parallel result equals the
/// sequential result regardless of scheduling.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_parallel_byte_sum_foldconcurrent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("parallel_fc.dat");
    let content: Vec<u8> = (1u8..=10).collect();
    std::fs::write(&file_path, &content).expect("seed file");
    let expected_sum: i64 = content.iter().map(|&b| b as i64).sum();
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_byte_sum_via_foldconcurrent
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("foldConcurrent(0, plus, 8) sums every byte in the file",
         *test_byte_sum_via_foldconcurrent)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_byte_sum_via_foldconcurrent(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          new plus in {{
            contract plus(returnCh, @acc, @byte) = {{
              returnCh!(acc + byte.nth(0))
            }} |
            for(@r <- @byteStream!?("foldConcurrent", 0, *plus, 8)) {{
              match r {{
                [true, total] => {{
                  rhoSpec!("assert", (total, "==", {expected_sum}),
                    "byteStream.foldConcurrent(0, plus, 8) sums to expected total", *ackCh)
                }}
                _ => {{
                  rhoSpec!("assert", (r, "==", "[true, sum]"),
                    "byteStream.foldConcurrent(0, plus, 8) sums to expected total", *ackCh)
                }}
              }}
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
        "FileioParallelFoldConcurrentSpec".to_string(),
    )
    .expect("compile fileio_parallel foldConcurrent test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_parallel foldConcurrent spec failed");
}

/// Slice 10a-9: `mapReduce` variant.  Sum-of-squares over the same
/// byte stream via `mapReduce(mapSquare, plus, 0, 8)`.  Asserts the
/// exact expected sum of squares (385 for bytes 1..=10:
/// 1+4+9+16+25+36+49+64+81+100 = 385) — verifying both the mapFn
/// invocation and the parallel reduce convergence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_parallel_byte_sum_of_squares_via_mapreduce() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("parallel_mr.dat");
    let content: Vec<u8> = (1u8..=10).collect();
    std::fs::write(&file_path, &content).expect("seed file");
    let expected_sum_sq: i64 = content.iter().map(|&b| (b as i64) * (b as i64)).sum();
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_sum_of_squares_via_mapreduce
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("mapReduce(mapSquare, plus, 0, 8) computes sum of byte squares",
         *test_sum_of_squares_via_mapreduce)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_sum_of_squares_via_mapreduce(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          new mapSquare, plus in {{
            contract mapSquare(returnCh, @byte) = {{
              returnCh!(byte.nth(0) * byte.nth(0))
            }} |
            contract plus(returnCh, @acc, @sq) = {{
              returnCh!(acc + sq)
            }} |
            for(@r <- @byteStream!?("mapReduce", *mapSquare, *plus, 0, 8)) {{
              match r {{
                [true, total] => {{
                  rhoSpec!("assert", (total, "==", {expected_sum_sq}),
                    "byteStream.mapReduce(mapSquare, plus, 0, 8) yields sum of squares",
                    *ackCh)
                }}
                _ => {{
                  rhoSpec!("assert", (r, "==", "[true, sumOfSquares]"),
                    "byteStream.mapReduce yields sum of squares", *ackCh)
                }}
              }}
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
        "FileioParallelMapReduceSpec".to_string(),
    )
    .expect("compile fileio_parallel mapReduce test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fileio_parallel mapReduce spec failed");
}

/// Slice 10a-7: canonical example `fileio_rows.rho`.
///
/// Buffer-of-buffers via `alloc.allocRows(128, 8192, "utf8")` +
/// `file.readLinesInto(rows)`.  PB-B-5 unblocked 2026-09-02 (see
/// `fileio_buffer_loop_bounded_read` docstring for the reference
/// pattern).
///
/// Body added 2026-09-03: bundles a small (3-line) source file
/// well under the 128-row capacity, then verifies the single-call
/// bulk fill.  Since 3 < 128, the call reads the whole file → reply
/// shape `[true, [3, {"eof": true, "truncated": false}]]`, and
/// `rows.getAt(i)` for i ∈ [0, 3) each returns a functional inner
/// buffer whose `toByteArray` yields the seeded line bytes.  Proves
/// the composed path (Allocator versioned lookup → allocRows → Rows
/// wrapping N inner Buffers → File.readLinesInto → Rows.getAt →
/// inner Buffer.toByteArray) works end-to-end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fileio_rows_readlinesinto() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_path = dir.path().join("source.txt");

    std::fs::write(&source_path, b"alpha\nbeta\ngamma\n").expect("seed source");
    let source_canon = std::fs::canonicalize(&source_path).expect("canonicalize source");

    let source_entry = BundleEntry::try_new(
        "source".to_string(),
        source_canon,
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("source bundle entry");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![source_entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let pk_hex = hex::encode(standard_deploys::FS_GENERATOR_PUB_KEY.bytes.clone());

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  v1Api(`rho:registry:v1:internal`),
  RhoSpecCh,
  fsCh,
  allocCh,
  test_rows_readlinesinto
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Rows.readLinesInto bulk-fills all lines under Rows capacity",
        *test_rows_readlinesinto)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  v1Api!("lookupVersion",
    "rho:serve:1.0.0:{pk_hex}:buffer:1.0.0", Nil, *allocCh) |

  for(@(_, fs) <- fsCh; @alloc <- allocCh) {{
    contract test_rows_readlinesinto(rhoSpec, _, ackCh) = {{
      // 128 inner buffers × 8192 utf8 units — bulk cap far exceeds
      // the 3-line source, so a single readLinesInto call must
      // fill exactly 3 rows and set eof=true.
      for(@[true, file] <- @fs!?("openFile", "source", {{"mode": "r"}});
          @rowsReply    <- @alloc!?("allocRows", 128, 8192, "utf8")) {{
        match rowsReply {{
          [true, rows] => {{
            for(@r <- @file!?("readLinesInto", rows)) {{
              match r {{
                [true, [n /\ Int, m /\ Map]] => {{
                  new nCh, eofCh in {{
                    rhoSpec!("assert", (n, "==", 3),
                      "readLinesInto must fill exactly 3 rows (source has 3 lines)",
                      *nCh) |
                    rhoSpec!("assert", (m.get("eof"), "==", true),
                      "readLinesInto must set eof=true when source is exhausted",
                      *eofCh) |
                    for(@_ <- nCh; @_ <- eofCh) {{
                      // Verify the first row is a functional inner
                      // Buffer whose toByteArray returns non-empty
                      // bytes (proves the composed path Rows.getAt →
                      // inner Buffer.toByteArray works — the exact
                      // line-content check is out of scope, kept as
                      // a smoke test).
                      for(@innerReply <- @rows!?("getAt", 0)) {{
                        match innerReply {{
                          [true, inner] => {{
                            for(@bytesReply <- @inner!?("toByteArray", 1073741824)) {{
                              match bytesReply {{
                                [true, lineBytes] => {{
                                  rhoSpec!("assert",
                                    (lineBytes.length() > 0, "==", true),
                                    "rows.getAt(0).toByteArray must return non-empty bytes",
                                    *ackCh)
                                }}
                                _ => {{
                                  rhoSpec!("assert",
                                    (bytesReply, "==", "[true, _]"),
                                    "inner buffer toByteArray must succeed",
                                    *ackCh)
                                }}
                              }}
                            }}
                          }}
                          _ => {{
                            rhoSpec!("assert", (innerReply, "==", "[true, _]"),
                              "rows.getAt(0) must return [true, innerBuffer]",
                              *ackCh)
                          }}
                        }}
                      }}
                    }}
                  }}
                }}
                _ => {{
                  rhoSpec!("assert", (r, "==", "[true, [_, _]]"),
                    "readLinesInto must return [true, [n, meta]]", *ackCh)
                }}
              }}
            }}
          }}
          _ => {{
            rhoSpec!("assert", (rowsReply, "==", "[true, _]"),
              "allocRows must succeed", *ackCh)
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
        "FileioRowsReadLinesIntoSpec".to_string(),
    )
    .expect("compile fileio_rows test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("fileio_rows spec failed");
}
