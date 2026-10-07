use super::super::{select_funding_with_unit_fee, FundingFeeSelection};
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct FundingFeeProposal<'a> {
    pub resource_assignment: &'a [Vec<u64>],
    pub fee_debits: &'a [u64],
    pub resource_next_cursor: Option<usize>,
    pub fee_next_cursor: usize,
}

impl CanonicalFundingProblem<'_> {
    pub fn select_with_unit_fee(
        &self,
        original_fee_eligible: &[bool],
        canonical_resource_cursor: usize,
        canonical_fee_cursor: usize,
        limits: FundingSearchLimits,
        budget: &HostWorkBudget,
    ) -> Result<Option<FundingFeeSelection>, FundingSearchError> {
        let count = self.sources.len();
        if original_fee_eligible.len() != count {
            return Err(FundingAssignmentError::InvalidDimensions.into());
        }
        reserve_work(budget, HostWorkDimension::SearchStateBytes, count)?;
        reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
        let mut fee_eligible = filled_vec(count, false)?;
        for (position, original) in self.source_order.iter().copied().enumerate() {
            fee_eligible[position] = original_fee_eligible[original];
        }
        select_funding_with_unit_fee(
            self.problem(),
            &fee_eligible,
            canonical_resource_cursor,
            canonical_fee_cursor,
            limits,
            budget,
        )
    }

    pub fn verify_with_unit_fee(
        &self,
        original_fee_eligible: &[bool],
        canonical_resource_cursor: usize,
        canonical_fee_cursor: usize,
        proposed: FundingFeeProposal<'_>,
        limits: FundingSearchLimits,
        budget: &HostWorkBudget,
    ) -> Result<bool, FundingSearchError> {
        let n = self.sources.len();
        let m = self.obligations.len();
        reserve_work(budget, HostWorkDimension::VerificationOperations, n)?;
        if proposed.resource_assignment.len() != n
            || proposed.fee_debits.len() != n
            || proposed
                .resource_assignment
                .iter()
                .any(|row| row.len() != m)
        {
            return Ok(false);
        }
        let Some(selected) = self.select_with_unit_fee(
            original_fee_eligible,
            canonical_resource_cursor,
            canonical_fee_cursor,
            limits,
            budget,
        )?
        else {
            return Ok(false);
        };
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(m.checked_add(1).ok_or(FundingSearchError::Overflow)?)
                .and_then(|count| count.checked_add(2))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        if selected.resource_next_cursor() != proposed.resource_next_cursor
            || selected.fee().next_cursor != proposed.fee_next_cursor
        {
            return Ok(false);
        }
        for (i, source) in self.source_order.iter().copied().enumerate() {
            if proposed.fee_debits[source] != selected.fee().debits[i] {
                return Ok(false);
            }
            for (j, obligation) in self.obligation_order.iter().copied().enumerate() {
                if proposed.resource_assignment[source][obligation]
                    != selected.resource_assignment()[i][j]
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "funding_identity_fee_tests.rs"]
mod tests;
