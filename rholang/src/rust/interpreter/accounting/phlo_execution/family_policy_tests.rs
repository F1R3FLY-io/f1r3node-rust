use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_obligation::PhloObligationKeyLimits;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn counted_run_order_matches_expanded_lexicographic_order(
        left in prop::collection::vec((any::<u8>(), 1_u64..12), 0..32),
        right in prop::collection::vec((any::<u8>(), 1_u64..12), 0..32),
    ) {
        let expanded = |runs: &[(u8, u64)]| -> Vec<_> {
            runs.iter().flat_map(|(v, n)| std::iter::repeat_n(*v, *n as usize)).collect()
        };
        let (lv, lc): (Vec<_>, Vec<_>) = left.iter().copied().unzip();
        let (rv, rc): (Vec<_>, Vec<_>) = right.iter().copied().unzip();
        prop_assert_eq!(compare_runs(&lv, &lc, &rv, &rc), expanded(&left).cmp(&expanded(&right)));
    }
}

#[test]
fn counted_run_order_skips_maximum_quantities_without_expansion() {
    assert_eq!(
        compare_runs(&[1, 2], &[u64::MAX, 1], &[1, 3], &[u64::MAX, 1]),
        Ordering::Less
    );
    assert_eq!(
        compare_runs(&[1, 1, 2], &[u64::MAX - 1, 1, 1], &[1, 2], &[u64::MAX, 1]),
        Ordering::Equal
    );
    assert_eq!(
        compare_runs(&[1], &[u64::MAX], &[1, 1], &[u64::MAX, u64::MAX]),
        Ordering::Less
    );
}

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};

const ENVIRONMENT: PhloEnvironment<'static> = PhloEnvironment {
    decimal_scale: 8,
    protocol_version: 6,
    network: b"family test network",
    shard: b"family test shard",
    asset: b"native asset",
    unit: b"native phlo",
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENVIRONMENT,
    weights: &[1, 2, 0],
    actual_price: 1,
}];

fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits() -> PhloFamilyFundingLimits {
    PhloFamilyFundingLimits {
        funding: PhloFundingLimits {
            sources: cap(129),
            cases: cap(20),
            obligations: cap(100),
            assignment_cells: 100_000,
            custody_bytes: 10_000,
        },
        keys: PhloObligationKeyLimits {
            wire: models::rust::phlo_wire::PhloWireLimits {
                total_bytes: 16_384,
                field_bytes: 8192,
            },
            authority_nodes: 100,
        },
        aggregate_key_bytes: 1_000_000,
    }
}

fn resource<'a>(authority: &'a Sig, location: &'a [u8]) -> PhloResource<'a> {
    PhloResource {
        location,
        class: 0,
        acquisition_terms: b"terms",
        authority,
    }
}

fn obligations<'a>(
    prepaid: &'a [PhloResource<'a>],
    required: &'a [PhloResource<'a>],
    fresh: &'a [PhloResource<'a>],
    outcome: PhloOutcome<'a>,
    signed_limit: u64,
) -> CheckedPhloObligations<'a> {
    let controls = check_phlo_controls(
        ENVIRONMENT,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: signed_limit,
            price_ceiling: 1,
            required_owner_ceilings: &[u64::MAX],
            permitted_schedules: &SCHEDULES,
        },
        SCHEDULES[0],
        signed_limit,
    )
    .unwrap();
    let execution = check_phlo_execution(
        controls,
        PhloExecutionWitness {
            available: prepaid,
            required,
            used: prepaid,
            unused: &[],
            fresh,
        },
        PhloExecutionLimits {
            resource_entries: 100_000,
            authority_nodes: 1_000_000,
            key_bytes: 16_000_000,
        },
    )
    .unwrap();
    project_phlo_obligations(execution, outcome, cap(100)).unwrap()
}

fn source(custody: &[u8], capacity: u64) -> PhloFundingSource<'_> {
    PhloFundingSource {
        custody,
        capacity,
        exposure_limit: capacity,
        debit_limit: capacity,
    }
}

fn input<'a>(
    sources: &'a [PhloFundingSource<'a>],
    outcomes: &'a [PhloFundingRequirement<'a>],
    total_exposure_limit: u128,
) -> PhloFamilyFundingInput<'a> {
    PhloFamilyFundingInput {
        sources,
        outcomes,
        total_exposure_limit,
        canonical_resource_cursor: 0,
        canonical_fee_cursor: 0,
    }
}

