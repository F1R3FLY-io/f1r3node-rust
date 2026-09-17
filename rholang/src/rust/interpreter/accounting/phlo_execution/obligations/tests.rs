use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_phlo_execution, PhloExecutionLimits, PhloExecutionWitness, PhloFailure,
};
use crate::rust::interpreter::accounting::Sig;

const ENVIRONMENT: PhloEnvironment<'static> = PhloEnvironment {
    decimal_scale: 8,
    protocol_version: 6,
    network: b"test network",
    shard: b"test shard",
    asset: b"native asset",
    unit: b"native phlo",
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENVIRONMENT,
    weights: &[1, 3, 0],
    actual_price: 2,
}];

fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        location: b"funding slot",
        class: 0,
        acquisition_terms: b"terms",
        authority,
    }
}

fn execution<'a>(
    schedules: &'a [PhloSchedule<'a>],
    prepaid: &'a [PhloResource<'a>],
    required: &'a [PhloResource<'a>],
    fresh: &'a [PhloResource<'a>],
) -> CheckedPhloExecution<'a> {
    let checked = check_phlo_controls(
        ENVIRONMENT,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 100_000,
            price_ceiling: schedules[0].actual_price,
            required_owner_ceilings: &[u64::MAX],
            permitted_schedules: schedules,
        },
        schedules[0],
        100_000,
    )
    .unwrap();
    check_phlo_execution(
        checked,
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
    .unwrap()
}

#[test]
fn projection_groups_identical_fresh_resources_without_rebilling_prepaid_resources() {
    let authority = Sig::Ground(vec![1]);
    let required = vec![resource(&authority); 5];
    let checked = execution(&SCHEDULES, &required[..2], &required, &required[2..]);
    let obligations =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(4)).unwrap();
    assert_eq!(obligations.execution(), checked);
    assert_eq!(obligations.outcome(), PhloOutcome::Accepted(&[]));
    assert_eq!(obligations.keys()[0], PhloObligationKey::Fee);
    assert_eq!(obligations.keys()[1..], vec![
        PhloObligationKey::Resource(
            resource(&authority)
        );
        1
    ]);
    assert_eq!(obligations.amounts(), &[1, 6]);
    assert_eq!(obligations.total(), 7);
    assert_eq!(
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(1)),
        Err(PhloObligationError::TooManyObligations)
    );
}

