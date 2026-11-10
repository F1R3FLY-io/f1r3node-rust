use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::par_bag::{with_component_key, ParBag, PAR_BAG_KEY_DOMAIN};
use models::rust::rholang::canonical_semantics::ParComponentTag;
use models::rust::rholang::protobuf_encoder;
use proptest::prelude::*;
use prost::Message;
use rspace_plus_plus::rspace::bag::Bag;

type DecodeResult<T> = Result<T, &'static str>;

struct RawNode {
    value: Option<(u64, usize)>,
    end: usize,
    edge: RawEdge,
}

enum RawEdge {
    Branch {
        first: Option<usize>,
        bytes: Vec<u8>,
    },
    Line {
        child: Option<usize>,
        suffix: Vec<u8>,
    },
}

struct SemanticNode {
    value: Option<(u64, usize)>,
    suffix: Vec<u8>,
    children: Vec<(u8, usize)>,
}

struct SourceSnapshot {
    mode: u8,
    keys: Vec<Vec<u8>>,
    bodies: Vec<Vec<u8>>,
    first_value_offset: usize,
    arena_start: usize,
    arena_end: usize,
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> DecodeResult<&'a [u8]> {
    let end = cursor.checked_add(len).ok_or("length overflow")?;
    let part = bytes.get(*cursor..end).ok_or("truncated region")?;
    *cursor = end;
    Ok(part)
}

fn protobuf_varint(bytes: &[u8], cursor: &mut usize) -> DecodeResult<u64> {
    let mut value = 0u64;
    for index in 0..10 {
        let byte = *take(bytes, cursor, 1)?.first().ok_or("truncated varint")?;
        if index == 9 && byte > 1 {
            return Err("protobuf varint overflow");
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            if index > 0 && byte == 0 {
                return Err("noncanonical protobuf varint");
            }
            return Ok(value);
        }
    }
    Err("unterminated protobuf varint")
}

fn act_varint(bytes: &[u8], cursor: &mut usize) -> DecodeResult<u64> {
    let first = take(bytes, cursor, 1)?[0];
    if first <= 247 {
        return Ok(u64::from(first));
    }
    let len = usize::from(first - 247);
    if len > 8 {
        return Err("ACT integer width");
    }
    let raw = take(bytes, cursor, len)?;
    if raw[len - 1] == 0 {
        return Err("noncanonical ACT integer");
    }
    let mut word = [0u8; 8];
    word[..len].copy_from_slice(raw);
    let value = u64::from_le_bytes(word);
    if value <= 247 {
        return Err("noncanonical ACT integer width");
    }
    Ok(value)
}

fn backward(id: usize, distance: u64) -> DecodeResult<usize> {
    let distance = usize::try_from(distance).map_err(|_| "ACT offset overflow")?;
    let target = id.checked_sub(distance).ok_or("ACT offset underflow")?;
    if distance == 0 || target < 16 {
        return Err("ACT offset outside node region");
    }
    Ok(target)
}

fn raw_node(arena: &[u8], id: usize) -> DecodeResult<RawNode> {
    let trailer_start = arena.len().checked_sub(8).ok_or("missing ACT trailer")?;
    if !(16..trailer_start).contains(&id) {
        return Err("ACT node outside object region");
    }
    let mut cursor = id;
    let head = take(arena, &mut cursor, 1)?[0];
    let value = if head & 0x40 != 0 {
        let offset = cursor;
        Some((act_varint(arena, &mut cursor)?, offset))
    } else {
        None
    };
    let edge = if head & 0x80 == 0 {
        let count = usize::from(head & 0x3f);
        if count > 32 {
            return Err("ACT branch count");
        }
        let first = if count == 0 {
            None
        } else {
            Some(backward(id, act_varint(arena, &mut cursor)?)?)
        };
        let bytes = if count < 32 {
            let bytes = take(arena, &mut cursor, count)?.to_vec();
            if bytes.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err("ACT branch order");
            }
            bytes
        } else {
            let mask = take(arena, &mut cursor, 32)?;
            let mut bytes = Vec::new();
            for (word, bits) in mask.iter().enumerate() {
                for bit in 0..8u8 {
                    if bits & (1 << bit) != 0 {
                        bytes.push((word * 8 + usize::from(bit)) as u8);
                    }
                }
            }
            if bytes.len() < 32 {
                return Err("ACT dense branch count");
            }
            bytes
        };
        RawEdge::Branch { first, bytes }
    } else {
        if head & 0x3e != 0 {
            return Err("ACT line reserved bits");
        }
        let child = if head & 1 != 0 {
            Some(backward(id, act_varint(arena, &mut cursor)?)?)
        } else {
            None
        };
        let line_id = backward(id, act_varint(arena, &mut cursor)?)?;
        let mut line_cursor = line_id;
        let len = usize::try_from(act_varint(arena, &mut line_cursor)?)
            .map_err(|_| "ACT line length overflow")?;
        if len == 0 {
            return Err("empty ACT line");
        }
        let suffix = take(arena, &mut line_cursor, len)?.to_vec();
        if line_cursor > id {
            return Err("ACT line overlaps node");
        }
        RawEdge::Line { child, suffix }
    };
    if cursor > trailer_start {
        return Err("ACT node overlaps trailer");
    }
    Ok(RawNode {
        value,
        end: cursor,
        edge,
    })
}

