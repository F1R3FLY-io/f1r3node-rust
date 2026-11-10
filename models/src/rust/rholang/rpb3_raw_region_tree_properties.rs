use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::epathmap_trie_codec;
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::match_schema::ResolvedProtobufPayload;
use crate::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalFieldValue};
use crate::rust::rholang::protobuf_encoder;
use crate::rust::rholang::schema_test_adapters::schema_fixtures;

#[derive(Clone, Copy)]
struct FieldCase {
    tag: u32,
    slot: usize,
    kind: ProtobufKind,
    child_type_id: Option<u16>,
    repeated: bool,
}

#[derive(Clone)]
struct SourceNode {
    tag: u32,
    slot: usize,
    wire: ProtobufWireType,
    encoded_start: usize,
    encoded_end: usize,
    payload_start: usize,
    payload_end: usize,
    children: Option<Vec<SourceNode>>,
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn append_varint(mut value: u64, bytes: &mut Vec<u8>) {
    while value >= 128 {
        bytes.push(((value as u8) & 127) | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}

fn auxiliary_length_field(tag: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    append_varint(u64::from(tag) << 3 | 2, &mut bytes);
    append_varint(body.len() as u64, &mut bytes);
    bytes.extend_from_slice(body);
    bytes
}

fn auxiliary_gint(value: i16) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(value))),
    }])
}

fn all_field_cases(program: &'static RhoMatchProgram) -> Vec<FieldCase> {
    let mut cases = Vec::new();
    for (slot, field) in program.fields.iter().enumerate() {
        match field.kind {
            RhoMatchFieldKind::Oneof(arms) => {
                for arm in arms {
                    let resolved = resolve_protobuf_field(program, arm.tag).unwrap();
                    let child_type_id = match arm.payload {
                        RhoMatchPayloadKind::Message(type_id) => Some(type_id),
                        _ => None,
                    };
                    assert_eq!(resolved.slot(), slot);
                    cases.push(FieldCase {
                        tag: arm.tag,
                        slot,
                        kind: arm.protobuf_kind,
                        child_type_id,
                        repeated: false,
                    });
                }
            }
            kind => {
                let tag = program.protobuf_fields[slot].tag;
                let resolved = resolve_protobuf_field(program, tag).unwrap();
                let child_type_id = match kind {
                    RhoMatchFieldKind::OptionalMessage(type_id)
                    | RhoMatchFieldKind::MessageSequence(type_id) => Some(type_id),
                    _ => None,
                };
                assert_eq!(resolved.slot(), slot);
                cases.push(FieldCase {
                    tag,
                    slot,
                    kind: resolved.protobuf_kind(),
                    child_type_id,
                    repeated: resolved.repeated(),
                });
            }
        }
    }
    cases
}

fn selected_field_cases(program: &'static RhoMatchProgram, seed: &mut u64) -> Vec<FieldCase> {
    let mut selected = Vec::new();
    let cases = all_field_cases(program);
    for (slot, field) in program.fields.iter().enumerate() {
        match field.kind {
            RhoMatchFieldKind::Oneof(arms) => {
                let arm = &arms[(next(seed) as usize) % arms.len()];
                selected.push(
                    cases
                        .iter()
                        .find(|case| case.tag == arm.tag)
                        .copied()
                        .unwrap(),
                );
            }
            _ => {
                selected.push(
                    cases
                        .iter()
                        .find(|case| case.slot == slot)
                        .copied()
                        .unwrap(),
                );
            }
        }
    }
    selected
}

