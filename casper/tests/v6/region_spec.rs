use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::executor::EnvelopeExecutor;
use casper::rust::v6::gate::{Refusal, Verdict};
use casper::rust::v6::genesis_wallets::{cells_at, read_wallet_balance};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{Cosignature, DeployData};
use rholang::rust::interpreter::accounting::rev_stack::journal::Source;
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, CellStore, RevStacks, TokenKind, WalletOwner,
};
use rholang::rust::interpreter::rho_type::RhoString;
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
    assert_eq!(played.merged, vec![true]);
    assert!(played.settlement.is_balanced());
    let moved: u64 = played.settlement.transfers.iter().map(|t| t.amount).sum();
    assert_eq!(moved, 2);
    assert_eq!(
        balance(&rm, &played.settled_root, &alice).await,
        100_000 - 2 - played.settlement.debited()
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
}

#[tokio::test]
async fn a_stripped_envelope_does_not_decode_and_the_complete_envelope_runs_after_it() {
    use models::rust::casper::protocol::casper_message::DeployData as Wire;

    let (alice, bob, carol, proposer) = (party(), party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 50_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let address = VaultAddress::from_public_key(&carol.pk)
        .unwrap()
        .to_base58();
    let term = format!(
        r#"new w(`rho:rev:wallet`), c(`rho:system:cosignerId:0`) in {{
             {{% w!("transfer", (*c, "{address}", 700), "out") %}}[c]
           }}"#
    );
    let complete = cosigned(&term, 1, &alice, &bob, 1);
    let mut stripped = complete.clone();
    stripped.data.cosignatures.clear();
    assert!(Wire::from_proto(Wire::to_proto_ref(&stripped)).is_err());
    let decoded =
        Wire::from_proto(Wire::to_proto_ref(&complete)).expect("the complete envelope decodes");
    assert_eq!(decoded.sig, stripped.sig);
    let played = executor
        .execute(&pre, vec![decoded], block_data(&proposer))
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    assert_eq!(balance(&rm, &played.settled_root, &carol).await, 700);
    let refused = executor
        .execute(&pre, vec![stripped], block_data(&proposer))
        .await
        .expect("the block executes");
    assert_eq!(
        refused.gate.entries[0].verdict,
        Verdict::Refused(Refusal::InvalidTerms)
    );
}

#[tokio::test]
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
    assert_eq!(balance(&rm, &played.settled_root, &bob).await, 6_000);
}

fn naming(term: &str, ts: i64, who: &Party, cosigner: &Party, price: i64) -> Signed<DeployData> {
    let mut data = envelope(term, ts, who, 1_000, price).data;
    data.required_cosigners = vec![cosigner.pk.bytes.to_vec()];
    Signed::create(data, Box::new(Secp256k1), who.sk.clone()).unwrap()
}

fn cosigned(term: &str, ts: i64, who: &Party, cosigner: &Party, price: i64) -> Signed<DeployData> {
    let mut deploy = naming(term, ts, who, cosigner, price);
    let cosignature = Cosignature::sign(&deploy, &cosigner.sk, &Secp256k1.name()).unwrap();
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
             {{% w!("transfer", ((*d, *c), "{address}", 700), "out") %}}[d (*) c]
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
    let settled = &played.settled_root;
    let paid = played
        .settlement
        .debits
        .get(&wallet_channel_id(&pair))
        .copied()
        .unwrap_or(0);
    assert_eq!(
        read_wallet_balance(&rm, settled, &pair).await.unwrap(),
        50_000 - 700 - paid
    );
    assert_eq!(balance(&rm, settled, &bob).await, 700);

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
    let settled = &played.settled_root;
    let paid = played
        .settlement
        .debits
        .get(&wallet_channel_id(&pair))
        .copied()
        .unwrap_or(0);
    assert_eq!(
        read_wallet_balance(&rm, settled, &pair).await.unwrap(),
        50_000 - paid
    );
    assert_eq!(balance(&rm, settled, &bob).await, 0);
}

