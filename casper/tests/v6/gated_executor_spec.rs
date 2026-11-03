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
    assert_eq!(balance(&rm, &played.mid_root, &bob).await, 0);
    assert_eq!(
        balance(&rm, &played.mid_root, &alice).await,
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
    let settled = &played.settled_root;
    assert_eq!(balance(&rm, settled, &bob).await, 6_000);
    let paid = played.settlement.debited();
    assert!(paid > 0);
    assert_eq!(balance(&rm, settled, &alice).await, 4_000 - paid);
    assert_eq!(played.settlement.transfers.len(), 1);
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
            v6: None,
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

#[tokio::test]
async fn the_record_gives_the_block_s_rev_transfers() {
    use casper::rust::v6::backend::ExecutionBackend;
    use casper::rust::v6::block_record::BlockRecord;
    use casper::rust::v6::executor::BlockExecutor;
    use casper::rust::v6::history::{transfers, wallet_history, TransferKind};
    use casper::rust::v6::record::ExecutionRecord;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
    };
    use prost::bytes::Bytes;
    use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;

    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let b = BlockMessage {
        block_hash: Bytes::from_static(b"v6-history-block"),
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
            deploys: vec![
                ProcessedDeploy::empty(envelope(TWO_COMMS, 1, &alice, 1_000, 3)),
                ProcessedDeploy::empty(envelope(ONE_COMM, 2, &bob, 1_000, 2)),
            ],
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
    };
    let record = executor.execute(&pre, &b).await.unwrap();
    let record = BlockRecord::decode(&record.encode()).unwrap();

    let all = transfers(&record);
    let settlement = record.settlement().unwrap();
    let moved: u64 = all.iter().map(|t| t.amount).sum();
    assert_eq!(moved, settlement.debited());
    let to_proposer: u64 = all
        .iter()
        .filter(|t| t.kind == TransferKind::CommCharge)
        .map(|t| t.amount)
        .sum();
    assert_eq!(to_proposer, settlement.proposer_credit);
    assert_eq!(to_proposer, 2 * 3 + 2);
    let compute = rholang::rust::interpreter::accounting::rev_stack::wallet_channel_id(
        &compute_wallet_owner(),
    );
    assert!(all
        .iter()
        .filter(|t| t.kind == TransferKind::ComputeCharge)
        .all(|t| t.to == compute));

    let alices = wallet_history([&record], &wallet(&alice));
    assert!(alices.iter().all(|t| t.from == wallet(&alice)));
    assert_eq!(
        alices.iter().map(|t| t.amount).sum::<u64>(),
        settlement.debits[&wallet(&alice)]
    );
    let proposers = wallet_history([&record], &wallet(&proposer));
    assert_eq!(proposers.len(), 2);
    assert!(balance(&rm, record.post_root(), &proposer).await >= to_proposer);
}

