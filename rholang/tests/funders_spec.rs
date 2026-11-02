use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::funders::{draw, share, shares};
use rholang::rust::interpreter::accounting::rev_stack::hooks::{install_genesis_hooks, HookAuthority};
use rholang::rust::interpreter::accounting::rev_stack::process::process_owner;
use rholang::rust::interpreter::accounting::rev_stack::{HotStoreCells, RevStacks, WalletOwner};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct Key {
    pk: PublicKey,
    owner: WalletOwner,
    address: String,
}

fn key() -> Key {
    let (_, pk) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    Key { pk, owner, address }
}

async fn runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    create_runtimes(store, false, &mut Vec::new()).await.0
}

fn cells(runtime: &RhoRuntimeImpl) -> HotStoreCells {
    HotStoreCells(runtime.reducer.space.hot_store().unwrap())
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

async fn reply(
    runtime: &RhoRuntimeImpl,
    signer: &Key,
    cosigners: &[&Key],
    term: &str,
    out: &str,
) -> (bool, Par) {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: signer.pk.clone(),
            deploy_id: vec![1],
            cosigners: cosigners.iter().map(|c| c.pk.clone()).collect(),
            phlo_price: 0,
        })
        .await;
    let result = runtime
        .evaluate(
            term,
            Cost::create(i64::MAX, "funders spec".to_string()),
            with_deployer_id(&signer.pk),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let data = runtime
        .get_data(&RhoString::create_par(out.to_string()))
        .await;
    let datum = data.first().expect("a reply");
    let (ok, value) = RhoTuple2::unapply(&datum.a.pars[0]).unwrap();
    (RhoBoolean::unapply(&ok).unwrap(), value)
}

const PROCESS: &str = r#"@"contract"!(1)"#;

fn fund(amount: u64, out: &str) -> String {
    format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{
             w!("fund", (*d, {{ {PROCESS} }}, {amount}), "{out}")
           }}"#
    )
}

fn refuse(funder: &str, out: &str) -> String {
    format!(
        r#"new w(`rho:rev:wallet`) in {{ w!("refuse", ({{ {PROCESS} }}, "{funder}"), "{out}") }}"#
    )
}

fn process() -> WalletOwner { process_owner(&Compiler::source_to_adt(PROCESS).unwrap()) }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deposits_are_recorded_per_funder_and_only_the_authority_refuses_a_share() {
    let runtime = runtime().await;
    let (alice, carol, bob, dave) = (key(), key(), key(), key());
    stacks(&runtime).deposit(&alice.owner, 5_000).unwrap();
    stacks(&runtime).deposit(&carol.owner, 5_000).unwrap();
    install_genesis_hooks(
        &cells(&runtime),
        &HookAuthority::new(vec![bob.owner.clone(), dave.owner.clone()], 2).unwrap(),
    )
    .unwrap();

    assert!(reply(&runtime, &alice, &[], &fund(3_000, "o1"), "o1").await.0);
    assert!(reply(&runtime, &carol, &[], &fund(1_000, "o2"), "o2").await.0);
    assert_eq!(stacks(&runtime).balance(&process()).unwrap(), 4_000);
    assert_eq!(share(&cells(&runtime), &process(), &alice.owner).unwrap(), 3_000);
    assert_eq!(share(&cells(&runtime), &process(), &carol.owner).unwrap(), 1_000);

    let (ok, why) = reply(&runtime, &alice, &[], &refuse(&carol.address, "r6"), "r6").await;
    assert!(!ok);
    assert_eq!(
        RhoString::unapply(&why).as_deref(),
        Some("only the hook authority may refuse a funder's share")
    );
    let (ok, _) = reply(&runtime, &bob, &[], &refuse(&carol.address, "r7"), "r7").await;
    assert!(!ok);

    assert!(reply(&runtime, &bob, &[&dave], &refuse(&carol.address, "r8"), "r8").await.0);
    assert_eq!(stacks(&runtime).balance(&process()).unwrap(), 3_000);
    assert_eq!(stacks(&runtime).balance(&carol.owner).unwrap(), 5_000);
    assert_eq!(share(&cells(&runtime), &process(), &carol.owner).unwrap(), 0);
    assert_eq!(share(&cells(&runtime), &process(), &alice.owner).unwrap(), 3_000);

    let (ok, why) = reply(&runtime, &bob, &[&dave], &refuse(&carol.address, "r9"), "r9").await;
    assert!(!ok);
    assert_eq!(
        RhoString::unapply(&why).as_deref(),
        Some("the funder holds no share of this process")
    );
    let (ok, value) = reply(
        &runtime,
        &alice,
        &[],
        &format!(
            r#"new w(`rho:rev:wallet`) in {{ w!("share", ({{ {PROCESS} }}, "{}"), "s1") }}"#,
            alice.address
        ),
        "s1",
    )
    .await;
    assert!(ok);
    assert_eq!(RhoNumber::unapply(&value), Some(3_000));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_debit_draws_pro_rata_across_funders() {
    let runtime = runtime().await;
    let (alice, carol, erin) = (key(), key(), key());
    for k in [&alice, &carol, &erin] {
        stacks(&runtime).deposit(&k.owner, 5_000).unwrap();
    }
    assert!(reply(&runtime, &alice, &[], &fund(1, "o3"), "o3").await.0);
    assert!(reply(&runtime, &carol, &[], &fund(1, "o4"), "o4").await.0);
    assert!(reply(&runtime, &erin, &[], &fund(1, "o5"), "o5").await.0);
    let drawn = draw(&cells(&runtime), &process(), 2).unwrap();
    assert_eq!(drawn.iter().map(|(_, a)| a).sum::<u64>(), 2);
    let left = shares(&cells(&runtime), &process()).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].1, 1);
}
