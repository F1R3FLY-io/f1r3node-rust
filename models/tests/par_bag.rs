use std::collections::BTreeMap;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EMap, EMinus, ENot, ESet, Expr, GSysAuthToken,
    GUnforgeable, If, KeyValuePair, Match, New, Par, Receive, Send, Var,
};
use models::rust::par_bag::{
    canonical_collection_remainder, canonical_connective_parts, canonical_count_expr_is_hole,
    canonical_expr_oneof_tag, canonical_expr_parts, canonical_if_parts, canonical_match_parts,
    canonical_new_parts, canonical_par_connective_used, canonical_par_locally_free_is_empty,
    canonical_receive_parts, canonical_send_parts, canonical_single_expr_from_par,
    canonical_single_if_from_par, canonical_single_match_from_par, canonical_single_new_from_par,
    canonical_single_receive_from_par, canonical_single_send_from_par, with_component_key,
    CanonicalConnectiveCursorError, CanonicalConnectiveParts, CanonicalExprParts,
    CanonicalParBagKey, ParBag, ParBagCodecError, ParBagProtobufError, ParComponentTag,
    PAR_BAG_KEY_DOMAIN,
};
use models::rust::par_construction::{ParConstruction, PlannedConnective};
use models::rust::rholang::implicits::{single_expr, single_unforgeable};
use models::rust::rholang::protobuf_schema_tables::EXPR_INSTANCE_PROTOBUF_VARIANTS;
use prost::Message as _;
use rspace_plus_plus::rspace::bag::{Bag, BagError, Multiplicity};

mod par_corpus;
use par_corpus as corpus;

/// The shared codec corpus intentionally contains missing required children so
/// protobuf decoders exercise absent-field behavior. Canonical sorting has
/// always rejected those shapes; ParBag's domain is the sortable runtime AST.
fn canonical_par_corpus() -> Par {
    let mut par = Par::default();
    par.replace_sends(vec![Send {
        chan: Some(Par::default()),
        ..Send::default()
    }]);
    par.replace_receives(vec![Receive {
        body: Some(Par::default()),
        ..Receive::default()
    }]);
    par.replace_news(vec![New {
        p: Some(Par::default()),
        ..New::default()
    }]);
    par.replace_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    }]);
    par.replace_matches(vec![Match {
        target: Some(Par::default()),
        ..Match::default()
    }]);
    par.replace_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
    }]);
    par.replace_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::ConnBool(true)),
    }]);
    par.replace_bundles(vec![Bundle {
        body: Some(Par::default()),
        ..Bundle::default()
    }]);
    par.replace_conditionals(vec![If {
        condition: Some(Par::default()),
        if_true: Some(Par::default()),
        if_false: Some(Par::default()),
        ..If::default()
    }]);
    par
}

fn connective_bytes(values: &[i64]) -> Vec<u8> {
    models::rust::rholang::protobuf_encoder::encode_to_vec(&Connective {
        connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
            ps: values
                .iter()
                .map(|value| {
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(*value)),
                    }])
                })
                .collect(),
        })),
    })
}

fn connective_children(bytes: &[u8]) -> models::rust::par_bag::CanonicalConnectiveChildren<'_> {
    match canonical_connective_parts(bytes).expect("canonical connective") {
        CanonicalConnectiveParts::And(children) => children,
        other => panic!("expected AND children, got {other:?}"),
    }
}

#[test]
fn canonical_connective_cursor_refuses_a_different_or_truncated_body() {
    let first = connective_bytes(&[1, 2]);
    let second = connective_bytes(&[3, 4]);
    let first_children = connective_children(&first);
    let second_children = connective_children(&second);
    assert_eq!(
        first_children.message().len(),
        second_children.message().len()
    );

    let truncated = &first_children.message()[..first_children.message().len() - 1];
    let expected = first_children.clone().collect::<Vec<_>>();
    let mut borrowed = first_children;
    assert_eq!(borrowed.len(), expected.len());
    for expected_child in &expected {
        assert_eq!(borrowed.next(), Some(*expected_child));
    }
    assert!(borrowed.is_empty());
    assert_eq!(borrowed.next(), None);

    assert!(matches!(
        first_children.bind_owned(second_children.message()),
        Err(CanonicalConnectiveCursorError::BodyMismatch)
    ));
    assert!(matches!(
        first_children.bind_owned(truncated),
        Err(CanonicalConnectiveCursorError::BodyMismatch)
    ));
    let copied_body = first_children.message().to_vec();
    assert!(matches!(
        first_children.bind_owned(copied_body),
        Err(CanonicalConnectiveCursorError::BodyMismatch)
    ));

    let message = first_children.message();
    let mut owned = first_children
        .bind_owned(message)
        .expect("the exact borrowed body becomes its immutable owner");
    assert_eq!(owned.len(), expected.len());
    for expected_child in &expected {
        let range = owned.next().expect("owned cursor retains every child");
        assert_eq!(&owned.body()[range], *expected_child);
    }
    assert!(owned.is_empty());
    assert_eq!(owned.next(), None);

    let mut partial = first_children;
    let skipped = partial
        .next()
        .expect("the borrowed cursor has a first child");
    assert_eq!(skipped, expected[0]);
    let mut owned = partial
        .bind_owned(message)
        .expect("an advanced cursor transfers its exact current position");
    assert_eq!(owned.len(), 1);
    let range = owned.next().expect("the remaining child survives transfer");
    assert_eq!(&owned.body()[range], expected[1]);
    assert!(owned.is_empty());

    let mut reused = first.clone();
    {
        let old_children = connective_children(&reused);
        assert_eq!(old_children.clone().collect::<Vec<_>>(), expected);
    }
    reused.copy_from_slice(&second);
    let replacement_children = connective_children(&reused);
    assert_eq!(
        replacement_children.clone().collect::<Vec<_>>(),
        second_children.collect::<Vec<_>>()
    );
}

#[test]
fn canonical_connective_cursor_switching_as_ref_counterexample() {
    use std::cell::Cell;
    use std::sync::Arc;

    struct SwitchingOwner {
        first: Arc<Vec<u8>>,
        second: Arc<Vec<u8>>,
        body_start: usize,
        switched: Cell<bool>,
    }

    impl AsRef<[u8]> for SwitchingOwner {
        fn as_ref(&self) -> &[u8] {
            if self.switched.replace(true) {
                &self.second[self.body_start..]
            } else {
                &self.first[self.body_start..]
            }
        }
    }

    let first = Arc::new(connective_bytes(&[1]));
    let second = Arc::new(connective_bytes(&[2]));
    let children = connective_children(first.as_slice());
    let owner = SwitchingOwner {
        first: Arc::clone(&first),
        second,
        body_start: first.len() - children.message().len(),
        switched: Cell::new(false),
    };
    let initial = owner.as_ref();
    assert_eq!(initial.as_ptr(), children.message().as_ptr());
    assert_eq!(initial, children.message());
    let resumed = owner.as_ref();
    assert_eq!(resumed.len(), initial.len());
    assert_ne!(resumed.as_ptr(), initial.as_ptr());
    assert_ne!(resumed, initial);
    assert_eq!(
        resumed,
        connective_children(owner.second.as_slice()).message()
    );
}

#[test]
fn canonical_connective_cursor_protobuf_region_owner_stability() {
    use models::rust::rholang::protobuf_cursor::{CanonicalByteRange, ProtobufRegion};

    let region = ProtobufRegion::from(connective_bytes(&[1, 2]));
    let children = connective_children(region.as_bytes());
    let expected = children.clone().collect::<Vec<_>>();
    let body = region
        .subregion(CanonicalByteRange {
            start: region.len() - children.message().len(),
            end: region.len(),
        })
        .expect("validated body is a subregion");
    let mut owned = children.bind_owned(body).expect("owner preserves the body");
    for expected_child in expected {
        let range = owned.next().expect("validated child remains present");
        assert_eq!(&owned.body().as_bytes()[range], expected_child);
    }
    assert!(owned.is_empty());
    assert_eq!(owned.next(), None);
}

