use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;

fn scalar_body(kind: ProtobufKind, default: bool) -> Option<Vec<u8>> {
    let body = match kind {
        ProtobufKind::Bool
        | ProtobufKind::Int32
        | ProtobufKind::Sint32
        | ProtobufKind::Uint32
        | ProtobufKind::Int64
        | ProtobufKind::Sint64
        | ProtobufKind::Uint64 => vec![u8::from(!default)],
        ProtobufKind::Sfixed32 | ProtobufKind::Fixed32 | ProtobufKind::Float => {
            vec![u8::from(!default), 0, 0, 0]
        }
        ProtobufKind::Sfixed64 | ProtobufKind::Fixed64 | ProtobufKind::Double => {
            vec![u8::from(!default), 0, 0, 0, 0, 0, 0, 0]
        }
        ProtobufKind::String
        | ProtobufKind::Bytes
        | ProtobufKind::RepeatedString
        | ProtobufKind::RepeatedBytes => {
            if default {
                Vec::new()
            } else {
                vec![b'a']
            }
        }
        _ => return None,
    };
    Some(body)
}

fn scalar_cases() -> Vec<(&'static RhoMatchProgram, u32, ProtobufKind, ScalarPresence)> {
    let mut cases = Vec::new();
    for (_, resident) in RHO_MATCH_REGISTRY {
        let program = wire_program_by_type_id(resident.type_id).unwrap();
        for (slot, field) in program.fields.iter().enumerate() {
            match field.kind {
                RhoMatchFieldKind::Oneof(arms) => {
                    for arm in arms {
                        if scalar_body(arm.protobuf_kind, false).is_some() {
                            cases.push((
                                program,
                                arm.tag,
                                arm.protobuf_kind,
                                ScalarPresence::Oneof,
                            ));
                        }
                    }
                }
                _ => {
                    let descriptor = &program.protobuf_fields[slot];
                    if scalar_body(descriptor.kind, false).is_some() {
                        let presence = if matches!(
                            field.kind,
                            RhoMatchFieldKind::StringSequence | RhoMatchFieldKind::BytesSequence
                        ) {
                            ScalarPresence::Repeated
                        } else {
                            ScalarPresence::Plain
                        };
                        cases.push((program, descriptor.tag, descriptor.kind, presence));
                    }
                }
            }
        }
    }
    cases
}

fn scalar_field_bytes(tag: u32, kind: ProtobufKind, body: &[u8]) -> Vec<u8> {
    let wire = kind.wire_type().unwrap();
    let mut bytes = Vec::new();
    prost::encoding::encode_varint((u64::from(tag) << 3) | wire.code(), &mut bytes);
    if wire == ProtobufWireType::LengthDelimited {
        prost::encoding::encode_varint(body.len() as u64, &mut bytes);
    }
    bytes.extend_from_slice(body);
    bytes
}

fn production_scalar_accepts(
    program: &'static RhoMatchProgram,
    tag: u32,
    kind: ProtobufKind,
    presence: ScalarPresence,
    body: &[u8],
) -> bool {
    let bytes = scalar_field_bytes(tag, kind, body);
    let region =
        IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), 0);
    let mut frame = MessageFrame::from_message_region(region, program).unwrap();
    let field = frame.cursor.next_field().unwrap().unwrap();
    assert_eq!(field.number, tag);
    assert_eq!(
        &bytes[field.payload_range.start..field.payload_range.end],
        body,
    );
    assert!(frame.cursor.next_field().unwrap().is_none());
    let resolved = resolve_protobuf_field(program, tag).unwrap();
    assert_eq!(resolved.protobuf_kind(), kind);
    let accepted = validate_scalar(&frame, &field, kind, presence).is_ok();
    if accepted {
        let semantic = semantic_scalar(&field, kind).unwrap();
        let first = body.first().copied().unwrap_or_default();
        let matched = match (kind, semantic) {
            (ProtobufKind::Bool, SemanticScalar::Bool(value)) => value == (first != 0),
            (ProtobufKind::Int32, SemanticScalar::I32(value))
            | (ProtobufKind::Sfixed32, SemanticScalar::I32(value)) => value == i32::from(first),
            (ProtobufKind::Sint32, SemanticScalar::I32(value)) => {
                value == if first == 0 { 0 } else { -1 }
            }
            (
                ProtobufKind::Uint32 | ProtobufKind::Fixed32 | ProtobufKind::Float,
                SemanticScalar::U32(value),
            ) => value == u32::from(first),
            (ProtobufKind::Int64 | ProtobufKind::Sfixed64, SemanticScalar::I64(value)) => {
                value == i64::from(first)
            }
            (ProtobufKind::Sint64, SemanticScalar::I64(value)) => {
                value == if first == 0 { 0 } else { -1 }
            }
            (
                ProtobufKind::Uint64 | ProtobufKind::Fixed64 | ProtobufKind::Double,
                SemanticScalar::U64(value),
            ) => value == u64::from(first),
            (
                ProtobufKind::String | ProtobufKind::RepeatedString,
                SemanticScalar::String(value),
            )
            | (ProtobufKind::Bytes | ProtobufKind::RepeatedBytes, SemanticScalar::Bytes(value)) => {
                value.bytes().eq(body.iter().copied())
            }
            _ => false,
        };
        assert!(
            matched,
            "{}.{} {kind:?} semantic scalar differs from source",
            program.type_name, tag
        );
    }
    accepted
}

