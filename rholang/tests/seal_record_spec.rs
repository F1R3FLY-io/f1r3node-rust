use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployerId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::seal::{of_continuation, SealKind};
use rholang::rust::interpreter::accounting::rev_stack::{RevStacks, TokenKind, WalletOwner};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::RhoString;
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

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
