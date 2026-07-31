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
//! written as `0x0F ++ uv(|prost bytes|) ++ canonical prost bytes`. The payload
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
//! [`the_small_stack_is_actually_small`], which shows the same thread size
//! defeating an ordinary recursive walk.
//!
//! ### ★★ `Drop` is excluded from the probe, deliberately, and it is not a dodge
//!
//! `drop_in_place::<Par>` is **144 B/level release / 464 debug** and is a *named,
//! standing* residual: `docs/design/stack-safety/stack-safety-report-2026-07-29.md`
//! §8.2 records that the obvious repair — an iterative `impl Drop for Par` — is
//! **REFUTED by measurement**, so `Drop` stays on `par_children::dismantle_all` at
//! call sites and `par_drop` stays in `TRIPWIRE_DEPTH`.
//!
//! A probe that built its fixture inside the small thread would therefore be
//! measuring `Drop`, not the encoder, and would read red no matter how the encoder
//! was written. Every fixture here is built on the default stack, handed to the
//! probe as an [`Arc<Par>`] (whose clone and drop are refcount arithmetic), and
//! released on the default stack when the test returns. The probe measures exactly
//! the traversal it names.
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
//! It now carries a **descend budget** (`STABILITY_DESCEND_BUDGET = 64`) in the
//! shape `88ec2734` established for the derived `Clone`: native frames while the
//! budget lasts, suspension onto a heap worklist past it, and the budget spent
//! only at the one cut-set edge (`Par` inside an `EList`/`ETuple`). ★ The shallow
//! case allocates **nothing** — `Vec::new()` does not allocate until its first
//! push — so the hot encode path is unchanged.
//!
//! ## ⚠ The residual, named rather than left for a stack trace
//!
//! 1. **The escape arm's READER is still `prost::Message::decode`**, capped at
//!    term depth 33 by prost's private `RECURSION_LIMIT`. That is registered as
//!    #130 in `rholang/tests/par_read_ceiling_site_registry.rs` and is **not**
//!    closed here. ⇒ The escape arm's write/read asymmetry *widens*: the writer
//!    used to abort somewhere around depth 6,900 and now does not stop at all,
//!    while the reader still stops at 34. The standing owner ruling forbids
//!    closing that gap by capping the writer, so the remaining work is an
//!    unbounded protobuf reader — a separate deliverable.
//! 2. **An `EPathMap` nested inside the payload is an OPAQUE LEAF** to the
//!    iterative encoder (`prost_wire.rs` §D: its `encode_raw` has three arms of
//!    which only one is a field walk, and which one fires depends on a `OnceLock`
//!    another thread may fill). Bytes are identical; the native stack is **not**
//!    bounded through that one shape. [`the_epathmap_residual_is_real`] exhibits
//!    it rather than describing it.

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
    Par {
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
    Par {
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

/// `wrappers` nested single-element tuples around a GROUND leaf — `eval_stable_par`
/// answers `true`, so `encode_trie_path` takes the STRUCTURAL arms.
fn deep_stable(wrappers: usize) -> Par {
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

/// Run `body` against an already-built term on a [`SMALL_STACK`]-byte thread.
///
/// ⚠ The term is passed as an `Arc<Par>` and the caller keeps a reference, so the
/// probe thread's drop is a refcount decrement and the `Par` is released on the
/// default stack. See the header: measuring `Drop` here would measure a residual
/// whose repair is refuted, not the traversal under test.
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
// §1 non-vacuity — the instrument can fail
// ---------------------------------------------------------------------------

/// ⚠ The control. A gate whose instrument cannot fail measures nothing, and this
/// campaign has four recorded false zeros from probes that measured nothing at all
/// (`docs/design/stack-safety/stack-safety-report-2026-07-29.md` §4.6).
///
/// `<Par as prost::Message>::encoded_len` is the derived recursive walk. Here it
/// is called on a term whose depth needs more native stack than the probe thread
/// has, on a thread that is *allowed* to die — and the assertion is that the
/// thread does **not** come back. That is the instrument's calibration: the same
/// `SMALL_STACK` that the escape arm survives is one an ordinary recursive walk
/// over the same term does not.
///
/// ★ It is not "a test that expects a panic", and ⚠ **it is not an assertion
/// either** — that was measured. A stack overflow in a child thread is a
/// `SIGSEGV` that the runtime turns into `fatal runtime error: stack overflow,
/// aborting` + `SIGABRT`; the process dies and the `assert!` below is **never
/// reached**. Observed verbatim on 2026-07-30:
///
/// ```text
///   thread 'derived-encoded-len-control' (3737979) has overflowed its stack
///   fatal runtime error: stack overflow, aborting
///   … (signal: 6, SIGABRT: process abort signal)
/// ```
///
/// So the control's evidence is the **abort message**, produced by running this
/// test deliberately, and the `assert!` exists only to catch the *other*
/// direction: if the recursive walk ever fits, the test returns and fails loudly
/// rather than passing silently. That is why it is `#[ignore]`d — a control that
/// aborts the harness cannot share a process with the tests it calibrates.
#[test]
#[ignore = "the control kills its probe thread by design; run it deliberately with \
            `--ignored` — a SIGSEGV in a child thread aborts the whole harness on some \
            libc/profile combinations, which would take the other tests down with it"]
fn the_small_stack_is_actually_small() {
    let deep = Arc::new(deep_unstable(
        *DEPTH_LADDER.last().expect("the ladder is non-empty"),
    ));
    let handle = Arc::clone(&deep);
    let outcome = thread::Builder::new()
        .name("derived-encoded-len-control".to_string())
        .stack_size(SMALL_STACK)
        .spawn(move || Message::encoded_len(&*handle))
        .expect("spawn")
        .join();
    assert!(
        outcome.is_err(),
        "★ the CONTROL SURVIVED: `<Par as prost::Message>::encoded_len` completed on a \
         {SMALL_STACK}-byte stack at depth {}, returning {outcome:?}. Then this file's \
         {SMALL_STACK} is not small enough to distinguish the recursive walk from the \
         iterative one, and every 'it fits' assertion in §2 is vacuous. Raise the ladder or \
         lower the stack — but not below PTHREAD_STACK_MIN (16,384), which clamps.",
        DEPTH_LADDER.last().expect("non-empty")
    );
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
/// through `EList.ps` / `ETuple.ps`. They now carry a descend budget. Isolated
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
        // budget's suspension path is only exercised when the walk does not
        // short-circuit.
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
        let len = on_small_stack("prost-encode", &deep, |term| {
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

/// ★ And the payload bytes are the DERIVED encoder's bytes, at every rung the
/// derived encoder can still reach.
///
/// Byte identity is the whole licence for the substitution: the escape payload is
/// part of a trie key, a trie key is part of the tag-8 `serialized_paths` stream,
/// and that stream is consensus-visible. This runs on the **default** stack,
/// because the derived leg is the one that needs the room.
#[test]
fn the_escape_payload_is_byte_identical_to_the_derived_encoding() {
    // Every rung here is well inside the derived encoder's reach on a default
    // (8 MiB) test stack, so the comparison is a comparison and not a survival
    // test. The deep rungs are covered by the previous test, which has no oracle
    // *because there is none that can run*.
    for wrappers in [0usize, 1, 8, 33, 34, 64, 128] {
        let par = deep_unstable(wrappers);
        let derived = Message::encode_to_vec(&par);
        let iterative = protobuf_encoder::encode_to_vec(&par);
        assert_eq!(
            iterative, derived,
            "depth {wrappers}: the iterative prost encoder disagrees with the derived one. \
             These bytes are an escape payload inside a trie key inside proto field 8; a \
             difference here is a consensus fork."
        );

        let key = encode_trie_path(&par);
        assert_eq!(key.first().copied(), Some(tag::ESCAPE));
        // The key's payload region, located by re-deriving the framing rather
        // than by a magic offset.
        let mut header = vec![tag::ESCAPE];
        let mut len = derived.len() as u64;
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
            [header, derived].concat(),
            "depth {wrappers}: the trie key is not `0x0F ++ uv(len) ++ canonical prost bytes`"
        );
    }
}

/// The round trip, at every depth the READER can still reach — and the reader's
/// own boundary, stated so the residual is not mistaken for closed.
#[test]
fn the_escape_arm_round_trips_up_to_its_readers_boundary() {
    // The escape payload's reader is `prost::Message::decode`, whose private
    // `RECURSION_LIMIT` of 100 message levels stops at term depth 33. Below it,
    // the arm round-trips to the byte.
    for wrappers in [0usize, 1, 8, 20] {
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

    // ⚠ THE RESIDUAL, exhibited. The writer no longer stops; the reader still
    // does. The ruling forbids closing that by capping the writer, so what is
    // left is an unbounded protobuf reader — registered as #130.
    let past_reader = deep_unstable(64);
    let key = encode_trie_path(&past_reader);
    assert!(
        !key.is_empty(),
        "the writer must still produce a key past the reader's boundary — that IS the residual"
    );
    let refusal = decode_trie_path(&key)
        .expect_err("★ the escape arm's reader accepted depth 64. prost's RECURSION_LIMIT is \
                     NOT lifted by this change; if it now accepts this, #130's residual has \
                     moved and this file's claim needs re-measuring");
    assert!(
        format!("{refusal:?}").contains("EscapePayloadInvalid"),
        "the refusal came from somewhere other than the payload's prost decode: {refusal:?}"
    );
}

/// ⚠ The `EPathMap` residual, exhibited rather than described.
///
/// `prost_wire.rs` §D gives `EPathMap` **no** field program: its `encode_raw` has
/// three arms and which one fires depends on a shadow cell, so the iterative
/// encoder treats it as ONE opaque node and recurses *inside* it exactly as the
/// derived path does. The bytes are identical — which is what this asserts — and
/// the native stack is not bounded through that shape, which is why it is named
/// here instead of being discovered from a stack trace.
#[test]
fn the_epathmap_residual_is_real() {
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
    let par = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    };

    assert_eq!(
        protobuf_encoder::encode_to_vec(&par),
        Message::encode_to_vec(&par),
        "★ the opaque-leaf interception must be BYTE-EXACT parity with \
         `prost::encoding::message::encode` at that position; it is the one place the \
         iterative encoder hands a subtree back to the derived path"
    );

    let key = encode_trie_path(&par);
    assert_eq!(
        key.first().copied(),
        Some(tag::ESCAPE),
        "the fixture must be ¬eval_stable, or it does not exercise the escape arm at all"
    );
}
