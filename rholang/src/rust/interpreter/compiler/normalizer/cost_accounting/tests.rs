//! Tests for the cost-accounting transpiler: the signature algebra `Σ`, the
//! token chain `K`, the fuel gate `T` (de Bruijn structure), the uniform and
//! lollipop desugarings, ring-fencing via `new`-bound signatures, and
//! end-to-end determinism. Surface syntax is Greg's `app:concrete` (bare token
//! stacks `s :: ()`, compound `s1 (*) s2`, `# P`, `-o`). End-to-end cases go
//! through [`Compiler::source_to_adt`] (parse → normalize → sort), the
//! production chokepoint.

use std::collections::HashMap;

use models::create_bit_vector;
use models::rhoapi::Par;
use models::rust::utils::{new_boundvar_par, new_freevar_par};
use prost::Message;

use super::ir::{ResourceSignature, Sig};
use super::sig::{canon_ground, canon_quote, supply_channel};
use crate::rust::interpreter::compiler::compiler::Compiler;

fn ground(name: &str) -> Sig { Sig::Ground(canon_ground(name)) }

/// Σ⟦s⟧ for a free ground principal, the way the lowering computes it.
fn ground_channel(name: &str) -> Par { supply_channel(&ground(name)) }

// ─────────────────────────── Σ (signature algebra) ──────────────────────────

#[test]
fn sigma_ground_and_quote_axes_are_disjoint() {
    // Same canonical bytes on different axes must not collide (domain sep).
    let g = Sig::Ground(b"alice".to_vec());
    let q = Sig::Quote(b"alice".to_vec());
    assert_ne!(g.key(), q.key(), "ground/quote keys are domain-separated");
    assert_ne!(
        supply_channel(&g),
        supply_channel(&q),
        "ground/quote channels are domain-separated"
    );
}

#[test]
fn sigma_ground_and_bound_axes_are_disjoint() {
    // A free ground sig and a `new`-bound sig with the same canonical bytes must
    // never alias — that is the ring-fencing guarantee (domain separation).
    let g = Sig::Ground(b"vaultSig".to_vec());
    let b = Sig::Bound(b"vaultSig".to_vec());
    assert_ne!(g.key(), b.key(), "ground/bound keys are domain-separated");
    assert_ne!(
        supply_channel(&g),
        supply_channel(&b),
        "a free sig and a bound sig never share a channel"
    );
}

#[test]
fn sigma_compound_is_commutative_and_associative() {
    let (a, b, c) = (ground("a"), ground("b"), ground("c"));
    // Smart constructor: (a*b)*c == a*(b*c) == c*b*a.
    let left = Sig::compound(vec![Sig::compound(vec![a.clone(), b.clone()]), c.clone()]);
    let right = Sig::compound(vec![a.clone(), Sig::compound(vec![b.clone(), c.clone()])]);
    let reversed = Sig::compound(vec![c.clone(), b.clone(), a.clone()]);
    assert_eq!(left, right, "associative");
    assert_eq!(left, reversed, "commutative");
    assert_eq!(
        supply_channel(&left).encode_to_vec(),
        supply_channel(&reversed).encode_to_vec(),
        "Σ is permutation-invariant"
    );
    let manual_ab = Sig::Compound(vec![a.clone(), b.clone()]);
    let manual_ba = Sig::Compound(vec![b, a]);
    assert_eq!(
        supply_channel(&manual_ab).encode_to_vec(),
        supply_channel(&manual_ba).encode_to_vec(),
        "Σ⟦a (*) b⟧ = Σ⟦b (*) a⟧ byte-identical"
    );
}

#[test]
fn sigma_is_injective_on_distinct_signatures() {
    let a = ground_channel("a");
    let b = ground_channel("b");
    let ab = supply_channel(&Sig::compound(vec![ground("a"), ground("b")]));
    assert_ne!(a, b, "distinct grounds ⇒ distinct channels");
    assert_ne!(a, ab, "atom ≠ compound");
    assert_ne!(b, ab);
}

