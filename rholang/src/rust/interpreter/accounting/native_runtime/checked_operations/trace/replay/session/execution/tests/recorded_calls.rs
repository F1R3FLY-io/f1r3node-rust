//! DR-114: recorded external-service calls in native funded execution. Play
//! calls a service once and records the reply on the triggering produce.
//! Replay produces the record and never calls the service.

use models::rhoapi::{ListParWithRandom, Par};
use proptest::prelude::*;
use prost::Message;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

use super::*;
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource;
use crate::rust::interpreter::accounting::native_runtime::tests::{
    priced_config_from, with_priced_contract_from,
};
// Added by G3 (DR-135, S3b H8): the forged replies of the plain-data tests.
use crate::rust::interpreter::accounting::owner_funding::plain_data_tests::review_forgeries;
use crate::rust::interpreter::grpc_client_service::{GrpcClientMockConfig, GrpcClientService};
use crate::rust::interpreter::io::errors::FserrCode;
use crate::rust::interpreter::io::response;
use crate::rust::interpreter::openai_service::{OpenAIMockConfig, OpenAIService};
// Changed by G3 (DR-135, S3b H8): the tests build a deep list reply.
// use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};
use crate::rust::interpreter::rho_type::{RhoList, RhoNumber, RhoString};
// Changed by G3 (DR-135, S3b H8): the tests call the play side of the
// plain-data check.
// use crate::rust::interpreter::system_processes::{
//     is_native_recorded_op, non_deterministic_ops, BodyRefs,
// };
use crate::rust::interpreter::system_processes::{
    is_native_recorded_op, native_played_reply, non_deterministic_ops, BodyRefs,
};

type Outcome = (Result<(), InterpreterError>, EvaluationFailureSummary);

/// The record room of the tests. It holds every reply of this module.
const ROOM: u64 = 1 << 20;

const GPT4_TO_OUT: &str = r#"
    new gpt4(`rho:ai:gpt4`), ack in {
        gpt4!("hello", *ack) | for (@answer <- ack) { @"out"!(answer) }
    }
"#;

struct Call<'a> {
    term: &'a str,
    limit: u64,
    room: u64,
    source: NativeBoundSource,
    play: ExternalServices,
    replay: ExternalServices,
    /// The channels whose data the run reads, in order.
    outs: Vec<String>,
}

impl<'a> Call<'a> {
    fn new(term: &'a str) -> Self {
        Self {
            term,
            limit: 1_000_000,
            room: ROOM,
            source: NativeBoundSource::Certificate,
            play: ExternalServices::noop(),
            replay: ExternalServices::noop(),
            outs: vec!["out".to_owned()],
        }
    }
}

struct Run {
    played: Outcome,
    replayed: Outcome,
    used: u64,
    replay_used: Option<u64>,
    complete: bool,
    output: Vec<Par>,
    replay_output: Vec<Par>,
    /// The data of each channel of `Call::outs`, after play and after replay.
    outputs: Vec<Vec<Par>>,
    replay_outputs: Vec<Vec<Par>>,
    records: Vec<Vec<Vec<u8>>>,
}

impl Run {
    /// Replay reproduces play, and the replayed session is complete.
    fn assert_replay_agrees(&self) {
        assert_eq!(self.replayed.0, self.played.0);
        assert_eq!(self.replayed.1, self.played.1);
        assert!(self.complete, "the replayed session must be complete");
        assert_eq!(self.replay_used, Some(self.used));
        assert_eq!(self.replay_output, self.output);
    }

    /// The one record of the run, decoded.
    fn record(&self) -> Vec<Par> {
        let [record] = self.records.as_slice() else {
            panic!("expected one record, found {}", self.records.len());
        };
        record
            .iter()
            .map(|bytes| Par::decode(bytes.as_slice()).expect("a record holds encoded pars"))
            .collect()
    }
}

fn channel(name: &str) -> Par {
    models::rust::utils::new_gstring_par(name.to_owned(), Vec::new(), false)
}

