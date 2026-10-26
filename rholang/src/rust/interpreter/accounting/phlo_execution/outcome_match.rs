use std::num::NonZeroUsize;

use models::rust::phlo_obligation::{PhloObligationKeyError, PhloObligationKeyLimits};
use thiserror::Error;

use super::partition::{normalize_partition, NormalizedPartition, PhloPartitionError};
use super::{
    CheckedNativeSignedPhloFamilyPolicy, CheckedPhloExecution, NativePhloPolicyError,
    NativeScopedPhloFundingCapture, PhloCaptureLimits, PhloExecutionError, PhloExecutionLimits,
    PhloOutcome, WorkBudget,
};
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;
use crate::rust::interpreter::accounting::phlo_controls::CheckedPhloControls;

#[derive(Clone, Copy, Debug)]
pub struct PhloOutcomeMatchLimits {
    pub execution: PhloExecutionLimits,
    pub key: PhloObligationKeyLimits,
    pub aggregate_key_bytes: usize,
    pub cases: NonZeroUsize,
}

#[derive(Debug, Error)]
pub enum PhloOutcomeMatchError {
    #[error("observed execution has no matching prepared funding outcome")]
    NoMatchingOutcome,
    #[error("matching funding outcomes have different canonical settlements")]
    ConflictingMatchingOutcomes,
    #[error("funding outcome scan exceeds the configured case limit")]
    TooManyCases,
    #[error(transparent)]
    Execution(#[from] PhloExecutionError),
    #[error(transparent)]
    Key(#[from] PhloObligationKeyError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
    #[error(transparent)]
    Native(#[from] NativePhloPolicyError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservedOutcome {
    Rejected,
    Accepted(EvaluationFailureSummary),
}

impl From<PhloPartitionError> for PhloOutcomeMatchError {
    fn from(error: PhloPartitionError) -> Self {
        match error {
            PhloPartitionError::Execution(error) => Self::Execution(error),
            PhloPartitionError::Key(error) => Self::Key(error),
            PhloPartitionError::Search(error) => Self::Search(error),
        }
    }
}

#[derive(Debug)]
struct NormalizedExecution<'a> {
    controls: CheckedPhloControls<'a>,
    partitions: Vec<NormalizedPartition>,
}

fn normalize_outcome(outcome: PhloOutcome<'_>) -> Result<ObservedOutcome, PhloOutcomeMatchError> {
    Ok(match outcome {
        PhloOutcome::AdmissionRejected => ObservedOutcome::Rejected,
        PhloOutcome::Accepted(failures) => ObservedOutcome::Accepted(
            failures
                .iter()
                .fold(EvaluationFailureSummary::default(), |summary, failure| {
                    summary.union(EvaluationFailureSummary::single(*failure))
                }),
        ),
    })
}

fn normalize_execution<'a>(
    execution: CheckedPhloExecution<'a>,
    limits: PhloOutcomeMatchLimits,
) -> Result<NormalizedExecution<'a>, PhloOutcomeMatchError> {
    let [available, required, used, unused, fresh] = execution.witness().parts();
    let parts = [
        available,
        required,
        used,
        unused,
        fresh,
        super::ResourceEntries {
            occurrences: &[],
            counted: execution.retained_acquisitions(),
        },
    ];
    let mut remaining = limits.execution.resource_entries;
    for part in &parts {
        remaining = remaining
            .checked_sub(part.len())
            .ok_or(PhloExecutionError::TooManyResourceEntries)?;
    }
    let mut partitions = Vec::new();
    partitions
        .try_reserve_exact(6)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut keys_work = WorkBudget {
        remaining_nodes: limits.execution.authority_nodes,
        remaining_bytes: limits.execution.key_bytes,
    };
    let mut remaining_bytes = limits.aggregate_key_bytes;
    for part in parts {
        partitions.push(normalize_partition(
            part,
            limits.key,
            &mut remaining_bytes,
            &mut keys_work,
        )?);
    }
    Ok(NormalizedExecution {
        controls: execution.controls(),
        partitions,
    })
}

fn normalized_executions_equal(
    left: &NormalizedExecution<'_>,
    right: &NormalizedExecution<'_>,
) -> Result<bool, PhloOutcomeMatchError> {
    if left.controls != right.controls {
        return Ok(false);
    }
    for (left, right) in left.partitions.iter().zip(&right.partitions) {
        if left.order.len() != right.order.len() {
            return Ok(false);
        }
        for (l, r) in left.order.iter().zip(&right.order) {
            if left.quantities[*l] != right.quantities[*r] {
                return Ok(false);
            }
            let (left, right) = (&left.keys[*l], &right.keys[*r]);
            if left != right {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn equivalent_captures<A>(
    left: &NativeScopedPhloFundingCapture<'_, A>,
    right: &NativeScopedPhloFundingCapture<'_, A>,
) -> Result<bool, PhloOutcomeMatchError> {
    let (left, right) = (left.scoped(), right.scoped());
    let (a, b) = (left.capture(), right.capture());
    Ok(a.sources() == b.sources()
        && a.obligation_keys() == b.obligation_keys()
        && a.amounts() == b.amounts()
        && a.obligations()
            .map(|item| item.quantity())
            .eq(b.obligations().map(|item| item.quantity()))
        && a.eligible() == b.eligible()
        && a.assignment() == b.assignment()
        && left.resource_transition() == right.resource_transition()
        && left.fee_transition() == right.fee_transition())
}

impl<'a, A> CheckedNativeSignedPhloFamilyPolicy<'a, A> {
    pub fn capture_matching_discharge(
        &self,
        controls: CheckedPhloControls<'_>,
        discharge: &super::PreparedPhloDischarge<'_>,
        outcome: PhloOutcome<'_>,
        limits: PhloOutcomeMatchLimits,
        capture_limits: PhloCaptureLimits,
    ) -> Result<NativeScopedPhloFundingCapture<'a, A>, PhloOutcomeMatchError> {
        let observed =
            super::check_counted_phlo_execution(controls, discharge.witness(), limits.execution)?;
        self.capture_matching_execution(observed, outcome, limits, capture_limits)
    }

    pub fn capture_matching_execution(
        &self,
        observed: CheckedPhloExecution<'_>,
        outcome: PhloOutcome<'_>,
        limits: PhloOutcomeMatchLimits,
        capture_limits: PhloCaptureLimits,
    ) -> Result<NativeScopedPhloFundingCapture<'a, A>, PhloOutcomeMatchError> {
        let cases = self
            .policy()
            .signed_intent()
            .intent()
            .bound()
            .consent()
            .family()
            .cases();
        if cases.len() > limits.cases.get() {
            return Err(PhloOutcomeMatchError::TooManyCases);
        }
        let outcome = normalize_outcome(outcome)?;
        let observed = normalize_execution(observed, limits)?;
        let mut selected = None;
        for (branch, case) in cases.iter().enumerate() {
            if normalize_outcome(case.obligations.outcome())? != outcome {
                continue;
            }
            let expected = normalize_execution(case.obligations.execution(), limits)?;
            if !normalized_executions_equal(&expected, &observed)? {
                continue;
            }
            let capture = self.capture_case(branch, capture_limits)?;
            if let Some(first) = &selected {
                if !equivalent_captures(first, &capture)? {
                    return Err(PhloOutcomeMatchError::ConflictingMatchingOutcomes);
                }
            } else {
                selected = Some(capture);
            }
        }
        selected.ok_or(PhloOutcomeMatchError::NoMatchingOutcome)
    }
}

#[cfg(test)]
mod tests;
