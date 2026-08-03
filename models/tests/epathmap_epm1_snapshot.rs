use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, ESet, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path};
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::rholang::{bincode_encoder, protobuf_decoder, protobuf_encoder};
use prost::encoding::{encode_key, encode_varint, WireType};
use prost::Message;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;

fn gint(value: i64) -> Par {
    let mut par = Par::default();
    par.exprs.push(Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    });
    par
}

fn gstring(value: &str) -> Par {
    let mut par = Par::default();
    par.exprs.push(Expr {
        expr_instance: Some(ExprInstance::GString(value.to_owned())),
    });
    par
}

fn epathmap_carrier(map: EPathMap) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    }
}

fn nested_map_value_chain(depth: usize) -> EPathMap { nested_map_value_chain_from(depth, 0) }

fn nested_map_value_chain_from(depth: usize, leaf: i64) -> EPathMap {
    assert!(depth > 0);
    let mut value = gint(leaf);
    for _ in 1..depth {
        value = epathmap_carrier(EPathMap::new_map(
            [(gstring("key"), value)],
            Vec::new(),
            false,
            None,
        ));
    }
    EPathMap::new_map([(gstring("key"), value)], Vec::new(), false, None)
}

fn deep_escaped(depth: usize) -> Par {
    let mut inner = gint(1);
    for _ in 0..depth {
        inner = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![inner],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![inner],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> usize {
    let mut value = 0usize;
    let mut shift = 0usize;
    loop {
        let byte = bytes[*cursor];
        *cursor += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
        shift += 7;
    }
}

fn legacy_set_field(entry: &Par) -> Vec<u8> {
    let payload = protobuf_encoder::encode_to_vec(entry);
    let mut bytes = Vec::with_capacity(payload.len() + 12);
    encode_key(1, WireType::LengthDelimited, &mut bytes);
    encode_varint(payload.len() as u64, &mut bytes);
    bytes.extend_from_slice(&payload);
    bytes
}

#[test]
fn protobuf_field_9_is_the_canonical_epm1_snapshot() {
    let map = EPathMap::new(vec![gint(1), gint(2), gint(3)], Vec::new(), false, None);
    let snapshot = map.trie_snapshot();
    assert!(snapshot.starts_with(b"EPM1\x01\x01"));
    assert!(snapshot.windows(8).any(|window| window == b"ACTree03"));

    let bytes = protobuf_encoder::encode_to_vec(&map);
    assert_eq!(bytes[0], 9u8 << 3 | 2, "field 9 must be length-delimited");
    let mut cursor = 1usize;
    let len = read_varint(&bytes, &mut cursor);
    assert_eq!(len, snapshot.len());
    assert_eq!(&bytes[cursor..], snapshot);
    assert_eq!(bytes, map.encode_to_vec());

    let generated = protobuf_decoder::decode_epath_map(bytes.as_slice()).unwrap();
    let message = EPathMap::decode(bytes.as_slice()).unwrap();
    assert_eq!(generated, map);
    assert_eq!(message, map);
}

#[test]
fn bincode_copies_the_same_epm1_snapshot_and_both_readers_agree() {
    let map = EPathMap::new(vec![gint(4), gint(5)], Vec::new(), false, None);
    let generated = bincode_encoder::encode(&map);
    let derived = bincode::serialize(&map).unwrap();
    assert_eq!(generated, derived);

    let len = u64::from_le_bytes(generated[..8].try_into().unwrap()) as usize;
    assert_eq!(len, map.trie_snapshot().len());
    assert_eq!(&generated[8..8 + len], map.trie_snapshot());

    let derived_round: EPathMap = bincode::deserialize(&generated).unwrap();
    assert_eq!(derived_round, map);

    let mut root = Par::default();
    root.exprs.push(Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
    });
    let root_bytes = bincode_encoder::encode(&root);
    assert_eq!(Par::cold_decode(&root_bytes).unwrap(), root);
}

#[test]
fn empty_maps_use_the_mode_neutral_epm1_encoding() {
    let map = EPathMap::default();
    assert_eq!(map.mode(), EPathMapMode::Empty);
    assert_eq!(map.trie_snapshot(), b"EPM1\x01\x00\x00\x00");
    assert_eq!(protobuf_encoder::encode_to_vec(&map)[0], 9u8 << 3 | 2);
}

#[test]
fn cold_clone_family_builds_one_snapshot_and_mutation_detaches_only_the_writer() {
    let original = EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None);
    let mut sibling = original.clone();

    // Both values were cloned while the cache was cold. Warming either member
    // must publish one allocation to the whole clone family.
    let original_ptr = original.trie_snapshot().as_ptr();
    assert_eq!(sibling.trie_snapshot().as_ptr(), original_ptr);
    assert_eq!(original.trie_snapshot().as_ptr(), original_ptr);

    // Copy-on-write mutation replaces the writer's cache cell. The untouched
    // sibling retains both the original PathMap root and its EPM1 snapshot.
    sibling.insert_entry(gint(3));
    assert_ne!(sibling.trie_snapshot(), original.trie_snapshot());
    assert_ne!(sibling.trie_snapshot().as_ptr(), original_ptr);
    assert_eq!(original.trie_snapshot().as_ptr(), original_ptr);
}

