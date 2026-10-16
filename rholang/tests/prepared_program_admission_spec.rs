use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ENot, Expr, Par};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::rholang::protobuf_encoder;
use proptest::prelude::*;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    authority_demand, cost_signature_to_sig, AuthorityByteEventKind,
};
use rholang::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
    NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{NativeBudgetRecording, NativeRuntimeConfig, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::frontend::{
    prepare_program, PreparationError, PreparedProgram, ProgramFrontend, PREPARATION_BINDING_V1,
    PREPARED_PROGRAM_ABI_V1,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::rho_runtime::{
    create_runtime_from_kv_store_for_epoch, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event as RSpaceEvent, IOEvent};

const PROVIDER_SOURCE: &str = "this is deliberately not a Rholang program";

fn scalar(value: i64) -> Par {
    let mut process = Par::default();
    process.exprs.push(Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    });
    process
}

fn environment() -> HashMap<String, Par> { HashMap::from([("supplied".to_owned(), scalar(42))]) }

fn random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[17; 128]) }

async fn with_cost_runtime<F, Fut, R>(f: F) -> R
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: Future<Output = R>,
{
    with_cost_runtime_for_epoch(EventCodecEpoch::CostV2, f).await
}

async fn with_cost_runtime_for_epoch<F, Fut, R>(epoch: EventCodecEpoch, f: F) -> R
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: Future<Output = R>,
{
    let mut manager = InMemoryStoreManager::new();
    let runtime = create_runtime_from_kv_store_for_epoch(
        epoch,
        manager.r_space_stores().await.expect("cost-bearing stores"),
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        Arc::new(Box::new(Matcher)),
        ExternalServices::noop(),
    )
    .await
    .expect("cost-bearing runtime");
    assert_eq!(runtime.reducer.space.codec_epoch(), epoch);
    assert!(!runtime.cost.is_unmetered());
    runtime
        .cost
        .set_deploy_signature_funded(b"prepared-binding-native", Sig::Ground(b"alice".to_vec()));
    f(runtime).await
}

fn native_config() -> NativeRuntimeConfig { native_config_for_epoch(EventCodecEpoch::CostV2) }

fn native_config_for_epoch(epoch: EventCodecEpoch) -> NativeRuntimeConfig {
    let descriptor = PhloScheduleV1 {
        protocol_version: match epoch {
            EventCodecEpoch::CostV1 => 6,
            EventCodecEpoch::CostV2 => 7,
            EventCodecEpoch::Legacy => panic!("native fixture requires a cost-bearing epoch"),
        },
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([2, 3, 5, 7])
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let binding =
        PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).expect("native schedule");
    let selected = binding.schedule();
    let schedules = [selected];
    let ceilings = [2];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 1_000_000,
            price_ceiling: 2,
            required_owner_ceilings: &ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        1_000_000,
    )
    .expect("checked native controls");
    let contract = NativePhloExecutionContract::new(controls, &binding)
        .expect("checked native execution contract");
    NativeRuntimeConfig::new(
        contract,
        NativeBudgetTraceLimits {
            attempts: 100_000,
            path_segments: 4096,
            regions: NativePhloRegionLimits {
                regions: 1000,
                encoded_authority_bytes: 10_000_000,
            },
        },
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000))),
    )
    .with_event_codec_epoch(epoch)
}

fn assert_native_charge(result: &EvaluateResult) {
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let used = result.native_phlo_usage.expect("native phlo usage");
    assert!(used > 0 && used <= 1_000_000);
    assert!(result.byte_observations.has_complete_measurements());
    assert!(!result.byte_observations.rows.is_empty());
    let recording = result
        .native_budget_recording
        .as_ref()
        .expect("native recording");
    assert_eq!(recording.used, used);
    assert!(!recording.attempts.is_empty());
}

