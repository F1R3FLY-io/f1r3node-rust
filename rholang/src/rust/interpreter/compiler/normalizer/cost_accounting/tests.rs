use models::rhoapi::{CostTokenStack, New, Par, Receive};

use super::ir::Sig;
use super::sig::canon_ground;
use crate::rust::interpreter::accounting::Sig as NativeSig;
use crate::rust::interpreter::compiler::compiler::Compiler;

#[test]
fn signed_send_preserves_authority_without_extra_comm() {
    let signed = Compiler::source_to_adt(r#"{% @"r"!(42) %}[ s ]"#).expect("signed send compiles");
    let plain = Compiler::source_to_adt(r#"@"r"!(42)"#).expect("plain send compiles");
    assert_ne!(signed, plain, "the signed region must survive compilation");
    assert_eq!(signed.decode_occurrences::<models::rhoapi::Send>().len(), 1);
}

#[test]
fn signed_for_preserves_authority_without_extra_comm() {
    let signed = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ Nil } %}[ s ]"#)
        .expect("signed for compiles");
    let plain = Compiler::source_to_adt(r#"for(x <- @"ch"){ Nil }"#).expect("plain for compiles");
    assert_ne!(
        signed, plain,
        "the receive authority must survive compilation"
    );
    assert_eq!(signed.decode_occurrences::<Receive>().len(), 1);
}

#[test]
fn lollipop_for_preserves_handoff_without_extra_comm() {
    let signed = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ @"out"!(42) } %}[ a -o b ]"#)
        .expect("lollipop for compiles");
    let uniform_a = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ @"out"!(42) } %}[ a ]"#)
        .expect("uniform a compiles");
    let uniform_b = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ @"out"!(42) } %}[ b ]"#)
        .expect("uniform b compiles");
    assert_ne!(signed, uniform_a, "the continuation must be re-signed by b");
    assert_ne!(signed, uniform_b, "the rendezvous must remain funded by a");
    assert_eq!(signed.decode_occurrences::<Receive>().len(), 1);
}

