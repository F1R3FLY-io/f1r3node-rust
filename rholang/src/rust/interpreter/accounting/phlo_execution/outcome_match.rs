use std::mem::size_of;
use std::num::NonZeroUsize;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_obligation::{
    PhloObligationKeyError, PhloObligationKeyLimits, PhloObligationKeyV1,
};
use thiserror::Error;

use super::{
    resource_key_with_host_work, CheckedNativeSignedPhloFamilyPolicy, CheckedPhloExecution,
    NativePhloPolicyError, NativeScopedPhloFundingCapture, PhloCaptureLimits, PhloExecutionError,
    PhloExecutionLimits, PhloOutcome, ResourceEntries, WorkBudget,
};
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonical_funding_key_order, filled_vec, reserve_work, FundingSearchError,
};
use crate::rust::interpreter::accounting::phlo_controls::{CheckedPhloControls, PhloSchedule};
use crate::rust::interpreter::host_work::HostWorkBudget;

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

#[derive(Debug)]
struct NormalizedPartition {
    keys: Vec<Vec<u8>>,
    quantities: Vec<u64>,
    order: Vec<usize>,
}

#[derive(Debug)]
struct NormalizedExecution<'a> {
    controls: CheckedPhloControls<'a>,
    partitions: Vec<NormalizedPartition>,
}

fn normalize_outcome(
    outcome: PhloOutcome<'_>,
    budget: &HostWorkBudget,
) -> Result<ObservedOutcome, PhloOutcomeMatchError> {
    Ok(match outcome {
        PhloOutcome::AdmissionRejected => ObservedOutcome::Rejected,
        PhloOutcome::Accepted(failures) => {
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                failures.len(),
            )?;
            ObservedOutcome::Accepted(
                failures
                    .iter()
                    .fold(EvaluationFailureSummary::default(), |summary, failure| {
                        summary.union(EvaluationFailureSummary::single(*failure))
                    }),
            )
        }
    })
}

fn normalize_partition(
    resources: ResourceEntries<'_>,
    limits: PhloObligationKeyLimits,
    remaining_bytes: &mut usize,
    keys_work: &mut WorkBudget,
    budget: &HostWorkBudget,
) -> Result<NormalizedPartition, PhloOutcomeMatchError> {
    reserve_work(budget, HostWorkDimension::SearchCandidates, resources.len())?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        resources
            .len()
            .checked_mul(
                size_of::<Vec<u8>>() + size_of::<&[u8]>() + size_of::<u64>() + size_of::<usize>(),
            )
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut keys = filled_vec(resources.len(), Vec::new())?;
    let mut quantities = filled_vec(resources.len(), 0_u64)?;
    for ((key, quantity), entry) in keys.iter_mut().zip(&mut quantities).zip(resources.iter()) {
        *quantity = entry.quantity;
        let resource =
            resource_key_with_host_work(entry.resource, keys_work, Some(budget))?.into_wire()?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            resource
                .authority
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(16))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut key_limits = limits;
        key_limits.wire.total_bytes = key_limits.wire.total_bytes.min(*remaining_bytes);
        let record = PhloObligationKeyV1::Resource(resource);
        let prepared = record.prepare_encoding(key_limits)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            prepared.encoded_len(),
        )?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            prepared.encoded_len(),
        )?;
        *remaining_bytes = remaining_bytes
            .checked_sub(prepared.encoded_len())
            .ok_or(FundingSearchError::Overflow)?;
        *key = prepared.encode()?;
    }
    let mut refs = filled_vec(keys.len(), &[][..])?;
    for (reference, key) in refs.iter_mut().zip(&keys) {
        *reference = key;
    }
    let sorted = canonical_funding_key_order(&refs, budget)?;
    let mut order: Vec<usize> = Vec::new();
    order
        .try_reserve_exact(sorted.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for index in sorted {
        if let Some(previous) = order.last().copied() {
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                keys[index]
                    .len()
                    .min(keys[previous].len())
                    .checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?,
            )?;
            if keys[index] == keys[previous] {
                quantities[previous] = quantities[previous]
                    .checked_add(quantities[index])
                    .ok_or(PhloExecutionError::ArithmeticOverflow)?;
                continue;
            }
        }
        order.push(index);
    }
    Ok(NormalizedPartition {
        keys,
        quantities,
        order,
    })
}

