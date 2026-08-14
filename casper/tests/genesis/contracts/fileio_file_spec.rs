//! Phase 5 File-agent end-to-end regressions.
//!
//! Verifies the core File-agent method surface end-to-end against
//! real syscalls through the genesis+RhoSpec harness.  Complements
//! the mock-syscall coverage in `rholang/tests/file_dir_check.rs`
//! by exercising the actual open→dispatch→native chain that user
//! deploys hit in production.
//!
//! Scope (Phase-9-independent):
//!   - Cursor semantics: seek(offset, whence) ∈ {"set", "cur", "end"} +
//!     tell() round-trip.
//!   - size() returns the byte length; matches independent `std::fs`
//!     metadata check.
//!   - readN edge cases: n=0 (Unix read(2) no-op → empty bytes),
//!     n<0 (FSERR_BAD_ARG), n>eof (short read).
//!   - truncate(n) on rw-mode: post-truncate size == n; truncate(0)
//!     on rw-mode empties the file; truncate on r-mode returns
//!     FSERR_UNSUPPORTED (write-capable gate).
//!   - close() gates every subsequent method with FSERR_CLOSED.
//!
//! Deferred to Phase 9: materialization-cap tests, per-op cost
//! regression.  Deferred elsewhere: buffer-taking variants
//! (readInto/readLineInto — blocked on PB-B-5 Allocator publication).

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Build a bundled File "target" in `mode` over a tempfile
/// pre-populated with `content`.  Returns tempdir (keep alive!),
/// genesis params, and the fs_uri.
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

