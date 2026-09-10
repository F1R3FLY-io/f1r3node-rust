use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::casper::{
    CostAuthorityBornStackProto, CostAuthorityByteEventProto, CostAuthorityEventProto,
    CostAuthorityFundingCertificateProto, CostAuthorityPhysicalEventDrawProto,
    CostAuthorityResourceProto, CostAuthorityStackReservationProto, CostAuthorityWitnessProto,
    ProcessedSystemDeployProto,
};
use models::rhoapi::{CostAuthority, CostRegion};
use models::rust::casper::protocol::casper_message::{
    DeployAdmissionStatus, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy,
    SystemDeployData,
};
use proptest::prelude::*;
use prost::bytes::Bytes;

use super::tests::execution_evidence;
use super::RuntimeManager;
use crate::rust::util::construct_deploy;

macro_rules! field_mutations {
    ($base:expr, $ty:path, {$($field:ident => $change:expr),+ $(,)?}) => {{
        let original: &$ty = &$base;
        let $ty { $($field: _,)+ } = original;
        vec![$({
            let mut changed: $ty = original.clone();
            ($change)(&mut changed.$field);
            (stringify!($field), changed)
        }),+]
    }};
}

fn change_bytes(value: &mut Bytes) {
    let mut bytes = value.to_vec();
    bytes.push(1);
    *value = bytes.into();
}

fn change_u32(value: &mut u32) { *value = value.wrapping_add(1); }

fn change_u64(value: &mut u64) { *value = value.wrapping_add(1); }

fn change_i32(value: &mut i32) { *value = value.wrapping_add(1); }

fn append_default<T: Default>(values: &mut Vec<T>) { values.push(T::default()); }

fn change_presence<T: Default>(value: &mut Option<T>) {
    *value = if value.is_some() {
        None
    } else {
        Some(T::default())
    };
}

fn change_string(value: &mut String) { value.push('x'); }

fn change_i64(value: &mut i64) { *value = value.wrapping_add(1); }

fn bound_evidence(timestamp: u32) -> ProcessedDeploy {
    let mut seed = execution_evidence();
    seed.deploy.data.time_stamp = i64::from(timestamp);
    let envelope = Cosigned::create_single_envelope(
        seed.deploy.data.clone(),
        seed.deploy.sig_algorithm.clone(),
        construct_deploy::DEFAULT_SEC.clone(),
    )
    .unwrap();
    let mut bound = ProcessedDeploy::empty_from_cosigned(&envelope);
    bound.authority_funding_certificate = seed.authority_funding_certificate;
    bound.authority_cost_witness = seed.authority_cost_witness;
    bound.deploy_log = seed.deploy_log;
    bound.pre_state_hash = seed.pre_state_hash;
    bound.post_state_hash = seed.post_state_hash;
    bound.to_cosigned().unwrap();
    bound
}

fn assert_changed(before: &ProcessedDeploy, after: ProcessedDeploy, field: &str) {
    assert_ne!(before, &after, "mutation did not change {field}");
    assert_ne!(
        RuntimeManager::replay_payload_hash(std::slice::from_ref(before), &[], false),
        RuntimeManager::replay_payload_hash(&[after], &[], false),
        "replay payload did not bind {field}"
    );
}

fn certificate_mutations(
    certificate: &CostAuthorityFundingCertificateProto,
) -> Vec<(&'static str, CostAuthorityFundingCertificateProto)> {
    field_mutations!(certificate, CostAuthorityFundingCertificateProto, {
        protocol_version => change_u32,
        program_hash => change_bytes,
        pre_state_root => change_bytes,
        reservation_id => change_bytes,
        demand_kind => change_i32,
        demand => append_default,
        proof => change_bytes,
        unprovable_reason => change_u32,
        allocation => append_default,
        stack_reservations => append_default,
        fee_allocation => append_default,
        fee_recipient => change_bytes,
        byte_cost_schedule_version => change_u32,
        byte_cost_schedule_digest => change_bytes,
        byte_cost_bound => change_u64,
        byte_allocation => append_default,
    })
}

