use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployerId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::bond::bond_balance;
use rholang::rust::interpreter::accounting::rev_stack::seal::{of_continuation, SealKind};
use rholang::rust::interpreter::accounting::rev_stack::{
    HotStoreCells, RevStacks, TokenKind, WalletOwner,
};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

const ALICE: [u8; 65] = [4; 65];
const BOB: [u8; 65] = [9; 65];

const LEGACY_GOLDEN_ROOT: &str = "e26a23507c0a4342d8f3542202053350254e0a325e43a0b08106cca3aacaaaea";

fn deployer_env(key: &[u8]) -> HashMap<String, Par> {
    let id = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: key.to_vec(),
        })),
    }]);
    HashMap::from([("rho:system:deployerId".to_string(), id)])
}

const LEGACY_TERM: &str = r#"new d(`rho:system:deployerId`), x in {
    for (@a <- @"golden") { @"out"!(a) } |
    for (@a <= @"served") { @"served-out"!(a) } |
    contract @"c"(y) = { y!(1) } |
    for (@a <- @"guarded" where a > 1) { Nil } |
    for (_ <- @"left"; _ <- @"right") { Nil } |
    @"left"!(1) |
    x!(*d) |
    for (@p <- x) { @"kept"!(p) }
}"#;

fn owner(key: &[u8]) -> WalletOwner {
    WalletOwner::from_public_key(&PublicKey::from_bytes(key)).unwrap()
}

fn cells(runtime: &RhoRuntimeImpl) -> HotStoreCells {
    HotStoreCells(runtime.reducer.space.hot_store().unwrap())
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

async fn run(
    runtime: &RhoRuntimeImpl,
    key: &[u8],
    cosigners: &[&[u8]],
    envelope: u8,
    source: &str,
) -> Vec<InterpreterError> {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: PublicKey::from_bytes(key),
            deploy_id: vec![envelope; 4],
            cosigners: cosigners.iter().map(|c| PublicKey::from_bytes(c)).collect(),
            phlo_price: 5,
            phlo_limit: 0,
        })
        .await;
    let mut env = deployer_env(key);
    for (i, c) in cosigners.iter().enumerate() {
        let id = deployer_env(c).remove("rho:system:deployerId").unwrap();
        env.insert(format!("rho:system:cosignerId:{i}"), id);
    }
    runtime
        .evaluate(
            source,
            Cost::create(i64::MAX, "seal record spec".to_string()),
            env,
            Blake2b512Random::create_from_bytes(&[envelope; 32]),
        )
        .await
        .unwrap()
        .errors
}

fn slot_owner(runtime: &RhoRuntimeImpl, channel: &str) -> WalletOwner {
    let data = runtime
        .reducer
        .space
        .hot_store()
        .unwrap()
        .get_data(&RhoString::create_par(channel.to_string()));
    let par = &data[0].a.pars[0];
    let par = match par.bundles.as_slice() {
        [bundle] => bundle.body.clone().unwrap(),
        _ => par.clone(),
    };
    match &par.unforgeables[0].unf_instance {
        Some(UnfInstance::GPrivateBody(private)) => WalletOwner::Unforgeable(private.id.clone()),
        other => panic!("{other:?}"),
    }
}

async fn sealed_runtime<F, Fut>(f: F)
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    with_runtime("seal-record-", |runtime: RhoRuntimeImpl| async move {
        runtime.set_seal_records(true);
        stacks(&runtime).deposit(&owner(&ALICE), 1_000).unwrap();
        stacks(&runtime).deposit(&owner(&BOB), 1_000).unwrap();
        f(runtime).await
    })
    .await
}

