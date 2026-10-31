use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::executor::EnvelopeExecutor;
use casper::rust::v6::gate::{Refusal, Verdict};
use casper::rust::v6::genesis_wallets::read_wallet_balance;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{Cosignature, DeployData};
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use rholang::rust::interpreter::system_processes::BlockData;
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

async fn shard(funded: &[(&Party, u128)]) -> (Arc<RuntimeManager>, StateHash) {
    let rm = mk_runtime_manager("v6-region", Some(Genesis::default_mergeable_tags_arc())).await;
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

async fn balance(rm: &RuntimeManager, root: &StateHash, who: &Party) -> u64 {
    read_wallet_balance(rm, root, &who.owner).await.unwrap()
}

fn wallet(who: &Party) -> Vec<u8> {
    rholang::rust::interpreter::accounting::rev_stack::wallet_channel_id(&who.owner)
}

const PLAIN: &str = r#"new d(`rho:system:deployerId`) in {
    new x in { x!(1) | for (_ <- x) { Nil } }
}"#;

const SIGNED: &str = r#"new d(`rho:system:deployerId`) in {
    {% new x in { x!(1) | for (_ <- x) { Nil } } %}[d]
}"#;

const UNIFORM: &str = r#"new d(`rho:system:deployerId`), x in {
    x!(1) | {% for (_ <- x) { Nil } %}[d]
}"#;

const FOREIGN: &str = r#"new s in { {% new x in { x!(1) | for (_ <- x) { Nil } } %}[s] }"#;

#[tokio::test]
async fn charges_under_the_deployers_signed_term_land_on_the_deployers_wallet() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let envelopes = vec![
        envelope(PLAIN, 1, &alice, 1_000, 3),
        envelope(SIGNED, 2, &bob, 1_000, 3),
        envelope(UNIFORM, 3, &bob, 1_000, 5),
    ];
    let played = executor
        .execute(&pre, envelopes, block_data(&proposer))
        .await
        .expect("the block executes");

    assert_eq!(played.gate.admitted(), vec![true, true, true]);
    assert_eq!(played.merged, vec![true, true, true]);
    let charges = &played.settlement.charges;
    assert_eq!(charges[1].wallet, wallet(&bob));
    assert_eq!(charges[1].comm_phlo, charges[0].comm_phlo);
    assert_eq!(charges[1].comm_phlo, 1);
    assert_eq!(charges[2].wallet, wallet(&bob));
    assert_eq!(charges[2].comm_phlo, 1);
    assert!(played.settlement.is_balanced());

    let settled = &played.settled_root;
    let debits = &played.settlement.debits;
    assert_eq!(
        balance(&rm, settled, &bob).await,
        100_000 - debits[&wallet(&bob)]
    );
    assert_eq!(
        debits[&wallet(&bob)],
        charges[1].comm_rev + charges[1].compute_rev + charges[2].comm_rev + charges[2].compute_rev
    );

    let again = executor
        .replay(
            &pre,
            played.processed.clone(),
            &played.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the block replays");
    assert_eq!(again.settled_root, played.settled_root);
    assert_eq!(again.settlement, played.settlement);
}

#[tokio::test]
async fn the_gate_refuses_a_signed_term_that_names_another_signature() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let played = executor
        .execute(
            &pre,
            vec![
                envelope(FOREIGN, 1, &alice, 1_000, 3),
                envelope(SIGNED, 2, &alice, 1_000, 3),
            ],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");

    assert_eq!(
        played.gate.entries[0].verdict,
        Verdict::Refused(Refusal::ForeignSignature)
    );
    assert!(played.gate.entries[1].admitted());
    assert_eq!(played.merged, vec![false, true]);
}

