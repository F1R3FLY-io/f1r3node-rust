use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostStack, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gstring_par;
use proptest::prelude::*;
use rholang::rust::interpreter::accounting::authority::{
    authority_demand, cost_signature_to_sig, AuthorityByteEventKind,
};
use rholang::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{BillableKind, Sig, SignatureChannel};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::rho_runtime::{
    create_runtime_from_kv_store_for_epoch, RhoRuntime, RhoRuntimeImpl,
};
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event as RSpaceEvent, IOEvent, Produce};

mod transfer_properties;

async fn runtime() -> RhoRuntimeImpl { runtime_for_epoch(EventCodecEpoch::CostV1).await }

async fn runtime_for_epoch(epoch: EventCodecEpoch) -> RhoRuntimeImpl {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.unwrap();
    create_runtime_from_kv_store_for_epoch(
        epoch,
        stores,
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        Arc::new(Box::new(Matcher)),
        ExternalServices::noop(),
    )
    .await
    .expect("located authority runtime uses the cost-bearing event format")
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

#[tokio::test]
async fn unmetered_runtime_communication_has_no_authority_or_byte_trace() {
    let mut runtime = runtime().await;
    runtime.cost.set_unmetered(true);

    let result = runtime
        .evaluate_with_phlo(
            r#"for(_ <- @"unmetered"){ Nil } | @"unmetered"!(0)"#,
            Cost::create(1, "unmetered runtime communication"),
        )
        .await
        .unwrap();

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.cost.value, 0);
    assert_eq!(result.quantitative_byte_cost, 0);
    assert!(result.authority_events.is_empty());
    assert!(result.authority_byte_events.is_empty());
}