#[tokio::test]
async fn a_stored_continuation_carries_its_authors_record() {
    sealed_runtime(|runtime| async move {
        let store = r#"new d(`rho:system:deployerId`) in {
            for (@a <- @"golden") { Nil } |
            {% for (@a <- @"signed") { Nil } %}[d]
        }"#;
        assert!(run(&runtime, &ALICE, &[&BOB], 7, store).await.is_empty());
        let space = runtime.reducer.space.hot_store().unwrap();
        let at = |channel: &str| {
            let waiting = space.get_continuations(&[RhoString::create_par(channel.to_string())]);
            of_continuation(&waiting[0].continuation).unwrap().unwrap()
        };
        let top = at("golden");
        assert_eq!(top.kind, SealKind::Deployment);
        assert_eq!(top.owner, owner(&ALICE));
        assert_eq!(top.origin.envelope, vec![7; 4]);
        assert_eq!(top.origin.signers, vec![owner(&ALICE), owner(&BOB)]);
        assert_eq!(top.origin.phlo_price, 5);
        let signed = at("signed");
        assert_eq!(signed.kind, SealKind::Signed);
        assert_eq!(signed.owner, owner(&ALICE));
        assert_eq!(signed.origin, top.origin);
    })
    .await;
}

#[tokio::test]
async fn a_stack_pays_from_its_enclosing_seal_not_the_deployer() {
    sealed_runtime(|runtime| async move {
        let term = r#"new a(`rho:system:cosignerId:0`), slot in {
            {% @"x"!(1) | slot :: slot :: () %}[a] | @"slot"!(*slot)
        }"#;
        assert!(run(&runtime, &BOB, &[&ALICE], 1, term).await.is_empty());
        let slot = slot_owner(&runtime, "slot");
        assert_eq!(stacks(&runtime).balance(&slot).unwrap(), 2);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 998);
        assert_eq!(stacks(&runtime).balance(&owner(&BOB)).unwrap(), 1_000);
        let bare = r#"new slot2 in { slot2 :: () | @"slot2"!(*slot2) }"#;
        assert!(run(&runtime, &BOB, &[&ALICE], 2, bare).await.is_empty());
        assert_eq!(stacks(&runtime).balance(&owner(&BOB)).unwrap(), 999);
    })
    .await;
}

#[tokio::test]
async fn a_region_sealed_in_its_origin_runs_for_a_later_runner_and_pays_from_its_sealer() {
    sealed_runtime(|runtime| async move {
        let store = r#"new d(`rho:system:deployerId`), slot in {
            for (_ <= @"pub") { {% @"ran"!(1) | slot :: () %}[d] } | @"slot"!(*slot)
        }"#;
        assert!(run(&runtime, &ALICE, &[], 1, store).await.is_empty());
        let slot = slot_owner(&runtime, "slot");
        assert!(run(&runtime, &BOB, &[], 2, r#"@"pub"!(0)"#)
            .await
            .is_empty());
        assert_eq!(stacks(&runtime).balance(&slot).unwrap(), 1);
        assert!(run(&runtime, &ALICE, &[], 3, r#"@"pub"!(0)"#)
            .await
            .is_empty());
        assert_eq!(stacks(&runtime).balance(&slot).unwrap(), 2);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 998);
        assert_eq!(stacks(&runtime).balance(&owner(&BOB)).unwrap(), 1_000);
    })
    .await;
}

#[tokio::test]
async fn a_fired_continuation_does_not_inherit_the_firing_region() {
    sealed_runtime(|runtime| async move {
        let store = r#"new slot in { for (_ <- @"x") { slot :: () } | @"slot"!(*slot) }"#;
        assert!(run(&runtime, &BOB, &[], 1, store).await.is_empty());
        let slot = slot_owner(&runtime, "slot");
        let fire = r#"new d(`rho:system:deployerId`) in { {% @"x"!(1) %}[d] }"#;
        assert!(run(&runtime, &ALICE, &[], 2, fire).await.is_empty());
        assert_eq!(stacks(&runtime).balance(&slot).unwrap(), 1);
        assert_eq!(stacks(&runtime).balance(&owner(&BOB)).unwrap(), 999);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1_000);
    })
    .await;
}

#[tokio::test]
async fn a_foreign_seal_in_a_later_runners_own_code_is_refused() {
    sealed_runtime(|runtime| async move {
        let publish = r#"new d(`rho:system:deployerId`) in { @"aid"!(*d) }"#;
        assert!(run(&runtime, &ALICE, &[], 1, publish).await.is_empty());
        let steal = r#"new slot in { for (a <<- @"aid") { {% @"x"!(1) | slot :: () %}[a] } }"#;
        let errors = run(&runtime, &BOB, &[], 2, steal).await;
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, InterpreterError::ReduceError(m) if m.contains("signers"))),
            "{errors:?}"
        );
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1_000);
    })
    .await;
}

