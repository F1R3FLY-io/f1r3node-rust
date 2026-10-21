//! Added by G3 (DR-135): the network tests of owner funding (G3 plan v3b,
//! §8.2). A stored process runs for its owner, and the owner pays its share
//! at its own signed price. An interaction that an owner cannot pay does not
//! fire, and the run of the client settles without it.
//!
//! Each network test runs three nodes. Node 0 proposes each block, every
//! node processes the block and accepts it, and every node holds the same
//! settlement receipt. The tests check public behavior: the receipt rows and
//! amounts, vault balances, data and continuations at a state root, block
//! validity, and the test records of a node.
//!
//! The module is a child of `offered_funded_api_test`, so it shares that
//! module's fixtures. The helpers of `offered_gateway_slot_api_test` are
//! private to that sibling module. This module therefore builds small
//! helpers of the same form from the shared fixtures.
//!
//! Each party is a key with a funded genesis vault. Every client signs price
//! 2, the price of `owner_direct_offer`. T17 gives its owner price 3.
//!
//! Step S7 added T11, T21, T22 and the check of equal client lanes and owner
//! rooms (G-1). Step S9 added the other network tests. T13 and T23 are not in
//! this module. T13 is a unit test of step S6 (`owner_replay.rs`). T23 is a
//! runtime-level test of step S8 in `offered_batch_runtime_test.rs`, because a
//! test node has no hook that sets the play budget of its proposer.

use std::collections::BTreeSet;

use casper::rust::api::block_api::{OfferedPurseSettlement, OfferedSettlementReceipt};
use casper::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use casper::rust::util::rholang::costacc::direct_wallet_funding::{
    gate_offered_candidate, OfferedGate, OfferedGateInput,
};
use casper::rust::util::rholang::costacc::offered_acceptance::OfferedUsageKind;
use casper::rust::util::rholang::costacc::offered_grants::offered_grant_issue_call_limits;
use casper::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_production_limits, offered_funded_v6_recording_limits,
};
use casper::rust::util::rholang::costacc::vault_payer::purse_of;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::authority::compound_cost_signatures;
use rholang::rust::interpreter::accounting::owner_funding::OwnerPurseSet;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::{
    decode_native_budget_recording, encode_native_budget_recording,
};
use rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString};
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::*;
// Changed by G3 (DR-135, S11): T25 signs a limit of its own.
// use crate::helper::offered_deploy::{
//     owner_offer_priced, owner_offer_with_lanes, OfferedDeploy, PermittedLane, OFFER_PHLO_LIMIT,
//     TEST_SHARD,
// };
use crate::helper::offered_deploy::{
    owner_offer_limited, owner_offer_priced, owner_offer_with_lanes, OfferedDeploy, PermittedLane,
    OFFER_PHLO_LIMIT, TEST_SHARD,
};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisContext;

// ── Shared helpers ──────────────────────────────────────────────────────────

/// The price that `owner_direct_offer` signs. Every client of this module
/// signs its offers at this price.
const CLIENT_PRICE: u64 = 2;

/// The fixed fee of an offered deploy in atomic REV (`FIXED_FEE_REV` of the
/// acceptance relation).
const FEE_REV: u64 = 1;

/// The amount that a sponsor moves into a purse that a test funds.
const PURSE_FUNDS: u64 = 100_000;

/// The amount that a sponsor adds to a drained owner wallet. It covers many
/// owner shares of the small services of this module.
const TOP_UP: u64 = 10_000_000;

/// The largest number of dry runs that one drain calibration takes.
const CALIBRATION_RUNS: usize = 4;

/// The key of a test party. `owner_network` gives each party a funded
/// genesis vault.
fn party(seed: u8) -> (PrivateKey, PublicKey) {
    let secret = PrivateKey::from_bytes(&[seed; 32]);
    let public = Secp256k1.to_public(&secret);
    (secret, public)
}

fn vault_of(key: &(PrivateKey, PublicKey)) -> VaultAddress {
    VaultAddress::from_public_key(&key.1).expect("a public key has a vault address")
}

fn post_state(block: &BlockMessage) -> StateHash { block.body.state.post_state_hash.clone() }

/// A three-node network whose genesis funds a vault of each of `parties`
/// (`offered_v6_genesis_with_signers`).
async fn owner_network(parties: &[(PrivateKey, PublicKey)]) -> Vec<TestNode> {
    let genesis = super::offered_v6_genesis_with_signers(3, parties).await;
    TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .expect("a three-node network starts")
}

/// Proposes `offer` alone on node 0, and requires that its run settles as a
/// success. Every node processes the block and accepts it, and every node
/// holds the same receipt. Returns the block and the receipt.
async fn settle(
    nodes: &mut [TestNode],
    offer: models::casper::DeployDataProto,
) -> (BlockMessage, OfferedSettlementReceipt) {
    let (block, id) = super::offered_block(nodes, 0, offer).await;
    assert!(
        !block.body.deploys[0].is_failed(),
        "the run settles as a success"
    );
    let receipt = super::accepted_everywhere_with_one_receipt(nodes, &block, &id).await;
    (block, receipt)
}

/// The data on the public channel `name` at `root`, read on node 0. Every node
/// accepted the block of `root`, so every node holds these data.
async fn data(nodes: &[TestNode], root: &StateHash, name: &str) -> Vec<Par> {
    nodes[0]
        .runtime_manager
        .get_data(
            root.clone(),
            &new_gstring_par(name.to_owned(), Vec::new(), false),
        )
        .await
        .expect("the data at the root are readable")
}

/// The integers on the public channel `name` at `root`, in ascending order.
async fn ints(nodes: &[TestNode], root: &StateHash, name: &str) -> Vec<i64> {
    let mut values = data(nodes, root, name)
        .await
        .iter()
        .map(|par| RhoNumber::unapply(par).expect("each datum on the channel is an integer"))
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}

/// The booleans on the public channel `name` at `root`.
async fn flags(nodes: &[TestNode], root: &StateHash, name: &str) -> Vec<bool> {
    data(nodes, root, name)
        .await
        .iter()
        .map(|par| RhoBoolean::unapply(par).expect("each datum on the channel is a boolean"))
        .collect()
}

/// The number of continuations that wait on the public channel `name` alone
/// at `root`.
async fn waiting(nodes: &[TestNode], root: &StateHash, name: &str) -> usize {
    nodes[0]
        .runtime_manager
        .get_continuation(root.clone(), vec![new_gstring_par(
            name.to_owned(),
            Vec::new(),
            false,
        )])
        .await
        .expect("the continuations at the root are readable")
        .len()
}

/// The vault address that an installer published on the public channel
/// `name` at `root`.
async fn published_address(nodes: &[TestNode], root: &StateHash, name: &str) -> VaultAddress {
    let published = data(nodes, root, name).await;
    let address = published
        .first()
        .and_then(RhoString::unapply)
        .expect("the installer published a vault address");
    VaultAddress::parse(&address).expect("a published vault address parses")
}

/// The row of the purse at `address` in `receipt`, when the purse paid.
fn row<'a>(
    receipt: &'a OfferedSettlementReceipt,
    address: &VaultAddress,
) -> Option<&'a OfferedPurseSettlement> {
    let address = address.to_base58();
    receipt
        .purses
        .iter()
        .find(|purse| purse.address == address.as_bytes())
}

/// Checks that the rows of `receipt` carry its whole REV, as the gateway test
/// checks: the resource REV of the rows is the resource REV of the receipt,
/// and the fees of the rows are its fee.
fn assert_rows_add_up(receipt: &OfferedSettlementReceipt) {
    assert_eq!(
        receipt
            .purses
            .iter()
            .map(|purse| purse.resource_rev)
            .sum::<u128>(),
        receipt.rev_spent - receipt.fee_rev,
        "the rows carry the resource REV of the receipt"
    );
    assert_eq!(
        receipt
            .purses
            .iter()
            .map(|purse| purse.fee_rev)
            .sum::<u128>(),
        receipt.fee_rev,
        "the rows carry the fee of the receipt"
    );
}

/// Checks that only `client` pays in `receipt`: one row, which carries the
/// whole REV and the fee.
fn assert_client_only(receipt: &OfferedSettlementReceipt, client: &VaultAddress) {
    assert_eq!(
        receipt.purses.len(),
        1,
        "only the client pays: {:?}",
        receipt.purses
    );
    let paid = row(receipt, client).expect("the client's row is the only row");
    assert_eq!(paid.fee_rev, receipt.fee_rev, "the client pays the fee");
    assert_eq!(
        paid.resource_rev + paid.fee_rev,
        receipt.rev_spent,
        "the client pays the whole REV"
    );
}

/// Checks the row of the owner purse at `address` in `receipt`. The row pays
/// resource REV and no fee. Its post balance is the purse balance at `root`,
/// and the purse fell by the row from `before`. Returns the resource REV.
async fn owner_paid(
    nodes: &[TestNode],
    root: &StateHash,
    receipt: &OfferedSettlementReceipt,
    address: &VaultAddress,
    before: u64,
) -> u128 {
    // `OwnerSettlement::request` burns each owner debit with fee 0, and
    // `merge_owner_rows` gives an owner purse a row of its own unless a client
    // row holds the same purse (execution/settlement.rs). The owners of these
    // tests are never client payers, so each owner row is its own. The
    // settlement reads the post balance of the row at its checkpoint, after
    // the owner request (`settle_checked_wallet_inner`).
    let paid = row(receipt, address).expect("the owner purse has a row of its own");
    assert!(paid.resource_rev > 0, "the owner pays for its share");
    assert_eq!(paid.fee_rev, 0, "an owner row pays no fee");
    let after = super::vault_balance(nodes, root, address).await;
    assert_eq!(
        paid.post_balance, after,
        "the row reports the purse balance after the block"
    );
    assert_eq!(
        u128::from(after) + paid.resource_rev,
        u128::from(before),
        "the purse falls by its row"
    );
    paid.resource_rev
}

/// The REV of `receipt` when every payer pays at the client's price: the fresh
/// and retained phlo at `phlo_price`, plus the fee.
fn rev_at_client_price(receipt: &OfferedSettlementReceipt) -> u128 {
    (u128::from(receipt.fresh_phlo) + u128::from(receipt.retained_phlo))
        * u128::from(receipt.phlo_price)
        + receipt.fee_rev
}

