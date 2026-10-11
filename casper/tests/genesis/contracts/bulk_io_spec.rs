use std::collections::HashMap;
use std::path::{Path, PathBuf};

use casper::rust::genesis::contracts::fs_genesis::{
    bulkio_versioned_uri, BundleConsensusMode, BundleEntry, BundleEntryKind,
    BULK_IMPORT_LOGICAL_NAME,
};
use casper::rust::genesis::contracts::standard_deploys;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::io::bulk::{hex_root, read_sealed_root, seal_tree, staging_path};

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::{GenesisBuilder, DEFAULT_VALIDATOR_PKS};

fn validator_hex(i: usize) -> String { hex::encode(DEFAULT_VALIDATOR_PKS[i].bytes.clone()) }

fn stage_tree(import_root: &Path, stage_id: &str, body: &[u8]) -> String {
    let dir = staging_path(import_root, stage_id);
    std::fs::create_dir_all(dir.join("data/ab")).unwrap();
    std::fs::write(dir.join("data/ab/part-00000.rec"), body).unwrap();
    std::fs::write(dir.join("PROVENANCE"), b"{}\n").unwrap();
    hex_root(&seal_tree(&dir).unwrap())
}

fn provisioned(
    mode: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    crate::util::genesis_builder::GenesisParameters,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let entry = BundleEntry::try_new(
        BULK_IMPORT_LOGICAL_NAME.to_string(),
        root.clone(),
        BundleEntryKind::Dir,
        mode.to_string(),
        BundleConsensusMode::Consensus,
    )
    .unwrap();
    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];
    (dir, root, params)
}

fn spec_source(tests: &str, body: &str) -> String {
    let uri = bulkio_versioned_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    format!(
        r#"
new
  rl(`rho:registry:lookup`),
  v1Api(`rho:registry:v1:internal`),
  mkDid(`rho:test:deployerId:make`),
  RhoSpecCh, bulkCh, theTest
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite", [({tests}, *theTest)])
  }} |
  v1Api!("lookupVersion", "{uri}", Nil, *bulkCh) |
  for(@bulkIo <- bulkCh) {{
    contract theTest(rhoSpec, _, ackCh) = {{
      new d0, d1, d2, d3, dx in {{
        mkDid!("deployerId", "{v0}".hexToBytes(), *d0) |
        mkDid!("deployerId", "{v1}".hexToBytes(), *d1) |
        mkDid!("deployerId", "{v2}".hexToBytes(), *d2) |
        mkDid!("deployerId", "{v3}".hexToBytes(), *d3) |
        mkDid!("deployerId", "{vx}".hexToBytes(), *dx) |
        for(@v0 <- d0 & @v1 <- d1 & @v2 <- d2 & @v3 <- d3 & @vx <- dx) {{
          {body}
        }}
      }}
    }}
  }}
}}
"#,
        tests = format!("\"{tests}\""),
        v0 = validator_hex(0),
        v1 = validator_hex(1),
        v2 = validator_hex(2),
        v3 = validator_hex(3),
        vx = hex::encode(standard_deploys::FS_GENERATOR_PUB_KEY.bytes.clone()),
    )
}

fn spec_record(sid: &str, ns: &str) -> String {
    format!(
        r#"{{"stageId": "{sid}", "namespace": "{ns}", "sourceRoot": "{h}", "manifestHash": "{h}", "transform": "tabular/1", "sources": [], "expiresIn": 100}}"#,
        h = "cd".repeat(32)
    )
}

