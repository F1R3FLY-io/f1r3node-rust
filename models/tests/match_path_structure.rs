mod par_corpus;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::{Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send};
use models::rust::par_bag::{ParBagComponent, ParComponentTag};
use models::rust::rhoapi_ext::EPathMap;
use models::rust::rholang::match_path::{
    append_match_path_event, encode_match_path, encode_par_match_atom, encode_par_match_path,
    epathmap_match_digest, RHO_MATCH_PATH_VERSION,
};
use models::rust::rholang::match_path_cursor::{EncodedRhoMatchPathCursor, RhoMatchPathToken};
use models::rust::rholang::match_region_path::{
    append_region_event, CanonicalRhoPathRegionCursor, CanonicalRhoPathRegionEvent,
    CanonicalRhoRegionScalar,
};
use models::rust::rholang::match_schema::{RhoPathCursor, RhoPathEvent, RhoScalarRef};
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::rholang::protobuf_cursor::CanonicalFieldCursor;
use models::rust::rholang::protobuf_encoder;
use proptest::prelude::*;
use rspace_plus_plus::rspace::bag::Multiplicity;

fn frozen_resident_unary_fixture(depth: usize, mut terminal: Par) -> Par {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::EList;

    for _ in 0..depth {
        terminal = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![terminal],
                ..EList::default()
            })),
        }]);
    }
    terminal
}

fn assert_fixture_representation_equal(actual: &Par, expected: &Par) {
    use std::hash::{Hash, Hasher};

    assert_eq!(actual, expected);
    assert_eq!(actual.cmp(expected), std::cmp::Ordering::Equal);
    assert_eq!(
        actual.protobuf_bytes().unwrap(),
        expected.protobuf_bytes().unwrap()
    );
    assert_eq!(
        models::rust::rholang::bincode_encoder::encode(actual),
        models::rust::rholang::bincode_encoder::encode(expected),
    );
    assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    assert_eq!(actual.locally_free(), expected.locally_free());
    assert_eq!(actual.connective_used(), expected.connective_used());
    assert_eq!(actual.cardinality(), expected.cardinality());
    assert_eq!(actual.distinct_len(), expected.distinct_len());
    assert_eq!(
        actual.family_cardinality::<Expr>(),
        expected.family_cardinality::<Expr>()
    );
    let mut actual_hash = std::collections::hash_map::DefaultHasher::new();
    let mut expected_hash = std::collections::hash_map::DefaultHasher::new();
    actual.hash(&mut actual_hash);
    expected.hash(&mut expected_hash);
    assert_eq!(actual_hash.finish(), expected_hash.finish());
}

#[test]
fn bulk_unary_fixture_matches_frozen_resident_oracle() {
    assert_eq!(par_corpus::gint(0).protobuf_bytes().unwrap(), [
        42, 2, 16, 0
    ]);
    for depth in [0, 1, 2, 15, 16, 17, 31, 32, 33, 127, 128] {
        for leaf in [i64::MIN, -1, 0, 1, i64::MAX] {
            let terminal = par_corpus::gint(leaf);
            let mut terminal_component_score = 0;
            terminal.visit_encoded::<Expr>(|entry| terminal_component_score += entry.score().len());
            let expected = frozen_resident_unary_fixture(depth, terminal.clone());
            let actual = Par::wrap_unary_elist(depth, terminal).unwrap();
            assert_fixture_representation_equal(&actual, &expected);
            assert_eq!(
                actual.canonical_unary_elist_gint_parts(),
                Some((depth, leaf))
            );
            if depth != 0 {
                actual.visit_encoded::<Expr>(|entry| {
                    assert_eq!(entry.score().len(), terminal_component_score + 54 * depth);
                    assert_eq!(entry.multiplicity(), Multiplicity::ONE);
                });
            }
        }
    }
    for depth in [0, 1, 4] {
        let terminal = par_corpus::all_par_fields();
        let expected = frozen_resident_unary_fixture(depth, terminal.clone());
        let actual = Par::wrap_unary_elist(depth, terminal).unwrap();
        assert_fixture_representation_equal(&actual, &expected);
    }
    for depth in [0, 1, 2, 15, 16, 17, 31, 32, 33, 127, 128] {
        let expected = frozen_resident_unary_fixture(depth, Par::default());
        let actual = Par::wrap_unary_elist(depth, Par::default()).unwrap();
        assert_fixture_representation_equal(&actual, &expected);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn bulk_unary_fixture_preserves_arbitrary_terminal_metadata(
        depth in 0usize..40,
        leaf in any::<i64>(),
        metadata in proptest::collection::vec(any::<u8>(), 0..8),
        connective in any::<bool>(),
        count in 1u64..5,
    ) {
        let expression = Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(leaf)),
        };
        let mut terminal = Par::default();
        terminal.insert_component_many(
            &expression, Multiplicity::new(count).unwrap(), metadata, connective,
        ).unwrap();
        let expected = frozen_resident_unary_fixture(depth, terminal.clone());
        let actual = Par::wrap_unary_elist(depth, terminal).unwrap();
        assert_fixture_representation_equal(&actual, &expected);
        if depth != 0 {
            prop_assert_eq!(actual.cardinality(), 1);
            prop_assert!(actual.locally_free().is_empty());
            prop_assert!(!actual.connective_used());
        }
    }
}