#[test]
fn live_store_preserves_map_mode_and_key_value_associations_on_both_surfaces() {
    let map = EPathMap::new_map(
        vec![(gstring("k1"), gint(11)), (gstring("k2"), gint(22))],
        Vec::new(),
        false,
        None,
    );
    assert_eq!(map.mode(), EPathMapMode::Map);
    assert!(map.trie_snapshot().starts_with(b"EPM1\x01\x02"));
    assert_eq!(map.get_map_value(&gstring("k1")).unwrap(), Some(&gint(11)));
    assert_eq!(map.get_map_value(&gstring("k2")).unwrap(), Some(&gint(22)));

    let protobuf = protobuf_encoder::encode_to_vec(&map);
    assert_eq!(protobuf, map.encode_to_vec());
    let generated_protobuf = protobuf_decoder::decode_epath_map(protobuf.as_slice()).unwrap();
    let message_protobuf = EPathMap::decode(protobuf.as_slice()).unwrap();
    for decoded in [&generated_protobuf, &message_protobuf] {
        assert_eq!(decoded.mode(), EPathMapMode::Map);
        assert_eq!(decoded.trie_snapshot(), map.trie_snapshot());
        assert_eq!(
            decoded.get_map_value(&gstring("k1")).unwrap(),
            Some(&gint(11))
        );
        assert_eq!(
            decoded.get_map_value(&gstring("k2")).unwrap(),
            Some(&gint(22))
        );
    }

    let generated_bincode = bincode_encoder::encode(&map);
    let serde_bincode = bincode::serialize(&map).unwrap();
    assert_eq!(generated_bincode, serde_bincode);
    let decoded_bincode: EPathMap = bincode::deserialize(&generated_bincode).unwrap();
    assert_eq!(decoded_bincode.mode(), EPathMapMode::Map);
    assert_eq!(decoded_bincode.trie_snapshot(), map.trie_snapshot());
    assert_eq!(
        decoded_bincode.get_map_value(&gstring("k2")).unwrap(),
        Some(&gint(22))
    );
}

#[test]
fn neutral_empty_selects_once_and_mixed_membership_is_rejected() {
    let mut set = EPathMap::default();
    set.try_insert_entry(gint(1)).unwrap();
    assert_eq!(set.mode(), EPathMapMode::Set);
    assert!(set.insert_map_entry(gstring("k"), gint(2)).is_err());

    let mut map = EPathMap::default();
    map.insert_map_entry(gstring("k"), gint(2)).unwrap();
    assert_eq!(map.mode(), EPathMapMode::Map);
    assert!(map.try_insert_entry(gint(1)).is_err());
    assert_eq!(
        map.remove_greatest_map_entry().unwrap(),
        Some((gstring("k"), gint(2)))
    );
    assert_eq!(map.mode(), EPathMapMode::Empty);
    map.try_insert_entry(gint(3)).unwrap();
    assert_eq!(map.mode(), EPathMapMode::Set);

    let empty_map_constructor = EPathMap::new_map(Vec::new(), Vec::new(), false, None);
    assert_eq!(empty_map_constructor.mode(), EPathMapMode::Empty);
}

#[test]
fn value_bearing_map_is_a_lossless_nested_canonical_path_segment() {
    let nested = EPathMap::new_map(vec![(gstring("key"), gint(7))], Vec::new(), false, None);
    let mut carrier = Par::default();
    carrier.exprs.push(Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(nested)),
    });

    let key = encode_trie_path(&carrier);
    assert_eq!(key[0], models::rust::canonical_path::tag::EPATHMAP_MAP);
    let mut snapshot_at = 1usize;
    let snapshot_len = read_varint(&key, &mut snapshot_at);
    let snapshot = &key[snapshot_at..snapshot_at + snapshot_len];
    let decoded_snapshot = models::rust::epathmap_trie_codec::decode(snapshot)
        .unwrap_or_else(|error| panic!("{error}; snapshot={snapshot:02x?}; key={key:02x?}"));
    assert_eq!(decoded_snapshot.mode(), EPathMapMode::Map);
    let decoded = decode_trie_path(&key).unwrap();
    let ExprInstance::EPathmapBody(decoded_map) = decoded.exprs[0]
        .expr_instance
        .as_ref()
        .expect("carrier has an expression")
    else {
        panic!("canonical path changed the nested map carrier")
    };
    assert_eq!(decoded_map.mode(), EPathMapMode::Map);
    assert_eq!(
        decoded_map.get_map_value(&gstring("key")).unwrap(),
        Some(&gint(7))
    );
    assert_eq!(encode_trie_path(&decoded), key);
}