fn branch_children(
    arena: &[u8],
    first: Option<usize>,
    bytes: Vec<u8>,
) -> DecodeResult<Vec<(u8, usize)>> {
    let mut next = first;
    let mut children = Vec::with_capacity(bytes.len());
    for byte in bytes {
        let id = next.ok_or("missing ACT child")?;
        let raw = raw_node(arena, id)?;
        children.push((byte, id));
        next = Some(raw.end);
    }
    Ok(children)
}

fn semantic_node(
    arena: &[u8],
    id: usize,
    seen: &mut BTreeSet<usize>,
) -> DecodeResult<SemanticNode> {
    if !seen.insert(id) {
        return Err("repeated ACT node");
    }
    let raw = raw_node(arena, id)?;
    let semantic = match raw.edge {
        RawEdge::Branch { first, bytes } => SemanticNode {
            value: raw.value,
            suffix: Vec::new(),
            children: branch_children(arena, first, bytes)?,
        },
        RawEdge::Line {
            child: None,
            suffix,
        } => SemanticNode {
            value: raw.value,
            suffix,
            children: Vec::new(),
        },
        RawEdge::Line {
            child: Some(wrapper),
            suffix,
        } => {
            if raw.value.is_some() || !seen.insert(wrapper) {
                return Err("invalid ACT line wrapper");
            }
            let wrapper_node = raw_node(arena, wrapper)?;
            let RawEdge::Branch { first, bytes } = wrapper_node.edge else {
                return Err("ACT line wrapper is not a branch");
            };
            SemanticNode {
                value: wrapper_node.value,
                suffix,
                children: branch_children(arena, first, bytes)?,
            }
        }
    };
    if semantic.value.is_none() && semantic.children.len() == 1 {
        return Err("uncompressed ACT unary node");
    }
    Ok(semantic)
}

fn walk(
    arena: &[u8],
    id: usize,
    prefix: &mut Vec<u8>,
    seen: &mut BTreeSet<usize>,
    entries: &mut Vec<(Vec<u8>, usize)>,
) -> DecodeResult<()> {
    let old_len = prefix.len();
    let node = semantic_node(arena, id, seen)?;
    prefix.extend_from_slice(&node.suffix);
    if let Some((value, source_offset)) = node.value {
        if value != 0 {
            return Err("EPM1 presence marker is not zero");
        }
        entries.push((prefix.clone(), source_offset));
    }
    for (edge, child) in node.children {
        if child >= id {
            return Err("ACT child is not before its parent");
        }
        prefix.push(edge);
        walk(arena, child, prefix, seen, entries)?;
        prefix.pop();
    }
    prefix.truncate(old_len);
    Ok(())
}