fn extend_wide_fixture(mut original: Par, amount: u64) -> Par {
    let (seed, _) = original
        .first::<Expr>()
        .unwrap()
        .expect("nonempty Expr family");
    original
        .insert_component_many(
            &seed,
            Multiplicity::new(amount).expect("positive fixture extension"),
            Vec::new(),
            false,
        )
        .expect("bounded fixture multiplicity");
    original
}

#[test]
fn wide_fixture_multiplicity_matches_frozen_occurrence_oracle() {
    for amount in [1, 2, 16, 64] {
        let original = par_corpus::all_par_fields();
        let (seed, _) = original.first::<Expr>().unwrap().unwrap();
        let before_count = original.count(&seed);
        let mut expected = original.clone();
        let mut occurrences = expected.decode_occurrences::<Expr>();
        occurrences.extend(std::iter::repeat_n(occurrences[0].clone(), amount as usize));
        expected.replace_exprs(occurrences);
        let actual = extend_wide_fixture(original.clone(), amount);
        assert_fixture_representation_equal(&actual, &expected);
        assert_eq!(actual.count(&seed), before_count + amount);
        assert_eq!(actual.distinct_len(), original.distinct_len());
        assert_eq!(
            actual.family_cardinality::<Send>(),
            original.family_cardinality::<Send>()
        );
    }
}

#[test]
fn wide_fixture_overflow_preserves_components_and_metadata() {
    let seed = Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(7)),
    };
    let mut original = Par::default();
    original
        .insert_component_many(&seed, Multiplicity::new(u64::MAX).unwrap(), vec![1], false)
        .unwrap();
    let snapshot = original.compact_snapshot().to_vec();
    assert!(original
        .insert_component_many(&seed, Multiplicity::ONE, vec![2], true)
        .is_err());
    assert_eq!(original.compact_snapshot(), snapshot);
    assert_eq!(original.count(&seed), u64::MAX);
    assert_eq!(original.locally_free(), [1]);
    assert!(!original.connective_used());
}

#[derive(Debug, PartialEq, Eq)]
enum SemanticScalar {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bytes(Vec<u8>),
    String(String),
    StringSequence(Vec<String>),
    BytesSequence(Vec<Vec<u8>>),
}

#[derive(Debug, PartialEq, Eq)]
enum SemanticProjection {
    NodeStart(u16),
    NodeEnd,
    FieldStart(u32),
    FieldEnd,
    Scalar(SemanticScalar),
    Absent,
    SequenceStart(u64),
    SequenceEnd,
    MapStart(u64),
    MapKey(String),
    MapEnd,
    OneofStart(u32),
    OneofEnd,
    OpaqueParBag {
        distinct: u64,
        cardinality: u64,
        digest: [u8; 32],
    },
    PathMap {
        mode: u8,
        len: u64,
        digest: [u8; 32],
    },
}

