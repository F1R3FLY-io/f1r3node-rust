use std::collections::BTreeMap;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_wire::PhloWireLimits;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, PhloOutcome, PhloResource,
};
use crate::rust::interpreter::accounting::Sig;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits() -> PhloDischargeLimits {
    PhloDischargeLimits {
        execution: PhloExecutionLimits {
            resource_entries: 512,
            authority_nodes: 65_536,
            key_bytes: 1_048_576,
        },
        key: PhloObligationKeyLimits {
            wire: PhloWireLimits {
                total_bytes: 16_384,
                field_bytes: 8192,
            },
            authority_nodes: 65_536,
        },
        aggregate_key_bytes: 1_048_576,
    }
}

fn resource(index: u8, authorities: &[Sig; 2]) -> PhloResource<'_> {
    PhloResource {
        location: if index & 1 == 0 { b"left" } else { b"right" },
        class: usize::from((index >> 1) & 1),
        acquisition_terms: if index & 4 == 0 {
            b"original"
        } else {
            b"changed"
        },
        authority: &authorities[usize::from((index >> 3) & 1)],
    }
}

fn authorities() -> [Sig; 2] {
    [
        Sig::Ground(vec![1]),
        Sig::And(
            Box::new(Sig::Ground(vec![1])),
            Box::new(Sig::Ground(vec![2])),
        ),
    ]
}

fn rows<'a>(input: &[(u8, u64)], authorities: &'a [Sig; 2]) -> Vec<PhloResourceAmount<'a>> {
    input
        .iter()
        .map(|(index, quantity)| PhloResourceAmount {
            resource: resource(*index, authorities),
            quantity: *quantity,
        })
        .collect()
}

fn counts(input: &[PhloResourceAmount<'_>], authorities: &[Sig; 2]) -> BTreeMap<u8, u64> {
    let mut result = BTreeMap::new();
    for row in input {
        assert!(row.quantity > 0);
        let index = (0..16)
            .find(|index| resource(*index, authorities) == row.resource)
            .unwrap();
        *result.entry(index).or_default() += row.quantity;
    }
    result
}

fn assert_checked(prepared: &PreparedPhloDischarge<'_>) {
    let environment = PhloEnvironment {
        decimal_scale: 8,
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        asset: b"REV",
        unit: b"phlo",
    };
    let schedules = [PhloSchedule {
        commitment: [1; 32],
        environment,
        weights: &[2, 0],
        actual_price: 1,
    }];
    let controls = check_phlo_controls(
        environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 100_000,
            price_ceiling: 1,
            required_owner_ceilings: &[1],
            permitted_schedules: &schedules,
        },
        schedules[0],
        100_000,
    )
    .unwrap();
    let checked =
        check_counted_phlo_execution(controls, prepared.witness(), limits().execution).unwrap();
    assert_eq!(
        checked.usage(),
        checked.prepaid_usage() + checked.fresh_usage()
    );
    assert_eq!(
        checked.retained_charge(PhloOutcome::Accepted(&[])),
        checked.fresh_usage() + 1
    );
}

#[test]
fn partial_prepaid_supply_derives_the_unique_partition() {
    let authorities = authorities();
    let available = rows(&[(0, 2), (1, 7)], &authorities);
    let required = rows(&[(0, 5), (2, 3)], &authorities);
    let prepared =
        prepare_counted_phlo_discharge(&available, &required, limits(), &work()).unwrap();
    let witness = prepared.witness();
    assert_eq!(counts(witness.used, &authorities), BTreeMap::from([(0, 2)]));
    assert_eq!(
        counts(witness.unused, &authorities),
        BTreeMap::from([(1, 7)])
    );
    assert_eq!(
        counts(witness.fresh, &authorities),
        BTreeMap::from([(0, 3), (2, 3)])
    );
    assert_checked(&prepared);
}

#[test]
fn every_complete_key_dimension_prevents_incompatible_credit() {
    let authorities = authorities();
    let available = rows(&[(0, 5)], &authorities);
    for changed in [1, 2, 4, 8] {
        let required = rows(&[(changed, 5)], &authorities);
        let prepared =
            prepare_counted_phlo_discharge(&available, &required, limits(), &work()).unwrap();
        let witness = prepared.witness();
        assert!(witness.used.is_empty());
        assert_eq!(witness.unused, available);
        assert_eq!(witness.fresh, required);
        assert_checked(&prepared);
    }
}