fn funding_work() -> HostWorkBudget {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn selection_limits() -> PhloObligationFundingLimits {
    use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;
    PhloObligationFundingLimits {
        search: FundingSearchLimits {
            source_cap: cap(129),
            obligation_cap: cap(100),
        },
        keys: wire_limits(),
        aggregate_key_bytes: 1_000_000,
    }
}

#[test]
fn checked_obligations_use_joint_fee_policy_and_feed_native_family_settlement() {
    use crate::rust::interpreter::accounting::phlo_execution::{
        check_phlo_funding_family, PhloFundingCase, PhloFundingLimits, PhloFundingSource,
    };
    let authority = Sig::Ground(vec![1]);
    let fresh = [resource(&authority)];
    let obligations = project_phlo_obligations(
        execution(&SCHEDULES, &[], &fresh, &fresh),
        PhloOutcome::Accepted(&[]),
        cap(2),
    )
    .unwrap();
    let edges = [vec![false, true], vec![true, true]];
    let input = PhloObligationFundingInput {
        source_keys: &[b"B", b"A"],
        capacities: &[2, 1],
        eligible: &edges,
        canonical_resource_cursor: 0,
        canonical_fee_cursor: 1,
    };
    let selected = obligations
        .select_funding(input, selection_limits(), &funding_work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.assignment(), &[vec![0, 2], vec![1, 0]]);
    assert_eq!(selected.resource_next_cursor(), Some(1));
    assert_eq!(selected.fee_next_cursor(), Some(1));
    let proposal = PhloObligationFundingProposal {
        assignment: selected.assignment(),
        resource_next_cursor: selected.resource_next_cursor(),
        fee_next_cursor: selected.fee_next_cursor(),
    };
    assert!(obligations
        .verify_funding(input, proposal, selection_limits(), &funding_work())
        .unwrap());
    assert!(!obligations
        .verify_funding(
            input,
            PhloObligationFundingProposal {
                resource_next_cursor: None,
                ..proposal
            },
            selection_limits(),
            &funding_work()
        )
        .unwrap());
    assert!(!obligations
        .verify_funding(
            input,
            PhloObligationFundingProposal {
                fee_next_cursor: None,
                ..proposal
            },
            selection_limits(),
            &funding_work()
        )
        .unwrap());
    assert!(!obligations
        .verify_funding(
            input,
            PhloObligationFundingProposal {
                assignment: &[vec![1, 1], vec![0, 1]],
                ..proposal
            },
            selection_limits(),
            &funding_work()
        )
        .unwrap());
    let sources = [
        PhloFundingSource {
            custody: b"B",
            capacity: 2,
            exposure_limit: 2,
            debit_limit: 2,
        },
        PhloFundingSource {
            custody: b"A",
            capacity: 1,
            exposure_limit: 1,
            debit_limit: 1,
        },
    ];
    let cases = [PhloFundingCase {
        obligations: &obligations,
        eligible: &edges,
        assignment: selected.assignment(),
    }];
    let family = check_phlo_funding_family(&sources, &cases, 3, PhloFundingLimits {
        sources: cap(2),
        cases: cap(1),
        obligations: cap(2),
        assignment_cells: 4,
        custody_bytes: 2,
    })
    .unwrap();
    let native = family.native_amounts(0).unwrap();
    assert_eq!(
        (
            native[0].custody(),
            native[0].acquisition(),
            native[0].fee(),
            native[0].refund()
        ),
        (&b"B"[..], 2, 0, 0)
    );
    assert_eq!(
        (
            native[1].custody(),
            native[1].acquisition(),
            native[1].fee(),
            native[1].refund()
        ),
        (&b"A"[..], 0, 1, 0)
    );
    assert!(obligations
        .select_funding(
            PhloObligationFundingInput {
                capacities: &[1, 1],
                ..input
            },
            selection_limits(),
            &funding_work()
        )
        .unwrap()
        .is_none());
    assert!(obligations
        .select_funding(
            PhloObligationFundingInput {
                source_keys: &[b"A", b"A"],
                ..input
            },
            selection_limits(),
            &funding_work()
        )
        .is_err());
}

#[test]
fn uncharged_outcomes_preserve_both_cursors_and_prepaid_success_charges_only_fee() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority)];
    let edges = [vec![false, false], vec![false, false]];
    let input = PhloObligationFundingInput {
        source_keys: &[b"B", b"A"],
        capacities: &[0, 0],
        eligible: &edges,
        canonical_resource_cursor: 1,
        canonical_fee_cursor: 0,
    };
    for outcome in [
        PhloOutcome::AdmissionRejected,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
    ] {
        let obligations = project_phlo_obligations(
            execution(&SCHEDULES, &[], &resources, &resources),
            outcome,
            cap(2),
        )
        .unwrap();
        let selected = obligations
            .select_funding(input, selection_limits(), &funding_work())
            .unwrap()
            .unwrap();
        assert_eq!(selected.assignment(), &[vec![0, 0], vec![0, 0]]);
        assert_eq!(selected.resource_next_cursor(), None);
        assert_eq!(selected.fee_next_cursor(), None);
    }
    let obligations = project_phlo_obligations(
        execution(&SCHEDULES, &resources, &resources, &[]),
        PhloOutcome::Accepted(&[]),
        cap(1),
    )
    .unwrap();
    let selected = obligations
        .select_funding(
            PhloObligationFundingInput {
                capacities: &[1, 1],
                eligible: &[vec![true], vec![true]],
                ..input
            },
            selection_limits(),
            &funding_work(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(selected.assignment(), &[vec![0], vec![1]]);
    assert_eq!(selected.resource_next_cursor(), None);
    assert_eq!(selected.fee_next_cursor(), Some(1));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_obligation_policy_preserves_named_sources_and_exact_charges(
        capacities in prop::collection::vec(0_u64..=8, 1..=8),
        fresh_count in 0_usize..=3,
        resource_seed in any::<usize>(), fee_seed in any::<usize>(),
        billable in any::<bool>(),
    ) {
        let authority = Sig::Ground(vec![1]);
        let fresh = vec![resource(&authority); fresh_count];
        let outcome = if billable { PhloOutcome::Accepted(&[])} else { PhloOutcome::AdmissionRejected };
        let obligations = project_phlo_obligations(execution(&SCHEDULES, &[], &fresh, &fresh),
            outcome, cap(fresh_count + 1)).unwrap();
        let keys: Vec<Vec<u8>> = (0..capacities.len()).map(|i| vec![i as u8 + 1]).collect();
        let source_keys: Vec<_> = keys.iter().map(Vec::as_slice).collect();
        let edges = vec![vec![true; obligations.keys().len()]; capacities.len()];
        let input = PhloObligationFundingInput {
            source_keys: &source_keys, capacities: &capacities, eligible: &edges,
            canonical_resource_cursor: resource_seed % capacities.len(),
            canonical_fee_cursor: fee_seed % capacities.len(),
        };
        let selected = obligations.select_funding(input, selection_limits(), &funding_work()).unwrap();
        prop_assert_eq!(selected.is_some(), capacities.iter().sum::<u64>() >= obligations.total());
        let reversed_keys: Vec<_> = source_keys.iter().copied().rev().collect();
        let reversed_capacities: Vec<_> = capacities.iter().copied().rev().collect();
        let reversed = obligations.select_funding(PhloObligationFundingInput {
            source_keys: &reversed_keys, capacities: &reversed_capacities, ..input
        }, selection_limits(), &funding_work()).unwrap();
        match (selected, reversed) {
            (Some(selected), Some(reversed)) => {
                prop_assert_eq!(selected.totals().total(), obligations.total());
                prop_assert_eq!(selected.assignment().iter().map(|row| row[0]).sum::<u64>(), u64::from(billable));
                prop_assert_eq!(selected.resource_next_cursor(), reversed.resource_next_cursor());
                prop_assert_eq!(selected.fee_next_cursor(), reversed.fee_next_cursor());
                prop_assert!(selected.assignment().iter().eq(reversed.assignment().iter().rev()));
            }
            (None, None) => (),
            _ => prop_assert!(false, "source order changed feasibility"),
        }
    }
}

fn wire_limits() -> PhloObligationKeyLimits {
    PhloObligationKeyLimits {
        wire: models::rust::phlo_wire::PhloWireLimits {
            total_bytes: 16_384,
            field_bytes: 8192,
        },
        authority_nodes: 100,
    }
}

#[test]
fn phlo_obligation_keys_bind_checked_resources_to_canonical_graphs() {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};

    use crate::rust::interpreter::accounting::monetary_allocation::{
        canonicalize_funding_problem, FundingMinimaxProblem, FundingSearchLimits,
    };
    use crate::rust::interpreter::host_work::HostWorkBudget;
    let authority = Sig::Ground(vec![1]);
    let resources = [
        resource(&authority),
        PhloResource {
            location: b"other",
            ..resource(&authority)
        },
        resource(&authority),
    ];
    let checked = execution(&SCHEDULES, &[], &resources, &resources);
    let obligations =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(4)).unwrap();
    let keys = obligations.encoded_keys(wire_limits(), 65_536).unwrap();
    assert_eq!(keys.len(), obligations.keys().len());
    assert_eq!(
        PhloObligationKeyV1::decode(&keys[0], wire_limits()).unwrap(),
        PhloObligationKeyV1::Fee
    );
    for (index, resource) in resources[..2].iter().enumerate() {
        let PhloObligationKeyV1::Resource(decoded) =
            PhloObligationKeyV1::decode(&keys[index + 1], wire_limits()).unwrap()
        else {
            panic!("resource and fee kinds differ");
        };
        assert_eq!(decoded, resource.wire_key(checked.limits).unwrap());
    }
    assert_eq!(obligations.amounts(), &[1, 4, 2]);
    assert_ne!(keys[1], keys[2]);
    let references: Vec<_> = keys.iter().map(Vec::as_slice).collect();
    let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)));
    let canonical = canonicalize_funding_problem(
        FundingMinimaxProblem {
            capacities: &[4, 4],
            obligations: obligations.amounts(),
            eligible: &[vec![true; 3], vec![true; 3]],
        },
        &[b"wallet:b", b"wallet:a"],
        &references,
        FundingSearchLimits {
            source_cap: cap(2),
            obligation_cap: cap(4),
        },
        &budget,
    )
    .unwrap();
    assert_eq!(canonical.obligation_keys()[0], keys[0]);
    assert_eq!(canonical.obligation_keys().len(), 3);
    assert_eq!(
        canonical.problem().obligations.iter().sum::<u64>(),
        obligations.total()
    );
}

