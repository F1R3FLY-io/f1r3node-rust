mod par_corpus;

use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Expr;
use models::rust::rholang::match_path::{
    encode_match_path, encode_par_match_path, shares_cow_bincode_identity, RHO_MATCH_PATH_VERSION,
};
use models::rust::rholang::match_path_cursor::{
    validate_match_path, EncodedRhoMatchPathCursor, RhoMatchPathScalar, RhoMatchPathToken,
};
use models::rust::rholang::match_schema_tables::ParCursor;

fn corpus_golden_manifest() -> String {
    let mut output = String::from("# RMP2 corpus v1: name\tencoded-bytes\tblake2b-256\n");
    for (name, par) in par_corpus::par_corpus() {
        let encoded = encode_par_match_path(&par);
        let digest = Blake2b256::hash(encoded.clone());
        writeln!(output, "{name}\t{}\t{}", encoded.len(), hex::encode(digest)).unwrap();
    }
    output
}

fn std_hash(value: &impl Hash) -> u64 {
    let mut state = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

#[test]
fn exhaustive_corpus_is_schema_valid_and_navigable() {
    for (name, par) in par_corpus::par_corpus() {
        let encoded = encode_par_match_path(&par);
        let validated =
            validate_match_path(&encoded).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(validated.root_program().type_name, "Par", "{name}");

        let mut token_count = 0;
        let mut node_count = 0;
        for token in EncodedRhoMatchPathCursor::new(&encoded).expect("RMP2 version") {
            let token = token.unwrap_or_else(|error| panic!("{name}: {error}"));
            token_count += 1;
            if matches!(token.token, RhoMatchPathToken::NodeStart { .. }) {
                node_count += 1;
                let value = validated
                    .value(token.start)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                let mut standalone = Vec::with_capacity(RHO_MATCH_PATH_VERSION.len() + value.len());
                standalone.extend_from_slice(&RHO_MATCH_PATH_VERSION);
                standalone.extend_from_slice(value);
                validate_match_path(&standalone)
                    .unwrap_or_else(|error| panic!("{name} subtree: {error}"));
            }
        }
        assert!(token_count > 0, "{name}");
        assert!(node_count > 0, "{name}");
    }
}

#[test]
fn decoded_tokens_canonically_reencode_every_schema_shape() {
    for (name, par) in par_corpus::par_corpus() {
        let encoded = encode_par_match_path(&par);
        let validated =
            validate_match_path(&encoded).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            validated.to_canonical_bytes(),
            encoded,
            "{name}: decoded tokens changed their canonical bytes"
        );
    }
}

#[test]
fn canonical_corpus_matches_the_platform_independent_golden_manifest() {
    let actual = corpus_golden_manifest();
    if std::env::var_os("UPDATE_RMP2_GOLDEN").is_some() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/goldens/rmp2-corpus-v1.tsv");
        std::fs::write(path, actual).expect("write reviewed RMP2 golden manifest");
        return;
    }
    let expected = include_str!("fixtures/goldens/rmp2-corpus-v1.tsv");
    assert_eq!(
        actual, expected,
        "RMP2 bytes drifted; review the schema/codec change and update the committed golden only \
         when a new format version is intentional"
    );
}

#[test]
fn validated_paths_compare_and_hash_by_canonical_bytes() {
    let left_bytes = encode_par_match_path(&par_corpus::gint(7));
    let equal_bytes = encode_par_match_path(&par_corpus::gint(7));
    let right_bytes = encode_par_match_path(&par_corpus::gint(8));
    let left = validate_match_path(&left_bytes).unwrap();
    let equal = validate_match_path(&equal_bytes).unwrap();
    let right = validate_match_path(&right_bytes).unwrap();

    assert_eq!(left, equal);
    assert_ne!(left, right);
    assert_eq!(left.cmp(&right), left_bytes.cmp(&right_bytes));

    assert_eq!(std_hash(&left), std_hash(&left_bytes.as_slice()));

    let mut original = EncodedRhoMatchPathCursor::new(&left_bytes).unwrap();
    let mut cloned = original.clone();
    assert_eq!(original.next(), cloned.next());
    assert_eq!(original, cloned);
}

