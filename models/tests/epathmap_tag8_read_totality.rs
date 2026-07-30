//! # `EPathMap` proto tag 8 — a writer that emits what its own reader refuses
//!
//! ## The asymmetry
//!
//! `EPathMap::encode_raw` has a **value arm**: a non-empty *ground* map is
//! written as proto field 8 (`serialized_paths`, `bytes`) carrying `U(m)` — the
//! length-framed concatenation of `encode_trie_path(entry)` over the entry trie
//! in trie order (`models/src/rust/rhoapi_ext.rs:972`,
//! `encode_ground_field8(&self.path_stream(), buf)`).
//!
//! * `encode_trie_path` is documented **TOTAL** and **unlimited in depth** (R3F-2,
//!   `canonical_path.rs:680-682`: *"unlimited depth; iterative; no panics"*).
//! * `EPathMap::merge_field`'s tag-8 arm decodes each key with
//!   `decode_trie_path` (`rhoapi_ext.rs:1069`), which refuses past
//!   `COLLECTION_DEPTH_LIMIT` **collection levels** with
//!   `CodecError::DepthLimitExceeded`.
//!
//! ⇒ A node can write a byte string that it — and every peer running the same
//! code — will refuse to read. That is the same class as the depth-33 prost read
//! ceiling, on a consensus wire.
//!
//! ## ★★ Why the "anchored ceilings" argument does NOT cover this path
//!
//! `models/tests/par_prost_depth_ceiling.rs::the_two_read_ceilings_are_anchored_together`
//! holds that `COLLECTION_DEPTH_LIMIT` and prost's `RECURSION_LIMIT` coincide on
//! the nested-list shape, and concludes: *"Raise the prost ceiling alone and the
//! trie decoder becomes the binding constraint one level lower; raise
//! `COLLECTION_DEPTH_LIMIT` alone and prost becomes it. Either move buys
//! nothing."*
//!
//! ⚠ **That conclusion is true of one transport path and false of the other**,
//! and the difference is *which* protobuf field carries the entry:
//!
//! | how an entry reaches an `EPathMap` | prost message levels spent on it | binding constraint |
//! |---|---|---|
//! | tag 1 — `ps`, `repeated Par` | one nested-message ladder per bracket | **prost** binds first, at 33 |
//! | ★ tag 8 — `serialized_paths`, `bytes` | **ZERO** — the payload is opaque | **`COLLECTION_DEPTH_LIMIT` alone** |
//! | built in-process by the reducer | none — never serialised | the evaluator |
//!
//! A `bytes` field is not descended into. `prost::encoding::bytes::merge` reads a
//! varint length and copies that many bytes; the trie-key stream inside is
//! structure to *this codec* and payload to *protobuf*. It is the same reason
//! `ProduceEventProto.outputValue` (`repeated bytes`) lets a block body carry
//! `Par`s of any depth without the block failing to decode — stated in
//! `par_prost_depth_ceiling.rs`'s own header.
//!
//! ⇒ **On the tag-8 path there is no second constraint behind
//! `COLLECTION_DEPTH_LIMIT`.** Lifting it buys the whole depth range, and it is
//! the only thing standing between a total writer and a partial reader.
//!
//! [`the_tag8_payload_costs_prost_no_message_levels`] measures the middle column
//! rather than asserting it: it drives one deep entry through **both** fields of
//! the same message type and shows the two failures are *different*, at
//! *different depths*, with the tag-8 envelope decoding cleanly while its
//! payload is refused.
//!
//! ## ⚠ What this file deliberately does NOT decide
//!
//! `EPathMap`'s **representation** is under a separate open ruling (#116: it must
//! *be* a trie map). Nothing here changes the stored form, the wire arms, the
//! choice between them, `U(m)`'s framing, or the key grammar. The only thing
//! that moves is *how deep a key the reader accepts* — a totality property of the
//! decoder, orthogonal to what the value is represented as.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path, CodecError};
use models::rust::rhoapi_ext::EPathMap;
use prost::Message;

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn gint(value: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

/// `(((…(7)…)))` — `wrappers` nested single-element ground tuples.
///
/// ⚠ A **tuple**, not a list: `canonical_path.rs`'s `BareTopLevelList` rule
/// forbids a top-level bare `0x0B`, and a tuple is `eval_stable` at every level,
/// which is what puts the containing map on the *ground* wire arm and therefore
/// on tag 8 at all.
fn deep_tuple(wrappers: u32) -> Par {
    let mut par = gint(7);
    for _ in 0..wrappers {
        par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                    ps: vec![par],
                    locally_free: Vec::new(),
                    connective_used: false,
                })),
            }],
            ..Default::default()
        };
    }
    par
}