#[tokio::test]
async fn a_stack_literal_moves_one_unit_per_cell_and_replays() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let stack = r#"new d(`rho:system:deployerId`), slot in { slot :: slot :: d :: () }"#;
    let played = executor
        .execute(
            &pre,
            vec![envelope(stack, 1, &alice, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");

    assert_eq!(played.gate.admitted(), vec![true]);
    assert!(!played.processed[0].is_failed);
    assert_eq!(
        balance(&rm, &played.runs[0].post_root, &alice).await,
        100_000 - 2
    );
    assert_eq!(played.merged, vec![true]);
    assert!(played.settlement.is_balanced());

    let again = executor
        .replay(
            &pre,
            played.processed.clone(),
            &played.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the block replays");
    assert_eq!(again.settled_root, played.settled_root);
}

#[tokio::test]
#[ignore = "the v6 merge drops native REV writes of a run (reported 2026-10-30)"]
async fn native_rev_writes_of_a_run_survive_the_merge() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 10_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let address = VaultAddress::from_public_key(&bob.pk).unwrap().to_base58();
    let term = format!(
        r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
             w!("transfer", (*deployerId, "{address}", 6000), "out")
           }}"#
    );
    let played = executor
        .execute(
            &pre,
            vec![envelope(&term, 1, &alice, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert_eq!(balance(&rm, &played.runs[0].post_root, &bob).await, 6_000);
    assert_eq!(balance(&rm, &played.settled_root, &bob).await, 6_000);
}

fn cosigned(term: &str, ts: i64, who: &Party, cosigner: &Party, price: i64) -> Signed<DeployData> {
    let mut deploy = envelope(term, ts, who, 1_000, price);
    let cosignature = Cosignature::sign(&deploy.data, &cosigner.sk, &Secp256k1.name()).unwrap();
    deploy.data.cosignatures.push(cosignature);
    deploy
}

const JOINT: &str = r#"new d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in {
    {% new x in { x!(1) | for (_ <- x) { Nil } } %}[d (*) c]
}"#;

#[tokio::test]
async fn each_component_of_a_compound_pays_the_full_charge() {
    let (alice, carol, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&carol, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let played = executor
        .execute(
            &pre,
            vec![cosigned(JOINT, 1, &alice, &carol, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");

    assert_eq!(played.gate.admitted(), vec![true]);
    assert_eq!(played.gate.entries[0].commitments.len(), 2);
    let charges = &played.settlement.charges;
    assert_eq!(charges.len(), 2);
    let mut paid: Vec<_> = charges.iter().map(|c| c.wallet.clone()).collect();
    paid.sort();
    let mut expected = vec![wallet(&alice), wallet(&carol)];
    expected.sort();
    assert_eq!(paid, expected);
    assert_eq!(charges[0].comm_rev, charges[1].comm_rev);
    assert_eq!(charges[0].compute_rev, charges[1].compute_rev);
    assert_eq!(charges[0].comm_phlo, 1);
    assert_eq!(played.settlement.proposer_credit, 2 * charges[0].comm_rev);
    assert!(played.settlement.is_balanced());

    let settled = &played.settled_root;
    let each = charges[0].comm_rev + charges[0].compute_rev;
    assert_eq!(balance(&rm, settled, &alice).await, 100_000 - each);
    assert_eq!(balance(&rm, settled, &carol).await, 100_000 - each);
    assert_eq!(
        balance(&rm, settled, &proposer).await,
        played.settlement.proposer_credit
    );

    let again = executor
        .replay(
            &pre,
            played.processed.clone(),
            &played.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the block replays");
    assert_eq!(again.settled_root, played.settled_root);
    assert_eq!(again.settlement, played.settlement);
}

#[tokio::test]
async fn the_gate_refuses_a_compound_that_a_component_cannot_cover_or_did_not_sign() {
    let (alice, carol, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&carol, 10)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let mut tampered = cosigned(JOINT, 3, &alice, &carol, 3);
    tampered.data.cosignatures[0].sig[4] ^= 1;
    let played = executor
        .execute(
            &pre,
            vec![
                cosigned(JOINT, 1, &alice, &carol, 3),
                envelope(JOINT, 2, &alice, 1_000, 3),
                tampered,
            ],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");

    assert!(matches!(
        played.gate.entries[0].verdict,
        Verdict::Refused(Refusal::InsufficientFunds { .. })
    ));
    assert_eq!(
        played.gate.entries[1].verdict,
        Verdict::Refused(Refusal::ForeignSignature)
    );
    assert_eq!(
        played.gate.entries[2].verdict,
        Verdict::Refused(Refusal::InvalidTerms)
    );
    assert_eq!(balance(&rm, &played.settled_root, &carol).await, 10);
}

async fn seed(
    rm: &RuntimeManager,
    root: &StateHash,
    owner: &WalletOwner,
    amount: u64,
) -> StateHash {
    use rholang::rust::interpreter::accounting::rev_stack::RevStacks;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    let mut runtime = rm.spawn_runtime().await.unwrap();
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(root))
        .await
        .unwrap();
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
        .deposit(owner, amount)
        .unwrap();
    runtime.create_checkpoint().await.root.to_bytes_prost()
}

fn joint(a: &Party, b: &Party) -> WalletOwner {
    WalletOwner::compound(vec![a.owner.clone(), b.owner.clone()]).unwrap()
}

#[tokio::test]
async fn a_funded_joint_wallet_pays_once_for_its_compound() {
    let (alice, carol, proposer) = (party(), party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000), (&carol, 100_000)]).await;
    let pair = joint(&alice, &carol);
    let pre = seed(&rm, &genesis, &pair, 50_000).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let played = executor
        .execute(
            &pre,
            vec![cosigned(JOINT, 1, &alice, &carol, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");

    assert_eq!(played.gate.admitted(), vec![true]);
    let charges = &played.settlement.charges;
    assert_eq!(charges.len(), 1);
    let joint_wallet = rholang::rust::interpreter::accounting::rev_stack::wallet_channel_id(&pair);
    assert_eq!(charges[0].wallet, joint_wallet);
    assert!(played.settlement.is_balanced());
    let settled = &played.settled_root;
    let each = charges[0].comm_rev + charges[0].compute_rev;
    assert_eq!(
        read_wallet_balance(&rm, settled, &pair).await.unwrap(),
        50_000 - each
    );
    assert_eq!(balance(&rm, settled, &alice).await, 100_000);
    assert_eq!(balance(&rm, settled, &carol).await, 100_000);

    let again = executor
        .replay(
            &pre,
            played.processed.clone(),
            &played.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the block replays");
    assert_eq!(again.settled_root, played.settled_root);
}

#[tokio::test]
async fn a_joint_wallet_is_spent_only_by_all_its_signers() {
    let (alice, carol, bob, proposer) = (party(), party(), party(), party());
    let (rm, genesis) = shard(&[(&alice, 100_000), (&carol, 100_000)]).await;
    let pair = joint(&alice, &carol);
    let pre = seed(&rm, &genesis, &pair, 50_000).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let address = VaultAddress::from_public_key(&bob.pk).unwrap().to_base58();
    let spend = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in {{
             w!("transfer", ((*d, *c), "{address}", 700), "out")
           }}"#
    );
    let played = executor
        .execute(
            &pre,
            vec![cosigned(&spend, 1, &alice, &carol, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    let run = &played.runs[0].post_root;
    assert_eq!(
        read_wallet_balance(&rm, run, &pair).await.unwrap(),
        50_000 - 700
    );
    assert_eq!(balance(&rm, run, &bob).await, 700);

    let leak = r#"new d(`rho:system:deployerId`) in { @"carol"!(*d) }"#;
    let leaked = executor
        .execute(
            &pre,
            vec![envelope(leak, 2, &carol, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the leak executes");
    let alone = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{
             for (@c <- @"carol") {{ w!("transfer", ((*d, c), "{address}", 700), "out") }}
           }}"#
    );
    let played = executor
        .execute(
            &leaked.settled_root,
            vec![envelope(&alone, 3, &alice, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    let run = &played.runs[0].post_root;
    assert_eq!(read_wallet_balance(&rm, run, &pair).await.unwrap(), 50_000);
    assert_eq!(balance(&rm, run, &bob).await, 0);
}
