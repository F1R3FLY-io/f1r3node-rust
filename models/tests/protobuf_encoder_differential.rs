//! # The PROTOBUF encoder differential — two properties, and a mutation proof
//!
//! ```text
//!   ∀t. protobuf_encoder::encode_to_vec(&t) == recursive_oracle_encode(&t)
//!   ∀t. protobuf_encoder::encoded_len(&t)   == recursive_oracle_len(&t)
//! ```
//!
//! The oracle deliberately reuses the generated bounded-field program while
//! interpreting its descents with native recursion. The production encoder
//! interprets the same program with explicit frames. This separates byte-schema
//! agreement from control-flow equivalence without leaving recursive code in the
//! production library.
//!
//! ## ⚠ Why byte identity is the property and length identity is not
//!
//! The two are asserted together and they are not redundant, but only one of
//! them can catch the defect this file exists for. **A field-order mistake
//! preserves the length exactly** — the same fields, the same bytes, in a
//! different order — so a length check, a round-trip, and any decode-based
//! comparison all pass. That is not hypothetical: the *serde* order of
//! `TaggedContinuation` already produced "a 95-byte encoding with its halves
//! exchanged" in this campaign, and it was the write differential that caught it.
//!
//! Length identity earns its place separately: it is the property of
//! [`protobuf_encoder::encoded_len`] *on its own*, which is the `` $\Theta(n)$ ``
//! twin of prost's `` $\Theta(d^2)$ `` `Message::encoded_len` and is valuable
//! wherever a size is needed without the bytes.
//!
//! ## ★★ Anti-vacuity: three mutations, each of which must ASSERT IT APPLIED
//!
//! A green differential between two functions that are secretly the same
//! function is indistinguishable from a green differential between two functions
//! that agree. [`the_protobuf_differential_can_go_red`] settles that by
//! construction. Each mutation:
//!
//! 1. constructs the bytes the mutated encoder *would* produce, from the oracle's
//!    own output — never from the encoder under test, which could not disagree
//!    with itself;
//! 2. **asserts the mutation changed something**, before asking the verdict
//!    anything. Two near-misses in this campaign were mutations that reported
//!    green because they had not applied, and one that stayed green because it
//!    did not change what was being compared;
//! 3. is then fed to the very verdict the properties above use, and must be
//!    REJECTED, naming the clause.
//!
//! | # | mutation | must fail as |
//! |---|---|---|
//! | M1 | `Par`'s fields in DECLARATION order rather than min-tag | byte identity, in the `bundles`/`connectives` region |
//! | M2 | `TaggedContinuation`'s `guard` before its oneof | byte identity — ★ **same length, same byte multiset** |
//! | M3 | skip-if-default dropped on one `bool` | byte identity **and** length identity |
//!
//! ## Runner discipline
//!
//! ⚠ `cargo test` runs a test body on a **spawned thread that honours
//! `RUST_MIN_STACK`**; `cargo nextest` runs it on the process **main thread,
//! which `RUST_MIN_STACK` does not affect**. f1r3node CI runs
//! `cargo test --release -p models` only. Every deep body here therefore runs
//! inside an explicit `std::thread::Builder::new().stack_size(N)` — the
//! precedent is `bincode_decoder_shapes.rs:639-660` and
//! `bincode_encoder_differential.rs:646-661` — and each such test's doc comment
//! states the stack it passes on **under both runners**.

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, Connective, Expr, ListParWithRandom, New, Par, ParWithRandom, Send,
    TaggedContinuation,
};
use models::rust::par_bag::canonical_expr_oneof_tag;
use models::rust::rholang::bincode_schema_tables::EXPR_INSTANCE_VARIANT_COUNT;
use models::rust::rholang::protobuf_encoder;
use models::rust::rholang::protobuf_schema::{ProtobufDescent, ProtobufNode, NO_RESUME};
use models::rust::rholang::protobuf_schema_tables::{
    expr_instance_protobuf_kind_by_tag, EXPR_INSTANCE_PROTOBUF_VARIANTS,
    PROTOBUF_CONFORMANCE_REGISTRY, TAGGEDCONTINUATION_PROTOBUF_PROGRAM,
};
use models::rust::test_utils::test_utils::generate_par;
use proptest::prelude::*;
use prost::encoding::{encode_key, encode_varint, encoded_len_varint, key_len, WireType};
use prost::Message;

mod par_corpus;
use par_corpus as corpus;

