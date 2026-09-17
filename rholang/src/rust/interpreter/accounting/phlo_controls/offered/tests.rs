use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_bounds::PhloBoundsError;

const ENV: PhloEnvironment<'static> = PhloEnvironment {
    protocol_version: 7,
    network: b"test",
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

fn check(
    offer: PhloOffer,
    minimum: u64,
    limit: u64,
    ceiling: u64,
    price: u64,
    owners: &[u64],
) -> Result<(), PhloOfferError> {
    let selected = schedule(price);
    let permitted = [selected];
    let terms = SignedPhloControls {
        limit,
        price_ceiling: ceiling,
        required_owner_ceilings: owners,
        permitted_schedules: &permitted,
    };
    check_offered_phlo_controls(ENV, minimum, u64::MAX, offer, terms, selected, 0).map(|_| ())
}

#[test]
fn permitted_cheaper_schedule_cannot_replace_signed_offer() {
    let selected = schedule(3);
    let permitted = [selected];
    let terms = SignedPhloControls {
        limit: 10,
        price_ceiling: 5,
        required_owner_ceilings: &[5, 8, 9],
        permitted_schedules: &permitted,
    };
    assert!(check_phlo_controls(ENV, 2, 100, terms, selected, 6).is_ok());
    assert_eq!(
        check_offered_phlo_controls(
            ENV,
            2,
            100,
            PhloOffer {
                limit: 10,
                price: 5
            },
            terms,
            selected,
            6
        ),
        Err(PhloOfferError::PriceMismatch)
    );
    let offer = PhloOffer {
        limit: 10,
        price: 3,
    };
    let checked = check_offered_phlo_controls(ENV, 2, 100, offer, terms, selected, 6).unwrap();
    assert_eq!(checked.offer(), offer);
    assert_eq!(checked.controls().numeric().schedule_charge_bound(), 19);
    assert_eq!(checked.controls().minimum_price(), 2);
}