#[tokio::test]
async fn a_user_transfer_and_bond_survive_play_and_replay() {
    use casper::rust::v6::backend::ExecutionBackend;
    use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
    use casper::rust::v6::executor::BlockExecutor;
    use casper::rust::v6::genesis_wallets::read_bond_balance;
    use casper::rust::v6::history::{transfers, TransferKind};
    use casper::rust::v6::record::ExecutionRecord;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
    };
    use prost::bytes::Bytes;
    use rholang::rust::interpreter::rho_type::RhoBoolean;

    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let term = format!(
        r#"new w(`rho:rev:wallet`), rb(`rho:rev:bond`), deployerId(`rho:system:deployerId`),
               rl(`rho:registry:lookup`), posCh, sent, bonded, registered, handleCh in {{
             w!("transfer", (*deployerId, "{}", 6000), *sent) |
             rb!("bond", (*deployerId, 1000), *bonded) |
             for (@(true, receipt) <- bonded) {{
               rl!(`rho:system:pos`, *posCh) |
               for (@(_, PoS) <- posCh) {{ @PoS!("bond", receipt, 1000, *registered, *handleCh) }}
             }} |
             for (@h <- handleCh) {{ @"handle"!(h) }}
           }}"#,
        bob.address
    );
    let b = BlockMessage {
        block_hash: Bytes::from_static(b"v6-transfer-and-bond"),
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
            deploys: vec![ProcessedDeploy::empty(envelope(
                &term, 1, &alice, 50_000, 1,
            ))],
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
    };

    let record = executor
        .execute(&pre, &b)
        .await
        .expect("the block executes");
    assert_eq!(record.executed()[0].outcome, EnvelopeOutcome::Completed);
    let post = record.post_root().clone();
    let settlement = record.settlement().unwrap().clone();
    assert_eq!(settlement.transfers.len(), 2);
    assert_eq!(balance(&rm, &post, &bob).await, 6_000);
    assert_eq!(
        read_bond_balance(&rm, &post, &alice.pk.bytes)
            .await
            .unwrap(),
        1_000
    );
    assert_eq!(
        balance(&rm, &post, &alice).await,
        100_000 - 7_000 - settlement.debited()
    );
    let users: Vec<_> = transfers(&record)
        .into_iter()
        .filter(|t| t.kind == TransferKind::UserTransfer)
        .collect();
    assert_eq!(users.iter().map(|t| t.amount).sum::<u64>(), 7_000);

    let bonded = format!(
        r#"new ret, rl(`rho:registry:lookup`), posCh, bondsCh in {{
             rl!(`rho:system:pos`, *posCh) |
             for (@(_, PoS) <- posCh) {{
               @PoS!("getBonds", *bondsCh) |
               for (@bonds <- bondsCh) {{ ret!(bonds.contains("{}".hexToBytes())) }}
             }}
           }}"#,
        hex::encode(&alice.pk.bytes)
    );
    let (answer, _) = rm
        .play_exploratory_deploy(bonded, &post, None)
        .await
        .unwrap();
    assert_eq!(RhoBoolean::unapply(&answer[0]), Some(true));
    let handle = rm
        .get_data(post.clone(), &rholang::rust::interpreter::rho_type::RhoString::create_par("handle".to_string()))
        .await
        .unwrap();
    let (admitted, _) = rholang::rust::interpreter::rho_type::RhoTuple2::unapply(&handle[0]).unwrap();
    assert_eq!(RhoBoolean::unapply(&admitted), Some(true));

    let decoded = BlockRecord::decode(&record.encode()).unwrap();
    assert_eq!(executor.replay(&decoded, &b).await.unwrap(), post);
    let mut forged = settlement.clone();
    forged.transfers[0].amount += 1;
    assert!(executor
        .replay(&decoded.clone().with_settlement(forged), &b)
        .await
        .is_err());
}