fn recursive_oracle_len_from(node: &dyn ProtobufNode, from: usize) -> u64 {
    let (bounded, descent) = node.protobuf_len_step(from);
    bounded
        + match descent {
            ProtobufDescent::Done => 0,
            ProtobufDescent::Node {
                resume,
                tag,
                node: child,
            } => {
                let child_len = recursive_oracle_len_from(child, 0);
                message_contribution(tag, child_len) + recursive_oracle_resume(node, resume)
            }
            ProtobufDescent::Seq {
                resume,
                tag,
                len,
                seq,
            } => {
                let children = (0..len)
                    .map(|index| {
                        message_contribution(
                            tag,
                            recursive_oracle_len_from(seq.protobuf_get(index), 0),
                        )
                    })
                    .sum::<u64>();
                children + recursive_oracle_resume(node, resume)
            }
            ProtobufDescent::Map { resume, tag, map } => {
                let default_par = Par::default();
                let entries = map
                    .iter()
                    .map(|(key, value)| {
                        let key_part = if key.is_empty() {
                            0
                        } else {
                            prost::encoding::string::encoded_len(1, key) as u64
                        };
                        let value_part = if value == &default_par {
                            0
                        } else {
                            message_contribution(2, recursive_oracle_len_from(value, 0))
                        };
                        message_contribution(tag, key_part + value_part)
                    })
                    .sum::<u64>();
                entries + recursive_oracle_resume(node, resume)
            }
            ProtobufDescent::EPathMapSnapshot { resume, tag, map } => {
                message_contribution(tag, map.trie_snapshot().len() as u64)
                    + recursive_oracle_resume(node, resume)
            }
        }
}

fn recursive_oracle_resume(node: &dyn ProtobufNode, resume: u16) -> u64 {
    if resume == NO_RESUME {
        0
    } else {
        recursive_oracle_len_from(node, usize::from(resume))
    }
}

fn message_contribution(tag: u32, len: u64) -> u64 {
    key_len(tag) as u64 + encoded_len_varint(len) as u64 + len
}

fn recursive_oracle_emit_from(node: &dyn ProtobufNode, from: usize, out: &mut Vec<u8>) {
    match node.protobuf_emit(from, out) {
        ProtobufDescent::Done => {}
        ProtobufDescent::Node {
            resume,
            tag,
            node: child,
        } => {
            emit_child_header(tag, child, out);
            recursive_oracle_emit_from(child, 0, out);
            recursive_oracle_emit_resume(node, resume, out);
        }
        ProtobufDescent::Seq {
            resume,
            tag,
            len,
            seq,
        } => {
            for index in 0..len {
                let child = seq.protobuf_get(index);
                emit_child_header(tag, child, out);
                recursive_oracle_emit_from(child, 0, out);
            }
            recursive_oracle_emit_resume(node, resume, out);
        }
        ProtobufDescent::Map { resume, tag, map } => {
            let default_par = Par::default();
            for (key, value) in map {
                let key_part = if key.is_empty() {
                    0
                } else {
                    prost::encoding::string::encoded_len(1, key) as u64
                };
                let value_part = if value == &default_par {
                    0
                } else {
                    message_contribution(2, recursive_oracle_len_from(value, 0))
                };
                encode_key(tag, WireType::LengthDelimited, out);
                encode_varint(key_part + value_part, out);
                if !key.is_empty() {
                    prost::encoding::string::encode(1, key, out);
                }
                if value != &default_par {
                    emit_child_header(2, value, out);
                    recursive_oracle_emit_from(value, 0, out);
                }
            }
            recursive_oracle_emit_resume(node, resume, out);
        }
        ProtobufDescent::EPathMapSnapshot { resume, tag, map } => {
            let snapshot = map.trie_snapshot();
            encode_key(tag, WireType::LengthDelimited, out);
            encode_varint(snapshot.len() as u64, out);
            out.extend_from_slice(snapshot);
            recursive_oracle_emit_resume(node, resume, out);
        }
    }
}

fn recursive_oracle_emit_resume(node: &dyn ProtobufNode, resume: u16, out: &mut Vec<u8>) {
    if resume != NO_RESUME {
        recursive_oracle_emit_from(node, usize::from(resume), out);
    }
}

fn emit_child_header(tag: u32, child: &dyn ProtobufNode, out: &mut Vec<u8>) {
    encode_key(tag, WireType::LengthDelimited, out);
    encode_varint(recursive_oracle_len_from(child, 0), out);
}

fn recursive_oracle_encode(value: &dyn ProtobufNode) -> Vec<u8> {
    let len = recursive_oracle_len_from(value, 0);
    let mut out = Vec::with_capacity(usize::try_from(len).expect("oracle length fits usize"));
    recursive_oracle_emit_from(value, 0, &mut out);
    out
}

// ===========================================================================
// §0  The VERDICTS, separated from the subject
// ===========================================================================

/// **The byte-identity verdict**, as a pure function of two observations.
///
/// Separating it from the encoder is what makes the mutation proof possible at
/// all: a verdict welded to the production encoder can only ever be handed bytes
/// that encoder produced, and every such observation is — by construction — one
/// it agrees with. With the judgement apart from the subject, the judge can be
/// shown a *wrong* answer and required to reject it.
fn byte_identity_verdict(label: &str, machine: &[u8], oracle: &[u8]) -> Result<(), String> {
    if machine == oracle {
        return Ok(());
    }
    if machine.len() != oracle.len() {
        return Err(format!(
            "PROTOBUF WRITE DIFFERENTIAL FAILED for `{label}`: length {} vs oracle {}. \
             A length difference means a field was emitted, omitted, or given the wrong \
             prefix — not merely mis-ordered.",
            machine.len(),
            oracle.len()
        ));
    }
    let at = machine
        .iter()
        .zip(oracle)
        .position(|(a, b)| a != b)
        .expect("equal length and unequal contents implies a differing index");
    Err(format!(
        "PROTOBUF WRITE DIFFERENTIAL FAILED for `{label}`: first difference at byte {at} \
         (machine 0x{:02x}, oracle 0x{:02x}); both are {} bytes. The two-pass emitter and \
        the recursive reference interpreter disagree, which is a CONSENSUS FORK.",
        machine[at],
        oracle[at],
        machine.len()
    ))
}

