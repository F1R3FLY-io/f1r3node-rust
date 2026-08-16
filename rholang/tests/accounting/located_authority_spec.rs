use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostStack, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{BillableKind, Sig, SignatureChannel};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn runtime() -> RhoRuntimeImpl {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    runtime
}

fn lane(signature: &str) -> [u8; 32] {
    let source = format!("{{% Nil %}}[ {signature} ]");
    let par = Compiler::source_to_adt(&source).unwrap();
    cost_signature_to_sig(par.cost_signed_terms[0].signature.as_ref().unwrap())
        .unwrap()
        .lane_hash()
}

async fn evaluate(runtime: &mut RhoRuntimeImpl, source: &str) {
    let result = runtime
        .evaluate_with_phlo(source, Cost::create(1_000_000, "located authority test"))
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

fn comm_count(runtime: &RhoRuntimeImpl) -> usize {
    runtime
        .get_cost_event_log()
        .iter()
        .filter(|event| event.kind == BillableKind::Comm)
        .count()
}

#[tokio::test]
async fn bare_surfaces_are_wrapped_independently_by_construction() {
    let mut runtime = runtime().await;
    let payer = Sig::Ground(b"default payer".to_vec());
    runtime
        .cost
        .set_deploy_signature_funded(b"default payer deploy", payer.clone());
    evaluate(&mut runtime, r#"for(_ <- @"x"){ Nil } | @"x"!(0)"#).await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&payer.lane_hash()), 2);
    let events = runtime.cost.authority_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].authority.regions.len(), 2);
    events[0].verify_authority().unwrap();
}

#[tokio::test]
async fn metered_unmatched_send_persists_its_payer_authority() {
    let mut runtime = runtime().await;
    let payer = Sig::Ground(b"persisted payer".to_vec());
    runtime
        .cost
        .set_deploy_signature_funded(b"persisted payer deploy", payer.clone());

    evaluate(&mut runtime, r#"@"waiting"!(0)"#).await;

    let data = runtime
        .get_data(&new_gstring_par("waiting".to_string(), Vec::new(), false))
        .await;
    let authority = data[0].a.cost_authority.as_ref().unwrap();
    assert_eq!(authority.regions.len(), 1);
    assert_eq!(
        cost_signature_to_sig(authority.regions[0].signature.as_ref().unwrap()).unwrap(),
        payer
    );
    assert!(runtime.cost.authority_realized().0.is_empty());
}

#[tokio::test]
async fn whole_redex_deduplicates_one_shared_region() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ Nil } | @"x"!(0) %}[ a ]"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.0.len(), 1);
}

#[tokio::test]
async fn explicit_region_authority_overrides_the_deploy_default() {
    let mut runtime = runtime().await;
    let default = Sig::Ground(b"default envelope payer".to_vec());
    runtime
        .cost
        .set_deploy_signature_funded(b"explicit region deploy", default.clone());
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ Nil } | @"x"!(0) %}[ explicit ]"#,
    )
    .await;

    let explicit = lane("explicit");
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&explicit), 1);
    assert_eq!(realized.get(&default.lane_hash()), 0);
    assert_eq!(realized.0.len(), 1);
    assert_eq!(runtime.cost.authority_events().len(), 1);
    assert_eq!(
        runtime.cost.authority_events()[0].debit,
        rholang::rust::interpreter::accounting::authority::ResourceMultiset::singleton(explicit, 1)
    );
}

#[tokio::test]
async fn separately_signed_surfaces_charge_every_distinct_region_atomically() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ Nil } %}[ a ] | {% @"x"!(0) %}[ b ]"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.get(&lane("b")), 1);
    assert_eq!(comm_count(&runtime), 1);
}

#[tokio::test]
async fn combined_signature_remains_one_indivisible_purse_cell() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ Nil } | @"x"!(0) %}[ a (*) b ]"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a (*) b")), 1);
    assert_eq!(realized.0.len(), 1);
}

#[tokio::test]
async fn lollipop_consumes_outer_then_continuation_authority_without_rewrapping() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ for(_ <- @"y"){ Nil } | @"y"!(0) } %}[ a -o b ] | @"x"!(0)"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.get(&lane("b")), 1);
    assert_eq!(comm_count(&runtime), 2);
}

