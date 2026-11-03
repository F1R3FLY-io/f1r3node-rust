use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployerId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::bond::bond_balance;
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::hooks::{
    install_genesis_hooks, rev_hook, HookAuthority, RevHook,
};
use rholang::rust::interpreter::accounting::rev_stack::{HotStoreCells, RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

const ALICE: [u8; 65] = [4; 65];
const BOB: [u8; 65] = [9; 65];
const MALLORY: [u8; 65] = [7; 65];

fn id(key: &[u8]) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: key.to_vec(),
        })),
    }])
}

fn owner(key: &[u8]) -> WalletOwner {
    WalletOwner::from_public_key(&PublicKey::from_bytes(key)).unwrap()
}

fn address(key: &[u8]) -> String {
    VaultAddress::from_public_key(&PublicKey::from_bytes(key))
        .unwrap()
        .to_base58()
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

fn cells(runtime: &RhoRuntimeImpl) -> HotStoreCells {
    HotStoreCells(runtime.reducer.space.hot_store().unwrap())
}

async fn run(runtime: &RhoRuntimeImpl, key: &[u8], cosigners: &[&[u8]], envelope: u8, source: &str) {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: PublicKey::from_bytes(key),
            deploy_id: vec![envelope; 4],
            cosigners: cosigners.iter().map(|c| PublicKey::from_bytes(c)).collect(),
            phlo_price: 1,
        })
        .await;
    let mut env = HashMap::from([("rho:system:deployerId".to_string(), id(key))]);
    for (i, c) in cosigners.iter().enumerate() {
        env.insert(format!("rho:system:cosignerId:{i}"), id(c));
    }
    let errors = runtime
        .evaluate(
            source,
            Cost::create(i64::MAX, "seal wiring spec".to_string()),
            env,
            Blake2b512Random::create_from_bytes(&[envelope; 32]),
        )
        .await
        .unwrap()
        .errors;
    assert!(errors.is_empty(), "{errors:?}");
}

async fn ok(runtime: &RhoRuntimeImpl, channel: &str) -> bool {
    let data = runtime
        .get_data(&RhoString::create_par(channel.to_string()))
        .await;
    let (ok, _) = RhoTuple2::unapply(&data.first().expect("a reply").a.pars[0]).unwrap();
    RhoBoolean::unapply(&ok).unwrap()
}

async fn sealed<F, Fut>(f: F)
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    with_runtime("seal-wiring-", |runtime: RhoRuntimeImpl| async move {
        runtime.set_seal_records(true);
        for key in [ALICE, BOB, MALLORY] {
            stacks(&runtime).deposit(&owner(&key), 1_000).unwrap();
        }
        f(runtime).await
    })
    .await
}

#[tokio::test]
async fn a_transfer_draws_only_on_a_wallet_whose_owner_holds_the_enclosing_seal() {
    sealed(|runtime| async move {
        let stored = format!(
            r#"new w(`rho:rev:wallet`) in {{
                 contract @"pay"(@from) = {{ w!("transfer", (from, "{}", 100), "stolen") }}
               }}"#,
            address(&MALLORY)
        );
        run(&runtime, &MALLORY, &[], 1, &stored).await;
        run(&runtime, &ALICE, &[], 2, r#"new d(`rho:system:deployerId`) in { @"pay"!(*d) }"#).await;
        assert!(!ok(&runtime, "stolen").await);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1_000);

        let own = format!(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{
                 w!("transfer", (*d, "{}", 100), "own")
               }}"#,
            address(&MALLORY)
        );
        run(&runtime, &ALICE, &[], 3, &own).await;
        assert!(ok(&runtime, "own").await);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 900);
    })
    .await;
}

#[tokio::test]
async fn a_bond_draws_only_on_the_holder_of_the_enclosing_seal() {
    sealed(|runtime| async move {
        let stored = r#"new b(`rho:rev:bond`) in {
                          contract @"bond-for"(@who) = { b!("bond", (who, 100), "forced") }
                        }"#;
        run(&runtime, &MALLORY, &[], 1, stored).await;
        run(&runtime, &ALICE, &[], 2, r#"new d(`rho:system:deployerId`) in { @"bond-for"!(*d) }"#).await;
        assert!(!ok(&runtime, "forced").await);
        assert_eq!(bond_balance(std::sync::Arc::new(cells(&runtime)), &ALICE).unwrap(), 0);

        let own = r#"new b(`rho:rev:bond`), d(`rho:system:deployerId`) in {
                       b!("bond", (*d, 100), "bonded")
                     }"#;
        run(&runtime, &ALICE, &[], 3, own).await;
        assert!(ok(&runtime, "bonded").await);
        assert_eq!(bond_balance(std::sync::Arc::new(cells(&runtime)), &ALICE).unwrap(), 100);
    })
    .await;
}

