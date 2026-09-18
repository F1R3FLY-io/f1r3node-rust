use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Bundle, EList, Expr, Par, Send};
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::rhoapi_ext::EPathMap;
use models::rust::rholang::match_schema_tables::{
    EXPR_RHO_MATCH_PROGRAM, PAR_PROTOBUF_REGION_PROGRAM,
};
use models::rust::rholang::protobuf_cursor::{CanonicalByteRange, ProtobufRegion};
use models::rust::rholang::protobuf_encoder;
use models::rust::rholang::protobuf_region::cut_at_par;
use models::rust::utils::new_gint_par;
use proptest::prelude::*;

proptest! {
    #[test]
    fn all_emission_modes_match_independent_typed_map_shells(
        pairs in prop::collection::vec((prop::option::of(any::<i64>()), prop::option::of(any::<i64>())), 0..24),
    ) {
        use models::rhoapi::{EMap, KeyValuePair};
        let pair = |(key, value): (Option<i64>, Option<i64>)| KeyValuePair {
            key: key.map(|v| new_gint_par(v, Vec::new(), false)),
            value: value.map(|v| new_gint_par(v, Vec::new(), false)),
        };
        let entries: Vec<_> = pairs.into_iter().chain([
            (None, None), (Some(7), None), (None, Some(9)), (Some(11), Some(13)),
        ]).map(pair).collect();
        let expr = |kvs| Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs, locally_free: vec![5], connective_used: true, remainder: None,
            })),
        };
        let original = expr(entries.clone());
        let shallow = expr(entries.iter().map(|kv| KeyValuePair {
            key: kv.key.as_ref().map(|_| Par::default()),
            value: kv.value.as_ref().map(|_| Par::default()),
        }).collect());
        let compact = expr(entries.iter().filter(|kv| kv.key.is_none() && kv.value.is_none())
            .map(|_| KeyValuePair::default()).collect());
        let original_bytes = protobuf_encoder::encode_to_vec(&original);
        let plan = cut_at_par(ProtobufRegion::from(original_bytes.clone()), &EXPR_RHO_MATCH_PROGRAM).unwrap();
        for (actual, expected) in [
            (plan.reconstructed_bytes(), original_bytes),
            (plan.shallow_bytes(), protobuf_encoder::encode_to_vec(&shallow)),
            (plan.compact_shell_bytes(), protobuf_encoder::encode_to_vec(&compact)),
        ] {
            prop_assert_eq!(actual.len(), expected.len());
            prop_assert_eq!(actual.as_ref(), expected.as_slice());
        }
    }
}

#[test]
fn all_modes_preserve_unknown_bytes_across_varint_boundaries() {
    use models::rust::rholang::match_schema_tables::SEND_RHO_MATCH_PROGRAM;
    use prost::encoding::{encode_key, encode_varint, WireType};
    for tag in [127, 128, 16_383, 16_384, 536_870_911] {
        for size in [0, 1, 127, 128, 16_383, 16_384] {
            let child = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GByteArray(vec![42; size])),
            }]);
            let original = Send {
                chan: Some(child),
                data: vec![new_gint_par(17, Vec::new(), false), Par::default()],
                persistent: true,
                ..Send::default()
            };
            let shallow = Send {
                chan: Some(Par::default()),
                data: vec![Par::default(), Par::default()],
                persistent: true,
                ..Send::default()
            };
            let compact = Send {
                persistent: true,
                ..Send::default()
            };
            let mut unknown = Vec::new();
            encode_key(tag, WireType::LengthDelimited, &mut unknown);
            encode_varint(size as u64, &mut unknown);
            unknown.extend(std::iter::repeat_n(91, size));
            let encode = |send: &Send| {
                let mut bytes = protobuf_encoder::encode_to_vec(send);
                bytes.extend_from_slice(&unknown);
                bytes
            };
            let encoded = encode(&original);
            let plan = cut_at_par(
                ProtobufRegion::from(encoded.clone()),
                &SEND_RHO_MATCH_PROGRAM,
            )
            .unwrap();
            assert_eq!(plan.reconstructed_bytes().as_ref(), encoded);
            assert_eq!(plan.shallow_bytes().as_ref(), encode(&shallow));
            assert_eq!(plan.compact_shell_bytes().as_ref(), encode(&compact));
            assert_eq!(plan.cuts().len(), 3);
        }
    }
}