#[test]
fn cow_identity_witness_is_sound_and_conservative() {
    let original = par_corpus::all_par_fields();
    let shared = original.clone();
    assert!(shares_cow_bincode_identity(
        ParCursor::from(&original),
        ParCursor::from(&shared),
    ));

    let independently_built = par_corpus::all_par_fields();
    assert_eq!(original, independently_built);
    assert!(!shares_cow_bincode_identity(
        ParCursor::from(&original),
        ParCursor::from(&independently_built),
    ));

    let mut metadata_changed = shared.clone();
    metadata_changed.replace_locally_free(vec![0xFE]);
    assert!(!shares_cow_bincode_identity(
        ParCursor::from(&original),
        ParCursor::from(&metadata_changed),
    ));

    let map = par_corpus::ground_pathmap();
    let shared_map = map.clone();
    assert!(shares_cow_bincode_identity(
        ParCursor::from(&map),
        ParCursor::from(&shared_map),
    ));
    let mut changed_map = shared_map;
    changed_map.insert_entry(par_corpus::gint(1_000));
    assert!(!shares_cow_bincode_identity(
        ParCursor::from(&map),
        ParCursor::from(&changed_map),
    ));
}

#[test]
fn malformed_tokens_lengths_fields_and_nesting_are_rejected() {
    let encoded = encode_par_match_path(&par_corpus::all_par_fields());

    let mut bad_version = encoded.clone();
    bad_version[0] ^= 0xFF;
    assert!(validate_match_path(&bad_version).is_err());

    let mut retired_rmp1 = encoded.clone();
    retired_rmp1[..RHO_MATCH_PATH_VERSION.len()].copy_from_slice(b"RMP\x01");
    assert!(
        validate_match_path(&retired_rmp1).is_err(),
        "the RMP2 reader must not reinterpret a retired RMP1 derived index"
    );

    let small = encode_par_match_path(&par_corpus::gint(7));
    for end in RHO_MATCH_PATH_VERSION.len()..small.len() {
        assert!(
            validate_match_path(&small[..end]).is_err(),
            "truncation at {end} was accepted"
        );
    }

    let spans = EncodedRhoMatchPathCursor::new(&encoded)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let mut unknown_type = encoded.clone();
    unknown_type[RHO_MATCH_PATH_VERSION.len() + 1..RHO_MATCH_PATH_VERSION.len() + 3]
        .copy_from_slice(&0u16.to_be_bytes());
    assert!(validate_match_path(&unknown_type).is_err());

    // A production Par path intentionally contains one opaque native ParBag
    // token. Use a direct generated component root for structural token
    // corruption so this test does not demand that matching decode the trie
    // back into the retired repeated-field representation.
    let bind_patterns = par_corpus::bind_pattern_corpus();
    let nested_encoded = encode_match_path(ParCursor::from(&bind_patterns[1].1));
    let nested_spans = EncodedRhoMatchPathCursor::new(&nested_encoded)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let nested_node = nested_spans
        .iter()
        .filter_map(|span| match span.token {
            RhoMatchPathToken::NodeStart { program } => Some((span, program.type_id)),
            _ => None,
        })
        .nth(1)
        .expect("all-fields corpus has nested messages");
    let replacement_type_id: u16 = if nested_node.1 == 1 { 2 } else { 1 };
    let mut wrong_nested_type = nested_encoded.clone();
    wrong_nested_type[nested_node.0.start + 1..nested_node.0.end]
        .copy_from_slice(&replacement_type_id.to_be_bytes());
    assert!(validate_match_path(&wrong_nested_type).is_err());

    let field = spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::FieldStart(_)))
        .unwrap();
    let mut wrong_field = encoded.clone();
    wrong_field[field.end - 1] ^= 1;
    assert!(validate_match_path(&wrong_field).is_err());

    let sequence = nested_spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::SequenceStart(_)))
        .unwrap();
    let mut wrong_count = nested_encoded.clone();
    wrong_count[sequence.end - 1] = wrong_count[sequence.end - 1].wrapping_add(1);
    assert!(validate_match_path(&wrong_count).is_err());

    let bool_expr = Expr {
        expr_instance: Some(ExprInstance::GBool(true)),
    };
    let bool_encoded = encode_match_path(ParCursor::from(&bool_expr));
    let bool_spans = EncodedRhoMatchPathCursor::new(&bool_encoded)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let boolean = bool_spans
        .iter()
        .find(|span| {
            matches!(
                span.token,
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::Bool(_))
            )
        })
        .unwrap();
    let mut invalid_bool = bool_encoded.clone();
    invalid_bool[boolean.end - 1] = 2;
    assert!(validate_match_path(&invalid_bool).is_err());

    let string_expr = Expr {
        expr_instance: Some(ExprInstance::GString("valid UTF-8".to_string())),
    };
    let string_encoded = encode_match_path(ParCursor::from(&string_expr));
    let string_spans = EncodedRhoMatchPathCursor::new(&string_encoded)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let string = string_spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::Scalar(RhoMatchPathScalar::String(value)) if !value.is_empty()))
        .unwrap();
    let mut invalid_utf8 = string_encoded.clone();
    invalid_utf8[string.start + 9] = 0xFF;
    assert!(validate_match_path(&invalid_utf8).is_err());

    let pathmap_value = par_corpus::ground_pathmap();
    let pathmap_encoded = encode_match_path(ParCursor::from(&pathmap_value));
    let pathmap_spans = EncodedRhoMatchPathCursor::new(&pathmap_encoded)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let pathmap = pathmap_spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::PathMap { .. }))
        .unwrap();
    let mut invalid_mode = pathmap_encoded.clone();
    invalid_mode[pathmap.start + 1] = u8::MAX;
    assert!(validate_match_path(&invalid_mode).is_err());

    let oneof = bool_spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::OneofStart(_)))
        .expect("direct Expr corpus has a oneof");
    let mut invalid_oneof_tag = bool_encoded.clone();
    invalid_oneof_tag[oneof.start + 1..oneof.end].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(validate_match_path(&invalid_oneof_tag).is_err());

    let mut unknown_token = encoded;
    unknown_token[RHO_MATCH_PATH_VERSION.len()] = 0xFF;
    assert!(validate_match_path(&unknown_token).is_err());
}

