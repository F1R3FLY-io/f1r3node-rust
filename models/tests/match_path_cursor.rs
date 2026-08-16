mod par_corpus;

use models::rust::rholang::match_path::{encode_par_match_path, RHO_MATCH_PATH_VERSION};
use models::rust::rholang::match_path_cursor::{
    validate_match_path, EncodedRhoMatchPathCursor, RhoMatchPathScalar, RhoMatchPathToken,
};

#[test]
fn exhaustive_corpus_is_schema_valid_and_navigable() {
    for (name, par) in par_corpus::par_corpus() {
        let encoded = encode_par_match_path(&par);
        let validated =
            validate_match_path(&encoded).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(validated.root_program().type_name, "Par", "{name}");

        let mut token_count = 0;
        let mut node_count = 0;
        for token in EncodedRhoMatchPathCursor::new(&encoded).expect("RMP1 version") {
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
fn malformed_tokens_lengths_fields_and_nesting_are_rejected() {
    let encoded = encode_par_match_path(&par_corpus::all_par_fields());

    let mut bad_version = encoded.clone();
    bad_version[0] ^= 0xFF;
    assert!(validate_match_path(&bad_version).is_err());

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

    let nested_node = spans
        .iter()
        .filter_map(|span| match span.token {
            RhoMatchPathToken::NodeStart { program } => Some((span, program.type_id)),
            _ => None,
        })
        .nth(1)
        .expect("all-fields corpus has nested messages");
    let replacement_type_id: u16 = if nested_node.1 == 1 { 2 } else { 1 };
    let mut wrong_nested_type = encoded.clone();
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

    let sequence = spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::SequenceStart(_)))
        .unwrap();
    let mut wrong_count = encoded.clone();
    wrong_count[sequence.end - 1] = wrong_count[sequence.end - 1].wrapping_add(1);
    assert!(validate_match_path(&wrong_count).is_err());

    let boolean = spans
        .iter()
        .find(|span| {
            matches!(
                span.token,
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::Bool(_))
            )
        })
        .unwrap();
    let mut invalid_bool = encoded.clone();
    invalid_bool[boolean.end - 1] = 2;
    assert!(validate_match_path(&invalid_bool).is_err());

    let string = spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::Scalar(RhoMatchPathScalar::String(value)) if !value.is_empty()))
        .unwrap();
    let mut invalid_utf8 = encoded.clone();
    invalid_utf8[string.start + 9] = 0xFF;
    assert!(validate_match_path(&invalid_utf8).is_err());

    let pathmap = spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::PathMap { .. }))
        .unwrap();
    let mut invalid_mode = encoded.clone();
    invalid_mode[pathmap.start + 1] = u8::MAX;
    assert!(validate_match_path(&invalid_mode).is_err());

    let oneof = spans
        .iter()
        .find(|span| matches!(span.token, RhoMatchPathToken::OneofStart(_)))
        .expect("all-fields corpus has oneofs");
    let mut invalid_oneof_tag = encoded.clone();
    invalid_oneof_tag[oneof.start + 1..oneof.end].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(validate_match_path(&invalid_oneof_tag).is_err());

    let mut unknown_token = encoded;
    unknown_token[RHO_MATCH_PATH_VERSION.len()] = 0xFF;
    assert!(validate_match_path(&unknown_token).is_err());
}

#[test]
fn validation_and_subtree_navigation_fit_a_small_native_stack() {
    std::thread::Builder::new()
        .name("encoded-rho-match-path".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let encoded = encode_par_match_path(&par_corpus::deep_par(20_000));
            let validated = validate_match_path(&encoded).expect("validate deep RMP1 path");
            assert_eq!(validated.bytes(), encoded);

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