#[test]
fn key_of_atom_is_the_channel_unforgeable_id() {
    let g = ground("s");
    let channel = supply_channel(&g);
    assert_eq!(channel.unforgeables.len(), 1);
    let id = match channel.unforgeables[0].unf_instance.as_ref().unwrap() {
        models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(p) => p.id.clone(),
        other => panic!("expected GPrivate, got {other:?}"),
    };
    assert_eq!(id, g.key().to_vec(), "key(atom) == GPrivate id of Σ⟦atom⟧");
}

// ─────────────────────────── T (signed-term gate) ───────────────────────────

#[test]
fn atomic_signed_term_lowers_to_a_fuel_gate() {
    let par = Compiler::source_to_adt("{% Nil %}[ s ]").expect("normalizes");
    assert_eq!(par.receives.len(), 1, "exactly one fuel gate");
    assert_eq!(par.sends.len(), 0);
    let gate = &par.receives[0];
    assert_eq!(gate.bind_count, 1);
    assert!(gate.condition.is_none(), "no where-guard on a fuel gate");
    assert_eq!(gate.binds.len(), 1);
    assert_eq!(gate.binds[0].free_count, 1);
    assert_eq!(
        gate.binds[0].patterns,
        vec![new_freevar_par(0, Vec::new())],
        "single variable pattern t"
    );
    assert_eq!(
        gate.binds[0].source.as_ref(),
        Some(&ground_channel("s")),
        "gate funds on Σ⟦s⟧"
    );
    assert_eq!(
        gate.body.as_ref(),
        Some(&new_boundvar_par(0, create_bit_vector(&[0]), false)),
        "body re-releases the payload: *t = BoundVar(0)"
    );
}

#[test]
fn compound_signed_term_nests_one_gate_per_component() {
    let par = Compiler::source_to_adt("{% Nil %}[ a (*) b ]").expect("normalizes");
    assert_eq!(par.receives.len(), 1, "one outer gate");
    let outer = &par.receives[0];
    assert_eq!(outer.bind_count, 1);
    let outer_body = outer.body.as_ref().expect("outer body");
    assert_eq!(outer_body.receives.len(), 1, "one inner gate nested");
    let inner = &outer_body.receives[0];

    let mut got = vec![
        outer.binds[0].source.clone().unwrap().encode_to_vec(),
        inner.binds[0].source.clone().unwrap().encode_to_vec(),
    ];
    got.sort();
    let mut want = vec![
        ground_channel("a").encode_to_vec(),
        ground_channel("b").encode_to_vec(),
    ];
    want.sort();
    assert_eq!(got, want, "gates fund on Σ⟦a⟧ and Σ⟦b⟧");

    assert_eq!(outer_body.exprs.len(), 1, "outer gate releases *t0");
    assert_eq!(
        outer_body.exprs[0],
        new_boundvar_par(0, create_bit_vector(&[0]), false).exprs[0],
        "*t0 = BoundVar(0)"
    );
    let inner_body = inner.body.as_ref().unwrap();
    assert_eq!(inner_body.exprs.len(), 1, "inner gate releases *t1");
    assert_eq!(
        inner_body.exprs[0],
        new_boundvar_par(0, create_bit_vector(&[0]), false).exprs[0],
        "*t1 = BoundVar(0)"
    );
}

#[test]
fn signed_term_and_stack_rendezvous_on_one_channel() {
    let par = Compiler::source_to_adt("{% Nil %}[ s ] | s :: ()").expect("normalizes");
    assert_eq!(par.receives.len(), 1);
    assert_eq!(par.sends.len(), 1);
    assert_eq!(
        par.receives[0].binds[0].source, par.sends[0].chan,
        "the gate consumes the token on the shared Σ⟦s⟧"
    );
}

#[test]
fn hash_signature_funds_on_a_quote_channel() {
    let par = Compiler::source_to_adt("{% Nil %}[ # { @0!(1) } ]").expect("normalizes");
    assert_eq!(par.receives.len(), 1);
    let source = par.receives[0].binds[0].source.as_ref().unwrap();
    assert_eq!(source.unforgeables.len(), 1, "Σ⟦#P⟧ is a GPrivate channel");
    assert_ne!(
        Some(source),
        Some(&ground_channel("anything")),
        "quote channel ≠ any ground channel"
    );
}

// ─────────────────────────────── K (token stack) ────────────────────────────

