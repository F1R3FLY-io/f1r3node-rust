use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::{create_runtimes, replay_charged};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const PLAYS: u8 = 30;

/// Plays `term` within `limit`, then replays play's log as casper does:
/// evaluate, recompute the searches' charge from the log, add it.
async fn play_and_replay(
    term: &str,
    limit: i64,
    seed: u8,
) -> (EvaluateResult, EvaluateResult, bool) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut play_rt, mut replay_rt, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let rand = Blake2b512Random::create_from_bytes(&[seed]);

    let played = play_rt
        .evaluate(
            term,
            Cost::create(limit, "play"),
            HashMap::new(),
            rand.clone(),
        )
        .await
        .unwrap();
    let log = play_rt.take_event_log().await;
    let charged_search = play_rt
        .get_cost_log()
        .iter()
        .any(|cost| cost.operation == "search" && cost.value > 0);

    let replayed = replay_charged(
        &mut replay_rt,
        log,
        term,
        Cost::create(limit, "replay"),
        HashMap::new(),
        rand,
    )
    .await
    .unwrap_or_else(|error| panic!("replaying {term} failed: {error:?}"));
    (played, replayed, charged_search)
}

fn is_oop(result: &EvaluateResult) -> bool {
    result
        .errors
        .iter()
        .any(|e| matches!(e, InterpreterError::OutOfPhlogistonsError))
}