fn services(openai: OpenAIService, grpc: GrpcClientService) -> ExternalServices {
    let mut services = ExternalServices::noop();
    services.openai = Arc::new(tokio::sync::Mutex::new(openai));
    services.grpc_client = grpc;
    services
}

fn pars(data: Vec<Datum<ListParWithRandom>>) -> Vec<Par> {
    data.into_iter().flat_map(|datum| datum.a.pars).collect()
}

fn failure(name: &str, code: &'static str, text: &str) -> Par {
    response::err(FserrCode(code), format!("{name}: {text}"))
}

/// The record of every introduction produce in the log that carries one.
fn records(log: &[Event]) -> Vec<Vec<Vec<u8>>> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if !produce.output_value.is_empty() => {
                Some(produce.output_value.clone())
            }
            _ => None,
        })
        .collect()
}

/// Puts `record` on the first matched produce slot of the log: on its
/// introduction, on its COMM copy and on its repetition key. The copies stay
/// equal, so the evidence check (E1) accepts the forged trace.
fn forge_record(log: &mut [Event], record: Vec<Vec<u8>>) {
    let slot = (1..log.len())
        .find(|&index| match (&log[index - 1], &log[index]) {
            (Event::IoEvent(IOEvent::Produce(produce)), Event::Comm(comm)) => {
                comm.produces.iter().any(|copy| copy.hash == produce.hash)
            }
            _ => false,
        })
        .expect("the term has a produce-triggered COMM")
        - 1;
    let mark = |produce: &mut Produce| {
        produce.output_value = record.clone();
        produce.is_deterministic = false;
    };
    let Event::IoEvent(IOEvent::Produce(introduction)) = &mut log[slot] else {
        unreachable!("the slot starts with its introduction")
    };
    let hash = introduction.hash.clone();
    mark(introduction);
    let Event::Comm(comm) = &mut log[slot + 1] else {
        unreachable!("the slot holds its COMM")
    };
    comm.produces
        .iter_mut()
        .filter(|copy| copy.hash == hash)
        .for_each(mark);
    comm.times_repeated = std::mem::take(&mut comm.times_repeated)
        .into_iter()
        .map(|(mut key, count)| {
            if key.hash == hash {
                mark(&mut key);
            }
            (key, count)
        })
        .collect();
}

/// Removes every record from the log, on all copies. The copies stay equal,
/// so the evidence check (E1) accepts the stripped trace.
fn strip_records(log: &mut [Event]) {
    let clear = |produce: &mut Produce| {
        produce.output_value.clear();
        produce.is_deterministic = true;
    };
    for event in log.iter_mut() {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => clear(produce),
            Event::Comm(comm) => {
                comm.produces.iter_mut().for_each(clear);
                comm.times_repeated = std::mem::take(&mut comm.times_repeated)
                    .into_iter()
                    .map(|(mut key, count)| {
                        clear(&mut key);
                        (key, count)
                    })
                    .collect();
            }
            Event::IoEvent(IOEvent::Consume(_)) => {}
        }
    }
}

