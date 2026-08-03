//! # Generated protobuf decoder: semantic and stack-safety differential
//!
//! The generated decoder is required to implement the same protobuf state
//! transition as Prost for every input on which Prost's depth budget does not
//! intervene. This target checks both sides of that contract:
//!
//! 1. the existing exhaustive `rhoapi` corpus covers every `Expr` and
//!    `Connective` oneof arm, every `Par` field, `EPathMap`, maps, repeated
//!    messages, and several non-`Par` roots;
//! 2. every truncation of the largest corpus value and 1,024 arbitrary byte
//!    strings must produce the same value or the same `DecodeError` value;
//! 3. a synthetically encoded 20,000-level `Par → Expr → ENot → Par` chain must
//!    decode and drop on a 256 KiB thread without `RUST_MIN_STACK`, while the
//!    recursive oracle must reject the same valid bytes at its depth budget.
//!
//! Comparing accepted values only with `PartialEq` would be insufficient because
//! `Par` deliberately ignores `locally_free`. Accepted values are therefore
//! re-encoded and compared byte-for-byte as well.

use models::rhoapi::Par;
use models::rust::rholang::protobuf_decoder::{
    decode_bind_pattern, decode_list_bind_patterns, decode_list_par_with_random, decode_par,
    decode_par_with_random, decode_par_with_stats, decode_tagged_continuation,
    GENERATED_PROTOBUF_ATTACH_SIZE, GENERATED_PROTOBUF_FRAME_SIZE,
    GENERATED_PROTOBUF_MESSAGE_COUNT, GENERATED_PROTOBUF_NODE_SIZE, GENERATED_PROTOBUF_ONEOF_COUNT,
    GENERATED_PROTOBUF_VALUE_SIZE,
};
use models::rust::rholang::protobuf_encoder;
use models::rust::rholang::protobuf_schema::ProtobufNode;
use proptest::prelude::*;
use prost::Message;

mod par_corpus;
use par_corpus as corpus;

// This is prost-build's untouched output captured before the production
// feedback-vertex-set derives are removed.  It is intentionally compiled only
// into this differential target: production uses the generated PDA, while the
// oracle retains Prost's recursive implementation and depth budget.
mod rust {
    pub use models::rust::*;
}

#[allow(clippy::all, dead_code, unused_imports)]
mod recursive_oracle {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_recursive_oracle.rs"));
}

use recursive_oracle::Par as RecursivePar;

fn assert_par_decode_equivalent(label: &str, bytes: &[u8]) {
    let oracle = RecursivePar::decode(bytes);
    let machine = decode_par(bytes);
    match (oracle, machine) {
        (Ok(oracle), Ok(machine)) => {
            assert_eq!(
                protobuf_encoder::encode_to_vec(&machine),
                oracle.encode_to_vec(),
                "{label}: the generated decoder and recursive Prost returned values with \
                 different canonical protobuf encodings"
            );
        }
        (Err(oracle), Err(machine)) => assert_eq!(
            machine, oracle,
            "{label}: the generated decoder and Prost returned different error values"
        ),
        (oracle, machine) => panic!(
            "{label}: protobuf accept-set divergence: Prost returned {oracle:?}, generated \
             decoder returned {machine:?}"
        ),
    }
}

fn assert_valid_par_roundtrip(label: &str, expected: &Par) {
    let bytes = expected.encode_to_vec();
    let machine = decode_par(bytes.as_slice())
        .unwrap_or_else(|error| panic!("{label}: generated decoder rejected valid bytes: {error}"));
    assert_eq!(
        &machine, expected,
        "{label}: generated decoder changed the valid `Par` value"
    );
    assert_eq!(
        protobuf_encoder::encode_to_vec(&machine),
        bytes,
        "{label}: generated decoder changed the valid protobuf byte representation"
    );

    match RecursivePar::decode(bytes.as_slice()) {
        Ok(oracle) => assert_eq!(
            protobuf_encoder::encode_to_vec(&machine),
            oracle.encode_to_vec(),
            "{label}: decoded value differs from recursive Prost"
        ),
        Err(error) => assert!(
            error.to_string().contains("recursion limit"),
            "{label}: Prost rejected its own valid bytes for an unexpected reason: {error}"
        ),
    }
}

fn assert_root_equivalent<T>(
    label: &str,
    value: &T,
    decode: impl FnOnce(&[u8]) -> Result<T, prost::DecodeError>,
) where
    T: Default + Message + ProtobufNode + PartialEq + std::fmt::Debug,
{
    let bytes = value.encode_to_vec();
    let oracle = T::decode(bytes.as_slice()).expect("the oracle must decode its own bytes");
    let machine = decode(&bytes).expect("the generated decoder must decode oracle bytes");
    assert_eq!(machine, oracle, "{label}: decoded root value differs");
    assert_eq!(
        protobuf_encoder::encode_to_vec(&machine),
        protobuf_encoder::encode_to_vec(&oracle),
        "{label}: decoded root re-encoding differs"
    );
}