#[tokio::test]
async fn the_compute_wallet_spends_under_the_quorum_or_the_installed_hook_and_nothing_it_calls() {
    sealed(|runtime| async move {
        install_genesis_hooks(
            &cells(&runtime),
            &HookAuthority::new(vec![owner(&ALICE), owner(&BOB)], 2).unwrap(),
        )
        .unwrap();
        stacks(&runtime).deposit(&compute_wallet_owner(), 100).unwrap();
        let payee = address(&MALLORY);
        let spend = |out: &str| {
            format!(r#"new w(`rho:rev:wallet`) in {{ w!("spendCompute", ("{payee}", 10), "{out}") }}"#)
        };

        run(&runtime, &ALICE, &[&BOB], 1, &spend("bare")).await;
        assert!(!ok(&runtime, "bare").await);
        let alone = format!(
            r#"new d(`rho:system:deployerId`) in {{ {{% {} %}}[d] }}"#,
            spend("alone")
        );
        run(&runtime, &ALICE, &[&BOB], 2, &alone).await;
        assert!(!ok(&runtime, "alone").await);
        let quorum = format!(
            r#"new d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in {{
                 {{% {} %}}[d (*) c]
               }}"#,
            spend("quorum")
        );
        run(&runtime, &ALICE, &[&BOB], 3, &quorum).await;
        assert!(ok(&runtime, "quorum").await);
        assert_eq!(stacks(&runtime).balance(&compute_wallet_owner()).unwrap(), 90);

        let called = format!(
            r#"new w(`rho:rev:wallet`) in {{
                 contract @"helper"(_) = {{ w!("spendCompute", ("{payee}", 5), "called") }}
               }}"#
        );
        run(&runtime, &MALLORY, &[], 4, &called).await;

        let hook = spend("as-hook");
        let calls = r#"@"helper"!(1)"#;
        let install = |h: &str, out: &str| {
            format!(
                r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`), ack in {{
                     {{% w!("replaceHook", {{ {h} }}, *ack) %}}[d (*) c] |
                     for (_ <- ack) {{ {{% {h} %}}[# {{ {h} }}] | @"{out}"!((true, Nil)) }}
                   }}"#
            )
        };
        run(&runtime, &ALICE, &[&BOB], 5, &install(&hook, "installed")).await;
        assert!(ok(&runtime, "installed").await);
        assert!(matches!(rev_hook(&cells(&runtime)).unwrap(), Some(RevHook::Contract(_))));
        assert!(ok(&runtime, "as-hook").await);
        assert_eq!(stacks(&runtime).balance(&compute_wallet_owner()).unwrap(), 80);

        run(&runtime, &ALICE, &[&BOB], 6, &install(calls, "installed-caller")).await;
        assert!(!ok(&runtime, "called").await);
        assert_eq!(stacks(&runtime).balance(&compute_wallet_owner()).unwrap(), 80);
    })
    .await;
}

#[tokio::test]
async fn a_hook_is_replaced_only_inside_a_region_the_quorum_sealed() {
    sealed(|runtime| async move {
        install_genesis_hooks(
            &cells(&runtime),
            &HookAuthority::new(vec![owner(&ALICE), owner(&BOB)], 2).unwrap(),
        )
        .unwrap();
        let unsealed = r#"new w(`rho:rev:wallet`) in { w!("replaceHook", { @"h"!(1) }, "unsealed") }"#;
        run(&runtime, &ALICE, &[&BOB], 1, unsealed).await;
        assert!(!ok(&runtime, "unsealed").await);
        assert_eq!(rev_hook(&cells(&runtime)).unwrap(), Some(RevHook::Noop));

        let sealed_term = r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), c(`rho:system:cosignerId:0`) in {
                               {% w!("replaceHook", { @"h"!(1) }, "sealed") %}[d (*) c]
                             }"#;
        run(&runtime, &ALICE, &[&BOB], 2, sealed_term).await;
        assert!(ok(&runtime, "sealed").await);
        assert!(matches!(rev_hook(&cells(&runtime)).unwrap(), Some(RevHook::Contract(_))));
    })
    .await;
}

#[tokio::test]
async fn a_wallet_or_bond_call_inside_data_received_at_run_time_moves_nothing() {
    sealed(|runtime| async move {
        let evaluator = r#"new d(`rho:system:deployerId`) in { @"alice-id"!(*d) | for (p <= @"eval") { *p } }"#;
        run(&runtime, &ALICE, &[], 1, evaluator).await;
        let payee = address(&MALLORY);
        let sent = format!(
            r#"for (@a <- @"alice-id") {{
                 @"eval"!({{ new w(`rho:rev:wallet`) in {{ w!("transfer", (a, "{payee}", 100), "moved") }} }}) |
                 @"eval"!({{ new b(`rho:rev:bond`) in {{ b!("bond", (a, 100), "bonded") }} }}) |
                 @"eval"!({{ new w(`rho:rev:wallet`) in {{ w!("balance", "{payee}", "read") }} }}) |
                 @"alice-id"!(a)
               }}"#
        );
        run(&runtime, &MALLORY, &[], 2, &sent).await;
        assert!(!ok(&runtime, "moved").await);
        assert!(!ok(&runtime, "bonded").await);
        assert!(ok(&runtime, "read").await);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1_000);
        assert_eq!(stacks(&runtime).balance(&owner(&MALLORY)).unwrap(), 1_000);
        assert_eq!(bond_balance(std::sync::Arc::new(cells(&runtime)), &ALICE).unwrap(), 0);

        let own = format!(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{
                 w!("transfer", (*d, "{payee}", 100), "own")
               }}"#
        );
        run(&runtime, &ALICE, &[], 3, &own).await;
        assert!(ok(&runtime, "own").await);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 900);
    })
    .await;
}

#[tokio::test]
async fn a_bond_admin_call_inside_data_received_at_run_time_moves_nothing() {
    sealed(|runtime| async move {
        let bond = r#"new b(`rho:rev:bond`), d(`rho:system:deployerId`) in { b!("bond", (*d, 100), "bonded") }"#;
        run(&runtime, &ALICE, &[], 1, bond).await;
        assert!(ok(&runtime, "bonded").await);
        {
            let _genesis = runtime.enter_genesis_phase();
            run(&runtime, &BOB, &[], 2, r#"new a(`rho:rev:bondAdmin`) in { @"pos-held"!(*a) }"#).await;
        }
        let evaluator = r#"for (p <= @"eval") { *p }"#;
        run(&runtime, &BOB, &[], 3, evaluator).await;
        let pk = hex::encode(ALICE);
        let sent = format!(
            r#"for (a <<- @"pos-held") {{
                 @"eval"!({{ a!("slash", "{pk}".hexToBytes(), "slashed") }})
               }}"#
        );
        run(&runtime, &MALLORY, &[], 4, &sent).await;
        assert!(!ok(&runtime, "slashed").await);
        assert_eq!(bond_balance(std::sync::Arc::new(cells(&runtime)), &ALICE).unwrap(), 100);

        let direct = format!(
            r#"for (a <<- @"pos-held") {{ a!("slash", "{pk}".hexToBytes(), "direct") }}"#
        );
        run(&runtime, &MALLORY, &[], 5, &direct).await;
        assert_eq!(bond_balance(std::sync::Arc::new(cells(&runtime)), &ALICE).unwrap(), 0);
    })
    .await;
}

#[tokio::test]
async fn code_with_no_record_draws_on_no_wallet_under_seal_records() {
    with_runtime("seal-wiring-", |runtime: RhoRuntimeImpl| async move {
        stacks(&runtime).deposit(&owner(&ALICE), 1_000).unwrap();
        let payee = address(&MALLORY);
        let installed = format!(
            r#"new w(`rho:rev:wallet`) in {{
                 contract @"pay"(@from, ret) = {{ w!("transfer", (from, "{payee}", 100), *ret) }}
               }}"#
        );
        run(&runtime, &BOB, &[], 1, &installed).await;
        let fire = |out: &str| {
            format!(r#"new d(`rho:system:deployerId`) in {{ @"pay"!(*d, "{out}") }}"#)
        };

        runtime.set_seal_records(true);
        run(&runtime, &ALICE, &[], 2, &fire("recorded")).await;
        assert!(!ok(&runtime, "recorded").await);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 1_000);

        runtime.set_seal_records(false);
        run(&runtime, &ALICE, &[], 3, &fire("legacy")).await;
        assert!(ok(&runtime, "legacy").await);
        assert_eq!(stacks(&runtime).balance(&owner(&ALICE)).unwrap(), 900);
    })
    .await;
}