fn opaque_par_projection(encoded: &[u8]) -> SemanticProjection {
    let mut cursor = EncodedRhoMatchPathCursor::new(encoded).expect("one RMP2 event");
    let span = cursor
        .next()
        .expect("the event emits one token")
        .expect("the emitted token is canonical");
    assert!(cursor.next().is_none(), "one event must emit one token");
    match span.token {
        RhoMatchPathToken::ParBag {
            distinct,
            cardinality,
            digest,
        } => SemanticProjection::OpaqueParBag {
            distinct,
            cardinality,
            digest: *digest,
        },
        other => panic!("expected one opaque ParBag token, got {other:?}"),
    }
}

fn resident_semantic_projection(root: ParCursor<'_>) -> Vec<SemanticProjection> {
    RhoPathCursor::new(root)
        .map(|event| match event {
            RhoPathEvent::EnterNode { program, .. } => {
                SemanticProjection::NodeStart(program.type_id)
            }
            RhoPathEvent::LeaveNode => SemanticProjection::NodeEnd,
            RhoPathEvent::EnterField(field) => SemanticProjection::FieldStart(field.tag),
            RhoPathEvent::LeaveField => SemanticProjection::FieldEnd,
            RhoPathEvent::Scalar(value) => SemanticProjection::Scalar(match value {
                RhoScalarRef::Bool(value) => SemanticScalar::Bool(value),
                RhoScalarRef::I32(value) => SemanticScalar::I32(value),
                RhoScalarRef::U32(value) => SemanticScalar::U32(value),
                RhoScalarRef::I64(value) => SemanticScalar::I64(value),
                RhoScalarRef::U64(value) => SemanticScalar::U64(value),
                RhoScalarRef::Bytes(value) => SemanticScalar::Bytes(value.to_vec()),
                RhoScalarRef::String(value) => SemanticScalar::String(value.to_owned()),
                RhoScalarRef::StringSequence(values) => {
                    SemanticScalar::StringSequence(values.to_vec())
                }
                RhoScalarRef::BytesSequence(values) => {
                    SemanticScalar::BytesSequence(values.to_vec())
                }
            }),
            RhoPathEvent::Absent => SemanticProjection::Absent,
            RhoPathEvent::EnterSequence(len) => SemanticProjection::SequenceStart(
                u64::try_from(len).expect("resident sequence length fits u64"),
            ),
            RhoPathEvent::LeaveSequence => SemanticProjection::SequenceEnd,
            RhoPathEvent::EnterMap(len) => SemanticProjection::MapStart(
                u64::try_from(len).expect("resident map length fits u64"),
            ),
            RhoPathEvent::MapKey(key) => SemanticProjection::MapKey(key.to_owned()),
            RhoPathEvent::LeaveMap => SemanticProjection::MapEnd,
            RhoPathEvent::EnterOneof(oneof) => SemanticProjection::OneofStart(oneof.protobuf_tag),
            RhoPathEvent::LeaveOneof => SemanticProjection::OneofEnd,
            RhoPathEvent::ParBag(_) => {
                let mut encoded = RHO_MATCH_PATH_VERSION.to_vec();
                append_match_path_event(&mut encoded, event);
                opaque_par_projection(&encoded)
            }
            RhoPathEvent::PathMap(map) => SemanticProjection::PathMap {
                mode: map.mode() as u8,
                len: u64::try_from(map.len()).expect("resident PathMap length fits u64"),
                digest: epathmap_match_digest(map),
            },
        })
        .collect()
}

