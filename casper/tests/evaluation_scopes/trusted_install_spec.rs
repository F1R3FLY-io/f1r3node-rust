// Added by DR-134 (DR-66 scope map and verification obligations): genesis play
// and replay run in the trusted install scope, which is unmetered, carries the
// Unit signature and records cost 0. Genesis replay is symmetric with genesis
// play, and a genesis that the legacy meter produced is refused before replay.

use casper::rust::errors::CasperError;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::costacc::offered_only::LEGACY_METERED_GENESIS_UNSUPPORTED;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, ProcessedUserDeploy};
use rholang::rust::interpreter::system_processes::BlockData;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::{
    generate_scope_id, mergeable_store_from_dyn, mk_test_rnode_store_manager_shared,
};

async fn fresh_runtime_manager() -> RuntimeManager {
    let mut kvs_manager = mk_test_rnode_store_manager_shared(generate_scope_id());
    let r_store = (*kvs_manager)
        .r_space_stores()
        .await
        .expect("the RSpace store opens");
    let m_store = mergeable_store_from_dyn(&mut *kvs_manager)
        .await
        .expect("the mergeable store opens");
    RuntimeManager::create_with_store(
        r_store,
        m_store,
        Genesis::default_mergeable_tags_arc(),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    )
}

#[tokio::test]
async fn genesis_deploys_run_in_the_trusted_install_scope_and_record_cost_zero() {
    let context = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let genesis = &context.genesis_block;
    assert!(!genesis.body.deploys.is_empty());
    for deploy in &genesis.body.deploys {
        assert_eq!(deploy.cost().cost, 0, "a genesis deploy records cost 0");
        assert!(!deploy.is_failed(), "a genesis deploy succeeds");
    }

    let manager = fresh_runtime_manager().await;
    let replayed = manager
        .replay_compute_state_envelopes(
            &RuntimeManager::empty_state_hash_fixed(),
            genesis.body.deploys.clone(),
            Vec::<ProcessedSystemDeploy>::new(),
            &BlockData::from_block(genesis),
            None,
            true,
        )
        .await
        .expect("genesis replay is symmetric with genesis play");
    assert_eq!(replayed, genesis.body.state.post_state_hash);
}

#[tokio::test]
async fn a_genesis_that_records_a_legacy_cost_is_refused_before_replay() {
    let context = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let genesis = &context.genesis_block;
    let mut deploys = genesis.body.deploys.clone();
    let ProcessedUserDeploy::Legacy(first) = &mut deploys[0] else {
        panic!("a genesis deploy is a body-format deploy");
    };
    first.cost = PCost { cost: 1 };

    let manager = fresh_runtime_manager().await;
    let error = manager
        .replay_compute_state_envelopes(
            &RuntimeManager::empty_state_hash_fixed(),
            deploys,
            Vec::<ProcessedSystemDeploy>::new(),
            &BlockData::from_block(genesis),
            None,
            true,
        )
        .await
        .expect_err("a legacy-metered genesis is historical execution");
    assert!(
        matches!(&error, CasperError::RuntimeError(message) if message == LEGACY_METERED_GENESIS_UNSUPPORTED),
        "expected the compatibility error, got {error:?}"
    );
}
