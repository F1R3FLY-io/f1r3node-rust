use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostSignature, GDeployerId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::seal::of_continuation;
use rholang::rust::interpreter::accounting::rev_stack::{RevStacks, WalletOwner};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::reduce::is_elaborated_deployment;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::RhoString;
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

const ALICE: [u8; 65] = [4; 65];
const BOB: [u8; 65] = [9; 65];

fn id(key: &[u8]) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: key.to_vec(),
        })),
    }])
}

fn env(key: &[u8]) -> HashMap<String, Par> { HashMap::from([("rho:system:deployerId".to_string(), id(key))]) }

fn owner(key: &[u8]) -> WalletOwner { WalletOwner::from_public_key(&PublicKey::from_bytes(key)).unwrap() }

fn deploy_data(key: &[u8], envelope: u8, limit: u64) -> DeployData {
    DeployData {
        timestamp: 1,
        deployer_id: PublicKey::from_bytes(key),
        deploy_id: vec![envelope; 4],
        cosigners: Vec::new(),
        phlo_price: 5,
        phlo_limit: limit,
    }
}

async fn run(runtime: &RhoRuntimeImpl, key: &[u8], envelope: u8, source: &str) -> Vec<InterpreterError> {
    runtime.set_deploy_data(deploy_data(key, envelope, 50_000)).await;
    runtime
        .evaluate(
            source,
            Cost::create(i64::MAX, "elaboration spec".to_string()),
            env(key),
            Blake2b512Random::create_from_bytes(&[envelope; 32]),
        )
        .await
        .unwrap()
        .errors
}

async fn sent(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
    runtime
        .get_data(&RhoString::create_par(channel.to_string()))
        .await
        .into_iter()
        .flat_map(|d| d.a.pars)
        .collect()
}

fn refused(errors: &[InterpreterError], text: &str) -> bool {
    errors
        .iter()
        .any(|e| matches!(e, InterpreterError::ReduceError(m) if m.contains(text)))
}

async fn sealed<F, Fut>(f: F)
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    with_runtime("elaboration-", |runtime: RhoRuntimeImpl| async move {
        runtime.set_seal_records(true);
        let stacks = RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap());
        stacks.deposit(&owner(&ALICE), 1_000_000).unwrap();
        stacks.deposit(&owner(&BOB), 1_000_000).unwrap();
        f(runtime).await
    })
    .await
}

fn alice_origin() -> models::rhoapi::SealOrigin {
    models::rhoapi::SealOrigin {
        envelope: vec![1; 4],
        signers: vec![owner(&ALICE).canonical_bytes()],
        phlo_price: 5,
        phlo_limit: 50_000,
    }
}

