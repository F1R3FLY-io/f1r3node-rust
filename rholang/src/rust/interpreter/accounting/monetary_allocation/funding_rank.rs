use std::cmp::Ordering;

use thiserror::Error;

use super::FundingAssignmentTotals;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingBurdenRank {
    descending_contributions: Vec<u64>,
    total: u64,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingBurdenError {
    #[error("funding burden comparison requires the same source count")]
    SourceCountMismatch,
    #[error("funding burden comparison requires the same total obligation")]
    ObligationMismatch,
}

impl FundingBurdenRank {
    pub fn from_assignment(assignment: &FundingAssignmentTotals) -> Self {
        let mut descending_contributions = assignment.source_debits().to_vec();
        descending_contributions.sort_unstable_by(|left, right| right.cmp(left));
        Self {
            descending_contributions,
            total: assignment.total(),
        }
    }

    pub fn descending_contributions(&self) -> &[u64] { &self.descending_contributions }

    pub fn total(&self) -> u64 { self.total }

    pub fn compare(&self, other: &Self) -> Result<Ordering, FundingBurdenError> {
        if self.descending_contributions.len() != other.descending_contributions.len() {
            return Err(FundingBurdenError::SourceCountMismatch);
        }
        if self.total != other.total {
            return Err(FundingBurdenError::ObligationMismatch);
        }
        Ok(self
            .descending_contributions
            .cmp(&other.descending_contributions))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use proptest::prelude::*;

    use super::*;
    use crate::rust::interpreter::accounting::monetary_allocation::{
        allocate_capped_max_min, check_funding_assignment,
    };

    fn assignment(values: &[u64]) -> FundingAssignmentTotals {
        let total = values
            .iter()
            .try_fold(0_u64, |sum, value| sum.checked_add(*value))
            .unwrap();
        check_funding_assignment(
            values,
            &[total],
            &vec![vec![true]; values.len()],
            &values.iter().map(|value| vec![*value]).collect::<Vec<_>>(),
            NonZeroUsize::new(values.len()).unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap()
    }

    fn rank(values: &[u64]) -> FundingBurdenRank {
        FundingBurdenRank::from_assignment(&assignment(values))
    }

    fn histogram_comparison(left: &[u64], right: &[u64]) -> Ordering {
        let mut counts = BTreeMap::<u64, (usize, usize)>::new();
        for value in left {
            counts.entry(*value).or_default().0 += 1;
        }
        for value in right {
            counts.entry(*value).or_default().1 += 1;
        }
        counts
            .values()
            .rev()
            .find_map(|(lhs, rhs)| (lhs != rhs).then(|| lhs.cmp(rhs)))
            .unwrap_or(Ordering::Equal)
    }

    #[test]
    fn minimax_rejects_leximin_and_maximum_only_rankings() {
        assert_eq!(
            rank(&[0, 5, 5]).compare(&rank(&[1, 1, 8])),
            Ok(Ordering::Less)
        );
        assert_eq!(
            rank(&[1, 1, 8]).compare(&rank(&[0, 5, 5])),
            Ok(Ordering::Greater)
        );
        assert_eq!(
            rank(&[5, 3, 2]).compare(&rank(&[5, 4, 1])),
            Ok(Ordering::Less)
        );
    }

    #[test]
    fn equal_rank_does_not_replace_the_source_assignment() {
        let original = assignment(&[3, 3, 2]);
        let alternate = assignment(&[2, 3, 3]);
        assert_ne!(original, alternate);
        let first = FundingBurdenRank::from_assignment(&original);
        let second = FundingBurdenRank::from_assignment(&alternate);
        assert_eq!(first.compare(&second), Ok(Ordering::Equal));
        assert_eq!(first.descending_contributions(), &[3, 3, 2]);
        assert_eq!(first.total(), 8);
        assert_eq!(original.source_debits(), &[3, 3, 2]);
        assert_eq!(alternate.source_debits(), &[2, 3, 3]);
    }

    #[test]
    fn different_cohort_sizes_and_obligations_are_not_comparable() {
        assert_eq!(
            rank(&[2]).compare(&rank(&[1, 1])),
            Err(FundingBurdenError::SourceCountMismatch)
        );
        assert_eq!(
            rank(&[2, 0]).compare(&rank(&[1, 0])),
            Err(FundingBurdenError::ObligationMismatch)
        );
    }

    #[test]
    fn exact_integer_boundaries_and_zero_contributions() {
        assert_eq!(
            rank(&[u64::MAX, 0]).compare(&rank(&[u64::MAX - 1, 1])),
            Ok(Ordering::Greater)
        );
        assert_eq!(
            rank(&[u64::MAX]).compare(&rank(&[u64::MAX])),
            Ok(Ordering::Equal)
        );
        for sources in 1..=129 {
            let values = vec![0; sources];
            let empty = rank(&values);
            assert_eq!(empty.descending_contributions(), values);
            assert_eq!(empty.compare(&empty), Ok(Ordering::Equal));
        }
    }

    #[test]
    fn exhaustive_small_fixed_total_comparisons_match_histogram_oracle() {
        for total in 0..=8 {
            let mut candidates = Vec::new();
            for first in 0..=total {
                for second in 0..=total - first {
                    let values = [first, second, total - first - second];
                    candidates.push((values, rank(&values)));
                }
            }
            for (left, left_rank) in &candidates {
                for (right, right_rank) in &candidates {
                    assert_eq!(
                        left_rank.compare(right_rank),
                        Ok(histogram_comparison(left, right))
                    );
                }
            }
        }
    }

    #[test]
    fn all_to_all_allocator_matches_complete_small_feasible_sets() {
        fn decode(mut encoded: usize, count: usize) -> Vec<u64> {
            (0..count)
                .map(|_| {
                    let digit = u64::try_from(encoded % 4).unwrap();
                    encoded /= 4;
                    digit
                })
                .collect()
        }

        for count in 1_u32..=4 {
            let sources = usize::try_from(count).unwrap();
            let combinations = 4_usize.pow(count);
            for encoded_capacity in 0..combinations {
                let capacities = decode(encoded_capacity, sources);
                let maximum = usize::try_from(capacities.iter().sum::<u64>()).unwrap();
                let mut best = vec![None::<Vec<u64>>; maximum + 1];
                for encoded_draw in 0..combinations {
                    let draws = decode(encoded_draw, sources);
                    if draws
                        .iter()
                        .zip(&capacities)
                        .any(|(draw, capacity)| draw > capacity)
                    {
                        continue;
                    }
                    let total = usize::try_from(draws.iter().sum::<u64>()).unwrap();
                    let replace = match &best[total] {
                        None => true,
                        Some(prior) => histogram_comparison(&draws, prior) == Ordering::Less,
                    };
                    if replace {
                        best[total] = Some(draws);
                    }
                }
                for (total, optimum) in best.iter().enumerate() {
                    let optimum = optimum.as_ref().unwrap();
                    for cursor in 0..sources {
                        let allocation = allocate_capped_max_min(
                            &capacities,
                            u64::try_from(total).unwrap(),
                            cursor,
                            NonZeroUsize::new(sources).unwrap(),
                        )
                        .unwrap();
                        assert_eq!(
                            histogram_comparison(&allocation.debits, optimum),
                            Ordering::Equal
                        );
                        assert_eq!(
                            rank(&allocation.debits).compare(&rank(optimum)),
                            Ok(Ordering::Equal)
                        );
                    }
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn generated_rank_matches_independent_oracle_and_order_laws(
            sources in 1_usize..130,
            owners in proptest::collection::vec((any::<u16>(), any::<u16>(), any::<u16>()), 0..257),
        ) {
            let mut left = vec![0_u64; sources];
            let mut middle = vec![0_u64; sources];
            let mut right = vec![0_u64; sources];
            for (a, b, c) in &owners {
                left[usize::from(*a) % sources] += 1;
                middle[usize::from(*b) % sources] += 1;
                right[usize::from(*c) % sources] += 1;
            }
            let lhs = rank(&left);
            let mid = rank(&middle);
            let rhs = rank(&right);
            let lm = lhs.compare(&mid).unwrap();
            let mr = mid.compare(&rhs).unwrap();
            let lr = lhs.compare(&rhs).unwrap();
            prop_assert_eq!(lm, histogram_comparison(&left, &middle));
            prop_assert_eq!(mr, histogram_comparison(&middle, &right));
            prop_assert_eq!(lr, histogram_comparison(&left, &right));
            prop_assert_eq!(lhs.compare(&lhs), Ok(Ordering::Equal));
            prop_assert_eq!(mid.compare(&lhs), Ok(lm.reverse()));
            if lm != Ordering::Greater && mr != Ordering::Greater {
                prop_assert_ne!(lr, Ordering::Greater);
            }
            left.reverse();
            prop_assert_eq!(lhs.compare(&rank(&left)), Ok(Ordering::Equal));
            prop_assert_eq!(lhs.total(), u64::try_from(owners.len()).unwrap());
        }
    }
}