#[test]
fn set_snapshot_is_independent_of_insertion_order_and_multiplicity_at_scale() {
    let forward = EPathMap::new(
        (0..2_000).map(gint).collect::<Vec<_>>(),
        Vec::new(),
        false,
        None,
    );
    let reverse_with_duplicates = EPathMap::new(
        (0..2_000)
            .rev()
            .chain([17, 31, 17, 31])
            .map(gint)
            .collect::<Vec<_>>(),
        Vec::new(),
        false,
        None,
    );
    assert_eq!(forward.len(), 2_000);
    assert_eq!(reverse_with_duplicates.len(), 2_000);
    assert_eq!(
        forward.trie_snapshot(),
        reverse_with_duplicates.trie_snapshot()
    );
    assert_eq!(
        protobuf_encoder::encode_to_vec(&forward),
        protobuf_encoder::encode_to_vec(&reverse_with_duplicates)
    );
    assert_eq!(
        bincode_encoder::encode(&forward),
        bincode_encoder::encode(&reverse_with_duplicates)
    );
}

#[test]
fn map_snapshot_preserves_values_across_compact_line_branch_and_dense_shapes() {
    let entries = (0..512)
        .map(|index| (gstring(&format!("key-{index:04}")), gint(index)))
        .collect::<Vec<_>>();
    let map = EPathMap::new_map(entries, Vec::new(), false, None);
    let snapshot = map.trie_snapshot().to_vec();

    let protobuf = protobuf_encoder::encode_to_vec(&map);
    let decoded = protobuf_decoder::decode_epath_map(protobuf.as_slice())
        .expect("the generated protobuf reader must decode the compact map snapshot");
    assert_eq!(decoded.trie_snapshot(), &snapshot);
    for index in [0, 1, 15, 127, 255, 511] {
        assert_eq!(
            decoded
                .get_map_value(&gstring(&format!("key-{index:04}")))
                .expect("the decoded value must remain in map mode"),
            Some(&gint(index))
        );
    }

    let bincode = bincode_encoder::encode(&map);
    let decoded: EPathMap = bincode::deserialize(&bincode)
        .expect("the bincode reader must decode the same compact map snapshot");
    assert_eq!(decoded.trie_snapshot(), &snapshot);
}

#[test]
fn epm1_round_trip_is_stack_safe_at_depth_4096_on_a_256_kib_stack() {
    const STACK: usize = 256 * 1024;
    std::thread::Builder::new()
        .name("epm1-depth-4096".to_owned())
        .stack_size(STACK)
        .spawn(|| {
            let map = EPathMap::new(vec![deep_escaped(4_096)], Vec::new(), false, None);
            assert_eq!(map.len(), 1, "the trie contains the deep entry");
            let snapshot = map.trie_snapshot().to_vec();

            let protobuf = protobuf_encoder::encode_to_vec(&map);
            let protobuf_round = protobuf_decoder::decode_epath_map(protobuf.as_slice())
                .expect("depth-4096 protobuf EPM1 round trip");
            assert_eq!(protobuf_round.trie_snapshot(), &snapshot);

            let bincode = bincode_encoder::encode(&map);
            let bincode_round: EPathMap =
                bincode::deserialize(&bincode).expect("depth-4096 bincode EPM1 round trip");
            assert_eq!(bincode_round.trie_snapshot(), &snapshot);
        })
        .expect("spawn the fixed small-stack EPM1 probe")
        .join()
        .expect("EPM1 must not consume native stack in proportion to term depth");
}

#[test]
fn nested_map_value_snapshots_stream_on_a_256_kib_stack() {
    const DEPTH: usize = 4_096;
    const STACK: usize = 256 * 1024;
    std::thread::Builder::new()
        .name("epm1-map-values-depth-4096".to_owned())
        .stack_size(STACK)
        .spawn(|| {
            let map = nested_map_value_chain(DEPTH);
            let snapshot = map.trie_snapshot().to_vec();

            let protobuf = protobuf_encoder::encode_to_vec(&map);
            assert_eq!(protobuf, map.encode_to_vec());
            let generated = protobuf_decoder::decode_epath_map(protobuf.as_slice())
                .expect("generated decoder accepts nested PathMap<Par> value snapshots");
            assert_eq!(generated.trie_snapshot(), snapshot);
            let message = EPathMap::decode(protobuf.as_slice())
                .expect("Message decoder accepts nested PathMap<Par> value snapshots");
            assert_eq!(message.trie_snapshot(), snapshot);

            let bincode = bincode_encoder::encode(&map);
            let bincode_round: EPathMap = bincode::deserialize(&bincode)
                .expect("bincode accepts nested PathMap<Par> value snapshots");
            assert_eq!(bincode_round.trie_snapshot(), snapshot);
        })
        .expect("spawn the fixed small-stack nested map-value probe")
        .join()
        .expect("nested PathMap<Par> snapshots must not consume native stack by depth");
}