#[test]
fn canonical_connective_cursor_child_length_varint_boundaries() {
    fn framed_width(mut len: usize) -> usize {
        let mut width = 1;
        while len >= 128 {
            len >>= 7;
            width += 1;
        }
        width
    }

    for boundary in [128usize, 16_384] {
        let mut below = None;
        let mut above = None;
        for payload_len in boundary.saturating_sub(32)..=boundary {
            let child = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("x".repeat(payload_len))),
            }]);
            let encoded_len = models::rust::rholang::protobuf_encoder::encode_to_vec(&child).len();
            if encoded_len < boundary {
                below = Some((child, encoded_len));
            } else {
                above = Some((child, encoded_len));
                break;
            }
        }
        let (below, below_len) = below.expect("child below varint boundary");
        let (above, above_len) = above.expect("child above varint boundary");
        assert_eq!(framed_width(below_len) + 1, framed_width(above_len));

        let expected = [
            models::rust::rholang::protobuf_encoder::encode_to_vec(&below),
            models::rust::rholang::protobuf_encoder::encode_to_vec(&above),
        ];
        let bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![below, above],
            })),
        });
        let children = connective_children(&bytes);
        assert_eq!(children.len(), 2);
        let mut owned = children
            .bind_owned(children.message())
            .expect("exact large body binds");
        let first = owned.next().expect("first framed child");
        assert_eq!(first.start, 1 + framed_width(below_len));
        assert_eq!(&owned.body()[first.clone()], expected[0].as_slice());
        let second = owned.next().expect("second framed child");
        assert_eq!(second.start, first.end + 1 + framed_width(above_len));
        assert_eq!(&owned.body()[second], expected[1].as_slice());
        assert!(owned.is_empty());
        assert_eq!(owned.next(), None);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn generated_connective_cursor_owner_pairing_and_exhaustion(
        values in proptest::collection::vec(0u8..64u8, 0..33),
        split in proptest::prelude::any::<usize>(),
    ) {
        let first_values = values.iter().map(|value| i64::from(*value)).collect::<Vec<_>>();
        let second_values = values
            .iter()
            .map(|value| i64::from((*value + 1) % 64))
            .collect::<Vec<_>>();
        let first = connective_bytes(&first_values);
        let second = connective_bytes(&second_values);
        let first_children = connective_children(&first);
        let second_children = connective_children(&second);
        proptest::prop_assert_eq!(first_children.message().len(), second_children.message().len());
        proptest::prop_assert_eq!(first_children.len(), values.len());
        proptest::prop_assert!(matches!(
            first_children.bind_owned(second_children.message()),
            Err(CanonicalConnectiveCursorError::BodyMismatch)
        ));
        proptest::prop_assert!(matches!(
            first_children.bind_owned(first_children.message().to_vec()),
            Err(CanonicalConnectiveCursorError::BodyMismatch)
        ));
        if !first_children.message().is_empty() {
            let truncated = &first_children.message()[..first_children.message().len() - 1];
            proptest::prop_assert!(matches!(
                first_children.bind_owned(truncated),
                Err(CanonicalConnectiveCursorError::BodyMismatch)
            ));
        }

        let expected = first_children.clone().collect::<Vec<_>>();
        let mut next_header = 0usize;
        let expected_ranges = expected
            .iter()
            .map(|child| {
                let mut length = child.len();
                let mut width = 1usize;
                while length >= 128 {
                    length >>= 7;
                    width += 1;
                }
                let start = next_header + 1 + width;
                let end = start + child.len();
                next_header = end;
                start..end
            })
            .collect::<Vec<_>>();
        proptest::prop_assert_eq!(next_header, first_children.message().len());
        let mut owned = first_children
            .bind_owned(first_children.message())
            .expect("original body binds");
        for (expected_child, expected_range) in expected.iter().zip(&expected_ranges) {
            let range = owned.next().expect("validated child remains present");
            proptest::prop_assert_eq!(&range, expected_range);
            proptest::prop_assert_eq!(&owned.body()[range], *expected_child);
        }
        proptest::prop_assert!(owned.is_empty());
        proptest::prop_assert_eq!(owned.next(), None);

        let consumed = split % (expected.len() + 1);
        let mut partial = first_children;
        for expected_child in expected.iter().take(consumed) {
            proptest::prop_assert_eq!(partial.next(), Some(*expected_child));
        }
        proptest::prop_assert_eq!(partial.len(), expected.len() - consumed);
        let mut retained = partial
            .bind_owned(first_children.message())
            .expect("advanced cursor binds its original body");
        proptest::prop_assert_eq!(retained.len(), expected.len() - consumed);
        for (expected_child, expected_range) in
            expected.iter().zip(&expected_ranges).skip(consumed)
        {
            let range = retained.next().expect("remaining validated child remains present");
            proptest::prop_assert_eq!(&range, expected_range);
            proptest::prop_assert_eq!(&retained.body()[range], *expected_child);
        }
        proptest::prop_assert!(retained.is_empty());
        proptest::prop_assert_eq!(retained.next(), None);

        let mut exhausted = first_children;
        while exhausted.next().is_some() {}
        proptest::prop_assert!(exhausted.is_empty());
        proptest::prop_assert!(matches!(
            exhausted.bind_owned(second_children.message()),
            Err(CanonicalConnectiveCursorError::BodyMismatch)
        ));

        let mut reused = first.clone();
        {
            let old_children = connective_children(&reused);
            proptest::prop_assert_eq!(old_children.len(), values.len());
        }
        reused.copy_from_slice(&second);
        let replacement_children = connective_children(&reused);
        proptest::prop_assert_eq!(
            replacement_children.collect::<Vec<_>>(),
            second_children.collect::<Vec<_>>()
        );
    }
}

#[test]
fn canonical_if_cursor_borrows_children_and_rejects_noncanonical_framing() {
    let condition = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(11)),
    }]);
    let if_true = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(12)),
    }]);
    let if_false = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(13)),
    }]);
    let conditional = If {
        condition: Some(condition.clone()),
        if_true: Some(if_true.clone()),
        if_false: Some(if_false.clone()),
        locally_free: vec![1],
        connective_used: true,
    };
    let conditional_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&conditional);
    let parts = canonical_if_parts(&conditional_bytes).expect("canonical If cursor");
    assert_eq!(
        parts.condition,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&condition)
    );
    assert_eq!(
        parts.if_true,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&if_true)
    );
    assert_eq!(
        parts.if_false,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&if_false)
    );

    let mut carrier = Par::default().with_conditionals(vec![conditional]);
    carrier.replace_locally_free(vec![2]);
    carrier.replace_connective_used(true);
    let carrier_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&carrier);
    assert_eq!(canonical_par_connective_used(&carrier_bytes), Some(true));
    assert_eq!(
        canonical_par_connective_used(&models::rust::rholang::protobuf_encoder::encode_to_vec(
            &condition
        )),
        Some(false)
    );
    assert_eq!(
        canonical_single_if_from_par(&carrier_bytes),
        Some(conditional_bytes.as_slice())
    );

    let mut trailing_if = conditional_bytes.clone();
    trailing_if.extend_from_slice(&[0x30, 0x01]);
    assert!(canonical_if_parts(&trailing_if).is_none());
    let mut trailing_carrier = carrier_bytes;
    trailing_carrier.extend_from_slice(&[0x08, 0x01]);
    assert!(canonical_single_if_from_par(&trailing_carrier).is_none());
}

