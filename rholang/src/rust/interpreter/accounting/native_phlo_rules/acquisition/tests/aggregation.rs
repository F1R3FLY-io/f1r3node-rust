//! C8 (DR-87): acquisition demand aggregated by (purse identity, class).

use std::num::NonZeroUsize;

use super::*;
use crate::rust::interpreter::accounting::phlo_execution::{
    project_phlo_obligations, CountedPhloExecutionWitness, PhloObligationKey,
};

/// The signature pool of the generated purses. Pool entries 2 and 3 share one
/// channel and differ in authority.
fn pooled_signature(index: usize) -> CostSignature {
    match index % 4 {
        0 => {
            sort_signature(&CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![CostSignature {
                        value: Some(Value::Ground(vec![0])),
                    }],
                })),
            })
            .term
        }
        1 => {
            sort_signature(&CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![
                        CostSignature {
                            value: Some(Value::Ground(vec![0])),
                        },
                        CostSignature {
                            value: Some(Value::Ground(vec![1])),
                        },
                    ],
                })),
            })
            .term
        }
        2 => CostSignature {
            value: Some(Value::Quote(Par::default())),
        },
        _ => CostSignature {
            value: Some(Value::Ground(Vec::new())),
        },
    }
}

/// Rows whose regions bind instance id k to pooled signature k mod 4, so
/// instance ids k and k + 4 are distinct purses with one identity.
fn pooled_snapshot(rows: &[(Vec<u8>, [u64; 3])]) -> ByteObservationSnapshot {
    let rows = rows
        .iter()
        .enumerate()
        .map(|(row, (instances, quantities))| {
            let mut event_id = [0; 32];
            event_id[..8].copy_from_slice(&(row as u64).to_be_bytes());
            Arc::new(ByteObservation {
                event_id,
                kind: AuthorityByteEventKind::Comm,
                authority: CostAuthority {
                    regions: instances
                        .iter()
                        .map(|instance| CostRegion {
                            instance_id: vec![*instance; 32],
                            signature: Some(pooled_signature(usize::from(*instance))),
                        })
                        .collect(),
                },
                measurement: Some(ByteCharge {
                    introduction_bytes: quantities[0],
                    transfer_bytes: quantities[1],
                    trace_bytes: quantities[2],
                }),
                legacy_amount: None,
            })
        })
        .collect();
    ByteObservationSnapshot {
        rows,
        metered_context: true,
        history_lost: false,
    }
}

/// Controls at the schedule's price whose limit exceeds every generated
/// usage. `ceilings` holds the price.
fn controls<'a>(
    schedules: &'a [crate::rust::interpreter::accounting::phlo_controls::PhloSchedule<'a>],
    ceilings: &'a [u64],
) -> crate::rust::interpreter::accounting::phlo_controls::CheckedPhloControls<'a> {
    let limit = 1 << 40;
    check_phlo_controls(
        schedules[0].environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: schedules[0].actual_price,
            required_owner_ceilings: ceilings,
            permitted_schedules: schedules,
        },
        schedules[0],
        limit,
    )
    .expect("controls fit")
}

fn discharge_limits(execution: PhloExecutionLimits) -> PhloDischargeLimits {
    PhloDischargeLimits {
        execution,
        key: key_limits(),
        aggregate_key_bytes: 20_000_000,
    }
}