#[tokio::test]
async fn a_kind_transfer_draws_on_the_enclosing_seal() {
    sealed_runtime(|runtime| async move {
        let mint = r#"new k, d(`rho:system:deployerId`) in {
            {% d :: d :: d :: () %}[k] | @"pub"!(bundle0{*k}) | @"aid"!(*d)
        }"#;
        assert!(run(&runtime, &ALICE, &[], 1, mint).await.is_empty());
        let WalletOwner::Unforgeable(name) = slot_owner(&runtime, "pub") else {
            panic!("a kind is an unforgeable name");
        };
        let kind = TokenKind::from_name(name).unwrap();
        let held = |who: &WalletOwner| stacks(&runtime).of_kind(&kind).balance(who).unwrap();
        assert_eq!(held(&owner(&ALICE)), 3);

        let own = r#"new s in { for (kind <<- @"pub") { {% s :: () %}[kind] } }"#;
        let errors = run(&runtime, &BOB, &[], 2, own).await;
        assert!(!errors.is_empty(), "Bob holds none of the kind");
        let steal = r#"new s in {
            for (a <<- @"aid") { for (kind <<- @"pub") { {% {% s :: () %}[kind] %}[a] } }
        }"#;
        let errors = run(&runtime, &BOB, &[], 3, steal).await;
        assert!(!errors.is_empty(), "Bob's code cannot seal for Alice");
        assert_eq!(held(&owner(&ALICE)), 3);

        let give = r#"new d(`rho:system:deployerId`), s in {
            for (kind <<- @"pub") { {% {% s :: () %}[kind] %}[d] } | @"s"!(*s)
        }"#;
        let errors = run(&runtime, &ALICE, &[], 4, give).await;
        assert!(errors.is_empty(), "{errors:?}");
        let s = slot_owner(&runtime, "s");
        assert_eq!(held(&owner(&ALICE)), 2);
        assert_eq!(held(&s), 1);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1_000);
    })
    .await;
}

#[tokio::test]
async fn a_kind_minted_inside_data_still_mints_and_a_received_kind_name_still_moves_in_code() {
    sealed_runtime(|runtime| async move {
        let contract = r#"for (p <= @"eval") { *p }"#;
        assert!(run(&runtime, &ALICE, &[], 1, contract).await.is_empty());
        let mint = r#"new k, d(`rho:system:deployerId`) in {
            @"eval"!({% d :: d :: () %}[k]) | @"pub"!(bundle0{*k})
        }"#;
        assert!(run(&runtime, &BOB, &[], 2, mint).await.is_empty());
        let WalletOwner::Unforgeable(name) = slot_owner(&runtime, "pub") else {
            panic!("a kind is an unforgeable name");
        };
        let kind = TokenKind::from_name(name).unwrap();
        let held = |who: &WalletOwner| stacks(&runtime).of_kind(&kind).balance(who).unwrap();
        assert_eq!(held(&owner(&BOB)), 2);
        let give = r#"new s in { for (kind <<- @"pub") { {% s :: () %}[kind] } | @"s"!(*s) }"#;
        assert!(run(&runtime, &BOB, &[], 3, give).await.is_empty());
        assert_eq!(held(&owner(&BOB)), 1);
    })
    .await;
}

struct Expect {
    alice: u64,
    bob: u64,
    slot: u64,
    bonded: u64,
    refused: bool,
}

