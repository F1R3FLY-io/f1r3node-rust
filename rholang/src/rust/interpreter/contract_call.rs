use std::pin::Pin;

use models::rhoapi::{ListParWithRandom, Par};

use super::dispatch::{decode_non_deterministic_output, DispatchType, RhoDispatch};
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
                                    // ★★ THE SAME CONSENSUS-CLASS `Par` READ as
                                    // `reduce.rs`'s `continue_produce_process` — the
                                    // system-contract path's copy of it. It is now the
                                    // same FUNCTION, not merely the same rule written
                                    // out again: the ceiling, its arithmetic, the
                                    // replay-failure path and the reachability finding
                                    // all live at `dispatch::decode_non_deterministic_output`,
                                    // next to the `dispatch_type` encoder that produced
                                    // these bytes.
                                    decode_non_deterministic_output(&produce.output_value)?,
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
                            // ★ The RETURN leg of the same ceiling, and the one
                            // place it runs on the PLAY path: these bytes were
                            // produced by `dispatch_type`'s `encode_to_vec` a
                            // few frames below, so this is an encode→decode
                            // round trip with no wire between its ends.
                            //
                            // ⚠ It is reached only when producing the operation's
                            // result onto its `ack` channel fires a continuation
                            // that is ITSELF a non-deterministic system process
                            // (`ack` bound to a system channel of matching
                            // arity). For the ordinary `ParBody` ack the arm is
                            // `DeterministicCall` and nothing is decoded — which
                            // is why the read ceiling's first encounter with real
                            // bytes is on the validator, not here.
                            //
                            // Same function as every other member of the class;
                            // see `dispatch::decode_non_deterministic_output` for
                            // the ceiling and the write-side reachability finding
                            // (`rho:ollama:models` at depth 1 is the deepest any
                            // registered operation returns, so this decode has
                            // thirty-two levels of headroom).
                            DispatchType::NonDeterministicCall(items) => {
                                decode_non_deterministic_output(&items)
                            }
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
