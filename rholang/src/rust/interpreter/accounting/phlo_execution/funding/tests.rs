use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_phlo_execution, project_phlo_obligations, PhloExecutionLimits, PhloExecutionWitness,
    PhloFailure, PhloOutcome, PhloResource,
};
use crate::rust::interpreter::accounting::Sig;

const ENV: PhloEnvironment<'static> = PhloEnvironment {
    decimal_scale: 8,
    protocol_version: 6,
    network: b"network",
    shard: b"shard",
    asset: b"native asset",
    unit: b"native phlo",
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENV,
    weights: &[1],
    actual_price: 2,
}];

fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

fn limits() -> PhloFundingLimits {
    PhloFundingLimits {
        sources: cap(256),
        cases: cap(64),
        obligations: cap(256),
        assignment_cells: 65_536,
        custody_bytes: 4096,
    }
}

fn source(custody: &[u8], capacity: u64) -> PhloFundingSource<'_> {
    PhloFundingSource {
        custody,
        capacity,
        exposure_limit: capacity,
        debit_limit: capacity,
    }
}

fn obligations<'a>(
    resources: &'a [PhloResource<'a>],
    outcome: PhloOutcome<'a>,
    limit: u64,
) -> CheckedPhloObligations<'a> {
    obligations_for_schedule(resources, outcome, limit, &SCHEDULES)
}

fn obligations_for_schedule<'a>(
    resources: &'a [PhloResource<'a>],
    outcome: PhloOutcome<'a>,
    limit: u64,
    schedules: &'a [PhloSchedule<'a>],
) -> CheckedPhloObligations<'a> {
    obligations_for_policy(resources, outcome, limit, schedules, 0)
}

fn obligations_for_policy<'a>(
    resources: &'a [PhloResource<'a>],
    outcome: PhloOutcome<'a>,
    limit: u64,
    schedules: &'a [PhloSchedule<'a>],
    minimum_price: u64,
) -> CheckedPhloObligations<'a> {
    let controls = check_phlo_controls(
        ENV,
        minimum_price,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: schedules[0].actual_price,
            required_owner_ceilings: &[2],
            permitted_schedules: schedules,
        },
        schedules[0],
        limit,
    )
    .unwrap();
    let execution = check_phlo_execution(
        controls,
        PhloExecutionWitness {
            available: &[],
            required: resources,
            used: &[],
            unused: &[],
            fresh: resources,
        },
        PhloExecutionLimits {
            resource_entries: 1024,
            authority_nodes: 8192,
            key_bytes: 65536,
        },
    )
    .unwrap();
    project_phlo_obligations(execution, outcome, cap(256)).unwrap()
}

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        authority,
        location: b"slot",
        acquisition_terms: b"terms",
        class: 0,
    }
}

#[test]
fn exclusive_three_source_cases_require_distinct_exposure_and_original_refunds() {
    let fee = obligations(&[], PhloOutcome::Accepted(&[]), 10);
    let sources = [source(b"a", 1), source(b"b", 1), source(b"c", 1)];
    let flows: Vec<Vec<Vec<u64>>> = (0..3)
        .map(|branch| {
            (0..3)
                .map(|payer| vec![u64::from(payer == branch)])
                .collect()
        })
        .collect();
    let permissions: Vec<Vec<Vec<bool>>> = (0..3)
        .map(|branch| (0..3).map(|payer| vec![payer == branch]).collect())
        .collect();
    let cases: Vec<_> = flows
        .iter()
        .zip(&permissions)
        .map(|(assignment, eligible)| PhloFundingCase {
            obligations: &fee,
            assignment,
            eligible,
        })
        .collect();
    assert_eq!(
        check_phlo_funding_family(&sources, &cases, 1, limits()),
        Err(PhloFundingError::TotalExposureExceeded)
    );
    let family = check_phlo_funding_family(&sources, &cases, 3, limits()).unwrap();
    assert_eq!(family.source_holds(), &[1, 1, 1]);
    assert_eq!(family.maximum_charge(), 1);
    assert_eq!(family.total_held(), 3);
    assert_eq!(family.total_exposure_limit(), 3);
    assert_eq!(family.sources(), sources);
    assert_eq!(family.cases(), cases);
    for selected in 0..3 {
        let debits = family
            .selected_assignment(selected)
            .unwrap()
            .source_debits();
        let refunds = family.refunds(selected).unwrap();
        for payer in 0..3 {
            assert_eq!(refunds[payer].custody, sources[payer].custody);
            assert_eq!(refunds[payer].amount + debits[payer], 1);
        }
    }
    assert_eq!(
        family.refunds(3),
        Err(FundingReservationError::UnknownBranch)
    );
}

