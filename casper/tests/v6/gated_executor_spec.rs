use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::executor::{EnvelopeExecutor, ExecutedBlock};
use casper::rust::v6::genesis_wallets::read_wallet_balance;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use casper::rust::v6::settle::Boxcar;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::DeployData;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

struct Party {
    sk: PrivateKey,
    pk: PublicKey,
    owner: WalletOwner,
    address: String,
}

fn party() -> Party {
    let (sk, pk): (PrivateKey, PublicKey) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    Party {
        sk,
        pk,
        owner,
        address,
    }
}

async fn shard(funded: &[(&Party, u128)]) -> (Arc<RuntimeManager>, StateHash) {
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
    (Arc::new(rm), block.body.state.post_state_hash.clone())
}

fn envelope(term: &str, ts: i64, who: &Party, limit: i64, price: i64) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        term.to_string(),
        ts,
        Some(limit),
        Some(price),
        Some(who.sk.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap()
}

fn block_data(proposer: &Party) -> BlockData {
    BlockData {
        time_stamp: 1,
        block_number: 1,
        sender: proposer.pk.clone(),
        seq_num: 1,
    }
}

const TWO_COMMS: &str = "new x in { x!(1) | for (_ <- x) { Nil } | x!(2) | for (_ <- x) { Nil } }";
const ONE_COMM: &str = "new y in { y!(3) | for (_ <- y) { Nil } }";

async fn balance(rm: &RuntimeManager, root: &StateHash, who: &Party) -> u64 {
    read_wallet_balance(rm, root, &who.owner).await.unwrap()
}

async fn replay(
    executor: &EnvelopeExecutor,
    pre: &StateHash,
    played: &ExecutedBlock,
    data: BlockData,
) -> ExecutedBlock {
    executor
        .replay(pre, played.processed.clone(), &played.gate.entries, data)
        .await
        .expect("the block replays")
}

fn wallet(who: &Party) -> Vec<u8> {
    rholang::rust::interpreter::accounting::rev_stack::wallet_channel_id(&who.owner)
}

#[tokio::test]
async fn comms_are_paid_to_the_proposer_in_the_canonical_form() {
    let (alice, bob, carol, proposer) = (party(), party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 50_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let envelopes = vec![
        envelope(TWO_COMMS, 1, &alice, 1_000, 3),
        envelope(ONE_COMM, 2, &alice, 1_000, 5),
        envelope(ONE_COMM, 3, &bob, 1_000, 7),
        envelope(ONE_COMM, 4, &carol, 1_000, 1),
    ];
    let played = executor
        .execute(&pre, envelopes, block_data(&proposer))
        .await
        .expect("the block executes");

    assert_eq!(played.gate.admitted(), vec![true, true, true, false]);
    assert_eq!(played.merged, vec![true, true, true, false]);
    let comm: Vec<_> = played
        .settlement
        .charges
        .iter()
        .map(|c| c.comm_rev)
        .collect();
    assert_eq!(comm, vec![2 * 3, 5, 7]);
    assert!(played.settlement.is_balanced());

    let settled = &played.settled_root;
    let debits = &played.settlement.debits;
    assert_eq!(
        balance(&rm, settled, &alice).await,
        100_000 - debits[&wallet(&alice)]
    );
    assert_eq!(
        balance(&rm, settled, &bob).await,
        50_000 - debits[&wallet(&bob)]
    );
    assert_eq!(balance(&rm, settled, &carol).await, 0);
    assert_eq!(
        balance(&rm, settled, &proposer).await,
        played.settlement.proposer_credit
    );

    let again = replay(&executor, &pre, &played, block_data(&proposer)).await;
    assert_eq!(again.settled_root, played.settled_root);
    assert_eq!(again.settlement, played.settlement);
}

#[tokio::test]
async fn the_box_car_size_never_changes_the_state() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let envelopes = || {
        vec![
            envelope(TWO_COMMS, 1, &alice, 1_000, 3),
            envelope(TWO_COMMS, 2, &alice, 1_000, 2),
        ]
    };
    let mut roots = Vec::new();
    for boxcar in [Boxcar::unbounded(), Boxcar::of(1), Boxcar::of(3)] {
        let executor = EnvelopeExecutor::new(rm.clone(), 2).with_boxcar(boxcar);
        let played = executor
            .execute(&pre, envelopes(), block_data(&proposer))
            .await
            .unwrap();
        let again = replay(&executor, &pre, &played, block_data(&proposer)).await;
        assert_eq!(again.settled_root, played.settled_root);
        roots.push(played.settled_root);
    }
    assert!(roots.windows(2).all(|w| w[0] == w[1]));
}

