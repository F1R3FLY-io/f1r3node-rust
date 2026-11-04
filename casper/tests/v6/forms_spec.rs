use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::executor::{EnvelopeExecutor, ExecutedBlock};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use casper::rust::v6::settle::{EnvelopeCharge, SurfaceCharge};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{Cosignature, DeployData};
use rholang::rust::interpreter::accounting::rev_stack::{wallet_channel_id, WalletOwner};
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

async fn shard(funded: &[&Party]) -> (Arc<RuntimeManager>, StateHash) {
    let rm = mk_runtime_manager("v6-forms", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = funded
        .iter()
        .map(|p| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: 100_000,
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

fn envelope(term: &str, ts: i64, who: &Party, price: i64) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        term.to_string(),
        ts,
        Some(5_000),
        Some(price),
        Some(who.sk.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap()
}

fn cosigned(term: &str, ts: i64, who: &Party, cosigner: &Party, price: i64) -> Signed<DeployData> {
    let mut data = envelope(term, ts, who, price).data;
    data.required_cosigners = vec![cosigner.pk.bytes.to_vec()];
    let mut deploy = Signed::create(data, Box::new(Secp256k1), who.sk.clone()).unwrap();
    let cosignature = Cosignature::sign(&deploy, &cosigner.sk, &Secp256k1.name()).unwrap();
    deploy.data.cosignatures.push(cosignature);
    deploy
}

fn block_data(proposer: &Party) -> BlockData {
    BlockData {
        time_stamp: 1,
        block_number: 1,
        sender: proposer.pk.clone(),
        seq_num: 1,
    }
}

async fn play(
    executor: &EnvelopeExecutor,
    pre: &StateHash,
    deploy: Signed<DeployData>,
    proposer: &Party,
) -> ExecutedBlock {
    let played = executor
        .execute(pre, vec![deploy], block_data(proposer))
        .await
        .expect("the block executes");
    assert_eq!(played.gate.admitted(), vec![true]);
    assert!(!played.processed[0].is_failed, "{:?}", played.processed[0]);
    assert!(played.settlement.is_balanced());
    let again = executor
        .replay(
            pre,
            played.processed.clone(),
            &played.gate.entries,
            block_data(proposer),
        )
        .await
        .expect("the block replays");
    assert_eq!(again.settled_root, played.settled_root);
    assert_eq!(again.settlement, played.settlement);
    played
}

fn charge<'a>(played: &'a ExecutedBlock, who: &Party) -> Option<&'a EnvelopeCharge> {
    let wallet = wallet_channel_id(&who.owner);
    played.settlement.charges.iter().find(|c| c.wallet == wallet)
}

fn surface<'a>(played: &'a ExecutedBlock, who: &Party) -> Option<&'a SurfaceCharge> {
    played
        .settlement
        .surface_charges
        .iter()
        .find(|sc| sc.purse == who.owner)
}

#[derive(Debug, PartialEq)]
struct Shape {
    payers: Vec<(Vec<u8>, u64, bool)>,
    surfaces: Vec<(Vec<u8>, u64, u64, bool)>,
}

fn shape(played: &ExecutedBlock, parties: &[&Party]) -> Shape {
    let index = |owner: &WalletOwner| {
        parties
            .iter()
            .position(|p| &p.owner == owner)
            .map(|i| vec![i as u8])
            .unwrap_or_default()
    };
    let wallet_index = |wallet: &[u8]| {
        parties
            .iter()
            .position(|p| wallet_channel_id(&p.owner) == wallet)
            .map(|i| vec![i as u8])
            .unwrap_or_default()
    };
    let mut payers: Vec<_> = played
        .settlement
        .charges
        .iter()
        .map(|c| (wallet_index(&c.wallet), c.comm_phlo, c.compute_phlo > 0))
        .collect();
    payers.sort();
    let mut surfaces: Vec<_> = played
        .settlement
        .surface_charges
        .iter()
        .map(|sc| (index(&sc.purse), sc.cells, sc.phlo_price, sc.compute > 0))
        .collect();
    surfaces.sort();
    Shape { payers, surfaces }
}

const BODY: &str = "new y in { y!(1) | for (_ <- y) { Nil } }";