fn region_semantic_projection<'a>(
    root: impl Iterator<
        Item = Result<
            CanonicalRhoPathRegionEvent<'a>,
            models::rust::rholang::protobuf_region::ProtobufRegionError,
        >,
    >,
) -> Vec<SemanticProjection> {
    enum PendingSequence {
        String { expected: u64, values: Vec<String> },
        Bytes { expected: u64, values: Vec<Vec<u8>> },
    }

    let mut output = Vec::new();
    let mut pending = None;
    for event in root {
        let event = event.expect("an admitted canonical region is valid");
        match event {
            CanonicalRhoPathRegionEvent::EnterStringSequence(expected) => {
                assert!(pending.is_none());
                pending = Some(PendingSequence::String {
                    expected,
                    values: Vec::new(),
                });
            }
            CanonicalRhoPathRegionEvent::StringSequenceElement(value) => {
                let Some(PendingSequence::String { values, .. }) = pending.as_mut() else {
                    panic!("string sequence element outside its sequence")
                };
                values.push(value.to_owned());
            }
            CanonicalRhoPathRegionEvent::LeaveStringSequence => {
                let Some(PendingSequence::String { expected, values }) = pending.take() else {
                    panic!("string sequence end outside its sequence")
                };
                assert_eq!(u64::try_from(values.len()).unwrap(), expected);
                output.push(SemanticProjection::Scalar(SemanticScalar::StringSequence(
                    values,
                )));
            }
            CanonicalRhoPathRegionEvent::EnterBytesSequence(expected) => {
                assert!(pending.is_none());
                pending = Some(PendingSequence::Bytes {
                    expected,
                    values: Vec::new(),
                });
            }
            CanonicalRhoPathRegionEvent::BytesSequenceElement(value) => {
                let Some(PendingSequence::Bytes { values, .. }) = pending.as_mut() else {
                    panic!("bytes sequence element outside its sequence")
                };
                values.push(value.to_vec());
            }
            CanonicalRhoPathRegionEvent::LeaveBytesSequence => {
                let Some(PendingSequence::Bytes { expected, values }) = pending.take() else {
                    panic!("bytes sequence end outside its sequence")
                };
                assert_eq!(u64::try_from(values.len()).unwrap(), expected);
                output.push(SemanticProjection::Scalar(SemanticScalar::BytesSequence(
                    values,
                )));
            }
            CanonicalRhoPathRegionEvent::EnterNode(program) => {
                output.push(SemanticProjection::NodeStart(program.type_id));
            }
            CanonicalRhoPathRegionEvent::LeaveNode => output.push(SemanticProjection::NodeEnd),
            CanonicalRhoPathRegionEvent::EnterField(field) => {
                output.push(SemanticProjection::FieldStart(field.tag));
            }
            CanonicalRhoPathRegionEvent::LeaveField => {
                output.push(SemanticProjection::FieldEnd);
            }
            CanonicalRhoPathRegionEvent::Scalar(value) => {
                output.push(SemanticProjection::Scalar(match value {
                    CanonicalRhoRegionScalar::Bool(value) => SemanticScalar::Bool(value),
                    CanonicalRhoRegionScalar::I32(value) => SemanticScalar::I32(value),
                    CanonicalRhoRegionScalar::U32(value) => SemanticScalar::U32(value),
                    CanonicalRhoRegionScalar::I64(value) => SemanticScalar::I64(value),
                    CanonicalRhoRegionScalar::U64(value) => SemanticScalar::U64(value),
                    CanonicalRhoRegionScalar::Bytes(value) => SemanticScalar::Bytes(value.to_vec()),
                    CanonicalRhoRegionScalar::String(value) => {
                        SemanticScalar::String(value.to_owned())
                    }
                }));
            }
            CanonicalRhoPathRegionEvent::Absent => output.push(SemanticProjection::Absent),
            CanonicalRhoPathRegionEvent::EnterSequence(len) => {
                output.push(SemanticProjection::SequenceStart(len));
            }
            CanonicalRhoPathRegionEvent::LeaveSequence => {
                output.push(SemanticProjection::SequenceEnd);
            }
            CanonicalRhoPathRegionEvent::EnterMap(len) => {
                output.push(SemanticProjection::MapStart(len));
            }
            CanonicalRhoPathRegionEvent::MapKey(key) => {
                output.push(SemanticProjection::MapKey(key.to_owned()));
            }
            CanonicalRhoPathRegionEvent::LeaveMap => output.push(SemanticProjection::MapEnd),
            CanonicalRhoPathRegionEvent::EnterOneof(oneof) => {
                output.push(SemanticProjection::OneofStart(oneof.tag));
            }
            CanonicalRhoPathRegionEvent::LeaveOneof => {
                output.push(SemanticProjection::OneofEnd);
            }
            CanonicalRhoPathRegionEvent::ParBag(_) => {
                let mut encoded = RHO_MATCH_PATH_VERSION.to_vec();
                append_region_event(&mut encoded, event)
                    .expect("the admitted ParBag summary is valid");
                output.push(opaque_par_projection(&encoded));
            }
            CanonicalRhoPathRegionEvent::PathMap(summary) => {
                output.push(SemanticProjection::PathMap {
                    mode: summary.mode() as u8,
                    len: summary.len(),
                    digest: summary.digest(),
                });
            }
        }
    }
    assert!(pending.is_none(), "semantic scalar sequence must terminate");
    output
}