#[test]
fn epathmap_shell_modes_preserve_keys_and_every_empty_value_slot() {
    for mode in [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map] {
        let gint = |value| new_gint_par(value, Vec::new(), false);
        let original = match mode {
            EPathMapMode::Empty => EPathMap::default(),
            EPathMapMode::Set => EPathMap::from_set_iter([gint(1), gint(2)], vec![3], true, None),
            EPathMapMode::Map => EPathMap::new_map(
                [
                    (gint(1), Par::default()),
                    (gint(2), gint(7)),
                    (gint(3), gint(9)),
                ],
                vec![3],
                true,
                None,
            ),
        };
        let shell = if mode == EPathMapMode::Map {
            EPathMap::new_map(
                [
                    (gint(1), Par::default()),
                    (gint(2), Par::default()),
                    (gint(3), Par::default()),
                ],
                vec![3],
                true,
                None,
            )
        } else {
            original.clone()
        };
        let expr = |map| Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        };
        let bytes = protobuf_encoder::encode_to_vec(&expr(original));
        let expected = protobuf_encoder::encode_to_vec(&expr(shell));
        let plan =
            cut_at_par(ProtobufRegion::from(bytes.clone()), &EXPR_RHO_MATCH_PROGRAM).unwrap();
        assert_eq!(plan.reconstructed_bytes().as_ref(), bytes);
        assert_eq!(plan.shallow_bytes().as_ref(), expected);
        assert_eq!(plan.compact_shell_bytes(), plan.shallow_bytes());
        assert_eq!(
            plan.cuts().len(),
            if mode == EPathMapMode::Map { 3 } else { 0 }
        );
    }
}

proptest! {
    #[test]
    fn checked_regions_match_native_slice_and_preserve_backing(
        bytes in prop::collection::vec(any::<u8>(), 0..128),
        lo in 0usize..160,
        hi in 0usize..160,
        a in any::<usize>(),
        b in any::<usize>(),
    ) {
        let root = ProtobufRegion::from(bytes.clone());
        let original_pointer = root.as_bytes().as_ptr();
        let result = root.subregion(CanonicalByteRange { start: lo, end: hi });
        let expected = bytes.get(lo..hi);
        prop_assert_eq!(result.is_some(), expected.is_some());
        if let Some(child) = result {
            prop_assert_eq!(child.as_bytes(), expected.unwrap());
            prop_assert_eq!(child.absolute_range(), lo..hi);
            prop_assert_eq!(child.as_bytes().as_ptr(), root.as_bytes()[lo..].as_ptr());
            let x = a % (child.len() + 1);
            let y = b % (child.len() + 1);
            let (start, end) = (x.min(y), x.max(y));
            let nested = child.subregion(CanonicalByteRange { start, end }).unwrap();
            let direct = root.subregion(CanonicalByteRange {
                start: lo + start,
                end: lo + end,
            }).unwrap();
            prop_assert_eq!(nested.absolute_range(), direct.absolute_range());
            prop_assert_eq!(nested.as_bytes(), direct.as_bytes());
            prop_assert_eq!(nested.as_bytes().as_ptr(), direct.as_bytes().as_ptr());
            let terminal = child.subregion(CanonicalByteRange {
                start: child.len(),
                end: child.len(),
            }).unwrap();
            prop_assert!(terminal.is_empty());
            prop_assert_eq!(terminal.absolute_range(), hi..hi);
            drop(terminal);
            drop(direct);
            drop(child);
            drop(root);
            prop_assert_eq!(nested.as_bytes(), &bytes[lo + start..lo + end]);
        } else {
            prop_assert_eq!(root.as_bytes(), bytes.as_slice());
            prop_assert_eq!(root.as_bytes().as_ptr(), original_pointer);
        }
    }

    #[test]
    fn ordered_cut_ordinals_preserve_empty_and_duplicate_children(
        values in prop::collection::vec(prop::option::of(any::<i64>()), 0..48),
    ) {
        let children: Vec<Par> = values.iter().copied()
            .chain([Some(7), None, Some(7), Some(9)]).map(|value| {
            value.map(|value| new_gint_par(value, Vec::new(), false)).unwrap_or_default()
        }).collect();
        let expected: Vec<Vec<u8>> = children.iter()
            .map(protobuf_encoder::encode_to_vec)
            .collect();
        let expr = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: children,
                locally_free: vec![5, 0],
                connective_used: false,
                remainder: None,
            })),
        };
        let encoded = protobuf_encoder::encode_to_vec(&expr);
        let root = ProtobufRegion::from(encoded.clone());
        let root_pointer = root.as_bytes().as_ptr();
        let plan = cut_at_par(root.clone(), &EXPR_RHO_MATCH_PROGRAM).unwrap();
        let reconstructed = plan.reconstructed_bytes();
        prop_assert_eq!(reconstructed.as_ref(), encoded.as_slice());
        prop_assert_eq!(plan.cuts().len(), expected.len());
        for (ordinal, (cut, bytes)) in plan.cuts().iter().zip(&expected).enumerate() {
            let region = cut.region();
            prop_assert_eq!(region.as_bytes(), bytes.as_slice());
            let range = region.absolute_range();
            prop_assert_eq!(region.as_bytes(), &encoded[range.clone()]);
            prop_assert_eq!(region.as_bytes().as_ptr(), root.as_bytes()[range.start..].as_ptr());
            let path = plan.cut_path(ordinal).unwrap();
            prop_assert_eq!(path.last().unwrap().occurrence, ordinal);
        }
        let cuts = plan.into_cuts();
        drop(root);
        prop_assert_eq!(cuts.len(), expected.len());
        for (cut, bytes) in cuts.iter().zip(&expected) {
            prop_assert_eq!(cut.region().as_bytes(), bytes.as_slice());
            prop_assert_eq!(
                cut.region().as_bytes().as_ptr() as usize,
                root_pointer as usize + cut.region().absolute_range().start
            );
        }
    }
}