fn assert_recording_unchanged(expected: &NativeBudgetRecording, runtime: &RhoRuntimeImpl) {
    let actual = runtime
        .cost
        .native_budget_recording()
        .expect("capture native history")
        .expect("native history retained");
    assert_eq!(actual.session, expected.session);
    assert_eq!(actual.used, expected.used);
    let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
    let attempts = |recording: &NativeBudgetRecording| {
        recording
            .attempts
            .iter()
            .map(|attempt| {
                (
                    attempt.occurrence.session,
                    recording
                        .paths
                        .suffix(attempt.occurrence.path, 0, &host)
                        .expect("causal path"),
                    attempt.occurrence.stage,
                    attempt.observation.clone(),
                    attempt.granted,
                )
            })
            .collect::<Vec<_>>()
    };
    let retries = |recording: &NativeBudgetRecording| {
        recording
            .retries
            .iter()
            .map(|retry| {
                (
                    retry.occurrence.session,
                    recording
                        .paths
                        .suffix(retry.occurrence.path, 0, &host)
                        .expect("retry path"),
                    retry.occurrence.stage,
                    retry.observation.clone(),
                    retry.accepted_attempt,
                    retry.fresh_before,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(attempts(&actual), attempts(expected));
    assert_eq!(retries(&actual), retries(expected));
}

fn prepared(source: &str) -> PreparedProgram {
    PreparedProgram::from_normalized(
        Compiler::source_to_adt_with_normalizer_env(source, HashMap::new())
            .expect("test oracle must normalize the fixture"),
    )
}

fn signature_lane(name: &str) -> [u8; 32] {
    let process = prepared(&format!("{{% Nil %}}[{name}]")).into_par();
    cost_signature_to_sig(
        process.cost_signed_terms[0]
            .signature
            .as_ref()
            .expect("fixture signature"),
    )
    .expect("canonical fixture signature")
    .lane_hash()
}

#[tokio::test]
async fn native_receive_introduction_retains_outer_authority_in_each_cost_epoch() {
    const SOURCE: &str = r#"{% for(_ <- @"waiting"){ Nil } %}[ outer -o continuation ]"#;
    for epoch in [EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
        with_cost_runtime_for_epoch(epoch, |mut runtime| async move {
            let creator = runtime.cost.signature().lane_hash();
            let result = runtime
                .evaluate_prepared_with_native_phlo(
                    prepared(SOURCE),
                    SOURCE.len() as u64,
                    random(),
                    None,
                    native_config_for_epoch(epoch),
                )
                .await
                .expect("native signed receiver evaluation");
            assert_native_charge(&result);
            let mut introductions = result
                .byte_observations
                .rows
                .iter()
                .filter(|row| row.kind == AuthorityByteEventKind::ConsumeIntroduction);
            let introduction = introductions.next().expect("receiver introduction");
            assert!(introductions.next().is_none());
            let demand =
                authority_demand(&introduction.authority).expect("receiver introduction authority");
            assert_eq!(demand.get(&signature_lane("outer")), 1);
            assert_eq!(demand.get(&signature_lane("continuation")), 0);
            assert_eq!(demand.get(&creator), 0);
            assert_eq!(demand.0.len(), 1);
            assert!(
                introduction
                    .measurement
                    .expect("measured receiver")
                    .introduction_bytes
                    > 0
            );
            let trace = runtime.take_event_log().await;
            let mut consumes = trace.iter().filter_map(|event| match event {
                RSpaceEvent::IoEvent(IOEvent::Consume(consume)) => Some(consume),
                _ => None,
            });
            let consume = consumes.next().expect("receiver trace source");
            assert!(consumes.next().is_none());
            assert_eq!(
                introduction.event_id,
                consume_introduction_identity(consume)
            );
        })
        .await;
    }
}

#[tokio::test]
async fn native_stack_introduction_retains_continuation_authority_and_trace_identity() {
    const SOURCE: &str =
        r#"{% for(_ <- @"x"){ continuation :: () } %}[ outer -o continuation ] | @"x"!(0)"#;
    for epoch in [EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
        with_cost_runtime_for_epoch(epoch, |mut runtime| async move {
            let creator = runtime.cost.signature().lane_hash();
            let result = runtime
                .evaluate_prepared_with_native_phlo(
                    prepared(SOURCE),
                    SOURCE.len() as u64,
                    random(),
                    None,
                    native_config_for_epoch(epoch),
                )
                .await
                .expect("native continuation stack evaluation");
            assert_native_charge(&result);
            assert_eq!(result.authority_stack_births.len(), 1);
            let birth = &result.authority_stack_births[0];
            let trace = runtime.take_event_log().await;
            let mut stack_sources = trace.iter().filter_map(|event| match event {
                RSpaceEvent::IoEvent(IOEvent::Produce(produce))
                    if produce.hash.bytes().as_slice() == birth.produce_hash.as_slice() =>
                {
                    Some(produce)
                }
                _ => None,
            });
            let source = stack_sources
                .next()
                .expect("stack receipt matches its trace source");
            assert!(stack_sources.next().is_none());
            let identity = produce_introduction_identity(source);
            let mut introductions = result.byte_observations.rows.iter().filter(|row| {
                row.kind == AuthorityByteEventKind::ProduceIntroduction && row.event_id == identity
            });
            let introduction = introductions
                .next()
                .expect("stack introduction matches its receipt");
            assert!(introductions.next().is_none());
            let demand =
                authority_demand(&introduction.authority).expect("stack introduction authority");
            assert_eq!(demand.get(&signature_lane("continuation")), 1);
            assert_eq!(demand.get(&signature_lane("outer")), 0);
            assert_eq!(demand.get(&creator), 0);
            assert_eq!(demand.0.len(), 1);
            assert!(
                introduction
                    .measurement
                    .expect("measured stack")
                    .introduction_bytes
                    > 0
            );
        })
        .await;
    }
}

struct CountingFrontend {
    version: u16,
    language_identity: Option<Vec<u8>>,
    source: String,
    version_calls: AtomicUsize,
    preparation_calls: AtomicUsize,
    result: Mutex<Option<Result<PreparedProgram, PreparationError>>>,
}

impl CountingFrontend {
    fn new(version: u16, result: Result<PreparedProgram, PreparationError>) -> Self {
        Self {
            version,
            language_identity: None,
            source: PROVIDER_SOURCE.to_owned(),
            version_calls: AtomicUsize::new(0),
            preparation_calls: AtomicUsize::new(0),
            result: Mutex::new(Some(result)),
        }
    }

    fn with_identity(mut self, identity: Vec<u8>) -> Self {
        self.language_identity = Some(identity);
        self
    }

    fn with_source(mut self, source: String) -> Self {
        self.source = source;
        self
    }

    fn assert_calls(&self, version: usize, preparation: usize) {
        assert_eq!(self.version_calls.load(Ordering::Relaxed), version);
        assert_eq!(self.preparation_calls.load(Ordering::Relaxed), preparation);
    }
}

impl ProgramFrontend for CountingFrontend {
    fn abi_version(&self) -> u16 {
        self.version_calls.fetch_add(1, Ordering::Relaxed);
        self.version
    }

    fn language_identity(&self) -> Option<&[u8]> { self.language_identity.as_deref() }

    fn prepare(
        &self,
        source: &str,
        supplied_environment: HashMap<String, Par>,
    ) -> Result<PreparedProgram, PreparationError> {
        self.preparation_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(source, self.source);
        assert_eq!(supplied_environment, environment());
        self.result
            .lock()
            .expect("frontend result lock")
            .take()
            .expect("a source must be prepared exactly once")
    }
}

#[test]
fn artifact_borrows_then_moves_without_cloning_the_process() {
    let process = scalar(42);
    let allocation = process.exprs.as_ptr();
    let bytes = process.encode_to_vec();
    let artifact = PreparedProgram::from_normalized(process);
    assert_eq!(artifact.abi_version(), PREPARED_PROGRAM_ABI_V1);
    assert_eq!(artifact.binding_version(), None);
    assert_eq!(artifact.source_binding(), None);
    assert!(std::ptr::eq(artifact.as_par(), artifact.as_par()));
    assert_eq!(artifact.as_par().exprs.as_ptr(), allocation);
    assert_eq!(artifact.as_par().encode_to_vec(), bytes);
    let process = artifact.into_par();
    assert_eq!(process.exprs.as_ptr(), allocation);
    assert_eq!(process.encode_to_vec(), bytes);
}

#[test]
fn bound_artifact_moves_source_identity_output_and_process_without_copying() {
    let process = scalar(42);
    let source = PROVIDER_SOURCE.as_bytes().to_vec();
    let language_identity = b"complete-language-identity".to_vec();
    let output = protobuf_encoder::encode_to_vec(&process);
    let allocations = (
        process.exprs.as_ptr(),
        source.as_ptr(),
        language_identity.as_ptr(),
        output.as_ptr(),
    );
    let artifact =
        PreparedProgram::from_normalized_with_binding(process, source, language_identity, output)
            .expect("exact output binding");
    assert_eq!(artifact.abi_version(), PREPARED_PROGRAM_ABI_V1);
    assert_eq!(artifact.binding_version(), Some(PREPARATION_BINDING_V1));
    let (source, identity, output) = artifact.source_binding().expect("retained source binding");
    assert_eq!(source.as_ptr(), allocations.1);
    assert_eq!(identity.as_ptr(), allocations.2);
    assert_eq!(output.as_ptr(), allocations.3);
    assert_eq!(source, PROVIDER_SOURCE.as_bytes());
    assert_eq!(identity, b"complete-language-identity");
    assert_eq!(output, protobuf_encoder::encode_to_vec(artifact.as_par()));
    assert_eq!(artifact.as_par().exprs.as_ptr(), allocations.0);
    let process = artifact.into_par();
    assert_eq!(process.exprs.as_ptr(), allocations.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn source_binding_admission_requires_both_exact_identities(
        value in any::<i64>(),
        source in proptest::collection::vec(any::<char>(), 0..64),
        identity in proptest::collection::vec(any::<u8>(), 0..128),
        change_source in any::<bool>(),
        change_identity in any::<bool>(),
    ) {
        let source: String = source.into_iter().collect();
        let mut claimed_source = source.as_bytes().to_vec();
        let mut claimed_identity = identity.clone();
        if change_source {
            claimed_source.push(0);
        }
        if change_identity {
            claimed_identity.push(0);
        }
        let process = scalar(value);
        let output = protobuf_encoder::encode_to_vec(&process);
        let prepared = PreparedProgram::from_normalized_with_binding(
            process, claimed_source, claimed_identity, output,
        ).expect("output binding is exact before source admission");
        let frontend = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(prepared))
            .with_identity(identity.clone())
            .with_source(source.clone());
        let result = prepare_program(&frontend, &source, environment());
        prop_assert_eq!(result.is_ok(), !change_source && !change_identity);
        frontend.assert_calls(1, 1);
        if let Ok(artifact) = result {
            let (bound_source, bound_identity, bound_output) = artifact.source_binding().unwrap();
            prop_assert_eq!(bound_source, source.as_bytes());
            prop_assert_eq!(bound_identity, identity.as_slice());
            prop_assert_eq!(bound_output, protobuf_encoder::encode_to_vec(artifact.as_par()));
        }
    }

    #[test]
    fn output_binding_rejects_changed_bytes(
        value in any::<i64>(),
        mutation in 0_usize..3,
    ) {
        let process = scalar(value);
        let mut output = protobuf_encoder::encode_to_vec(&process);
        match mutation {
            0 => output[0] ^= 1,
            1 => output.push(0),
            _ => { output.pop(); }
        }
        let result = PreparedProgram::from_normalized_with_binding(
            process, PROVIDER_SOURCE.as_bytes().to_vec(), vec![1, 2, 3], output,
        );
        prop_assert!(result.is_err());
    }

    #[test]
    fn output_binding_rejects_a_different_process(value in any::<i64>()) {
        let output = protobuf_encoder::encode_to_vec(&scalar(value));
        let result = PreparedProgram::from_normalized_with_binding(
            scalar(value.wrapping_add(1)), PROVIDER_SOURCE.as_bytes().to_vec(), vec![1], output,
        );
        prop_assert!(result.is_err());
    }
}

#[test]
fn preparation_rejects_missing_and_undeclared_source_bindings() {
    let missing = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(prepared("Nil")))
        .with_identity(vec![1, 2, 3]);
    let error = prepare_program(&missing, PROVIDER_SOURCE, environment())
        .err()
        .expect("required binding is missing");
    assert!(error
        .to_string()
        .contains("requires a prepared program source binding"));
    missing.assert_calls(1, 1);

    let bound = PreparedProgram::from_normalized_with_binding(
        Par::default(),
        PROVIDER_SOURCE.as_bytes().to_vec(),
        vec![1, 2, 3],
        Vec::new(),
    )
    .expect("empty process output");
    let undeclared = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(bound));
    let error = prepare_program(&undeclared, PROVIDER_SOURCE, environment())
        .err()
        .expect("bound artifact requires a declared identity");
    assert!(error
        .to_string()
        .contains("does not declare a language identity"));
    undeclared.assert_calls(1, 1);
}

#[test]
fn deep_bound_artifact_reuses_stack_safe_encoding_and_drop() {
    std::thread::Builder::new()
        .name("bound-artifact-small-stack".to_owned())
        .stack_size(128 * 1024)
        .spawn(|| {
            for consume in [false, true] {
                let mut process = Par::default();
                for _ in 0..50_000 {
                    let mut parent = Par::default();
                    parent.exprs.push(Expr {
                        expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(process) })),
                    });
                    process = parent;
                }
                let output = protobuf_encoder::encode_to_vec(&process);
                let artifact = PreparedProgram::from_normalized_with_binding(
                    process,
                    vec![1],
                    vec![2],
                    output,
                )
                .expect("deep exact output binding");
                assert_eq!(artifact.as_par().exprs.len(), 1);
                if consume {
                    drop(artifact.into_par());
                } else {
                    drop(artifact);
                }
            }
        })
        .expect("spawn bounded-stack binding test")
        .join()
        .expect("deep binding must not overflow the native stack");
}

