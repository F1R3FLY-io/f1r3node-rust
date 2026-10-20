use std::collections::BTreeMap;
use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn contracts() -> Vec<(String, i64)> {
    vec![
      (String::from("@0!(2)"), 97),
      (String::from("@0!(2) | @1!(1)"), 197),
      (String::from("for(x <- @0){ Nil }"), 128),
      (String::from("for(x <- @0){ Nil } | @0!(2)"), 329),
      (String::from("@0!!(0) | for (_ <- @0) { 0 }"), 342),
      (String::from("@0!!(0) | for (x <- @0) { 0 }"), 342),
      (String::from("@0!!(0) | for (@0 <- @0) { 0 }"), 336),
      (String::from("@0!!(0) | @0!!(0) | for (_ <- @0) { 0 }"), 443),
      (String::from("@0!!(0) | @1!!(1) | for (_ <- @0 & _ <- @1) { 0 }"), 596),
      (String::from("@0!(0) | for (_ <- @0) { 0 }"), 333),
      (String::from("@0!(0) | for (x <- @0) { 0 }"), 333),
      (String::from("@0!(0) | for (@0 <- @0) { 0 }"), 327),
      (String::from("@0!(0) | for (_ <= @0) { 0 }"), 354),
      (String::from("@0!(0) | for (x <= @0) { 0 }"), 356),
      (String::from("@0!(0) | for (@0 <= @0) { 0 }"), 341),
      (String::from("@0!(0) | @0!(0) | for (_ <= @0) { 0 }"), 574),
      (String::from("@0!(0) | for (@0 <- @0) { 0 } | @0!(0) | for (_ <- @0) { 0 }"), 663),
      (String::from("@0!(0) | for (@0 <- @0) { 0 } | @0!(0) | for (@1 <- @0) { 0 }"), 551),
      (String::from("@0!(0) | for (_ <<- @0) { 0 }"), 406),
      (String::from("@0!!(0) | for (_ <<- @0) { 0 }"), 343),
      (String::from("@0!!(0) | @0!!(0) | for (_ <<- @0) { 0 }"), 444),
      (String::from("new loop in {\n  contract loop(@n) = {\n    match n {\n      0 => Nil\n      _ => loop!(n-1)\n    }\n  } |\n  loop!(10)\n}"), 3846),
      (String::from("42 | @0!(2) | for (x <- @0) { Nil }"), 336),
      (String::from("@1!(1) |\n        for(x <- @1) { Nil } |\n        new x in { x!(10) | for(X <- x) { @2!(Set(X!(7)).add(*X).contains(10)) }} |\n        match 42 {\n          38 => Nil\n          42 =>\n@3!(42)\n        }\n     "), 1264),
      (String::from("new ret, keccak256Hash(`rho:crypto:keccak256Hash`) in {\n  keccak256Hash!(\"TEST\".toByteArray(), *ret) |\n  for (_ <- ret) { Nil }\n}"), 782),
    ]
}

async fn cost_and_log(contract: &str) -> (i64, Vec<Cost>, bool) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (space, _) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            store,
            Arc::new(Box::new(Matcher)),
        )
        .unwrap();
    let mut runtime = create_rho_runtime(
        space,
        Arc::new(std::collections::HashMap::new()),
        false,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await
    .unwrap();
    let _legacy_scope = runtime.cost.enter_legacy_scope();
    let parsing = contract.len() as i64;
    let result = runtime
        .evaluate_with_phlo(
            contract,
            Cost::create(10_000 - parsing, "legacy cost parity".to_string()),
        )
        .await
        .unwrap();
    (
        result.cost.value + parsing,
        runtime.get_cost_log(),
        result.errors.is_empty(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn legacy_contracts_cost_what_they_cost_on_dev() {
    let mut mismatches = Vec::new();
    for (contract, expected) in contracts() {
        let (cost, log, ok) = cost_and_log(&contract).await;
        assert!(ok, "contract errored: {contract}");
        if cost != expected {
            let mut by_operation: BTreeMap<String, (usize, i64)> = BTreeMap::new();
            for entry in &log {
                let slot = by_operation.entry(entry.operation.to_string()).or_default();
                slot.0 += 1;
                slot.1 += entry.value;
            }
            mismatches.push(format!(
                "{:?}: dev {expected}, now {cost} ({:+}); log {:?}",
                contract.replace('\n', " "),
                cost - expected,
                by_operation
            ));
        }
    }
    assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
}