/// A `transferBatch` from the sponsor's vault at `from` to each target, in the
/// form of the funding offer of the gateway tests. The batch creates each
/// target vault that does not exist yet (`depositTransferBatch` calls
/// `findOrCreate`).
fn transfer_batch(from: &VaultAddress, targets: &[(&VaultAddress, u64)]) -> String {
    let batch = targets
        .iter()
        .map(|(address, amount)| format!("(\"{}\", {amount})", address.to_base58()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"new rl(`rho:registry:lookup`), systemVaultCh, payerCh, authKeyCh, transferCh,
          deployerId(`rho:system:deployerId`) in {{
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {{
            @systemVault!("find", "{from}", *payerCh) |
            @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
            for (@(true, payer) <- payerCh & key <- authKeyCh) {{
              @payer!("transferBatch", [{batch}], *key, *transferCh) |
              for (@result <- transferCh) {{ @"owner-test-funding"!(result) }}
            }}
          }}
        }}"#,
        from = from.to_base58(),
    )
}

/// The sponsor funds each target purse in one block. At the root of that
/// block each target holds its amount more than at `root`. Returns the block.
async fn fund(
    nodes: &mut [TestNode],
    sponsor: &(PrivateKey, PublicKey),
    time_stamp: i64,
    root: &StateHash,
    targets: &[(&VaultAddress, u64)],
) -> BlockMessage {
    let mut before = Vec::with_capacity(targets.len());
    for (address, _) in targets {
        before.push(super::vault_balance(nodes, root, address).await);
    }
    let (block, _) = settle(
        nodes,
        owner_direct_offer(
            sponsor,
            time_stamp,
            transfer_batch(&vault_of(sponsor), targets),
        ),
    )
    .await;
    let funded = post_state(&block);
    for ((address, amount), before) in targets.iter().zip(before) {
        assert_eq!(
            super::vault_balance(nodes, &funded, address).await,
            before + amount,
            "the sponsor funds the purse"
        );
    }
    block
}

/// Moves all but `keep` atomic REV out of `owner`'s key wallet to `sink`, in
/// one owner-direct transfer at price 2, and returns the block. `root` is the
/// latest root, so the transfer runs at `root`.
///
/// The amount is calibrated. Each dry run measures the transfer of the
/// current amount at `root`, and the next amount leaves `keep` plus that
/// charge. The amount changes only when the measured charge changes, so the
/// loop stops at a fixed point after a few runs. The run then leaves exactly
/// `keep` plus its charge, so the held-capacity guard holds, and the
/// settlement leaves `keep`.
async fn drain_wallet_to(
    nodes: &mut [TestNode],
    owner: &(PrivateKey, PublicKey),
    time_stamp: i64,
    root: &StateHash,
    sink: &VaultAddress,
    keep: u64,
) -> BlockMessage {
    let wallet = vault_of(owner);
    let balance = super::vault_balance(nodes, root, &wallet).await;
    let rest = balance
        .checked_sub(keep)
        .expect("the wallet holds more than the amount that it keeps");
    let mut amount = rest;
    let mut calibrated = None;
    for _ in 0..CALIBRATION_RUNS {
        // The DR-71 dry run measures the COMMs and bytes of an owner-direct
        // offer of the same term by the same key at the same root (`dry_run.rs`,
        // module documentation). A transfer touches only vault data. Vault
        // data are DR-101 residue and have no owner purse, so no owner row
        // takes a part of the phlo. The receipt then reports that phlo as fresh
        // phlo with no retained phlo, and the transfer costs `2 * phlo + 1`
        // atomic REV. The assertion on `rev_spent` after the run checks this
        // prediction.
        let (_, phlo) = nodes[0]
            .runtime_manager
            .play_exploratory_deploy(
                super::vault_transfer(&wallet, sink, amount),
                root,
                Some(owner.1.clone()),
                nodes[0].adopted_policy(),
            )
            .await
            .expect("the dry run measures the drain");
        let charge = phlo
            .checked_mul(CLIENT_PRICE)
            .and_then(|rev| rev.checked_add(FEE_REV))
            .expect("the charge of a transfer fits u64");
        let next = rest
            .checked_sub(charge)
            .expect("the wallet covers the drain and its charge");
        if next == amount {
            calibrated = Some(charge);
            break;
        }
        amount = next;
    }
    let charge = calibrated.expect("the calibration of the drain reaches a fixed point");
    let (block, receipt) = settle(
        nodes,
        owner_direct_offer(
            owner,
            time_stamp,
            super::vault_transfer(&wallet, sink, amount),
        ),
    )
    .await;
    assert_eq!(
        receipt.rev_spent,
        u128::from(charge),
        "the calibration predicts the charge of the drain"
    );
    assert_eq!(
        super::vault_balance(nodes, &post_state(&block), &wallet).await,
        keep,
        "the drain leaves the kept amount in the wallet"
    );
    block
}

/// The number of runtime entries of the play of `offer` on `node`. Each pass
/// of the owner discovery enters the runtime once.
fn play_entries(node: &TestNode, offer: &models::casper::DeployDataProto) -> usize {
    node.runtime_manager
        .offered_budget_usages()
        .iter()
        .filter(|record| {
            record.kind == OfferedUsageKind::PlayRuntimeEntry
                && record.deploy_id == offer.deploy_id.as_ref()
        })
        .count()
}

// ── T5 and T6: a refused interaction ────────────────────────────────────────

/// The persistent service of W in T5 and T6. It fires only for an integer.
const INT_SERVICE: &str = r#"for (@{x /\ Int} <= @"int-svc") { @"int-svc-ran"!(x) }"#;

/// The state after the steps of T5, which T6 repeats.
struct RefusedService {
    /// The root after C's refused send.
    root: StateHash,
    /// C's receipt of the refused send.
    receipt: OfferedSettlementReceipt,
    /// C's balance before the refused send.
    client_before: u64,
}

/// The steps of T5: W stores `INT_SERVICE`, W drains its wallet to 1 atomic
/// REV, and C sends a matching integer.
async fn refused_int_service(
    nodes: &mut [TestNode],
    owner: &(PrivateKey, PublicKey),
    client: &(PrivateKey, PublicKey),
    sink: &VaultAddress,
) -> RefusedService {
    let (installed, receipt) =
        settle(nodes, owner_direct_offer(owner, 1, INT_SERVICE.to_string())).await;
    assert_eq!(receipt.purses.len(), 1, "W pays for its own service alone");
    let drained = drain_wallet_to(nodes, owner, 2, &post_state(&installed), sink, 1).await;
    let client_before = super::vault_balance(nodes, &post_state(&drained), &vault_of(client)).await;
    let (refused, receipt) = settle(
        nodes,
        owner_direct_offer(client, 3, r#"@"int-svc"!(1)"#.to_string()),
    )
    .await;
    RefusedService {
        root: post_state(&refused),
        receipt,
        client_before,
    }
}

/// T5 (DR-135 items 2, 3 and 5): W stores a persistent service and drains its
/// wallet to 1 atomic REV. C's matching send does not fire, because W cannot
/// pay its share of the COMM. C's run settles with one row, W pays nothing,
/// and the service and C's datum both stay stored.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unfunded_owner_interaction_does_not_fire_and_the_run_settles() {
    let [owner, client, sponsor] = [party(0x41), party(0x42), party(0x43)];
    let mut nodes = owner_network(&[owner.clone(), client.clone(), sponsor.clone()]).await;
    let refused = refused_int_service(&mut nodes, &owner, &client, &vault_of(&sponsor)).await;
    let root = &refused.root;
    assert_client_only(&refused.receipt, &vault_of(&client));
    assert_eq!(refused.receipt.phlo_price, CLIENT_PRICE);
    assert_eq!(
        refused.receipt.rev_spent,
        rev_at_client_price(&refused.receipt),
        "C pays only its own work, at its own price"
    );
    assert_eq!(
        u128::from(super::vault_balance(&nodes, root, &vault_of(&client)).await)
            + refused.receipt.rev_spent,
        u128::from(refused.client_before),
        "C's wallet pays C's receipt"
    );
    assert_eq!(
        super::vault_balance(&nodes, root, &vault_of(&owner)).await,
        1,
        "the refused COMM draws nothing from W"
    );
    assert_eq!(
        ints(&nodes, root, "int-svc").await,
        vec![1],
        "C's datum stays stored"
    );
    assert_eq!(
        waiting(&nodes, root, "int-svc").await,
        1,
        "W's service stays stored"
    );
    assert!(
        data(&nodes, root, "int-svc-ran").await.is_empty(),
        "the service does not fire"
    );
}

/// T6 (DR-135 item 5, R6): after the steps of T5, a sponsor tops up W's
/// wallet. D's non-matching send stays stored, and nothing fires, because a
/// produce keeps its own datum and C's refused datum does not wake. E's
/// matching send fires with E's own datum, and W pays its share. C's datum
/// stays stored.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_pair_fires_only_with_a_new_arrival() {
    let [owner, client, sponsor, second, third] = [
        party(0x44),
        party(0x45),
        party(0x46),
        party(0x47),
        party(0x48),
    ];
    let mut nodes = owner_network(&[
        owner.clone(),
        client.clone(),
        sponsor.clone(),
        second.clone(),
        third.clone(),
    ])
    .await;
    let refused = refused_int_service(&mut nodes, &owner, &client, &vault_of(&sponsor)).await;
    assert_client_only(&refused.receipt, &vault_of(&client));
    let owner_vault = vault_of(&owner);
    let (topped, _) = settle(
        &mut nodes,
        owner_direct_offer(
            &sponsor,
            4,
            super::vault_transfer(&vault_of(&sponsor), &owner_vault, TOP_UP),
        ),
    )
    .await;
    let funded = super::vault_balance(&nodes, &post_state(&topped), &owner_vault).await;
    assert_eq!(funded, 1 + TOP_UP, "the top-up gives W a budget again");

    let (other, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&second, 5, r#"@"int-svc"!("text")"#.to_string()),
    )
    .await;
    let root = post_state(&other);
    assert_client_only(&receipt, &vault_of(&second));
    assert!(
        data(&nodes, &root, "int-svc-ran").await.is_empty(),
        "nothing fires: D's datum does not match, and C's datum does not wake"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &owner_vault).await,
        funded,
        "W pays nothing"
    );

    let (matched, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&third, 6, r#"@"int-svc"!(2)"#.to_string()),
    )
    .await;
    let root = post_state(&matched);
    assert_eq!(
        ints(&nodes, &root, "int-svc-ran").await,
        vec![2],
        "the service fires with E's own datum"
    );
    assert_eq!(receipt.purses.len(), 2, "E and W pay");
    assert!(
        row(&receipt, &vault_of(&third)).is_some(),
        "E pays its own side"
    );
    owner_paid(&nodes, &root, &receipt, &owner_vault, funded).await;
    assert_rows_add_up(&receipt);
    let stored = data(&nodes, &root, "int-svc").await;
    assert_eq!(stored.len(), 2, "the data of C and D stay stored");
    assert!(
        stored.iter().any(|par| RhoNumber::unapply(par) == Some(1)),
        "C's refused datum stays stored"
    );
    assert!(
        stored
            .iter()
            .any(|par| RhoString::unapply(par).as_deref() == Some("text")),
        "D's datum stays stored"
    );
    assert_eq!(
        waiting(&nodes, &root, "int-svc").await,
        1,
        "the persistent service stays stored"
    );
}

