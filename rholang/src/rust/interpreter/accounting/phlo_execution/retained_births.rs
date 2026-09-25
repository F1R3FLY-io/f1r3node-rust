use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use prost::Message;
use thiserror::Error;

use super::{CanonicalPhloFundingCapture, CapturedPhloObligation, PhloObligationKey};
use crate::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, reserve_authority_signature_tree, AuthorityBornStack, AuthorityError,
};
use crate::rust::interpreter::accounting::monetary_allocation::{
    filled_vec, reserve_work, FundingSearchError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct RetainedBirthFunding<'a> {
    pub birth: &'a AuthorityBornStack,
    pub obligation_positions: &'a [usize],
}

#[derive(Clone, Copy, Debug)]
pub struct RetainedBirthFundingLimits {
    pub births: usize,
    pub cells: usize,
    pub obligations: usize,
    pub authority_bytes: usize,
}

#[derive(Debug)]
pub struct CheckedRetainedBirthFunding<'c, 'a, 'b> {
    capture: &'c CanonicalPhloFundingCapture<'a>,
    births: Vec<RetainedBirthFunding<'b>>,
}

impl<'c, 'a, 'b> CheckedRetainedBirthFunding<'c, 'a, 'b> {
    pub fn capture(&self) -> &'c CanonicalPhloFundingCapture<'a> { self.capture }

    pub fn births(&self) -> &[RetainedBirthFunding<'b>] { &self.births }

    pub fn cells(
        &self,
    ) -> impl Iterator<
        Item = (
            &'b AuthorityBornStack,
            usize,
            CapturedPhloObligation<'c, 'a>,
        ),
    > + '_ {
        let capture = self.capture;
        self.births.iter().flat_map(move |binding| {
            binding
                .obligation_positions
                .iter()
                .enumerate()
                .map(move |(index, position)| {
                    (
                        binding.birth,
                        index,
                        capture
                            .obligations()
                            .nth(*position)
                            .expect("checked birth obligation"),
                    )
                })
        })
    }
}

#[derive(Debug, Error)]
pub enum RetainedBirthFundingError {
    #[error("retained birth funding exceeds its structural limit")]
    Limit,
    #[error("retained birth funding repeats a physical stack or produce identity")]
    DuplicateBirth,
    #[error("retained birth funding does not cover each physical cell")]
    CellCount,
    #[error("retained birth funding names an unknown obligation")]
    UnknownObligation,
    #[error("retained birth funding cannot use consumed resources or fees")]
    NotRetained,
    #[error("retained birth cell authority differs from its funded resource")]
    AuthorityMismatch,
    #[error("retained birth quantities differ from the checked funding capture")]
    QuantityMismatch,
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
}

impl<'a> CanonicalPhloFundingCapture<'a> {
    pub fn bind_retained_births<'c, 'b>(
        &'c self,
        bindings: &[RetainedBirthFunding<'b>],
        limits: RetainedBirthFundingLimits,
        budget: &HostWorkBudget,
    ) -> Result<CheckedRetainedBirthFunding<'c, 'a, 'b>, RetainedBirthFundingError> {
        let columns = self.obligations().len();
        if bindings.len() > limits.births || columns > limits.obligations {
            return Err(RetainedBirthFundingError::Limit);
        }
        let mut cell_count = 0usize;
        for binding in bindings {
            if binding.birth.cells.is_empty()
                || binding.birth.cells.len() != binding.obligation_positions.len()
            {
                return Err(RetainedBirthFundingError::CellCount);
            }
            cell_count = cell_count
                .checked_add(binding.birth.cells.len())
                .filter(|n| *n <= limits.cells)
                .ok_or(RetainedBirthFundingError::Limit)?;
        }
        let bytes = bindings
            .len()
            .checked_mul(size_of::<RetainedBirthFunding>())
            .and_then(|n| {
                columns
                    .checked_mul(size_of::<u64>())
                    .and_then(|m| n.checked_add(m))
            })
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        let work = bindings
            .len()
            .checked_mul(128 * (1 + bindings.len().checked_ilog2().unwrap_or(0) as usize))
            .and_then(|n| n.checked_add(cell_count))
            .and_then(|n| n.checked_add(columns))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
        let mut births = Vec::new();
        births
            .try_reserve_exact(bindings.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        births.extend_from_slice(bindings);
        births.sort_unstable_by_key(|binding| binding.birth.produce_hash);
        if births
            .windows(2)
            .any(|pair| pair[0].birth.produce_hash == pair[1].birth.produce_hash)
        {
            return Err(RetainedBirthFundingError::DuplicateBirth);
        }
        births.sort_unstable_by_key(|binding| binding.birth.stack_id);
        if births
            .windows(2)
            .any(|pair| pair[0].birth.stack_id == pair[1].birth.stack_id)
        {
            return Err(RetainedBirthFundingError::DuplicateBirth);
        }
        let mut quantities = filled_vec(columns, 0u64)?;
        let mut remaining_bytes = limits.authority_bytes;
        let mut depth = 0;
        for binding in &births {
            for (cell, position) in binding.birth.cells.iter().zip(binding.obligation_positions) {
                let obligation = self
                    .obligations()
                    .nth(*position)
                    .ok_or(RetainedBirthFundingError::UnknownObligation)?;
                let PhloObligationKey::RetainedResource(resource) = obligation.key() else {
                    return Err(RetainedBirthFundingError::NotRetained);
                };
                reserve_authority_signature_tree(cell, budget, &mut depth)?;
                let bytes = cell.encoded_len();
                remaining_bytes = remaining_bytes
                    .checked_sub(bytes)
                    .ok_or(RetainedBirthFundingError::Limit)?;
                reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
                reserve_work(budget, HostWorkDimension::VerificationOperations, bytes)?;
                if cost_signature_to_sig(cell)? != *resource.authority {
                    return Err(RetainedBirthFundingError::AuthorityMismatch);
                }
                quantities[*position] = quantities[*position]
                    .checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?;
            }
        }
        for (obligation, actual) in self.obligations().zip(quantities) {
            let expected = match obligation.key() {
                PhloObligationKey::RetainedResource(_) => obligation.quantity(),
                _ => 0,
            };
            if actual != expected {
                return Err(RetainedBirthFundingError::QuantityMismatch);
            }
        }
        Ok(CheckedRetainedBirthFunding {
            capture: self,
            births,
        })
    }
}