#[test]
fn par_bag_token_is_the_fixed_49_byte_rmp2_summary() {
    let encoded = encode_par_match_path(&par_corpus::all_par_fields());
    let spans = EncodedRhoMatchPathCursor::new(&encoded)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let token = spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::ParBag { .. }))
        .expect("a Par root has one ParBag token");
    assert_eq!(token.end - token.start, 1 + 8 + 8 + 32);
}

#[test]
fn validation_and_subtree_navigation_fit_a_small_native_stack() {
    std::thread::Builder::new()
        .name("encoded-rho-match-path".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let encoded = encode_par_match_path(&par_corpus::deep_par(20_000));
            let validated = validate_match_path(&encoded).expect("validate deep RMP2 path");
            assert_eq!(validated.bytes(), encoded);
            assert_eq!(validated.to_canonical_bytes(), encoded);

            let deepest = EncodedRhoMatchPathCursor::new(&encoded)
                .unwrap()
                .filter_map(|token| {
                    let token = token.unwrap();
                    match token.token {
                        RhoMatchPathToken::NodeStart { .. } => Some(token.start),
                        _ => None,
                    }
                })
                .last()
                .expect("deep path has a node");
            assert!(!validated.value(deepest).unwrap().is_empty());
        })
        .expect("spawn small-stack encoded cursor")
        .join()
        .expect("small-stack encoded cursor must complete");
}
