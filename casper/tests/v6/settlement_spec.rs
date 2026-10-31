use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::gate::{gate_block, GateInput, PhloLimitDemand, StoreReader};
use casper::rust::v6::genesis_wallets::read_wallet_balance;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use casper::rust::v6::run::EnvelopeRun;
use casper::rust::v6::settle::{settle_block, Boxcar};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

struct Party {
    sk: PrivateKey,
    pk: PublicKey,
    owner: WalletOwner,
}

fn party() -> Party {
    let (sk, pk): (PrivateKey, PublicKey) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    Party { sk, pk, owner }
}

async fn shard(funded: &[(&Party, u128)]) -> (RuntimeManager, StateHash) {
    let rm = mk_runtime_manager("v6-settlement", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = funded
        .iter()
        .map(|(p, balance)| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: *balance,
        })
        .collect();
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    let root = block.body.state.post_state_hash.clone();
    (rm, root)
}

fn input(who: &Party, ts: i64, limit: i64, price: i64) -> GateInput {
    let deploy = construct_deploy::source_deploy(
        "Nil".to_string(),
        ts,
        Some(limit),
        Some(price),
        Some(who.sk.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap();
    GateInput::from_deploy(&deploy, Vec::new()).unwrap()
}

fn run(comms: u64) -> EnvelopeRun { metered(comms, 0) }

fn metered(comms: u64, compute: u64) -> EnvelopeRun {
    EnvelopeRun {
        envelope_sig: Default::default(),
        comm_phlo: comms,
        compute_phlo: compute,
        failed: false,
        user_log: Vec::new(),
        post_root: Default::default(),
    }
}

async fn balance(rm: &RuntimeManager, root: &StateHash, who: &Party) -> u64 {
    read_wallet_balance(rm, root, &who.owner).await.unwrap()
}

#[tokio::test]
async fn comms_are_paid_to_the_proposer_in_the_canonical_form() {
    let (alice, bob, carol, proposer) = (party(), party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 50_000)]).await;
    let inputs = vec![
        input(&alice, 1, 1_000, 3),
        input(&alice, 2, 1_000, 5),
        input(&bob, 3, 1_000, 7),
        input(&carol, 4, 1_000, 1),
        input(&bob, 5, 4, 2),
    ];
    let runs = vec![run(2), run(1), run(1), run(1), run(9)];
    let reader = StoreReader::at(&rm, &pre).await.unwrap();
    let gate = gate_block(&reader, &PhloLimitDemand, &inputs).unwrap();
    assert_eq!(gate.admitted(), vec![true, true, true, false, true]);

    let (settlement, settled) = settle_block(
        &rm,
        &pre,
        &inputs,
        &gate.entries,
        &runs,
        &vec![Vec::new(); runs.len()],
        &vec![true; runs.len()],
        &proposer.owner,
        Boxcar::unbounded(),
    )
    .await
    .unwrap();
    let comm_rev = 2 * 3 + 5 + 7 + 4 * 2;
    assert_eq!(settlement.proposer_credit, comm_rev);
    assert!(settlement.is_balanced());
    assert_eq!(settlement.charges.len(), 4);
    assert_eq!(settlement.charges[3].comm_phlo, 4);

    assert_eq!(balance(&rm, &settled, &alice).await, 100_000 - 11);
    assert_eq!(balance(&rm, &settled, &bob).await, 50_000 - 15);
    assert_eq!(balance(&rm, &settled, &carol).await, 0);
    assert_eq!(balance(&rm, &settled, &proposer).await, comm_rev);
    let mut total = 0;
    for p in [&alice, &bob, &carol, &proposer] {
        total += balance(&rm, &settled, p).await;
    }
    assert_eq!(total, 150_000);

    let (again, settled_again) = settle_block(
        &rm,
        &pre,
        &inputs,
        &gate.entries,
        &runs,
        &vec![Vec::new(); runs.len()],
        &vec![true; runs.len()],
        &proposer.owner,
        Boxcar::unbounded(),
    )
    .await
    .unwrap();
    assert_eq!(again, settlement);
    assert_eq!(settled_again, settled);
}

#[tokio::test]
async fn the_box_car_size_never_changes_the_state() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let inputs = vec![
        input(&alice, 1, 1_000, 3),
        input(&bob, 2, 1_000, 2),
        input(&alice, 3, 1_000, 1),
    ];
    let runs = vec![run(10), run(7), run(1)];
    let reader = StoreReader::at(&rm, &pre).await.unwrap();
    let gate = gate_block(&reader, &PhloLimitDemand, &inputs).unwrap();
    let mut roots = Vec::new();
    for boxcar in [
        Boxcar::unbounded(),
        Boxcar::of(1),
        Boxcar::of(3),
        Boxcar::of(64),
    ] {
        let (_, root) = settle_block(
            &rm,
            &pre,
            &inputs,
            &gate.entries,
            &runs,
            &vec![Vec::new(); runs.len()],
            &vec![true; runs.len()],
            &proposer.owner,
            boxcar,
        )
        .await
        .unwrap();
        roots.push(root);
    }
    assert!(roots.windows(2).all(|w| w[0] == w[1]));
    assert_ne!(roots[0], pre);
}

