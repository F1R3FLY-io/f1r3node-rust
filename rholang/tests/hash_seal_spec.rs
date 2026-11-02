use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployerId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
use rholang::rust::interpreter::accounting::rev_stack::{RevStacks, WalletOwner};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoNumber, RhoString};
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

const ALICE: [u8; 65] = [4; 65];
const CAROL: [u8; 65] = [9; 65];

fn deployer_env(key: &[u8]) -> HashMap<String, Par> {
    let id = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: key.to_vec(),
        })),
    }]);
    HashMap::from([("rho:system:deployerId".to_string(), id)])
}

fn owner(key: &[u8]) -> WalletOwner {
    WalletOwner::from_public_key(&PublicKey::from_bytes(key)).unwrap()
}

async fn run_as(runtime: &RhoRuntimeImpl, key: &[u8], source: &str) -> Vec<InterpreterError> {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: PublicKey::from_bytes(key),
            deploy_id: vec![1],
            cosigners: Vec::new(),
            phlo_price: 0,
        })
        .await;
    runtime
        .evaluate(
            source,
            Cost::create(i64::MAX, "hash seal spec".to_string()),
            deployer_env(key),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .expect("evaluation runs")
        .errors
}

fn refused(errors: &[InterpreterError], reason: &str) -> bool {
    errors.iter().any(
        |error| matches!(error, InterpreterError::ReduceError(message) if message.contains(reason)),
    )
}

