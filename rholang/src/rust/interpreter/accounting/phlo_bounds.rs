use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumericPhloBounds {
    signed_charge_ceiling: u64,
    schedule_charge_bound: u64,
}

impl NumericPhloBounds {
    pub fn signed_charge_ceiling(self) -> u64 { self.signed_charge_ceiling }

    pub fn schedule_charge_bound(self) -> u64 { self.schedule_charge_bound }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloBoundsError {
    #[error("phlo bounds require owner price consent")]
    MissingOwnerConsent,
    #[error("resource bound exceeds the signed phlo limit")]
    LimitExceeded,
    #[error("schedule price exceeds the signed phlo price ceiling")]
    PriceCeilingExceeded,
    #[error("schedule price exceeds a required owner's price ceiling")]
    OwnerPriceCeilingExceeded,
    #[error("phlo charge arithmetic overflow")]
    ChargeOverflow,
    #[error("signed charge ceiling exceeds the machine maximum")]
    MachineMaximumExceeded,
}

pub fn check_numeric_phlo_bounds(
    machine_max: u64,
    limit: u64,
    ceiling: u64,
    price: u64,
    bound: u64,
    owners: &[u64],
) -> Result<NumericPhloBounds, PhloBoundsError> {
    if owners.is_empty() {
        return Err(PhloBoundsError::MissingOwnerConsent);
    }
    if bound > limit {
        return Err(PhloBoundsError::LimitExceeded);
    }
    if price > ceiling {
        return Err(PhloBoundsError::PriceCeilingExceeded);
    }
    if owners.iter().any(|owner| price > *owner) {
        return Err(PhloBoundsError::OwnerPriceCeilingExceeded);
    }
    let signed_charge_ceiling = limit
        .checked_mul(ceiling)
        .and_then(|amount| amount.checked_add(1))
        .ok_or(PhloBoundsError::ChargeOverflow)?;
    if signed_charge_ceiling > machine_max {
        return Err(PhloBoundsError::MachineMaximumExceeded);
    }
    let schedule_charge_bound = bound
        .checked_mul(price)
        .and_then(|amount| amount.checked_add(1))
        .ok_or(PhloBoundsError::ChargeOverflow)?;
    Ok(NumericPhloBounds {
        signed_charge_ceiling,
        schedule_charge_bound,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn reference(
        machine_max: u64,
        limit: u64,
        ceiling: u64,
        price: u64,
        bound: u64,
        owners: &[u64],
    ) -> Option<(u128, u128)> {
        let signed = u128::from(limit) * u128::from(ceiling) + 1;
        let scheduled = u128::from(bound) * u128::from(price) + 1;
        (!owners.is_empty()
            && bound <= limit
            && price <= ceiling
            && owners.iter().all(|owner| price <= *owner)
            && signed <= u128::from(machine_max))
        .then_some((signed, scheduled))
    }

    fn values(result: Result<NumericPhloBounds, PhloBoundsError>) -> Option<(u128, u128)> {
        result.ok().map(|checked| {
            (
                u128::from(checked.signed_charge_ceiling()),
                u128::from(checked.schedule_charge_bound()),
            )
        })
    }

    #[test]
    fn signed_and_actual_price_bounds_remain_distinct() {
        let checked = check_numeric_phlo_bounds(31, 10, 3, 2, 10, &[3, 5, 8]).unwrap();
        assert_eq!(checked.signed_charge_ceiling(), 31);
        assert_eq!(checked.schedule_charge_bound(), 21);
        let tighter = check_numeric_phlo_bounds(31, 10, 3, 2, 6, &[3, 5, 8]).unwrap();
        assert_eq!(tighter.signed_charge_ceiling(), 31);
        assert_eq!(tighter.schedule_charge_bound(), 13);
    }

    #[test]
    fn required_owner_count_never_multiplies_the_bill() {
        let expected = check_numeric_phlo_bounds(31, 10, 3, 2, 10, &[3]);
        for count in 1..=256 {
            assert_eq!(
                check_numeric_phlo_bounds(31, 10, 3, 2, 10, &vec![3; count]),
                expected,
            );
        }
        assert_eq!(
            check_numeric_phlo_bounds(31, 10, 3, 2, 10, &[]),
            Err(PhloBoundsError::MissingOwnerConsent),
        );
    }

    #[test]
    fn every_required_owner_can_reject_an_otherwise_funded_draw() {
        for count in [1, 2, 3, 64, 65, 129] {
            for position in 0..count {
                let mut owners = vec![3; count];
                owners[position] = 1;
                assert_eq!(
                    check_numeric_phlo_bounds(31, 10, 3, 2, 10, &owners),
                    Err(PhloBoundsError::OwnerPriceCeilingExceeded),
                );
            }
        }
    }

    #[test]
    fn each_numeric_rejection_is_explicit() {
        assert_eq!(
            check_numeric_phlo_bounds(31, 10, 3, 2, 11, &[3]),
            Err(PhloBoundsError::LimitExceeded),
        );
        assert_eq!(
            check_numeric_phlo_bounds(31, 10, 3, 4, 10, &[4]),
            Err(PhloBoundsError::PriceCeilingExceeded),
        );
        assert_eq!(
            check_numeric_phlo_bounds(30, 10, 3, 2, 10, &[3]),
            Err(PhloBoundsError::MachineMaximumExceeded),
        );
        assert_eq!(
            check_numeric_phlo_bounds(u64::MAX, u64::MAX, 2, 0, 0, &[0]),
            Err(PhloBoundsError::ChargeOverflow),
        );
        assert_eq!(
            check_numeric_phlo_bounds(u64::MAX, u64::MAX, 1, 0, 0, &[0]),
            Err(PhloBoundsError::ChargeOverflow),
        );
    }

    #[test]
    fn fee_zero_limits_and_full_width_boundaries_are_exact() {
        let maximum =
            check_numeric_phlo_bounds(u64::MAX, u64::MAX - 1, 1, 1, u64::MAX - 1, &[1]).unwrap();
        assert_eq!(maximum.signed_charge_ceiling(), u64::MAX);
        assert_eq!(maximum.schedule_charge_bound(), u64::MAX);
        for (limit, ceiling, price, bound) in [
            (0, u64::MAX, u64::MAX, 0),
            (u64::MAX, 0, 0, u64::MAX),
            (0, 0, 0, 0),
        ] {
            let checked =
                check_numeric_phlo_bounds(1, limit, ceiling, price, bound, &[price]).unwrap();
            assert_eq!(checked.signed_charge_ceiling(), 1);
            assert_eq!(checked.schedule_charge_bound(), 1);
            assert_eq!(
                check_numeric_phlo_bounds(0, limit, ceiling, price, bound, &[price]),
                Err(PhloBoundsError::MachineMaximumExceeded),
            );
        }
    }

    #[test]
    fn exhaustive_small_inputs_match_the_formal_predicate() {
        for limit in 0..=5 {
            for ceiling in 0..=5 {
                for price in 0..=5 {
                    for bound in 0..=5 {
                        for machine_max in 0..=26 {
                            for owners in [vec![], vec![0], vec![2, 5, 3], vec![5; 4]] {
                                assert_eq!(
                                    values(check_numeric_phlo_bounds(
                                        machine_max,
                                        limit,
                                        ceiling,
                                        price,
                                        bound,
                                        &owners,
                                    )),
                                    reference(machine_max, limit, ceiling, price, bound, &owners),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn full_width_inputs_match_wide_integer_reference(
            machine_max in any::<u64>(),
            limit in any::<u64>(),
            ceiling in any::<u64>(),
            price in any::<u64>(),
            bound in any::<u64>(),
            owners in prop::collection::vec(any::<u64>(), 0..257),
        ) {
            prop_assert_eq!(
                values(check_numeric_phlo_bounds(machine_max, limit, ceiling, price, bound, &owners)),
                reference(machine_max, limit, ceiling, price, bound, &owners),
            );
        }

        #[test]
        fn admitted_bounds_conserve_limits_under_owner_reordering(
            limit in any::<u32>(),
            ceiling in any::<u32>(),
            price_seed in any::<u64>(),
            bound_seed in any::<u64>(),
            owner_offsets in prop::collection::vec(any::<u32>(), 1..257),
        ) {
            let limit = u64::from(limit);
            let ceiling = u64::from(ceiling);
            let price = price_seed % (ceiling + 1);
            let bound = bound_seed % (limit + 1);
            let owners: Vec<_> = owner_offsets.iter().map(|offset| price + u64::from(*offset)).collect();
            let expected = check_numeric_phlo_bounds(u64::MAX, limit, ceiling, price, bound, &owners).unwrap();
            prop_assert_eq!(u128::from(expected.signed_charge_ceiling()), u128::from(limit) * u128::from(ceiling) + 1);
            prop_assert_eq!(u128::from(expected.schedule_charge_bound()), u128::from(bound) * u128::from(price) + 1);
            prop_assert!(expected.schedule_charge_bound() <= expected.signed_charge_ceiling());
            let mut reordered = owners.clone();
            reordered.reverse();
            prop_assert_eq!(check_numeric_phlo_bounds(u64::MAX, limit, ceiling, price, bound, &reordered), Ok(expected));
            reordered.extend_from_slice(&owners);
            prop_assert_eq!(check_numeric_phlo_bounds(u64::MAX, limit, ceiling, price, bound, &reordered), Ok(expected));
            prop_assert_eq!(check_numeric_phlo_bounds(expected.signed_charge_ceiling() - 1, limit, ceiling, price, bound, &owners), Err(PhloBoundsError::MachineMaximumExceeded));
        }
    }
}
