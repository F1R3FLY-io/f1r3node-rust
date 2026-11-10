use std::collections::BTreeMap;

use proptest::prelude::*;
use prost::Message;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, New, Par};
use crate::rust::par_bag::{with_component_key, PAR_BAG_KEY_DOMAIN};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalFieldValue};
use crate::rust::rholang::{protobuf_decoder, protobuf_encoder};

struct SourceMapEntry<'a> {
    start: usize,
    end: usize,
    payload_start: usize,
    payload_end: usize,
    encoded: &'a [u8],
    body: &'a [u8],
    key: &'a str,
    value: Option<&'a [u8]>,
}

fn length_field(tag: u32, body: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::new();
    prost::encoding::encode_key(
        tag,
        prost::encoding::WireType::LengthDelimited,
        &mut encoded,
    );
    prost::encoding::encode_varint(body.len() as u64, &mut encoded);
    encoded.extend_from_slice(body);
    encoded
}

fn gint(value: i16) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(value))),
    }])
}

fn typed_par_score(value: &Par) -> Vec<u8> {
    let mut score = Vec::new();
    value
        .components()
        .append_par_score_key(value.connective_used(), &mut score);
    score
}

fn typed_new_score(value: &New) -> Vec<u8> {
    with_component_key(value, |key| {
        let trailer = key.len() - std::mem::size_of::<u64>();
        let score_len =
            usize::try_from(u64::from_be_bytes(key[trailer..].try_into().unwrap())).unwrap();
        let score_start = PAR_BAG_KEY_DOMAIN.len() + 1;
        key[score_start..score_start + score_len].to_vec()
    })
}

fn generated_new(low: i16, high: i16, root: i16, seed: u8) -> New {
    let mut injections = BTreeMap::new();
    injections.insert(format!("z{seed:02x}"), gint(high));
    injections.insert(format!("m{seed:02x}"), Par::default());
    injections.insert(format!("a{seed:02x}"), gint(low));
    New {
        p: Some(gint(root)),
        uri: vec![format!("uri{seed:02x}")],
        injections,
        ..New::default()
    }
}

fn pathmap_par(key: i16, value: i16, set_mode: bool) -> Par {
    let map = if set_mode {
        EPathMap::from_set_iter([gint(key)], Vec::new(), false, None)
    } else {
        EPathMap::new_map([(gint(key), gint(value))], Vec::new(), false, None)
    };
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }])
}

fn source_map_entries(bytes: &[u8]) -> Vec<SourceMapEntry<'_>> {
    assert_eq!(NEW_RHO_MATCH_PROGRAM.type_id, 20);
    let descriptor = resolve_protobuf_field(&NEW_RHO_MATCH_PROGRAM, 4).unwrap();
    assert_eq!(descriptor.slot(), 3);
    assert!(matches!(
        descriptor.field().kind,
        RhoMatchFieldKind::StringMessageMap(type_id)
            if type_id == PAR_PROTOBUF_REGION_PROGRAM.type_id
    ));

    let mut root = CanonicalFieldCursor::in_prost_slot_order(bytes);
    let mut entries = Vec::new();
    while let Some(field) = root.next_field().unwrap() {
        let resolved = resolve_protobuf_field(&NEW_RHO_MATCH_PROGRAM, field.number).unwrap();
        if resolved.slot() != 3 {
            continue;
        }
        assert_eq!(field.number, 4);
        let CanonicalFieldValue::LengthDelimited(body) = field.value else {
            panic!("generated New map entry must have a length-delimited body")
        };
        let mut entry = CanonicalFieldCursor::in_prost_slot_order(body);
        let key_field = entry.next_field().unwrap().expect("generated map key");
        assert_eq!(key_field.number, 1);
        let CanonicalFieldValue::LengthDelimited(key_bytes) = key_field.value else {
            panic!("generated map key must be length-delimited")
        };
        let key = std::str::from_utf8(key_bytes).unwrap();
        assert!(!key.is_empty());
        let value = entry.next_field().unwrap().map(|field| {
            assert_eq!(field.number, 2);
            let CanonicalFieldValue::LengthDelimited(value) = field.value else {
                panic!("generated map value must be length-delimited")
            };
            value
        });
        assert!(entry.next_field().unwrap().is_none());
        entries.push(SourceMapEntry {
            start: field.encoded_range.start,
            end: field.encoded_range.end,
            payload_start: field.payload_range.start,
            payload_end: field.payload_range.end,
            encoded: &bytes[field.encoded_range.start..field.encoded_range.end],
            body,
            key,
            value,
        });
    }
    entries
}