/// **The length-identity verdict.**
///
/// ⚠ It cannot see a field-order mistake, and says so — see the module header.
fn length_identity_verdict(label: &str, machine: usize, oracle: usize) -> Result<(), String> {
    if machine == oracle {
        return Ok(());
    }
    Err(format!(
        "PROTOBUF LENGTH DIFFERENTIAL FAILED for `{label}`: {machine} vs oracle {oracle}. \
         The memoized bottom-up length pass and the recursive reference interpreter disagree, so \
         every nested length prefix below this node is wrong."
    ))
}

/// Both properties, for one value.
fn assert_encodes_identically<T>(label: &str, value: &T)
where T: ProtobufNode + Message {
    let oracle_bytes = recursive_oracle_encode(value);
    let machine_bytes = protobuf_encoder::encode_to_vec(value);
    if let Err(why) = byte_identity_verdict(label, &machine_bytes, &oracle_bytes) {
        panic!("{why}");
    }
    if let Err(why) = length_identity_verdict(
        label,
        protobuf_encoder::encoded_len(value),
        usize::try_from(recursive_oracle_len_from(value, 0)).expect("oracle length fits usize"),
    ) {
        panic!("{why}");
    }
    // …and the length the machine reports must be the length it WROTE. A pass
    // that agreed with the oracle about the size while emitting a different
    // number of bytes would satisfy both verdicts above and still be broken.
    assert_eq!(
        machine_bytes.len(),
        protobuf_encoder::encoded_len(value),
        "`{label}`: the machine wrote {} bytes but reports an encoded length of {}. Its two \
         passes disagree with EACH OTHER, which no comparison against the oracle can see.",
        machine_bytes.len(),
        protobuf_encoder::encoded_len(value)
    );
    assert_eq!(
        value.encode_to_vec(),
        oracle_bytes,
        "{label}: Message surface"
    );
    assert_eq!(
        value.encoded_len(),
        oracle_bytes.len(),
        "{label}: Message length surface"
    );
}

/// `encode_into` must append exactly what `encode_to_vec` returns, and disturb
/// nothing already in the buffer.
fn assert_appends_identically<T>(label: &str, value: &T)
where T: ProtobufNode + Message {
    const PREFIX: &[u8] = b"\xDE\xAD\xBE\xEF";
    let mut buffer = PREFIX.to_vec();
    protobuf_encoder::encode_into(value, &mut buffer);
    assert_eq!(
        &buffer[..PREFIX.len()],
        PREFIX,
        "`{label}`: `encode_into` overwrote bytes that were already in the buffer"
    );
    let oracle = recursive_oracle_encode(value);
    if let Err(why) = byte_identity_verdict(label, &buffer[PREFIX.len()..], &oracle) {
        panic!("{why} (via `encode_into`)");
    }
}

#[test]
fn par_callback_failure_rolls_back_the_complete_append() {
    let value = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(17)),
    }]);
    let mut output = b"existing-prefix".to_vec();
    let unchanged = output.clone();

    let error = protobuf_encoder::ProtobufEncodePlan::new(&value)
        .try_encode_into_with_par(&mut output, |_par, output| {
            output.extend_from_slice(b"partial-callback-output");
            Err::<(), _>("intentional callback failure")
        })
        .expect_err("the injected Par callback must fail");

    assert_eq!(error, "intentional callback failure");
    assert_eq!(
        output, unchanged,
        "a callback error must restore the caller's exact original prefix"
    );
}

// ===========================================================================
// §1  The exhaustive corpus — shared, and not weakened for this direction
// ===========================================================================

/// ★ Coverage against the GENERATED variant set, never a hand list. A 37th arm
/// added to the `.proto` moves this bar by itself.
#[test]
fn every_expr_instance_encodes_identically() {
    let arms = corpus::every_expr_instance();
    assert_eq!(
        arms.len(),
        EXPR_INSTANCE_VARIANT_COUNT,
        "the corpus must carry one representative of EVERY generated ExprInstance arm; \
         the generated table has {} and the corpus has {}",
        EXPR_INSTANCE_VARIANT_COUNT,
        arms.len()
    );
    for (label, instance) in arms {
        let expr = Expr {
            expr_instance: Some(instance),
        };
        assert_encodes_identically(&format!("Expr::{label}"), &expr);
        assert_appends_identically(&format!("Expr::{label}"), &expr);
    }
}