// ── T7: a fired body ────────────────────────────────────────────────────────

/// The contract of W in T7. Its body sends the received value on `"echoed"`.
const ECHO_CONTRACT: &str = r#"contract @"echo"(@x) = { @"echoed"!(x) }"#;

/// T7 (DR-135 item 1, R1): C calls W's contract. The fired body runs for its
/// owner W, so W's row pays for the COMM and the body, and the datum on
/// `"echoed"` carries W's region. C's later read of that datum therefore draws
/// W's purse as the owner of the datum.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fired_body_charges_its_owner() {
    let [owner, client] = [party(0x49), party(0x4a)];
    let mut nodes = owner_network(&[owner.clone(), client.clone()]).await;
    let owner_vault = vault_of(&owner);
    let client_vault = vault_of(&client);
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&owner, 1, ECHO_CONTRACT.to_string()),
    )
    .await;
    let before = super::vault_balance(&nodes, &post_state(&installed), &owner_vault).await;

    let (called, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 2, r#"@"echo"!(42)"#.to_string()),
    )
    .await;
    let root = post_state(&called);
    assert_eq!(
        ints(&nodes, &root, "echoed").await,
        vec![42],
        "the body runs"
    );
    assert_eq!(receipt.purses.len(), 2, "C and W pay");
    owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
    let client_row = row(&receipt, &client_vault).expect("C pays its own side");
    assert_eq!(
        client_row.fee_rev, receipt.fee_rev,
        "the client pays the fee"
    );
    assert_rows_add_up(&receipt);

    let before = super::vault_balance(&nodes, &root, &owner_vault).await;
    let (read, receipt) = settle(
        &mut nodes,
        owner_direct_offer(
            &client,
            3,
            r#"for (@y <- @"echoed") { @"echo-read"!(y) }"#.to_string(),
        ),
    )
    .await;
    let root = post_state(&read);
    assert_eq!(ints(&nodes, &root, "echo-read").await, vec![42]);
    assert_eq!(
        receipt.purses.len(),
        2,
        "the datum on \"echoed\" carries W's region, so W pays its side of the read"
    );
    owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
}

// ── T8: the lollipop transferee ─────────────────────────────────────────────

// Changed by G3 (DR-135, S9): the run that creates `entry` cannot store a
// process that `entry` seals, because `entry` cannot pay for its
// introduction there (R8, decision A). The first install run now stores an
// installer-owned continuation, and a second run fires it after a sponsor
// funds the entry. The replaced installer and its test text follow.
// /// The installer of T8: a trigger signed `[entry -o slot]`, which is
// /// `{for(..) {body}[slot]}[entry]` (P1:1274-1283). The entry owns the stored
// /// trigger, and the slot owns the body. The body runs no received value
// /// (Rule H, §13.5). The installer publishes both vault addresses.
// const LOLLIPOP_INSTALLER: &str = r#"new entry, slot, entryAddressCh, slotAddressCh,
//       VaultAddress(`rho:vault:address`) in {
//       {% for (_ <- @"lollipop-trigger") { @"lollipop-ran"!(true) } %}[entry -o slot] |
//       VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
//       for (@address <- entryAddressCh) { @"lollipop-entry-address"!!(address) } |
//       VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
//       for (@address <- slotAddressCh) { @"lollipop-slot-address"!!(address) }
//     }"#;
// /// T8 (DR-135 items 4 and 5, A5): the transferee of a lollipop pays for the
// /// body. With a funded entry and an empty slot, C's call does not fire, and
// /// the entry pays nothing. After a sponsor funds the slot, C's next call
// /// fires. The entry pays its side of the COMM, and the slot pays for the
// /// body.

/// The first install run of T8. It publishes the vault addresses of `entry`
/// and `slot`, and it stores an installer-owned continuation on
/// `"lollipop-install"`. When the continuation fires, its body stores a
/// trigger signed `[entry -o slot]`, which is `{for(..) {body}[slot]}[entry]`
/// (P1:1274-1283). The entry owns the stored trigger and pays for its
/// introduction, and the slot owns the body. The body runs no received value
/// (Rule H, §13.5).
const LOLLIPOP_INSTALLER: &str = r#"new entry, slot, entryAddressCh, slotAddressCh,
      VaultAddress(`rho:vault:address`) in {
      for (_ <- @"lollipop-install") {
        {% for (_ <- @"lollipop-trigger") { @"lollipop-ran"!(true) } %}[entry -o slot]
      } |
      VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
      for (@address <- entryAddressCh) { @"lollipop-entry-address"!!(address) } |
      VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
      for (@address <- slotAddressCh) { @"lollipop-slot-address"!!(address) }
    }"#;

/// Added by G3 (DR-135, S9): the second install run of T8. It fires the
/// installer's continuation, which stores the trigger.
const LOLLIPOP_INSTALL: &str = r#"@"lollipop-install"!(true)"#;

/// T8 (DR-135 items 4 and 5, A5): the transferee of a lollipop pays for the
/// body. The funded entry pays for the introduction of its trigger. With an
/// empty slot, C's call does not fire, and the entry pays nothing. After a
/// sponsor funds the slot, C's next call fires. The entry pays its side of the
/// COMM, and the slot pays for the body.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_lollipop_transferee_pays() {
    let [installer, client, sponsor] = [party(0x4b), party(0x4c), party(0x4d)];
    let mut nodes = owner_network(&[installer.clone(), client.clone(), sponsor.clone()]).await;
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 1, LOLLIPOP_INSTALLER.to_string()),
    )
    .await;
    let root = post_state(&installed);
    let entry = published_address(&nodes, &root, "lollipop-entry-address").await;
    let slot = published_address(&nodes, &root, "lollipop-slot-address").await;
    fund(&mut nodes, &sponsor, 2, &root, &[(&entry, PURSE_FUNDS)]).await;
    // Added by G3 (DR-135, S9): the second install run stores the trigger,
    // and the entry pays for it.
    let (stored, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 2, LOLLIPOP_INSTALL.to_string()),
    )
    .await;
    let root = post_state(&stored);
    assert_eq!(
        waiting(&nodes, &root, "lollipop-trigger").await,
        1,
        "the second install run stores the trigger"
    );
    owner_paid(&nodes, &root, &receipt, &entry, PURSE_FUNDS).await;
    let entry_funds = super::vault_balance(&nodes, &root, &entry).await;

    let (refused, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 3, r#"@"lollipop-trigger"!(1)"#.to_string()),
    )
    .await;
    let root = post_state(&refused);
    assert_client_only(&receipt, &vault_of(&client));
    assert_eq!(
        super::vault_balance(&nodes, &root, &entry).await,
        // Changed by G3 (DR-135, S9): the entry paid for its trigger.
        // PURSE_FUNDS,
        entry_funds,
        "the refused call charges the entry nothing"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &slot).await,
        0,
        "the slot stays empty"
    );
    assert!(
        data(&nodes, &root, "lollipop-ran").await.is_empty(),
        "the body does not run"
    );
    assert_eq!(
        ints(&nodes, &root, "lollipop-trigger").await,
        vec![1],
        "C's datum stays stored"
    );
    assert_eq!(
        waiting(&nodes, &root, "lollipop-trigger").await,
        1,
        "the trigger stays stored"
    );

    fund(&mut nodes, &sponsor, 4, &root, &[(&slot, PURSE_FUNDS)]).await;
    let (fired, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 5, r#"@"lollipop-trigger"!(2)"#.to_string()),
    )
    .await;
    let root = post_state(&fired);
    assert_eq!(
        flags(&nodes, &root, "lollipop-ran").await,
        vec![true],
        "the body runs"
    );
    assert_eq!(receipt.purses.len(), 3, "C, the entry and the slot pay");
    // Changed by G3 (DR-135, S9): the entry paid for its trigger.
    // owner_paid(&nodes, &root, &receipt, &entry, PURSE_FUNDS).await;
    owner_paid(&nodes, &root, &receipt, &entry, entry_funds).await;
    owner_paid(&nodes, &root, &receipt, &slot, PURSE_FUNDS).await;
    assert!(
        row(&receipt, &vault_of(&installer)).is_none(),
        "the installer owns neither the trigger nor the body"
    );
    assert_rows_add_up(&receipt);
    assert_eq!(
        ints(&nodes, &root, "lollipop-trigger").await,
        vec![1],
        "C's refused datum does not wake"
    );
}

// ── T9: a compound owner ────────────────────────────────────────────────────

