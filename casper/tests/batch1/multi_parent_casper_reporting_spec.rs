// See casper/src/main/scala/coop/rchain/casper/batch1/MultiParentCasperReportingSpec.scala

// ReportingCasper IS ported to Rust (casper/src/rust/reporting_casper.rs): the `ReportingCasper`
// trait, the real `RhoReporterCasper` implementation, `ReportingRuntime`, the `trace` method, and
// the `rho_reporter` factory all exist and are wired into the node's block-report API
// (node/src/rust/runtime/setup.rs, casper/src/rust/api/block_report_api.rs). This test exercises the
// invariant the Scala spec checked: replaying a block through the reporting runtime reproduces
// exactly the same post-state as the block produced under normal multi-parent execution — that is
// what "behaves the same way as multi-parent casper" means.

use casper::rust::reporting_casper;
use casper::rust::util::construct_deploy;
use rholang::rust::interpreter::external_services::ExternalServices;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_test_rnode_store_manager_shared;

#[tokio::test]
async fn reporting_casper_should_behave_the_same_way_as_multi_parent_casper() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");

    let correct_rholang = r#" for(@a <- @"1"){ Nil } | @"1"!("x") "#;

    let deploy = construct_deploy::source_deploy_now(
        correct_rholang.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");

    // Add the block via the normal (multi-parent) path. `add_block_from_deploys` runs the block
    // through the full validation pipeline (which itself replays and checks the post-state), so the
    // returned block's recorded post-state is, by construction, reproducible by replay.
    let signed_block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");

    // The node shares its RSpace history under `genesis.rspace_scope_id`. Open a store manager on the
    // same scope to hand the reporting runtime the committed pre-state (the genesis post-state) that
    // it resets to before replaying the block's deploys.
    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("Failed to open shared RSpace stores");

    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        ExternalServices::noop(),
    );

    let replay = reporter
        .trace(&signed_block)
        .await
        .expect("ReportingCasper::trace failed");

    // "Behaves the same way as multi-parent casper": the reporting replay reproduces the exact
    // post-state hash the block recorded under normal execution.
    assert_eq!(
        replay.post_state_hash,
        signed_block.body.state.post_state_hash.to_vec(),
        "reporting replay post-state must equal the block's recorded post-state"
    );

    // The single user deploy in the block is traced (one DeployReportResult).
    assert_eq!(
        replay.deploy_report_result.len(),
        1,
        "reporting should trace the one user deploy in the block"
    );
}