/// The constant-time lookup and the diagnostic table are generated from the
/// same descriptor vector, while the corpus and emitted field key independently
/// witness that every current Rust arm is represented.
#[test]
fn generated_expr_oneof_lookup_is_exhaustive_and_matches_encoded_arms() {
    let arms = corpus::every_expr_instance();
    assert_eq!(arms.len(), EXPR_INSTANCE_VARIANT_COUNT);
    assert_eq!(arms.len(), EXPR_INSTANCE_PROTOBUF_VARIANTS.len());

    for ((label, instance), field) in arms.into_iter().zip(EXPR_INSTANCE_PROTOBUF_VARIANTS) {
        assert_eq!(label, field.name, "the corpus and descriptor table drifted");
        assert_eq!(
            expr_instance_protobuf_kind_by_tag(field.tag),
            Some(field.kind),
            "{label}: generated tag lookup disagrees with its descriptor table"
        );

        let bytes = protobuf_encoder::encode_to_vec(&Expr {
            expr_instance: Some(instance),
        });
        let mut input = bytes.as_slice();
        let key = prost::encoding::decode_varint(&mut input).unwrap_or_else(|error| {
            panic!("{label}: generated encoder wrote no field key: {error}")
        });
        assert_eq!(
            u32::try_from(key >> 3).ok(),
            Some(field.tag),
            "{label}: emitted arm tag disagrees with the descriptor"
        );
        assert_eq!(
            key & 0x07,
            field
                .kind
                .wire_type()
                .expect("a selected oneof arm has one concrete wire type")
                .code(),
            "{label}: emitted wire type disagrees with the descriptor kind"
        );
        assert_eq!(
            canonical_expr_oneof_tag(&bytes),
            Some(field.tag),
            "{label}: the shallow ParBag envelope proof missed a generated arm"
        );
    }

    for unknown in [0, 26, 38, u32::MAX] {
        assert_eq!(
            expr_instance_protobuf_kind_by_tag(unknown),
            None,
            "unknown tag {unknown} must retain the semantic fallback"
        );
    }
}

#[test]
fn every_connective_instance_encodes_identically() {
    for (label, instance) in corpus::every_connective_instance() {
        let connective = Connective {
            connective_instance: Some(instance),
        };
        assert_encodes_identically(&format!("Connective::{label}"), &connective);
    }
}

#[test]
fn every_unforgeable_and_opt_var_encodes_identically() {
    for (i, unf) in corpus::every_unforgeable().into_iter().enumerate() {
        assert_encodes_identically(&format!("GUnforgeable[{i}]"), &unf);
    }
    for (i, var) in corpus::every_opt_var().into_iter().enumerate() {
        let bp = BindPattern {
            patterns: vec![corpus::gint(i as i64)],
            remainder: var,
            free_count: i as i32,
        };
        assert_encodes_identically(&format!("BindPattern.remainder[{i}]"), &bp);
    }
}

/// The whole shared corpus, through every cold-store root.
///
/// ★ Reused AS IS from `models/tests/par_corpus/mod.rs` — the same
/// `par_corpus()`, `all_par_fields()`, `deep_par`, `deep_mixed_par` and both
/// `EPathMap` arms the bincode differential runs on. A protobuf direction tested
/// on a corpus the serde direction never sees would be a second corpus to keep
/// in step.
#[test]
fn every_root_and_shape_encodes_identically() {
    let mut checked = 0usize;
    for (label, par) in corpus::par_corpus() {
        assert_encodes_identically(&format!("Par::{label}"), &par);
        checked += 1;
    }
    for (label, v) in corpus::list_par_with_random_corpus() {
        assert_encodes_identically(&format!("ListParWithRandom::{label}"), &v);
        checked += 1;
    }
    for (label, v) in corpus::bind_pattern_corpus() {
        assert_encodes_identically(&format!("BindPattern::{label}"), &v);
        checked += 1;
    }
    for (label, v) in corpus::tagged_continuation_corpus() {
        assert_encodes_identically(&format!("TaggedContinuation::{label}"), &v);
        checked += 1;
    }
    for (label, v) in corpus::par_with_random_corpus() {
        assert_encodes_identically(&format!("ParWithRandom::{label}"), &v);
        checked += 1;
    }
    for (label, v) in corpus::list_bind_patterns_corpus() {
        assert_encodes_identically(&format!("ListBindPatterns::{label}"), &v);
        checked += 1;
    }
    assert!(
        checked >= 40,
        "the shared corpus collapsed to {checked} cases — a differential over an empty \
         corpus reports a comfortable pass"
    );
}