#[tokio::test]
async fn peek_restoration_preserves_neutral_data_and_charges_the_active_deploy() {
    let mut runtime = runtime().await;
    let channel = new_gstring_par("neutral-peek".to_string(), Vec::new(), false);
    runtime
        .inj(
            Compiler::source_to_adt(r#"@"neutral-peek"!(0)"#).unwrap(),
            Env::new(),
            Blake2b512Random::create_from_bytes(b"neutral peek setup"),
        )
        .await
        .unwrap();
    let before = runtime.get_data(&channel).await.expect("before query");
    assert_eq!(before.len(), 1);
    assert!(before[0].a.cost_authority.is_none());

    let payer = Sig::Ground(b"neutral peek active payer".to_vec());
    runtime
        .cost
        .set_deploy_signature_funded(b"neutral peek deploy", payer.clone());
    let result = runtime
        .evaluate_with_phlo(
            r#"for(_ <<- @"neutral-peek"){ Nil }"#,
            Cost::create(1_000_000, "neutral peek restoration"),
        )
        .await
        .unwrap();

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let after = runtime.get_data(&channel).await.expect("after query");
    assert_eq!(after.len(), 1);
    assert!(after[0].a.cost_authority.is_none());
    assert_eq!(
        result
            .authority_byte_events
            .iter()
            .filter(|event| event.kind == AuthorityByteEventKind::ProduceIntroduction)
            .count(),
        1
    );
    assert!(result.authority_byte_events.iter().all(|event| {
        authority_demand(&event.authority)
            .is_ok_and(|demand| demand.get(&payer.lane_hash()) == 1 && demand.0.len() == 1)
    }));
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
async fn processed_cost_is_exactly_comm_units_plus_quantitative_bytes() {
    let mut runtime = runtime().await;
    let payer = Sig::Ground(b"combined debit payer".to_vec());
    runtime
        .cost
        .set_deploy_signature_funded(b"combined debit deploy", payer);
    let result = runtime
        .evaluate_with_phlo(
            r#"for(_ <- @"x"){ Nil } | @"x"!(0)"#,
            Cost::create(1_000_000, "combined debit test"),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let comm_units = u64::try_from(
        runtime
            .get_cost_event_log()
            .iter()
            .filter(|event| event.kind == BillableKind::Comm)
            .count(),
    )
    .unwrap();

    assert_eq!(
        u64::try_from(result.cost.value).unwrap(),
        comm_units + result.quantitative_byte_cost,
        "{:?}",
        runtime.get_cost_event_log()
    );
    assert_eq!(comm_units, 1);
    assert_eq!(
        result.authority_realized.0.values().copied().sum::<u64>(),
        2
    );
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
        .await
        .expect("waiting query");
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

fn stack_birth_and_first_comm_positions(
    trace: &[RSpaceEvent],
    produce_hash: &[u8; 32],
) -> (usize, usize) {
    let birth = trace
        .iter()
        .position(|event| {
            matches!(
                event,
                RSpaceEvent::IoEvent(IOEvent::Produce(produce))
                    if produce.hash.bytes().as_slice() == produce_hash.as_slice()
            )
        })
        .unwrap();
    let comm = trace
        .iter()
        .position(|event| matches!(event, RSpaceEvent::Comm(_)))
        .unwrap();
    (birth, comm)
}

async fn check_epoch_introduction_identity(epoch: EventCodecEpoch, payer: u8, payload: i32) {
    let mut runtime = runtime_for_epoch(epoch).await;
    assert_eq!(runtime.reducer.space.codec_epoch(), epoch);
    runtime.cost.set_deploy_signature_funded(
        b"epoch introduction identity deploy",
        Sig::Ground(vec![payer; 32]),
    );
    let outer = format!("outer{payer}");
    let inner = format!("inner{payer}");
    let source = format!(
        "{{% for(@value <- @\"epoch-input\") {{ {inner} :: () | @\"epoch-output\"!(value) }} %}}[ {outer} -o {inner} ] | @\"epoch-input\"!({payload})"
    );
    let result = runtime
        .evaluate_with_phlo(
            &source,
            Cost::create(1_000_000, "epoch introduction identity"),
        )
        .await
        .expect("epoch introduction evaluation");
    assert!(result.errors.is_empty(), "{epoch:?}: {:?}", result.errors);
    assert_eq!(result.authority_stack_births.len(), 1);
    let trace = runtime.take_event_log().await;
    let consume = trace
        .iter()
        .find_map(|event| match event {
            RSpaceEvent::IoEvent(IOEvent::Consume(source)) => Some(source),
            _ => None,
        })
        .expect("signed receive introduction appears in trace");
    let assert_introduction = |identity, kind, payer| {
        let events: Vec<_> = result
            .authority_byte_events
            .iter()
            .filter(|event| event.event_id == identity && event.kind == kind)
            .collect();
        assert_eq!(events.len(), 1, "{epoch:?}: introduction {identity:?}");
        let event = events[0];
        event
            .verify_authority()
            .expect("canonical introduction authority");
        let demand = authority_demand(&event.authority).expect("introduction demand");
        assert_eq!(demand.get(&payer), 1);
        assert_eq!(demand.0.len(), 1);
        assert!(event.amount > 0);
    };
    assert_introduction(
        consume_introduction_identity(consume),
        AuthorityByteEventKind::ConsumeIntroduction,
        lane(&outer),
    );
    let stack = Compiler::source_to_adt(&format!("{inner} :: ()"))
        .expect("stack source")
        .cost_stacks
        .remove(0);
    let channel = SignatureChannel::from_sig(
        &cost_signature_to_sig(stack.cells.first().expect("stack head"))
            .expect("stack head signature"),
    )
    .par;
    let data = runtime.get_data(&channel).await.expect("stack query");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].a.cost_stack.as_ref(), Some(&stack));
    assert!(data[0].a.cost_authority.is_none());
    let expected = Produce::create_for_epoch(epoch, &channel, data[0].a.as_ref(), false)
        .expect("epoch stack source");
    assert_eq!(data[0].source.hash, expected.hash);
    assert_eq!(data[0].source.channel_hash, expected.channel_hash);
    let birth = &result.authority_stack_births[0];
    assert_eq!(
        birth.produce_hash.as_slice(),
        expected.hash.bytes().as_slice()
    );
    let (birth_position, comm_position) =
        stack_birth_and_first_comm_positions(&trace, &birth.produce_hash);
    assert!(comm_position < birth_position, "{epoch:?}: {trace:?}");
    assert_introduction(
        produce_introduction_identity(&expected),
        AuthorityByteEventKind::ProduceIntroduction,
        lane(&inner),
    );
    let output = runtime
        .get_data(&new_gstring_par(
            "epoch-output".to_string(),
            Vec::new(),
            false,
        ))
        .await
        .expect("output query");
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].a.pars, vec![Compiler::source_to_adt(
        &payload.to_string()
    )
    .expect("payload source")]);
    assert_introduction(
        produce_introduction_identity(&output[0].source),
        AuthorityByteEventKind::ProduceIntroduction,
        lane(&inner),
    );
}

