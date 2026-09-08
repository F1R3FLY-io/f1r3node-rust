use super::*;
use crate::rhoapi::{EPathMap, Var};
use crate::rust::rholang::canonical_score_bytes::CanonicalScoreArena;
use crate::rust::rholang::canonical_semantics::CanonicalScoreAlgebra;
use crate::rust::rholang::sorter::score_tree::compare_score;

fn score_tree(snapshot: &[u8], connective: bool) -> Tree<ScoreAtom> {
    Tree::Node(vec![
        Tree::Leaf(ScoreAtom::create_from_i64(13)),
        Tree::Leaf(ScoreAtom::create_from_bytes(snapshot.to_vec())),
        Tree::Leaf(ScoreAtom::create_from_i64(i64::from(connective))),
    ])
}

fn checked_score(snapshot: &[u8], connective: bool) -> Vec<u8> {
    assert_eq!(Score::EPATHMAP, 13);
    let mut actual = Vec::new();
    begin_score_node(&mut actual, Score::EPATHMAP);
    append_bytes_score_atom(&mut actual, snapshot);
    append_int_score_atom(&mut actual, i64::from(connective));
    end_score_node(&mut actual);
    let mut tree_bytes = Vec::new();
    encode_order_key(&score_tree(snapshot, connective), &mut tree_bytes);
    let mut arena = CanonicalScoreArena::new();
    let bytes = arena.bytes(snapshot).unwrap();
    let score =
        canonical_semantics::rpb3_pathmap_score(&mut arena, false, bytes, connective).unwrap();
    assert_eq!(actual, tree_bytes);
    assert_eq!(actual, arena.cursor(score).collect::<Vec<_>>());
    assert_eq!(actual.len(), arena.encoded_len(score));
    assert_eq!(
        actual.len(),
        26 + snapshot.len() + snapshot.iter().filter(|&&b| b == 0).count()
    );
    let (decoded, used) = decode_order_key(&actual).unwrap();
    assert_eq!(used, actual.len());
    assert_eq!(
        compare_score(&decoded, &score_tree(snapshot, connective)),
        Ordering::Equal
    );
    actual
}

