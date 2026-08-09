use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path};
use models::rust::rholang::par_children::dismantle;
use models::rust::rholang::protobuf_encoder;

fn integer(value: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

fn epathmap_carrier(map: EPathMap) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    }
}

fn nested_map_key_chain(depth: usize) -> Par {
    let mut key = integer(0);
    for _ in 0..depth {
        key = epathmap_carrier(EPathMap::new_map(
            [(key, integer(0))],
            Vec::new(),
            false,
            None,
        ));
    }
    key
}

fn varint_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

fn push_varint(out: &mut Vec<u8>, mut value: usize) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn act_varint_len(value: usize) -> usize {
    if value <= 247 {
        1
    } else {
        1 + ((usize::BITS - value.leading_zeros()) as usize).div_ceil(8)
    }
}

fn push_act_varint(out: &mut Vec<u8>, value: usize) {
    if value <= 247 {
        out.push(value as u8);
        return;
    }
    let bytes = value.to_le_bytes();
    let len = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .expect("a value above 247 is nonzero")
        + 1;
    out.push(247 + len as u8);
    out.extend_from_slice(&bytes[..len]);
}

fn one_entry_map_path_len(child_len: usize, value_len: usize) -> usize {
    let line_data_len = act_varint_len(child_len) + child_len;
    let arena_len = 16 + line_data_len + 2 + act_varint_len(line_data_len) + 8;
    let snapshot_len =
        6 + varint_len(arena_len) + arena_len + varint_len(1) + varint_len(value_len) + value_len;
    1 + varint_len(snapshot_len) + snapshot_len
}

fn canonical_one_entry_map_key_chain(depth: usize) -> Vec<u8> {
    let leaf = encode_trie_path(&integer(0));
    let value = protobuf_encoder::encode_to_vec(&integer(0));
    let mut child_lengths = Vec::with_capacity(depth);
    let mut path_len = leaf.len();
    for _ in 0..depth {
        child_lengths.push(path_len);
        path_len = one_entry_map_path_len(path_len, value.len());
    }

    let mut bytes = Vec::with_capacity(path_len);
    for &child_len in child_lengths.iter().rev() {
        let line_data_len = act_varint_len(child_len) + child_len;
        let arena_len = 16 + line_data_len + 2 + act_varint_len(line_data_len) + 8;
        let snapshot_len = 6
            + varint_len(arena_len)
            + arena_len
            + varint_len(1)
            + varint_len(value.len())
            + value.len();
        bytes.push(models::rust::canonical_path::tag::EPATHMAP_MAP);
        push_varint(&mut bytes, snapshot_len);
        bytes.extend_from_slice(b"EPM1");
        bytes.push(1);
        bytes.push(2);
        push_varint(&mut bytes, arena_len);
        bytes.extend_from_slice(b"ACTree03");
        let root_offset = 16 + line_data_len;
        bytes.extend_from_slice(&(root_offset as u64).to_le_bytes());
        push_act_varint(&mut bytes, child_len);
    }

    bytes.extend_from_slice(&leaf);
    for &child_len in &child_lengths {
        let line_data_len = act_varint_len(child_len) + child_len;
        bytes.extend_from_slice(&[0xc0, 0x00]);
        push_act_varint(&mut bytes, line_data_len);
        bytes.extend_from_slice(&[0; 8]);
        push_varint(&mut bytes, 1);
        push_varint(&mut bytes, value.len());
        bytes.extend_from_slice(&value);
    }
    assert_eq!(bytes.len(), path_len);
    bytes
}

#[test]
fn canonical_map_key_worklist_preserves_bounded_reference_values() {
    for depth in [1, 2, 4, 8, 16, 32] {
        let expected = nested_map_key_chain(depth);
        let bytes = encode_trie_path(&expected);
        assert_eq!(canonical_one_entry_map_key_chain(depth), bytes);
        let actual = decode_trie_path(&bytes)
            .expect("the generated canonical key must remain accepted by the worklist decoder");

        assert_eq!(actual, expected, "semantic value changed at depth {depth}");
        assert_eq!(
            encode_trie_path(&actual),
            bytes,
            "canonical bytes changed at depth {depth}"
        );

        dismantle(actual);
        dismantle(expected);
    }
}

