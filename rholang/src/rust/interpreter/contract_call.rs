use std::pin::Pin;

use models::rhoapi::{ListParWithRandom, Par};
use prost::Message;

use super::dispatch::{DispatchType, RhoDispatch};
use super::errors::InterpreterError;
use super::rho_runtime::RhoISpace;

/**
 * This is a tool for unapplying the messages sent to the system contracts.
 *
 * The unapply returns (Producer, Seq[Par]).
 *
 * The Producer is the function with the signature (Seq[Par], Par) => F[Unit] which can be used to send a message
 * through a channel. The first argument with type Seq[Par] is the content of the message and the second argument is
 * the channel.
 *
 * Note that the random generator and the sequence number extracted from the incoming message are required for sending
 * messages back to the caller so they are given as the first argument list to the produce function.
 *
 * The Seq[Par] returned by unapply contains the message content and can be further unapplied as needed to match the
 * required signature.
 *
 * @param space the rspace instance
 * @param dispatcher the dispatcher
 *
 * See rholang/src/main/scala/coop/rchain/rholang/interpreter/ContractCall.scala
 */
pub struct ContractCall {
    pub space: RhoISpace,
    pub dispatcher: RhoDispatch,
}

pub type Producer = Box<
    dyn FnOnce(
            &[Par],
            &Par,
        )
            -> Pin<Box<dyn futures::Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>
        + Send,
>;

impl ContractCall {
    pub fn unapply(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Option<(Producer, bool, Vec<Par>, Vec<Par>)> {
        if contract_args.0.len() == 1 {
            let (args, rand, is_replay, previous) = (
                contract_args.0[0].pars.clone(),
                contract_args.0[0].random_state.clone(),
                contract_args.1,
                contract_args.2,
            );

            let space = self.space.clone();
            let dispatcher = self.dispatcher.clone();
            let produce = Box::new(move |values: &[Par], ch: &Par| {
                let space = space.clone();
                let rand = rand.clone();
                // clone inputs locally to satisfy ownership of underlying APIs
                let values_vec: Vec<Par> = values.to_vec();
                let ch_cloned: Par = ch.clone();
                Box::pin(async move {
                    let produce_result = space
                        .produce(
                            ch_cloned,
                            ListParWithRandom {
                                pars: values_vec,
                                random_state: rand,
                            },
                            false,
                        )
                        .await?;

                    let is_replay = space.is_replay().await;

                    let dispatch_result = match produce_result {
                        Some((cont, channels, produce)) => {
                            dispatcher
                                .dispatch(
                                    cont.continuation,
                                    channels.iter().map(|c| c.matched_datum.clone()).collect(),
                                    is_replay,
                                    // ★★ THE SAME CONSENSUS-CLASS `Par::decode` AS
                                    // `reduce.rs`'s `continue_produce_process` — the
                                    // system-contract path's copy of it, and the full
                                    // derivation lives at that call site.
                                    //
                                    // `prost` enforces `RECURSION_LIMIT = 100`
                                    // nested-message levels, `Par → Expr → EList → Par`
                                    // costs 3 per bracket, so this accepts term depth 33
                                    // and returns `Err` at 34 — while `encode` has no
                                    // matching limit, so those bytes were writable.
                                    //
                                    // ⚠ The asymmetry is in the SUPPLY, not here.
                                    // `Produce::create` sets `output_value: vec![]` and
                                    // `RSpace::locked_produce` returns exactly that, so
                                    // on the PROPOSER this iterator is empty;
                                    // `ReplayRSpace::locked_produce` returns the trace's
                                    // `Produce`, so on the VALIDATOR it decodes bytes
                                    // that came from the block. `ProduceEventProto
                                    // .outputValue` is `repeated bytes`, so the block
                                    // itself decodes fine and only replay does not.
                                    //
                                    // Executable:
                                    // `rholang/tests/replay_output_value_depth_ceiling.rs`,
                                    // `models/tests/par_prost_depth_ceiling.rs`.
                                    // Analysis:
                                    // `docs/design/audits/theta-depth-traversals-2026-07-26.md`
                                    // §7.3.
                                    produce
                                        .output_value
                                        .iter()
                                        .map(|p| {
                                            Par::decode(&p[..]).map_err(|e| {
                                                InterpreterError::DecodeError(e.to_string())
                                            })
                                        })
                                        .collect::<Result<Vec<_>, _>>()?,
                                    // System-contract producer: outside the deploy's parallel tree, so
                                    // its continuation eval starts at the empty coordinate.
                                    smallvec::SmallVec::new(),
                                )
                                .await
                        }

                        None => Ok(DispatchType::Skip),
                    };

                    match dispatch_result {
                        Ok(dispatch_type) => match dispatch_type {
                            // ★ The RETURN leg of the same ceiling: the bytes a
                            // non-deterministic op produced, read back as `Par`s.
                            // Same limit (33 accepts, 34 `Err`s), same missing
                            // counterpart on `encode`.
                            //
                            // ⚠ **This is where a NINTH non-deterministic operation
                            // would make the ceiling live.** The eight that exist
                            // today construct returns of depth ≤ 3 (the deepest
                            // behind a non-default feature), so the limit is never
                            // approached — but the guard is those eight functions'
                            // RETURN SHAPES, not a check. An op that echoes a
                            // caller-supplied `Par` back through here writes it into
                            // `output_value`, into the block, and into the
                            // validator's replay decode at `reduce.rs`, with no
                            // other change anywhere.
                            //
                            // Executable:
                            // `rholang/tests/replay_output_value_depth_ceiling.rs`.
                            // Analysis:
                            // `docs/design/audits/theta-depth-traversals-2026-07-26.md`
                            // §7.3.
                            DispatchType::NonDeterministicCall(items) => items
                                .iter()
                                .map(|p| {
                                    Par::decode(&p[..])
                                        .map_err(|e| InterpreterError::DecodeError(e.to_string()))
                                })
                                .collect::<Result<Vec<_>, _>>(),
                            DispatchType::FailedNonDeterministicCall(e) => Err(e),
                            DispatchType::DeterministicCall => Ok(Vec::new()),
                            DispatchType::Skip => Ok(Vec::new()),
                        },
                        Err(e) => Err(e),
                    }
                })
                    as Pin<
                        Box<
                            dyn futures::Future<Output = Result<Vec<Par>, InterpreterError>> + Send,
                        >,
                    >
            });

            Some((produce, is_replay, previous, args))
        } else {
            None
        }
    }
}