fn int(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn check_import(
    value: i64,
    map_lf: bool,
    map_connective: bool,
    remainder: bool,
    root_lf: bool,
    root_connective: bool,
) -> (Vec<u8>, usize, usize) {
    let pathmap = EPathMap::new_map(
        [(int(1), int(value))],
        if map_lf { vec![1] } else { Vec::new() },
        map_connective,
        remainder.then(Var::default),
    );
    let snapshot = pathmap.trie_snapshot().to_vec();
    let expr = Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
    };
    let expr_bytes = protobuf_encoder::encode_to_vec(&expr);
    let expected_score = checked_score(&snapshot, map_connective);
    let mut typed_score = Vec::new();
    assert!(append_pathmap_expr_score(&expr, &mut typed_score));
    assert_eq!(typed_score, expected_score);
    let expected = Par::default()
        .with_exprs(vec![expr])
        .with_locally_free(if root_lf { vec![7] } else { Vec::new() })
        .with_connective_used(root_connective);
    let protobuf = protobuf_encoder::encode_to_vec(&expected);
    let imported = import_canonical_singleton_epathmap_par(&protobuf).unwrap();
    assert_eq!(imported, expected);
    assert_eq!(protobuf_encoder::encode_to_vec(&imported), protobuf);
    assert_eq!(imported.locally_free(), expected.locally_free());
    assert_eq!(imported.connective_used(), root_connective);
    assert_eq!(
        imported.components().compact_snapshot(),
        expected.components().compact_snapshot()
    );
    let mut expected_key = vec![82, 80, 66, 3, 5];
    expected_key.extend_from_slice(&expected_score);
    expected_key.extend_from_slice(&expr_bytes);
    expected_key.extend_from_slice(&(expected_score.len() as u64).to_be_bytes());
    let mut visits = 0;
    imported.visit_encoded::<Expr>(|entry| {
        visits += 1;
        assert_eq!(entry.score(), expected_score);
        assert_eq!(entry.payload(), expr_bytes);
        assert_eq!(entry.multiplicity(), Multiplicity::ONE);
    });
    assert_eq!(visits, 1);
    for par in [&imported, &expected] {
        let mut keys = 0;
        par.components()
            .components
            .visit_prefix(&[], |key, multiplicity| {
                keys += 1;
                assert_eq!(key, expected_key);
                assert_eq!(multiplicity, Multiplicity::ONE);
                assert!(validate_canonical_key(key).is_ok());
                let (tag, score, payload) = split_key_parts(key).unwrap();
                assert_eq!(tag, ParComponentTag::Expr);
                assert_eq!(score, expected_score);
                assert_eq!(payload, expr_bytes);
            });
        assert_eq!(keys, 1);
    }
    let n = snapshot.len();
    let zeros = snapshot.iter().filter(|&&b| b == 0).count();
    assert_eq!(expected_key.len(), 39 + n + zeros + expr_bytes.len());
    let reserve = n
        .checked_mul(2)
        .and_then(|size| 5usize.checked_add(size))
        .and_then(|size| size.checked_add(expr_bytes.len()))
        .and_then(|size| size.checked_add(64))
        .unwrap();
    assert_eq!(reserve, 69 + 2 * n + expr_bytes.len());
    assert!(expected_key.len() <= reserve);
    (expected_key, expected_score.len(), expr_bytes.len())
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..proptest::test_runner::Config::with_cases(1_024)
    })]

    #[test]
    fn canonical_epathmap_score_proof_three_way_bytes(
        snapshot in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..513),
    ) {
        for connective in [false, true] {
            checked_score(&snapshot, connective);
        }
    }

    #[test]
    fn canonical_epathmap_score_proof_full_import_key_observations(
        value in proptest::prelude::any::<i64>(),
        metadata in proptest::prelude::any::<(bool, bool, bool, bool, bool)>(),
    ) {
        check_import(value, metadata.0, metadata.1, metadata.2, metadata.3, metadata.4);
    }

    #[test]
    fn canonical_epathmap_score_proof_order_and_prefix_separation(
        left in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..65),
        right in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..65),
        left_flag in proptest::prelude::any::<bool>(),
        right_flag in proptest::prelude::any::<bool>(),
    ) {
        let a = checked_score(&left, left_flag);
        let b = checked_score(&right, right_flag);
        proptest::prop_assert_eq!(a.cmp(&b), compare_score(&score_tree(&left, left_flag), &score_tree(&right, right_flag)));
        if a != b {
            proptest::prop_assert!(!a.starts_with(&b) && !b.starts_with(&a));
            let mut a_suffix = a.clone();
            a_suffix.extend([255, 255]);
            let mut b_suffix = b.clone();
            b_suffix.extend([0, 0]);
            proptest::prop_assert_eq!(a_suffix.cmp(&b_suffix), a.cmp(&b));
        }
    }
}

#[test]
fn canonical_epathmap_score_proof_causal_mutations_and_controls() {
    for snapshot in [
        vec![],
        vec![0],
        vec![255],
        vec![0; 128],
        vec![1; 128],
        vec![0, 255, 0, 1],
    ] {
        let low = checked_score(&snapshot, false);
        let high = checked_score(&snapshot, true);
        assert!(low < high);
    }
    let (key, score_len, payload_len) = check_import(7, false, false, false, false, false);
    assert!(validate_canonical_key(&key).is_ok());
    let mut constructor = key.clone();
    constructor[15] = 14;
    let mut escape = key.clone();
    let zero_at = key[5..5 + score_len]
        .windows(2)
        .position(|bytes| bytes == [0, 255])
        .unwrap();
    escape[5 + zero_at + 1] = 1;
    let mut connective = key.clone();
    connective[5 + score_len - 2] ^= 1;
    let mut payload = key.clone();
    payload[5 + score_len] = 0;
    let mut trailer = key.clone();
    trailer[5 + score_len + payload_len..].copy_from_slice(&((score_len + 1) as u64).to_be_bytes());
    for (label, mutant) in [
        ("constructor", constructor),
        ("escape", escape),
        ("connective", connective),
        ("payload", payload),
        ("trailer", trailer),
    ] {
        assert_ne!(mutant, key);
        assert!(validate_canonical_key(&mutant).is_err(), "{label}");
    }
    let (other, other_score_len, _) = check_import(7, true, false, true, false, false);
    assert_eq!(score_len, other_score_len);
    assert_eq!(&key[5..5 + score_len], &other[5..5 + score_len]);
    let (_, _, left_payload) = split_key_parts(&key).unwrap();
    let (_, _, right_payload) = split_key_parts(&other).unwrap();
    assert!(!left_payload.starts_with(right_payload) && !right_payload.starts_with(left_payload));
    assert_eq!(key.cmp(&other), left_payload.cmp(right_payload));
}