/// ⚠★ **The shapes the PROTOBUF direction has that the serde direction does
/// not**, enumerated rather than hoped for.
///
/// Each is a place where prost's rules differ from bincode's, so the shared
/// corpus — built for the serde differential — does not necessarily reach them:
///
/// * **skip-at-default** on every scalar family, including the awkward ones
///   (`-0.0` is not `0.0` bitwise but `f64` compares them EQUAL, and `GDouble`
///   is a `fixed64` carrying raw bits, so it is a `u64` here and skipping is by
///   integer comparison);
/// * a **map value at its default**, which prost omits from the entry —
///   and the predicate is the HAND-WRITTEN `PartialEq` that ignores
///   `locally_free`, so `Par { locally_free: vec![1], .. }` IS skipped;
/// * a **map key at its default** (the empty string), which prost also omits;
/// * a **oneof arm at its default**, which prost does NOT skip
///   (`scalar::Field::new_oneof` rewrites `Kind::Plain` to `Kind::Required`).
#[test]
fn the_protobuf_specific_awkward_shapes_encode_identically() {
    // ── skip-at-default: a Send with every flag false and every bytes empty ──
    assert_encodes_identically("Send::all-default", &Send::default());
    assert_encodes_identically("Send::one-flag", &Send {
        persistent: true,
        ..Default::default()
    });

    // ── ★ a map whose VALUE is default, and one whose value only carries
    //    `locally_free` — which the hand-written `PartialEq` treats as default ──
    for (label, value) in [
        ("default-value", Par::default()),
        // `<Par as PartialEq>::eq` ignores `locally_free`, so prost's
        // `val == &Par::default()` is TRUE here and the value is OMITTED.
        ("locally-free-only", corpus::tagged(0x7F)),
        ("real-value", corpus::gint(5)),
    ] {
        let par = models::par_from_default! {
            news: vec![New {
                bind_count: 1,
                p: None,
                uri: vec![],
                injections: {
                    let mut m = BTreeMap::new();
                    m.insert("k".to_string(), value.clone());
                    m
                },
                locally_free: vec![],
            }],
            ..Default::default()
        };
        assert_encodes_identically(&format!("New.injections::{label}"), &par);
    }

    // ── a map KEY at its default (the empty string), alone and mixed ──
    let par = models::par_from_default! {
        news: vec![New {
            bind_count: 0,
            p: None,
            uri: vec![],
            injections: {
                let mut m = BTreeMap::new();
                m.insert(String::new(), Par::default()); // BOTH halves skipped
                m.insert("a".to_string(), corpus::gint(1));
                m
            },
            locally_free: vec![],
        }],
        ..Default::default()
    };
    assert_encodes_identically("New.injections::empty-key-and-default-value", &par);

    // ── ★ oneof arms AT THEIR DEFAULTS, which prost must NOT skip ──
    for (label, instance) in [
        ("GBool(false)", ExprInstance::GBool(false)),
        ("GInt(0)", ExprInstance::GInt(0)),
        ("GString(empty)", ExprInstance::GString(String::new())),
        ("GUri(empty)", ExprInstance::GUri(String::new())),
        ("GByteArray(empty)", ExprInstance::GByteArray(Vec::new())),
        ("GDouble(+0.0)", ExprInstance::GDouble(0.0f64.to_bits())),
        ("GDouble(-0.0)", ExprInstance::GDouble((-0.0f64).to_bits())),
        ("GBigInt(empty)", ExprInstance::GBigInt(Vec::new())),
    ] {
        let expr = Expr {
            expr_instance: Some(instance),
        };
        assert_encodes_identically(&format!("oneof-default::{label}"), &expr);
        // …and the encoding must be NON-EMPTY, because an arm that prost wrote
        // and this encoder skipped would agree with an oracle that also skipped.
        assert!(
            !recursive_oracle_encode(&expr).is_empty(),
            "`{label}`: the ORACLE encoded a set-but-default oneof arm to zero bytes. That \
             would make this case vacuous — protobuf presence requires the arm to be written."
        );
    }

    // ── the whole `TaggedContinuation` corpus, which is the message whose two
    //    field orders are exactly reversed between the formats ──
    for (label, tc) in corpus::tagged_continuation_corpus() {
        assert_encodes_identically(&format!("TaggedContinuation::{label}"), &tc);
        assert_appends_identically(&format!("TaggedContinuation::{label}"), &tc);
    }
}

// ===========================================================================
// §2  ★★ ANTI-VACUITY — the differential can go RED
// ===========================================================================

/// Rebuild `Par`'s encoding with its fields in an arbitrary order.
///
/// ★ Built from the ORACLE's own per-field encodings, so the "mutated" bytes are
/// a genuine re-ordering of the truth rather than something this file invented.
/// A mutation constructed by the encoder under test could not disagree with it.
fn par_encoded_in_order(par: &Par, order: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for name in order {
        // One `Par` carrying ONLY this field, encoded by the oracle. Every `Par`
        // field is repeated-message or scalar, so its encoding is
        // self-delimiting and concatenating them in any order is a well-formed
        // protobuf message — which is exactly why the defect is invisible to a
        // decoder.
        let mut single = Par::default();
        match *name {
            "sends" => single.replace_sends(par.decode_occurrences()),
            "receives" => single.replace_receives(par.decode_occurrences()),
            "news" => single.replace_news(par.decode_occurrences()),
            "exprs" => single.replace_exprs(par.decode_occurrences()),
            "matches" => single.replace_matches(par.decode_occurrences()),
            "unforgeables" => single.replace_unforgeables(par.decode_occurrences()),
            "connectives" => single.replace_connectives(par.decode_occurrences()),
            "locally_free" => single.replace_locally_free(par.locally_free().to_vec()),
            "connective_used" => single.replace_connective_used(par.connective_used()),
            "bundles" => single.replace_bundles(par.decode_occurrences()),
            "conditionals" => single.replace_conditionals(par.decode_occurrences()),
            other => panic!("par_encoded_in_order: unknown field `{other}`"),
        }
        out.extend_from_slice(&recursive_oracle_encode(&single));
    }
    out
}

