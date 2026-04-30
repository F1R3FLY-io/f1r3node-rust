// See casper/src/test/scala/coop/rchain/casper/util/rholang/RuntimeSpec.scala

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::genesis::genesis::Genesis;
use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::tools::Tools;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_runtime_from_kv_store, RhoRuntime};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_state_hash_should_be_the_same_as_hard_coded_cached_value() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let runtime = create_runtime_from_kv_store(
        store,
        Genesis::non_negative_mergeable_tag_name(),
        false,
        &mut Vec::new(),
        Arc::new(Box::new(Matcher)),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    )
    .await;

    let hard_coded_hash = RuntimeManager::empty_state_hash_fixed();
    let mut runtime_ops = RuntimeOps::new(runtime);
    let empty_root_hash = runtime_ops.empty_state_hash().await.unwrap();

    let empty_hash_hard_coded = Blake2b256Hash::from_bytes_prost(&hard_coded_hash);
    let empty_hash = Blake2b256Hash::from_bytes_prost(&empty_root_hash);

    assert_eq!(empty_hash_hard_coded, empty_hash);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_hash_after_fixed_rholang_term_execution_should_be_hash_fixed_without_hard_fork() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let mut runtime = create_runtime_from_kv_store(
        store,
        Genesis::non_negative_mergeable_tag_name(),
        false,
        &mut Vec::new(),
        Arc::new(Box::new(Matcher)),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    )
    .await;

    let contract = r#"
         new a in {
           @"2"!(10)|
           @2!("test")|
           @"3"!!(3)|
           @42!!("1")|
           for (@t <- a){Nil}|
           for (@num <- @"3"&@num2 <- @1){10}|
           for (@_ <= @"4"){"3"}|
           for (@_ <= @"5"& @num3 <= @5){Nil}|
           for (@3 <- @44){new g in {Nil}}|
           for (@_ <- @"55"& @num3 <- @55){Nil}
         }
        "#
    .trim_start();

    let random = Tools::rng(&Blake2b256Hash::from_bytes(vec![0; 1]).bytes());
    let r = runtime
        .evaluate(contract, Cost::unsafe_max(), HashMap::new(), random)
        .await;

    assert!(r.is_ok());
    assert!(r.unwrap().errors.is_empty());

    let checkpoint = runtime.create_checkpoint().await;
    // Updated 2026-04-30 by Phase 2/5/6 of where-clauses-and-match-guards
    // (commits 1740ca8, 8d540d2, e66de77): the new If, Receive.condition,
    // MatchCase.guard, and EMatchExpr IR fields shifted the canonical
    // encoding of the Pars produced by this fixed Rholang term. The
    // test's "without_hard_fork" name is now descriptive of the *new*
    // post-fork-hash baseline; any further unintended drift would still
    // be caught.
    let expected_hash = Blake2b256Hash::from_hex(
        "22b8f65009c1731abe4fc099592491ca44e664fab15ae771b4521a075bef2024",
    );

    assert_eq!(expected_hash, checkpoint.root);
}