#[test]
fn stack_lowers_to_a_token_send() {
    let par = Compiler::source_to_adt("s :: ()").expect("normalizes");
    assert_eq!(par.sends.len(), 1);
    assert_eq!(par.receives.len(), 0);
    let send = &par.sends[0];
    assert_eq!(
        send.chan.as_ref(),
        Some(&ground_channel("s")),
        "token on Σ⟦s⟧"
    );
    assert_eq!(send.data, vec![Par::default()], "payload K⟦()⟧ = Nil");
    assert!(!send.persistent);
}

#[test]
fn multi_layer_stack_is_a_right_folded_send_chain() {
    // a :: b :: () = Σ⟦a⟧!( Σ⟦b⟧!( Nil ) )
    let par = Compiler::source_to_adt("a :: b :: ()").expect("normalizes");
    assert_eq!(par.sends.len(), 1);
    let head = &par.sends[0];
    assert_eq!(
        head.chan.as_ref(),
        Some(&ground_channel("a")),
        "head on Σ⟦a⟧"
    );
    assert_eq!(head.data.len(), 1);
    let payload = &head.data[0];
    assert_eq!(payload.sends.len(), 1, "payload is the remaining stack");
    assert_eq!(
        payload.sends[0].chan.as_ref(),
        Some(&ground_channel("b")),
        "next layer on Σ⟦b⟧"
    );
    assert_eq!(payload.sends[0].data, vec![Par::default()], "tail () = Nil");
}

// ─────────────────────────── desugar (uniform / lollipop) ───────────────────

#[test]
fn uniform_signing_signs_the_continuation() {
    let par = Compiler::source_to_adt("{% for(x <- @Nil){ Nil } %}[ s ]").expect("normalizes");
    let sig_s = ground_channel("s");
    assert_eq!(par.receives.len(), 1, "outer fuel gate");
    let outer = &par.receives[0];
    assert_eq!(
        outer.binds[0].source.as_ref(),
        Some(&sig_s),
        "outer gate funds on Σ⟦s⟧"
    );

    let user_for = &outer.body.as_ref().unwrap().receives[0];
    assert_eq!(
        user_for.binds[0].source.as_ref(),
        Some(&Par::default()),
        "the user's for(x <- @Nil)"
    );
    let continuation_gate = &user_for.body.as_ref().unwrap().receives[0];
    assert_eq!(
        continuation_gate.binds[0].source.as_ref(),
        Some(&sig_s),
        "the continuation is metered by Σ⟦s⟧ too"
    );
}

#[test]
fn lollipop_funds_rendezvous_with_s1_and_continuation_with_s2() {
    let par = Compiler::source_to_adt("{% for(x <- @Nil){ Nil } %}[ a -o b ]").expect("normalizes");
    assert_eq!(par.receives.len(), 1);
    let outer = &par.receives[0];
    assert_eq!(
        outer.binds[0].source.as_ref(),
        Some(&ground_channel("a")),
        "rendezvous funded by s1 = a"
    );
    let user_for = &outer.body.as_ref().unwrap().receives[0];
    let continuation_gate = &user_for.body.as_ref().unwrap().receives[0];
    assert_eq!(
        continuation_gate.binds[0].source.as_ref(),
        Some(&ground_channel("b")),
        "continuation funded by s2 = b"
    );
}

#[test]
fn lollipop_without_a_comm_is_rejected() {
    assert!(
        Compiler::source_to_adt("{% Nil %}[ a -o b ]").is_err(),
        "a lollipop needs a comm (for) to transfer through"
    );
}

#[test]
fn wildcard_ground_signature_is_rejected() {
    assert!(
        Compiler::source_to_adt("{% Nil %}[ _ ]").is_err(),
        "a wildcard is not a fundable principal"
    );
}

// ─────────────── ring-fencing via a `new`-bound signature ────────────────────

