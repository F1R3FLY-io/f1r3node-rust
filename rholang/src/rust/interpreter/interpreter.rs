use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use tokio::sync::RwLock;
use tracing::{event, Level};

use super::accounting::authority::{
    AuthorityByteEvent, AuthorityEvent, AuthorityStackBirth, ResourceMultiset,
};
use super::accounting::byte_receipts::ByteObservationSnapshot;
use super::accounting::costs::Cost;
use super::accounting::economic_failure::{classify_errors, EvaluationFailureSummary};
use super::accounting::phlo_execution::PhloFailure;
use super::accounting::{
    NativeBudgetRecording, NativeReplayAccountingSnapshot, NativeRuntimeConfig, RuntimeBudget,
    SignedProcess,
};
use super::compiler::compiler::Compiler;
use super::errors::InterpreterError;
use super::metrics_constants::{
    INJ_ATTEMPT_BUILD_NORMALIZED_TERM_TIME_METRIC, INJ_ATTEMPT_REDUCE_TERM_TIME_METRIC,
    INTERPRETER_METRICS_SOURCE,
};
use super::reduce::{DebruijnInterpreter, ReducerCore};

#[cfg(test)]
#[path = "interpreter_source_tests.rs"]
mod source_tests;

//See rholang/src/main/scala/coop/rchain/rholang/interpreter/Interpreter.scala

// NOTE: Manual marks are used instead of trace_i() for async operations.
// This is the correct pattern for async code and matches Scala's Span[F].traceI() semantics.
#[derive(Clone, Debug, Default)]
pub struct EvaluateResult {
    pub cost: Cost,
    pub errors: Vec<InterpreterError>,
    pub economic_failures: EvaluationFailureSummary,
    pub mergeable: HashMap<Par, MergeType>,
    pub authority_events: Vec<AuthorityEvent<[u8; 32]>>,
    pub authority_byte_events: Vec<AuthorityByteEvent>,
    pub byte_observations: ByteObservationSnapshot,
    pub authority_realized: ResourceMultiset<[u8; 32]>,
    pub authority_stack_births: Vec<AuthorityStackBirth>,
    pub quantitative_byte_cost: u64,
    pub native_phlo_usage: Option<u64>,
    pub native_budget_recording: Option<NativeBudgetRecording>,
    pub native_operation_recording:
        Option<std::sync::Arc<[super::accounting::NativeOperationRecord]>>,
}

#[allow(async_fn_in_trait)]
pub trait Interpreter {
    async fn inj_attempt(
        &self,
        reducer: &DebruijnInterpreter,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
    ) -> Result<EvaluateResult, InterpreterError>;
}

pub struct InterpreterImpl {
    c: RuntimeBudget,
    merge_chs: Arc<RwLock<HashMap<Par, MergeType>>>,
}

impl Interpreter for InterpreterImpl {
    async fn inj_attempt(
        &self,
        reducer: &DebruijnInterpreter,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
    ) -> Result<EvaluateResult, InterpreterError> {
        self.inj_attempt_inner(
            reducer,
            term,
            initial_phlo,
            normalizer_env,
            rand,
            authority_allocation,
            None,
        )
        .await
    }
}

impl InterpreterImpl {
    pub async fn inj_attempt_with_native_phlo(
        &self,
        reducer: &ReducerCore,
        term: &str,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
        config: NativeRuntimeConfig,
    ) -> Result<EvaluateResult, InterpreterError> {
        self.inj_attempt_inner(
            reducer,
            term,
            Cost::create(0, "native resource reservation"),
            normalizer_env,
            rand,
            authority_allocation,
            Some(config),
        )
        .await
    }