fn witness_mutations(
    witness: &CostAuthorityWitnessProto,
) -> Vec<(&'static str, CostAuthorityWitnessProto)> {
    field_mutations!(witness, CostAuthorityWitnessProto, {
        protocol_version => change_u32,
        certificate_id => change_bytes,
        pre_state_root => change_bytes,
        post_state_root => change_bytes,
        events => append_default,
        realized => append_default,
        settlement => append_default,
        physical_draws => append_default,
        born_stacks => append_default,
        byte_cost_schedule_version => change_u32,
        byte_cost_schedule_digest => change_bytes,
        byte_cost => change_u64,
        byte_settlement => append_default,
        byte_events => append_default,
    })
}

fn resource_at(deploy: &mut ProcessedDeploy, path: usize, resource: CostAuthorityResourceProto) {
    let certificate = deploy.authority_funding_certificate.as_mut().unwrap();
    let witness = deploy.authority_cost_witness.as_mut().unwrap();
    match path {
        0 => certificate.demand = vec![resource],
        1 => certificate.allocation = vec![resource],
        2 => certificate.fee_allocation = vec![resource],
        3 => certificate.byte_allocation = vec![resource],
        4 => witness.realized = vec![resource],
        5 => witness.settlement = vec![resource],
        6 => witness.byte_settlement = vec![resource],
        7 => {
            witness.events = vec![CostAuthorityEventProto {
                event_id: Bytes::from_static(b"event"),
                debit: vec![resource],
                authority: None,
            }]
        }
        8 => {
            witness.physical_draws = vec![CostAuthorityPhysicalEventDrawProto {
                event_id: Bytes::from_static(b"draw"),
                balances: vec![resource],
                stack_ids: Vec::new(),
            }]
        }
        _ => unreachable!(),
    }
}

fn witness_with<T: Clone>(
    before: &ProcessedDeploy,
    initial: &T,
    mutations: Vec<(&str, T)>,
    insert: impl Fn(&mut CostAuthorityWitnessProto, T),
) {
    let mut original = before.clone();
    insert(
        original.authority_cost_witness.as_mut().unwrap(),
        initial.clone(),
    );
    for (field, value) in mutations {
        let mut changed = original.clone();
        insert(changed.authority_cost_witness.as_mut().unwrap(), value);
        assert_changed(&original, changed, field);
    }
}