#[test]
fn complete_family_fits_exposure_when_independent_preferred_allocations_do_not() {
    let authority = Sig::Ground(vec![1]);
    let alpha_resources = [resource(&authority, b"a1"), resource(&authority, b"a2")];
    let beta_resources = [
        resource(&authority, b"z1"),
        resource(&authority, b"z2"),
        resource(&authority, b"z2"),
    ];
    let alpha = obligations(
        &[],
        &alpha_resources,
        &alpha_resources,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let beta = obligations(
        &[],
        &beta_resources,
        &beta_resources,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let sources = [
        source(b"A", 2),
        source(b"B", 2),
        source(b"C", 2),
        source(b"D", 2),
        source(b"E", 1),
    ];
    let alpha_edges = [
        vec![false, false, false],
        vec![false, true, false],
        vec![false, true, true],
        vec![false, true, false],
        vec![true, false, false],
    ];
    let beta_edges = [
        vec![false, true, false],
        vec![false, true, false],
        vec![false, false, true],
        vec![false, false, true],
        vec![true, false, false],
    ];
    let requirements = [
        PhloFundingRequirement {
            obligations: &beta,
            eligible: &beta_edges,
        },
        PhloFundingRequirement {
            obligations: &alpha,
            eligible: &alpha_edges,
        },
    ];
    let selected = select_phlo_funding_family(input(&sources, &requirements, 4), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.canonical_outcome_positions(), &[1, 0]);
    assert_eq!(selected.source_holds(), &[0, 1, 1, 1, 1]);
    assert_eq!(selected.total_held(), 4);
    let resource_debits: Vec<Vec<u64>> = selected
        .assignments()
        .iter()
        .map(|case| case.iter().map(|row| row[1..].iter().sum()).collect())
        .collect();
    assert_eq!(resource_debits, vec![vec![0, 1, 1, 1, 0], vec![
        0, 1, 1, 0, 0
    ]]);
    let cases: Vec<_> = requirements
        .iter()
        .zip(selected.assignments())
        .map(|(requirement, assignment)| PhloFundingCase {
            obligations: requirement.obligations,
            eligible: requirement.eligible,
            assignment,
        })
        .collect();
    let checked = check_phlo_funding_family(&sources, &cases, 4, limits().funding).unwrap();
    assert_eq!(checked.source_holds(), selected.source_holds());
    for (case, expected) in [(0, (3, 1, 0)), (1, (2, 1, 1))] {
        let native = checked.native_amounts(case).unwrap();
        let actual = native
            .iter()
            .fold((0, 0, 0), |(acquisition, fee, refund), source| {
                (
                    acquisition + source.acquisition(),
                    fee + source.fee(),
                    refund + source.refund(),
                )
            });
        assert_eq!(actual, expected);
    }
}

#[test]
fn canonical_outcome_priority_couples_payers_despite_a_complete_later_projection() {
    let authority = Sig::Ground(vec![1]);
    let first_resources = [resource(&authority, b"a")];
    let second_resources = [resource(&authority, b"z")];
    let first = obligations(
        &[],
        &first_resources,
        &first_resources,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let second = obligations(
        &[],
        &second_resources,
        &second_resources,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let sources = [
        source(b"A", 2),
        source(b"B", 2),
        source(b"C", 2),
        source(b"D", 1),
    ];
    let first_edges = [
        vec![false, false],
        vec![false, true],
        vec![false, true],
        vec![true, false],
    ];
    let second_edges = [
        vec![false, true],
        vec![false, true],
        vec![true, true],
        vec![false, true],
    ];
    let requirements = [
        PhloFundingRequirement {
            obligations: &first,
            eligible: &first_edges,
        },
        PhloFundingRequirement {
            obligations: &second,
            eligible: &second_edges,
        },
    ];
    for second_payer in 0..sources.len() {
        let mut first_assignment = vec![vec![0; 2]; sources.len()];
        let mut second_assignment = vec![vec![0; 2]; sources.len()];
        first_assignment[3][0] = 1;
        first_assignment[if second_payer == 1 { 1 } else { 2 }][1] = 1;
        second_assignment[2][0] = 1;
        second_assignment[second_payer][1] = 1;
        let cases = [
            PhloFundingCase {
                obligations: &first,
                eligible: &first_edges,
                assignment: &first_assignment,
            },
            PhloFundingCase {
                obligations: &second,
                eligible: &second_edges,
                assignment: &second_assignment,
            },
        ];
        assert!(check_phlo_funding_family(&sources, &cases, 3, limits().funding).is_ok());
    }
    let standalone = [requirements[1]];
    let independently_selected =
        select_phlo_funding_family(input(&sources, &standalone, 3), limits(), &work())
            .unwrap()
            .unwrap();
    assert_eq!(independently_selected.assignments()[0], vec![
        vec![0, 1],
        vec![0, 0],
        vec![1, 0],
        vec![0, 0]
    ]);
    let selected = select_phlo_funding_family(input(&sources, &requirements, 3), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.canonical_outcome_positions(), &[0, 1]);
    assert_eq!(selected.assignments(), &[
        vec![vec![0, 0], vec![0, 1], vec![0, 0], vec![1, 0]],
        vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![0, 0]],
    ]);
    assert_eq!(selected.source_holds(), &[0, 1, 1, 1]);
    assert_eq!(selected.total_held(), 3);
    let cases: Vec<_> = requirements
        .iter()
        .zip(selected.assignments())
        .map(|(requirement, assignment)| PhloFundingCase {
            obligations: requirement.obligations,
            eligible: requirement.eligible,
            assignment,
        })
        .collect();
    let checked = check_phlo_funding_family(&sources, &cases, 3, limits().funding).unwrap();
    for case in 0..2 {
        let native = checked.native_amounts(case).unwrap();
        assert_eq!(
            native
                .iter()
                .map(|amount| amount.acquisition())
                .collect::<Vec<_>>(),
            vec![0, 1, 0, 0]
        );
        assert_eq!(
            native.iter().map(|amount| amount.fee()).collect::<Vec<_>>(),
            if case == 0 {
                vec![0, 0, 0, 1]
            } else {
                vec![0, 0, 1, 0]
            }
        );
        assert_eq!(
            native
                .iter()
                .map(|amount| amount.refund())
                .collect::<Vec<_>>(),
            if case == 0 {
                vec![0, 0, 1, 0]
            } else {
                vec![0, 0, 0, 1]
            }
        );
    }
}

#[test]
fn family_coupling_changes_the_first_resource_draws_from_the_standalone_cursor_basis() {
    let authority = Sig::Ground(vec![1]);
    let first_resources = [resource(&authority, b"a"); 2];
    let second_resources = [resource(&authority, b"z"); 3];
    let first = obligations(
        &[],
        &first_resources,
        &first_resources,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let second = obligations(
        &[],
        &second_resources,
        &second_resources,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let sources = [source(b"A", 3), source(b"B", 3), source(b"C", 3)];
    let first_edges = [vec![true, true], vec![false, true], vec![false, true]];
    let second_edges = [vec![false, true], vec![true, true], vec![false, true]];
    let standalone = first
        .select_funding(
            PhloObligationFundingInput {
                source_keys: &[b"A", b"B", b"C"],
                capacities: &[3, 3, 3],
                eligible: &first_edges,
                canonical_resource_cursor: 0,
                canonical_fee_cursor: 0,
            },
            PhloObligationFundingLimits {
                search: FundingSearchLimits {
                    source_cap: cap(3),
                    obligation_cap: cap(4),
                },
                keys: limits().keys,
                aggregate_key_bytes: limits().aggregate_key_bytes,
            },
            &work(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        standalone
            .assignment()
            .iter()
            .map(|row| row[1..].iter().sum::<u64>())
            .collect::<Vec<_>>(),
        vec![1, 1, 0]
    );
    assert_eq!(standalone.resource_next_cursor(), Some(2));
    let first_concentrated = [vec![1, 2], vec![0, 0], vec![0, 0]];
    let second_concentrated = [vec![0, 3], vec![1, 0], vec![0, 0]];
    let marginal_witness = [
        PhloFundingCase {
            obligations: &first,
            eligible: &first_edges,
            assignment: &first_concentrated,
        },
        PhloFundingCase {
            obligations: &second,
            eligible: &second_edges,
            assignment: &second_concentrated,
        },
    ];
    assert_eq!(
        check_phlo_funding_family(&sources, &marginal_witness, 4, limits().funding)
            .unwrap()
            .source_holds(),
        &[3, 1, 0]
    );
    let requirements = [
        PhloFundingRequirement {
            obligations: &first,
            eligible: &first_edges,
        },
        PhloFundingRequirement {
            obligations: &second,
            eligible: &second_edges,
        },
    ];
    let selected = select_phlo_funding_family(input(&sources, &requirements, 4), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.canonical_outcome_positions(), &[0, 1]);
    assert_eq!(selected.source_holds(), &[1, 2, 1]);
    assert_eq!(selected.total_held(), 4);
    assert!(!selected.cursor_transitions()[0].resource_unrestricted());
    assert_eq!(
        selected.cursor_transitions()[0].resource_next_cursor(),
        Some(1)
    );
    assert_ne!(
        selected.cursor_transitions()[0].resource_next_cursor(),
        standalone.resource_next_cursor()
    );
    assert!(selected.cursor_transitions()[0]
        .resource_restriction_witness()
        .is_some());
    assert_eq!(selected.cursor_transitions()[0].possible_fee_payers(), &[
        true, false, false
    ]);
    assert_eq!(selected.cursor_transitions()[1].possible_fee_payers(), &[
        false, true, false
    ]);
    let resource_debits: Vec<Vec<u64>> = selected
        .assignments()
        .iter()
        .map(|assignment| assignment.iter().map(|row| row[1..].iter().sum()).collect())
        .collect();
    assert_eq!(resource_debits, vec![vec![0, 1, 1], vec![1, 1, 1]]);
    assert_eq!(
        selected.assignments()[0]
            .iter()
            .map(|row| row[0])
            .collect::<Vec<_>>(),
        vec![1, 0, 0]
    );
    assert_eq!(
        selected.assignments()[1]
            .iter()
            .map(|row| row[0])
            .collect::<Vec<_>>(),
        vec![0, 1, 0]
    );
    let cases: Vec<_> = requirements
        .iter()
        .zip(selected.assignments())
        .map(|(requirement, assignment)| PhloFundingCase {
            obligations: requirement.obligations,
            eligible: requirement.eligible,
            assignment,
        })
        .collect();
    let checked = check_phlo_funding_family(&sources, &cases, 4, limits().funding).unwrap();
    assert_eq!(
        checked
            .native_amounts(0)
            .unwrap()
            .iter()
            .map(|amount| amount.refund())
            .collect::<Vec<_>>(),
        vec![0, 1, 0]
    );
    assert_eq!(
        checked
            .native_amounts(1)
            .unwrap()
            .iter()
            .map(|amount| amount.refund())
            .collect::<Vec<_>>(),
        vec![0, 0, 0]
    );
}

#[test]
fn duplicate_occurrences_and_duplicate_outcomes_are_retained_without_duplicate_holds() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot"); 3];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let sources = [source(b"A", 4)];
    let edges = [vec![true; 2]];
    let requirement = PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    };
    let single = [requirement];
    let repeated = [requirement, requirement, requirement];
    let one = select_phlo_funding_family(input(&sources, &single, 4), limits(), &work())
        .unwrap()
        .unwrap();
    let three = select_phlo_funding_family(input(&sources, &repeated, 4), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(one.assignments(), &[vec![vec![1, 3]]]);
    assert_eq!(three.assignments(), &vec![one.assignments()[0].clone(); 3]);
    assert_eq!(three.source_holds(), one.source_holds());
    assert_eq!(three.total_held(), 4);
    assert_eq!(three.cursor_transitions(), &vec![
        one.cursor_transitions()[0]
            .clone();
        3
    ]);
    let mut positions = three.canonical_outcome_positions().to_vec();
    positions.sort_unstable();
    assert_eq!(positions, vec![0, 1, 2]);
    assert!(
        select_phlo_funding_family(input(&sources, &single, 3), limits(), &work())
            .unwrap()
            .is_none()
    );
}

#[test]
fn duplicate_multiwallet_outcomes_retain_one_canonical_cursor_transition() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot"); 2];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let sources = [source(b"B", 2), source(b"A", 2), source(b"C", 2)];
    let edges = [vec![true; 2], vec![true; 2], vec![true; 2]];
    let requirement = PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    };
    let single = [requirement];
    let duplicates = [requirement; 3];
    let single_input = PhloFamilyFundingInput {
        canonical_resource_cursor: 1,
        canonical_fee_cursor: 2,
        ..input(&sources, &single, 3)
    };
    let once = select_phlo_funding_family(single_input, limits(), &work())
        .unwrap()
        .unwrap();
    let repeated = select_phlo_funding_family(
        PhloFamilyFundingInput {
            outcomes: &duplicates,
            ..single_input
        },
        limits(),
        &work(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(repeated.assignments(), &vec![
        once.assignments()[0].clone();
        3
    ]);
    assert_eq!(repeated.source_holds(), once.source_holds());
    assert_eq!(repeated.cursor_transitions(), &vec![
        once.cursor_transitions(
        )[0]
        .clone();
        3
    ]);
    assert_eq!(once.cursor_transitions()[0].resource_next_cursor(), Some(0));
    assert_eq!(once.cursor_transitions()[0].fee_next_cursor(), Some(0));
    assert_eq!(once.cursor_transitions()[0].possible_fee_payers(), &[
        true, true, true
    ]);
}

#[test]
fn fee_only_and_nonbillable_outcomes_have_exact_separate_charges() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot")];
    let success = obligations(&fresh, &fresh, &[], PhloOutcome::Accepted(&[]), 100);
    let rejected = obligations(&[], &fresh, &fresh, PhloOutcome::AdmissionRejected, 100);
    let failed = obligations(
        &[],
        &fresh,
        &fresh,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
        100,
    );
    let sources = [source(b"B", 1), source(b"A", 1)];
    let fee_edges = [vec![true], vec![true]];
    let zero_edges = [vec![false; 2], vec![false; 2]];
    let requirements = [
        PhloFundingRequirement {
            obligations: &success,
            eligible: &fee_edges,
        },
        PhloFundingRequirement {
            obligations: &rejected,
            eligible: &zero_edges,
        },
        PhloFundingRequirement {
            obligations: &failed,
            eligible: &zero_edges,
        },
    ];
    let selected = select_phlo_funding_family(input(&sources, &requirements, 1), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.assignments(), &[
        vec![vec![0], vec![1]],
        vec![vec![0, 0], vec![0, 0]],
        vec![vec![0, 0], vec![0, 0]]
    ]);
    assert_eq!(selected.source_holds(), &[0, 1]);
    let success_transition = &selected.cursor_transitions()[0];
    assert!(success_transition.resource_unrestricted());
    assert_eq!(success_transition.resource_next_cursor(), None);
    assert_eq!(success_transition.fee_next_cursor(), Some(1));
    assert_eq!(success_transition.possible_fee_payers(), &[true, true]);
    for transition in &selected.cursor_transitions()[1..] {
        assert!(transition.resource_unrestricted());
        assert_eq!(transition.resource_next_cursor(), None);
        assert_eq!(transition.fee_next_cursor(), None);
        assert_eq!(transition.possible_fee_payers(), &[false, false]);
        assert_eq!(transition.resource_restriction_witness(), None);
    }
}

#[test]
fn funding_rejects_aliases_control_mismatch_and_obsolete_occurrence_dimensions() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot"); 2];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let different_controls = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 101);
    let edges = [vec![true; 2], vec![true; 2]];
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    }];
    let aliases = [source(b"A", 4), source(b"A", 4)];
    assert!(matches!(
        select_phlo_funding_family(input(&aliases, &requirements, 8), limits(), &work()),
        Err(PhloFamilyFundingError::Identity(
            FundingIdentityError::DuplicateSource
        ))
    ));
    let sources = [source(b"A", 4), source(b"B", 4)];
    let mixed = [requirements[0], PhloFundingRequirement {
        obligations: &different_controls,
        eligible: &edges,
    }];
    assert!(matches!(
        select_phlo_funding_family(input(&sources, &mixed, 8), limits(), &work()),
        Err(PhloFamilyFundingError::Funding(
            PhloFundingError::DifferentControls
        ))
    ));
    let conflicting = [vec![true, true, false], vec![true; 3]];
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &conflicting,
    }];
    let error = select_phlo_funding_family(input(&sources, &requirements, 8), limits(), &work())
        .unwrap_err();
    assert!(matches!(
        error,
        PhloFamilyFundingError::Search(FundingSearchError::InvalidProblem(
            FundingAssignmentError::InvalidDimensions
        ))
    ));
}