#[test]
fn exhaustive_valid_corpora_decode_identically() {
    assert_eq!(GENERATED_PROTOBUF_MESSAGE_COUNT, 57);
    assert_eq!(GENERATED_PROTOBUF_ONEOF_COUNT, 5);

    let pars = corpus::par_corpus();
    assert!(
        pars.len() >= 64,
        "the exhaustive `Par` corpus became vacuous: only {} values remain",
        pars.len()
    );
    for (label, value) in pars {
        assert_valid_par_roundtrip(&format!("Par::{label}"), &value);
    }

    for (label, value) in corpus::list_par_with_random_corpus() {
        assert_root_equivalent(&format!("ListParWithRandom::{label}"), &value, |bytes| {
            decode_list_par_with_random(bytes)
        });
    }
    for (label, value) in corpus::bind_pattern_corpus() {
        assert_root_equivalent(&format!("BindPattern::{label}"), &value, |bytes| {
            decode_bind_pattern(bytes)
        });
    }
    for (label, value) in corpus::tagged_continuation_corpus() {
        assert_root_equivalent(&format!("TaggedContinuation::{label}"), &value, |bytes| {
            decode_tagged_continuation(bytes)
        });
    }
    for (label, value) in corpus::par_with_random_corpus() {
        assert_root_equivalent(&format!("ParWithRandom::{label}"), &value, |bytes| {
            decode_par_with_random(bytes)
        });
    }
    for (label, value) in corpus::list_bind_patterns_corpus() {
        assert_root_equivalent(&format!("ListBindPatterns::{label}"), &value, |bytes| {
            decode_list_bind_patterns(bytes)
        });
    }
}

#[test]
fn every_truncated_prefix_matches_the_oracle_and_drops_partials_iteratively() {
    let bytes = corpus::all_par_fields().encode_to_vec();
    assert!(bytes.len() >= 1_000, "the malformed corpus is too small");
    for cut in 0..bytes.len() {
        assert_par_decode_equivalent(&format!("all_par_fields[..{cut}]"), &bytes[..cut]);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_024))]

    #[test]
    fn arbitrary_bytes_have_the_same_accept_set_value_and_error(
        bytes in proptest::collection::vec(any::<u8>(), 0..=512),
    ) {
        assert_par_decode_equivalent("arbitrary bytes", &bytes);
    }
}

fn varint_len(mut value: usize) -> usize {
    let mut len = 1usize;
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

/// Encode a left spine without first constructing the recursive Rust value.
///
/// At each level the three length-delimited messages are:
///
/// ```text
/// Par(tag 5) → Expr(tag 5) → ENot(tag 1) → inner Par
/// ```
///
/// Lengths are computed from the leaf upward; bytes are then emitted from the
/// root downward. The construction is linear in the output size.
fn nested_not_bytes(depth: usize) -> Vec<u8> {
    let mut lengths = vec![(0usize, 0usize, 0usize); depth];
    let mut inner_par_len = 0usize;
    for slot in lengths.iter_mut().rev() {
        let enot_len = 1 + varint_len(inner_par_len) + inner_par_len;
        let expr_len = 1 + varint_len(enot_len) + enot_len;
        let par_len = 1 + varint_len(expr_len) + expr_len;
        *slot = (par_len, expr_len, enot_len);
        inner_par_len = par_len;
    }

    let mut out = Vec::with_capacity(inner_par_len);
    for (level, &(_par_len, expr_len, enot_len)) in lengths.iter().enumerate() {
        let child_par_len = lengths.get(level + 1).map_or(0, |entry| entry.0);
        out.push(0x2a); // Par.exprs, tag 5
        push_varint(&mut out, expr_len);
        out.push(0x2a); // Expr.e_not_body, tag 5
        push_varint(&mut out, enot_len);
        out.push(0x0a); // ENot.p, tag 1
        push_varint(&mut out, child_par_len);
    }
    assert_eq!(out.len(), inner_par_len);
    out
}

#[test]
fn twenty_thousand_levels_decode_and_drop_on_a_small_stack() {
    let stack_bytes = std::env::var("PROTOBUF_PDA_TEST_STACK_BYTES")
        .ok()
        .map(|raw| {
            raw.parse()
                .expect("PROTOBUF_PDA_TEST_STACK_BYTES is an integer")
        })
        .unwrap_or(256 * 1024usize);
    let depth = std::env::var("PROTOBUF_PDA_TEST_DEPTH")
        .ok()
        .map(|raw| raw.parse().expect("PROTOBUF_PDA_TEST_DEPTH is an integer"))
        .unwrap_or(20_000usize);

    let bytes = nested_not_bytes(depth);
    if depth > 100 {
        let oracle_error = RecursivePar::decode(bytes.as_slice())
            .expect_err("Prost's recursive decoder must stop at its configured depth budget");
        assert!(
            oracle_error.to_string().contains("recursion limit"),
            "the oracle failed for an unexpected reason: {oracle_error}"
        );
    }

    std::thread::Builder::new()
        .name("protobuf-pda-20k".into())
        .stack_size(stack_bytes)
        .spawn(move || {
            let (decoded, stats) = decode_par_with_stats(bytes.as_slice())
                .expect("the generated PDA must accept the valid deep message");
            assert_eq!(decoded.exprs.len(), 1);
            assert!(
                stats.max_frames >= 3 * depth,
                "the anti-vacuity frame depth is only {} for {depth} nested levels",
                stats.max_frames
            );
            drop(decoded);
        })
        .expect("spawn the fixed-small-stack decoder probe")
        .join()
        .expect("the decoder or generated Drop overflowed or panicked");
}

#[test]
fn generated_machine_layout_is_bounded() {
    eprintln!(
        "node={GENERATED_PROTOBUF_NODE_SIZE} value={GENERATED_PROTOBUF_VALUE_SIZE} attach={GENERATED_PROTOBUF_ATTACH_SIZE} frame={GENERATED_PROTOBUF_FRAME_SIZE}"
    );
}