// Changed by G3 (DR-135, S9): the run that creates `a` and `b` cannot store
// a service that they seal, because they cannot pay for its introduction
// there (R8, decision A). The first install run now stores a persistent
// installer-owned continuation, and later runs fire it. The replaced
// installer follows.
// /// The installer of T9: a persistent service signed `{{..}[a]}[b]`. The
// /// normalizer flattens the nested signature into the canonical compound of
// /// `a` and `b` (`flatten_nested_signature`). The installer publishes both
// /// names and both vault addresses.
// const COMPOUND_INSTALLER: &str = r#"new a, b, aAddressCh, bAddressCh,
//       VaultAddress(`rho:vault:address`) in {
//       {% {% for (_ <= @"compound-svc") { Nil } %}[a] %}[b] |
//       @"compound-name-a"!(*a) |
//       @"compound-name-b"!(*b) |
//       VaultAddress!("fromUnforgeable", *a, *aAddressCh) |
//       for (@address <- aAddressCh) { @"compound-a-address"!!(address) } |
//       VaultAddress!("fromUnforgeable", *b, *bAddressCh) |
//       for (@address <- bAddressCh) { @"compound-b-address"!!(address) }
//     }"#;

/// The first install run of T9. It publishes the names `a` and `b` and their
/// vault addresses, and it stores a persistent installer-owned continuation
/// on `"compound-install"`. Each time the continuation fires, its body
/// introduces a persistent service signed `{{..}[a]}[b]`. The normalizer
/// flattens the nested signature into the canonical compound of `a` and `b`
/// (`flatten_nested_signature`).
const COMPOUND_INSTALLER: &str = r#"new a, b, aAddressCh, bAddressCh,
      VaultAddress(`rho:vault:address`) in {
      for (_ <= @"compound-install") {
        {% {% for (_ <= @"compound-svc") { Nil } %}[a] %}[b]
      } |
      @"compound-name-a"!(*a) |
      @"compound-name-b"!(*b) |
      VaultAddress!("fromUnforgeable", *a, *aAddressCh) |
      for (@address <- aAddressCh) { @"compound-a-address"!!(address) } |
      VaultAddress!("fromUnforgeable", *b, *bAddressCh) |
      for (@address <- bAddressCh) { @"compound-b-address"!!(address) }
    }"#;

/// Added by G3 (DR-135, S9): an install run of T9 after the first. It fires
/// the installer's continuation, which introduces the service.
const COMPOUND_INSTALL: &str = r#"@"compound-install"!(true)"#;

/// The cost signature that the reducer gives a `new` name in a signed term:
/// `Name` of the sorted Par that holds only the name's unforgeable
/// (`substitute_cost_signature`). `published` is the name that the owner sent
/// as a datum.
fn name_signature(published: &Par) -> CostSignature {
    // The env value of a `new` name is a Par that holds its unforgeable and
    // nothing else (`allocate_new_bindings`, `interpreter/util/mod.rs`), and
    // the reducer signs with the sorted env value (`substitute_cost_signature`).
    // This Par therefore equals the Par in the stored signature. T9 also
    // compares the vault of each rebuilt name with the vault that
    // `rho:vault:address` published.
    let [unforgeable] = published.unforgeables.as_slice() else {
        panic!("a published name holds one unforgeable: {published:?}");
    };
    let name = Par {
        unforgeables: vec![unforgeable.clone()],
        ..Default::default()
    };
    CostSignature {
        value: Some(Value::Name(ParSortMatcher::sort_match(&name).term)),
    }
}

// Changed by G3 (DR-135, S9): the refused step moves to the introduction
// of the service (decision A). The replaced text follows.
// /// T9 (DR-135 items 2 and 4, P1 Rules 2 and 3): the owner of the service is
// /// the compound of `a` and `b`. With only `a` funded, C's call does not fire,
// /// because Rule 2 takes both halves or nothing. With `a` and `b` funded, each
// /// pays half of the owner share. With the compound's lane vault funded, the
// /// vault pays the whole share, and the atoms pay nothing.
/// T9 (DR-135 items 2 and 4, P1 Rules 2 and 3): the owner of the service is
/// the compound of `a` and `b`. With only `a` funded, the install run stores
/// nothing, because Rule 2 takes both halves or nothing. With `a` and `b`
/// funded, the next install run stores the service, and `a` and `b` pay for
/// its introduction. C's call then fires, and each atom pays half of the
/// owner share. With the compound's lane vault funded, the vault pays the
/// whole share, and the atoms pay nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_compound_owner_pays_by_its_vault_or_equal_shares() {
    let [installer, client, sponsor] = [party(0x4e), party(0x4f), party(0x50)];
    let mut nodes = owner_network(&[installer.clone(), client.clone(), sponsor.clone()]).await;
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 1, COMPOUND_INSTALLER.to_string()),
    )
    .await;
    let root = post_state(&installed);
    let a = published_address(&nodes, &root, "compound-a-address").await;
    let b = published_address(&nodes, &root, "compound-b-address").await;
    let a_name = data(&nodes, &root, "compound-name-a")
        .await
        .first()
        .map(name_signature)
        .expect("the installer published the name a");
    let b_name = data(&nodes, &root, "compound-name-b")
        .await
        .first()
        .map(name_signature)
        .expect("the installer published the name b");
    assert_eq!(
        vault_payer(&a_name)
            .expect("a private name has a vault")
            .address,
        a,
        "the rebuilt signature of a locates the vault of a"
    );
    assert_eq!(
        vault_payer(&b_name)
            .expect("a private name has a vault")
            .address,
        b,
        "the rebuilt signature of b locates the vault of b"
    );
    // The normalizer flattens the nested seal into the canonical compound,
    // whose elements are sorted (`flatten_nested_signature`). A compound in
    // another order is not canonical, and `vault_payer` rejects it.
    let compound = compound_cost_signatures(&a_name, &b_name)
        .expect("two private names form a canonical compound");
    let Some(OwnerPurseSet::Compound { atoms, .. }) = purse_of(&compound) else {
        panic!("a compound of private names has a lane vault and atom purses");
    };
    assert_eq!(atoms.len(), 2, "the compound has the atoms a and b");
    let vault = vault_payer(&compound)
        .expect("a compound has a lane vault")
        .address;

    fund(&mut nodes, &sponsor, 2, &root, &[(&a, PURSE_FUNDS)]).await;
    // Changed by G3 (DR-135, S9): the installer's second run is refused at
    // the introduction, in place of C's first call. The replaced call follows.
    // let (refused, receipt) = settle(
    //     &mut nodes,
    //     owner_direct_offer(&client, 3, r#"@"compound-svc"!(1)"#.to_string()),
    // )
    // .await;
    // let root = post_state(&refused);
    // assert_client_only(&receipt, &vault_of(&client));
    // assert_eq!(
    //     super::vault_balance(&nodes, &root, &a).await,
    //     PURSE_FUNDS,
    //     "a does not pay alone"
    // );
    // assert_eq!(
    //     ints(&nodes, &root, "compound-svc").await,
    //     vec![1],
    //     "C's datum stays stored"
    // );
    let (refused, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 2, COMPOUND_INSTALL.to_string()),
    )
    .await;
    let root = post_state(&refused);
    assert_client_only(&receipt, &vault_of(&installer));
    assert_eq!(
        super::vault_balance(&nodes, &root, &a).await,
        PURSE_FUNDS,
        "a does not pay alone"
    );
    assert_eq!(
        waiting(&nodes, &root, "compound-svc").await,
        0,
        "the refused introduction stores nothing"
    );

    fund(&mut nodes, &sponsor, 4, &root, &[(&b, PURSE_FUNDS)]).await;
    // Added by G3 (DR-135, S9): the installer's third run stores the
    // service, and `a` and `b` pay for its introduction.
    let (stored, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 3, COMPOUND_INSTALL.to_string()),
    )
    .await;
    let root = post_state(&stored);
    assert_eq!(
        waiting(&nodes, &root, "compound-svc").await,
        1,
        "the service is stored"
    );
    assert_eq!(receipt.purses.len(), 3, "the installer, a and b pay");
    owner_paid(&nodes, &root, &receipt, &a, PURSE_FUNDS).await;
    owner_paid(&nodes, &root, &receipt, &b, PURSE_FUNDS).await;
    let a_funds = super::vault_balance(&nodes, &root, &a).await;
    let b_funds = super::vault_balance(&nodes, &root, &b).await;

    let (split, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 5, r#"@"compound-svc"!(2)"#.to_string()),
    )
    .await;
    let root = post_state(&split);
    assert_eq!(receipt.purses.len(), 3, "C, a and b pay");
    // Changed by G3 (DR-135, S9): a and b paid for the introduction first.
    // let a_share = owner_paid(&nodes, &root, &receipt, &a, PURSE_FUNDS).await;
    // let b_share = owner_paid(&nodes, &root, &receipt, &b, PURSE_FUNDS).await;
    let a_share = owner_paid(&nodes, &root, &receipt, &a, a_funds).await;
    let b_share = owner_paid(&nodes, &root, &receipt, &b, b_funds).await;
    assert_eq!(a_share, b_share, "a and b pay equal halves");
    assert!(
        row(&receipt, &vault).is_none(),
        "the empty compound vault pays nothing"
    );
    assert_rows_add_up(&receipt);

    let a_left = super::vault_balance(&nodes, &root, &a).await;
    let b_left = super::vault_balance(&nodes, &root, &b).await;
    fund(&mut nodes, &sponsor, 6, &root, &[(&vault, PURSE_FUNDS)]).await;
    let (whole, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 7, r#"@"compound-svc"!(3)"#.to_string()),
    )
    .await;
    let root = post_state(&whole);
    assert_eq!(receipt.purses.len(), 2, "C and the compound vault pay");
    let vault_share = owner_paid(&nodes, &root, &receipt, &vault, PURSE_FUNDS).await;
    // The two calls have one shape: one COMM with the service, and an integer
    // datum of one encoded size. The meter counts only COMMs and stored bytes
    // (DR-131), so it gives both calls one owner share.
    assert_eq!(
        vault_share,
        a_share + b_share,
        "the vault pays the whole share that a and b split"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &a).await,
        a_left,
        "a pays nothing while the vault can pay"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &b).await,
        b_left,
        "b pays nothing while the vault can pay"
    );
}

// ── T10: a forgeable owner ──────────────────────────────────────────────────

/// The write of T10: a datum signed with the spelled ground `bob`.
const FORGED_WRITE: &str = r#"{% @"forged-data"!(1) %}[bob]"#;

/// The read of T10.
const FORGED_READ: &str = r#"for (@x <- @"forged-data") { @"forged-read"!(x) }"#;