async fn play_and_replay(call: Call<'_>, forge: impl FnOnce(&mut Vec<Event>)) -> Run {
    let funding = FundingFixture::default();
    let (weights, price, limit, source) = (funding.weights, funding.price, call.limit, call.source);
    let parsed = Compiler::source_to_adt(call.term).expect("the term parses");
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.expect("stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("the space opens");
    play.create_checkpoint().await.expect("the root checkpoint");
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(b"native-recorded-call", funding.authority());
    let settings = priced_config_from(limit, weights, price, source).with_record_room(call.room);
    budget
        .reset_for_native_execution(settings)
        .expect("native play starts");
    let (reducer, block_data, _, deploy_data) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        budget.clone(),
        call.play,
        crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
    )
    .await
    .expect("the play environment");
    block_data.write().await.block_number = 123;
    deploy_data.write().await.timestamp = 456;
    let rand = || Blake2b512Random::create_from_bytes(b"native-recorded-call");
    let scope = budget.enter_comm_accounting_scope();
    let played = reducer.inj_with_observation(parsed.clone(), rand()).await;
    drop(scope);
    let recording = budget
        .native_budget_recording()
        .expect("a native recording")
        .expect("a native recording");
    let operations = budget
        .native_operation_recording()
        .expect("a native operation recording")
        .expect("a native operation recording");
    let mut outputs = Vec::with_capacity(call.outs.len());
    for name in &call.outs {
        outputs.push(pars(play.get_data(&channel(name)).await));
    }
    let mut log = play.create_soft_checkpoint().await.log;
    let records = records(&log);
    forge(&mut log);
    let trace = with_priced_contract_from(limit, limit, weights, price, source, |contract| {
        contract
            .expect("the limit is an admissible bound")
            .check_operation_journal(recording.session, &recording, operations, journal_limits())
            .expect("the journal checks")
    })
    .bind_trace(log.into(), NativeOperationTraceLimits {
        events: 100,
        source_entries: 1000,
        source_bytes: 1_000_000,
        telemetry_items: 100,
        telemetry_bytes: 100_000,
    })
    .expect("the trace binds");
    let replay_budget = RuntimeBudget::new(Cost::unsafe_max());
    replay_budget.set_deploy_signature_funded(b"native-recorded-call", funding.authority());
    let replay_settings = priced_config_from(limit, weights, price, source);
    replay_budget
        .reset_for_native_execution(replay_settings)
        .expect("native replay starts");
    let mut environment = create_native_replay_env(
        trace,
        history,
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        replay_budget,
        call.replay,
    )
    .await
    .expect("the replay environment");
    environment.block_data.write().await.block_number = 123;
    environment.deploy_data.write().await.timestamp = 456;
    let replayed = tokio::time::timeout(
        Duration::from_secs(10),
        environment.evaluate_raw(parsed, rand()),
    )
    .await
    .expect("native replay must complete");
    let complete = environment.check_complete().await.is_ok();
    let replay_used = environment.completed_usage().await.ok();
    let mut replay_outputs = Vec::with_capacity(call.outs.len());
    for name in &call.outs {
        replay_outputs.push(
            environment
                .get_data(&channel(name))
                .await
                .map(pars)
                .unwrap_or_default(),
        );
    }
    Run {
        played,
        replayed,
        used: recording.used,
        replay_used,
        complete,
        output: outputs.concat(),
        replay_output: replay_outputs.concat(),
        outputs,
        replay_outputs,
        records,
    }
}

/// T1: play records the reply of the service. Replay produces the record
/// and never asks its own service, which would answer differently.
#[tokio::test]
async fn native_replay_uses_the_recorded_reply_and_never_calls_the_service() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let run = play_and_replay(
        Call {
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("A")),
                GrpcClientService::new_noop(),
            ),
            replay: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("B")),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |_| {},
    )
    .await;
    assert_eq!(run.played.0, Ok(()));
    assert_eq!(run.output, vec![RhoString::create_par("A".to_owned())]);
    assert_eq!(run.record(), vec![RhoString::create_par("A".to_owned())]);
    run.assert_replay_agrees();
}

/// T1: a fire-and-forget call records its `Nil` reply. Only play reaches
/// the gRPC client.
#[tokio::test]
async fn native_grpc_tell_reaches_the_client_in_play_only() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let played_client = GrpcClientMockConfig::create("localhost", 50051);
    let replayed_client = GrpcClientMockConfig::create("localhost", 50051);
    let run = play_and_replay(
        Call {
            play: services(
                OpenAIService::new_noop(),
                GrpcClientService::new_mock(played_client.clone()),
            ),
            replay: services(
                OpenAIService::new_noop(),
                GrpcClientService::new_mock(replayed_client.clone()),
            ),
            ..Call::new(
                r#"new tell(`rho:io:grpcTell`) in { tell!("localhost", 50051, "payload") }"#,
            )
        },
        |_| {},
    )
    .await;
    assert_eq!(run.played.0, Ok(()));
    assert!(played_client.was_called());
    assert!(!replayed_client.was_called());
    assert_eq!(run.record(), vec![Par::default()]);
    run.assert_replay_agrees();
}