#[test]
fn phlo_obligation_key_encoding_enforces_aggregate_limits_even_when_unbilled() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority); 3];
    let checked = execution(&SCHEDULES, &[], &resources, &resources);
    for outcome in [PhloOutcome::Accepted(&[]), PhloOutcome::AdmissionRejected] {
        let obligations = project_phlo_obligations(checked, outcome, cap(4)).unwrap();
        let expected = obligations.encoded_keys(wire_limits(), 65_536).unwrap();
        let total = expected.iter().map(Vec::len).sum();
        assert_eq!(
            obligations.encoded_keys(wire_limits(), total).unwrap(),
            expected
        );
        assert!(obligations.encoded_keys(wire_limits(), total - 1).is_err());
        let mut nodes = wire_limits();
        nodes.authority_nodes = 0;
        assert_eq!(
            obligations.encoded_keys(nodes, 65_536),
            Err(PhloObligationError::Execution(
                PhloExecutionError::TooManyAuthorityNodes
            ))
        );
        nodes.authority_nodes = 1;
        assert_eq!(obligations.encoded_keys(nodes, 65_536).unwrap(), expected);
    }
}

#[test]
fn metered_phlo_keys_charge_actual_structure_not_configured_byte_ceilings() {
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    let authority = Sig::And(
        Box::new(Sig::Ground(vec![1])),
        Box::new(Sig::Quote(vec![2])),
    );
    let resources = [resource(&authority); 2];
    let checked = execution(&SCHEDULES, &[], &resources, &resources);
    let obligations =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(3)).unwrap();
    let expected = obligations.encoded_keys(wire_limits(), 65_536).unwrap();
    let actual_bytes = expected.iter().map(Vec::len).sum::<usize>();
    let generous = || HostWorkLimits::uniform(HostWorkLimit::new(1_000_000));
    let baseline = HostWorkBudget::new(generous());
    assert_eq!(
        obligations
            .encoded_keys_with_budget(wire_limits(), actual_bytes, &baseline)
            .unwrap(),
        expected
    );
    let enlarged = HostWorkBudget::new(generous());
    assert_eq!(
        obligations
            .encoded_keys_with_budget(wire_limits(), usize::MAX, &enlarged)
            .unwrap(),
        expected
    );
    assert_eq!(baseline.usages(), enlarged.usages());
    assert!(baseline.usage(HostWorkDimension::SearchStateBytes).get() > actual_bytes as u64);
    for dimension in [
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let measured = baseline.usage(dimension).get();
        for allowed in 0..=measured {
            let mut limits = generous();
            limits.set(dimension, HostWorkLimit::new(allowed));
            let budget = HostWorkBudget::new(limits);
            let result = obligations.encoded_keys_with_budget(wire_limits(), actual_bytes, &budget);
            if allowed == measured {
                assert_eq!(result.unwrap(), expected);
            } else {
                assert!(matches!(
                    result,
                    Err(PhloObligationError::Execution(
                        PhloExecutionError::HostWork(_)
                    ))
                ));
                assert!(budget.is_rejected());
            }
            assert!(budget.usage(dimension).get() <= allowed);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn metered_phlo_keys_preserve_encoding_for_generated_compound_occurrences(
        owners in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..16), 1..24),
        count in 0usize..8, unbilled in any::<bool>(),
    ) {
        use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
        let mut authority = Sig::Ground(owners[0].clone());
        for owner in &owners[1..] {
            authority = Sig::And(Box::new(authority), Box::new(Sig::Quote(owner.clone())));
        }
        let resources = vec![resource(&authority); count];
        let checked = execution(&SCHEDULES, &[], &resources, &resources);
        let outcome = if unbilled { PhloOutcome::AdmissionRejected } else { PhloOutcome::Accepted(&[]) };
        let obligations = project_phlo_obligations(checked, outcome, cap(count + 1)).unwrap();
        let limits = PhloObligationKeyLimits { authority_nodes: 1024, ..wire_limits() };
        let expected = obligations.encoded_keys(limits, 1_048_576).unwrap();
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)));
        let encoded = obligations.encoded_keys_with_budget(limits, 1_048_576, &budget).unwrap();
        prop_assert_eq!(encoded, expected);
    }
}