#[test]
fn deep_artifacts_reuse_stack_safe_process_drop_on_both_ownership_paths() {
    std::thread::Builder::new()
        .name("prepared-artifact-small-stack".to_owned())
        .stack_size(128 * 1024)
        .spawn(|| {
            for consume in [false, true] {
                let mut process = Par::default();
                for _ in 0..50_000 {
                    let mut parent = Par::default();
                    parent.exprs.push(Expr {
                        expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(process) })),
                    });
                    process = parent;
                }
                let artifact = PreparedProgram::from_normalized(process);
                assert_eq!(artifact.as_par().exprs.len(), 1);
                if consume {
                    drop(artifact.into_par());
                } else {
                    drop(artifact);
                }
            }
        })
        .expect("spawn bounded-stack ownership test")
        .join()
        .expect("deep prepared ownership must not overflow the native stack");
}

#[test]
fn preparation_is_explicit_versioned_and_invoked_once() {
    let frontend = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(prepared("Nil")));
    let artifact = prepare_program(&frontend, PROVIDER_SOURCE, environment())
        .expect("compatible frontend preparation");
    assert_eq!(artifact.as_par(), &Par::default());
    frontend.assert_calls(1, 1);
}

async fn establish_prior_state(runtime: &mut RhoRuntimeImpl) {
    let result = runtime
        .evaluate_with_native_phlo(
            "{% @\"prior-state\"!(7) %}[alice]",
            HashMap::new(),
            random(),
            None,
            native_config(),
        )
        .await
        .expect("prior native deployment completes");
    assert_native_charge(&result);
    assert_eq!(runtime.cost.native_phlo_usage(), result.native_phlo_usage);
}

