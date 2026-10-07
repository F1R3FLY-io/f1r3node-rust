use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, PhloOutcome,
};
use crate::rust::interpreter::accounting::Sig;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits() -> PhloExecutionLimits {
    PhloExecutionLimits {
        resource_entries: 10_000,
        authority_nodes: 1_000_000,
        key_bytes: 10_000_000,
    }
}

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        location: b"purse",
        class: 0,
        authority,
        acquisition_terms: b"current-terms",
    }
}

fn authority(owners: usize) -> Sig {
    (0..owners)
        .map(|index| Sig::Ground((index as u64).to_be_bytes().to_vec()))
        .reduce(|left, right| Sig::And(Box::new(left), Box::new(right)))
        .unwrap_or(Sig::Unit)
}

#[test]
fn measured_prepaid_keeps_exact_counts_for_zero_and_large_authority_groups() {
    for owners in [0, 1, 3, 65, 1024] {
        let owner = authority(owners);
        let measured = [PhloResourceAmount {
            resource: resource(&owner),
            quantity: 2,
        }];
        let assignment = MeasuredPhloPrepaidUse {
            demand_index: 0,
            resource: PhloResource {
                acquisition_terms: b"original-terms",
                ..measured[0].resource
            },
        };
        let bound =
            bind_measured_phlo_prepaid(&measured, &[assignment; 2], limits(), &budget()).unwrap();
        assert_eq!(bound.witness().used.len(), 2);
        assert!(bound.witness().fresh.is_empty());
        assert!(matches!(
            bind_measured_phlo_prepaid(&measured, &[assignment; 3], limits(), &budget()),
            Err(MeasuredPhloPrepaidError::ExcessConsumption)
        ));
    }
}

#[test]
fn measured_prepaid_rejects_each_identity_change_and_overdraw_without_input_mutation() {
    let owner = Sig::Ground(vec![1]);
    let quote = Sig::Quote(vec![1]);
    let duplicated = Sig::And(Box::new(owner.clone()), Box::new(owner.clone()));
    let measured = [PhloResourceAmount {
        resource: resource(&owner),
        quantity: 1,
    }];
    let original = measured;
    let assignment = MeasuredPhloPrepaidUse {
        demand_index: 0,
        resource: PhloResource {
            acquisition_terms: b"original-terms",
            ..measured[0].resource
        },
    };
    for changed in [
        PhloResource {
            location: b"other",
            ..assignment.resource
        },
        PhloResource {
            class: 1,
            ..assignment.resource
        },
        PhloResource {
            authority: &quote,
            ..assignment.resource
        },
        PhloResource {
            authority: &duplicated,
            ..assignment.resource
        },
    ] {
        assert!(matches!(
            bind_measured_phlo_prepaid(
                &measured,
                &[MeasuredPhloPrepaidUse {
                    resource: changed,
                    ..assignment
                }],
                limits(),
                &budget()
            ),
            Err(MeasuredPhloPrepaidError::ShapeMismatch)
        ));
    }
    assert!(matches!(
        bind_measured_phlo_prepaid(
            &measured,
            &[MeasuredPhloPrepaidUse {
                demand_index: 1,
                ..assignment
            }],
            limits(),
            &budget()
        ),
        Err(MeasuredPhloPrepaidError::MissingDemand)
    ));
    assert!(matches!(
        bind_measured_phlo_prepaid(&measured, &[assignment, assignment], limits(), &budget()),
        Err(MeasuredPhloPrepaidError::ExcessConsumption)
    ));
    let bound = bind_measured_phlo_prepaid(&measured, &[assignment], limits(), &budget()).unwrap();
    assert_eq!(
        bound.witness().used[0].resource.acquisition_terms,
        b"original-terms"
    );
    assert!(bound.witness().fresh.is_empty());
    assert_eq!(measured, original);
}

