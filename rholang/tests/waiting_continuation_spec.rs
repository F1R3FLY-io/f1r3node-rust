use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_receive_issued_twice_under_one_seed_waits_twice_and_takes_both_sends() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let channel = new_gstring_par("ch".to_string(), Vec::new(), false);

    for (term, seed) in [
        (r#"for (_ <- @"ch") { Nil }"#, b"receive".as_slice()),
        (r#"for (_ <- @"ch") { Nil }"#, b"receive".as_slice()),
    ] {
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                HashMap::new(),
                Blake2b512Random::create_from_bytes(seed),
            )
            .await
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }
    assert_eq!(
        runtime.get_continuations(vec![channel.clone()]).await.len(),
        2
    );

    let result = runtime
        .evaluate(
            r#"@"ch"!(1) | @"ch"!(2)"#,
            Cost::unsafe_max(),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(b"sends"),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(runtime
        .get_continuations(vec![channel.clone()])
        .await
        .is_empty());
    assert!(runtime.get_data(&channel).await.is_empty());
}
