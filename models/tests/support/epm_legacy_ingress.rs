use std::borrow::Cow;

use pathmap::PathMap;
use proptest::prelude::*;

use super::*;
use crate::rhoapi::{EList, EPathMap, ETuple, Expr, GBigRational, GFixedPoint, Par, Var};
use crate::rust::canonical_path::{
    declared_canonical_path_metadata, decode_trie_path, encode_trie_path, prepare_legacy_trie_path,
    validate_canonical_path,
};
use crate::rust::epathmap_trie_codec::EPathMapRepr;
use crate::rust::pathmap_crate_type_mapper::{PathFrameError, PathFrames};

fn integer(n: i64) -> Par { expression(ExprInstance::GInt(n)) }
fn expression(value: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(value),
    }])
}

fn framed(keys: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for key in keys {
        bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
        bytes.extend_from_slice(key);
    }
    bytes
}

#[allow(deprecated)]
fn frozen_merge(
    parent: &mut EPathMap,
    bytes: &[u8],
) -> std::result::Result<(), prost::DecodeError> {
    if parent.mode() == crate::rust::epathmap_trie_codec::EPathMapMode::Map {
        return Err(prost::DecodeError::new(
            "EPathMap serialized_paths cannot be merged into map-mode storage",
        ));
    }
    let mut entries = EPathMap::default();
    for frame in PathFrames::new(bytes) {
        let key = frame.map_err(|fault| match fault {
            PathFrameError::LengthOverflow { .. } => {
                prost::DecodeError::new("EPathMap serialized_paths: key length overflow")
            }
            PathFrameError::TruncatedKey { .. } => {
                prost::DecodeError::new("EPathMap serialized_paths: truncated key")
            }
            PathFrameError::TruncatedLength { .. } => prost::DecodeError::new(
                "EPathMap serialized_paths: trailing bytes after the final key",
            ),
        })?;
        let value = decode_trie_path(key).map_err(|error| {
            prost::DecodeError::new(format!("EPathMap serialized_paths key: {error:?}"))
        })?;
        entries.insert_entry(value);
    }
    parent.extend_entries(&entries);
    Ok(())
}

fn same(left: &EPathMap, right: &EPathMap) {
    assert_eq!(left.mode(), right.mode());
    assert_eq!(left.trie_snapshot(), right.trie_snapshot());
    assert_eq!(left.locally_free, right.locally_free);
    assert_eq!(left.connective_used, right.connective_used);
    assert_eq!(left.remainder, right.remainder);
    let a = left.entry_trie();
    let b = right.entry_trie();
    assert_eq!(a.len(), b.len());
    assert_eq!(a.entries_stable(), b.entries_stable());
    assert_eq!(
        a.entries_reducer_eval_identity(),
        b.entries_reducer_eval_identity()
    );
    assert_eq!(a.union_locally_free(), b.union_locally_free());
    assert_eq!(a.any_connective_used(), b.any_connective_used());
}

fn check(initial: &EPathMap, bytes: &[u8]) -> Option<String> {
    let mut arena = ParConstruction::new();
    let repr = match initial.representation() {
        EPathMapRepr::Empty => EPathMapRepr::Empty,
        EPathMapRepr::Set(map) => EPathMapRepr::Set(map.clone()),
        EPathMapRepr::Map(map) => {
            let mut result = PathMap::new();
            for (key, value) in map.iter() {
                result.insert(key, arena.import_par(value).unwrap());
            }
            EPathMapRepr::Map(result)
        }
    };
    let mut planned = owned_slots::EPathMap {
        repr: arena.own_decoded_epm_repr(repr),
        locally_free: initial.locally_free.clone(),
        connective_used: initial.connective_used,
        remainder: initial.remainder.clone(),
    };
    let mut expected = initial.clone();
    let mut actual = initial.clone();
    let oracle = frozen_merge(&mut expected, bytes);
    let resident = actual.merge_serialized_paths(bytes);
    let prepared = planned.merge_serialized_paths(bytes);
    assert_eq!(oracle.as_ref().err(), resident.as_ref().err());
    assert_eq!(oracle.as_ref().err(), prepared.as_ref().err());
    let materialized = planned
        .project(&mut PublicProjection { arena: &mut arena })
        .unwrap();
    same(&actual, &expected);
    same(&materialized, &expected);
    if oracle.is_err() {
        same(&actual, initial);
    }
    oracle.err().map(|error| error.to_string())
}

