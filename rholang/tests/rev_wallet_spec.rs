use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployId, GUnforgeable, Par};
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::{native, RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn phlo() -> Cost { Cost::create(i64::MAX, "rev wallet spec".to_string()) }

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

fn transfer_term(to: &str, amount: i64) -> String {
    format!(
        r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
             w!("transfer", (*deployerId, "{to}", {amount}), "out")
           }}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transfer_moves_rev_from_the_deployer_to_the_payee() {
    let (runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();

    let errors = eval_as(&runtime, &alice, &transfer_term(&bob.address, 300)).await;
    assert!(errors.is_empty(), "{errors:?}");

    let out = take(&runtime, "out").await;
    assert_eq!(out.len(), 1);
    assert_eq!(either(&out[0]), (true, Par::default()));
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 700);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 300);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn balance_reads_the_wallet_by_address() {
    let (runtime, _) = runtimes().await;
    let alice = key();
    stacks(&runtime).deposit(&alice.owner, 40).unwrap();
    stacks(&runtime).deposit(&alice.owner, 2).unwrap();

    let term = format!(
        r#"new w(`rho:rev:wallet`) in {{ w!("balance", "{}", "out") }}"#,
        alice.address
    );
    assert!(eval_as(&runtime, &alice, &term).await.is_empty());
    let out = take(&runtime, "out").await;
    assert_eq!(either(&out[0]), (true, RhoNumber::create_par(42)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_overdraft_is_refused_and_changes_nothing() {
    let (runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 100).unwrap();

    assert!(eval_as(&runtime, &alice, &transfer_term(&bob.address, 101))
        .await
        .is_empty());
    let out = take(&runtime, "out").await;
    assert_eq!(
        either(&out[0]),
        (
            false,
            RhoString::create_par("Insufficient funds".to_string())
        )
    );
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 100);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_deployer_id_can_pay() {
    let (runtime, _) = runtimes().await;
    let (alice, mallory) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 100).unwrap();

    let term = format!(
        r#"new w(`rho:rev:wallet`) in {{ w!("transfer", ("{}", "{}", 50), "out") }}"#,
        alice.address, mallory.address
    );
    assert!(eval_as(&runtime, &mallory, &term).await.is_empty());
    let (ok, _) = either(&take(&runtime, "out").await[0]);
    assert!(!ok);

    assert!(
        eval_as(&runtime, &mallory, &transfer_term(&mallory.address, 50))
            .await
            .is_empty()
    );
    let (ok, _) = either(&take(&runtime, "out").await[0]);
    assert!(!ok, "mallory's own wallet is empty");
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 100);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_amounts_are_refused() {
    let (runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 100).unwrap();
    for amount in [0, -5] {
        assert!(
            eval_as(&runtime, &alice, &transfer_term(&bob.address, amount))
                .await
                .is_empty()
        );
        let out = take(&runtime, "out").await;
        let (ok, _) = either(out.last().unwrap());
        assert!(!ok, "amount {amount} was accepted");
    }
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 100);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn find_returns_a_handle_that_cannot_read_or_write_the_wallet() {
    let (runtime, _) = runtimes().await;
    let alice = key();
    stacks(&runtime).deposit(&alice.owner, 100).unwrap();

    let find = format!(
        r#"new w(`rho:rev:wallet`) in {{ w!("find", "{}", "out") }}"#,
        alice.address
    );
    assert!(eval_as(&runtime, &alice, &find).await.is_empty());
    let (ok, handle) = either(&take(&runtime, "out").await[0]);
    assert!(ok);
    assert_eq!(handle.bundles.len(), 1);
    assert!(!handle.bundles[0].read_flag && !handle.bundles[0].write_flag);

    let steal = r#"for (@(_, h) <- @"out") { for (@cell <- h) { @"stolen"!(cell) } }"#;
    let mint = r#"for (@(_, h) <- @"out") { @h!(("rev", 1000000)) }"#;
    for term in [steal, mint] {
        let errors = eval_as(&runtime, &alice, term).await;
        assert!(!errors.is_empty(), "{term} was not refused");
        assert!(
            eval_as(&runtime, &alice, &find).await.is_empty(),
            "re-publish the handle"
        );
    }
    assert!(take(&runtime, "stolen").await.is_empty());
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 100);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn play_and_replay_reach_the_same_state() {
    let (mut runtime, mut replay) = runtimes().await;
    let (alice, bob, carol) = (key(), key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    stacks(&runtime).deposit(&bob.owner, 500).unwrap();
    let start = runtime.create_checkpoint().await.root;

    let term = format!(
        r#"new w(`rho:rev:wallet`), deployerId(`rho:system:deployerId`) in {{
             w!("transfer", (*deployerId, "{bob}", 10), "out") |
             w!("transfer", (*deployerId, "{carol}", 20), "out") |
             w!("transfer", (*deployerId, "{bob}", 30), "out")
           }}"#,
        bob = bob.address,
        carol = carol.address
    );
    assert!(eval_as(&runtime, &alice, &term).await.is_empty());
    let played = runtime.create_checkpoint().await;

    replay.reset(&start).await.unwrap();
    replay.rig(played.log.clone()).await.unwrap();
    let errors = eval_as(&replay, &alice, &term).await;
    assert!(errors.is_empty(), "{errors:?}");
    replay.check_replay_data().await.unwrap();
    let replayed = replay.create_checkpoint().await;

    assert_eq!(played.root, replayed.root);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 940);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 540);
    assert_eq!(stacks(&runtime).balance(&carol.owner).unwrap(), 20);
    assert_eq!(stacks(&replay).balance(&carol.owner).unwrap(), 20);
}

fn deploy_id(sig: &[u8]) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: sig.to_vec() })),
    }])
}

async fn eval_deploy(runtime: &RhoRuntimeImpl, signer: &Key, sig: &[u8], term: &str) {
    let mut env = with_deployer_id(&signer.pk);
    env.insert("rho:system:deployId".to_string(), deploy_id(sig));
    let result = runtime.evaluate(term, phlo(), env, rand()).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cli_transfer_term_pays_and_reports_on_the_deploy_id() {
    let (runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();

    let term = native::transfer_term(&bob.address, 125).unwrap();
    eval_deploy(&runtime, &alice, b"sig-1", &term).await;

    let reported = runtime.get_data(&deploy_id(b"sig-1")).await;
    assert_eq!(reported.len(), 1);
    assert_eq!(either(&reported[0].a.pars[0]), (true, Par::default()));
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 875);
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 125);

    let query = native::balance_term(&bob.address).unwrap();
    eval_as(&runtime, &alice, &query.replace("*return", "\"bal\"")).await;
    let result = take(&runtime, "bal").await;
    assert_eq!(native::parse_balance(&result[0]), Ok(125));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_vault_demo_scripts_run_on_the_native_path() {
    let (runtime, _) = runtimes().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();

    let transfer =
        include_str!("../examples/vault_demo/3.transfer_funds.rho").replace("%TO", &bob.address);
    assert!(eval_as(&runtime, &alice, &transfer).await.is_empty());
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 100);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 900);

    let check = include_str!("../examples/vault_demo/2.check_balance.rho")
        .replace("%VAULT_ADDR", &bob.address);
    assert!(eval_as(&runtime, &bob, &check).await.is_empty());

    let bad = include_str!("../examples/vault_demo/3.transfer_funds.rho").replace("%TO", "lala");
    assert!(eval_as(&runtime, &alice, &bad).await.is_empty());
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 900);
}
