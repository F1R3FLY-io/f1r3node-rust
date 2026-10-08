//! The native certification harness of decision 47 (design v2, section 6).
//!
//! Proposer self-certification and validator replay certify an offered deploy in five steps:
//! the journal check, the trace binding, the native replay environment, its evaluation and the
//! completeness check (`casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/
//! replay.rs`). Before these steps, the proposer encodes the budget recording and the operation
//! journal into the cost evidence of the offer, and the validator decodes them
//! (`casper/src/rust/util/rholang/costacc/offered_evidence.rs`).
//!
//! This harness runs the same steps at the v6 limits of the offered funded protocol. The limits
//! come from `models::rust::cost_protocol_limits`. The harness restates the derivations of
//! `casper/src/rust/util/rholang/costacc/production_limits.rs`, because a rholang test cannot
//! depend on casper.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::cost_protocol_limits::{
    offered_funded_v6_host_work_limits, offered_funded_v6_limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
    NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{
    decode_native_budget_recording, decode_native_operation_journal,
    encode_native_budget_recording, encode_native_operation_journal, funding_sig_compound,
    NativeBudgetRecording, NativeObservationLink, NativeOperationJournalLimits,
    NativeOperationRecord, NativeOperationTraceLimits, NativeRecordingWireLimits,
    NativeRuntimeConfig, RuntimeBudget, Sig,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::rho_runtime::{
    create_native_replay_env, create_rho_runtime, NativeReplayEnvironment, RhoHistoryRepository,
    RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::spaces::namespace::{self, ReifiedId};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::SUB_OPERATION_TAG;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

/// The deploy signature of every native run.
pub const DEPLOY: &[u8] = b"rrs-native-spaces";
/// The seed of the deploy's random state, the same in native play and native replay.
pub const SEED: &[u8] = b"rrs-native-spaces-random";
/// A native phlo limit that no program of the suites reaches.
pub const AMPLE_LIMIT: u64 = 1_000_000;
/// The weights of the compute, introduction, transfer and trace classes, and the price.
pub const WEIGHTS: [u64; 4] = [0, 1, 1, 0];
const PRICE: u64 = 3;
/// The public channel that the programs forward their results to.
pub const OUT: &str = "rrs-native-out";
/// The path segments of the v6 native trace: `V6_NATIVE_PATH_SEGMENTS` of
/// `casper/src/rust/util/rholang/costacc/production_limits.rs`.
const V6_NATIVE_PATH_SEGMENTS: usize = 1_024;

// ─── The v6 limits (restated from `production_limits.rs`) ──────────────────────────────────────

/// `offered_funded_v6_trace_limits`: the budget trace limits of native play and replay.
pub fn trace_limits() -> NativeBudgetTraceLimits {
    let protocol = offered_funded_v6_limits();
    NativeBudgetTraceLimits {
        attempts: protocol.deploy_log_events,
        path_segments: V6_NATIVE_PATH_SEGMENTS,
        regions: NativePhloRegionLimits {
            regions: protocol.envelope.payload.funding.authority_nodes,
            encoded_authority_bytes: protocol.funding_case.key_bytes,
        },
    }
}

/// `offered_funded_v6_recording_limits`: the wire limits of the recording in the evidence.
pub fn recording_limits() -> NativeRecordingWireLimits {
    let protocol = offered_funded_v6_limits();
    NativeRecordingWireLimits {
        wire: PhloWireLimits {
            total_bytes: protocol.evidence.field_bytes,
            field_bytes: protocol.evidence.field_bytes,
        },
        budget: trace_limits(),
        operations: protocol.deploy_log_events,
        source_entries: protocol.deploy_log_items,
        footprint_entries: protocol.deploy_log_items,
        footprint_bytes: protocol.evidence.field_bytes,
        predecessor_edges: protocol.deploy_log_items,
        total_path_segments: protocol.deploy_log_items,
    }
}

/// `offered_funded_v6_replay_limits().journal`: the limits of the journal check.
pub fn journal_limits() -> NativeOperationJournalLimits {
    let protocol = offered_funded_v6_limits();
    let recording = recording_limits();
    NativeOperationJournalLimits {
        budget: recording.budget,
        operations: recording.operations,
        total_path_segments: protocol.deploy_log_items,
        source_entries: recording.source_entries,
        footprint_entries: recording.footprint_entries,
        footprint_bytes: recording.footprint_bytes,
        predecessor_edges: recording.predecessor_edges,
    }
}

/// `offered_funded_v6_replay_limits().trace`: the limits of the trace binding.
pub fn trace_bind_limits() -> NativeOperationTraceLimits {
    let protocol = offered_funded_v6_limits();
    NativeOperationTraceLimits {
        events: protocol.deploy_log_events,
        source_entries: protocol.deploy_log_items,
        source_bytes: protocol.deploy_log_bytes,
        telemetry_items: protocol.deploy_log_items,
        telemetry_bytes: protocol.deploy_log_bytes,
    }
}

/// A fresh host-work budget of the v6 profile, as production gives each play and each replay.
pub fn host_work() -> HostWorkBudget { HostWorkBudget::new(offered_funded_v6_host_work_limits()) }

// ─── The native phlo contract ──────────────────────────────────────────────────────────────────

fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(WEIGHTS)
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: PRICE,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// Runs `action` with the native phlo contract of a deploy whose phlo limit is `limit`.
pub fn with_contract<T>(
    limit: u64,
    action: impl FnOnce(NativePhloExecutionContract<'_>) -> T,
) -> T {
    let descriptor = schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("the test schedule binds");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [PRICE];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: PRICE,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        limit,
    )
    .expect("the test controls check");
    let contract =
        NativePhloExecutionContract::new(controls, &binding).expect("the test contract builds");
    action(contract)
}

/// The native runtime configuration of a deploy whose phlo limit is `limit`.
pub fn config(limit: u64, host: HostWorkBudget) -> NativeRuntimeConfig {
    with_contract(limit, |contract| {
        NativeRuntimeConfig::new(contract, trace_limits(), host)
    })
}

pub fn random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(SEED) }

pub fn funding() -> Sig { funding_sig_compound(&[&1u64.to_be_bytes()]) }

// ─── Play ──────────────────────────────────────────────────────────────────────────────────────

/// A fresh runtime over the empty state, with the history at the empty root, and its history.
/// The system processes are installed in the hot store only, as the native replay environment
/// installs them again.
async fn fresh_runtime() -> (RhoRuntimeImpl, RhoHistoryRepository) {
    let mut stores = InMemoryStoreManager::new();
    let (space, _) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory play and replay space");
    space.create_checkpoint().await.expect("the empty root");
    let history = space.get_history_repository();
    let runtime = create_rho_runtime(
        space,
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await
    .expect("a play runtime");
    (runtime, history)
}

/// The values on [`OUT`], one Debug form for each datum, sorted.
async fn outputs_of(runtime: &RhoRuntimeImpl) -> Vec<String> {
    let channel = new_gstring_par(OUT.to_string(), Vec::new(), false);
    let mut outputs: Vec<String> = runtime
        .get_data(&channel)
        .await
        .into_iter()
        .map(|datum| format!("{:?}", datum.a.pars))
        .collect();
    outputs.sort_unstable();
    outputs
}

/// The play of one deploy: its result, its event log, the history of its pre-state, its
/// post-state root and its outputs.
pub struct Played {
    pub result: EvaluateResult,
    pub log: Vec<Event>,
    pub history: RhoHistoryRepository,
    pub root: Blake2b256Hash,
    pub outputs: Vec<String>,
}

/// Plays `term` as a native deploy whose phlo limit is `limit`, on a fresh runtime.
pub async fn native_play(term: &str, limit: u64) -> Played {
    let (mut runtime, history) = fresh_runtime().await;
    runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
    let result = runtime
        .evaluate_with_native_phlo(
            term,
            HashMap::new(),
            random(),
            None,
            config(limit, host_work()),
        )
        .await
        .expect("native play evaluates");
    let log = runtime
        .create_soft_checkpoint()
        .await
        .log
        .into_iter()
        .collect();
    let outputs = outputs_of(&runtime).await;
    let root = runtime.create_checkpoint().await.root;
    Played {
        result,
        log,
        history,
        root,
        outputs,
    }
}

// ─── Certification ─────────────────────────────────────────────────────────────────────────────

/// The budget recording and the operation journal of `played`, encoded into the evidence wire
/// at the v6 recording limits and decoded from it, as the proposer commits them and the
/// validator reads them.
pub fn committed_evidence(
    played: &Played,
) -> (NativeBudgetRecording, Arc<[NativeOperationRecord]>) {
    let recording = played
        .result
        .native_budget_recording
        .clone()
        .expect("native play records its budget");
    let journal = played
        .result
        .native_operation_recording
        .clone()
        .expect("native play records its operations");
    let wire = recording_limits();
    let encoder = host_work();
    let budget_bytes = encode_native_budget_recording(&recording, wire, &encoder)
        .expect("the budget recording fits the v6 evidence wire");
    let journal_bytes = encode_native_operation_journal(&journal, &recording.paths, wire, &encoder)
        .expect("the operation journal fits the v6 evidence wire");
    let decoder = host_work();
    let mut decoded = decode_native_budget_recording(&budget_bytes, wire, &decoder)
        .expect("the budget recording decodes");
    let decoded_journal =
        decode_native_operation_journal(&journal_bytes, &mut decoded, wire, &decoder)
            .expect("the operation journal decodes");
    assert_eq!(decoded.used, recording.used, "the evidence keeps the usage");
    assert_eq!(
        decoded.session, recording.session,
        "the evidence keeps the session"
    );
    assert_eq!(
        decoded_journal.as_ref(),
        journal.as_ref(),
        "the evidence keeps the journal rows"
    );
    (decoded, decoded_journal)
}

/// The native replay environment of `played` over `journal`, at the v6 limits, before its
/// evaluation. Returns the rejection of the journal check or of the trace binding.
pub async fn replay_environment(
    played: &Played,
    limit: u64,
    recording: &NativeBudgetRecording,
    journal: Arc<[NativeOperationRecord]>,
) -> Result<NativeReplayEnvironment, String> {
    replay_environment_with(played, limit, recording, journal, host_work()).await
}

/// [`replay_environment`] with the host-work budget `host` of the certification.
pub async fn replay_environment_with(
    played: &Played,
    limit: u64,
    recording: &NativeBudgetRecording,
    journal: Arc<[NativeOperationRecord]>,
    host: HostWorkBudget,
) -> Result<NativeReplayEnvironment, String> {
    let trace = with_contract(limit, |contract| {
        contract.check_operation_journal(
            recording.session,
            recording,
            journal,
            journal_limits(),
            &host,
        )
    })
    .map_err(|error| format!("the journal check rejects the journal: {error}"))?
    .bind_trace(played.log.clone().into(), trace_bind_limits(), &host)
    .map_err(|error| format!("the trace binding rejects the log: {error}"))?;
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, funding());
    budget
        .reset_for_native_execution(config(limit, host.clone()))
        .expect("the replay budget enters native execution");
    Ok(create_native_replay_env(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        host,
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
    )
    .await
    .expect("the native replay environment builds"))
}

/// The certification of a play: the result of native replay, the completeness check, the
/// completed usage and the exported post-state root.
pub struct Certification {
    pub evaluation: Result<EvaluateResult, InterpreterError>,
    pub complete: Result<(), String>,
    pub usage: Result<u64, String>,
    pub root: Result<Blake2b256Hash, String>,
}

/// Evaluates `term` in `environment` and finishes the certification.
pub async fn finish(mut environment: NativeReplayEnvironment, term: &str) -> Certification {
    let parsed = Compiler::source_to_adt(term).expect("the program parses");
    let evaluation = environment.evaluate(parsed, random()).await;
    let complete = environment
        .check_complete()
        .await
        .map_err(|error| error.to_string());
    let usage = environment
        .completed_usage()
        .await
        .map_err(|error| error.to_string());
    let root = environment
        .export()
        .await
        .map(|exported| exported.root().clone())
        .map_err(|error| error.to_string());
    Certification {
        evaluation,
        complete,
        usage,
        root,
    }
}

/// Certifies `played` through its committed evidence, as validator replay does.
pub async fn certify(played: &Played, term: &str, limit: u64) -> Certification {
    let (recording, journal) = committed_evidence(played);
    let environment = replay_environment(played, limit, &recording, journal)
        .await
        .expect("the committed evidence checks");
    finish(environment, term).await
}

/// Checks that `certification` reproduces `played`: the errors of play, a complete journal, the
/// phlo usage of play, the same authority events and byte observations, and the post-state root
/// of play.
pub fn assert_reproduces(label: &str, played: &Played, certification: &Certification) {
    let evaluation = certification
        .evaluation
        .as_ref()
        .unwrap_or_else(|error| panic!("{label}: native replay evaluates: {error:?}"));
    assert_eq!(evaluation.errors, played.result.errors, "{label}: errors");
    assert_eq!(
        certification.complete,
        Ok(()),
        "{label}: replay consumes every journal row of play"
    );
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    assert_eq!(
        certification.usage,
        Ok(recording.used),
        "{label}: replay uses the phlo of play"
    );
    assert_eq!(
        evaluation.native_phlo_usage, played.result.native_phlo_usage,
        "{label}: native phlo usage"
    );
    assert_eq!(
        evaluation.authority_events, played.result.authority_events,
        "{label}: authority events"
    );
    assert_eq!(
        evaluation.byte_observations.rows, played.result.byte_observations.rows,
        "{label}: byte observations"
    );
    assert_eq!(
        certification.root.as_ref(),
        Ok(&played.root),
        "{label}: replay reaches the post-state root of play"
    );
}

/// True when the certification rejects the play: native replay fails, its deploy fails, the
/// journal is incomplete, or the post-state root is not exported.
pub fn rejected(certification: &Certification) -> bool {
    match &certification.evaluation {
        Err(_) => true,
        Ok(evaluation) => {
            !evaluation.errors.is_empty()
                || certification.complete.is_err()
                || certification.root.is_err()
        }
    }
}

// ─── Journal rows ──────────────────────────────────────────────────────────────────────────────

/// The channels of the footprint of `row`, decoded from their bincode bytes.
pub fn footprint_channels(row: &NativeOperationRecord) -> Vec<Par> {
    row.footprint
        .iter()
        .map(|bytes| bincode::deserialize(bytes).expect("a footprint entry is a channel"))
        .collect()
}

/// True when `channel` is a channel of a native space: a Reified id other than an op channel.
pub fn is_space_channel(channel: &Par) -> bool {
    !matches!(
        namespace::reified_id_of(channel),
        None | Some(ReifiedId::Op(_))
    )
}

/// True when `row` is a staged space operation: its path ends with a sub-order segment
/// `(i, SUB_OPERATION_TAG)` (decision 19, D3).
pub fn is_space_row(recording: &NativeBudgetRecording, row: &NativeOperationRecord) -> bool {
    recording
        .paths
        .last_segment(row.occurrence.path)
        .is_some_and(|(_, tag)| tag == SUB_OPERATION_TAG)
}

/// The budget attempts that `row` links to: its introduction and the observation of its COMM.
pub fn row_attempts(row: &NativeOperationRecord) -> Vec<usize> {
    let mut attempts = Vec::with_capacity(2);
    if let NativeObservationLink::Attempt(index) = row.introduction {
        attempts.push(index);
    }
    if let Some(comm) = &row.comm {
        if let NativeObservationLink::Attempt(index) = comm.observation {
            attempts.push(index);
        }
    }
    attempts
}