#[tokio::test]
async fn a_term_naming_its_own_origin_and_the_deployment_kind_with_an_owner_who_did_not_sign_is_refused() {
    sealed(|runtime| async move {
        let mut par = Compiler::source_to_adt_elaborated(r#"@"ran"!(1)"#, env(&ALICE), alice_origin()).unwrap();
        assert!(is_elaborated_deployment(&par, &alice_origin()));
        par.cost_signed_terms[0].signature = Some(CostSignature {
            value: Some(Value::Name(id(&BOB))),
        });
        assert!(is_elaborated_deployment(&par, &alice_origin()), "the kind and the origin are still the deployment's");
        runtime.set_deploy_data(deploy_data(&ALICE, 1, 50_000)).await;
        let error = runtime
            .reducer
            .inj(par, Blake2b512Random::create_from_bytes(&[1; 32]))
            .await
            .unwrap_err();
        assert!(format!("{error:?}").contains("signers"), "{error:?}");
        assert!(sent(&runtime, "ran").await.is_empty());
    })
    .await;
}

#[tokio::test]
async fn an_unelaborated_term_is_not_a_deployment() {
    let plain = Compiler::source_to_adt(r#"@"ran"!(1)"#).unwrap();
    assert!(!is_elaborated_deployment(&plain, &alice_origin()));
    let elaborated = Compiler::source_to_adt_elaborated(r#"@"ran"!(1)"#, env(&ALICE), alice_origin()).unwrap();
    let mut other = alice_origin();
    other.envelope = vec![2; 4];
    assert!(!is_elaborated_deployment(&elaborated, &other));
    let explicit = Compiler::source_to_adt_explicit(
        r#"new d(`rho:system:deployerId`) in { {% @"ran"!(1) %}[d] }"#,
        env(&ALICE),
        alice_origin(),
    )
    .unwrap();
    assert!(!is_elaborated_deployment(&explicit, &alice_origin()), "a signed region is not the deployment seal");
}

#[tokio::test]
async fn a_seal_in_received_data_is_checked_against_the_running_membrane() {
    sealed(|runtime| async move {
        let publish = r#"new d(`rho:system:deployerId`) in { @"held"!({% @"ran"!(1) %}[d]) }"#;
        assert!(run(&runtime, &ALICE, 1, publish).await.is_empty());
        let errors = run(&runtime, &BOB, 2, r#"for (@p <- @"held") { p }"#).await;
        assert!(refused(&errors, "signers"), "{errors:?}");
        assert!(sent(&runtime, "ran").await.is_empty());
    })
    .await;
}


#[tokio::test]
async fn a_stored_body_fires_under_its_own_seal_and_carries_its_declared_limit() {
    sealed(|runtime| async move {
        let install = r#"new d(`rho:system:deployerId`) in {
            {% for (_ <- @"x") { @"fired"!(1) } %}[d ; 700]
        }"#;
        assert!(run(&runtime, &ALICE, 1, install).await.is_empty());
        let space = runtime.reducer.space.hot_store().unwrap();
        let waiting = space.get_continuations(&[RhoString::create_par("x".to_string())]);
        let record = of_continuation(&waiting[0].continuation).unwrap().unwrap();
        assert_eq!(record.owner, owner(&ALICE));
        assert_eq!(record.origin.envelope, vec![1; 4]);
        assert_eq!(record.origin.phlo_limit, 700, "the stored copy has the continuation seal's limit");
        assert!(run(&runtime, &BOB, 2, r#"@"x"!(0)"#).await.is_empty());
        assert_eq!(sent(&runtime, "fired").await.len(), 1);
    })
    .await;
}

#[tokio::test]
async fn a_received_continuation_is_stored_under_the_running_record() {
    sealed(|runtime| async move {
        let publish = r#"new d(`rho:system:deployerId`) in {
            @"held"!(for (_ <- @"q") { {% @"ran"!(1) %}[d ; 700] })
        }"#;
        assert!(run(&runtime, &ALICE, 1, publish).await.is_empty());
        assert!(run(&runtime, &BOB, 2, r#"for (@p <- @"held") { p }"#).await.is_empty());
        let space = runtime.reducer.space.hot_store().unwrap();
        let waiting = space.get_continuations(&[RhoString::create_par("q".to_string())]);
        let record = of_continuation(&waiting[0].continuation).unwrap().unwrap();
        assert_eq!(record.owner, owner(&BOB));
        assert_eq!(record.origin.envelope, vec![2; 4]);
        assert_eq!(record.origin.phlo_limit, 50_000, "a foreign seal's limit is not taken");
        let errors = run(&runtime, &BOB, 3, r#"@"q"!(0)"#).await;
        assert!(sent(&runtime, "ran").await.is_empty(), "{errors:?}");
    })
    .await;
}

#[tokio::test]
async fn a_signer_index_outside_its_own_origin_is_refused() {
    sealed(|runtime| async move {
        let par = Compiler::source_to_adt_elaborated(r#"@"ran"!(1)"#, env(&ALICE), alice_origin()).unwrap();
        runtime.set_deploy_data(deploy_data(&BOB, 2, 50_000)).await;
        let error = runtime
            .reducer
            .inj(par, Blake2b512Random::create_from_bytes(&[2; 32]))
            .await
            .unwrap_err();
        assert!(format!("{error:?}").contains("signer by index"), "{error:?}");
        assert!(sent(&runtime, "ran").await.is_empty());
    })
    .await;
}

#[tokio::test]
#[ignore = "measures the cost that elaboration adds"]
async fn measure_the_cost_of_elaboration() {
    use prost::Message;
    for term in [
        r#"@"x"!(1)"#,
        r#"new y in { y!(1) | for (_ <- y) { Nil } }"#,
        r#"new y, z in { y!(1) | for (_ <- y) { z!(2) } | for (_ <- z) { Nil } }"#,
    ] {
        let plain = Compiler::source_to_adt_with_normalizer_env(term, env(&ALICE)).unwrap();
        let elaborated = Compiler::source_to_adt_elaborated(term, env(&ALICE), alice_origin()).unwrap();
        let mut costs = Vec::new();
        for elaborate in [false, true] {
            let cost = with_runtime("elaboration-cost-", |runtime: RhoRuntimeImpl| async move {
                runtime.set_seal_records(true);
                runtime.set_deploy_data(deploy_data(&ALICE, 1, 50_000)).await;
                if !elaborate {
                    runtime.reducer.take_elaboration();
                }
                let result = runtime
                    .evaluate(term, Cost::create(i64::MAX, "measure".to_string()), env(&ALICE), Blake2b512Random::create_from_bytes(&[1; 32]))
                    .await
                    .unwrap();
                result.cost.value
            })
            .await;
            costs.push(cost);
        }
        eprintln!(
            "bytes {} -> {}, phlo {} -> {}: {term}",
            plain.encoded_len(),
            elaborated.encoded_len(),
            costs[0],
            costs[1]
        );
    }
}

#[tokio::test]
async fn a_seal_in_data_naming_the_running_owner_runs_and_the_owner_pays() {
    sealed(|runtime| async move {
        let contract = r#"new d(`rho:system:deployerId`) in { @"aid"!(*d) | for (@p <= @"eval") { p } }"#;
        assert!(run(&runtime, &ALICE, 1, contract).await.is_empty());
        let stacks = RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap());
        let before = stacks.balance(&owner(&ALICE)).unwrap();
        let send = r#"for (a <<- @"aid") { new slot in { @"eval"!({% @"ran"!(1) | slot :: () %}[a]) } }"#;
        let errors = run(&runtime, &BOB, 2, send).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(sent(&runtime, "ran").await.len(), 1);
        assert_eq!(stacks.balance(&owner(&ALICE)).unwrap(), before - 1, "u1 pays the stack cell");
    })
    .await;
}
