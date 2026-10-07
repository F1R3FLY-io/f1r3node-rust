use proptest::prelude::*;

use super::*;

const ENV: PhloEnvironment<'static> = PhloEnvironment {
    protocol_version: 6,
    network: b"network",
    shard: b"root",
    asset: b"REV",
    unit: b"smallest REV unit",
    decimal_scale: 8,
};

fn schedule(price: u64) -> PhloSchedule<'static> {
    PhloSchedule {
        commitment: [1; 32],
        environment: ENV,
        weights: &[1],
        actual_price: price,
    }
}

#[test]
fn high_signed_ceiling_does_not_authorize_below_minimum_tariff() {
    let selected = schedule(2);
    let permitted = [selected];
    let terms = SignedPhloControls {
        limit: 10,
        price_ceiling: 100,
        required_owner_ceilings: &[100, 200, 300],
        permitted_schedules: &permitted,
    };
    assert!(check_numeric_phlo_bounds(u64::MAX, 10, 100, 2, 10, &[100, 200, 300]).is_ok());
    assert_eq!(
        check_phlo_controls(ENV, 3, u64::MAX, terms, selected, 10),
        Err(PhloControlsError::BelowMinimumPrice)
    );
    let checked = check_phlo_controls(ENV, 2, u64::MAX, terms, selected, 10).unwrap();
    assert_eq!(checked.minimum_price(), 2);
    assert_eq!(checked.numeric().schedule_charge_bound(), 21);
}

#[test]
fn captured_minimum_does_not_replace_the_actual_price_or_owner_ceilings() {
    let selected = schedule(3);
    let permitted = [selected];
    let terms = SignedPhloControls {
        limit: 10,
        price_ceiling: 5,
        required_owner_ceilings: &[3, 5],
        permitted_schedules: &permitted,
    };
    let first = check_phlo_controls(ENV, 1, u64::MAX, terms, selected, 10).unwrap();
    let second = check_phlo_controls(ENV, 2, u64::MAX, terms, selected, 10).unwrap();
    assert_ne!(first, second);
    assert_eq!(first.numeric(), second.numeric());
    assert_eq!(first.numeric().schedule_charge_bound(), 31);
    assert_eq!(first.minimum_price(), 1);
    assert_eq!(second.minimum_price(), 2);
    let denied = SignedPhloControls {
        required_owner_ceilings: &[2, 5],
        ..terms
    };
    assert_eq!(
        check_phlo_controls(ENV, 1, u64::MAX, denied, selected, 10),
        Err(PhloControlsError::Numeric(
            PhloBoundsError::OwnerPriceCeilingExceeded
        ))
    );
}

#[test]
fn small_price_domain_matches_the_complete_chain_acceptance_predicate() {
    for minimum in 0..=4 {
        for price in 0..=4 {
            for ceiling in 0..=4 {
                for first in 0..=4 {
                    for second in 0..=4 {
                        for bound in 0..=4 {
                            let selected = schedule(price);
                            let permitted = [selected];
                            let owners = [first, second];
                            let terms = SignedPhloControls {
                                limit: 3,
                                price_ceiling: ceiling,
                                required_owner_ceilings: &owners,
                                permitted_schedules: &permitted,
                            };
                            let expected = minimum <= price
                                && price <= ceiling
                                && price <= first
                                && price <= second
                                && bound <= 3
                                && 3 * ceiling < 10;
                            let result =
                                check_phlo_controls(ENV, minimum, 10, terms, selected, bound);
                            assert_eq!(
                                result.is_ok(),
                                expected,
                                "{minimum}/{price}/{ceiling}/{first}/{second}/{bound}"
                            );
                            if let Ok(checked) = result {
                                assert_eq!(checked.minimum_price(), minimum);
                                assert_eq!(
                                    checked.numeric().schedule_charge_bound(),
                                    bound * price + 1
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn native_chain_check_matches_unbounded_arithmetic_specification(
        minimum in any::<u64>(), price in any::<u64>(), ceiling in any::<u64>(),
        limit in any::<u64>(), bound in any::<u64>(), machine_max in any::<u64>(),
        owners in prop::collection::vec(any::<u64>(), 0..20),
    ) {
        let selected = schedule(price);
        let permitted = [selected];
        let terms = SignedPhloControls { limit, price_ceiling: ceiling, required_owner_ceilings: &owners, permitted_schedules: &permitted };
        let expected = minimum <= price && price <= ceiling && bound <= limit && !owners.is_empty()
            && owners.iter().all(|owner| price <= *owner)
            && u128::from(limit) * u128::from(ceiling) < u128::from(machine_max);
        prop_assert_eq!(check_phlo_controls(ENV, minimum, machine_max, terms, selected, bound).is_ok(), expected);
    }

    #[test]
    fn raising_the_chain_floor_cannot_add_admissions(
        first in 0u64..100, second in 0u64..100, price in 0u64..100,
        ceiling in 0u64..100, owners in prop::collection::vec(0u64..100, 1..20),
    ) {
        let selected = schedule(price);
        let permitted = [selected];
        let terms = SignedPhloControls { limit: 10, price_ceiling: ceiling, required_owner_ceilings: &owners, permitted_schedules: &permitted };
        let lower = check_phlo_controls(ENV, first.min(second), u64::MAX, terms, selected, 10);
        let higher = check_phlo_controls(ENV, first.max(second), u64::MAX, terms, selected, 10);
        if higher.is_ok() { prop_assert!(lower.is_ok()); }
        let zero = check_phlo_controls(ENV, 0, u64::MAX, terms, selected, 10);
        let numeric = check_numeric_phlo_bounds(u64::MAX, 10, ceiling, price, 10, &owners);
        prop_assert_eq!(zero.map(|checked| checked.numeric()), numeric.map_err(PhloControlsError::Numeric));
    }
}
