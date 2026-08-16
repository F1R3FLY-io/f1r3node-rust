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
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::{
    generate_scope_id, mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_shared,
};

#[tokio::test]
async fn reporting_casper_should_behave_the_same_way_as_multi_parent_casper() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to create standalone node");

    let first = construct_deploy::source_deploy_now(
        r#" for(@a <- @"1"){ Nil } | @"1"!("x") "#.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");
    let second = construct_deploy::source_deploy_now(
        r#" @"2"!("y") "#.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("Failed to construct deploy");

    // Add the block via the normal (multi-parent) path. `add_block_from_deploys` runs the block
    // through the full validation pipeline (which itself replays and checks the post-state), so the
    // returned block's recorded post-state is, by construction, reproducible by replay.
    let signed_block = node
        .add_block_from_deploys(&[first, second])
        .await
        .expect("Failed to add block");

    let intermediate =
        Blake2b256Hash::from_bytes_prost(&signed_block.body.deploys[1].pre_state_hash);
    let mut isolated_kvm = mk_test_rnode_store_manager_shared(generate_scope_id());
    let (isolated_runtime_manager, _) =
        mk_runtime_manager_with_history_at(&mut *isolated_kvm).await;
    let replayed_genesis = isolated_runtime_manager
        .replay_block_from_consensus_data(
            &genesis.genesis_block.body.state.pre_state_hash,
            &genesis.genesis_block,
            None,
        )
        .await
        .expect("Failed to replay genesis into isolated reporting history");
    assert_eq!(
        replayed_genesis,
        genesis.genesis_block.body.state.post_state_hash
    );
    assert!(!isolated_runtime_manager.has_root(&intermediate).unwrap());

    let rspace_store = isolated_kvm
        .r_space_stores()
        .await
        .expect("Failed to open isolated RSpace stores");

    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_store,
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

    assert_eq!(
        replay.deploy_report_result.len(),
        2,
        "reporting should trace both user deploys in the block"
    );
    assert!(isolated_runtime_manager.has_root(&intermediate).unwrap());
}
