//! DR-114: recorded external-service calls in native funded execution. Play
//! calls a service once and records the reply on the triggering produce.
//! Replay produces the record and never calls the service.

use models::rhoapi::{ListParWithRandom, Par};
// Changed by the merge of feature/cost-accounted-rho: the parent module
// imports the proptest prelude, and `use super::*` brings it here. This
// import was unused.
// use proptest::prelude::*;
use prost::Message;
// Added by the merge of feature/cost-accounted-rho (finding R1-10).
use rspace_plus_plus::rspace::hashing::stable_hash_provider::channel_hash_for_epoch;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

use super::*;
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource;
use crate::rust::interpreter::accounting::native_runtime::tests::{
    priced_config_from, with_priced_contract_from,
};
use crate::rust::interpreter::grpc_client_service::{GrpcClientMockConfig, GrpcClientService};
use crate::rust::interpreter::io::errors::FserrCode;
use crate::rust::interpreter::io::response;
use crate::rust::interpreter::openai_service::{OpenAIMockConfig, OpenAIService};
use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};
use crate::rust::interpreter::system_processes::{
    is_native_recorded_op, non_deterministic_ops, BodyRefs,
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
    /// Added by the merge of feature/cost-accounted-rho (finding R1-10): the
    /// extra system processes of play and of replay.
    processes: fn() -> Vec<Definition>,
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
            processes: extra_processes,
        }
    }
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-10): a guard
/// that always commits.
struct CommitAlways;

impl ProduceCommitGuard for CommitAlways {
    fn with_commit(&self, commit: Box<dyn FnOnce() + '_>) -> Result<(), RSpaceError> {
        commit();
        Ok(())
    }
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-10): the echo
/// of `extra_processes`, with one change: its reply is a guarded produce. The
/// URN, the channel and the body ref stay the same, so a run with this echo
/// and a run with the unguarded echo differ only in the guard. Node code
/// makes no guarded produce of its own. MeTTaIL's semantic services make
/// them, and this echo stands in for them.
fn guarded_echo_processes() -> Vec<Definition> {
    vec![Definition {
        urn: "rho:test:nativeEcho".to_owned(),
        fixed_channel: models::rust::utils::new_gstring_par(
            "native-echo".to_owned(),
            Vec::new(),
            false,
        ),
        arity: 2,
        body_ref: 901,
        remainder: None,
        handler: Box::new(|ctx| {
            Box::new(move |args| {
                let ctx = ctx.clone();
                Box::pin(async move {
                    let call = crate::rust::interpreter::contract_call::ContractCall {
                        space: ctx.space.clone(),
                        dispatcher: ctx.dispatcher.clone(),
                    };
                    let (produce, _, _, values) = call
                        .unapply_guarded(args, Arc::new(CommitAlways))
                        .ok_or_else(|| {
                            InterpreterError::ReduceError("invalid guarded echo call".to_owned())
                        })?;
                    let [value, ack] = values.as_slice() else {
                        return Err(InterpreterError::ReduceError(
                            "invalid guarded echo arity".to_owned(),
                        ));
                    };
                    produce(std::slice::from_ref(value), ack).await?;
                    Ok(vec![value.clone()])
                })
            })
        }),
    }]
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
    // Changed by the merge of feature/cost-accounted-rho (finding R1-25):
    // F1R3Lang's `Datum` shares its payload by `Arc`.
    // data.into_iter().flat_map(|datum| datum.a.pars).collect()
    data.into_iter()
        .flat_map(|datum| datum.a.pars.clone())
        .collect()
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
            // Added by the merge of feature/cost-accounted-rho: F1R3Lang's
            // resting COMM holds copies of the produces that it retires.
            Event::RestingComm(resting) => Arc::make_mut(resting)
                .ordered_produces
                .iter_mut()
                .for_each(clear),
        }
    }
}

