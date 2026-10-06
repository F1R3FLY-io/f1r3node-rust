use std::mem::size_of;
use std::sync::atomic::{AtomicU8, Ordering};

use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use rspace_plus_plus::rspace::errors::RSpaceError as SpaceError;
use thiserror::Error;

use super::phlo_execution::PhloFailure;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvaluationFailureSummary {
    bits: u8,
}

impl EvaluationFailureSummary {
    pub const fn single(failure: PhloFailure) -> Self {
        Self {
            bits: match failure {
                PhloFailure::User => 1,
                PhloFailure::Platform => 2,
                PhloFailure::Certificate => 4,
                PhloFailure::Unclassified => 8,
            },
        }
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    pub const fn contains(self, failure: PhloFailure) -> bool {
        self.bits & Self::single(failure).bits != 0
    }

    pub const fn permits_retained_charge(self) -> bool { self.bits & !1 == 0 }
}

pub(crate) trait FailureAtomic {
    fn include(&self, bits: u8);
    fn read(&self) -> u8;
}

impl FailureAtomic for AtomicU8 {
    fn include(&self, bits: u8) { self.fetch_or(bits, Ordering::AcqRel); }
    fn read(&self) -> u8 { self.load(Ordering::Acquire) }
}

#[derive(Default)]
pub(crate) struct FailureRecorder<A = AtomicU8> {
    bits: A,
}

impl<A: FailureAtomic> FailureRecorder<A> {
    pub(crate) fn record(&self, summary: EvaluationFailureSummary) {
        self.bits.include(summary.bits);
    }
    pub(crate) fn snapshot(&self) -> EvaluationFailureSummary {
        EvaluationFailureSummary {
            bits: self.bits.read(),
        }
    }
}

#[derive(Debug, Error)]
pub enum FailureClassificationError {
    #[error(transparent)]
    HostWork(#[from] HostWorkReservationError),
    #[error("failure classification allocation failed")]
    AllocationFailed,
    #[error("failure classification work overflowed")]
    Overflow,
}

fn reserve(
    budget: Option<&HostWorkBudget>,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), FailureClassificationError> {
    if let Some(budget) = budget {
        let amount = u64::try_from(amount).map_err(|_| FailureClassificationError::Overflow)?;
        budget.reserve(dimension, HostWorkUnits::new(amount))?;
    }
    Ok(())
}

pub fn classify_errors(
    errors: &[InterpreterError],
    budget: Option<&HostWorkBudget>,
) -> Result<EvaluationFailureSummary, FailureClassificationError> {
    let mut result = EvaluationFailureSummary::default();
    let mut pending: Vec<&[InterpreterError]> = Vec::new();
    let mut remaining = errors;
    loop {
        let Some((error, rest)) = remaining.split_first() else {
            match pending.pop() {
                Some(next) => {
                    remaining = next;
                    continue;
                }
                None => return Ok(result),
            }
        };
        remaining = rest;
        reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
        use InterpreterError::*;
        let (failure, children) = match error {
            UserAbortError
            | MethodNotDefined { .. }
            | MethodArgumentNumberMismatch { .. }
            | OperatorNotDefined { .. }
            | OperatorExpectedError { .. }
            | IfConditionTypeError { .. } => (Some(PhloFailure::User), None),
            OutOfPhlogistonsError | RSpaceError(SpaceError::OutOfPhlogistons) => {
                (Some(PhloFailure::Certificate), None)
            }
            RSpaceError(_)
            | BugFoundError(_)
            | UndefinedRequiredProtobufFieldError(_)
            | HostWorkRejected
            | DecodeError(_)
            | OpenAIError(_)
            | OllamaError(_)
            | ChromaDBError(_)
            | IoError(_)
            | CanNotReplayFailedNonDeterministicProcess => (Some(PhloFailure::Platform), None),
            AggregateError { interpreter_errors } => (
                interpreter_errors
                    .is_empty()
                    .then_some(PhloFailure::Unclassified),
                Some(interpreter_errors.as_slice()),
            ),
            NonDeterministicProcessFailure { cause, .. }
            | ProduceFailureWithOutput { cause, .. } => (
                Some(PhloFailure::Platform),
                Some(std::slice::from_ref(cause.as_ref())),
            ),
            NormalizerError(_)
            | SyntaxError(_)
            | LexerError(_)
            | ParserError(_)
            | EncodeError(_)
            | UnexpectedBundleContent(_)
            | UnrecognizedNormalizerError(_)
            | TopLevelWildcardsNotAllowedError(_)
            | TopLevelFreeVariablesNotAllowedError(_)
            | TopLevelLogicalConnectivesNotAllowedError(_)
            | SubstituteError(_)
            | PatternReceiveError(_)
            | SetupError(_)
            | UnrecognizedInterpreterError(_)
            | SortMatchError(_)
            | ReduceError(_)
            | UnexpectedProcContext { .. }
            | UnexpectedReuseOfProcContextFree { .. }
            | UnboundVariableRefSpan { .. }
            | UnboundVariableRefPos { .. }
            | ReceiveOnSameChannelsError { .. }
            | UnexpectedNameContext { .. }
            | UnexpectedReuseOfNameContextFree { .. }
            | IllegalArgumentError(_) => (Some(PhloFailure::Unclassified), None),
        };
        if let Some(failure) = failure {
            result = result.union(EvaluationFailureSummary::single(failure));
        }
        if let Some(children) = children.filter(|children| !children.is_empty()) {
            if !remaining.is_empty() {
                reserve(
                    budget,
                    HostWorkDimension::SearchStateBytes,
                    size_of::<&[InterpreterError]>(),
                )?;
                pending
                    .try_reserve(1)
                    .map_err(|_| FailureClassificationError::AllocationFailed)?;
                pending.push(remaining);
            }
            remaining = children;
        }
    }
}

#[cfg(test)]
mod tests;
