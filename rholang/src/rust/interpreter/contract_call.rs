use std::pin::Pin;

use models::rhoapi::{ListParWithRandom, Par};
use prost::Message;
use rspace_plus_plus::rspace::util::unpack_tuple_with_peek;

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

                    match produce_result {
                        Some((cont, channels, produce)) => {
                            let (continuation, data_list, peek) =
                                unpack_tuple_with_peek((cont, channels));
                            let matched_data = data_list
                                .iter()
                                .map(|(_, matched, _, _)| matched.clone())
                                .collect();
                            let dispatch = async {
                                let dispatch_type = dispatcher
                                    .dispatch(
                                        continuation,
                                        matched_data,
                                        is_replay,
                                        produce
                                            .output_value
                                            .iter()
                                            .map(|p| {
                                                Par::decode(&p[..]).map_err(|e| {
                                                    InterpreterError::DecodeError(e.to_string())
                                                })
                                            })
                                            .collect::<Result<Vec<_>, _>>()?,
                                    )
                                    .await?;
                                match dispatch_type {
                                    DispatchType::NonDeterministicCall(items) => items
                                        .iter()
                                        .map(|p| {
                                            Par::decode(&p[..]).map_err(|e| {
                                                InterpreterError::DecodeError(e.to_string())
                                            })
                                        })
                                        .collect::<Result<Vec<_>, _>>(),
                                    DispatchType::FailedNonDeterministicCall(e) => Err(e),
                                    DispatchType::DeterministicCall | DispatchType::Skip => {
                                        Ok(Vec::new())
                                    }
                                }
                            };
                            if peek {
                                let reducer = dispatcher
                                    .reducer
                                    .get()
                                    .and_then(|weak| weak.upgrade())
                                    .ok_or_else(|| {
                                        InterpreterError::BugFoundError(
                                            "Reducer not initialized".to_string(),
                                        )
                                    })?;
                                let (result, restoration) = futures::join!(
                                    dispatch,
                                    reducer.restore_peeked_data(data_list)
                                );
                                match restoration {
                                    Ok(()) => result,
                                    Err(error) => {
                                        let mut errors = Vec::new();
                                        if let Err(dispatch_error) = result {
                                            errors.push(dispatch_error);
                                        }
                                        errors.push(error);
                                        reducer
                                            .aggregate_evaluator_errors(errors)
                                            .map(|_| Vec::new())
                                    }
                                }
                            } else {
                                dispatch.await
                            }
                        }
                        None => Ok(Vec::new()),
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
