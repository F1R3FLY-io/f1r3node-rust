use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalFieldValue};
use crate::rust::rholang::protobuf_encoder;

fn length_field(tag: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    prost::encoding::encode_key(tag, prost::encoding::WireType::LengthDelimited, &mut bytes);
    prost::encoding::encode_varint(payload.len() as u64, &mut bytes);
    bytes.extend_from_slice(payload);
    bytes
}

fn generated_par(component_count: usize, integer: i16, connective: bool, cache: bool) -> Par {
    let exprs = (0..component_count)
        .map(|index| Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(integer) + index as i64)),
        })
        .collect();
    let mut par = Par::default().with_exprs(exprs);
    if connective {
        par = par.with_connective_used(true);
    }
    if cache {
        par = par.with_locally_free(vec![1, 3, 5]);
    }
    par
}

fn source_par_summary(bytes: &[u8]) -> (usize, bool) {
    let mut cursor = CanonicalFieldCursor::in_prost_slot_order(bytes);
    let mut components = 0;
    let mut connective = false;
    while let Some(field) = cursor.next_field().expect("generated Par framing succeeds") {
        let resolved = resolve_protobuf_field(&PAR_PROTOBUF_REGION_PROGRAM, field.number)
            .expect("generated Par wire tag resolves");
        match resolved.field().kind {
            RhoMatchFieldKind::MessageSequence(_) => {
                assert!(matches!(
                    field.value,
                    CanonicalFieldValue::LengthDelimited(_)
                ));
                components += 1;
            }
            RhoMatchFieldKind::Bool => {
                assert_eq!(field.number, 10);
                assert!(!connective);
                assert_eq!(field.value, CanonicalFieldValue::Varint(1));
                assert_eq!(
                    &bytes[field.payload_range.start..field.payload_range.end],
                    &[1],
                );
                connective = true;
            }
            RhoMatchFieldKind::Bytes => assert_eq!(field.number, 9),
            _ => panic!("generated Par has an unexpected source field kind"),
        }
    }
    (components, connective)
}

#[test]
fn generated_par_message_sequence_steps_count_every_source_family() {
    let program = &PAR_PROTOBUF_REGION_PROGRAM;
    assert_eq!(program.type_id, 1);
    let component_tags = program
        .fields
        .iter()
        .filter_map(|field| match field.kind {
            RhoMatchFieldKind::MessageSequence(_) => Some(field.tag),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(component_tags.as_slice(), &[
        1, 2, 4, 5, 6, 7, 8, 11, 12, 13
    ]);
    let connective = resolve_protobuf_field(program, 10).unwrap();
    assert_eq!(connective.slot(), 8);
    assert_eq!(connective.field().kind, RhoMatchFieldKind::Bool);

    for tag in component_tags {
        let bytes = length_field(tag, &[]);
        let region =
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), 0);
        let mut frame = MessageFrame::from_message_region(region, program).unwrap();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let before = frame.cursor.stats();
        let field = frame.cursor.next_field().unwrap().unwrap();
        assert_eq!(field.number, tag);
        assert_eq!(frame.par_components, 0);
        let step = message_field_step(&mut frame, &mut stats, field, before);
        assert!(
            step.is_ok(),
            "Par tag {tag} failed its generated field step"
        );
        assert_eq!(frame.par_components, 1, "Par tag {tag} was not counted");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_par_admission_summary_matches_original_field_windows(
        count in 0usize..5,
        integer in any::<i16>(),
        connective in any::<bool>(),
        cache in any::<bool>(),
    ) {
        let par = generated_par(count, integer, connective, cache);
        let bytes = protobuf_encoder::encode_to_vec(&par);
        let (source_count, source_connective) = source_par_summary(&bytes);
        prop_assert_eq!(source_count, count);
        prop_assert_eq!(source_connective, connective);
        let admitted = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &PAR_PROTOBUF_REGION_PROGRAM,
        );
        prop_assert!(admitted.is_ok(), "generated Par rejected: {admitted:?}");
        prop_assert_eq!(
            admitted.unwrap().semantic_default_par(),
            Some(source_count == 0 && !source_connective),
        );
    }

    #[test]
    fn generated_par_connective_mutations_and_default_map_value_are_refused(
        count in 0usize..4,
        integer in any::<i16>(),
        key in 0x21u8..0x7f,
    ) {
        let par = generated_par(count, integer, true, false);
        let bytes = protobuf_encoder::encode_to_vec(&par);
        let mut cursor = CanonicalFieldCursor::in_prost_slot_order(&bytes);
        let mut connective_range = None;
        while let Some(field) = cursor.next_field().unwrap() {
            if field.number == 10 {
                connective_range = Some(field.payload_range);
            }
        }
        let connective_range = connective_range.expect("generated true connective is encoded");
        prop_assert_eq!(connective_range.end - connective_range.start, 1);
        prop_assert_eq!(bytes[connective_range.start], 1);
        for (replacement, expected_detail) in [
            (0, "explicit protobuf scalar default"),
            (2, "protobuf bool is not zero or one"),
        ] {
            let mut malformed = bytes.clone();
            malformed[connective_range.start] = replacement;
            let result = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&malformed),
                &PAR_PROTOBUF_REGION_PROGRAM,
            );
            let refused = matches!(
                result,
                Err(CanonicalProtobufAdmissionError::Schema {
                    field: 10, detail, ..
                }) if detail == expected_detail
            );
            prop_assert!(refused);
        }
        let mut nonminimal = bytes.clone();
        nonminimal.splice(connective_range.start..connective_range.end, [0x81, 0x00]);
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&nonminimal),
            &PAR_PROTOBUF_REGION_PROGRAM,
        ).is_err());

        let mut duplicate = bytes.clone();
        prost::encoding::encode_key(10, prost::encoding::WireType::Varint, &mut duplicate);
        prost::encoding::encode_varint(1, &mut duplicate);
        let duplicated = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&duplicate),
            &PAR_PROTOBUF_REGION_PROGRAM,
        );
        let refused = matches!(
            duplicated,
            Err(CanonicalProtobufAdmissionError::Schema {
                field: 10,
                detail: "canonical singular or oneof field is duplicated",
                ..
            })
        );
        prop_assert!(refused);

        let key_field = length_field(1, &[key]);
        let omitted = length_field(4, &key_field);
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&omitted),
            &NEW_RHO_MATCH_PROGRAM,
        ).is_ok());
        let mut nondefault_entry = key_field.clone();
        nondefault_entry.extend_from_slice(&length_field(2, &bytes));
        let explicit_nondefault = length_field(4, &nondefault_entry);
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&explicit_nondefault),
            &NEW_RHO_MATCH_PROGRAM,
        ).is_ok());
        for default_par in [
            Par::default(),
            Par::default().with_locally_free(vec![1, 3, 5]),
        ] {
            let mut entry = key_field.clone();
            entry.extend_from_slice(&length_field(2, &protobuf_encoder::encode_to_vec(&default_par)));
            let explicit_default = length_field(4, &entry);
            let rejected = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&explicit_default),
                &NEW_RHO_MATCH_PROGRAM,
            );
            let refused = matches!(
                rejected,
                Err(CanonicalProtobufAdmissionError::Schema {
                    detail: "explicit protobuf map value equals the omitted default",
                    ..
                })
            );
            prop_assert!(refused);
        }
    }
}