    async fn inj_attempt_inner(
        &self,
        reducer: &ReducerCore,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
        native: Option<NativeRuntimeConfig>,
    ) -> Result<EvaluateResult, InterpreterError> {
        // Using tracing events for async context
        // Scala spans: "set-initial-cost", "build-normalized-term", "reduce-term"
        // Implemented as debug events since this is an async function
        if initial_phlo.value < 0 {
            return Ok(EvaluateResult {
                cost: Cost::create(0, "invalid initial phlo"),
                errors: vec![InterpreterError::IllegalArgumentError(format!(
                    "Initial phlo must be non-negative, got {}",
                    initial_phlo.value
                ))],
                economic_failures: EvaluationFailureSummary::single(PhloFailure::Unclassified),
                mergeable: HashMap::new(),
                authority_events: Vec::new(),
                authority_byte_events: Vec::new(),
                byte_observations: ByteObservationSnapshot::default(),
                authority_realized: ResourceMultiset::default(),
                authority_stack_births: Vec::new(),
                quantitative_byte_cost: 0,
                native_phlo_usage: None,
                native_budget_recording: None,
                native_operation_recording: None,
            });
        }

        let native_mode = native.is_some();
        if let Some(native) = native {
            self.c.reset_for_native_execution(native)?;
        }
        let evaluation_result: Result<EvaluateResult, InterpreterError> = {
            // Phase: build-normalized-term — parse the source string into an AST.
            let parsed = match Self::parse_source(term, normalizer_env) {
                Ok(parsed) => parsed,
                Err(error) => return self.handle_error(error),
            };
            // Trace: set-initial-cost (matching Scala's Span[F].traceI("set-initial-cost"))
            let parsed = {
                event!(
                    Level::DEBUG,
                    mark = "started-set-initial-cost",
                    "inj_attempt"
                );
                let signed_process = SignedProcess::metered(
                    parsed,
                    self.c.signature(),
                    u64::try_from(initial_phlo.value).unwrap_or(0),
                );
                if !native_mode {
                    self.c.reset_from_signed_process(&signed_process);
                }
                if let Some(allocation) = authority_allocation {
                    self.c.install_authority_allocation(allocation);
                }
                event!(
                    Level::DEBUG,
                    mark = "finished-set-initial-cost",
                    "inj_attempt"
                );
                signed_process
                    .source_process()
                    .cloned()
                    .expect("metered deploy must retain source process")
            };
            // Reset mergeable-channel tracking before reducing the new term.
            {
                let mut merge_chs_lock = self.merge_chs.write().await;
                merge_chs_lock.clear();
            }
            // Phase: reduce-term — execute the parsed AST through RSpace.
            let phase_start = Instant::now();
            event!(Level::DEBUG, mark = "started-reduce-term", "inj_attempt");
            let _comm_accounting_scope =
                (!self.c.is_legacy()).then(|| self.c.enter_comm_accounting_scope());
            let (reduce_result, economic_failures) =
                reducer.inj_with_observation(parsed, rand).await;
            metrics::histogram!(
                INJ_ATTEMPT_REDUCE_TERM_TIME_METRIC,
                "source" => INTERPRETER_METRICS_SOURCE
            )
            .record(phase_start.elapsed().as_secs_f64());
            match reduce_result {
                Ok(()) => {
                    event!(Level::DEBUG, mark = "finished-reduce-term", "inj_attempt");
                    let mergeable_channels = { self.merge_chs.read().await.clone() };
                    self.capture_result(Vec::new(), economic_failures, mergeable_channels, None)
                }
                Err(e) => {
                    event!(Level::DEBUG, mark = "failed-reduce-term", "inj_attempt");
                    self.handle_observed_error(e, economic_failures)
                }
            }
        };
        if self.c.is_legacy() {
            evaluation_result.map(|result| legacy_failure_cost(result, initial_phlo))
        } else {
            evaluation_result
        }
    }
}

fn legacy_failure_cost(mut result: EvaluateResult, initial_phlo: Cost) -> EvaluateResult {
    result.cost = match result.errors.as_slice() {
        []
        | [InterpreterError::UserAbortError]
        | [InterpreterError::IfConditionTypeError { .. }] => result.cost,
        [InterpreterError::OperatorNotDefined { .. }]
        | [InterpreterError::OperatorExpectedError { .. }] => {
            Cost::create(0, "legacy parse failure")
        }
        _ => initial_phlo,
    };
    result
}

impl InterpreterImpl {
    pub fn parse_source(
        term: &str,
        normalizer_env: HashMap<String, Par>,
    ) -> Result<Par, InterpreterError> {
        let phase_start = Instant::now();
        event!(
            Level::DEBUG,
            mark = "started-build-normalized-term",
            "inj_attempt"
        );
        let result = Compiler::source_to_adt_with_normalizer_env(term, normalizer_env)
            .map_err(|error| InterpreterError::ParserError(error.to_string()));
        if result.is_ok() {
            event!(
                Level::DEBUG,
                mark = "finished-build-normalized-term",
                "inj_attempt"
            );
        } else {
            event!(
                Level::DEBUG,
                mark = "failed-build-normalized-term",
                "inj_attempt"
            );
        }
        metrics::histogram!(
            INJ_ATTEMPT_BUILD_NORMALIZED_TERM_TIME_METRIC,
            "source" => INTERPRETER_METRICS_SOURCE
        )
        .record(phase_start.elapsed().as_secs_f64());
        result
    }