#[test]
fn canonical_new_and_send_cursors_borrow_children_and_reject_noncanonical_framing() {
    let channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(21)),
    }]);
    let datum = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(22)),
    }]);
    let body = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(23)),
    }]);

    let send = Send {
        chan: Some(channel.clone()),
        data: vec![datum.clone()],
        persistent: true,
        locally_free: vec![1],
        connective_used: true,
        cost_region: None,
    };
    let send_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&send);
    let send_parts = canonical_send_parts(&send_bytes).expect("canonical Send cursor");
    assert_eq!(
        send_parts.channel,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&channel)
    );
    assert_eq!(send_parts.data.len(), 1);
    assert_eq!(
        send_parts.data[0],
        models::rust::rholang::protobuf_encoder::encode_to_vec(&datum)
    );
    assert!(send_parts.persistent);

    let new = New {
        bind_count: 3,
        p: Some(body.clone()),
        uri: vec!["rho:id:one".to_owned()],
        injections: BTreeMap::from([("x".to_owned(), datum)]),
        locally_free: vec![2],
    };
    let new_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&new);
    let new_parts = canonical_new_parts(&new_bytes).expect("canonical New cursor");
    assert_eq!(
        new_parts.bind_count, 6,
        "sint32 is retained as zig-zag wire data"
    );
    assert_eq!(
        new_parts.body,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&body)
    );

    let send_carrier = Par::default().with_sends(vec![send]);
    let send_carrier_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&send_carrier);
    assert_eq!(
        canonical_single_send_from_par(&send_carrier_bytes),
        Some(send_bytes.as_slice())
    );
    let new_carrier = Par::default().with_news(vec![new]);
    let new_carrier_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&new_carrier);
    assert_eq!(
        canonical_single_new_from_par(&new_carrier_bytes),
        Some(new_bytes.as_slice())
    );

    let mut malformed_send = send_bytes;
    malformed_send.extend_from_slice(&[0x18, 0x01]);
    assert!(canonical_send_parts(&malformed_send).is_none());
    let mut malformed_new = new_bytes;
    malformed_new.extend_from_slice(&[0x10, 0x00]);
    assert!(canonical_new_parts(&malformed_new).is_none());
    let mut duplicate_root_metadata = send_carrier_bytes;
    duplicate_root_metadata.extend_from_slice(&[0x52, 0x01, 0x01]);
    assert!(canonical_single_send_from_par(&duplicate_root_metadata).is_none());
}

#[test]
fn canonical_recursive_family_cursors_expose_only_semantic_children() {
    let child = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(31)),
    }]);
    let child_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&child);

    let receive = Receive {
        body: Some(child.clone()),
        peek: true,
        bind_count: 2,
        locally_free: vec![1],
        connective_used: true,
        condition: Some(child.clone()),
        ..Receive::default()
    };
    let receive_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&receive);
    let receive_parts = canonical_receive_parts(&receive_bytes).expect("canonical Receive cursor");
    assert!(receive_parts.binds.is_empty());
    assert_eq!(receive_parts.body, child_bytes);
    assert!(!receive_parts.persistent);

    let match_term = Match {
        target: Some(child.clone()),
        locally_free: vec![2],
        connective_used: true,
        ..Match::default()
    };
    let match_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&match_term);
    let match_parts = canonical_match_parts(&match_bytes).expect("canonical Match cursor");
    assert_eq!(match_parts.target, child_bytes);
    assert!(match_parts.cases.is_empty());

    let expression = Expr {
        expr_instance: Some(ExprInstance::ENotBody(ENot {
            p: Some(child.clone()),
        })),
    };
    let expression_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&expression);
    assert_eq!(
        canonical_expr_parts(&expression_bytes),
        Some(CanonicalExprParts::Unary {
            tag: 5,
            child: child_bytes.as_slice(),
        })
    );

    let receive_carrier = Par::default().with_receives(vec![receive]);
    assert_eq!(
        canonical_single_receive_from_par(&models::rust::rholang::protobuf_encoder::encode_to_vec(
            &receive_carrier
        )),
        Some(receive_bytes.as_slice())
    );
    let match_carrier = Par::default().with_matches(vec![match_term]);
    assert_eq!(
        canonical_single_match_from_par(&models::rust::rholang::protobuf_encoder::encode_to_vec(
            &match_carrier
        )),
        Some(match_bytes.as_slice())
    );
    let expr_carrier = Par::default().with_exprs(vec![expression]);
    assert_eq!(
        canonical_single_expr_from_par(&models::rust::rholang::protobuf_encoder::encode_to_vec(
            &expr_carrier
        )),
        Some(expression_bytes.as_slice())
    );

    let mut malformed_receive = receive_bytes;
    malformed_receive.extend_from_slice(&[0x18, 0x00]);
    assert!(canonical_receive_parts(&malformed_receive).is_none());
    let mut malformed_match = match_bytes;
    malformed_match.extend_from_slice(&[0x2a, 0x00]);
    assert!(canonical_match_parts(&malformed_match).is_none());
    let mut malformed_expr = expression_bytes;
    malformed_expr.extend_from_slice(&[0x28, 0x00]);
    assert!(canonical_expr_parts(&malformed_expr).is_none());
}

#[test]
fn canonical_expr_envelope_separates_outer_variant_proof_from_child_validity() {
    let encode = |instance| {
        models::rust::rholang::protobuf_encoder::encode_to_vec(&Expr {
            expr_instance: Some(instance),
        })
    };
    let child = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    }]);

    for (instance, expected_tag) in [
        (ExprInstance::GBool(true), 1),
        (ExprInstance::GInt(-7), 2),
        (ExprInstance::GString("rho".to_owned()), 3),
        (ExprInstance::GDouble((-0.0f64).to_bits()), 34),
    ] {
        let bytes = encode(instance);
        assert_eq!(canonical_expr_oneof_tag(&bytes), Some(expected_tag));
        assert!(
            canonical_expr_parts(&bytes).is_none(),
            "scalar tag {expected_tag} is an envelope, not a recursive cursor family"
        );
    }

    let absent_left = encode(ExprInstance::EMinusBody(EMinus {
        p1: None,
        p2: Some(child.clone()),
    }));
    assert_eq!(canonical_expr_oneof_tag(&absent_left), Some(10));
    assert!(
        canonical_expr_parts(&absent_left).is_none(),
        "the shallow envelope must not bless an absent required child"
    );

    let well_formed = encode(ExprInstance::EMinusBody(EMinus {
        p1: Some(child.clone()),
        p2: Some(child),
    }));
    assert_eq!(canonical_expr_oneof_tag(&well_formed), Some(10));
    assert!(matches!(
        canonical_expr_parts(&well_formed),
        Some(CanonicalExprParts::Binary { tag: 10, .. })
    ));

    // This cursor proves only the outer arm under authoritative ParBag-byte
    // provenance. Payload semantics remain the generated decoder's job.
    assert_eq!(canonical_expr_oneof_tag(&[0x08, 0x02]), Some(1));
    assert_eq!(canonical_expr_oneof_tag(&[0x1a, 0x01, 0xff]), Some(3));

    let duplicate_bool = {
        let one = encode(ExprInstance::GBool(true));
        [one.as_slice(), one.as_slice()].concat()
    };
    for malformed in [
        Vec::new(),
        vec![0x2a, 0x01],                      // truncated length-delimited body
        vec![0xaa, 0x00, 0x00],                // non-minimal field key
        vec![0x08, 0x80, 0x00],                // non-minimal varint value
        vec![0x2a, 0x80, 0x00],                // non-minimal length
        duplicate_bool,                        // a second oneof field
        vec![0x2b, 0x2c],                      // groups are not Expr oneof encodings
        vec![0x92, 0x02, 0x00],                // fixed64 tag with length-delimited wire
        vec![0x91, 0x02, 0, 0, 0, 0, 0, 0, 0], // truncated fixed64 payload
    ] {
        assert_eq!(canonical_expr_oneof_tag(&malformed), None);
    }
}

