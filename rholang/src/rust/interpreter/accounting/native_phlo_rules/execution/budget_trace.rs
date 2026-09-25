use std::cmp::Ordering;
use std::mem::size_of;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
use prost::Message;
use thiserror::Error;

use super::{NativePhloExecutionContract, NativePhloExecutionError, NativePhloRegionLimits};
use crate::rust::interpreter::accounting::authority::{
    reserve_authority_signature_tree, AuthorityByteEventKind,
};
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::monetary_allocation::{reserve_work, FundingSearchError};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeAttemptStage {
    ProduceIntroduction,
    ConsumeIntroduction,
    Comm,
}

impl From<AuthorityByteEventKind> for NativeAttemptStage {
    fn from(kind: AuthorityByteEventKind) -> Self {
        match kind {
            AuthorityByteEventKind::ProduceIntroduction => Self::ProduceIntroduction,
            AuthorityByteEventKind::ConsumeIntroduction => Self::ConsumeIntroduction,
            AuthorityByteEventKind::Comm => Self::Comm,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeBudgetOccurrence {
    pub session: [u8; 32],
    pub path: Vec<(u64, u64)>,
    pub stage: NativeAttemptStage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBudgetAttempt {
    pub occurrence: NativeBudgetOccurrence,
    pub observation: Arc<ByteObservation>,
    pub granted: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeBudgetTraceLimits {
    pub attempts: usize,
    pub path_segments: usize,
    pub regions: NativePhloRegionLimits,
}

#[derive(Debug, Error)]
pub enum NativeBudgetTraceError {
    #[error(transparent)]
    Execution(#[from] NativePhloExecutionError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
    #[error("native attempt trace exceeds its entry or occurrence path limit")]
    Limit,
    #[error("native attempt belongs to another execution session")]
    Session,
    #[error("native attempt stage differs from its observation")]
    Stage,
    #[error("native attempt occurrence is duplicated")]
    Duplicate,
    #[error("native attempt decision differs from its checked prefix")]
    Decision,
    #[error("native replay occurrence is absent from the checked trace")]
    Unknown,
    #[error("native replay observation differs from its checked attempt")]
    Observation,
    #[error("native replay attempt was already consumed")]
    Consumed,
    #[error("native replay did not consume its complete budget trace")]
    Incomplete,
    #[error("native replay usage differs from its checked total")]
    Usage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeBudgetReplayDecision {
    Accepted { usage: u64 },
    Denied,
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub struct CheckedNativeBudgetTrace {
    evidence: CheckedNativeBudgetEvidence,
    consumed: Vec<bool>,
    used: u64,
    remaining: usize,
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub(in crate::rust::interpreter::accounting) struct CheckedNativeBudgetEvidence {
    rows: Arc<[NativeBudgetAttempt]>,
    ordered: Vec<usize>,
    charges: Vec<Option<u64>>,
    comparison_bytes: Vec<usize>,
    total: u64,
    path_limit: usize,
}

impl NativePhloExecutionContract<'_> {
    pub fn check_budget_trace(
        &self,
        session: [u8; 32],
        rows: Arc<[NativeBudgetAttempt]>,
        limits: NativeBudgetTraceLimits,
        budget: &HostWorkBudget,
    ) -> Result<CheckedNativeBudgetTrace, NativeBudgetTraceError> {
        let evidence = self.check_budget_evidence(session, rows, limits, budget)?;
        let remaining = evidence.rows.len();
        let mut consumed = allocate(remaining, budget)?;
        consumed.resize(remaining, false);
        Ok(CheckedNativeBudgetTrace {
            evidence,
            consumed,
            used: 0,
            remaining,
        })
    }

    pub(in crate::rust::interpreter::accounting) fn check_budget_evidence(
        &self,
        session: [u8; 32],
        rows: Arc<[NativeBudgetAttempt]>,
        limits: NativeBudgetTraceLimits,
        budget: &HostWorkBudget,
    ) -> Result<CheckedNativeBudgetEvidence, NativeBudgetTraceError> {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        if rows.len() > limits.attempts {
            return Err(NativeBudgetTraceError::Limit);
        }
        let mut ordered = allocate(rows.len(), budget)?;
        let mut charges = allocate(rows.len(), budget)?;
        let mut comparison_bytes = allocate(rows.len(), budget)?;
        for (index, row) in rows.iter().enumerate() {
            if row.occurrence.path.len() > limits.path_segments {
                return Err(NativeBudgetTraceError::Limit);
            }
            reserve_work(budget, HostWorkDimension::VerificationOperations, 34)?;
            if row.occurrence.session != session {
                return Err(NativeBudgetTraceError::Session);
            }
            if row.occurrence.stage != NativeAttemptStage::from(row.observation.kind) {
                return Err(NativeBudgetTraceError::Stage);
            }
            ordered.push(index);
        }
        let compare = |left: usize, right: usize| {
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                occurrence_work(
                    rows[left]
                        .occurrence
                        .path
                        .len()
                        .min(rows[right].occurrence.path.len()),
                ),
            )?;
            Ok(rows[left].occurrence.cmp(&rows[right].occurrence))
        };
        sort_indices(&mut ordered, &compare)?;
        for pair in ordered.windows(2) {
            if compare(pair[0], pair[1])? == Ordering::Equal {
                return Err(NativeBudgetTraceError::Duplicate);
            }
        }
        let mut used = 0_u64;
        for row in rows.iter() {
            let charge = match self.prepare(Arc::clone(&row.observation), limits.regions, budget) {
                Ok(prepared) => Some(prepared.usage()),
                Err(NativePhloExecutionError::Overflow) => None,
                Err(error) => return Err(error.into()),
            };
            let next = charge
                .and_then(|amount| used.checked_add(amount))
                .filter(|next| *next <= self.policy.bound);
            if row.granted != next.is_some() {
                return Err(NativeBudgetTraceError::Decision);
            }
            if let Some(next) = next {
                used = next;
            }
            charges.push(charge);
            comparison_bytes.push(observation_comparison_bytes(&row.observation, budget)?);
        }
        Ok(CheckedNativeBudgetEvidence {
            rows,
            ordered,
            charges,
            comparison_bytes,
            total: used,
            path_limit: limits.path_segments,
        })
    }
}

impl CheckedNativeBudgetTrace {
    pub fn total(&self) -> u64 { self.evidence.total }

    pub fn remaining(&self) -> usize { self.remaining }

    pub fn consume(
        &mut self,
        occurrence: &NativeBudgetOccurrence,
        observation: &ByteObservation,
        budget: &HostWorkBudget,
    ) -> Result<NativeBudgetReplayDecision, NativeBudgetTraceError> {
        if occurrence.path.len() > self.evidence.path_limit {
            return Err(NativeBudgetTraceError::Limit);
        }
        let comparisons = (usize::BITS - self.evidence.ordered.len().leading_zeros()) as usize + 1;
        let work = occurrence_work(occurrence.path.len()).saturating_mul(comparisons);
        reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
        let position = self
            .evidence
            .ordered
            .binary_search_by(|index| self.evidence.rows[*index].occurrence.cmp(occurrence))
            .map_err(|_| NativeBudgetTraceError::Unknown)?;
        let index = self.evidence.ordered[position];
        if self.consumed[index] {
            return Err(NativeBudgetTraceError::Consumed);
        }
        let decision = self.evidence.authenticate(index, observation, budget)?;
        if let NativeBudgetReplayDecision::Accepted { usage } = decision {
            let next = self
                .used
                .checked_add(usage)
                .filter(|next| *next <= self.evidence.total)
                .ok_or(NativeBudgetTraceError::Usage)?;
            self.used = next;
        }
        self.consumed[index] = true;
        self.remaining -= 1;
        Ok(decision)
    }

    pub fn finish(&self) -> Result<u64, NativeBudgetTraceError> {
        if self.remaining != 0 {
            return Err(NativeBudgetTraceError::Incomplete);
        }
        if self.used != self.evidence.total {
            return Err(NativeBudgetTraceError::Usage);
        }
        Ok(self.used)
    }
}

impl CheckedNativeBudgetEvidence {
    pub(in crate::rust::interpreter::accounting) fn total(&self) -> u64 { self.total }

    pub(in crate::rust::interpreter::accounting) fn authenticate(
        &self,
        index: usize,
        observation: &ByteObservation,
        budget: &HostWorkBudget,
    ) -> Result<NativeBudgetReplayDecision, NativeBudgetTraceError> {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        let row = self
            .rows
            .get(index)
            .ok_or(NativeBudgetTraceError::Unknown)?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationBytes,
            self.comparison_bytes[index],
        )?;
        if row.observation.as_ref() != observation {
            return Err(NativeBudgetTraceError::Observation);
        }
        if row.granted {
            Ok(NativeBudgetReplayDecision::Accepted {
                usage: self.charges[index].ok_or(NativeBudgetTraceError::Usage)?,
            })
        } else {
            Ok(NativeBudgetReplayDecision::Denied)
        }
    }
}

fn occurrence_work(segments: usize) -> usize { segments.saturating_mul(2).saturating_add(35) }

pub(in crate::rust::interpreter::accounting) fn observation_comparison_bytes(
    observation: &ByteObservation,
    budget: &HostWorkBudget,
) -> Result<usize, NativeBudgetTraceError> {
    let mut depth = 0;
    for region in &observation.authority.regions {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        if let Some(signature) = region.signature.as_ref() {
            reserve_authority_signature_tree(signature, budget, &mut depth)
                .map_err(NativePhloExecutionError::from)?;
        }
    }
    observation
        .authority
        .encoded_len()
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(size_of::<ByteObservation>()))
        .ok_or(NativeBudgetTraceError::Limit)
}

fn sort_indices(
    indices: &mut [usize],
    compare: &impl Fn(usize, usize) -> Result<Ordering, NativeBudgetTraceError>,
) -> Result<(), NativeBudgetTraceError> {
    for root in (0..indices.len() / 2).rev() {
        sift_indices(indices, root, compare)?;
    }
    for end in (1..indices.len()).rev() {
        indices.swap(0, end);
        sift_indices(&mut indices[..end], 0, compare)?;
    }
    Ok(())
}

fn sift_indices(
    indices: &mut [usize],
    mut root: usize,
    compare: &impl Fn(usize, usize) -> Result<Ordering, NativeBudgetTraceError>,
) -> Result<(), NativeBudgetTraceError> {
    while root < indices.len() / 2 {
        let mut child = 2 * root + 1;
        if child + 1 < indices.len()
            && compare(indices[child], indices[child + 1])? == Ordering::Less
        {
            child += 1;
        }
        if compare(indices[root], indices[child])? != Ordering::Less {
            break;
        }
        indices.swap(root, child);
        root = child;
    }
    Ok(())
}

fn allocate<T>(count: usize, budget: &HostWorkBudget) -> Result<Vec<T>, NativeBudgetTraceError> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(NativeBudgetTraceError::Limit)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
