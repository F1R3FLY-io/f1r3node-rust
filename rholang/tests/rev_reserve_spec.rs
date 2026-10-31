use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::reserve::{
    clear_reservation, reservation, set_reservation, Reservation,
};
use rholang::rust::interpreter::accounting::rev_stack::{HotStoreCells, RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::DeployData;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn phlo() -> Cost { Cost::create(i64::MAX, "rev reserve spec".to_string()) }

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

async fn runtimes() -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    (runtime, replay)
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

async fn sign_as(runtime: &RhoRuntimeImpl, signer: &Key) {
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: signer.pk.clone(),
            deploy_id: vec![1],
            cosigners: Vec::new(),
        })
        .await;
}

async fn eval_as(runtime: &RhoRuntimeImpl, signer: &Key, term: &str) -> Vec<String> {
    sign_as(runtime, signer).await;
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

fn transfer_term(to: &str, amount: i64, out: &str) -> String {
    format!(
        r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
             w!("transfer", (*deployerId, "{to}", {amount}), "{out}")
           }}"#
    )
}

fn balance_term(address: &str, out: &str) -> String {
    format!(r#"new w(`rho:rev:wallet`) in {{ w!("balance", "{address}", "{out}") }}"#)
}

fn cells(runtime: &RhoRuntimeImpl) -> HotStoreCells {
    HotStoreCells(runtime.reducer.space.hot_store().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_sees_its_visible_balance_and_cannot_spend_held_rev() {
    let (runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    set_reservation(
        &cells(&runtime),
        &alice.owner,
        Reservation {
            visible: 300,
            held: 500,
        },
    )
    .unwrap();

    assert!(
        eval_as(&runtime, &alice, &balance_term(&alice.address, "b"))
            .await
            .is_empty()
    );
    assert_eq!(
        either(&take(&runtime, "b").await[0]),
        (true, RhoNumber::create_par(700))
    );

    assert!(
        eval_as(&runtime, &alice, &transfer_term(&bob.address, 501, "t1"))
            .await
            .is_empty()
    );
    let (ok, _) = either(&take(&runtime, "t1").await[0]);
    assert!(!ok);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 1_000);

    assert!(
        eval_as(&runtime, &alice, &transfer_term(&bob.address, 500, "t2"))
            .await
            .is_empty()
    );
    assert_eq!(
        either(&take(&runtime, "t2").await[0]),
        (true, Par::default())
    );
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 500);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 500);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cleared_reservation_leaves_no_trace_in_the_state() {
    let (mut runtime, _) = runtimes().await;
    let alice = key();
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    let before = runtime.create_checkpoint().await.root;
    set_reservation(
        &cells(&runtime),
        &alice.owner,
        Reservation {
            visible: 10,
            held: 20,
        },
    )
    .unwrap();
    assert_eq!(
        reservation(&cells(&runtime), &alice.owner).unwrap().held,
        20
    );
    clear_reservation(&cells(&runtime), &alice.owner).unwrap();
    assert_eq!(
        reservation(&cells(&runtime), &alice.owner).unwrap(),
        Reservation::default()
    );
    assert_eq!(runtime.create_checkpoint().await.root, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_journal_holds_transfers_out_of_the_state() {
    use rholang::rust::interpreter::accounting::rev_stack::journal::{
        close_journal, journal_open, open_journal,
    };
    let (mut runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    let before = runtime.create_checkpoint().await.root;
    open_journal(&cells(&runtime)).unwrap();
    assert!(journal_open(&cells(&runtime)));

    assert!(eval_as(&runtime, &alice, &transfer_term(&bob.address, 600, "j1")).await.is_empty());
    assert_eq!(either(&take(&runtime, "j1").await[0]), (true, Par::default()));
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 1_000);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 0);
    assert!(eval_as(&runtime, &alice, &balance_term(&alice.address, "jb")).await.is_empty());
    assert_eq!(either(&take(&runtime, "jb").await[0]), (true, RhoNumber::create_par(400)));
    assert!(eval_as(&runtime, &alice, &transfer_term(&bob.address, 401, "j2")).await.is_empty());
    assert!(!either(&take(&runtime, "j2").await[0]).0);

    sign_as(&runtime, &bob).await;
    let forged = runtime
        .evaluate(
            &transfer_term(&bob.address, 100, "j3"),
            phlo(),
            with_deployer_id(&alice.pk),
            rand(),
        )
        .await
        .unwrap();
    assert!(forged.errors.is_empty());
    let (ok, why) = either(&take(&runtime, "j3").await[0]);
    assert!(!ok);
    assert_eq!(
        RhoString::unapply(&why).as_deref(),
        Some("the payer must be a signer of this deployment")
    );

    let journal = close_journal(&cells(&runtime)).unwrap();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].from, alice.owner);
    assert_eq!(journal[0].to, bob.owner);
    assert_eq!(journal[0].amount, 600);
    assert!(!journal_open(&cells(&runtime)));
    let _ = before;
}
