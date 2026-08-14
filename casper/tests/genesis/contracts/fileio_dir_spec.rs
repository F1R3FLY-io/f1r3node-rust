//! Phase 5 Dir-agent end-to-end regressions.
//!
//! Complements the mock-syscall Dir coverage in `file_dir_check.rs`
//! by exercising the real openat-descent + fs_stat / fs_exists /
//! openFileImpl chain through the genesis+RhoSpec harness.
//!
//! Scope (Phase-9-independent):
//!   - `Dir.exists(rel)` reports present/absent children correctly.
//!   - `Dir.openFile(rel, "rw")` on a "r"-mode Dir returns
//!     FSERR_UNSUPPORTED (monotonic mode attenuation).
//!   - `Dir.openFile(rel, "r")` on a "r"-mode Dir returns a File cap
//!     whose readN drains the seeded content.
//!
//! Deferred: Dir mutations (removeFile/removeDir/rename/copyFile)
//! require rw-mode Dir bundles + on-disk verification patterns
//! that duplicate `fileio_file_spec` scaffolding; landing as a
//! follow-up slice keeps this file small.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Bundle a Dir at a tempdir with `child.txt` inside.  Returns
/// (tempdir, params, fs_uri) — tempdir must be kept alive.
fn bundle_dir_with_child(
    dir_mode: &str,
    child_bytes: &[u8],
) -> (
    tempfile::TempDir,
    crate::util::genesis_builder::GenesisParameters,
    String,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    std::fs::write(root.join("child.txt"), child_bytes).expect("seed child");

    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root,
        BundleEntryKind::Dir,
        dir_mode.to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    (dir, params, fs_uri)
}

/// Dir.exists reports present/absent children correctly.
/// Both calls run under the same Dir cap so the tempdir + genesis
/// setup amortize.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_exists_reports_present_and_absent() {
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", b"child content");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_exists_reports_present_and_absent
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.exists reports present + absent children",
        *test_exists_reports_present_and_absent)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_exists_reports_present_and_absent(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@rPresent <- @dir!?("exists", "child.txt")) {{
          for(@rAbsent <- @dir!?("exists", "no-such-file.txt")) {{
            match [rPresent, rAbsent] {{
              [[true, true], [true, false]] => {{
                rhoSpec!("assert", (true, "==", true),
                  "exists reports present + absent correctly", *ackCh)
              }}
              _ => {{
                rhoSpec!("assert", ([rPresent, rAbsent], "==",
                  "[[true, true], [true, false]]"),
                  "exists reports present + absent correctly", *ackCh)
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
        CompiledRholangSource::new(test_source, HashMap::new(), "DirExistsSpec".to_string())
            .expect("compile dir_exists spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("dir_exists spec failed");
}

/// Dir.openFile mode attenuation: an "r"-mode Dir refuses to yield
/// a "rw"-mode File child.  Pins Dir.rho line 220's
/// `"requested mode exceeds Dir attenuation"` gate.  Regression
/// would let a subordinate escalate mode by requesting "rw" on a
/// read-only Dir cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_openfile_mode_attenuation() {
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", b"child content");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_mode_attenuation
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.openFile('child', 'rw') on r-Dir returns FSERR_UNSUPPORTED",
        *test_openfile_mode_attenuation)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_openfile_mode_attenuation(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@r <- @dir!?("openFile", "child.txt", "rw")) {{
          match r {{
            [false, "FSERR_UNSUPPORTED", _] => {{
              rhoSpec!("assert", (true, "==", true),
                "Dir.openFile('child', 'rw') on r-Dir gates with FSERR_UNSUPPORTED",
                *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, _]"),
                "Dir.openFile('child', 'rw') on r-Dir gates with FSERR_UNSUPPORTED",
                *ackCh)
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
        "DirOpenFileAttenuationSpec".to_string(),
    )
    .expect("compile dir_openfile_attenuation spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("dir_openfile_attenuation spec failed");
}

/// Dir.openFile(rel, "r") + File.readN roundtrip on the returned
/// child cap.  Exercises the openFileImpl(canonRoot=dir, subPath="",
/// rel="child.txt") chain from Dir plus the File.readN native
/// dispatch, and verifies the read bytes match the seeded content
/// exactly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_openfile_child_readn_roundtrip() {
    let child_bytes = b"hello dir spec"; // 14 bytes
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", child_bytes);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_dir_openfile_readn_roundtrip
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.openFile('child.txt') + readN yields seeded content",
        *test_dir_openfile_readn_roundtrip)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_dir_openfile_readn_roundtrip(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@[true, file] <- @dir!?("openFile", "child.txt", "r")) {{
          for(@[true, bytes] <- @file!?("readN", 64)) {{
            // "hello dir spec".toUtf8Bytes() as hex.
            rhoSpec!("assert",
              (bytes, "==", "68656c6c6f2064697220737065".hexToBytes() ++ "63".hexToBytes()),
              "readN on Dir-opened child returns seeded content", *ackCh)
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
        "DirOpenFileReadNSpec".to_string(),
    )
    .expect("compile dir_openfile_readn spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("dir_openfile_readn spec failed");
}
