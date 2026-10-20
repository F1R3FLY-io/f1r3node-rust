use models::rust::phlo_schedule::{
    PhloResourceClassV1, PhloScheduleError, PhloScheduleLimits, PhloScheduleV1,
};
use models::rust::phlo_wire::PhloWireError;
use thiserror::Error;

use super::phlo_bounds::{check_numeric_phlo_bounds, NumericPhloBounds, PhloBoundsError};

mod wire;
pub use wire::{PhloControlsBinding, PhloControlsView};

mod offered;
pub use offered::{check_offered_phlo_controls, CheckedPhloOffer, PhloOffer, PhloOfferError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloEnvironment<'a> {
    pub protocol_version: u64,
    pub network: &'a [u8],
    pub shard: &'a [u8],
    pub asset: &'a [u8],
    pub unit: &'a [u8],
    pub decimal_scale: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloSchedule<'a> {
    pub commitment: [u8; 32],
    pub environment: PhloEnvironment<'a>,
    pub weights: &'a [u64],
    pub actual_price: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloScheduleBinding<'a> {
    descriptor: &'a PhloScheduleV1<'a>,
    commitment: [u8; 32],
    weights: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloSchedulePolicy<'a> {
    environment: PhloEnvironment<'a>,
    classes: &'a [PhloResourceClassV1<'a>],
    compatibility_rule: [u8; 32],
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("selected phlo schedule differs from the required resource policy")]
pub struct PhloSchedulePolicyMismatch;

impl<'a> PhloScheduleBinding<'a> {
    pub fn new(
        descriptor: &'a PhloScheduleV1<'a>,
        limits: PhloScheduleLimits,
    ) -> Result<Self, PhloScheduleError> {
        let commitment = descriptor.digest(limits)?;
        let mut weights = Vec::new();
        weights
            .try_reserve_exact(descriptor.classes.len())
            .map_err(|_| PhloWireError::AllocationFailed)?;
        weights.extend(descriptor.classes.iter().map(|class| class.weight));
        Ok(Self {
            descriptor,
            commitment,
            weights,
        })
    }

    pub fn descriptor(&self) -> &'a PhloScheduleV1<'a> { self.descriptor }

    pub fn policy(&self) -> PhloSchedulePolicy<'_> {
        PhloSchedulePolicy {
            environment: self.schedule().environment,
            classes: &self.descriptor.classes,
            compatibility_rule: self.descriptor.compatibility_rule,
        }
    }

    pub fn bind_policy(
        &self,
        required: PhloSchedulePolicy<'_>,
    ) -> Result<PhloSchedule<'_>, PhloSchedulePolicyMismatch> {
        if self.policy() != required {
            return Err(PhloSchedulePolicyMismatch);
        }
        Ok(self.schedule())
    }

    pub fn schedule(&self) -> PhloSchedule<'_> {
        PhloSchedule {
            commitment: self.commitment,
            environment: PhloEnvironment {
                protocol_version: self.descriptor.protocol_version,
                network: self.descriptor.network,
                shard: self.descriptor.shard,
                asset: self.descriptor.settlement_asset,
                unit: self.descriptor.settlement_unit,
                decimal_scale: self.descriptor.decimal_scale,
            },
            weights: &self.weights,
            actual_price: self.descriptor.actual_price,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedPhloControls<'a> {
    pub limit: u64,
    pub price_ceiling: u64,
    pub required_owner_ceilings: &'a [u64],
    pub permitted_schedules: &'a [PhloSchedule<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloControls<'a> {
    minimum_price: u64,
    schedule: PhloSchedule<'a>,
    terms: SignedPhloControls<'a>,
    resource_bound: u64,
    numeric: NumericPhloBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFundingTerms<'a> {
    pub required_owner_ceilings: &'a [u64],
    pub asset: &'a [u8],
    pub schedule_commitment: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloFundingTerms<'a> {
    controls: CheckedPhloControls<'a>,
    terms: PhloFundingTerms<'a>,
}

impl<'a> CheckedPhloFundingTerms<'a> {
    pub fn controls(self) -> CheckedPhloControls<'a> { self.controls }
    pub fn terms(self) -> PhloFundingTerms<'a> { self.terms }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloFundingTermsError {
    #[error("funding-right owner ceilings differ from the execution consent")]
    OwnerCeilingsMismatch,
    #[error("funding-right asset differs from the execution schedule")]
    AssetMismatch,
    #[error("funding-right schedule commitment differs from the execution schedule")]
    ScheduleMismatch,
}

pub fn check_phlo_funding_terms<'a>(
    controls: CheckedPhloControls<'a>,
    terms: PhloFundingTerms<'a>,
) -> Result<CheckedPhloFundingTerms<'a>, PhloFundingTermsError> {
    if terms.required_owner_ceilings != controls.terms().required_owner_ceilings {
        return Err(PhloFundingTermsError::OwnerCeilingsMismatch);
    }
    if terms.asset != controls.schedule().environment.asset {
        return Err(PhloFundingTermsError::AssetMismatch);
    }
    if terms.schedule_commitment != controls.schedule().commitment {
        return Err(PhloFundingTermsError::ScheduleMismatch);
    }
    Ok(CheckedPhloFundingTerms { controls, terms })
}

