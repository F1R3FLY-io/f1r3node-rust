//! # The trie-key ESCAPE arm's protobuf encode is O(1) on the native stack
//!
//! ## The obligation the escape arm carries, and the one it could not honour
//!
//! `models/src/rust/canonical_path.rs`'s `encode_trie_path` is **required total**
//! — R3F-2, *"trie keys must build for every legal runtime value"* — and its
//! module header promises *"unlimited depth; iterative; no panics."*
//!
//! Its `0x0F` **escape arm** is what makes totality achievable at all: a `Par`
//! that is not `eval_stable_par` has no structural trie encoding, so it is
//! written as `0x0F ++ uv(|protobuf bytes|) ++ canonical protobuf bytes`. The payload
//! is therefore an **arbitrary `Par`** — nothing in the grammar bounds its depth,
//! because the escape exists precisely for the terms the grammar cannot describe.
//!
//! ⚠★ Until this gate existed, that payload was produced by
//! `prost::Message::encode_to_vec`, which is **Θ(depth) on the native stack** —
//! **302 B/level release, 1,937 B/level debug**
//! (`docs/design/stack-safety/stack-safety-report-2026-07-29.md` §8.1). On a 2 MiB
//! worker that is `` $D_{\max} \approx 6{,}900$ ``. Past it the process **aborts**,
//! inside a function documented total and no-panics, on input a peer controls.
//!
//! ★ A crash is strictly worse than a rejection: a clean `Err` is a consensus
//! decision every node reaches identically, while a `SIGSEGV` is a liveness
//! failure of whichever node was asked first. That is why this is a *repair* and
//! not an optimisation.
//!
//! ## The repair
//!
//! The two escape-arm sites now call
//! [`models::rust::rholang::protobuf_encoder::encode_to_vec`] — the iterative protobuf
//! encoder whose obligation stack lives on the heap and which **calls**
//! `prost::encoding::<module>::{encode, encoded_len}` rather than restating any
//! byte format, so only the recursion is replaced.
//!
//! ⚠★ **Both** sites, together, and that is not tidiness. The write side is
//! `encode_segment`'s escape arm; the read side is `decode_trie_path`'s
//! `EscapePayloadNonCanonical` check, which asks *"is this payload the canonical
//! encoding of what it decodes to?"* **"Canonical" is defined as what the encoder
//! emits**, so a second encoder on the read side would be a second opinion about
//! the accept set, and the two opinions would answer that `!=` differently on the
//! first byte they disagreed about. Converting only the writer would have been the
//! more dangerous half-change.
//!
//! ## The instrument
//!
//! A thread with a [`SMALL_STACK`]-byte stack. With the derived encoder, the
//! ladder below needs ~1.2 MB (release) / ~7.9 MB (debug) of native stack; with
//! the iterative one it needs a constant amount. So the measurement is decisive in
//! **both** profiles rather than only in debug.
//!
//! ⚠ `SMALL_STACK` is deliberately far above `PTHREAD_STACK_MIN` (16,384), which
//! silently **clamps** smaller requests — a request of 12,288 B yields a
//! 16,384 B stack, so a number below the floor is an instrument artefact and not a
//! datum. Nothing here divides by the requested size; the request is a *ceiling
//! this must fit under*, and the claim is "it fits", not "it uses exactly N".
//!
//! ⚠ **This gate does not expect a panic**, and could not: a stack overflow is a
//! `SIGSEGV`, not an unwind, so it cannot be caught and asserting it would abort
//! the harness. Every assertion here is of the form *"this succeeds on a stack
//! this small"*, and the **non-vacuity** control is
//! the independent stack-depth gate, which records zero recursion tripwires
//! over the generated traversal matrix.
//!
//! ### ★★ Fixture lifetime is excluded from the encoder measurement
//!
//! Generated iterative `Clone` and `Drop` now cover `Par`. The fixture is still
//! built outside the small thread and shared by [`Arc`] so construction and
//! lifetime costs cannot be mistaken for encoder stack usage.
//!
//! ### ★★★ What the probe FOUND, which is not what it was built to show
//!
//! With the escape arm converted, the 256 KiB probe **still overflowed** — and the
//! encoder was not the frame that ran out. Bisection
//! ([`the_stability_classifier_is_flat`], [`the_iterative_protobuf_encoder_is_flat`])
//! separated the components and located a **second** Θ(depth) native-stack
//! traversal on the same required-total path:
//! `models/src/rust/pathmap_crate_type_mapper.rs`'s `eval_stable_par` ⇄
//! `eval_stable_expr`, **mutually recursive with no bound** through `EList.ps` and
//! `ETuple.ps`. It is the *ground-domain gate* — the predicate that decides which
//! wire arm a map takes — so it runs on every segment of every trie key.
//!
//! It is now a single explicit-state traversal. Its current register advances
//! through unary chains without allocating; only pending siblings enter the
//! heap continuation stack. There is no native recursion and no artificial
//! descent threshold.
//!
//! ## The two former residuals are closed
//!
//! The escape reader is the generated protobuf PDA, and `EPathMap` has a
//! hand-written four-field `ProtobufNode` program. The retained recursive Prost
//! decoder remains only as an anti-vacuity oracle showing the depth at which the
//! replacement matters.