#[tokio::test]
async fn lollipop_does_not_charge_an_inert_continuation() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ Nil } %}[ a -o b ] | @"x"!(0)"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.get(&lane("b")), 0);
    assert_eq!(realized.0.len(), 1);
    assert_eq!(comm_count(&runtime), 1);
}

#[tokio::test]
async fn uniform_signing_does_not_charge_an_inert_continuation() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ Nil } %}[ a ] | @"x"!(0)"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.0.len(), 1);
    assert_eq!(comm_count(&runtime), 1);
}

#[tokio::test]
async fn right_associative_lollipop_chain_threads_each_authority_once() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ for(_ <- @"y"){ for(_ <- @"z"){ Nil } } } %}[ a -o b -o c ] | @"x"!(0) | @"y"!(0) | @"z"!(0)"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.get(&lane("b")), 1);
    assert_eq!(realized.get(&lane("c")), 1);
    assert_eq!(realized.0.len(), 3);
    assert_eq!(comm_count(&runtime), 3);
}

#[tokio::test]
async fn compound_lollipop_requires_the_joint_outer_purse_then_continuation_purse() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"{% for(_ <- @"x"){ for(_ <- @"y"){ Nil } | @"y"!(0) } %}[ a (*) b -o c ] | @"x"!(0)"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a (*) b")), 1);
    assert_eq!(realized.get(&lane("a")), 0);
    assert_eq!(realized.get(&lane("b")), 0);
    assert_eq!(realized.get(&lane("c")), 1);
    assert_eq!(realized.0.len(), 2);
    assert_eq!(comm_count(&runtime), 2);
}

#[tokio::test]
async fn unit_cannot_be_materialized_as_a_stack_cell() {
    let runtime = runtime().await;
    let result = runtime
        .inj(
            Par {
                cost_stacks: vec![CostStack {
                    cells: vec![CostSignature {
                        value: Some(Value::Unit(true)),
                    }],
                }],
                ..Par::default()
            },
            Env::new(),
            Blake2b512Random::create_from_bytes(b"unit stack cell"),
        )
        .await;
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unit cannot be stored as a token-stack cell"),
        "{:?}",
        error
    );
    assert!(runtime.cost.authority_events().is_empty());
    assert!(runtime.cost.authority_realized().0.is_empty());
}

#[tokio::test]
async fn signed_join_collects_all_clause_authorities_in_one_comm() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"for({% _ <- @"x" %}[ a ] & {% _ <- @"y" %}[ b ]){ Nil } | @"x"!(0) | @"y"!(0)"#,
    )
    .await;
    let realized = runtime.cost.authority_realized();
    assert_eq!(realized.get(&lane("a")), 1);
    assert_eq!(realized.get(&lane("b")), 1);
    assert_eq!(comm_count(&runtime), 1);
}

#[tokio::test]
async fn bound_slot_identity_is_the_runtime_unforgeable_name() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"new slot in { {% for(_ <- @"x"){ Nil } | @"x"!(0) %}[ slot ] | @"published"!(*slot) }"#,
    )
    .await;
    let published = runtime
        .get_data(&new_gstring_par("published".to_string(), Vec::new(), false))
        .await;
    let normalized = ParSortMatcher::sort_match(&published[0].a.pars[0]).term;
    let expected = rholang::rust::interpreter::accounting::Sig::Ground(
        prost::Message::encode_to_vec(&normalized),
    )
    .lane_hash();
    assert_eq!(runtime.cost.authority_realized().get(&expected), 1);
}

#[tokio::test]
async fn signed_send_payload_retains_its_authority_when_received_and_run() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"for(@p <- @"carrier"){ p } | @"carrier"!( {% for(_ <- @"work"){ Nil } | @"work"!(0) %}[ payload ] )"#,
    )
    .await;
    assert_eq!(runtime.cost.authority_realized().get(&lane("payload")), 1);
}

#[tokio::test]
async fn token_stack_send_payload_retains_order_when_received_and_run() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"for(@p <- @"carrier"){ p } | @"carrier"!( first :: second :: () )"#,
    )
    .await;
    let par = Compiler::source_to_adt(r#"first :: second :: ()"#).unwrap();
    let stack = par.cost_stacks[0].clone();
    let signature = cost_signature_to_sig(&stack.cells[0]).unwrap();
    let data = runtime
        .get_data(&SignatureChannel::from_sig(&signature).par)
        .await;
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].a.cost_stack.as_ref(), Some(&stack));
}

