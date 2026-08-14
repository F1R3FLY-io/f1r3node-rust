//! Phase 4 Stream-library end-to-end regressions.
//!
//! Verifies the LineStream negative-path matrix mandated by plan
//! §Test infrastructure line 778:
//!
//!   `LineStream.chunk` / `foldChunks` / `foldConcurrent` /
//!   `mapReduce` all → `[false, "FSERR_UNSUPPORTED", _]`
//!
//! Rationale: LineStream's outer chunk container would require
//! multiple concurrently-active inner CharStreams sharing the source
//! cursor (spec §Stream §111), violating the single-active-inner rule.
//! The `chunk` chunkBuilder returns FSERR_UNSUPPORTED at the container-
//! pack step (File.rho line 4443); `foldChunks` propagates that error
//! unchanged (comment at Stream.rho line 277); `foldConcurrent` and
//! `mapReduce` are unimplemented and fall through to the agent's
//! `default(...@args)` arm which returns the same error code.
//!
//! Also pins Stream error-propagation on the sequential combinators:
//! calling `fold` / `foldChunks` on an explicitly-closed stream must
//! return `FSERR_CLOSED` from the state-cell peek, not silently
//! succeed with the initial accumulator.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Build a bundled File "target" in "r" mode over a tempfile
/// pre-populated with multi-line content, and return the genesis
/// parameters plus the fs_uri.
fn bundle_lines_file() -> (
    tempfile::TempDir,
    crate::util::genesis_builder::GenesisParameters,
    String,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("lines.txt");
    std::fs::write(&file_path, b"alpha\nbeta\ngamma\n").expect("seed file");
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
    (dir, params, fs_uri)
}