#[test]
fn binding_offer_preserves_the_original_checked_controls() {
    let selected = schedule(3);
    let permitted = [selected];
    let terms = SignedPhloControls {
        limit: 10,
        price_ceiling: 5,
        required_owner_ceilings: &[5, 8, 9],
        permitted_schedules: &permitted,
    };
    let controls = check_phlo_controls(ENV, 2, 51, terms, selected, 6).unwrap();
    let offer = PhloOffer {
        limit: 10,
        price: 3,
    };
    let bound = controls.bind_offer(offer).unwrap();
    assert_eq!(bound.controls(), controls);
    assert_eq!(bound.offer(), offer);
    for (offer, expected) in [
        (
            PhloOffer {
                limit: -1,
                price: 3,
            },
            PhloOfferError::NegativeLimit,
        ),
        (
            PhloOffer {
                limit: 10,
                price: -1,
            },
            PhloOfferError::NegativePrice,
        ),
        (
            PhloOffer { limit: 9, price: 3 },
            PhloOfferError::LimitMismatch,
        ),
        (
            PhloOffer {
                limit: 10,
                price: 2,
            },
            PhloOfferError::PriceMismatch,
        ),
        (
            PhloOffer {
                limit: 10,
                price: 5,
            },
            PhloOfferError::PriceMismatch,
        ),
    ] {
        assert_eq!(controls.bind_offer(offer), Err(expected));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn bound_offer_refines_without_reconstructing_numeric_evidence(
        limit in 0_u64..=u32::MAX as u64,
        price in 0_u64..=u32::MAX as u64,
        owners in 1_usize..258,
        minimum_seed in any::<u64>(),
        bound_seed in any::<u64>(),
        changed_limit in any::<i64>(),
        changed_price in any::<i64>(),
    ) {
        let selected = schedule(price);
        let permitted = [selected];
        let ceilings = vec![price; owners];
        let terms = SignedPhloControls {
            limit, price_ceiling: price, required_owner_ceilings: &ceilings,
            permitted_schedules: &permitted,
        };
        let minimum = minimum_seed % (price + 1);
        let bound = bound_seed % (limit + 1);
        let machine_max = limit * price + 1;
        let controls = check_phlo_controls(ENV, minimum, machine_max, terms, selected, bound).unwrap();
        let result = controls.bind_offer(PhloOffer { limit: changed_limit, price: changed_price });
        prop_assert_eq!(result.is_ok(), changed_limit == limit as i64 && changed_price == price as i64);
        let offered = controls.bind_offer(PhloOffer { limit: limit as i64, price: price as i64 }).unwrap();
        prop_assert_eq!(offered.controls(), controls);
        prop_assert_eq!(offered.controls().minimum_price(), minimum);
        prop_assert_eq!(offered.controls().resource_bound(), bound);
        prop_assert_eq!(offered.controls().numeric(), controls.numeric());
    }
}

#[test]
fn signed_scalar_domains_and_limit_equality_are_enforced() {
    assert_eq!(
        check(
            PhloOffer {
                limit: -1,
                price: 2
            },
            0,
            1,
            3,
            2,
            &[3]
        ),
        Err(PhloOfferError::NegativeLimit)
    );
    assert_eq!(
        check(
            PhloOffer {
                limit: 1,
                price: -1
            },
            0,
            1,
            3,
            2,
            &[3]
        ),
        Err(PhloOfferError::NegativePrice)
    );
    assert_eq!(
        check(PhloOffer { limit: 1, price: 2 }, 0, 2, 3, 2, &[3]),
        Err(PhloOfferError::LimitMismatch)
    );
    assert_eq!(
        check(PhloOffer { limit: 0, price: 0 }, 0, 0, 0, 0, &[0]),
        Ok(())
    );
    assert_eq!(
        check(
            PhloOffer {
                limit: i64::MAX,
                price: 0
            },
            0,
            i64::MAX as u64,
            0,
            0,
            &[0]
        ),
        Ok(())
    );
    assert_eq!(
        check(
            PhloOffer {
                limit: 0,
                price: i64::MAX
            },
            0,
            0,
            i64::MAX as u64,
            i64::MAX as u64,
            &[u64::MAX]
        ),
        Ok(())
    );
}

#[test]
fn chain_minimum_and_each_owner_remain_independent_requirements() {
    let offer = PhloOffer {
        limit: 10,
        price: 3,
    };
    assert_eq!(
        check(offer, 4, 10, 100, 3, &[100; 257]),
        Err(PhloOfferError::Controls(
            PhloControlsError::BelowMinimumPrice
        ))
    );
    assert_eq!(check(offer, 3, 10, 100, 3, &[100; 257]), Ok(()));
    for position in 0..257 {
        let mut owners = vec![100; 257];
        owners[position] = 2;
        assert_eq!(
            check(offer, 2, 10, 100, 3, &owners),
            Err(PhloOfferError::Controls(PhloControlsError::Numeric(
                PhloBoundsError::OwnerPriceCeilingExceeded
            )))
        );
    }
    assert!(check(offer, 0, 10, 100, 3, &[]).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn offered_checker_matches_refinement_predicate(
        offered_limit in prop_oneof![0i64..30, any::<i64>()],
        offered_price in prop_oneof![0i64..10, any::<i64>()],
        limit in 0u64..30, price in 0u64..10, ceiling in 0u64..12,
        minimum in 0u64..10, bound in 0u64..30, maximum in 0u64..400,
        owners in prop::collection::vec(0u64..12, 0..65),
    ) {
        let selected = schedule(price);
        let permitted = [selected];
        let terms = SignedPhloControls { limit, price_ceiling: ceiling, required_owner_ceilings: &owners, permitted_schedules: &permitted };
        let offer = PhloOffer { limit: offered_limit, price: offered_price };
        let expected = u64::try_from(offered_limit) == Ok(limit)
            && u64::try_from(offered_price) == Ok(price)
            && minimum <= price && price <= ceiling && bound <= limit
            && !owners.is_empty() && owners.iter().all(|cap| price <= *cap)
            && u128::from(limit) * u128::from(ceiling) < u128::from(maximum);
        prop_assert_eq!(check_offered_phlo_controls(ENV, minimum, maximum, offer, terms, selected, bound).is_ok(), expected);
    }

    #[test]
    fn arbitrary_owner_count_and_order_do_not_change_offered_charge(
        price in 0u64..100, limit in 0u64..100, owner_count in 1usize..513,
        extra in 0u64..100, work_seed in any::<u64>(),
    ) {
        let bound = work_seed % (limit + 1);
        let owners: Vec<_> = (0..owner_count).map(|index| price + index as u64 + extra).collect();
        let selected = schedule(price);
        let permitted = [selected];
        let terms = SignedPhloControls { limit, price_ceiling: price + extra, required_owner_ceilings: &owners, permitted_schedules: &permitted };
        let offer = PhloOffer { limit: limit as i64, price: price as i64 };
        let checked = check_offered_phlo_controls(ENV, price, u64::MAX, offer, terms, selected, bound).unwrap();
        prop_assert_eq!(checked.controls().numeric().schedule_charge_bound(), bound * price + 1);
        let reversed: Vec<_> = owners.iter().copied().rev().collect();
        let reversed_terms = SignedPhloControls { required_owner_ceilings: &reversed, ..terms };
        let reordered = check_offered_phlo_controls(ENV, price, u64::MAX, offer, reversed_terms, selected, bound).unwrap();
        prop_assert_eq!(checked.controls().numeric(), reordered.controls().numeric());
    }

    #[test]
    fn full_width_success_and_overflow_match_widened_arithmetic(
        limit in 0i64..=i64::MAX, price in 0i64..=i64::MAX,
        owner_count in 1usize..65,
    ) {
        let owners = vec![price as u64; owner_count];
        let result = check(PhloOffer { limit, price }, 0, limit as u64, price as u64, price as u64, &owners);
        prop_assert_eq!(result.is_ok(), (limit as u128) * (price as u128) < u128::from(u64::MAX));
    }
}