#[tokio::test]
async fn checkpoint_reset_preserves_payload_sorts_and_continuation_authority() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"@"carrier"!( {% Nil %}[ payload ], first :: second :: () ) | {% for(_ <- @"waiting"){ Nil } %}[ wait_outer -o wait_inner ]"#,
    )
    .await;

    let carrier = new_gstring_par("carrier".to_string(), Vec::new(), false);
    let waiting = new_gstring_par("waiting".to_string(), Vec::new(), false);
    let expected_signed = Compiler::source_to_adt(r#"{% Nil %}[ payload ]"#).unwrap();
    let expected_stack = Compiler::source_to_adt(r#"first :: second :: ()"#).unwrap();
    let before_data = runtime.get_data(&carrier).await;
    let before_continuations = runtime.get_continuations(vec![waiting.clone()]).await;

    assert_eq!(before_data.len(), 1);
    assert_eq!(before_data[0].a.pars.len(), 2);
    assert_eq!(
        before_data[0].a.pars[0].cost_signed_terms,
        expected_signed.cost_signed_terms
    );
    assert_eq!(
        before_data[0].a.pars[1].cost_stacks,
        expected_stack.cost_stacks
    );
    assert_eq!(before_continuations.len(), 1);
    assert!(before_continuations[0]
        .continuation
        .cost_authority
        .is_some());

    let checkpoint = runtime.create_checkpoint().await;
    runtime.reset(&checkpoint.root).await.unwrap();

    assert_eq!(runtime.get_data(&carrier).await, before_data);
    assert_eq!(
        runtime.get_continuations(vec![waiting]).await,
        before_continuations
    );
}

#[tokio::test]
async fn derived_timeout_race_emits_one_signed_error_and_cannot_fire_late_funding() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"new gate in {
          gate!(0) |
          for(_ <- @"funds" & _ <- gate){ @"funded"!(0) } |
          for(_ <- @"timeout" & _ <- gate){ @"errors"!( {% @"ErrNoTokens"!(0) %}[ validator ] ) } |
          @"timeout"!(0)
        }"#,
    )
    .await;

    let errors = new_gstring_par("errors".to_string(), Vec::new(), false);
    let funded = new_gstring_par("funded".to_string(), Vec::new(), false);
    let expected = Compiler::source_to_adt(r#"{% @"ErrNoTokens"!(0) %}[ validator ]"#).unwrap();
    let before = runtime.get_data(&errors).await;
    assert_eq!(before.len(), 1);
    assert_eq!(
        before[0].a.pars[0].cost_signed_terms,
        expected.cost_signed_terms
    );
    assert!(runtime.get_data(&funded).await.is_empty());

    evaluate(&mut runtime, r#"@"funds"!(0)"#).await;
    assert_eq!(runtime.get_data(&errors).await, before);
    assert!(runtime.get_data(&funded).await.is_empty());
}

#[tokio::test]
async fn derived_timeout_race_emits_one_funded_result_and_cannot_fire_late_timeout() {
    let mut runtime = runtime().await;
    evaluate(
        &mut runtime,
        r#"new gate in {
          gate!(0) |
          for(_ <- @"funds" & _ <- gate){ @"funded"!(0) } |
          for(_ <- @"timeout" & _ <- gate){ @"errors"!( {% @"ErrNoTokens"!(0) %}[ validator ] ) } |
          @"funds"!(0)
        }"#,
    )
    .await;

    let errors = new_gstring_par("errors".to_string(), Vec::new(), false);
    let funded = new_gstring_par("funded".to_string(), Vec::new(), false);
    let before = runtime.get_data(&funded).await;
    assert_eq!(before.len(), 1);
    assert!(runtime.get_data(&errors).await.is_empty());

    evaluate(&mut runtime, r#"@"timeout"!(0)"#).await;
    assert_eq!(runtime.get_data(&funded).await, before);
    assert!(runtime.get_data(&errors).await.is_empty());
}
