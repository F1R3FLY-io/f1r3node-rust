//! LIVE ground-map matching (a real COMM through the tuplespace).
//!
//! A GROUND EPathMap normalizes to trie order (a PathMap zipper walk, NO sort),
//! so a `for`-pattern map and a sent map built in DIFFERENT order become
//! structurally EQUAL and the spatial matcher fires the COMM. This is the
//! order-insensitive matching property that cannot be proven statically — it
//! must run through the real reducer + tuplespace.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::create_from_bytes(&[
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x18, 0x19,
    ])
}

fn gstring_channel(name: &str) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }])
}

/// Evaluate `program` on a fresh runtime, then return the number of data at
/// channel @"out" (1 ⇒ the COMM body ran; 0 ⇒ no match).
async fn out_count(prefix: &str, program: &str) -> usize {
    let program = program.to_string();
    with_runtime(prefix, move |runtime: RhoRuntimeImpl| async move {
        runtime
            .evaluate(&program, Cost::unsafe_max(), HashMap::new(), fixed_rand())
            .await
            .expect("evaluate must not fail structurally");
        runtime.get_data(&gstring_channel("out")).await.len()
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn permuted_ground_maps_comm_fires() {
    // Sent map and receive pattern are the SAME ground map in DIFFERENT order.
    let program = r#"
        @"c"!( {| ["apple"], ["banana"] |} ) |
        for( @{| ["banana"], ["apple"] |} <- @"c" ) { @"out"!("matched") }
    "#;
    assert_eq!(
        out_count("epm-live-match-", program).await,
        1,
        "a permuted ground-map pattern must match the sent map ⇒ COMM fires"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicated_entries_comm_fires() {
    // The sent map has a duplicate entry that dedups to the pattern's multiset.
    let program = r#"
        @"c"!( {| ["apple"], ["banana"], ["apple"] |} ) |
        for( @{| ["banana"], ["apple"] |} <- @"c" ) { @"out"!("matched") }
    "#;
    assert_eq!(
        out_count("epm-live-dup-", program).await,
        1,
        "a duplicated-entry ground map must match its deduped pattern ⇒ COMM fires"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nested_permuted_ground_maps_comm_fires() {
    // The single entry is a nested map built in the opposite inner order.
    let program = r#"
        @"c"!( {| [ {| ["a"], ["b"] |} ] |} ) |
        for( @{| [ {| ["b"], ["a"] |} ] |} <- @"c" ) { @"out"!("matched") }
    "#;
    assert_eq!(
        out_count("epm-live-nested-", program).await,
        1,
        "a permuted NESTED ground-map pattern must match ⇒ COMM fires"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distinct_ground_maps_do_not_match() {
    // Different entry multisets ⇒ no structural match ⇒ no COMM.
    let program = r#"
        @"c"!( {| ["apple"], ["banana"] |} ) |
        for( @{| ["apple"], ["cherry"] |} <- @"c" ) { @"out"!("matched") }
    "#;
    assert_eq!(
        out_count("epm-live-nomatch-", program).await,
        0,
        "distinct ground maps must NOT match ⇒ no COMM"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn var_pattern_binds_the_ground_map() {
    // A non-ground binder pattern (a bare variable) still receives the sent
    // ground map — pre-wire binder matching is unchanged.
    let program = r#"
        @"c"!( {| ["apple"], ["banana"] |} ) |
        for( @m <- @"c" ) { @"out"!("bound") }
    "#;
    assert_eq!(
        out_count("epm-live-binder-", program).await,
        1,
        "a variable pattern still binds the sent ground map ⇒ COMM fires"
    );
}
