//! GATE-1 tests for native cost-accounted surface recognition (W1 Phase 1).
//!
//! Four properties:
//!  1. **Double-metering avoidance** — a signed term `{% P %}[s]` normalizes to a
//!     `Par` *structurally identical* to its unsigned inner `P` (recognition emits
//!     no `for(t <- Σ⟦s⟧)` gate node; uniform-signing of a `for` continuation
//!     re-dissolves through recognition). Full `Par` equality is strictly stronger
//!     than a send/receive node-count comparison.
//!  2. **Recognition coverage** — every surface form (ground / `#P` / compound /
//!     lollipop signed terms + bare token stack) normalizes successfully.
//!  3. **Content-addressed + ring-fenced bridge** — `canon_*` is deterministic and
//!     key-separated, and the `ir::Sig → accounting::Sig` bridge maps atoms by
//!     content, folds `Bound` under `DOMAIN_BOUND` (the ring-fence, disjoint from a
//!     free ground atom by leading byte), and lowers a compound to the `And`-fold.
//!  4. **Pattern rejection** — cost syntax is rejected in match-case, receive-bind,
//!     and contract-formal pattern positions.

use models::rhoapi::Par;

use super::ir::{Sig, DOMAIN_BOUND};
use super::sig::{canon_bound, canon_ground};
use crate::rust::interpreter::accounting::Sig as NativeSig;
use crate::rust::interpreter::compiler::compiler::Compiler;

// --- (1) double-metering: signed term ≡ unsigned inner --------------------

#[test]
fn signed_send_normalizes_identically_to_unsigned_send() {
    let signed = Compiler::source_to_adt(r#"{% @"r"!(42) %}[ s ]"#).expect("signed send compiles");
    let plain = Compiler::source_to_adt(r#"@"r"!(42)"#).expect("plain send compiles");
    assert_eq!(
        signed, plain,
        "recognition must emit NO gate node — `{{% P %}}[s]` normalizes to exactly `P`"
    );
}

#[test]
fn signed_for_normalizes_identically_to_unsigned_for() {
    let signed = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ Nil } %}[ s ]"#)
        .expect("signed for compiles");
    let plain =
        Compiler::source_to_adt(r#"for(x <- @"ch"){ Nil }"#).expect("plain for compiles");
    assert_eq!(
        signed, plain,
        "uniform-signing of the continuation re-dissolves; no gate node is added"
    );
}

#[test]
fn lollipop_for_normalizes_identically_to_unsigned_for() {
    // `{ for(R){P} }_{a ⊸ b}` desugars to `{ for(R){ {P}_b } }_a`; both `a` and the
    // re-signed continuation are recognition-only, so the Par is still just the
    // bare `for`.
    let signed = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ Nil } %}[ a -o b ]"#)
        .expect("lollipop for compiles");
    let plain =
        Compiler::source_to_adt(r#"for(x <- @"ch"){ Nil }"#).expect("plain for compiles");
    assert_eq!(signed, plain, "a lollipop transfer adds no gate node either");
}

// --- (2) recognition coverage ---------------------------------------------

