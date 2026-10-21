//! Added by DR-134 (DR-65, gap G2): several offered deploys in one block, end
//! to end through the proposer and validators. A batch settles in canonical
//! order, each member from the settled root of the member before it. A funding
//! group whose holds do not fit its supply at R0 is rejected entirely. A member
//! whose own code drains the room that a later member holds becomes a charged
//! user failure (LCE 8.3), and validators reject a producer that ignores the
//! later holds (O-13).
//!
//! The module is a child of `offered_funded_api_test`, so it shares that
//! module's fixtures.

use casper::rust::util::rholang::costacc::offered_acceptance::{
    OfferedCheckRole, OfferedUsageKind,
};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::*;
use crate::helper::offered_deploy::owner_offer_limited;
use crate::helper::test_node::TestNode;

fn vault_of(key: &(PrivateKey, PublicKey)) -> VaultAddress {
    VaultAddress::from_public_key(&key.1).expect("a public key has a vault address")
}

/// A key with no genesis vault.
fn fresh_key(seed: u8) -> (PrivateKey, PublicKey) {
    let secret = PrivateKey::from_bytes(&[seed; 32]);
    let public = Secp256k1.to_public(&secret);
    (secret, public)
}

/// A transfer of `amount` from the deployer's vault `from` to `to`, which the
/// term creates first, because a transfer to an unregistered vault never
/// answers.
fn fund_vault(from: &VaultAddress, to: &VaultAddress, amount: u64) -> String {
    format!(
        r#"new rl(`rho:registry:lookup`), systemVaultCh, vaultCh, targetCh, authKeyCh, resultCh,
          deployerId(`rho:system:deployerId`) in {{
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {{
            @systemVault!("find", "{}", *vaultCh) |
            @systemVault!("findOrCreate", "{}", *targetCh) |
            @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
            for (@(true, vault) <- vaultCh & @(true, _) <- targetCh & key <- authKeyCh) {{
              @vault!("transfer", "{}", {amount}, *key, *resultCh) |
              for (@result <- resultCh) {{ @"fund-result"!(result) }}
            }}
          }}
        }}"#,
        from.to_base58(),
        to.to_base58(),
        to.to_base58(),
    )
}

/// Submits `offers` to node 0, creates one block, and has every node process
/// it. The block carries the offers in canonical order.
async fn batch_block(
    nodes: &mut [TestNode],
    offers: &[models::casper::DeployDataProto],
) -> models::rust::casper::protocol::casper_message::BlockMessage {
    for offer in offers {
        BlockAPI::deploy_offered(&nodes[0].engine_cell, offer.clone(), &None, false, "root")
            .await
            .expect("a well-formed offer is admitted");
    }
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the batch is proposed");
    for node in nodes.iter_mut() {
        assert!(
            matches!(
                node.process_block(block.clone())
                    .await
                    .expect("the block processes"),
                Either::Right(_)
            ),
            "every node accepts the batch"
        );
    }
    block
}

/// The receipt of `id` on every node, which must agree.
async fn agreed_receipt(
    nodes: &[TestNode],
    id: &[u8],
) -> casper::rust::api::block_api::OfferedSettlementReceipt {
    let mut receipts = Vec::with_capacity(nodes.len());
    for node in nodes {
        receipts.push(
            BlockAPI::find_offered_settlement_receipt(&node.engine_cell, &id.to_vec())
                .await
                .expect("the receipt query succeeds")
                .expect("every node holds the receipt"),
        );
    }
    assert!(receipts.windows(2).all(|pair| pair[0] == pair[1]));
    receipts.remove(0)
}

/// Funds a fresh key with `amount` in one block and returns the key.
async fn funded_fresh_key(
    nodes: &mut [TestNode],
    genesis: &crate::util::genesis_builder::GenesisContext,
    seed: u8,
    amount: u64,
) -> (PrivateKey, PublicKey) {
    let funder = genesis.genesis_vaults[0].clone();
    let fresh = fresh_key(seed);
    let funding = owner_direct_offer(
        &funder,
        1,
        fund_vault(&vault_of(&funder), &vault_of(&fresh), amount),
    );
    let block = batch_block(nodes, std::slice::from_ref(&funding)).await;
    assert!(
        !block.body.deploys[0].is_failed(),
        "the funding transfer succeeds"
    );
    assert_eq!(
        super::vault_balance(nodes, &block.body.state.post_state_hash, &vault_of(&fresh)).await,
        amount
    );
    fresh
}