fn par_bag_tokens(path: &[u8]) -> Vec<(u64, u64, [u8; 32])> {
    EncodedRhoMatchPathCursor::new(path)
        .expect("RMP2 version")
        .filter_map(|token| match token.expect("canonical RMP2 token").token {
            RhoMatchPathToken::ParBag {
                distinct,
                cardinality,
                digest,
            } => Some((distinct, cardinality, *digest)),
            _ => None,
        })
        .collect()
}

#[test]
fn structural_paths_are_deterministic_and_schema_sensitive() {
    let corpus = par_corpus::par_corpus();
    for (name, value) in &corpus {
        let first = encode_par_match_path(value);
        let second = encode_par_match_path(value);
        assert_eq!(first, second, "{name}");
        assert!(first.starts_with(&RHO_MATCH_PATH_VERSION), "{name}");
    }

    for pair in corpus.windows(2) {
        assert_ne!(
            encode_par_match_path(&pair[0].1),
            encode_par_match_path(&pair[1].1),
            "adjacent exhaustive corpus terms {} and {} must remain distinguishable",
            pair[0].0,
            pair[1].0
        );
    }
}

#[test]
fn admitted_expr_regions_match_the_generated_cursor_for_every_oneof_arm() {
    for (name, instance) in par_corpus::every_expr_instance() {
        let carrier = Par::default().with_exprs(vec![models::rhoapi::Expr {
            expr_instance: Some(instance),
        }]);
        let stored = carrier
            .first::<models::rhoapi::Expr>()
            .expect("test oracle decodes one admitted Expr")
            .expect("the carrier has one Expr")
            .0;
        assert_eq!(
            encode_par_match_atom(&carrier),
            encode_match_path(ParCursor::from(&stored)),
            "{name}: protobuf-region and generated-cursor RMP2 disagree"
        );
    }
}

#[test]
fn resident_and_canonical_region_sources_have_identical_semantic_field_projection() {
    let mut compared = 0usize;
    for (name, instance) in par_corpus::every_expr_instance() {
        let carrier = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(instance),
        }]);
        let stored = carrier
            .first::<Expr>()
            .expect("test oracle decodes one admitted Expr")
            .expect("the carrier has one Expr")
            .0;
        let resident = resident_semantic_projection(ParCursor::from(&stored));
        let mut region = None;
        carrier.visit_encoded::<Expr>(|component| {
            region = Some(region_semantic_projection(
                CanonicalRhoPathRegionCursor::admitted_expr(component),
            ));
        });
        assert_eq!(
            resident,
            region.expect("the sole Expr has one admitted component region"),
            "{name}: resident and canonical-region semantic events differ"
        );
        compared += 1;
    }
    assert_eq!(
        compared,
        par_corpus::every_expr_instance().len(),
        "the projection differential must cover every maintained Expr oneof arm"
    );
}

