//! The errors of the Reified RSpaces reducer hooks.

use models::rust::reified::encoding::{Marker, ReifiedDecodeError};

use crate::rust::interpreter::errors::InterpreterError;

/// A Reified construct that the reducer rejects. The deploy fails closed with
/// `InterpreterError::ReduceError("reified:<Code>: …")`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReifiedReduceError {
    #[error("{0}")]
    Malformed(ReifiedDecodeError),
    #[error("the {marker} construct needs the Reified RSpaces runtime, which is not available")]
    RuntimeUnavailable { marker: Marker },
    #[error("a send to this channel takes no params")]
    ParamsUnsupported,
    #[error("a receive from this channel takes no query")]
    QuerySpecUnsupported,
}

impl ReifiedReduceError {
    /// The stable code of the error, the `<Code>` of `reified:<Code>:`.
    pub fn code(&self) -> &'static str {
        match self {
            ReifiedReduceError::Malformed(_) => "Malformed",
            ReifiedReduceError::RuntimeUnavailable { .. } => "RuntimeUnavailable",
            ReifiedReduceError::ParamsUnsupported => "ParamsUnsupported",
            ReifiedReduceError::QuerySpecUnsupported => "QuerySpecUnsupported",
        }
    }
}

impl From<ReifiedReduceError> for InterpreterError {
    fn from(error: ReifiedReduceError) -> Self {
        InterpreterError::ReduceError(format!("reified:{}: {}", error.code(), error))
    }
}