#[test]
fn grouping_preserves_expanded_outcome_priority_not_aggregate_amount_order() {
    let authority = Sig::Ground(vec![1]);
    let alpha = [
        resource(&authority, b"a"),
        resource(&authority, b"a"),
        resource(&authority, b"z"),
    ];
    let beta = [
        resource(&authority, b"a"),
        resource(&authority, b"z"),
        resource(&authority, b"z"),
    ];
    let alpha_bill = obligations(&[], &alpha, &alpha, PhloOutcome::Accepted(&[]), 100);
    let beta_bill = obligations(&[], &beta, &beta, PhloOutcome::Accepted(&[]), 100);
    assert!(alpha_bill.amounts() > beta_bill.amounts());
    let sources = [source(b"A", 4), source(b"B", 4)];
    let edges = [vec![true; 3], vec![true; 3]];
    let requirements = [
        PhloFundingRequirement {
            obligations: &beta_bill,
            eligible: &edges,
        },
        PhloFundingRequirement {
            obligations: &alpha_bill,
            eligible: &edges,
        },
    ];
    let selected = select_phlo_funding_family(input(&sources, &requirements, 8), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.canonical_outcome_positions(), &[1, 0]);
    let reversed = [requirements[1], requirements[0]];
    let permuted = select_phlo_funding_family(input(&sources, &reversed, 8), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(permuted.canonical_outcome_positions(), &[0, 1]);
    assert_eq!(selected.source_holds(), permuted.source_holds());
    for original in 0..2 {
        assert_eq!(
            selected.assignments()[original],
            permuted.assignments()[1 - original]
        );
        assert_eq!(
            selected.cursor_transitions()[original],
            permuted.cursor_transitions()[1 - original]
        );
    }
}

#[test]
fn exposure_and_debit_caps_apply_to_resource_plus_fee_not_resources_alone() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot"); 2];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let edges = [vec![true; 2]];
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    }];
    for source in [
        PhloFundingSource {
            debit_limit: 2,
            ..source(b"A", 3)
        },
        PhloFundingSource {
            exposure_limit: 2,
            ..source(b"A", 3)
        },
    ] {
        assert!(
            select_phlo_funding_family(input(&[source], &requirements, 3), limits(), &work())
                .unwrap()
                .is_none()
        );
    }
    let sources = [source(b"A", 3)];
    assert_eq!(
        select_phlo_funding_family(input(&sources, &requirements, 3), limits(), &work())
            .unwrap()
            .unwrap()
            .source_holds(),
        &[3]
    );
}