fn replace_entry(bytes: &[u8], source: &SourceMapEntry<'_>, body: &[u8]) -> Vec<u8> {
    let mut mutated = bytes[..source.start].to_vec();
    mutated.extend_from_slice(&length_field(4, body));
    mutated.extend_from_slice(&bytes[source.end..]);
    mutated
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_new_map_returns_preserve_source_order_body_and_typed_par_score(
        low in any::<i16>(),
        high in any::<i16>(),
        root in any::<i16>(),
        seed in any::<u8>(),
    ) {
        let generated = generated_new(low, high, root, seed);
        let bytes = protobuf_encoder::encode_to_vec(&generated);
        let source = source_map_entries(&bytes);
        prop_assert_eq!(source.len(), 3);
        prop_assert!(source.windows(2).all(|pair| pair[0].key < pair[1].key));
        prop_assert_eq!(source.iter().filter(|entry| entry.value.is_some()).count(), 2);
        prop_assert_eq!(source.iter().filter(|entry| entry.value.is_none()).count(), 1);

        let typed = New::decode(bytes.as_slice()).unwrap();
        let generated_decoded = protobuf_decoder::decode_new(bytes.as_slice()).unwrap();
        prop_assert_eq!(&typed, &generated);
        prop_assert_eq!(&generated_decoded, &typed);
        let typed_encoded = typed.encode_to_vec();
        prop_assert_eq!(typed_encoded.as_slice(), bytes.as_slice());

        rpb3_root_finish_trace::start();
        let admitted = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &NEW_RHO_MATCH_PROGRAM,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(admitted.is_ok(), "generated New rejected: {admitted:?}");
        let typed_root_score = typed_new_score(&typed);
        let admitted = admitted.unwrap();
        prop_assert_eq!(admitted.score_bytes(), Some(typed_root_score.as_slice()));
        prop_assert_eq!(trace.map_completes.len(), source.len());
        prop_assert_eq!(trace.new_finishes.len(), 1);
        let score_image = trace.score_arena.as_ref().unwrap();
        let transcripts = rpb3_root_finish_trace::replay_score_source(score_image);
        let finish = &trace.new_finishes[0];
        prop_assert!(finish.complete);
        prop_assert_eq!(finish.body.as_slice(), bytes.as_slice());
        prop_assert_eq!(
            transcripts[finish.score_handle.index()].as_slice(),
            typed_root_score.as_slice(),
        );
        let NewFinishInput::Complete { injections, .. } = &finish.input else {
            prop_assert!(false, "generated New lost its complete typed result");
            unreachable!()
        };
        prop_assert_eq!(injections.len(), source.len());

        for ((entry, returned), (finished_key, handle)) in
            source.iter().zip(&trace.map_completes).zip(injections)
        {
            let expected = typed.injections.get(entry.key).unwrap();
            prop_assert!(std::ptr::eq(returned.frame.owner, &NEW_RHO_MATCH_PROGRAM));
            prop_assert_eq!(returned.frame.outer_field, 4);
            prop_assert_eq!(returned.frame.value_type_id, PAR_PROTOBUF_REGION_PROGRAM.type_id);
            prop_assert_eq!(returned.frame.region_start.offset, entry.payload_start);
            prop_assert_eq!(returned.frame.region_end.offset, entry.payload_end);
            prop_assert_eq!(returned.frame.region_start.source, returned.frame.region_end.source);
            prop_assert_eq!(returned.frame.body.as_slice(), &bytes[entry.payload_start..entry.payload_end]);
            prop_assert_eq!(returned.frame.body.as_slice(), entry.body);
            prop_assert_eq!(returned.summary.key.as_slice(), entry.key.as_bytes());
            prop_assert_eq!(finished_key.as_slice(), entry.key.as_bytes());
            match (entry.value, returned.summary.value.as_ref(), handle) {
                (None, None, None) => {
                    prop_assert_eq!(expected, &Par::default());
                }
                (Some(body), Some(par), Some(handle)) => {
                    let typed_value = Par::decode(body).unwrap();
                    let generated_value = protobuf_decoder::decode_par(body).unwrap();
                    prop_assert_eq!(&typed_value, expected);
                    prop_assert_eq!(&generated_value, &typed_value);
                    prop_assert_ne!(&typed_value, &Par::default());
                    let reencoded = protobuf_encoder::encode_to_vec(&typed_value);
                    prop_assert_eq!(reencoded.as_slice(), body);
                    prop_assert_eq!(par.identity_prefix.as_slice(), body);
                    prop_assert!(par.identity_suffix.is_empty());
                    prop_assert!(!par.semantic_default);
                    prop_assert_eq!(par.score_handle, *handle);
                    let typed_score = typed_par_score(&typed_value);
                    prop_assert_eq!(transcripts[handle.index()].as_slice(), typed_score.as_slice());
                    let child = validate_canonical_protobuf(
                        ProtobufRegion::copy_from_slice(body),
                        &PAR_PROTOBUF_REGION_PROGRAM,
                    ).unwrap();
                    prop_assert_eq!(child.semantic_default_par(), Some(false));
                    prop_assert_eq!(child.score_bytes(), Some(typed_score.as_slice()));
                }
                _ => prop_assert!(false, "map value presence differs from source and typed oracle"),
            }
        }
    }

    #[test]
    fn generated_new_map_source_mutations_refuse_order_and_explicit_defaults(
        low in any::<i16>(),
        high in any::<i16>(),
        root in any::<i16>(),
        seed in any::<u8>(),
    ) {
        let generated = generated_new(low, high, root, seed);
        let bytes = protobuf_encoder::encode_to_vec(&generated);
        let source = source_map_entries(&bytes);
        prop_assert_eq!(source.len(), 3);
        prop_assert!(source[0].value.is_some());
        prop_assert!(source[1].value.is_none());
        prop_assert!(source[2].value.is_some());
        prop_assert_eq!(source[2].end, bytes.len());

        let mut permuted = bytes[..source[0].start].to_vec();
        for entry in source.iter().rev() {
            permuted.extend_from_slice(entry.encoded);
        }
        prop_assert_eq!(permuted.len(), bytes.len());
        prop_assert_eq!(&New::decode(permuted.as_slice()).unwrap(), &generated);
        let order_result = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&permuted),
            &NEW_RHO_MATCH_PROGRAM,
        );
        let order_refused_as_expected = matches!(&order_result,
            Err(CanonicalProtobufAdmissionError::Schema {
                field: 4,
                detail: "protobuf map keys are not strictly ascending",
                ..
            })
        );
        prop_assert!(order_refused_as_expected, "unexpected map-order result: {order_result:?}");

        let mut explicit_default_body = source[1].body.to_vec();
        explicit_default_body.extend_from_slice(&length_field(2, &[]));
        let explicit_default = replace_entry(&bytes, &source[1], &explicit_default_body);
        prop_assert_eq!(&New::decode(explicit_default.as_slice()).unwrap(), &generated);
        let default_result = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&explicit_default),
            &NEW_RHO_MATCH_PROGRAM,
        );
        let default_refused_as_expected = matches!(&default_result,
            Err(CanonicalProtobufAdmissionError::Schema {
                field: 2,
                detail: "explicit protobuf map value equals the omitted default",
                ..
            })
        );
        prop_assert!(default_refused_as_expected, "unexpected map-default result: {default_result:?}");

        let mut invalid_par = source[0].value.unwrap().to_vec();
        prost::encoding::encode_key(10, prost::encoding::WireType::Varint, &mut invalid_par);
        prost::encoding::encode_varint(0, &mut invalid_par);
        let mut invalid_entry = length_field(1, source[0].key.as_bytes());
        invalid_entry.extend_from_slice(&length_field(2, &invalid_par));
        let invalid_scalar = replace_entry(&bytes, &source[0], &invalid_entry);
        prop_assert_eq!(&New::decode(invalid_scalar.as_slice()).unwrap(), &generated);
        let scalar_result = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&invalid_scalar),
            &NEW_RHO_MATCH_PROGRAM,
        );
        let scalar_refused_as_expected = matches!(&scalar_result,
            Err(CanonicalProtobufAdmissionError::Schema {
                field: 10,
                detail: "explicit protobuf scalar default",
                ..
            })
        );
        prop_assert!(scalar_refused_as_expected, "unexpected scalar-default result: {scalar_result:?}");
    }

    #[test]
    fn generated_mixed_new_root_close_preserves_nested_pathmap_and_source_order(
        low in any::<i16>(),
        high in any::<i16>(),
        root in any::<i16>(),
        seed in any::<u8>(),
        set_mode in any::<bool>(),
    ) {
        let mut generated = generated_new(low, high, root, seed);
        let pathmap = pathmap_par(low, high, set_mode);
        let pathmap_key = format!("z{seed:02x}");
        generated.injections.insert(pathmap_key.clone(), pathmap.clone());
        let bytes = protobuf_encoder::encode_to_vec(&generated);
        let source = source_map_entries(&bytes);
        prop_assert_eq!(source.len(), 3);
        prop_assert!(source.windows(2).all(|pair| pair[0].key < pair[1].key));
        let typed = New::decode(bytes.as_slice()).unwrap();
        let generated_decoded = protobuf_decoder::decode_new(bytes.as_slice()).unwrap();
        prop_assert_eq!(&typed, &generated);
        prop_assert_eq!(&generated_decoded, &typed);
        let expected_score = typed_new_score(&typed);
        let typed_pathmap_score = typed_par_score(&pathmap);
        let pathmap_source = source.iter().find(|entry| entry.key == pathmap_key).unwrap();
        let nested_bytes = pathmap_source.value.unwrap();
        prop_assert_eq!(&Par::decode(nested_bytes).unwrap(), &pathmap);

        rpb3_root_finish_trace::start();
        let admitted = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &NEW_RHO_MATCH_PROGRAM,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(admitted.is_ok(), "generated nested New rejected: {admitted:?}");
        let admitted = admitted.unwrap();
        prop_assert_eq!(admitted.score_bytes(), Some(expected_score.as_slice()));
        prop_assert_eq!(trace.map_completes.len(), source.len());
        prop_assert_eq!(trace.new_finishes.len(), 1);
        prop_assert!(!trace.epathmap_events.is_empty());
        let finish = &trace.new_finishes[0];
        prop_assert!(finish.complete);
        prop_assert_eq!(finish.body.as_slice(), bytes.as_slice());
        let transcripts = rpb3_root_finish_trace::replay_score_source(
            trace.score_arena.as_ref().unwrap(),
        );
        prop_assert_eq!(
            transcripts[finish.score_handle.index()].as_slice(),
            expected_score.as_slice(),
        );
        let NewFinishInput::Complete { injections, .. } = &finish.input else {
            prop_assert!(false, "generated nested New lost complete root result");
            unreachable!()
        };
        prop_assert_eq!(injections.len(), source.len());
        for ((entry, returned), (key, handle)) in
            source.iter().zip(&trace.map_completes).zip(injections)
        {
            prop_assert_eq!(returned.summary.key.as_slice(), entry.key.as_bytes());
            prop_assert_eq!(key.as_slice(), entry.key.as_bytes());
            if entry.key == pathmap_key {
                let Some(handle) = handle else {
                    prop_assert!(false, "nested PathMap map value lost its score handle");
                    unreachable!()
                };
                let returned_par = returned.summary.value.as_ref().unwrap();
                prop_assert_eq!(returned_par.identity_prefix.as_slice(), nested_bytes);
                prop_assert_eq!(returned_par.score_handle, *handle);
                prop_assert_eq!(
                    transcripts[handle.index()].as_slice(),
                    typed_pathmap_score.as_slice(),
                );
            }
        }

        let mut reversed = bytes[..source[0].start].to_vec();
        for entry in source.iter().rev() {
            reversed.extend_from_slice(entry.encoded);
        }
        prop_assert_eq!(&New::decode(reversed.as_slice()).unwrap(), &generated);
        let refused = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&reversed),
            &NEW_RHO_MATCH_PROGRAM,
        );
        let refused_as_expected = matches!(&refused,
            Err(CanonicalProtobufAdmissionError::Schema {
                field: 4,
                detail: "protobuf map keys are not strictly ascending",
                ..
            })
        );
        prop_assert!(refused_as_expected, "unexpected map-order result: {refused:?}");
    }
}
