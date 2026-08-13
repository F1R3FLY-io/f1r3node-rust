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
            [false, "FSERR_UNSUPPORTED", _msg] => {{
              rhoSpec!("assert", (true, "==", true),
                "chown on consensus-cap returns FSERR_UNSUPPORTED", *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, _]"),
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