/// T10 (DR-135 item 2, R2): anyone can spell the ground signature `bob`, so
/// `bob` has no owner purse. W writes a datum sealed `[bob]` and pays for it
/// as a client. A sponsor funds the lane vault of `bob`. C's read does not
/// fire, and the lane vault keeps its funds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forgeable_owner_seal_draws_nothing() {
    let [writer, client, sponsor] = [party(0x51), party(0x52), party(0x53)];
    let mut nodes = owner_network(&[writer.clone(), client.clone(), sponsor.clone()]).await;
    let bob = CostSignature {
        value: Some(Value::Ground(canon_ground("bob"))),
    };
    assert!(
        purse_of(&bob).is_none(),
        "a spelled ground has no owner purse"
    );
    let lane_vault = vault_payer(&bob)
        .expect("a ground signature has a lane vault")
        .address;
    let every_class = (0..selected_schedule().classes.len()).collect::<Vec<_>>();
    // A lane that a source of the offer permits is a client lane: the check
    // at R0 builds the client lanes with `client_lane_set` over the funding
    // lane and `source_lanes` (`offered_owner_terms` and `check_with_estimate`,
    // execution/gate.rs), and `OwnerFundingBinding::is_owner_region`
    // (owner_funding.rs) treats a region on a client lane as a client region.
    // W's own vault therefore pays for the `[bob]` datum as for a client
    // region, and the write is not owner-refused (R8).
    let write = owner_offer_with_lanes(
        &writer,
        1,
        FORGED_WRITE.to_string(),
        0,
        TEST_SHARD.to_string(),
        OFFER_PHLO_LIMIT,
        &[PermittedLane {
            signature: &bob,
            classes: &every_class,
        }],
    );
    let (written, receipt) = settle(&mut nodes, write).await;
    assert_client_only(&receipt, &vault_of(&writer));
    let root = post_state(&written);
    assert_eq!(
        ints(&nodes, &root, "forged-data").await,
        vec![1],
        "the write stores the datum"
    );
    fund(&mut nodes, &sponsor, 2, &root, &[(
        &lane_vault,
        PURSE_FUNDS,
    )])
    .await;

    let (refused, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 3, FORGED_READ.to_string()),
    )
    .await;
    let root = post_state(&refused);
    assert_client_only(&receipt, &vault_of(&client));
    assert_eq!(
        super::vault_balance(&nodes, &root, &lane_vault).await,
        PURSE_FUNDS,
        "the lane vault of bob pays nothing"
    );
    assert!(
        data(&nodes, &root, "forged-read").await.is_empty(),
        "the read does not fire"
    );
    assert_eq!(
        ints(&nodes, &root, "forged-data").await,
        vec![1],
        "the datum stays stored"
    );
    assert_eq!(
        waiting(&nodes, &root, "forged-data").await,
        1,
        "C's read stays stored"
    );
}

// ── T11: an owner drain ─────────────────────────────────────────────────────

/// The contract of W in T11. It moves `amount` from W's wallet to `sink` with
/// W's own vault key, which the installing run holds.
fn spending_contract(owner: &VaultAddress, sink: &VaultAddress) -> String {
    format!(
        r#"new rl(`rho:registry:lookup`), systemVaultCh, vaultCh, authKeyCh,
          deployerId(`rho:system:deployerId`) in {{
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {{
            @systemVault!("find", "{owner}", *vaultCh) |
            @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
            for (@(true, vault) <- vaultCh & key <- authKeyCh) {{
              contract @"spend-owner-funds"(@amount, ret) = {{
                @vault!("transfer", "{sink}", amount, *key, *ret)
              }}
            }}
          }}
        }}"#,
        owner = owner.to_base58(),
        sink = sink.to_base58(),
    )
}

/// The call of T11. It asks W's contract to move `amount`, and it publishes
/// the transfer result.
fn spending_call(amount: u64) -> String {
    format!(
        r#"new ret in {{
          @"spend-owner-funds"!({amount}, *ret) |
          for (@result <- ret) {{ @"spend-result"!(result) }}
        }}"#
    )
}

/// T11 (DR-135 item 8, DR-65 item 5): when C calls it, W's contract moves all
/// but 1 atomic REV out of W's wallet. W's owner debit then exceeds the
/// wallet, so the held-capacity guard fails, and the run rolls back as a
/// charged user failure. W pays its row from the restored balance, and the
/// transfer moves nothing (plan §7 "Guard").
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_drain_fails_the_guard_and_charges_restored_balances() {
    let [owner, client, sink] = [party(0x54), party(0x55), party(0x56)];
    let mut nodes = owner_network(&[owner.clone(), client.clone(), sink.clone()]).await;
    let owner_vault = vault_of(&owner);
    let client_vault = vault_of(&client);
    let sink_vault = vault_of(&sink);
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&owner, 1, spending_contract(&owner_vault, &sink_vault)),
    )
    .await;
    let root = post_state(&installed);
    let owner_before = super::vault_balance(&nodes, &root, &owner_vault).await;
    let client_before = super::vault_balance(&nodes, &root, &client_vault).await;
    let sink_before = super::vault_balance(&nodes, &root, &sink_vault).await;

    let (called, id) = super::offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&client, 2, spending_call(owner_before - 1)),
    )
    .await;
    assert!(
        called.body.deploys[0].is_failed(),
        "the failed guard publishes a charged user failure"
    );
    let receipt = super::accepted_everywhere_with_one_receipt(&mut nodes, &called, &id).await;
    let root = post_state(&called);
    // Each pass of the producer, the rolled-back pass included, folds the
    // recording into the owner rows and settles them
    // (`produce_native_offered_candidate_inner`, execution/producer.rs), and
    // replay settles them on its rolled-back path too
    // (`settle_replayed_candidate`, execution/replay.rs). The owner budget is
    // at most the balance at the execution root (`owner_budget`), so the
    // restored balance covers the debit.
    owner_paid(&nodes, &root, &receipt, &owner_vault, owner_before).await;
    let client_row = row(&receipt, &client_vault).expect("C pays its own side");
    assert_eq!(
        u128::from(super::vault_balance(&nodes, &root, &client_vault).await)
            + client_row.resource_rev
            + client_row.fee_rev,
        u128::from(client_before),
        "C pays its row from its restored balance"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &sink_vault).await,
        sink_before,
        "the rolled-back transfer moves nothing"
    );
    assert!(
        data(&nodes, &root, "spend-result").await.is_empty(),
        "the rollback removes the transfer result"
    );
    assert_rows_add_up(&receipt);
}

// ── T12: a peek ─────────────────────────────────────────────────────────────

/// The data of W in T12: two equal datums on two channels of equal length.
const PEEKABLE_DATA: &str = r#"@"peek-a"!(7) | @"peek-b"!(7)"#;

/// The peek of C in T12. It copies the datum on `channel` to the channel
/// `"{channel}-copy"`.
fn peek_term(channel: &str) -> String {
    format!(r#"for (@x <<- @"{channel}") {{ @"{channel}-copy"!(x) }}"#)
}

/// T12 (DR-135 item 7, R9): C peeks W's datum. The datum remains, and W pays
/// for the COMM and for the restoration of its datum. With a budget 1 atomic
/// REV below that share, C's peek of W's equal second datum does not fire.
/// That datum also remains, because the COMM reserves the whole share,
/// restoration included.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peek_on_owner_data_restores_the_datum() {
    let [owner, client, sponsor] = [party(0x57), party(0x58), party(0x59)];
    let mut nodes = owner_network(&[owner.clone(), client.clone(), sponsor.clone()]).await;
    let owner_vault = vault_of(&owner);
    let (written, _) = settle(
        &mut nodes,
        owner_direct_offer(&owner, 1, PEEKABLE_DATA.to_string()),
    )
    .await;
    let before = super::vault_balance(&nodes, &post_state(&written), &owner_vault).await;

    let (peeked, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 2, peek_term("peek-a")),
    )
    .await;
    let root = post_state(&peeked);
    assert_eq!(
        ints(&nodes, &root, "peek-a").await,
        vec![7],
        "the peeked datum remains"
    );
    assert_eq!(ints(&nodes, &root, "peek-a-copy").await, vec![7]);
    assert_eq!(receipt.purses.len(), 2, "C and W pay");
    let peek_share = owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
    assert_rows_add_up(&receipt);

    let keep = u64::try_from(peek_share - 1).expect("an owner share fits u64");
    drain_wallet_to(&mut nodes, &owner, 3, &root, &vault_of(&sponsor), keep).await;
    // The two peeks have one shape on W's side: equal datums on channels of
    // equal length. C's own body makes the copy, so the copy is not part of
    // W's share. The meter counts only COMMs and stored bytes (DR-131), so it
    // gives both peeks one owner share,
    // restoration included. W's budget is then 1 atomic REV below the share of
    // this peek, and the COMM reserves the whole share (G-4), so the peek does
    // not fire.
    let (refused, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 4, peek_term("peek-b")),
    )
    .await;
    let root = post_state(&refused);
    assert_client_only(&receipt, &vault_of(&client));
    assert_eq!(
        ints(&nodes, &root, "peek-b").await,
        vec![7],
        "the datum remains"
    );
    assert!(
        data(&nodes, &root, "peek-b-copy").await.is_empty(),
        "the refused peek does not fire"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &owner_vault).await,
        keep,
        "W pays nothing for the refused peek"
    );
    assert_eq!(
        waiting(&nodes, &root, "peek-b").await,
        1,
        "C's refused peek stays stored"
    );
}

// ── T17: the owner's price ──────────────────────────────────────────────────

/// The persistent service of W in T17, which W installs at `OWNER_PRICE`.
const PRICED_SERVICE: &str = r#"for (@x <= @"priced-svc") { @"priced-ran"!(x) }"#;

/// The price that W signs in T17.
const OWNER_PRICE: u64 = 3;