#[test]
fn zero_value_occurrences_and_distinct_resource_keys_are_not_dropped() {
    let unit = Sig::Unit;
    let authority = Sig::Ground(vec![1]);
    let required = [
        resource(&unit),
        PhloResource {
            class: 2,
            ..resource(&authority)
        },
        resource(&authority),
        PhloResource {
            location: b"other location",
            ..resource(&authority)
        },
        PhloResource {
            acquisition_terms: b"other terms",
            ..resource(&authority)
        },
    ];
    let checked = execution(&SCHEDULES, &[], &required, &required);
    let obligations =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(6)).unwrap();
    assert_eq!(obligations.amounts(), &[1, 0, 0, 2, 2, 2]);
    for (index, resource) in required.iter().enumerate() {
        assert_eq!(
            obligations.keys()[index + 1],
            PhloObligationKey::Resource(*resource)
        );
    }
    let zero_price = [PhloSchedule {
        actual_price: 0,
        ..SCHEDULES[0]
    }];
    let free = execution(&zero_price, &[], &required, &required);
    let projected = project_phlo_obligations(free, PhloOutcome::Accepted(&[]), cap(6)).unwrap();
    assert_eq!(projected.amounts(), &[1, 0, 0, 0, 0, 0]);
    assert_eq!(projected.keys(), obligations.keys());
}

