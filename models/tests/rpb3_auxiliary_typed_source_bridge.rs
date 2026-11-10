use std::collections::BTreeMap;
use std::ops::Range;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, Expr, New, Par};
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_bag::{with_component_key, ParBag, PAR_BAG_KEY_DOMAIN};
use models::rust::rholang::canonical_semantics::ParComponentTag;
use models::rust::rholang::match_schema::{RhoMatchFieldKind, RhoMatchPayloadKind};
use models::rust::rholang::match_schema_tables::RHO_MATCH_REGISTRY;
use models::rust::rholang::protobuf_encoder;
use models::rust::rholang::protobuf_schema::ProtobufKind;
use proptest::prelude::*;
use prost::Message;
use rspace_plus_plus::rspace::bag::Bag;

fn generated_pathmap_tags() -> (u32, u32) {
    let (_, map_program) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(name, _)| *name == "EPathMap")
        .expect("generated EPathMap program");
    assert_eq!(map_program.type_id, 31);
    let snapshot_slot = map_program
        .fields
        .iter()
        .position(|field| field.kind == RhoMatchFieldKind::PathMap)
        .expect("generated EPathMap snapshot field");
    let snapshot_field = &map_program.fields[snapshot_slot];
    assert_eq!(snapshot_field.tag, 9);
    assert_eq!(
        map_program.protobuf_fields[snapshot_slot].kind,
        ProtobufKind::EPathMapSnapshot
    );

    let (_, expr_program) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(name, _)| *name == "Expr")
        .expect("generated Expr program");
    assert_eq!(expr_program.type_id, 24);
    let pathmap_arm = expr_program
        .fields
        .iter()
        .filter_map(|field| match field.kind {
            RhoMatchFieldKind::Oneof(arms) => Some(arms),
            _ => None,
        })
        .flatten()
        .find(|arm| {
            matches!(arm.payload, RhoMatchPayloadKind::Message(id) if id == map_program.type_id)
        })
        .expect("generated Expr EPathMap arm");
    assert_eq!(pathmap_arm.protobuf_kind, ProtobufKind::Message);
    (pathmap_arm.tag, snapshot_field.tag)
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> u64 {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*cursor).expect("complete generated varint");
        *cursor += 1;
        value |= u64::from(byte & 0x7f)
            .checked_shl(shift)
            .expect("generated varint shift fits u64");
        if byte & 0x80 == 0 {
            return value;
        }
    }
    panic!("generated varint is too long")
}

fn put_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn delimited_fields(bytes: &[u8], target: u32) -> Vec<(usize, Range<usize>)> {
    let mut cursor = 0;
    let mut selected = Vec::new();
    while cursor < bytes.len() {
        let field_start = cursor;
        let key = read_varint(bytes, &mut cursor);
        let tag = u32::try_from(key >> 3).expect("generated tag fits u32");
        match key & 7 {
            0 => {
                read_varint(bytes, &mut cursor);
            }
            2 => {
                let length = usize::try_from(read_varint(bytes, &mut cursor))
                    .expect("generated length fits usize");
                let end = cursor
                    .checked_add(length)
                    .expect("generated length does not overflow");
                assert!(end <= bytes.len());
                if tag == target {
                    selected.push((field_start, cursor..end));
                }
                cursor = end;
            }
            wire => panic!("unexpected generated wire type {wire}"),
        }
    }
    selected
}

fn delimited_field(bytes: &[u8], target: u32) -> (usize, Range<usize>) {
    let mut fields = delimited_fields(bytes, target);
    assert_eq!(fields.len(), 1);
    fields.pop().unwrap()
}

fn replace_delimited_field(bytes: &[u8], tag: u32, replacement: &[u8]) -> Vec<u8> {
    let (start, payload) = delimited_field(bytes, tag);
    let mut result = bytes[..start].to_vec();
    put_varint((u64::from(tag) << 3) | 2, &mut result);
    put_varint(u64::try_from(replacement.len()).unwrap(), &mut result);
    result.extend_from_slice(replacement);
    result.extend_from_slice(&bytes[payload.end..]);
    result
}

fn score_integer(value: i64) -> Vec<u8> {
    let mut output = vec![1, 1];
    output.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
    output
}

fn score_snapshot(snapshot: &[u8]) -> Vec<u8> {
    let mut output = vec![1, 3];
    for byte in snapshot {
        output.push(*byte);
        if *byte == 0 {
            output.push(255);
        }
    }
    output.extend_from_slice(&[0, 0]);
    output
}