/// T17 (DR-135 item 4, R4): W signs price 3, and C signs price 2. Each region
/// of W's service records price 3, so W's row is a multiple of 3. The REV of
/// the receipt is the client part at price 2 plus W's part at price 3, and
/// not `(fresh + retained) * 2 + fee`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owners_pay_at_their_own_signed_price() {
    let [owner, client] = [party(0x5b), party(0x5c)];
    let mut nodes = owner_network(&[owner.clone(), client.clone()]).await;
    let owner_vault = vault_of(&owner);
    let (installed, receipt) = settle(
        &mut nodes,
        owner_offer_priced(&owner, 1, PRICED_SERVICE.to_string(), OWNER_PRICE),
    )
    .await;
    assert_eq!(receipt.phlo_price, OWNER_PRICE, "W signs its own price");
    let before = super::vault_balance(&nodes, &post_state(&installed), &owner_vault).await;

    let (called, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 2, r#"@"priced-svc"!(1)"#.to_string()),
    )
    .await;
    let root = post_state(&called);
    assert_eq!(
        ints(&nodes, &root, "priced-ran").await,
        vec![1],
        "the body runs"
    );
    assert_eq!(receipt.phlo_price, CLIENT_PRICE, "C signs its own price");
    assert_eq!(receipt.purses.len(), 2, "C and W pay");
    let owner_rev = owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
    assert_eq!(
        owner_rev % u128::from(OWNER_PRICE),
        0,
        "W pays its phlo at price 3"
    );
    let client_row = row(&receipt, &vault_of(&client)).expect("C pays its own side");
    assert_eq!(
        client_row.resource_rev % u128::from(CLIENT_PRICE),
        0,
        "C pays its phlo at price 2"
    );
    assert_rows_add_up(&receipt);
    let at_client_price = rev_at_client_price(&receipt);
    assert_ne!(
        receipt.rev_spent, at_client_price,
        "the receipt does not price W's phlo at C's price"
    );
    // The receipt's `fresh_phlo` includes the owner phlo: the producer adds
    // the owner usage to the fresh usage of the client (`fresh_phlo`,
    // execution/producer.rs), and the evidence prices `resource_rev = (fresh -
    // owner_phlo + retained) * price + owner_rev`
    // (`NativeCostEvidenceV1::resource_rev`, models native_cost_evidence.rs).
    let owner_phlo = owner_rev / u128::from(OWNER_PRICE);
    assert_eq!(
        receipt.rev_spent,
        at_client_price - owner_phlo * u128::from(CLIENT_PRICE) + owner_rev,
        "W's phlo is priced at W's price, and C's phlo at C's price"
    );
}

// ── T18: a held process in a system body ────────────────────────────────────

/// The call of W in T18: `"map2 <-"` with W's function. The chain waits on
/// the public channels `"map2-a"` and `"map2-b"`. The function leaves a
/// receive on `"map2-leftover"`, and W publishes the sum that the chain
/// returns.
const MAP2_CHAIN: &str = r#"new rl(`rho:registry:lookup`), eitherCh, ret, f in {
      rl!(`rho:lang:either`, *eitherCh) |
      for (@(_, either) <- eitherCh) {
        @either!("map2 <-", "map2-a", "map2-b",
          for (@x, @y, r <- f) {
            r!(x + y) | for (_ <- @"map2-leftover") { @"map2-leftover-ran"!(true) }
          },
          *ret)
      } |
      for (@(true, sum) <- ret) { @"map2-result"!(sum) }
    }"#;

/// T18 (DR-135 item 9, Rule H, H6): W gives its function to the `"map2 <-"`
/// chain of `Either.rho`, and C feeds the chain later.
///
/// - In W's run, the body of `"map2 <-"` fires on W's datum. The datum carries
///   code, so H6 gives the caller set {W}, and the receive that the body
///   stores on `"map2-a"` records W.
/// - That stored body holds `f` as a held marker of W (H2). In C's run, the
///   send to `"map2"` strips the marker, and its datum records W (H5).
/// - The body of `"map2"` has the caller W (H6), and `match f` gives `fName`
///   the holder W (H3). The function therefore runs for W in C's run, and its
///   leftover receive carries W's region.
///
/// D's later send to the leftover draws W's purse and never C's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_process_in_either_map2_runs_for_its_caller() {
    let [owner, feeder, reader] = [party(0x5d), party(0x5e), party(0x5f)];
    let mut nodes = owner_network(&[owner.clone(), feeder.clone(), reader.clone()]).await;
    let owner_vault = vault_of(&owner);
    let feeder_vault = vault_of(&feeder);
    let (called, _) = settle(
        &mut nodes,
        owner_direct_offer(&owner, 1, MAP2_CHAIN.to_string()),
    )
    .await;
    let before = super::vault_balance(&nodes, &post_state(&called), &owner_vault).await;

    let (fed, receipt) = settle(
        &mut nodes,
        owner_direct_offer(
            &feeder,
            2,
            r#"@"map2-a"!((true, 1)) | @"map2-b"!((true, 2))"#.to_string(),
        ),
    )
    .await;
    let root = post_state(&fed);
    assert_eq!(
        ints(&nodes, &root, "map2-result").await,
        vec![3],
        "the chain runs W's function"
    );
    assert_eq!(
        waiting(&nodes, &root, "map2-leftover").await,
        1,
        "the function leaves its receive"
    );
    owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
    assert_rows_add_up(&receipt);

    let owner_before = super::vault_balance(&nodes, &root, &owner_vault).await;
    let feeder_before = super::vault_balance(&nodes, &root, &feeder_vault).await;
    let (read, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&reader, 3, r#"@"map2-leftover"!(1)"#.to_string()),
    )
    .await;
    let root = post_state(&read);
    assert_eq!(
        flags(&nodes, &root, "map2-leftover-ran").await,
        vec![true],
        "the leftover fires"
    );
    assert_eq!(receipt.purses.len(), 2, "D and W pay");
    owner_paid(&nodes, &root, &receipt, &owner_vault, owner_before).await;
    assert!(
        row(&receipt, &feeder_vault).is_none(),
        "C does not pay for W's leftover"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &feeder_vault).await,
        feeder_before,
        "C's wallet does not change"
    );
}

// ── T19: racing joins over the data of two owners ───────────────────────────

/// The racing joins of C in T19. Each owner keeps a persistent datum on its
/// own channel, and C adds a datum on each channel.
const RACING_JOINS: &str = r#"@"race-own-a"!(3) | @"race-own-b"!(4) |
    for (@x <- @"race-own-a" & @y <- @"race-own-b") { @"race-own-out"!(x + y) } |
    for (@x <- @"race-own-a" & @y <- @"race-own-b") { @"race-own-out"!(x * y) }"#;

/// T19 (DR-135 items 5 and 6, DR-132): two owners keep persistent data on two
/// channels, and C runs two racing joins over both channels with data of its
/// own, with 8 seeds. Each block replays on every node with equal receipts
/// and equal outputs. Only C and the two owners pay, and the wallet of each
/// owner falls by exactly its row.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_racing_join_seal_replays_on_every_node() {
    let [first_owner, second_owner, client] = [party(0x60), party(0x64), party(0x65)];
    let mut nodes =
        owner_network(&[first_owner.clone(), second_owner.clone(), client.clone()]).await;
    settle(
        &mut nodes,
        owner_direct_offer(&first_owner, 1, r#"@"race-own-a"!!(1)"#.to_string()),
    )
    .await;
    let (stored, _) = settle(
        &mut nodes,
        owner_direct_offer(&second_owner, 1, r#"@"race-own-b"!!(2)"#.to_string()),
    )
    .await;
    let mut root = post_state(&stored);
    let owners = [vault_of(&first_owner), vault_of(&second_owner)];
    let client_vault = vault_of(&client);
    let payers = [
        client_vault.to_base58(),
        owners[0].to_base58(),
        owners[1].to_base58(),
    ];
    let out = new_gstring_par("race-own-out".to_string(), Vec::new(), false);
    for seed in 1..=8 {
        let mut balances = Vec::with_capacity(owners.len());
        for owner in &owners {
            balances.push(super::vault_balance(&nodes, &root, owner).await);
        }
        let (block, receipt) = settle(
            &mut nodes,
            owner_direct_offer(&client, 100 + seed, RACING_JOINS.to_string()),
        )
        .await;
        root = post_state(&block);
        assert_rows_add_up(&receipt);
        assert!(
            receipt
                .purses
                .iter()
                .all(|purse| payers.iter().any(|payer| purse.address == payer.as_bytes())),
            "seed {seed}: only C and the two owners pay"
        );
        assert_eq!(
            row(&receipt, &client_vault)
                .expect("C pays its own side")
                .fee_rev,
            receipt.fee_rev,
            "seed {seed}: C pays the fee"
        );
        for (owner, before) in owners.iter().zip(&balances) {
            let after = super::vault_balance(&nodes, &root, owner).await;
            let paid = match row(&receipt, owner) {
                Some(paid) => {
                    assert_eq!(paid.fee_rev, 0, "seed {seed}: an owner row pays no fee");
                    assert_eq!(paid.post_balance, after, "seed {seed}: the row is current");
                    paid.resource_rev
                }
                None => 0,
            };
            assert_eq!(
                u128::from(after) + paid,
                u128::from(*before),
                "seed {seed}: the owner wallet falls by exactly its row"
            );
        }
        let mut outputs = Vec::with_capacity(nodes.len());
        for node in &nodes {
            outputs.push(
                node.runtime_manager
                    .get_data(root.clone(), &out)
                    .await
                    .expect("each node reads the output channel at the post-state"),
            );
        }
        assert!(
            outputs.windows(2).all(|pair| pair[0] == pair[1]),
            "seed {seed}: every node holds the same output"
        );
    }
}

// ── T20: a persistent receive of an owner without budget ────────────────────

/// The persistent service of W in T20.
const KEPT_SERVICE: &str = r#"for (@x <= @"kept-svc") { @"kept-ran"!(x) }"#;

/// T20 (DR-135 item 6, A4): W's budget covers exactly one firing of its
/// persistent service. C sends two values in one run. One fires, and W pays
/// for exactly that firing. The other value does not fire and stays stored.
/// W's persistent receive stays stored, because its re-issue has no owner
/// share.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_out_of_budget_keeps_its_persistent_receive() {
    let [owner, client, sponsor] = [party(0x66), party(0x67), party(0x68)];
    let mut nodes = owner_network(&[owner.clone(), client.clone(), sponsor.clone()]).await;
    let owner_vault = vault_of(&owner);
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&owner, 1, KEPT_SERVICE.to_string()),
    )
    .await;
    let before = super::vault_balance(&nodes, &post_state(&installed), &owner_vault).await;
    let (fired, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 2, r#"@"kept-svc"!(1)"#.to_string()),
    )
    .await;
    let root = post_state(&fired);
    let one_firing = owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
    let one_firing = u64::try_from(one_firing).expect("an owner share fits u64");

    drain_wallet_to(
        &mut nodes,
        &owner,
        3,
        &root,
        &vault_of(&sponsor),
        one_firing,
    )
    .await;
    // Each firing has one shape: one COMM with the service, a body that stores
    // one integer of one encoded size, and a re-issue with no owner share (A4).
    // The meter counts only COMMs and stored bytes (DR-131), so it gives each
    // firing one owner share. W's budget then covers exactly one of the two
    // firings of this run.
    let (raced, receipt) = settle(
        &mut nodes,
        owner_direct_offer(
            &client,
            4,
            r#"@"kept-svc"!(2) | @"kept-svc"!(3)"#.to_string(),
        ),
    )
    .await;
    let root = post_state(&raced);
    let paid = owner_paid(&nodes, &root, &receipt, &owner_vault, one_firing).await;
    assert_eq!(
        paid,
        u128::from(one_firing),
        "W pays for exactly one firing"
    );
    assert_eq!(
        super::vault_balance(&nodes, &root, &owner_vault).await,
        0,
        "W has no budget left"
    );
    assert_eq!(
        waiting(&nodes, &root, "kept-svc").await,
        1,
        "the persistent receive stays stored"
    );
    let ran = ints(&nodes, &root, "kept-ran").await;
    let left = ints(&nodes, &root, "kept-svc").await;
    assert_eq!(
        ran.len(),
        2,
        "the first send and one of the two later sends fire"
    );
    assert_eq!(ran[0], 1, "the first send fired");
    assert_eq!(left.len(), 1, "the refused send stays stored");
    let mut raced_values = vec![ran[1], left[0]];
    raced_values.sort_unstable();
    assert_eq!(
        raced_values,
        vec![2, 3],
        "one later send fires and the other stays stored"
    );
}

