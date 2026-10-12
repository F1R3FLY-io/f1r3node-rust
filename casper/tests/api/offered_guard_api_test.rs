//! G1-7 (DR-65 item 5, design §8.3): the held-capacity guard with one offer
//! per block. A member whose own code spends the funds that its settlement
//! needs becomes a charged user failure. It rolls back to its execution root
//! and pays its billable work and fee from the restored balance. Every
//! validator recomputes the guard of a committed rollback (arbitration M2).
//!
//! The module is a child of `offered_funded_api_test`, so it shares that
//! module's fixtures.

use casper::rust::block_status::BlockError;
use casper::rust::util::rholang::costacc::offered_acceptance::{
    OfferedCheckRole, OfferedUsageKind,
};
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::*;
use crate::helper::test_node::TestNode;

/// The outcomes of the held-capacity guards that `role` evaluated on `node`
/// for `offer`.
fn guards(
    node: &TestNode,
    offer: &models::casper::DeployDataProto,
    role: OfferedCheckRole,
) -> Vec<bool> {
    node.runtime_manager
        .offered_budget_usages()
        .into_iter()
        .filter_map(|record| match record.kind {
            OfferedUsageKind::HeldCapacityGuard { role: found, holds }
                if found == role && record.deploy_id == offer.deploy_id.as_ref() =>
            {
                Some(holds)
            }
            _ => None,
        })
        .collect()
}

fn vault_of(
    key: &(
        crypto::rust::private_key::PrivateKey,
        crypto::rust::public_key::PublicKey,
    ),
) -> VaultAddress {
    VaultAddress::from_public_key(&key.1).expect("a public key has a vault address")
}

/// A self-drain (design §12): the deploy moves all but one atomic unit of
/// its own funding vault away. The guard on the success path fails, so the
/// producer rolls the run back and publishes a charged user failure. Every
/// validator recomputes the guard, finds it failing, and settles the same
/// charge from the restored balance. The transfer leaves no trace. A later
/// transfer that leaves enough funds passes the guard and settles normally.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_self_drain_becomes_a_charged_user_failure_with_equal_receipts() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let owner = genesis.genesis_vaults[0].clone();
    let recipient = genesis.genesis_vaults[2].clone();
    let owner_vault = vault_of(&owner);
    let recipient_vault = vault_of(&recipient);
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let owner_before = super::vault_balance(&nodes, &genesis_root, &owner_vault).await;
    let recipient_before = super::vault_balance(&nodes, &genesis_root, &recipient_vault).await;

    let drain = owner_direct_offer(
        &owner,
        1,
        super::vault_transfer(&owner_vault, &recipient_vault, owner_before - 1),
    );
    let (block, _) = super::offered_block(&mut nodes, 0, drain.clone()).await;
    assert!(
        block.body.deploys[0].is_failed(),
        "the self-drain is published as a user failure"
    );
    let receipt =
        super::accepted_everywhere_with_one_receipt(&mut nodes, &block, &drain.deploy_id).await;
    assert!(receipt.rev_spent > 0, "the rolled-back run is charged");
    assert_eq!(
        receipt.rev_spent,
        u128::from(receipt.fresh_phlo) * u128::from(receipt.phlo_price) + receipt.fee_rev
    );
    let root = block.body.state.post_state_hash.clone();
    let owner_after = super::vault_balance(&nodes, &root, &owner_vault).await;
    assert_eq!(
        u128::from(owner_after) + receipt.rev_spent,
        u128::from(owner_before),
        "the balance is the restored balance minus the charge"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &recipient_vault).await,
        recipient_before,
        "the rolled-back transfer moved nothing"
    );
    assert_eq!(
        guards(&nodes[0], &drain, OfferedCheckRole::Play),
        vec![false],
        "the producer's guard fails once on the success path"
    );
    for (index, node) in nodes.iter().enumerate() {
        let replays = guards(node, &drain, OfferedCheckRole::Replay);
        assert!(!replays.is_empty(), "node {index} recomputes the guard");
        assert!(
            replays.iter().all(|holds| !holds),
            "node {index} confirms the guard failure: {replays:?}"
        );
    }

    let transfer = owner_direct_offer(
        &owner,
        2,
        super::vault_transfer(&owner_vault, &recipient_vault, 1_000),
    );
    let (block, _) = super::offered_block(&mut nodes, 0, transfer.clone()).await;
    assert!(!block.body.deploys[0].is_failed());
    let receipt =
        super::accepted_everywhere_with_one_receipt(&mut nodes, &block, &transfer.deploy_id).await;
    let next_root = block.body.state.post_state_hash.clone();
    assert_eq!(
        u128::from(super::vault_balance(&nodes, &next_root, &owner_vault).await)
            + 1_000
            + receipt.rev_spent,
        u128::from(owner_after),
        "a transfer that leaves enough funds settles normally"
    );
    assert_eq!(
        super::vault_balance(&nodes, &next_root, &recipient_vault).await,
        recipient_before + 1_000
    );
    assert_eq!(guards(&nodes[0], &transfer, OfferedCheckRole::Play), vec![
        true
    ]);
    assert!(
        nodes
            .iter()
            .all(|node| guards(node, &transfer, OfferedCheckRole::Replay).is_empty()),
        "replay recomputes the guard only for a committed rollback"
    );
}

/// A dishonest producer (arbitration M2). A hook on the producer makes its
/// guard fail for a deploy whose guard holds. The producer rolls the run back
/// and publishes a charged user failure. Every other validator recomputes the
/// guard, finds that it holds, and rejects the block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validators_reject_a_claimed_guard_failure_whose_guard_holds() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("a three-node network starts");
    let owner = genesis.genesis_vaults[0].clone();
    let offer = owner_direct_offer(&owner, 1, "@\"guard-honest\"!(1)".to_string());
    nodes[0]
        .runtime_manager
        .force_held_capacity_guard_failure_for_tests(&offer.deploy_id);
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    assert!(
        block.body.deploys[0].is_failed(),
        "the producer publishes a rollback"
    );
    for (index, node) in nodes.iter_mut().enumerate().skip(1) {
        let status = node
            .process_block(block.clone())
            .await
            .expect("validation completes");
        assert!(
            matches!(
                &status,
                Either::Left(BlockError::BlockException(error))
                    if error.to_string().contains("the held-capacity guard holds")
            ),
            "node {index} rejects the block: {status:?}"
        );
        assert!(!node.casper.dag_contains(&block.block_hash));
    }
    for (index, node) in nodes.iter().enumerate().skip(1) {
        let replays = guards(node, &offer, OfferedCheckRole::Replay);
        assert!(!replays.is_empty(), "node {index} recomputes the guard");
        assert!(
            replays.iter().all(|holds| *holds),
            "node {index} finds a holding guard in every replay attempt: {replays:?}"
        );
    }
}