#[test]
fn funding_family_rejects_mixed_chain_minima_even_with_identical_charges() {
    let first = obligations_for_policy(&[], PhloOutcome::Accepted(&[]), 10, &SCHEDULES, 1);
    let second = obligations_for_policy(&[], PhloOutcome::Accepted(&[]), 10, &SCHEDULES, 2);
    assert_eq!(
        first.execution().controls().numeric(),
        second.execution().controls().numeric()
    );
    let sources = [source(b"payer", 1)];
    let assignment = vec![vec![1]];
    let eligible = vec![vec![true]];
    let cases = [
        PhloFundingCase {
            obligations: &first,
            eligible: &eligible,
            assignment: &assignment,
        },
        PhloFundingCase {
            obligations: &second,
            eligible: &eligible,
            assignment: &assignment,
        },
    ];
    assert_eq!(
        check_phlo_funding_family(&sources, &cases, 1, limits()),
        Err(PhloFundingError::DifferentControls)
    );
}

#[test]
fn each_source_cap_is_required_and_duplicate_custody_cannot_multiply_capacity() {
    let fee = obligations(&[], PhloOutcome::Accepted(&[]), 10);
    let allowed = vec![vec![true]];
    let flows = vec![vec![1]];
    let cases = [PhloFundingCase {
        obligations: &fee,
        eligible: &allowed,
        assignment: &flows,
    }];
    for field in 0..3 {
        let mut payer = source(b"payer", 1);
        match field {
            0 => payer.capacity = 0,
            1 => payer.exposure_limit = 0,
            _ => payer.debit_limit = 0,
        }
        assert!(check_phlo_funding_family(&[payer], &cases, 1, limits()).is_err());
    }
    let copied = b"payer".to_vec();
    let duplicate = [source(b"payer", 1), source(&copied, 1)];
    let allowed = vec![vec![true], vec![true]];
    let flows = vec![vec![1], vec![0]];
    let cases = [PhloFundingCase {
        obligations: &fee,
        eligible: &allowed,
        assignment: &flows,
    }];
    assert_eq!(
        check_phlo_funding_family(&duplicate, &cases, 2, limits()),
        Err(PhloFundingError::DuplicateCustody)
    );
}

#[test]
fn zero_charge_case_releases_every_hold_to_original_custody() {
    let a = Sig::Ground(vec![1]);
    let resources = [resource(&a)];
    let success = obligations(&resources, PhloOutcome::Accepted(&[]), 10);
    let failed = obligations(
        &resources,
        PhloOutcome::Accepted(&[PhloFailure::User, PhloFailure::Platform]),
        10,
    );
    let sources = [source(b"first", 1), source(b"second", 2)];
    let allowed = vec![vec![true, false], vec![false, true]];
    let charged = vec![vec![1, 0], vec![0, 2]];
    let released = vec![vec![0, 0]; 2];
    let cases = [
        PhloFundingCase {
            obligations: &success,
            eligible: &allowed,
            assignment: &charged,
        },
        PhloFundingCase {
            obligations: &failed,
            eligible: &allowed,
            assignment: &released,
        },
    ];
    let family = check_phlo_funding_family(&sources, &cases, 3, limits()).unwrap();
    assert_eq!(family.source_holds(), &[1, 2]);
    assert_eq!(family.selected_assignment(1).unwrap().total(), 0);
    assert_eq!(family.refunds(1).unwrap(), vec![
        PhloRefund {
            custody: b"first",
            amount: 1
        },
        PhloRefund {
            custody: b"second",
            amount: 2
        }
    ]);
}

