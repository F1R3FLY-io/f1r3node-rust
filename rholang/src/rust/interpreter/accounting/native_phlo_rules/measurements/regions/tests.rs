use std::sync::Arc;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::super::tests::{row, snapshot};
use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use crate::rust::interpreter::accounting::native_phlo_rules::tests::schedule;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloDimension, NativePhloRules,
};

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)))
}

fn limits() -> NativePhloRegionLimits {
    NativePhloRegionLimits {
        regions: 100_000,
        encoded_authority_bytes: 10_000_000,
    }
}

fn with_regions(mut observation: Arc<ByteObservation>, count: usize) -> Arc<ByteObservation> {
    let row = Arc::make_mut(&mut observation);
    let template = row.authority.regions[0].clone();
    row.authority.regions = (0..count)
        .map(|index| {
            let mut region = template.clone();
            region.instance_id = vec![0; 32];
            region.instance_id[24..].copy_from_slice(&(index as u64).to_be_bytes());
            region
        })
        .collect();
    observation
}

fn project(
    input: &ByteObservationSnapshot,
    rules: &NativePhloRules,
) -> Vec<(ByteObservation, CostRegion, usize, u64)> {
    let measured = rules.measure(input, 32).unwrap();
    let demands = measured.region_demands(limits(), &budget()).unwrap();
    demands
        .occurrences()
        .map(|demand| {
            let part = demand.measurement();
            (
                part.observation().clone(),
                demand.region().clone(),
                part.class(),
                part.quantity(),
            )
        })
        .collect()
}

#[test]
fn quoted_and_named_authorities_remain_distinct_complete_regions() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let mut observation = with_regions(row(AuthorityByteEventKind::Comm, [1; 3], 0), 2);
    let authority = &mut Arc::make_mut(&mut observation).authority;
    authority.regions[0].signature = Some(CostSignature {
        value: Some(Value::Quote(Default::default())),
    });
    authority.regions[1].signature = Some(CostSignature {
        value: Some(Value::Name(Default::default())),
    });
    let input = snapshot(vec![observation]);
    let projected = project(&input, &rules);
    assert_eq!(projected.len(), 8);
    for pair in projected.chunks_exact(2) {
        assert_eq!(pair[0].1, input.rows[0].authority.regions[0]);
        assert_eq!(pair[1].1, input.rows[0].authority.regions[1]);
        assert_ne!(pair[0].1.signature, pair[1].1.signature);
    }
}

#[test]
fn projection_retains_unit_compound_regions_and_repeated_occurrences() {
    let mut policy = schedule();
    policy.classes.rotate_left(1);
    let rules = NativePhloRules::resolve(&policy).unwrap();
    for owners in [0, 1, 3, 65] {
        let observation = with_regions(row(AuthorityByteEventKind::Comm, [3, 5, 7], owners), 65);
        let input = snapshot(vec![observation.clone(), observation.clone()]);
        let measured = rules.measure(&input, 2).unwrap();
        let demands = measured.region_demands(limits(), &budget()).unwrap();
        let actual: Vec<_> = demands.occurrences().collect();
        assert_eq!(actual.len(), 2 * 4 * 65);
        for (index, demand) in actual.iter().enumerate() {
            let part = demand.measurement();
            let dimension = (index / 65) % 4;
            assert_eq!(part.dimension(), NativePhloDimension::ALL[dimension]);
            assert_eq!(part.class(), (dimension + 3) % 4);
            assert_eq!(part.quantity(), [1, 3, 5, 7][dimension]);
            assert!(std::ptr::eq(part.observation(), observation.as_ref()));
            assert!(std::ptr::eq(
                demand.region(),
                &observation.authority.regions[index % 65]
            ));
        }
        assert_eq!(Arc::strong_count(&observation), 3);
    }
}