#[tokio::test]
async fn a_run_cannot_spend_the_rev_its_block_has_committed() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 10_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let transfer = |amount: u64, out: &str| {
        format!(
            r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
                 w!("transfer", (*deployerId, "{}", {amount}), "{out}")
               }}"#,
            bob.address
        )
    };
    let envelopes = vec![
        envelope(&transfer(8_000, "too-much"), 1, &alice, 1_000, 3),
        envelope(ONE_COMM, 2, &alice, 1_000, 1),
    ];
    let played = executor
        .execute(&pre, envelopes, block_data(&proposer))
        .await
        .unwrap();
    assert_eq!(played.gate.admitted(), vec![true, true]);
    assert_eq!(balance(&rm, &played.runs[0].post_root, &bob).await, 0);
    assert_eq!(
        balance(&rm, &played.runs[0].post_root, &alice).await,
        10_000
    );
    let paid = played.settlement.debited();
    assert_eq!(
        balance(&rm, &played.settled_root, &alice).await,
        10_000 - paid
    );

    let envelopes = vec![envelope(&transfer(6_000, "fits"), 3, &alice, 1_000, 3)];
    let played = executor
        .execute(&pre, envelopes, block_data(&proposer))
        .await
        .unwrap();
    let run = &played.runs[0].post_root;
    assert_eq!(balance(&rm, run, &bob).await, 6_000);
    assert_eq!(balance(&rm, run, &alice).await, 4_000);
    assert!(played.settlement.debited() > 0);
}

#[tokio::test]
async fn a_block_record_carries_the_gate_and_replays_it() {
    use casper::rust::v6::backend::ExecutionBackend;
    use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
    use casper::rust::v6::executor::BlockExecutor;
    use casper::rust::v6::gate::Verdict;
    use casper::rust::v6::record::ExecutionRecord;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
    };
    use prost::bytes::Bytes;

    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let deploys = vec![
        ProcessedDeploy::empty(envelope(TWO_COMMS, 1, &alice, 1_000, 3)),
        ProcessedDeploy::empty(envelope(ONE_COMM, 2, &bob, 1_000, 1)),
        ProcessedDeploy::empty(envelope(TWO_COMMS, 3, &alice, 5, 2)),
    ];
    let b = BlockMessage {
        block_hash: Bytes::from_static(b"v6-gated-block"),
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
                block_number: 1,
            },
            deploys,
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
        },
        justifications: vec![],
        sender: proposer.pk.bytes.clone(),
        seq_num: 1,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    };
    let record = executor
        .execute(&pre, &b)
        .await
        .expect("the block executes");
    let outcomes: Vec<_> = record.executed().iter().map(|e| e.outcome).collect();
    assert_eq!(
        outcomes,
        vec![
            EnvelopeOutcome::Completed,
            EnvelopeOutcome::NotRun,
            EnvelopeOutcome::Exhausted,
        ]
    );
    assert_eq!(record.gate().len(), 3);
    let settlement = record
        .settlement()
        .expect("the record carries the settlement");
    assert!(settlement.is_balanced());
    assert_eq!(
        settlement.charged_phlo(&b.body.deploys[2].deploy.sig),
        Some(5)
    );
    let policy = casper::rust::v6::resource_policy::ShardResourcePolicy::default();
    let deltas: u64 = record.gate().iter().map(|g| g.delta).sum();
    let bound = policy
        .record_bound(deltas, record.gate().len() as u64)
        .unwrap();
    assert!(
        (record.encode().len() as u128) <= bound,
        "{} > {bound}",
        record.encode().len()
    );
    let without_close = BlockRecord::new(
        record.target().clone(),
        record.pre_root().clone(),
        record.mid_root().clone(),
        record.post_root().clone(),
        record.entries().to_vec(),
        Vec::new(),
    )
    .unwrap()
    .with_gate(record.gate().to_vec())
    .unwrap()
    .with_settlement(settlement.clone());
    assert!((without_close.encode().len() as u128) <= bound);
    assert!(balance(&rm, record.post_root(), &proposer).await > 0);

    let decoded = BlockRecord::decode(&record.encode()).unwrap();
    assert_eq!(decoded, record);
    assert_eq!(
        &executor.replay(&decoded, &b).await.unwrap(),
        record.post_root()
    );

    let mut tampered = settlement.clone();
    tampered.charges[0].comm_phlo += 1;
    tampered.charges[0].comm_rev += tampered.charges[0].phlo_price;
    tampered.proposer_credit += tampered.charges[0].phlo_price;
    let forged = record.clone().with_settlement(tampered);
    assert!(BlockRecord::decode(&forged.encode()).is_ok());
    assert!(executor.replay(&forged, &b).await.is_err());

    let mut gate = record.gate().to_vec();
    gate[1].verdict = Verdict::Admitted;
    let forged = BlockRecord::new(
        b.block_hash.clone(),
        record.pre_root().clone(),
        record.mid_root().clone(),
        record.post_root().clone(),
        record.entries().to_vec(),
        record.system_deploys().to_vec(),
    )
    .unwrap()
    .with_gate(gate)
    .unwrap();
    assert!(executor.replay(&forged, &b).await.is_err());
}
