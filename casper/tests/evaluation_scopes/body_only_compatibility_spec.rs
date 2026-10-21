// Added by DR-134 (DR-66 items 2 and 5): only offered-funded deploys execute.
// A body-only deploy is refused at admission, and body-only play and replay
// return the compatibility error before any state reset. A genesis needs the
// active protocol-6 offered-funded policy.

use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use casper::rust::casper::Casper;
use casper::rust::errors::CasperError;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::offered_only::{
    BODY_ONLY_DEPLOYS_UNSUPPORTED, PROTOCOL_6_GENESIS_REQUIRED,
};
use models::rust::casper::protocol::casper_message::{
    ProcessedDeploy, ProcessedSystemDeploy, ProcessedUserDeploy,
};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use rholang::rust::interpreter::system_processes::BlockData;

use crate::helper::offered_deploy::selected_schedule;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn is_compatibility_error(
    result: &Result<impl std::fmt::Debug, CasperError>,
    expected: &str,
) -> bool {
    matches!(result, Err(CasperError::RuntimeError(message)) if message == expected)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_body_only_deploy_is_refused_before_any_state_reset() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a one-validator network");
    let policy = nodes[0]
        .casper
        .adopted_resource_policy
        .clone()
        .expect("the offered genesis has an adopted policy");
    let root = genesis.genesis_block.body.state.post_state_hash.clone();
    let body_only = construct_deploy::source_deploy_now(
        "@0!(0)".to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("a body-only deploy");

    assert!(matches!(
        nodes[0].casper.deploy(body_only.clone()),
        Err(CasperError::RuntimeError(message)) if message == "new deploys require offered-funded cost accounting"
    ));

    let manager = nodes[0].runtime_manager.clone();
    let play = manager
        .compute_state_with_bonds_envelopes(
            &root,
            vec![PendingDeployCandidate::Legacy(body_only.clone())],
            Vec::new(),
            BlockData::empty(),
            None,
            None,
            Some(&policy),
        )
        .await;
    assert!(
        is_compatibility_error(&play, BODY_ONLY_DEPLOYS_UNSUPPORTED),
        "play: {play:?}"
    );

    let replay = manager
        .replay_compute_state_envelopes_with_policy(
            &root,
            vec![ProcessedUserDeploy::Legacy(ProcessedDeploy::empty(
                body_only,
            ))],
            Vec::<ProcessedSystemDeploy>::new(),
            &BlockData::empty(),
            None,
            false,
            Some(&policy),
            Some(&root),
        )
        .await;
    assert!(
        is_compatibility_error(&replay, BODY_ONLY_DEPLOYS_UNSUPPORTED),
        "replay: {replay:?}"
    );

    nodes[0].allow_empty_blocks = true;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("a block without user deploys still builds on the same root");
    assert_eq!(block.body.state.pre_state_hash, root);
}

#[tokio::test]
async fn genesis_creation_requires_the_active_protocol_6_policy() {
    let context = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None).2;
    let node = TestNode::standalone(context)
        .await
        .expect("a standalone node");

    let without_policy =
        Genesis::create_genesis_block_with_policy(&node.runtime_manager, &parameters, None).await;
    assert!(is_compatibility_error(
        &without_policy,
        PROTOCOL_6_GENESIS_REQUIRED
    ));

    let inactive = PhloGenesisPolicy::from_schedule(&selected_schedule())
        .expect("the selected schedule forms a policy");
    let with_inactive = Genesis::create_genesis_block_with_policy(
        &node.runtime_manager,
        &parameters,
        Some(&inactive),
    )
    .await;
    assert!(is_compatibility_error(
        &with_inactive,
        PROTOCOL_6_GENESIS_REQUIRED
    ));
}
