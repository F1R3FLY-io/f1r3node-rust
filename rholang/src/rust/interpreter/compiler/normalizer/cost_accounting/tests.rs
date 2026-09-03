use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostSignature, GPrivate, GUnforgeable, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;

use super::sig::canon_ground;
use crate::rust::interpreter::accounting::authority::cost_signature_to_sig;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::{RuntimeBudget, Sig, SignatureChannel};
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::metering::MeteredMachine;
use crate::rust::interpreter::substitute::{Substitute, SubstituteTrait};

fn assert_ground_signature(signature: &CostSignature, name: &str) {
    assert_eq!(
        signature.value,
        Some(CostSignatureValue::Ground(canon_ground(name)))
    );
}

#[test]
fn signed_send_preserves_the_wrapper_and_inner_process() {
    let signed = Compiler::source_to_adt(r#"{% @"r"!(42) %}[ s ]"#).expect("signed send compiles");
    let plain = Compiler::source_to_adt(r#"@"r"!(42)"#).expect("plain send compiles");
    assert!(signed.sends.is_empty());
    assert_eq!(signed.cost_signed_terms.len(), 1);
    let wrapped = &signed.cost_signed_terms[0];
    assert_eq!(wrapped.body.as_ref(), Some(&plain));
    assert_ground_signature(wrapped.signature.as_ref().unwrap(), "s");
}

#[test]
fn uniform_signing_preserves_the_outer_and_continuation_wrappers() {
    let signed = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ Nil } %}[ s ]"#)
        .expect("signed for compiles");
    assert_eq!(signed.cost_signed_terms.len(), 1);
    let outer = &signed.cost_signed_terms[0];
    assert_ground_signature(outer.signature.as_ref().unwrap(), "s");
    let receive = &outer.body.as_ref().unwrap().receives[0];
    let continuation = receive.body.as_ref().unwrap();
    assert_eq!(continuation.cost_signed_terms.len(), 1);
    assert_ground_signature(
        continuation.cost_signed_terms[0]
            .signature
            .as_ref()
            .unwrap(),
        "s",
    );
}

#[test]
fn lollipop_preserves_distinct_rendezvous_and_continuation_authorities() {
    let signed = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ Nil } %}[ a -o b ]"#)
        .expect("lollipop for compiles");
    let outer = &signed.cost_signed_terms[0];
    assert_ground_signature(outer.signature.as_ref().unwrap(), "a");
    let continuation = outer.body.as_ref().unwrap().receives[0]
        .body
        .as_ref()
        .unwrap();
    assert_ground_signature(
        continuation.cost_signed_terms[0]
            .signature
            .as_ref()
            .unwrap(),
        "b",
    );
}

#[test]
fn lollipop_chain_desugars_right_associatively_without_a_runtime_connective() {
    let signed = Compiler::source_to_adt(
        r#"{% for(_ <- @"x"){ for(_ <- @"y"){ for(_ <- @"z"){ Nil } } } %}[ a -o b -o c ]"#,
    )
    .expect("lollipop chain compiles");
    let outer = &signed.cost_signed_terms[0];
    assert_ground_signature(outer.signature.as_ref().unwrap(), "a");
    let middle = &outer.body.as_ref().unwrap().receives[0]
        .body
        .as_ref()
        .unwrap()
        .cost_signed_terms[0];
    assert_ground_signature(middle.signature.as_ref().unwrap(), "b");
    let inner = &middle.body.as_ref().unwrap().receives[0]
        .body
        .as_ref()
        .unwrap()
        .cost_signed_terms[0];
    assert_ground_signature(inner.signature.as_ref().unwrap(), "c");
}