#[tokio::test]
async fn a_block_with_no_charge_leaves_the_state_alone() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100)]).await;
    let inputs = vec![input(&alice, 1, 10, 0)];
    let reader = StoreReader::at(&rm, &pre).await.unwrap();
    let gate = gate_block(&reader, &PhloLimitDemand, &inputs).unwrap();
    let (settlement, root) = settle_block(
        &rm,
        &pre,
        &inputs,
        &gate.entries,
        &[run(5)],
        &[Vec::new()],
        &[true],
        &proposer.owner,
        Boxcar::unbounded(),
    )
    .await
    .unwrap();
    assert_eq!(settlement.proposer_credit, 0);
    assert_eq!(root, pre);
}

async fn compute_wallet(rm: &RuntimeManager, root: &StateHash) -> u64 {
    let cells = casper::rust::v6::genesis_wallets::cells_at(rm, root)
        .await
        .unwrap();
    rholang::rust::interpreter::accounting::rev_stack::compute::compute_balance(
        std::sync::Arc::new(cells),
    )
    .unwrap()
}

#[tokio::test]
async fn compute_is_paid_to_the_compute_wallet_and_the_charge_never_passes_delta() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let inputs = vec![
        input(&alice, 1, 1_000, 3),
        input(&bob, 2, 50, 2),
        input(&bob, 3, 10, 1),
    ];
    let runs = vec![metered(4, 600), metered(20, 500), metered(30, 5)];
    let reader = StoreReader::at(&rm, &pre).await.unwrap();
    let gate = gate_block(&reader, &PhloLimitDemand, &inputs).unwrap();
    let mut roots = Vec::new();
    for boxcar in [Boxcar::unbounded(), Boxcar::of(7)] {
        let (settlement, settled) = settle_block(
            &rm,
            &pre,
            &inputs,
            &gate.entries,
            &runs,
            &vec![Vec::new(); runs.len()],
            &vec![true; runs.len()],
            &proposer.owner,
            boxcar,
        )
        .await
        .unwrap();
        let c = &settlement.charges;
        assert_eq!((c[0].comm_phlo, c[0].compute_phlo), (4, 600));
        assert_eq!((c[1].comm_phlo, c[1].compute_phlo), (20, 30));
        assert_eq!((c[2].comm_phlo, c[2].compute_phlo), (10, 0));
        assert_eq!(settlement.proposer_credit, 4 * 3 + 20 * 2 + 10);
        assert_eq!(settlement.compute_credit, 600 * 3 + 30 * 2);
        assert!(settlement.is_balanced());
        assert_eq!(balance(&rm, &settled, &alice).await, 100_000 - 12 - 1_800);
        assert_eq!(balance(&rm, &settled, &bob).await, 100_000 - 100 - 10);
        assert_eq!(balance(&rm, &settled, &proposer).await, 62);
        assert_eq!(compute_wallet(&rm, &settled).await, 1_860);
        assert_eq!(compute_wallet(&rm, &pre).await, 0);
        roots.push(settled);
    }
    assert_eq!(roots[0], roots[1]);
}

#[tokio::test]
async fn settlement_releases_unspent_commitments_and_conserves_the_supply() {
    use casper::rust::v6::settle::audit;
    use rholang::rust::interpreter::accounting::rev_stack::wallet_channel_id;

    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let inputs = vec![
        input(&alice, 1, 1_000, 3),
        input(&alice, 2, 100, 2),
        input(&bob, 3, 500, 1),
    ];
    let runs = vec![metered(4, 96), metered(50, 90), metered(0, 0)];
    let reader = StoreReader::at(&rm, &pre).await.unwrap();
    let gate = gate_block(&reader, &PhloLimitDemand, &inputs).unwrap();
    let (settlement, settled) = settle_block(
        &rm,
        &pre,
        &inputs,
        &gate.entries,
        &runs,
        &vec![Vec::new(); runs.len()],
        &vec![true; runs.len()],
        &proposer.owner,
        Boxcar::unbounded(),
    )
    .await
    .unwrap();

    let a = wallet_channel_id(&alice.owner);
    let b = wallet_channel_id(&bob.owner);
    assert_eq!(settlement.debits[&a], 300 + 200);
    assert_eq!(settlement.released[&a], 3_000 + 200 - 500);
    assert_eq!(settlement.released[&b], 500);
    assert!(!settlement.debits.contains_key(&b) || settlement.debits[&b] == 0);

    let (_, _, params) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    let validators = &params.proof_of_stake.validators;
    let bonds: u128 = validators.iter().map(|v| v.stake as u128).sum();
    let mut wallets = vec![
        alice.owner.clone(),
        bob.owner.clone(),
        proposer.owner.clone(),
    ];
    wallets.extend(
        validators.iter().map(|v| {
            rholang::rust::interpreter::accounting::rev_stack::bond::bond_owner(&v.pk.bytes)
        }),
    );
    for root in [&pre, &settled] {
        let cells = casper::rust::v6::genesis_wallets::cells_at(&rm, root)
            .await
            .unwrap();
        let audit = audit(std::sync::Arc::new(cells), &wallets).unwrap();
        assert_eq!(audit.legacy_bonds, 0);
        assert!(bonds > 0);
        assert_eq!(audit.supply as u128, 200_000 + bonds);
        assert!(audit.conserved(), "{audit:?}");
    }
}