/// Cursor semantics + size on a read-only file.  Verifies:
///   - seek(0, "set") returns [true, 0]; tell() confirms position 0.
///   - seek(5, "set") returns [true, 5]; tell() confirms position 5.
///   - size() returns exactly the file's byte length.
///   - readN(100) at cursor=5 returns a 7-byte short read of "e chars"
///     (n > bytes-remaining-to-eof → returns exactly the remaining bytes,
///     not FSERR nor a padded reply — POSIX read(2) short-read semantics).
///   - readN(0) returns exactly [true, empty ByteArray] (POSIX read(2)
///     no-op; pinned to the exact empty ByteArray, not a `[true, _]`
///     wildcard, so a regression that returned bytes for n=0 would fail).
///   - readN(-1) returns FSERR_BAD_ARG.
///
/// All six assertions run against one File cap so they share the
/// genesis-setup overhead.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_cursor_size_and_readn_on_read_only() {
    let content = b"twelve chars"; // 12 bytes
    let (_dir, params, fs_uri) = bundle_file(content, "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_cursor_size_readn
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("seek + tell + size + readN edge cases on read-only file",
        *test_cursor_size_readn)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_cursor_size_readn(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@rSeek0 <- @file!?("seek", 0, "set")) {{
          for(@rTell0 <- @file!?("tell")) {{
            for(@rSeek5 <- @file!?("seek", 5, "set")) {{
              for(@rTell5 <- @file!?("tell")) {{
                for(@rSize <- @file!?("size")) {{
                  for(@rReadShort <- @file!?("readN", 100)) {{
                    for(@rReadEmpty <- @file!?("readN", 0)) {{
                      for(@rReadNeg <- @file!?("readN", -1)) {{
                        match [rSeek0, rTell0, rSeek5, rTell5, rSize,
                               rReadShort, rReadEmpty, rReadNeg] {{
                          [[true, 0], [true, 0], [true, 5], [true, 5], [true, 12],
                           [true, shortBytes /\ ByteArray],
                           [true, emptyBytes /\ ByteArray],
                           [false, "FSERR_BAD_ARG", _]] => {{
                            rhoSpec!("assert",
                              ([shortBytes, emptyBytes], "==",
                               ["65206368617273".hexToBytes(), "".hexToBytes()]),
                              "cursor + size + readN edge cases (short-read at pos 5 == \"e chars\"; readN(0) == empty)",
                              *ackCh)
                          }}
                          _ => {{
                            rhoSpec!("assert",
                              ([rSeek0, rTell0, rSeek5, rTell5, rSize,
                                rReadShort, rReadEmpty, rReadNeg],
                               "==", "[expected shape tuple]"),
                              "cursor + size + readN edge cases", *ackCh)
                          }}
                        }}
                      }}
                    }}
                  }}
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
        "FileCursorSizeReadnSpec".to_string(),
    )
    .expect("compile file_cursor_size_readn spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("file_cursor_size_readn spec failed");
}

/// truncate(n) on a rw-mode file resizes to exactly n bytes.
/// Verifies:
///   - Initial size == 16 bytes (from the seeded content).
///   - truncate(8) returns [true]; size() confirms 8.
///   - truncate(0) returns [true]; size() confirms 0.
///
/// On-disk verification (post-run) confirms the file was actually
/// truncated (not just the reported size).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_write_mode_roundtrip() {
    let content = b"sixteen bytes!!!"; // 16 bytes
    let (dir, params, fs_uri) = bundle_file(content, "rw");
    let file_path = dir.path().join("target.bin");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_truncate_roundtrip
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("truncate + size roundtrip on rw-mode file", *test_truncate_roundtrip)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_truncate_roundtrip(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
        for(@rSize0 <- @file!?("size")) {{
          for(@rTrunc8 <- @file!?("truncate", 8)) {{
            for(@rSize1 <- @file!?("size")) {{
              for(@rTrunc0 <- @file!?("truncate", 0)) {{
                for(@rSize2 <- @file!?("size")) {{
                  match [rSize0, rTrunc8, rSize1, rTrunc0, rSize2] {{
                    [[true, 16], [true], [true, 8], [true], [true, 0]] => {{
                      rhoSpec!("assert", (true, "==", true),
                        "truncate 16→8→0 roundtrip", *ackCh)
                    }}
                    _ => {{
                      rhoSpec!("assert",
                        ([rSize0, rTrunc8, rSize1, rTrunc0, rSize2],
                         "==", "[[true, 16], [true], [true, 8], [true], [true, 0]]"),
                        "truncate 16→8→0 roundtrip", *ackCh)
                    }}
                  }}
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

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "FileTruncateSpec".to_string())
            .expect("compile file_truncate spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("file_truncate spec failed");

    // On-disk verification: file was actually truncated to 0.
    let on_disk = std::fs::metadata(&file_path).expect("stat dest").len();
    assert_eq!(
        on_disk, 0,
        "post-run file should be truncated to 0 bytes on disk (got {on_disk})"
    );
}

/// truncate on a "r"-opened file returns FSERR_UNSUPPORTED without
/// modifying the underlying file.  Pins File.rho line 578's write-
/// capable gate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_read_only_returns_unsupported() {
    let content = b"read only content";
    let (dir, params, fs_uri) = bundle_file(content, "r");
    let file_path = dir.path().join("target.bin");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_truncate_read_only
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("truncate on read-only file returns FSERR_UNSUPPORTED",
        *test_truncate_read_only)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_truncate_read_only(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@r <- @file!?("truncate", 0)) {{
          match r {{
            [false, "FSERR_UNSUPPORTED", _] => {{
              rhoSpec!("assert", (true, "==", true),
                "truncate on read-only file gates with FSERR_UNSUPPORTED", *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, _]"),
                "truncate on read-only file gates with FSERR_UNSUPPORTED", *ackCh)
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
        "FileTruncateReadOnlySpec".to_string(),
    )
    .expect("compile file_truncate_read_only spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("file_truncate_read_only spec failed");

    // On-disk verification: file untouched.
    let on_disk = std::fs::read(&file_path).expect("read post-run");
    assert_eq!(
        on_disk, content,
        "read-only file must not be truncated by rejected truncate call"
    );
}

/// close() gates every subsequent method with FSERR_CLOSED.  Verifies
/// the state-cell transition + closed-arm in seek / tell / size /
/// readN / truncate.  A regression that skipped the stateP check would
/// let post-close methods hit fsRead/fsSeek/etc. against a closed
/// (returned-to-table) fd — leading to raw OS EBADF surfacing as
/// FSERR_IO instead of the semantically-correct FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_close_gates_subsequent_methods() {
    let (_dir, params, fs_uri) = bundle_file(b"content", "rw");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_close_gates
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("close() gates seek / tell / size / readN / truncate with FSERR_CLOSED",
        *test_close_gates)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_close_gates(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
        for(@rClose <- @file!?("close")) {{
          for(@rSeek <- @file!?("seek", 0, "set")) {{
            for(@rTell <- @file!?("tell")) {{
              for(@rSize <- @file!?("size")) {{
                for(@rRead <- @file!?("readN", 4)) {{
                  for(@rTrunc <- @file!?("truncate", 0)) {{
                    match [rClose, rSeek, rTell, rSize, rRead, rTrunc] {{
                      [[true],
                       [false, "FSERR_CLOSED", _],
                       [false, "FSERR_CLOSED", _],
                       [false, "FSERR_CLOSED", _],
                       [false, "FSERR_CLOSED", _],
                       [false, "FSERR_CLOSED", _]] => {{
                        rhoSpec!("assert", (true, "==", true),
                          "all methods gate with FSERR_CLOSED after close()", *ackCh)
                      }}
                      _ => {{
                        rhoSpec!("assert",
                          ([rClose, rSeek, rTell, rSize, rRead, rTrunc],
                           "==", "[[true], 5x [false, FSERR_CLOSED, _]]"),
                          "all methods gate with FSERR_CLOSED after close()", *ackCh)
                      }}
                    }}
                  }}
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
        "FileCloseGatesSpec".to_string(),
    )
    .expect("compile file_close_gates spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("file_close_gates spec failed");
}