/// T2: a service failure, a bad argument and an exhausted record room give
/// paid `[false, code, message]` replies. They are recorded and replayed
/// like a success, and the deploy does not fail.
#[tokio::test]
async fn native_failure_replies_are_recorded_and_replayed() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let failed = play_and_replay(
        Call {
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::error_on_first_call()),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |_| {},
    )
    .await;
    let bad_argument = play_and_replay(
        Call {
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("A")),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(&GPT4_TO_OUT.replace(r#""hello""#, "42"))
        },
        |_| {},
    )
    .await;
    let too_large = play_and_replay(
        Call {
            room: 0,
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("A")),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |_| {},
    )
    .await;
    let disabled = play_and_replay(
        Call::new(
            r#"
            new chat(`rho:ollama:chat`), ack in {
                chat!("model", "hello", *ack) | for (@answer <- ack) { @"out"!(answer) }
            }
            "#,
        ),
        |_| {},
    )
    .await;
    for (run, expected) in [
        (
            &failed,
            failure("gpt4", "EXT_FAILED", "external service failed"),
        ),
        (
            &bad_argument,
            failure("gpt4", "EXT_BAD_ARG", "invalid argument"),
        ),
        (
            &too_large,
            failure(
                "gpt4",
                "EXT_OUTPUT_TOO_LARGE",
                "external output exceeds the record room",
            ),
        ),
        (
            &disabled,
            failure("ollama_chat", "EXT_FAILED", "external service failed"),
        ),
    ] {
        assert_eq!(run.played.0, Ok(()));
        assert_eq!(run.output, vec![expected.clone()]);
        assert_eq!(run.record(), vec![expected]);
        run.assert_replay_agrees();
    }
}

/// T3 (gap G-A): the signed limit pays the request but not the reply. The
/// record stays on the trigger, so replay produces the same reply and meets
/// the same denial, a charged user failure (DR-113).
#[tokio::test]
async fn native_reply_past_the_signed_limit_keeps_its_record() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let reply = "A".repeat(20_000);
    let run = play_and_replay(
        Call {
            limit: 5_000,
            source: NativeBoundSource::SignedLimit,
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion(&reply)),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |_| {},
    )
    .await;
    assert_eq!(run.played.0, Err(InterpreterError::SignedLimitExhausted));
    assert!(run.played.1.contains(PhloFailure::User));
    assert!(run.played.1.permits_retained_charge());
    assert!(run.output.is_empty());
    assert_eq!(run.record(), vec![RhoString::create_par(reply)]);
    run.assert_replay_agrees();
}

/// T4 (P2): a persistent send to a recorded process is a classified user
/// failure, raised before the service is reached.
#[tokio::test]
async fn native_persistent_send_to_a_recorded_process_is_a_user_failure() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let played_client = GrpcClientMockConfig::create("localhost", 50051);
    let run = play_and_replay(
        Call {
            play: services(
                OpenAIService::new_noop(),
                GrpcClientService::new_mock(played_client.clone()),
            ),
            ..Call::new(
                r#"new tell(`rho:io:grpcTell`) in { tell!!("localhost", 50051, "payload") }"#,
            )
        },
        |_| {},
    )
    .await;
    assert!(
        matches!(
            &run.played.0,
            Err(InterpreterError::SystemProcessShapeError(_))
        ),
        "{:?}",
        run.played.0
    );
    assert!(run.played.1.contains(PhloFailure::User));
    assert!(run.played.1.permits_retained_charge());
    assert!(!played_client.was_called());
    assert!(run.records.is_empty());
    run.assert_replay_agrees();
}

