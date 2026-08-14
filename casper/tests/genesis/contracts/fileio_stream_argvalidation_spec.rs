//! Argument validation + boundary coverage for
//! `Stream.foldConcurrent` / `Stream.mapReduce`.
//!
//! Complements the sunny-path tests in `fileio_examples_spec.rs`
//! (`fileio_parallel_byte_sum_foldconcurrent`,
//! `fileio_parallel_byte_sum_of_squares_via_mapreduce`) and the
//! LineStream refusal test in `fileio_stream_spec.rs`.  Each test
//! pins one of the impl's declared error paths or spec-mandated
//! boundary behaviors so a refactor that silently drops a guard is
//! caught here rather than at runtime by a user deploy.
//!
//! ## Coverage matrix
//!
//! | Behavior                                    | foldConcurrent | mapReduce |
//! |---------------------------------------------|----------------|-----------|
//! | workers ≤ 0 → FSERR_BAD_ARG                 |       ✓        |     ✓     |
//! | workers non-Int → FSERR_BAD_ARG             |       ✓        |     ✓     |
//! | workers > 256 → FSERR_QUOTA_EXCEEDED (§253) |       ✓        |     ✓     |
//! | workers = 1 → sequential equivalence (§252) |       ✓        |     ✓     |
//! | empty stream → [true, init]                 |       ✓        |     ✓     |
//!
//! Each argument-validation test bundles a small file, minted from
//! bytes 1..=10, and reuses one open ByteStream to run all four
//! arg-validation checks sequentially — the BAD_ARG /
//! QUOTA_EXCEEDED paths short-circuit before touching the source
//! producer, so cursor state is unaffected between checks.  Boundary
//! tests use their own dedicated bundle (empty file or bytes 1..=10).

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

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

