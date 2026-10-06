use std::num::NonZeroUsize;

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingAssignmentTotals {
    source_debits: Vec<u64>,
    total: u64,
}

impl FundingAssignmentTotals {
    pub fn source_debits(&self) -> &[u64] { &self.source_debits }

    pub fn total(&self) -> u64 { self.total }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingAssignmentError {
    #[error("funding assignment requires at least one physical source")]
    EmptySources,
    #[error("funding assignment exceeds the configured source cap")]
    TooManySources,
    #[error("funding assignment exceeds the configured obligation cap")]
    TooManyObligations,
    #[error("funding assignment dimensions do not match sources and obligations")]
    InvalidDimensions,
    #[error("funding assignment uses an ineligible source-to-obligation edge")]
    IneligibleDraw,
    #[error("funding assignment exceeds a source capacity")]
    InsufficientSourceCapacity,
    #[error("funding assignment does not exactly cover an obligation")]
    ObligationMismatch,
    #[error("funding assignment arithmetic overflow")]
    Overflow,
}

pub fn check_funding_assignment(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    assignment: &[Vec<u64>],
    source_cap: NonZeroUsize,
    obligation_cap: NonZeroUsize,
) -> Result<FundingAssignmentTotals, FundingAssignmentError> {
    let sources = capacities.len();
    let count = obligations.len();
    if sources == 0 {
        return Err(FundingAssignmentError::EmptySources);
    }
    if sources > source_cap.get() {
        return Err(FundingAssignmentError::TooManySources);
    }
    if count > obligation_cap.get() {
        return Err(FundingAssignmentError::TooManyObligations);
    }
    if eligible.len() != sources
        || assignment.len() != sources
        || eligible.iter().any(|row| row.len() != count)
        || assignment.iter().any(|row| row.len() != count)
    {
        return Err(FundingAssignmentError::InvalidDimensions);
    }

    let mut source_debits = Vec::with_capacity(sources);
    let mut funded = vec![0_u64; count];
    let mut total = 0_u64;
    for ((row, permitted), capacity) in assignment.iter().zip(eligible).zip(capacities) {
        let mut debit = 0_u64;
        for ((amount, allowed), obligation_total) in row.iter().zip(permitted).zip(&mut funded) {
            if *amount != 0 && !allowed {
                return Err(FundingAssignmentError::IneligibleDraw);
            }
            debit = debit
                .checked_add(*amount)
                .ok_or(FundingAssignmentError::Overflow)?;
            *obligation_total = obligation_total
                .checked_add(*amount)
                .ok_or(FundingAssignmentError::Overflow)?;
        }
        if debit > *capacity {
            return Err(FundingAssignmentError::InsufficientSourceCapacity);
        }
        total = total
            .checked_add(debit)
            .ok_or(FundingAssignmentError::Overflow)?;
        source_debits.push(debit);
    }
    if funded != obligations {
        return Err(FundingAssignmentError::ObligationMismatch);
    }
    Ok(FundingAssignmentTotals {
        source_debits,
        total,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

    fn check(
        capacities: &[u64],
        obligations: &[u64],
        eligible: &[Vec<bool>],
        assignment: &[Vec<u64>],
    ) -> Result<FundingAssignmentTotals, FundingAssignmentError> {
        check_funding_assignment(
            capacities,
            obligations,
            eligible,
            assignment,
            cap(64),
            cap(64),
        )
    }

    fn reference_valid(
        capacities: &[u64],
        obligations: &[u64],
        eligible: &[Vec<bool>],
        assignment: &[Vec<u64>],
    ) -> bool {
        let rows: Vec<u128> = assignment
            .iter()
            .map(|row| row.iter().copied().map(u128::from).sum())
            .collect();
        let columns: Vec<u128> = (0..obligations.len())
            .map(|column| assignment.iter().map(|row| u128::from(row[column])).sum())
            .collect();
        rows.iter().sum::<u128>() <= u128::from(u64::MAX)
            && rows
                .iter()
                .zip(capacities)
                .all(|(draw, cap)| *draw <= u128::from(*cap))
            && columns
                .iter()
                .zip(obligations)
                .all(|(draw, need)| *draw == u128::from(*need))
            && assignment.iter().zip(eligible).all(|(row, allowed)| {
                row.iter()
                    .zip(allowed)
                    .all(|(amount, permitted)| *amount == 0 || *permitted)
            })
    }

    #[test]
    fn greedy_failure_does_not_imply_infeasibility() {
        let eligible = vec![vec![true, true], vec![true, false]];
        let valid = check(&[1, 1], &[1, 1], &eligible, &[vec![0, 1], vec![1, 0]]).unwrap();
        assert_eq!(valid.source_debits(), &[1, 1]);
        assert_eq!(valid.total(), 2);
        assert_eq!(
            check(&[1, 1], &[1, 1], &eligible, &[vec![1, 1], vec![0, 0]]),
            Err(FundingAssignmentError::InsufficientSourceCapacity)
        );
    }

    #[test]
    fn aggregate_equal_shares_do_not_override_eligibility() {
        let eligible = vec![vec![true, true], vec![false, true]];
        let valid = check(&[2, 10], &[2, 1], &eligible, &[vec![2, 0], vec![0, 1]]).unwrap();
        assert_eq!(valid.source_debits(), &[2, 1]);
        assert_eq!(
            check(&[2, 10], &[2, 1], &eligible, &[vec![1, 0], vec![1, 1]]),
            Err(FundingAssignmentError::IneligibleDraw)
        );
    }

    #[test]
    fn restricted_exclusive_branches_need_distinct_reservations() {
        let first = vec![vec![true], vec![false]];
        let second = vec![vec![false], vec![true]];
        let a = check(&[1, 1], &[1], &first, &[vec![1], vec![0]]).unwrap();
        let b = check(&[1, 1], &[1], &second, &[vec![0], vec![1]]).unwrap();
        assert_eq!(a.total(), 1);
        assert_eq!(b.total(), 1);
        let reserve: u64 = a
            .source_debits()
            .iter()
            .zip(b.source_debits())
            .map(|(a, b)| (*a).max(*b))
            .sum();
        assert_eq!(reserve, 2);
        assert!(check(&[1, 0], &[1], &second, &[vec![0], vec![1]]).is_err());
    }

    #[test]
    fn dimensions_and_independent_caps_are_enforced() {
        assert_eq!(
            check(&[], &[], &[], &[]),
            Err(FundingAssignmentError::EmptySources)
        );
        assert_eq!(
            check(&[1], &[1], &[], &[vec![1]]),
            Err(FundingAssignmentError::InvalidDimensions)
        );
        assert_eq!(
            check(&[1], &[1], &[vec![true]], &[vec![]]),
            Err(FundingAssignmentError::InvalidDimensions)
        );
        assert_eq!(
            check(&[1], &[1], &[vec![]], &[vec![1]]),
            Err(FundingAssignmentError::InvalidDimensions)
        );
        assert_eq!(
            check_funding_assignment(
                &[1, 1],
                &[],
                &[vec![], vec![]],
                &[vec![], vec![]],
                cap(1),
                cap(2)
            ),
            Err(FundingAssignmentError::TooManySources)
        );
        assert_eq!(
            check_funding_assignment(
                &[2],
                &[1, 1],
                &[vec![true, true]],
                &[vec![1, 1]],
                cap(1),
                cap(1)
            ),
            Err(FundingAssignmentError::TooManyObligations)
        );
        assert_eq!(check(&[0], &[], &[vec![]], &[vec![]]).unwrap().total(), 0);
    }

    #[test]
    fn arithmetic_does_not_wrap_or_saturate() {
        let max = u64::MAX;
        assert_eq!(
            check(&[max], &[max], &[vec![true]], &[vec![max]])
                .unwrap()
                .total(),
            max
        );
        assert_eq!(
            check(&[max], &[max, 1], &[vec![true, true]], &[vec![max, 1]]),
            Err(FundingAssignmentError::Overflow)
        );
        assert_eq!(
            check(&[max, 1], &[max], &[vec![true], vec![true]], &[
                vec![max],
                vec![1]
            ]),
            Err(FundingAssignmentError::Overflow)
        );
        assert_eq!(
            check(
                &[max, 1],
                &[max, 1],
                &[vec![true, true], vec![true, true]],
                &[vec![max, 0], vec![0, 1]]
            ),
            Err(FundingAssignmentError::Overflow)
        );
    }

    #[test]
    fn obligation_coverage_is_exact() {
        assert_eq!(
            check(&[4], &[3], &[vec![true]], &[vec![2]]),
            Err(FundingAssignmentError::ObligationMismatch)
        );
        assert_eq!(
            check(&[4], &[3], &[vec![true]], &[vec![4]]),
            Err(FundingAssignmentError::ObligationMismatch)
        );
        assert_eq!(
            check(&[0], &[0], &[vec![false]], &[vec![0]])
                .unwrap()
                .total(),
            0
        );
    }

    #[test]
    fn every_configured_source_arity_is_supported() {
        for maximum in [1, 3, 64, 128] {
            for count in 1..=maximum {
                let capacities = vec![3; count];
                let obligations = [count as u64, 2 * count as u64];
                let eligible = vec![vec![true, true]; count];
                let assignment = vec![vec![1, 2]; count];
                let result = check_funding_assignment(
                    &capacities,
                    &obligations,
                    &eligible,
                    &assignment,
                    cap(maximum),
                    cap(2),
                )
                .unwrap();
                assert_eq!(result.source_debits(), capacities);
                assert_eq!(result.total(), 3 * count as u64);
            }
            assert_eq!(
                check_funding_assignment(
                    &vec![0; maximum + 1],
                    &[],
                    &vec![vec![]; maximum + 1],
                    &vec![vec![]; maximum + 1],
                    cap(maximum),
                    cap(2),
                ),
                Err(FundingAssignmentError::TooManySources)
            );
        }
    }

    #[test]
    fn complete_small_candidate_sets_match_the_formal_feasibility_oracle() {
        for left in 0_u64..=2 {
            for right in 0_u64..=2 {
                for graph in 0_u8..16 {
                    let a = graph & 1 != 0;
                    let b = graph & 2 != 0;
                    let c = graph & 4 != 0;
                    let d = graph & 8 != 0;
                    let eligible = vec![vec![a, b], vec![c, d]];
                    let expected = (left >= 2 && a && b)
                        || (right >= 2 && c && d)
                        || (left >= 1 && right >= 1 && ((a && d) || (b && c)));
                    let mut accepted = 0;
                    for encoded in 0_u64..81 {
                        let flows = vec![vec![encoded % 3, (encoded / 3) % 3], vec![
                            (encoded / 9) % 3,
                            (encoded / 27) % 3,
                        ]];
                        let checked = check(&[left, right], &[1, 1], &eligible, &flows);
                        assert_eq!(
                            checked.is_ok(),
                            reference_valid(&[left, right], &[1, 1], &eligible, &flows)
                        );
                        if let Ok(candidate) = checked {
                            accepted += 1;
                            assert_eq!(candidate.total(), 2);
                            assert!(flows
                                .iter()
                                .flatten()
                                .all(|amount| *amount <= candidate.total()));
                        }
                    }
                    assert_eq!(
                        accepted > 0,
                        expected,
                        "capacities=({left},{right}), graph={graph}"
                    );
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn assignments_match_independent_wide_integer_oracle(
            raw in any::<[[u8; 3]; 3]>(), allowed in any::<[[bool; 3]; 3]>(),
            capacities in any::<[u16; 3]>(), demands in any::<[u16; 3]>()
        ) {
            let flows: Vec<Vec<u64>> = raw.iter().map(|row| row.iter().map(|v| u64::from(*v)).collect()).collect();
            let eligible: Vec<Vec<bool>> = allowed.iter().map(|row| row.to_vec()).collect();
            let caps: Vec<u64> = capacities.iter().map(|v| u64::from(*v)).collect();
            let needs: Vec<u64> = demands.iter().map(|v| u64::from(*v)).collect();
            prop_assert_eq!(check(&caps, &needs, &eligible, &flows).is_ok(), reference_valid(&caps, &needs, &eligible, &flows));
        }

        #[test]
        fn feasible_witnesses_conserve_and_reorder(
            raw in any::<[[u8; 3]; 3]>(), extra in any::<[u8; 3]>()
        ) {
            let mut flows: Vec<Vec<u64>> = raw.iter().map(|row| row.iter().map(|v| u64::from(*v)).collect()).collect();
            let mut eligible: Vec<Vec<bool>> = flows.iter().map(|row| row.iter().map(|v| *v != 0).collect()).collect();
            let mut caps: Vec<u64> = flows.iter().zip(extra).map(|(row, slack)| row.iter().sum::<u64>() + u64::from(slack)).collect();
            let mut needs: Vec<u64> = (0..3).map(|col| flows.iter().map(|row| row[col]).sum()).collect();
            let before = check(&caps, &needs, &eligible, &flows).unwrap();
            prop_assert_eq!(before.total(), needs.iter().sum::<u64>());
            prop_assert!(reference_valid(&caps, &needs, &eligible, &flows));
            caps.reverse(); flows.reverse(); eligible.reverse(); needs.reverse();
            for row in &mut flows { row.reverse(); }
            for row in &mut eligible { row.reverse(); }
            let after = check(&caps, &needs, &eligible, &flows).unwrap();
            let mut expected = before.source_debits().to_vec(); expected.reverse();
            prop_assert_eq!(after.source_debits(), expected.as_slice());
            prop_assert_eq!(before.total(), after.total());
        }
    }
}