#[test]
fn typed_obligation_permutation_restores_the_original_columns() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [
        resource(&authority, b"z"),
        resource(&authority, b"a"),
        PhloResource {
            class: 1,
            ..resource(&authority, b"m")
        },
    ];
    let permutation = [2, 0, 1];
    let permuted_fresh = permutation.map(|i| fresh[i]);
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let permuted_bill = obligations(
        &[],
        &permuted_fresh,
        &permuted_fresh,
        PhloOutcome::Accepted(&[]),
        100,
    );
    let sources = [source(b"B", 5), source(b"A", 5)];
    let edges = [vec![true, true, false, true], vec![true, false, true, true]];
    let permuted_edges: Vec<_> = edges
        .iter()
        .map(|row| {
            std::iter::once(row[0])
                .chain(permutation.iter().map(|i| row[i + 1]))
                .collect()
        })
        .collect();
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    }];
    let permuted_requirements = [PhloFundingRequirement {
        obligations: &permuted_bill,
        eligible: &permuted_edges,
    }];
    let original = select_phlo_funding_family(input(&sources, &requirements, 5), limits(), &work())
        .unwrap()
        .unwrap();
    let permuted = select_phlo_funding_family(
        input(&sources, &permuted_requirements, 5),
        limits(),
        &work(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(original.source_holds(), permuted.source_holds());
    for row in 0..sources.len() {
        assert_eq!(
            original.assignments()[0][row][0],
            permuted.assignments()[0][row][0]
        );
        for (column, original_column) in permutation.iter().copied().enumerate() {
            assert_eq!(
                original.assignments()[0][row][original_column + 1],
                permuted.assignments()[0][row][column + 1]
            );
        }
    }
}

#[test]
fn funding_supports_more_than_two_wallets_without_fixed_width_identity_masks() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot")];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let keys: Vec<_> = (0_u16..129).map(u16::to_be_bytes).collect();
    let sources: Vec<_> = keys.iter().map(|key| source(key, 2)).collect();
    let edges: Vec<_> = (0..129).map(|i| vec![i == 128; 2]).collect();
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    }];
    let selected = select_phlo_funding_family(input(&sources, &requirements, 2), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.source_holds().len(), 129);
    assert_eq!(&selected.source_holds()[..128], &[0; 128]);
    assert_eq!(selected.source_holds()[128], 2);
    assert_eq!(selected.assignments()[0][128], vec![1, 1]);
}