fn score_string(value: &str) -> Vec<u8> {
    let mut output = vec![1, 2];
    for byte in value.bytes() {
        output.push(byte);
        if byte == 0 {
            output.push(255);
        }
    }
    output.extend_from_slice(&[0, 0]);
    output
}

fn score_node(children: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut output = vec![2];
    for child in children {
        output.extend(child);
    }
    output.push(0);
    output
}

fn score_constructor(constructor: i64, children: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    score_node(std::iter::once(score_integer(constructor)).chain(children))
}

fn pathmap_score(snapshot: &[u8], connective_used: bool) -> Vec<u8> {
    let mut output = vec![2];
    output.extend(score_integer(13));
    output.extend(score_snapshot(snapshot));
    output.extend(score_integer(i64::from(connective_used)));
    output.push(0);
    output
}

fn source_component_key(family: ParComponentTag, body: &[u8], score: &[u8]) -> Vec<u8> {
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    key.push(family.byte());
    key.extend_from_slice(score);
    key.extend_from_slice(body);
    key.extend_from_slice(&u64::try_from(score.len()).unwrap().to_be_bytes());
    key
}

fn source_expr_key(body: &[u8], score: &[u8]) -> Vec<u8> {
    source_component_key(ParComponentTag::Expr, body, score)
}

fn simple_par_score(body: &[u8], mutate_integer: bool) -> Vec<u8> {
    let typed = Par::decode(body).expect("typed Par value");
    assert_eq!(typed.encode_to_vec(), body);
    let expressions = delimited_fields(body, 5);
    let expr_score = if expressions.is_empty() {
        assert!(!mutate_integer);
        assert_eq!(typed, Par::default());
        None
    } else {
        assert_eq!(expressions.len(), 1);
        let expr_body = &body[expressions[0].1.clone()];
        let expr = Expr::decode(expr_body).expect("typed scalar Expr");
        assert_eq!(typed.count::<Expr>(&expr), 1);
        assert_eq!(expr.encode_to_vec(), expr_body);
        Some(match expr.expr_instance.expect("present scalar Expr") {
            ExprInstance::GInt(value) => {
                score_constructor(2, [score_integer(value + i64::from(mutate_integer))])
            }
            ExprInstance::GString(value) => {
                assert!(!mutate_integer);
                score_constructor(3, [score_string(&value)])
            }
            other => panic!("unexpected generated Expr {other:?}"),
        })
    };
    let mut children = Vec::new();
    if let Some(expr_score) = expr_score {
        children.push(expr_score);
    }
    children.push(score_integer(i64::from(typed.connective_used())));
    score_constructor(999, children)
}

fn source_new_score(body: &[u8], map_tag: u32, mutate_first_value: bool) -> Vec<u8> {
    let typed = New::decode(body).expect("typed New");
    assert_eq!(typed.encode_to_vec(), body);
    let (_, par_range) = delimited_field(body, 2);
    let par_body = &body[par_range];
    assert_eq!(typed.p.as_ref().unwrap(), &Par::decode(par_body).unwrap());
    assert!(typed.uri.is_empty());
    let mut children = vec![score_integer(i64::from(typed.bind_count)), score_integer(0)];
    let map_entries = delimited_fields(body, map_tag);
    assert_eq!(map_entries.len(), typed.injections.len());
    assert!(!map_entries.is_empty());
    let mut previous = None;
    for (index, (_, range)) in map_entries.iter().enumerate() {
        let entry = &body[range.clone()];
        let (_, key_range) = delimited_field(entry, 1);
        let key = std::str::from_utf8(&entry[key_range]).expect("typed map key");
        if let Some(previous) = previous {
            assert!(previous < key);
        }
        previous = Some(key);
        let (_, value_range) = delimited_field(entry, 2);
        let value_body = &entry[value_range];
        assert_eq!(
            typed.injections.get(key),
            Some(&Par::decode(value_body).unwrap())
        );
        children.push(score_node([
            score_string(key),
            simple_par_score(value_body, mutate_first_value && index == 0),
        ]));
    }
    children.push(simple_par_score(par_body, false));
    score_constructor(303, children)
}