#[test]
fn canonical_expr_outer_envelope_rejects_wrong_wire_for_every_generated_arm() {
    let arms = corpus::every_expr_instance();
    assert_eq!(arms.len(), EXPR_INSTANCE_PROTOBUF_VARIANTS.len());
    for ((label, instance), field) in arms.into_iter().zip(EXPR_INSTANCE_PROTOBUF_VARIANTS) {
        assert_eq!(label, field.name);
        let mut bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&Expr {
            expr_instance: Some(instance),
        });
        let actual = field
            .kind
            .wire_type()
            .expect("a selected oneof arm has one concrete wire type")
            .code() as u8;
        let wrong = [0u8, 1, 2, 5]
            .into_iter()
            .find(|wire| *wire != actual)
            .expect("the protobuf alphabet has more than one wire type");
        bytes[0] = (bytes[0] & !0x07) | wrong;
        assert_eq!(
            canonical_expr_oneof_tag(&bytes),
            None,
            "{label}: a descriptor-incompatible wire type was accepted"
        );
    }
}

fn assert_count_expr_classification(expr: &Expr) -> Vec<u8> {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::EVar;
    use models::rust::rholang::{protobuf_decoder, protobuf_encoder};

    let bytes = protobuf_encoder::encode_to_vec(expr);
    let decoded =
        protobuf_decoder::decode_expr(bytes.as_slice()).expect("generated Expr bytes decode");
    let expected = matches!(
        decoded.expr_instance,
        Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::FreeVar(_) | VarInstance::Wildcard(_)),
            }),
        }))
    );
    assert_eq!(canonical_count_expr_is_hole(&bytes), Some(expected));
    bytes
}

#[test]
fn canonical_count_expr_is_hole_covers_every_generated_arm() {
    let arms = corpus::every_expr_instance();
    assert_eq!(EXPR_INSTANCE_PROTOBUF_VARIANTS.len(), 36);
    assert_eq!(arms.len(), EXPR_INSTANCE_PROTOBUF_VARIANTS.len());
    for ((label, instance), field) in arms.into_iter().zip(EXPR_INSTANCE_PROTOBUF_VARIANTS) {
        assert_eq!(label, field.name);
        let mut bytes = assert_count_expr_classification(&Expr {
            expr_instance: Some(instance),
        });
        let actual = field
            .kind
            .wire_type()
            .expect("a selected oneof arm has one concrete wire type")
            .code() as u8;
        for wrong in [0u8, 1, 2, 5].into_iter().filter(|wire| *wire != actual) {
            bytes[0] = (bytes[0] & !0x07) | wrong;
            assert_eq!(
                canonical_count_expr_is_hole(&bytes),
                None,
                "{label}: a descriptor-incompatible wire type was classified"
            );
        }
    }
}

#[test]
fn canonical_count_expr_is_hole_preserves_missing_forms_and_signed_variables() {
    use models::rhoapi::var::{VarInstance, WildcardMsg};
    use models::rhoapi::{EMethod, EVar};

    let mut expressions = vec![
        Expr::default(),
        Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar { v: None })),
        },
        Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var::default()),
            })),
        },
        Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                }),
            })),
        },
        Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot { p: None })),
        },
        Expr {
            expr_instance: Some(ExprInstance::EMinusBody(EMinus { p1: None, p2: None })),
        },
        Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod::default())),
        },
    ];
    for level in [i32::MIN, -1, 0, 1, i32::MAX] {
        for variable in [VarInstance::FreeVar(level), VarInstance::BoundVar(level)] {
            expressions.push(Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(variable),
                    }),
                })),
            });
        }
    }
    for expr in expressions {
        assert_count_expr_classification(&expr);
    }
}

#[test]
fn canonical_count_expr_is_hole_refuses_unproved_framing() {
    let one = assert_count_expr_classification(&Expr {
        expr_instance: Some(ExprInstance::GBool(true)),
    });
    for malformed in [
        vec![0x2a, 0x01],
        vec![0xaa, 0x00, 0x00],
        vec![0x08, 0x80, 0x00],
        vec![0x2a, 0x80, 0x00],
        [one.as_slice(), one.as_slice()].concat(),
        vec![0x2b, 0x2c],
        vec![0x92, 0x02, 0x00],
        vec![0x91, 0x02, 0, 0, 0, 0, 0, 0, 0],
        vec![0xf8, 0x07, 0x00],
        vec![0x9a, 0x01, 0x02, 0x12, 0x00],
        vec![0x9a, 0x01, 0x04, 0x0a, 0x02, 0x20, 0x00],
        vec![
            0x9a, 0x01, 0x08, 0x0a, 0x06, 0x10, 0x80, 0x80, 0x80, 0x80, 0x10,
        ],
        vec![0x9a, 0x01, 0x06, 0x0a, 0x04, 0x10, 0x00, 0x10, 0x01],
        vec![0x9a, 0x01, 0x05, 0x0a, 0x03, 0x1a, 0x01, 0x00],
        vec![0x9a, 0x01, 0x04, 0x0a, 0x00, 0x0a, 0x00],
    ] {
        assert_eq!(
            canonical_count_expr_is_hole(&malformed),
            None,
            "unproved Expr framing was classified: {malformed:02x?}"
        );
    }
}

#[test]
fn canonical_unordered_collection_cursors_borrow_members_and_remainders() {
    use models::rhoapi::var::{VarInstance, WildcardMsg};

    let key = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(41)),
    }]);
    let value = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(42)),
    }]);
    let free_remainder = Var {
        var_instance: Some(VarInstance::FreeVar(-7)),
    };
    let free_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&free_remainder);

    let set_expr = Expr {
        expr_instance: Some(ExprInstance::ESetBody(ESet {
            ps: vec![value.clone()],
            locally_free: vec![1],
            connective_used: true,
            remainder: Some(free_remainder),
        })),
    };
    let set_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&set_expr);
    let Some(CanonicalExprParts::Sequence {
        tag: 22,
        children,
        remainder: Some(remainder),
    }) = canonical_expr_parts(&set_bytes)
    else {
        panic!("canonical ESet shape")
    };
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0],
        models::rust::rholang::protobuf_encoder::encode_to_vec(&value)
    );
    assert_eq!(remainder, free_bytes);
    assert_eq!(
        canonical_collection_remainder(remainder),
        Some((Some(-7), false))
    );

    let wildcard = Var {
        var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
    };
    let wildcard_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&wildcard);
    let map_expr = Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(key.clone()),
                value: Some(value.clone()),
            }],
            locally_free: vec![2],
            connective_used: true,
            remainder: Some(wildcard),
        })),
    };
    let map_bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&map_expr);
    let Some(CanonicalExprParts::Map {
        entries,
        remainder: Some(remainder),
    }) = canonical_expr_parts(&map_bytes)
    else {
        panic!("canonical EMap shape")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].0,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&key)
    );
    assert_eq!(
        entries[0].1,
        models::rust::rholang::protobuf_encoder::encode_to_vec(&value)
    );
    assert_eq!(remainder, wildcard_bytes);
    assert_eq!(
        canonical_collection_remainder(remainder),
        Some((None, true))
    );

    let plain = models::rust::rholang::protobuf_encoder::encode_to_vec(&Par::default());
    assert_eq!(canonical_par_locally_free_is_empty(&plain), Some(true));
    let mut summarized = Par::default();
    summarized.replace_locally_free(vec![1]);
    assert_eq!(
        canonical_par_locally_free_is_empty(
            &models::rust::rholang::protobuf_encoder::encode_to_vec(&summarized)
        ),
        Some(false)
    );

    let bound = Var {
        var_instance: Some(VarInstance::BoundVar(1)),
    };
    assert_eq!(
        canonical_collection_remainder(&models::rust::rholang::protobuf_encoder::encode_to_vec(
            &bound
        )),
        None
    );
    assert_eq!(canonical_collection_remainder(&[0x1a, 0x01, 0x00]), None);
}