#[test]
fn recognizes_hash_and_compound_signed_terms() {
    Compiler::source_to_adt(r#"{% Nil %}[ # { @0!(1) } ]"#).expect("hash `#P` sig recognized");
    Compiler::source_to_adt(r#"{% Nil %}[ a (*) b ]"#).expect("compound `(*)` sig recognized");
}

#[test]
fn nested_signatures_flatten_by_the_cost_monad_multiplication() {
    let nested = Compiler::source_to_adt(r#"{% {% @"x"!(0) %}[ b ] %}[ a ]"#).unwrap();
    assert_eq!(nested.cost_signed_terms.len(), 1);
    let outer = &nested.cost_signed_terms[0];
    assert!(outer.body.as_ref().unwrap().cost_signed_terms.is_empty());
    let signature = cost_signature_to_sig(outer.signature.as_ref().unwrap()).unwrap();
    match signature {
        Sig::And(left, right) => {
            let elements = [*left, *right];
            assert!(elements.contains(&Sig::Ground(canon_ground("a"))));
            assert!(elements.contains(&Sig::Ground(canon_ground("b"))));
        }
        other => panic!("expected flattened compound signature, got {other:?}"),
    }
}

#[test]
fn bare_token_stack_is_preserved_as_a_first_class_term() {
    let stack = Compiler::source_to_adt(r#"a :: ()"#).expect("token stack compiles");
    assert_eq!(stack.cost_stacks.len(), 1);
    assert_eq!(stack.cost_stacks[0].cells.len(), 1);
    assert_ground_signature(&stack.cost_stacks[0].cells[0], "a");
    assert!(stack.sends.is_empty());
    assert!(stack.receives.is_empty());
}

#[test]
fn signed_terms_and_token_stacks_preserve_their_send_payload_sorts() {
    let send = Compiler::source_to_adt(r#"@"ch"!( {% @"x"!(1) %}[ s ], t :: u :: () )"#)
        .expect("cost-accounted payloads compile");
    let data = &send.sends[0].data;
    assert_eq!(data.len(), 2);
    assert_eq!(data[0].cost_signed_terms.len(), 1);
    assert!(data[0].cost_stacks.is_empty());
    assert_ground_signature(
        data[0].cost_signed_terms[0].signature.as_ref().unwrap(),
        "s",
    );
    assert_eq!(data[1].cost_stacks.len(), 1);
    assert!(data[1].cost_signed_terms.is_empty());
    assert_eq!(data[1].cost_stacks[0].cells.len(), 2);
    assert_ground_signature(&data[1].cost_stacks[0].cells[0], "t");
    assert_ground_signature(&data[1].cost_stacks[0].cells[1], "u");
}

#[test]
fn structural_quote_preserves_signed_term_and_stack_provenance() {
    let quoted = Compiler::source_to_adt(r#"@{ {% @"x"!(1) %}[ s ] | t :: () }!(0)"#)
        .expect("quoted cost-accounted term compiles");
    let channel = quoted.sends[0].chan.as_ref().unwrap();
    assert_eq!(channel.cost_signed_terms.len(), 1);
    assert_eq!(channel.cost_stacks.len(), 1);
    assert_ground_signature(
        channel.cost_signed_terms[0].signature.as_ref().unwrap(),
        "s",
    );
    assert_ground_signature(&channel.cost_stacks[0].cells[0], "t");
}

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
fn wire_bridge_maps_ground_quote_and_compound_without_a_second_identity_scheme() {
    let ground = Compiler::source_to_adt(r#"{% Nil %}[ a ]"#).unwrap();
    let quote = Compiler::source_to_adt(r#"{% Nil %}[ # { @0!(1) } ]"#).unwrap();
    let compound = Compiler::source_to_adt(r#"{% Nil %}[ b (*) a ]"#).unwrap();

    assert_eq!(
        cost_signature_to_sig(ground.cost_signed_terms[0].signature.as_ref().unwrap()).unwrap(),
        Sig::Ground(canon_ground("a"))
    );
    assert!(matches!(
        cost_signature_to_sig(quote.cost_signed_terms[0].signature.as_ref().unwrap()).unwrap(),
        Sig::Quote(_)
    ));
    let resolved =
        cost_signature_to_sig(compound.cost_signed_terms[0].signature.as_ref().unwrap()).unwrap();
    match resolved {
        Sig::And(left, right) => {
            let atoms = [*left, *right];
            assert!(atoms.contains(&Sig::Ground(canon_ground("a"))));
            assert!(atoms.contains(&Sig::Ground(canon_ground("b"))));
        }
        other => panic!("expected compound signature, got {other:?}"),
    }
}

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

#[test]
fn bound_signature_resolves_to_the_actual_unforgeable_name() {
    let normalized = Compiler::source_to_adt(r#"new slot in { {% @"x"!(1) %}[ slot ] }"#).unwrap();
    let body = normalized.news[0].p.as_ref().unwrap().clone();
    assert!(matches!(
        body.cost_signed_terms[0].signature.as_ref().unwrap().value,
        Some(CostSignatureValue::BoundLevel(0))
    ));

    let name = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![7; 32] })),
    }]);
    let env = Env::new().put(name.clone());
    let substitute = Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    };
    let resolved = substitute.substitute_no_sort(body, 0, &env).unwrap();
    let signature = resolved.cost_signed_terms[0].signature.as_ref().unwrap();
    let canonical_name = ParSortMatcher::sort_match(&name).term;
    assert_eq!(
        signature.value,
        Some(CostSignatureValue::Name(canonical_name.clone()))
    );
    assert_eq!(
        cost_signature_to_sig(signature).unwrap(),
        Sig::Ground(canonical_name.encode_to_vec())
    );
}

#[test]
fn ground_and_quote_with_equal_canonical_bytes_share_a_channel() {
    let bytes = b"same-atom-content".to_vec();
    let ground = SignatureChannel::from_sig(&Sig::Ground(bytes.clone())).par;
    let quote = SignatureChannel::from_sig(&Sig::Quote(bytes)).par;
    assert_eq!(ground, quote);
}

#[test]
fn user_surface_sig_never_aliases_an_envelope_pool() {
    use crate::rust::interpreter::accounting::{envelope_sig_compound, envelope_sig_single};

    let envelopes = [
        envelope_sig_single(b"validator-ed25519-signature-bytes-0001"),
        envelope_sig_compound(&[b"cosigner-a-sig-bytes", b"cosigner-b-sig-bytes"]),
    ];

    let sources = [
        r#"{% Nil %}[ attacker_pool ]"#,
        r#"{% Nil %}[ # { @0!(1) } ]"#,
        r#"{% Nil %}[ a (*) b ]"#,
    ];
    let users = sources.map(|source| {
        let par = Compiler::source_to_adt(source).unwrap();
        cost_signature_to_sig(par.cost_signed_terms[0].signature.as_ref().unwrap()).unwrap()
    });

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

#[test]
fn signed_join_preserves_per_clause_authority_without_changing_data_arity() {
    let signed =
        Compiler::source_to_adt(r#"new x, w in { for( {% y <- x %}[ s ] & @z <- w ){ Nil } }"#)
            .expect("signed-clause join compiles");
    let plain = Compiler::source_to_adt(r#"new x, w in { for( y <- x & @z <- w ){ Nil } }"#)
        .expect("plain join compiles");
    let body = signed.news[0].p.as_ref().expect("new body");
    assert_eq!(body.receives.len(), 1, "one receive (the join)");
    assert_eq!(
        body.receives[0].binds.len(),
        2,
        "the data join has exactly its 2 natural clauses — no fuel bind"
    );
    let signed_binds = body.receives[0]
        .binds
        .iter()
        .filter(|bind| bind.cost_signature.is_some())
        .collect::<Vec<_>>();
    assert_eq!(signed_binds.len(), 1);
    assert_ground_signature(signed_binds[0].cost_signature.as_ref().unwrap(), "s");

    let mut erased = signed.clone();
    for bind in &mut erased.news[0].p.as_mut().unwrap().receives[0].binds {
        bind.cost_signature = None;
    }
    assert_eq!(erased, plain);
}

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