fn build_region(
    program: &'static RhoMatchProgram,
    depth: u8,
    seed: &mut u64,
) -> (Vec<u8>, Vec<SourceNode>) {
    let cases = selected_field_cases(program, seed);
    let forced_child_slot = if depth > 0 {
        cases
            .iter()
            .find(|case| case.child_type_id.is_some())
            .map(|case| case.slot)
    } else {
        None
    };
    let mut bytes = Vec::new();
    let mut nodes = Vec::new();
    let mut selected_slots = 0;
    for case in cases {
        let selected = Some(case.slot) == forced_child_slot || next(seed) % 3 == 0;
        if !selected || (selected_slots >= 3 && Some(case.slot) != forced_child_slot) {
            continue;
        }
        selected_slots += 1;
        let count = if case.repeated && next(seed) & 1 == 1 {
            2
        } else {
            1
        };
        for _ in 0..count {
            let wire = case.kind.wire_type().unwrap();
            let encoded_start = bytes.len();
            append_varint((u64::from(case.tag) << 3) | wire.code(), &mut bytes);
            let (payload, children) = match case.child_type_id {
                Some(type_id) => {
                    let child = wire_program_by_type_id(type_id).unwrap();
                    let (body, children) = if depth == 0 {
                        (Vec::new(), Vec::new())
                    } else {
                        build_region(child, depth - 1, seed)
                    };
                    (body, Some(children))
                }
                None => match wire {
                    ProtobufWireType::Varint => (vec![((next(seed) % 127) + 1) as u8], None),
                    ProtobufWireType::Fixed32 => (next(seed).to_le_bytes()[..4].to_vec(), None),
                    ProtobufWireType::Fixed64 => (next(seed).to_le_bytes().to_vec(), None),
                    ProtobufWireType::LengthDelimited => {
                        (vec![((next(seed) % 127) + 1) as u8], None)
                    }
                },
            };
            if wire == ProtobufWireType::LengthDelimited {
                append_varint(payload.len() as u64, &mut bytes);
            }
            let payload_start = bytes.len();
            bytes.extend_from_slice(&payload);
            nodes.push(SourceNode {
                tag: case.tag,
                slot: case.slot,
                wire,
                encoded_start,
                encoded_end: bytes.len(),
                payload_start,
                payload_end: bytes.len(),
                children,
            });
        }
    }
    (bytes, nodes)
}