#[test]
fn canonical_region_coalesces_only_adjacent_identical_occurrences() {
    let one = Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(1)),
    };
    let two = Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(2)),
    };
    let nested = Par::default().with_exprs(vec![
        one.clone(),
        one.clone(),
        two.clone(),
        two.clone(),
        two,
    ]);

    let mut resident_records = Vec::new();
    nested.visit_encoded::<Expr>(|entry| {
        resident_records.push((
            u32::from(<Expr as ParBagComponent>::TAG.byte()),
            entry.multiplicity().get(),
            entry.payload().to_vec(),
        ));
    });
    assert_eq!(resident_records.len(), 2);
    assert_eq!(
        resident_records
            .iter()
            .map(|(_, multiplicity, _)| *multiplicity)
            .collect::<Vec<_>>(),
        vec![2, 3],
        "the resident Bag has two exact multiplicity records"
    );

    let protobuf = protobuf_encoder::encode_to_vec(&nested);
    let mut cursor = CanonicalFieldCursor::new(&protobuf);
    let mut region_occurrences = Vec::new();
    while let Some(field) = cursor.next_field().expect("canonical Par protobuf") {
        assert_eq!(
            field.number,
            u32::from(ParComponentTag::Expr.byte()),
            "this fixture contains only Expr component occurrences"
        );
        region_occurrences.push(
            field
                .expect_length_delimited()
                .expect("Expr occurrence is length-delimited")
                .to_vec(),
        );
    }
    let expanded_resident = resident_records
        .iter()
        .flat_map(|(_, multiplicity, payload)| {
            std::iter::repeat_n(payload.clone(), usize::try_from(*multiplicity).unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        region_occurrences, expanded_resident,
        "canonical protobuf must present each equal multiplicity as one adjacent occurrence run"
    );

    let expected_digest: [u8; 32] = Blake2b256::hash_stream(|feed| {
        feed(b"RhoMatchPath/ParBag/components/v2");
        for (family, multiplicity, payload) in &resident_records {
            feed(&family.to_be_bytes());
            feed(&multiplicity.to_be_bytes());
            feed(&u64::try_from(payload.len()).unwrap().to_be_bytes());
            feed(payload);
        }
    })
    .try_into()
    .expect("BLAKE2b-256 emits 32 bytes");
    let expected = (2, 5, expected_digest);
    assert_eq!(par_bag_tokens(&encode_par_match_path(&nested)), vec![
        expected
    ]);

    let carrier = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::EListBody(
            models::rhoapi::EList {
                ps: vec![nested],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            },
        )),
    }]);
    assert_eq!(
        par_bag_tokens(&encode_par_match_atom(&carrier)),
        vec![expected],
        "the canonical-region pending-run coalescer must emit the resident transcript"
    );
}

#[test]
fn nested_par_projection_is_one_opaque_bag_token_plus_root_metadata() {
    fn carrier(nested: Par) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::EListBody(
                models::rhoapi::EList {
                    ps: vec![nested],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                },
            )),
        }])
    }

    let nested = par_corpus::all_par_fields();
    let outer = carrier(nested.clone());
    let stored = outer
        .first::<Expr>()
        .expect("test oracle decodes one admitted Expr")
        .expect("the carrier has one Expr")
        .0;
    let resident = encode_match_path(ParCursor::from(&stored));
    let region = encode_par_match_atom(&outer);
    assert_eq!(resident, region);

    let tokens = EncodedRhoMatchPathCursor::new(&region)
        .expect("RMP2 version")
        .collect::<Result<Vec<_>, _>>()
        .expect("canonical RMP2 path");
    let node_names = tokens
        .iter()
        .filter_map(|token| match token.token {
            RhoMatchPathToken::NodeStart { program } => Some(program.type_name),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        node_names,
        vec!["Expr", "EList", "Par"],
        "no component below the nested ParBag may become a structural child node"
    );
    let opaque = par_bag_tokens(&region);
    assert_eq!(opaque.len(), 1);
    assert_eq!(opaque[0].0, u64::try_from(nested.distinct_len()).unwrap());
    assert_eq!(opaque[0].1, nested.cardinality());

    let mut metadata_changed = nested;
    metadata_changed.replace_locally_free(vec![0xD1, 0xFF]);
    let changed = encode_par_match_atom(&carrier(metadata_changed));
    assert_eq!(
        par_bag_tokens(&changed),
        opaque,
        "root metadata is outside the opaque component transcript"
    );
    assert_ne!(
        changed, region,
        "nested root metadata remains an explicit semantic field after the opaque token"
    );
}