#[tokio::test]
async fn source_binding_refusals_preserve_prior_budget_authority_and_rspace() {
    with_cost_runtime(|mut runtime| async move {
        establish_prior_state(&mut runtime).await;
        let prior_cost = runtime.cost.total_cost();
        let prior_remaining = runtime.cost.get();
        let prior_signature = runtime.cost.signature();
        let prior_log = runtime.get_cost_log();
        let prior_native = runtime.cost.native_budget_recording().unwrap().unwrap();
        let prior_authority = runtime.cost.authority_events();
        let prior_observations = runtime.cost.byte_observations();
        let prior_root = runtime.create_checkpoint().await.root;

        for defect in 0..5 {
            let process = prepared("@1!(2)").into_par();
            let mut source = PROVIDER_SOURCE.as_bytes().to_vec();
            let mut identity = vec![1, 2, 3];
            let mut output = protobuf_encoder::encode_to_vec(&process);
            let result = match defect {
                0 => Ok(PreparedProgram::from_normalized(process)),
                _ => {
                    match defect {
                        1 => source.push(0),
                        2 => identity.push(0),
                        3 => output.push(0),
                        _ => {}
                    }
                    PreparedProgram::from_normalized_with_binding(process, source, identity, output)
                }
            };
            let frontend = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, result);
            let frontend = if defect == 4 {
                frontend
            } else {
                frontend.with_identity(vec![1, 2, 3])
            };
            let result = runtime
                .evaluate_with_frontend(
                    &frontend,
                    PROVIDER_SOURCE,
                    Cost::create(77, "must not reset"),
                    environment(),
                    random(),
                )
                .await
                .expect("binding rejection uses the normal result envelope");
            frontend.assert_calls(1, 1);
            assert_eq!(result.cost, Cost::create(0, "parse failure"));
            assert!(matches!(result.errors.as_slice(), [
                InterpreterError::ParserError(_)
            ]));
            assert!(result.mergeable.is_empty());
            assert_eq!(runtime.cost.total_cost(), prior_cost);
            assert_eq!(runtime.cost.get(), prior_remaining);
            assert_eq!(runtime.cost.signature(), prior_signature);
            assert_eq!(runtime.get_cost_log(), prior_log);
            assert_eq!(runtime.cost.native_phlo_usage(), Some(prior_native.used));
            assert_eq!(runtime.cost.authority_events(), prior_authority);
            assert_eq!(runtime.cost.byte_observations(), prior_observations);
            assert_recording_unchanged(&prior_native, &runtime);
            assert_eq!(runtime.create_checkpoint().await.root, prior_root);
        }
    })
    .await;
}