fn check_region(
    program: &'static RhoMatchProgram,
    bytes: &[u8],
    source: &[u8],
    base: usize,
    nodes: &[SourceNode],
) -> usize {
    let mut cursor = CanonicalFieldCursor::in_prost_slot_order(bytes);
    let mut recursive_nodes = 0;
    let mut last_slot = None;
    let mut singular_seen = BTreeSet::new();
    for node in nodes {
        let field = cursor.next_field().unwrap().unwrap();
        let resolved = resolve_protobuf_field(program, field.number).unwrap();
        assert_eq!(field.number, node.tag);
        assert_eq!(resolved.slot(), node.slot);
        assert_eq!(resolved.protobuf_kind().wire_type(), Some(node.wire));
        assert_eq!(field.wire_type as u8, node.wire as u8);
        assert_eq!(field.encoded_range.start, node.encoded_start);
        assert_eq!(field.encoded_range.end, node.encoded_end);
        assert_eq!(field.payload_range.start, node.payload_start);
        assert_eq!(field.payload_range.end, node.payload_end);
        assert!(last_slot.is_none_or(|last| node.slot >= last));
        if !resolved.repeated() {
            assert!(singular_seen.insert(node.slot));
        }
        last_slot = Some(node.slot);
        assert_eq!(
            &source[base + node.payload_start..base + node.payload_end],
            &bytes[node.payload_start..node.payload_end]
        );
        if let Some(children) = &node.children {
            let CanonicalFieldValue::LengthDelimited(payload) = field.value else {
                panic!("generated message child has no length-delimited payload")
            };
            let child_type_id = match resolved.payload() {
                ResolvedProtobufPayload::Oneof(arm) => match arm.payload {
                    RhoMatchPayloadKind::Message(type_id) => type_id,
                    _ => panic!("selected oneof arm is not a message"),
                },
                ResolvedProtobufPayload::Declared(_) => match resolved.field().kind {
                    RhoMatchFieldKind::OptionalMessage(type_id)
                    | RhoMatchFieldKind::MessageSequence(type_id) => type_id,
                    _ => panic!("selected generated field is not a message"),
                },
            };
            let child = wire_program_by_type_id(child_type_id).unwrap();
            assert_eq!(payload.len(), node.payload_end - node.payload_start);
            recursive_nodes +=
                1 + check_region(child, payload, source, base + node.payload_start, children);
        }
    }
    assert!(cursor.next_field().unwrap().is_none());
    recursive_nodes
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn recursive_generated_regions_preserve_resolved_fields_and_source_offsets(
        root_index in 0usize..RHO_MATCH_REGISTRY.len(),
        depth in 0u8..4,
        mut seed in any::<u64>(),
    ) {
        let program = wire_program_by_type_id(RHO_MATCH_REGISTRY[root_index].1.type_id).unwrap();
        let (bytes, nodes) = build_region(program, depth, &mut seed);
        let nested = check_region(program, &bytes, &bytes, 0, &nodes);
        prop_assert!(nested <= bytes.len());
    }

    #[test]
    fn generated_pathmap_auxiliary_preserves_snapshot_source_and_refuses_bad_body(
        key in any::<i16>(),
        value in any::<i16>(),
        set_mode in any::<bool>(),
        trailing in any::<u8>(),
    ) {
        let map = if set_mode {
            EPathMap::from_set_iter([auxiliary_gint(key)], Vec::new(), false, None)
        } else {
            EPathMap::new_map(
                [(auxiliary_gint(key), auxiliary_gint(value))],
                Vec::new(), false, None,
            )
        };
        let encoded = protobuf_encoder::encode_to_vec(&map);
        let snapshot = map.trie_snapshot();
        let descriptor = resolve_protobuf_field(&EPATHMAP_RHO_MATCH_PROGRAM, 9).unwrap();
        prop_assert_eq!(descriptor.slot(), 3);
        prop_assert_eq!(descriptor.field().kind, RhoMatchFieldKind::PathMap);
        prop_assert_eq!(descriptor.protobuf_kind(), ProtobufKind::EPathMapSnapshot);

        let mut cursor = CanonicalFieldCursor::in_prost_slot_order(&encoded);
        let field = cursor.next_field().unwrap().unwrap();
        prop_assert_eq!(field.number, 9);
        let CanonicalFieldValue::LengthDelimited(body) = field.value else {
            prop_assert!(false, "generated PathMap snapshot is not length-delimited");
            unreachable!()
        };
        prop_assert_eq!(field.encoded_range.start, 0);
        prop_assert_eq!(field.encoded_range.end, encoded.len());
        prop_assert_eq!(field.payload_range.end - field.payload_range.start, snapshot.len());
        prop_assert_eq!(body, snapshot);
        prop_assert_eq!(
            &encoded[field.payload_range.start..field.payload_range.end],
            snapshot
        );
        prop_assert!(cursor.next_field().unwrap().is_none());

        let accepted = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&encoded),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        );
        prop_assert!(accepted.is_ok(), "generated PathMap rejected: {accepted:?}");
        prop_assert_eq!(accepted.unwrap().stats().epathmaps, 1);

        let mut wrong_magic = encoded.clone();
        wrong_magic[field.payload_range.start] ^= 1;
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&wrong_magic),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        ).is_err());

        let mut after_arena_len = &snapshot[6..];
        let arena_len = usize::try_from(
            prost::encoding::decode_varint(&mut after_arena_len).unwrap(),
        ).unwrap();
        let arena_start = snapshot.len() - after_arena_len.len();
        prop_assert!(arena_len >= 8);
        prop_assert_eq!(&snapshot[arena_start..arena_start + 8], b"ACTree03");
        let mut wrong_act = encoded.clone();
        wrong_act[field.payload_range.start + arena_start] = 255;
        prop_assert_eq!(
            epathmap_trie_codec::snapshot_route_hint(snapshot).unwrap(),
            epathmap_trie_codec::snapshot_route_hint(
                &wrong_act[field.payload_range.start..field.payload_range.end]
            ).unwrap()
        );
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&wrong_act),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        ).is_err());

        let mut extra_body = snapshot.to_vec();
        extra_body.push(trailing);
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::from(auxiliary_length_field(9, &extra_body)),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        ).is_err());
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::from(auxiliary_length_field(9, &[])),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        ).is_err());
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::from(Vec::new()),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        ).is_err());
    }
}

