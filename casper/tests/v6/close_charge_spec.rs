use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
use casper::rust::v6::close_charge::{pre_charge, shard_floor, CloseCharge};
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
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

const FUNDS: u64 = 20_000_000;

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

async fn shard(funded: &[&Party]) -> (Arc<RuntimeManager>, StateHash) { shard_at_floor(funded, 1).await }

async fn shard_at_floor(funded: &[&Party], floor: i64) -> (Arc<RuntimeManager>, StateHash) {
    let rm = mk_runtime_manager("v6-close-charge", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.proof_of_stake.epoch_length = 1;
    genesis.proof_of_stake.quarantine_length = 10;
    genesis.proof_of_stake.min_phlo_price = floor;
    genesis.vaults = funded
        .iter()
        .map(|p| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: FUNDS as u128,
        })
        .collect();
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    (Arc::new(rm), block.body.state.post_state_hash.clone())
}

fn envelope(term: &str, ts: i64, who: &Party) -> ProcessedDeploy { envelope_limited(term, ts, who, 200_000) }

fn envelope_limited(term: &str, ts: i64, who: &Party, limit: i64) -> ProcessedDeploy {
    ProcessedDeploy::empty(
        construct_deploy::source_deploy(
            term.to_string(),
            ts,
            Some(limit),
            Some(1),
            Some(who.sk.clone()),
            None,
            Some("root".to_string()),
        )
        .unwrap(),
    )
}

fn block(number: i64, deploys: Vec<ProcessedDeploy>, proposer: &Party) -> BlockMessage {
    BlockMessage {
        block_hash: Bytes::from(format!("v6-close-charge-{number}").into_bytes()),
        header: Header {
            parents_hash_list: vec![],
            timestamp: number,
            version: 1,
            extra_bytes: Bytes::new(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
                bonds: vec![],
                block_number: number,
            },
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

async fn run_record(executor: &BlockExecutor, root: &StateHash, block: &BlockMessage) -> BlockRecord {
    let record = executor.execute(root, block).await.expect("the block executes");
    for entry in record.executed() {
        assert_eq!(entry.outcome, EnvelopeOutcome::Completed);
    }
    let replayed = executor.replay(&record, block).await.expect("the block replays");
    assert_eq!(&replayed, record.post_root());
    record
}

async fn balance(rm: &RuntimeManager, root: &StateHash, owner: &WalletOwner) -> u64 {
    read_wallet_balance(rm, root, owner).await.unwrap()
}

struct Close {
    proposer: u64,
    compute: u64,
}

async fn close(rm: &RuntimeManager, pre: &StateHash, record: &BlockRecord, proposer: &Party) -> Close {
    let settlement = record.settlement().unwrap();
    let post = record.post_root();
    let proposer_after_settlement = balance(rm, pre, &proposer.owner).await + settlement.proposer_credit;
    let compute_after_settlement = balance(rm, pre, &compute_wallet_owner()).await + settlement.compute_credit;
    Close {
        proposer: proposer_after_settlement - balance(rm, post, &proposer.owner).await,
        compute: balance(rm, post, &compute_wallet_owner()).await - compute_after_settlement,
    }
}

fn bond_term(continuation: &str) -> String {
    format!(
        r#"new rb(`rho:rev:bond`), deployerId(`rho:system:deployerId`), rl(`rho:registry:lookup`), posCh,
               bonded, registered, handleCh in {{
             rl!(`rho:system:pos`, *posCh) |
             for (@(_, PoS) <- posCh) {{
               rb!("bond", (*deployerId, 1000), *bonded) |
               for (@(true, receipt) <- bonded) {{ @PoS!("bond", receipt, 1000, *registered, *handleCh) }} |
               for (@r <- registered) {{ @"registered"!(r) }} |
               for (@h <- handleCh) {{ @"handle"!(h) | {continuation} }}
             }}
           }}"#
    )
}

async fn charging(rm: &Arc<RuntimeManager>, root: &StateHash) -> BlockExecutor {
    let floor = shard_floor(rm, root).await.unwrap();
    BlockExecutor::new(rm.clone(), 2).with_close_charge(CloseCharge::new(1, 0, floor))
}

#[tokio::test]
async fn the_proposer_pays_for_the_close_pos_code_and_the_rest_of_the_pre_charge_stays() {
    let (alice, proposer, other) = (party(), party(), party());
    let (rm, genesis) = shard(&[&alice, &proposer, &other]).await;
    let executor = charging(&rm, &genesis).await;

    let record = run_record(&executor, &genesis, &block(1, vec![], &proposer)).await;
    let charged = close(&rm, &genesis, &record, &proposer).await;
    assert!(charged.proposer > 0, "the close's PoS code is charged");
    assert!(charged.proposer < pre_charge(1, 0, 1), "only the actual work is charged");
    assert_eq!(charged.compute, charged.proposer, "the charge goes to the compute wallet");
    let settlement = record.settlement().unwrap();
    assert_eq!(
        balance(&rm, record.post_root(), &proposer.owner).await,
        FUNDS + settlement.proposer_credit - charged.proposer
    );

    let by_other = run_record(&executor, &genesis, &block(1, vec![], &other)).await;
    assert_eq!(close(&rm, &genesis, &by_other, &other).await.proposer, charged.proposer);
    assert_eq!(balance(&rm, by_other.post_root(), &proposer.owner).await, FUNDS);
}

#[tokio::test]
async fn the_close_is_priced_at_the_shard_floor() {
    let proposer = party();
    let (rm1, genesis1) = shard_at_floor(&[&proposer], 1).await;
    let (rm3, genesis3) = shard_at_floor(&[&proposer], 3).await;
    assert_eq!(shard_floor(&rm3, &genesis3).await.unwrap(), 3);
    let at1 = run_record(&charging(&rm1, &genesis1).await, &genesis1, &block(1, vec![], &proposer)).await;
    let at3 = run_record(&charging(&rm3, &genesis3).await, &genesis3, &block(1, vec![], &proposer)).await;
    let phlo = close(&rm1, &genesis1, &at1, &proposer).await.proposer;
    assert_eq!(close(&rm3, &genesis3, &at3, &proposer).await.proposer, 3 * phlo);
}

#[tokio::test]
async fn the_charge_is_capped_at_the_pre_charge() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[&alice, &proposer]).await;
    let small = CloseCharge {
        price: 1,
        pre_charge: 1_000,
    };
    let executor = BlockExecutor::new(rm.clone(), 2).with_close_charge(small);
    let record = run_record(&executor, &genesis, &block(1, vec![], &proposer)).await;
    assert_eq!(close(&rm, &genesis, &record, &proposer).await.proposer, 1_000);
}

#[tokio::test]
async fn the_close_charge_is_inside_the_post_state_root() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[&alice, &proposer]).await;
    let executor = charging(&rm, &genesis).await;
    let free_executor = BlockExecutor::new(rm.clone(), 2);
    let b = block(1, vec![envelope("new x in { x!(1) }", 1, &alice)], &proposer);

    let charged = run_record(&executor, &genesis, &b).await;
    let free = free_executor.execute(&genesis, &b).await.unwrap();
    assert_ne!(charged.post_root(), free.post_root());
    assert_eq!(charged.mid_root(), free.mid_root());
    assert!(executor.replay(&free, &b).await.is_err(), "a record without the charge does not replay");
    assert!(free_executor.replay(&charged, &b).await.is_err(), "nor does a charged record replay uncharged");
}