/// LineStream.chunk(n) → FSERR_UNSUPPORTED.  Pins File.rho line 4443's
/// outer chunkBuilder rejection through the full Stream.chunk dispatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn line_stream_chunk_returns_fserr_unsupported() {
    let (_dir, params, fs_uri) = bundle_lines_file();

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_line_stream_chunk_unsupported
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("LineStream.chunk returns FSERR_UNSUPPORTED", *test_line_stream_chunk_unsupported)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_line_stream_chunk_unsupported(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, lineStream] <- @file!?("lines")) {{
          for(@r <- @lineStream!?("chunk", 10)) {{
            match r {{
              [false, "FSERR_UNSUPPORTED", _] => {{
                rhoSpec!("assert", (true, "==", true),
                  "chunk on LineStream returns FSERR_UNSUPPORTED", *ackCh)
              }}
              _ => {{
                rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, _]"),
                  "chunk on LineStream returns FSERR_UNSUPPORTED", *ackCh)
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
        "LineStreamChunkSpec".to_string(),
    )
    .expect("compile line_stream_chunk spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("line_stream_chunk spec failed");
}

/// LineStream.foldChunks propagates chunk's FSERR_UNSUPPORTED.  Pins
/// Stream.rho line 277's "foldChunks surfaces that same error unchanged"
/// contract through foldChunksLoop.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn line_stream_foldchunks_returns_fserr_unsupported() {
    let (_dir, params, fs_uri) = bundle_lines_file();

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  noopCombine,
  test_line_stream_foldchunks_unsupported
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("LineStream.foldChunks returns FSERR_UNSUPPORTED",
        *test_line_stream_foldchunks_unsupported)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract noopCombine(returnCh, @acc, @_chunk) = {{ returnCh!(acc) }} |
    contract test_line_stream_foldchunks_unsupported(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, lineStream] <- @file!?("lines")) {{
          for(@r <- @lineStream!?("foldChunks", 0, 10, *noopCombine)) {{
            match r {{
              [false, "FSERR_UNSUPPORTED", _] => {{
                rhoSpec!("assert", (true, "==", true),
                  "foldChunks on LineStream returns FSERR_UNSUPPORTED", *ackCh)
              }}
              _ => {{
                rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, _]"),
                  "foldChunks on LineStream returns FSERR_UNSUPPORTED", *ackCh)
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
        "LineStreamFoldChunksSpec".to_string(),
    )
    .expect("compile line_stream_foldchunks spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("line_stream_foldchunks spec failed");
}

/// LineStream.foldConcurrent / .mapReduce fall through to the Stream
/// agent's `default(...@args)` arm since neither method is implemented
/// (Stream.rho line 23: "Deferred to follow-up commits").  Pins the
/// default-arm contract so a future landing of these methods on any
/// specialization other than LineStream will need to also add an
/// explicit LineStream-rejection branch (spec §Stream §111).
///
/// Runs both dispatches sequentially against the same LineStream so
/// the whole-outer-stream sequential lock (Phase 8 slice 8a step 4e-4)
/// only needs one acquisition — running foldConcurrent + mapReduce as
/// two independent testSuite entries would race for the same-path
/// sequential lock with distinct holders, blocking the second forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn line_stream_parallel_combinators_return_fserr_unsupported() {
    let (_dir, params, fs_uri) = bundle_lines_file();

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  noopBinary,
  noopUnary,
  test_line_stream_parallel_unsupported
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("LineStream.foldConcurrent and .mapReduce return FSERR_UNSUPPORTED",
        *test_line_stream_parallel_unsupported)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract noopBinary(returnCh, @acc, @_v) = {{ returnCh!(acc) }} |
    contract noopUnary(returnCh, @v) = {{ returnCh!(v) }} |
    contract test_line_stream_parallel_unsupported(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, lineStream] <- @file!?("lines")) {{
          for(@rFold <- @lineStream!?("foldConcurrent", 0, *noopBinary, 4)) {{
            for(@rMap <- @lineStream!?("mapReduce", *noopUnary, *noopBinary, 0, 4)) {{
              match [rFold, rMap] {{
                [[false, "FSERR_UNSUPPORTED", _], [false, "FSERR_UNSUPPORTED", _]] => {{
                  rhoSpec!("assert", (true, "==", true),
                    "foldConcurrent and mapReduce both return FSERR_UNSUPPORTED", *ackCh)
                }}
                _ => {{
                  rhoSpec!("assert", ([rFold, rMap], "==",
                    "[[false, FSERR_UNSUPPORTED, _], [false, FSERR_UNSUPPORTED, _]]"),
                    "foldConcurrent and mapReduce both return FSERR_UNSUPPORTED", *ackCh)
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
        "LineStreamParallelSpec".to_string(),
    )
    .expect("compile line_stream_parallel spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("line_stream_parallel spec failed");
}

/// Stream.fold on an explicitly-closed stream returns FSERR_CLOSED.
/// Guards against a silent-success-with-init regression where fold
/// might treat a closed source as "no more elements" and return
/// `[true, init]` without diagnosis.  The underlying discipline is:
/// `close()` transitions state → "closed"; subsequent next() calls
/// short-circuit with FSERR_CLOSED (Stream.rho line 145-147); fold's
/// loop propagates that terminal reply unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stream_fold_on_closed_returns_fserr_closed() {
    let (_dir, params, fs_uri) = bundle_lines_file();

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  noopCombine,
  test_fold_on_closed_stream
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("fold on closed stream returns FSERR_CLOSED", *test_fold_on_closed_stream)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract noopCombine(returnCh, @acc, @_v) = {{ returnCh!(acc) }} |
    contract test_fold_on_closed_stream(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@_closeReply <- @byteStream!?("close")) {{
            for(@r <- @byteStream!?("fold", 0, *noopCombine)) {{
              match r {{
                [false, "FSERR_CLOSED", _] => {{
                  rhoSpec!("assert", (true, "==", true),
                    "fold on closed byteStream returns FSERR_CLOSED", *ackCh)
                }}
                _ => {{
                  rhoSpec!("assert", (r, "==", "[false, FSERR_CLOSED, _]"),
                    "fold on closed byteStream returns FSERR_CLOSED", *ackCh)
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
        "StreamFoldClosedSpec".to_string(),
    )
    .expect("compile stream_fold_closed spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("stream_fold_closed spec failed");
}