fn occurrence_count(par: &Par) -> usize {
    usize::try_from(par.cardinality()).expect("fixture cardinality fits usize")
}

#[test]
fn singleton_projections_require_one_total_bag_occurrence() {
    let bundle = Bundle {
        body: Some(Par::default()),
        write_flag: true,
        read_flag: false,
    };
    let connective = Connective {
        connective_instance: Some(ConnectiveInstance::ConnBool(true)),
    };
    let conditional = If {
        condition: Some(Par::default()),
        if_true: Some(Par::default()),
        if_false: Some(Par::default()),
        ..If::default()
    };
    let unforgeable = GUnforgeable {
        unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
    };

    assert_eq!(
        Par::default()
            .with_bundles(vec![bundle.clone()])
            .single_bundle(),
        Some(bundle.clone())
    );
    assert!(Par::default()
        .with_bundles(vec![bundle])
        .with_conditionals(vec![conditional.clone()])
        .single_bundle()
        .is_none());
    assert_eq!(
        Par::default()
            .with_connectives(vec![connective.clone()])
            .single_connective(),
        Some(connective.clone())
    );
    assert!(Par::default()
        .with_connectives(vec![connective.clone()])
        .with_conditionals(vec![conditional])
        .single_connective()
        .is_none());
    assert!(Par::default()
        .with_connectives(vec![connective])
        .with_unforgeables(vec![unforgeable])
        .single_connective()
        .is_none());

    let expression = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let unforgeable = GUnforgeable {
        unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
    };
    assert_eq!(
        single_expr(&Par::default().with_exprs(vec![expression.clone()])),
        Some(expression.clone())
    );
    assert!(single_expr(
        &Par::default()
            .with_exprs(vec![expression])
            .with_unforgeables(vec![unforgeable.clone()])
    )
    .is_none());
    assert_eq!(
        single_unforgeable(&Par::default().with_unforgeables(vec![unforgeable.clone()])),
        Some(unforgeable.clone())
    );
    assert!(single_unforgeable(
        &Par::default()
            .with_unforgeables(vec![unforgeable])
            .with_conditionals(vec![If {
                condition: Some(Par::default()),
                if_true: Some(Par::default()),
                if_false: Some(Par::default()),
                ..If::default()
            }])
    )
    .is_none());

    assert!(Par::default().is_nil());
    assert!(!Par::default()
        .with_conditionals(vec![If {
            condition: Some(Par::default()),
            if_true: Some(Par::default()),
            if_false: Some(Par::default()),
            ..If::default()
        }])
        .is_nil());
}

#[test]
fn all_components_are_is_exact_across_families_and_saturated_cardinality() {
    let expression = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let mut pure = Par::default();
    pure.insert_component_many(
        &expression,
        Multiplicity::new(u64::MAX).expect("positive maximum multiplicity"),
        Vec::new(),
        false,
    )
    .expect("one endpoint can hold the maximum multiplicity");
    pure.insert_component(
        &Expr {
            expr_instance: Some(ExprInstance::GInt(2)),
        },
        Vec::new(),
        false,
    )
    .expect("a second Expr endpoint may saturate projected cardinality");

    assert!(pure.cardinality_is_saturated());
    assert!(pure.family_cardinality_is_saturated::<Expr>());
    assert!(
        pure.all_components_are::<Expr>(),
        "arbitrary Expr multiplicity remains a pure Expr family"
    );

    pure.insert_component(&Send::default(), Vec::new(), false)
        .expect("foreign family endpoint");
    assert_eq!(
        pure.cardinality(),
        pure.family_cardinality::<Expr>(),
        "the projected counters intentionally demonstrate why cardinality equality is unsound"
    );
    assert!(
        !pure.all_components_are::<Expr>(),
        "an exact family-domain predicate must see the foreign endpoint after saturation"
    );

    assert!(Par::default().all_components_are::<Expr>());
    assert!(Par::default().all_components_are::<If>());
}

#[test]
fn family_partitions_are_lazy_multiplicity_allocations_in_legacy_first_order() {
    let first = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let second = Expr {
        expr_instance: Some(ExprInstance::GInt(2)),
    };
    let mut bag = ParBag::new();
    bag.insert_many(&first, Multiplicity::new(3).expect("positive multiplicity"))
        .expect("first endpoint");
    bag.insert_many(
        &second,
        Multiplicity::new(2).expect("positive multiplicity"),
    )
    .expect("second endpoint");

    let allocations = bag
        .family_partitions::<Expr>(1, 2)
        .map(|(selected, complement)| {
            (
                selected.count(&first),
                selected.count(&second),
                complement.count(&first),
                complement.count(&second),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(allocations, vec![
        (1, 0, 2, 2),
        (2, 0, 1, 2),
        (0, 1, 3, 1),
        (1, 1, 2, 1),
        (0, 2, 3, 0),
    ]);

    let mut compressed = ParBag::new();
    compressed
        .insert_many(
            &first,
            Multiplicity::new(u64::MAX).expect("positive maximum multiplicity"),
        )
        .expect("one endpoint stores the full multiplicity domain");
    let mut partitions = compressed.family_partitions::<Expr>(u64::MAX, u64::MAX);
    let (selected, complement) = partitions
        .next()
        .expect("the exact full-cardinality partition exists");
    assert_eq!(selected.count(&first), u64::MAX);
    assert!(complement.is_empty());
    assert!(partitions.next().is_none());
}

#[test]
fn planned_import_retains_singleton_bundle_and_connective_algebra() {
    let leaf = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    }]);
    let bundle = Bundle {
        body: Some(leaf.clone()),
        write_flag: true,
        read_flag: false,
    };
    let connective = Connective {
        connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
            ps: vec![leaf.clone(), Par::default()],
        })),
    };

    let mut construction = ParConstruction::new();
    let planned_bundle = construction
        .import_par(&Par::default().with_bundles(vec![bundle]))
        .expect("import singleton Bundle");
    let (planned_body, write_flag, read_flag) = construction
        .single_bundle(planned_bundle)
        .expect("retain singleton Bundle shape");
    assert_eq!(construction.finish(planned_body).unwrap(), leaf);
    assert!(write_flag);
    assert!(!read_flag);

    let planned_connective = construction
        .import_par(&Par::default().with_connectives(vec![connective]))
        .expect("import singleton Connective");
    let PlannedConnective::And(children) = construction
        .single_connective(planned_connective)
        .expect("retain singleton Connective shape")
    else {
        panic!("imported ConnAnd must retain its planned algebraic shape");
    };
    assert_eq!(children.len(), 2);
    assert_eq!(construction.finish(children[0]).unwrap(), leaf);
    assert_eq!(construction.finish(children[1]).unwrap(), Par::default());
}

#[test]
fn empty_par_is_one_unambiguous_empty_trie() {
    let imported = Par::default().into_components();
    assert!(imported.is_empty());
    assert_eq!(imported.distinct_len(), 0);
    assert_eq!(imported.cardinality(), 0);
    assert_eq!(
        ParBag::from_compact_bytes(&imported.compact_bytes()).expect("empty image"),
        imported
    );
}

#[test]
fn legacy_import_preserves_every_typed_occurrence() {
    let par = canonical_par_corpus();
    let bag = par.clone().into_components();
    assert_eq!(
        bag.cardinality(),
        u64::try_from(occurrence_count(&par)).expect("fixture cardinality fits u64")
    );

    for value in par.decode_occurrences::<Send>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Receive>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<New>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Expr>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Match>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<GUnforgeable>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Connective>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Bundle>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<If>() {
        assert!(bag.contains(&value));
    }
}

#[test]
fn equal_occurrences_collapse_to_one_path_with_checked_multiplicity() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    };
    let par = Par::default().with_exprs(vec![value.clone(), value.clone(), value.clone()]);
    let bag = par.into_components();
    assert_eq!(bag.distinct_len(), 1);
    assert_eq!(bag.cardinality(), 3);
    assert_eq!(bag.count(&value), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
}