#[test]
fn every_nonbillable_outcome_keeps_keys_and_clears_all_demands() {
    let authority = Sig::Ground(vec![1]);
    let required = [resource(&authority)];
    let checked = execution(&SCHEDULES, &[], &required, &required);
    for outcome in [
        PhloOutcome::AdmissionRejected,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
        PhloOutcome::Accepted(&[PhloFailure::User, PhloFailure::Certificate]),
        PhloOutcome::Accepted(&[PhloFailure::Unclassified, PhloFailure::User]),
    ] {
        let projected = project_phlo_obligations(checked, outcome, cap(2)).unwrap();
        assert_eq!(projected.keys(), &[
            PhloObligationKey::Fee,
            PhloObligationKey::Resource(required[0])
        ]);
        assert_eq!(projected.amounts(), &[0, 0]);
        assert_eq!(projected.total(), 0);
        assert_eq!(projected.outcome(), outcome);
        let funded = projected
            .check_assignment(&[0], &[vec![false; 2]], &[vec![0; 2]], cap(1), cap(2))
            .unwrap();
        assert_eq!(funded.total(), 0);
    }
    let accepted =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[PhloFailure::User]), cap(2))
            .unwrap();
    assert_eq!(accepted.amounts(), &[1, 2]);
}

#[test]
fn equal_aggregate_cost_cannot_move_resource_charges_into_the_fee() {
    let authority = Sig::Ground(vec![1]);
    let required = [resource(&authority)];
    let projected = project_phlo_obligations(
        execution(&SCHEDULES, &[], &required, &required),
        PhloOutcome::Accepted(&[]),
        cap(2),
    )
    .unwrap();
    for wrong in [vec![3, 0], vec![0, 3], vec![2, 1]] {
        assert_eq!(
            projected.check_assignment(&[3], &[vec![true; 2]], &[wrong], cap(1), cap(2)),
            Err(FundingAssignmentError::ObligationMismatch)
        );
    }
    assert_eq!(
        projected.check_assignment(&[3], &[vec![true, false]], &[vec![1, 2]], cap(1), cap(2)),
        Err(FundingAssignmentError::IneligibleDraw)
    );
    assert_eq!(
        projected.check_assignment(&[3], &[vec![false, true]], &[vec![1, 2]], cap(1), cap(2)),
        Err(FundingAssignmentError::IneligibleDraw)
    );
    assert_eq!(
        projected.check_assignment(&[3], &[vec![true]], &[vec![3]], cap(1), cap(2)),
        Err(FundingAssignmentError::InvalidDimensions)
    );
}

