use std::sync::{Arc, OnceLock, Weak};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{ListParWithRandom, Par, TaggedContinuation};
use prost::Message;
use smallvec::SmallVec;

use super::env::Env;
use super::errors::InterpreterError;
use super::reduce::ReducerCore;
// Changed by DR-114: native funded execution has its own recorded set.
// use super::system_processes::{non_deterministic_ops, RhoDispatchMap};
use super::system_processes::{is_native_recorded_op, non_deterministic_ops, RhoDispatchMap};
use super::unwrap_option_safe;

pub fn build_env(data_list: Vec<ListParWithRandom>) -> Env<Par> {
    let pars: Vec<Par> = data_list.into_iter().flat_map(|list| list.pars).collect();
    let mut env = Env::new();

    for par in pars {
        env = env.put(par);
    }

    env
}

#[derive(Clone)]
pub struct RholangAndScalaDispatcher {
    pub _dispatch_table: RhoDispatchMap,
    pub reducer: Arc<OnceLock<Weak<ReducerCore>>>,
}

pub type RhoDispatch = Arc<RholangAndScalaDispatcher>;

pub enum DispatchType {
    NonDeterministicCall(Vec<Vec<u8>>),
    /// Indicates a non-deterministic process failed during execution.
    /// Contains the error wrapped for proper replay handling.
    FailedNonDeterministicCall(InterpreterError),
    DeterministicCall,
    Skip,
}

/// ★★ THE CONSENSUS-CLASS `Par` READ — the trace's `output_value`, decoded.
///
/// This is the inverse of [`RholangAndScalaDispatcher::dispatch_type`], which
/// sits thirty lines below it in this file. ★ **The pair lives in one module on
/// purpose**: it was previously spelled out three times — twice in `reduce.rs`
/// (`continue_produce_process`, `continue_consume_process`) and once in
/// `contract_call.rs` — so the ceiling described below had three
/// implementations and any future change to it had three edit sites, one of
/// which could be missed. There is now one.
///
/// # Stack-safe boundary
///
/// The old derived prost reader stopped at 100 nested-message levels, creating
/// a play/replay asymmetry because `ProduceEventProto.outputValue` is opaque
/// `repeated bytes`: play could write a `Par` that replay could not read. This
/// boundary now calls the generated protobuf PDA. Its work and continuation
/// stacks live on the heap, it imposes no term-depth limit, and its result is
/// byte-for-byte differential-tested against prost on the former accepted
/// domain.
///
/// `models/tests/par_protobuf_stack_safety.rs` exercises depth 4,096 through
/// `Par` and every production envelope on a fixed 256 KiB stack.
/// `rholang/tests/replay_output_value_stack_safety.rs` splices the same deep
/// bytes through this real play/replay boundary and requires both sides to
/// agree. `rholang/tests/output_value_write_side_reachability.rs` separately
/// keeps the set of non-deterministic producers and their returned shapes
/// enumerated.
pub fn decode_non_deterministic_output(
    previous_output: &[Vec<u8>],
) -> Result<Vec<Par>, InterpreterError> {
    let mut decoded = Vec::with_capacity(previous_output.len());
    for bytes in previous_output {
        decoded.push(
            models::rust::rholang::protobuf_decoder::decode_par(&bytes[..])
                .map_err(|e| InterpreterError::DecodeError(e.to_string()))?,
        );
    }
    Ok(decoded)
}