#[tokio::test]
async fn the_gate_admits_a_mint_of_a_kind_and_the_block_replays() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let mint = r#"new k, d(`rho:system:deployerId`) in { {% d :: d :: () %}[k] }"#;
    let played = executor
        .execute(
            &pre,
            vec![envelope(mint, 1, &alice, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    assert!(!played.processed[0].is_failed);
    assert_eq!(
        balance(&rm, &played.mid_root, &alice).await,
        100_000
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
}

async fn published(rm: &RuntimeManager, root: &StateHash, channel: &str) -> Vec<u8> {
    let cells = cells_at(rm, root).await.unwrap();
    let data = cells.get_data(&RhoString::create_par(channel.to_string()));
    let [datum] = data.as_slice() else {
        panic!("one datum on {channel}");
    };
    let par = &datum.a.pars[0];
    let name = match par.bundles.as_slice() {
        [bundle] => bundle.body.clone().unwrap(),
        _ => par.clone(),
    };
    match &name.unforgeables[0].unf_instance {
        Some(UnfInstance::GPrivateBody(private)) => private.id.clone(),
        other => panic!("{other:?}"),
    }
}

async fn kind_balance(
    rm: &RuntimeManager,
    root: &StateHash,
    kind: &TokenKind,
    owner: &WalletOwner,
) -> u64 {
    RevStacks::new(Arc::new(cells_at(rm, root).await.unwrap()))
        .of_kind(kind)
        .balance(owner)
        .unwrap()
}

#[tokio::test]
async fn a_kind_mint_and_a_kind_transfer_reach_the_settled_root() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let mint = r#"new k, d(`rho:system:deployerId`) in {
        {% d :: d :: d :: () %}[k] | @"pub"!(bundle0{*k})
    }"#;
    let minted = executor
        .execute(
            &pre,
            vec![envelope(mint, 1, &alice, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the mint executes");
    assert_eq!(minted.gate.admitted(), vec![true]);
    assert!(!minted.processed[0].is_failed);
    assert!(minted
        .settlement
        .transfers
        .iter()
        .any(|t| t.from == Source::Mint));
    let kind = TokenKind::from_name(published(&rm, &minted.settled_root, "pub").await).unwrap();
    assert!(!kind.is_rev());
    assert_eq!(
        kind_balance(&rm, &minted.settled_root, &kind, &alice.owner).await,
        3
    );
    let again = executor
        .replay(
            &pre,
            minted.processed.clone(),
            &minted.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the mint replays");
    assert_eq!(again.settled_root, minted.settled_root);

    let give = r#"new slot in {
        for (kind <<- @"pub") { {% slot :: slot :: () %}[kind] } | @"slot"!(*slot)
    }"#;
    let given = executor
        .execute(
            &minted.settled_root,
            vec![envelope(give, 2, &alice, 1_000, 3)],
            block_data(&proposer),
        )
        .await
        .expect("the transfer executes");
    assert_eq!(given.gate.admitted(), vec![true]);
    assert!(!given.processed[0].is_failed);
    let settled = &given.settled_root;
    let slot = WalletOwner::Unforgeable(published(&rm, settled, "slot").await);
    assert_eq!(kind_balance(&rm, settled, &kind, &alice.owner).await, 1);
    assert_eq!(kind_balance(&rm, settled, &kind, &slot).await, 2);
    assert_eq!(
        balance(&rm, settled, &alice).await + given.settlement.debited(),
        balance(&rm, &minted.settled_root, &alice).await
    );
    let again = executor
        .replay(
            &minted.settled_root,
            given.processed.clone(),
            &given.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the transfer replays");
    assert_eq!(again.settled_root, given.settled_root);
}

#[tokio::test]
async fn a_lifted_cosignature_carries_no_authority_under_another_deployer() {
    let (alice, bob, carol, mallory, proposer) = (party(), party(), party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 50_000), (&mallory, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let address = VaultAddress::from_public_key(&carol.pk)
        .unwrap()
        .to_base58();
    let term = format!(
        r#"new w(`rho:rev:wallet`), c(`rho:system:cosignerId:0`) in {{
             {{% w!("transfer", (*c, "{address}", 700), "out") %}}[c]
           }}"#
    );
    let original = cosigned(&term, 1, &alice, &bob, 1);
    let mut data = original.data.clone();
    data.cosignatures.clear();
    let mut lifted = Signed::create(data, Box::new(Secp256k1), mallory.sk.clone()).unwrap();
    lifted
        .data
        .cosignatures
        .push(original.data.cosignatures[0].clone());
    let played = executor
        .execute(&pre, vec![original, lifted], block_data(&proposer))
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true, false]);
    assert_eq!(
        played.gate.entries[1].verdict,
        Verdict::Refused(Refusal::InvalidTerms)
    );
    let settled = &played.settled_root;
    assert_eq!(balance(&rm, settled, &carol).await, 700);
    assert_eq!(balance(&rm, settled, &bob).await, 50_000 - 700 - paid(&played, &bob));
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
async fn a_fired_continuation_draws_on_its_author_not_on_the_firing_region() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let store = r#"new slot in { for (_ <- @"x") { slot :: () } | @"slot"!(*slot) }"#;
    let first = executor
        .execute(
            &pre,
            vec![envelope(store, 1, &bob, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the store executes");
    let fire = r#"new d(`rho:system:deployerId`) in { {% @"x"!(1) %}[d] }"#;
    let second = executor
        .execute(
            &first.settled_root,
            vec![envelope(fire, 2, &alice, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the firing executes");
    assert!(!second.processed[0].is_failed);
    let root = &second.settled_root;
    assert_eq!(rev_at(&rm, root, "slot").await, 1);
    assert_eq!(
        balance(&rm, root, &bob).await,
        100_000 - paid(&first, &bob) - 1
    );
    assert_eq!(
        balance(&rm, root, &alice).await,
        100_000 - paid(&second, &alice)
    );
    let again = executor
        .replay(
            &first.settled_root,
            second.processed.clone(),
            &second.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the firing replays");
    assert_eq!(again.settled_root, second.settled_root);
}

#[tokio::test]
async fn a_region_sealed_in_its_origin_pays_from_its_sealer_for_any_later_runner() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let store = r#"new d(`rho:system:deployerId`), slot in {
        for (_ <= @"pub") { {% slot :: () %}[d] } | @"slot"!(*slot)
    }"#;
    let mut root = pre.clone();
    let mut alice_paid = 0;
    let mut bob_paid = 0;
    for (ts, who, term) in [
        (1, &alice, store),
        (2, &bob, r#"@"pub"!(0)"#),
        (3, &alice, r#"@"pub"!(0)"#),
    ] {
        let played = executor
            .execute(
                &root,
                vec![envelope(term, ts, who, 1_000, 1)],
                block_data(&proposer),
            )
            .await
            .expect("the block executes");
        assert_eq!(played.gate.admitted(), vec![true]);
        assert!(!played.processed[0].is_failed, "{term}");
        let again = executor
            .replay(
                &root,
                played.processed.clone(),
                &played.gate.entries,
                block_data(&proposer),
            )
            .await
            .expect("the block replays");
        assert_eq!(again.settled_root, played.settled_root);
        alice_paid += paid(&played, &alice);
        bob_paid += paid(&played, &bob);
        root = played.settled_root.clone();
    }
    assert_eq!(rev_at(&rm, &root, "slot").await, 2);
    assert_eq!(balance(&rm, &root, &alice).await, 100_000 - alice_paid - 2);
    assert_eq!(balance(&rm, &root, &bob).await, 100_000 - bob_paid);
}

#[tokio::test]
async fn a_seal_sent_as_data_to_an_owners_contract_draws_on_no_wallet() {
    let (alice, mallory, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&mallory, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let contract = r#"new d(`rho:system:deployerId`) in {
        @"aid"!(*d) | for (p <= @"eval") { *p }
    }"#;
    let first = executor
        .execute(
            &pre,
            vec![envelope(contract, 1, &alice, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the contract executes");
    assert!(!first.processed[0].is_failed);
    let attack = r#"new slot in {
        for (a <<- @"aid") { @"eval"!({% slot :: slot :: () %}[a]) } | @"slot"!(*slot)
    }"#;
    let second = executor
        .execute(
            &first.settled_root,
            vec![envelope(attack, 2, &mallory, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the attack executes");
    let root = &second.settled_root;
    assert!(second.processed[0].is_failed);
    assert!(second.settlement.transfers.is_empty());
    assert_eq!(
        balance(&rm, root, &alice).await,
        100_000 - paid(&first, &alice)
    );
    let again = executor
        .replay(
            &first.settled_root,
            second.processed.clone(),
            &second.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the attack replays");
    assert_eq!(again.settled_root, second.settled_root);
}

#[tokio::test]
async fn a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let term = r#"new a(`rho:system:cosignerId:0`), slot in {
        {% @"x"!(1) | slot :: slot :: () %}[a] | @"slot"!(*slot)
    }"#;
    let played = executor
        .execute(
            &pre,
            vec![cosigned(term, 1, &bob, &alice, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    assert!(!played.processed[0].is_failed);
    let settled = &played.settled_root;
    assert_eq!(rev_at(&rm, settled, "slot").await, 2);
    assert_eq!(
        balance(&rm, settled, &alice).await,
        100_000 - paid(&played, &alice) - 2
    );
    assert_eq!(balance(&rm, settled, &bob).await, 100_000);
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
async fn a_term_that_fails_to_normalize_is_charged_to_the_deployer_alone() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let broken = r#"new c(`rho:system:cosignerId:0`), slot in { {% slot :: () %}[c] | nope }"#;
    let played = executor
        .execute(
            &pre,
            vec![cosigned(broken, 1, &alice, &bob, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    assert!(played.processed[0].is_failed);
    assert!(played.settlement.transfers.is_empty());
    let debited: Vec<_> = played.settlement.debits.keys().cloned().collect();
    assert_eq!(debited, vec![wallet(&alice)]);
    let settled = &played.settled_root;
    assert_eq!(
        balance(&rm, settled, &alice).await,
        100_000 - paid(&played, &alice)
    );
    assert_eq!(balance(&rm, settled, &bob).await, 100_000);
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

fn paid(played: &casper::rust::v6::executor::ExecutedBlock, who: &Party) -> u64 {
    played
        .settlement
        .debits
        .get(&wallet(who))
        .copied()
        .unwrap_or(0)
}

async fn rev_at(rm: &RuntimeManager, root: &StateHash, channel: &str) -> u64 {
    let slot = WalletOwner::Unforgeable(published(rm, root, channel).await);
    read_wallet_balance(rm, root, &slot).await.unwrap()
}

#[tokio::test]
async fn the_nested_kind_form_passes_the_gate_and_draws_only_on_its_sealer() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let mint = r#"new k, d(`rho:system:deployerId`) in {
        {% d :: d :: d :: () %}[k] | @"pub"!(bundle0{*k}) | @"aid"!(*d)
    }"#;
    let give = r#"new d(`rho:system:deployerId`), s in {
        for (kind <<- @"pub") { {% {% s :: () %}[kind] %}[d] } | @"s"!(*s)
    }"#;
    let steal = r#"new s in {
        for (a <<- @"aid") { for (kind <<- @"pub") { {% {% s :: () %}[kind] %}[a] } }
    }"#;
    let mut root = pre.clone();
    let mut kind = None;
    for (ts, who, term, fails) in [
        (1, &alice, mint, false),
        (2, &alice, give, false),
        (3, &bob, steal, true),
    ] {
        let played = executor
            .execute(
                &root,
                vec![envelope(term, ts, who, 1_000, 1)],
                block_data(&proposer),
            )
            .await
            .expect("the block executes");
        assert_eq!(played.gate.admitted(), vec![true], "{term}");
        assert_eq!(played.processed[0].is_failed, fails, "{term}");
        let again = executor
            .replay(
                &root,
                played.processed.clone(),
                &played.gate.entries,
                block_data(&proposer),
            )
            .await
            .expect("the block replays");
        assert_eq!(again.settled_root, played.settled_root);
        root = played.settled_root.clone();
        if kind.is_none() {
            kind = TokenKind::from_name(published(&rm, &root, "pub").await);
        }
    }
    let kind = kind.unwrap();
    let s = WalletOwner::Unforgeable(published(&rm, &root, "s").await);
    assert_eq!(kind_balance(&rm, &root, &kind, &alice.owner).await, 2);
    assert_eq!(kind_balance(&rm, &root, &kind, &s).await, 1);
    assert_eq!(kind_balance(&rm, &root, &kind, &bob.owner).await, 0);
}

#[tokio::test]
async fn a_sealed_stored_receive_that_sends_the_same_datum_on_each_firing_keeps_merging() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let store = r#"new d(`rho:system:deployerId`) in {
        for (_ <= @"pub") { {% @"ran"!(1) %}[d] }
    }"#;
    let mut root = pre.clone();
    for (ts, who, term) in [
        (1, &alice, store),
        (2, &bob, r#"@"pub"!(0)"#),
        (3, &alice, r#"@"pub"!(0)"#),
        (4, &bob, r#"@"pub"!(0)"#),
    ] {
        let played = executor
            .execute(
                &root,
                vec![envelope(term, ts, who, 1_000, 1)],
                block_data(&proposer),
            )
            .await
            .expect("the block executes");
        assert!(!played.processed[0].is_failed, "{term}");
        assert_eq!(played.merged, vec![true], "block {ts}");
        let again = executor
            .replay(
                &root,
                played.processed.clone(),
                &played.gate.entries,
                block_data(&proposer),
            )
            .await
            .expect("the block replays");
        assert_eq!(again.settled_root, played.settled_root);
        root = played.settled_root.clone();
    }
}
