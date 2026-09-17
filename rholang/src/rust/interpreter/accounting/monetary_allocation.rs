use std::num::NonZeroUsize;

use thiserror::Error;

mod cohort;
mod cursor;
mod evidence;
mod funding_adjacency;
mod funding_arithmetic;
mod funding_augmentation;
mod funding_assignment;
mod funding_box;
mod funding_cell_bound;
mod funding_witness;
mod funding_deficit;
mod funding_discovery;
mod funding_domain;
mod funding_domain_witness;
mod funding_feasibility;
mod funding_graph;
mod funding_minimax;
mod funding_minimax_certificate;
mod funding_optimal_domain;
mod funding_cyclic_tie;
mod funding_rank;
mod funding_policy;
mod funding_fee_completion;
mod funding_fee_policy;
mod funding_family;
mod funding_family_priority;
mod funding_family_optimizer;
mod funding_family_slice;
mod funding_family_cursor;
pub use funding_family_cursor::{
    select_funding_family_policy, FundingFamilyPolicySelection, FundingOutcomeCursorTransition,
};
mod funding_identity;
mod funding_reservation;
mod funding_batch;
pub use cohort::{
    scope_for_custodies as monetary_scope_for_custodies, AllToAllContributionPlan, MonetaryCohort,
    MonetaryCohortAllocation, MonetaryCohortError, MonetaryCohortPlan, MonetaryPayer,
};
pub use cursor::{MonetaryCursor, MonetaryCursorError, MonetaryCursorTransition};
pub use evidence::{MonetaryFeeEvidence, MonetaryFeeFields, MONETARY_FEE_POLICY_VERSION};
pub use funding_assignment::{
    check_funding_assignment, FundingAssignmentError, FundingAssignmentTotals,
};
pub use funding_batch::{
    check_fixed_funding_batch, CheckedFixedFundingBatch, FixedFundingBatchEntry, FundingBatchError,
};
pub use funding_box::{
    check_lower_funding_cut, solve_box_funding_feasibility, FundingBoxFeasibility,
    FundingBoxProblem,
};
pub use funding_cell_bound::{
    check_funding_cell_deficit, solve_funding_cell_bound, FundingCellQuery,
};
pub use funding_cyclic_tie::{
    check_funding_cyclic_tie_certificate, select_funding_cyclic_tie, FundingCyclicTieCertificate,
    FundingCyclicTieSelection, FundingPrefixExclusion,
};
pub use funding_deficit::check_funding_deficit;
pub use funding_domain::{classify_fixed_funding_domain, FundingDomainClassification};
pub use funding_domain_witness::{
    build_funding_domain_counterexample, check_funding_domain_counterexample,
    FundingDomainCounterexample, FundingDomainCounterexampleView,
};
pub use funding_family::{
    solve_funding_family_feasibility, FundingFamilyError, FundingFamilyLimits,
    FundingFamilyProblem, FundingFamilyWitness,
};
pub use funding_family_optimizer::{
    optimize_funding_family, verify_funding_family_allocation, FundingFamilyAllocation,
    FundingFamilyOptimizationProblem, FundingFamilyProposal, FundingOutcomeAllocation,
    FundingOutcomeProblem, FundingOutcomeProposal,
};
pub use funding_family_priority::FundingFamilyResourcePriority;
pub(super) use funding_feasibility::{filled_vec, reserve_work};
pub use funding_feasibility::{
    solve_funding_feasibility, FundingFeasibility, FundingSearchError, FundingSearchLimits,
};
pub use funding_fee_completion::can_complete_unit_fee;
pub use funding_fee_policy::{select_funding_with_unit_fee, FundingFeeSelection};
pub use funding_identity::{
    canonicalize_funding_problem, key_order as canonical_funding_key_order,
    CanonicalFundingProblem, FundingFeeProposal, FundingIdentityError,
};
pub use funding_minimax::{solve_fixed_funding_minimax_rank, FundingMinimaxRankResult};
pub use funding_minimax_certificate::{
    certify_funding_minimax, check_funding_minimax_certificate, FundingExcessCut,
    FundingMinimaxCertificate, FundingMinimaxProblem,
};
pub use funding_optimal_domain::{derive_funding_optimal_domain, FundingOptimalDomain};
pub use funding_policy::{
    select_fixed_funding_policy, FundingPolicyFragment, FundingPolicyResult, FundingPolicySelection,
};
pub use funding_rank::{FundingBurdenError, FundingBurdenRank};
pub use funding_reservation::{FundingBranchReservation, FundingReservationError};
pub use funding_witness::{
    check_fixed_funding_witness, select_fixed_funding_witness, FundingEntryMinimum,
    FundingWitnessCertificate, FundingWitnessResult,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonetaryAllocation {
    pub debits: Vec<u64>,
    pub next_cursor: usize,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MonetaryAllocationError {
    #[error("monetary allocation requires at least one payer")]
    EmptyPayers,
    #[error("monetary allocation exceeds the configured payer cap")]
    TooManyPayers,
    #[error("monetary allocation cursor is outside the payer cohort")]
    InvalidCursor,
    #[error("monetary allocation exceeds available payer capacity")]
    InsufficientCapacity,
    #[error("monetary allocation arithmetic overflow")]
    Overflow,
}

fn capped_sum(capacities: &[u64], level: u64) -> Result<u128, MonetaryAllocationError> {
    capacities.iter().try_fold(0_u128, |total, capacity| {
        total
            .checked_add(u128::from((*capacity).min(level)))
            .ok_or(MonetaryAllocationError::Overflow)
    })
}

pub fn allocate_capped_max_min(
    capacities: &[u64],
    obligation: u64,
    cursor: usize,
    payer_cap: NonZeroUsize,
) -> Result<MonetaryAllocation, MonetaryAllocationError> {
    let count = capacities.len();
    if count == 0 {
        return Err(MonetaryAllocationError::EmptyPayers);
    }
    if count > payer_cap.get() {
        return Err(MonetaryAllocationError::TooManyPayers);
    }
    if cursor >= count {
        return Err(MonetaryAllocationError::InvalidCursor);
    }
    let target = u128::from(obligation);
    if capped_sum(capacities, obligation)? < target {
        return Err(MonetaryAllocationError::InsufficientCapacity);
    }

    let mut lower = 0_u64;
    let mut upper = obligation;
    while lower < upper {
        let middle = lower + (upper - lower).div_ceil(2);
        if capped_sum(capacities, middle)? <= target {
            lower = middle;
        } else {
            upper = middle - 1;
        }
    }

    let mut debits: Vec<_> = capacities
        .iter()
        .map(|capacity| (*capacity).min(lower))
        .collect();
    let mut residual = target - capped_sum(capacities, lower)?;
    let mut next_cursor = cursor;
    let mut payer = cursor;
    for _ in 0..count {
        if residual == 0 {
            break;
        }
        let following = if payer == count - 1 { 0 } else { payer + 1 };
        if capacities[payer] > lower {
            debits[payer] = debits[payer]
                .checked_add(1)
                .ok_or(MonetaryAllocationError::Overflow)?;
            residual -= 1;
            next_cursor = following;
        }
        payer = following;
    }
    if residual != 0 {
        return Err(MonetaryAllocationError::InsufficientCapacity);
    }
    Ok(MonetaryAllocation {
        debits,
        next_cursor,
    })
}

#[cfg(test)]
mod tests;
