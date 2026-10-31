use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployerId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::region::named_owners;
use rholang::rust::interpreter::accounting::rev_stack::{RevStacks, WalletOwner};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
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

fn named(source: &str) -> Vec<WalletOwner> {
    let term = Compiler::source_to_adt_with_normalizer_env(source, deployer_env(&ALICE)).unwrap();
    named_owners(&term)
}

async fn run_as(runtime: &RhoRuntimeImpl, key: &[u8], source: &str) -> Vec<InterpreterError> {
    run_with(runtime, key, &[], source).await
}

async fn run_with(
    runtime: &RhoRuntimeImpl,
    key: &[u8],
    cosigners: &[&[u8]],
    source: &str,
) -> Vec<InterpreterError> {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: PublicKey::from_bytes(key),
            deploy_id: vec![1],
            cosigners: cosigners.iter().map(|c| PublicKey::from_bytes(c)).collect(),
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
            Cost::create(i64::MAX, "signed region spec".to_string()),
            env,
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

const OWN: &str = r#"new d(`rho:system:deployerId`) in { {% @"out"!(1) %}[d] }"#;

#[test]
fn the_gate_sees_the_deployer_as_the_only_name_of_an_own_signed_term() {
    assert_eq!(named(OWN), vec![owner(&ALICE)]);
    assert_eq!(
        named(r#"new d(`rho:system:deployerId`) in { for ({% @y <- @"c" %}[d]) { Nil } }"#),
        vec![owner(&ALICE)]
    );
    assert_eq!(named(r#"new x in { x!(1) }"#), vec![]);
}

#[test]
fn the_gate_sees_a_foreign_owner_for_any_other_signature() {
    for source in [
        r#"new s in { {% Nil %}[s] }"#,
        r#"{% Nil %}[bob]"#,
        r#"{% Nil %}[# { 1 }]"#,
        r#"new d(`rho:system:deployerId`), s in { {% Nil %}[d (*) s] }"#,
        r#"for (d <- @"k") { {% Nil %}[d] }"#,
        r#"new d(`rho:system:deployerId`) in { [{% Nil %}[d], {% Nil %}[e]] }"#,
    ] {
        let names = named(source);
        assert!(!names.is_empty(), "{source}");
        assert!(names.iter().any(|n| *n != owner(&ALICE)), "{source}");
    }
}

#[tokio::test]
async fn a_signed_term_of_the_deployer_runs_its_body() {
    with_runtime("signed-region-", |runtime| async move {
        assert!(run_as(&runtime, &ALICE, OWN).await.is_empty());
        let probe = r#"for (@1 <- @"out") { {% Nil %}[bob] }"#;
        let errors = run_as(&runtime, &ALICE, probe).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
    })
    .await;
}

#[tokio::test]
async fn a_signed_term_of_another_signature_is_refused() {
    with_runtime("signed-region-", |runtime| async move {
        let errors = run_as(&runtime, &ALICE, r#"new s in { {% Nil %}[s] }"#).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
        let errors = run_as(&runtime, &ALICE, r#"{% Nil %}[bob]"#).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
    })
    .await;
}

#[tokio::test]
async fn a_signed_bind_must_name_the_deployer() {
    with_runtime("signed-region-", |runtime| async move {
        let own = r#"new d(`rho:system:deployerId`) in { for ({% @y <- @"c" %}[d]) { Nil } }"#;
        assert!(run_as(&runtime, &ALICE, own).await.is_empty());
        let foreign = r#"new s in { for ({% @y <- @"c" %}[s]) { Nil } }"#;
        let errors = run_as(&runtime, &ALICE, foreign).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
    })
    .await;
}

#[tokio::test]
async fn a_stored_signed_term_runs_only_for_its_signer() {
    with_runtime("signed-region-", |runtime| async move {
        let store = r#"new d(`rho:system:deployerId`) in { @"held"!({% Nil %}[d]) }"#;
        assert!(run_as(&runtime, &ALICE, store).await.is_empty());
        let errors = run_as(&runtime, &CAROL, r#"for (@p <- @"held") { p }"#).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");

        assert!(run_as(&runtime, &ALICE, store).await.is_empty());
        let errors = run_as(&runtime, &ALICE, r#"for (@p <- @"held") { p }"#).await;
        assert!(errors.is_empty(), "{errors:?}");
    })
    .await;
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

async fn take(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
    runtime
        .get_data(&RhoString::create_par(channel.to_string()))
        .await
        .into_iter()
        .flat_map(|d| d.a.pars)
        .collect()
}

#[tokio::test]
async fn each_stack_cell_moves_one_unit_from_the_writer_to_the_wallet_at_its_signature() {
    with_runtime("signed-region-", |runtime| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 1_000).unwrap();
        let source = r#"new w(`rho:rev:wallet`), slot in {
            slot :: slot :: slot :: () | w!("balance", *slot, "slot")
        }"#;
        let errors = run_as(&runtime, &ALICE, source).await;
        assert!(errors.is_empty(), "{errors:?}");
        let reply = take(&runtime, "slot").await;
        let (ok, balance) = RhoTuple2::unapply(&reply[0]).unwrap();
        assert_eq!(RhoBoolean::unapply(&ok), Some(true));
        assert_eq!(RhoNumber::unapply(&balance), Some(3));
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 997);
    })
    .await;
}

#[tokio::test]
async fn a_stack_to_the_writers_own_signature_conserves_the_writers_balance() {
    with_runtime("signed-region-", |runtime| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 10).unwrap();
        let source = r#"new d(`rho:system:deployerId`) in { d :: d :: () }"#;
        assert!(run_as(&runtime, &ALICE, source).await.is_empty());
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 10);
    })
    .await;
}

#[tokio::test]
async fn a_stack_the_writer_cannot_pay_moves_nothing() {
    with_runtime("signed-region-", |runtime| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 1).unwrap();
        let errors = run_as(&runtime, &ALICE, "new s, t in { s :: t :: () }").await;
        assert!(refused(&errors, "cannot be paid"), "{errors:?}");
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1);
        let errors = run_as(&runtime, &ALICE, "{% Nil %}[bob] | bob :: ()").await;
        assert!(!errors.is_empty());
    })
    .await;
}

const JOINT: &str = r#"new d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in {
    {% @"joint"!(1) %}[d (*) c]
}"#;

#[tokio::test]
async fn a_compound_of_signers_runs_and_a_compound_with_a_non_signer_is_refused() {
    with_runtime("signed-region-", |runtime| async move {
        assert!(run_with(&runtime, &ALICE, &[&CAROL], JOINT)
            .await
            .is_empty());
        let errors = run_with(
            &runtime,
            &ALICE,
            &[&CAROL],
            r#"new d(`rho:system:deployerId`), s in { {% Nil %}[d (*) s] }"#,
        )
        .await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
        let stored = r#"new c(`rho:system:cosignerId:0`) in { @"held"!({% Nil %}[c]) }"#;
        assert!(run_with(&runtime, &ALICE, &[&CAROL], stored)
            .await
            .is_empty());
        let errors = run_as(&runtime, &ALICE, r#"for (@p <- @"held") { p }"#).await;
        assert!(refused(&errors, "deployment's signers"), "{errors:?}");
    })
    .await;
}
