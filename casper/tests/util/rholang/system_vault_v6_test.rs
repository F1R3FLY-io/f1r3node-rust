use casper::rust::genesis::contracts::standard_deploys;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::{RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const REGISTRY: &str = include_str!("../../../src/main/resources/Registry.rho");
const SYSTEM_VAULT_V6: &str = include_str!("../../../src/main/resources/SystemVaultV6.rho");

struct Key {
    pk: PublicKey,
    owner: WalletOwner,
    address: String,
}

fn key() -> Key {
    let (_, pk): (PrivateKey, PublicKey) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    Key { pk, owner, address }
}

fn phlo() -> Cost { Cost::create(i64::MAX, "system vault v6".to_string()) }

fn rand(seed: u64) -> Blake2b512Random { Blake2b512Random::create_from_bytes(&seed.to_be_bytes()) }

async fn eval(runtime: &RhoRuntimeImpl, signer: &Key, term: &str, seed: u64) {
    let result = runtime
        .evaluate(term, phlo(), with_deployer_id(&signer.pk), rand(seed))
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

async fn eval_signed(runtime: &RhoRuntimeImpl, signer: &Key, term: &str, seed: u64) {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: signer.pk.clone(),
            deploy_id: seed.to_be_bytes().to_vec(),
            cosigners: Vec::new(),
            phlo_price: 0,
            phlo_limit: 0,
        })
        .await;
    eval(runtime, signer, term, seed).await;
}

fn system_key(private_key_hex: &str) -> Key {
    let sk = PrivateKey::from_bytes(&hex::decode(private_key_hex).unwrap());
    let pk = Secp256k1.to_public(&sk);
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    Key { pk, owner, address }
}

async fn runtime_with_facade() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, true, &mut Vec::new()).await;
    let registry = system_key(standard_deploys::REGISTRY_PK);
    eval(&runtime, &registry, REGISTRY, 1).await;
    let vault = system_key(standard_deploys::SYSTEM_VAULT_PK);
    runtime
        .set_deploy_data(DeployData {
            timestamp: standard_deploys::SYSTEM_VAULT_TIMESTAMP,
            deployer_id: vault.pk.clone(),
            deploy_id: vec![2],
            cosigners: Vec::new(),
            phlo_price: 0,
            phlo_limit: 0,
        })
        .await;
    eval(&runtime, &vault, SYSTEM_VAULT_V6, 2).await;
    runtime.set_seal_records(true);
    runtime
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

fn either(par: &Par) -> (bool, Par) {
    let (flag, value) = RhoTuple2::unapply(par).unwrap();
    (RhoBoolean::unapply(&flag).unwrap(), value)
}

fn with_system_vault(body: &str) -> String {
    format!(
        r#"new rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`), svCh in {{
             rl!(`rho:vault:system`, *svCh) |
             for (@(_, SystemVault) <- svCh) {{ {body} }}
           }}"#
    )
}

fn transfer(to: &str, amount: i64, out: &str) -> String {
    format!(
        r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
             w!("transfer", (*deployerId, "{to}", {amount}), "{out}")
           }}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_native_transfer_shows_in_the_facades_balance_under_seal_records() {
    let runtime = runtime_with_facade().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();

    eval_signed(&runtime, &alice, &transfer(&bob.address, 250, "out"), 10).await;
    assert_eq!(
        either(&take(&runtime, "out").await[0]),
        (true, Par::default())
    );
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 750);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 250);

    let balance = with_system_vault(&format!(
        r#"@SystemVault!("balance", "{}", "bal")"#,
        bob.address
    ));
    eval_signed(&runtime, &alice, &balance, 11).await;
    assert_eq!(
        either(&take(&runtime, "bal").await[0]),
        (true, RhoNumber::create_par(250))
    );

    let find = with_system_vault(&format!(
        r#"@SystemVault!("find", "{}", "found")"#,
        bob.address
    ));
    eval_signed(&runtime, &alice, &find, 12).await;
    let (ok, handle) = either(&take(&runtime, "found").await[0]);
    assert!(ok);
    assert_eq!(handle.bundles.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_native_overdraft_is_refused_under_seal_records() {
    let runtime = runtime_with_facade().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 10).unwrap();

    eval_signed(&runtime, &alice, &transfer(&bob.address, 11, "out"), 10).await;
    let (ok, _) = either(&take(&runtime, "out").await[0]);
    assert!(!ok);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 10);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_facade_has_no_transfer() {
    let runtime = runtime_with_facade().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    let call = with_system_vault(&format!(
        r#"@SystemVault!("transfer", "{}", 100, *deployerId, "out")"#,
        bob.address
    ));
    eval_signed(&runtime, &alice, &call, 10).await;
    assert!(take(&runtime, "out").await.is_empty());
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 1_000);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 0);
}