#[test]
fn new_bound_signature_mints_on_a_ring_fenced_channel() {
    // `new s in { s :: () }`: the token's channel is keyed on the BINDER, not on
    // the free spelling, so it never aliases the global (free) Σ⟦s⟧.
    let par = Compiler::source_to_adt("new s in { s :: () }").expect("normalizes");
    assert_eq!(par.news.len(), 1);
    let body = par.news[0].p.as_ref().unwrap();
    assert_eq!(body.sends.len(), 1, "one ring-fenced token");
    assert_ne!(
        body.sends[0].chan.as_ref(),
        Some(&ground_channel("s")),
        "ring-fenced: not the global (free) Σ⟦s⟧"
    );
}

#[test]
fn same_binder_shares_a_channel_distinct_binders_are_disjoint() {
    // Two stacks under the SAME `new s` mint on the same channel (one binder).
    let same = Compiler::source_to_adt("new s in { s :: () | s :: () }").expect("normalizes");
    let body = same.news[0].p.as_ref().unwrap();
    assert_eq!(body.sends.len(), 2);
    assert_eq!(
        body.sends[0].chan, body.sends[1].chan,
        "same `new s` binder ⇒ same ring-fenced channel"
    );

    // A SHADOWING inner `new s` is a DISTINCT binder ⇒ a disjoint channel.
    let diff =
        Compiler::source_to_adt("new s in { s :: () | new s in { s :: () } }").expect("normalizes");
    let outer = diff.news[0].p.as_ref().unwrap();
    let s_outer = &outer.sends[0].chan;
    let s_inner = &outer.news[0].p.as_ref().unwrap().sends[0].chan;
    assert_ne!(
        s_outer, s_inner,
        "a shadowing `new s` is a distinct binder ⇒ disjoint ring-fenced channel"
    );
}

#[test]
fn a_free_signature_mints_on_the_global_channel() {
    // A free `s` (no enclosing binder) mints on the global content-addressed
    // Σ⟦s⟧ — the §9 shared-rendezvous channel.
    let par = Compiler::source_to_adt("s :: ()").expect("normalizes");
    assert_eq!(par.sends.len(), 1);
    assert_eq!(
        par.sends[0].chan.as_ref(),
        Some(&ground_channel("s")),
        "free sig ⇒ global Σ⟦s⟧"
    );
}

// ───────────────────────────── determinism / canon ──────────────────────────

#[test]
fn lowering_is_deterministic_across_renormalizations() {
    let src = "{% Nil %}[ a (*) b ] | a :: b :: ()";
    let first = Compiler::source_to_adt(src)
        .expect("normalizes")
        .encode_to_vec();
    for _ in 0..100 {
        assert_eq!(
            Compiler::source_to_adt(src)
                .expect("normalizes")
                .encode_to_vec(),
            first,
            "byte-identical across re-normalizations"
        );
    }
}

// ───────────────────────── pattern-position rejection ───────────────────────

#[test]
fn signed_term_in_a_match_pattern_is_rejected() {
    assert!(
        Compiler::source_to_adt("match 1 { {% Nil %}[ s ] => Nil }").is_err(),
        "a signed term cannot be a match pattern"
    );
}

#[test]
fn token_stack_in_a_receive_pattern_is_rejected() {
    assert!(
        Compiler::source_to_adt("for( @{ s :: () } <- @0 ){ Nil }").is_err(),
        "a token stack cannot be a receive-bind pattern"
    );
}

#[test]
fn cost_syntax_in_a_contract_formal_is_rejected() {
    assert!(
        Compiler::source_to_adt("contract @\"c\"( @{% Nil %}[ s ] ) = { Nil }").is_err(),
        "cost syntax cannot be a contract formal"
    );
}

// ───────────────────── joins + nested (recursion) coverage ───────────────────

#[test]
fn join_with_a_signed_continuation_meters_the_continuation() {
    let par =
        Compiler::source_to_adt("for( x <- @0 & y <- @1 ){ {% Nil %}[ s ] }").expect("normalizes");
    assert_eq!(par.receives.len(), 1, "the join is one receive…");
    let join = &par.receives[0];
    assert_eq!(join.binds.len(), 2, "…with two binds");
    let body = join.body.as_ref().unwrap();
    assert_eq!(body.receives.len(), 1, "continuation is a fuel gate");
    assert_eq!(
        body.receives[0].binds[0].source.as_ref(),
        Some(&ground_channel("s")),
        "continuation metered by Σ⟦s⟧"
    );
}