#[tokio::test]
async fn one_run_per_block_pays_natively_from_a_wallet() {
    let (alice, bob, carol, proposer) = (party(), party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&carol, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let transfer = |to: &Party, amount: u64| {
        format!(
            r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`), out in {{
                 w!("transfer", (*deployerId, "{}", {amount}), *out)
               }}"#,
            to.address
        )
    };
    let envelopes = vec![
        envelope(&transfer(&bob, 1_000), 1, &alice, 1_000, 1),
        envelope(&transfer(&bob, 2_000), 2, &alice, 1_000, 1),
        envelope(&transfer(&bob, 3_000), 3, &carol, 1_000, 1),
    ];
    let played = executor
        .execute(&pre, envelopes, block_data(&proposer))
        .await
        .unwrap();
    assert_eq!(played.gate.admitted(), vec![true, true, true]);
    assert_eq!(played.merged, vec![true, false, true]);
    let moved: Vec<u64> = played
        .settlement
        .transfers
        .iter()
        .map(|t| t.amount)
        .collect();
    assert_eq!(moved, vec![1_000, 3_000]);
    let settled = &played.settled_root;
    assert_eq!(balance(&rm, settled, &bob).await, 4_000);
    let debits = &played.settlement.debits;
    assert_eq!(
        balance(&rm, settled, &alice).await,
        100_000 - 1_000 - debits[&wallet(&alice)]
    );

    let again = replay(&executor, &pre, &played, block_data(&proposer)).await;
    assert_eq!(again.merged, played.merged);
    assert_eq!(again.settled_root, played.settled_root);
    assert_eq!(again.settlement, played.settlement);
}

#[tokio::test]
async fn parallel_transfers_that_overdraw_together_fail_the_run_in_play_and_replay() {
    use casper::rust::v6::backend::ExecutionBackend;
    use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
    use casper::rust::v6::executor::BlockExecutor;
    use casper::rust::v6::record::ExecutionRecord;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
    };
    use prost::bytes::Bytes;

    let (alice, bob, carol, proposer) = (party(), party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let term = format!(
        r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`), toBob, toCarol in {{
             w!("transfer", (*deployerId, "{}", 30000), *toBob) |
             w!("transfer", (*deployerId, "{}", 30000), *toCarol)
           }}"#,
        bob.address, carol.address
    );
    let b = BlockMessage {
        block_hash: Bytes::from_static(b"v6-overdraw"),
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
            deploys: vec![ProcessedDeploy::empty(envelope(
                &term, 1, &alice, 50_000, 1,
            ))],
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
    };

    let record = executor
        .execute(&pre, &b)
        .await
        .expect("the block executes");
    assert_eq!(record.executed()[0].outcome, EnvelopeOutcome::Overdrawn);
    let post = record.post_root().clone();
    let settlement = record.settlement().unwrap().clone();
    assert!(settlement.transfers.is_empty());
    assert!(settlement.debited() > 0);
    assert_eq!(balance(&rm, &post, &bob).await, 0);
    assert_eq!(balance(&rm, &post, &carol).await, 0);
    assert_eq!(
        balance(&rm, &post, &alice).await,
        100_000 - settlement.debited()
    );

    let decoded = BlockRecord::decode(&record.encode()).unwrap();
    assert_eq!(executor.replay(&decoded, &b).await.unwrap(), post);
}

#[tokio::test]
async fn a_self_sealed_envelope_is_paid_from_its_process_wallet_in_play_and_replay() {
    use casper::rust::v6::backend::ExecutionBackend;
    use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
    use casper::rust::v6::executor::BlockExecutor;
    use casper::rust::v6::record::ExecutionRecord;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
    };
    use prost::bytes::Bytes;
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::accounting::rev_stack::{wallet_channel_id, RevStacks};
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let (alice, proposer) = (party(), party());
    let (rm, genesis_root) = shard(&[]).await;
    let process = process_owner(&Compiler::source_to_adt(r#"@"x"!(1)"#).unwrap());
    let mut runtime = rm.spawn_runtime().await.unwrap();
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(&genesis_root))
        .await
        .unwrap();
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
        .deposit(&process, 10_000)
        .unwrap();
    let pre = runtime.create_checkpoint().await.root.to_bytes_prost();

    let executor = BlockExecutor::new(rm.clone(), 2);
    let term = r#"{% @"x"!(1) %}[# { @"x"!(1) }]"#;
    let b = BlockMessage {
        block_hash: Bytes::from_static(b"v6-self-sealed"),
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
            deploys: vec![ProcessedDeploy::empty(envelope(
                term, 1, &alice, 1_000, 1,
            ))],
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
    };

    let record = executor
        .execute(&pre, &b)
        .await
        .expect("the block executes");
    assert_eq!(record.executed()[0].outcome, EnvelopeOutcome::Completed);
    let settlement = record.settlement().unwrap().clone();
    assert!(settlement.debited() > 0);
    assert!(settlement
        .charges
        .iter()
        .all(|c| c.wallet == wallet_channel_id(&process)));
    let post = record.post_root().clone();
    assert_eq!(balance(&rm, &post, &alice).await, 0);
    assert_eq!(
        read_wallet_balance(&rm, &post, &process).await.unwrap(),
        10_000 - settlement.debited()
    );

    let decoded = BlockRecord::decode(&record.encode()).unwrap();
    assert_eq!(executor.replay(&decoded, &b).await.unwrap(), post);
}

async fn shard_with_authority(
    funded: &[(&Party, u128)],
    members: &[&Party],
    quorum: u32,
) -> (Arc<RuntimeManager>, StateHash) {
    let rm = mk_runtime_manager("v6-funders", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = funded
        .iter()
        .map(|(p, balance)| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: *balance,
        })
        .collect();
    genesis.proof_of_stake.pos_multi_sig_public_keys =
        members.iter().map(|m| hex::encode(&m.pk.bytes)).collect();
    genesis.proof_of_stake.pos_multi_sig_quorum = quorum;
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    (Arc::new(rm), block.body.state.post_state_hash.clone())
}

#[tokio::test]
async fn funders_are_drawn_pro_rata_and_only_the_authority_refuses_a_share_in_play_and_replay() {
    use casper::rust::v6::genesis_wallets::cells_at;
    use models::rust::casper::protocol::casper_message::Cosignature;
    use rholang::rust::interpreter::accounting::rev_stack::funders::share;
    use rholang::rust::interpreter::accounting::rev_stack::journal::Source;
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::compiler::compiler::Compiler;

    let (alice, carol, bob, dave, erin, proposer) =
        (party(), party(), party(), party(), party(), party());
    let (rm, genesis) = shard_with_authority(
        &[(&alice, 100_000), (&carol, 100_000), (&bob, 100_000), (&dave, 100_000)],
        &[&bob, &dave],
        2,
    )
    .await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let process = process_owner(&Compiler::source_to_adt(r#"@"x"!(1)"#).unwrap());
    let fund = |amount: u64| {
        format!(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), ack in {{
                 w!("fund", (*d, {{ @"x"!(1) }}, {amount}), *ack)
               }}"#
        )
    };
    let shares_at = |root: StateHash| {
        let rm = rm.clone();
        let (alice, carol) = (alice.owner.clone(), carol.owner.clone());
        let process = process.clone();
        async move {
            let cells = cells_at(&rm, &root).await.unwrap();
            (
                share(&cells, &process, &alice).unwrap(),
                share(&cells, &process, &carol).unwrap(),
                read_wallet_balance(&rm, &root, &process).await.unwrap(),
            )
        }
    };

    let first = executor
        .execute(
            &genesis,
            vec![
                envelope(&fund(3_000), 1, &alice, 50_000, 1),
                envelope(&fund(1_000), 2, &carol, 50_000, 1),
            ],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert_eq!(first.merged, vec![true, true]);
    assert_eq!(
        replay(&executor, &genesis, &first, block_data(&proposer)).await.settled_root,
        first.settled_root
    );
    assert_eq!(shares_at(first.settled_root.clone()).await, (3_000, 1_000, 4_000));

    let sealed = r#"{% @"x"!(1) %}[# { @"x"!(1) }]"#;
    let second = executor
        .execute(
            &first.settled_root,
            vec![envelope(sealed, 3, &erin, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .unwrap();
    let drawn = second.settlement.debited();
    assert!(drawn > 0);
    let (a, c, held) = shares_at(second.settled_root.clone()).await;
    assert_eq!(a + c, 4_000 - drawn);
    assert_eq!(held, 4_000 - drawn);
    let (from_alice, from_carol) = (3_000 - a, 1_000 - c);
    assert!(from_alice.abs_diff(3 * from_carol) <= 3, "{from_alice} {from_carol}");
    assert_eq!(
        replay(&executor, &first.settled_root, &second, block_data(&proposer))
            .await
            .settled_root,
        second.settled_root
    );

    let refuse = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`), ack in {{ {{% w!("refuse", ({{ @"x"!(1) }}, "{}"), *ack) %}}[d (*) c] }}"#,
        carol.address
    );
    let by_bob = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), ack in {{ {{% w!("refuse", ({{ @"x"!(1) }}, "{}"), *ack) %}}[d] }}"#,
        carol.address
    );
    let alone = executor
        .execute(
            &second.settled_root,
            vec![envelope(&by_bob, 4, &bob, 50_000, 1)],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert!(alone.settlement.transfers.is_empty());

    let unsealed = format!(
        r#"new w(`rho:rev:wallet`), ack in {{ w!("refuse", ({{ @"x"!(1) }}, "{}"), *ack) }}"#,
        carol.address
    );
    let mut unsealed_data = envelope(&unsealed, 6, &bob, 50_000, 1).data;
    unsealed_data.required_cosigners = vec![dave.pk.bytes.to_vec()];
    let mut cosigned_only =
        Signed::create(unsealed_data, Box::new(Secp256k1), bob.sk.clone()).unwrap();
    cosigned_only
        .data
        .cosignatures
        .push(Cosignature::sign(&cosigned_only, &dave.sk, &Secp256k1.name()).unwrap());
    let outside = executor
        .execute(&second.settled_root, vec![cosigned_only], block_data(&proposer))
        .await
        .unwrap();
    assert!(outside.settlement.transfers.is_empty());

    let mut quorum_data = envelope(&refuse, 5, &bob, 50_000, 1).data;
    quorum_data.required_cosigners = vec![dave.pk.bytes.to_vec()];
    let mut by_quorum = Signed::create(quorum_data, Box::new(Secp256k1), bob.sk.clone()).unwrap();
    by_quorum
        .data
        .cosignatures
        .push(Cosignature::sign(&by_quorum, &dave.sk, &Secp256k1.name()).unwrap());
    let third = executor
        .execute(&second.settled_root, vec![by_quorum], block_data(&proposer))
        .await
        .unwrap();
    assert_eq!(third.settlement.transfers.len(), 1);
    assert!(matches!(third.settlement.transfers[0].from, Source::Share { .. }));
    assert_eq!(third.settlement.transfers[0].amount, c);
    assert_eq!(
        balance(&rm, &third.settled_root, &carol).await,
        balance(&rm, &second.settled_root, &carol).await + c
    );
    assert_eq!(shares_at(third.settled_root.clone()).await, (a, 0, a));
    assert_eq!(
        replay(&executor, &second.settled_root, &third, block_data(&proposer))
            .await
            .settled_root,
        third.settled_root
    );
}


#[tokio::test]
async fn a_user_deploy_cannot_reach_the_genesis_mint_through_the_registry_lookup() {
    use casper::rust::v6::genesis_wallets::read_genesis_supply;

    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000)]).await;
    let supply = read_genesis_supply(&rm, &genesis).await.unwrap();
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let leak = |command: &str, arg: String| {
        format!(
            r#"new lookup(`rho:internal:registry_lookup`), ret in {{
                 lookup!("rho:rev:genesis", *ret) |
                 for (g <- ret) {{ g!("{command}", {arg}, "out") }}
               }}"#
        )
    };
    let played = executor
        .execute(
            &genesis,
            vec![
                envelope(
                    &leak("mint", format!(r#"[("{}", 1000000)]"#, alice.address)),
                    1,
                    &alice,
                    50_000,
                    1,
                ),
                envelope(
                    &leak(
                        "bonds",
                        format!(
                            r#"[("{}".hexToBytes(), 1000000)]"#,
                            hex::encode(&alice.pk.bytes)
                        ),
                    ),
                    2,
                    &alice,
                    50_000,
                    1,
                ),
            ],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert!(played.runs.iter().all(|r| r.failed));
    let post = played.settled_root.clone();
    assert_eq!(read_genesis_supply(&rm, &post).await.unwrap(), supply);
    assert_eq!(
        balance(&rm, &post, &alice).await,
        100_000 - played.settlement.debited()
    );
    assert_eq!(
        replay(&executor, &genesis, &played, block_data(&proposer)).await.settled_root,
        post
    );
}

#[tokio::test]
async fn a_grant_settles_draws_inside_its_window_in_play_and_replay() {
    let (sponsor, newcomer, proposer) = (party(), party(), party());
    let (rm, genesis) = shard(&[(&sponsor, 100_000), (&newcomer, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let at = |block_number: i64| BlockData {
        time_stamp: 1,
        block_number,
        sender: proposer.pk.clone(),
        seq_num: 1,
    };
    let grant = r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), slot, funded in {
        w!("fundGrant", (*d, (*slot, 2, 5), 3000), *funded) |
        for (_ <- funded) { @"grant-slot"!(*slot) }
      }"#;
    let first = executor
        .execute(&genesis, vec![envelope(grant, 1, &sponsor, 50_000, 1)], at(1))
        .await
        .unwrap();
    assert_eq!(first.settlement.transfers.len(), 1);
    let draw = format!(
        r#"new w(`rho:rev:wallet`), out in {{
             for (@slot <<- @"grant-slot") {{ w!("drawGrant", ((slot, 2, 5), "{}", 1000), *out) }}
           }}"#,
        newcomer.address
    );
    let inside = executor
        .execute(
            &first.settled_root,
            vec![envelope(&draw, 2, &newcomer, 50_000, 1)],
            at(3),
        )
        .await
        .unwrap();
    assert_eq!(inside.settlement.transfers.len(), 1);
    assert_eq!(inside.settlement.transfers[0].amount, 1_000);
    assert_eq!(
        replay(&executor, &first.settled_root, &inside, at(3)).await.settled_root,
        inside.settled_root
    );
    let late = executor
        .execute(
            &inside.settled_root,
            vec![envelope(&draw, 3, &newcomer, 50_000, 1)],
            at(9),
        )
        .await
        .unwrap();
    assert!(late.settlement.transfers.is_empty());
    assert_eq!(
        replay(&executor, &inside.settled_root, &late, at(9)).await.settled_root,
        late.settled_root
    );
}

#[tokio::test]
async fn a_stack_in_a_self_sealed_region_draws_on_the_funders_shares_pro_rata() {
    use casper::rust::v6::genesis_wallets::cells_at;
    use models::rust::casper::protocol::casper_message::Cosignature;
    use rholang::rust::interpreter::accounting::rev_stack::funders::{share, shares};
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::compiler::compiler::Compiler;

    let (alice, carol, bob, dave, erin, proposer) =
        (party(), party(), party(), party(), party(), party());
    let (rm, genesis) = shard_with_authority(
        &[(&alice, 100_000), (&carol, 100_000), (&bob, 100_000), (&dave, 100_000)],
        &[&bob, &dave],
        2,
    )
    .await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let cells = vec![r#"# { @"sink"!(1) }"#; 20].join(" :: ");
    let p = format!("{cells} :: ()");
    let sink = process_owner(&Compiler::source_to_adt(r#"@"sink"!(1)"#).unwrap());
    let process = process_owner(&Compiler::source_to_adt(&p).unwrap());
    let fund = |amount: u64| {
        format!(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), ack in {{
                 w!("fund", (*d, {{ {p} }}, {amount}), *ack)
               }}"#
        )
    };
    let record = |root: StateHash| {
        let (rm, process) = (rm.clone(), process.clone());
        let (a, c) = (alice.owner.clone(), carol.owner.clone());
        async move {
            let cells = cells_at(&rm, &root).await.unwrap();
            let held: u64 = shares(&cells, &process).unwrap().iter().map(|(_, s)| s).sum();
            (
                share(&cells, &process, &a).unwrap(),
                share(&cells, &process, &c).unwrap(),
                held,
                read_wallet_balance(&rm, &root, &process).await.unwrap(),
            )
        }
    };

    let funded = executor
        .execute(
            &genesis,
            vec![
                envelope(&fund(3_000), 1, &alice, 50_000, 1),
                envelope(&fund(1_000), 2, &carol, 50_000, 1),
            ],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert_eq!(record(funded.settled_root.clone()).await, (3_000, 1_000, 4_000, 4_000));

    let sealed = format!("{{% {p} %}}[# {{ {p} }}]");
    let spent = executor
        .execute(
            &funded.settled_root,
            vec![envelope(&sealed, 3, &erin, 3_000, 1)],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert!(
        spent.merged[0],
        "gate {:?} failed {} overdrawn {} phlo {}",
        spent.gate.entries[0].verdict,
        spent.runs[0].failed,
        spent.overdrawn[0],
        spent.runs[0].phlo_used()
    );
    let charge = spent.settlement.debited();
    let (a, c, held, wallet) = record(spent.settled_root.clone()).await;
    assert_eq!(read_wallet_balance(&rm, &spent.settled_root, &sink).await.unwrap(), 20);
    assert_eq!(wallet, 4_000 - 20 - charge);
    assert_eq!(held, wallet);
    assert_eq!(a + c, held);
    assert!((3_000 - a).abs_diff(3 * (1_000 - c)) <= 3, "{a} {c}");
    assert_eq!(
        replay(&executor, &funded.settled_root, &spent, block_data(&proposer))
            .await
            .settled_root,
        spent.settled_root
    );

    let refuse = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`), ack in {{ {{% w!("refuse", ({{ {p} }}, "{}"), *ack) %}}[d (*) c] }}"#,
        carol.address
    );
    let mut quorum_data = envelope(&refuse, 4, &bob, 50_000, 1).data;
    quorum_data.required_cosigners = vec![dave.pk.bytes.to_vec()];
    let mut by_quorum = Signed::create(quorum_data, Box::new(Secp256k1), bob.sk.clone()).unwrap();
    by_quorum
        .data
        .cosignatures
        .push(Cosignature::sign(&by_quorum, &dave.sk, &Secp256k1.name()).unwrap());
    let refused = executor
        .execute(&spent.settled_root, vec![by_quorum], block_data(&proposer))
        .await
        .unwrap();
    assert_eq!(refused.settlement.transfers.len(), 1);
    assert_eq!(refused.settlement.transfers[0].amount, c);
    assert_eq!(
        balance(&rm, &refused.settled_root, &carol).await,
        balance(&rm, &spent.settled_root, &carol).await + c
    );
    assert_eq!(record(refused.settled_root.clone()).await, (a, 0, a, a));
    assert_eq!(
        replay(&executor, &spent.settled_root, &refused, block_data(&proposer))
            .await
            .settled_root,
        refused.settled_root
    );
}

#[tokio::test]
async fn an_expired_grant_returns_its_funders_shares_in_play_and_replay() {
    use rholang::rust::interpreter::accounting::rev_stack::journal::Source;

    let (sponsor, carol, newcomer, anyone, proposer) =
        (party(), party(), party(), party(), party());
    let (rm, genesis) = shard(&[
        (&sponsor, 100_000),
        (&carol, 100_000),
        (&newcomer, 100_000),
        (&anyone, 100_000),
    ])
    .await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let at = |block_number: i64| BlockData {
        time_stamp: 1,
        block_number,
        sender: proposer.pk.clone(),
        seq_num: 1,
    };
    let step = |pre: StateHash, envelope: Signed<DeployData>, block: i64| {
        let (executor, rm) = (&executor, rm.clone());
        async move {
            let played = executor.execute(&pre, vec![envelope], at(block)).await.unwrap();
            let again = replay(executor, &pre, &played, at(block)).await;
            assert_eq!(again.settled_root, played.settled_root);
            let _ = rm;
            played
        }
    };
    let first = step(
        genesis.clone(),
        envelope(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), slot, funded in {
                 w!("fundGrant", (*d, (*slot, 2, 5), 3000), *funded) |
                 for (_ <- funded) { @"grant-slot"!!(*slot) }
               }"#,
            1,
            &sponsor,
            50_000,
            1,
        ),
        1,
    )
    .await;
    let second = step(
        first.settled_root.clone(),
        envelope(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), out in {
                 for (@slot <<- @"grant-slot") { w!("fundGrant", (*d, (slot, 2, 5), 1000), *out) }
               }"#,
            2,
            &carol,
            50_000,
            1,
        ),
        2,
    )
    .await;
    assert_eq!(second.settlement.transfers.len(), 1);
    let draw = format!(
        r#"new w(`rho:rev:wallet`), out in {{
             for (@slot <<- @"grant-slot") {{ w!("drawGrant", ((slot, 2, 5), "{}", 1000), *out) }}
           }}"#,
        newcomer.address
    );
    let third = step(
        second.settled_root.clone(),
        envelope(&draw, 3, &newcomer, 50_000, 1),
        3,
    )
    .await;
    assert_eq!(third.settlement.transfers.len(), 1);
    let back = format!(
        r#"new w(`rho:rev:wallet`), a, b in {{
             for (@slot <<- @"grant-slot") {{
               w!("returnGrant", ((slot, 2, 5), "{}"), *a) |
               w!("returnGrant", ((slot, 2, 5), "{}"), *b)
             }}
           }}"#,
        sponsor.address, carol.address
    );
    let early = step(
        third.settled_root.clone(),
        envelope(&back, 4, &anyone, 50_000, 1),
        5,
    )
    .await;
    assert!(early.settlement.transfers.is_empty());
    let late = step(
        third.settled_root.clone(),
        envelope(&back, 5, &anyone, 50_000, 1),
        9,
    )
    .await;
    let mut returned: Vec<(bool, u64)> = late
        .settlement
        .transfers
        .iter()
        .map(|t| (matches!(t.from, Source::Share { .. }), t.amount))
        .collect();
    returned.sort();
    assert_eq!(returned, vec![(true, 750), (true, 2_250)]);
    assert_eq!(
        balance(&rm, &late.settled_root, &sponsor).await,
        balance(&rm, &third.settled_root, &sponsor).await + 2_250
    );
    assert_eq!(
        balance(&rm, &late.settled_root, &carol).await,
        balance(&rm, &third.settled_root, &carol).await + 750
    );
}

