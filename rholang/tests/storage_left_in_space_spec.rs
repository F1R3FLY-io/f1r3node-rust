use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const STORAGE: [&str; 6] = [
    "consume storage",
    "produces storage",
    "event storage cost",
    "comm event storage cost",
    "consume storage refund",
    "produces storage refund",
];

async fn runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

async fn storage(
    runtime: &mut RhoRuntimeImpl,
    left_in_space: bool,
    term: &str,
) -> (i64, Vec<String>) {
    if left_in_space {
        runtime.cost.charge_storage_left_in_space();
    }
    runtime.cost.clear_log();
    let result = runtime
        .evaluate(
            term,
            Cost::create(10_000_000, "storage spec".to_string()),
            Default::default(),
            Blake2b512Random::create_from_bytes(&[3]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{term}: {:?}", result.errors);
    let charges: Vec<Cost> = runtime
        .cost
        .get_log()
        .into_iter()
        .filter(|c| STORAGE.contains(&c.operation.as_ref()))
        .collect();
    (
        charges.iter().map(|c| c.value).sum(),
        charges.iter().map(|c| c.operation.to_string()).collect(),
    )
}

async fn fresh(left_in_space: bool, term: &str) -> (i64, Vec<String>) {
    storage(&mut runtime().await, left_in_space, term).await
}

fn data_on(name: &str) -> Par { new_gstring_par(name.to_string(), Vec::new(), false) }

#[tokio::test]
async fn a_matched_send_and_receive_pay_no_storage() {
    for term in [
        "new x in { x!(1) | for (_ <- x) { Nil } }",
        "new x in { for (_ <- x) { Nil } | x!(1) }",
        "new x, y in { x!(1) | y!(2) | for (_ <- x & _ <- y) { Nil } }",
    ] {
        let (paid, ops) = fresh(true, term).await;
        assert_eq!(paid, 0, "{term}");
        assert!(fresh(false, term).await.0 > 0, "{term}");
        assert!(
            !ops.iter().any(|op| op == "comm event storage cost"),
            "{term}"
        );
    }
}

#[tokio::test]
async fn an_unmatched_send_or_receive_pays_its_storage_once() {
    for term in ["new x in { x!(1) }", "new x in { for (_ <- x) { Nil } }"] {
        let (paid, _) = fresh(true, term).await;
        let (dev, _) = fresh(false, term).await;
        assert!(paid > 0, "{term}");
        assert_eq!(paid, dev, "{term}");
    }
    let (one, _) = fresh(true, "new x in { x!(1) }").await;
    let (left, _) = fresh(true, "new x in { x!(1) | x!(1) | for (_ <- x) { Nil } }").await;
    assert_eq!(left, one);
}

#[tokio::test]
async fn a_persistent_send_or_receive_pays_once_however_often_it_matches() {
    let (send, _) = fresh(true, "new x in { x!!(1) }").await;
    let (matched, _) = fresh(
        true,
        "new x in { x!!(1) | for (_ <- x) { Nil } | for (_ <- x) { Nil } | for (_ <- x) { Nil } }",
    )
    .await;
    assert!(send > 0);
    assert_eq!(matched, send);

    let (contract, _) = fresh(true, "new x in { contract x(@n) = { Nil } }").await;
    let (called, _) = fresh(
        true,
        "new x in { contract x(@n) = { Nil } | x!(1) | x!(2) | x!(3) }",
    )
    .await;
    assert!(contract > 0);
    assert_eq!(called, contract);
}

#[tokio::test]
async fn a_run_gets_no_refund_for_what_an_earlier_run_left() {
    let mut runtime = runtime().await;
    let (left, _) = storage(&mut runtime, true, r#"@"pre"!(1) | @"pre"!(2)"#).await;
    assert!(left > 0);
    let (taken, ops) = storage(&mut runtime, true, r#"for (_ <- @"pre") { Nil }"#).await;
    assert_eq!(taken, 0);
    assert!(!ops.iter().any(|op| op == "comm event storage cost"));
    assert_eq!(runtime.get_data(&data_on("pre")).await.len(), 1);

    let mut runtime = self::runtime().await;
    storage(&mut runtime, false, r#"@"pre"!(1) | @"pre"!(2)"#).await;
    let (dev, _) = storage(&mut runtime, false, r#"for (_ <- @"pre") { Nil }"#).await;
    assert!(dev != 0);
}

#[tokio::test]
async fn a_peek_at_what_an_earlier_run_left_pays_nothing() {
    let mut runtime = runtime().await;
    storage(&mut runtime, true, r#"@"seen"!(1)"#).await;
    let (peeked, _) = storage(&mut runtime, true, r#"for (_ <<- @"seen") { Nil }"#).await;
    assert_eq!(peeked, 0);
    assert_eq!(runtime.get_data(&data_on("seen")).await.len(), 1);

    let (own, _) = fresh(true, "new x in { x!(1) }").await;
    let (own_peeked, _) = fresh(true, "new x in { x!(1) | for (_ <<- x) { Nil } }").await;
    assert_eq!(own_peeked, own);
}