#[test]
fn controls_and_duplicate_key_permissions_cannot_change_within_a_checked_family() {
    let ordinary = obligations(&[], PhloOutcome::Accepted(&[]), 10);
    let changed = obligations(&[], PhloOutcome::Accepted(&[]), 11);
    let source = [source(b"payer", 5)];
    let allowed = vec![vec![true]];
    let flows = vec![vec![1]];
    let cases = [
        PhloFundingCase {
            obligations: &ordinary,
            eligible: &allowed,
            assignment: &flows,
        },
        PhloFundingCase {
            obligations: &changed,
            eligible: &allowed,
            assignment: &flows,
        },
    ];
    assert_eq!(
        check_phlo_funding_family(&source, &cases, 5, limits()),
        Err(PhloFundingError::DifferentControls)
    );
    let a = Sig::Ground(vec![1]);
    let resources = [resource(&a); 2];
    let repeated = obligations(
        &resources,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
        10,
    );
    let inconsistent = vec![vec![true, true, false]];
    let zero = vec![vec![0; 3]];
    let cases = [PhloFundingCase {
        obligations: &repeated,
        eligible: &inconsistent,
        assignment: &zero,
    }];
    assert_eq!(
        check_phlo_funding_family(&source, &cases, 5, limits()),
        Err(PhloFundingError::Assignment(
            FundingAssignmentError::InvalidDimensions
        ))
    );
}

#[test]
fn dimensions_and_all_work_limits_reject_before_assignment_checks() {
    let fee = obligations(&[], PhloOutcome::Accepted(&[]), 10);
    let sources = [source(b"a", 1), source(b"b", 1)];
    let allowed = vec![vec![true]; 2];
    let flows = vec![vec![1], vec![0]];
    let cases = [PhloFundingCase {
        obligations: &fee,
        eligible: &allowed,
        assignment: &flows,
    }; 2];
    let exact = PhloFundingLimits {
        sources: cap(2),
        cases: cap(2),
        obligations: cap(1),
        assignment_cells: 4,
        custody_bytes: 2,
    };
    assert!(check_phlo_funding_family(&sources, &cases, 1, exact).is_ok());
    for (changed, expected) in [
        (
            PhloFundingLimits {
                sources: cap(1),
                ..exact
            },
            PhloFundingError::TooManySources,
        ),
        (
            PhloFundingLimits {
                cases: cap(1),
                ..exact
            },
            PhloFundingError::TooManyCases,
        ),
        (
            PhloFundingLimits {
                assignment_cells: 3,
                ..exact
            },
            PhloFundingError::TooManyAssignmentCells,
        ),
        (
            PhloFundingLimits {
                custody_bytes: 1,
                ..exact
            },
            PhloFundingError::TooManyCustodyBytes,
        ),
    ] {
        assert_eq!(
            check_phlo_funding_family(&sources, &cases, 1, changed),
            Err(expected)
        );
    }
    assert_eq!(
        check_phlo_funding_family(&[], &cases, 1, exact),
        Err(PhloFundingError::EmptySources)
    );
    assert_eq!(
        check_phlo_funding_family(&sources, &[], 1, exact),
        Err(PhloFundingError::EmptyCases)
    );
    let malformed = [PhloFundingCase {
        assignment: &[],
        ..cases[0]
    }];
    assert_eq!(
        check_phlo_funding_family(&sources, &malformed, 1, exact),
        Err(PhloFundingError::Assignment(
            FundingAssignmentError::InvalidDimensions
        ))
    );
    let a = Sig::Unit;
    let resources = [resource(&a)];
    let two = obligations(&resources, PhloOutcome::Accepted(&[]), 10);
    let over = [PhloFundingCase {
        obligations: &two,
        ..cases[0]
    }];
    assert_eq!(
        check_phlo_funding_family(&sources, &over, 1, exact),
        Err(PhloFundingError::TooManyObligations)
    );
}

