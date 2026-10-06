use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use thiserror::Error;

use super::{CapturedPhloObligation, PhloObligationKey};
use crate::rust::interpreter::accounting::monetary_allocation::{
    filled_vec, reserve_work, FundingSearchError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct RetainedCellBackingLimits {
    pub cells: usize,
    pub contributions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedCellContribution {
    pub source_index: usize,
    pub amount: u64,
}

#[derive(Debug)]
pub struct CapturedRetainedCellBacking<'c, 'a> {
    obligation: CapturedPhloObligation<'c, 'a>,
    unit_value: u64,
    split: CellBacking,
}

#[derive(Debug, PartialEq, Eq)]
struct CellBacking {
    offsets: Vec<usize>,
    contributions: Vec<RetainedCellContribution>,
}

impl<'c, 'a> CapturedRetainedCellBacking<'c, 'a> {
    pub fn obligation(&self) -> CapturedPhloObligation<'c, 'a> { self.obligation }
    pub fn unit_value(&self) -> u64 { self.unit_value }
    pub fn contribution_count(&self) -> usize { self.split.contributions.len() }
    pub fn cells(&self) -> impl ExactSizeIterator<Item = &[RetainedCellContribution]> {
        self.split
            .offsets
            .windows(2)
            .map(|range| &self.split.contributions[range[0]..range[1]])
    }
}

#[derive(Debug, Error)]
pub enum RetainedCellBackingError {
    #[error("cell backing requires a retained-resource obligation")]
    NotRetained,
    #[error("cell backing requires a positive quantity and an exact unit value")]
    InvalidQuantity,
    #[error("cell backing exceeds its structural limit")]
    Limit,
    #[error("cell backing does not preserve the checked source and cell totals")]
    InconsistentAmounts,
    #[error(transparent)]
    Work(#[from] FundingSearchError),
}

impl<'c, 'a> CapturedPhloObligation<'c, 'a> {
    pub fn split_retained_cells(
        self,
        limits: RetainedCellBackingLimits,
        budget: &HostWorkBudget,
    ) -> Result<CapturedRetainedCellBacking<'c, 'a>, RetainedCellBackingError> {
        if !matches!(self.key(), PhloObligationKey::RetainedResource(_)) {
            return Err(RetainedCellBackingError::NotRetained);
        }
        let quantity = self.quantity();
        if quantity == 0 || !self.amount().is_multiple_of(quantity) {
            return Err(RetainedCellBackingError::InvalidQuantity);
        }
        let unit_value = self.amount() / quantity;
        let split = split_backing(
            quantity,
            unit_value,
            self.contributions().map(|(_, amount)| amount),
            limits,
            budget,
        )?;
        Ok(CapturedRetainedCellBacking {
            obligation: self,
            unit_value,
            split,
        })
    }
}

fn split_backing(
    quantity: u64,
    unit_value: u64,
    sources: impl ExactSizeIterator<Item = u64>,
    limits: RetainedCellBackingLimits,
    budget: &HostWorkBudget,
) -> Result<CellBacking, RetainedCellBackingError> {
    let count = usize::try_from(quantity).map_err(|_| RetainedCellBackingError::Limit)?;
    if count == 0 || count > limits.cells {
        return Err(RetainedCellBackingError::Limit);
    }
    let total = quantity
        .checked_mul(unit_value)
        .ok_or(FundingSearchError::Overflow)?;
    let offset_count = count.checked_add(1).ok_or(FundingSearchError::Overflow)?;
    let maximum = if unit_value == 0 {
        0
    } else {
        count
            .checked_add(sources.len())
            .and_then(|n| n.checked_sub(1))
            .ok_or(FundingSearchError::Overflow)?
            .min(limits.contributions)
    };
    let bytes = offset_count
        .checked_mul(size_of::<usize>())
        .and_then(|n| {
            maximum
                .checked_mul(size_of::<RetainedCellContribution>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(FundingSearchError::Overflow)?;
    let work = offset_count
        .checked_add(sources.len())
        .and_then(|n| n.checked_add(maximum))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
    let mut result = CellBacking {
        offsets: filled_vec(offset_count, 0usize)?,
        contributions: Vec::new(),
    };
    result
        .contributions
        .try_reserve_exact(maximum)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut cell = 0usize;
    let mut unfilled = unit_value;
    let mut remaining_total = total;
    for (source_index, mut amount) in sources.enumerate() {
        remaining_total = remaining_total
            .checked_sub(amount)
            .ok_or(RetainedCellBackingError::InconsistentAmounts)?;
        while amount > 0 {
            if cell >= count || unfilled == 0 {
                return Err(RetainedCellBackingError::InconsistentAmounts);
            }
            if result.contributions.len() >= maximum {
                return Err(RetainedCellBackingError::Limit);
            }
            let contribution = amount.min(unfilled);
            result.contributions.push(RetainedCellContribution {
                source_index,
                amount: contribution,
            });
            amount -= contribution;
            unfilled -= contribution;
            if unfilled == 0 {
                cell += 1;
                result.offsets[cell] = result.contributions.len();
                unfilled = unit_value;
            }
        }
    }
    if remaining_total != 0 || (unit_value != 0 && cell != count) {
        return Err(RetainedCellBackingError::InconsistentAmounts);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