#[test]
fn every_generated_message_program_with_children_has_a_recursive_source_witness() {
    let mut message_programs = 0;
    let mut recursive_fields = 0;
    for (_, resident) in RHO_MATCH_REGISTRY {
        let program = wire_program_by_type_id(resident.type_id).unwrap();
        if !all_field_cases(program)
            .iter()
            .any(|case| case.child_type_id.is_some())
        {
            continue;
        }
        let mut seed = (1..=256u64)
            .map(|probe| (u64::from(program.type_id) << 32) | probe)
            .find(|probe| {
                let mut candidate = *probe;
                selected_field_cases(program, &mut candidate)
                    .iter()
                    .any(|case| case.child_type_id.is_some())
            })
            .unwrap();
        let (bytes, nodes) = build_region(program, 2, &mut seed);
        let nested = check_region(program, &bytes, &bytes, 0, &nodes);
        assert!(nested > 0, "{}", program.type_name);
        message_programs += 1;
        recursive_fields += nested;
    }
    assert!(message_programs > 0);
    assert!(recursive_fields >= message_programs);
}

#[test]
fn all_generated_programs_resolve_slot_order_and_singular_duplicates() {
    let mut covered = BTreeSet::new();
    let mut descending_pairs = 0;
    let mut singular_pairs = 0;
    for (_, resident) in RHO_MATCH_REGISTRY {
        let program = wire_program_by_type_id(resident.type_id).unwrap();
        covered.insert(program.type_id);
        let cases = all_field_cases(program);
        for first in &cases {
            let resolved = resolve_protobuf_field(program, first.tag).unwrap();
            assert_eq!(resolved.slot(), first.slot);
            assert_eq!(resolved.repeated(), first.repeated);
            assert_eq!(resolved.protobuf_kind().wire_type(), first.kind.wire_type());
            if !first.repeated {
                singular_pairs += 1;
            }
            descending_pairs += cases
                .iter()
                .filter(|second| second.slot < first.slot)
                .count();
        }
    }
    assert_eq!(covered.len(), RHO_MATCH_REGISTRY.len());
    assert!(descending_pairs > 0);
    assert!(singular_pairs > 0);
}

#[test]
fn accepted_single_field_fixtures_reject_reversed_slots_and_duplicate_singular_slots() {
    let mut accepted: BTreeMap<u16, Vec<(usize, bool, u32, Vec<u8>)>> = BTreeMap::new();
    for fixture in schema_fixtures() {
        let mut cursor = CanonicalFieldCursor::in_prost_slot_order(&fixture.bytes);
        let Some(field) = cursor.next_field().unwrap() else {
            continue;
        };
        if cursor.next_field().unwrap().is_some() {
            continue;
        }
        let program = wire_program_by_type_id(fixture.program.type_id).unwrap();
        if validate_canonical_protobuf(ProtobufRegion::copy_from_slice(&fixture.bytes), program)
            .is_ok()
        {
            let resolved = resolve_protobuf_field(program, field.number).unwrap();
            accepted.entry(program.type_id).or_default().push((
                resolved.slot(),
                resolved.repeated(),
                field.number,
                fixture.bytes,
            ));
        }
    }
    let mut checked_order = 0;
    let mut checked_duplicate = 0;
    for (type_id, cases) in accepted {
        let program = wire_program_by_type_id(type_id).unwrap();
        for (slot, repeated, tag, bytes) in &cases {
            if !repeated {
                let mut duplicate = bytes.clone();
                duplicate.extend_from_slice(bytes);
                let result = validate_canonical_protobuf(
                    ProtobufRegion::copy_from_slice(&duplicate),
                    program,
                );
                assert!(
                    matches!(&result, Err(CanonicalProtobufAdmissionError::Schema {
                    field,
                    detail: "canonical singular or oneof field is duplicated",
                    ..
                }) if *field == *tag),
                    "{result:?}"
                );
                checked_duplicate += 1;
            }
            for (lower_slot, _, lower_tag, lower_bytes) in &cases {
                if lower_slot >= slot {
                    continue;
                }
                let mut descending = bytes.clone();
                descending.extend_from_slice(lower_bytes);
                let result = validate_canonical_protobuf(
                    ProtobufRegion::copy_from_slice(&descending),
                    program,
                );
                assert!(
                    matches!(&result, Err(CanonicalProtobufAdmissionError::Schema {
                    field,
                    detail: "field is outside canonical Prost slot order",
                    ..
                }) if *field == *lower_tag),
                    "{result:?}"
                );
                checked_order += 1;
            }
        }
    }
    assert!(checked_order > 0);
    assert!(checked_duplicate > 0);
}
