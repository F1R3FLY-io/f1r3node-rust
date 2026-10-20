use proptest::prelude::*;

use super::*;

const ENV: PhloEnvironment<'static> = PhloEnvironment {
    protocol_version: 6,
    network: b"network",
    shard: b"shard",
    asset: b"REV",
    unit: b"native",
    decimal_scale: 8,
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENV,
    weights: &[1],
    actual_price: 2,
}];

fn checked() -> CheckedPhloControls<'static> {
    check_phlo_controls(
        ENV,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 10,
            price_ceiling: 3,
            required_owner_ceilings: &[3, 4],
            permitted_schedules: &SCHEDULES,
        },
        SCHEDULES[0],
        8,
    )
    .unwrap()
}

fn terms() -> PhloFundingTerms<'static> {
    PhloFundingTerms {
        required_owner_ceilings: &[3, 4],
        asset: b"REV",
        schedule_commitment: [1; 32],
    }
}

#[test]
fn funding_terms_bind_actual_execution_without_changing_checked_costs() {
    let controls = checked();
    let result = check_phlo_funding_terms(controls, terms()).unwrap();
    assert_eq!(result.controls(), controls);
    assert_eq!(result.terms(), terms());
    assert_eq!(result.controls().numeric(), controls.numeric());
    assert_eq!(result.controls().resource_bound(), 8);
}

#[test]
fn declared_schedule_alias_cannot_replace_actual_execution_commitment() {
    let declared = PhloFundingTerms {
        schedule_commitment: [7; 32],
        ..terms()
    };
    assert_eq!(
        check_phlo_funding_terms(checked(), declared),
        Err(PhloFundingTermsError::ScheduleMismatch)
    );
    assert_eq!(checked().schedule().commitment, [1; 32]);
    for byte in 0..32 {
        for bit in 0..8 {
            let mut changed = terms();
            changed.schedule_commitment[byte] ^= 1 << bit;
            assert_eq!(
                check_phlo_funding_terms(checked(), changed),
                Err(PhloFundingTermsError::ScheduleMismatch)
            );
        }
    }
}

#[test]
fn terms_reject_asset_substitution_and_missing_or_altered_owner_ceilings() {
    let changed = PhloFundingTerms {
        asset: b"another asset",
        ..terms()
    };
    assert_eq!(
        check_phlo_funding_terms(checked(), changed),
        Err(PhloFundingTermsError::AssetMismatch)
    );
    for ceilings in [vec![], vec![3], vec![3, 5], vec![4, 3], vec![3, 4, 1]] {
        let changed = PhloFundingTerms {
            required_owner_ceilings: &ceilings,
            ..terms()
        };
        assert_eq!(
            check_phlo_funding_terms(checked(), changed),
            Err(PhloFundingTermsError::OwnerCeilingsMismatch)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn generated_funding_terms_require_exact_context_for_arbitrary_owner_cohorts(
        ceilings in prop::collection::vec(2u64..=u64::MAX, 1..130),
        commitment in any::<[u8; 32]>(), asset in prop::collection::vec(any::<u8>(), 1..65),
        offset in any::<usize>(),
    ) {
        let environment = PhloEnvironment {asset: &asset, ..ENV};
        let schedules = [PhloSchedule {commitment, environment, ..SCHEDULES[0]}];
        let controls = check_phlo_controls(environment, 0, u64::MAX, SignedPhloControls {
            limit: 10, price_ceiling: 3, required_owner_ceilings: &ceilings,
            permitted_schedules: &schedules,
        }, schedules[0], 8).unwrap();
        let terms = PhloFundingTerms {required_owner_ceilings: &ceilings, asset: &asset, schedule_commitment: commitment};
        prop_assert_eq!(check_phlo_funding_terms(controls, terms).unwrap().controls(), controls);
        let mut bad_commitment = commitment; bad_commitment[offset % 32] ^= 1;
        prop_assert_eq!(check_phlo_funding_terms(controls, PhloFundingTerms {schedule_commitment: bad_commitment, ..terms}), Err(PhloFundingTermsError::ScheduleMismatch));
        let mut bad_asset = asset.clone(); bad_asset.push(0);
        prop_assert_eq!(check_phlo_funding_terms(controls, PhloFundingTerms {asset: &bad_asset, ..terms}), Err(PhloFundingTermsError::AssetMismatch));
        let mut bad_ceilings = ceilings.clone(); let index = offset % bad_ceilings.len(); bad_ceilings[index] ^= 1;
        prop_assert_eq!(check_phlo_funding_terms(controls, PhloFundingTerms {required_owner_ceilings: &bad_ceilings, ..terms}), Err(PhloFundingTermsError::OwnerCeilingsMismatch));
    }
}