#[test]
fn nested_map_value_term_operations_are_stack_safe_on_a_256_kib_stack() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    const DEPTH: usize = 4_096;
    const STACK: usize = 256 * 1024;
    std::thread::Builder::new()
        .name("epathmap-term-ops-depth-4096".to_owned())
        .stack_size(STACK)
        .spawn(|| {
            let left = nested_map_value_chain_from(DEPTH, 0);
            let equal = nested_map_value_chain_from(DEPTH, 0);
            let different = nested_map_value_chain_from(DEPTH, 1);

            assert!(left == equal);
            assert!(left != different);
            assert_eq!(left.cmp(&equal), std::cmp::Ordering::Equal);
            assert_ne!(left.cmp(&different), std::cmp::Ordering::Equal);

            let mut left_hash = DefaultHasher::new();
            left.hash(&mut left_hash);
            let mut equal_hash = DefaultHasher::new();
            equal.hash(&mut equal_hash);
            assert_eq!(left_hash.finish(), equal_hash.finish());
        })
        .expect("spawn the fixed small-stack term-operation probe")
        .join()
        .expect("EPathMap Eq, Hash, Ord, and teardown must not consume native stack by depth");
}

#[test]
fn legacy_tag_1_uses_the_generated_decoder_at_depth_4096_on_a_256_kib_stack() {
    const STACK: usize = 256 * 1024;
    std::thread::Builder::new()
        .name("epathmap-legacy-tag-1-depth-4096".to_owned())
        .stack_size(STACK)
        .spawn(|| {
            let bytes = legacy_set_field(&deep_escaped(4_096));
            let generated = protobuf_decoder::decode_epath_map(bytes.as_slice())
                .expect("generated EPathMap decoder accepts deep legacy tag 1");
            let message = EPathMap::decode(bytes.as_slice())
                .expect("Message EPathMap decoder accepts deep legacy tag 1");
            assert_eq!(generated.mode(), EPathMapMode::Set);
            assert_eq!(generated.len(), 1);
            assert_eq!(generated.trie_snapshot(), message.trie_snapshot());
        })
        .expect("spawn the fixed small-stack legacy protobuf probe")
        .join()
        .expect("legacy tag 1 must not consume native stack in proportion to term depth");
}

#[test]
fn deeply_nested_unknown_groups_are_skipped_iteratively_by_both_epathmap_readers() {
    const DEPTH: usize = 16_384;
    const STACK: usize = 256 * 1024;
    std::thread::Builder::new()
        .name("epathmap-unknown-groups-depth-16384".to_owned())
        .stack_size(STACK)
        .spawn(|| {
            let expected = EPathMap::default();
            let canonical = protobuf_encoder::encode_to_vec(&expected);
            let mut bytes = Vec::with_capacity(canonical.len() + DEPTH * 2);
            for _ in 0..DEPTH {
                encode_key(15, WireType::StartGroup, &mut bytes);
            }
            for _ in 0..DEPTH {
                encode_key(15, WireType::EndGroup, &mut bytes);
            }
            bytes.extend_from_slice(&canonical);

            let generated = protobuf_decoder::decode_epath_map(bytes.as_slice())
                .expect("generated EPathMap decoder skips nested unknown groups");
            let message = EPathMap::decode(bytes.as_slice())
                .expect("Message EPathMap decoder skips nested unknown groups");
            assert_eq!(generated, expected);
            assert_eq!(message, expected);
        })
        .expect("spawn the fixed small-stack unknown-group probe")
        .join()
        .expect("unknown groups must not consume native stack in proportion to nesting");
}

#[test]
fn protobuf_rejects_set_and_map_storage_in_either_field_order() {
    let map = EPathMap::new_map([(gstring("key"), gint(7))], Vec::new(), false, None);
    let map_field = protobuf_encoder::encode_to_vec(&map);
    let set_field = legacy_set_field(&gint(1));

    for bytes in [
        [map_field.as_slice(), set_field.as_slice()].concat(),
        [set_field.as_slice(), map_field.as_slice()].concat(),
    ] {
        assert!(protobuf_decoder::decode_epath_map(bytes.as_slice()).is_err());
        assert!(EPathMap::decode(bytes.as_slice()).is_err());
    }
}