async fn two_party(stored: &str, sent: &str) -> (Shape, ExecutedBlock, Party, Party) {
    let (bob, alice, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[&bob, &alice]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let installed = play(&executor, &pre, envelope(stored, 1, &bob, 1), &proposer).await;
    let fired = play(
        &executor,
        &installed.settled_root,
        envelope(sent, 2, &alice, 2),
        &proposer,
    )
    .await;
    (shape(&fired, &[&bob, &alice]), fired, bob, alice)
}

#[tokio::test]
async fn the_two_party_form_charges_each_surface_and_the_installer_for_its_continuation() {
    let stored = format!(
        r#"new d(`rho:system:deployerId`) in {{ {{% for (_ <- @"x") {{ {{% {BODY} %}}[d] }} %}}[d] }}"#
    );
    let sent = r#"new d(`rho:system:deployerId`) in { {% @"x"!(1) %}[d] }"#;
    let (form, fired, bob, alice) = two_party(&stored, sent).await;

    let installer = surface(&fired, &bob).expect("the installer's surface pays");
    assert_eq!(installer.cells, 2, "the receive's cell and the COMM inside the continuation");
    assert_eq!(installer.phlo_price, 1, "at the installer's origin price");
    assert!(installer.compute > 0, "the installer pays for the continuation's work");
    let sender = charge(&fired, &alice).expect("the sender pays");
    assert_eq!(sender.comm_phlo, 1, "the send surface pays its own cell");
    assert_eq!(sender.phlo_price, 2);
    assert!(charge(&fired, &bob).is_none());

    let sugar = format!(r#"for (_ <- @"x") {{ {BODY} }}"#);
    let (implicit, _, _, _) = two_party(&sugar, r#"@"x"!(1)"#).await;
    assert_eq!(implicit, form, "the implicit seal elaborates to the two-party form");
}

#[tokio::test]
async fn the_one_party_form_pays_once_for_a_redex_under_one_seal() {
    let (alice, proposer) = (party(), party());
    let (rm, pre) = shard(&[&alice]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let form = format!(
        r#"new d(`rho:system:deployerId`) in {{ {{% for (_ <- @"x") {{ {BODY} }} | @"x"!(1) %}}[d] }}"#
    );
    let played = play(&executor, &pre, envelope(&form, 1, &alice, 1), &proposer).await;
    let paid = charge(&played, &alice).expect("the one seal pays");
    assert_eq!(paid.comm_phlo, 2, "one cell for each redex, not one for each surface");
    assert!(played.settlement.surface_charges.is_empty());
    let elaborated = shape(&played, &[&alice]);

    let sugar = format!(r#"for (_ <- @"x") {{ {BODY} }} | @"x"!(1)"#);
    let (rm, pre) = shard(&[&alice]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let played = play(&executor, &pre, envelope(&sugar, 1, &alice, 1), &proposer).await;
    assert_eq!(shape(&played, &[&alice]), elaborated);
}

#[tokio::test]
async fn the_lollipop_charges_the_comm_to_s1_and_the_continuation_to_s2() {
    let (alice, carol, proposer) = (party(), party(), party());
    let run = |term: String| {
        let (alice, carol, proposer) = (&alice, &carol, &proposer);
        async move {
            let (rm, pre) = shard(&[alice, carol]).await;
            let executor = EnvelopeExecutor::new(rm.clone(), 2);
            play(&executor, &pre, cosigned(&term, 1, alice, carol, 1), proposer).await
        }
    };
    let names = r#"new d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in"#;
    let lollipop = format!(
        r#"{names} {{ {{% for (_ <- @"x") {{ {BODY} }} %}}[d -o c] | @"x"!(1) }}"#
    );
    let layers = format!(
        r#"{names} {{ {{% for (_ <- @"x") {{ {{% {BODY} %}}[c] }} %}}[d] | @"x"!(1) }}"#
    );
    let played = run(lollipop).await;
    let s1 = charge(&played, &alice).expect("s1 pays the COMM");
    let s2 = charge(&played, &carol).expect("s2 pays the continuation");
    assert_eq!(s1.comm_phlo, 1);
    assert_eq!(s2.comm_phlo, 1, "the COMM inside the continuation is s2's");
    assert!(s2.compute_phlo > 0);
    let sugar = shape(&played, &[&alice, &carol]);
    assert_eq!(shape(&run(layers).await, &[&alice, &carol]), sugar);
}

#[tokio::test]
async fn separately_named_regions_pay_only_their_own_work() {
    let (alice, carol, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[&alice, &carol]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let term = format!(
        r#"new d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in {{
             {{% {BODY} %}}[d] | {{% {BODY} | {BODY} %}}[c]
           }}"#
    );
    let played = play(&executor, &pre, cosigned(&term, 1, &alice, &carol, 1), &proposer).await;
    assert_eq!(charge(&played, &alice).unwrap().comm_phlo, 1);
    assert_eq!(charge(&played, &carol).unwrap().comm_phlo, 2);
}

#[tokio::test]
async fn work_that_stored_code_pays_for_does_not_count_against_the_clients_limit() {
    let (bob, alice, proposer) = (party(), party(), party());
    let (rm, pre) = shard(&[&bob, &alice]).await;
    let executor = EnvelopeExecutor::new(rm.clone(), 2);
    let heavy = (0..40).fold("Nil".to_string(), |inner, _| {
        format!("new y in {{ y!(1) | for (_ <- y) {{ {inner} }} }}")
    });
    let stored = format!(r#"for (_ <- @"x") {{ {heavy} }}"#);
    let mut install = envelope(&stored, 1, &bob, 1);
    install.data.phlo_limit = 100_000;
    let install = Signed::create(install.data, Box::new(Secp256k1), bob.sk.clone()).unwrap();
    let installed = play(&executor, &pre, install, &proposer).await;

    let limit = 1_000;
    let mut fire = envelope(r#"@"x"!(1)"#, 2, &alice, 1).data;
    fire.phlo_limit = limit;
    let fire = Signed::create(fire, Box::new(Secp256k1), alice.sk.clone()).unwrap();
    let fired = play(&executor, &installed.settled_root, fire, &proposer).await;
    let installer = surface(&fired, &bob).expect("the installer pays the continuation");
    assert!(
        installer.compute > limit as u64,
        "the continuation's work ({}) is more than the client's limit",
        installer.compute
    );
    let client = charge(&fired, &alice).expect("the client pays its own part");
    assert!(client.comm_phlo + client.compute_phlo <= limit as u64);
    assert!(fired.runs[0].compute_phlo <= limit as u64, "the run's meter counts the client's own work only");
}
