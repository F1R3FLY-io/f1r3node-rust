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
use super::accounting::costs::Cost;
use super::accounting::{RuntimeBudget, SignedProcess};
use super::compiler::compiler::Compiler;
use super::errors::InterpreterError;
use super::metrics_constants::{
    INJ_ATTEMPT_BUILD_NORMALIZED_TERM_TIME_METRIC, INJ_ATTEMPT_REDUCE_TERM_TIME_METRIC,
    INTERPRETER_METRICS_SOURCE,
};
use super::reduce::DebruijnInterpreter;

//See rholang/src/main/scala/coop/rchain/rholang/interpreter/Interpreter.scala

// NOTE: Manual marks are used instead of trace_i() for async operations.
// This is the correct pattern for async code and matches Scala's Span[F].traceI() semantics.
#[derive(Clone, Debug, Default)]
pub struct EvaluateResult {
    pub cost: Cost,
    pub errors: Vec<InterpreterError>,
    pub mergeable: HashMap<Par, MergeType>,
    pub authority_events: Vec<AuthorityEvent<[u8; 32]>>,
    pub authority_byte_events: Vec<AuthorityByteEvent>,
    pub authority_realized: ResourceMultiset<[u8; 32]>,
    pub authority_stack_births: Vec<AuthorityStackBirth>,
    pub quantitative_byte_cost: u64,
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
                mergeable: HashMap::new(),
                authority_events: Vec::new(),
                authority_byte_events: Vec::new(),
                authority_realized: ResourceMultiset::default(),
                authority_stack_births: Vec::new(),
                quantitative_byte_cost: 0,
            });
        }

        let evaluation_result: Result<EvaluateResult, InterpreterError> = {
            // Phase: build-normalized-term — parse the source string into an AST.
            let parsed = {
                let phase_start = Instant::now();
                event!(
                    Level::DEBUG,
                    mark = "started-build-normalized-term",
                    "inj_attempt"
                );
                let result = match Compiler::source_to_adt_with_normalizer_env(term, normalizer_env)
                {
                    Ok(p) => {
                        event!(
                            Level::DEBUG,
                            mark = "finished-build-normalized-term",
                            "inj_attempt"
                        );
                        Ok(p)
                    }
                    Err(e) => {
                        event!(
                            Level::DEBUG,
                            mark = "failed-build-normalized-term",
                            "inj_attempt"
                        );
                        Err(self.handle_error(InterpreterError::ParserError(e.to_string())))
                    }
                };
                metrics::histogram!(
                    INJ_ATTEMPT_BUILD_NORMALIZED_TERM_TIME_METRIC,
                    "source" => INTERPRETER_METRICS_SOURCE
                )
                .record(phase_start.elapsed().as_secs_f64());
                match result {
                    Ok(p) => p,
                    Err(err) => return err,
                }
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
                self.c.reset_from_signed_process(&signed_process);
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
            let _comm_accounting_scope = self.c.enter_comm_accounting_scope();
            let reduce_result = reducer.inj(parsed, rand).await;
            metrics::histogram!(
                INJ_ATTEMPT_REDUCE_TERM_TIME_METRIC,
                "source" => INTERPRETER_METRICS_SOURCE
            )
            .record(phase_start.elapsed().as_secs_f64());
            match reduce_result {
                Ok(()) => {
                    event!(Level::DEBUG, mark = "finished-reduce-term", "inj_attempt");
                    let mergeable_channels = { self.merge_chs.read().await.clone() };

                    Ok(EvaluateResult {
                        cost: self.c.total_cost(),
                        errors: Vec::new(),
                        mergeable: mergeable_channels,
                        authority_events: self.c.authority_events(),
                        authority_byte_events: self.c.authority_byte_events(),
                        authority_realized: self.c.authority_realized(),
                        authority_stack_births: self.c.authority_stack_births(),
                        quantitative_byte_cost: self.c.quantitative_byte_cost(),
                    })
                }
                Err(e) => {
                    event!(Level::DEBUG, mark = "failed-reduce-term", "inj_attempt");
                    self.handle_error(e)
                }
            }
        };
        evaluation_result
    }
}

impl InterpreterImpl {
    pub fn new(
        cost: RuntimeBudget,
        merge_chs: Arc<RwLock<HashMap<Par, MergeType>>>,
    ) -> InterpreterImpl {
        InterpreterImpl { c: cost, merge_chs }
    }

    fn handle_error(&self, error: InterpreterError) -> Result<EvaluateResult, InterpreterError> {
        if matches!(&error, InterpreterError::ParserError(_)) {
            return Ok(EvaluateResult {
                cost: Cost::create(0, "parse failure"),
                errors: vec![error],
                mergeable: HashMap::new(),
                authority_events: Vec::new(),
                authority_byte_events: Vec::new(),
                authority_realized: ResourceMultiset::default(),
                authority_stack_births: Vec::new(),
                quantitative_byte_cost: 0,
            });
        }

        self.c.rollback_authority_stack_transfers()?;
        let errors = match error {
            InterpreterError::AggregateError { interpreter_errors } => interpreter_errors,
            error => vec![error],
        };
        Ok(EvaluateResult {
            cost: self.c.total_cost(),
            errors,
            mergeable: HashMap::new(),
            authority_events: self.c.authority_events(),
            authority_byte_events: self.c.authority_byte_events(),
            authority_realized: self.c.authority_realized(),
            authority_stack_births: self.c.authority_stack_births(),
            quantitative_byte_cost: self.c.quantitative_byte_cost(),
        })
    }
}