#[test]
fn arbitrary_source_counts_can_fund_distinct_typed_obligations() {
    for count in [1, 2, 3, 17, 64, 129] {
        let authorities: Vec<_> = (0..count)
            .map(|index: usize| Sig::Ground(index.to_le_bytes().to_vec()))
            .collect();
        let resources: Vec<_> = authorities.iter().map(resource).collect();
        let projected = project_phlo_obligations(
            execution(&SCHEDULES, &[], &resources, &resources),
            PhloOutcome::Accepted(&[]),
            cap(count + 1),
        )
        .unwrap();
        let mut capacities = vec![2; count];
        capacities[0] += 1;
        let mut eligible = vec![vec![false; count + 1]; count];
        let mut assignment = vec![vec![0; count + 1]; count];
        eligible[0][0] = true;
        assignment[0][0] = 1;
        for source in 0..count {
            eligible[source][source + 1] = true;
            assignment[source][source + 1] = 2;
        }
        let funded = projected
            .check_assignment(
                &capacities,
                &eligible,
                &assignment,
                cap(count),
                cap(count + 1),
            )
            .unwrap();
        assert_eq!(funded.source_debits(), capacities);
        assert_eq!(funded.total(), 2 * count as u64 + 1);
    }
}

#[test]
fn empty_execution_and_fully_prepaid_execution_keep_the_separate_fee() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority)];
    for checked in [
        execution(&SCHEDULES, &[], &[], &[]),
        execution(&SCHEDULES, &resources, &resources, &[]),
    ] {
        let projected =
            project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(1)).unwrap();
        assert_eq!(projected.keys(), &[PhloObligationKey::Fee]);
        assert_eq!(projected.amounts(), &[1]);
        assert_eq!(projected.total(), 1);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_projection_preserves_typed_occurrences_values_and_charge(
        entries in prop::collection::vec((0_usize..3, 0_usize..6, 0_usize..6), 1..65),
        raw_failures in prop::collection::vec(0_u8..4, 0..16),
        admitted in any::<bool>(),
    ) {
        let authorities: Vec<_> = (0..entries.len()).map(|index| Sig::Ground(index.to_le_bytes().to_vec())).collect();
        let mut prepaid = Vec::new();
        let mut required = Vec::new();
        let mut fresh = Vec::new();
        let mut expected_fresh_phlo = 0_u64;
        for ((class, paid_count, new_count), authority) in entries.iter().zip(&authorities) {
            let item = PhloResource { class: *class, ..resource(authority) };
            prepaid.extend(std::iter::repeat_n(item, *paid_count));
            required.extend(std::iter::repeat_n(item, paid_count + new_count));
            fresh.extend(std::iter::repeat_n(item, *new_count));
            expected_fresh_phlo += SCHEDULES[0].weights[*class] * *new_count as u64;
        }
        let failures: Vec<_> = raw_failures.iter().map(|tag| match tag {
            0 => PhloFailure::User, 1 => PhloFailure::Platform, 2 => PhloFailure::Certificate, _ => PhloFailure::Unclassified,
        }).collect();
        let outcome = if admitted { PhloOutcome::Accepted(&failures) } else { PhloOutcome::AdmissionRejected };
        let billable = admitted && raw_failures.iter().all(|tag| *tag == 0);
        let projected = project_phlo_obligations(execution(&SCHEDULES, &prepaid, &required, &fresh), outcome, cap(fresh.len() + 1)).unwrap();
        let distinct: Vec<_> = entries.iter().zip(&authorities)
            .filter(|((_, _, count), _)| *count > 0).collect();
        prop_assert_eq!(projected.keys().len(), distinct.len() + 1);
        prop_assert_eq!(projected.amounts().len(), distinct.len() + 1);
        prop_assert_eq!(projected.keys()[0], PhloObligationKey::Fee);
        prop_assert_eq!(projected.amounts()[0], u64::from(billable));
        for (index, ((class, _, quantity), authority)) in distinct.into_iter().enumerate() {
            let item = PhloResource { class: *class, ..resource(authority) };
            prop_assert_eq!(projected.keys()[index + 1], PhloObligationKey::Resource(item));
            prop_assert_eq!(projected.amounts()[index + 1], if billable { 2 * SCHEDULES[0].weights[*class] * *quantity as u64 } else { 0 });
        }
        prop_assert_eq!(projected.total(), if billable { 1 + 2 * expected_fresh_phlo } else { 0 });
        prop_assert_eq!(projected.total(), projected.amounts().iter().sum::<u64>());
        prop_assert_eq!(projected.total(), projected.execution().retained_charge(outcome));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn counted_obligations_preserve_expanded_wallet_debits_and_cursors(
        capacities in prop::collection::vec(0_u64..12, 1..9),
        quantities in prop::array::uniform3(0_usize..5),
        permissions in prop::collection::vec(prop::array::uniform4(any::<bool>()), 8),
        resource_cursor in any::<usize>(), fee_cursor in any::<usize>(),
    ) {
        use crate::rust::interpreter::accounting::monetary_allocation::{
            canonicalize_funding_problem, FundingMinimaxProblem,
        };
        use crate::rust::interpreter::accounting::phlo_execution::{
            check_counted_phlo_execution, CountedPhloExecutionWitness, PhloResourceAmount,
        };
        let authorities: Vec<_> = (0..3).map(|i| Sig::Ground(vec![i])).collect();
        let pool: Vec<_> = authorities.iter().map(resource).collect();
        let compact: Vec<_> = pool.iter().zip(quantities).filter(|(_, n)| *n != 0)
            .map(|(resource, quantity)| PhloResourceAmount { resource: *resource, quantity: quantity as u64 }).collect();
        let expanded: Vec<_> = pool.iter().zip(quantities).flat_map(|(r, n)| std::iter::repeat_n(*r, n)).collect();
        let reference = execution(&SCHEDULES, &[], &expanded, &expanded);
        let compact_checked = check_counted_phlo_execution(reference.controls(), CountedPhloExecutionWitness {
            available: &[], required: &compact, used: &[], unused: &[], fresh: &compact,
        }, reference.limits).unwrap();
        let bill = project_phlo_obligations(compact_checked, PhloOutcome::Accepted(&[]), cap(4)).unwrap();
        let source_ids: Vec<_> = (0..capacities.len()).map(|i| vec![i as u8 + 1]).collect();
        let source_keys: Vec<_> = source_ids.iter().map(Vec::as_slice).collect();
        let compact_edges: Vec<_> = permissions[..capacities.len()].iter().map(|bits| {
            std::iter::once(bits[0]).chain((0..3).filter(|i| quantities[*i] > 0).map(|i| bits[i + 1])).collect()
        }).collect();
        let new = bill.select_funding(PhloObligationFundingInput {
            source_keys: &source_keys, capacities: &capacities, eligible: &compact_edges,
            canonical_resource_cursor: resource_cursor % capacities.len(),
            canonical_fee_cursor: fee_cursor % capacities.len(),
        }, selection_limits(), &funding_work()).unwrap();
        let encoded: Vec<_> = expanded.iter().map(|r| PhloObligationKeyV1::Resource(
            r.wire_key(reference.limits).unwrap()).encode(wire_limits()).unwrap()).collect();
        let keys: Vec<_> = encoded.iter().map(Vec::as_slice).collect();
        let amounts = vec![2; expanded.len()];
        let edges: Vec<Vec<bool>> = permissions[..capacities.len()].iter().map(|bits|
            (0..3).flat_map(|i| std::iter::repeat_n(bits[i + 1], quantities[i])).collect()).collect();
        let fee_edges: Vec<_> = permissions[..capacities.len()].iter().map(|bits| bits[0]).collect();
        let old = canonicalize_funding_problem(FundingMinimaxProblem {
            capacities: &capacities, obligations: &amounts, eligible: &edges,
        }, &source_keys, &keys, selection_limits().search, &funding_work()).unwrap();
        let selected = old.select_with_unit_fee(&fee_edges, resource_cursor % capacities.len(),
            fee_cursor % capacities.len(), selection_limits().search, &funding_work()).unwrap();
        prop_assert_eq!(new.is_some(), selected.is_some());
        if let (Some(new), Some(old)) = (new, selected) {
            prop_assert_eq!(new.resource_next_cursor(), old.resource_next_cursor());
            prop_assert_eq!(new.fee_next_cursor(), Some(old.fee().next_cursor));
            for source in 0..capacities.len() {
                prop_assert_eq!(new.assignment()[source][0], old.fee().debits[source]);
                prop_assert_eq!(new.assignment()[source][1..].iter().sum::<u64>(),
                    old.resource_assignment()[source].iter().sum::<u64>());
            }
        }
    }
}