fn shapes() -> Vec<(&'static str, String)> {
    let junk = format!(
        "new c in {{ {} | {} | c!(-1) | for (@-1 <- c) {{ Nil }} }}",
        (0..60)
            .map(|i| format!("c!({i})"))
            .collect::<Vec<_>>()
            .join(" | "),
        (0..10)
            .map(|_| "for (@-2 <- c) { Nil }")
            .collect::<Vec<_>>()
            .join(" | ")
    );
    vec![
        (
            "contention",
            "new c in { c!(1) | c!(2) | c!(3) | for (@x <- c) { Nil } | for (@2 <- c) { Nil } }"
                .to_string(),
        ),
        (
            "persistent send",
            "new c in { c!!(1) | for (@x <- c) { Nil } | for (@y <- c) { Nil } | for (@z <- c) { Nil } }"
                .to_string(),
        ),
        (
            "contract",
            "new c in { contract c(@n) = { Nil } | c!(1) | c!(2) | c!(3) | c!(4) | c!(5) }"
                .to_string(),
        ),
        (
            "peek",
            "new c in { c!(1) | c!(2) | for (@x <<- c) { Nil } | for (@y <<- c) { Nil } | for (@z <- c) { Nil } }"
                .to_string(),
        ),
        (
            "join",
            "new a, b in { a!(1) | b!(2) | a!(3) | b!(4) | for (@x <- a & @y <- b) { Nil } | for (@3 <- a & @4 <- b) { Nil } }"
                .to_string(),
        ),
        (
            "persistent join",
            "new a, b in { for (@x <= a & @y <- b) { Nil } | a!(1) | b!(1) | b!(2) | b!(3) | a!(2) }"
                .to_string(),
        ),
        (
            "peek with persistent send",
            "new c, d in { c!!(1) | d!(0) | for (@x <<- c & @y <- d) { Nil } | for (@z <- c) { Nil } }"
                .to_string(),
        ),
        ("junk", junk),
        (
            "chain",
            "new a, b, c in { a!(1) | for (@x <- a) { b!(x) | b!(x + 1) } | for (@y <- b) { c!(y) } | for (@z <- b) { c!(z) } | for (@_ <- c) { Nil } | for (@_ <- c) { Nil } }"
                .to_string(),
        ),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_charges_every_search_what_play_charged() {
    for (name, term) in shapes() {
        let mut charged_something = false;
        for seed in 0..PLAYS {
            let (played, replayed, charged_search) = play_and_replay(&term, 10_000_000, seed).await;
            assert!(
                played.errors.is_empty(),
                "{name}: play failed: {:?}",
                played.errors
            );
            assert!(
                replayed.errors.is_empty(),
                "{name} seed {seed}: replay failed: {:?}",
                replayed.errors
            );
            assert_eq!(
                replayed.cost.value, played.cost.value,
                "{name} seed {seed}: replay charged {}, play {}",
                replayed.cost.value, played.cost.value
            );
            charged_something |= charged_search;
        }
        assert!(
            charged_something,
            "{name}: no search examined a candidate in {PLAYS} plays"
        );
    }
}

/// Plays and replays `setup`, then `term` against what `setup` left waiting,
/// returning the play and replay of `term`.
async fn play_and_replay_after(
    setup: &str,
    term: &str,
    seed: u8,
) -> (EvaluateResult, EvaluateResult) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut play_rt, mut replay_rt, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut results = Vec::new();
    for (step, source) in [setup, term].into_iter().enumerate() {
        let rand = Blake2b512Random::create_from_bytes(&[seed, step as u8]);
        let played = play_rt
            .evaluate(
                source,
                Cost::create(10_000_000, "play"),
                HashMap::new(),
                rand.clone(),
            )
            .await
            .unwrap();
        let log = play_rt.take_event_log().await;
        let replayed = replay_charged(
            &mut replay_rt,
            log,
            source,
            Cost::create(10_000_000, "replay"),
            HashMap::new(),
            rand,
        )
        .await
        .unwrap_or_else(|error| panic!("replaying {source} failed: {error:?}"));
        replay_rt.check_replay_data().await.unwrap();
        replay_rt.take_event_log().await;
        results.push((played, replayed));
    }
    results.pop().expect("the term was played")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_charges_searches_over_what_earlier_terms_left_waiting() {
    let cases = [
        (
            "joins on a channel the term never touches",
            r#"for (_ <- @"a" & _ <- @"b") { Nil } | for (_ <- @"b" & _ <- @"a") { Nil } | @"b"!(0) | @"b"!(1)"#,
            r#"@"a"!(1) | @"a"!(2) | @"a"!(3)"#,
        ),
        (
            "peeks",
            r#"for (_ <<- @"r") { Nil } | for (_ <<- @"r") { Nil } | for (_ <- @"r" & _ <- @"s") { Nil }"#,
            r#"@"r"!(1) | for (_ <- @"r") { Nil } | @"s"!(1)"#,
        ),
        (
            "data",
            r#"@"c"!(1) | @"c"!(2) | @"c"!!(3)"#,
            r#"for (@2 <- @"c") { Nil } | for (_ <- @"c") { Nil } | for (@3 <- @"c") { Nil } | for (@4 <= @"c") { Nil }"#,
        ),
    ];
    for (name, setup, term) in cases {
        for seed in 0..PLAYS {
            let (played, replayed) = play_and_replay_after(setup, term, seed).await;
            assert!(
                played.errors.is_empty(),
                "{name}: play failed: {:?}",
                played.errors
            );
            assert!(
                replayed.errors.is_empty(),
                "{name} seed {seed}: replay failed: {:?}",
                replayed.errors
            );
            assert_eq!(
                replayed.cost.value, played.cost.value,
                "{name} seed {seed}: replay charged {}, play {}",
                replayed.cost.value, played.cost.value
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_play_the_searches_run_out_of_phlo_replays_out_of_phlo() {
    for (name, term) in shapes() {
        let (full, _, _) = play_and_replay(&term, 10_000_000, 0).await;
        assert!(full.errors.is_empty(), "{name}: {:?}", full.errors);
        for shortfall in [1, 7, 30, 120] {
            let limit = full.cost.value - shortfall;
            for seed in 0..5u8 {
                let (played, replayed, _) = play_and_replay(&term, limit, seed).await;
                assert_eq!(
                    is_oop(&replayed),
                    is_oop(&played),
                    "{name} limit {limit} seed {seed}: play {:?}, replay {:?}",
                    played.errors,
                    replayed.errors
                );
                assert_eq!(
                    replayed.cost.value, played.cost.value,
                    "{name} limit {limit} seed {seed}"
                );
            }
        }
    }
}