fn check_key(key: &[u8]) {
    let oracle = decode_trie_path(key);
    let prepared = prepare_legacy_trie_path(key);
    match (oracle, prepared) {
        (Ok(value), Ok((bytes, metadata))) => {
            assert_eq!(bytes.as_ref(), encode_trie_path(&value));
            let (twice, _) = prepare_legacy_trie_path(&bytes).unwrap();
            assert_eq!(twice, bytes);
            assert_eq!(metadata.locally_free, value.locally_free());
            assert_eq!(metadata.connective_used, value.connective_used());
        }
        (Err(a), Err(b)) => assert_eq!(a, b),
        _ => panic!("legacy key normalization changed acceptance: {key:?}"),
    }
}

#[test]
fn accepted_legacy_escape_alias_is_refiled_under_its_normalized_key() {
    let alias = [0x0f, 7, 0x2a, 5, 0xa2, 1, 2, 0x0a, 0];
    validate_canonical_path(&alias).unwrap();
    let decoded = decode_trie_path(&alias).unwrap();
    let normalized = encode_trie_path(&decoded);
    assert_eq!(normalized, [0x0f, 0, 0]);
    assert_ne!(alias.as_slice(), normalized);
    let mut frames = Vec::from((alias.len() as u32).to_le_bytes());
    frames.extend_from_slice(&alias);
    let mut actual = EPathMap::default();
    actual.merge_serialized_paths(&frames).unwrap();
    assert!(actual.entry_trie().set_trie().contains(&normalized));
    assert!(!actual.entry_trie().set_trie().contains(alias));
    let (prepared, _) = prepare_legacy_trie_path(&alias).unwrap();
    assert!(matches!(prepared, Cow::Owned(_)));
    assert_eq!(prepared.as_ref(), normalized);
    for keys in [vec![alias.to_vec(), normalized.clone()], vec![
        normalized.clone(),
        alias.to_vec(),
    ]] {
        assert!(check(&EPathMap::default(), &framed(&keys)).is_none());
    }
}

#[test]
fn normalization_matches_every_scalar_collection_and_escape_boundary() {
    let mut corpus = vec![
        Par::default(),
        integer(i64::MIN),
        integer(i64::MAX),
        expression(ExprInstance::GBool(false)),
        expression(ExprInstance::GBool(true)),
        expression(ExprInstance::GString("a\0λ".into())),
        expression(ExprInstance::GUri("rho:test".into())),
        expression(ExprInstance::GByteArray(vec![0, 255])),
        expression(ExprInstance::GBigInt(vec![0, 0, 1])),
        expression(ExprInstance::GDouble(u64::MAX)),
        expression(ExprInstance::GBigRat(GBigRational {
            numerator: vec![0, 1],
            denominator: vec![0, 2],
        })),
        expression(ExprInstance::GFixedPoint(GFixedPoint {
            unscaled: vec![0, 1],
            scale: u32::MAX,
        })),
        expression(ExprInstance::ETupleBody(ETuple {
            ps: vec![integer(1), integer(2)],
            ..Default::default()
        })),
        Par::default().with_unforgeables(vec![crate::rhoapi::GUnforgeable {
            unf_instance: Some(crate::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
                crate::rhoapi::GPrivate {
                    id: vec![0, 255, 7],
                },
            )),
        }]),
    ];
    for list in [
        EList {
            ps: vec![Par::default()],
            ..Default::default()
        },
        EList {
            ps: vec![Par::default()],
            locally_free: vec![1],
            ..Default::default()
        },
        EList {
            ps: vec![Par::default()],
            connective_used: true,
            ..Default::default()
        },
        EList {
            ps: vec![Par::default()],
            remainder: Some(Var::default()),
            ..Default::default()
        },
    ] {
        corpus.push(expression(ExprInstance::EListBody(list)));
    }
    let list = expression(ExprInstance::EListBody(EList {
        ps: vec![Par::default()],
        ..Default::default()
    }));
    corpus.push(list.with_locally_free(vec![1]));
    corpus.push(list.with_connective_used(true));
    let mut map = PathMap::new();
    for n in 0..32 {
        map.insert(encode_trie_path(&integer(n)), integer(n + 1));
    }
    map.create_path(&[7, 2, 1]);
    let mut pathmap = EPathMap::default();
    pathmap
        .replace_decoded_representation(EPathMapRepr::Map(map))
        .unwrap();
    corpus.push(expression(ExprInstance::EPathmapBody(pathmap)));
    corpus.push(expression(ExprInstance::EPathmapBody(
        EPathMap::from_set_iter([integer(1), integer(2)], vec![], false, None),
    )));
    for par in corpus {
        let key = encode_trie_path(&par);
        check_key(&key);
        let (prepared, _) = prepare_legacy_trie_path(&key).unwrap();
        assert!(
            matches!(prepared, Cow::Borrowed(_)),
            "canonical key should be borrowed: {key:?}"
        );
        assert!(check(&EPathMap::default(), &framed(&[key])).is_none());
    }
    let mut split_alias = vec![0x0f, 7, 0x2a, 5, 0xa2, 1, 2, 0x0a, 0];
    split_alias.push(0);
    check_key(&split_alias);
    assert!(matches!(
        prepare_legacy_trie_path(&split_alias).unwrap().0,
        Cow::Borrowed(_)
    ));
}

