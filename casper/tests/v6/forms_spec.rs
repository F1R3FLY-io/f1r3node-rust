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
        .replay_recorded(
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
    let heavy = (0..12).fold("Nil".to_string(), |inner, _| {
        format!("new y in {{ y!(1) | for (_ <- y) {{ {inner} }} }}")
    });
    let stored = format!(
        r#"new d(`rho:system:deployerId`) in {{ {{% for (_ <- @"x") {{ {heavy} }} %}}[d ; 20000] }}"#
    );
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

mod elaboration {
    use std::collections::HashMap;

    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{CostSignature, GDeployerId, GUnforgeable, Par, SealOrigin};
    use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use models::rust::rholang::sorter::sortable::Sortable;
    use rholang::rust::interpreter::accounting::rev_stack::seal::origin_digest;
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::compiler::normalizer::cost_accounting::elaborate::{
        KIND_DEPLOYMENT, KIND_PROCESS, KIND_SIGNED,
    };
    use rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground;

    use super::BODY;

    const LIMIT: u64 = 9_000;

    fn id(key: u8) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: vec![key; 65],
            })),
        }])
    }

    fn env() -> HashMap<String, Par> {
        HashMap::from([
            ("rho:system:deployerId".to_string(), id(4)),
            ("rho:system:cosignerId:0".to_string(), id(9)),
        ])
    }

    fn origin() -> SealOrigin {
        SealOrigin {
            envelope: vec![7; 4],
            signers: vec![vec![4; 65], vec![9; 65]],
            phlo_price: 3,
            phlo_limit: LIMIT,
        }
    }

    fn elaborated(sugar: &str) -> Par {
        ParSortMatcher::sort_match(&Compiler::source_to_adt_elaborated(sugar, env(), origin()).unwrap()).term
    }

    fn to_deployment(par: &mut Par) {
        let u1 = CostSignature {
            value: Some(Value::Ground(canon_ground("u1"))),
        };
        for term in par.cost_signed_terms.iter_mut() {
            if term.signature.as_ref() == Some(&u1) {
                term.signature = Some(CostSignature {
                    value: Some(Value::Signer(0)),
                });
                term.seal.as_mut().unwrap().kind = KIND_DEPLOYMENT;
            }
            term.body.iter_mut().for_each(to_deployment);
        }
        for receive in par.receives.iter_mut() {
            receive.body.iter_mut().for_each(to_deployment);
        }
        for new in par.news.iter_mut() {
            new.p.iter_mut().for_each(to_deployment);
        }
        for m in par.matches.iter_mut() {
            for case in m.cases.iter_mut() {
                case.source.iter_mut().for_each(to_deployment);
            }
        }
    }

    fn form(explicit: &str) -> Par {
        let mut par = Compiler::source_to_adt_explicit(explicit, env(), origin()).unwrap();
        to_deployment(&mut par);
        ParSortMatcher::sort_match(&par).term
    }

    fn body_under(seal: &str) -> String {
        format!("new y in {{ y!(1) | for (_ <- y) {{ {{% Nil %}}[{seal}] }} }}")
    }

    const NAMES: &str = r#"new d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in"#;

    #[test]
    fn the_implicit_seal_elaborates_to_the_two_party_form_for_the_installer() {
        let sugar = format!(r#"for (_ <- @"x") {{ {BODY} }}"#);
        let explicit = format!(r#"{{% for (_ <- @"x") {{ {{% {} %}}[u1] }} %}}[u1]"#, body_under("u1"));
        assert_eq!(elaborated(&sugar), form(&explicit));
    }

    #[test]
    fn a_redex_under_the_implicit_seal_elaborates_to_the_one_party_form() {
        let sugar = format!(r#"for (_ <- @"x") {{ {BODY} }} | @"x"!(1)"#);
        let explicit = format!(
            r#"{{% for (_ <- @"x") {{ {{% {} %}}[u1] }} | @"x"!(1) %}}[u1]"#,
            body_under("u1")
        );
        assert_eq!(elaborated(&sugar), form(&explicit));
    }

    #[test]
    fn the_lollipop_elaborates_to_s1_for_the_comm_and_s2_for_the_continuation() {
        let sugar = format!(r#"{NAMES} {{ {{% for (_ <- @"x") {{ {BODY} }} %}}[d -o c] | @"x"!(1) }}"#);
        let explicit = format!(
            r#"{{% {NAMES} {{ {{% for (_ <- @"x") {{ {{% {} %}}[c] }} %}}[d] | @"x"!(1) }} %}}[u1]"#,
            body_under("c")
        );
        assert_eq!(elaborated(&sugar), form(&explicit));
    }

    #[test]
    fn a_nested_region_elaborates_to_its_compound_seal() {
        let sugar = format!(r#"{NAMES} {{ {{% {{% {BODY} %}}[c] %}}[d] | @"z"!(0) }}"#);
        let explicit = format!(
            r#"{{% {NAMES} {{ {{% {} %}}[d (*) c] | @"z"!(0) }} %}}[u1]"#,
            body_under("c")
        );
        assert_eq!(elaborated(&sugar), form(&explicit));
    }

    #[test]
    fn a_hash_sealed_region_keeps_its_body_and_takes_the_process_kind() {
        let p = r#"@"h"!(1)"#;
        let sugar = format!(r#"{{% {p} %}}[# {{ {p} }}] | @"z"!(0)"#);
        let explicit = format!(r#"{{% {{% {p} %}}[# {{ {p} }}] | @"z"!(0) %}}[u1]"#);
        let par = elaborated(&sugar);
        assert_eq!(par, form(&explicit));
        let region = &par.cost_signed_terms[0].body.as_ref().unwrap().cost_signed_terms[0];
        assert_eq!(region.seal.as_ref().unwrap().kind, KIND_PROCESS);
    }

    #[test]
    fn a_declared_limit_seals_the_region_and_its_continuations() {
        let sugar = format!(r#"{NAMES} {{ {{% for (_ <- @"x") {{ {BODY} }} %}}[d ; 5000] | @"z"!(0) }}"#);
        let explicit = format!(
            r#"{{% {NAMES} {{ {{% for (_ <- @"x") {{ {{% {} %}}[d ; 5000] }} %}}[d ; 5000] | @"z"!(0) }} %}}[u1]"#,
            body_under("d ; 5000")
        );
        let par = elaborated(&sugar);
        assert_eq!(par, form(&explicit));
        let region = &par.cost_signed_terms[0].body.as_ref().unwrap().news[0].p.as_ref().unwrap().cost_signed_terms[0];
        let seal = region.seal.as_ref().unwrap();
        assert_eq!((seal.kind, seal.phlo_limit), (KIND_SIGNED, 5_000));
    }

    #[test]
    fn an_undeclared_limit_is_the_deployments_limit() {
        let par = elaborated(r#"for (_ <- @"x") { Nil }"#);
        let top = &par.cost_signed_terms[0];
        let seal = top.seal.as_ref().unwrap();
        assert_eq!(seal.kind, KIND_DEPLOYMENT);
        assert_eq!(seal.origin, origin_digest(&origin()));
        assert_eq!(seal.phlo_limit, LIMIT);
        let continuation = &top.body.as_ref().unwrap().receives[0].body.as_ref().unwrap().cost_signed_terms[0];
        assert_eq!(continuation.seal, top.seal);
    }

    #[test]
    fn a_process_sent_as_data_is_not_elaborated() {
        let par = elaborated(r#"@"x"!(for (_ <- @"y") { Nil })"#);
        let sent = &par.cost_signed_terms[0].body.as_ref().unwrap().sends[0].data[0];
        assert!(sent.receives[0].body.as_ref().unwrap().cost_signed_terms.is_empty());
    }

    #[test]
    fn a_binder_that_shadows_its_seal_is_refused() {
        let sugar = r#"new d(`rho:system:deployerId`) in { {% for (d <- @"x") { Nil } %}[d] }"#;
        let error = Compiler::source_to_adt_elaborated(sugar, env(), origin()).unwrap_err();
        assert!(format!("{error:?}").contains("rebound"), "{error:?}");
    }

}

#[tokio::test]
async fn the_default_seal_limit_is_the_signed_limit_when_the_run_is_limited_below_it() {
    use casper::rust::rholang::runtime::RuntimeOps;
    use rholang::rust::interpreter::accounting::rev_stack::seal::of_continuation;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rholang::rust::interpreter::rho_type::RhoString;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let bob = party();
    let (rm, pre) = shard(&[&bob]).await;
    let mut install = envelope(r#"for (_ <- @"x") { Nil }"#, 1, &bob, 1).data;
    install.phlo_limit = 50_000;
    let install = Signed::create(install, Box::new(Secp256k1), bob.sk.clone()).unwrap();
    let mut ops = RuntimeOps::new(rm.spawn_runtime().await.unwrap());
    ops.runtime.reset(&Blake2b256Hash::from_bytes_prost(&pre)).await.unwrap();
    ops.runtime.set_seal_records(true);
    let (processed, _) = ops.process_deploy_limited(install, 3_000).await.unwrap();
    assert!(!processed.is_failed, "{processed:?}");
    assert!(processed.cost.cost <= 3_000, "the run is limited to the committed bound");
    let space = ops.runtime.reducer.space.hot_store().unwrap();
    let waiting = space.get_continuations(&[RhoString::create_par("x".to_string())]);
    let record = of_continuation(&waiting[0].continuation).unwrap().unwrap();
    assert_eq!(record.origin.phlo_limit, 50_000, "the stored seal carries the signed phloLimit");
}