fn source_snapshot(bytes: &[u8]) -> DecodeResult<SourceSnapshot> {
    if !bytes.starts_with(b"EPM1\x01") || bytes.len() < 8 {
        return Err("EPM1 header");
    }
    let mode = bytes[5];
    if mode != 1 && mode != 2 {
        return Err("nonempty EPM1 mode");
    }
    let mut cursor = 6;
    let arena_len = usize::try_from(protobuf_varint(bytes, &mut cursor)?)
        .map_err(|_| "EPM1 arena length overflow")?;
    let arena_start = cursor;
    let arena = take(bytes, &mut cursor, arena_len)?;
    let arena_end = cursor;
    if arena.len() < 25 || &arena[..8] != b"ACTree03" || arena[arena.len() - 8..] != [0; 8] {
        return Err("ACT envelope");
    }
    let root = usize::try_from(u64::from_le_bytes(arena[8..16].try_into().unwrap()))
        .map_err(|_| "ACT root overflow")?;
    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();
    walk(arena, root, &mut Vec::new(), &mut seen, &mut paths)?;
    if paths.is_empty() {
        return Err("nonempty EPM1 has no values");
    }
    if paths.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err("ACT endpoint order");
    }
    let count = usize::try_from(protobuf_varint(bytes, &mut cursor)?)
        .map_err(|_| "EPM1 value count overflow")?;
    if (mode == 1 && count != 0) || (mode == 2 && count != paths.len()) {
        return Err("EPM1 value count mismatch");
    }
    let mut bodies = Vec::with_capacity(count);
    for _ in 0..count {
        let len = usize::try_from(protobuf_varint(bytes, &mut cursor)?)
            .map_err(|_| "EPM1 value length overflow")?;
        let body = take(bytes, &mut cursor, len)?;
        let typed = Par::decode(body).map_err(|_| "invalid EPM1 Par body")?;
        if typed.encode_to_vec() != body {
            return Err("noncanonical EPM1 Par body");
        }
        bodies.push(body.to_vec());
    }
    if cursor != bytes.len() {
        return Err("EPM1 trailing bytes");
    }
    Ok(SourceSnapshot {
        mode,
        keys: paths.iter().map(|(key, _)| key.clone()).collect(),
        bodies,
        first_value_offset: arena_start + paths[0].1,
        arena_start,
        arena_end,
    })
}

fn scalar(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn string(value: String) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value)),
    }])
}

fn nested_list(first: i64, second: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![scalar(first), scalar(second)],
            ..EList::default()
        })),
    }])
}

fn snapshot_field(body: &[u8]) -> &[u8] {
    let mut cursor = 0;
    while cursor < body.len() {
        let field = protobuf_varint(body, &mut cursor).expect("canonical generated field key");
        match field & 7 {
            0 => {
                protobuf_varint(body, &mut cursor).expect("canonical generated scalar");
            }
            2 => {
                let len =
                    usize::try_from(protobuf_varint(body, &mut cursor).expect("field length"))
                        .expect("generated length fits usize");
                let region = take(body, &mut cursor, len).expect("complete generated field");
                if field >> 3 == 9 {
                    return region;
                }
            }
            _ => panic!("unexpected generated field wire type"),
        }
    }
    panic!("generated EPathMap has no field-9 snapshot")
}

fn put_protobuf_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 128 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn map_body_with_snapshot(snapshot: &[u8]) -> Vec<u8> {
    let mut body = vec![9 << 3 | 2];
    put_protobuf_varint(u64::try_from(snapshot.len()).unwrap(), &mut body);
    body.extend_from_slice(snapshot);
    body
}

fn expression_body_with_map(original: &[u8], map_body: &[u8]) -> Vec<u8> {
    let mut cursor = 0;
    let field = protobuf_varint(original, &mut cursor).expect("one generated Expr arm");
    assert_eq!(field & 7, 2);
    let old_len = usize::try_from(protobuf_varint(original, &mut cursor).unwrap()).unwrap();
    assert_eq!(cursor + old_len, original.len());
    let mut body = Vec::new();
    put_protobuf_varint(field, &mut body);
    put_protobuf_varint(u64::try_from(map_body.len()).unwrap(), &mut body);
    body.extend_from_slice(map_body);
    body
}

fn score_integer(value: i64) -> Vec<u8> {
    let mut score = vec![1, 1];
    score.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
    score
}

fn score_snapshot(snapshot: &[u8]) -> Vec<u8> {
    let mut score = vec![1, 3];
    for byte in snapshot {
        score.push(*byte);
        if *byte == 0 {
            score.push(255);
        }
    }
    score.extend_from_slice(&[0, 0]);
    score
}