/// ★★★ **THE MUTATION PROOF.** Three perturbations, each of which must (a)
/// demonstrably apply and (b) be REJECTED by the very verdict the properties use.
///
/// A CONTROL passes the same verdict before and after all three, so a verdict
/// that rejected everything would fail here too.
#[test]
fn the_protobuf_differential_can_go_red() {
    // ── the fixture: a `Par` whose 11 fields are ALL populated, so every
    //    re-ordering is observable ──
    let par = models::par_from_default! {
        sends: vec![Send {
            chan: Some(corpus::gint(1)),
            persistent: true,
            ..Default::default()
        }],
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }],
        bundles: vec![models::rhoapi::Bundle {
            body: Some(corpus::gint(2)),
            write_flag: true,
            read_flag: false,
        }],
        connectives: vec![Connective {
            connective_instance: None,
        }],
        conditionals: vec![models::rhoapi::If {
            condition: Some(corpus::gint(3)),
            ..Default::default()
        }],
        locally_free: vec![0xAA, 0xBB],
        connective_used: true,
        ..Default::default()
    };
    let truth = recursive_oracle_encode(&par);

    // CONTROL: the real encoder, judged by the real verdict.
    assert!(
        byte_identity_verdict("control", &protobuf_encoder::encode_to_vec(&par), &truth).is_ok(),
        "the CONTROL must pass, or every rejection below is a statement about the judge \
         rather than about the mutation"
    );

    // ── M1: `Par`'s fields in DECLARATION order rather than ascending min tag ──
    //
    // This is the exact defect a generator that reused the bincode order table
    // would produce. `bundles` (11) and `conditionals` (12) are DECLARED before
    // `connectives` (8), `locally_free` (9) and `connective_used` (10).
    let declaration_order = [
        "sends",
        "receives",
        "news",
        "exprs",
        "matches",
        "unforgeables",
        "bundles",
        "connectives",
        "conditionals",
        "locally_free",
        "connective_used",
    ];
    let tag_order = [
        "sends",
        "receives",
        "news",
        "exprs",
        "matches",
        "unforgeables",
        "connectives",
        "locally_free",
        "connective_used",
        "bundles",
        "conditionals",
    ];
    // `Par` is repository-owned rather than generated. The control below
    // proves this is the custom encoder's actual tag order byte-for-byte.
    assert_ne!(
        declaration_order, tag_order,
        "M1 IS INERT: the two orders coincide, so the 'mutation' changes nothing and its \
         rejection below would be about something else"
    );
    let m1 = par_encoded_in_order(&par, &declaration_order);
    let m1_control = par_encoded_in_order(&par, &tag_order);
    assert_eq!(
        m1_control, truth,
        "the re-encoding helper must reproduce the oracle EXACTLY under the tag order, or M1 \
         is measuring the helper rather than the order"
    );
    assert_ne!(
        m1, truth,
        "M1 DID NOT APPLY: re-encoding `Par` in declaration order produced the oracle's own \
         bytes. Every mutation must demonstrate it changed something before its rejection \
         means anything."
    );
    let why = byte_identity_verdict("M1/declaration-order", &m1, &truth)
        .expect_err("M1 must be REJECTED");
    assert!(
        why.contains("first difference at byte"),
        "M1 must be rejected by the BYTE-IDENTITY clause — the fields are the same fields, so \
         a length check cannot see it; got: {why}"
    );
    // ★ …and the first difference must be in the region the reordering moved:
    // everything up to `unforgeables` is common to both orders.
    let common_prefix = par_encoded_in_order(&par, &[
        "sends",
        "receives",
        "news",
        "exprs",
        "matches",
        "unforgeables",
    ]);
    let at: usize = why
        .split("first difference at byte ")
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .and_then(|s| s.parse().ok())
        .expect("the verdict names the differing byte");
    assert_eq!(
        at,
        common_prefix.len(),
        "M1's first difference must fall exactly where the two orders diverge — at the end of \
         the common `sends..unforgeables` prefix ({} bytes), i.e. in the \
         `bundles`/`connectives` region",
        common_prefix.len()
    );

    // ── M2: `TaggedContinuation`'s `guard` before its oneof ──
    //
    // ★★ The encoding has the SAME LENGTH and the SAME BYTE MULTISET, so a
    // length check would NOT see it and neither would a round-trip. This is the
    // message whose SERDE order already cost this campaign a 95-byte encoding
    // with its halves exchanged — and for protobuf the correct order is the
    // OPPOSITE of that fix.
    let tc = TaggedContinuation {
        guard: Some(corpus::gint(11)),
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(corpus::gint(22)),
            random_state: vec![9, 9, 9],
        })),
        cost_region: None,
        clause_regions: Vec::new(),
    };
    let tc_truth = recursive_oracle_encode(&tc);
    assert!(
        byte_identity_verdict(
            "control-tc",
            &protobuf_encoder::encode_to_vec(&tc),
            &tc_truth
        )
        .is_ok(),
        "the TaggedContinuation control must pass"
    );
    let generated_tc: Vec<&str> = TAGGEDCONTINUATION_PROTOBUF_PROGRAM
        .iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(
        generated_tc,
        vec!["tagged_cont", "guard", "cost_region", "clause_regions"],
        "the generated `TaggedContinuation` prost program must put the ONEOF first (it holds \
         tags 1-2; `guard` is tag 3), or M2 perturbs the wrong order"
    );
    let guard_only = TaggedContinuation {
        guard: tc.guard.clone(),
        tagged_cont: None,
        cost_region: None,
        clause_regions: Vec::new(),
    };
    let cont_only = TaggedContinuation {
        guard: None,
        tagged_cont: tc.tagged_cont.clone(),
        cost_region: None,
        clause_regions: Vec::new(),
    };
    let m2 = {
        let mut v = recursive_oracle_encode(&guard_only);
        v.extend_from_slice(&recursive_oracle_encode(&cont_only));
        v
    };
    assert_eq!(
        m2.len(),
        tc_truth.len(),
        "M2 must PRESERVE the length — that is the whole point: a length check cannot see it"
    );
    let mut m2_sorted = m2.clone();
    let mut truth_sorted = tc_truth.clone();
    m2_sorted.sort_unstable();
    truth_sorted.sort_unstable();
    assert_eq!(
        m2_sorted, truth_sorted,
        "M2 must preserve the BYTE MULTISET too — the halves are exchanged, not altered"
    );
    assert_ne!(
        m2, tc_truth,
        "M2 DID NOT APPLY: writing `guard` before the oneof produced the oracle's own bytes"
    );
    let why =
        byte_identity_verdict("M2/guard-first", &m2, &tc_truth).expect_err("M2 must be REJECTED");
    assert!(
        why.contains("first difference at byte"),
        "M2 must be rejected by the BYTE-IDENTITY clause. Same length, same multiset — a \
         length check and a round-trip both PASS on it; got: {why}"
    );
    assert!(
        length_identity_verdict("M2/guard-first", m2.len(), tc_truth.len()).is_ok(),
        "★ and the LENGTH verdict must ACCEPT M2, which is the executable form of 'a length \
         check would not see it'"
    );

    // ── M3: skip-if-default dropped on one `bool` ──
    //
    // `Send.persistent` is tag 3. prost writes NOTHING for `persistent: false`
    // (`prost-derive-0.14.3/src/field/scalar.rs:116-125`); writing `3:0` anyway
    // is well-formed protobuf that every decoder accepts as `false`.
    let send = Send {
        chan: Some(corpus::gint(4)),
        persistent: false,
        ..Default::default()
    };
    let send_truth = recursive_oracle_encode(&send);
    assert!(
        byte_identity_verdict(
            "control-send",
            &protobuf_encoder::encode_to_vec(&send),
            &send_truth
        )
        .is_ok(),
        "the Send control must pass"
    );
    // Build the unskipped bytes with prost's OWN encoder, so the mutation is
    // "prost without the guard" rather than a hand-rolled varint.
    let m3 = {
        let mut v = send_truth.clone();
        prost::encoding::bool::encode(3u32, &false, &mut v);
        v
    };
    assert_ne!(
        m3, send_truth,
        "M3 DID NOT APPLY: emitting `persistent: false` produced the oracle's own bytes, which \
         would mean prost does not skip at default after all"
    );
    assert!(
        m3.len() > send_truth.len(),
        "M3 must LENGTHEN the encoding — that is what distinguishes it from M1 and M2"
    );
    let why = byte_identity_verdict("M3/no-skip-at-default", &m3, &send_truth)
        .expect_err("M3 must be REJECTED by byte identity");
    assert!(
        why.contains("length"),
        "M3 must be rejected by the LENGTH clause of the byte verdict; got: {why}"
    );
    let why = length_identity_verdict("M3/no-skip-at-default", m3.len(), send_truth.len())
        .expect_err("M3 must ALSO be rejected by the length verdict");
    assert!(why.contains("disagree"), "got: {why}");

    // ── the controls again, AFTER all three, so a verdict that latched into
    //    rejecting cannot pass this test ──
    for (label, value, truth) in [
        (
            "control-after/Par",
            protobuf_encoder::encode_to_vec(&par),
            truth,
        ),
        (
            "control-after/TC",
            protobuf_encoder::encode_to_vec(&tc),
            tc_truth,
        ),
        (
            "control-after/Send",
            protobuf_encoder::encode_to_vec(&send),
            send_truth,
        ),
    ] {
        assert!(
            byte_identity_verdict(label, &value, &truth).is_ok(),
            "the verdict must still ACCEPT the truth after rejecting all three mutations"
        );
    }
}