#[test]
fn every_generated_scalar_source_kind_obeys_presence_and_value_grammar() {
    let cases = scalar_cases();
    let mut kinds = BTreeSet::new();
    let mut plain = 0;
    let mut present_default = 0;
    for (program, tag, kind, presence) in cases.iter().copied() {
        kinds.insert(format!("{kind:?}"));
        let valid = scalar_body(kind, false).unwrap();
        assert!(
            production_scalar_accepts(program, tag, kind, presence, &valid),
            "{}.{} {kind:?} valid source rejected",
            program.type_name,
            tag,
        );
        let default = scalar_body(kind, true).unwrap();
        let accepted = production_scalar_accepts(program, tag, kind, presence, &default);
        match presence {
            ScalarPresence::Plain => {
                plain += 1;
                assert!(
                    !accepted,
                    "{}.{} explicit default accepted",
                    program.type_name, tag
                );
            }
            ScalarPresence::Repeated | ScalarPresence::Oneof => {
                present_default += 1;
                assert!(
                    accepted,
                    "{}.{} legal default refused",
                    program.type_name, tag
                );
            }
        }
    }
    assert!(cases.len() >= 40);
    assert!(kinds.len() >= 10);
    assert!(plain > 0 && present_default > 0);
}

#[test]
fn every_generated_scalar_invalid_domain_is_refused() {
    let mut rejected = BTreeSet::new();
    for (program, tag, kind, presence) in scalar_cases() {
        let bad = match kind {
            ProtobufKind::Bool => Some(vec![2]),
            ProtobufKind::Int32 => Some(vec![0x80, 0x80, 0x80, 0x80, 0x08]),
            ProtobufKind::Sint32 | ProtobufKind::Uint32 => Some(vec![0x80, 0x80, 0x80, 0x80, 0x10]),
            ProtobufKind::String | ProtobufKind::RepeatedString => Some(vec![0xff]),
            _ => None,
        };
        if let Some(body) = bad {
            assert!(
                !production_scalar_accepts(program, tag, kind, presence, &body),
                "{}.{} {kind:?} invalid source admitted",
                program.type_name,
                tag,
            );
            rejected.insert(format!("{kind:?}"));
        }
    }
    assert!(rejected.len() >= 6);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_string_scalar_mutations_refuse_invalid_utf8(
        index in any::<usize>(),
        bad_utf8 in prop_oneof![Just(vec![0xff]), Just(vec![0xc0, 0x80]), Just(vec![0xe0, 0x80, 0x80])],
    ) {
        let cases = scalar_cases()
            .into_iter()
            .filter(|(_, _, kind, _)| {
                matches!(kind, ProtobufKind::String | ProtobufKind::RepeatedString)
            })
            .collect::<Vec<_>>();
        prop_assert!(!cases.is_empty());
        let (program, tag, kind, presence) = cases[index % cases.len()];
        prop_assert!(!production_scalar_accepts(program, tag, kind, presence, &bad_utf8));
    }
}