#[test]
fn admitted_expr_regions_preserve_absence_defaults_and_encoding_width_boundaries() {
    use models::rhoapi::expr::ExprInstance;

    let cases = [
        ("absent oneof", Expr::default()),
        ("present default bool", Expr {
            expr_instance: Some(ExprInstance::GBool(false)),
        }),
        ("present empty message", Expr {
            expr_instance: Some(ExprInstance::EListBody(Default::default())),
        }),
        ("negative zigzag boundary", Expr {
            expr_instance: Some(ExprInstance::GInt(-65)),
        }),
        ("positive zigzag boundary", Expr {
            expr_instance: Some(ExprInstance::GInt(64)),
        }),
        ("one-byte length boundary", Expr {
            expr_instance: Some(ExprInstance::GString("x".repeat(127))),
        }),
        ("two-byte length boundary", Expr {
            expr_instance: Some(ExprInstance::GString("x".repeat(128))),
        }),
        ("three-byte length boundary", Expr {
            expr_instance: Some(ExprInstance::GByteArray(vec![0xA5; 16_384])),
        }),
    ];

    let mut paths = Vec::new();
    for (name, expr) in cases {
        let carrier = Par::default().with_exprs(vec![expr]);
        let stored = carrier
            .first::<Expr>()
            .expect("test oracle decodes one admitted Expr")
            .expect("the carrier has one Expr")
            .0;
        let resident = encode_par_match_atom(&carrier);
        let region = encode_match_path(ParCursor::from(&stored));
        assert_eq!(resident, region, "{name}: resident/region RMP2 mismatch");
        paths.push(resident);
    }
    for pair in paths.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "adjacent absence/default/width controls must remain distinguishable"
        );
    }
}

#[test]
fn resident_and_region_par_bag_summaries_match_for_every_canonical_family() {
    let nested = par_corpus::all_par_fields();
    assert!(nested.family_cardinality::<Send>() > 0);
    assert!(nested.family_cardinality::<Receive>() > 0);
    assert!(nested.family_cardinality::<New>() > 0);
    assert!(nested.family_cardinality::<Expr>() > 0);
    assert!(nested.family_cardinality::<Match>() > 0);
    assert!(nested.family_cardinality::<GUnforgeable>() > 0);
    assert!(nested.family_cardinality::<Connective>() > 0);
    assert!(nested.family_cardinality::<Bundle>() > 0);
    assert!(nested.family_cardinality::<If>() > 0);
    let carrier = Par::default().with_exprs(vec![models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::EListBody(
            models::rhoapi::EList {
                ps: vec![nested],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            },
        )),
    }]);
    let stored = carrier
        .first::<models::rhoapi::Expr>()
        .expect("test oracle decodes one admitted Expr")
        .expect("the carrier has one Expr")
        .0;
    assert_eq!(
        encode_par_match_atom(&carrier),
        encode_match_path(ParCursor::from(&stored)),
        "resident and canonical-region summaries must share one RMP2 transcript"
    );
}

#[test]
fn rmp2_parbag_transcript_preserves_multiplicity_without_occurrence_expansion() {
    let one = models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(1)),
    };
    let two = models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(2)),
    };
    let left = Par::default().with_exprs(vec![one.clone(), one.clone(), two.clone()]);
    let right = Par::default().with_exprs(vec![one, two.clone(), two]);
    assert_eq!(left.distinct_len(), right.distinct_len());
    assert_eq!(left.cardinality(), right.cardinality());
    assert_ne!(
        encode_par_match_path(&left),
        encode_par_match_path(&right),
        "equal distinct/cardinality projections must not erase per-component multiplicity"
    );
}