#[test]
fn deferred_validation_preserves_all_entry_folds_for_escape_form_maps() {
    let mut key = Par::default();
    key.locally_free = vec![0b0000_0101];
    key.connective_used = true;
    let value = models::par_from_default! {
        exprs: vec![
            Expr { expr_instance: Some(ExprInstance::GInt(1)) },
            Expr { expr_instance: Some(ExprInstance::GInt(2)) },
        ],
        locally_free: vec![0b0000_1010],
        ..Default::default()
    };
    let split_key = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![integer(3), Par::default()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    };
    let map = EPathMap::new_map(
        [(key, value), (split_key, integer(4))],
        Vec::new(),
        false,
        None,
    );
    let expected = (
        map.entry_trie().entries_stable(),
        map.entry_trie().entries_reducer_eval_identity(),
        map.entry_trie().union_locally_free().to_vec(),
        map.entry_trie().any_connective_used(),
    );
    let root = epathmap_carrier(map);
    let bytes = encode_trie_path(&root);
    let actual = decode_trie_path(&bytes).expect("the canonical escape-form map must decode");
    let ExprInstance::EPathmapBody(actual_map) = actual.exprs[0]
        .expr_instance
        .as_ref()
        .expect("the carrier has one expression")
    else {
        panic!("the decoded carrier is not an EPathMap")
    };
    let observed = (
        actual_map.entry_trie().entries_stable(),
        actual_map.entry_trie().entries_reducer_eval_identity(),
        actual_map.entry_trie().union_locally_free().to_vec(),
        actual_map.entry_trie().any_connective_used(),
    );
    assert_eq!(observed, expected);
    assert_eq!(encode_trie_path(&actual), bytes);
    dismantle(actual);
    dismantle(root);
}

#[test]
fn split_escape_paths_preserve_canonical_list_form() {
    let escape = encode_trie_path(&Par::default());
    let mut canonical = Vec::with_capacity(escape.len() * 2 + 1);
    canonical.extend_from_slice(&escape);
    canonical.extend_from_slice(&escape);
    canonical.push(models::rust::canonical_path::tag::TERM);
    let decoded = decode_trie_path(&canonical).expect("split escape path must remain canonical");
    assert_eq!(encode_trie_path(&decoded), canonical);
    dismantle(decoded);
}

#[test]
fn canonical_map_key_worklist_reports_a_reproducible_linear_ladder() {
    for depth in [512usize, 1_024, 2_048, 4_096] {
        let bytes = canonical_one_entry_map_key_chain(depth);
        let started = std::time::Instant::now();
        let actual = decode_trie_path(&bytes)
            .expect("the explicit key/fold worklist accepts each canonical ladder image");
        let decode_elapsed = started.elapsed();
        assert_eq!(encode_trie_path(&actual), bytes);
        dismantle(actual);
        println!("canonical-key-ladder depth={depth} decode={decode_elapsed:?}");
    }
}

#[test]
fn canonical_map_key_worklist_is_stack_safe_at_depth_20000() {
    const DEPTH: usize = 20_000;
    const STACK: usize = 256 * 1024;

    std::thread::Builder::new()
        .name("epathmap-canonical-key-depth-20000".to_owned())
        .stack_size(STACK)
        .spawn(move || {
            let bytes = canonical_one_entry_map_key_chain(DEPTH);
            let actual = decode_trie_path(&bytes)
                .expect("the explicit key/fold worklist accepts its 20,000-level image");
            assert_eq!(encode_trie_path(&actual), bytes);
            dismantle(actual);
        })
        .expect("spawn the fixed small-stack canonical-key probe")
        .join()
        .expect("canonical-key validation and teardown must not consume host stack by depth");
}