#[test]
fn compiled_typed_keys_reuse_the_exact_authoritative_path() {
    let present = Expr {
        expr_instance: Some(ExprInstance::GString("rho:prepared".into())),
    };
    let absent = Expr {
        expr_instance: Some(ExprInstance::GString("rho:absent".into())),
    };
    let present_key = CanonicalParBagKey::compile(&present);
    let absent_key = CanonicalParBagKey::compile(&absent);
    assert_eq!(
        present_key.as_bytes(),
        with_component_key(&present, <[u8]>::to_vec)
    );

    let mut bag = ParBag::new();
    bag.insert_key_many(&present_key, Multiplicity::new(3).expect("positive"))
        .expect("insert compiled key");
    assert!(bag.contains_key(&present_key));
    assert!(!bag.contains_key(&absent_key));
    assert_eq!(bag.count_key(&present_key), 3);
    assert_eq!(
        bag.remove_key_many(&present_key, Multiplicity::new(2).expect("positive")),
        Some(Multiplicity::new(1).expect("positive"))
    );
    assert_eq!(bag.remove_key(&present_key), None);
}

#[test]
fn constructor_tags_prevent_cross_type_key_collisions() {
    let bundle = Bundle {
        body: Some(Par::default()),
        ..Default::default()
    };
    let send = Send {
        chan: Some(Par::default()),
        ..Default::default()
    };
    let bundle_key = with_component_key(&bundle, <[u8]>::to_vec);
    let send_key = with_component_key(&send, <[u8]>::to_vec);

    assert_eq!(&bundle_key[..PAR_BAG_KEY_DOMAIN.len()], &PAR_BAG_KEY_DOMAIN);
    assert_eq!(&send_key[..PAR_BAG_KEY_DOMAIN.len()], &PAR_BAG_KEY_DOMAIN);
    assert_eq!(
        bundle_key[PAR_BAG_KEY_DOMAIN.len()],
        ParComponentTag::Bundle.byte()
    );
    assert_eq!(
        send_key[PAR_BAG_KEY_DOMAIN.len()],
        ParComponentTag::Send.byte()
    );
    assert_ne!(bundle_key, send_key);
}

#[test]
fn typed_prefix_cursor_decodes_only_the_requested_family() {
    let par = canonical_par_corpus();
    let bag = par.clone().into_components();
    let mut decoded = Vec::new();
    bag.try_visit_decoded::<Expr, ()>(|value, multiplicity| {
        decoded.push((value, multiplicity.get()));
        Ok(())
    })
    .expect("typed traversal");

    assert_eq!(decoded.len(), bag.family_distinct_len::<Expr>());
    assert_eq!(
        decoded.iter().map(|(_, count)| *count).sum::<u64>(),
        par.family_cardinality::<Expr>()
    );
}

#[test]
fn encoded_cursor_exposes_the_canonical_payload_without_decoding() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(99)),
    };
    let mut bag = ParBag::new();
    bag.insert_many(&value, Multiplicity::new(7).expect("positive"))
        .expect("insert");

    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(&value);
    let mut visits = 0usize;
    bag.visit_encoded::<Expr>(|entry| {
        visits += 1;
        assert_eq!(entry.payload(), expected);
        assert_eq!(entry.multiplicity().get(), 7);
    });
    assert_eq!(visits, 1);
}

#[test]
fn clone_is_copy_on_write_and_family_removal_is_prefix_native() {
    let par = canonical_par_corpus();
    let original = par.clone().into_components();
    let mut changed = original.clone();
    let expected_distinct = changed.family_distinct_len::<Expr>();
    let expected_cardinality = changed.family_cardinality::<Expr>();
    let removed = changed.remove_family::<Expr>();

    assert_eq!(removed, (expected_distinct, expected_cardinality));
    assert_eq!(changed.family_cardinality::<Expr>(), 0);
    assert_eq!(
        original.family_cardinality::<Expr>(),
        par.family_cardinality::<Expr>()
    );
}

#[test]
fn family_metrics_track_every_mutation_and_compact_round_trip() {
    let first = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let second = Expr {
        expr_instance: Some(ExprInstance::GInt(2)),
    };
    let send = Send {
        chan: Some(Par::default()),
        ..Send::default()
    };
    let mut bag = ParBag::new();
    bag.insert_many(&first, Multiplicity::new(3).expect("positive"))
        .expect("insert first expression");
    bag.insert_many(&second, Multiplicity::new(2).expect("positive"))
        .expect("insert second expression");
    bag.insert(&send).expect("insert send");
    assert_eq!(bag.family_cardinality::<Expr>(), 5);
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);
    assert_eq!(bag.family_cardinality::<Send>(), 1);

    assert_eq!(
        bag.remove_many(&first, Multiplicity::new(2).expect("positive")),
        Some(Multiplicity::ONE)
    );
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);
    assert!(matches!(
        bag.try_remove_many(&first, Multiplicity::new(2).expect("positive")),
        Err(BagError::InsufficientMultiplicity {
            available: 1,
            requested: 2
        })
    ));
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);

    assert_eq!(
        bag.remove_many(&second, Multiplicity::new(9).expect("positive")),
        None
    );
    assert_eq!(bag.family_cardinality::<Expr>(), 1);
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.remove_family::<Send>(), (1, 1));
    assert_eq!(bag.family_cardinality::<Send>(), 0);
    assert_eq!(bag.family_distinct_len::<Send>(), 0);

    let decoded = ParBag::from_compact_bytes(bag.compact_snapshot()).expect("compact round trip");
    assert_eq!(decoded.family_cardinality::<Expr>(), 1);
    assert_eq!(decoded.family_distinct_len::<Expr>(), 1);
    assert_eq!(decoded.family_cardinality::<Send>(), 0);
}

#[test]
fn endpoint_overflow_is_atomic_while_bag_lattice_remains_closed() {
    let first = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let second = Expr {
        expr_instance: Some(ExprInstance::GInt(2)),
    };
    let maximum = Multiplicity::new(u64::MAX).expect("positive");

    let mut bag = ParBag::new();
    bag.insert_many(&first, maximum).expect("maximum family");
    let snapshot_ptr = bag.compact_snapshot().as_ptr();
    assert!(matches!(
        bag.insert(&first),
        Err(BagError::MultiplicityOverflow {
            current: u64::MAX,
            added: 1
        })
    ));
    assert_eq!(bag.family_cardinality::<Expr>(), u64::MAX);
    assert!(!bag.family_cardinality_is_saturated::<Expr>());
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.compact_snapshot().as_ptr(), snapshot_ptr);

    bag.insert(&second)
        .expect("a distinct endpoint preserves the u64 value domain");
    assert_eq!(bag.family_cardinality::<Expr>(), u64::MAX);
    assert!(bag.family_cardinality_is_saturated::<Expr>());
    assert_eq!(bag.cardinality(), u64::MAX);
    assert!(bag.cardinality_is_saturated());
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);

    let mut raw = Bag::new();
    with_component_key(&first, |key| {
        raw.insert_many(key, maximum).expect("raw maximum endpoint");
    });
    with_component_key(&second, |key| {
        raw.insert(key).expect("raw second endpoint");
    });
    let decoded = ParBag::from_compact_bytes(&raw.compact_bytes()).expect("valid ACT family");
    assert_eq!(decoded.family_cardinality::<Expr>(), u64::MAX);
    assert!(decoded.family_cardinality_is_saturated::<Expr>());
    assert!(decoded.cardinality_is_saturated());
    assert_eq!(decoded.family_distinct_len::<Expr>(), 2);
}