    pub fn new(
        cost: RuntimeBudget,
        merge_chs: Arc<RwLock<HashMap<Par, MergeType>>>,
    ) -> InterpreterImpl {
        InterpreterImpl { c: cost, merge_chs }
    }

    fn handle_error(&self, error: InterpreterError) -> Result<EvaluateResult, InterpreterError> {
        let failures = classify_errors(std::slice::from_ref(&error))
            .unwrap_or_else(|_| EvaluationFailureSummary::single(PhloFailure::Platform));
        self.handle_observed_error(error, failures)
    }

    fn handle_observed_error(
        &self,
        error: InterpreterError,
        economic_failures: EvaluationFailureSummary,
    ) -> Result<EvaluateResult, InterpreterError> {
        self.handle_observed_error_with_evidence(error, economic_failures, None)
    }

    fn handle_observed_error_with_evidence(
        &self,
        error: InterpreterError,
        economic_failures: EvaluationFailureSummary,
        evidence: Option<NativeReplayAccountingSnapshot>,
    ) -> Result<EvaluateResult, InterpreterError> {
        if matches!(&error, InterpreterError::ParserError(_)) {
            return Ok(EvaluateResult {
                cost: Cost::create(0, "parse failure"),
                errors: vec![error],
                economic_failures,
                mergeable: HashMap::new(),
                authority_events: Vec::new(),
                authority_byte_events: Vec::new(),
                byte_observations: ByteObservationSnapshot::default(),
                authority_realized: ResourceMultiset::default(),
                authority_stack_births: Vec::new(),
                quantitative_byte_cost: 0,
                native_phlo_usage: None,
                native_budget_recording: None,
                native_operation_recording: None,
            });
        }

        self.c.rollback_authority_stack_transfers()?;
        let errors = match error {
            InterpreterError::AggregateError { interpreter_errors } => interpreter_errors,
            error => vec![error],
        };
        self.capture_result(errors, economic_failures, HashMap::new(), evidence)
    }

    pub(crate) fn complete_replay_result(
        &self,
        execution: Result<(), InterpreterError>,
        economic_failures: EvaluationFailureSummary,
        mergeable: HashMap<Par, MergeType>,
        evidence: NativeReplayAccountingSnapshot,
    ) -> Result<EvaluateResult, InterpreterError> {
        match execution {
            Ok(()) => self.capture_result(Vec::new(), economic_failures, mergeable, Some(evidence)),
            Err(error) => {
                self.handle_observed_error_with_evidence(error, economic_failures, Some(evidence))
            }
        }
    }

    fn capture_result(
        &self,
        errors: Vec<InterpreterError>,
        economic_failures: EvaluationFailureSummary,
        mergeable: HashMap<Par, MergeType>,
        evidence: Option<NativeReplayAccountingSnapshot>,
    ) -> Result<EvaluateResult, InterpreterError> {
        // Changed by D-O4 (DR-89): rows are copied only without evidence.
        // self.c.reserve_native_result_backing()?;
        let (
            byte_observations,
            native_phlo_usage,
            native_budget_recording,
            native_operation_recording,
        ) = match evidence {
            Some(evidence) => {
                let (recording, operations, observations) = evidence.into_parts();
                (
                    observations,
                    Some(recording.used),
                    Some(recording),
                    Some(operations),
                )
            }
            None => (
                self.c.byte_observations(),
                self.c.native_phlo_usage(),
                self.c.native_budget_recording()?,
                self.c.native_operation_recording()?,
            ),
        };
        let authority_byte_events = byte_observations.legacy_events();
        let quantitative_byte_cost = authority_byte_events
            .iter()
            .try_fold(0_u64, |sum, event| sum.checked_add(event.amount))
            .expect("validated byte-cost trace overflow");
        Ok(EvaluateResult {
            cost: self.c.total_cost(),
            errors,
            economic_failures,
            mergeable,
            authority_events: self.c.authority_events(),
            authority_byte_events,
            byte_observations,
            authority_realized: self.c.authority_realized(),
            authority_stack_births: self.c.authority_stack_births(),
            quantitative_byte_cost,
            native_phlo_usage,
            native_budget_recording,
            native_operation_recording,
        })
    }
}