#[test]
fn canonical_outcome_order_binds_permissions_not_only_obligation_keys() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot")];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let sources = [source(b"A", 2), source(b"B", 2), source(b"C", 2)];
    let alpha_edges = [vec![true, false], vec![true, true], vec![true, true]];
    let beta_edges = [vec![true, true], vec![true, true], vec![true, false]];
    let requirements = [
        PhloFundingRequirement {
            obligations: &bill,
            eligible: &beta_edges,
        },
        PhloFundingRequirement {
            obligations: &bill,
            eligible: &alpha_edges,
        },
    ];
    let original = select_phlo_funding_family(input(&sources, &requirements, 3), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(original.canonical_outcome_positions(), &[1, 0]);
    let reversed = [requirements[1], requirements[0]];
    let permuted = select_phlo_funding_family(input(&sources, &reversed, 3), limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(permuted.canonical_outcome_positions(), &[0, 1]);
    assert_eq!(original.source_holds(), permuted.source_holds());
    assert_eq!(original.assignments()[0], permuted.assignments()[1]);
    assert_eq!(original.assignments()[1], permuted.assignments()[0]);
    assert_eq!(
        original.cursor_transitions()[0],
        permuted.cursor_transitions()[1]
    );
    assert_eq!(
        original.cursor_transitions()[1],
        permuted.cursor_transitions()[0]
    );
}

#[test]
fn malformed_later_outcome_is_not_hidden_by_an_infeasible_earlier_outcome() {
    let bill = obligations(&[], &[], &[], PhloOutcome::Accepted(&[]), 100);
    let sources = [source(b"A", 0)];
    let edges = [vec![true]];
    let malformed_edges = [vec![]];
    let requirements = [
        PhloFundingRequirement {
            obligations: &bill,
            eligible: &edges,
        },
        PhloFundingRequirement {
            obligations: &bill,
            eligible: &malformed_edges,
        },
    ];
    assert!(
        select_phlo_funding_family(input(&sources, &requirements, 0), limits(), &work()).is_err()
    );
}

#[test]
fn recomputation_accepts_only_the_canonical_complete_family_and_exact_holds() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot")];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let sources = [source(b"A", 2), source(b"B", 2)];
    let edges = [vec![true; 2], vec![true; 2]];
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    }];
    let funding_input = input(&sources, &requirements, 4);
    let selected = select_phlo_funding_family(funding_input, limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.assignments(), &[vec![vec![1, 1], vec![0, 0]]]);
    assert!(verify_phlo_funding_family(
        funding_input,
        PhloFamilyFundingProposal {
            assignments: selected.assignments(),
            source_holds: selected.source_holds(),
        },
        limits(),
        &work()
    )
    .unwrap());
    assert!(!verify_phlo_funding_family(
        funding_input,
        PhloFamilyFundingProposal {
            assignments: selected.assignments(),
            source_holds: &[2, 1],
        },
        limits(),
        &work()
    )
    .unwrap());
    let alternative = [vec![vec![0, 0], vec![1, 1]]];
    let cases = [PhloFundingCase {
        obligations: &bill,
        eligible: &edges,
        assignment: &alternative[0],
    }];
    let checked = check_phlo_funding_family(&sources, &cases, 4, limits().funding).unwrap();
    assert_eq!(checked.source_holds(), &[0, 2]);
    assert!(!verify_phlo_funding_family(
        funding_input,
        PhloFamilyFundingProposal {
            assignments: &alternative,
            source_holds: checked.source_holds(),
        },
        limits(),
        &work()
    )
    .unwrap());
}

