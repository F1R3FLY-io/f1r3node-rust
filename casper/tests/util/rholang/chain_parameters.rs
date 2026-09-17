#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_parameters_round_trip_through_genesis() {
    use crate::util::genesis_builder::GenesisBuilder;
    use crate::util::rholang::resources::{
        mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_from_genesis,
    };

    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4));
    parameters.2.proof_of_stake.max_parent_depth = 21;
    parameters.2.proof_of_stake.deploy_lifespan = 70;
    parameters.2.proof_of_stake.min_phlo_price = 3;

    let genesis_context = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis with consensus parameters");
    let post_state = genesis_context
        .genesis_block
        .body
        .state
        .post_state_hash
        .clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (runtime_manager, _history) = mk_runtime_manager_with_history_at(&mut *kvm).await;

    let read = runtime_manager
        .get_consensus_parameters(&post_state)
        .await
        .expect("on-chain consensus-parameters query");

    assert_eq!(
        read,
        Some((21, 70, 3)),
        "genesis must bake the consensus parameters into the PoS contract and \
         expose them via getConsensusParameters"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_parameters_genesis_rejects_out_of_range_values() {
    use crate::util::genesis_builder::GenesisBuilder;

    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4));
    parameters.2.proof_of_stake.max_parent_depth = 0;

    let result = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await;
    let err = result
        .err()
        .expect("a zero maxParentDepth must fail genesis construction");
    assert!(
        err.to_string().contains("maxParentDepth out of range"),
        "the error must name the offending parameter, got: {err}"
    );
}

async fn adopted_parameters(
    genesis_context: &crate::util::genesis_builder::GenesisContext,
    local_values: (i32, i64, i64),
) -> (i32, i64, i64) {
    use std::sync::{Arc, Mutex};

    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{hash_set_casper, CasperShardConf, MultiParentCasper};
    use casper::rust::engine::block_retriever::BlockRetriever;
    use casper::rust::estimator::Estimator;
    use comm::rust::rp::connect::{Connections, ConnectionsCell};
    use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

    use crate::engine::setup;
    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, casper_buffer_storage_from_dyn,
        key_value_deploy_storage_from_dyn, mk_runtime_manager_with_history_at,
        mk_test_rnode_store_manager_from_genesis,
    };

    let mut kvm = mk_test_rnode_store_manager_from_genesis(genesis_context);
    let (runtime_manager, _history) = mk_runtime_manager_with_history_at(&mut *kvm).await;
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let block_dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");
    let deploy_storage = key_value_deploy_storage_from_dyn(&mut *kvm)
        .await
        .expect("deploy storage");
    let casper_buffer = casper_buffer_storage_from_dyn(&mut *kvm)
        .await
        .expect("casper buffer");
    let mut buffer_kvm = InMemoryStoreManager::new();
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        KeyValueRejectedDeployBuffer::new(&mut buffer_kvm)
            .await
            .expect("rejected buffer"),
    ));

    let local_peer = setup::peer_node("adoption-local", 40400);
    let rp_conf = create_rp_conf_ask(local_peer.clone(), None, None);
    let block_retriever = BlockRetriever::new(
        casper_buffer.clone(),
        Arc::new(TransportLayerStub::new()),
        ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer]))),
        },
        rp_conf,
    );

    let mut local_conf = CasperShardConf::new();
    local_conf.max_parent_depth = local_values.0;
    local_conf.deploy_lifespan = local_values.1;
    local_conf.min_phlo_price = local_values.2;

    let casper = hash_set_casper(
        block_retriever,
        F1r3flyEvents::new(),
        Arc::new(runtime_manager),
        Estimator::apply(),
        block_store,
        block_dag_storage,
        deploy_storage,
        rejected_deploy_buffer,
        casper_buffer,
        None,
        local_conf,
        genesis_context.genesis_block.clone(),
        casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
    )
    .await
    .expect("hash_set_casper");

    let adopted = casper.casper_shard_conf();
    (
        adopted.max_parent_depth,
        adopted.deploy_lifespan,
        adopted.min_phlo_price,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_parameters_are_adopted_from_chain_over_local_config() {
    use crate::util::genesis_builder::GenesisBuilder;

    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4));
    parameters.2.proof_of_stake.max_parent_depth = 21;
    parameters.2.proof_of_stake.deploy_lifespan = 70;
    parameters.2.proof_of_stake.min_phlo_price = 3;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis with consensus parameters");
    let (first, second) = tokio::join!(
        adopted_parameters(&genesis, (15, 50, 1)),
        adopted_parameters(&genesis, (7, 90, 8)),
    );
    assert_eq!(first, (21, 70, 3));
    assert_eq!(second, first);
}
