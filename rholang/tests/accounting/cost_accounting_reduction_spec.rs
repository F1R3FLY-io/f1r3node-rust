//! END-TO-END REDUCTION tests for cost-accounted Rholang (Greg `app:concrete`
//! syntax: bare token stacks `s :: ()`, compound `s1 (*) s2`, `# P`, `-o`).
//!
//! These complement the structural tests in
//! `compiler/normalizer/cost_accounting/tests.rs` (which assert only the shape
//! of the lowered `Par`) by running the FULL parser → normalizer → reducer
//! pipeline (`RhoRuntime::evaluate_with_term`) and asserting the cost semantics
//! actually EXECUTE: a fuel gate consumes its token and runs the continuation;
//! an unfunded gate blocks (metering); the splitter fires for a combined-cell
//! token; a `new`-bound signature ring-fences its fuel. Each continuation is an
//! observable `@"r"!(v)` send read back with `get_data`.

use models::rhoapi::{ListParWithRandom, Par};
use models::rust::utils::{new_gint_par, new_gstring_par};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _replay, _history) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

fn gint(n: i64) -> Par { new_gint_par(n, Vec::new(), false) }

fn channel(name: &str) -> Par { new_gstring_par(name.to_string(), Vec::new(), false) }

/// The flattened data resting at `@"name"` after reduction.
async fn data_at(runtime: &RhoRuntimeImpl, name: &str) -> Vec<Par> {
    let rows: Vec<Datum<ListParWithRandom>> = runtime.get_data(&channel(name)).await;
    rows.into_iter().flat_map(|d| d.a.pars).collect()
}

/// Reduce `source` to completion (asserting no normalize/reduce errors).
async fn run(source: &str) -> RhoRuntimeImpl {
    let mut runtime = fresh_runtime().await;
    let result = runtime
        .evaluate_with_term(source)
        .await
        .unwrap_or_else(|e| panic!("evaluate errored for {source:?}: {e:?}"));
    assert!(
        result.errors.is_empty(),
        "unexpected evaluation errors for {source:?}: {:?}",
        result.errors
    );
    runtime
}

// ───────────────────────────── R1 (atomic gate) ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r1_funded_gate_consumes_token_and_runs_continuation() {
    let rt = run(r#"{% @"r"!(42) %}[ s ] | s :: ()"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(42)],
        "the gate consumed the token, released the balance, and ran @\"r\"!(42)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r1_unfunded_gate_blocks_the_continuation() {
    // No token ⇒ the gate parks and the continuation never runs (metering).
    let rt = run(r#"{% @"r"!(42) %}[ s ]"#).await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "no fuel ⇒ continuation must not run"
    );
}

