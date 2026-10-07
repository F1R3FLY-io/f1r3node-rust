use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::reserve_work;
use super::{
    allocate_capped_max_min, certify_funding_minimax, check_funding_cyclic_tie_certificate,
    check_funding_minimax_certificate, classify_fixed_funding_domain,
    derive_funding_optimal_domain, select_fixed_funding_witness, select_funding_cyclic_tie,
    solve_fixed_funding_minimax_rank, FundingAssignmentError, FundingAssignmentTotals,
    FundingCyclicTieCertificate, FundingDomainClassification, FundingDomainCounterexample,
    FundingMinimaxCertificate, FundingMinimaxProblem, FundingMinimaxRankResult, FundingSearchError,
    FundingSearchLimits, FundingWitnessCertificate, FundingWitnessResult,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingPolicyFragment {
    Unrestricted,
    Restricted {
        counterexample: FundingDomainCounterexample,
        contribution_certificate: FundingCyclicTieCertificate,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingPolicySelection {
    assignment: Vec<Vec<u64>>,
    totals: FundingAssignmentTotals,
    fragment: FundingPolicyFragment,
    rank_certificate: FundingMinimaxCertificate,
    witness_certificate: FundingWitnessCertificate,
    next_cursor: Option<usize>,
}

impl FundingPolicySelection {
    pub(super) fn into_resource_parts(
        self,
    ) -> (
        Vec<Vec<u64>>,
        FundingAssignmentTotals,
        FundingPolicyFragment,
        Option<usize>,
    ) {
        (
            self.assignment,
            self.totals,
            self.fragment,
            self.next_cursor,
        )
    }

    pub fn assignment(&self) -> &[Vec<u64>] { &self.assignment }
    pub fn totals(&self) -> &FundingAssignmentTotals { &self.totals }
    pub fn fragment(&self) -> &FundingPolicyFragment { &self.fragment }
    pub fn rank_certificate(&self) -> &FundingMinimaxCertificate { &self.rank_certificate }
    pub fn witness_certificate(&self) -> &FundingWitnessCertificate { &self.witness_certificate }
    pub fn next_cursor(&self) -> Option<usize> { self.next_cursor }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingPolicyResult {
    Selected(FundingPolicySelection),
    Infeasible { selected_obligations: Vec<bool> },
}

pub(super) fn canonical_assignment(
    problem: FundingMinimaxProblem<'_>,
    contributions: &[u64],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<
    (
        Vec<Vec<u64>>,
        FundingAssignmentTotals,
        FundingWitnessCertificate,
    ),
    FundingSearchError,
> {
    match select_fixed_funding_witness(
        FundingMinimaxProblem {
            capacities: contributions,
            ..problem
        },
        limits,
        budget,
    )? {
        FundingWitnessResult::Canonical {
            assignment,
            totals,
            certificate,
        } => {
            if problem.check_assignment(&assignment, limits, budget)? != totals {
                return Err(FundingSearchError::InvalidResult);
            }
            Ok((assignment, totals, certificate))
        }
        FundingWitnessResult::Infeasible { .. } => Err(FundingSearchError::InvalidResult),
    }
}

pub(super) struct RestrictedFundingCandidate {
    pub assignment: Vec<Vec<u64>>,
    pub totals: FundingAssignmentTotals,
    pub rank_certificate: FundingMinimaxCertificate,
    pub witness_certificate: FundingWitnessCertificate,
    pub contribution_certificate: FundingCyclicTieCertificate,
}

pub(super) fn select_restricted_candidate(
    problem: FundingMinimaxProblem<'_>,
    cursor: usize,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<Option<RestrictedFundingCandidate>, FundingSearchError> {
    let FundingMinimaxRankResult::Optimal {
        assignment,
        certificate,
        ..
    } = solve_fixed_funding_minimax_rank(problem, limits, budget)?
    else {
        return Ok(None);
    };
    let domain =
        derive_funding_optimal_domain(problem, &assignment, certificate.cuts(), limits, budget)?;
    let contribution = select_funding_cyclic_tie(&domain, cursor, limits, budget)?;
    let (assignment, totals, witness_certificate) =
        canonical_assignment(problem, contribution.totals.source_debits(), limits, budget)?;
    if domain.check_assignment(&assignment, limits, budget)? != totals
        || !check_funding_minimax_certificate(
            problem,
            &assignment,
            certificate.cuts(),
            limits,
            budget,
        )?
        || !check_funding_cyclic_tie_certificate(
            &domain,
            &assignment,
            cursor,
            contribution.certificate.exclusions(),
            limits,
            budget,
        )?
    {
        return Err(FundingSearchError::InvalidResult);
    }
    Ok(Some(RestrictedFundingCandidate {
        assignment,
        totals,
        rank_certificate: certificate,
        witness_certificate,
        contribution_certificate: contribution.certificate,
    }))
}

pub fn select_fixed_funding_policy(
    problem: FundingMinimaxProblem<'_>,
    cursor: usize,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingPolicyResult, FundingSearchError> {
    let count = problem.capacities.len();
    if count == 0 {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if count > limits.source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if cursor >= count {
        return Err(FundingSearchError::InvalidPriorityCursor);
    }
    match classify_fixed_funding_domain(
        problem.capacities,
        problem.obligations,
        problem.eligible,
        limits,
        budget,
    )? {
        FundingDomainClassification::Infeasible {
            selected_obligations,
        } => Ok(FundingPolicyResult::Infeasible {
            selected_obligations,
        }),
        FundingDomainClassification::Unrestricted { totals, .. } => {
            reserve_work(
                budget,
                HostWorkDimension::SearchCandidates,
                count.checked_mul(68).ok_or(FundingSearchError::Overflow)?,
            )?;
            reserve_work(
                budget,
                HostWorkDimension::SearchStateBytes,
                count
                    .checked_mul(size_of::<u64>())
                    .ok_or(FundingSearchError::Overflow)?,
            )?;
            let allocation = allocate_capped_max_min(
                problem.capacities,
                totals.total(),
                cursor,
                limits.source_cap,
            )
            .map_err(|_| FundingSearchError::InvalidResult)?;
            let (assignment, totals, witness_certificate) =
                canonical_assignment(problem, &allocation.debits, limits, budget)?;
            let rank_certificate = certify_funding_minimax(problem, &assignment, limits, budget)?
                .ok_or(FundingSearchError::InvalidResult)?;
            let next_cursor = (totals.total() != 0).then_some(allocation.next_cursor);
            Ok(FundingPolicyResult::Selected(FundingPolicySelection {
                assignment,
                totals,
                fragment: FundingPolicyFragment::Unrestricted,
                rank_certificate,
                witness_certificate,
                next_cursor,
            }))
        }
        FundingDomainClassification::Restricted { counterexample, .. } => {
            let selected = select_restricted_candidate(problem, cursor, limits, budget)?
                .ok_or(FundingSearchError::InvalidResult)?;
            let next_cursor = (selected.totals.total() != 0).then_some(if cursor == count - 1 {
                0
            } else {
                cursor + 1
            });
            Ok(FundingPolicyResult::Selected(FundingPolicySelection {
                assignment: selected.assignment,
                totals: selected.totals,
                fragment: FundingPolicyFragment::Restricted {
                    counterexample,
                    contribution_certificate: selected.contribution_certificate,
                },
                rank_certificate: selected.rank_certificate,
                witness_certificate: selected.witness_certificate,
                next_cursor,
            }))
        }
    }
}

#[cfg(test)]
#[path = "funding_policy_tests.rs"]
mod tests;