#[tokio::test]
async fn a_continuation_fired_in_the_close_charges_its_installer() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[&alice, &proposer]).await;
    let executor = charging(&rm, &genesis).await;
    let heavy = "new y in { y!(1) | for (_ <- y) { new z in { z!(2) | for (_ <- z) { Nil } } } }";

    let light = run_record(&executor, &genesis, &block(1, vec![envelope(&bond_term("Nil"), 1, &alice)], &proposer)).await;
    let loaded = run_record(&executor, &genesis, &block(1, vec![envelope(&bond_term(heavy), 1, &alice)], &proposer)).await;

    let light_close = close(&rm, &genesis, &light, &proposer).await;
    let loaded_close = close(&rm, &genesis, &loaded, &proposer).await;
    let installer_pays = |close: &Close| close.compute - close.proposer;
    assert!(installer_pays(&loaded_close) > installer_pays(&light_close));
    let light_left = balance(&rm, light.post_root(), &alice.owner).await;
    let loaded_left = balance(&rm, loaded.post_root(), &alice.owner).await;
    assert!(loaded_left < light_left, "the installer pays more for the heavier continuation");
}

#[tokio::test]
async fn genesis_charges_nobody() {
    let (alice, bob) = (party(), party());
    let (rm, genesis) = shard(&[&alice, &bob]).await;
    assert_eq!(balance(&rm, &genesis, &compute_wallet_owner()).await, 0);
    assert_eq!(balance(&rm, &genesis, &alice.owner).await, FUNDS);
    assert_eq!(balance(&rm, &genesis, &bob.owner).await, FUNDS);
}

#[tokio::test]
async fn no_deployment_makes_the_close_fail() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis) = shard(&[&alice, &bob, &proposer]).await;
    let executor = charging(&rm, &genesis).await;
    let endless = r#"contract @"spin"(@n) = { @"spin"!(n + 1) } | @"spin"!(0)"#;
    let junk = r#"new rl(`rho:registry:lookup`), posCh, ret in {
        rl!(`rho:system:pos`, *posCh) |
        for (@(_, PoS) <- posCh) {
          @PoS!("bond", "junk", -5, *ret, *ret) |
          @PoS!("withdraw", Nil, *ret) |
          @PoS!("closeBlock", Nil, *ret) |
          @PoS!("slash", Nil, Nil, *ret) |
          @PoS!("noSuchMethod")
        }
      }"#;
    let b = block(
        1,
        vec![
            envelope_limited(&bond_term(endless), 1, &alice, (FUNDS - 120_000) as i64),
            envelope(junk, 2, &bob),
        ],
        &proposer,
    );
    let record = executor
        .execute(&genesis, &b)
        .await
        .expect("the close runs to completion");
    assert!(!record.system_deploys().is_empty());
    let replayed = executor.replay(&record, &b).await.expect("the block replays");
    assert_eq!(&replayed, record.post_root());
    let left = balance(&rm, record.post_root(), &alice.owner).await;
    let refusals = casper::rust::v6::stops::system_refusals(record.system_deploys()).unwrap();
    assert!(!refusals.is_empty(), "the installer's wallet stops the work that the close fired");
    assert!(left < FUNDS, "the installer pays for the work that the close fired");
}