#[test]
fn exact_bag_algebra_preserves_multiplicity_laws() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(42)),
    };
    let mut one = ParBag::new();
    one.insert(&value).expect("one");
    let mut three = ParBag::new();
    three
        .insert_many(&value, Multiplicity::new(3).expect("positive"))
        .expect("three");

    let sum = one.sum(&three).expect("sum");
    let join = one.join(&three);
    let meet = one.meet(&three);
    let difference = three.subtract(&one);
    assert_eq!(sum.count(&value), 4);
    assert_eq!(sum.family_cardinality::<Expr>(), 4);
    assert_eq!(join.count(&value), 3);
    assert_eq!(join.family_cardinality::<Expr>(), 3);
    assert_eq!(meet.count(&value), 1);
    assert_eq!(meet.family_cardinality::<Expr>(), 1);
    assert_eq!(difference.count(&value), 2);
    assert_eq!(difference.family_cardinality::<Expr>(), 2);
    assert!(one.is_subbag_of(&three));
}

#[test]
fn scalar_multiplication_reuses_each_canonical_component_key() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(42)),
    };
    let mut source = ParBag::new();
    source.insert(&value).expect("source");
    let scaled = source.scale(17).expect("scaled");

    assert_eq!(source.count(&value), 1);
    assert_eq!(scaled.count(&value), 17);
    assert_eq!(scaled.family_distinct_len::<Expr>(), 1);
    assert_eq!(scaled.family_cardinality::<Expr>(), 17);
    assert!(source.scale(0).expect("zero").is_empty());
}

#[test]
fn direct_act_image_round_trips_without_an_entry_projection() {
    let bag = canonical_par_corpus().into_components();
    let bytes = bag.compact_bytes();
    assert_eq!(ParBag::from_compact_bytes(&bytes).expect("decode"), bag);

    let bincode_bytes = bincode::serialize(&bag).expect("serialize");
    let decoded: ParBag = bincode::deserialize(&bincode_bytes).expect("deserialize");
    assert_eq!(decoded, bag);
}

#[derive(Debug, PartialEq, Eq)]
struct AdmittedLegacyObservations {
    protobuf: Vec<u8>,
    bincode: Vec<u8>,
    event_preimages: [Vec<u8>; 3],
    events: [Vec<u8>; 2],
    records: [Vec<u8>; 3],
    history_root: Vec<u8>,
}

fn admitted_legacy_observations(par: &Par) -> AdmittedLegacyObservations {
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{BindPattern, ListParWithRandom, ParWithRandom, TaggedContinuation};
    use models::rust::event_hash_bytes::{
        event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
        event_hash_bytes_tagged_continuation,
    };
    use models::rust::rholang::{bincode_encoder, protobuf_encoder};
    use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
    use rspace_plus_plus::rspace::hot_store_action::{
        HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
    };
    use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
    use rspace_plus_plus::rspace::serializers::serializers::{
        encode_continuations, encode_datums, encode_joins,
    };
    use rspace_plus_plus::rspace::state::rspace_state_authority::RSpaceStateAuthority;

    #[derive(serde::Serialize)]
    struct LegacyDatumHashImage {
        pars: Vec<Par>,
        random_state: Vec<u8>,
    }

    #[derive(serde::Serialize)]
    struct LegacyContinuationHashImage {
        guard: Option<Par>,
        tagged_cont: Option<TaggedCont>,
    }

    let datum_value = ListParWithRandom {
        pars: vec![par.clone(), par.clone()],
        random_state: vec![1, 7, 9],
        cost_region: None,
    };
    let pattern = BindPattern {
        patterns: vec![par.clone()],
        remainder: Some(Var::default()),
        free_count: -3,
    };
    let body = TaggedContinuation {
        guard: Some(par.clone()),
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(par.clone()),
            random_state: vec![2, 8],
        })),
        cost_region: None,
        clause_regions: Vec::new(),
    };
    let event_preimages = [
        event_hash_bytes_list_par_with_random(&datum_value),
        event_hash_bytes_bind_pattern(&pattern),
        event_hash_bytes_tagged_continuation(&body),
    ];
    assert_eq!(
        event_preimages[0],
        bincode::serialize(&LegacyDatumHashImage {
            pars: datum_value.pars.clone(),
            random_state: datum_value.random_state.clone(),
        })
        .unwrap()
    );
    assert_eq!(event_preimages[1], bincode::serialize(&pattern).unwrap());
    assert_eq!(
        event_preimages[2],
        bincode::serialize(&LegacyContinuationHashImage {
            guard: body.guard.clone(),
            tagged_cont: body.tagged_cont.clone(),
        })
        .unwrap()
    );
    let datum = Datum::create(par, datum_value, false);
    let channels = vec![par.clone()];
    let continuation = WaitingContinuation::create(
        &channels,
        &vec![pattern],
        &body,
        true,
        std::collections::BTreeSet::from([0]),
    );
    let events = [
        bincode::serialize(&datum.source).unwrap(),
        bincode::serialize(&continuation.source).unwrap(),
    ];
    let datums = vec![datum.clone(), datum];
    let continuations = vec![continuation];
    let joins = vec![channels.clone()];
    let records = [
        encode_datums(&datums),
        encode_continuations(&continuations),
        encode_joins(&joins),
    ];
    let authority = RSpaceStateAuthority::in_memory();
    let repository = HistoryRepositoryInstances::<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >::repository(authority.clone())
    .unwrap();
    let checkpoint = repository
        .checkpoint(vec![
            HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                channel: par.clone(),
                data: datums,
            })),
            HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
                channels,
                continuations,
            })),
            HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                channel: par.clone(),
                joins,
            })),
        ])
        .unwrap();
    let root = checkpoint.root();
    assert_eq!(
        authority.current_verified_root().unwrap().unwrap().root(),
        &root
    );
    let reader = checkpoint.get_history_reader(&root).unwrap();
    assert_eq!(encode_datums(&reader.base().get_data(par)), records[0]);
    assert_eq!(
        encode_continuations(&reader.base().get_continuations(&vec![par.clone()])),
        records[1]
    );
    assert_eq!(encode_joins(&reader.base().get_joins(par)), records[2]);
    let bincode = bincode_encoder::encode(par);
    assert_eq!(bincode, bincode::serialize(par).unwrap());
    AdmittedLegacyObservations {
        protobuf: protobuf_encoder::encode_to_vec(par),
        bincode,
        event_preimages,
        events,
        records,
        history_root: root.bytes(),
    }
}

fn assert_admitted_legacy_observations(label: &str, original: &Par) {
    let expected = admitted_legacy_observations(original);
    let admitted = Par::from_parts(
        ParBag::from_compact_bytes(original.compact_snapshot())
            .unwrap_or_else(|error| panic!("{label}: compact admission failed: {error}")),
        original.locally_free.clone(),
        original.connective_used,
    );
    assert_eq!(admitted_legacy_observations(&admitted), expected, "{label}");
}

#[test]
fn admitted_compact_rpb3_preserves_codec_event_and_history_observations() {
    let all_families = canonical_par_corpus();
    assert_eq!(
        [
            all_families.family_cardinality::<Send>(),
            all_families.family_cardinality::<Receive>(),
            all_families.family_cardinality::<New>(),
            all_families.family_cardinality::<Expr>(),
            all_families.family_cardinality::<Match>(),
            all_families.family_cardinality::<GUnforgeable>(),
            all_families.family_cardinality::<Connective>(),
            all_families.family_cardinality::<Bundle>(),
            all_families.family_cardinality::<If>(),
        ],
        [1; 9]
    );
    assert_admitted_legacy_observations("all-nine-families", &all_families);
    let corpus = corpus::par_corpus();
    assert!(!corpus.is_empty());
    for (label, original) in corpus {
        assert_admitted_legacy_observations(&label, &original);
    }
    let repeated = corpus::gint(0).components().scale(3).unwrap();
    let original = Par::from_parts(repeated, vec![0x15], true);
    assert_admitted_legacy_observations("multiplicity-and-root-metadata", &original);
}