#[test]
fn nested_region_overrides_outer_signer() {
    let inner_b =
        Compiler::source_to_adt(r#"{% {% @"r"!(42) %}[ b ] %}[ a ]"#).expect("nested b compiles");
    let inner_c =
        Compiler::source_to_adt(r#"{% {% @"r"!(42) %}[ c ] %}[ a ]"#).expect("nested c compiles");
    assert_ne!(
        inner_b, inner_c,
        "the inner region's signer must be retained"
    );
}

// --- (2) recognition coverage ---------------------------------------------

#[test]
fn recognizes_hash_and_compound_signed_terms() {
    Compiler::source_to_adt(r#"{% Nil %}[ # { @0!(1) } ]"#).expect("hash `#P` sig recognized");
    Compiler::source_to_adt(r#"{% Nil %}[ a (*) b ]"#).expect("compound `(*)` sig recognized");
}

#[test]
fn bare_token_stack_preserves_directive_without_minting() {
    let stack = Compiler::source_to_adt(r#"a :: ()"#).expect("token stack compiles");
    assert_ne!(
        stack,
        Par::default(),
        "the ordered stack must survive compilation"
    );
    assert!(stack
        .decode_occurrences::<models::rhoapi::Send>()
        .is_empty());
    assert!(stack.decode_occurrences::<Receive>().is_empty());
    assert_eq!(stack.decode_occurrences::<CostTokenStack>().len(), 1);
    assert_eq!(
        stack.decode_occurrences::<CostTokenStack>()[0].layers,
        vec![Sig::Ground(canon_ground("a")).to_cost_signature()],
    );
}

#[test]
fn token_stack_layers_keep_source_order() {
    let ab = Compiler::source_to_adt(r#"a :: b :: ()"#).expect("ordered stack compiles");
    let ba = Compiler::source_to_adt(r#"b :: a :: ()"#).expect("reversed stack compiles");
    assert_ne!(ab, ba);
    assert_eq!(ab.decode_occurrences::<CostTokenStack>()[0].layers, vec![
        Sig::Ground(canon_ground("a")).to_cost_signature(),
        Sig::Ground(canon_ground("b")).to_cost_signature(),
    ],);
}

// --- (3) content-addressed + ring-fenced bridge ---------------------------

#[test]
fn canon_ground_is_deterministic_and_spelling_keyed() {
    assert_eq!(
        canon_ground("s"),
        canon_ground("s"),
        "deterministic per spelling"
    );
    assert_ne!(
        canon_ground("s"),
        canon_ground("t"),
        "distinct spellings ⇒ distinct keys"
    );
}

#[test]
fn to_native_maps_atoms_by_content() {
    assert_eq!(
        Sig::Ground(b"g".to_vec()).to_native().unwrap(),
        NativeSig::Ground(b"g".to_vec())
    );
    assert_eq!(
        Sig::Quote(b"p".to_vec()).to_native().unwrap(),
        NativeSig::Quote(b"p".to_vec())
    );
}

#[test]
fn bound_signer_requires_runtime_substitution() {
    assert!(Sig::Bound(0).to_native().is_err());
    assert_eq!(Sig::Bound(0).to_cost_signature().atoms.len(), 1);
}

#[test]
fn bound_signer_wire_is_distinct_from_free_ground() {
    assert_ne!(
        Sig::Bound(0).to_cost_signature(),
        Sig::Ground(canon_ground("s")).to_cost_signature()
    );
}

#[test]
fn to_native_compound_is_the_and_fold_of_its_atoms() {
    let a = Sig::Ground(b"a".to_vec());
    let b = Sig::Ground(b"b".to_vec());
    let compound = Sig::compound(vec![a, b]);
    let native = compound.to_native().unwrap();
    match &native {
        NativeSig::And(left, right) => {
            let atoms = [left.as_ref(), right.as_ref()];
            assert!(
                atoms
                    .iter()
                    .any(|atom| *atom == &NativeSig::Ground(b"a".to_vec())),
                "And-fold retains atom a"
            );
            assert!(
                atoms
                    .iter()
                    .any(|atom| *atom == &NativeSig::Ground(b"b".to_vec())),
                "And-fold retains atom b"
            );
        }
        other => panic!(
            "expected an And-fold for a 2-atom compound, got {:?}",
            other
        ),
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
use rholang_parser::{RholangParser, SourcePos};

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
        validated::Validated::Good(procs) => procs
            .into_iter()
            .next()
            .expect("exactly one top-level proc"),
        validated::Validated::Fail(_) => panic!("parse failed: {source}"),
    };
    let sig = match parsed.proc {
        Proc::SignedTerm { sig, .. } => sig,
        _ => panic!("expected a top-level signed term: {source}"),
    };
    signature_to_native_sig(sig, &bmc, &env, &parser).expect("signature resolves")
}

#[test]
fn free_ground_channel_is_spelling_keyed_and_bound_requires_runtime() {
    let empty = BoundMapChain::<VarSort>::new();
    let bound1 =
        BoundMapChain::<VarSort>::new().put_pos(("g".to_string(), VarSort::NameSort, SourcePos {
            line: 1,
            col: 1,
        }));

    let free_a = ground_channel("g", &empty);
    let free_b = ground_channel("g", &empty);

    assert_eq!(
        free_a, free_b,
        "two FREE `g` ⇒ the SAME channel (§9 rendezvous)"
    );
    let parser = RholangParser::new();
    let env = HashMap::new();
    let g = Signature::Ground(Name::NameVar(Var::Id(Id {
        name: "g",
        pos: SourcePos { line: 0, col: 0 },
    })));
    assert!(signature_to_native_sig(&g, &bound1, &env, &parser).is_err());
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
                user,
                envelope
            );
        }
    }
}

// --- (6) Phase 4: signed joins (per-clause signed binds) -------------------

#[test]
fn signed_join_preserves_clause_authority_at_natural_arity() {
    let signed =
        Compiler::source_to_adt(r#"new x, w in { for( {% y <- x %}[ s ] & @z <- w ){ Nil } }"#)
            .expect("signed-clause join compiles");
    let plain = Compiler::source_to_adt(r#"new x, w in { for( y <- x & @z <- w ){ Nil } }"#)
        .expect("plain join compiles");
    assert_ne!(signed, plain, "the signed clause must survive compilation");

    // The data join keeps its NATURAL arity — exactly the two data clauses, with
    // no fuel bind folded in.
    let news = signed.decode_occurrences::<New>();
    let body = news[0].p.as_ref().expect("new body");
    let receives = body.decode_occurrences::<Receive>();
    assert_eq!(receives.len(), 1, "one receive (the join)");
    assert_eq!(
        receives[0].binds.len(),
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
    let result = Compiler::source_to_adt(
        r#"new x, w in { for( {% y <- x %}[ a -o b ] & @z <- w ){ Nil } }"#,
    );
    let err = result.expect_err("a lollipop clause signature must be rejected");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("lollipop") || msg.contains("transfer"),
        "expected the lollipop-not-fundable clause-sig rejection, got: {}",
        msg
    );
}