#[tokio::test]
async fn bound_frontend_uses_the_existing_native_funding_path() {
    with_cost_runtime(|mut runtime| async move {
        let process = prepared("{% @1!(2) %}[alice]").into_par();
        let output = protobuf_encoder::encode_to_vec(&process);
        let artifact = PreparedProgram::from_normalized_with_binding(
            process,
            PROVIDER_SOURCE.as_bytes().to_vec(),
            vec![1, 2, 3],
            output,
        )
        .expect("exact bound send");
        let frontend = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(artifact))
            .with_identity(vec![1, 2, 3]);
        let program = prepare_program(&frontend, PROVIDER_SOURCE, environment())
            .expect("bound source admission");
        let result = runtime
            .evaluate_prepared_with_native_phlo(
                program,
                PROVIDER_SOURCE.len() as u64,
                random(),
                None,
                native_config(),
            )
            .await
            .expect("bound frontend deployment");
        frontend.assert_calls(1, 1);
        assert_native_charge(&result);
        let contents = runtime.get_hot_changes().await;
        let row = contents
            .get(&vec![scalar(1)])
            .expect("actual channel write");
        assert_eq!(row.data.len(), 1);
        assert_eq!(row.data[0].a.pars, vec![scalar(2)]);
    })
    .await;
}