/// T5 (§4.1 option A): a malformed call of a deterministic system process is
/// a classified user failure in native funded execution.
#[tokio::test]
async fn native_malformed_system_call_is_a_user_failure() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let run = play_and_replay(
        Call::new(r#"new hash(`rho:crypto:sha256Hash`), ack in { hash!(42, *ack) }"#),
        |_| {},
    )
    .await;
    assert_eq!(
        run.played.0,
        Err(InterpreterError::SystemProcessShapeError(
            "Incorrect arguments for sha256Hash".to_owned()
        ))
    );
    assert!(run.played.1.contains(PhloFailure::User));
    assert!(run.played.1.permits_retained_charge());
    run.assert_replay_agrees();
}

/// T6 (E2): a record on a COMM of the user's own contract passes the slot
/// check, because its copies are equal. Replay rejects it, so a block cannot
/// hand a forged record to the user's code.
#[tokio::test]
async fn native_replay_rejects_a_record_on_an_unrecorded_call() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let term = r#"new x, y in { y!(0) | for (_ <- y) { x!(1) } | for (_ <- x) { Nil } }"#;
    let honest = play_and_replay(Call::new(term), |_| {}).await;
    assert_eq!(honest.played.0, Ok(()));
    assert!(honest.records.is_empty());
    honest.assert_replay_agrees();
    let forged = play_and_replay(Call::new(term), |log| {
        forge_record(log, vec![RhoNumber::create_par(7).encode_to_vec()])
    })
    .await;
    assert_eq!(forged.played.0, Ok(()));
    let error = format!("{:?}", forged.replayed.0);
    assert!(
        error.contains("native replay found a recorded output on an unrecorded call"),
        "{error}"
    );
}

/// T6 (`missing_record_rejected`): a recorded call whose record is removed
/// from every copy passes the slot check. Replay rejects it, because it never
/// asks the service.
#[tokio::test]
async fn native_replay_rejects_a_recorded_call_without_its_record() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let stripped = play_and_replay(
        Call {
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("A")),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |log| strip_records(log),
    )
    .await;
    assert_eq!(stripped.played.0, Ok(()));
    assert_eq!(stripped.records.len(), 1, "play recorded the reply");
    let error = format!("{:?}", stripped.replayed.0);
    assert!(
        error.contains("native replay of gpt4 has no recorded output"),
        "{error}"
    );
    // Added by DR-132 (D2-B): racing play can record the receive on `ack`
    // after the reply, so the receive waits on the reply that the failed
    // call never produces. The stall rule ends that wait.
    assert!(error.contains("native replay stalled"), "{error}");
    assert!(!stripped.complete, "a stalled replay is not complete");
}