#[test]
fn impossible_and_reversed_ranges_refuse_without_mutation() {
    let root = ProtobufRegion::from(vec![1, 2, 3, 4]);
    let child = root
        .subregion(CanonicalByteRange { start: 1, end: 3 })
        .unwrap();
    for (start, end) in [(1, 0), (0, 3), (usize::MAX, usize::MAX), (0, usize::MAX)] {
        assert!(child.subregion(CanonicalByteRange { start, end }).is_none());
        assert_eq!(child.as_bytes(), &[2, 3]);
        assert_eq!(child.absolute_range(), 1..3);
    }
    let empty = child
        .subregion(CanonicalByteRange { start: 2, end: 2 })
        .unwrap();
    assert_eq!(empty.absolute_range(), 3..3);
}

#[test]
fn epathmap_groups_preserve_value_order_with_surrounding_cuts() {
    let gint = |value| new_gint_par(value, Vec::new(), false);
    let maps = [
        EPathMap::default(),
        EPathMap::from_set_iter([gint(3)], Vec::new(), false, None),
        EPathMap::new_map(
            [(gint(1), gint(11)), (gint(2), Par::default())],
            Vec::new(),
            false,
            None,
        ),
        EPathMap::new_map(
            [(gint(4), gint(44)), (gint(5), gint(55))],
            Vec::new(),
            false,
            None,
        ),
    ];
    let par = Par::default()
        .with_sends(vec![Send {
            chan: Some(gint(100)),
            data: vec![Par::default()],
            ..Send::default()
        }])
        .with_exprs(
            maps.into_iter()
                .map(|map| Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(map)),
                })
                .collect(),
        )
        .with_bundles(vec![Bundle {
            body: Some(gint(200)),
            ..Bundle::default()
        }]);
    let mut expected = Vec::new();
    par.visit_encoded::<Expr>(|entry| {
        let expr = entry.decode().unwrap();
        let Some(ExprInstance::EPathmapBody(map)) = expr.expr_instance else {
            panic!("expected EPathMap")
        };
        let mode = map.mode();
        let mut values = Vec::new();
        if mode == EPathMapMode::Map {
            map.entry_trie()
                .for_each_raw_map_entry(|_, value| {
                    values.push(protobuf_encoder::encode_to_vec(value))
                })
                .unwrap();
        }
        expected.push((mode, values));
    });
    let encoded = protobuf_encoder::encode_to_vec(&par);
    let root = ProtobufRegion::from(encoded.clone());
    let plan = cut_at_par(root.clone(), &PAR_PROTOBUF_REGION_PROGRAM).unwrap();
    assert_eq!(plan.reconstructed_bytes().as_ref(), encoded.as_slice());
    assert_eq!(plan.epathmaps().len(), 4);
    assert_eq!(plan.cuts().len(), 7);
    assert_eq!(
        plan.cuts()[0].region().as_bytes(),
        protobuf_encoder::encode_to_vec(&gint(100))
    );
    assert!(plan.cuts()[1].region().is_empty());
    let mut ordinal = 2;
    for (group, (mode, values)) in plan.epathmaps().iter().zip(&expected) {
        assert_eq!(group.mode(), *mode);
        assert_eq!(group.cut_range(), ordinal..ordinal + values.len());
        for (cut, value) in plan.cuts()[group.cut_range()].iter().zip(values) {
            let region = cut.region();
            assert_eq!(region.as_bytes(), value);
            assert_eq!(
                region.as_bytes().as_ptr(),
                root.as_bytes()[region.absolute_range().start..].as_ptr()
            );
        }
        ordinal += values.len();
    }
    assert_eq!(ordinal, 6);
    assert_eq!(
        plan.cuts()[ordinal].region().as_bytes(),
        protobuf_encoder::encode_to_vec(&gint(200))
    );
    let cuts = plan.into_cuts();
    drop(root);
    for cut in cuts {
        assert_eq!(
            cut.region().as_bytes(),
            &encoded[cut.region().absolute_range()]
        );
    }
}