/// Two offers of one signer share its purse, so they form one group. Both
/// settle in one block, in canonical order, and every node agrees.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_same_signer_pair_settles_in_one_block() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let owner = genesis.genesis_vaults[0].clone();
    let first = owner_direct_offer(&owner, 1, "@\"pair-a\"!(1)".to_string());
    let second = owner_direct_offer(&owner, 2, "@\"pair-b\"!(2)".to_string());
    let block = batch_block(&mut nodes, &[second.clone(), first.clone()]).await;
    assert_eq!(block.body.deploys.len(), 2);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        first.deploy_id.as_ref()
    );
    assert_eq!(
        block.body.deploys[1].identity_bytes(),
        second.deploy_id.as_ref()
    );
    assert!(block.body.deploys.iter().all(|deploy| !deploy.is_failed()));
    let first_receipt = agreed_receipt(&nodes, &first.deploy_id).await;
    let second_receipt = agreed_receipt(&nodes, &second.deploy_id).await;
    assert!(first_receipt.rev_spent > 0 && second_receipt.rev_spent > 0);
}

/// Offers of different signers share no purse. Both settle in one block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offers_of_different_signers_settle_in_one_block() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let first = owner_direct_offer(
        &genesis.genesis_vaults[0],
        1,
        "@\"apart-a\"!(1)".to_string(),
    );
    // The key of genesis_vaults[1] is the registry key, which BlockAPI refuses
    // as a system key, so the second signer is genesis_vaults[2].
    // let second = owner_direct_offer(&genesis.genesis_vaults[1], 2, "@\"apart-b\"!(2)".to_string());
    let second = owner_direct_offer(
        &genesis.genesis_vaults[2],
        2,
        "@\"apart-b\"!(2)".to_string(),
    );
    let block = batch_block(&mut nodes, &[first.clone(), second.clone()]).await;
    assert_eq!(block.body.deploys.len(), 2);
    agreed_receipt(&nodes, &first.deploy_id).await;
    agreed_receipt(&nodes, &second.deploy_id).await;
}

/// DR-65 item 4: each offer holds its limit (an incomplete estimate) and fits
/// the purse alone, and the two holds together exceed it. The group is
/// rejected entirely before any execution, with no charge, and both offers
/// are quarantined at the block pre-state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_over_supply_group_is_rejected_entirely_with_no_charge() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    // Each hold is 100,000 · 2 + 1 = 200,001; the supply is 300,000.
    let fresh = funded_fresh_key(&mut nodes, &genesis, 0x61, 300_000).await;
    let first = owner_offer_limited(
        &fresh,
        10,
        "@\"supply-a\"!(1)".to_string(),
        0,
        "root".to_string(),
        100_000,
    );
    let second = owner_offer_limited(
        &fresh,
        11,
        "@\"supply-b\"!(2)".to_string(),
        0,
        "root".to_string(),
        100_000,
    );
    for offer in [&first, &second] {
        BlockAPI::deploy_offered(&nodes[0].engine_cell, offer.clone(), &None, false, "root")
            .await
            .expect("a well-formed offer is admitted");
    }
    // `latest_block_number` is the largest height plus 1, the number of the
    // block that the proposer attempts, and the quarantine records it.
    let next_block = nodes[0]
        .casper
        .block_dag()
        .await
        .expect("the DAG reads")
        .latest_block_number();
    let result = nodes[0].create_block_unsafe(&[]).await;
    assert!(
        result.is_err(),
        "the rejected group leaves no deploy to propose"
    );
    let storage = nodes[0].deploy_storage.lock();
    for offer in [&first, &second] {
        let id = models::rust::deploy_id::DeployIdV6::try_from(offer.deploy_id.to_vec())
            .expect("an offered deploy id has 32 bytes");
        let rejection = storage
            .envelope_rejection(&id)
            .expect("each member of the group is quarantined");
        assert!(
            rejection.reason.contains("insufficient phlogiston"),
            "{}",
            rejection.reason
        );
        // assert_eq!(rejection.block_number, pre_state + 1);
        assert_eq!(rejection.block_number, next_block);
    }
}

/// The purse of the drain example of LCE 8.3, with amounts that fit the
/// measured use of the vault transfer (about 449,161 phlo, so a charge of
/// about 898,323 at price 2). The purse holds 3,000,000 at R0.
const DRAIN_SUPPLY: u64 = 3_000_000;
/// The first member moves 1,500,000 out of the purse. Its limit of 700,000
/// covers the transfer, and it holds 700,000 · 2 + 1 = 1,400,001.
const DRAIN_AMOUNT: u64 = 1_500_000;
const DRAIN_LIMIT: u64 = 700_000;
/// The second member holds 500,000 · 2 + 1 = 1,000,001. The holds fit the
/// supply together (2,400,002), so the group is accepted.
const LATER_LIMIT: u64 = 500_000;

