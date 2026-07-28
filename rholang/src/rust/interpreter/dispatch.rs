use std::sync::{Arc, OnceLock, Weak};

use smallvec::SmallVec;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{ListParWithRandom, Par, TaggedContinuation};
use prost::Message;

use super::env::Env;
use super::errors::InterpreterError;
use super::reduce::DebruijnInterpreter;
use super::system_processes::{non_deterministic_ops, RhoDispatchMap};
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
    pub reducer: Arc<OnceLock<Weak<DebruijnInterpreter>>>,
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
/// # The ceiling
///
/// `prost` enforces `RECURSION_LIMIT = 100` nested-message levels
/// (`prost-0.14.3/src/lib.rs:30`). It is **not `pub`**, and its only knob
/// (`no-recursion-limit`) *removes* the limit and is set in no `Cargo.toml` or
/// `Cargo.lock` — so it cannot be configured here. The `Par → Expr → EList →
/// Par` spine costs three message levels per bracket, so this decode **accepts
/// term depth 33 and returns `Err` at 34**, measured, in both profiles.
/// `encode` has no matching limit, so those bytes were writable.
///
/// # ⚠ Why this is consensus-class, and where the asymmetry actually is
///
/// It is in the **supply**, not in this expression.
///
/// * **Play.** `Produce::create` sets `output_value: vec![]` and
///   `RSpace::locked_produce` returns exactly that freshly-created `Produce`, so
///   on the proposer the slice handed here is EMPTY, nothing is decoded, and
///   then the bytes are written into the block.
/// * **Replay.** `ReplayRSpace::locked_produce` returns the `Produce` **from
///   the trace**, whose `output_value` came from the block, so on the validator
///   this decode runs on real bytes.
///
/// `ProduceEventProto.outputValue` is `repeated bytes`
/// (`models/src/main/protobuf/CasperMessage.proto:393`), so the block body
/// decodes without descending into them and cannot reject them; the decode here
/// is a fresh top-level one with the full budget, and therefore sits at the
/// **bare** 33/34 ceiling rather than any wrapped one.
///
/// An `Err` here becomes an entry in `EvaluateResult::errors` ⇒
/// `eval_successful = false` (`casper/src/rust/rholang/replay_runtime.rs:427`)
/// ⇒ the `is_failed != !eval_successful` check at `:443` ⇒
/// `ReplayFailure::ReplayStatusMismatch` ⇒ `handle_errors` returns
/// `Either::Right(None)` ⇒ `InvalidBlock::InvalidTransaction`, which
/// `casper/src/rust/block_status.rs::is_slashable` answers **true** for.
///
/// # ⚠ NOT reachable from a deploy — and that is now CHECKED, not assumed
///
/// `output_value` is written from one site, `reduce.rs`'s
/// `mark_as_non_deterministic`, gated on `non_deterministic_ops()`.
/// `rholang/tests/output_value_write_side_reachability.rs` drives **every**
/// member of that set through a real deploy and measures what it writes: the
/// deepest is `rho:ollama:models` at term depth **1**, thirty-two levels below
/// the ceiling. ★ That file takes its denominator from `non_deterministic_ops()`
/// itself, so a NINTH operation fails the suite until its depth is recorded —
/// which is the difference between a guard and a sentence about eight function
/// bodies.
///
/// # Executable
///
/// `rholang/tests/replay_output_value_depth_ceiling.rs` (red on replay, green
/// on play, one bool apart) · `models/tests/par_prost_depth_ceiling.rs` (the
/// boundary, per envelope) ·
/// `rholang/tests/output_value_write_side_reachability.rs` (the write side).
/// Analysis: `docs/design/audits/theta-depth-traversals-2026-07-26.md` §7.3.
/// ⚠ The sibling ceiling `COLLECTION_DEPTH_LIMIT = 32`
/// (`models/src/rust/canonical_path.rs`) is explicitly anchored to this one:
/// they move together or not at all.
pub fn decode_non_deterministic_output(
    previous_output: &[Vec<u8>],
) -> Result<Vec<Par>, InterpreterError> {
    let mut decoded = Vec::with_capacity(previous_output.len());
    for bytes in previous_output {
        decoded.push(
            Par::decode(&bytes[..]).map_err(|e| InterpreterError::DecodeError(e.to_string()))?,
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
        // println!("\ndispatcher dispatch");
        // println!("continuation: {:?}", continuation);
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
                    reducer.eval_with_path(body, &env, merged_rand, path).await?;

                    Ok(DispatchType::DeterministicCall)
                }
                TaggedCont::ScalaBodyRef(_ref) => {
                    let is_non_deterministic = non_deterministic_ops().contains(&_ref);
                    // println!("self {:p}", self);
                    let dispatch_table = self._dispatch_table.read().await;
                    // println!(
                    //     "dispatch_table at ScalaBodyRef: {:?}",
                    //     dispatch_table.keys()
                    // );
                    match dispatch_table.get(&_ref) {
                        Some(f) => {
                            match f((data_list, is_replay, previous_output)).await {
                                Ok(output) => RholangAndScalaDispatcher::dispatch_type(
                                    is_non_deterministic,
                                    output,
                                ),
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