#[tokio::test]
async fn a_process_with_new_hashes_the_same_in_every_deploy_and_is_funded_across_deploys() {
    use casper::rust::v6::genesis_wallets::cells_at;
    use models::rust::normalizer_env::with_deployer_id;
    use rholang::rust::interpreter::accounting::rev_stack::funders::{share, shares};
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::compiler::compiler::Compiler;

    let (alice, carol, bob, proposer) = (party(), party(), party(), party());
    let (rm, genesis) = shard_with_authority(
        &[(&alice, 100_000), (&carol, 100_000), (&bob, 100_000)],
        &[&bob],
        1,
    )
    .await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let p = r#"new x in { x!(1) | for (_ <- x) { @"sink"!(1) } }"#;
    let under = |party: &Party| {
        process_owner(&Compiler::source_to_adt_with_normalizer_env(p, with_deployer_id(&party.pk)).unwrap())
    };
    let process = process_owner(&Compiler::source_to_adt(p).unwrap());
    assert_eq!(under(&alice), process);
    assert_eq!(under(&carol), process);
    let fund = |amount: u64| {
        format!(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), ack in {{
                 w!("fund", (*d, {{ {p} }}, {amount}), *ack)
               }}"#
        )
    };
    let record = |root: StateHash| {
        let (rm, process) = (rm.clone(), process.clone());
        let (a, c) = (alice.owner.clone(), carol.owner.clone());
        async move {
            let cells = cells_at(&rm, &root).await.unwrap();
            let held: u64 = shares(&cells, &process).unwrap().iter().map(|(_, s)| s).sum();
            (
                share(&cells, &process, &a).unwrap(),
                share(&cells, &process, &c).unwrap(),
                held,
                read_wallet_balance(&rm, &root, &process).await.unwrap(),
            )
        }
    };

    let first = executor
        .execute(&genesis, vec![envelope(&fund(3_000), 1, &alice, 50_000, 1)], block_data(&proposer))
        .await
        .unwrap();
    assert_eq!(record(first.settled_root.clone()).await, (3_000, 0, 3_000, 3_000));
    let second = executor
        .execute(
            &first.settled_root,
            vec![envelope(&fund(1_000), 2, &carol, 50_000, 1)],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert_eq!(record(second.settled_root.clone()).await, (3_000, 1_000, 4_000, 4_000));
    assert_eq!(
        replay(&executor, &first.settled_root, &second, block_data(&proposer))
            .await
            .settled_root,
        second.settled_root
    );
}