fn pathmap_score(snapshot: &[u8]) -> Vec<u8> {
    let mut score = vec![2];
    score.extend(score_integer(13));
    score.extend(score_snapshot(snapshot));
    score.extend(score_integer(0));
    score.push(0);
    score
}

fn expression_key(body: &[u8], snapshot: &[u8]) -> Vec<u8> {
    let score = pathmap_score(snapshot);
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    key.push(ParComponentTag::Expr.byte());
    key.extend_from_slice(&score);
    key.extend_from_slice(body);
    key.extend_from_slice(&u64::try_from(score.len()).unwrap().to_be_bytes());
    key
}

fn admitted_expression(key: &[u8]) -> bool {
    let mut bag = Bag::new();
    bag.insert(key).expect("one generated key fits a bag");
    ParBag::try_from_bag(bag).is_ok()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_nonempty_set_and_map_match_independent_epm1_topology_and_values(
        seed in any::<u16>(),
        first in any::<i16>(),
        second in any::<i16>(),
        reverse in any::<bool>(),
    ) {
        let keys = vec![
            string(format!("common/{seed:04x}/a")),
            string(format!("common/{seed:04x}/z")),
            string(format!("other\0{seed:04x}")),
            scalar(i64::from(seed)),
        ];
        let values = vec![
            scalar(i64::from(first)),
            nested_list(i64::from(first), i64::from(second)),
            string(format!("value\0{seed:04x}")),
            scalar(i64::from(second)),
        ];
        let mut pairs: Vec<_> = keys.iter().cloned().zip(values.iter().cloned()).collect();
        if reverse {
            pairs.reverse();
        }
        let mut expected = BTreeMap::new();
        for (key, value) in &pairs {
            prop_assert!(expected.insert(encode_trie_path(key), value.clone()).is_none());
        }
        for mode in [1u8, 2] {
            let map = if mode == 1 {
                EPathMap::from_set_iter(pairs.iter().map(|(key, _)| key.clone()), Vec::new(), false, None)
            } else {
                EPathMap::new_map(pairs.clone(), Vec::new(), false, None)
            };
            let body = protobuf_encoder::encode_to_vec(&map);
            let snapshot = snapshot_field(&body);
            let oracle = source_snapshot(snapshot).expect("generated EPM1 has valid independent topology");
            prop_assert_eq!(oracle.mode, mode);
            prop_assert_eq!(oracle.keys, expected.keys().cloned().collect::<Vec<_>>());
            if mode == 1 {
                prop_assert!(oracle.bodies.is_empty());
            } else {
                let expected_values: Vec<_> = expected.values().map(Message::encode_to_vec).collect();
                prop_assert_eq!(oracle.bodies, expected_values);
            }

            prop_assert_eq!(&body, &map_body_with_snapshot(snapshot));
            let expression = Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map)),
            };
            let expression_body = protobuf_encoder::encode_to_vec(&expression);
            let valid_key = expression_key(&expression_body, snapshot);
            prop_assert_eq!(valid_key.clone(), with_component_key(&expression, <[u8]>::to_vec));
            prop_assert!(admitted_expression(&valid_key));

            let mut bad_magic = snapshot.to_vec();
            bad_magic[0] ^= 1;
            let mut bad_root = snapshot.to_vec();
            bad_root[oracle.arena_start + 8..oracle.arena_start + 16].fill(0);

            let mut bad_marker = snapshot.to_vec();
            bad_marker[oracle.first_value_offset] = 1;

            let mut bad_trailer = snapshot.to_vec();
            bad_trailer[oracle.arena_end - 1] = 1;

            let mut truncated = snapshot.to_vec();
            truncated.pop();
            for invalid in [&bad_magic, &bad_root, &bad_marker, &bad_trailer, &truncated] {
                prop_assert!(source_snapshot(invalid).is_err());
                let invalid_map = map_body_with_snapshot(invalid);
                prop_assert!(EPathMap::decode(invalid_map.as_slice()).is_err());
                let invalid_expression = expression_body_with_map(&expression_body, &invalid_map);
                prop_assert!(Expr::decode(invalid_expression.as_slice()).is_err());
                prop_assert!(!admitted_expression(&expression_key(&invalid_expression, invalid)));
            }
        }
    }
}