/// DR-132 (decision D2-B): a recorded reply changed on every copy passes the
/// slot check. Replay produces the changed reply, which no row records, so
/// the receive that waits on the recorded reply can never run. Replay rejects
/// the block when every live task waits on a recorded operation.
#[tokio::test]
async fn native_replay_rejects_a_changed_recorded_reply() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let changed = play_and_replay(
        Call {
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("A")),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |log| {
            forge_record(log, vec![
                RhoString::create_par("B".to_owned()).encode_to_vec()
            ])
        },
    )
    .await;
    assert_eq!(changed.played.0, Ok(()));
    assert_eq!(changed.record(), vec![RhoString::create_par(
        "A".to_owned()
    )]);
    let error = format!("{:?}", changed.replayed.0);
    assert!(error.contains("native replay stalled"), "{error}");
    assert!(!changed.complete, "a stalled replay is not complete");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// The invariants of Rocq `RecordedExternalReplay` on the implementation:
    /// every recorded call gets exactly one reply, the recorded successes fill
    /// the room exactly, and replay gives each call the reply of play, in
    /// whatever order it meets the calls.
    #[test]
    fn parallel_recorded_calls_replay_call_by_call(
        calls in 1usize..=4,
        reply_length in 0usize..300,
        room in 0u64..2_000,
    ) {
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
        let text = "a".repeat(reply_length);
        let reply = RhoString::create_par(text.clone());
        let cost = u64::try_from(reply.encoded_len()).expect("a small reply") + 64;
        let acks = (0..calls).map(|index| format!("ack{index}")).collect::<Vec<_>>();
        let sends = (0..calls)
            .map(|index| {
                format!(
                    "gpt4!(\"prompt {index}\", *ack{index}) | for (@answer <- ack{index}) {{ @\"out{index}\"!(answer) }}"
                )
            })
            .collect::<Vec<_>>();
        let term = format!(
            "new gpt4(`rho:ai:gpt4`), {} in {{ {} }}",
            acks.join(", "),
            sends.join(" | ")
        );
        let run = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(play_and_replay(
                Call {
                    room,
                    outs: (0..calls).map(|index| format!("out{index}")).collect(),
                    play: services(
                        OpenAIService::Mock(OpenAIMockConfig::single_completion(&text)),
                        GrpcClientService::new_noop(),
                    ),
                    ..Call::new(&term)
                },
                |_| {},
            ));
        let too_large = failure(
            "gpt4",
            "EXT_OUTPUT_TOO_LARGE",
            "external output exceeds the record room",
        );
        prop_assert_eq!(&run.played.0, &Ok(()));
        prop_assert!(run.outputs.iter().all(|output| output.len() == 1));
        prop_assert_eq!(run.records.len(), calls);
        let successes = run.output.iter().filter(|par| **par == reply).count();
        let failures = run.output.iter().filter(|par| **par == too_large).count();
        prop_assert_eq!(successes + failures, calls);
        prop_assert_eq!(
            u64::try_from(successes).expect("a small count"),
            (room / cost).min(u64::try_from(calls).expect("a small count"))
        );
        prop_assert_eq!(&run.replayed.0, &run.played.0);
        prop_assert_eq!(&run.replayed.1, &run.played.1);
        prop_assert!(run.complete);
        prop_assert_eq!(run.replay_used, Some(run.used));
        prop_assert_eq!(&run.replay_outputs, &run.outputs);
    }
}

/// T7: a print is not recorded. Replay neither formats nor prints, and it
/// still delivers the acknowledgement.
#[tokio::test]
async fn native_print_replays_without_a_record() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let run = play_and_replay(
        Call::new(
            r#"
            new print(`rho:io:stdoutAck`), log(`rho:io:stderr`), ack in {
                print!("printed in play", *ack) | log!("logged in play") |
                for (_ <- ack) { @"out"!(1) }
            }
            "#,
        ),
        |_| {},
    )
    .await;
    assert_eq!(run.played.0, Ok(()));
    assert_eq!(run.output, vec![RhoNumber::create_par(1)]);
    assert!(run.records.is_empty());
    run.assert_replay_agrees();
}

/// T10: the native recorded set holds dev's nondeterministic set and the four
/// Chroma processes that dev also runs in replay. Prints and deterministic
/// processes are not recorded.
#[test]
fn native_recorded_set_extends_dev_set() {
    assert!(non_deterministic_ops()
        .into_iter()
        .all(is_native_recorded_op));
    for body_ref in [
        BodyRefs::CHROMA_CREATE_COLLECTION,
        BodyRefs::CHROMA_GET_COLLECTION_META,
        BodyRefs::CHROMA_UPSERT_ENTRIES,
        BodyRefs::CHROMA_DELETE_DOCUMENTS,
    ] {
        assert!(is_native_recorded_op(body_ref));
    }
    for body_ref in [
        BodyRefs::STDOUT,
        BodyRefs::STDOUT_ACK,
        BodyRefs::STDERR,
        BodyRefs::STDERR_ACK,
        BodyRefs::SHA256_HASH,
    ] {
        assert!(!is_native_recorded_op(body_ref));
    }
}

/// Added by G3 (DR-135, S3b H8): plays `GPT4_TO_OUT` with the honest reply
/// "A" and replays it with the record replaced by `forged` on every copy.
async fn replay_with_forged_record(forged: &Par) -> Run {
    play_and_replay(
        Call {
            play: services(
                OpenAIService::Mock(OpenAIMockConfig::single_completion("A")),
                GrpcClientService::new_noop(),
            ),
            ..Call::new(GPT4_TO_OUT)
        },
        |log| forge_record(log, vec![forged.encode_to_vec()]),
    )
    .await
}