fn payloads() -> Vec<(&'static str, String, Expect)> {
    let bob = VaultAddress::from_public_key(&PublicKey::from_bytes(&BOB))
        .unwrap()
        .to_base58();
    let paid = |alice, bob, slot, bonded| Expect {
        alice,
        bob,
        slot,
        bonded,
        refused: false,
    };
    vec![
        (
            "owner seal",
            "{% slot :: () %}[a]".to_string(),
            Expect {
                refused: true,
                ..paid(1_000, 1_000, 0, 0)
            },
        ),
        (
            "foreign seal",
            "{% slot :: () %}[b]".to_string(),
            Expect {
                refused: true,
                ..paid(1_000, 1_000, 0, 0)
            },
        ),
        (
            "bare stack",
            "slot :: ()".to_string(),
            paid(999, 1_000, 1, 0),
        ),
        (
            "kind mint",
            "{% slot :: () %}[k]".to_string(),
            paid(1_000, 1_000, 0, 0),
        ),
        (
            "wallet",
            format!(r#"new w(`rho:rev:wallet`) in {{ w!("transfer", (*a, "{bob}", 100), "native") }}"#),
            paid(900, 1_100, 0, 0),
        ),
        (
            "bond",
            r#"new b(`rho:rev:bond`) in { b!("bond", (*a, 100), "native") }"#.to_string(),
            paid(900, 1_000, 0, 100),
        ),
    ]
}

async fn native_reply(runtime: &RhoRuntimeImpl) -> Option<bool> {
    let data = runtime.get_data(&RhoString::create_par("native".to_string())).await;
    let (ok, _) = RhoTuple2::unapply(&data.first()?.a.pars[0])?;
    RhoBoolean::unapply(&ok)
}

async fn route(contract: &str, deliveries: &[&str]) {
    let contract = contract.to_string();
    let deliveries: Vec<String> = deliveries.iter().map(|d| d.to_string()).collect();
    for (n, (name, payload, expect)) in payloads().into_iter().enumerate() {
        let contract = contract.clone();
        let deliveries = deliveries.clone();
        sealed_runtime(|runtime| async move {
            let setup =
                format!(r#"new d(`rho:system:deployerId`) in {{ @"aid"!(*d) | {contract} }}"#);
            assert!(run(&runtime, &ALICE, &[], 1, &setup).await.is_empty());
            let mut errors = Vec::new();
            for (i, delivery) in deliveries.iter().enumerate() {
                let source = if i == 0 {
                    format!(
                        r#"new slot, k, b(`rho:system:deployerId`) in {{
                            @"slot"!(*slot) | @"kind"!(bundle0{{*k}}) |
                            for (a <<- @"aid") {{ {} }}
                        }}"#,
                        delivery.replace("PAYLOAD", &payload)
                    )
                } else {
                    delivery.clone()
                };
                errors.extend(run(&runtime, &BOB, &[], 2 + i as u8, &source).await);
            }
            let refused = errors.iter().any(
                |e| matches!(e, InterpreterError::ReduceError(m) if m.contains("signers") || m.contains("origin")),
            );
            let slot = slot_owner(&runtime, "slot");
            let balances = (
                stacks(&runtime).balance(&owner(&ALICE)).unwrap(),
                stacks(&runtime).balance(&owner(&BOB)).unwrap(),
                stacks(&runtime).balance(&slot).unwrap(),
                bond_balance(std::sync::Arc::new(cells(&runtime)), &ALICE).unwrap(),
            );
            assert_eq!(
                balances,
                (expect.alice, expect.bob, expect.slot, expect.bonded),
                "{name} on route {n}: {errors:?}"
            );
            assert_eq!(refused, expect.refused, "{name} on route {n}: {errors:?}");
            if !expect.refused {
                assert!(errors.is_empty(), "{name} on route {n}: {errors:?}");
            }
            if name == "wallet" || name == "bond" {
                assert_eq!(
                    native_reply(&runtime).await,
                    Some(true),
                    "{name} on route {n}"
                );
            }
            if name == "kind mint" {
                let WalletOwner::Unforgeable(id) = slot_owner(&runtime, "kind") else {
                    panic!("a kind is an unforgeable name");
                };
                let kind = TokenKind::from_name(id).unwrap();
                assert_eq!(stacks(&runtime).of_kind(&kind).balance(&slot).unwrap(), 1);
            }
        })
        .await;
    }
}

