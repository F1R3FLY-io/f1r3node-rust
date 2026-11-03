use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::stops::{Refusal, ReplayStops};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

const TERM: &str = r#"@"a"!(1) | @"b"!(2) | for (_ <- @"c") { Nil }"#;

async fn runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

fn name(n: &str) -> Par { new_gstring_par(n.to_string(), Vec::new(), false) }

async fn run(runtime: &mut RhoRuntimeImpl) {
    let result = runtime
        .evaluate(
            TERM,
            Cost::create(10_000_000, "replay stops spec".to_string()),
            Default::default(),
            Blake2b512Random::create_from_bytes(&[5]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

fn produce_hashes(log: &[Event]) -> Vec<Vec<u8>> {
    log.iter()
        .filter_map(|e| match e {
            Event::IoEvent(IOEvent::Produce(p)) => Some(p.hash.bytes()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn under_a_stopped_membrane_replay_stores_only_the_logged_io() {
    let mut runtime = runtime().await;
    let start = runtime.create_checkpoint().await.root;
    run(&mut runtime).await;
    assert_eq!(runtime.get_data(&name("a")).await.len(), 1);
    assert_eq!(runtime.get_data(&name("b")).await.len(), 1);
    let log = runtime.take_event_log().await;
    let produces = produce_hashes(&log);
    assert_eq!(produces.len(), 2);
    let a_hash = runtime.get_data(&name("a")).await[0].source.hash.bytes();
    assert!(produces.contains(&a_hash));

    runtime.reset(&start).await.unwrap();
    let refusal = Refusal {
        membrane: None,
        step: vec![0xff; 32],
        drawn: 0,
        cost: 0,
    };
    let stops = ReplayStops::new(&[refusal], vec![a_hash]);
    runtime.cost.set_replay_stops(Some(stops.clone()));
    run(&mut runtime).await;
    runtime.cost.set_replay_stops(None);

    assert_eq!(runtime.get_data(&name("a")).await.len(), 1);
    assert!(runtime.get_data(&name("b")).await.is_empty());
    assert!(runtime.get_continuations(vec![name("c")]).await.is_empty());
    assert_eq!(stops.refused(), 2);
}

#[tokio::test]
async fn without_a_stop_replay_stores_everything() {
    let mut runtime = runtime().await;
    let stops = ReplayStops::new(&[], Vec::<Vec<u8>>::new());
    runtime.cost.set_replay_stops(Some(stops.clone()));
    run(&mut runtime).await;
    runtime.cost.set_replay_stops(None);
    assert_eq!(runtime.get_data(&name("b")).await.len(), 1);
    assert_eq!(runtime.get_continuations(vec![name("c")]).await.len(), 1);
    assert_eq!(stops.refused(), 0);
}
