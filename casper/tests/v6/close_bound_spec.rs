use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
use casper::rust::v6::close_charge::{pre_charge_phlo, CloseCharge};
use casper::rust::v6::executor::BlockExecutor;
use casper::rust::v6::genesis_wallets::read_wallet_balance;
use casper::rust::v6::record::ExecutionRecord;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

const FUNDS: u128 = 1_000_000_000_000_000;

const MEASURE: CloseCharge = CloseCharge {
    price: 1,
    pre_charge: (FUNDS / 2) as u64,
};

struct Party {
    sk: PrivateKey,
    pk: PublicKey,
    owner: WalletOwner,
}

fn party() -> Party {
    let (sk, pk) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    Party { sk, pk, owner }
}

async fn shard(funded: &[&Party], validators: usize, epoch_length: i32) -> (Arc<RuntimeManager>, StateHash) {
    let keys: Vec<(PrivateKey, PublicKey)> = (0..validators).map(|_| Secp256k1.new_key_pair()).collect();
    shard_with(funded, keys, epoch_length, 1).await
}

async fn shard_with(
    funded: &[&Party],
    keys: Vec<(PrivateKey, PublicKey)>,
    epoch_length: i32,
    quarantine_length: i32,
) -> (Arc<RuntimeManager>, StateHash) {
    let rm = mk_runtime_manager("v6-close-bound", Some(Genesis::default_mergeable_tags_arc())).await;
    let bonds: HashMap<PublicKey, i64> = keys.iter().map(|(_, pk)| (pk.clone(), 1_000)).collect();
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters(keys, &bonds);
    genesis.version = PROTOCOL_V6;
    genesis.proof_of_stake.epoch_length = epoch_length;
    genesis.proof_of_stake.quarantine_length = quarantine_length;
    genesis.vaults = funded
        .iter()
        .map(|p| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: FUNDS,
        })
        .chain(bonds.keys().map(|pk| Vault {
            vault_address: VaultAddress::from_public_key(pk).unwrap(),
            initial_balance: FUNDS,
        }))
        .collect();
    let block = Genesis::create_genesis_block_with_policy(&rm, &genesis, Some(&ShardResourcePolicy::default()))
        .await
        .unwrap();
    (Arc::new(rm), block.body.state.post_state_hash.clone())
}

fn envelope(term: &str, ts: i64, who: &Party, limit: i64) -> ProcessedDeploy {
    ProcessedDeploy::empty(
        construct_deploy::source_deploy(term.to_string(), ts, Some(limit), Some(1), Some(who.sk.clone()), None, Some("root".to_string()))
            .unwrap(),
    )
}

