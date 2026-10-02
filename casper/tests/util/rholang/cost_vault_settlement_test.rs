use std::num::NonZeroUsize;

use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::rholang::costacc::monetary_cursor;
use casper::rust::util::rholang::costacc::vault_cost_deploy::{
    ApplyCostDeploy, ApplyPhloCostDeploy, VaultAllocation, VaultSettlement,
};
use casper::rust::util::rholang::costacc::vault_payer::balance_query_source;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy_result::SystemDeployResult;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryCursor, MonetaryCursorTransition,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::rholang::resources::with_runtime_manager;

async fn balance(manager: &RuntimeManager, root: &StateHash, address: &VaultAddress) -> i64 {
    let (values, _) = manager
        .play_exploratory_deploy(balance_query_source(address), root, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn cursor(
    manager: &RuntimeManager,
    root: &StateHash,
    scope: [u8; 32],
) -> Option<MonetaryCursor> {
    let (values, _) = manager
        .play_exploratory_deploy(monetary_cursor::query_source(&scope), root, None)
        .await
        .unwrap();
    monetary_cursor::decode_snapshot(&values, NonZeroUsize::new(2).unwrap()).unwrap()
}

fn request(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    revision: i64,
    position: i64,
) -> ApplyPhloCostDeploy {
    let count = NonZeroUsize::new(2).unwrap();
    let expected = MonetaryCursor::new(revision, position, count).unwrap();
    ApplyPhloCostDeploy::new(
        ApplyCostDeploy::new(
            [0xe1; 32],
            vec![VaultAllocation::new(payer.to_base58(), 5).unwrap()],
            vec![VaultSettlement::new(payer.to_base58(), 2, 1).unwrap()],
            recipient.to_base58(),
            Blake2b512Random::create_from_bytes(&[0xe1]),
        )
        .unwrap(),
        Some(MonetaryCursorTransition::new([0xe2; 32], expected, 1, count).unwrap()),
        Some(MonetaryCursorTransition::new([0xe3; 32], expected, 1, count).unwrap()),
        count,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn general_wallet_cost_settlement_advances_both_cursors_once() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xd1; 32] });
        let before = balance(&manager, &initial, &payer).await;
        assert_eq!(cursor(&manager, &initial, [0xe2; 32]).await, None);
        assert_eq!(cursor(&manager, &initial, [0xe3; 32]).await, None);
        let mut runtime = RuntimeOps::new(manager.spawn_runtime().await);
        let first = match runtime
            .play_system_deploy(&initial, &mut request(&payer, &recipient, 0, 0))
            .await
            .unwrap()
        {
            SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
            SystemDeployResult::PlayFailed { .. } => panic!("first settlement failed"),
        };
        assert_eq!(balance(&manager, &first, &payer).await, before - 3);
        assert_eq!(balance(&manager, &first, &recipient).await, 1);
        let advanced = Some(MonetaryCursor::new(1, 1, NonZeroUsize::new(2).unwrap()).unwrap());
        assert_eq!(cursor(&manager, &first, [0xe2; 32]).await, advanced);
        assert_eq!(cursor(&manager, &first, [0xe3; 32]).await, advanced);
        assert!(matches!(
            runtime
                .play_system_deploy(&first, &mut request(&payer, &recipient, 0, 0))
                .await
                .unwrap(),
            SystemDeployResult::PlayFailed { .. }
        ));
        let failed_root = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost();
        assert_eq!(balance(&manager, &failed_root, &payer).await, before - 3);
        assert_eq!(balance(&manager, &failed_root, &recipient).await, 1);
        assert_eq!(cursor(&manager, &failed_root, [0xe2; 32]).await, advanced);
        assert_eq!(cursor(&manager, &failed_root, [0xe3; 32]).await, advanced);
    })
    .await
    .unwrap();
}