#[test]
fn malformed_or_missing_region_evidence_rejects_the_complete_projection() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let valid = with_regions(row(AuthorityByteEventKind::Comm, [1; 3], 3), 2);
    for case in 0..9 {
        let mut bad = valid.clone();
        let authority = &mut Arc::make_mut(&mut bad).authority;
        let expected = match case {
            0 => {
                authority.regions.clear();
                AuthorityError::MissingAuthority
            }
            1 => {
                authority.regions[0].instance_id.pop();
                AuthorityError::InvalidRegionIdentity
            }
            2 => {
                authority.regions[0].signature = None;
                AuthorityError::MissingSignature
            }
            3 => {
                authority.regions[1] = authority.regions[0].clone();
                AuthorityError::NonCanonicalAuthority
            }
            4 => {
                authority.regions.reverse();
                AuthorityError::NonCanonicalAuthority
            }
            5 => {
                authority.regions[1].instance_id = authority.regions[0].instance_id.clone();
                authority.regions[1].signature = Some(CostSignature {
                    value: Some(Value::Unit(true)),
                });
                AuthorityError::NonCanonicalAuthority
            }
            6 => {
                authority.regions[0].signature = Some(CostSignature {
                    value: Some(Value::BoundLevel(0)),
                });
                AuthorityError::UnresolvedBoundLevel
            }
            7 => {
                authority.regions[0].signature = Some(CostSignature {
                    value: Some(Value::Unit(false)),
                });
                AuthorityError::NonCanonicalSignature
            }
            _ => {
                let Some(Value::Compound(compound)) = authority.regions[0]
                    .signature
                    .as_mut()
                    .unwrap()
                    .value
                    .as_mut()
                else {
                    panic!("compound fixture")
                };
                compound.elements.reverse();
                AuthorityError::NonCanonicalSignature
            }
        };
        let input = snapshot(vec![valid.clone(), bad]);
        let original = input.clone();
        let measured = rules.measure(&input, 2).unwrap();
        assert_eq!(
            measured.region_demands(limits(), &budget()).unwrap_err(),
            NativePhloRegionError::Authority(expected),
            "case {case}"
        );
        assert_eq!(input, original);
    }
}

#[test]
fn aggregate_region_bytes_and_host_work_limits_reject_without_a_partial_result() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let observation = with_regions(row(AuthorityByteEventKind::Comm, [u64::MAX; 3], 3), 2);
    let input = snapshot(vec![observation.clone()]);
    let measured = rules.measure(&input, 1).unwrap();
    let exact = NativePhloRegionLimits {
        regions: 2,
        encoded_authority_bytes: observation.authority.encoded_len(),
    };
    assert_eq!(
        measured
            .region_demands(exact, &budget())
            .unwrap()
            .occurrences()
            .count(),
        8
    );
    assert_eq!(
        measured
            .region_demands(
                NativePhloRegionLimits {
                    regions: 1,
                    ..exact
                },
                &budget()
            )
            .unwrap_err(),
        NativePhloRegionError::RegionLimit
    );
    assert_eq!(
        measured
            .region_demands(
                NativePhloRegionLimits {
                    encoded_authority_bytes: exact.encoded_authority_bytes - 1,
                    ..exact
                },
                &budget()
            )
            .unwrap_err(),
        NativePhloRegionError::AuthorityByteLimit
    );
    for dimension in [
        HostWorkDimension::AuthorityNodes,
        HostWorkDimension::AuthorityDepth,
        HostWorkDimension::StructuralBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(10_000));
        caps.set(dimension, HostWorkLimit::new(0));
        let work = HostWorkBudget::new(caps);
        assert_eq!(
            measured.region_demands(exact, &work).unwrap_err(),
            NativePhloRegionError::Authority(AuthorityError::HostWorkRejected)
        );
        assert!(work.is_rejected());
    }
    let observation = with_regions(row(AuthorityByteEventKind::Comm, [0; 3], 3), 2);
    let input = snapshot(vec![observation.clone(), observation.clone()]);
    let measured = rules.measure(&input, 2).unwrap();
    assert_eq!(
        measured
            .region_demands(
                NativePhloRegionLimits {
                    regions: 3,
                    ..limits()
                },
                &budget()
            )
            .unwrap_err(),
        NativePhloRegionError::RegionLimit
    );
    assert_eq!(
        measured
            .region_demands(
                NativePhloRegionLimits {
                    encoded_authority_bytes: observation.authority.encoded_len(),
                    ..limits()
                },
                &budget()
            )
            .unwrap_err(),
        NativePhloRegionError::AuthorityByteLimit
    );
}