proptest! {
    #[test]
    fn replay_payload_binds_system_evidence_fields(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
    ) {
        let user = execution_evidence();
        let system = ProcessedSystemDeploy::Succeeded {
            event_list: user.deploy_log.clone(),
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
            pre_state_hash: bytes.into(),
            post_state_hash: user.post_state_hash.clone(),
        };
        let original = system.clone().to_proto();
        let original_hash = RuntimeManager::replay_payload_hash(&[], &[system], false);
        for (field, changed) in field_mutations!(original, ProcessedSystemDeployProto, {
            system_deploy => change_presence,
            deploy_log => |events: &mut Vec<models::casper::EventProto>| events.push(original.deploy_log[0].clone()),
            error_msg => change_string,
            pre_state_hash => change_bytes,
            post_state_hash => change_bytes,
        }) {
            let result = ProcessedSystemDeploy::from_proto(changed);
            if field == "system_deploy" {
                prop_assert!(result.is_err(), "a successful system result requires its deploy");
            } else {
                let changed_hash = RuntimeManager::replay_payload_hash(&[], &[result.unwrap()], false);
                prop_assert_ne!(&changed_hash, &original_hash, "system field {}", field);
            }
        }
    }

    #[test]
    fn replay_payload_binds_genesis_user_order_and_system_presence(timestamp in any::<u32>()) {
        let first = bound_evidence(timestamp);
        let second = bound_evidence(timestamp.wrapping_add(1));
        let users = [first, second];
        let system = ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
            pre_state_hash: Bytes::from_static(b"before"),
            post_state_hash: Bytes::from_static(b"after"),
        };
        let ordinary = RuntimeManager::replay_payload_hash(&users, &[], false);
        prop_assert_ne!(&ordinary, &RuntimeManager::replay_payload_hash(&users, &[], true));
        prop_assert_ne!(&ordinary, &RuntimeManager::replay_payload_hash(&users, &[system], false));
        let mut reversed = users.clone();
        reversed.reverse();
        prop_assert_ne!(&ordinary, &RuntimeManager::replay_payload_hash(&reversed, &[], false));
        let repeated = [users[0].clone(), users[1].clone(), users[0].clone()];
        prop_assert_ne!(&ordinary, &RuntimeManager::replay_payload_hash(&repeated, &[], false));
    }

    #[test]
    fn replay_payload_binds_or_rejects_every_processed_field(timestamp in any::<u32>()) {
        let original = bound_evidence(timestamp);
        for (field, changed) in field_mutations!(original, ProcessedDeploy, {
            deploy => |deploy: &mut Signed<DeployData>| change_string(&mut deploy.data.term),
            envelope_commitment => change_bytes,
            cost => |cost: &mut models::rhoapi::PCost| change_u64(&mut cost.cost),
            deploy_log => |events: &mut Vec<Event>| events.push(original.deploy_log[0].clone()),
            is_failed => |failed: &mut bool| *failed = !*failed,
            system_deploy_error => |error: &mut Option<String>| *error = Some("changed".to_string()),
            cosigners => append_default,
            cosigner_threshold => change_i32,
            pre_state_hash => change_bytes,
            post_state_hash => change_bytes,
            authority_funding_certificate => change_presence,
            authority_cost_witness => change_presence,
            admission_status => |status: &mut DeployAdmissionStatus| *status = DeployAdmissionStatus::Rejected,
        }) {
            if matches!(field, "deploy" | "envelope_commitment" | "cosigners" | "cosigner_threshold") {
                prop_assert!(changed.to_cosigned().is_err(), "forged envelope field {field} was accepted");
            } else {
                changed.to_cosigned().unwrap();
                assert_changed(&original, changed, field);
            }
        }
    }

    #[test]
    fn replay_envelope_rejects_every_unsigned_intent_change(timestamp in any::<u32>()) {
        let original = bound_evidence(timestamp);
        for (field, data) in field_mutations!(original.deploy.data, DeployData, {
            term => change_string,
            language => change_string,
            time_stamp => change_i64,
            valid_after_block_number => change_i64,
            shard_id => change_string,
            expiration_timestamp => |expiry: &mut Option<i64>| *expiry = Some(i64::from(timestamp) + 1),
            authority_presentations => append_default,
        }) {
            let mut changed = original.clone();
            changed.deploy.data = data;
            prop_assert!(changed.to_cosigned().is_err(), "unsigned intent field {field} was accepted");
        }
    }

    #[test]
    fn replay_payload_binds_every_certificate_field(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
        amount in any::<u64>(),
    ) {
        let mut original = execution_evidence();
        let certificate = original.authority_funding_certificate.as_mut().unwrap();
        certificate.proof = bytes.into();
        certificate.byte_cost_bound = amount;
        let mutations = certificate_mutations(certificate);
        for (field, value) in mutations {
            let mut changed = original.clone();
            changed.authority_funding_certificate = Some(value);
            assert_changed(&original, changed, field);
        }
        let mut absent = original.clone();
        absent.authority_funding_certificate = None;
        assert_changed(&original, absent, "certificate presence");
    }

    #[test]
    fn replay_payload_binds_every_witness_field(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
        amount in any::<u64>(),
    ) {
        let mut original = execution_evidence();
        let witness = original.authority_cost_witness.as_mut().unwrap();
        witness.certificate_id = bytes.into();
        witness.byte_cost = amount;
        let mutations = witness_mutations(witness);
        for (field, value) in mutations {
            let mut changed = original.clone();
            changed.authority_cost_witness = Some(value);
            assert_changed(&original, changed, field);
        }
        let mut absent = original.clone();
        absent.authority_cost_witness = None;
        assert_changed(&original, absent, "witness presence");
    }

    #[test]
    fn replay_payload_binds_nested_resource_keys_and_amounts(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
        amount in any::<u64>(),
    ) {
        let resource = CostAuthorityResourceProto { key: bytes.into(), amount };
        for path in 0..9 {
            let mut original = execution_evidence();
            resource_at(&mut original, path, resource.clone());
            for (field, value) in field_mutations!(resource, CostAuthorityResourceProto, {
                key => change_bytes,
                amount => change_u64,
            }) {
                let mut changed = original.clone();
                resource_at(&mut changed, path, value);
                assert_changed(&original, changed, &format!("resource path {path}.{field}"));
            }
        }
    }

    #[test]
    fn replay_payload_binds_nested_stack_event_and_authority_fields(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
        amount in any::<u64>(),
    ) {
        let original = execution_evidence();
        let bytes: Bytes = bytes.into();
        let reservation = CostAuthorityStackReservationProto { stack_id: bytes.clone(), pop_count: amount };
        let mut reserved = original.clone();
        reserved.authority_funding_certificate.as_mut().unwrap().stack_reservations = vec![reservation.clone()];
        for (field, value) in field_mutations!(reservation, CostAuthorityStackReservationProto, {
            stack_id => change_bytes,
            pop_count => change_u64,
        }) {
            let mut changed = reserved.clone();
            changed.authority_funding_certificate.as_mut().unwrap().stack_reservations = vec![value];
            assert_changed(&reserved, changed, field);
        }

        let event = CostAuthorityEventProto { event_id: bytes.clone(), debit: Vec::new(), authority: None };
        witness_with(&original, &event, field_mutations!(event, CostAuthorityEventProto, {
            event_id => change_bytes,
            debit => append_default,
            authority => change_presence,
        }), |witness, value| witness.events = vec![value]);

        let byte_event = CostAuthorityByteEventProto { event_id: bytes.clone(), kind: 0, authority: None, amount };
        witness_with(&original, &byte_event, field_mutations!(byte_event, CostAuthorityByteEventProto, {
            event_id => change_bytes,
            kind => change_i32,
            authority => change_presence,
            amount => change_u64,
        }), |witness, value| witness.byte_events = vec![value]);

        let draw = CostAuthorityPhysicalEventDrawProto { event_id: bytes.clone(), balances: Vec::new(), stack_ids: Vec::new() };
        witness_with(&original, &draw, field_mutations!(draw, CostAuthorityPhysicalEventDrawProto, {
            event_id => change_bytes,
            balances => append_default,
            stack_ids => append_default,
        }), |witness, value| witness.physical_draws = vec![value]);

        let born = CostAuthorityBornStackProto { stack_id: bytes.clone(), produce_hash: bytes.clone(), cells: Vec::new() };
        witness_with(&original, &born, field_mutations!(born, CostAuthorityBornStackProto, {
            stack_id => change_bytes,
            produce_hash => change_bytes,
            cells => append_default,
        }), |witness, value| witness.born_stacks = vec![value]);

        let region = CostRegion { instance_id: bytes.to_vec(), signature: None };
        for (field, value) in field_mutations!(region, CostRegion, {
            instance_id => append_default,
            signature => change_presence,
        }) {
            for byte in [false, true] {
                let initial = CostAuthority { regions: vec![region.clone()] };
                let changed = CostAuthority { regions: vec![value.clone()] };
                witness_with(&original, &initial, vec![(field, changed)], |witness, value| {
                    if byte {
                        witness.byte_events = vec![CostAuthorityByteEventProto { authority: Some(value), ..byte_event.clone() }];
                    } else {
                        witness.events = vec![CostAuthorityEventProto { authority: Some(value), ..event.clone() }];
                    }
                });
            }
        }
    }

    #[test]
    fn replay_payload_preserves_economic_event_order_and_multiplicity(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
    ) {
        let first = CostAuthorityEventProto { event_id: bytes.into(), debit: Vec::new(), authority: None };
        let mut second = first.clone();
        change_bytes(&mut second.event_id);
        let mut original = execution_evidence();
        original.authority_cost_witness.as_mut().unwrap().events = vec![first.clone(), second.clone()];
        let mut reversed = original.clone();
        reversed.authority_cost_witness.as_mut().unwrap().events.reverse();
        assert_changed(&original, reversed, "economic event order");
        let mut repeated = original.clone();
        repeated.authority_cost_witness.as_mut().unwrap().events.push(first);
        assert_changed(&original, repeated, "economic event multiplicity");
    }
}