fn block(number: i64, deploys: Vec<ProcessedDeploy>, proposer: &Party) -> BlockMessage {
    BlockMessage {
        block_hash: Bytes::from(format!("v6-close-bound-{number}").into_bytes()),
        header: Header { parents_hash_list: vec![], timestamp: number, version: 1, extra_bytes: Bytes::new() },
        body: Body {
            state: F1r3flyState { pre_state_hash: Bytes::new(), post_state_hash: Bytes::new(), bonds: vec![], block_number: number },
            deploys,
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
            v6: None,
        },
        justifications: vec![],
        sender: proposer.pk.bytes.clone(),
        seq_num: number as i32,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

async fn close_phlo(rm: &RuntimeManager, pre: &StateHash, record: &BlockRecord, proposer: &Party) -> u64 {
    let settlement = record.settlement().unwrap();
    let before = read_wallet_balance(rm, pre, &proposer.owner).await.unwrap() + settlement.proposer_credit;
    before - read_wallet_balance(rm, record.post_root(), &proposer.owner).await.unwrap()
}

fn bonds_term(count: usize) -> String {
    let registrations = (0..count)
        .map(|i| {
            format!(
                r#"new registered, handleCh in {{
                     @PoS!("bond", receipt, 1, *registered, *handleCh) |
                     for (@r <- registered) {{ @"registered-{i}"!(r) }}
                   }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        r#"new rb(`rho:rev:bond`), deployerId(`rho:system:deployerId`), rl(`rho:registry:lookup`), posCh, bonded in {{
             rl!(`rho:system:pos`, *posCh) |
             for (@(_, PoS) <- posCh) {{
               rb!("bond", (*deployerId, 1000), *bonded) |
               for (@(true, receipt) <- bonded) {{ {registrations} }}
             }}
           }}"#
    )
}

async fn measure(validators: usize, epoch_length: i32, bonds: usize) -> u64 {
    let proposer = party();
    let bonders: Vec<Party> = (0..bonds).map(|_| party()).collect();
    let mut funded: Vec<&Party> = bonders.iter().collect();
    funded.push(&proposer);
    let (rm, genesis) = shard(&funded, validators, epoch_length).await;
    let executor = BlockExecutor::new(rm.clone(), 8).with_close_charge(MEASURE);
    let deploys = bonders
        .iter()
        .enumerate()
        .map(|(i, who)| envelope(&bonds_term(1), i as i64 + 1, who, 1_000_000))
        .collect();
    let b = block(1, deploys, &proposer);
    let record = executor.execute(&genesis, &b).await.expect("the block executes");
    let merged = record
        .executed()
        .iter()
        .filter(|entry| entry.outcome == EnvelopeOutcome::Completed)
        .count();
    assert!(record
        .executed()
        .iter()
        .all(|entry| matches!(entry.outcome, EnvelopeOutcome::Completed | EnvelopeOutcome::Conflicted)));
    eprintln!("merged bonds: {merged} of {bonds}");
    assert_eq!(&executor.replay(&record, &b).await.unwrap(), record.post_root());
    let phlo = close_phlo(&rm, &genesis, &record, &proposer).await;
    eprintln!("close phlo: validators {validators}, epoch_length {epoch_length}, pending bonds {bonds}: {phlo}");
    phlo
}

#[tokio::test]
async fn the_close_stays_inside_its_pre_charge() {
    for (validators, epoch_length, bonds) in [
        (1, 1000, 0),
        (1, 1, 0),
        (10, 1, 0),
        (100, 1000, 0),
        (100, 1, 0),
        (100, 1, 1),
        (100, 1, 16),
        (100, 1, 64),
    ] {
        let phlo = measure(validators, epoch_length, bonds).await;
        let bound = pre_charge_phlo((validators + bonds) as u64);
        assert!(phlo <= bound, "{phlo} > {bound}");
    }
}

#[tokio::test]
async fn an_epoch_with_many_withdrawers_stays_inside_its_pre_charge() {
    for withdrawers in [10, 30] {
        for (bonds, phlo) in withdrawers_close(100, withdrawers).await {
            let bound = pre_charge_phlo(bonds);
            assert!(phlo <= bound, "{withdrawers} withdrawers, {bonds} bonds: {phlo} > {bound}");
        }
    }
}

fn withdraw_term() -> String {
    r#"new rb(`rho:rev:bond`), d(`rho:system:deployerId`), rl(`rho:registry:lookup`), posCh, handleCh, out in {
         rb!("handle", *d, *handleCh) |
         for (@(true, handle) <- handleCh) {
           rl!(`rho:system:pos`, *posCh) |
           for (@(_, PoS) <- posCh) { @PoS!("withdraw", handle, *out) | for (@r <- out) { @"answer"!(r) } }
         }
       }"#
    .to_string()
}

async fn withdrawers_close(validators: usize, withdrawers: usize) -> [(u64, u64); 2] {
    let proposer = party();
    let keys: Vec<(PrivateKey, PublicKey)> = (0..validators).map(|_| Secp256k1.new_key_pair()).collect();
    let parties: Vec<Party> = keys
        .iter()
        .map(|(sk, pk)| Party {
            sk: sk.clone(),
            pk: pk.clone(),
            owner: WalletOwner::from_public_key(pk).unwrap(),
        })
        .collect();
    let (rm, genesis) = shard_with(&[&proposer], keys, 1000, 0).await;
    let executor = BlockExecutor::new(rm.clone(), 2).with_close_charge(MEASURE);
    let mut root = genesis;
    for (i, who) in parties.iter().take(withdrawers).enumerate() {
        let n = i as i64 + 1;
        let b = block(n, vec![envelope(&withdraw_term(), n, who, 1_000_000)], &proposer);
        let record = executor.execute(&root, &b).await.expect("the withdrawal executes");
        assert_eq!(record.executed()[0].outcome, EnvelopeOutcome::Completed);
        root = record.post_root().clone();
    }
    let mut closes = [(0, 0); 2];
    for (epoch, number) in [1000i64, 2000].into_iter().enumerate() {
        let bonds = if epoch == 0 { validators } else { validators - withdrawers };
        let b = block(number, vec![], &proposer);
        let record = executor.execute(&root, &b).await.expect("the epoch close executes");
        assert_eq!(&executor.replay(&record, &b).await.unwrap(), record.post_root());
        let phlo = close_phlo(&rm, &root, &record, &proposer).await;
        eprintln!("close phlo: validators {validators}, {withdrawers} withdrawers, epoch {} with {bonds} bonds: {phlo}", epoch + 1);
        closes[epoch] = (bonds as u64, phlo);
        root = record.post_root().clone();
    }
    closes
}

#[tokio::test]
#[ignore = "measures the close for the FX-16 pre-charge terms"]
async fn measure_the_close_per_bond() {
    for validators in [10, 50, 100, 150, 200] {
        measure(validators, 1000, 0).await;
        measure(validators, 1, 0).await;
    }
}

#[tokio::test]
#[ignore = "measures the close for the FX-16 pre-charge terms"]
async fn measure_the_close_per_released_withdrawer() {
    for withdrawers in [0, 10, 30, 60, 99] {
        withdrawers_close(100, withdrawers).await;
    }
}
