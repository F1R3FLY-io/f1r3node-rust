use super::funding_feasibility::filled_vec;
use super::{
    check_funding_assignment, solve_funding_feasibility, FundingAssignmentError,
    FundingAssignmentTotals, FundingFeasibility, FundingSearchError, FundingSearchLimits,
};

#[derive(Clone, Copy, Debug)]
pub struct FundingBoxProblem<'a> {
    pub lower: &'a [u64],
    pub upper: &'a [u64],
    pub obligations: &'a [u64],
    pub eligible: &'a [Vec<bool>],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingBoxFeasibility {
    Feasible {
        assignment: Vec<Vec<u64>>,
        totals: FundingAssignmentTotals,
    },
    UpperDeficit {
        selected_obligations: Vec<bool>,
    },
    LowerDeficit {
        selected_sources: Vec<bool>,
    },
}

impl FundingBoxProblem<'_> {
    pub(super) fn validate(self, limits: FundingSearchLimits) -> Result<(), FundingSearchError> {
        let sources = self.upper.len();
        let obligations = self.obligations.len();
        if sources == 0 {
            return Err(FundingAssignmentError::EmptySources.into());
        }
        if sources > limits.source_cap.get() {
            return Err(FundingAssignmentError::TooManySources.into());
        }
        if obligations > limits.obligation_cap.get() {
            return Err(FundingAssignmentError::TooManyObligations.into());
        }
        if self.lower.len() != sources
            || self.eligible.len() != sources
            || self.eligible.iter().any(|row| row.len() != obligations)
        {
            return Err(FundingAssignmentError::InvalidDimensions.into());
        }
        if self.lower.iter().zip(self.upper).any(|(lo, hi)| lo > hi) {
            return Err(FundingSearchError::InvalidSourceBounds);
        }
        self.obligations
            .iter()
            .try_fold(0_u64, |total, amount| total.checked_add(*amount))
            .ok_or(FundingAssignmentError::Overflow)?;
        Ok(())
    }

    fn verify_assignment(
        self,
        assignment: &[Vec<u64>],
        limits: FundingSearchLimits,
    ) -> Result<FundingAssignmentTotals, FundingSearchError> {
        check_funding_assignment(
            self.upper,
            self.obligations,
            self.eligible,
            assignment,
            limits.source_cap,
            limits.obligation_cap,
        )
        .map_err(|_| FundingSearchError::InvalidResult)
    }
}