#[test]
fn native_amount_lowering_preserves_boundaries_and_rejects_invalid_arithmetic() {
    let maximum = i64::MAX as u64;
    for (hold, debit, fee) in [
        (0, 0, 0),
        (maximum, maximum, 0),
        (maximum, maximum, maximum),
        (maximum, 0, 0),
        (maximum, maximum - 1, 1),
    ] {
        let result = NativePhloSourceAmounts::checked(b"source", hold, debit, fee).unwrap();
        assert_eq!(result.custody(), b"source");
        assert_eq!(result.hold() as u64, hold);
        assert_eq!(result.acquisition() as u64 + result.fee() as u64, debit);
        assert_eq!(
            result.acquisition() as u64 + result.fee() as u64 + result.refund() as u64,
            hold
        );
    }
    for (hold, debit, fee) in [
        (maximum + 1, 0, 0),
        (maximum, maximum + 1, 0),
        (maximum, maximum, maximum + 1),
        (u64::MAX, 0, 0),
    ] {
        assert_eq!(
            NativePhloSourceAmounts::checked(b"source", hold, debit, fee),
            Err(NativePhloAmountError::OutOfRange)
        );
    }
    for (hold, debit, fee) in [(0, 1, 0), (1, 0, 1), (4, 3, 4)] {
        assert_eq!(
            NativePhloSourceAmounts::checked(b"source", hold, debit, fee),
            Err(NativePhloAmountError::InconsistentAmounts)
        );
    }
}

