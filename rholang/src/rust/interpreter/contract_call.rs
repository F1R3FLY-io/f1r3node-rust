use std::pin::Pin;

use models::rhoapi::{ListParWithRandom, Par};
use prost::Message;

use super::dispatch::{DispatchType, RhoDispatch};
use super::errors::InterpreterError;

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
 * The reply is sent through the reducer's produce, so a COMM it fires is handled exactly like one fired by a
 * Rholang send.
 *
 * @param dispatcher the dispatcher
 *
 * See rholang/src/main/scala/coop/rchain/rholang/interpreter/ContractCall.scala
 */
pub struct ContractCall {
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

            let dispatcher = self.dispatcher.clone();
            let produce = Box::new(move |values: &[Par], ch: &Par| {
                let values_vec: Vec<Par> = values.to_vec();
                let ch_cloned: Par = ch.clone();
                Box::pin(async move {
                    let reducer = dispatcher.reducer()?;
                    let dispatch_type = reducer
                        .produce(
                            ch_cloned,
                            ListParWithRandom {
                                pars: values_vec,
                                random_state: rand,
                                seal: None,
                            },
                            false,
                        )
                        .await?;

                    match dispatch_type {
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
