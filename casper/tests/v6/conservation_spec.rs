use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::conservation::{check_at, AuditSchedule, Conservation, WalletIndex};
use casper::rust::v6::executor::BlockExecutor;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, TokenKind, WalletOwner,
};
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

struct Party {
    sk: PrivateKey,
    pk: PublicKey,
}

fn party() -> Party {
    let (sk, pk): (PrivateKey, PublicKey) = Secp256k1.new_key_pair();
    Party { sk, pk }
}

fn owner(p: &Party) -> WalletOwner { WalletOwner::from_public_key(&p.pk).unwrap() }

async fn shard(funded: &[(&Party, u128)]) -> (Arc<RuntimeManager>, BlockMessage) {
    let rm = mk_runtime_manager(
        "v6-conservation",
        Some(Genesis::default_mergeable_tags_arc()),
    )
    .await;
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
    (Arc::new(rm), block)
}

fn block(proposer: &Party, deploys: Vec<ProcessedDeploy>, number: i64) -> BlockMessage {
    BlockMessage {
        block_hash: Bytes::from(format!("v6-conservation-{number}").into_bytes()),
        header: Header {
            parents_hash_list: vec![],
            timestamp: 1,
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
        seq_num: 1,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

fn transfer(from: &Party, to: &Party, amount: u64) -> ProcessedDeploy {
    let address = VaultAddress::from_public_key(&to.pk).unwrap().to_base58();
    let term = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{
             w!("transfer", (*d, "{address}", {amount}), "out")
           }}"#
    );
    deploy(from, term, 1)
}

fn deploy(from: &Party, term: String, timestamp: i64) -> ProcessedDeploy {
    ProcessedDeploy::empty(
        construct_deploy::source_deploy(
            term,
            timestamp,
            Some(1_000),
            Some(2),
            Some(from.sk.clone()),
            None,
            Some("root".to_string()),
        )
        .unwrap(),
    )
}

fn rev(
    audits: &[casper::rust::v6::conservation::KindAudit],
) -> &casper::rust::v6::conservation::KindAudit {
    audits
        .iter()
        .find(|a| a.kind == TokenKind::rev())
        .expect("REV is always audited")
}

#[tokio::test]
async fn the_genesis_index_accounts_for_the_whole_supply() {
    let (alice, bob) = (party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000), (&bob, 7)]).await;
    let index = WalletIndex::from_genesis(&genesis);
    assert!(index
        .wallets(&TokenKind::rev())
        .any(|w| *w == wallet_channel_id(&owner(&alice))));
    let audits = check_at(&rm, &genesis.body.state.post_state_hash, &index)
        .await
        .unwrap();
    let rev = rev(&audits);
    assert!(rev.minted > 100_007);
    assert!(rev.conserved(), "{rev:?}");
}

#[tokio::test]
async fn an_index_that_misses_a_funded_wallet_fails_the_audit() {
    let alice = party();
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let full = WalletIndex::from_genesis(&genesis);
    let mut partial = WalletIndex::default();
    for wallet in full.wallets(&TokenKind::rev()) {
        if *wallet != wallet_channel_id(&owner(&alice)) {
            partial.insert(TokenKind::rev(), wallet.clone());
        }
    }
    let audits = check_at(&rm, &genesis.body.state.post_state_hash, &partial)
        .await
        .unwrap();
    assert!(!rev(&audits).conserved());
}

#[tokio::test]
async fn play_and_replay_audit_on_schedule_and_index_the_new_wallets() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let pre = genesis.body.state.post_state_hash.clone();
    let conservation = Conservation::new(
        AuditSchedule { interval: 1 },
        WalletIndex::from_genesis(&genesis),
    );
    let executor = BlockExecutor::new(rm.clone(), 2).with_conservation(conservation.clone());
    let b = block(&proposer, vec![transfer(&alice, &bob, 600)], 1);

    let record = executor
        .execute(&pre, &b)
        .await
        .expect("the block executes");
    let settlement = record.settlement().expect("a settlement");
    assert!(settlement.debited() > 0, "the transfer is charged");
    let (number, audits) = conservation.last().expect("the audit ran in play");
    assert_eq!(number, 1);
    assert!(rev(&audits).conserved(), "{:?}", rev(&audits));
    assert_eq!(conservation.failures(), 0);
    let index = conservation.index();
    for wallet in [
        wallet_channel_id(&owner(&bob)),
        wallet_channel_id(&owner(&proposer)),
    ] {
        assert!(index.wallets(&TokenKind::rev()).any(|w| *w == wallet));
    }

    let replayer = Conservation::new(
        AuditSchedule { interval: 1 },
        WalletIndex::from_genesis(&genesis),
    );
    let post = BlockExecutor::new(rm.clone(), 2)
        .with_conservation(replayer.clone())
        .replay(&record, &b)
        .await
        .expect("the block replays");
    assert_eq!(
        &post,
        casper::rust::v6::record::ExecutionRecord::post_root(&record)
    );
    let (number, audits) = replayer.last().expect("the audit ran in replay");
    assert_eq!(number, 1);
    assert!(rev(&audits).conserved());
    assert_eq!(replayer.failures(), 0);
}

