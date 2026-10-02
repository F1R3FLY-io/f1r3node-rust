use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{PhloConversionCompositionV2, PhloFundingIntentV1};
use models::rust::phlo_resource::PhloResourceKeyV1;
use models::rust::phlo_schedule::PhloResourceClassV1;
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;

use super::*;

fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: vec![PhloResourceClassV1 {
            identity: b"compute",
            measurement_unit: b"unit",
            measurement_rule: [1; 32],
            valuation_rule: [2; 32],
            weight: 1,
        }],
        actual_price: 10,
        compatibility_rule: [3; 32],
    }
}

fn intent() -> PhloFundingIntentV2<'static> {
    let selected = schedule();
    let source = PhloSourcePolicyV1::new(
        b"custody",
        200,
        200,
        true,
        Vec::<PhloResourceKeyV1<'_>>::new(),
        PhloSourceLimits {
            wire: PhloWireLimits {
                total_bytes: 4096,
                field_bytes: 2048,
            },
            resource_permissions: 0,
            authority_nodes: 0,
        },
    )
    .unwrap();
    PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 10,
                price_ceiling: 10,
                required_owner_ceilings: vec![10, 12],
                permitted_schedules: vec![selected.clone()],
            },
            schedule_commitment: selected.digest(PhloGenesisPolicy::LIMITS).unwrap(),
            total_exposure: 200,
            sources: vec![source],
        },
        grant_uses: vec![],
        conversion: PhloConversionCompositionV2::NoConversion,
    }
}

fn observation() -> StateBoundObservation {
    StateBoundObservation {
        envelope_identity: [1; 32],
        funding_root: [2; 32],
        execution_root: [7; 32],
        settlement_root: [3; 32],
        final_root: [4; 32],
        phlo_used: 3,
        fresh_phlo: 3,
        retained_phlo: 0,
        resource_rev: 30,
        fee_rev: 1,
        operation_evidence_hash: [5; 32],
        funding_evidence_hash: [6; 32],
        failure_class: 0,
    }
}

#[test]
fn offered_preflight_binds_signed_limit_price_owner_schedule_and_fee_ceiling() {
    let mut record = intent();
    let selected = schedule();
    assert_eq!(
        check_offer_terms(&record, &selected, 10, 10, 10, 10).unwrap(),
        101
    );
    for (limit, price, minimum, max_limit) in [
        (11, 10, 10, 20),
        (10, 11, 10, 10),
        (10, 10, 11, 10),
        (10, 10, 10, 9),
    ] {
        assert!(check_offer_terms(&record, &selected, minimum, limit, price, max_limit).is_err());
    }
    record.base.controls.required_owner_ceilings[0] = 9;
    assert!(check_offer_terms(&record, &selected, 10, 10, 10, 10).is_err());
    record.base.controls.required_owner_ceilings[0] = 10;
    record.base.total_exposure = 100;
    assert!(check_offer_terms(&record, &selected, 10, 10, 10, 10).is_err());
    record.base.total_exposure = 200;
    record.base.schedule_commitment[0] ^= 1;
    assert!(check_offer_terms(&record, &selected, 10, 10, 10, 10).is_err());
    record.base.schedule_commitment[0] ^= 1;
    record.base.controls.permitted_schedules.clear();
    assert!(check_offer_terms(&record, &selected, 10, 10, 10, 10).is_err());
    record
        .base
        .controls
        .permitted_schedules
        .push(selected.clone());
    record.base.sources.clear();
    assert!(check_offer_terms(&record, &selected, 10, 10, 10, 10).is_err());
}

#[test]
fn concrete_schedule_selection_comes_from_signed_intent() {
    let mut record = intent();
    let (selected, bytes) = select_signed_schedule(&record, 10).unwrap();
    assert_eq!(selected.actual_price, 10);
    assert_eq!(bytes, schedule().encode(PhloGenesisPolicy::LIMITS).unwrap());
    assert!(select_signed_schedule(&record, 11).is_err());
    record.base.schedule_commitment[0] ^= 1;
    assert!(select_signed_schedule(&record, 10).is_err());
    record.base.schedule_commitment[0] ^= 1;
    record.base.controls.permitted_schedules.push(schedule());
    assert!(select_signed_schedule(&record, 10).is_err());
}

#[test]
fn private_replay_comparison_checks_complete_observation_root_limit_and_separate_fee() {
    let expected = observation();
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &expected, &expected
    )
    .is_ok());
    let mut prepaid = expected.clone();
    prepaid.phlo_used = 5;
    prepaid.fresh_phlo = 2;
    prepaid.retained_phlo = 1;
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &prepaid, &prepaid
    )
    .is_ok());
    prepaid.retained_phlo = 6;
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &prepaid, &prepaid
    )
    .is_err());
    for changed in [
        StateBoundObservation {
            final_root: [8; 32],
            ..expected.clone()
        },
        StateBoundObservation {
            operation_evidence_hash: [8; 32],
            ..expected.clone()
        },
        StateBoundObservation {
            funding_evidence_hash: [8; 32],
            ..expected.clone()
        },
        StateBoundObservation {
            failure_class: 1,
            ..expected.clone()
        },
    ] {
        assert!(compare_state_bound_observations(
            [1; 32], [2; 32], [7; 32], 10, 10, &expected, &changed
        )
        .is_err());
    }
    let mut changed = expected.clone();
    changed.execution_root = [9; 32];
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &changed, &changed
    )
    .is_err());
    let mut changed = expected.clone();
    changed.envelope_identity = [9; 32];
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &changed, &changed
    )
    .is_err());
    let mut changed = expected.clone();
    changed.phlo_used = 11;
    changed.resource_rev = 110;
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &changed, &changed
    )
    .is_err());
    let mut changed = expected.clone();
    changed.resource_rev = 29;
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &changed, &changed
    )
    .is_err());
    let mut changed = expected.clone();
    changed.fee_rev = 2;
    assert!(compare_state_bound_observations(
        [1; 32], [2; 32], [7; 32], 10, 10, &changed, &changed
    )
    .is_err());
    assert!(compare_state_bound_observations(
        [1; 32],
        [2; 32],
        [7; 32],
        u64::MAX,
        u64::MAX,
        &expected,
        &expected
    )
    .is_err());
}

#[test]
fn publication_remains_closed_without_authenticated_scope_and_schedule() {
    let prepared = PreparedOfferedCandidate {
        intent: intent(),
        selected_schedule_bytes: schedule().encode(PhloGenesisPolicy::LIMITS).unwrap(),
        envelope_identity: [1; 32],
        funding_root: [2; 32],
        execution_root: [7; 32],
        phlo_limit: 10,
        phlo_price: 10,
        fee_rev: 1,
        rev_ceiling: 101,
    };
    let observed = observation();
    assert!(prepared
        .compare_private_replay(&observed, &observed)
        .is_ok());
    assert!(prepared
        .authorize_private_draft([9; 32], true, Some(0))
        .is_err());
    assert!(prepared
        .authorize_private_draft([1; 32], false, Some(0))
        .is_err());
    assert!(prepared
        .authorize_private_draft([1; 32], true, None)
        .is_err());
    let draft = prepared
        .authorize_private_draft([1; 32], true, Some(0))
        .expect("a complete private cut may be settled in isolation");
    assert_eq!(draft.envelope_identity(), [1; 32]);
    assert!(prepared.authorize_publication().is_err());
}
