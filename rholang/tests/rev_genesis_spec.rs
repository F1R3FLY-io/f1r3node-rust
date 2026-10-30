use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::genesis::genesis_supply;
use rholang::rust::interpreter::accounting::rev_stack::{HotStoreCells, RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn phlo() -> Cost { Cost::create(i64::MAX, "rev genesis spec".to_string()) }

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[]) }

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
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

fn supply(runtime: &RhoRuntimeImpl) -> u64 {
    genesis_supply(&HotStoreCells(runtime.reducer.space.hot_store().unwrap())).unwrap()
}

async fn eval_as(runtime: &RhoRuntimeImpl, signer: &Key, term: &str) -> Vec<String> {
    let result = runtime
        .evaluate(term, phlo(), with_deployer_id(&signer.pk), rand())
        .await
        .unwrap();
    result.errors.iter().map(|e| e.to_string()).collect()
}

async fn take(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
    runtime
        .get_data(&RhoString::create_par(channel.to_string()))
        .await
        .into_iter()
        .flat_map(|d| d.a.pars)
        .collect()
}

fn mint_term(lines: &[(&str, i64)]) -> String {
    let list = lines
        .iter()
        .map(|(a, n)| format!("(\"{a}\", {n})"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(r#"new g(`rho:rev:genesis`) in {{ g!("mint", [{list}], "out") }}"#)
}

#[tokio::test]
async fn a_user_deploy_cannot_bind_the_genesis_mint() {
    let runtime = runtime().await;
    let (alice, bob) = (key(), key());
    let errors = eval_as(&runtime, &alice, &mint_term(&[(&bob.address, 1_000)])).await;
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("reserved for genesis"), "{errors:?}");
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 0);
    assert_eq!(supply(&runtime), 0);
}

#[tokio::test]
async fn genesis_mints_one_stack_per_wallet_and_records_the_supply() {
    let runtime = runtime().await;
    runtime.disable_fs_native_urn_filter();
    let (genesis, alice, bob) = (key(), key(), key());
    let term = mint_term(&[
        (&alice.address, 700),
        (&bob.address, 200),
        (&alice.address, 100),
    ]);
    assert!(eval_as(&runtime, &genesis, &term).await.is_empty());
    let out = take(&runtime, "out").await;
    let (ok, value) = RhoTuple2::unapply(&out[0]).unwrap();
    assert_eq!(RhoBoolean::unapply(&ok), Some(true));
    assert_eq!(RhoNumber::unapply(&value), Some(1_000));

    let stacks = stacks(&runtime);
    assert_eq!(stacks.balance(&alice.owner).unwrap(), 800);
    assert_eq!(stacks.balance(&bob.owner).unwrap(), 200);
    let hot = runtime.reducer.space.hot_store().unwrap();
    for owner in [&alice.owner, &bob.owner] {
        let channel = rholang::rust::interpreter::accounting::rev_stack::wallet_channel(owner);
        assert_eq!(hot.get_data(&channel).len(), 1);
    }
    assert_eq!(supply(&runtime), 1_000);

    assert!(
        eval_as(&runtime, &genesis, &mint_term(&[(&bob.address, 50)]))
            .await
            .is_empty()
    );
    assert_eq!(supply(&runtime), 1_050);
    assert_eq!(stacks.balance(&bob.owner).unwrap(), 250);
}

#[tokio::test]
async fn genesis_installs_the_hook_authority_and_a_noop_rev_hook() {
    use rholang::rust::interpreter::accounting::rev_stack::hooks::{
        hook_authority, rev_hook, RevHook,
    };
    let runtime = runtime().await;
    let (genesis, alice, bob) = (key(), key(), key());
    let term = format!(
        r#"new g(`rho:rev:genesis`) in {{ g!("hooks", (2, ["{}", "{}"]), "out") }}"#,
        alice.address, bob.address
    );
    let refused = eval_as(&runtime, &genesis, &term).await;
    assert!(refused[0].contains("reserved for genesis"), "{refused:?}");
    runtime.disable_fs_native_urn_filter();
    assert!(eval_as(&runtime, &genesis, &term).await.is_empty());
    let cells = HotStoreCells(runtime.reducer.space.hot_store().unwrap());
    let authority = hook_authority(&cells).unwrap().unwrap();
    assert_eq!(authority.threshold(), 2);
    assert_eq!(authority.members().len(), 2);
    assert!(authority.authorizes(&[alice.owner.clone(), bob.owner.clone()]));
    assert_eq!(rev_hook(&cells).unwrap(), Some(RevHook::Noop));
}

#[tokio::test]
async fn a_malformed_line_mints_nothing() {
    let runtime = runtime().await;
    runtime.disable_fs_native_urn_filter();
    let (genesis, alice) = (key(), key());
    let term = mint_term(&[(&alice.address, 10), ("not-an-address", 5)]);
    assert!(eval_as(&runtime, &genesis, &term).await.is_empty());
    let out = take(&runtime, "out").await;
    let (ok, _) = RhoTuple2::unapply(&out[0]).unwrap();
    assert_eq!(RhoBoolean::unapply(&ok), Some(false));
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 0);
    assert_eq!(supply(&runtime), 0);
}

#[tokio::test]
async fn genesis_records_the_bond_total_once() {
    use rholang::rust::interpreter::accounting::rev_stack::genesis::legacy_bonds;
    let runtime = runtime().await;
    runtime.disable_fs_native_urn_filter();
    let (genesis, alice) = (key(), key());
    assert!(
        eval_as(&runtime, &genesis, &mint_term(&[(&alice.address, 100)]))
            .await
            .is_empty()
    );
    let bonds = r#"new g(`rho:rev:genesis`) in { g!("bonds", 40, "out") }"#;
    assert!(eval_as(&runtime, &genesis, bonds).await.is_empty());
    let cells = HotStoreCells(runtime.reducer.space.hot_store().unwrap());
    assert_eq!(supply(&runtime), 140);
    assert_eq!(legacy_bonds(&cells).unwrap(), 40);
    assert!(eval_as(&runtime, &genesis, bonds).await.is_empty());
    assert_eq!(supply(&runtime), 140);
}