#[tokio::test]
async fn preparation_and_version_failures_do_not_reset_or_charge_prior_state() {
    with_cost_runtime(|mut runtime| async move {
        establish_prior_state(&mut runtime).await;
        let prior_cost = runtime.cost.total_cost();
        let prior_remaining = runtime.cost.get();
        let prior_signature = runtime.cost.signature();
        let prior_log = runtime.get_cost_log();
        let prior_native = runtime.cost.native_budget_recording().unwrap().unwrap();
        let prior_authority = runtime.cost.authority_events();
        let prior_observations = runtime.cost.byte_observations();
        let prior_root = runtime.create_checkpoint().await.root;

        for (version, expected_calls, diagnostic) in [
            (PREPARED_PROGRAM_ABI_V1, 1, "rejected preparation"),
            (PREPARED_PROGRAM_ABI_V1 + 1, 0, "Unsupported frontend ABI"),
        ] {
            let frontend =
                CountingFrontend::new(version, Err(PreparationError::new("rejected preparation")));
            let result = runtime
                .evaluate_with_frontend(
                    &frontend,
                    PROVIDER_SOURCE,
                    Cost::create(77, "must not reset"),
                    environment(),
                    random(),
                )
                .await
                .expect("preparation rejection uses the normal result envelope");
            frontend.assert_calls(1, expected_calls);
            assert_eq!(result.cost, Cost::create(0, "parse failure"));
            assert!(matches!(
                result.errors.as_slice(),
                [InterpreterError::ParserError(message)] if message.contains(diagnostic)
            ));
            assert!(result.mergeable.is_empty());
            assert_eq!(runtime.cost.total_cost(), prior_cost);
            assert_eq!(runtime.cost.get(), prior_remaining);
            assert_eq!(runtime.cost.signature(), prior_signature);
            assert_eq!(runtime.get_cost_log(), prior_log);
            assert_eq!(runtime.cost.native_phlo_usage(), Some(prior_native.used));
            assert_eq!(runtime.cost.authority_events(), prior_authority);
            assert_eq!(runtime.cost.byte_observations(), prior_observations);
            assert_recording_unchanged(&prior_native, &runtime);
            assert_eq!(runtime.create_checkpoint().await.root, prior_root);
        }
    })
    .await;
}

