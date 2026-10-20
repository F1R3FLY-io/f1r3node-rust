use std::num::NonZeroUsize;

use super::FundingAssignmentError;

pub fn check_funding_deficit(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    selected: &[bool],
    source_cap: NonZeroUsize,
    obligation_cap: NonZeroUsize,
) -> Result<bool, FundingAssignmentError> {
    if capacities.is_empty() {
        return Err(FundingAssignmentError::EmptySources);
    }
    if capacities.len() > source_cap.get() {
        return Err(FundingAssignmentError::TooManySources);
    }
    if obligations.len() > obligation_cap.get() {
        return Err(FundingAssignmentError::TooManyObligations);
    }
    if selected.len() != obligations.len()
        || eligible.len() != capacities.len()
        || eligible.iter().any(|row| row.len() != obligations.len())
    {
        return Err(FundingAssignmentError::InvalidDimensions);
    }
    let mut total = 0_u64;
    let mut demand = 0_u64;
    for (&amount, &chosen) in obligations.iter().zip(selected) {
        total = total
            .checked_add(amount)
            .ok_or(FundingAssignmentError::Overflow)?;
        if chosen {
            demand = demand
                .checked_add(amount)
                .ok_or(FundingAssignmentError::Overflow)?;
        }
    }
    let mut supply = 0_u128;
    for (&capacity, permitted) in capacities.iter().zip(eligible) {
        if permitted
            .iter()
            .zip(selected)
            .any(|(allowed, chosen)| *allowed && *chosen)
        {
            supply = supply
                .checked_add(u128::from(capacity))
                .ok_or(FundingAssignmentError::Overflow)?;
        }
    }
    Ok(supply < u128::from(demand))
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
        selected: &[bool],
    ) -> Result<bool, FundingAssignmentError> {
        check_funding_deficit(
            capacities,
            obligations,
            eligible,
            selected,
            cap(128),
            cap(128),
        )
    }

    #[test]
    fn unrelated_wealth_does_not_disprove_a_deficit() {
        assert_eq!(
            check(&[1, 100], &[2], &[vec![true], vec![false]], &[true]),
            Ok(true)
        );
        assert_eq!(
            check(&[2, 100], &[2], &[vec![true], vec![false]], &[true]),
            Ok(false)
        );
    }

    #[test]
    fn shared_capacity_is_counted_once_and_all_neighbors_are_included() {
        assert_eq!(
            check(&[3], &[2, 2], &[vec![true, true]], &[true, true]),
            Ok(true)
        );
        assert_eq!(
            check(&[1, 3], &[2, 2], &[vec![true, true], vec![true, true]], &[
                true, true
            ]),
            Ok(false)
        );
        assert_eq!(
            check(&[1, 1], &[1, 1], &[vec![true, true], vec![true, false]], &[
                true, true
            ]),
            Ok(false)
        );
    }

    #[test]
    fn zero_and_wide_capacity_boundaries_are_exact() {
        assert_eq!(check(&[0], &[], &[vec![]], &[]), Ok(false));
        assert_eq!(check(&[0], &[1], &[vec![true]], &[false]), Ok(false));
        assert_eq!(check(&[0], &[0], &[vec![false]], &[true]), Ok(false));
        assert_eq!(
            check(
                &[u64::MAX, u64::MAX],
                &[u64::MAX],
                &[vec![true], vec![true]],
                &[true]
            ),
            Ok(false)
        );
        assert_eq!(
            check(&[u64::MAX - 1], &[u64::MAX], &[vec![true]], &[true]),
            Ok(true)
        );
        assert_eq!(
            check(&[0], &[u64::MAX, 1], &[vec![true, true]], &[false, true]),
            Err(FundingAssignmentError::Overflow)
        );
    }

    #[test]
    fn malformed_problems_are_not_reported_as_infeasible() {
        assert_eq!(
            check(&[], &[], &[], &[]),
            Err(FundingAssignmentError::EmptySources)
        );
        assert_eq!(
            check(&[0], &[1], &[], &[true]),
            Err(FundingAssignmentError::InvalidDimensions)
        );
        assert_eq!(
            check(&[0], &[1], &[vec![]], &[true]),
            Err(FundingAssignmentError::InvalidDimensions)
        );
        assert_eq!(
            check(&[0], &[1], &[vec![true]], &[]),
            Err(FundingAssignmentError::InvalidDimensions)
        );
        assert_eq!(
            check_funding_deficit(&[0, 0], &[], &[vec![], vec![]], &[], cap(1), cap(1)),
            Err(FundingAssignmentError::TooManySources)
        );
        assert_eq!(
            check_funding_deficit(
                &[0],
                &[1, 1],
                &[vec![true, true]],
                &[true, true],
                cap(1),
                cap(1)
            ),
            Err(FundingAssignmentError::TooManyObligations)
        );
    }

    #[test]
    fn complete_small_allocations_agree_with_deficit_certificates() {
        for mask in 0..16 {
            let eligible = vec![vec![mask & 1 != 0, mask & 2 != 0], vec![
                mask & 4 != 0,
                mask & 8 != 0,
            ]];
            for c0 in 0..=2 {
                for c1 in 0..=2 {
                    for q0 in 0..=2 {
                        for q1 in 0..=2 {
                            let feasible = (0..=q0).any(|a| {
                                (0..=q1).any(|b| {
                                    a + b <= c0
                                        && q0 - a + q1 - b <= c1
                                        && (a == 0 || eligible[0][0])
                                        && (b == 0 || eligible[0][1])
                                        && (q0 == a || eligible[1][0])
                                        && (q1 == b || eligible[1][1])
                                })
                            });
                            let mut has_deficit = false;
                            for subset in 0..4 {
                                let selected = [subset & 1 != 0, subset & 2 != 0];
                                let deficit =
                                    check(&[c0, c1], &[q0, q1], &eligible, &selected).unwrap();
                                assert!(!(deficit && feasible));
                                has_deficit |= deficit;
                            }
                            assert_eq!(has_deficit, !feasible);
                        }
                    }
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn every_cut_of_a_valid_assignment_has_enough_capacity(
            sources in 1_usize..129,
            obligations_count in 0_usize..33,
            seed in prop::collection::vec(any::<u16>(), 1..257),
            choices in prop::collection::vec(any::<bool>(), 1..65),
        ) {
            let mut capacities = vec![0_u64; sources];
            let mut demands = vec![0_u64; obligations_count];
            let mut eligible = vec![vec![false; obligations_count]; sources];
            for (i, capacity) in capacities.iter_mut().enumerate() {
                for (j, demand) in demands.iter_mut().enumerate() {
                    let index = i * obligations_count + j;
                    let amount = u64::from(seed[index % seed.len()]);
                    *capacity += amount;
                    *demand += amount;
                    eligible[i][j] = amount != 0 || choices[index % choices.len()];
                }
            }
            let selected: Vec<bool> = (0..obligations_count).map(|j| choices[j % choices.len()]).collect();
            prop_assert_eq!(check(&capacities, &demands, &eligible, &selected), Ok(false));
            capacities.reverse();
            eligible.reverse();
            demands.reverse();
            eligible.iter_mut().for_each(|row| row.reverse());
            let reversed: Vec<bool> = selected.into_iter().rev().collect();
            prop_assert_eq!(check(&capacities, &demands, &eligible, &reversed), Ok(false));
        }

        #[test]
        fn arbitrary_cuts_match_independent_wide_integer_reference(
            capacities in prop::collection::vec(any::<u64>(), 1..129),
            demands in prop::collection::vec(0_u64..1_000_000, 0..33),
            bits in prop::collection::vec(any::<bool>(), 1..257),
        ) {
            let eligible: Vec<Vec<bool>> = (0..capacities.len()).map(|i|
                (0..demands.len()).map(|j| bits[(i * demands.len() + j) % bits.len()]).collect()
            ).collect();
            let selected: Vec<bool> = (0..demands.len()).map(|j| bits[j % bits.len()]).collect();
            let chosen: Vec<usize> = (0..demands.len()).filter(|j| selected[*j]).collect();
            let demand: u128 = chosen.iter().map(|j| u128::from(demands[*j])).sum();
            let neighbors: std::collections::BTreeSet<usize> = chosen.iter().flat_map(|j|
                (0..capacities.len()).filter(|i| eligible[*i][*j])
            ).collect();
            let supply: u128 = neighbors.iter().map(|i| u128::from(capacities[*i])).sum();
            prop_assert_eq!(check(&capacities, &demands, &eligible, &selected), Ok(demand > supply));
        }
    }
}
