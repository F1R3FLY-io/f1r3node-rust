use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::GSysAuthToken;
use models::rhoapi::Par;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::bond::{bond_balance, bond_owner, handle};
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::{HotStoreCells, RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{
    RhoBoolean, RhoByteArray, RhoNumber, RhoString, RhoSysAuthToken, RhoTuple2,
};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn phlo() -> Cost { Cost::create(i64::MAX, "rev bond spec".to_string()) }

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[]) }

struct Key {
    pk: PublicKey,
    owner: WalletOwner,
}

fn key() -> Key {
    let (_, pk) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    Key { pk, owner }
}

async fn runtimes() -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    (runtime, replay)
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
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

fn either(par: &Par) -> (bool, Par) {
    let (flag, value) = RhoTuple2::unapply(par).expect("an (ok, value) pair");
    (RhoBoolean::unapply(&flag).expect("a boolean"), value)
}

fn cells(runtime: &RhoRuntimeImpl) -> std::sync::Arc<HotStoreCells> {
    std::sync::Arc::new(HotStoreCells(runtime.reducer.space.hot_store().unwrap()))
}

fn system_token() -> Par { RhoSysAuthToken::create_par(GSysAuthToken::default()) }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deployer_bonds_its_own_rev_natively() {
    let (runtime, _) = runtimes().await;
    let alice = key();
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    let term = r#"new b(`rho:rev:bond`), deployerId(`rho:system:deployerId`) in {
                    b!("bond", (*deployerId, 300), "out")
                  }"#;
    assert!(eval_as(&runtime, &alice, term).await.is_empty());
    assert_eq!(
        either(&take(&runtime, "out").await[0]),
        (true, Par::default())
    );
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 700);
    assert_eq!(bond_balance(cells(&runtime), &alice.pk.bytes).unwrap(), 300);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_system_can_release_or_slash_a_bond() {
    let (runtime, _) = runtimes().await;
    let (alice, mallory) = (key(), key());
    stacks(&runtime)
        .deposit(&bond_owner(&alice.pk.bytes), 500)
        .unwrap();
    let store = runtime.reducer.space.hot_store();
    let pk = RhoByteArray::create_par(alice.pk.bytes.to_vec());
    let tuple = |ps: Vec<Par>| {
        Par::default().with_exprs(vec![models::rhoapi::Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::ETupleBody(
                models::rhoapi::ETuple {
                    ps,
                    locally_free: Vec::new(),
                    connective_used: false,
                },
            )),
        }])
    };

    let term = format!(
        r#"new b(`rho:rev:bond`) in {{ b!("slash", ("not a token", "{}".hexToBytes()), "out") }}"#,
        hex::encode(&alice.pk.bytes)
    );
    assert!(eval_as(&runtime, &mallory, &term).await.is_empty());
    let (ok, _) = either(&take(&runtime, "out").await[0]);
    assert!(!ok);
    assert_eq!(bond_balance(cells(&runtime), &alice.pk.bytes).unwrap(), 500);

    let forged = handle(
        store.clone(),
        "release",
        &tuple(vec![
            RhoString::create_par("x".into()),
            pk.clone(),
            RhoNumber::create_par(5),
        ]),
    )
    .unwrap();
    assert!(!either(&forged).0);

    let released = handle(
        store.clone(),
        "release",
        &tuple(vec![system_token(), pk.clone(), RhoNumber::create_par(200)]),
    )
    .unwrap();
    assert!(either(&released).0);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 200);

    let slashed = handle(
        store.clone(),
        "slash",
        &tuple(vec![system_token(), pk.clone()]),
    )
    .unwrap();
    assert_eq!(either(&slashed), (true, RhoNumber::create_par(300)));
    assert_eq!(bond_balance(cells(&runtime), &alice.pk.bytes).unwrap(), 0);
    assert_eq!(
        stacks(&runtime).balance(&compute_wallet_owner()).unwrap(),
        300
    );
    let again = handle(store, "slash", &tuple(vec![system_token(), pk])).unwrap();
    assert_eq!(either(&again), (true, RhoNumber::create_par(0)));
}