async fn play_and_replay(call: Call<'_>, forge: impl FnOnce(&mut Vec<Event>)) -> Run {
    let funding = FundingFixture::default();
    let (weights, price, limit, source) = (funding.weights, funding.price, call.limit, call.source);
    let parsed = Compiler::source_to_adt(call.term).expect("the term parses");
    let mut stores = InMemoryStoreManager::new();
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // play space has the cost epoch of the harness.
    // let (play, _) = RSpace::create_with_replay(
    //     stores.r_space_stores().await.expect("stores"),
    //     Arc::new(Box::new(Matcher)),
    // )
    // .expect("the space opens");
    let (play, _) = RSpace::create_with_replay_for_epoch(
        NATIVE_EPOCH,
        stores.r_space_stores().await.expect("stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("the space opens");
    play.create_checkpoint().await.expect("the root checkpoint");
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(b"native-recorded-call", funding.authority());
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // budget binds the cost epoch of the harness, as production binds the
    // epoch of its adopted policy.
    // let settings = priced_config_from(limit, weights, price, source).with_record_room(call.room);
    let settings = priced_config_from(limit, weights, price, source)
        .with_record_room(call.room)
        .with_event_codec_epoch(NATIVE_EPOCH);
    budget
        .reset_for_native_execution(settings)
        .expect("native play starts");
    let (reducer, block_data, _, deploy_data) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        // Changed by the merge of feature/cost-accounted-rho (finding R1-10):
        // the call names its system processes.
        // &mut extra_processes(),
        &mut (call.processes)(),
        budget.clone(),
        call.play,
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
        // Changed by the merge of feature/cost-accounted-rho (finding R1-12):
        // F1R3Lang's `get_data` is fallible.
        // outputs.push(pars(play.get_data(&channel(name)).await));
        outputs.push(pars(
            play.get_data(&channel(name))
                .await
                .expect("the play space reads the output channel"),
        ));
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
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // replay budget binds the cost epoch of the harness.
    // let replay_settings = priced_config_from(limit, weights, price, source);
    let replay_settings =
        priced_config_from(limit, weights, price, source).with_event_codec_epoch(NATIVE_EPOCH);
    replay_budget
        .reset_for_native_execution(replay_settings)
        .expect("native replay starts");
    // Changed by the merge of feature/cost-accounted-rho (finding R1-11):
    // F1R3Lang's native replay takes the codec epoch of its budget first.
    let mut environment = create_native_replay_env(
        // Changed by the merge of feature/cost-accounted-rho (finding R1-33):
        // native replay has the cost epoch of the play space.
        // EventCodecEpoch::Legacy,
        NATIVE_EPOCH,
        trace,
        history,
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        // Changed by the merge of feature/cost-accounted-rho (finding R1-10):
        // the call names its system processes.
        // &mut extra_processes(),
        &mut (call.processes)(),
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

/// Added by the merge of feature/cost-accounted-rho (finding R1-10): the term
/// of the reply tests. The echo replies on the private name `ack`. A system
/// channel cannot take the reply, because the user holds only a write-only
/// bundle of it, and the echo produces on that bundle. The reducer spawns the
/// echo's continuation after both branches of the Par, so the consume on
/// `ack` rests before the reply. The call and the reply therefore each fire a
/// COMM on their produce.
const ECHO_TO_ACK: &str = r#"
    new echo(`rho:test:nativeEcho`), ack in { echo!(7, *ack) | for (@x <- ack) { @"out"!(x) } }
"#;

/// Added by the merge of feature/cost-accounted-rho (finding R1-10): puts
/// `record` on the slot of the echo's reply. That slot is the first produce
/// that fires a COMM and is not on the echo's channel.
fn forge_reply_record(log: &mut [Event], record: Vec<Vec<u8>>) {
    let echo = channel_hash_for_epoch(NATIVE_EPOCH, &channel("native-echo"))
        .expect("the echo channel has a key in the epoch of the harness");
    let reply = (1..log.len())
        .find(|&index| match (&log[index - 1], &log[index]) {
            (Event::IoEvent(IOEvent::Produce(produce)), Event::Comm(comm)) => {
                produce.channel_hash != echo
                    && comm.produces.iter().any(|copy| copy.hash == produce.hash)
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("the echo's reply fires a COMM on its produce: {log:?}"))
        - 1;
    forge_record(&mut log[reply..], record);
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-10): native
/// replay measures a guarded produce as it measures an unguarded produce.
/// Both resolve the introduction record (D-F2, DR-118), so the guard changes
/// no charge.
#[tokio::test]
async fn native_replay_measures_a_guarded_reply_as_an_unguarded_reply() {
    let unguarded = play_and_replay(Call::new(ECHO_TO_ACK), |_| {}).await;
    let guarded = play_and_replay(
        Call {
            processes: guarded_echo_processes,
            ..Call::new(ECHO_TO_ACK)
        },
        |_| {},
    )
    .await;
    for run in [&unguarded, &guarded] {
        assert_eq!(run.played.0, Ok(()));
        assert_eq!(run.output, vec![RhoNumber::create_par(7)]);
        assert!(run.records.is_empty(), "the echo records nothing");
        run.assert_replay_agrees();
    }
    assert_eq!(guarded.used, unguarded.used, "the guard changes no charge");
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-10): T6 (E2)
/// for the reply of a system process. A record on the reply passes the slot
/// check, because its copies are equal. Replay rejects it for the unguarded
/// reply and for the guarded reply.
#[tokio::test]
async fn native_replay_rejects_a_record_on_a_guarded_reply() {
    for (name, processes) in [
        ("unguarded", extra_processes as fn() -> Vec<Definition>),
        ("guarded", guarded_echo_processes),
    ] {
        let forged = play_and_replay(
            Call {
                processes,
                ..Call::new(ECHO_TO_ACK)
            },
            |log| forge_reply_record(log, vec![RhoNumber::create_par(7).encode_to_vec()]),
        )
        .await;
        assert_eq!(forged.played.0, Ok(()), "{name}");
        let error = format!("{:?}", forged.replayed.0);
        assert!(
            error.contains("native replay found a recorded output on an unrecorded call"),
            "{name}: {error}"
        );
    }
}

/// T6 (`missing_record_rejected`): a recorded call whose record is removed
/// from every copy passes the slot check. Replay rejects it, because it never
/// asks the service.
#[tokio::test]
async fn native_replay_rejects_a_recorded_call_without_its_record() {
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
