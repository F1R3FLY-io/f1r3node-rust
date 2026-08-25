//! Representation and compatibility gates for the authoritative PathMap-backed `Par`.

use std::io::Write as _;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, Expr, GSysAuthToken, GUnforgeable, If, Match, MatchCase, New, Par, Receive,
    ReceiveBind, Send,
};
use prost::bytes::Buf;
use prost::Message;

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn valid_all_families() -> Par {
    let mut par = Par::default();
    par.replace_sends(vec![Send {
        chan: Some(integer(1)),
        data: vec![integer(2)],
        ..Send::default()
    }]);
    par.replace_receives(vec![Receive {
        binds: vec![ReceiveBind {
            source: Some(integer(3)),
            patterns: vec![integer(4)],
            ..ReceiveBind::default()
        }],
        body: Some(integer(5)),
        condition: Some(integer(6)),
        ..Receive::default()
    }]);
    par.replace_news(vec![New {
        p: Some(integer(7)),
        ..New::default()
    }]);
    par.replace_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(8)),
    }]);
    par.replace_matches(vec![Match {
        target: Some(integer(9)),
        cases: vec![MatchCase {
            pattern: Some(integer(10)),
            source: Some(integer(11)),
            guard: Some(integer(12)),
            ..MatchCase::default()
        }],
        ..Match::default()
    }]);
    par.replace_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
    }]);
    par.replace_bundles(vec![Bundle {
        body: Some(integer(13)),
        ..Bundle::default()
    }]);
    par.replace_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::ConnBool(true)),
    }]);
    par.replace_conditionals(vec![If {
        condition: Some(integer(14)),
        if_true: Some(integer(15)),
        if_false: Some(integer(16)),
        ..If::default()
    }]);
    par.replace_locally_free(vec![0x05]);
    par.replace_connective_used(true);
    par
}

/// Frozen test-only projection of the legacy generated `Par` repeated-field
/// schema. Production must never reconstruct these vectors; the oracle exists
/// precisely to keep the trie-native encoder independent of its authority.
struct LegacyProtobufParts {
    before_root: Vec<u8>,
    root: Vec<u8>,
    after_root: Vec<u8>,
}

impl LegacyProtobufParts {
    fn into_bytes(self) -> Vec<u8> {
        let mut output =
            Vec::with_capacity(self.before_root.len() + self.root.len() + self.after_root.len());
        output.extend_from_slice(&self.before_root);
        output.extend_from_slice(&self.root);
        output.extend_from_slice(&self.after_root);
        output
    }
}

fn legacy_repeated_field_oracle_parts(value: &Par) -> LegacyProtobufParts {
    let mut before_root = Vec::new();
    for occurrence in value.decode_occurrences::<Send>() {
        prost::encoding::message::encode(1, &occurrence, &mut before_root);
    }
    for occurrence in value.decode_occurrences::<Receive>() {
        prost::encoding::message::encode(2, &occurrence, &mut before_root);
    }
    for occurrence in value.decode_occurrences::<New>() {
        prost::encoding::message::encode(4, &occurrence, &mut before_root);
    }
    for occurrence in value.decode_occurrences::<Expr>() {
        prost::encoding::message::encode(5, &occurrence, &mut before_root);
    }
    for occurrence in value.decode_occurrences::<Match>() {
        prost::encoding::message::encode(6, &occurrence, &mut before_root);
    }
    for occurrence in value.decode_occurrences::<GUnforgeable>() {
        prost::encoding::message::encode(7, &occurrence, &mut before_root);
    }
    for occurrence in value.decode_occurrences::<Connective>() {
        prost::encoding::message::encode(8, &occurrence, &mut before_root);
    }

    let mut root = Vec::new();
    if !value.locally_free().is_empty() {
        prost::encoding::bytes::encode(9, &value.locally_free, &mut root);
    }
    if value.connective_used() {
        prost::encoding::bool::encode(10, &value.connective_used(), &mut root);
    }

    let mut after_root = Vec::new();
    for occurrence in value.decode_occurrences::<Bundle>() {
        prost::encoding::message::encode(11, &occurrence, &mut after_root);
    }
    for occurrence in value.decode_occurrences::<If>() {
        prost::encoding::message::encode(12, &occurrence, &mut after_root);
    }
    LegacyProtobufParts {
        before_root,
        root,
        after_root,
    }
}