/// A single-entry `EPathMap` holding `entry`.
fn map_of(entry: Par) -> EPathMap {
    EPathMap::new(vec![entry], Vec::new(), false, None)
}

/// The `Par` wrapper an `EPathMap` travels inside, so the outer decode is an
/// ordinary `Par::decode` exactly as production performs it.
fn par_of(map: EPathMap) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    }
}

/// Whether a `prost::DecodeError` is the recursion-limit refusal.
///
/// ⚠ Matched on the rendered message because `DecodeErrorKind` is private in
/// `prost-0.14.3`. The string is prost's own
/// (`DecodeErrorKind::RecursionLimitReached`), and matching *some* error would
/// prove nothing: a truncated buffer and a wrong tag are also errors.
fn is_recursion_limit(error: &prost::DecodeError) -> bool {
    format!("{error}").contains("recursion limit reached")
}

/// Whether a `prost::DecodeError` came from this codec's own depth refusal.
fn is_trie_depth_limit(error: &prost::DecodeError) -> bool {
    format!("{error}").contains("DepthLimitExceeded")
}

// ---------------------------------------------------------------------------
// §1 the depth at which each path fails — MEASURED, not assumed
// ---------------------------------------------------------------------------

/// The greatest `wrappers` for which `f` succeeds, searched upward from 1 and
/// bounded so a total reader terminates the search rather than running forever.
fn last_accepting(ceiling: u32, mut accepts: impl FnMut(u32) -> bool) -> Option<u32> {
    let mut last = None;
    for wrappers in 1..=ceiling {
        match accepts(wrappers) {
            true => last = Some(wrappers),
            false => return last,
        }
    }
    last
}

/// ★★ **THE ASYMMETRY, exhibited on one term through both fields.**
///
/// The tag-8 stream is written for an entry of *any* depth and read back only
/// below `COLLECTION_DEPTH_LIMIT`; and the prost envelope carrying it decodes
/// **cleanly** at depths where the tag-1 path has already been refused for
/// hundreds of levels. So the two ceilings do not coincide on this path, and
/// prost is not the constraint behind the trie codec's.
#[test]
fn the_tag8_payload_costs_prost_no_message_levels() {
    // ── (a) the trie WRITER is total: a key exists at every depth we try. ──
    const PROBE_DEPTH: u32 = 400;
    let deep = deep_tuple(PROBE_DEPTH);
    let key = encode_trie_path(&deep);
    assert!(
        !key.is_empty(),
        "the trie encoder is documented TOTAL (R3F-2, `canonical_path.rs:680`); it produced no \
         key at depth {PROBE_DEPTH}"
    );

    // ── (b) the ENVELOPE around that key encodes AND decodes through prost. ──
    //
    // This is the measurement the header's middle column claims: a `bytes`
    // field costs ZERO nested-message levels, so an envelope whose tag-8
    // payload nests 400 deep is, to prost, a flat message with a big byte
    // string in it.
    let ground = map_of(deep.clone());
    let envelope = par_of(ground);
    let bytes = envelope.encode_to_vec();
    assert!(
        bytes.len() > PROBE_DEPTH as usize,
        "control: the encoding must actually contain the deep key, not an elision"
    );

    // ── (c) the SAME depth through tag 1 is refused by prost itself. ──
    //
    // `Par → Expr → ETuple → Par` is one bracket per three message levels, so
    // 400 brackets is ~1,200 levels against prost's budget of 100.
    let via_tag1 = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![deep.clone()],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }],
        ..Default::default()
    };
    let tag1_error = Par::decode(&via_tag1.encode_to_vec()[..])
        .expect_err("depth 400 through a nested-message ladder must exceed prost's budget");
    assert!(
        is_recursion_limit(&tag1_error),
        "control: the tag-1 path must fail on prost's RECURSION LIMIT, not on something else — \
         got {tag1_error:?}"
    );

    // ── (d) ★ and the tag-8 envelope fails on the TRIE codec's limit instead,
    //        which is what proves the two ceilings are independent here. ──
    match Par::decode(&bytes[..]) {
        Err(error) => {
            assert!(
                is_trie_depth_limit(&error),
                "★ the tag-8 envelope was refused, but NOT by the trie codec's depth limit. If \
                 this is prost's recursion limit then the header's claim is wrong and the two \
                 ceilings really are anchored on this path too. Got: {error:?}"
            );
            assert!(
                !is_recursion_limit(&error),
                "★ prost's recursion limit fired on a `bytes` payload, which would mean prost \
                 descends into byte strings. Got: {error:?}"
            );
        }
        Ok(_) => {
            // The reader has been made TOTAL: the envelope round-trips, and the
            // entry survives. This is the post-fix branch, and it asserts the
            // fix rather than merely tolerating it.
            let back = Par::decode(&bytes[..]).expect("re-decode");
            let ExprInstance::EPathmapBody(map) = back.exprs[0]
                .expr_instance
                .clone()
                .expect("the decoded Par carries its expr")
            else {
                panic!("the decoded Par is not an EPathMap");
            };
            let entries = map.ps();
            assert_eq!(entries.len(), 1, "the deep entry survived as ONE entry");
            assert_eq!(
                encode_trie_path(&entries[0]),
                key,
                "the decoded entry is not a byte-level fixed point of its own key"
            );
        }
    }
}

