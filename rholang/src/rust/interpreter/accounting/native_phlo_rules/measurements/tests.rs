use std::sync::Arc;

use models::rhoapi::{CostAuthority, CostRegion};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::sig_to_cost_signature;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::native_phlo_rules::tests::schedule;
use crate::rust::interpreter::accounting::Sig;

pub(super) fn row(
    kind: AuthorityByteEventKind,
    quantities: [u64; 3],
    owners: usize,
) -> Arc<ByteObservation> {
    let mut signature = Sig::Unit;
    for owner in 0..owners {
        let atom = Sig::Ground(vec![owner as u8]);
        signature = if owner == 0 {
            atom
        } else {
            Sig::And(Box::new(signature), Box::new(atom))
        };
    }
    Arc::new(ByteObservation {
        event_id: [7; 32],
        kind,
        authority: CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![3; 32],
                signature: Some(sig_to_cost_signature(&signature).unwrap()),
            }],
        },
        measurement: Some(ByteCharge {
            introduction_bytes: quantities[0],
            transfer_bytes: quantities[1],
            trace_bytes: quantities[2],
        }),
        legacy_amount: None,
    })
}

pub(super) fn snapshot(rows: Vec<Arc<ByteObservation>>) -> ByteObservationSnapshot {
    ByteObservationSnapshot {
        rows,
        metered_context: true,
        history_lost: false,
    }
}

#[test]
fn measurement_keeps_authority_identity_multiplicity_and_policy_indices() {
    let mut policy = schedule();
    policy.classes.rotate_left(2);
    let rules = NativePhloRules::resolve(&policy).unwrap();
    let receipt = row(AuthorityByteEventKind::Comm, [3, 5, 7], 65);
    let input = snapshot(vec![Arc::clone(&receipt), Arc::clone(&receipt)]);
    let captured = rules.measure(&input, 2).unwrap();
    let emitted: Vec<_> = captured.occurrences().collect();
    assert_eq!(emitted.len(), 8);
    for (part, dimension) in emitted
        .iter()
        .zip(NativePhloDimension::ALL.into_iter().cycle())
    {
        assert!(std::ptr::eq(part.observation(), receipt.as_ref()));
        assert_eq!(part.dimension(), dimension);
        assert_eq!(part.class(), (dimension.index() + 2) % 4);
        assert_eq!(part.quantity(), [1, 3, 5, 7][dimension.index()]);
    }
    for dimension in NativePhloDimension::ALL {
        assert_eq!(captured.total(dimension), [2, 6, 10, 14][dimension.index()]);
    }
    assert_eq!(Arc::strong_count(&receipt), 3);
}

#[test]
fn raw_only_unit_authority_still_requires_every_positive_dimension() {
    let mut policy = schedule();
    let input = snapshot(vec![row(AuthorityByteEventKind::Comm, [0, 5, 7], 0)]);
    let original = input.clone();
    for omitted in [
        NativePhloDimension::Compute,
        NativePhloDimension::Transfer,
        NativePhloDimension::Trace,
    ] {
        policy.classes = schedule()
            .classes
            .into_iter()
            .filter(|class| class.measurement_rule != omitted.measurement_rule())
            .collect();
        let rules = NativePhloRules::resolve(&policy).unwrap();
        assert_eq!(
            rules.measure(&input, 1).unwrap_err(),
            NativePhloMeasurementError::Rule(NativePhloRuleError::Missing(omitted))
        );
    }
    policy.classes = schedule()
        .classes
        .into_iter()
        .filter(|class| {
            class.measurement_rule != NativePhloDimension::Introduction.measurement_rule()
        })
        .collect();
    let rules = NativePhloRules::resolve(&policy).unwrap();
    assert_eq!(rules.measure(&input, 1).unwrap().occurrences().count(), 3);
    assert_eq!(input, original);
}