#[test]
fn exhausted_host_work_and_key_storage_return_errors_not_infeasibility() {
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority, b"slot")];
    let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
    let sources = [source(b"A", 2)];
    let edges = [vec![true; 2]];
    let requirements = [PhloFundingRequirement {
        obligations: &bill,
        eligible: &edges,
    }];
    let zero_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(
        select_phlo_funding_family(input(&sources, &requirements, 2), limits(), &zero_work)
            .is_err()
    );
    assert!(select_phlo_funding_family(
        input(&sources, &requirements, 2),
        PhloFamilyFundingLimits {
            aggregate_key_bytes: 0,
            ..limits()
        },
        &work()
    )
    .is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_source_and_outcome_permutations_preserve_named_funding(
        capacities in prop::collection::vec(0_u64..=4, 1..=6),
        fresh_count in 0_usize..=3,
        source_order in prop::collection::vec(any::<u32>(), 6),
        reverse_outcomes in any::<bool>(),
        resource_seed in any::<usize>(), fee_seed in any::<usize>(),
    ) {
        let authority = Sig::Ground(vec![1]);
        let fresh = vec![resource(&authority, b"slot"); fresh_count];
        let bill = obligations(&[], &fresh, &fresh, PhloOutcome::Accepted(&[]), 100);
        let rejected = obligations(&[], &fresh, &fresh, PhloOutcome::AdmissionRejected, 100);
        let counted_fresh: Vec<_> = if fresh_count == 0 { vec![] } else {
            vec![PhloResourceAmount { resource: fresh[0], quantity: fresh_count as u64 }]
        };
        let counted_execution = check_counted_phlo_execution(
            bill.execution().controls(),
            CountedPhloExecutionWitness {
                available: &[], required: &counted_fresh, used: &[], unused: &[], fresh: &counted_fresh,
            },
            PhloExecutionLimits { resource_entries: 100_000, authority_nodes: 1_000_000, key_bytes: 16_000_000 },
        ).unwrap();
        let counted_bill = project_phlo_obligations(counted_execution, PhloOutcome::Accepted(&[]), cap(100)).unwrap();
        let counted_rejected = project_phlo_obligations(counted_execution, PhloOutcome::AdmissionRejected, cap(100)).unwrap();
        let keys: Vec<Vec<u8>> = (0..capacities.len()).map(|i| vec![i as u8 + 1]).collect();
        let sources: Vec<_> = keys.iter().zip(&capacities).map(|(key, capacity)| source(key, *capacity)).collect();
        let edges = vec![vec![true; bill.keys().len()]; sources.len()];
        let requirements = [PhloFundingRequirement { obligations: &bill, eligible: &edges },
            PhloFundingRequirement { obligations: &rejected, eligible: &edges }];
        let exposure = capacities.iter().map(|v| u128::from(*v)).sum();
        let original_input = PhloFamilyFundingInput {
            canonical_resource_cursor: resource_seed % sources.len(),
            canonical_fee_cursor: fee_seed % sources.len(),
            ..input(&sources, &requirements, exposure)
        };
        let original = select_phlo_funding_family(original_input, limits(), &work()).unwrap();
        let mut order: Vec<_> = (0..sources.len()).collect();
        order.sort_by_key(|i| (source_order[*i], *i));
        let permuted_sources: Vec<_> = order.iter().map(|i| sources[*i]).collect();
        let permuted_edges: Vec<_> = order.iter().map(|i| edges[*i].clone()).collect();
        let mut permuted_requirements = [PhloFundingRequirement { obligations: &counted_bill, eligible: &permuted_edges },
            PhloFundingRequirement { obligations: &counted_rejected, eligible: &permuted_edges }];
        if reverse_outcomes { permuted_requirements.reverse(); }
        let permuted = select_phlo_funding_family(PhloFamilyFundingInput {
            sources: &permuted_sources, outcomes: &permuted_requirements, ..original_input
        }, limits(), &work()).unwrap();
        prop_assert_eq!(original.is_some(), permuted.is_some());
        if let (Some(original), Some(permuted)) = (original, permuted) {
            prop_assert_eq!(original.total_held(), permuted.total_held());
            for original_case in 0..2 {
                let permuted_case = if reverse_outcomes { 1 - original_case } else { original_case };
                prop_assert_eq!(&original.cursor_transitions()[original_case], &permuted.cursor_transitions()[permuted_case]);
            }
            for (position, original_source) in order.iter().copied().enumerate() {
                prop_assert_eq!(original.source_holds()[original_source], permuted.source_holds()[position]);
                for original_case in 0..2 {
                    let permuted_case = if reverse_outcomes { 1 - original_case } else { original_case };
                    prop_assert_eq!(&original.assignments()[original_case][original_source],
                        &permuted.assignments()[permuted_case][position]);
                }
            }
            let cases: Vec<_> = requirements.iter().zip(original.assignments()).map(|(requirement, assignment)|
                PhloFundingCase { obligations: requirement.obligations, eligible: requirement.eligible, assignment }).collect();
            prop_assert!(check_phlo_funding_family(&sources, &cases, exposure, limits().funding).is_ok());
        }
    }
}
