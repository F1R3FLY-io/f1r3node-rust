use std::cmp::Ordering;

use thiserror::Error;

use super::funding_feasibility::filled_vec;
use super::{
    select_fixed_funding_policy, FundingAssignmentError, FundingMinimaxProblem,
    FundingPolicyResult, FundingSearchError, FundingSearchLimits,
};

#[path = "funding_identity_fee.rs"]
mod fee;
pub use fee::FundingFeeProposal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalFundingProblem<'a> {
    sources: Vec<&'a [u8]>,
    obligations: Vec<&'a [u8]>,
    capacities: Vec<u64>,
    amounts: Vec<u64>,
    eligible: Vec<Vec<bool>>,
    source_order: Vec<usize>,
    obligation_order: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingIdentityError {
    #[error("funding identity count differs from the captured matrix")]
    InvalidIdentityDimensions,
    #[error("funding identities must be nonempty canonical keys")]
    EmptyIdentity,
    #[error("funding repeats a physical custody identity before alias resolution")]
    DuplicateSource,
    #[error("equal obligation identities have different amounts or eligibility")]
    InconsistentObligation,
    #[error(transparent)]
    Search(#[from] FundingSearchError),
}

fn compare_keys(left: &[u8], right: &[u8]) -> Result<Ordering, FundingSearchError> {
    Ok(left.cmp(right))
}

pub fn key_order(keys: &[&[u8]]) -> Result<Vec<usize>, FundingSearchError> {
    let count = keys.len();
    let mut order = filled_vec(count, 0_usize)?;
    for (index, value) in order.iter_mut().enumerate() {
        *value = index;
    }
    let mut buffer = filled_vec(count, 0_usize)?;
    let mut width = 1;
    while width < count {
        let mut start = 0;
        while start < count {
            let middle = start + width.min(count - start);
            let end = middle + width.min(count - middle);
            let (mut left, mut right) = (start, middle);
            for output in &mut buffer[start..end] {
                let take_left = left < middle
                    && (right == end
                        || compare_keys(keys[order[left]], keys[order[right]])?
                            != Ordering::Greater);
                if take_left {
                    *output = order[left];
                    left += 1;
                } else {
                    *output = order[right];
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(&mut order, &mut buffer);
        width = width.checked_mul(2).unwrap_or(count);
    }
    let mut seen = filled_vec(count, false)?;
    for (position, index) in order.iter().copied().enumerate() {
        let visited = seen
            .get_mut(index)
            .ok_or(FundingSearchError::InvalidResult)?;
        if *visited {
            return Err(FundingSearchError::InvalidResult);
        }
        *visited = true;
        if position > 0
            && compare_keys(keys[order[position - 1]], keys[index])? == Ordering::Greater
        {
            return Err(FundingSearchError::InvalidResult);
        }
    }
    Ok(order)
}

pub fn canonicalize_funding_problem<'a>(
    problem: FundingMinimaxProblem<'_>,
    source_keys: &[&'a [u8]],
    obligation_keys: &[&'a [u8]],
    limits: FundingSearchLimits,
) -> Result<CanonicalFundingProblem<'a>, FundingIdentityError> {
    let n = problem.capacities.len();
    let m = problem.obligations.len();
    if n == 0 {
        return Err(FundingSearchError::from(FundingAssignmentError::EmptySources).into());
    }
    if n > limits.source_cap.get() {
        return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
    }
    if m > limits.obligation_cap.get() {
        return Err(FundingSearchError::from(FundingAssignmentError::TooManyObligations).into());
    }
    if source_keys.len() != n || obligation_keys.len() != m {
        return Err(FundingIdentityError::InvalidIdentityDimensions);
    }
    if problem.eligible.len() != n || problem.eligible.iter().any(|row| row.len() != m) {
        return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
    }
    if source_keys
        .iter()
        .chain(obligation_keys)
        .any(|key| key.is_empty())
    {
        return Err(FundingIdentityError::EmptyIdentity);
    }
    let source_order = key_order(source_keys)?;
    let obligation_order = key_order(obligation_keys)?;
    for pair in source_order.windows(2) {
        if compare_keys(source_keys[pair[0]], source_keys[pair[1]])? == Ordering::Equal {
            return Err(FundingIdentityError::DuplicateSource);
        }
    }
    for pair in obligation_order.windows(2) {
        if compare_keys(obligation_keys[pair[0]], obligation_keys[pair[1]])? == Ordering::Equal
            && (problem.obligations[pair[0]] != problem.obligations[pair[1]]
                || problem
                    .eligible
                    .iter()
                    .any(|row| row[pair[0]] != row[pair[1]]))
        {
            return Err(FundingIdentityError::InconsistentObligation);
        }
    }
    let mut sources = filled_vec(n, &[][..])?;
    let mut obligations = filled_vec(m, &[][..])?;
    let mut capacities = filled_vec(n, 0_u64)?;
    let mut amounts = filled_vec(m, 0_u64)?;
    let mut eligible = filled_vec(n, Vec::new())?;
    for (i, original) in source_order.iter().copied().enumerate() {
        sources[i] = source_keys[original];
        capacities[i] = problem.capacities[original];
        eligible[i] = filled_vec(m, false)?;
        for (j, target) in obligation_order.iter().copied().enumerate() {
            eligible[i][j] = problem.eligible[original][target];
        }
    }
    for (j, original) in obligation_order.iter().copied().enumerate() {
        obligations[j] = obligation_keys[original];
        amounts[j] = problem.obligations[original];
    }
    Ok(CanonicalFundingProblem {
        sources,
        obligations,
        capacities,
        amounts,
        eligible,
        source_order,
        obligation_order,
    })
}

impl<'a> CanonicalFundingProblem<'a> {
    pub fn original_source_positions(&self) -> &[usize] { &self.source_order }
    pub fn original_obligation_positions(&self) -> &[usize] { &self.obligation_order }
    pub fn source_keys(&self) -> &[&'a [u8]] { &self.sources }
    pub fn obligation_keys(&self) -> &[&'a [u8]] { &self.obligations }
    pub fn problem(&self) -> FundingMinimaxProblem<'_> {
        FundingMinimaxProblem {
            capacities: &self.capacities,
            obligations: &self.amounts,
            eligible: &self.eligible,
        }
    }
    pub fn select(
        &self,
        canonical_cursor: usize,
        limits: FundingSearchLimits,
    ) -> Result<FundingPolicyResult, FundingSearchError> {
        select_fixed_funding_policy(self.problem(), canonical_cursor, limits)
    }
    pub fn restore_assignment(
        &self,
        assignment: &[Vec<u64>],
    ) -> Result<Vec<Vec<u64>>, FundingSearchError> {
        let n = self.sources.len();
        let m = self.obligations.len();
        if assignment.len() != n || assignment.iter().any(|row| row.len() != m) {
            return Err(FundingAssignmentError::InvalidDimensions.into());
        }
        let mut original = filled_vec(n, Vec::new())?;
        for row in &mut original {
            *row = filled_vec(m, 0_u64)?;
        }
        for (i, source) in self.source_order.iter().copied().enumerate() {
            for (j, target) in self.obligation_order.iter().copied().enumerate() {
                original[source][target] = assignment[i][j];
            }
        }
        Ok(original)
    }

    pub fn verify_assignment(
        &self,
        canonical_cursor: usize,
        proposed: &[Vec<u64>],
        proposed_next_cursor: Option<usize>,
        limits: FundingSearchLimits,
    ) -> Result<bool, FundingSearchError> {
        let n = self.sources.len();
        let m = self.obligations.len();
        if proposed.len() != n || proposed.iter().any(|row| row.len() != m) {
            return Ok(false);
        }
        let FundingPolicyResult::Selected(selected) = self.select(canonical_cursor, limits)? else {
            return Ok(false);
        };
        if selected.next_cursor() != proposed_next_cursor {
            return Ok(false);
        }
        for (i, original_source) in self.source_order.iter().copied().enumerate() {
            for (j, original_obligation) in self.obligation_order.iter().copied().enumerate() {
                if proposed[original_source][original_obligation] != selected.assignment()[i][j] {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "funding_identity_tests.rs"]
mod tests;