// ──────────────────────── R2 / R4 (compound / separate) ─────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_compound_gate_funded_by_split_tokens_runs() {
    // R2: a compound `{P}_{a (*) b}` is funded by SEPARATE token stacks — one per
    // component — `a :: () | b :: ()` (parallel free tokens on Σa, Σb).
    let rt = run(r#"{% @"r"!(1) %}[ a (*) b ] | a :: () | b :: ()"#).await;
    assert_eq!(data_at(&rt, "r").await, vec![gint(1)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_three_way_compound_gate_funded_by_separate_tokens() {
    let rt = run(r#"{% @"r"!(1) %}[ a (*) b (*) c ] | a :: () | b :: () | c :: ()"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(1)],
        "3 nested gates reduce"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_three_way_combined_token_via_splitter() {
    // n=3 combined-cell: the splitter chains the components in key-sorted order.
    let rt = run(r#"{% @"r"!(1) %}[ a (*) b (*) c ] | a (*) b (*) c :: ()"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(1)],
        "splitter chains n=3"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r4_separate_wraps_each_run_when_funded() {
    let rt = run(r#"{% @"r1"!(1) %}[ a ] | {% @"r2"!(2) %}[ b ] | a :: () | b :: ()"#).await;
    assert_eq!(data_at(&rt, "r1").await, vec![gint(1)]);
    assert_eq!(data_at(&rt, "r2").await, vec![gint(2)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_layer_stack_threads_through_gates() {
    // a :: b :: () = Σ⟦a⟧!(Σ⟦b⟧!(Nil)). The a-gate consumes the a-token and `*t`
    // releases the b-token, which the b-gate then consumes — one chained stack
    // funds two gates in sequence.
    let rt = run(r#"{% @"r1"!(1) %}[ a ] | {% @"r2"!(2) %}[ b ] | a :: b :: ()"#).await;
    assert_eq!(data_at(&rt, "r1").await, vec![gint(1)]);
    assert_eq!(data_at(&rt, "r2").await, vec![gint(2)]);
}

// ──────────────────── R3 (combined-cell token + splitter) ────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_combined_token_funds_compound_gate_via_splitter() {
    // The stack mints ONE combined token on Σ⟦a (*) b⟧; the persistent splitter
    // converts it to the chained split form the nested compound gate threads.
    let rt = run(r#"{% @"r"!(1) %}[ a (*) b ] | a (*) b :: ()"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(1)],
        "splitter bridged the combined token to the component gates"
    );
}

// ─────────────────────────── uniform signing sugar ──────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uniform_signing_meters_the_continuation() {
    // {for(x<-ch){@"r"!(*x)}}_s needs TWO s-tokens (the comm + the re-signed
    // continuation).
    let rt =
        run(r#"{% for(x <- @"ch"){ @"r"!(*x) } %}[ s ] | s :: () | s :: () | @"ch"!(7)"#).await;
    assert_eq!(data_at(&rt, "r").await, vec![gint(7)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uniform_signing_continuation_blocks_without_second_token() {
    let rt = run(r#"{% for(x <- @"ch"){ @"r"!(*x) } %}[ s ] | s :: () | @"ch"!(7)"#).await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "continuation is unfunded"
    );
}

// ─────────────────────────────── lollipop sugar ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lollipop_funds_rendezvous_with_s1_and_continuation_with_s2() {
    let rt = run(r#"{% for(x <- @"ch"){ @"r"!(*x) } %}[ a -o b ] | a :: () | b :: () | @"ch"!(7)"#)
        .await;
    assert_eq!(data_at(&rt, "r").await, vec![gint(7)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lollipop_continuation_blocks_without_the_transfer_target_token() {
    let rt = run(r#"{% for(x <- @"ch"){ @"r"!(*x) } %}[ a -o b ] | a :: () | @"ch"!(7)"#).await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "no s2 token ⇒ no continuation"
    );
}

// ─────────────── ring-fencing via a `new`-bound signature ────────────────────
// Greg's concrete syntax has no located purse; ring-fencing is realised by a
// `new`-bound signature (binding-sensitive Σ⟦s⟧: a bound sig is keyed on its
// binder, a free sig is content-addressed by spelling).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_bound_signature_funds_an_in_scope_gate() {
    // Token and gate share the `new s` binder ⇒ same Σ⟦s⟧ ⇒ the gate fires.
    let rt = run(r#"new s in { {% @"r"!(1) %}[ s ] | s :: () }"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(1)],
        "a new-bound signature funds a gate in the same scope"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_bound_fuel_is_ring_fenced_from_a_free_sig_gate() {
    // The token's sig is `new`-bound (Σ⟦bound s⟧); the gate outside the scope
    // uses the FREE sig `s` (Σ⟦free s⟧ ≠ Σ⟦bound s⟧) ⇒ it can never drain the
    // ring-fenced fuel.
    let rt = run(r#"(new s in { s :: () }) | {% @"r"!(1) %}[ s ]"#).await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "a free-sig gate must not consume new-bound (ring-fenced) fuel"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn free_shared_signature_still_rendezvous() {
    // §9: the SAME free signature shared by two parties is one global channel —
    // a gate and a token both on free `s` rendezvous (cross-principal funding).
    let rt = run(r#"{% @"r"!(1) %}[ s ] | s :: ()"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(1)],
        "free signatures rendezvous by spelling (the §9 shared-fee pattern)"
    );
}

// ─────────────── same-signature multi-token BUDGET (depth-k stack) ───────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_sig_depth2_budget_funds_two_distinct_gates() {
    // s :: s :: () is a depth-2 budget for ONE signature: two distinct same-sig
    // gates each fire once (the first releases the tail token for the second).
    let rt = run(r#"{% @"r1"!(1) %}[ s ] | {% @"r2"!(2) %}[ s ] | s :: s :: ()"#).await;
    assert_eq!(
        data_at(&rt, "r1").await,
        vec![gint(1)],
        "first same-sig gate fired"
    );
    assert_eq!(
        data_at(&rt, "r2").await,
        vec![gint(2)],
        "second same-sig gate fired off the re-released tail token"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_sig_depth2_budget_meters_out_the_third_identical_gate() {
    // Three IDENTICAL same-sig gates on a depth-2 budget ⇒ EXACTLY two fire.
    let rt =
        run(r#"{% @"r"!(9) %}[ s ] | {% @"r"!(9) %}[ s ] | {% @"r"!(9) %}[ s ] | s :: s :: ()"#)
            .await;
    let rs = data_at(&rt, "r").await;
    assert_eq!(
        rs.len(),
        2,
        "exactly 2 of 3 identical same-sig gates fire on a depth-2 budget"
    );
    assert!(
        rs.iter().all(|p| *p == gint(9)),
        "both fired gates emit the identical effect"
    );
}

// ─────────────── return channel forwarded THROUGH a fuel gate ────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fuel_gate_forwards_outer_new_bound_return_channel() {
    // A gated send forwards an OUTER new-bound `*ret` through the gate; the
    // de-Bruijn offset across the synthetic fuel binder must be correct.
    let rt = run(r#"
        new ret in {
          {% @"svc"!("ping", *ret) %}[ s ] | s :: ()
          | for (@msg, reply <- @"svc") { reply!(msg) }
          | for (@got <- ret) { @"r"!(got) }
        }
    "#)
    .await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![channel("ping")],
        "the gate-forwarded return channel round-trips (de-Bruijn offset correct)"
    );
}

// ───────────── per-clause signed binds `{% y<-x %}[s]` (Axis-C join) ─────────
// Greg `app:concrete` SignedBind: a bind clause whose rendezvous is funded
// INDEPENDENTLY by its own signature. Lowered (`lower_signed_join`) by stripping
// the clause to a plain linear bind and gating the recovered `for` with one fuel
// gate per clause atom — so the comm fires only once every clause's fuel is
// consumed (the join funded by the product `s_1 (*) … (*) s_k`). Unlike a signed
// TERM, the continuation is NOT re-signed: one token per clause, not two.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_bind_funded_rendezvous_fires_with_one_token() {
    // The source `@"ch"` has a sender AND the clause fuel `s :: ()` is present ⇒
    // the gated rendezvous fires. ONE token suffices (the bind meters the
    // rendezvous, not the continuation — contrast uniform signing's two).
    let rt = run(r#"for( {% x <- @"ch" %}[ s ] ){ @"r"!(*x) } | s :: () | @"ch"!(7)"#).await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(7)],
        "a funded signed bind consumes one token + the rendezvous and runs the body"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_bind_blocks_the_rendezvous_without_fuel() {
    // THE CRUX: the source `@"ch"` has a sender, but WITHOUT the clause fuel the
    // join must NOT fire — proving the bind genuinely meters the rendezvous
    // (a structural-only treatment would let it fire unfunded).
    let rt = run(r#"for( {% x <- @"ch" %}[ s ] ){ @"r"!(*x) } | @"ch"!(7)"#).await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "no clause fuel ⇒ the rendezvous is parked even though @\"ch\" has a sender"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_join_fires_when_every_clause_is_funded() {
    // An `&`-join of two signed clauses, each with its own fuel: the join fires
    // only once BOTH `a` and `b` tokens are consumed (funded by `a (*) b`).
    let rt = run(
        r#"for( {% x <- @"c1" %}[ a ] & {% y <- @"c2" %}[ b ] ){ @"r1"!(*x) | @"r2"!(*y) }
           | a :: () | b :: () | @"c1"!(7) | @"c2"!(8)"#,
    )
    .await;
    assert_eq!(
        data_at(&rt, "r1").await,
        vec![gint(7)],
        "clause 1 rendezvous ran"
    );
    assert_eq!(
        data_at(&rt, "r2").await,
        vec![gint(8)],
        "clause 2 rendezvous ran"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_join_blocks_when_one_clause_is_unfunded() {
    // Only `a`'s fuel is present; `b`'s is missing. The Axis-C join needs every
    // clause's fuel (the product `a (*) b`), so the WHOLE join parks — neither
    // rendezvous fires, even though both sources have senders.
    let rt = run(
        r#"for( {% x <- @"c1" %}[ a ] & {% y <- @"c2" %}[ b ] ){ @"r1"!(*x) | @"r2"!(*y) }
           | a :: () | @"c1"!(7) | @"c2"!(8)"#,
    )
    .await;
    assert!(
        data_at(&rt, "r1").await.is_empty() && data_at(&rt, "r2").await.is_empty(),
        "a missing clause fuel parks the entire join"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_and_plain_binds_mix_in_one_join() {
    // A signed clause `{% x<-@"c1" %}[s]` joined with a PLAIN clause `y<-@"c2"`:
    // only the signed clause needs fuel; the plain clause rendezvous freely.
    let rt = run(r#"for( {% x <- @"c1" %}[ s ] & y <- @"c2" ){ @"r"!(*x) }
           | s :: () | @"c1"!(7) | @"c2"!(8)"#)
    .await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(7)],
        "the signed clause is fuel-gated; the plain clause is not"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_and_plain_join_blocks_without_the_clause_fuel() {
    // Both sources have senders, but the signed clause is unfunded ⇒ the atomic
    // join parks (the plain clause cannot fire alone).
    let rt = run(r#"for( {% x <- @"c1" %}[ s ] & y <- @"c2" ){ @"r"!(*x) }
           | @"c1"!(7) | @"c2"!(8)"#)
    .await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "an unfunded signed clause parks the whole join, plain clause included"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_bound_clause_signature_funds_an_in_scope_signed_bind() {
    // Binding-sensitive Σ threads through per-clause binds too: a `new s` clause
    // sig and its in-scope token share Σ⟦bound s⟧ ⇒ the gated rendezvous fires.
    let rt = run(r#"new s in { for( {% x <- @"ch" %}[ s ] ){ @"r"!(*x) } | s :: () | @"ch"!(7) }"#)
        .await;
    assert_eq!(
        data_at(&rt, "r").await,
        vec![gint(7)],
        "a new-bound clause signature funds a signed bind in the same scope"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_bind_fuel_is_ring_fenced_by_a_new_bound_signature() {
    // The token's sig is `new`-bound (Σ⟦bound s⟧); the per-clause gate outside
    // the scope uses the FREE sig `s` ⇒ it can never drain the ring-fenced fuel,
    // so the rendezvous parks despite @"ch" having a sender.
    let rt =
        run(r#"(new s in { s :: () }) | for( {% x <- @"ch" %}[ s ] ){ @"r"!(*x) } | @"ch"!(7)"#)
            .await;
    assert!(
        data_at(&rt, "r").await.is_empty(),
        "a free-sig signed bind must not consume new-bound (ring-fenced) fuel"
    );
}

// ──────────────────────────── pattern rejection ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cost_syntax_in_a_match_pattern_is_rejected() {
    let mut runtime = fresh_runtime().await;
    let result = runtime
        .evaluate_with_term(r#"match 1 { {% Nil %}[ s ] => Nil }"#)
        .await;
    let rejected = match result {
        Err(_) => true,
        Ok(r) => !r.errors.is_empty(),
    };
    assert!(rejected, "a signed term cannot be a match pattern");
}