#[tokio::test]
async fn negative_budget_rejects_before_frontend_access_and_prepared_execution() {
    with_cost_runtime(|mut runtime| async move {
        establish_prior_state(&mut runtime).await;
        let prior_cost = runtime.cost.total_cost();
        let prior_remaining = runtime.cost.get();
        let prior_signature = runtime.cost.signature();
        let prior_log = runtime.get_cost_log();
        let prior_native = runtime.cost.native_budget_recording().unwrap().unwrap();
        let prior_authority = runtime.cost.authority_events();
        let prior_observations = runtime.cost.byte_observations();
        let prior_root = runtime.create_checkpoint().await.root;
        let frontend = CountingFrontend::new(0, Ok(prepared("@1!(2)")));
        for initial in [-1, i64::MIN] {
            let from_source = runtime
                .evaluate_with_frontend(
                    &frontend,
                    PROVIDER_SOURCE,
                    Cost::create(initial, "negative"),
                    environment(),
                    random(),
                )
                .await
                .expect("negative source budget rejection");
            let from_prepared = runtime
                .evaluate_prepared(
                    prepared("@1!(2)"),
                    Cost::create(initial, "negative"),
                    random(),
                )
                .await
                .expect("negative prepared budget rejection");
            for result in [from_source, from_prepared] {
                assert_eq!(result.cost, Cost::create(0, "invalid initial phlo"));
                assert!(matches!(result.errors.as_slice(),
                    [InterpreterError::IllegalArgumentError(message)]
                    if message == &format!("Initial phlo must be non-negative, got {initial}")));
                assert!(result.mergeable.is_empty());
            }
            frontend.assert_calls(0, 0);
            assert_eq!(runtime.cost.total_cost(), prior_cost);
            assert_eq!(runtime.cost.get(), prior_remaining);
            assert_eq!(runtime.cost.signature(), prior_signature);
            assert_eq!(runtime.get_cost_log(), prior_log);
            assert_eq!(runtime.cost.native_phlo_usage(), Some(prior_native.used));
            assert_eq!(runtime.cost.authority_events(), prior_authority);
            assert_eq!(runtime.cost.byte_observations(), prior_observations);
            assert_recording_unchanged(&prior_native, &runtime);
            assert_eq!(runtime.create_checkpoint().await.root, prior_root);
        }
    })
    .await;
}

#[tokio::test]
async fn explicit_frontend_does_not_reparse_source_and_uses_native_metering() {
    with_cost_runtime(|mut runtime| async move {
        let frontend =
            CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(prepared("{% @1!(2) %}[alice]")));
        let program = prepare_program(&frontend, PROVIDER_SOURCE, environment())
            .expect("explicit source admission");
        let result = runtime
            .evaluate_prepared_with_native_phlo(
                program,
                PROVIDER_SOURCE.len() as u64,
                random(),
                None,
                native_config(),
            )
            .await
            .expect("explicit frontend deployment");
        frontend.assert_calls(1, 1);
        assert_native_charge(&result);
        let contents = runtime.get_hot_changes().await;
        let row = contents
            .get(&vec![scalar(1)])
            .expect("actual channel write");
        assert_eq!(row.data.len(), 1);
        assert_eq!(row.data[0].a.pars, vec![scalar(2)]);
    })
    .await;
}

#[tokio::test]
async fn successful_prepared_admission_resets_budget() {
    with_cost_runtime(|mut runtime| async move {
        establish_prior_state(&mut runtime).await;
        let signature = runtime.cost.signature();
        let root = runtime.create_checkpoint().await.root;
        for initial in [0, 11, i64::MAX] {
            let result = runtime
                .evaluate_prepared(prepared("Nil"), Cost::create(initial, "reset"), random())
                .await
                .expect("prepared Nil admission");
            assert!(result.errors.is_empty());
            assert_eq!(result.cost.value, 0);
            assert_eq!(runtime.cost.get().value, initial);
            assert_eq!(runtime.cost.signature(), signature);
            assert_eq!(runtime.cost.native_phlo_usage(), None);
            assert!(runtime.cost.native_budget_recording().unwrap().is_none());
            assert!(result.mergeable.is_empty());
            assert_eq!(runtime.create_checkpoint().await.root, root);
        }
    })
    .await;
}