#[test]
fn signed_term_as_a_send_payload_is_lowered() {
    let par = Compiler::source_to_adt("@0!( {% Nil %}[ s ] )").expect("normalizes");
    assert_eq!(par.sends.len(), 1);
    let payload = &par.sends[0].data[0];
    assert_eq!(payload.receives.len(), 1, "payload signed term → fuel gate");
    assert_eq!(
        payload.receives[0].binds[0].source.as_ref(),
        Some(&ground_channel("s"))
    );
}

// ─────────────── combined-cell tokens + splitter (R3/R5) ─────────────────────

#[test]
fn combined_cell_stack_emits_a_persistent_splitter() {
    // a (*) b :: () = Σ⟦a (*) b⟧!(Nil) ∣ for(c <= Σ⟦a (*) b⟧){ Σ⟦a⟧!(Σ⟦b⟧!(*c)) }
    let par = Compiler::source_to_adt("a (*) b :: ()").expect("normalizes");
    let combined = supply_channel(&Sig::compound(vec![ground("a"), ground("b")]));

    assert!(
        par.sends
            .iter()
            .any(|s| s.chan.as_ref() == Some(&combined) && s.data == vec![Par::default()]),
        "combined token minted on Σ⟦a (*) b⟧"
    );

    let splitter = par
        .receives
        .iter()
        .find(|r| r.binds[0].source.as_ref() == Some(&combined))
        .expect("splitter present");
    assert!(splitter.persistent, "splitter is replicated");
    assert_eq!(splitter.bind_count, 1);

    let outer = &splitter.body.as_ref().unwrap().sends[0];
    let inner = &outer.data[0].sends[0];
    let mut got = vec![
        outer.chan.clone().unwrap().encode_to_vec(),
        inner.chan.clone().unwrap().encode_to_vec(),
    ];
    got.sort();
    let mut want = vec![
        ground_channel("a").encode_to_vec(),
        ground_channel("b").encode_to_vec(),
    ];
    want.sort();
    assert_eq!(got, want, "splitter re-emits on Σ⟦a⟧, Σ⟦b⟧");
}

#[test]
fn combined_token_funds_a_compound_gate_via_the_splitter() {
    let par = Compiler::source_to_adt("{% Nil %}[ a (*) b ] | a (*) b :: ()").expect("normalizes");
    let combined = supply_channel(&Sig::compound(vec![ground("a"), ground("b")]));
    let (cha, chb) = (ground_channel("a"), ground_channel("b"));

    assert!(
        par.sends.iter().any(|s| s.chan.as_ref() == Some(&combined)),
        "combined token"
    );
    assert!(
        par.receives
            .iter()
            .any(|r| r.persistent && r.binds[0].source.as_ref() == Some(&combined)),
        "splitter"
    );
    assert!(
        par.receives.iter().any(|r| !r.persistent
            && (r.binds[0].source.as_ref() == Some(&cha)
                || r.binds[0].source.as_ref() == Some(&chb))),
        "compound gate on a component channel"
    );
}

#[test]
fn combined_token_lowering_is_deterministic() {
    let src = "{% Nil %}[ a (*) b ] | a (*) b :: ()";
    let first = Compiler::source_to_adt(src)
        .expect("normalizes")
        .encode_to_vec();
    for _ in 0..50 {
        assert_eq!(
            Compiler::source_to_adt(src)
                .expect("normalizes")
                .encode_to_vec(),
            first
        );
    }
}

#[test]
fn canon_quote_is_deterministic() {
    let parser = rholang_parser::RholangParser::new();
    let env: HashMap<String, Par> = HashMap::new();
    let procs = match parser.parse("@0!(1)") {
        validated::Validated::Good(p) => p,
        validated::Validated::Fail(e) => panic!("parse failed: {e:?}"),
    };
    let proc = procs.into_iter().next().unwrap();
    let first = canon_quote(&proc, &env, &parser).expect("canon");
    let second = canon_quote(&proc, &env, &parser).expect("canon");
    assert_eq!(
        first, second,
        "canon_quote is a deterministic function of P"
    );
    assert_eq!(Sig::Quote(first).key().len(), 32);
}