/// The generated prost table is what the assertions are measured against, so a
/// degenerate table would make the coverage above vacuous.
#[test]
fn the_generated_protobuf_table_is_not_degenerate() {
    assert!(
        PROTOBUF_CONFORMANCE_REGISTRY.len() >= 56,
        "the prost registry collapsed to {} rows",
        PROTOBUF_CONFORMANCE_REGISTRY.len()
    );
    assert!(
        !PROTOBUF_CONFORMANCE_REGISTRY
            .iter()
            .any(|(name, _)| *name == "Par"),
        "repository-owned Par must not acquire a descriptor-generated field program"
    );
    assert_eq!(
        TAGGEDCONTINUATION_PROTOBUF_PROGRAM.len(),
        4,
        "`TaggedContinuation` includes the oneof, guard, and both authority fields"
    );
}

// ===========================================================================
// §3  Proptest — depth and breadth past what enumeration reaches
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Both properties over arbitrary generated terms.
    #[test]
    fn generated_pars_encode_identically(par in generate_par(3)) {
        let oracle = recursive_oracle_encode(&par);
        let machine = protobuf_encoder::encode_to_vec(&par);
        prop_assert!(
            byte_identity_verdict("generated", &machine, &oracle).is_ok(),
            "{}", byte_identity_verdict("generated", &machine, &oracle).unwrap_err()
        );
        prop_assert_eq!(protobuf_encoder::encoded_len(&par) as u64, recursive_oracle_len_from(&par, 0));
        prop_assert_eq!(par.encode_to_vec(), oracle);
    }

    /// Both properties through every cold-store root.
    #[test]
    fn all_roots_encode_identically(par in generate_par(2)) {
        let datum = ListParWithRandom { pars: vec![par.clone()], random_state: vec![1u8, 2, 3],  cost_region: None, };
        let pattern = BindPattern { patterns: vec![par.clone()], remainder: None, free_count: 3 };
        let cont = TaggedContinuation {
            guard: Some(par.clone()),
            tagged_cont: Some(TaggedCont::ScalaBodyRef(7)),
            cost_region: None,
            clause_regions: Vec::new(),
        };
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&par), recursive_oracle_encode(&par));
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&datum), recursive_oracle_encode(&datum));
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&pattern), recursive_oracle_encode(&pattern));
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&cont), recursive_oracle_encode(&cont));
        prop_assert_eq!(protobuf_encoder::encoded_len(&cont) as u64, recursive_oracle_len_from(&cont, 0));
    }

    /// Wide sequences: the counted-repeat path past its first iteration, where
    /// every element carries its own key and length prefix.
    #[test]
    fn wide_sequences_encode_identically(n in 0usize..64) {
        let par = models::par_from_default! {
            exprs: (0..n).map(|i| Expr { expr_instance: Some(ExprInstance::GInt(i as i64)) }).collect(),
            sends: (0..n).map(|i| Send { chan: Some(corpus::gint(i as i64)), ..Default::default() }).collect(),
            ..Default::default()
        };
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&par), recursive_oracle_encode(&par));
    }

    /// Maps: keys and values independently at or off their defaults.
    #[test]
    fn maps_encode_identically(keys in prop::collection::vec("[a-c]{0,2}", 0..6), fill in prop::collection::vec(any::<bool>(), 0..6)) {
        let mut injections = BTreeMap::new();
        for (i, k) in keys.iter().enumerate() {
            let v = if fill.get(i).copied().unwrap_or(false) { corpus::gint(i as i64) } else { Par::default() };
            injections.insert(k.clone(), v);
        }
        let par = models::par_from_default! {
            news: vec![New { bind_count: 1, p: None, uri: vec![], injections, locally_free: vec![] }],
            ..Default::default()
        };
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&par), recursive_oracle_encode(&par));
        prop_assert_eq!(protobuf_encoder::encoded_len(&par) as u64, recursive_oracle_len_from(&par, 0));
    }
}