#[tokio::test]
async fn prepared_and_source_paths_preserve_results_costs_rng_and_state() {
    for (source, budget) in [
        ("Nil", 0),
        ("@1!(2)", 1),
        ("new name in { @0!(*name) }", 1),
        ("new x in { x!(7) | for (@value <- x) { @0!(value) } }", 10),
        ("@1!(1) | @2!(2)", 1),
        ("@2!(3.noSuchMethod())", 10),
    ] {
        let (legacy_result, legacy_root, legacy_signature) =
            with_runtime("prepared-source-oracle-", |mut runtime| async move {
                runtime.cost.set_deploy_signature(&[29; 64]);
                let result = runtime
                    .evaluate(
                        source,
                        Cost::create(budget, "fixture"),
                        HashMap::new(),
                        random(),
                    )
                    .await
                    .expect("source result");
                (
                    result,
                    runtime.create_checkpoint().await.root,
                    runtime.cost.signature(),
                )
            })
            .await;
        with_runtime("prepared-source-comparison-", |mut runtime| async move {
            runtime.cost.set_deploy_signature(&[29; 64]);
            let result = runtime
                .evaluate_prepared(prepared(source), Cost::create(budget, "fixture"), random())
                .await
                .expect("prepared result");
            assert_eq!(result.cost, legacy_result.cost, "cost for {source}");
            assert_eq!(
                result
                    .errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                legacy_result
                    .errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "errors for {source}",
            );
            assert_eq!(
                result.mergeable, legacy_result.mergeable,
                "merge tracking for {source}"
            );
            assert_eq!(
                runtime.cost.signature(),
                legacy_signature,
                "signature for {source}"
            );
            assert_eq!(
                runtime.create_checkpoint().await.root,
                legacy_root,
                "RSpace/RNG for {source}"
            );
        })
        .await;
    }
}

#[tokio::test]
async fn raw_prepared_failure_and_source_wrapper_keep_distinct_checkpoint_ownership() {
    for raw_prepared in [false, true] {
        with_runtime("prepared-checkpoint-", |mut runtime| async move {
            runtime.cost.set_deploy_signature(&[29; 64]);
            let prior = runtime
                .evaluate(
                    "@\"prior-state\"!(7)",
                    Cost::create(9, "legacy prior state"),
                    HashMap::new(),
                    random(),
                )
                .await
                .expect("legacy prior state");
            assert!(prior.errors.is_empty(), "{:?}", prior.errors);
            assert_eq!(runtime.reducer.space.codec_epoch(), EventCodecEpoch::Legacy);
            let before = runtime.create_checkpoint().await.root;
            let source = "for (_ <- @\"prior-state\") { @2!(3.noSuchMethod()) }";
            let result = if raw_prepared {
                runtime
                    .evaluate_prepared(prepared(source), Cost::create(9, "raw"), random())
                    .await
            } else {
                runtime
                    .evaluate_with_phlo(source, Cost::create(9, "wrapper"))
                    .await
            }
            .expect("evaluation error remains in the result envelope");
            assert!(!result.errors.is_empty());
            assert_eq!(result.cost.value, 0);
            let after = runtime.create_checkpoint().await.root;
            assert_eq!(before == after, !raw_prepared);
        })
        .await;
    }
}

#[tokio::test]
async fn prepared_admission_preserves_explicit_unmetered_mode() {
    with_runtime("prepared-mode-", |runtime| async move {
        for unmetered in [false, true] {
            runtime.cost.set_unmetered(unmetered);
            let result = runtime
                .evaluate_prepared(prepared("Nil"), Cost::create(7, "mode"), random())
                .await
                .expect("mode result");
            assert!(result.errors.is_empty());
            assert_eq!(runtime.cost.is_unmetered(), unmetered);
            assert_eq!(
                runtime.cost.get().value,
                if unmetered { i64::MAX } else { 7 }
            );
            assert_eq!(result.cost.value, 0);
        }
    })
    .await;
}