impl RholangAndScalaDispatcher {
    pub async fn dispatch(
        &self,
        continuation: TaggedContinuation,
        data_list: Vec<ListParWithRandom>,
        is_replay: bool,
        previous_output: Vec<Par>,
        // Coordinate of the dispatching continuation (async counter driver): forwarded to the
        // ParBody re-entry `reducer.eval` so the recursion's error coordinates continue the tree.
        // Inherent method (no trait change). Display-only (NOT consensus).
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        let system_body = continuation
            .cost_authority
            .as_ref()
            .is_some_and(crate::rust::interpreter::accounting::authority::is_system_seal);
        match continuation.tagged_cont {
            Some(cont) => match cont {
                TaggedCont::ParBody(par_with_rand) => {
                    let env = build_env(data_list.clone());
                    let mut randoms =
                        vec![Blake2b512Random::from_bytes(&par_with_rand.random_state)];
                    randoms.extend(
                        data_list
                            .iter()
                            .map(|p| Blake2b512Random::from_bytes(&p.random_state)),
                    );

                    let reducer = self
                        .reducer
                        .get()
                        .and_then(|weak| weak.upgrade())
                        .ok_or_else(|| {
                            InterpreterError::BugFoundError("Reducer not initialized".to_string())
                        })?;
                    let body = unwrap_option_safe(par_with_rand.body)?;
                    let merged_rand = Blake2b512Random::merge(randoms);
                    reducer
                        .eval_continuation(body, env, merged_rand, system_body, path)
                        .await?;

                    Ok(DispatchType::DeterministicCall)
                }
                TaggedCont::ScalaBodyRef(_ref) => {
                    // Changed by DR-114: native funded execution records and
                    // replays its own set of external-service processes.
                    // let is_non_deterministic = non_deterministic_ops().contains(&_ref);
                    let native = self.native_execution_active();
                    let is_non_deterministic = if native {
                        is_native_recorded_op(_ref)
                    } else {
                        non_deterministic_ops().contains(&_ref)
                    };
                    let dispatch_table = self._dispatch_table.read().await;
                    match dispatch_table.get(&_ref) {
                        Some(f) => {
                            match f((data_list, is_replay, previous_output)).await {
                                Ok(output) => RholangAndScalaDispatcher::dispatch_type(
                                    is_non_deterministic,
                                    output,
                                ),
                                // Added by DR-114: in native funded execution a
                                // recorded reply that cannot be produced keeps its
                                // record, and a malformed system-process call is a
                                // classified user failure (§4.1 option A).
                                // Changed by the merge of feature/cost-accounted-rho:
                                // F1R3Lang's `InterpreterError` has an iterative `Drop`,
                                // so a pattern cannot move the message out of it
                                // (E0509). The arm takes the message by reference.
                                // Err(e) if native => match e {
                                //     e @ InterpreterError::ProduceFailureWithOutput { .. }
                                //         if is_non_deterministic =>
                                //     {
                                //         Ok(DispatchType::FailedNonDeterministicCall(e))
                                //     }
                                //     InterpreterError::IllegalArgumentError(message) => {
                                //         Err(InterpreterError::SystemProcessShapeError(message))
                                //     }
                                //     e => Err(e),
                                // },
                                Err(mut e) if native => match e {
                                    e @ InterpreterError::ProduceFailureWithOutput { .. }
                                        if is_non_deterministic =>
                                    {
                                        Ok(DispatchType::FailedNonDeterministicCall(e))
                                    }
                                    InterpreterError::IllegalArgumentError(ref mut message) => {
                                        Err(InterpreterError::SystemProcessShapeError(
                                            std::mem::take(message),
                                        ))
                                    }
                                    e => Err(e),
                                },
                                Err(e) if is_non_deterministic => {
                                    // Non-deterministic process failed - return FailedNonDeterministicCall
                                    // so the produce event can be marked as failed for replay safety
                                    Ok(DispatchType::FailedNonDeterministicCall(e))
                                }
                                Err(e) => Err(e),
                            }
                        }
                        None => Err(InterpreterError::BugFoundError(format!(
                            "dispatch: no function for {}",
                            _ref,
                        ))),
                    }
                }
            },
            None => Ok(DispatchType::Skip),
        }
    }

    /// Added by DR-114: whether the reducer runs native funded execution.
    pub(crate) fn native_execution_active(&self) -> bool {
        self.reducer
            .get()
            .and_then(|weak| weak.upgrade())
            .is_some_and(|reducer| reducer.metering.budget().native_execution_active())
    }

    fn dispatch_type(
        is_non_deterministic: bool,
        output: Vec<Par>,
    ) -> Result<DispatchType, InterpreterError> {
        if is_non_deterministic {
            Ok(DispatchType::NonDeterministicCall(
                output.iter().map(|p| p.encode_to_vec()).collect(),
            ))
        } else {
            Ok(DispatchType::DeterministicCall)
        }
    }
}