fn normalize_execution<'a>(
    execution: CheckedPhloExecution<'a>,
    limits: PhloOutcomeMatchLimits,
    budget: &HostWorkBudget,
) -> Result<NormalizedExecution<'a>, PhloOutcomeMatchError> {
    let parts = execution.witness().parts();
    let mut remaining = limits.execution.resource_entries;
    for part in parts {
        remaining = remaining
            .checked_sub(part.len())
            .ok_or(PhloExecutionError::TooManyResourceEntries)?;
    }
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        5 * size_of::<NormalizedPartition>(),
    )?;
    let mut partitions = Vec::new();
    partitions
        .try_reserve_exact(5)
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
            budget,
        )?);
    }
    Ok(NormalizedExecution {
        controls: execution.controls(),
        partitions,
    })
}

fn reserve_schedule_comparison(
    schedule: PhloSchedule<'_>,
    budget: &HostWorkBudget,
) -> Result<(), FundingSearchError> {
    let env = schedule.environment;
    let mut work = schedule
        .weights
        .len()
        .checked_add(64)
        .ok_or(FundingSearchError::Overflow)?;
    for bytes in [env.network, env.shard, env.asset, env.unit] {
        work = work
            .checked_add(bytes.len())
            .ok_or(FundingSearchError::Overflow)?;
    }
    reserve_work(budget, HostWorkDimension::VerificationOperations, work)
}

fn reserve_controls_comparison(
    controls: CheckedPhloControls<'_>,
    budget: &HostWorkBudget,
) -> Result<(), FundingSearchError> {
    let terms = controls.terms();
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        terms
            .required_owner_ceilings
            .len()
            .checked_add(terms.permitted_schedules.len())
            .and_then(|n| n.checked_add(16))
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    reserve_schedule_comparison(controls.schedule(), budget)?;
    for schedule in terms.permitted_schedules {
        reserve_schedule_comparison(*schedule, budget)?;
    }
    Ok(())
}

fn normalized_executions_equal(
    left: &NormalizedExecution<'_>,
    right: &NormalizedExecution<'_>,
    budget: &HostWorkBudget,
) -> Result<bool, PhloOutcomeMatchError> {
    reserve_controls_comparison(left.controls, budget)?;
    reserve_controls_comparison(right.controls, budget)?;
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
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                left.len()
                    .min(right.len())
                    .checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?,
            )?;
            if left != right {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn reserve_capture_comparison<A>(
    capture: &NativeScopedPhloFundingCapture<'_, A>,
    budget: &HostWorkBudget,
) -> Result<(), FundingSearchError> {
    let capture = capture.scoped().capture();
    reserve_work(budget, HostWorkDimension::VerificationOperations, 128)?;
    for source in capture.sources() {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            source
                .source()
                .custody
                .len()
                .checked_add(12)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
    }
    for key in capture.obligation_keys() {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            key.len()
                .checked_add(1)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
    }
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        capture.amounts().len(),
    )?;
    for row in capture.eligible() {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            row.len()
                .checked_add(1)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
    }
    for row in capture.assignment() {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            row.len()
                .checked_add(1)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
    }
    Ok(())
}

fn equivalent_captures<A>(
    left: &NativeScopedPhloFundingCapture<'_, A>,
    right: &NativeScopedPhloFundingCapture<'_, A>,
    budget: &HostWorkBudget,
) -> Result<bool, PhloOutcomeMatchError> {
    reserve_capture_comparison(left, budget)?;
    reserve_capture_comparison(right, budget)?;
    let (left, right) = (left.scoped(), right.scoped());
    let (a, b) = (left.capture(), right.capture());
    Ok(a.sources() == b.sources()
        && a.obligation_keys() == b.obligation_keys()
        && a.amounts() == b.amounts()
        && a.eligible() == b.eligible()
        && a.assignment() == b.assignment()
        && left.resource_transition() == right.resource_transition()
        && left.fee_transition() == right.fee_transition())
}

impl<'a, A> CheckedNativeSignedPhloFamilyPolicy<'a, A> {
    pub fn capture_matching_execution(
        &self,
        observed: CheckedPhloExecution<'_>,
        outcome: PhloOutcome<'_>,
        limits: PhloOutcomeMatchLimits,
        capture_limits: PhloCaptureLimits,
        budget: &HostWorkBudget,
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
        reserve_work(budget, HostWorkDimension::SearchCandidates, cases.len())?;
        let outcome = normalize_outcome(outcome, budget)?;
        let observed = normalize_execution(observed, limits, budget)?;
        let mut selected = None;
        for (branch, case) in cases.iter().enumerate() {
            if normalize_outcome(case.obligations.outcome(), budget)? != outcome {
                continue;
            }
            let expected = normalize_execution(case.obligations.execution(), limits, budget)?;
            if !normalized_executions_equal(&expected, &observed, budget)? {
                continue;
            }
            let capture = self.capture_case(branch, capture_limits, budget)?;
            if let Some(first) = &selected {
                if !equivalent_captures(first, &capture, budget)? {
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
