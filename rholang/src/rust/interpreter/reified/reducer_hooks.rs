//! The Reified RSpaces recognition hooks of the reducer (plan decision 22).
//!
//! This file is the child module `reified_hooks` of `reduce.rs`. Hook H1, the term filter, is
//! `util::is_evaluated_process_expr`. On a term without a marker tuple, each hook is a check
//! that does not allocate, and the reducer behaves as at HEAD. Until the Reified RSpaces
//! runtime exists, every recognized construct fails closed.

use std::future::Future;
use std::pin::Pin;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{CostAuthority, ETuple, ListParWithRandom, Par};
use models::rust::reified::encoding::{
    decode_process_tuple, decode_query_source, split_trailing_params,
};

use super::ReducerCore;
use crate::rust::interpreter::dispatch::DispatchType;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;

mod errors;

use errors::ReifiedReduceError;

impl ReducerCore {
    /// Hook H2: evaluates a USE or NEWSPACE tuple in process position. The term filter (H1)
    /// admits no other tuple.
    pub(super) async fn eval_marker(
        &self,
        tuple: &ETuple,
        _env: &Env<Par>,
        _rand: Blake2b512Random,
        _authority: &CostAuthority,
    ) -> Result<(), InterpreterError> {
        let construct = decode_process_tuple(tuple).map_err(ReifiedReduceError::Malformed)?;
        Err(ReifiedReduceError::RuntimeUnavailable {
            marker: construct.marker(),
        }
        .into())
    }

    /// Hook H3: the `produce` of `eval_send`. A trailing PARAMS tuple fails closed. Other
    /// data goes to `produce` unchanged.
    pub(super) fn reified_produce<'a>(
        &'a self,
        chan: Par,
        data: ListParWithRandom,
        persistent: bool,
        introduction_authority: CostAuthority,
    ) -> Pin<
        Box<dyn Future<Output = Result<DispatchType, InterpreterError>> + std::marker::Send + 'a>,
    > {
        let rejection = match split_trailing_params(&data.pars) {
            Ok(None) => return self.produce(chan, data, persistent, introduction_authority),
            Ok(Some(_)) => ReifiedReduceError::ParamsUnsupported,
            Err(error) => ReifiedReduceError::Malformed(error),
        };
        Box::pin(std::future::ready(Err(rejection.into())))
    }
}

/// Hook H4: checks the evaluated source of a receive bind before `unbundle_receive` checks
/// its bundle. A QUERY tuple fails closed.
pub(super) fn reject_query_source(source: &Par) -> Result<(), InterpreterError> {
    match decode_query_source(source) {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(ReifiedReduceError::QuerySpecUnsupported.into()),
        Err(error) => Err(ReifiedReduceError::Malformed(error).into()),
    }
}
