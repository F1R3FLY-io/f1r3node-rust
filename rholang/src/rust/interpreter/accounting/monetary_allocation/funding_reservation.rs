use std::num::NonZeroUsize;

use thiserror::Error;

use super::FundingAssignmentTotals;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingReservationError {
    #[error("funding reservation requires physical sources")]
    EmptySources,
    #[error("funding reservation exceeds the configured source cap")]
    TooManySources,
    #[error("funding reservation requires certified branches")]
    EmptyBranches,
    #[error("funding reservation exceeds the configured branch cap")]
    TooManyBranches,
    #[error("funding reservation dimensions do not match")]
    InvalidDimensions,
    #[error("funding reservation exceeds source capacity")]
    InsufficientCapacity,
    #[error("funding reservation exceeds permitted source exposure")]
    ExposureExceeded,
    #[error("funding settlement selects an unknown branch")]
    UnknownBranch,
    #[error("funding reservation arithmetic overflow")]
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingBranchReservation {
    source_holds: Vec<u64>,
    branches: Vec<FundingAssignmentTotals>,
    total_held: u128,
    maximum_charge: u64,
}

impl FundingBranchReservation {
    pub fn from_plans(
        capacities: &[u64],
        exposure_limits: &[u64],
        branches: Vec<FundingAssignmentTotals>,
        source_cap: NonZeroUsize,
        branch_cap: NonZeroUsize,
    ) -> Result<Self, FundingReservationError> {
        let count = capacities.len();
        if count == 0 {
            return Err(FundingReservationError::EmptySources);
        }
        if count > source_cap.get() {
            return Err(FundingReservationError::TooManySources);
        }
        if branches.is_empty() {
            return Err(FundingReservationError::EmptyBranches);
        }
        if branches.len() > branch_cap.get() {
            return Err(FundingReservationError::TooManyBranches);
        }
        if exposure_limits.len() != count
            || branches
                .iter()
                .any(|plan| plan.source_debits().len() != count)
        {
            return Err(FundingReservationError::InvalidDimensions);
        }
        let mut source_holds = vec![0_u64; count];
        let mut maximum_charge = 0;
        for branch in &branches {
            maximum_charge = maximum_charge.max(branch.total());
            for (held, debit) in source_holds.iter_mut().zip(branch.source_debits()) {
                *held = (*held).max(*debit);
            }
        }
        let mut total_held = 0_u128;
        for ((held, capacity), permitted) in
            source_holds.iter().zip(capacities).zip(exposure_limits)
        {
            if held > capacity {
                return Err(FundingReservationError::InsufficientCapacity);
            }
            if held > permitted {
                return Err(FundingReservationError::ExposureExceeded);
            }
            total_held = total_held
                .checked_add(u128::from(*held))
                .ok_or(FundingReservationError::Overflow)?;
        }
        Ok(Self {
            source_holds,
            branches,
            total_held,
            maximum_charge,
        })
    }

    pub fn source_holds(&self) -> &[u64] { &self.source_holds }

    pub fn total_held(&self) -> u128 { self.total_held }

    pub fn maximum_charge(&self) -> u64 { self.maximum_charge }

    pub fn branch(
        &self,
        selected: usize,
    ) -> Result<&FundingAssignmentTotals, FundingReservationError> {
        self.branches
            .get(selected)
            .ok_or(FundingReservationError::UnknownBranch)
    }

    pub fn refunds(&self, selected: usize) -> Result<Vec<u64>, FundingReservationError> {
        let branch = self.branch(selected)?;
        self.source_holds
            .iter()
            .zip(branch.source_debits())
            .map(|(held, debit)| {
                held.checked_sub(*debit)
                    .ok_or(FundingReservationError::InsufficientCapacity)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::rust::interpreter::accounting::monetary_allocation::check_funding_assignment;

    fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

    fn plan(draws: &[u64]) -> FundingAssignmentTotals {
        let assignment: Vec<_> = draws.iter().map(|draw| vec![*draw]).collect();
        check_funding_assignment(
            draws,
            &[draws.iter().sum()],
            &vec![vec![true]; draws.len()],
            &assignment,
            cap(draws.len()),
            cap(1),
        )
        .unwrap()
    }

    fn reserve(
        capacities: &[u64],
        exposure: &[u64],
        branches: Vec<FundingAssignmentTotals>,
    ) -> Result<FundingBranchReservation, FundingReservationError> {
        FundingBranchReservation::from_plans(capacities, exposure, branches, cap(129), cap(64))
    }

    #[test]
    fn restricted_branches_preserve_each_original_sources_refund() {
        let left = check_funding_assignment(
            &[1, 1],
            &[1],
            &[vec![true], vec![false]],
            &[vec![1], vec![0]],
            cap(2),
            cap(1),
        )
        .unwrap();
        let right = check_funding_assignment(
            &[1, 1],
            &[1],
            &[vec![false], vec![true]],
            &[vec![0], vec![1]],
            cap(2),
            cap(1),
        )
        .unwrap();
        let branches = vec![left, right];
        assert_eq!(
            reserve(&[1, 1], &[1, 0], branches.clone()),
            Err(FundingReservationError::ExposureExceeded),
        );
        let reservation = reserve(&[1, 1], &[1, 1], branches).unwrap();
        assert_eq!(reservation.source_holds(), &[1, 1]);
        assert_eq!(reservation.maximum_charge(), 1);
        assert_eq!(reservation.total_held(), 2);
        assert_eq!(reservation.refunds(0).unwrap(), vec![0, 1]);
        assert_eq!(reservation.refunds(1).unwrap(), vec![1, 0]);
        assert_eq!(
            reservation.refunds(2),
            Err(FundingReservationError::UnknownBranch)
        );
    }

    #[test]
    fn wide_reservation_totals_do_not_expand_the_retained_charge() {
        let reservation = reserve(&[u64::MAX, u64::MAX], &[u64::MAX, u64::MAX], vec![
            plan(&[u64::MAX, 0]),
            plan(&[0, u64::MAX]),
        ])
        .unwrap();
        assert_eq!(reservation.total_held(), 2 * u128::from(u64::MAX));
        assert_eq!(reservation.maximum_charge(), u64::MAX);
        assert_eq!(reservation.refunds(0).unwrap(), vec![0, u64::MAX]);
        assert_eq!(reservation.refunds(1).unwrap(), vec![u64::MAX, 0]);
    }

    #[test]
    fn reservation_validates_dimensions_caps_capacity_and_exposure() {
        assert_eq!(
            reserve(&[], &[], vec![]),
            Err(FundingReservationError::EmptySources)
        );
        assert_eq!(
            reserve(&[1], &[1], vec![]),
            Err(FundingReservationError::EmptyBranches)
        );
        assert_eq!(
            reserve(&[1], &[], vec![plan(&[1])]),
            Err(FundingReservationError::InvalidDimensions)
        );
        assert_eq!(
            reserve(&[1], &[1], vec![plan(&[1, 0])]),
            Err(FundingReservationError::InvalidDimensions)
        );
        assert_eq!(
            reserve(&[0], &[1], vec![plan(&[1])]),
            Err(FundingReservationError::InsufficientCapacity)
        );
        assert_eq!(
            reserve(&[1], &[0], vec![plan(&[1])]),
            Err(FundingReservationError::ExposureExceeded)
        );
        assert_eq!(
            FundingBranchReservation::from_plans(
                &[1, 1],
                &[1, 1],
                vec![plan(&[1, 0])],
                cap(1),
                cap(1)
            ),
            Err(FundingReservationError::TooManySources)
        );
        assert_eq!(
            FundingBranchReservation::from_plans(
                &[1],
                &[1],
                vec![plan(&[1]), plan(&[1])],
                cap(1),
                cap(1)
            ),
            Err(FundingReservationError::TooManyBranches)
        );
    }

    #[test]
    fn reservation_supports_all_source_counts_through_the_configured_cap() {
        for count in 1..=129 {
            let draws = vec![1; count];
            let reservation = reserve(&draws, &draws, vec![plan(&draws)]).unwrap();
            assert_eq!(reservation.source_holds(), draws);
            assert_eq!(reservation.refunds(0).unwrap(), vec![0; count]);
            assert_eq!(reservation.total_held(), count as u128);
        }
        let zero = reserve(&[0], &[0], vec![plan(&[0])]).unwrap();
        assert_eq!(zero.total_held(), 0);
        assert_eq!(zero.maximum_charge(), 0);
        assert_eq!(zero.refunds(0).unwrap(), vec![0]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn branch_reservations_match_maximum_and_refund_oracles(
            rows in (1_usize..33, 1_usize..9).prop_flat_map(|(sources, branches)|
                proptest::collection::vec(proptest::collection::vec(0_u64..100, sources), branches)),
        ) {
            let sources = rows[0].len();
            let expected: Vec<u64> = (0..sources).map(|source| rows.iter().map(|row| row[source]).max().unwrap()).collect();
            let plans: Vec<_> = rows.iter().map(|row| plan(row)).collect();
            let before = plans.clone();
            let reservation = reserve(&expected, &expected, plans).unwrap();
            prop_assert_eq!(reservation.source_holds(), expected.as_slice());
            prop_assert_eq!(reservation.total_held(), expected.iter().map(|amount| u128::from(*amount)).sum::<u128>());
            for (index, row) in rows.iter().enumerate() {
                let refund = reservation.refunds(index).unwrap();
                for source in 0..sources {
                    prop_assert_eq!(refund[source] + row[source], expected[source]);
                }
                prop_assert_eq!(refund.iter().map(|amount| u128::from(*amount)).sum::<u128>() + u128::from(reservation.branch(index).unwrap().total()), reservation.total_held());
                prop_assert_eq!(reservation.branch(index).unwrap(), &before[index]);
            }
            let mut reversed = before;
            reversed.reverse();
            let reordered = reserve(&expected, &expected, reversed).unwrap();
            prop_assert_eq!(reordered.source_holds(), reservation.source_holds());
            prop_assert_eq!(reordered.maximum_charge(), reservation.maximum_charge());
            for (source, held) in expected.iter().enumerate() {
                if *held > 0 {
                    let mut insufficient = expected.clone();
                    insufficient[source] -= 1;
                    prop_assert_eq!(reserve(&expected, &insufficient, rows.iter().map(|row| plan(row)).collect()), Err(FundingReservationError::ExposureExceeded));
                }
            }
        }
    }
}
