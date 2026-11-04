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
    let of = |p: &Party| charges.iter().find(|c| c.wallet == wallet(p)).unwrap();
    let (deployer, component) = (of(&alice), of(&carol));
    assert_eq!(component.comm_phlo, 1);
    assert_eq!(deployer.comm_phlo, component.comm_phlo);
    assert!(component.compute_phlo > 0);
    assert!(
        deployer.compute_phlo > component.compute_phlo,
        "the deployer pays the compound's work in full and the top level as well"
    );
    assert_eq!(
        played.settlement.proposer_credit,
        deployer.comm_rev + component.comm_rev
    );
    assert!(played.settlement.is_balanced());

    let settled = &played.settled_root;
    let total = |c: &casper::rust::v6::settle::EnvelopeCharge| c.comm_rev + c.compute_rev;
    let each = total(component);
    assert_eq!(balance(&rm, settled, &alice).await, 100_000 - total(deployer));
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
    assert_eq!(charges.len(), 2);
    let joint_wallet = rholang::rust::interpreter::accounting::rev_stack::wallet_channel_id(&pair);
    let joint_charge = charges.iter().find(|c| c.wallet == joint_wallet).unwrap();
    let top_level = charges.iter().find(|c| c.wallet == wallet(&alice)).unwrap();
    assert_eq!(joint_charge.comm_phlo, 1);
    assert!(played.settlement.is_balanced());
    let settled = &played.settled_root;
    let each = joint_charge.comm_rev + joint_charge.compute_rev;
    assert_eq!(
        read_wallet_balance(&rm, settled, &pair).await.unwrap(),
        50_000 - each
    );
    assert_eq!(
        balance(&rm, settled, &alice).await,
        100_000 - (top_level.comm_rev + top_level.compute_rev)
    );
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
    assert!(paid(&second, &bob) > 0);
    let installer = second
        .settlement
        .surface_charges
        .iter()
        .find(|sc| sc.purse == bob.owner)
        .expect("the installer's surface is charged");
    assert_eq!(installer.cells, 1);
    assert_eq!(installer.phlo_price, 1);
    assert_eq!(installer.owed, installer.cells + installer.compute);
    assert_eq!(
        balance(&rm, root, &bob).await,
        100_000 - paid(&first, &bob) - paid(&second, &bob) - 1
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
async fn data_sent_to_an_owners_contract_runs_under_the_owners_membrane_and_is_paid_there() {
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
    let sent = r#"new slot in {
        for (a <<- @"aid") { @"eval"!({% slot :: slot :: () %}[a]) } | @"slot"!(*slot)
    }"#;
    let second = executor
        .execute(
            &first.settled_root,
            vec![envelope(sent, 2, &mallory, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the delivery executes");
    let root = &second.settled_root;
    assert!(!second.processed[0].is_failed);
    assert_eq!(rev_at(&rm, root, "slot").await, 2);
    assert_eq!(
        balance(&rm, root, &alice).await,
        100_000 - paid(&first, &alice) - 2 - paid(&second, &alice)
    );
    assert_eq!(
        balance(&rm, root, &mallory).await,
        100_000 - paid(&second, &mallory)
    );
    let owner = second
        .settlement
        .surface_charges
        .iter()
        .find(|sc| sc.purse == alice.owner)
        .expect("the owner's membrane is charged");
    assert!(owner.compute > 0, "the owner pays for the continuation's work");
    let sender = second
        .settlement
        .charges
        .iter()
        .find(|c| c.wallet == wallet(&mallory))
        .expect("the sender pays its own part");
    assert_eq!(
        sender.compute_phlo,
        second.runs[0].compute_phlo,
        "the client's run counts only the client's own work (Decision 86)"
    );
    let again = executor
        .replay(
            &first.settled_root,
            second.processed.clone(),
            &second.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the delivery replays");
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
    let top_level = paid(&played, &bob);
    assert!(top_level > 0, "the deployer pays for the top level under its implicit seal");
    assert_eq!(balance(&rm, settled, &bob).await, 100_000 - top_level);
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

fn surface_of(who: &Party, envelope: &Signed<DeployData>, price: u64) -> rholang::rust::interpreter::accounting::comm_ledger::Surface {
    rholang::rust::interpreter::accounting::comm_ledger::Surface {
        owner: who.owner.canonical_bytes(),
        envelope: envelope.sig.to_vec(),
        phlo_price: price,
    }
}

#[tokio::test]
async fn each_surface_of_a_comm_pays_one_cell_at_its_origin_price() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let store = envelope(r#"contract @"svc"(_) = { Nil }"#, 1, &alice, 1_000, 1);
    let first = executor
        .execute(&pre, vec![store.clone()], block_data(&proposer))
        .await
        .unwrap();
    let call = envelope(r#"@"svc"!(0)"#, 2, &bob, 1_000, 2);
    let second = executor
        .execute(&first.settled_root, vec![call.clone()], block_data(&proposer))
        .await
        .unwrap();
    let mut expected = vec![
        (Some(surface_of(&alice, &store, 1)), 1),
        (Some(surface_of(&bob, &call, 2)), 1),
    ];
    expected.sort();
    assert_eq!(second.surfaces[0], expected);
    assert_eq!(second.settlement.charges[0].comm_phlo, 1);
    let alice_cell = &second.settlement.surface_charges;
    assert_eq!(alice_cell.len(), 1);
    assert_eq!(alice_cell[0].purse, alice.owner);
    assert_eq!((alice_cell[0].cells, alice_cell[0].owed, alice_cell[0].drawn), (1, 1, 1));
    assert!(second.settlement.is_balanced());
    assert_eq!(paid(&second, &alice), 1);
    assert_eq!(
        balance(&rm, &second.settled_root, &alice).await,
        balance(&rm, &first.settled_root, &alice).await - 1
    );
    let again = executor
        .replay(
            &first.settled_root,
            second.processed.clone(),
            &second.gate.entries,
            block_data(&proposer),
        )
        .await
        .unwrap();
    assert_eq!(again.surfaces, second.surfaces);
    assert_eq!(again.settlement, second.settlement);
    assert_eq!(again.settled_root, second.settled_root);
}

#[tokio::test]
async fn surfaces_under_one_seal_pay_once() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    for (ts, term) in [
        (1, r#"new x in { x!(1) | for (_ <- x) { Nil } }"#),
        (
            2,
            r#"new d(`rho:system:deployerId`) in { {% for (_ <- @"q") { Nil } %}[d] | @"q"!(1) }"#,
        ),
    ] {
        let one = envelope(term, ts, &alice, 1_000, 1);
        let played = executor
            .execute(&pre, vec![one.clone()], block_data(&proposer))
            .await
            .unwrap();
        assert!(!played.processed[0].is_failed, "{term}");
        assert_eq!(played.surfaces[0], vec![(Some(surface_of(&alice, &one, 1)), 1)], "{term}");
        assert!(played.settlement.surface_charges.is_empty());
        assert_eq!(played.settlement.charges[0].comm_phlo, 1);
    }
}

fn foreign_purses(played: &casper::rust::v6::executor::ExecutedBlock, run: usize) -> Vec<Vec<u8>> {
    let envelope = &played.gate.entries[run].envelope_sig;
    played.surfaces[run]
        .iter()
        .filter_map(|(s, _)| s.as_ref())
        .filter(|s| s.envelope.as_slice() != envelope.as_ref())
        .map(|s| s.owner.clone())
        .collect()
}

#[tokio::test]
async fn a_datum_stored_in_an_earlier_block_is_charged_to_its_sender() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let first = executor
        .execute(
            &pre,
            vec![envelope(r#"@"in"!(5)"#, 1, &alice, 1_000, 2)],
            block_data(&proposer),
        )
        .await
        .expect("the send executes");
    assert!(!first.processed[0].is_failed);
    let second = executor
        .execute(
            &first.settled_root,
            vec![envelope(r#"for (_ <- @"in") { Nil }"#, 2, &bob, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the receive executes");
    assert!(!second.processed[0].is_failed);
    assert_eq!(
        foreign_purses(&second, 0),
        vec![alice.owner.canonical_bytes()]
    );
    let charges = &second.settlement.surface_charges;
    assert_eq!(charges.len(), 1);
    assert_eq!(charges[0].purse, alice.owner);
    assert_eq!(
        (charges[0].phlo_price, charges[0].cells, charges[0].drawn),
        (2, 1, 2)
    );
    let root = &second.settled_root;
    assert_eq!(paid(&second, &alice), 2);
    assert_eq!(
        balance(&rm, root, &alice).await,
        100_000 - paid(&first, &alice) - 2
    );
    assert_eq!(
        balance(&rm, root, &bob).await,
        100_000 - paid(&second, &bob)
    );
    let again = executor
        .replay(
            &first.settled_root,
            second.processed.clone(),
            &second.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the receive replays");
    assert_eq!(again.settled_root, second.settled_root);
    assert_eq!(again.surfaces, second.surfaces);
}

#[tokio::test]
async fn an_unfunded_stored_receive_is_not_a_match_candidate_in_play_or_replay() {
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis_root) = shard(&[(&bob, 100_000)]).await;
    let service = r#"contract @"svc"(_) = { Nil }"#;
    let process = process_owner(&Compiler::source_to_adt(service).unwrap());
    let mut runtime = rm.spawn_runtime().await.unwrap();
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(&genesis_root))
        .await
        .unwrap();
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
        .deposit(&process, 10_000)
        .unwrap();
    let pre = runtime.create_checkpoint().await.root.to_bytes_prost();
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let sealed = format!("{{% {service} %}}[# {{ {service} }}]");
    let first = executor
        .execute(
            &pre,
            vec![envelope(&sealed, 1, &alice, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the service installs");
    assert_eq!(
        first.gate.admitted(),
        vec![true],
        "{:?}",
        first.gate.entries[0].verdict
    );
    assert!(!first.processed[0].is_failed);
    let root = first.settled_root.clone();
    let left = read_wallet_balance(&rm, &root, &process).await.unwrap();
    assert!(left > 1 && left < 10_000);

    let call = || envelope(r#"@"svc"!(0)"#, 2, &bob, 1_000, 1);
    let funded = executor
        .execute(&root, vec![call()], block_data(&proposer))
        .await
        .expect("the funded call executes");
    assert_eq!(foreign_purses(&funded, 0), vec![process.canonical_bytes()]);
    assert_eq!(funded.settlement.surface_charges.len(), 1);
    assert_eq!(funded.settlement.surface_charges[0].drawn, 1);
    assert_eq!(
        read_wallet_balance(&rm, &funded.settled_root, &process)
            .await
            .unwrap(),
        left - 1
    );

    let holding = envelope(&sealed, 3, &alice, left as i64, 1);
    let held = executor
        .execute(&root, vec![call(), holding], block_data(&proposer))
        .await
        .expect("the held call executes");
    assert_eq!(
        held.gate.admitted(),
        vec![true, true],
        "{:?}",
        held.gate.entries[1].verdict
    );
    assert!(foreign_purses(&held, 0).is_empty());
    assert!(held
        .settlement
        .surface_charges
        .iter()
        .all(|c| c.envelope_sig != held.gate.entries[0].envelope_sig));

    for played in [&funded, &held] {
        let again = executor
            .replay_recorded(
                &root,
                played.processed.clone(),
                Some(played.work.clone()),
                &played.gate.entries,
                block_data(&proposer),
            )
            .await
            .expect("the call replays");
        assert_eq!(again.settled_root, played.settled_root);
        assert_eq!(again.surfaces, played.surfaces);
    }

    assert_eq!(funded.processed[0].deploy, held.processed[0].deploy);
    assert_ne!(funded.processed[0].deploy_log, held.processed[0].deploy_log);
    let stale = vec![funded.processed[0].clone(), held.processed[1].clone()];
    assert!(executor
        .replay(&root, stale, &held.gate.entries, block_data(&proposer))
        .await
        .is_err());
}

#[tokio::test]
async fn work_under_an_underfunded_installer_stops_and_the_client_pays_nothing_for_it() {
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis_root) = shard(&[(&bob, 100_000)]).await;
    let service = r#"contract @"svc"(_) = {
        new loop in { contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(300) }
    }"#;
    let process = process_owner(&Compiler::source_to_adt(service).unwrap());
    let mut runtime = rm.spawn_runtime().await.unwrap();
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(&genesis_root))
        .await
        .unwrap();
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
        .deposit(&process, 200_000)
        .unwrap();
    let pre = runtime.create_checkpoint().await.root.to_bytes_prost();
    let executor = EnvelopeExecutor::new(rm.clone(), 4);
    let sealed = format!("{{% {service} %}}[# {{ {service} }}]");
    let first = executor
        .execute(
            &pre,
            vec![envelope(&sealed, 1, &alice, 1_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the service installs");
    assert_eq!(
        first.gate.admitted(),
        vec![true],
        "{:?}",
        first.gate.entries[0].verdict
    );
    let root = first.settled_root.clone();
    let left = read_wallet_balance(&rm, &root, &process).await.unwrap();
    let call = || envelope(r#"@"svc"!(0)"#, 2, &bob, 1_000, 1);

    let funded = executor
        .execute(&root, vec![call()], block_data(&proposer))
        .await
        .expect("the funded call executes");
    assert!(
        !funded.processed[0].is_failed,
        "{:?} {:?} {:?}",
        funded.processed[0].cost, funded.metered[0], funded.work[0]
    );
    assert!(funded.metered[0].refusals.is_empty());
    let full = funded.metered[0].drawn[&process.canonical_bytes()];
    assert!(full > 600, "{full}");

    let room = 200;
    let holding = envelope(&sealed, 3, &alice, (left - room) as i64, 1);
    let held = executor
        .execute(&root, vec![call(), holding], block_data(&proposer))
        .await
        .expect("the held call executes");
    assert_eq!(held.gate.admitted(), vec![true, true]);
    assert!(!held.processed[0].is_failed);
    let meter = &held.metered[0];
    assert!(!meter.refusals.is_empty());
    let drawn = meter.drawn[&process.canonical_bytes()];
    assert!(drawn <= room, "{drawn}");
    assert!(meter.refusals.iter().all(|r| r.drawn == drawn));
    assert!(meter.refusals.iter().any(|r| drawn + r.charge > room));
    assert_eq!(paid(&held, &bob), paid(&funded, &bob));

    let logged: Vec<_> = held.metered.iter().map(|m| m.refusals.clone()).collect();
    for _ in 0..8 {
        let again = executor
            .replay_with_refusals(
                &root,
                held.processed.clone(),
                &held.gate.entries,
                block_data(&proposer),
                logged.clone(),
                Some(held.work.clone()),
            )
            .await
            .expect("the stopped call replays");
        assert_eq!(again.settled_root, held.settled_root);
        assert_eq!(again.metered[0].drawn, meter.drawn);
        assert_eq!(again.settlement, held.settlement);
    }

    let unlogged = executor
        .replay_with_refusals(
            &root,
            held.processed.clone(),
            &held.gate.entries,
            block_data(&proposer),
            vec![Vec::new(); held.processed.len()],
            Some(held.work.clone()),
        )
        .await;
    assert!(unlogged.map_or(true, |r| r.settled_root != held.settled_root));
    let from_log = executor
        .replay_recorded(
            &root,
            held.processed.clone(),
            Some(held.work.clone()),
            &held.gate.entries,
            block_data(&proposer),
        )
        .await
        .expect("the refusals in the deploy log replay");
    assert_eq!(from_log.settled_root, held.settled_root);
    assert_eq!(from_log.settlement, held.settlement);

    let mut early = logged.clone();
    early[0].iter_mut().for_each(|r| r.drawn = 0);
    assert!(executor
        .replay_with_refusals(
            &root,
            held.processed.clone(),
            &held.gate.entries,
            block_data(&proposer),
            early,
            Some(held.work.clone())
        )
        .await
        .is_err());

    let replays = |logged: Vec<Vec<_>>| {
        let (executor, root, held) = (&executor, &root, &held);
        let proposer = &proposer;
        async move {
            executor
                .replay_with_refusals(
                    root,
                    held.processed.clone(),
                    &held.gate.entries,
                    block_data(proposer),
                    logged,
                    Some(held.work.clone()),
                )
                .await
                .map(|again| again.settled_root == held.settled_root)
                .unwrap_or(false)
        }
    };
    let mut high = logged.clone();
    high[0].iter_mut().for_each(|r| r.drawn += 1);
    assert!(!replays(high).await, "a drawn amount set too high");

    let tighter = executor
        .execute(
            &root,
            vec![
                call(),
                envelope(&sealed, 3, &alice, (left - room / 2) as i64, 1),
            ],
            block_data(&proposer),
        )
        .await
        .expect("the tighter call executes");
    let earlier: Vec<_> = tighter.metered.iter().map(|m| m.refusals.clone()).collect();
    assert!(!earlier[0].is_empty());
    assert!(earlier[0]
        .iter()
        .all(|r| !logged[0].iter().any(|l| l.step == r.step)));
    assert!(
        !replays(earlier).await,
        "an admitted step changed to refused"
    );
}

#[tokio::test]
async fn substitution_into_a_fired_continuation_is_charged_to_its_installer() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let install = r#"
        contract @"wide"(@x) = { @"sink"!([x, x, x, x, x, x, x, x, x, x, x, x, x, x, x, x]) } |
        contract @"thin"(@x) = { Nil }
    "#;
    let first = executor
        .execute(
            &pre,
            vec![envelope(install, 1, &alice, 5_000, 1)],
            block_data(&proposer),
        )
        .await
        .expect("the services install");
    assert!(!first.processed[0].is_failed);
    let payload = format!("{:?}", vec![7u8; 40]).replace(' ', "");
    let call = |name: &str| envelope(&format!(r#"@"{name}"!({payload})"#), 2, &bob, 50_000, 1);
    let mut owners = Vec::new();
    let mut clients = Vec::new();
    for name in ["wide", "thin"] {
        let played = executor
            .execute(&first.settled_root, vec![call(name)], block_data(&proposer))
            .await
            .expect("the call executes");
        assert!(!played.processed[0].is_failed);
        let owner: u64 = played.work[0]
            .iter()
            .filter(|(s, _)| {
                s.as_ref()
                    .is_some_and(|s| s.owner == alice.owner.canonical_bytes())
            })
            .map(|(_, w)| *w)
            .sum();
        owners.push(owner);
        clients.push(played.runs[0].compute_phlo);
    }
    assert!(owners[0] > owners[1], "{owners:?}");
    assert_eq!(
        clients[0], clients[1],
        "the firer pays the same for either body"
    );
}

async fn funded_process(
    rm: &RuntimeManager,
    root: &StateHash,
    term: &str,
    amount: u64,
) -> (WalletOwner, StateHash) {
    use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let process = process_owner(&Compiler::source_to_adt(term).unwrap());
    let mut runtime = rm.spawn_runtime().await.unwrap();
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(root))
        .await
        .unwrap();
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
        .deposit(&process, amount)
        .unwrap();
    (process, runtime.create_checkpoint().await.root.to_bytes_prost())
}

#[tokio::test]
async fn two_runs_that_each_fit_a_gated_wallet_but_not_together_pay_in_full() {
    let (alice, bob, carol, proposer) = (party(), party(), party(), party());
    let (rm, genesis_root) = shard(&[(&bob, 100_000), (&carol, 100_000)]).await;
    let service = r#"contract @"svc"(_) = {
        new loop in { contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(300) }
    }"#;
    let (process, pre) = funded_process(&rm, &genesis_root, service, 56_000).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 4);
    let sealed = format!("{{% {service} %}}[# {{ {service} }}]");
    let first = executor
        .execute(&pre, vec![envelope(&sealed, 1, &alice, 1_000, 1)], block_data(&proposer))
        .await
        .expect("the service installs");
    let root = first.settled_root.clone();
    let left = read_wallet_balance(&rm, &root, &process).await.unwrap();
    let room = left;
    let calls = vec![
        envelope(r#"@"svc"!(0)"#, 4, &bob, 1_000, 1),
        envelope(r#"@"svc"!(0)"#, 5, &carol, 1_000, 1),
    ];
    let outcome = executor.execute(&root, calls, block_data(&proposer)).await;
    let block = outcome.expect("two runs that each fit execute");
    let charges: Vec<_> = block
        .settlement
        .surface_charges
        .iter()
        .filter(|sc| sc.purse == process)
        .collect();
    let drawn: u64 = charges.iter().map(|sc| sc.drawn).sum();
    assert!(drawn <= room, "{drawn} > {room}");
    assert!(
        charges.iter().all(|sc| sc.drawn == sc.owed),
        "no step under the membrane runs unpaid: {charges:?}"
    );
}

#[tokio::test]
async fn the_gate_refuses_a_seal_that_resolves_only_at_run_time() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[(&alice, 100_000), (&bob, 100_000)]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let shapes = [
        r#"new c(`rho:system:cosignerId:0`), ch in {
             ch!(*c) | for (s <- ch) { {% new x in { x!(1) } %}[s] }
           }"#,
        r#"new c(`rho:system:cosignerId:0`), ch, slot in {
             ch!(*c) | for (s <- ch) { {% new x in { x!(1) } | slot :: () %}[s] }
           }"#,
        r#"new c(`rho:system:cosignerId:0`), ch, slot in {
             ch!(*c) | for (s <- ch) { {% @"x"!(1) | slot :: slot :: () %}[s] } | @"slot"!(*slot)
           }"#,
    ];
    for (i, term) in shapes.iter().enumerate() {
        let played = executor
            .execute(&pre, vec![cosigned(term, i as i64 + 1, &alice, &bob, 1)], block_data(&proposer))
            .await
            .expect("the block executes");
        assert_eq!(
            played.gate.entries[0].verdict,
            Verdict::Refused(Refusal::ForeignSignature),
            "shape {i}"
        );
    }
}