// ── T21: owner discovery ────────────────────────────────────────────────────

/// The read of the discovery test. The second read runs only after the
/// first read fires.
const NESTED_READ: &str = r#"for (_ <- @"discovery-first") {
    for (_ <- @"discovery-second") { @"discovery-done"!(true) }
}"#;

/// T21 (plan §8.2) and R3 (plan S7 edit 1): play runs again while it notes an
/// owner purse that its table does not list. The first pass lists nothing,
/// so the first read is refused, and the pass notes the first writer. The
/// second pass lists the first writer, the first read fires, and the second
/// read notes the second writer. The third pass lists both writers, and both
/// reads fire. The shipped owner table lists both writers, and each writer
/// pays its share from its own wallet.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owner_discovery_reruns_until_every_consulted_purse_is_listed() {
    let [first, second, reader] = [party(0x61), party(0x62), party(0x63)];
    let mut nodes = owner_network(&[first.clone(), second.clone(), reader.clone()]).await;
    settle(
        &mut nodes,
        owner_direct_offer(&first, 1, r#"@"discovery-first"!(1)"#.to_string()),
    )
    .await;
    let (stored, _) = settle(
        &mut nodes,
        owner_direct_offer(&second, 2, r#"@"discovery-second"!(2)"#.to_string()),
    )
    .await;
    let before = post_state(&stored);
    let first_before = super::vault_balance(&nodes, &before, &vault_of(&first)).await;
    let second_before = super::vault_balance(&nodes, &before, &vault_of(&second)).await;
    let offer = owner_direct_offer(&reader, 3, NESTED_READ.to_string());
    let (read, receipt) = settle(&mut nodes, offer.clone()).await;
    assert_eq!(
        play_entries(&nodes[0], &offer),
        3,
        "play runs once more for each level of discovery"
    );
    assert_eq!(receipt.purses.len(), 3, "{:?}", receipt.purses);
    let root = post_state(&read);
    let evidence = read.body.deploys[0]
        .as_offered()
        .expect("the block holds an offered deploy")
        .evidence(offered_funded_v6_limits().evidence)
        .expect("the committed evidence decodes");
    let recording = decode_native_budget_recording(
        evidence.budget_recording,
        offered_funded_v6_recording_limits(),
    )
    .expect("the committed budget recording decodes");
    let mut listed = recording
        .owner_table
        .iter()
        .map(|entry| {
            vault_payer(&entry.signature)
                .expect("a listed purse has a payer")
                .address
                .to_base58()
        })
        .collect::<Vec<_>>();
    listed.sort();
    let mut writers = vec![vault_of(&first).to_base58(), vault_of(&second).to_base58()];
    writers.sort();
    assert_eq!(listed, writers, "the shipped table lists both writers");
    owner_paid(&nodes, &root, &receipt, &vault_of(&first), first_before).await;
    owner_paid(&nodes, &root, &receipt, &vault_of(&second), second_before).await;
    assert_eq!(
        flags(&nodes, &root, "discovery-done").await,
        vec![true],
        "both reads fire"
    );
    assert_rows_add_up(&receipt);
}

// ── T22: a tampered owner table ─────────────────────────────────────────────

/// T22 (R3, plan §6.2 "After the run"): a producer re-encodes the evidence
/// of a block with a raised owner budget. Each validator re-reads the listed
/// purse at the execution root, finds another budget, and rejects the block.
/// The block as produced stays valid.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tampered_owner_table_is_rejected() {
    let [writer, reader] = [party(0x71), party(0x72)];
    let mut nodes = owner_network(&[writer.clone(), reader.clone()]).await;
    settle(
        &mut nodes,
        owner_direct_offer(&writer, 1, r#"@"tampered-data"!(1)"#.to_string()),
    )
    .await;
    let (block, id) = super::offered_block(
        &mut nodes,
        0,
        owner_direct_offer(
            &reader,
            2,
            r#"for (_ <- @"tampered-data") { Nil }"#.to_string(),
        ),
    )
    .await;
    let protocol = offered_funded_v6_limits();
    let recording_limits = offered_funded_v6_recording_limits();
    let offered = block.body.deploys[0]
        .as_offered()
        .expect("the block holds an offered deploy");
    let evidence = offered
        .evidence(protocol.evidence)
        .expect("the committed evidence decodes");
    let mut recording = decode_native_budget_recording(evidence.budget_recording, recording_limits)
        .expect("the committed budget recording decodes");
    assert_eq!(
        encode_native_budget_recording(&recording, recording_limits)
            .expect("the decoded recording encodes"),
        evidence.budget_recording,
        "the recording round trips"
    );
    assert_eq!(
        recording.owner_table.len(),
        1,
        "the run lists the writer's purse"
    );
    let mut entries = recording.owner_table.to_vec();
    entries[0].budget += 1;
    recording.owner_table = entries.into();
    let budget_recording = encode_native_budget_recording(&recording, recording_limits)
        .expect("the tampered recording encodes");
    let mut altered_evidence = evidence.clone();
    altered_evidence.budget_recording = &budget_recording;
    let mut altered_proto = offered
        .to_proto(protocol.evidence)
        .expect("the offered deploy encodes");
    altered_proto.native_cost_evidence = Some(
        altered_evidence
            .encode(protocol.evidence)
            .expect("the tampered evidence encodes")
            .into(),
    );
    let mut altered = block.clone();
    altered.body.deploys[0] =
        ProcessedUserDeploy::from_proto(altered_proto).expect("the tampered deploy decodes");
    let altered = resign_block(
        &altered,
        &nodes[0]
            .validator_id_opt
            .as_ref()
            .expect("node 0 is a validator")
            .private_key,
    );
    assert_ne!(altered.block_hash, block.block_hash);
    for node in nodes.iter_mut().skip(1) {
        let verdict = node
            .process_block(altered.clone())
            .await
            .expect("the validator processes the block");
        assert!(
            matches!(
                &verdict,
                Either::Left(BlockError::BlockException(error))
                    if error.to_string().contains("a recorded owner budget differs")
            ),
            "the validator rejects the raised budget: {verdict:?}"
        );
        assert!(!node.casper.dag_contains(&altered.block_hash));
    }
    super::accepted_everywhere_with_one_receipt(&mut nodes, &block, &id).await;
}

// ── T24: an owner-refused introduction ──────────────────────────────────────

/// The persistent service of W in T24. Its body acknowledges each call
/// unsigned, and it signs one more send with `slot`, which nobody funds. The
/// installer publishes the vault address of `slot`.
const SKIPPING_SERVICE: &str = r#"new slot, slotAddressCh, VaultAddress(`rho:vault:address`) in {
      for (@x <= @"skip-svc") { @"skip-ack"!(x) | {% @"skip-stored"!(x) %}[slot] } |
      VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
      for (@address <- slotAddressCh) { @"skip-slot-address"!!(address) }
    }"#;

/// T24 (DR-135 item 6, R8): C calls W's service. The body runs for W, but the
/// owner of its signed send is the empty slot, which cannot pay for the
/// datum. That introduction stores nothing, and the rest of the body runs.
/// C's run settles, C and W pay, and the slot pays nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_body_introduction_is_skipped_when_its_owner_cannot_pay() {
    let [owner, client] = [party(0x6b), party(0x6c)];
    let mut nodes = owner_network(&[owner.clone(), client.clone()]).await;
    let owner_vault = vault_of(&owner);
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&owner, 1, SKIPPING_SERVICE.to_string()),
    )
    .await;
    let root = post_state(&installed);
    let slot = published_address(&nodes, &root, "skip-slot-address").await;
    let before = super::vault_balance(&nodes, &root, &owner_vault).await;

    let (called, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&client, 2, r#"@"skip-svc"!(5)"#.to_string()),
    )
    .await;
    let root = post_state(&called);
    assert_eq!(
        ints(&nodes, &root, "skip-ack").await,
        vec![5],
        "the rest of the body runs"
    );
    assert!(
        data(&nodes, &root, "skip-stored").await.is_empty(),
        "the refused introduction stores nothing"
    );
    assert_eq!(receipt.purses.len(), 2, "C and W pay");
    owner_paid(&nodes, &root, &receipt, &owner_vault, before).await;
    assert!(row(&receipt, &slot).is_none(), "the slot pays nothing");
    assert_eq!(
        super::vault_balance(&nodes, &root, &slot).await,
        0,
        "the slot stays empty"
    );
    assert_rows_add_up(&receipt);
}