/// The encoded keys, amounts and quantities of a set of obligations, sorted by
/// key, with the total.
fn obligation_summary(
    obligations: &crate::rust::interpreter::accounting::phlo_execution::CheckedPhloObligations<'_>,
) -> (Vec<(Vec<u8>, u64, u64)>, u64) {
    let keys = obligations
        .encoded_keys(key_limits(), 20_000_000)
        .expect("keys encode");
    let mut rows = keys
        .into_iter()
        .zip(obligations.amounts().iter().copied())
        .zip(obligations.quantities().iter().copied())
        .map(|((key, amount), quantity)| (key, amount, quantity))
        .collect::<Vec<_>>();
    rows.sort();
    (rows, obligations.total())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// C8 (DR-87; `AggregatedAcquisitionDemand.aggregate_preserves_counts`,
    /// `_weighted_usage`, `_partition`, `aggregate_keys_distinct`): the
    /// aggregated demand holds distinct keys whose quantities sum their
    /// occurrences, it does not depend on the row order, and it checks,
    /// discharges and projects like the per-occurrence demand. With the entry
    /// limit at the distinct count, the aggregated witness passes and the
    /// per-occurrence witness fails whenever it has more entries.
    #[test]
    fn aggregated_witness_checks_like_occurrence_witness(
        rows in prop::collection::vec(
            (prop::collection::btree_set(0_u8..8, 1..4), prop::array::uniform3(0_u64..2000)),
            1..7,
        ),
        price in 0_u64..50,
        weights in prop::array::uniform4(0_u64..20),
    ) {
        let rows = rows
            .into_iter()
            .map(|(instances, quantities)| (instances.into_iter().collect::<Vec<_>>(), quantities))
            .collect::<Vec<_>>();
        let mut descriptor = schedule();
        for (class, weight) in descriptor.classes.iter_mut().zip(weights) {
            class.weight = weight;
        }
        descriptor.actual_price = price;
        let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
        let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
        let rules = NativePhloRules::resolve(&descriptor).unwrap();
        let summarize = |rows: &[(Vec<u8>, [u64; 3])]| {
            let input = pooled_snapshot(rows);
            let measured = rules.measure(&input, rows.len()).unwrap();
            let regions = measured.region_demands(region_limits(), &budget()).unwrap();
            let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
            let prepared = located
                .prepare_acquisition_demand(&binding, &terms, limits(), &budget())
                .unwrap();
            prepared
                .resources()
                .iter()
                .map(|entry| {
                    (
                        entry.resource.location.to_vec(),
                        entry.resource.class,
                        entry.resource.authority.clone(),
                        entry.quantity,
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut reversed = rows.clone();
        reversed.reverse();
        prop_assert_eq!(summarize(&rows), summarize(&reversed));

        let input = pooled_snapshot(&rows);
        let measured = rules.measure(&input, rows.len()).unwrap();
        let regions = measured.region_demands(region_limits(), &budget()).unwrap();
        let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
        let prepared = located
            .prepare_acquisition_demand(&binding, &terms, limits(), &budget())
            .unwrap();
        let entries = prepared.resources();
        for (index, left) in entries.iter().enumerate() {
            for right in &entries[index + 1..] {
                prop_assert_ne!(left.resource, right.resource);
            }
        }
        let occurrences = prepared.occurrences().map(|(_, amount)| amount).collect::<Vec<_>>();
        prop_assert_eq!(occurrences.len(), prepared.occurrence_count());
        let mut sums = vec![0_u64; entries.len()];
        for (position, amount) in occurrences.iter().enumerate() {
            let entry = prepared.occurrence_entry(position).unwrap();
            prop_assert_eq!(entries[entry].resource, amount.resource);
            sums[entry] += amount.quantity;
        }
        for (entry, sum) in entries.iter().zip(&sums) {
            prop_assert_eq!(entry.quantity, *sum);
        }

        let schedules = [binding.schedule()];
        let ceilings = [schedules[0].actual_price];
        let controls = controls(&schedules, &ceilings);
        let by_occurrence =
            prepare_counted_phlo_discharge(&[], &occurrences, discharge_limits(execution_limits()), &budget())
                .unwrap();
        let aggregated =
            prepare_counted_phlo_discharge(&[], entries, discharge_limits(execution_limits()), &budget())
                .unwrap();
        let by_occurrence =
            check_counted_phlo_execution(controls, by_occurrence.witness(), execution_limits()).unwrap();
        let aggregated =
            check_counted_phlo_execution(controls, aggregated.witness(), execution_limits()).unwrap();
        prop_assert_eq!(aggregated.usage(), by_occurrence.usage());
        prop_assert_eq!(aggregated.prepaid_usage(), by_occurrence.prepaid_usage());
        prop_assert_eq!(aggregated.fresh_usage(), by_occurrence.fresh_usage());
        prop_assert_eq!(
            aggregated.retained_charge(PhloOutcome::Accepted(&[])),
            by_occurrence.retained_charge(PhloOutcome::Accepted(&[]))
        );
        let cap = NonZeroUsize::new(4_096).unwrap();
        let aggregated_obligations =
            project_phlo_obligations(aggregated, PhloOutcome::Accepted(&[]), cap).unwrap();
        let occurrence_obligations =
            project_phlo_obligations(by_occurrence, PhloOutcome::Accepted(&[]), cap).unwrap();
        prop_assert_eq!(
            obligation_summary(&aggregated_obligations),
            obligation_summary(&occurrence_obligations)
        );

        // The required and fresh parts each hold the demand, so the entry
        // limit of the distinct witness is twice the distinct count.
        let tight = PhloExecutionLimits {
            resource_entries: 2 * entries.len(),
            ..execution_limits()
        };
        let distinct = check_counted_phlo_execution(
            controls,
            CountedPhloExecutionWitness {
                available: &[],
                used: &[],
                unused: &[],
                required: entries,
                fresh: entries,
            },
            tight,
        );
        prop_assert!(distinct.is_ok());
        let per_occurrence = check_counted_phlo_execution(
            controls,
            CountedPhloExecutionWitness {
                available: &[],
                used: &[],
                unused: &[],
                required: &occurrences,
                fresh: &occurrences,
            },
            tight,
        );
        prop_assert_eq!(per_occurrence.is_ok(), occurrences.len() <= entries.len());
    }
}

/// C8 (DR-87): a demand with many occurrences of a few keys gives one
/// obligation for each key and the fee, settlement encodes each key once, and
/// the encoding work is the work of a demand with one occurrence per key.
#[test]
fn settlement_encodes_each_key_once() {
    let descriptor = schedule();
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let schedules = [binding.schedule()];
    let ceilings = [schedules[0].actual_price];
    let controls = controls(&schedules, &ceilings);
    let encode_work = |input: &ByteObservationSnapshot, copies: usize| {
        let measured = rules.measure(input, copies).unwrap();
        let regions = measured.region_demands(region_limits(), &budget()).unwrap();
        let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
        let prepared = located
            .prepare_acquisition_demand(&binding, &terms, limits(), &budget())
            .unwrap();
        let distinct = prepared.resources().len();
        let discharge = prepare_counted_phlo_discharge(
            &[],
            prepared.resources(),
            discharge_limits(execution_limits()),
            &budget(),
        )
        .unwrap();
        let checked =
            check_counted_phlo_execution(controls, discharge.witness(), execution_limits())
                .unwrap();
        let obligations = project_phlo_obligations(
            checked,
            PhloOutcome::Accepted(&[]),
            NonZeroUsize::new(4_096).unwrap(),
        )
        .unwrap();
        assert_eq!(obligations.keys().len(), distinct + 1);
        assert_eq!(
            obligations
                .keys()
                .iter()
                .filter(|key| matches!(key, PhloObligationKey::Fee))
                .count(),
            1
        );
        let keys = obligations.encoded_keys(key_limits(), 20_000_000).unwrap();
        let total = keys.iter().map(Vec::len).sum::<usize>();
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
        assert!(obligations.encoded_keys(key_limits(), total).is_ok());
        assert!(obligations.encoded_keys(key_limits(), total - 1).is_err());
        let host = budget();
        obligations
            .encoded_keys_with_budget(key_limits(), total, &host)
            .unwrap();
        (distinct, host.usages())
    };
    let many = snapshot([3, 5, 7], 3, 64);
    let mut single = snapshot([3, 5, 7], 3, 1);
    Arc::make_mut(&mut single.rows[0])
        .authority
        .regions
        .truncate(1);
    let (many_keys, many_work) = encode_work(&many, 64);
    let (single_keys, single_work) = encode_work(&single, 1);
    assert_eq!(many_keys, 4);
    assert_eq!(single_keys, 4);
    assert_eq!(many_work, single_work);
}

/// C8 (DR-87): the shape of the gateway funding flow. Two purse identities
/// meet the four classes in 2,904 located occurrences. The obligations are
/// the eight resource keys and the fee, and the demand fits the original
/// protocol-6 limits (4,096 entries, 4,096 authority nodes, 262,144 key
/// bytes), which the per-occurrence demand of 5,808 entries exceeds.
#[test]
fn funding_flow_has_nine_keys() {
    let descriptor = schedule();
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let rows = (0..363)
        .map(|_| (vec![0_u8, 1], [3, 5, 7]))
        .collect::<Vec<_>>();
    let input = pooled_snapshot(&rows);
    let measured = rules.measure(&input, rows.len()).unwrap();
    let regions = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
    let original = PhloExecutionLimits {
        resource_entries: 4_096,
        authority_nodes: 4_096,
        key_bytes: 262_144,
    };
    let prepared = located
        .prepare_acquisition_demand(
            &binding,
            &terms,
            NativePhloAcquisitionLimits {
                entries: 4_096,
                ..limits()
            },
            &budget(),
        )
        .unwrap();
    assert_eq!(prepared.occurrence_count(), 2_904);
    assert_eq!(prepared.resources().len(), 8);
    let schedules = [binding.schedule()];
    let ceilings = [schedules[0].actual_price];
    let controls = controls(&schedules, &ceilings);
    let discharge = prepare_counted_phlo_discharge(
        &[],
        prepared.resources(),
        discharge_limits(original),
        &budget(),
    )
    .unwrap();
    let checked = check_counted_phlo_execution(controls, discharge.witness(), original).unwrap();
    let obligations = project_phlo_obligations(
        checked,
        PhloOutcome::Accepted(&[]),
        NonZeroUsize::new(4_096).unwrap(),
    )
    .unwrap();
    assert_eq!(obligations.keys().len(), 9);
    let occurrences = prepared
        .occurrences()
        .map(|(_, amount)| amount)
        .collect::<Vec<_>>();
    assert!(check_counted_phlo_execution(
        controls,
        CountedPhloExecutionWitness {
            available: &[],
            used: &[],
            unused: &[],
            required: &occurrences,
            fresh: &occurrences,
        },
        original,
    )
    .is_err());
}
