use std::time::Instant;

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
        })
        .await;
    eval(&runtime, &vault, SYSTEM_VAULT_V6, 2).await;
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
    with_system_vault(&format!(
        r#"@SystemVault!("transfer", "{to}", {amount}, *deployerId, "{out}")"#
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_facade_transfers_natively_and_reads_balances() {
    let runtime = runtime_with_facade().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();

    eval(&runtime, &alice, &transfer(&bob.address, 250, "out"), 10).await;
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
    eval(&runtime, &alice, &balance, 11).await;
    assert_eq!(
        either(&take(&runtime, "bal").await[0]),
        (true, RhoNumber::create_par(250))
    );

    let find = with_system_vault(&format!(
        r#"@SystemVault!("find", "{}", "found")"#,
        bob.address
    ));
    eval(&runtime, &alice, &find, 12).await;
    let (ok, handle) = either(&take(&runtime, "found").await[0]);
    assert!(ok);
    assert_eq!(handle.bundles.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_facade_refuses_an_overdraft() {
    let runtime = runtime_with_facade().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 10).unwrap();

    eval(&runtime, &alice, &transfer(&bob.address, 11, "out"), 10).await;
    let (ok, _) = either(&take(&runtime, "out").await[0]);
    assert!(!ok);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 10);
}

async fn sequential_transfers(
    runtime: &RhoRuntimeImpl,
    count: usize,
    direct: bool,
    seed: u64,
) -> f64 {
    let payers: Vec<Key> = (0..count).map(|_| key()).collect();
    let payee = key();
    for payer in &payers {
        stacks(runtime).deposit(&payer.owner, 1_000).unwrap();
    }
    let started = Instant::now();
    for (i, payer) in payers.iter().enumerate() {
        let out = format!("out-{seed}-{i}");
        let term = if direct {
            format!(
                r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
                     w!("transfer", (*deployerId, "{}", 1), "{out}")
                   }}"#,
                payee.address
            )
        } else {
            transfer(&payee.address, 1, &out)
        };
        eval(runtime, payer, &term, seed + i as u64).await;
    }
    let elapsed = started.elapsed().as_secs_f64();
    assert_eq!(stacks(runtime).balance(&payee.owner).unwrap(), count as u64);
    elapsed
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "benchmark: run with --release --ignored --nocapture"]
async fn transfer_benchmark() {
    const N: usize = 300;

    let runtime = runtime_with_facade().await;
    sequential_transfers(&runtime, 20, false, 1_000).await;
    let facade = sequential_transfers(&runtime, N, false, 10_000).await;
    let direct = sequential_transfers(&runtime, N, true, 20_000).await;

    let (a, b) = (runtime_with_facade().await, runtime_with_facade().await);
    let started = Instant::now();
    let (ta, tb) = tokio::join!(
        sequential_transfers(&a, N / 2, false, 30_000),
        sequential_transfers(&b, N / 2, false, 40_000)
    );
    let parallel = started.elapsed().as_secs_f64();

    let whale = key();
    stacks(&runtime).deposit(&whale.owner, 1_000_000).unwrap();
    let payees: Vec<Key> = (0..N).map(|_| key()).collect();
    let body = payees
        .iter()
        .map(|p| {
            format!(
                r#"@SystemVault!("transfer", "{}", 1, *deployerId, "batch")"#,
                p.address
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let started = Instant::now();
    eval(&runtime, &whale, &with_system_vault(&body), 99).await;
    let batch = started.elapsed().as_secs_f64();
    assert_eq!(take(&runtime, "batch").await.len(), N);
    assert_eq!(
        stacks(&runtime).balance(&whale.owner).unwrap(),
        1_000_000 - N as u64
    );

    let n = N as f64;
    println!("transfer benchmark ({N} transfers each):");
    println!(
        "  facade, one deploy per transfer:  {:.3} ms per transfer, {:.0}/s",
        facade * 1e3 / n,
        n / facade
    );
    println!(
        "  native, one deploy per transfer:  {:.3} ms per transfer, {:.0}/s",
        direct * 1e3 / n,
        n / direct
    );
    println!(
        "  facade, two runtimes in parallel: {:.0}/s ({:.3} s and {:.3} s per lane)",
        n / parallel,
        ta,
        tb
    );
    println!("  facade, all in one deploy:        {:.0}/s", n / batch);
}