proptest! {
    #[test]
    fn framed_ingress_matches_frozen_reader_and_key_normalization(
        entries in prop::collection::vec((any::<i64>(), prop::collection::vec(any::<u8>(), 0..20), any::<bool>()), 0..20),
        tail in prop::collection::vec(any::<u8>(), 0..6),
    ) {
        let keys: Vec<_> = entries.into_iter().map(|(n, raw, canonical)| {
            if canonical { encode_trie_path(&integer(n)) } else { raw }
        }).collect();
        for key in &keys { check_key(key); }
        let mut bytes = framed(&keys);
        bytes.extend_from_slice(&tail);
        let initial = EPathMap::from_set_iter([integer(99)], vec![8], true, Some(Var::default()));
        check(&initial, &bytes);
    }
}

#[test]
fn legacy_fields_are_atomic_mode_checked_and_metadata_preserving() {
    let mut typed_set = EPathMap::default();
    typed_set
        .replace_decoded_representation(EPathMapRepr::Set(PathMap::new()))
        .unwrap();
    let mut typed_map = EPathMap::default();
    typed_map
        .replace_decoded_representation(EPathMapRepr::Map(PathMap::new()))
        .unwrap();
    let populated_map = EPathMap::new_map(
        [(integer(1), integer(2))],
        vec![4],
        true,
        Some(Var::default()),
    );
    let populated_set = EPathMap::from_set_iter([integer(1)], vec![4], true, Some(Var::default()));
    let mut dangling = PathMap::new();
    dangling.create_path(&[7, 2, 1]);
    let mut topology_set = EPathMap::default();
    topology_set
        .replace_decoded_representation(EPathMapRepr::Set(dangling))
        .unwrap();
    for initial in [EPathMap::default(), typed_set, populated_set, topology_set] {
        assert!(check(&initial, &[]).is_none());
        let prefix = framed(&[encode_trie_path(&integer(2))]);
        assert!(check(&initial, &prefix).is_none());
        for (suffix, message) in [
            (vec![1], "trailing bytes after the final key"),
            (vec![2, 0, 0, 0, 1], "truncated key"),
            (vec![1, 0, 0, 0, 255], "key: ReservedTag(255)"),
        ] {
            let mut bytes = prefix.clone();
            bytes.extend_from_slice(&suffix);
            let error = check(&initial, &bytes).unwrap();
            assert_eq!(
                error,
                format!(
                    "failed to decode Protobuf message: EPathMap serialized_paths{}{}",
                    if message.starts_with("key:") {
                        " "
                    } else {
                        ": "
                    },
                    message
                )
            );
        }
    }
    for initial in [typed_map, populated_map] {
        for bytes in [vec![], vec![1], framed(&[vec![255]])] {
            assert_eq!(check(&initial, &bytes).unwrap(), "failed to decode Protobuf message: EPathMap serialized_paths cannot be merged into map-mode storage");
        }
    }
}

#[test]
fn split_metadata_belongs_to_outer_list_not_first_segment() {
    let mut decorated = Par::default().with_locally_free(vec![2]);
    decorated = decorated.with_connective_used(true);
    for children in [
        vec![Par::default()],
        vec![decorated.clone(), integer(2)],
        vec![integer(2), decorated],
    ] {
        let key = encode_trie_path(&expression(ExprInstance::EListBody(EList {
            ps: children,
            ..Default::default()
        })));
        let validated = validate_canonical_path(&key).unwrap();
        let declared = declared_canonical_path_metadata(&key).unwrap();
        assert_eq!(declared.stable, validated.stable);
        assert_eq!(
            declared.reducer_eval_identity,
            validated.reducer_eval_identity
        );
        assert_eq!(declared.locally_free, validated.locally_free);
        assert_eq!(declared.connective_used, validated.connective_used);
        assert!(!declared.reducer_eval_identity);
        assert!(declared.locally_free.is_empty());
        assert!(!declared.connective_used);
        assert!(check(&EPathMap::default(), &framed(&[key])).is_none());
    }
}

