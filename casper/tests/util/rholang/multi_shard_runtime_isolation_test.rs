use casper::rust::errors::CasperError;
use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::rholang::costacc::vault_cost_deploy::{
    ProtocolBurnDeploy, ProtocolMintDeploy,
};
use casper::rust::util::rholang::costacc::vault_payer::balance_query_source;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy_result::SystemDeployResult;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources::{
    generate_scope_id, genesis_context, mk_runtime_manager_with_history_at,
    mk_test_rnode_store_manager_shared,
};

async fn runtime_at_scope(scope: &str) -> RuntimeManager {
    let mut store = mk_test_rnode_store_manager_shared(scope.to_string());
    let (runtime, _) = mk_runtime_manager_with_history_at(&mut *store).await;
    runtime
}

async fn isolated_runtime(
    genesis: &GenesisContext,
    scope: &str,
) -> Result<RuntimeManager, CasperError> {
    let runtime = runtime_at_scope(scope).await;
    let post_state = runtime
        .replay_block_from_consensus_data(
            &genesis.genesis_block.body.state.pre_state_hash,
            &genesis.genesis_block,
            None,
        )
        .await?;
    assert_eq!(post_state, genesis.genesis_block.body.state.post_state_hash);
    Ok(runtime)
}

async fn balance(runtime: &RuntimeManager, state: &StateHash, address: &VaultAddress) -> i64 {
    let (values, _) = runtime
        .play_exploratory_deploy(balance_query_source(address), state, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn mint(
    runtime: &RuntimeManager,
    state: &StateHash,
    address: &VaultAddress,
    amount: i64,
    seed: u8,
) -> StateHash {
    let spawned = runtime.spawn_runtime().await;
    let mut ops = RuntimeOps::new(spawned);
    match ops
        .play_system_deploy(
            state,
            &mut ProtocolMintDeploy::new(
                address.to_base58(),
                amount,
                Blake2b512Random::create_from_bytes(&[seed]),
            )
            .unwrap(),
        )
        .await
        .unwrap()
    {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
        SystemDeployResult::PlayFailed { .. } => panic!("protocol mint failed"),
    }
}

async fn burn(
    runtime: &RuntimeManager,
    state: &StateHash,
    address: &VaultAddress,
    sender: &PublicKey,
    amount: i64,
    sequence: i32,
    seed: u8,
) -> Option<StateHash> {
    let spawned = runtime.spawn_runtime().await;
    spawned
        .set_block_data(BlockData {
            time_stamp: 0,
            block_number: 1,
            sender: sender.clone(),
            seq_num: sequence,
        })
        .await;
    let mut ops = RuntimeOps::new(spawned);
    match ops
        .play_system_deploy(
            state,
            &mut ProtocolBurnDeploy::new(
                address.to_base58(),
                amount,
                Blake2b512Random::create_from_bytes(&[seed]),
            )
            .unwrap(),
        )
        .await
        .unwrap()
    {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => Some(state_hash),
        SystemDeployResult::PlayFailed { .. } => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_shards_keep_vault_balances_roots_and_failures_isolated() {
    crate::init_logger();
    let genesis = genesis_context().await.unwrap();
    let left_genesis = genesis.clone();
    let right_genesis = genesis.clone();
    let left_scope = generate_scope_id();
    let right_scope = generate_scope_id();
    let (left, right) = tokio::try_join!(
        isolated_runtime(&left_genesis, &left_scope),
        isolated_runtime(&right_genesis, &right_scope)
    )
    .unwrap();
    let initial_state = genesis.genesis_block.body.state.post_state_hash.clone();
    let validator = genesis.validator_pks()[0].clone();
    let address = VaultAddress::from_public_key(&validator).unwrap();

    let (left_initial, right_initial) = tokio::join!(
        balance(&left, &initial_state, &address),
        balance(&right, &initial_state, &address)
    );
    assert_eq!(left_initial, right_initial);

    let (left_minted, right_minted) = tokio::join!(
        mint(&left, &initial_state, &address, 111, 0x31),
        mint(&right, &initial_state, &address, 222, 0x41)
    );
    let (left_final, right_final) = tokio::join!(
        burn(&left, &left_minted, &address, &validator, 17, 1, 0x32),
        burn(&right, &right_minted, &address, &validator, 29, 1, 0x42)
    );
    let left_final = left_final.unwrap();
    let right_final = right_final.unwrap();
    assert_ne!(left_final, right_final);

    let (left_balance, right_balance) = tokio::join!(
        balance(&left, &left_final, &address),
        balance(&right, &right_final, &address)
    );
    assert_eq!(left_balance, left_initial + 111 - 17);
    assert_eq!(right_balance, right_initial + 222 - 29);

    let left_root = Blake2b256Hash::from_bytes_prost(&left_final);
    let right_root = Blake2b256Hash::from_bytes_prost(&right_final);
    assert!(left.has_root(&left_root).unwrap());
    assert!(!left.has_root(&right_root).unwrap());
    assert!(right.has_root(&right_root).unwrap());
    assert!(!right.has_root(&left_root).unwrap());

    let overdraw = right_balance.checked_add(1).unwrap();
    let (left_topped_up, right_overdraw) = tokio::join!(
        mint(&left, &left_final, &address, 7, 0x33),
        burn(
            &right,
            &right_final,
            &address,
            &validator,
            overdraw,
            2,
            0x43
        )
    );
    assert!(right_overdraw.is_none());
    let (left_after_top_up, right_after_overdraw) = tokio::join!(
        balance(&left, &left_topped_up, &address),
        balance(&right, &right_final, &address)
    );
    assert_eq!(left_after_top_up, left_balance + 7);
    assert_eq!(right_after_overdraw, right_balance);

    drop(left);
    drop(right);
    let (left_restarted, right_restarted) = tokio::join!(
        runtime_at_scope(&left_scope),
        runtime_at_scope(&right_scope)
    );
    assert!(left_restarted.has_root(&left_root).unwrap());
    assert!(!left_restarted.has_root(&right_root).unwrap());
    assert!(right_restarted.has_root(&right_root).unwrap());
    assert!(!right_restarted.has_root(&left_root).unwrap());
    let (left_after_restart, right_after_restart) = tokio::join!(
        balance(&left_restarted, &left_topped_up, &address),
        balance(&right_restarted, &right_final, &address)
    );
    assert_eq!(left_after_restart, left_balance + 7);
    assert_eq!(right_after_restart, right_balance);
}