/// foldConcurrent argument-validation matrix — pins the four
/// pre-source-touch error arms (Stream.rho:350-402):
///   - workers = 0             → FSERR_BAD_ARG "workers must be positive"
///   - workers = -1            → FSERR_BAD_ARG "workers must be positive"
///   - workers = "seven"       → FSERR_BAD_ARG "workers must be an integer"
///   - workers = 300           → FSERR_QUOTA_EXCEEDED "workers exceeds cap (256)"
/// All four checks reuse a single ByteStream because the arms
/// short-circuit before probing the source or spawning workers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foldconcurrent_argument_validation() {
    let content: Vec<u8> = (1u8..=10).collect();
    let (_dir, params, fs_uri) = bundle_file(&content, "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  noop,
  test_foldconcurrent_argval
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("foldConcurrent rejects invalid workers with exact codes/messages",
        *test_foldconcurrent_argval)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract noop(returnCh, @acc, @_v) = {{ returnCh!(acc) }} |
    contract test_foldconcurrent_argval(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@rZero <- @byteStream!?("foldConcurrent", 0, *noop, 0)) {{
            for(@rNeg <- @byteStream!?("foldConcurrent", 0, *noop, -1)) {{
              for(@rNonInt <- @byteStream!?("foldConcurrent", 0, *noop, "seven")) {{
                for(@rOverCap <- @byteStream!?("foldConcurrent", 0, *noop, 300)) {{
                  rhoSpec!("assert",
                    ([rZero, rNeg, rNonInt, rOverCap], "==",
                     [[false, "FSERR_BAD_ARG", "workers must be positive"],
                      [false, "FSERR_BAD_ARG", "workers must be positive"],
                      [false, "FSERR_BAD_ARG", "workers must be an integer"],
                      [false, "FSERR_QUOTA_EXCEEDED", "workers exceeds cap (256)"]]),
                    "foldConcurrent argument validation matrix", *ackCh)
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
        "FoldConcurrentArgValSpec".to_string(),
    )
    .expect("compile foldconcurrent_argval spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("foldconcurrent_argval spec failed");
}

/// mapReduce argument-validation matrix — pins the same four error
/// arms in mapReduce (Stream.rho:428-472).  Same rationale as
/// foldconcurrent_argument_validation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mapreduce_argument_validation() {
    let content: Vec<u8> = (1u8..=10).collect();
    let (_dir, params, fs_uri) = bundle_file(&content, "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  noopMap,
  noopReduce,
  test_mapreduce_argval
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("mapReduce rejects invalid workers with exact codes/messages",
        *test_mapreduce_argval)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract noopMap(returnCh, @v) = {{ returnCh!(v) }} |
    contract noopReduce(returnCh, @acc, @_m) = {{ returnCh!(acc) }} |
    contract test_mapreduce_argval(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@rZero <- @byteStream!?("mapReduce", *noopMap, *noopReduce, 0, 0)) {{
            for(@rNeg <- @byteStream!?("mapReduce", *noopMap, *noopReduce, 0, -1)) {{
              for(@rNonInt <- @byteStream!?("mapReduce", *noopMap, *noopReduce, 0, "seven")) {{
                for(@rOverCap <- @byteStream!?("mapReduce", *noopMap, *noopReduce, 0, 300)) {{
                  rhoSpec!("assert",
                    ([rZero, rNeg, rNonInt, rOverCap], "==",
                     [[false, "FSERR_BAD_ARG", "workers must be positive"],
                      [false, "FSERR_BAD_ARG", "workers must be positive"],
                      [false, "FSERR_BAD_ARG", "workers must be an integer"],
                      [false, "FSERR_QUOTA_EXCEEDED", "workers exceeds cap (256)"]]),
                    "mapReduce argument validation matrix", *ackCh)
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
        "MapReduceArgValSpec".to_string(),
    )
    .expect("compile mapreduce_argval spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("mapreduce_argval spec failed");
}

/// foldConcurrent with workers=1 must produce the same total as the
/// multi-worker version — spec §252 explicitly guarantees "effectively
/// runs sequentially when workers is 1".  Guards against a spawner
/// off-by-one (`k <= 0` vs. `k < 0`, or `spawner!(k)` vs.
/// `spawner!(k - 1)`) where workers=8 would still pass but workers=1
/// would deadlock or miscount.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foldconcurrent_workers_one_matches_sequential() {
    let content: Vec<u8> = (1u8..=10).collect();
    let expected_sum: i64 = content.iter().map(|&b| b as i64).sum();
    let (_dir, params, fs_uri) = bundle_file(&content, "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  plus,
  test_workers_one
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("foldConcurrent(0, plus, 1) matches expected sum", *test_workers_one)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract plus(returnCh, @acc, @byte) = {{
      returnCh!(acc + byte.nth(0))
    }} |
    contract test_workers_one(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@r <- @byteStream!?("foldConcurrent", 0, *plus, 1)) {{
            rhoSpec!("assert", (r, "==", [true, {expected_sum}]),
              "foldConcurrent(0, plus, 1) sums to expected total", *ackCh)
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
        "FoldConcurrentWorkersOneSpec".to_string(),
    )
    .expect("compile foldconcurrent workers=1 spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("foldconcurrent workers=1 spec failed");
}

/// mapReduce with workers=1 must produce the same total as the
/// multi-worker version.  Same rationale as
/// foldconcurrent_workers_one_matches_sequential.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mapreduce_workers_one_matches_sequential() {
    let content: Vec<u8> = (1u8..=10).collect();
    let expected_sum_sq: i64 = content.iter().map(|&b| (b as i64) * (b as i64)).sum();
    let (_dir, params, fs_uri) = bundle_file(&content, "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  mapSquare,
  plus,
  test_mr_workers_one
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("mapReduce(mapSquare, plus, 0, 1) matches expected sum of squares",
        *test_mr_workers_one)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract mapSquare(returnCh, @byte) = {{
      returnCh!(byte.nth(0) * byte.nth(0))
    }} |
    contract plus(returnCh, @acc, @sq) = {{
      returnCh!(acc + sq)
    }} |
    contract test_mr_workers_one(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@r <- @byteStream!?("mapReduce", *mapSquare, *plus, 0, 1)) {{
            rhoSpec!("assert", (r, "==", [true, {expected_sum_sq}]),
              "mapReduce(mapSquare, plus, 0, 1) sums squares to expected total",
              *ackCh)
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
        "MapReduceWorkersOneSpec".to_string(),
    )
    .expect("compile mapreduce workers=1 spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("mapreduce workers=1 spec failed");
}

/// foldConcurrent over an empty stream must return `[true, init]` —
/// all workers see EOS immediately, publish `("ok", Nil)` to doneCh,
/// and the collector peeks accP (still holding init) for the final
/// value.  Guards against a regression where empty-stream handling
/// deadlocks (worker never publishes) or returns a wrong-shaped
/// reply (e.g., init not preserved).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foldconcurrent_empty_stream_returns_init() {
    let (_dir, params, fs_uri) = bundle_file(b"", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  plus,
  test_empty_fc
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("foldConcurrent over empty stream returns [true, init]", *test_empty_fc)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract plus(returnCh, @acc, @byte) = {{
      returnCh!(acc + byte.nth(0))
    }} |
    contract test_empty_fc(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@r <- @byteStream!?("foldConcurrent", 42, *plus, 4)) {{
            rhoSpec!("assert", (r, "==", [true, 42]),
              "foldConcurrent over empty stream returns init unchanged", *ackCh)
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
        "FoldConcurrentEmptySpec".to_string(),
    )
    .expect("compile foldconcurrent empty spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("foldconcurrent empty spec failed");
}

/// mapReduce over an empty stream must return `[true, init]` —
/// all workers see EOS immediately, publish `("ok", [])` (empty
/// partial); foldPartials over N `[]` partials returns
/// `[true, init]` without invoking reduceFn.  Guards against a
/// regression in foldPartials' empty-partial handling.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mapreduce_empty_stream_returns_init() {
    let (_dir, params, fs_uri) = bundle_file(b"", "r");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  mapSquare,
  plus,
  test_empty_mr
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("mapReduce over empty stream returns [true, init]", *test_empty_mr)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract mapSquare(returnCh, @byte) = {{
      returnCh!(byte.nth(0) * byte.nth(0))
    }} |
    contract plus(returnCh, @acc, @sq) = {{
      returnCh!(acc + sq)
    }} |
    contract test_empty_mr(rhoSpec, _, ackCh) = {{
      for(@[true, file] <- @fs!?("openFile", "target", {{"mode": "r"}})) {{
        for(@[true, byteStream] <- @file!?("bytes")) {{
          for(@r <- @byteStream!?("mapReduce", *mapSquare, *plus, 99, 4)) {{
            rhoSpec!("assert", (r, "==", [true, 99]),
              "mapReduce over empty stream returns init unchanged", *ackCh)
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
        "MapReduceEmptySpec".to_string(),
    )
    .expect("compile mapreduce empty spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("mapreduce empty spec failed");
}