fn legacy_repeated_field_oracle(value: &Par) -> Vec<u8> {
    legacy_repeated_field_oracle_parts(value).into_bytes()
}

#[test]
fn protobuf_stream_is_exact_and_round_trips_without_a_vec_shadow() {
    let value = Par::default()
        .with_exprs(vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
        ])
        .with_locally_free(vec![0x05])
        .with_connective_used(true);

    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(&value);
    let mut streamed = Vec::new();
    value.encode_protobuf(&mut streamed).expect("stream Par");
    assert_eq!(streamed, expected);
    let mut direct = Vec::new();
    value
        .encode_protobuf_direct(&mut direct)
        .expect("direct one-shot Par");
    assert_eq!(direct, expected);
    assert_eq!(
        value.protobuf_encoded_len().expect("length"),
        expected.len()
    );
    assert_eq!(value.protobuf_bytes().expect("collect"), expected);
    assert_eq!(Par::decode(expected.as_slice()).expect("decode"), value);
    assert_eq!(value.distinct_len(), 2);
    assert_eq!(value.cardinality(), 3);
}

#[test]
fn direct_default_prepared_prost_and_generated_par_encoders_match_the_legacy_schema() {
    let base = valid_all_families();
    let value = Par::from_parts(
        base.components()
            .scale(2)
            .expect("double all multiplicities"),
        base.locally_free().to_vec(),
        base.connective_used(),
    );
    let expected = legacy_repeated_field_oracle(&value);

    let mut direct = Vec::with_capacity(expected.len());
    value
        .encode_protobuf_direct(&mut direct)
        .expect("direct trie-native encode");
    assert_eq!(direct, expected);

    let mut cold_default = Vec::with_capacity(expected.len());
    value
        .encode_protobuf(&mut cold_default)
        .expect("cold default encode");
    assert_eq!(cold_default, expected);
    assert_eq!(value.encode_to_vec(), expected, "prost Message entry point");
    assert_eq!(
        models::rust::rholang::protobuf_encoder::encode_to_vec(&value),
        expected,
        "generated protobuf PDA entry point"
    );
    let mut cold_chunks = Vec::with_capacity(expected.len());
    value
        .try_visit_protobuf_chunks(|chunk| {
            cold_chunks.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("cold borrowed-chunk stream");
    assert_eq!(cold_chunks, expected);

    let mut direct_writer = std::io::Cursor::new(Vec::with_capacity(expected.len()));
    value
        .try_visit_protobuf_chunks_direct(|chunk| direct_writer.write_all(chunk))
        .expect("direct fallible writer stream");
    assert_eq!(direct_writer.into_inner(), expected);

    let component_bytes = value
        .components()
        .protobuf_components_encoded_len()
        .expect("component projection length");
    value
        .prepare_protobuf_projection(component_bytes)
        .expect("bounded explicit preparation");
    let mut prepared = Vec::with_capacity(expected.len());
    value
        .encode_protobuf(&mut prepared)
        .expect("prepared projection encode");
    assert_eq!(prepared, expected);
    let mut prepared_chunks = Vec::with_capacity(expected.len());
    value
        .try_visit_protobuf_chunks(|chunk| {
            prepared_chunks.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("prepared borrowed-chunk stream");
    assert_eq!(prepared_chunks, expected);
    assert_eq!(
        value.encode_to_vec(),
        expected,
        "warm prost Message entry point"
    );
    assert_eq!(
        models::rust::rholang::protobuf_encoder::encode_to_vec(&value),
        expected,
        "warm generated protobuf PDA entry point"
    );

    for direct in [true, false] {
        let mut callback_count = 0usize;
        let mut staged = Vec::new();
        let mut public = vec![0xee];
        let mut sink = |chunk: &[u8]| {
            let callback = callback_count;
            callback_count += 1;
            if callback == 2 {
                staged.extend_from_slice(&chunk[..chunk.len().div_ceil(2)]);
                return Err(17u8);
            }
            staged.extend_from_slice(chunk);
            Ok(())
        };
        let result = if direct {
            value.try_visit_protobuf_chunks_direct(&mut sink)
        } else {
            value.try_visit_protobuf_chunks(&mut sink)
        };
        if result.is_ok() {
            public = staged.clone();
        }
        assert!(matches!(
            result,
            Err(models::rust::par::ParProtobufSinkError::Sink(17))
        ));
        assert_eq!(
            callback_count, 3,
            "emission must stop at the failed callback"
        );
        assert!(!staged.is_empty());
        assert!(expected.starts_with(&staged));
        assert!(staged.len() < expected.len());
        assert_eq!(
            public,
            [0xee],
            "a failed private sink must not replace its public value"
        );
    }
    assert_eq!(
        value.protobuf_encoded_len().expect("encoded length"),
        expected.len()
    );
    assert_eq!(Par::decode(expected.as_slice()).expect("decode"), value);
}

#[test]
fn protobuf_oracle_rejects_shifted_split_and_omitted_after_root_family() {
    let value = valid_all_families();
    let parts = legacy_repeated_field_oracle_parts(&value);
    assert!(!parts.before_root.is_empty());
    assert!(!parts.root.is_empty());
    assert!(!parts.after_root.is_empty());

    let expected = legacy_repeated_field_oracle(&value);
    let mut projection = parts.before_root.clone();
    projection.extend_from_slice(&parts.after_root);
    let correct_split = parts.before_root.len();
    let mut correct = projection[..correct_split].to_vec();
    correct.extend_from_slice(&parts.root);
    correct.extend_from_slice(&projection[correct_split..]);
    assert_eq!(correct, expected);

    let shifted_split = correct_split - 1;
    let mut shifted = projection[..shifted_split].to_vec();
    shifted.extend_from_slice(&parts.root);
    shifted.extend_from_slice(&projection[shifted_split..]);
    assert_ne!(
        shifted, expected,
        "the byte-equivalence gate must reject a one-byte split mutant"
    );

    let mut omitted_if = value.clone();
    omitted_if.replace_conditionals(Vec::new());
    assert_ne!(
        legacy_repeated_field_oracle(&omitted_if),
        expected,
        "the independent oracle must reject omission of an after-root family"
    );
}

#[test]
fn prepared_empty_component_projection_emits_only_nonempty_chunks() {
    let empty = Par::default();
    empty
        .prepare_protobuf_projection(0)
        .expect("empty projection fits an exact zero-byte cap");
    let mut empty_callbacks = 0usize;
    empty
        .try_visit_protobuf_chunks(|chunk| {
            empty_callbacks += 1;
            assert!(!chunk.is_empty());
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("empty protobuf stream");
    assert_eq!(empty_callbacks, 0);

    let metadata = Par::default()
        .with_locally_free(vec![0x05, 0x80])
        .with_connective_used(true);
    let expected = legacy_repeated_field_oracle(&metadata);
    metadata
        .prepare_protobuf_projection(0)
        .expect("metadata does not consume the component retention budget");
    let mut prepared = Vec::new();
    metadata
        .try_visit_protobuf_chunks(|chunk| {
            assert!(!chunk.is_empty());
            prepared.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("prepared metadata stream");
    let mut direct = Vec::new();
    metadata
        .try_visit_protobuf_chunks_direct(|chunk| {
            assert!(!chunk.is_empty());
            direct.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("direct metadata stream");
    assert_eq!(prepared, expected);
    assert_eq!(direct, expected);
}

#[test]
fn protobuf_merge_accumulates_multiplicity_without_repeated_storage() {
    let occurrence = integer(11);
    let mut bytes = occurrence.encode_to_vec();
    bytes.extend_from_within(..);
    let decoded = Par::decode(bytes.as_slice()).expect("decode repeated occurrence");
    assert_eq!(decoded.distinct_len(), 1);
    assert_eq!(decoded.cardinality(), 2);
}

#[test]
fn first_and_visitors_preserve_canonical_order_and_multiplicity() {
    let value = Par::default().with_exprs(vec![
        Expr {
            expr_instance: Some(ExprInstance::GInt(9)),
        },
        Expr {
            expr_instance: Some(ExprInstance::GInt(3)),
        },
        Expr {
            expr_instance: Some(ExprInstance::GInt(3)),
        },
    ]);
    let (first, multiplicity) = value.first::<Expr>().unwrap().unwrap();
    assert_eq!(first.expr_instance, Some(ExprInstance::GInt(3)));
    assert_eq!(multiplicity.get(), 2);
    assert_eq!(value.decode_occurrences::<Expr>().len(), 3);
}

#[test]
fn chunked_protobuf_decode_dispatches_every_component_family() {
    let source = valid_all_families();
    let bytes = source.encode_to_vec();
    let split = bytes.len() / 2;
    let chunks = (&bytes[..split]).chain(&bytes[split..]);
    assert_eq!(Par::decode(chunks).expect("chunked decode"), source);
}

#[test]
fn protobuf_decode_rejects_truncated_component_bodies() {
    assert!(Par::decode([0x0a, 0x05, 0x00].as_slice()).is_err());
}

#[test]
fn bincode_is_one_compact_snapshot_plus_root_metadata() {
    let value = integer(9)
        .parallel(integer(9))
        .expect("finite multiplicity")
        .with_locally_free(vec![0x05])
        .with_connective_used(true);
    let snapshot = value.compact_snapshot();
    let bytes = bincode::serialize(&value).expect("serialize");
    assert_eq!(
        bytes,
        models::rust::rholang::bincode_encoder::encode(&value)
    );
    assert_eq!(
        u64::from_le_bytes(bytes[..8].try_into().expect("snapshot length")),
        snapshot.len() as u64
    );
    assert_eq!(&bytes[8..8 + snapshot.len()], snapshot);
    assert_eq!(bincode::deserialize::<Par>(&bytes).expect("decode"), value);
}

#[test]
fn clone_is_cow_and_mutation_detaches() {
    let value = integer(9);
    let mut shared = value.clone();
    assert_eq!(
        shared.compact_snapshot().as_ptr(),
        value.compact_snapshot().as_ptr()
    );
    let added = Expr {
        expr_instance: Some(ExprInstance::GInt(41)),
    };
    shared
        .insert_component(&added, Vec::new(), false)
        .expect("mutate clone");
    assert!(!value.contains(&added));
    assert!(shared.contains(&added));
    assert_ne!(shared.compact_snapshot(), value.compact_snapshot());
}

#[test]
fn equality_hashing_and_ordering_ignore_only_root_locally_free() {
    let plain = integer(7);
    let tagged = plain.with_locally_free(vec![0xff]);
    assert_eq!(plain, tagged);
    assert_eq!(plain.cmp(&tagged), std::cmp::Ordering::Equal);

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let digest = |value: &Par| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(digest(&plain), digest(&tagged));
}

#[test]
fn bag_sum_scaling_and_metadata_join_are_checked() {
    let left = integer(1).with_locally_free(vec![0b0000_0001]);
    let right = integer(1)
        .with_locally_free(vec![0b0000_0010])
        .with_connective_used(true);
    let combined = left.parallel(right).expect("bag sum");
    assert_eq!(combined.distinct_len(), 1);
    assert_eq!(combined.cardinality(), 2);
    assert_eq!(combined.locally_free(), [0b0000_0011]);
    assert!(combined.connective_used());

    let scaled = combined.clone().scale_occurrences(7).expect("scale");
    assert_eq!(scaled.cardinality(), 14);
    let zero = combined.scale_occurrences(0).expect("zero scale");
    assert!(zero.components_is_empty());
    assert!(zero.locally_free().is_empty());
    assert!(!zero.connective_used());
}

#[test]
fn replacement_and_prepend_cover_every_family_without_generated_imports() {
    let source = valid_all_families();
    let mut replaced = source.clone();
    replaced.replace_sends(Vec::new());
    replaced.replace_receives(Vec::new());
    replaced.replace_news(Vec::new());
    replaced.replace_exprs(Vec::new());
    replaced.replace_matches(Vec::new());
    replaced.replace_unforgeables(Vec::new());
    replaced.replace_bundles(Vec::new());
    replaced.replace_connectives(Vec::new());
    replaced.replace_conditionals(Vec::new());
    assert!(replaced.components_is_empty());

    let send = source.first::<Send>().unwrap().unwrap().0;
    assert_eq!(source.clone().prepend_send(send.clone()).count(&send), 2);
    let receive = source.first::<Receive>().unwrap().unwrap().0;
    assert_eq!(
        source
            .clone()
            .prepend_receive(receive.clone())
            .count(&receive),
        2
    );
    let matched = source.first::<Match>().unwrap().unwrap().0;
    assert_eq!(
        source
            .clone()
            .prepend_match(matched.clone())
            .count(&matched),
        2
    );
    let conditional = source.first::<If>().unwrap().unwrap().0;
    assert_eq!(
        source.prepend_if(conditional.clone()).count(&conditional),
        2
    );
}