#[test]
fn maximum_quantities_are_counted_without_expansion_and_overflow_rejects() {
    let authorities = authorities();
    let available = rows(&[(0, u64::MAX)], &authorities);
    let required = rows(&[(0, u64::MAX - 1)], &authorities);
    let prepared =
        prepare_counted_phlo_discharge(&available, &required, limits(), &work()).unwrap();
    let witness = prepared.witness();
    assert_eq!(witness.used.len(), 1);
    assert_eq!(witness.used[0].quantity, u64::MAX - 1);
    assert_eq!(witness.unused.len(), 1);
    assert_eq!(witness.unused[0].quantity, 1);
    assert!(witness.fresh.is_empty());
    let overflowing = rows(&[(0, u64::MAX), (0, 1)], &authorities);
    assert!(matches!(
        prepare_counted_phlo_discharge(&overflowing, &required, limits(), &work()),
        Err(PhloPartitionError::Execution(
            PhloExecutionError::ArithmeticOverflow
        ))
    ));
}

#[test]
fn all_representation_and_work_bounds_apply_before_a_result_is_exposed() {
    let authorities = authorities();
    let available = rows(&[(0, 1)], &authorities);
    for dimension in 0..6 {
        let mut bounded = limits();
        match dimension {
            0 => bounded.execution.resource_entries = 2,
            1 => bounded.execution.authority_nodes = 2,
            2 => bounded.execution.key_bytes = 1,
            3 => bounded.aggregate_key_bytes = 1,
            4 => bounded.key.wire.total_bytes = 1,
            5 => bounded.key.authority_nodes = 0,
            _ => unreachable!(),
        }
        assert!(
            prepare_counted_phlo_discharge(&available, &available, bounded, &work()).is_err(),
            "dimension {dimension}"
        );
    }
    let zero_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(prepare_counted_phlo_discharge(&available, &available, limits(), &zero_work).is_err());
    let zero = rows(&[(0, 0)], &authorities);
    assert!(matches!(
        prepare_counted_phlo_discharge(&zero, &available, limits(), &work()),
        Err(PhloPartitionError::Execution(
            PhloExecutionError::ZeroQuantity
        ))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_typed_discharge_matches_expanded_occurrences_and_permutations(
        available in prop::collection::vec((0_u8..16, 1_u64..8), 0..24),
        required in prop::collection::vec((0_u8..16, 1_u64..8), 0..24),
    ) {
        let authorities = authorities();
        let mut remaining: Vec<_> = available.iter().flat_map(|(key, quantity)| std::iter::repeat_n(*key, *quantity as usize)).collect();
        let mut used = BTreeMap::new();
        let mut fresh = BTreeMap::new();
        for key in required.iter().flat_map(|(key, quantity)| std::iter::repeat_n(*key, *quantity as usize)) {
            if let Some(index) = remaining.iter().position(|candidate| *candidate == key) {
                remaining.swap_remove(index);
                *used.entry(key).or_insert(0_u64) += 1;
            } else {
                *fresh.entry(key).or_insert(0_u64) += 1;
            }
        }
        let mut unused = BTreeMap::new();
        for key in remaining { *unused.entry(key).or_insert(0_u64) += 1; }
        let a = rows(&available, &authorities);
        let r = rows(&required, &authorities);
        let prepared = prepare_counted_phlo_discharge(&a, &r, limits(), &work()).unwrap();
        let witness = prepared.witness();
        prop_assert_eq!(counts(witness.used, &authorities), used);
        prop_assert_eq!(counts(witness.unused, &authorities), unused);
        prop_assert_eq!(counts(witness.fresh, &authorities), fresh);
        assert_checked(&prepared);
        let mut reversed_a = a.clone(); reversed_a.reverse();
        let mut reversed_r = r.clone(); reversed_r.reverse();
        let reversed = prepare_counted_phlo_discharge(&reversed_a, &reversed_r, limits(), &work()).unwrap();
        let other = reversed.witness();
        prop_assert_eq!((witness.used, witness.unused, witness.fresh), (other.used, other.unused, other.fresh));
        let mut split_a = Vec::new();
        for amount in &a {
            split_a.extend(std::iter::repeat_n(PhloResourceAmount {quantity: 1, ..*amount}, amount.quantity as usize));
        }
        let split = prepare_counted_phlo_discharge(&split_a, &r, limits(), &work()).unwrap();
        let other = split.witness();
        prop_assert_eq!((witness.used, witness.unused, witness.fresh), (other.used, other.unused, other.fresh));
    }
}