#[tokio::test]
async fn an_off_schedule_block_is_not_audited() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let conservation = Conservation::new(
        AuditSchedule { interval: 4 },
        WalletIndex::from_genesis(&genesis),
    );
    BlockExecutor::new(rm.clone(), 2)
        .with_conservation(conservation.clone())
        .execute(
            &genesis.body.state.post_state_hash,
            &block(&proposer, vec![], 3),
        )
        .await
        .expect("the block executes");
    assert!(conservation.last().is_none());
    assert!(conservation
        .index()
        .wallets(&TokenKind::rev())
        .any(|w| *w == wallet_channel_id(&owner(&proposer))));
}

#[tokio::test]
async fn a_minted_kind_is_audited_against_its_mints() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let conservation = Conservation::new(
        AuditSchedule { interval: 1 },
        WalletIndex::from_genesis(&genesis),
    );
    let executor = BlockExecutor::new(rm.clone(), 2).with_conservation(conservation.clone());
    let mint = r#"new k, d(`rho:system:deployerId`) in {
        {% d :: d :: d :: () %}[k] | @"pub"!(bundle0{*k})
    }"#;
    let first = block(&proposer, vec![deploy(&alice, mint.to_string(), 1)], 1);
    let minted = executor
        .execute(&genesis.body.state.post_state_hash, &first)
        .await
        .expect("the mint executes");
    let give = r#"new slot in {
        for (kind <<- @"pub") { {% slot :: slot :: () %}[kind] } | @"slot"!(*slot)
    }"#;
    let second = block(&proposer, vec![deploy(&alice, give.to_string(), 2)], 2);
    executor
        .execute(
            casper::rust::v6::record::ExecutionRecord::post_root(&minted),
            &second,
        )
        .await
        .expect("the transfer executes");

    let (number, audits) = conservation.last().expect("the audit ran");
    assert_eq!(number, 2);
    assert_eq!(conservation.failures(), 0);
    assert!(rev(&audits).conserved());
    let [kind] = audits
        .iter()
        .filter(|a| !a.kind.is_rev())
        .collect::<Vec<_>>()[..]
    else {
        panic!("one issued kind: {audits:?}");
    };
    assert_eq!(kind.minted, 3);
    assert_eq!(kind.held, 3);
    assert_eq!(kind.wallets, 2);
    assert!(kind.conserved());
}

async fn memory_store() -> Arc<dyn shared::rust::store::key_value_store::KeyValueStore> {
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new()
        .store("v6-executor-state".to_string())
        .await
        .unwrap()
}

#[tokio::test]
async fn replaying_a_minting_block_counts_its_mints_once() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let conservation = Conservation::new(
        AuditSchedule { interval: 1 },
        WalletIndex::from_genesis(&genesis),
    );
    let executor = BlockExecutor::new(rm.clone(), 2).with_conservation(conservation.clone());
    let mint = r#"new k, d(`rho:system:deployerId`) in { {% d :: d :: d :: () %}[k] }"#;
    let first = block(&proposer, vec![deploy(&alice, mint.to_string(), 1)], 1);
    let record = executor
        .execute(&genesis.body.state.post_state_hash, &first)
        .await
        .expect("the mint executes");
    executor
        .replay(&record, &first)
        .await
        .expect("the mint replays");
    let (_, audits) = conservation.last().expect("the audit ran");
    let kind = audits
        .iter()
        .find(|a| !a.kind.is_rev())
        .expect("the minted kind is audited");
    assert_eq!(kind.minted, 3);
    assert!(kind.conserved(), "{kind:?}");
    assert_eq!(conservation.failures(), 0);
}

#[tokio::test]
async fn the_wallet_index_survives_a_restart() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let store = memory_store().await;
    let schedule = AuditSchedule { interval: 1 };
    let conservation = Conservation::open(schedule, &genesis, store.clone()).unwrap();
    let executor = BlockExecutor::new(rm.clone(), 2).with_conservation(conservation.clone());
    let paid = block(&proposer, vec![transfer(&alice, &bob, 700)], 1);
    executor
        .execute(&genesis.body.state.post_state_hash, &paid)
        .await
        .expect("the transfer executes");
    let bob_wallet = wallet_channel_id(&owner(&bob));
    assert!(conservation
        .index()
        .wallets(&TokenKind::rev())
        .any(|w| *w == bob_wallet));

    let reopened = Conservation::open(schedule, &genesis, store).unwrap();
    assert_eq!(reopened.index(), {
        let mut index = conservation.index();
        index.take_fresh();
        index
    });
    assert!(reopened
        .index()
        .wallets(&TokenKind::rev())
        .any(|w| *w == bob_wallet));
    let fresh = Conservation::open(schedule, &genesis, memory_store().await).unwrap();
    assert!(!fresh
        .index()
        .wallets(&TokenKind::rev())
        .any(|w| *w == bob_wallet));
}

#[tokio::test]
#[ignore = "measurement: the audit's cost by index size"]
async fn audit_time_by_index_size() {
    let alice = party();
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    for size in [100usize, 1_000, 10_000] {
        let mut index = WalletIndex::from_genesis(&genesis);
        for i in 0..size {
            index.insert(TokenKind::rev(), (i as u64).to_be_bytes().repeat(4));
        }
        let started = std::time::Instant::now();
        let audits = check_at(&rm, &genesis.body.state.post_state_hash, &index)
            .await
            .unwrap();
        println!(
            "wallets={} audit={:?}",
            index.len(&TokenKind::rev()),
            started.elapsed()
        );
        assert!(rev(&audits).conserved());
    }
}