impl<'a> CheckedPhloControls<'a> {
    pub fn minimum_price(self) -> u64 { self.minimum_price }

    pub fn schedule(self) -> PhloSchedule<'a> { self.schedule }

    pub fn terms(self) -> SignedPhloControls<'a> { self.terms }

    pub fn resource_bound(self) -> u64 { self.resource_bound }

    pub fn numeric(self) -> NumericPhloBounds { self.numeric }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloControlsError {
    #[error("actual phlo price is below the on-chain minimum")]
    BelowMinimumPrice,
    #[error("selected phlo schedule is not explicitly permitted")]
    ScheduleNotPermitted,
    #[error("phlo schedule protocol version does not match the execution context")]
    ProtocolVersionMismatch,
    #[error("phlo schedule network does not match the execution context")]
    NetworkMismatch,
    #[error("phlo schedule shard does not match the execution context")]
    ShardMismatch,
    #[error("phlo schedule asset does not match the native settlement asset")]
    AssetMismatch,
    #[error("phlo schedule unit does not match the native phlo unit")]
    UnitMismatch,
    #[error("phlo schedule decimal scale does not match the native denomination")]
    DecimalScaleMismatch,
    #[error(transparent)]
    Numeric(#[from] PhloBoundsError),
}

pub fn check_phlo_controls<'a>(
    environment: PhloEnvironment<'_>,
    minimum_price: u64,
    machine_max: u64,
    terms: SignedPhloControls<'a>,
    schedule: PhloSchedule<'a>,
    resource_bound: u64,
) -> Result<CheckedPhloControls<'a>, PhloControlsError> {
    if !terms.permitted_schedules.contains(&schedule) {
        return Err(PhloControlsError::ScheduleNotPermitted);
    }
    for (matches, error) in [
        (
            schedule.environment.protocol_version == environment.protocol_version,
            PhloControlsError::ProtocolVersionMismatch,
        ),
        (
            schedule.environment.network == environment.network,
            PhloControlsError::NetworkMismatch,
        ),
        (
            schedule.environment.shard == environment.shard,
            PhloControlsError::ShardMismatch,
        ),
        (
            schedule.environment.asset == environment.asset,
            PhloControlsError::AssetMismatch,
        ),
        (
            schedule.environment.unit == environment.unit,
            PhloControlsError::UnitMismatch,
        ),
        (
            schedule.environment.decimal_scale == environment.decimal_scale,
            PhloControlsError::DecimalScaleMismatch,
        ),
    ] {
        if !matches {
            return Err(error);
        }
    }
    if schedule.actual_price < minimum_price {
        return Err(PhloControlsError::BelowMinimumPrice);
    }
    let numeric = check_numeric_phlo_bounds(
        machine_max,
        terms.limit,
        terms.price_ceiling,
        schedule.actual_price,
        resource_bound,
        terms.required_owner_ceilings,
    )?;
    Ok(CheckedPhloControls {
        minimum_price,
        schedule,
        terms,
        resource_bound,
        numeric,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn environment() -> PhloEnvironment<'static> {
        PhloEnvironment {
            protocol_version: 6,
            network: b"test network",
            shard: b"test shard",
            asset: b"native asset",
            unit: b"native phlo",
            decimal_scale: 8,
        }
    }

    fn schedule() -> PhloSchedule<'static> {
        PhloSchedule {
            commitment: [1; 32],
            environment: environment(),
            weights: &[1, 2, 3, 4],
            actual_price: 2,
        }
    }

