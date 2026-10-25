use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::hash::keccak256::Keccak256;
use models::rhoapi::Par;
use models::rust::utils::{new_gbool_par, new_gstring_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::RhoByteArray;
use rholang::rust::interpreter::test_utils::resources::{create_runtimes, with_runtime};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn channel(name: &str) -> Par { new_gstring_par(name.to_string(), Vec::new(), false) }

async fn execute_and_replay(terms: &[&str]) -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let (mut runtime, mut replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    for (step, term) in terms.iter().enumerate() {
        let rand = Blake2b512Random::create_from_bytes(&step.to_le_bytes());
        let played = runtime
            .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand.clone())
            .await
            .unwrap();
        assert!(played.errors.is_empty(), "play: {:?}", played.errors);
        replay.rig(runtime.take_event_log().await).await.unwrap();
        let replayed = replay
            .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand)
            .await
            .unwrap();
        assert!(replayed.errors.is_empty(), "replay: {:?}", replayed.errors);
        replay.check_replay_data().await.unwrap();
        replay.take_event_log().await;
        assert_eq!(played.cost, replayed.cost);
    }
    assert_eq!(
        runtime.create_checkpoint().await.root,
        replay.create_checkpoint().await.root
    );
    (runtime, replay)
}

async fn assert_value(runtime: &RhoRuntimeImpl, name: &str, expected: Par) {
    let data = runtime.get_data(&channel(name)).await;
    assert_eq!(data.len(), 1, "missing or duplicate result on {name}");
    assert_eq!(data[0].a.pars, vec![expected]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hash_reply_survives_pending_peeks_and_is_consumed_once() {
    let (runtime, replay) = execute_and_replay(&[
        r#"for (@first <<- @"reply") {
            @"first"!(first) | @"firstDone"!(Nil)
        } |
        for (@second <<- @"reply") {
            @"second"!(second) | @"secondDone"!(Nil)
        } |
        for (_ <- @"firstDone" & _ <- @"secondDone") {
            for (@value <- @"reply") {
                @"consumed"!(value) |
                for (_ <- @"reply") { @"duplicate"!(true) }
            }
        }"#,
        r#"new hash(`rho:crypto:keccak256Hash`) in { hash!("test".toUtf8Bytes(), "reply") }"#,
    ])
    .await;
    for runtime in [&runtime, &replay] {
        let hash = RhoByteArray::create_par(Keccak256::hash(b"test".to_vec()));
        for name in ["first", "second", "consumed"] {
            assert_value(runtime, name, hash.clone()).await;
        }
        assert!(runtime.get_data(&channel("reply")).await.is_empty());
        assert!(runtime.get_data(&channel("duplicate")).await.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_reply_peek_restores_joined_data_without_duplicating_persistent_data() {
    for persistent in [false, true] {
        let setup = format!(
            r#"@"state"{}(("tag", 7)) |
            for (_ <<- @"reply" & @("tag", n) <<- @"state") {{
                @"peeked"!(n == 7) |
                for (_ <- @"reply" & @state <- @"state") {{
                    @"consumed"!(state == ("tag", 7))
                }}
            }}"#,
            if persistent { "!!" } else { "!" }
        );
        let (runtime, replay) = execute_and_replay(&[
            &setup,
            r#"new ack(`rho:io:stdoutAck`) in { ack!("peek regression", "reply") }"#,
        ])
        .await;
        for runtime in [&runtime, &replay] {
            for name in ["peeked", "consumed"] {
                assert_value(runtime, name, new_gbool_par(true, Vec::new(), false)).await;
            }
            assert!(runtime.get_data(&channel("reply")).await.is_empty());
            let state = runtime.get_data(&channel("state")).await;
            assert_eq!(state.len(), usize::from(persistent));
            if persistent {
                assert!(state[0].persist);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_reply_peek_reports_body_errors() {
    with_runtime("system-reply-peek-error-", |mut runtime| async move {
        let setup = runtime
            .evaluate_with_term(r#"for (_ <<- @"reply") { if (1) { Nil } else { Nil } }"#)
            .await
            .unwrap();
        assert!(setup.errors.is_empty());
        let result = runtime
            .evaluate_with_term(
                r#"new hash(`rho:crypto:keccak256Hash`) in { hash!("test".toUtf8Bytes(), "reply") }"#,
            )
            .await
            .unwrap();
        assert!(
            matches!(result.errors.as_slice(), [InterpreterError::IfConditionTypeError { .. }]),
            "unexpected errors: {:?}",
            result.errors
        );
    })
    .await;
}