#[test]
fn admitted_observation_witness_detects_lost_multiplicity_and_root_metadata() {
    let singleton = corpus::gint(0);
    let original = Par::from_parts(singleton.components().scale(3).unwrap(), vec![0x15], true);
    let expected = admitted_legacy_observations(&original);
    let lost_occurrences = Par::from_parts(singleton.into_components(), vec![0x15], true);
    let mutant = admitted_legacy_observations(&lost_occurrences);
    assert_ne!(mutant.protobuf, expected.protobuf);
    assert_ne!(mutant.bincode, expected.bincode);
    assert_ne!(mutant.event_preimages, expected.event_preimages);
    assert_ne!(mutant.events, expected.events);
    assert_ne!(mutant.records, expected.records);
    assert_ne!(mutant.history_root, expected.history_root);
    let lost_metadata = Par::from_parts(original.components().clone(), vec![], true);
    assert_eq!(lost_metadata, original);
    assert_ne!(
        admitted_legacy_observations(&lost_metadata).protobuf,
        expected.protobuf
    );
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn admitted_compact_observations_preserve_generated_occurrences(
        values in proptest::collection::vec(proptest::prelude::any::<i16>(), 0..9),
        locally_free in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4),
        connective_used in proptest::prelude::any::<bool>(),
    ) {
        let original = Par::default().with_exprs(values.into_iter().flat_map(|value| {
            let expr = Expr { expr_instance: Some(ExprInstance::GInt(i64::from(value))) };
            [expr.clone(), expr]
        }).collect()).with_locally_free(locally_free).with_connective_used(connective_used);
        assert_admitted_legacy_observations("generated-duplicate-occurrences", &original);
    }
}

#[test]
fn protobuf_streams_legacy_canonical_order_without_reconstructing_production_vecs() {
    let mut source = canonical_par_corpus();
    let mut exprs = source.decode_occurrences::<Expr>();
    exprs.push(exprs[0].clone());
    exprs.reverse();
    source.replace_exprs(exprs);

    // The Par encoder and component stream traverse the same authoritative
    // trie; the latter omits only root metadata, which this fixture leaves at
    // its defaults.
    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(&source);
    let bag = source.into_components();
    let mut actual = Vec::new();
    bag.encode_protobuf_components(&mut actual)
        .expect("direct stream");
    assert_eq!(actual, expected);
    assert_eq!(
        bag.protobuf_components_bytes().expect("collected stream"),
        actual
    );
    assert_eq!(
        bag.protobuf_components_encoded_len().expect("length"),
        actual.len()
    );
}

#[test]
fn deep_component_import_and_act_validation_are_stack_safe() {
    let par = corpus::deep_par(4_096);
    let bag = par.into_components();
    assert_eq!(bag.cardinality(), 1);
    let bytes = bag.compact_bytes();
    assert_eq!(
        ParBag::from_compact_bytes(&bytes).expect("deep decode"),
        bag
    );
    let protobuf = bag.protobuf_components_bytes().expect("deep protobuf");
    let decoded = Par::decode(protobuf.as_slice()).expect("stack-safe Par protobuf decoder");
    assert_eq!(decoded.family_cardinality::<Expr>(), 1);
}

#[test]
fn protobuf_expansion_rejects_impossible_output_length_before_allocating() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let mut bag = ParBag::new();
    bag.insert_many(
        &value,
        Multiplicity::new(u64::MAX).expect("positive multiplicity"),
    )
    .expect("logical multiplicity fits Bag");
    assert!(matches!(
        bag.protobuf_components_encoded_len(),
        Err(ParBagProtobufError::EncodedLengthOverflow {
            tag: ParComponentTag::Expr,
            multiplicity: u64::MAX,
            ..
        })
    ));

    let mut component_output = vec![0xa5, 0x5a];
    assert!(matches!(
        bag.encode_protobuf_components(&mut component_output),
        Err(ParBagProtobufError::EncodedLengthOverflow { .. })
    ));
    assert_eq!(
        component_output,
        [0xa5, 0x5a],
        "an impossible multiplicity must fail before publishing a prefix"
    );

    let par = Par::from_parts(bag, Vec::new(), false);
    let mut par_output = vec![0xc3];
    assert!(par.encode_protobuf(&mut par_output).is_err());
    assert_eq!(
        par_output,
        [0xc3],
        "Par must preflight the complete expansion before root fields are emitted"
    );
    let mut published_chunks = 0usize;
    assert!(par
        .try_visit_protobuf_chunks(|_| {
            published_chunks += 1;
            Ok::<_, std::convert::Infallible>(())
        })
        .is_err());
    assert_eq!(
        published_chunks, 0,
        "invalid multiplicity must fail preflight before a fallible sink observes any chunk"
    );
}

#[test]
fn act_import_rejects_every_key_outside_the_typed_language() {
    let mut bad_domain = b"BAD\x03\x05".to_vec();
    bad_domain.extend_from_slice(&0u64.to_be_bytes());
    let mut bad_tag = b"RPB\x03\xff".to_vec();
    bad_tag.extend_from_slice(&0u64.to_be_bytes());
    let mut bad_score = b"RPB\x03\x05".to_vec();
    bad_score.push(0xff);
    bad_score.extend_from_slice(&1u64.to_be_bytes());
    let invalid_paths = [
        vec![],
        bad_domain.clone(),
        bad_tag.clone(),
        bad_score.clone(),
    ];

    let mut observed = BTreeMap::new();
    for path in invalid_paths {
        let mut raw = Bag::new();
        raw.insert(&path)
            .expect("raw Bag accepts arbitrary byte paths");
        let error = ParBag::from_compact_bytes(&raw.compact_bytes()).expect_err("invalid key");
        observed.insert(path, error);
    }

    assert!(matches!(
        observed.get(&[][..]),
        Some(ParBagCodecError::KeyTooShort { .. })
    ));
    assert!(matches!(
        observed.get(bad_domain.as_slice()),
        Some(ParBagCodecError::InvalidDomain { .. })
    ));
    assert!(matches!(
        observed.get(bad_tag.as_slice()),
        Some(ParBagCodecError::InvalidTag(0xff))
    ));
    assert!(matches!(
        observed.get(bad_score.as_slice()),
        Some(ParBagCodecError::InvalidOrderKey { .. })
    ));
}

#[test]
fn exact_key_validation_rejects_noncanonical_unknown_protobuf_fields() {
    let value = GUnforgeable::default();
    let mut key = with_component_key(&value, <[u8]>::to_vec);
    // Unknown varint field 100. The protobuf machine accepts and ignores it,
    // but regenerating the canonical RPB3 key removes it.
    let trailer = key.split_off(key.len() - std::mem::size_of::<u64>());
    key.extend_from_slice(&[0xa0, 0x06, 0x01]);
    key.extend_from_slice(&trailer);
    let mut raw = Bag::new();
    raw.insert(&key).expect("raw Bag");

    assert!(matches!(
        ParBag::from_compact_bytes(&raw.compact_bytes()),
        Err(ParBagCodecError::NonCanonicalComponent {
            tag: ParComponentTag::GUnforgeable
        })
    ));
}

#[test]
fn protobuf_component_bodies_preserve_nested_locally_free_metadata() {
    let mut value = Send::default();
    let mut channel = Par::default();
    channel.replace_locally_free(vec![0x80, 0x01]);
    value.chan = Some(channel);
    let mut bag = ParBag::new();
    bag.insert(&value).expect("insert");

    bag.try_visit_decoded::<Send, ()>(|decoded, multiplicity| {
        assert_eq!(multiplicity.get(), 1);
        assert_eq!(decoded.chan.expect("channel").locally_free(), [0x80, 0x01]);
        Ok(())
    })
    .expect("decode stored protobuf body");
}
