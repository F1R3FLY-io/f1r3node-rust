use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use tokio::sync::RwLock;
use tracing::{event, Level};

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
        match error {
            // Source that fails before a metered source state exists consumes no token cost.
            InterpreterError::ParserError(_) => Ok(EvaluateResult {
                cost: Cost::create(0, "parse failure"),
                errors: vec![error],
                mergeable: HashMap::new(),
            }),

            // For Out Of Phlogistons error initial cost is used because evaluated cost can be higher
            // all phlos are consumed
            InterpreterError::OutOfPhlogistonsError => Ok(EvaluateResult {
                cost: self.c.total_cost(),
                errors: vec![error],
                mergeable: HashMap::new(),
            }),

            // User triggered abort - execution failed, return cost consumed so far
            InterpreterError::UserAbortError => Ok(EvaluateResult {
                cost: self.c.total_cost(),
                errors: vec![error],
                mergeable: HashMap::new(),
            }),

            // InterpreterError(s) - multiple errors are result of parallel execution
            InterpreterError::AggregateError { interpreter_errors } => Ok(EvaluateResult {
                cost: self.c.total_cost(),
                errors: interpreter_errors,
                mergeable: HashMap::new(),
            }),

            // These malformed forms can escape parser classification and fail
            // during reduction. They still happen before a valid source-token
            // transition, so they return a parse-style zero-cost result.
            InterpreterError::OperatorNotDefined {
                op: _,
                other_type: _,
            } => Ok(EvaluateResult {
                cost: Cost::create(0, "parse failure"),
                errors: vec![error],
                mergeable: HashMap::new(),
            }),

            // Same admission boundary as OperatorNotDefined: no source-token
            // transition has fired, so no token cost is consumed.
            InterpreterError::OperatorExpectedError {
                op: _,
                expected: _,
                other_type: _,
            } => Ok(EvaluateResult {
                cost: Cost::create(0, "parse failure"),
                errors: vec![error],
                mergeable: HashMap::new(),
            }),

            // InterpreterError is returned as a result
            _ => Ok(EvaluateResult {
                cost: self.c.total_cost(),
                errors: vec![error],
                mergeable: HashMap::new(),
            }),
        }
    }
}