#[test]
fn native_family_lowering_checks_every_source_before_returning_any_amounts() {
    let a = Sig::Ground(vec![1]);
    let resources = [resource(&a)];
    let maximum = i64::MAX as u64;
    for large in [maximum, maximum + 1] {
        let weights = [large];
        let schedules = [PhloSchedule {
            weights: &weights,
            actual_price: 1,
            ..SCHEDULES[0]
        }];
        let obligations =
            obligations_for_schedule(&resources, PhloOutcome::Accepted(&[]), large, &schedules);
        let sources = [
            source(b"fee", 1),
            source(b"resource", large),
            source(b"unused", 0),
        ];
        let allowed = vec![vec![true, false], vec![false, true], vec![false, false]];
        let assignment = vec![vec![1, 0], vec![0, large], vec![0, 0]];
        let cases = [PhloFundingCase {
            obligations: &obligations,
            eligible: &allowed,
            assignment: &assignment,
        }];
        let family =
            check_phlo_funding_family(&sources, &cases, u128::from(large) + 1, limits()).unwrap();
        let original = family.clone();
        assert_eq!(
            family.native_amounts(1),
            Err(NativePhloAmountError::Selection(
                FundingReservationError::UnknownBranch
            ))
        );
        if large > maximum {
            assert_eq!(
                family.native_amounts(0),
                Err(NativePhloAmountError::OutOfRange)
            );
        } else {
            let native = family.native_amounts(0).unwrap();
            assert_eq!(native.len(), 3);
            assert_eq!(native[0].fee(), 1);
            assert_eq!(native[1].acquisition(), i64::MAX);
            assert_eq!(native[1].fee(), 0);
            assert_eq!(native[2].custody(), b"unused");
            assert_eq!(native[2].hold(), 0);
            assert_eq!(
                native
                    .iter()
                    .map(|amount| amount.hold() as u128)
                    .sum::<u128>(),
                family.total_held()
            );
        }
        assert_eq!(family, original);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_outcome_families_require_one_captured_chain_minimum(
        descriptions in prop::collection::vec((0_u64..=2, any::<bool>()), 0..=64),
        common_minimum in 0_u64..=2,
        use_common in any::<bool>(),
        reverse in any::<bool>(),
    ) {
        let minima: Vec<_> = descriptions.iter()
            .map(|(minimum, _)| if use_common { common_minimum } else { *minimum })
            .collect();
        let obligations: Vec<_> = descriptions.iter().zip(&minima)
            .map(|((_, charged), minimum)| obligations_for_policy(
                &[],
                if *charged { PhloOutcome::Accepted(&[]) } else { PhloOutcome::Accepted(&[PhloFailure::Platform]) },
                10,
                &SCHEDULES,
                *minimum,
            ))
            .collect();
        let sources = [source(b"payer", 1)];
        let assignments: Vec<_> = descriptions.iter()
            .map(|(_, charged)| vec![vec![u64::from(*charged)]])
            .collect();
        let eligible = vec![vec![true]];
        let mut cases: Vec<_> = obligations.iter().zip(&assignments)
            .map(|(obligations, assignment)| PhloFundingCase { obligations, eligible: &eligible, assignment })
            .collect();
        if reverse {
            cases.reverse();
        }
        let result = check_phlo_funding_family(&sources, &cases, 1, limits());
        if minima.is_empty() {
            prop_assert_eq!(result, Err(PhloFundingError::EmptyCases));
        } else if minima.iter().any(|minimum| *minimum != minima[0]) {
            prop_assert_eq!(result, Err(PhloFundingError::DifferentControls));
        } else {
            let checked = result.unwrap();
            prop_assert_eq!(checked.cases().len(), descriptions.len());
            for case in checked.cases() {
                prop_assert_eq!(case.obligations.execution().controls().minimum_price(), minima[0]);
            }
        }
    }

    #[test]
    fn native_amounts_match_full_width_acceptance_and_conservation(
        hold in any::<u64>(), debit in any::<u64>(), fee in any::<u64>(),
    ) {
        let expected = hold <= i64::MAX as u64 && debit <= hold && fee <= debit;
        let result = NativePhloSourceAmounts::checked(b"source", hold, debit, fee);
        prop_assert_eq!(result.is_ok(), expected);
        if let Ok(amounts) = result {
            prop_assert!(amounts.hold() >= 0 && amounts.acquisition() >= 0 && amounts.fee() >= 0 && amounts.refund() >= 0);
            prop_assert_eq!(amounts.hold() as u128, u128::from(hold));
            prop_assert_eq!(amounts.fee() as u128, u128::from(fee));
            prop_assert_eq!(amounts.acquisition() as u128 + amounts.fee() as u128, u128::from(debit));
            prop_assert_eq!(amounts.acquisition() as u128 + amounts.fee() as u128 + amounts.refund() as u128, u128::from(hold));
        }
    }

    #[test]
    fn positive_native_amounts_cover_generated_signed_range_partitions(
        hold in 0_u64..=i64::MAX as u64, debit_seed in any::<u64>(), fee_seed in any::<u64>(),
    ) {
        let debit = debit_seed % (hold + 1);
        let fee = fee_seed % (debit + 1);
        let amounts = NativePhloSourceAmounts::checked(b"source", hold, debit, fee).unwrap();
        prop_assert_eq!(amounts.acquisition() as u64, debit - fee);
        prop_assert_eq!(amounts.refund() as u64, hold - debit);
        prop_assert_eq!(amounts.fee() as u64, fee);
    }

    #[test]
    fn generated_resource_assignments_preserve_independent_source_and_total_bounds(
        data in (1_usize..17, 0_usize..9, 1_usize..9).prop_flat_map(|(sources, resources, branches)|
            (Just((sources, resources)), prop::collection::vec(
                (any::<bool>(), 0..sources, prop::collection::vec((0..sources, 0..sources), resources)), branches))),
    ) {
        let ((count, resource_count), descriptions) = data;
        let ids: Vec<_> = (0..count).map(|index| index.to_le_bytes().to_vec()).collect();
        let authorities: Vec<_> = (0..resource_count).map(|index| Sig::Ground(index.to_le_bytes().to_vec())).collect();
        let resources: Vec<_> = authorities.iter().map(resource).collect();
        let charged = obligations(&resources, PhloOutcome::Accepted(&[]), 10);
        let zero = obligations(&resources, PhloOutcome::Accepted(&[PhloFailure::Platform]), 10);
        let mut flows = Vec::new();
        let mut allowed = Vec::new();
        let mut independent_debits = Vec::new();
        for (billable, fee_source, draws) in &descriptions {
            let mut branch_flows = vec![vec![0_u64; resource_count + 1]; count];
            let mut branch_allowed = vec![vec![false; resource_count + 1]; count];
            let mut debits = vec![0_u64; count];
            branch_allowed[*fee_source][0] = true;
            if *billable {
                branch_flows[*fee_source][0] = 1;
                debits[*fee_source] += 1;
            }
            for (slot, (first, second)) in draws.iter().enumerate() {
                for source in [*first, *second] {
                    branch_allowed[source][slot + 1] = true;
                    if *billable {
                        branch_flows[source][slot + 1] += 1;
                        debits[source] += 1;
                    }
                }
            }
            flows.push(branch_flows);
            allowed.push(branch_allowed);
            independent_debits.push(debits);
        }
        let expected: Vec<_> = (0..count).map(|index| independent_debits.iter().map(|row| row[index]).max().unwrap()).collect();
        let sources: Vec<_> = ids.iter().zip(&expected).map(|(id, held)| source(id, *held)).collect();
        let cases: Vec<_> = flows.iter().zip(&allowed).zip(&descriptions).map(|((assignment, eligible), (billable, _, _))| PhloFundingCase {
            obligations: if *billable { &charged } else { &zero }, assignment, eligible,
        }).collect();
        let total: u128 = expected.iter().map(|value| u128::from(*value)).sum();
        let family = check_phlo_funding_family(&sources, &cases, total, limits()).unwrap();
        prop_assert_eq!(family.source_holds(), expected.as_slice());
        for (branch, expected_debits) in independent_debits.iter().enumerate() {
            let assignment = family.selected_assignment(branch).unwrap();
            prop_assert_eq!(assignment.source_debits(), expected_debits.as_slice());
            prop_assert_eq!(assignment.total(), if descriptions[branch].0 { 1 + 2 * resource_count as u64 } else { 0 });
            let refunds = family.refunds(branch).unwrap();
            for source in 0..count {
                prop_assert_eq!(refunds[source].amount + expected_debits[source], expected[source]);
                prop_assert_eq!(refunds[source].custody, ids[source].as_slice());
            }
        }
        for (index, held) in expected.iter().enumerate().filter(|(_, held)| **held != 0) {
            for field in 0..3 {
                let mut too_low = sources.clone();
                match field {
                    0 => too_low[index].capacity = held - 1,
                    1 => too_low[index].exposure_limit = held - 1,
                    _ => too_low[index].debit_limit = held - 1,
                }
                prop_assert!(check_phlo_funding_family(&too_low, &cases, total, limits()).is_err());
            }
        }
    }

    #[test]
    fn arbitrary_outcome_families_preserve_maximum_holds_and_original_refunds(
        data in (1_usize..130).prop_flat_map(|sources| (Just(sources), prop::collection::vec((0..sources, any::<bool>()), 1..33))),
    ) {
        let (count, selections) = data;
        let ids: Vec<_> = (0..count).map(|source| source.to_le_bytes().to_vec()).collect();
        let sources: Vec<_> = ids.iter().map(|id| source(id, 1)).collect();
        let charged = obligations(&[], PhloOutcome::Accepted(&[]), 10);
        let zero = obligations(&[], PhloOutcome::AdmissionRejected, 10);
        let assignments: Vec<Vec<Vec<u64>>> = selections.iter().map(|(selected, billable)| (0..count).map(|source| vec![u64::from(*billable && source == *selected)]).collect()).collect();
        let allowed: Vec<Vec<Vec<bool>>> = selections.iter().map(|(selected, _)| (0..count).map(|source| vec![source == *selected]).collect()).collect();
        let mut cases: Vec<_> = assignments.iter().zip(&allowed).zip(&selections).map(|((assignment, eligible), (_, billable))| PhloFundingCase { obligations: if *billable { &charged } else { &zero }, assignment, eligible }).collect();
        let expected: Vec<u64> = (0..count).map(|source| u64::from(selections.iter().any(|(selected, billable)| *billable && *selected == source))).collect();
        let total: u128 = expected.iter().map(|value| u128::from(*value)).sum();
        let family = check_phlo_funding_family(&sources, &cases, total, limits()).unwrap();
        prop_assert_eq!(family.source_holds(), expected.as_slice());
        prop_assert_eq!(family.total_held(), total);
        for branch in 0..cases.len() {
            let refunds = family.refunds(branch).unwrap();
            let debits = family.selected_assignment(branch).unwrap().source_debits();
            for source in 0..count {
                prop_assert_eq!(refunds[source].custody, ids[source].as_slice());
                prop_assert_eq!(refunds[source].amount + debits[source], expected[source]);
            }
            prop_assert_eq!(refunds.iter().map(|refund| u128::from(refund.amount)).sum::<u128>() + u128::from(family.selected_assignment(branch).unwrap().total()), total);
        }
        if total != 0 {
            prop_assert_eq!(check_phlo_funding_family(&sources, &cases, total - 1, limits()), Err(PhloFundingError::TotalExposureExceeded));
        }
        cases.reverse();
        let reordered = check_phlo_funding_family(&sources, &cases, total, limits()).unwrap();
        prop_assert_eq!(reordered.source_holds(), expected.as_slice());
    }
}