#[test]
fn introductions_are_not_comms_and_empty_complete_execution_has_no_occurrences() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let input = snapshot(vec![
        row(AuthorityByteEventKind::ProduceIntroduction, [3, 0, 0], 1),
        row(AuthorityByteEventKind::ConsumeIntroduction, [5, 0, 0], 1),
    ]);
    let captured = rules.measure(&input, 2).unwrap();
    assert_eq!(captured.total(NativePhloDimension::Compute), 0);
    assert_eq!(captured.total(NativePhloDimension::Introduction), 8);
    assert_eq!(captured.occurrences().count(), 2);
    let empty = snapshot(Vec::new());
    let captured = rules.measure(&empty, 0).unwrap();
    assert_eq!(captured.occurrences().count(), 0);
    for dimension in NativePhloDimension::ALL {
        assert_eq!(captured.total(dimension), 0);
    }
}

#[test]
fn maximum_quantities_do_not_expand_or_wrap_and_late_invalid_rows_reject_capture() {
    let rules = NativePhloRules::resolve(&schedule()).unwrap();
    let mut input = snapshot(vec![row(AuthorityByteEventKind::Comm, [u64::MAX; 3], 1)]);
    assert_eq!(rules.measure(&input, 2).unwrap().occurrences().count(), 4);
    input
        .rows
        .push(row(AuthorityByteEventKind::Comm, [1; 3], 1));
    assert_eq!(
        rules.measure(&input, 2).unwrap_err(),
        NativePhloMeasurementError::Observation(ByteMeasurementError::Overflow)
    );
    input.rows[0] = row(AuthorityByteEventKind::Comm, [0; 3], 1);
    Arc::make_mut(&mut input.rows[1]).measurement = None;
    assert_eq!(
        rules.measure(&input, 2).unwrap_err(),
        NativePhloMeasurementError::Observation(ByteMeasurementError::Incomplete)
    );
    assert_eq!(
        rules.measure(&input, 1).unwrap_err(),
        NativePhloMeasurementError::Observation(ByteMeasurementError::EntryLimit)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn projection_matches_independent_row_oracle_under_arbitrary_order_and_owners(
        samples in prop::collection::vec((0_u8..3, prop::array::uniform3(0_u64..100_000), 0_usize..17, any::<bool>()), 0..33),
        rotation in 0_usize..4,
        split in any::<usize>(),
    ) {
        let mut policy = schedule();
        policy.classes.rotate_left(rotation);
        let rules = NativePhloRules::resolve(&policy).unwrap();
        let input = snapshot(samples.iter().map(|(kind, quantities, owners, legacy)| {
            let kind = AuthorityByteEventKind::from_tag(*kind).unwrap();
            let mut receipt = row(kind, *quantities, *owners);
            Arc::make_mut(&mut receipt).legacy_amount = legacy.then_some(123);
            receipt
        }).collect());
        let original = input.clone();
        let captured = rules.measure(&input, 32).unwrap();
        let mut expected_totals = [0_u64; 4];
        let mut expected = Vec::new();
        for (receipt, (kind, quantities, _, _)) in input.rows.iter().zip(&samples) {
            let values = [u64::from(*kind == 2), quantities[0], quantities[1], quantities[2]];
            for (dimension, quantity) in values.into_iter().enumerate() {
                expected_totals[dimension] += quantity;
                if quantity > 0 {
                    expected.push((receipt.as_ref(), dimension, (dimension + 4 - rotation) % 4, quantity));
                }
            }
        }
        let observed: Vec<_> = captured.occurrences()
            .map(|part| (part.observation, part.dimension.index(), part.class, part.quantity)).collect();
        prop_assert_eq!(observed, expected);
        for dimension in NativePhloDimension::ALL {
            prop_assert_eq!(captured.total(dimension), expected_totals[dimension.index()]);
        }
        let split = split % (input.rows.len() + 1);
        let left = snapshot(input.rows[..split].to_vec());
        let right = snapshot(input.rows[split..].to_vec());
        let left = rules.measure(&left, 32).unwrap();
        let right = rules.measure(&right, 32).unwrap();
        let reversed = snapshot(input.rows.iter().rev().cloned().collect());
        let reversed = rules.measure(&reversed, 32).unwrap();
        for dimension in NativePhloDimension::ALL {
            prop_assert_eq!(captured.total(dimension), left.total(dimension) + right.total(dimension));
            prop_assert_eq!(captured.total(dimension), reversed.total(dimension));
        }
        prop_assert_eq!(input, original);
    }
}