async fn run(name: &str, source: String, params: crate::util::genesis_builder::GenesisParameters) {
    let compiled =
        CompiledRholangSource::new(source, HashMap::new(), name.to_string()).expect("compile");
    RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params)
        .run_tests()
        .await
        .unwrap_or_else(|e| panic!("{name} failed: {e:?}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stage_attest_commit_swaps_tree_into_place() {
    let (_tmp, root, params) = provisioned("r");
    let sid = "11".repeat(32);
    let result = stage_tree(&root, &sid, b"F1BK\x01hello");
    let ns = "demo/people";
    let rec = spec_record(&sid, ns);
    let body = format!(
        r#"
          for(@s1 <- @bulkIo!?("stage", v0, {rec});
              @s2 <- @bulkIo!?("stage", v1, {rec});
              @sx <- @bulkIo!?("stage", vx, {spec_x});
              @c0 <- @bulkIo!?("commit", v0, "{sid}");
              @a0 <- @bulkIo!?("attest", v0, "{sid}", "{result}");
              @a0again <- @bulkIo!?("attest", v0, "{sid}", "{result}");
              @ax <- @bulkIo!?("attest", vx, "{sid}", "{result}");
              @a1 <- @bulkIo!?("attest", v1, "{sid}", "{result}");
              @a2 <- @bulkIo!?("attest", v2, "{sid}", "{result}");
              @c1 <- @bulkIo!?("commit", v1, "{sid}");
              @a3 <- @bulkIo!?("attest", v3, "{sid}", "{result}");
              @cx <- @bulkIo!?("commit", vx, "{sid}");
              @c2 <- @bulkIo!?("commit", v2, "{sid}");
              @c3 <- @bulkIo!?("commit", v3, "{sid}");
              @r <- @bulkIo!?("root", "{ns}");
              @st <- @bulkIo!?("status", "{sid}")) {{
            match [s1, s2, sx, c0, a0, a0again, ax, a1, a2, c1, a3, cx, c2, c3, r, st] {{
              [[true, _], [false, "FSERR_ALREADY_EXISTS", _], [false, "FSERR_PERM", _],
               [false, "FSERR_BUSY", _], [true, 1], [false, "FSERR_ALREADY_EXISTS", _],
               [false, "FSERR_PERM", _], [true, 2], [true, 3], [false, "FSERR_BUSY", _],
               [true, 4], [false, "FSERR_PERM", _], [true, "{result}"],
               [false, "FSERR_BUSY", _], [true, "{result}"], [true, {{"status": "committed" ..._}}]] => {{
                rhoSpec!("assert", (true, "==", true), "bulk stage/attest/commit lifecycle", *ackCh)
              }}
              other => {{
                rhoSpec!("assert", (other, "==", "expected lifecycle replies"), "bulk stage/attest/commit lifecycle", *ackCh)
              }}
            }}
          }}
        "#,
        spec_x = spec_record(&"22".repeat(32), "demo/other"),
    );
    run(
        "BulkIoLifecycleSpec",
        spec_source("bulk lifecycle", &body),
        params,
    )
    .await;
    let target = root.join(ns);
    assert_eq!(hex_root(&read_sealed_root(&target).unwrap()), result);
    assert!(!staging_path(&root, &sid).exists());
    assert_eq!(
        std::fs::read(target.join("data/ab/part-00000.rec")).unwrap(),
        b"F1BK\x01hello"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conflicting_attestation_blocks_commit() {
    let (_tmp, root, params) = provisioned("r");
    let sid = "33".repeat(32);
    let result = stage_tree(&root, &sid, b"F1BK\x01x");
    let other = "44".repeat(32);
    let rec = spec_record(&sid, "demo/conflict");
    let body = format!(
        r#"
          for(@_ <- @bulkIo!?("stage", v0, {rec});
              @_ <- @bulkIo!?("attest", v0, "{sid}", "{result}");
              @_ <- @bulkIo!?("attest", v1, "{sid}", "{result}");
              @_ <- @bulkIo!?("attest", v2, "{sid}", "{result}");
              @_ <- @bulkIo!?("attest", v3, "{sid}", "{other}");
              @c <- @bulkIo!?("commit", v0, "{sid}");
              @bad <- @bulkIo!?("attest", v1, "{sid}", "NOTHEX");
              @ab <- @bulkIo!?("abort", v1, "{sid}");
              @ab0 <- @bulkIo!?("abort", v0, "{sid}");
              @r <- @bulkIo!?("root", "demo/conflict")) {{
            match [c, bad, ab, ab0, r] {{
              [[false, "FSERR_BUSY", "attestations missing or in conflict"],
               [false, "FSERR_BAD_ARG", _], [false, "FSERR_PERM", _], [true, "aborted"],
               [true, "{zero}"]] => {{
                rhoSpec!("assert", (true, "==", true), "conflict blocks commit", *ackCh)
              }}
              x => {{
                rhoSpec!("assert", (x, "==", "expected conflict replies"), "conflict blocks commit", *ackCh)
              }}
            }}
          }}
        "#,
        zero = "0".repeat(64),
    );
    run(
        "BulkIoConflictSpec",
        spec_source("bulk conflict", &body),
        params,
    )
    .await;
    assert!(!root.join("demo/conflict").exists());
    assert!(staging_path(&root, &sid).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writable_import_root_disables_bulk_import() {
    let (_tmp, _root, params) = provisioned("rw");
    let sid = "55".repeat(32);
    let rec = spec_record(&sid, "demo/x");
    let body = format!(
        r#"
          for(@s <- @bulkIo!?("stage", v0, {rec})) {{
            match s {{
              [false, "FSERR_UNSUPPORTED", _] => {{
                rhoSpec!("assert", (true, "==", true), "rw import root is refused", *ackCh)
              }}
              x => {{
                rhoSpec!("assert", (x, "==", "FSERR_UNSUPPORTED"), "rw import root is refused", *ackCh)
              }}
            }}
          }}
        "#
    );
    run(
        "BulkIoDisabledSpec",
        spec_source("bulk disabled", &body),
        params,
    )
    .await;
}