#[test]
fn epathmap_token_uses_the_native_logical_trie_without_a_snapshot() {
    let set_forward = EPathMap::from_set_iter(
        [par_corpus::gint(1), par_corpus::gint(2)],
        Vec::new(),
        false,
        None,
    );
    assert!(format!("{set_forward:?}").contains("snapshot_cached: false"));
    assert_eq!(
        hex::encode(epathmap_match_digest(&set_forward)),
        "9a637685a61e33171ea5e35a90b60d82e92697517a4fc56c02d7fbbeddd5befd"
    );
    assert!(format!("{set_forward:?}").contains("snapshot_cached: false"));

    // Moving a clone into ParBag construction is allowed to materialize its
    // canonical endpoint snapshot. Keep that independent from the native
    // digest subject above: the property under test is that the EPathMap
    // digest itself does not force a snapshot.
    let wrapped_forward = EPathMap::from_set_iter(
        [par_corpus::gint(1), par_corpus::gint(2)],
        Vec::new(),
        false,
        None,
    );
    let wrapped_reverse = EPathMap::from_set_iter(
        [par_corpus::gint(2), par_corpus::gint(1)],
        Vec::new(),
        false,
        None,
    );
    assert_eq!(
        encode_par_match_path(&wrap(wrapped_forward)),
        encode_par_match_path(&wrap(wrapped_reverse))
    );

    let map_a = EPathMap::new_map(
        [(par_corpus::gint(1), par_corpus::gint(10))],
        Vec::new(),
        false,
        None,
    );
    let map_b = EPathMap::new_map(
        [(par_corpus::gint(1), par_corpus::gint(20))],
        Vec::new(),
        false,
        None,
    );
    let set = EPathMap::from_set_iter([par_corpus::gint(1)], Vec::new(), false, None);

    // Map values are intentionally delegated to the native EPathMap matcher.
    // Their shared structural token is a conservative false-positive lane,
    // never an identity or acceptance decision.
    assert_eq!(
        encode_match_path(ParCursor::from(&map_a)),
        encode_match_path(ParCursor::from(&map_b))
    );
    assert_ne!(
        encode_match_path(ParCursor::from(&set)),
        encode_match_path(ParCursor::from(&set_forward))
    );
}

#[test]
fn compact_trie_topology_stream_matches_native_zipper_for_all_endpoint_shapes() {
    use models::rust::canonical_path::encode_trie_path;

    fn assert_region_matches_native(name: &str, map: EPathMap) {
        let carrier = wrap(map);
        let stored = carrier
            .first::<models::rhoapi::Expr>()
            .expect("test oracle decodes one admitted Expr")
            .expect("the carrier has one Expr")
            .0;
        assert_eq!(
            encode_par_match_atom(&carrier),
            encode_match_path(ParCursor::from(&stored)),
            "{name}: compact topology stream differs from live zipper"
        );
    }

    assert_region_matches_native("neutral empty", EPathMap::default());

    let mut deep_line = EPathMap::from_set_iter([par_corpus::gint(1)], Vec::new(), false, None);
    deep_line.create_path(&vec![0xA5; 4_096]).unwrap();
    assert_region_matches_native("deep compressed line", deep_line);

    let endpoint = par_corpus::gint(7);
    let endpoint_path = encode_trie_path(&endpoint);
    let mut branch = EPathMap::from_set_iter([endpoint], Vec::new(), false, None);
    let mut child = endpoint_path.clone();
    child.push(0x7F);
    branch.create_path(&child).unwrap();
    branch.create_path(&[0x20, 0x01]).unwrap();
    branch.create_path(&[0x20, 0x02]).unwrap();
    assert_region_matches_native(
        "branch, value-bearing prefix, and value-free leaves",
        branch,
    );
}

#[test]
fn deep_and_wide_structural_encoding_fits_a_small_native_stack() {
    std::thread::Builder::new()
        .name("rho-match-path".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let deep = encode_par_match_path(&par_corpus::deep_par(20_000));
            assert!(
                deep.len() < 256,
                "the native ParBag token must remain compact regardless of AST depth"
            );

            let wide = extend_wide_fixture(par_corpus::all_par_fields(), 20_000);
            let wide = encode_par_match_path(&wide);
            assert!(
                wide.len() < 256,
                "the native ParBag token must remain compact regardless of sibling width"
            );
        })
        .expect("spawn small-stack structural encoder")
        .join()
        .expect("small-stack structural encoder must complete");
}

fn wrap(map: EPathMap) -> Par {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;

    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }])
}