#[test]
fn empty_measurements_do_not_invent_demand_and_zero_rows_do_not_hide_malformed_regions() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let empty = snapshot(Vec::new());
    let measured = rules.measure(&empty, 0).unwrap();
    assert_eq!(
        measured
            .region_demands(
                NativePhloRegionLimits {
                    regions: 0,
                    encoded_authority_bytes: 0
                },
                &budget()
            )
            .unwrap()
            .occurrences()
            .count(),
        0
    );
    let mut zero = with_regions(
        row(AuthorityByteEventKind::ProduceIntroduction, [0; 3], 0),
        0,
    );
    let input = snapshot(vec![zero.clone()]);
    let measured = rules.measure(&input, 1).unwrap();
    assert_eq!(
        measured
            .region_demands(limits(), &budget())
            .unwrap()
            .occurrences()
            .count(),
        0
    );
    Arc::make_mut(&mut zero).authority.regions.push(CostRegion {
        instance_id: vec![0; 32],
        signature: None,
    });
    let input = snapshot(vec![zero]);
    let measured = rules.measure(&input, 1).unwrap();
    assert_eq!(
        measured.region_demands(limits(), &budget()).unwrap_err(),
        NativePhloRegionError::Authority(AuthorityError::MissingSignature)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn region_projection_refines_the_full_cross_product_without_unit_expansion(
        samples in prop::collection::vec((0_u8..3, prop::array::uniform3(0_u64..100_000), 0_usize..17, 1_usize..17), 0..17),
        rotation in 0_usize..4,
        split in any::<usize>(),
    ) {
        let mut policy = schedule();
        policy.classes.rotate_left(rotation);
        let rules = NativePhloRules::resolve(&policy).unwrap();
        let input = snapshot(samples.iter().map(|(kind, quantities, owners, regions)| {
            with_regions(row(AuthorityByteEventKind::from_tag(*kind).unwrap(), *quantities, *owners), *regions)
        }).collect());
        let measured = rules.measure(&input, 16).unwrap();
        let demands = measured.region_demands(limits(), &budget()).unwrap();
        let actual: Vec<_> = demands.occurrences().map(|demand| {
            let part = demand.measurement();
            (part.observation(), part.dimension(), part.class(), part.quantity(), demand.region())
        }).collect();
        let mut expected = Vec::new();
        for (observation, (kind, quantities, _, _)) in input.rows.iter().zip(&samples) {
            let quantities = [u64::from(*kind == 2), quantities[0], quantities[1], quantities[2]];
            for (index, quantity) in quantities.into_iter().enumerate() {
                if quantity == 0 { continue; }
                for region in &observation.authority.regions {
                    expected.push((observation.as_ref(), NativePhloDimension::ALL[index], (index + 4 - rotation) % 4, quantity, region));
                }
            }
        }
        prop_assert_eq!(&actual, &expected);
        let split = split % (input.rows.len() + 1);
        let left = snapshot(input.rows[..split].to_vec());
        let right = snapshot(input.rows[split..].to_vec());
        let original_projection = project(&input, &rules);
        let mut composed = project(&left, &rules);
        composed.extend(project(&right, &rules));
        prop_assert_eq!(&original_projection, &composed);
        let reverse = snapshot(input.rows.iter().rev().cloned().collect());
        let mut expected_reverse = Vec::new();
        for row in &reverse.rows {
            for (observation, _, class, quantity, region) in &expected {
                if std::ptr::eq(*observation, row.as_ref()) {
                    expected_reverse.push(((*observation).clone(), (*region).clone(), *class, *quantity));
                }
            }
        }
        prop_assert_eq!(project(&reverse, &rules), expected_reverse);
        let twice = snapshot(input.rows.iter().chain(input.rows.iter()).cloned().collect());
        prop_assert_eq!(project(&twice, &rules), [original_projection.as_slice(), original_projection.as_slice()].concat());
    }
}