// ===========================================================================
// §4  Depth — where the two encoders' complexity classes separate
// ===========================================================================

/// The recursive reference is intentionally exercised only on bounded terms;
/// the production machine's unbounded-depth evidence lives in the dedicated
/// small-stack suite.
#[test]
fn bounded_recursive_oracle_terms_encode_identically() {
    for depth in [1usize, 2, 8, 16, 32] {
        let par = corpus::deep_par(depth);
        assert_encodes_identically(&format!("deep_par({depth})"), &par);
        let mixed = corpus::deep_mixed_par(depth);
        assert_encodes_identically(&format!("deep_mixed_par({depth})"), &mixed);
    }
}

/// ★★ The repository-owned PathMap `Par` is an opaque terminal to the
/// generic protobuf PDA.
///
/// Its component bodies and lengths are already present in canonical trie
/// keys, so exposing decoded child nodes to the generic length table would be a
/// representation regression. Depth and sibling width must therefore leave
/// both the generic length table and operation stacks constant.
#[test]
fn pathmap_backed_par_does_not_rebuild_a_generated_length_table() {
    for depth in [4usize, 8, 16, 32] {
        let par = corpus::deep_par(depth);
        assert_eq!(
            protobuf_encoder::len_table_size(&par),
            1,
            "depth-{depth} Par must occupy exactly the root length-table slot and expose no decoded children"
        );
        let (len_hw, emit_hw) = protobuf_encoder::op_stack_high_water(&par);
        assert!(
            len_hw <= 2 && emit_hw <= 1,
            "depth-{depth} opaque Par used generic PDA stacks: ({len_hw}, {emit_hw})"
        );
    }

    let wide = models::par_from_default! {
        exprs: (0..4096)
            .map(|i| Expr {
                expr_instance: Some(ExprInstance::GInt(i)),
            })
            .collect(),
        ..Default::default()
    };
    let (len_hw, emit_hw) = protobuf_encoder::op_stack_high_water(&wide);
    assert!(
        len_hw <= 2 && emit_hw <= 1,
        "a 4,096-component PathMap Par must remain one opaque protobuf operation; \
         observed (len {len_hw}, emit {emit_hw})"
    );
    assert_eq!(
        protobuf_encoder::len_table_size(&wide),
        1,
        "wide PathMap Par must occupy only the root length-table slot"
    );
}