/// ★ The trie codec's own boundary, **searched** rather than transcribed.
///
/// Before the repair this finds a finite last-accepting depth; after it, the
/// search runs to its probe ceiling and returns it, which is the observable
/// difference between "bounded" and "total".
#[test]
fn the_trie_reader_boundary_is_searched_not_transcribed() {
    /// High enough that finding it as the answer means *no limit was hit* —
    /// twelve times the retired `COLLECTION_DEPTH_LIMIT` of 32.
    const PROBE_CEILING: u32 = 384;

    let boundary = last_accepting(PROBE_CEILING, |wrappers| {
        let key = encode_trie_path(&deep_tuple(wrappers));
        decode_trie_path(&key).is_ok()
    })
    .expect("depth 1 must decode — otherwise the probe itself is broken");

    // Non-vacuity: the encoder must be total across the whole probe range, or
    // "the decoder accepted everything" would be a statement about the encoder.
    for wrappers in [1u32, 32, 33, 100, PROBE_CEILING] {
        assert!(
            !encode_trie_path(&deep_tuple(wrappers)).is_empty(),
            "the trie ENCODER produced nothing at depth {wrappers}"
        );
    }

    assert_eq!(
        boundary, PROBE_CEILING,
        "★ the trie-key READER is still bounded: it accepts {boundary} collection levels and \
         refuses {}. The writer (`encode_trie_path`) is TOTAL at every one of those depths, so \
         this node emits tag-8 byte strings it will not read back. The owner ruling (2026-07-29) \
         is that there is NO artificial depth cap for consensus, so the repair is an UNBOUNDED \
         READER — not a capped writer.",
        boundary + 1
    );
}