#[test]
fn measured_prepaid_enforces_limits_and_keeps_large_quantities_counted() {
    let owner = Sig::Ground(vec![1]);
    let measured = [PhloResourceAmount {
        resource: resource(&owner),
        quantity: u64::MAX,
    }];
    let assignment = MeasuredPhloPrepaidUse {
        demand_index: 0,
        resource: measured[0].resource,
    };
    let bound = bind_measured_phlo_prepaid(&measured, &[assignment], limits(), &budget()).unwrap();
    assert_eq!(bound.witness().fresh.len(), 1);
    assert_eq!(bound.witness().fresh[0].quantity, u64::MAX - 1);
    assert_eq!(bound.witness().used.len(), 1);
    for changed in [
        PhloExecutionLimits {
            resource_entries: 1,
            ..limits()
        },
        PhloExecutionLimits {
            authority_nodes: 2,
            ..limits()
        },
        PhloExecutionLimits {
            key_bytes: 1,
            ..limits()
        },
    ] {
        assert!(bind_measured_phlo_prepaid(&measured, &[assignment], changed, &budget()).is_err());
    }
    for dimension in [
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
        caps.set(dimension, HostWorkLimit::new(0));
        let work = HostWorkBudget::new(caps);
        assert!(bind_measured_phlo_prepaid(&measured, &[assignment], limits(), &work).is_err());
        assert!(work.is_rejected());
    }
    assert!(bind_measured_phlo_prepaid(
        &[PhloResourceAmount {
            quantity: 0,
            ..measured[0]
        }],
        &[],
        limits(),
        &budget()
    )
    .is_err());
    assert!(bind_measured_phlo_prepaid(&[], &[], limits(), &budget())
        .unwrap()
        .witness()
        .required
        .is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn measured_prepaid_refines_per_occurrence_conservation_and_sequence_composition(
        quantities in prop::collection::vec(1_u64..12, 1..12),
        positions in prop::collection::vec(0_usize..13, 0..48),
        split in any::<usize>(),
        owners in 0_usize..70,
        weight in 0_u64..20,
        price in 0_u64..30,
        force_valid in any::<bool>(),
    ) {
        let mut allowed = quantities.clone();
        let positions: Vec<_> = positions.into_iter().filter(|&index| {
            if !force_valid { return true; }
            if let Some(quantity) = allowed.get_mut(index) {
                if *quantity > 0 { *quantity -= 1; return true; }
            }
            false
        }).collect();
        let owner = authority(owners);
        let measured: Vec<_> = quantities.iter().enumerate().map(|(index, &quantity)| PhloResourceAmount {
            resource: PhloResource { class: index % 2, ..resource(&owner) }, quantity,
        }).collect();
        let assignments: Vec<_> = positions.iter().map(|&demand_index| MeasuredPhloPrepaidUse {
            demand_index,
            resource: PhloResource {
                class: demand_index % 2,
                acquisition_terms: if demand_index % 2 == 0 { b"old-a" } else { b"old-b" },
                ..resource(&owner)
            },
        }).collect();
        let mut residual = quantities.clone();
        let expected = positions.iter().try_for_each(|&index| {
            let quantity = residual.get_mut(index).ok_or(())?;
            *quantity = quantity.checked_sub(1).ok_or(())?;
            Ok::<_, ()>(())
        }).is_ok();
        let result = bind_measured_phlo_prepaid(&measured, &assignments, limits(), &budget());
        prop_assert_eq!(result.is_ok(), expected);
        if !expected { return Ok(()); }
        let bound = result.unwrap();
        let witness = bound.witness();
        prop_assert_eq!(witness.used.len(), positions.len());
        prop_assert!(witness.unused.is_empty());
        let actual_residual: Vec<_> = witness.fresh.iter().map(|amount| amount.quantity).collect();
        prop_assert_eq!(actual_residual, residual.iter().copied().filter(|q| *q > 0).collect::<Vec<_>>());
        for (amount, assignment) in witness.used.iter().zip(&assignments) {
            prop_assert_eq!(amount.resource, assignment.resource);
            prop_assert_eq!(amount.quantity, 1);
        }
        let cut = split % (positions.len() + 1);
        let mut middle = measured.clone();
        for &index in &positions[..cut] { middle[index].quantity -= 1; }
        let positive: Vec<_> = middle.iter().copied().filter(|amount| amount.quantity > 0).collect();
        let translated: Vec<_> = assignments[cut..].iter().map(|assignment| MeasuredPhloPrepaidUse {
            demand_index: middle[..assignment.demand_index].iter().filter(|amount| amount.quantity > 0).count(),
            ..*assignment
        }).collect();
        let rest = bind_measured_phlo_prepaid(&positive, &translated, limits(), &budget()).unwrap();
        prop_assert_eq!(rest.witness().fresh, witness.fresh);
        let mut reversed = assignments.clone();
        reversed.reverse();
        let reverse = bind_measured_phlo_prepaid(&measured, &reversed, limits(), &budget()).unwrap();
        prop_assert_eq!(reverse.witness().fresh, witness.fresh);
        let total = quantities.iter().sum::<u64>() * weight * owners as u64;
        let prepaid = positions.len() as u64 * weight * owners as u64;
        let weights = [weight, weight];
        let schedule = PhloSchedule {
            commitment: [1; 32],
            environment: PhloEnvironment { protocol_version: 6, network: b"test", shard: b"root", asset: b"REV", unit: b"phlo", decimal_scale: 8 },
            weights: &weights,
            actual_price: price,
        };
        let schedules = [schedule];
        let ceilings = [price];
        let controls = check_phlo_controls(schedule.environment, 0, u64::MAX, SignedPhloControls {
            limit: total, price_ceiling: price, required_owner_ceilings: &ceilings, permitted_schedules: &schedules,
        }, schedule, total).unwrap();
        let checked = check_counted_phlo_execution(controls, witness, limits()).unwrap();
        prop_assert_eq!(checked.usage(), total);
        prop_assert_eq!(checked.prepaid_usage(), prepaid);
        prop_assert_eq!(checked.fresh_usage(), total - prepaid);
        prop_assert_eq!(checked.retained_charge(PhloOutcome::Accepted(&[])), 1 + (total - prepaid) * price);
    }
}