fn epm1_value_ranges(snapshot: &[u8]) -> (u8, Range<usize>, Vec<(usize, Range<usize>)>) {
    assert!(snapshot.starts_with(b"EPM1\x01"));
    let mode = snapshot[5];
    let mut cursor = 6;
    let topology_len = usize::try_from(read_varint(snapshot, &mut cursor)).unwrap();
    let topology_start = cursor;
    cursor += topology_len;
    assert!(cursor <= snapshot.len());
    let count = usize::try_from(read_varint(snapshot, &mut cursor)).unwrap();
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let start = cursor;
        let length = usize::try_from(read_varint(snapshot, &mut cursor)).unwrap();
        let end = cursor + length;
        assert!(end <= snapshot.len());
        values.push((start, cursor..end));
        cursor = end;
    }
    assert_eq!(cursor, snapshot.len());
    (mode, topology_start..topology_start + topology_len, values)
}

fn replace_epm1_first_value(snapshot: &[u8], replacement: &[u8]) -> Vec<u8> {
    let (_, _, values) = epm1_value_ranges(snapshot);
    let (start, body) = &values[0];
    let mut result = snapshot[..*start].to_vec();
    put_varint(u64::try_from(replacement.len()).unwrap(), &mut result);
    result.extend_from_slice(replacement);
    result.extend_from_slice(&snapshot[body.end..]);
    result
}

fn scalar_par(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn string_par(value: String) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value)),
    }])
}

fn generated_new_map_tag() -> u32 {
    let (_, program) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(name, _)| *name == "New")
        .expect("generated New program");
    let (_, par_program) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(name, _)| *name == "Par")
        .expect("generated Par program");
    assert_eq!(program.type_id, 20);
    let field = program
        .fields
        .iter()
        .find(|field| matches!(field.kind, RhoMatchFieldKind::StringMessageMap(_)))
        .expect("generated StringMessageMap field");
    assert!(
        matches!(field.kind, RhoMatchFieldKind::StringMessageMap(id) if id == par_program.type_id)
    );
    assert_eq!(field.tag, 4);
    field.tag
}

fn admits(key: &[u8]) -> Result<ParBag, models::rust::par_bag::ParBagCodecError> {
    let mut raw = Bag::new();
    raw.insert(key).expect("one source key fits a bag");
    ParBag::try_from_bag(raw)
}