/// A deep key REJECTED for a reason other than depth must still be rejected —
/// the repair lifts a *ceiling*, not the grammar.
///
/// ⚠ This is the half of a totality change that is easy to get wrong: making a
/// reader total by making it *permissive* widens the accept set, which is a fork.
/// Each case below is refused for its own structural reason at a depth well past
/// the retired limit.
#[test]
fn lifting_the_ceiling_does_not_widen_the_grammar() {
    use models::rust::canonical_path::tag;

    // A truncated key: the innermost tuple's element is missing.
    let mut truncated = encode_trie_path(&deep_tuple(64));
    truncated.pop();
    assert!(
        decode_trie_path(&truncated).is_err(),
        "a truncated deep key must still be refused"
    );

    // A reserved tag at depth: `0x10..=0xFF` may never begin a segment.
    let mut reserved: Vec<u8> = Vec::new();
    for _ in 0..64 {
        reserved.push(tag::ETUPLE);
        reserved.push(1); // one element
    }
    reserved.push(tag::RESERVED_FLOOR);
    assert!(
        matches!(
            decode_trie_path(&reserved),
            Err(CodecError::ReservedTag(_)) | Err(CodecError::Truncated)
        ),
        "a reserved tag at depth 64 must still be refused: {:?}",
        decode_trie_path(&reserved)
    );

    // The trie-only escape below a top-level segment position: the hereditary
    // rule forbids it at ANY depth, not only below 32.
    let mut escaped: Vec<u8> = Vec::new();
    for _ in 0..64 {
        escaped.push(tag::ETUPLE);
        escaped.push(1);
    }
    escaped.push(tag::ESCAPE);
    escaped.push(1);
    escaped.push(0);
    assert!(
        decode_trie_path(&escaped).is_err(),
        "a nested escape at depth 64 must still be refused: {:?}",
        decode_trie_path(&escaped)
    );

    // The empty input is still the empty input.
    assert_eq!(decode_trie_path(&[]), Err(CodecError::EmptyInput));
}

/// ★ The repair must be a **round trip**, not merely an acceptance: a deep key
/// that decodes must re-encode to the bytes it came from, at depths the retired
/// limit refused.
///
/// This is what separates "the reader is total" from "the reader stopped
/// complaining": a decoder that dropped levels would accept and answer wrongly.
#[test]
fn deep_keys_round_trip_past_the_retired_limit() {
    for wrappers in [33u32, 40, 64, 128, 256] {
        let original = deep_tuple(wrappers);
        let key = encode_trie_path(&original);
        let Ok(back) = decode_trie_path(&key) else {
            // Pre-repair: the reader is bounded and this test is the RED.
            panic!(
                "depth {wrappers} does not decode. The writer produced {} bytes for it, so this \
                 node emits a tag-8 key stream it cannot read back.",
                key.len()
            );
        };
        assert_eq!(
            encode_trie_path(&back),
            key,
            "depth {wrappers} decoded but is not a byte-level fixed point — levels were lost"
        );
    }
}

/// ★★ The whole point, end to end: a ground `EPathMap` whose entry is deeper
/// than the retired limit **round-trips through the production `Par` wire**.
///
/// This is the consensus-class statement. `map_of` puts the entry on the ground
/// arm, so `encode_raw` takes the tag-8 branch; the envelope is an ordinary
/// `Par`, decoded by an ordinary `Par::decode`, which is what
/// `rholang/.../reduce.rs` performs on a replay pass.
#[test]
fn a_ground_epathmap_deeper_than_the_retired_limit_round_trips_on_the_wire() {
    for wrappers in [33u32, 48, 96] {
        let entry = deep_tuple(wrappers);
        let envelope = par_of(map_of(entry.clone()));
        let bytes = envelope.encode_to_vec();

        let back = Par::decode(&bytes[..]).unwrap_or_else(|e| {
            panic!(
                "a ground EPathMap with a depth-{wrappers} entry encoded to {} bytes and did not \
                 decode: {e:?}. The writer emitted what its own reader refuses.",
                bytes.len()
            )
        });

        assert_eq!(
            back.encode_to_vec(),
            bytes,
            "the depth-{wrappers} envelope is not a byte-level fixed point"
        );

        let ExprInstance::EPathmapBody(map) = back.exprs[0]
            .expr_instance
            .clone()
            .expect("the decoded Par carries its expr")
        else {
            panic!("the decoded Par is not an EPathMap");
        };
        assert_eq!(map.ps().len(), 1, "depth {wrappers}: the entry set is not a singleton");
        assert_eq!(
            encode_trie_path(&map.ps()[0]),
            encode_trie_path(&entry),
            "depth {wrappers}: the decoded entry is not the entry that was written"
        );
    }
}
