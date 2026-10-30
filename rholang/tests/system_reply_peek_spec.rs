use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::hash::keccak256::Keccak256;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, Par, TaggedContinuation};
use models::rust::utils::{new_freevar_par, new_gbool_par, new_gstring_par};
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::contract_call::ContractCall;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::RhoByteArray;
use rholang::rust::interpreter::system_processes::BodyRefs;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

fn channel(name: &str) -> Par { new_gstring_par(name.to_string(), Vec::new(), false) }

async fn execute_and_replay(terms: &[&str]) -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    execute_and_replay_with_errors(terms, &vec![Vec::new(); terms.len()]).await
}

async fn execute_and_replay_with_errors(
    terms: &[&str],
    expected_errors: &[Vec<InterpreterError>],
) -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    assert_eq!(terms.len(), expected_errors.len());
    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let (mut runtime, mut replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    for (step, term) in terms.iter().enumerate() {
        let rand = Blake2b512Random::create_from_bytes(&step.to_le_bytes());
        let played = runtime
            .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand.clone())
            .await
            .unwrap();
        assert_eq!(played.errors, expected_errors[step], "play step {step}");
        replay.rig(runtime.take_event_log().await).await.unwrap();
        let replayed = replay
            .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand)
            .await
            .unwrap();
        assert_eq!(replayed.errors, played.errors, "replay step {step}");
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
    let (runtime, replay) = execute_and_replay_with_errors(
        &[
            r#"for (_ <<- @"reply") { if (1) { Nil } else { Nil } }"#,
            r#"new hash(`rho:crypto:keccak256Hash`) in { hash!("test".toUtf8Bytes(), "reply") }"#,
            r#"for (@value <- @"reply") { @"afterError"!(value) }"#,
        ],
        &[
            vec![],
            vec![InterpreterError::IfConditionTypeError {
                actual_type: "Int".into(),
            }],
            vec![],
        ],
    )
    .await;
    for runtime in [&runtime, &replay] {
        assert_value(
            runtime,
            "afterError",
            RhoByteArray::create_par(Keccak256::hash(b"test".to_vec())),
        )
        .await;
        assert!(runtime.get_data(&channel("reply")).await.is_empty());
    }
}