#[tokio::test]
async fn route_drop_of_a_received_name() {
    route(r#"for (p <= @"r") { *p }"#, &[r#"@"r"!(PAYLOAD)"#]).await;
}

#[tokio::test]
async fn route_a_received_process_variable() {
    route(r#"for (@p <= @"r") { p }"#, &[r#"@"r"!(PAYLOAD)"#]).await;
}

#[tokio::test]
async fn route_a_method_result_run_as_a_process() {
    route(r#"for (@l <= @"r") { l.nth(0) }"#, &[r#"@"r"!([PAYLOAD])"#]).await;
}

#[tokio::test]
async fn route_a_received_value_in_a_stored_continuation_that_fires_later() {
    route(
        r#"for (@p <= @"r") { for (_ <- @"go") { p } }"#,
        &[r#"@"r"!(PAYLOAD)"#, r#"@"go"!(0)"#],
    )
    .await;
}

#[tokio::test]
async fn route_data_that_comes_back_out_of_the_tuplespace() {
    route(
        r#"for (_ <= @"go") { for (@q <<- @"store") { q } }"#,
        &[r#"@"store"!(PAYLOAD)"#, r#"@"go"!(0)"#],
    )
    .await;
}

#[tokio::test]
async fn route_a_persistent_send() {
    route(r#"for (@p <- @"r") { p }"#, &[r#"@"r"!!(PAYLOAD)"#]).await;
}

#[tokio::test]
async fn route_a_match_binding() {
    route(
        r#"for (@m <= @"r") { match m { x => x } }"#,
        &[r#"@"r"!(PAYLOAD)"#],
    )
    .await;
}

#[tokio::test]
async fn route_a_received_process_inside_the_owners_own_region() {
    route(r#"for (@p <= @"r") { {% p %}[d] }"#, &[r#"@"r"!(PAYLOAD)"#]).await;
}

#[tokio::test]
async fn route_new_if_and_bundle_bodies() {
    route(
        r#"for (@p <= @"r") { new z in { if (true) { bundle0 { p } } else { Nil } } }"#,
        &[r#"@"r"!(PAYLOAD)"#],
    )
    .await;
}

#[tokio::test]
async fn route_a_continuation_stored_by_received_data() {
    route(
        r#"for (@p <= @"r") { p }"#,
        &[r#"@"r"!(for (_ <- @"go") { PAYLOAD })"#, r#"@"go"!(0)"#],
    )
    .await;
}

#[tokio::test]
async fn route_a_received_value_in_a_stored_contract_body() {
    route(
        r#"for (@p <= @"r") { contract @"c"(_) = { p } }"#,
        &[r#"@"r"!(PAYLOAD)"#, r#"@"c"!(0)"#],
    )
    .await;
}

#[tokio::test]
async fn a_legacy_runtime_keeps_the_golden_root() {
    with_runtime(
        "seal-record-golden-",
        |runtime: RhoRuntimeImpl| async move {
            runtime
                .set_deploy_data(DeployData {
                    timestamp: 1,
                    deployer_id: PublicKey::from_bytes(&ALICE),
                    deploy_id: vec![1],
                    cosigners: Vec::new(),
                    phlo_price: 0,
                    phlo_limit: 0,
                })
                .await;
            let result = runtime
                .evaluate(
                    LEGACY_TERM,
                    Cost::create(i64::MAX, "test".to_string()),
                    deployer_env(&ALICE),
                    Blake2b512Random::create_from_bytes(&[7; 32]),
                )
                .await
                .unwrap();
            assert!(result.errors.is_empty(), "{:?}", result.errors);
            let mut runtime = runtime;
            let root = hex::encode(runtime.create_checkpoint().await.root.bytes());
            assert_eq!(root, LEGACY_GOLDEN_ROOT);
        },
    )
    .await;
}

#[tokio::test]
async fn a_v6_runtime_records_seals_and_changes_the_root() {
    with_runtime("seal-record-v6-", |runtime: RhoRuntimeImpl| async move {
        runtime.set_seal_records(true);
        runtime
            .set_deploy_data(DeployData {
                timestamp: 1,
                deployer_id: PublicKey::from_bytes(&ALICE),
                deploy_id: vec![1],
                cosigners: Vec::new(),
                phlo_price: 0,
                phlo_limit: 0,
            })
            .await;
        let result = runtime
            .evaluate(
                LEGACY_TERM,
                Cost::create(i64::MAX, "test".to_string()),
                deployer_env(&ALICE),
                Blake2b512Random::create_from_bytes(&[7; 32]),
            )
            .await
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let mut runtime = runtime;
        let root = hex::encode(runtime.create_checkpoint().await.root.bytes());
        assert_ne!(root, LEGACY_GOLDEN_ROOT);
    })
    .await;
}