async fn sent(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
    runtime
        .get_data(&RhoString::create_par(channel.to_string()))
        .await
        .into_iter()
        .flat_map(|d| d.a.pars)
        .collect()
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

#[tokio::test]
async fn a_term_sealed_with_its_own_hash_runs_without_a_signer() {
    with_runtime("hash-seal-", |runtime| async move {
        let source = r#"{% @"out"!(1) %}[# { @"out"!(1) }]"#;
        let errors = run_as(&runtime, &ALICE, source).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(sent(&runtime, "out").await, vec![RhoNumber::create_par(1)]);
    })
    .await;
}

#[tokio::test]
async fn a_hash_seal_on_any_other_body_is_refused() {
    with_runtime("hash-seal-", |runtime| async move {
        let errors = run_as(&runtime, &ALICE, r#"{% Nil %}[# { @"out"!(1) }]"#).await;
        assert!(refused(&errors, "only when its body is P"), "{errors:?}");
        let errors = run_as(&runtime, &ALICE, r#"{% @"out"!(2) %}[# { @"out"!(1) }]"#).await;
        assert!(refused(&errors, "only when its body is P"), "{errors:?}");
        assert!(sent(&runtime, "out").await.is_empty());
    })
    .await;
}

#[tokio::test]
async fn a_hash_seal_on_a_bind_is_refused() {
    with_runtime("hash-seal-", |runtime| async move {
        let source = r#"for ({% @y <- @"c" %}[# { Nil }]) { Nil }"#;
        let errors = run_as(&runtime, &ALICE, source).await;
        assert!(refused(&errors, "only when its body is P"), "{errors:?}");
    })
    .await;
}

#[tokio::test]
async fn a_compound_with_a_hash_seal_needs_the_other_components_signers() {
    with_runtime("hash-seal-", |runtime| async move {
        let own = r#"new d(`rho:system:deployerId`) in {
            {% @"joint"!(1) %}[d (*) # { @"joint"!(1) }]
        }"#;
        assert!(run_as(&runtime, &ALICE, own).await.is_empty());
        assert_eq!(
            sent(&runtime, "joint").await,
            vec![RhoNumber::create_par(1)]
        );
        let foreign = r#"{% @"joint"!(2) %}[bob (*) # { @"joint"!(2) }]"#;
        let errors = run_as(&runtime, &ALICE, foreign).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
    })
    .await;
}

#[tokio::test]
async fn a_stored_hash_sealed_term_runs_for_any_deployment_that_fires_it() {
    with_runtime("hash-seal-", |runtime| async move {
        let store = r#"@"held"!({% @"ran"!(1) %}[# { @"ran"!(1) }])"#;
        assert!(run_as(&runtime, &ALICE, store).await.is_empty());
        let errors = run_as(&runtime, &CAROL, r#"for (@p <- @"held") { p }"#).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(sent(&runtime, "ran").await, vec![RhoNumber::create_par(1)]);
    })
    .await;
}

#[tokio::test]
async fn a_stack_cell_naming_a_hash_deposits_into_that_process_wallet() {
    with_runtime("hash-seal-", |runtime| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 100).unwrap();
        let errors = run_as(
            &runtime,
            &ALICE,
            r#"# { @"c"!(1) } :: # { @"c"!(1) } :: ()"#,
        )
        .await;
        assert!(errors.is_empty(), "{errors:?}");
        let process = Compiler::source_to_adt(r#"@"c"!(1)"#).unwrap();
        assert_eq!(
            stacks(&runtime).balance(&process_owner(&process)).unwrap(),
            2
        );
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 98);
    })
    .await;
}

#[tokio::test]
async fn each_instance_of_a_hash_sealed_template_has_its_own_wallet() {
    with_runtime("hash-seal-", |runtime| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 100).unwrap();
        let template = r#"for (@x <= @"in") { {% @"out"!(x) %}[# { @"out"!(x) }] } |
                          @"in"!(1) | @"in"!(2)"#;
        let errors = run_as(&runtime, &ALICE, template).await;
        assert!(errors.is_empty(), "{errors:?}");
        let mut out = sent(&runtime, "out").await;
        out.sort_by_key(RhoNumber::unapply);
        assert_eq!(out, vec![RhoNumber::create_par(1), RhoNumber::create_par(2)]);
        let errors = run_as(&runtime, &ALICE, r#"# { @"out"!(1) } :: ()"#).await;
        assert!(errors.is_empty(), "{errors:?}");
        let one = process_owner(&Compiler::source_to_adt(r#"@"out"!(1)"#).unwrap());
        let two = process_owner(&Compiler::source_to_adt(r#"@"out"!(2)"#).unwrap());
        assert_ne!(one, two);
        assert_eq!(stacks(&runtime).balance(&one).unwrap(), 1);
        assert_eq!(stacks(&runtime).balance(&two).unwrap(), 0);
    })
    .await;
}

#[tokio::test]
async fn a_hash_seal_is_checked_on_the_substituted_process() {
    with_runtime("hash-seal-", |runtime| async move {
        let same = r#"for (@x <- @"a") { {% @"y"!(1) %}[# { @"y"!(x) }] } | @"a"!(1)"#;
        let errors = run_as(&runtime, &ALICE, same).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(sent(&runtime, "y").await, vec![RhoNumber::create_par(1)]);
        let other = r#"for (@x <- @"b") { {% @"z"!(1) %}[# { @"z"!(x) }] } | @"b"!(2)"#;
        let errors = run_as(&runtime, &ALICE, other).await;
        assert!(refused(&errors, "only when its body is P"), "{errors:?}");
        assert!(sent(&runtime, "z").await.is_empty());
    })
    .await;
}

#[tokio::test]
async fn a_hash_sealed_region_may_not_read_the_running_deployment() {
    use rholang::rust::interpreter::accounting::rev_stack::process::self_sealed_envelope;
    with_runtime("hash-seal-", |runtime| async move {
        runtime.set_seal_records(true);
        let block = r#"{% new b(`rho:block:data`) in { b!("blk") } %}
                       [# { new b(`rho:block:data`) in { b!("blk") } }]"#;
        let errors = run_as(&runtime, &ALICE, block).await;
        assert!(refused(&errors, "answers from the running deployment"), "{errors:?}");

        let built = r#"{% new lookup(`rho:internal:registry_lookup`), ret in {
                            lookup!("rho:deploy:" ++ "data", *ret) |
                            for (d <- ret) { @"got"!(true) }
                          } %}
                       [# { new lookup(`rho:internal:registry_lookup`), ret in {
                            lookup!("rho:deploy:" ++ "data", *ret) |
                            for (d <- ret) { @"got"!(true) }
                          } }]"#;
        let errors = run_as(&runtime, &ALICE, built).await;
        assert!(refused(&errors, "answers from the running deployment"), "{errors:?}");
        assert!(sent(&runtime, "got").await.is_empty());

        let outside = r#"new lookup(`rho:internal:registry_lookup`), fresh, ret in {
                           lookup!("rho:deploy:" ++ "data", *ret) | for (d <- ret) { @"plain"!(true) }
                         }"#;
        assert!(run_as(&runtime, &ALICE, outside).await.is_empty());
        assert_eq!(sent(&runtime, "plain").await.len(), 1);

        let author = r#"@"held"!({% new d(`rho:system:deployerId`) in { @"who"!(*d) } %}
                                [# { new d(`rho:system:deployerId`) in { @"who"!(*d) } }])"#;
        assert!(run_as(&runtime, &ALICE, author).await.is_empty());
        let errors = run_as(&runtime, &CAROL, r#"for (@p <- @"held") { p }"#).await;
        assert!(errors.is_empty(), "{errors:?}");
        let who = sent(&runtime, "who").await;
        assert_eq!(who.len(), 1);
        assert_eq!(who[0], deployer_env(&ALICE)["rho:system:deployerId"]);

        let own = r#"{% new d(`rho:system:deployerId`) in { @"who"!(*d) } %}
                     [# { new d(`rho:system:deployerId`) in { @"who"!(*d) } }]"#;
        let term = Compiler::source_to_adt_with_normalizer_env(own, deployer_env(&ALICE)).unwrap();
        assert!(self_sealed_envelope(&term).is_some());
    })
    .await;
}

async fn with_seal_records<F, Fut>(f: F)
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    with_runtime("hash-seal-records-", |runtime: RhoRuntimeImpl| async move {
        runtime.set_seal_records(true);
        f(runtime).await
    })
    .await
}

#[tokio::test]
async fn with_seal_records_a_hash_seal_is_still_checked_on_the_substituted_process() {
    with_seal_records(|runtime| async move {
        let same = r#"for (@x <- @"a") { {% @"y"!(1) %}[# { @"y"!(x) }] } | @"a"!(1)"#;
        let errors = run_as(&runtime, &ALICE, same).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(sent(&runtime, "y").await, vec![RhoNumber::create_par(1)]);
        let other = r#"for (@x <- @"b") { {% @"z"!(1) %}[# { @"z"!(x) }] } | @"b"!(2)"#;
        let errors = run_as(&runtime, &ALICE, other).await;
        assert!(refused(&errors, "only when its body is P"), "{errors:?}");
        assert!(sent(&runtime, "z").await.is_empty());
    })
    .await;
}

#[tokio::test]
async fn with_seal_records_each_instance_of_a_hash_sealed_template_still_has_its_own_wallet() {
    with_seal_records(|runtime| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 100).unwrap();
        let template = r#"for (@x <= @"in") { {% @"out"!(x) %}[# { @"out"!(x) }] } |
                          @"in"!(1) | @"in"!(2)"#;
        let errors = run_as(&runtime, &ALICE, template).await;
        assert!(errors.is_empty(), "{errors:?}");
        let errors = run_as(&runtime, &ALICE, r#"# { @"out"!(1) } :: ()"#).await;
        assert!(errors.is_empty(), "{errors:?}");
        let one = process_owner(&Compiler::source_to_adt(r#"@"out"!(1)"#).unwrap());
        let two = process_owner(&Compiler::source_to_adt(r#"@"out"!(2)"#).unwrap());
        assert_ne!(one, two);
        assert_eq!(stacks(&runtime).balance(&one).unwrap(), 1);
        assert_eq!(stacks(&runtime).balance(&two).unwrap(), 0);
    })
    .await;
}