async fn assert_restoration_error(
    body_ref: i64,
    dispatch_error: Option<InterpreterError>,
    expected: Vec<InterpreterError>,
) {
    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let calls = Arc::new(AtomicUsize::new(0));
    install_recorded_service(&runtime, BodyRefs::STDOUT, calls.clone()).await;
    let cost = runtime.cost.clone();
    runtime
        .reducer
        .dispatcher
        ._dispatch_table
        .write()
        .await
        .insert(
            body_ref,
            Box::new(move |_| {
                cost.set(Cost::create(0, "force restoration failure"));
                Box::pin(std::future::ready(match dispatch_error.clone() {
                    Some(error) => Err(error),
                    None => Ok(vec![channel("dispatch output")]),
                }))
            }),
        );
    runtime.cost.set(Cost::unsafe_max());
    assert!(runtime
        .reducer
        .space
        .consume(
            vec![channel("reply")],
            vec![BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            }],
            TaggedContinuation {
                tagged_cont: Some(TaggedCont::ScalaBodyRef(body_ref)),
                guard: None,
            },
            false,
            BTreeSet::from([0]),
        )
        .await
        .unwrap()
        .is_none());
    let evaluated = runtime
        .evaluate(
            r#"new svc(`rho:io:stdout`) in { svc!("reply") }"#,
            Cost::create(1_000_000, "restoration failure test"),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        runtime.cost.get().value < 0,
        "restoration must exhaust phlo: remaining {:?}, errors {:?}",
        runtime.cost.get(),
        evaluated.errors
    );
    assert_eq!(evaluated.errors, expected, "body ref {body_ref}");
    assert!(runtime.get_data(&channel("reply")).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_reply_peek_reports_restoration_errors() {
    for body_ref in [BodyRefs::DEV_NULL, BodyRefs::OLLAMA_MODELS] {
        assert_restoration_error(body_ref, None, vec![RSpaceError::from(
            InterpreterError::OutOfPhlogistonsError,
        )
        .into()])
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_reply_peek_combines_dispatch_and_restoration_errors() {
    for body_ref in [BodyRefs::DEV_NULL, BodyRefs::OLLAMA_MODELS] {
        let dispatch_error = InterpreterError::ReduceError("dispatch failed".into());
        assert_restoration_error(body_ref, Some(dispatch_error.clone()), vec![
            dispatch_error,
            RSpaceError::from(InterpreterError::OutOfPhlogistonsError).into(),
        ])
        .await;
        assert_restoration_error(body_ref, Some(InterpreterError::UserAbortError), vec![
            InterpreterError::UserAbortError,
        ])
        .await;
    }
}

async fn install_recorded_service(
    runtime: &RhoRuntimeImpl,
    body_ref: i64,
    calls: Arc<AtomicUsize>,
) {
    let space = runtime.reducer.space.clone();
    let dispatcher = Arc::downgrade(&runtime.reducer.dispatcher);
    runtime
        .reducer
        .dispatcher
        ._dispatch_table
        .write()
        .await
        .insert(
            body_ref,
            Box::new(move |args| {
                let call = ContractCall {
                    space: space.clone(),
                    dispatcher: dispatcher.upgrade().unwrap(),
                };
                let calls = calls.clone();
                Box::pin(async move {
                    let (produce, replay, previous, args) = call.unapply(args).unwrap();
                    let output = if replay {
                        previous
                    } else {
                        calls.fetch_add(1, Ordering::SeqCst);
                        vec![channel("recorded output")]
                    };
                    produce(&output, &args[0]).await?;
                    Ok(output)
                })
            }),
        );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_deterministic_outputs_survive_normal_and_peek_reply_dispatch() {
    for receive in ["<-", "<<-"] {
        let mut manager = InMemoryStoreManager::new();
        let store = manager.r_space_stores().await.unwrap();
        let (mut runtime, mut replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
        let calls = Arc::new(AtomicUsize::new(0));
        install_recorded_service(&runtime, BodyRefs::OLLAMA_MODELS, calls.clone()).await;
        install_recorded_service(&replay, BodyRefs::OLLAMA_MODELS, calls.clone()).await;
        let setup = format!(r#"for (@value {receive} @"reply") {{ @"result"!(value) }}"#);
        for (step, term) in [
            setup.as_str(),
            r#"new svc(`rho:ollama:models`) in { svc!("reply") }"#,
        ]
        .iter()
        .enumerate()
        {
            let rand = Blake2b512Random::create_from_bytes(&step.to_le_bytes());
            let played = runtime
                .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand.clone())
                .await
                .unwrap();
            assert!(played.errors.is_empty(), "{:?}", played.errors);
            let log = runtime.take_event_log().await;
            if step == 1 {
                let recorded = log
                    .iter()
                    .flat_map(|event| match event {
                        Event::IoEvent(IOEvent::Produce(produce)) => std::slice::from_ref(produce),
                        Event::Comm(comm) => comm.produces.as_slice(),
                        _ => &[],
                    })
                    .filter(|produce| !produce.is_deterministic)
                    .collect::<Vec<_>>();
                assert!(!recorded.is_empty());
                for produce in recorded {
                    assert!(!produce.failed);
                    assert_eq!(produce.output_value, vec![
                        channel("recorded output").encode_to_vec()
                    ]);
                }
            }
            replay.rig(log).await.unwrap();
            let replayed = replay
                .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand)
                .await
                .unwrap();
            assert_eq!(replayed.errors, played.errors);
            assert_eq!(played.cost, replayed.cost);
            replay.check_replay_data().await.unwrap();
            replay.take_event_log().await;
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            runtime.create_checkpoint().await.root,
            replay.create_checkpoint().await.root
        );
        for runtime in [&runtime, &replay] {
            assert_value(runtime, "result", channel("recorded output")).await;
            if receive == "<<-" {
                assert_value(runtime, "reply", channel("recorded output")).await;
            } else {
                assert!(runtime.get_data(&channel("reply")).await.is_empty());
            }
        }
    }
}