#[test]
fn recognizes_hash_and_compound_signed_terms() {
    Compiler::source_to_adt(r#"{% Nil %}[ # { @0!(1) } ]"#).expect("hash `#P` sig recognized");
    Compiler::source_to_adt(r#"{% Nil %}[ a (*) b ]"#).expect("compound `(*)` sig recognized");
}

#[test]
fn bare_token_stack_mints_nothing() {
    // A bare token stack `a :: ()` resolves each layer (validation) and lowers to
    // the EMPTY process — it mints nothing in the normalizer (DR-13: only the Rust
    // supply producer writes `Σ⟦s⟧`).
    let stack = Compiler::source_to_adt(r#"a :: ()"#).expect("token stack compiles");
    assert_eq!(
        stack,
        Par::default(),
        "a bare token stack emits no send/receive node and mints nothing"
    );
}

// --- (3) content-addressed + ring-fenced bridge ---------------------------

#[test]
fn canon_ground_is_deterministic_and_spelling_keyed() {
    assert_eq!(canon_ground("s"), canon_ground("s"), "deterministic per spelling");
    assert_ne!(canon_ground("s"), canon_ground("t"), "distinct spellings ⇒ distinct keys");
}

#[test]
fn canon_bound_is_span_keyed() {
    use rholang_parser::{SourcePos, SourceSpan};
    let span_a = SourceSpan {
        start: SourcePos { line: 1, col: 0 },
        end: SourcePos { line: 1, col: 1 },
    };
    let span_b = SourceSpan {
        start: SourcePos { line: 2, col: 0 },
        end: SourcePos { line: 2, col: 1 },
    };
    assert_eq!(canon_bound(&span_a), canon_bound(&span_a), "deterministic per span");
    assert_ne!(
        canon_bound(&span_a),
        canon_bound(&span_b),
        "distinct binders (spans) ⇒ distinct ring-fenced keys"
    );
}

#[test]
fn to_native_maps_atoms_by_content() {
    assert_eq!(
        Sig::Ground(b"g".to_vec()).to_native(),
        NativeSig::Ground(b"g".to_vec())
    );
    assert_eq!(
        Sig::Quote(b"p".to_vec()).to_native(),
        NativeSig::Quote(b"p".to_vec())
    );
}

#[test]
fn to_native_folds_bound_under_domain_bound() {
    let content = b"1:0-1:1".to_vec();
    let mut expected = DOMAIN_BOUND.to_vec();
    expected.extend_from_slice(&content);
    assert_eq!(
        Sig::Bound(content).to_native(),
        NativeSig::Ground(expected),
        "a ring-fenced bound sig folds into a DOMAIN_BOUND-prefixed native ground atom"
    );
}

#[test]
fn bound_never_aliases_a_free_ground_of_the_same_name() {
    // The ring-fence (MINOR-6): a `new`-bound sig's native atom and a free ground
    // atom of the same identifier are disjoint by LEADING BYTE — `DOMAIN_BOUND`
    // begins `0x66` ('f'), a `canon_ground` encoding begins `0x2a` (the `Par.exprs`
    // field-5 protobuf tag).
    assert_eq!(DOMAIN_BOUND[0], b'f', "DOMAIN_BOUND leading byte is 'f' (0x66)");
    assert_eq!(
        canon_ground("s")[0],
        0x2a,
        "canon_ground leading byte is the Par.exprs protobuf tag (0x2a)"
    );
    let free = Sig::Ground(canon_ground("s")).to_native();
    let bound = Sig::Bound(b"any-span".to_vec()).to_native();
    assert_ne!(free, bound, "a bound sig can never alias a free ground sig");
}

#[test]
fn to_native_compound_is_the_and_fold_of_its_atoms() {
    let a = Sig::Ground(b"a".to_vec());
    let b = Sig::Ground(b"b".to_vec());
    let compound = Sig::compound(vec![a, b]);
    match compound.to_native() {
        NativeSig::And(left, right) => {
            let atoms = [*left, *right];
            assert!(
                atoms.contains(&NativeSig::Ground(b"a".to_vec())),
                "And-fold retains atom a"
            );
            assert!(
                atoms.contains(&NativeSig::Ground(b"b".to_vec())),
                "And-fold retains atom b"
            );
        }
        other => panic!("expected an And-fold for a 2-atom compound, got {:?}", other),
    }
}

// --- (4) pattern rejection ------------------------------------------------

fn assert_rejected_in_pattern(source: &str) {
    let result = Compiler::source_to_adt(source);
    let err = result.expect_err("cost syntax in pattern position must be rejected");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("pattern position"),
        "expected the cost-syntax pattern-guard error, got: {}",
        msg
    );
}

#[test]
fn rejects_signed_term_in_match_case_pattern() {
    assert_rejected_in_pattern(r#"match Nil { {% Nil %}[ s ] => Nil _ => Nil }"#);
}

#[test]
fn rejects_signed_term_in_receive_bind_pattern() {
    assert_rejected_in_pattern(r#"for( @{ {% Nil %}[ s ] } <- @"c" ){ Nil }"#);
}

#[test]
fn rejects_signed_term_in_contract_formal_pattern() {
    assert_rejected_in_pattern(r#"contract @"f"( @{ {% Nil %}[ s ] } ) = { Nil }"#);
}

// --- (5) Phase 2: native channel reconciliation + ring-fence + no-alias ----

use std::collections::HashMap;

use rholang_parser::ast::{Id, Name, Signature, Var};
use rholang_parser::{RholangParser, SourcePos, SourceSpan};

use super::sig::{signature_to_channel, signature_to_native_sig};
use crate::rust::interpreter::accounting::SignatureChannel;
use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::normalize::VarSort;

/// Resolve a ground signature `g` to its native supply channel, internalizing
/// the parser so the surface `Signature` value's lifetime ties to the local arena
/// (a ground sig allocates nothing in the arena). Binding-sensitivity is driven
/// by the supplied `bmc`: an empty chain ⇒ a FREE (content-by-spelling) channel;
/// a chain binding the name ⇒ a ring-fenced channel keyed on the binder span.
fn ground_channel(name: &str, bmc: &BoundMapChain<VarSort>) -> Par {
    let parser = RholangParser::new();
    let env = HashMap::new();
    let g = Signature::Ground(Name::NameVar(Var::Id(Id {
        name,
        pos: SourcePos { line: 0, col: 0 },
    })));
    signature_to_channel(&g, bmc, &env, &parser).expect("ground sig resolves to a channel")
}

/// Resolve the signature of a TOP-LEVEL signed term `{% P %}[ s ]` through the
/// real parse → `signature_to_native_sig` path (exercises `#P` canon-quoting,
/// compounds, etc. — not just hand-built atoms).
fn resolve_top_signed_sig(source: &str) -> crate::rust::interpreter::accounting::Sig {
    use rholang_parser::ast::Proc;
    let parser = RholangParser::new();
    let env = HashMap::new();
    let bmc = BoundMapChain::<VarSort>::new();
    let parsed = match parser.parse(source) {
        validated::Validated::Good(procs) => {
            procs.into_iter().next().expect("exactly one top-level proc")
        }
        validated::Validated::Fail(_) => panic!("parse failed: {source}"),
    };
    let sig = match parsed.proc {
        Proc::SignedTerm { sig, .. } => sig,
        _ => panic!("expected a top-level signed term: {source}"),
    };
    signature_to_native_sig(sig, &bmc, &env, &parser).expect("signature resolves")
}

#[test]
fn ring_fence_free_vs_bound_and_two_bound() {
    // The §9 rendezvous + ring-fencing trio, through the binding-sensitive
    // resolver: a FREE `g` is content-by-spelling (one global channel); a
    // `new`-bound `g` is ring-fenced to its binder span; two distinct binders
    // never collide.
    let empty = BoundMapChain::<VarSort>::new();
    let bound1 = BoundMapChain::<VarSort>::new().put_pos((
        "g".to_string(),
        VarSort::NameSort,
        SourcePos { line: 1, col: 1 },
    ));
    let bound2 = BoundMapChain::<VarSort>::new().put_pos((
        "g".to_string(),
        VarSort::NameSort,
        SourcePos { line: 9, col: 9 },
    ));

    let free_a = ground_channel("g", &empty);
    let free_b = ground_channel("g", &empty);
    let bound_a = ground_channel("g", &bound1);
    let bound_b = ground_channel("g", &bound2);

    assert_eq!(free_a, free_b, "two FREE `g` ⇒ the SAME channel (§9 rendezvous)");
    assert_ne!(free_a, bound_a, "a `new`-bound `g` is ring-fenced ⇒ distinct from free `g`");
    assert_ne!(bound_a, bound_b, "two distinct `new`-binders ⇒ distinct ring-fenced channels");
}

#[test]
fn minor7_ground_and_quote_collapse_at_the_channel() {
    // DR-1: the ground/quote AXIS does not affect channel derivation — equal atom
    // bytes ⇒ equal channel (intentional divergence from the transpiler's
    // axis-disjoint `sigma_ground_and_quote_axes_are_disjoint`).
    let bytes = b"same-atom-content".to_vec();
    let ground = SignatureChannel::from_sig(&NativeSig::Ground(bytes.clone())).par;
    let quote = SignatureChannel::from_sig(&NativeSig::Quote(bytes)).par;
    assert_eq!(
        ground, quote,
        "ground and quote with equal bytes derive the SAME channel (DR-1)"
    );
}

#[test]
fn user_surface_sig_never_aliases_an_envelope_pool() {
    // §5 no-alias security audit: a user-supplied surface signature (ground / `#P`
    // / compound / ring-fenced bound) resolved through the W1 bridge can NEVER
    // alias a deploy-ENVELOPE supply pool — neither its `Σ⟦s⟧` channel nor its
    // `lane_hash`. This is the guarantee that an in-term ground sig cannot drain a
    // system/protocol pool keyed by the envelope signature.
    use crate::rust::interpreter::accounting::{envelope_sig_compound, envelope_sig_single};

    let envelopes = [
        envelope_sig_single(b"validator-ed25519-signature-bytes-0001"),
        envelope_sig_compound(&[b"cosigner-a-sig-bytes", b"cosigner-b-sig-bytes"]),
    ];

    let users = [
        NativeSig::Ground(canon_ground("attacker_pool")),
        Sig::Bound(canon_bound(&SourceSpan {
            start: SourcePos { line: 3, col: 0 },
            end: SourcePos { line: 3, col: 4 },
        }))
        .to_native(),
        resolve_top_signed_sig(r#"{% Nil %}[ # { @0!(1) } ]"#),
        resolve_top_signed_sig(r#"{% Nil %}[ a (*) b ]"#),
    ];

    for user in &users {
        let user_chan = SignatureChannel::from_sig(user).par;
        for envelope in &envelopes {
            let env_chan = SignatureChannel::from_sig(envelope).par;
            assert_ne!(
                user_chan, env_chan,
                "a user surface sig ALIASED an envelope pool CHANNEL ({:?} vs {:?})",
                user, envelope
            );
            assert_ne!(
                user.lane_hash(),
                envelope.lane_hash(),
                "a user surface sig ALIASED an envelope LANE_HASH ({:?} vs {:?})",
                user, envelope
            );
        }
    }
}

// --- (6) Phase 4: signed joins (per-clause signed binds) -------------------

/// A `for` join carrying a per-clause signed bind `{% y <- x %}[s]` normalizes
/// IDENTICALLY to the unsigned join: the signed bind is demoted to its linear
/// bind and NO fuel bind is injected into the data join (Greg's rule — fuel is
/// never folded into a data join; the double-metering avoidance extended to
/// joins). The data join keeps its natural arity.
#[test]
fn signed_join_normalizes_identically_to_unsigned_join() {
    let signed =
        Compiler::source_to_adt(r#"new x, w in { for( {% y <- x %}[ s ] & @z <- w ){ Nil } }"#)
            .expect("signed-clause join compiles");
    let plain = Compiler::source_to_adt(r#"new x, w in { for( y <- x & @z <- w ){ Nil } }"#)
        .expect("plain join compiles");
    assert_eq!(
        signed, plain,
        "a signed-clause join normalizes to exactly the unsigned join (no fuel bind injected)"
    );

    // The data join keeps its NATURAL arity — exactly the two data clauses, with
    // no fuel bind folded in.
    let body = signed.news[0].p.as_ref().expect("new body");
    assert_eq!(body.receives.len(), 1, "one receive (the join)");
    assert_eq!(
        body.receives[0].binds.len(),
        2,
        "the data join has exactly its 2 natural clauses — no fuel bind"
    );
}

/// A per-clause signed bind validates its signature: a bare lollipop `a -o b`
/// (a TRANSFER capability, not a fundable atom `g | #P | s∘s`) is rejected in
/// clause funding position — only the OUTER signed-term sig may carry a lollipop
/// (which `recognize_signed_term` desugars through the `for`).
#[test]
fn signed_join_rejects_a_lollipop_clause_signature() {
    let result =
        Compiler::source_to_adt(r#"new x, w in { for( {% y <- x %}[ a -o b ] & @z <- w ){ Nil } }"#);
    let err = result.expect_err("a lollipop clause signature must be rejected");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("lollipop") || msg.contains("transfer"),
        "expected the lollipop-not-fundable clause-sig rejection, got: {}",
        msg
    );
}