#[tokio::test]
async fn cost_v2_introductions_keep_payers_and_stack_birth_identity() {
    check_epoch_introduction_identity(EventCodecEpoch::CostV2, 7, -42).await;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn generated_introductions_keep_epoch_identity_and_exact_payer(
        payer in any::<u8>(), payload in any::<i32>(),
    ) {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all().build().expect("property runtime");
        executor.block_on(async {
            for epoch in [EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
                check_epoch_introduction_identity(epoch, payer, payload).await;
            }
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_cost_stack_is_materialized_before_sibling_reduction() {
    let mut runtime = runtime().await;
    runtime.cost.set_deploy_signature_funded(
        b"parallel stack declaration deploy",
        Sig::Ground(b"parallel stack declaration payer".to_vec()),
    );
    let result = runtime
        .evaluate_with_phlo(
            r#"t :: t :: () | {% for(_ <- @"x"){ Nil } %}[ t ] | @"x"!(0)"#,
            Cost::create(1_000_000, "parallel stack declaration"),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.authority_stack_births.len(), 1);
    let trace = runtime.take_event_log().await;
    let (birth, comm) = stack_birth_and_first_comm_positions(
        &trace,
        &result.authority_stack_births[0].produce_hash,
    );
    assert!(birth < comm, "birth={birth}, comm={comm}, trace={trace:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn continuation_cost_stack_is_materialized_after_parent_reduction() {
    let mut runtime = runtime().await;
    runtime.cost.set_deploy_signature_funded(
        b"continuation stack declaration deploy",
        Sig::Ground(b"continuation stack declaration payer".to_vec()),
    );
    let result = runtime
        .evaluate_with_phlo(
            r#"for(_ <- @"x"){ t :: () } | @"x"!(0)"#,
            Cost::create(1_000_000, "continuation stack declaration"),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.authority_stack_births.len(), 1);
    let trace = runtime.take_event_log().await;
    let (birth, comm) = stack_birth_and_first_comm_positions(
        &trace,
        &result.authority_stack_births[0].produce_hash,
    );
    assert!(comm < birth, "comm={comm}, birth={birth}, trace={trace:?}");
}

#[tokio::test]
async fn continuation_cost_stack_introduction_is_charged_to_continuation_authority() {
    let mut runtime = runtime().await;
    runtime.cost.set_deploy_signature_funded(
        b"continuation stack byte deploy",
        Sig::Ground(b"continuation stack byte payer".to_vec()),
    );
    let result = runtime
        .evaluate_with_phlo(
            r#"{% for(_ <- @"x"){ continuation :: () } %}[ outer -o continuation ] | @"x"!(0)"#,
            Cost::create(1_000_000, "continuation stack byte authority"),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let outer = lane("outer");
    let continuation = lane("continuation");
    let event = result
        .authority_byte_events
        .iter()
        .find(|event| {
            event.kind == AuthorityByteEventKind::ProduceIntroduction
                && authority_demand(&event.authority)
                    .is_ok_and(|demand| demand.get(&continuation) == 1)
        })
        .unwrap();
    let demand = authority_demand(&event.authority).unwrap();
    assert_eq!(demand.get(&outer), 0);
    assert!(event.amount > 0);

    let stack = Compiler::source_to_adt(r#"continuation :: ()"#)
        .unwrap()
        .cost_stacks
        .remove(0);
    let channel =
        SignatureChannel::from_sig(&cost_signature_to_sig(stack.cells.first().unwrap()).unwrap())
            .par;
    let data = runtime.get_data(&channel).await.expect("signature query");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].a.cost_stack.as_ref(), Some(&stack));
    assert!(data[0].a.cost_authority.is_none());
}

#[tokio::test]
async fn lollipop_receiver_introduction_is_paid_by_its_outer_authority() {
    let mut runtime = runtime().await;
    let creator = Sig::Ground(b"lollipop receiver creator".to_vec());
    runtime
        .cost
        .set_deploy_signature_funded(b"lollipop receiver deploy", creator.clone());
    let result = runtime
        .evaluate_with_phlo(
            r#"{% for(_ <- @"waiting"){ Nil } %}[ outer -o continuation ]"#,
            Cost::create(1_000_000, "lollipop receiver introduction payer"),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let event = result
        .authority_byte_events
        .iter()
        .find(|event| event.kind == AuthorityByteEventKind::ConsumeIntroduction)
        .unwrap();
    let demand = authority_demand(&event.authority).unwrap();
    assert_eq!(demand.get(&creator.lane_hash()), 0);
    assert_eq!(demand.get(&lane("outer")), 1);
    assert_eq!(demand.get(&lane("continuation")), 0);

    let continuations = runtime
        .get_continuations(vec![new_gstring_par(
            "waiting".to_string(),
            Vec::new(),
            false,
        )])
        .await
        .expect("continuation query");
    assert_eq!(continuations.len(), 1);
    let interaction = continuations[0]
        .continuation
        .cost_authority
        .as_ref()
        .unwrap();
    let interaction_demand = authority_demand(interaction).unwrap();
    assert_eq!(interaction_demand.get(&lane("outer")), 1);
    assert_eq!(interaction_demand.get(&creator.lane_hash()), 0);
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
    let outer = lane("a");
    let continuation = lane("b");
    let byte_events = runtime.cost.authority_byte_events();
    assert!(byte_events.iter().any(|event| {
        rholang::rust::interpreter::accounting::authority::authority_demand(&event.authority)
            .unwrap()
            .get(&outer)
            > 0
    }));
    assert!(byte_events.iter().any(|event| {
        rholang::rust::interpreter::accounting::authority::authority_demand(&event.authority)
            .unwrap()
            .get(&continuation)
            > 0
    }));
    assert!(byte_events.iter().all(|event| {
        let demand =
            rholang::rust::interpreter::accounting::authority::authority_demand(&event.authority)
                .unwrap();
        demand.get(&outer) == 0 || demand.get(&continuation) == 0
    }));
    assert_eq!(
        byte_events.iter().map(|event| event.amount).sum::<u64>(),
        runtime.cost.quantitative_byte_cost()
    );
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
    let continuation = lane("b");
    assert!(runtime.cost.authority_byte_events().iter().all(|event| {
        rholang::rust::interpreter::accounting::authority::authority_demand(&event.authority)
            .unwrap()
            .get(&continuation)
            == 0
    }));
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
    let mut unit_stack = Par::default();
    unit_stack.cost_stacks.push(CostStack {
        cells: vec![CostSignature {
            value: Some(Value::Unit(true)),
        }],
        parallel_tail: Vec::new(),
    });
    let result = runtime
        .inj(
            unit_stack,
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
        .await
        .expect("published query");
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
        .await
        .expect("signature channel query");
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
    let before_data = runtime.get_data(&carrier).await.expect("carrier query");
    let before_continuations = runtime
        .get_continuations(vec![waiting.clone()])
        .await
        .expect("waiting query");

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

    assert_eq!(
        runtime
            .get_data(&carrier)
            .await
            .expect("reset carrier query"),
        before_data
    );
    assert_eq!(
        runtime
            .get_continuations(vec![waiting])
            .await
            .expect("reset waiting query"),
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
    let before = runtime.get_data(&errors).await.expect("error query");
    assert_eq!(before.len(), 1);
    assert_eq!(
        before[0].a.pars[0].cost_signed_terms,
        expected.cost_signed_terms
    );
    assert!(runtime
        .get_data(&funded)
        .await
        .expect("funded query")
        .is_empty());

    evaluate(&mut runtime, r#"@"funds"!(0)"#).await;
    assert_eq!(
        runtime.get_data(&errors).await.expect("error query"),
        before
    );
    assert!(runtime
        .get_data(&funded)
        .await
        .expect("funded query")
        .is_empty());
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
    let before = runtime.get_data(&funded).await.expect("funded query");
    assert_eq!(before.len(), 1);
    assert!(runtime
        .get_data(&errors)
        .await
        .expect("error query")
        .is_empty());

    evaluate(&mut runtime, r#"@"timeout"!(0)"#).await;
    assert_eq!(
        runtime.get_data(&funded).await.expect("funded query"),
        before
    );
    assert!(runtime
        .get_data(&errors)
        .await
        .expect("error query")
        .is_empty());
}