fn malformed_actree_snapshot() -> Vec<u8> {
    let mut snapshot = b"EPM1\x01\x02".to_vec();
    snapshot.push(25);
    snapshot.extend_from_slice(&[0; 25]);
    snapshot.push(0);
    snapshot
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_empty_pathmap_source_agrees_with_typed_decode_and_admission(
        connective_used in any::<bool>(),
        locally_free in prop::collection::vec(any::<u8>(), 0..5),
    ) {
        let (expr_tag, snapshot_tag) = generated_pathmap_tags();
        let map = EPathMap::new(Vec::<Par>::new(), locally_free.clone(), connective_used, None);
        prop_assert_eq!(map.mode(), EPathMapMode::Empty);
        let map_body = protobuf_encoder::encode_to_vec(&map);
        let (_, snapshot_range) = delimited_field(&map_body, snapshot_tag);
        let snapshot = &map_body[snapshot_range];
        prop_assert_eq!(snapshot, map.trie_snapshot());
        prop_assert_eq!(snapshot, b"EPM1\x01\x00\x00\x00");
        let typed_map = EPathMap::decode(map_body.as_slice()).expect("typed EPathMap decode");
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&typed_map), map_body.clone());
        prop_assert_eq!(typed_map.trie_snapshot(), snapshot);
        prop_assert_eq!(typed_map.connective_used, connective_used);
        prop_assert_eq!(&typed_map.locally_free, &locally_free);

        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        };
        let expr_body = protobuf_encoder::encode_to_vec(&expr);
        let (_, map_range) = delimited_field(&expr_body, expr_tag);
        prop_assert_eq!(&expr_body[map_range], map_body.as_slice());
        let typed_expr = Expr::decode(expr_body.as_slice()).expect("typed Expr decode");
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&typed_expr), expr_body.clone());

        let score = pathmap_score(snapshot, connective_used);
        let key = source_expr_key(&expr_body, &score);
        prop_assert_eq!(with_component_key(&expr, <[u8]>::to_vec), key.clone());
        let admitted = admits(&key).expect("generated empty PathMap admits");
        prop_assert_eq!(admitted.count::<Expr>(&expr), 1);

        let forged = pathmap_score(snapshot, !connective_used);
        prop_assert_ne!(&forged, &score);
        prop_assert!(admits(&source_expr_key(&expr_body, &forged)).is_err());

        let invalid_snapshot = malformed_actree_snapshot();
        let invalid_map = replace_delimited_field(&map_body, snapshot_tag, &invalid_snapshot);
        let invalid_expr = replace_delimited_field(&expr_body, expr_tag, &invalid_map);
        prop_assert!(EPathMap::decode(invalid_map.as_slice()).is_err());
        prop_assert!(Expr::decode(invalid_expr.as_slice()).is_err());
        let invalid_score = pathmap_score(&invalid_snapshot, connective_used);
        prop_assert!(admits(&source_expr_key(&invalid_expr, &invalid_score)).is_err());
    }

    #[test]
    fn generated_nonempty_pathmap_set_and_map_source_agree_with_typed_values_and_admission(
        seed in any::<u16>(),
        first_value in any::<i16>(),
        locally_free in prop::collection::vec(any::<u8>(), 0..5),
    ) {
        let (expr_tag, snapshot_tag) = generated_pathmap_tags();
        let first_key = string_par(format!("a{seed:04x}"));
        let second_key = string_par(format!("z{seed:04x}"));
        let first = scalar_par(i64::from(first_value));
        let second = scalar_par(i64::from(first_value) + 1);
        for connective_used in [false, true] {
            for mode in [EPathMapMode::Set, EPathMapMode::Map] {
                let map = match mode {
                    EPathMapMode::Set => EPathMap::from_set_iter(
                        [first_key.clone(), second_key.clone()],
                        locally_free.clone(),
                        connective_used,
                        None,
                    ),
                    EPathMapMode::Map => EPathMap::new_map(
                        [
                            (first_key.clone(), first.clone()),
                            (second_key.clone(), second.clone()),
                        ],
                        locally_free.clone(),
                        connective_used,
                        None,
                    ),
                    EPathMapMode::Empty => unreachable!(),
                };
                prop_assert_eq!(map.mode(), mode);
                prop_assert_eq!(map.len(), 2);
                let map_body = protobuf_encoder::encode_to_vec(&map);
                let (_, snapshot_range) = delimited_field(&map_body, snapshot_tag);
                let snapshot = &map_body[snapshot_range];
                prop_assert_eq!(snapshot, map.trie_snapshot());
                let (source_mode, topology, values) = epm1_value_ranges(snapshot);
                prop_assert_eq!(source_mode, mode as u8);
                prop_assert!(!topology.is_empty());
                let typed_map = EPathMap::decode(map_body.as_slice()).expect("typed nonempty EPathMap");
                prop_assert_eq!(typed_map.mode(), mode);
                prop_assert_eq!(typed_map.len(), 2);
                prop_assert_eq!(typed_map.trie_snapshot(), snapshot);
                prop_assert_eq!(typed_map.connective_used, connective_used);
                prop_assert_eq!(&typed_map.locally_free, &locally_free);
                prop_assert_eq!(protobuf_encoder::encode_to_vec(&typed_map), map_body.clone());
                if mode == EPathMapMode::Map {
                    prop_assert_eq!(values.len(), 2);
                    let mut decoded_values = values.iter().map(|(_, range)| {
                        let body = &snapshot[range.clone()];
                        let typed = Par::decode(body).expect("typed present EPM1 Par value");
                        assert_eq!(typed.encode_to_vec(), body);
                        let _ = simple_par_score(body, false);
                        body.to_vec()
                    }).collect::<Vec<_>>();
                    let mut expected_values = vec![
                        protobuf_encoder::encode_to_vec(&first),
                        protobuf_encoder::encode_to_vec(&second),
                    ];
                    decoded_values.sort();
                    expected_values.sort();
                    prop_assert_eq!(decoded_values, expected_values);
                } else {
                    prop_assert!(values.is_empty());
                }

                let expr = Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(map)),
                };
                let expr_body = protobuf_encoder::encode_to_vec(&expr);
                let (_, map_range) = delimited_field(&expr_body, expr_tag);
                prop_assert_eq!(&expr_body[map_range], map_body.as_slice());
                let typed_expr = Expr::decode(expr_body.as_slice()).expect("typed nonempty Expr");
                prop_assert_eq!(protobuf_encoder::encode_to_vec(&typed_expr), expr_body.clone());

                let score = pathmap_score(snapshot, connective_used);
                let key = source_expr_key(&expr_body, &score);
                prop_assert_eq!(with_component_key(&typed_expr, <[u8]>::to_vec), key.clone());
                prop_assert_eq!(admits(&key).expect("nonempty PathMap admits").count::<Expr>(&typed_expr), 1);
                let forged = pathmap_score(snapshot, !connective_used);
                prop_assert_ne!(&forged, &score);
                prop_assert!(admits(&source_expr_key(&expr_body, &forged)).is_err());

                let truncated = snapshot[..snapshot.len() - 1].to_vec();
                let invalid_map = replace_delimited_field(&map_body, snapshot_tag, &truncated);
                let invalid_expr = replace_delimited_field(&expr_body, expr_tag, &invalid_map);
                prop_assert!(EPathMap::decode(invalid_map.as_slice()).is_err());
                prop_assert!(Expr::decode(invalid_expr.as_slice()).is_err());
                prop_assert!(admits(&source_expr_key(
                    &invalid_expr,
                    &pathmap_score(&truncated, connective_used),
                )).is_err());

                if mode == EPathMapMode::Map {
                    let first_body = &snapshot[values[0].1.clone()];
                    let mut noncanonical_value = first_body.to_vec();
                    noncanonical_value.extend_from_slice(&[0x50, 0x00]);
                    prop_assert!(Par::decode(noncanonical_value.as_slice()).is_ok());
                    let invalid_snapshot = replace_epm1_first_value(snapshot, &noncanonical_value);
                    let invalid_map = replace_delimited_field(&map_body, snapshot_tag, &invalid_snapshot);
                    let invalid_expr = replace_delimited_field(&expr_body, expr_tag, &invalid_map);
                    prop_assert!(admits(&source_expr_key(
                        &invalid_expr,
                        &pathmap_score(&invalid_snapshot, connective_used),
                    )).is_err());
                }
            }
        }
    }

    #[test]
    fn generated_present_string_map_values_match_typed_source_score_and_admission(
        seed in any::<u16>(),
        first_value in any::<i16>(),
        second_value in any::<i16>(),
        body_value in any::<i16>(),
    ) {
        let map_tag = generated_new_map_tag();
        let mut injections = BTreeMap::new();
        injections.insert(format!("a\0{seed:04x}"), scalar_par(i64::from(first_value)));
        injections.insert(format!("z{seed:04x}"), scalar_par(i64::from(second_value)));
        let generated = New {
            p: Some(scalar_par(i64::from(body_value))),
            injections,
            ..New::default()
        };
        let body = protobuf_encoder::encode_to_vec(&generated);
        let typed = New::decode(body.as_slice()).expect("typed New with present map values");
        prop_assert_eq!(&typed, &generated);
        prop_assert_eq!(typed.encode_to_vec(), body.clone());
        let fields = delimited_fields(&body, map_tag);
        prop_assert_eq!(fields.len(), 2);
        prop_assert_eq!(fields[1].1.end, body.len());
        for (_, range) in &fields {
            let entry = &body[range.clone()];
            let (_, value_range) = delimited_field(entry, 2);
            prop_assert!(!value_range.is_empty());
            prop_assert!(Par::decode(&entry[value_range]).is_ok());
        }

        let score = source_new_score(&body, map_tag, false);
        let key = source_component_key(ParComponentTag::New, &body, &score);
        prop_assert_eq!(with_component_key(&typed, <[u8]>::to_vec), key.clone());
        prop_assert_eq!(admits(&key).expect("present StringMap admits").count::<New>(&typed), 1);
        let forged = source_new_score(&body, map_tag, true);
        prop_assert_ne!(&forged, &score);
        prop_assert!(admits(&source_component_key(ParComponentTag::New, &body, &forged)).is_err());

        let mut reversed = body[..fields[0].0].to_vec();
        reversed.extend_from_slice(&body[fields[1].0..fields[1].1.end]);
        reversed.extend_from_slice(&body[fields[0].0..fields[0].1.end]);
        prop_assert_eq!(New::decode(reversed.as_slice()).unwrap(), typed.clone());
        prop_assert!(admits(&source_component_key(ParComponentTag::New, &reversed, &score)).is_err());

        let first_entry = &body[fields[0].1.clone()];
        let malformed_entry = replace_delimited_field(first_entry, 2, &[0xff]);
        let mut malformed = body[..fields[0].0].to_vec();
        put_varint((u64::from(map_tag) << 3) | 2, &mut malformed);
        put_varint(u64::try_from(malformed_entry.len()).unwrap(), &mut malformed);
        malformed.extend_from_slice(&malformed_entry);
        malformed.extend_from_slice(&body[fields[0].1.end..]);
        prop_assert!(New::decode(malformed.as_slice()).is_err());
        prop_assert!(admits(&source_component_key(ParComponentTag::New, &malformed, &score)).is_err());
    }
}