    fn controls<'a>(permitted_schedules: &'a [PhloSchedule<'a>]) -> SignedPhloControls<'a> {
        SignedPhloControls {
            limit: 10,
            price_ceiling: 3,
            required_owner_ceilings: &[3, 5, 8],
            permitted_schedules,
        }
    }

    fn check<'a>(
        terms: SignedPhloControls<'a>,
        selected: PhloSchedule<'a>,
    ) -> Result<CheckedPhloControls<'a>, PhloControlsError> {
        check_phlo_controls(environment(), 0, 31, terms, selected, 10)
    }

    fn mutations(original: PhloSchedule<'static>) -> Vec<PhloSchedule<'static>> {
        vec![
            PhloSchedule {
                environment: PhloEnvironment {
                    protocol_version: 7,
                    ..original.environment
                },
                ..original
            },
            PhloSchedule {
                environment: PhloEnvironment {
                    network: b"other network",
                    ..original.environment
                },
                ..original
            },
            PhloSchedule {
                environment: PhloEnvironment {
                    shard: b"other shard",
                    ..original.environment
                },
                ..original
            },
            PhloSchedule {
                environment: PhloEnvironment {
                    asset: b"other asset",
                    ..original.environment
                },
                ..original
            },
            PhloSchedule {
                environment: PhloEnvironment {
                    unit: b"other unit",
                    ..original.environment
                },
                ..original
            },
            PhloSchedule {
                environment: PhloEnvironment {
                    decimal_scale: 9,
                    ..original.environment
                },
                ..original
            },
            PhloSchedule {
                commitment: [2; 32],
                ..original
            },
            PhloSchedule {
                weights: &[1, 2, 3, 5],
                ..original
            },
            PhloSchedule {
                weights: &[2, 1, 3, 4],
                ..original
            },
            PhloSchedule {
                weights: &[1, 2, 3],
                ..original
            },
            PhloSchedule {
                weights: &[1, 2, 3, 4, 0],
                ..original
            },
            PhloSchedule {
                actual_price: 1,
                ..original
            },
        ]
    }

    #[test]
    fn every_changed_schedule_field_requires_explicit_consent() {
        let selected = schedule();
        let permitted = [selected];
        for changed in mutations(selected) {
            assert_eq!(
                check(controls(&permitted), changed),
                Err(PhloControlsError::ScheduleNotPermitted)
            );
        }
        assert_eq!(
            check(controls(&[]), selected),
            Err(PhloControlsError::ScheduleNotPermitted)
        );
    }

    #[test]
    fn explicit_schedule_consent_cannot_replace_the_protocol_context() {
        let errors = [
            PhloControlsError::ProtocolVersionMismatch,
            PhloControlsError::NetworkMismatch,
            PhloControlsError::ShardMismatch,
            PhloControlsError::AssetMismatch,
            PhloControlsError::UnitMismatch,
            PhloControlsError::DecimalScaleMismatch,
        ];
        for (changed, error) in mutations(schedule()).into_iter().zip(errors) {
            let permitted = [changed];
            assert_eq!(check(controls(&permitted), changed), Err(error));
        }
    }

    #[test]
    fn schedule_equality_is_by_content_not_allocation_address() {
        let weights = vec![1, 2, 3, 4];
        let network = b"test network".to_vec();
        let selected = PhloSchedule {
            environment: PhloEnvironment {
                network: &network,
                ..environment()
            },
            weights: &weights,
            ..schedule()
        };
        let permitted = [schedule()];
        let checked = check(controls(&permitted), selected).unwrap();
        assert_eq!(checked.schedule(), selected);
        assert_eq!(checked.terms(), controls(&permitted));
        assert_eq!(checked.resource_bound(), 10);
        assert_eq!(checked.numeric().schedule_charge_bound(), 21);
        assert_eq!(checked.numeric().signed_charge_ceiling(), 31);
    }

    #[test]
    fn numeric_guards_remain_required_after_schedule_acceptance() {
        let permitted = [schedule()];
        let mut terms = controls(&permitted);
        terms.required_owner_ceilings = &[];
        assert_eq!(
            check(terms, schedule()),
            Err(PhloBoundsError::MissingOwnerConsent.into())
        );
        terms.required_owner_ceilings = &[1, 3, 8];
        assert_eq!(
            check(terms, schedule()),
            Err(PhloBoundsError::OwnerPriceCeilingExceeded.into())
        );
        terms.required_owner_ceilings = &[3, 8];
        terms.price_ceiling = 1;
        assert_eq!(
            check(terms, schedule()),
            Err(PhloBoundsError::PriceCeilingExceeded.into())
        );
        terms.price_ceiling = 3;
        terms.limit = 9;
        assert_eq!(
            check(terms, schedule()),
            Err(PhloBoundsError::LimitExceeded.into())
        );
        terms.limit = u64::MAX;
        assert_eq!(
            check(terms, schedule()),
            Err(PhloBoundsError::ChargeOverflow.into())
        );
    }

    #[test]
    fn checked_controls_retain_original_terms_when_future_terms_change() {
        let permitted = [schedule()];
        let terms = controls(&permitted);
        let original = check(terms, schedule()).unwrap();
        let later_schedule = PhloSchedule {
            actual_price: 1,
            ..schedule()
        };
        let later_permitted = [later_schedule];
        let later_terms = SignedPhloControls {
            price_ceiling: 1,
            required_owner_ceilings: &[1, 1, 1, 1],
            permitted_schedules: &later_permitted,
            ..terms
        };
        let later = check(later_terms, later_schedule).unwrap();
        assert_eq!(original.schedule(), schedule());
        assert_eq!(original.terms(), terms);
        assert_eq!(original.numeric().schedule_charge_bound(), 21);
        assert_eq!(later.numeric().schedule_charge_bound(), 11);
        assert_eq!(
            check(later_terms, schedule()),
            Err(PhloControlsError::ScheduleNotPermitted)
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn arbitrary_context_identifiers_keep_their_distinct_roles(
            version in any::<u64>(),
            identifiers in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..40), 4),
            price in 0_u64..100,
            weights in prop::collection::vec(any::<u64>(), 0..32),
        ) {
            let original_environment = PhloEnvironment {
                protocol_version: version,
                network: &identifiers[0], shard: &identifiers[1],
                asset: &identifiers[2], unit: &identifiers[3],
                decimal_scale: 8,
            };
            let original = PhloSchedule { commitment: [1; 32], environment: original_environment, weights: &weights, actual_price: price };
            let permitted = [original];
            let ceilings = [price];
            let terms = SignedPhloControls {
                limit: 10, price_ceiling: price,
                required_owner_ceilings: &ceilings, permitted_schedules: &permitted,
            };
            prop_assert!(check_phlo_controls(original_environment, 0, u64::MAX, terms, original, 10).is_ok());
            for position in 0..5 {
                let mut changed_identifiers = identifiers.clone();
                if position != 0 {
                    changed_identifiers[position - 1].push(0);
                }
                let changed = PhloSchedule {
                    environment: PhloEnvironment {
                        protocol_version: if position == 0 { version.wrapping_add(1) } else { version },
                        network: &changed_identifiers[0], shard: &changed_identifiers[1],
                        asset: &changed_identifiers[2], unit: &changed_identifiers[3],
                        decimal_scale: original_environment.decimal_scale,
                    },
                    ..original
                };
                prop_assert_eq!(check_phlo_controls(original_environment, 0, u64::MAX, terms, changed, 10), Err(PhloControlsError::ScheduleNotPermitted));
                let changed_permitted = [changed];
                let changed_terms = SignedPhloControls { permitted_schedules: &changed_permitted, ..terms };
                let expected = [
                    PhloControlsError::ProtocolVersionMismatch,
                    PhloControlsError::NetworkMismatch,
                    PhloControlsError::ShardMismatch,
                    PhloControlsError::AssetMismatch,
                    PhloControlsError::UnitMismatch,
                ][position];
                prop_assert_eq!(check_phlo_controls(original_environment, 0, u64::MAX, changed_terms, changed, 10), Err(expected));
            }
        }

        #[test]
        fn generated_schedule_checks_match_complete_field_oracle(
            price in 0_u64..8,
            ceiling in 0_u64..8,
            limit in 0_u64..16,
            bound in 0_u64..20,
            machine_max in 0_u64..130,
            owners in prop::collection::vec(0_u64..8, 0..130),
            weights in prop::collection::vec(any::<u64>(), 0..16),
            permitted_prices in prop::collection::vec(0_u64..8, 0..16),
            mismatch in 0_u8..8,
        ) {
            let mut selected = PhloSchedule { weights: &weights, actual_price: price, ..schedule() };
            match mismatch {
                1 => selected.environment.protocol_version += 1,
                2 => selected.environment.network = b"different",
                3 => selected.environment.shard = b"different",
                4 => selected.environment.asset = b"different",
                5 => selected.environment.unit = b"different",
                6 => selected.environment.decimal_scale += 1,
                _ => (),
            }
            let permitted: Vec<_> = permitted_prices.iter().map(|value| PhloSchedule { actual_price: *value, ..selected }).collect();
            let terms = SignedPhloControls {
                limit,
                price_ceiling: ceiling,
                required_owner_ceilings: &owners,
                permitted_schedules: &permitted,
            };
            let expected = !owners.is_empty()
                && bound <= limit
                && price <= ceiling
                && owners.iter().all(|owner| price <= *owner)
                && u128::from(limit) * u128::from(ceiling) < u128::from(machine_max)
                && permitted_prices.contains(&price)
                && (mismatch == 0 || mismatch == 7);
            let result = check_phlo_controls(environment(), 0, machine_max, terms, selected, bound);
            prop_assert_eq!(result.is_ok(), expected);
            if let Ok(checked) = result {
                prop_assert_eq!(checked.schedule(), selected);
                prop_assert_eq!(checked.terms(), terms);
                prop_assert_eq!(checked.resource_bound(), bound);
            }
        }

        #[test]
        fn arbitrary_permitted_schedule_position_preserves_admission(
            selected_index in 0_usize..64,
            price in 0_u64..100,
            weights in prop::collection::vec(any::<u64>(), 0..32),
            owners in 1_usize..257,
        ) {
            let selected = PhloSchedule { weights: &weights, actual_price: price, ..schedule() };
            let different = PhloSchedule { actual_price: price + 1, ..selected };
            let mut permitted = vec![different; 64];
            permitted[selected_index] = selected;
            let ceilings = vec![price; owners];
            let terms = SignedPhloControls {
                limit: 10, price_ceiling: price,
                required_owner_ceilings: &ceilings, permitted_schedules: &permitted,
            };
            let checked = check_phlo_controls(environment(), 0, u64::MAX, terms, selected, 10).unwrap();
            prop_assert_eq!(checked.numeric().schedule_charge_bound(), 10 * price + 1);
            let mut reordered_permitted = permitted.clone();
            reordered_permitted.reverse();
            let reordered_terms = SignedPhloControls { permitted_schedules: &reordered_permitted, ..terms };
            let reordered = check_phlo_controls(environment(), 0, u64::MAX, reordered_terms, selected, 10).unwrap();
            prop_assert_eq!(reordered.numeric(), checked.numeric());
            prop_assert_eq!(reordered.schedule(), checked.schedule());
        }
    }
}

#[cfg(test)]
mod binding_tests;

#[cfg(test)]
mod funding_terms_tests;

#[cfg(test)]
mod chain_price_tests;
