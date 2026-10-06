use std::cmp::Ordering;
use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::{
    filled_vec, reserve_work, FundingAssignmentError, FundingFamilyError, FundingFamilyLimits,
    FundingSearchError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingFamilyResourcePriority {
    ranks: Vec<u64>,
    ties: Vec<u64>,
    amounts: Vec<u64>,
    sources: usize,
    cursor: usize,
}

impl FundingFamilyResourcePriority {
    pub fn from_canonical_resource_draws(
        outcomes: &[&[u64]],
        cursor: usize,
        limits: FundingFamilyLimits,
        budget: &HostWorkBudget,
    ) -> Result<Self, FundingFamilyError> {
        let Some(first) = outcomes.first() else {
            return Err(FundingFamilyError::EmptyCases);
        };
        let sources = first.len();
        if outcomes.len() > limits.case_cap.get() {
            return Err(FundingFamilyError::TooManyCases);
        }
        if sources == 0 {
            return Err(FundingSearchError::from(FundingAssignmentError::EmptySources).into());
        }
        if sources > limits.search.source_cap.get() {
            return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
        }
        if cursor >= sources {
            return Err(FundingSearchError::InvalidPriorityCursor.into());
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            outcomes.len(),
        )?;
        if outcomes.iter().any(|row| row.len() != sources) {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        let cells = sources
            .checked_mul(outcomes.len())
            .ok_or(FundingSearchError::Overflow)?;
        let bytes = cells
            .checked_mul(2)
            .and_then(|n| n.checked_add(outcomes.len()))
            .and_then(|n| n.checked_mul(size_of::<u64>()))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            cells
                .checked_mul(sources.checked_add(2).ok_or(FundingSearchError::Overflow)?)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut ranks = filled_vec(cells, 0)?;
        let mut ties = filled_vec(cells, 0)?;
        let mut amounts = filled_vec(outcomes.len(), 0_u64)?;
        for (index, row) in outcomes.iter().enumerate() {
            amounts[index] = row.iter().try_fold(0_u64, |total, value| {
                total
                    .checked_add(*value)
                    .ok_or(FundingSearchError::Overflow)
            })?;
            let start = index * sources;
            let rank = &mut ranks[start..start + sources];
            rank.copy_from_slice(row);
            rank.sort_unstable_by(|a, b| b.cmp(a));
            for (target, value) in ties[start..start + sources]
                .iter_mut()
                .zip(row[cursor..].iter().chain(&row[..cursor]))
            {
                *target = *value;
            }
        }
        Ok(Self {
            ranks,
            ties,
            amounts,
            sources,
            cursor,
        })
    }

    pub fn compare(
        &self,
        other: &Self,
        budget: &HostWorkBudget,
    ) -> Result<Ordering, FundingFamilyError> {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            self.ranks
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(self.amounts.len()))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        if self.sources != other.sources
            || self.cursor != other.cursor
            || self.amounts != other.amounts
        {
            return Err(FundingFamilyError::IncompatiblePriorityContext);
        }
        Ok(self
            .ranks
            .cmp(&other.ranks)
            .then_with(|| other.ties.cmp(&self.ties)))
    }
}

#[cfg(test)]
#[path = "funding_family_priority_tests.rs"]
mod tests;