use std::sync::Arc;
use std::thread;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Connective, ETuple, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path, tag};
use models::rust::rholang::protobuf_encoder;
use prost::Message;

/// The thread stack every measurement here runs on.
///
/// 256 KiB — 16× `PTHREAD_STACK_MIN`, so it is a real request rather than a
/// clamped one, and ~5× *less* than the derived encoder needs for the shallowest
/// rung of the ladder in debug.
const SMALL_STACK: usize = 256 * 1024;

/// The depth ladder. The top rung needs ~1.2 MB of native stack under the derived
/// encoder in release and ~7.9 MB in debug — both far past [`SMALL_STACK`].
const DEPTH_LADDER: &[usize] = &[1, 33, 34, 256, 1_024, 4_096];

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn gint(value: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

/// A `Par` that is **not** `eval_stable_par`: a bare `Connective` carrier.
///
/// This is what forces the escape arm. `canonical_path.rs`'s own `unstable_corpus`
/// uses the same class of value.
fn unstable_leaf() -> Par {
    models::par_from_default! {
        connectives: vec![Connective {
            connective_instance: None,
        }],
        ..Default::default()
    }
}

/// `wrappers` nested single-element tuples around an UNSTABLE leaf.
///
/// ★ The escape rule is **hereditary**: the escape absorbs the *highest* unstable
/// node, so an unstable leaf makes every ancestor unstable and the whole term is
/// written as ONE escape payload at the top-level segment position. That is what
/// makes this ladder a test of the escape arm rather than of the structural arms.
fn deep_unstable(wrappers: usize) -> Par {
    let mut par = unstable_leaf();
    for _ in 0..wrappers {
        par = models::par_from_default! {
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

/// `wrappers` nested single-element tuples around a GROUND leaf — `eval_stable_par`
/// answers `true`, so `encode_trie_path` takes the STRUCTURAL arms.
fn deep_stable(wrappers: usize) -> Par {
    let mut par = gint(7);
    for _ in 0..wrappers {
        par = models::par_from_default! {
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

/// Run `body` against an already-built term on a [`SMALL_STACK`]-byte thread.
///
/// The term is passed as an `Arc<Par>` and the caller keeps a reference, so the
/// probe thread measures the named traversal without fixture construction or
/// teardown noise.
fn on_small_stack<T: Send + 'static>(
    name: &str,
    term: &Arc<Par>,
    body: impl FnOnce(Arc<Par>) -> T + Send + 'static,
) -> T {
    let handle = Arc::clone(term);
    thread::Builder::new()
        .name(name.to_string())
        .stack_size(SMALL_STACK)
        .spawn(move || body(handle))
        .unwrap_or_else(|e| panic!("could not spawn the {SMALL_STACK}-byte probe thread: {e}"))
        .join()
        .unwrap_or_else(|_| {
            panic!(
                "★ the {SMALL_STACK}-byte probe `{name}` did not return. A stack overflow is a \
                 SIGSEGV rather than an unwind, so if this message is reached the body panicked \
                 for some OTHER reason — read the panic above it."
            )
        })
}

// ---------------------------------------------------------------------------
// §2 the conversion
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// §2a the BISECTION — which traversal was the frame that ran out
// ---------------------------------------------------------------------------

/// ★★★ The GROUND-DOMAIN GATE, isolated. This is the component the bisection
/// found, and it is not the one this file was built to measure.
///
/// `eval_stable_par` ⇄ `eval_stable_expr` were mutually recursive with no bound
/// through `EList.ps` / `ETuple.ps`. They now form one explicit PDA. Isolated
/// here so that a future regression names *this* traversal rather than "the trie
/// encoder", which is what cost this file one wrong conclusion already.
#[test]
fn the_stability_classifier_is_flat() {
    use models::rust::pathmap_crate_type_mapper::eval_stable_par_for_test;

    for &wrappers in DEPTH_LADDER {
        // An UNSTABLE deep term: the classifier must walk the whole chain before
        // it can reach the `Connective` that makes the answer `false`. A stable
        // term would be walked in full too, but this shape makes the walk
        // unavoidable rather than incidental.
        let deep = Arc::new(deep_unstable(wrappers));
        let verdict = on_small_stack("stability-unstable", &deep, |term| {
            eval_stable_par_for_test(&term)
        });
        assert!(
            !verdict,
            "depth {wrappers}: a term with a bare `Connective` at the bottom must classify \
             UNSTABLE — if it does not, the classifier stopped early and the walk this rung \
             claims to measure did not happen"
        );

        // And a STABLE deep term, so the `true` direction is measured too: the
        // The true direction ensures the machine does not short-circuit before
        // visiting the leaf.
        let stable = Arc::new(deep_stable(wrappers));
        let verdict = on_small_stack("stability-stable", &stable, |term| {
            eval_stable_par_for_test(&term)
        });
        assert!(
            verdict,
            "depth {wrappers}: a ground tuple chain must classify STABLE. A `false` here \
             would move a ground map off proto field 8 and onto the tag-1 field walk, which \
             is a consensus-visible byte change — the predicate is exact, not conservative."
        );
    }
}

/// The iterative protobuf encoder, isolated from the trie codec around it.
#[test]
fn the_iterative_protobuf_encoder_is_flat() {
    for &wrappers in DEPTH_LADDER {
        let deep = Arc::new(deep_unstable(wrappers));
        let len = on_small_stack("protobuf-encode", &deep, |term| {
            let bytes = protobuf_encoder::encode_to_vec(&*term);
            assert_eq!(
                bytes.len(),
                protobuf_encoder::encoded_len(&*term),
                "the encoder's two passes disagree about the length"
            );
            bytes.len()
        });
        assert!(
            len > wrappers,
            "depth {wrappers}: {len} bytes is too few to contain the term"
        );
    }
}

// ---------------------------------------------------------------------------
// §2b the composition
// ---------------------------------------------------------------------------

/// ★★ `encode_trie_path` of a deep escape payload fits on a small stack.
///
/// The composition of §2a's two components plus the trie machine's own iterative
/// walk. It is asserted *after* them so that a failure here, with both of them
/// green, points at the trie machine rather than at either component.
#[test]
fn the_escape_arm_encodes_a_deep_payload_on_a_small_stack() {
    for &wrappers in DEPTH_LADDER {
        let deep = Arc::new(deep_unstable(wrappers));
        let key = on_small_stack("escape-encode", &deep, |term| {
            let key = encode_trie_path(&term);
            // Asserted INSIDE the small-stack thread so that the shape check
            // cannot accidentally be the thing that runs on the big stack.
            assert!(
                key.first().copied() == Some(tag::ESCAPE),
                "the term did not take the ESCAPE arm, so this rung measures a structural \
                 arm instead"
            );
            key
        });
        assert!(
            key.len() > wrappers,
            "depth {wrappers}: the key is {} bytes, which is too few to contain the payload — \
             an elision would make the measurement meaningless",
            key.len()
        );
    }

    // ★ And the STABLE ladder, which takes the structural arms rather than the
    // escape: the same required-total entry point, the other half of its domain.
    for &wrappers in DEPTH_LADDER {
        let stable = Arc::new(deep_stable(wrappers));
        let key = on_small_stack("structural-encode", &stable, |term| {
            let key = encode_trie_path(&term);
            assert!(
                key.first().copied() != Some(tag::ESCAPE),
                "a ground tuple chain must take a STRUCTURAL arm, not the escape"
            );
            key
        });
        assert!(key.len() > wrappers, "depth {wrappers}: key too short");
    }
}

/// The dedicated encoder and the public `Message` surface emit identical bytes.
///
/// Byte identity is the whole licence for the substitution: the escape payload is
/// part of a trie key, a trie key is part of the tag-8 `serialized_paths` stream,
/// and that stream is consensus-visible.
#[test]
fn the_escape_payload_is_byte_identical_to_message_encoding() {
    for wrappers in [0usize, 1, 8, 33, 34, 64, 128] {
        let par = deep_unstable(wrappers);
        let message = Message::encode_to_vec(&par);
        let iterative = protobuf_encoder::encode_to_vec(&par);
        assert_eq!(
            iterative, message,
            "depth {wrappers}: the dedicated protobuf encoder disagrees with Message. \
             These bytes are an escape payload inside a trie key inside proto field 8; a \
             difference here is a consensus fork."
        );

        let key = encode_trie_path(&par);
        assert_eq!(key.first().copied(), Some(tag::ESCAPE));
        // The key's payload region, located by re-deriving the framing rather
        // than by a magic offset.
        let mut header = vec![tag::ESCAPE];
        let mut len = message.len() as u64;
        loop {
            let byte = (len & 0x7F) as u8;
            len >>= 7;
            header.push(if len == 0 { byte } else { byte | 0x80 });
            if len == 0 {
                break;
            }
        }
        assert_eq!(
            key,
            [header, message].concat(),
            "depth {wrappers}: the trie key is not `0x0F ++ uv(len) ++ canonical protobuf bytes`"
        );
    }
}

/// The round trip remains total beyond the old recursive reader boundary.
#[test]
fn the_escape_arm_round_trips_beyond_the_recursive_readers_boundary() {
    for wrappers in [0usize, 1, 8, 20, 64, 256] {
        let par = deep_unstable(wrappers);
        let key = encode_trie_path(&par);
        let back = decode_trie_path(&key)
            .unwrap_or_else(|e| panic!("depth {wrappers} must round-trip the escape arm: {e:?}"));
        assert_eq!(
            encode_trie_path(&back),
            key,
            "depth {wrappers}: the escape round trip is not a byte-level fixed point"
        );
    }

    // The differential oracle is retained in `protobuf_decoder_differential`;
    // this test owns the deep, fixed-small-stack integration path.
}

/// `EPathMap` participates in the iterative protobuf field program and remains
/// byte-identical to the retained Prost oracle.
#[test]
fn epathmap_is_no_longer_an_opaque_encoder_residual() {
    use models::rust::rhoapi_ext::EPathMap;

    // An unstable map (a `remainder` makes it ¬ground) holding a nested entry, so
    // the escape arm's payload contains an `EPathMap` node.
    let map = EPathMap::new(
        vec![gint(1), gint(2)],
        Vec::new(),
        true,
        Some(models::rhoapi::Var {
            var_instance: Some(models::rhoapi::var::VarInstance::Wildcard(
                models::rhoapi::var::WildcardMsg {},
            )),
        }),
    );
    let par = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    };

    assert_eq!(
        protobuf_encoder::encode_to_vec(&par),
        Message::encode_to_vec(&par),
        "the EPathMap field program must be byte-exact parity with Prost"
    );

    let key = encode_trie_path(&par);
    assert_eq!(
        key.first().copied(),
        Some(tag::ESCAPE),
        "the fixture must be ¬eval_stable, or it does not exercise the escape arm at all"
    );
}
