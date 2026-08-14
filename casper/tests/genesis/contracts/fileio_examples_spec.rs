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
/// tail-recursive loop.  E2E test-side verification is
/// **deferred**: the Allocator agent is compiled into the FsGenesis
/// composed source but not published to user deploys — only Fs is
/// exported via `insertSigned` (see `fs_genesis.rs` MVP note §6).
/// User deploys cannot obtain a Buffer until the future Powerbox
/// slice PB-B-5 publishes an Allocator delegation at
/// `rho:lang:buffer:1.0.0`.
///
/// The `.rho` example is a documentation artifact that describes
/// the expected user surface; this test's body is a placeholder
/// that will be filled in once the publication lands.  Until then
/// it is `#[ignore]`-d so `cargo test` remains green.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "blocked on PB-B-5: Allocator not yet published to user deploys"]
async fn fileio_buffer_loop_bounded_read() {
    // See rholang/examples/fileio_buffer_loop.rho for the intended
    // user code.  Once PB-B-5 publishes the Allocator, this test
    // should:
    //   - Bundle a "target" file pre-populated with N lines.
    //   - Compose a RhoSpec source that runs the buffer-loop.
    //   - Assert every line is echoed via stdout in order.
    //   - Assert the loop terminates on eof=true.
    unimplemented!("blocked on PB-B-5: Allocator publication at rho:lang:buffer:1.0.0")
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
/// `file.readLinesInto(rows)`.  Same PB-B-5 block as slice 10a-6 —
/// the `.rho` example ships as documentation; this test's body is a
/// placeholder that will be filled in once the Allocator publication
/// lands.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "blocked on PB-B-5: Allocator not yet published to user deploys"]
async fn fileio_rows_readlinesinto() {
    // See rholang/examples/fileio_rows.rho for the intended user
    // code.  Once PB-B-5 publishes the Allocator, this test should:
    //   - Bundle a "target" file with N > 128 lines.
    //   - Run allocRows(128, 8192, "utf8") + readLinesInto.
    //   - Assert reply is [true, [128, {"eof": false, ...}]] (fills
    //     to buffer-of-buffers capacity).
    //   - Iterate rows.getAt(i) and assert each inner line matches
    //     the source file's i-th line.
    unimplemented!("blocked on PB-B-5: Allocator publication at rho:lang:buffer:1.0.0")
}