// ── T25: a fee-only caller ──────────────────────────────────────────────────

/// Added by G3 (DR-135, S11): the first install run of T25. It publishes the
/// vault addresses of `entry` and `slot`, and it stores an installer-owned
/// continuation on `"sponsored-install"`. When the continuation fires, its
/// body stores a persistent trigger signed `[entry -o slot]`. The entry owns
/// the trigger, and the slot owns the body. The body runs four COMMs of its
/// own and reports.
const SPONSORED_INSTALLER: &str = r#"new entry, slot, entryAddressCh, slotAddressCh,
      VaultAddress(`rho:vault:address`) in {
      for (_ <- @"sponsored-install") {
        {% for (_ <= @"sponsored-trigger") {
          new a, b, c, d in {
            a!(0) | for (_ <- a) { Nil } |
            b!(0) | for (_ <- b) { Nil } |
            c!(0) | for (_ <- c) { Nil } |
            d!(0) | for (_ <- d) { Nil }
          } |
          @"sponsored-ran"!(true)
        } %}[entry -o slot]
      } |
      VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
      for (@address <- entryAddressCh) { @"sponsored-entry-address"!!(address) } |
      VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
      for (@address <- slotAddressCh) { @"sponsored-slot-address"!!(address) }
    }"#;

/// Added by G3 (DR-135, S11): the cost evidence of the only deploy of
/// `block`.
fn block_evidence(block: &BlockMessage) -> (u64, u64, u64, u64) {
    let evidence = block.body.deploys[0]
        .as_offered()
        .expect("the block holds an offered deploy")
        .evidence(offered_funded_v6_limits().evidence)
        .expect("the committed evidence decodes");
    (
        evidence.phlo_used,
        evidence.owner_phlo,
        evidence.retained_phlo,
        evidence.phlo_limit,
    )
}

/// T25 (DR-135 item 13): a caller reserves only its own work. A sponsor
/// funds the entry and the slot of a persistent trigger. A funded caller
/// measures the caller's part of a call: the used phlo less the owner phlo,
/// which is smaller than the owner phlo. A fresh user, funded with exactly
/// that part at the client price plus the fee, signs that part as its limit
/// and calls. The run uses more than the user's limit, the owners pay their
/// shares, and every node accepts the block with an equal receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fee_only_caller_triggers_a_sponsor_funded_slot() {
    let [installer, sponsor, caller] = [party(0x6d), party(0x6e), party(0x6f)];
    let user = party(0x71);
    let mut nodes = owner_network(&[installer.clone(), sponsor.clone(), caller.clone()]).await;
    let (installed, _) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 1, SPONSORED_INSTALLER.to_string()),
    )
    .await;
    let root = post_state(&installed);
    let entry = published_address(&nodes, &root, "sponsored-entry-address").await;
    let slot = published_address(&nodes, &root, "sponsored-slot-address").await;
    fund(&mut nodes, &sponsor, 1, &root, &[
        (&entry, PURSE_FUNDS),
        (&slot, PURSE_FUNDS),
    ])
    .await;
    let (stored, _) = settle(
        &mut nodes,
        owner_direct_offer(&installer, 2, r#"@"sponsored-install"!(true)"#.to_string()),
    )
    .await;
    let root = post_state(&stored);
    assert_eq!(
        waiting(&nodes, &root, "sponsored-trigger").await,
        1,
        "the second install run stores the trigger"
    );

    let (calibrated, receipt) = settle(
        &mut nodes,
        owner_direct_offer(&caller, 1, r#"@"sponsored-trigger"!(1)"#.to_string()),
    )
    .await;
    assert_eq!(
        receipt.purses.len(),
        3,
        "the caller, the entry and the slot pay"
    );
    let (phlo_used, owner_phlo, _, _) = block_evidence(&calibrated);
    let client_part = phlo_used - owner_phlo;
    assert!(
        owner_phlo > client_part,
        "the owners do most of the work: owner {owner_phlo}, client {client_part}"
    );
    let root = post_state(&calibrated);

    let user_vault = vault_of(&user);
    fund(&mut nodes, &sponsor, 2, &root, &[(
        &user_vault,
        client_part * CLIENT_PRICE + FEE_REV,
    )])
    .await;
    let (called, receipt) = settle(
        &mut nodes,
        owner_offer_limited(
            &user,
            1,
            r#"@"sponsored-trigger"!(2)"#.to_string(),
            0,
            TEST_SHARD.to_string(),
            client_part,
        ),
    )
    .await;
    let root = post_state(&called);
    assert_eq!(
        flags(&nodes, &root, "sponsored-ran").await,
        vec![true, true],
        "both calls run the body"
    );
    assert_eq!(
        receipt.purses.len(),
        3,
        "the user, the entry and the slot pay"
    );
    let user_row = row(&receipt, &user_vault).expect("the user pays its own part");
    assert_eq!(user_row.fee_rev, receipt.fee_rev, "the user pays the fee");
    assert!(row(&receipt, &entry).is_some(), "the entry pays its side");
    assert!(row(&receipt, &slot).is_some(), "the slot pays the body");
    let (phlo_used, owner_phlo, retained_phlo, phlo_limit) = block_evidence(&called);
    assert!(
        phlo_used > phlo_limit,
        "the run uses {phlo_used}, more than the user's limit {phlo_limit}"
    );
    assert!(
        phlo_used - owner_phlo + retained_phlo <= phlo_limit,
        "the user's part fits its limit"
    );
    assert_rows_add_up(&receipt);
}

// ── G-1: equal client lanes and owner rooms ─────────────────────────────────

/// The check of `offer` at `root` on `node`, outside the block pipeline. The
/// check reads the URN map of the play runtime when `play` holds, as the
/// decision pass of a proposer does, and the URN map of the offered replay
/// runtime otherwise, as the witness replay of a validator does. Both read
/// the wallet snapshot at `root`.
async fn check_on(
    node: &TestNode,
    genesis: &GenesisContext,
    offer: &models::casper::DeployDataProto,
    root: &StateHash,
    play: bool,
) -> OfferedGate {
    let adopted = AdoptedResourcePolicy::load(
        &node.runtime_manager,
        &genesis.genesis_block,
        node.casper.casper_shard_conf(),
    )
    .await
    .expect("the adopted policy loads");
    let envelope = OfferedDeploy::from_proto(offer.clone()).envelope;
    let models::rust::deploy_envelope::DeployEnvelopeRef::OfferedFunded(signed) = envelope.view()
    else {
        panic!("the offer has an offered envelope");
    };
    let original: [u8; 32] = root.as_ref().try_into().expect("a root has 32 bytes");
    let funding = offered_funded_v6_limits().envelope.payload.funding;
    let prepared = prepare_offered_candidate(
        signed,
        &adopted,
        original,
        original,
        OfferedCandidateLimits {
            funding: PhloFundingIntentV2Limits {
                wire: funding.wire,
                base: funding,
                grant_uses: funding.wire.total_bytes / 8,
                grant_id_bytes: funding.wire.field_bytes,
                quote_evidence_bytes: funding.wire.field_bytes,
            },
            max_phlo_limit: i64::MAX as u64,
        },
    )
    .expect("the candidate prepares");
    let snapshot = node
        .runtime_manager
        .read_direct_offered_wallet_snapshot(&envelope, root, None)
        .await
        .expect("the wallet snapshot reads");
    let selected = prepared
        .selected_terms(&adopted)
        .expect("the signed schedule is selected");
    let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
        .expect("the schedule binds");
    let runtime = match play {
        true => node
            .runtime_manager
            .spawn_offered_play_runtime(offered_grant_issue_call_limits())
            .await
            .expect("the offered play runtime spawns"),
        false => node
            .runtime_manager
            .spawn_offered_runtime(offered_grant_issue_call_limits())
            .await
            .expect("the offered replay runtime spawns"),
    };
    gate_offered_candidate(OfferedGateInput {
        envelope: &envelope,
        prepared: &prepared,
        selected_terms: selected.bytes(),
        schedule: &schedule,
        minimum_price: adopted.genesis().minimum_price(),
        snapshot: &snapshot,
        evaluator: &runtime.reducer,
        limits: offered_funded_v6_production_limits().family,
    })
    .expect("the check accepts the offer")
}

/// G-1 (DR-135, S7): the check at R0 keeps the client lanes and the owner
/// room, and play and replay read them from it. The check of the proposer and
/// the check of a validator hold equal sets and rooms: the deploy lane and
/// each lane that a source permits, and the receipt rows that the client's
/// payer leaves free.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_play_and_replay_checks_hold_equal_client_lanes_and_rooms() {
    let genesis = super::offered_v6_genesis(2).await;
    let nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let client = genesis.genesis_vaults[0].clone();
    let principal = |public: &PublicKey| CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&public.bytes))),
    };
    let other = principal(&genesis.genesis_vaults[1].1);
    let offer = owner_offer_with_lanes(
        &client,
        1,
        "Nil".to_string(),
        0,
        TEST_SHARD.to_string(),
        OFFER_PHLO_LIMIT,
        &[PermittedLane {
            signature: &other,
            classes: &[0],
        }],
    );
    let root = genesis.genesis_block.body.state.post_state_hash.clone();
    let play = check_on(&nodes[0], &genesis, &offer, &root, true).await;
    let replay = check_on(&nodes[1], &genesis, &offer, &root, false).await;
    assert_eq!(play.client_lanes(), replay.client_lanes());
    assert_eq!(play.owner_room(), replay.owner_room());
    let lane = |signature: &CostSignature| {
        cost_signature_to_sig(signature)
            .expect("a lane signature converts")
            .lane_hash()
    };
    assert_eq!(
        play.client_lanes(),
        &BTreeSet::from([lane(&principal(&client.1)), lane(&other)]),
        "the client lanes are the deploy lane and the permitted lane"
    );
    assert_eq!(
        play.owner_room(),
        offered_funded_v6_limits().envelope.payload.funding.sources - 1,
        "the owner rows fill the receipt rows that the one payer leaves free"
    );
}
