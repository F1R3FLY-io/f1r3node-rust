use std::num::NonZeroU64;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::comm_ledger::CommTally;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::Event;

async fn runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

async fn run(runtime: &mut RhoRuntimeImpl, term: &str) -> (CommTally, u64) {
    runtime.cost.comm_ledger().reset();
    let _ = runtime.take_event_log().await;
    let result = runtime
        .evaluate(
            term,
            Cost::create(1_000_000, "comm ledger spec".to_string()),
            Default::default(),
            Blake2b512Random::create_from_bytes(&[7]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let comms = runtime
        .take_event_log()
        .await
        .iter()
        .filter(|e| matches!(e, Event::Comm(_)))
        .count() as u64;
    (runtime.cost.comm_ledger().take(), comms)
}

#[tokio::test]
async fn the_reducer_hook_sees_every_comm_in_the_event_log() {
    let mut runtime = runtime().await;
    for term in [
        "Nil",
        "new x in { x!(1) }",
        "new x in { x!(1) | for (_ <- x) { Nil } | x!(2) | for (_ <- x) { Nil } }",
        "new x, y in { x!(1) | y!(2) | for (_ <- x & _ <- y) { Nil } }",
        "new x in { contract x(@n) = { Nil } | x!(1) | x!(2) | x!(3) }",
        "new x, r in { x!(5) | for (@v <<- x) { r!(v) } | for (_ <- r) { Nil } }",
    ] {
        let (tally, logged) = run(&mut runtime, term).await;
        assert_eq!(tally.comms, logged, "{term}");
        assert_eq!(tally.batches.iter().sum::<u64>(), tally.comms, "{term}");
    }
}

#[tokio::test]
async fn box_car_batches_partition_the_comms() {
    let mut runtime = runtime().await;
    let term = "new x in { contract x(@n) = { Nil } | x!(1) | x!(2) | x!(3) | x!(4) | x!(5) }";
    let (unbounded, logged) = run(&mut runtime, term).await;
    assert_eq!(unbounded.comms, 5);
    assert_eq!(logged, 5);
    assert_eq!(unbounded.batches, vec![5]);
    runtime.cost.comm_ledger().set_boxcar(NonZeroU64::new(2));
    let (boxed, _) = run(&mut runtime, term).await;
    assert_eq!(boxed.comms, 5);
    assert_eq!(boxed.batches, vec![2, 2, 1]);
}