/// LCE 8.3, with the amounts above. After its run the first member leaves
/// 1,500,000, which covers its charge (about 898,323) but not its charge plus
/// the later hold of 1,000,001. So its guard fails: it becomes a charged user
/// failure, the drain moves nothing, and the second member settles. Without
/// the later member the guard would hold (1,500,000 − 898,323 ≥ 0). The first
/// draft used the amounts of the design example with a limit of 250,000,
/// which the transfer exceeds, so the drain failed on its own limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_drain_into_a_later_hold_becomes_a_charged_user_failure() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let fresh = funded_fresh_key(&mut nodes, &genesis, 0x62, DRAIN_SUPPLY).await;
    let recipient = genesis.genesis_vaults[2].clone();
    let fresh_vault = vault_of(&fresh);
    let recipient_vault = vault_of(&recipient);
    let drain = owner_offer_limited(
        &fresh,
        10,
        super::vault_transfer(&fresh_vault, &recipient_vault, DRAIN_AMOUNT),
        0,
        "root".to_string(),
        DRAIN_LIMIT,
    );
    let later = owner_offer_limited(
        &fresh,
        11,
        "@\"later\"!(1)".to_string(),
        0,
        "root".to_string(),
        LATER_LIMIT,
    );
    let root_before = nodes[0]
        .casper
        .block_dag()
        .await
        .expect("the DAG reads")
        .latest_block_number();
    let block = batch_block(&mut nodes, &[drain.clone(), later.clone()]).await;
    assert_eq!(
        block.body.deploys.len(),
        2,
        "both members ship (root {root_before})"
    );
    assert!(
        block.body.deploys[0].is_failed(),
        "the drain is a charged user failure"
    );
    assert!(
        !block.body.deploys[1].is_failed(),
        "the later member settles"
    );
    let drain_receipt = agreed_receipt(&nodes, &drain.deploy_id).await;
    let later_receipt = agreed_receipt(&nodes, &later.deploy_id).await;
    assert!(
        drain_receipt.rev_spent > 0,
        "the rolled-back drain is charged"
    );
    let root = block.body.state.post_state_hash.clone();
    assert_eq!(
        u128::from(super::vault_balance(&nodes, &root, &fresh_vault).await)
            + drain_receipt.rev_spent
            + later_receipt.rev_spent,
        u128::from(DRAIN_SUPPLY),
        "the drain moved nothing, and the purse paid both charges"
    );
    assert_eq!(
        guards(&nodes[0], &drain, OfferedCheckRole::Play),
        vec![false],
        "the producer's guard of the drain fails"
    );
}

/// O-13: a producer whose guard ignores the later holds commits the drain of
/// the 8.3 example as a success. Validators recompute the guard of a committed
/// success with later holds, find it failing, and reject the block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validators_reject_a_success_whose_guard_fails_with_the_later_holds() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let fresh = funded_fresh_key(&mut nodes, &genesis, 0x63, DRAIN_SUPPLY).await;
    let recipient = genesis.genesis_vaults[2].clone();
    let drain = owner_offer_limited(
        &fresh,
        10,
        super::vault_transfer(&vault_of(&fresh), &vault_of(&recipient), DRAIN_AMOUNT),
        0,
        "root".to_string(),
        DRAIN_LIMIT,
    );
    let later = owner_offer_limited(
        &fresh,
        11,
        "@\"later\"!(1)".to_string(),
        0,
        "root".to_string(),
        LATER_LIMIT,
    );
    nodes[0]
        .runtime_manager
        .ignore_later_holds_for_tests(drain.deploy_id.as_ref());
    for offer in [&drain, &later] {
        BlockAPI::deploy_offered(&nodes[0].engine_cell, offer.clone(), &None, false, "root")
            .await
            .expect("a well-formed offer is admitted");
    }
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the dishonest producer proposes");
    assert!(
        !block.body.deploys[0].is_failed(),
        "the producer commits the drain as a success"
    );
    let verdict = nodes[1]
        .process_block(block.clone())
        .await
        .expect("the validator processes the block");
    assert!(
        matches!(verdict, Either::Left(_)),
        "the validator rejects the block: {verdict:?}"
    );
}

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