#[test]
fn tag_one_preserves_modes_duplicates_and_outer_metadata() {
    for mode in 0..4 {
        let mut arena = ParConstruction::new();
        let repr = match mode {
            0 => EPathMapRepr::Empty,
            1 => EPathMapRepr::Set(PathMap::new()),
            2 => EPathMapRepr::Map(PathMap::new()),
            _ => {
                let mut map = PathMap::new();
                map.insert(encode_trie_path(&integer(3)), integer(4));
                EPathMapRepr::Map(map)
            }
        };
        let mut expected = EPathMap::default();
        expected.replace_decoded_representation(repr).unwrap();
        expected.locally_free = vec![8];
        expected.connective_used = true;
        expected.remainder = Some(Var::default());
        let planned_repr = match expected.representation() {
            EPathMapRepr::Empty => EPathMapRepr::Empty,
            EPathMapRepr::Set(map) => EPathMapRepr::Set(map.clone()),
            EPathMapRepr::Map(map) => {
                let mut planned = PathMap::new();
                for (key, value) in map.iter() {
                    planned.insert(key, arena.import_par(value).unwrap());
                }
                EPathMapRepr::Map(planned)
            }
        };
        let mut actual = owned_slots::EPathMap {
            repr: arena.own_decoded_epm_repr(planned_repr),
            locally_free: expected.locally_free.clone(),
            connective_used: expected.connective_used,
            remainder: expected.remainder.clone(),
        };
        for n in [1, 1, 2] {
            let component = owned_slots::Expr {
                expr_instance: Some(owned_slots::ExprExprInstance::GInt(n)),
            }
            .plan(&mut arena)
            .unwrap();
            let mut child = ProtobufParBuilder::default();
            arena
                .insert_protobuf_component(&mut child, component)
                .unwrap();
            let result = actual.insert_protobuf_par(&mut arena, child);
            let oracle = expected.try_insert_entry(integer(n));
            match oracle {
                Ok(()) => assert!(result.is_ok()),
                Err(error) => assert_eq!(
                    result.unwrap_err().to_string(),
                    format!("failed to decode Protobuf message: EPathMap ps: {error}")
                ),
            }
        }
        let resident = actual
            .project(&mut PublicProjection { arena: &mut arena })
            .unwrap();
        same(&resident, &expected);
    }
}

#[test]
fn planned_tag_one_and_public_projection_preserve_deep_values_without_internal_finishes() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut arena = ParConstruction::new();
            let mut child = ProtobufParBuilder::default();
            for _ in 0..8192 {
                let component = owned_slots::Expr {
                    expr_instance: Some(owned_slots::ExprExprInstance::ENotBody(
                        owned_slots::ENot { p: Some(child) },
                    )),
                }
                .plan(&mut arena)
                .unwrap();
                child = ProtobufParBuilder::default();
                arena
                    .insert_protobuf_component(&mut child, component)
                    .unwrap();
            }
            let mut parent = owned_slots::EPathMap::default();
            #[cfg(feature = "protobuf-projection-experiment")]
            {
                use crate::rust::par_construction::substitution_cost::{capture, Metric};
                let (_, counts) =
                    capture(|| parent.insert_protobuf_par(&mut arena, child).unwrap());
                assert_eq!(counts.get(Metric::FinishAttempts), 0);
            }
            #[cfg(not(feature = "protobuf-projection-experiment"))]
            parent.insert_protobuf_par(&mut arena, child).unwrap();
            let result = parent
                .project(&mut PublicProjection { arena: &mut arena })
                .unwrap();
            assert_eq!(result.len(), 1);
            let key = result.entry_trie().set_trie().iter().next().unwrap().0;
            let mut lengths = vec![0usize];
            for _ in 0..8192 {
                let child = *lengths.last().unwrap();
                let not = 1 + prost::encoding::encoded_len_varint(child as u64) + child;
                let expr = 1 + prost::encoding::encoded_len_varint(not as u64) + not;
                lengths.push(1 + prost::encoding::encoded_len_varint(expr as u64) + expr);
            }
            let mut expected = vec![0x0f];
            prost::encoding::encode_varint(*lengths.last().unwrap() as u64, &mut expected);
            for &child in lengths[..8192].iter().rev() {
                let not = 1 + prost::encoding::encoded_len_varint(child as u64) + child;
                let expr = 1 + prost::encoding::encoded_len_varint(not as u64) + not;
                for (tag, length) in [(0x2a, expr), (0x2a, not), (0x0a, child)] {
                    expected.push(tag);
                    prost::encoding::encode_varint(length as u64, &mut expected);
                }
            }
            assert_eq!(key, expected);
            let empty = ProtobufParBuilder::default()
                .project(&mut PublicProjection { arena: &mut arena })
                .unwrap();
            assert!(empty.is_empty());
        })
        .unwrap()
        .join()
        .unwrap();
}