/// Added by G3 (DR-135, S3b H8): the error of a replay whose record is not
/// plain data.
const NOT_PLAIN_REPLAY: &str = "native replay of gpt4 has a recorded output that is not plain data";

/// Added by G3 (DR-135, S3b H8): a forged record that is not plain data
/// passes the slot check, because its copies are equal. Replay checks the
/// record before it produces it, and rejects the block with a platform
/// error, as it does a missing record. Each forged shape of the review is
/// rejected.
#[tokio::test]
async fn native_replay_rejects_a_recorded_reply_that_is_not_plain_data() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    for (label, forged) in review_forgeries() {
        let run = replay_with_forged_record(&forged).await;
        assert_eq!(run.played.0, Ok(()), "{label}: play succeeds");
        assert_eq!(
            run.record(),
            vec![RhoString::create_par("A".to_owned())],
            "{label}: play records the honest reply"
        );
        let error = format!("{:?}", run.replayed.0);
        assert!(error.contains(NOT_PLAIN_REPLAY), "{label}: {error}");
        assert!(error.contains("BugFoundError"), "{label}: {error}");
    }
}

/// Added by G3 (DR-135, S3b H8): a forged record whose bundle lies 25 list
/// levels deep is rejected in replay too. Prost decodes at most 100 nested
/// messages, and each list level takes 3 (a Par, an Expr and an EList), so
/// the record decodes, and the plain-data check finds the bundle.
#[tokio::test]
async fn native_replay_rejects_a_deep_recorded_reply_that_is_not_plain_data() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let mut forged = Par {
        bundles: vec![models::rhoapi::Bundle {
            body: Some(RhoString::create_par("b".to_owned())),
            write_flag: true,
            read_flag: true,
        }],
        ..Default::default()
    };
    for _ in 0..25 {
        forged = RhoList::create_par(vec![forged]);
    }
    assert!(
        Par::decode(forged.encode_to_vec().as_slice()).is_ok(),
        "the forged record decodes"
    );
    let run = replay_with_forged_record(&forged).await;
    assert_eq!(run.played.0, Ok(()));
    let error = format!("{:?}", run.replayed.0);
    assert!(error.contains(NOT_PLAIN_REPLAY), "{error}");
}

/// Added by G3 (DR-135, S3b H8): in play, a service reply that is not plain
/// data becomes the paid `EXT_FAILED` failure reply before the size fold and
/// the record-room take, so it takes no room. A plain reply keeps its value
/// and takes its bytes of the room.
#[test]
fn native_play_turns_a_reply_that_is_not_plain_data_into_a_failure_that_takes_no_room() {
    let funding = FundingFixture::default();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(b"native-played-reply", funding.authority());
    budget
        .reset_for_native_execution(
            priced_config_from(
                1_000_000,
                funding.weights,
                funding.price,
                NativeBoundSource::Certificate,
            )
            .with_record_room(ROOM),
        )
        .expect("native play starts");
    let failed = failure("gpt4", "EXT_FAILED", "external output is not plain data");
    for (label, forged) in review_forgeries() {
        assert_eq!(
            native_played_reply(
                "gpt4",
                vec![RhoString::create_par("A".to_owned()), forged],
                &budget
            ),
            vec![failed.clone()],
            "{label}"
        );
    }
    let plain = RhoString::create_par("A".to_owned());
    let cost = u64::try_from(plain.encoded_len()).expect("a small reply") + 64;
    assert_eq!(
        native_played_reply("gpt4", vec![plain.clone()], &budget),
        vec![plain]
    );
    assert!(
        budget.take_native_record_room(ROOM - cost),
        "the forged replies took no room"
    );
    assert!(!budget.take_native_record_room(1), "the room is full");
}
