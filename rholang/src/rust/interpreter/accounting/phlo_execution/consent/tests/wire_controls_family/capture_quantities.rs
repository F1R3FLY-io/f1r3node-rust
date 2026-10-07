use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_obligation::PhloObligationKeyLimits;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;
use crate::rust::interpreter::accounting::phlo_execution::{
    CanonicalPhloFundingCapture, PhloCaptureLimits, PhloObligationKey, PhloResourceAmount,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Debug, PartialEq, Eq)]
struct Column {
    key: Vec<u8>,
    quantity: u64,
    amount: u64,
    contributions: Vec<(Vec<u8>, u64)>,
}

mod births;
mod cell_backing;

fn capture_columns(
    count: usize,
    price: u64,
    quantity: u64,
    rotation: usize,
    reverse: bool,
) -> Vec<Column> {
    with_capture(count, price, quantity, rotation, reverse, |_| {})
}

fn with_capture(
    count: usize,
    price: u64,
    quantity: u64,
    rotation: usize,
    reverse: bool,
    inspect: impl FnOnce(&CanonicalPhloFundingCapture<'_>),
) -> Vec<Column> {
    let cap = |n| NonZeroUsize::new(n).unwrap();
    let budget = || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
    let wire = PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    };
    let work = PhloExecutionLimits {
        resource_entries: 16,
        authority_nodes: 65_536,
        key_bytes: 1_048_576,
    };
    let control_limits = PhloControlsLimits {
        wire,
        owners: 1,
        schedules: 1,
        total_classes: 1,
    };
    let intent_limits = PhloFundingIntentLimits {
        wire,
        controls: control_limits,
        sources: count,
        resource_permissions: count * 2,
        authority_nodes: 65_536,
    };
    let source_limits = PhloSourceLimits {
        wire,
        resource_permissions: 2,
        authority_nodes: 65_536,
    };
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority), PhloResource {
        location: b"another slot",
        ..resource(&authority)
    }];
    let mut retained = vec![
        PhloResourceAmount {
            resource: resources[0],
            quantity,
        },
        PhloResourceAmount {
            resource: resources[1],
            quantity: quantity + 1,
        },
    ];
    if reverse {
        retained.reverse();
    }
    let descriptor = PhloControlsV1 {
        limit: 7,
        price_ceiling: price,
        required_owner_ceilings: vec![price],
        permitted_schedules: vec![PhloScheduleV1 {
            protocol_version: ENV.protocol_version,
            network: ENV.network,
            shard: ENV.shard,
            settlement_asset: ENV.asset,
            settlement_unit: ENV.unit,
            decimal_scale: ENV.decimal_scale,
            classes: vec![PhloResourceClassV1 {
                identity: b"COMM",
                measurement_unit: b"authority demand",
                measurement_rule: [2; 32],
                valuation_rule: [3; 32],
                weight: 7,
            }],
            actual_price: price,
            compatibility_rule: [4; 32],
        }],
    };
    let total = 1 + (2 * quantity + 2) * 7 * price;
    let mut identities = (0..count)
        .map(|i| (i as u64).to_be_bytes())
        .collect::<Vec<_>>();
    identities.rotate_left(rotation % count);
    if reverse {
        identities.reverse();
    }
    let record = PhloFundingIntentV1 {
        schedule_commitment: descriptor.permitted_schedules[0]
            .digest(control_limits.schedule(1))
            .unwrap(),
        controls: descriptor,
        total_exposure: u128::from(total),
        sources: identities
            .iter()
            .map(|key| {
                policy(key, total, true, &resources)
                    .wire_policy(work, source_limits)
                    .unwrap()
            })
            .collect(),
    };
    let binding = PhloFundingIntentBinding::new(&record, intent_limits).unwrap();
    let view = binding.view().unwrap();
    let selected = view.controls().permitted_schedules[0];
    let controls = check_phlo_controls(ENV, 0, u64::MAX, view.controls(), selected, 7).unwrap();
    let execution = check_phlo_execution(
        controls,
        PhloExecutionWitness {
            available: &[],
            required: &resources[..1],
            used: &[],
            unused: &[],
            fresh: &resources[..1],
        },
        work,
    )
    .unwrap()
    .with_retained_acquisitions(&retained, &budget())
    .unwrap();
    let obligations =
        project_phlo_obligations(execution, PhloOutcome::Accepted(&[]), cap(4)).unwrap();
    assert_eq!(obligations.total(), total);
    let sources = identities
        .iter()
        .map(|key| source(key, total))
        .collect::<Vec<_>>();
    let eligible = vec![vec![true; 4]; count];
    let assignment = identities
        .iter()
        .map(|key| {
            let index = u64::from_be_bytes(*key);
            obligations
                .amounts()
                .iter()
                .map(|amount| amount / count as u64 + u64::from(index < amount % count as u64))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let cases = [PhloFundingCase {
        obligations: &obligations,
        eligible: &eligible,
        assignment: &assignment,
    }];
    let family =
        check_phlo_funding_family(&sources, &cases, u128::from(total), PhloFundingLimits {
            sources: cap(count),
            cases: cap(1),
            obligations: cap(4),
            assignment_cells: count * 4,
            custody_bytes: count * 8,
        })
        .unwrap();
    let checked = view
        .check_family(
            &family,
            PhloFundingTerms {
                required_owner_ceilings: &record.controls.required_owner_ceilings,
                asset: ENV.asset,
                schedule_commitment: selected.commitment,
            },
            limits(),
        )
        .unwrap();
    let capture = checked
        .capture_case(
            0,
            PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: cap(count),
                    obligation_cap: cap(4),
                },
                key: PhloObligationKeyLimits {
                    wire,
                    authority_nodes: 65_536,
                },
                aggregate_key_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    assert_eq!(capture.obligations().len(), 4);
    inspect(&capture);
    let columns = capture
        .obligations()
        .map(|column| {
            let expected = match column.key() {
                PhloObligationKey::Fee => (1, 1),
                PhloObligationKey::Resource(key) => {
                    assert_eq!(key, resources[0]);
                    (1, 7 * price)
                }
                PhloObligationKey::RetainedResource(key) => {
                    let quantity = quantity + u64::from(key == resources[1]);
                    assert!(resources.contains(&key));
                    (quantity, quantity * 7 * price)
                }
            };
            assert_eq!((column.quantity(), column.amount()), expected);
            assert_eq!(column.contributions().len(), count);
            let contributions = column
                .contributions()
                .map(|(source, amount)| (source.source().custody.to_vec(), amount))
                .collect::<Vec<_>>();
            assert!(contributions.windows(2).all(|pair| pair[0].0 < pair[1].0));
            assert_eq!(
                contributions.iter().map(|row| row.1).sum::<u64>(),
                column.amount()
            );
            Column {
                key: column.encoded_key().to_vec(),
                quantity: column.quantity(),
                amount: column.amount(),
                contributions,
            }
        })
        .collect::<Vec<_>>();
    assert!(columns.windows(2).all(|pair| pair[0].key < pair[1].key));
    for (source_index, source) in capture.sources().iter().enumerate() {
        assert_eq!(
            columns
                .iter()
                .map(|column| column.contributions[source_index].1)
                .sum::<u64>(),
            source.debit()
        );
    }
    columns
}

#[test]
fn capture_preserves_retained_quantities_at_zero_price_and_arbitrary_wallet_counts() {
    for count in [1, 3, 65] {
        for price in [0, 1, 9] {
            assert_eq!(
                capture_columns(count, price, 13, 0, false),
                capture_columns(count, price, 13, count / 2, true)
            );
        }
    }
    let first = capture_columns(3, 0, 13, 0, false);
    let second = capture_columns(3, 0, 14, 0, false);
    assert_eq!(
        first.iter().map(|c| (&c.key, c.amount)).collect::<Vec<_>>(),
        second
            .iter()
            .map(|c| (&c.key, c.amount))
            .collect::<Vec<_>>()
    );
    assert_ne!(first, second);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn capture_quantity_permutation_preserves_complete_columns(
        count in 1usize..76,
        price in 0u64..100,
        quantity in 1u64..1000,
        rotation in any::<usize>(),
    ) {
        let expected = capture_columns(count, price, quantity, 0, false);
        let actual = capture_columns(count, price, quantity, rotation, true);
        prop_assert_eq!(expected, actual);
    }
}