pub fn check_lower_funding_cut(
    problem: FundingBoxProblem<'_>,
    selected: &[bool],
    limits: FundingSearchLimits,
) -> Result<bool, FundingSearchError> {
    problem.validate(limits)?;
    if selected.len() != problem.upper.len() {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    let requested = problem
        .lower
        .iter()
        .zip(selected)
        .filter(|(_, chosen)| **chosen)
        .try_fold(0_u128, |sum, (amount, _)| {
            sum.checked_add(u128::from(*amount))
        })
        .ok_or(FundingSearchError::Overflow)?;
    let mut available = 0_u128;
    for (j, amount) in problem.obligations.iter().enumerate() {
        if problem
            .eligible
            .iter()
            .zip(selected)
            .any(|(row, chosen)| *chosen && row[j])
        {
            available = available
                .checked_add(u128::from(*amount))
                .ok_or(FundingSearchError::Overflow)?;
        }
    }
    Ok(requested > available)
}

pub fn solve_box_funding_feasibility(
    problem: FundingBoxProblem<'_>,
    limits: FundingSearchLimits,
) -> Result<FundingBoxFeasibility, FundingSearchError> {
    problem.validate(limits)?;
    match solve_funding_feasibility(problem.upper, problem.obligations, problem.eligible, limits)? {
        FundingFeasibility::Infeasible {
            selected_obligations,
        } => Ok(FundingBoxFeasibility::UpperDeficit {
            selected_obligations,
        }),
        FundingFeasibility::Feasible { assignment, totals } => {
            rebalance(problem, assignment, totals, limits)
        }
    }
}

fn rebalance(
    problem: FundingBoxProblem<'_>,
    mut assignment: Vec<Vec<u64>>,
    mut totals: FundingAssignmentTotals,
    limits: FundingSearchLimits,
) -> Result<FundingBoxFeasibility, FundingSearchError> {
    const ABSENT: usize = usize::MAX;
    let sources = problem.upper.len();
    let vertices = sources
        .checked_add(problem.obligations.len())
        .ok_or(FundingSearchError::Overflow)?;
    let mut parents = filled_vec(vertices, ABSENT)?;
    let mut queue = filled_vec(vertices, 0_usize)?;
    queue.clear();
    let mut path = filled_vec(sources, (0_usize, 0_usize, 0_usize))?;
    path.clear();
    for receiver in 0..sources {
        while totals.source_debits()[receiver] < problem.lower[receiver] {
            parents.fill(ABSENT);
            parents[receiver] = receiver;
            queue.clear();
            queue.push(receiver);
            let mut cursor = 0;
            let mut donor = None;
            while cursor < queue.len() {
                let node = queue[cursor];
                cursor += 1;
                if node < sources {
                    if totals.source_debits()[node] > problem.lower[node] {
                        donor = Some(node);
                        break;
                    }
                    for (j, allowed) in problem.eligible[node].iter().enumerate() {
                        let target = sources + j;
                        if *allowed && parents[target] == ABSENT {
                            parents[target] = node;
                            queue.push(target);
                        }
                    }
                } else {
                    let j = node - sources;
                    for (i, row) in assignment.iter().enumerate() {
                        if row[j] > 0 && parents[i] == ABSENT {
                            parents[i] = node;
                            queue.push(i);
                        }
                    }
                }
            }
            let Some(donor) = donor else {
                let mut selected = filled_vec(sources, false)?;
                for (i, member) in selected.iter_mut().enumerate() {
                    *member = parents[i] != ABSENT;
                }
                if !check_lower_funding_cut(problem, &selected, limits)? {
                    return Err(FundingSearchError::InvalidResult);
                }
                return Ok(FundingBoxFeasibility::LowerDeficit {
                    selected_sources: selected,
                });
            };
            let mut amount = (problem.lower[receiver] - totals.source_debits()[receiver])
                .min(totals.source_debits()[donor] - problem.lower[donor]);
            path.clear();
            let mut node = donor;
            while node != receiver {
                let obligation_node = parents[node];
                if obligation_node < sources || obligation_node >= vertices || path.len() >= sources
                {
                    return Err(FundingSearchError::InvalidResult);
                }
                let prior = parents[obligation_node];
                if prior >= sources {
                    return Err(FundingSearchError::InvalidResult);
                }
                let j = obligation_node - sources;
                amount = amount.min(assignment[node][j]);
                path.push((prior, j, node));
                node = prior;
            }
            if amount == 0 {
                return Err(FundingSearchError::InvalidResult);
            }
            #[cfg(test)]
            let before = assignment.clone();
            for &(prior, j, next) in path.iter().rev() {
                assignment[next][j] = assignment[next][j]
                    .checked_sub(amount)
                    .ok_or(FundingSearchError::InvalidResult)?;
                assignment[prior][j] = assignment[prior][j]
                    .checked_add(amount)
                    .ok_or(FundingSearchError::InvalidResult)?;
            }
            let next = problem.verify_assignment(&assignment, limits)?;
            for (i, (&old, &new)) in totals
                .source_debits()
                .iter()
                .zip(next.source_debits())
                .enumerate()
            {
                let expected = if i == receiver {
                    old.checked_add(amount)
                } else if i == donor {
                    old.checked_sub(amount)
                } else {
                    Some(old)
                };
                if expected != Some(new) || new < old.min(problem.lower[i]) {
                    return Err(FundingSearchError::InvalidResult);
                }
            }
            #[cfg(test)]
            tests::assert_transition(
                problem,
                &before,
                &assignment,
                receiver,
                donor,
                amount,
                &path,
            );
            totals = next;
        }
    }
    Ok(FundingBoxFeasibility::Feasible { assignment, totals })
}

#[cfg(test)]
#[path = "funding_box_tests.rs"]
mod tests;
