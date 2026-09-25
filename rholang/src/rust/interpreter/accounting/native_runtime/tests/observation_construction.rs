use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use prost::Message;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::native_phlo_rules::NativeBudgetReplayDecision;
use crate::rust::interpreter::accounting::{byte_accounting, observation_construction as build};

fn inputs(size: usize) -> (Par, ListParWithRandom, TaggedContinuation, COMM) {
    let channel = Par::default();
    let data = ListParWithRandom {
        random_state: vec![7; size],
        cost_authority: Some(authority(1)),
        ..Default::default()
    };
    let continuation = TaggedContinuation {
        cost_authority: Some(authority(1)),
        ..Default::default()
    };
    let producer = Produce::create(&channel, &data, false);
    let source = COMM {
        consume: Consume::create(
            &vec![channel.clone()],
            &vec![BindPattern::default()],
            &continuation,
            false,
        ),
        produces: vec![producer.clone()],
        peeks: Default::default(),
        times_repeated: [(producer, 1)].into_iter().collect(),
    };
    (channel, data, continuation, source)
}

#[test]
fn native_observation_construction_keeps_introduction_and_payload_authorities_separate() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(u64::MAX, [1, 1, 1, 1]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let (channel, data, continuation, comm) = inputs(17);
    let introduction_authority = authority(3);
    let producer = &comm.produces[0];
    let event_id = byte_accounting::produce_introduction_identity(producer);
    budget
        .register_introduction_authority(
            event_id,
            AuthorityByteEventKind::ProduceIntroduction,
            &introduction_authority,
        )
        .unwrap();
    let resolved = budget
        .introduction_authority(event_id, AuthorityByteEventKind::ProduceIntroduction)
        .unwrap();
    let before_rows = budget.byte_observations();
    let before_usage = budget.native_phlo_usage();
    let before_registry = budget.introduction_authorities.lock().unwrap().clone();
    let expected = build::produce_introduction(producer, &channel, &data, &resolved)
        .unwrap()
        .into_native()
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            build::produce_introduction(producer, &channel, &data, &resolved)
                .unwrap()
                .into_native()
                .unwrap(),
            expected
        );
    }
    assert_eq!(budget.byte_observations(), before_rows);
    assert_eq!(budget.native_phlo_usage(), before_usage);
    assert!(budget
        .native_budget_recording()
        .unwrap()
        .unwrap()
        .attempts
        .is_empty());
    assert_eq!(
        *budget.introduction_authorities.lock().unwrap(),
        before_registry
    );
    assert_eq!(expected.authority, resolved);
    assert_ne!(expected.authority, data.cost_authority.clone().unwrap());
    assert_eq!(
        expected.measurement.unwrap().introduction_bytes,
        (channel.encoded_len() + data.encoded_len()) as u64 + 64
    );
    with_operation(&budget, || {
        budget.reserve_produce_introduction_measured(
            event_id,
            &resolved,
            expected.measurement.unwrap(),
            false,
        )
    })
    .unwrap();
    assert_eq!(budget.byte_observations().rows[0].as_ref(), &expected);

    let channels = [channel];
    let patterns = [BindPattern::default()];
    let expected = build::consume_introduction(
        &comm.consume,
        &channels,
        &patterns,
        &continuation,
        &introduction_authority,
    )
    .unwrap()
    .into_native()
    .unwrap();
    with_operation(&budget, || {
        budget.reserve_consume_introduction_measured(
            expected.event_id,
            &introduction_authority,
            expected.measurement.unwrap(),
            false,
        )
    })
    .unwrap();
    assert_eq!(budget.byte_observations().rows[1].as_ref(), &expected);
    assert_ne!(expected.authority, continuation.cost_authority.unwrap());
}

#[test]
fn native_observation_construction_does_not_insert_fallback_authority() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(u64::MAX, [1, 1, 1, 1]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let (channel, data, _, comm) = inputs(17);
    let id = byte_accounting::produce_introduction_identity(&comm.produces[0]);
    let kind = AuthorityByteEventKind::ProduceIntroduction;
    let fallback = budget.fallback_introduction_authority(id, kind).unwrap();
    let observed = build::produce_introduction(&comm.produces[0], &channel, &data, &fallback)
        .unwrap()
        .into_native()
        .unwrap();
    assert!(budget.introduction_authorities.lock().unwrap().is_empty());
    assert!(budget.byte_observations().rows.is_empty());
    assert!(budget
        .native_budget_recording()
        .unwrap()
        .unwrap()
        .attempts
        .is_empty());
    assert_eq!(budget.native_phlo_usage(), Some(0));
    let resolved = budget.introduction_authority(id, kind).unwrap();
    assert_eq!(resolved, observed.authority);
    assert_eq!(budget.introduction_authorities.lock().unwrap().len(), 1);
}

#[test]
fn native_comm_construction_matches_recorded_measurements_for_every_persistence_pair() {
    for continuation_persistent in [false, true] {
        for data_persistent in [false, true] {
            let budget = RuntimeBudget::new(Cost::unsafe_max());
            budget
                .reset_for_native_execution(config(u64::MAX, [1, 1, 1, 1]))
                .unwrap();
            let _scope = budget.enter_comm_accounting_scope();
            let (channel, data, continuation, mut comm) = inputs(33);
            let producer = Produce::create(&channel, &data, data_persistent);
            comm.consume = Consume::create(
                &vec![channel],
                &vec![BindPattern::default()],
                &continuation,
                continuation_persistent,
            );
            comm.produces = vec![producer.clone()];
            comm.times_repeated = [(producer, 1)].into_iter().collect();
            let observed = build::comm(&comm, &continuation, continuation_persistent, &[(
                &data,
                data_persistent,
            )])
            .unwrap()
            .into_native()
            .unwrap();
            let raw = observed.measurement.unwrap();
            assert_eq!(raw.transfer_bytes, data.encoded_len() as u64);
            assert_eq!(raw.trace_bytes, 128);
            assert_eq!(raw.introduction_bytes, 0);
            assert!(observed.legacy_amount.is_none());
            assert_eq!(observed.authority.regions.len(), 1);
            assert_eq!(
                observed.authority == authority(1),
                !continuation_persistent && !data_persistent
            );
            with_operation(&budget, || {
                budget.reserve_comm_authority_measured(observed.event_id, &observed.authority, raw)
            })
            .unwrap();
            assert_eq!(budget.byte_observations().rows[0].as_ref(), &observed);
            let before_usage = budget.native_phlo_usage();
            let before_rows = budget.byte_observations();
            assert_eq!(
                build::comm(&comm, &continuation, continuation_persistent, &[(
                    &data,
                    data_persistent
                )])
                .unwrap()
                .into_native()
                .unwrap(),
                observed
            );
            assert_eq!(budget.native_phlo_usage(), before_usage);
            assert_eq!(budget.byte_observations(), before_rows);
            assert_eq!(
                budget
                    .native_budget_recording()
                    .unwrap()
                    .unwrap()
                    .attempts
                    .len(),
                1
            );
        }
    }
}

#[test]
fn native_observation_construction_rejects_empty_and_conflicting_authority_but_preserves_unit() {
    let (channel, data, mut continuation, comm) = inputs(1);
    assert!(build::produce_introduction(
        &comm.produces[0],
        &channel,
        &data,
        &CostAuthority::default()
    )
    .unwrap()
    .into_native()
    .is_err());
    let unit = authority(0);
    let observed = build::produce_introduction(&comm.produces[0], &channel, &data, &unit)
        .unwrap()
        .into_native()
        .unwrap();
    assert_eq!(observed.authority, unit);
    assert!(!observed.authority.regions.is_empty());
    assert!(authority::authority_demand(&observed.authority)
        .unwrap()
        .0
        .is_empty());
    continuation.cost_authority.as_mut().unwrap().regions[0].signature =
        unit.regions[0].signature.clone();
    assert!(build::comm(&comm, &continuation, true, &[(&data, false)]).is_err());
}

#[test]
fn native_observation_preserves_legacy_measurements_without_legacy_amounts() {
    for owners in [0, 1, 3] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let _scope = budget.enter_comm_accounting_scope();
        let auth = authority(owners);
        let (channel, mut data, mut continuation, comm) = inputs(19);
        data.cost_authority = Some(auth.clone());
        continuation.cost_authority = Some(auth.clone());
        let native = build::produce_introduction(&comm.produces[0], &channel, &data, &auth)
            .unwrap()
            .into_native()
            .unwrap();
        budget
            .reserve_produce_introduction_measured(
                native.event_id,
                &auth,
                native.measurement.unwrap(),
                false,
            )
            .unwrap();
        let mut recorded = budget.byte_observations().rows[0].as_ref().clone();
        assert_eq!(
            recorded.legacy_amount,
            Some(
                native
                    .measurement
                    .unwrap()
                    .cost(byte_accounting::BYTE_COST_SCHEDULE_V1)
                    .unwrap()
            )
        );
        recorded.legacy_amount = None;
        assert_eq!(recorded, native);
        let native = build::comm(&comm, &continuation, false, &[(&data, false)])
            .unwrap()
            .into_native()
            .unwrap();
        budget
            .reserve_comm_authority_measured(
                native.event_id,
                &native.authority,
                native.measurement.unwrap(),
            )
            .unwrap();
        let mut recorded = budget.byte_observations().rows[1].as_ref().clone();
        assert_eq!(recorded.legacy_amount.is_some(), owners > 0);
        recorded.legacy_amount = None;
        assert_eq!(recorded, native);
    }
}

#[tokio::test]
async fn native_typed_replay_preserves_denied_comm_and_accepted_introduction_usage() {
    for produce in [false, true] {
        let (channel, data, continuation, comm) = inputs(200);
        let auth = authority(1);
        let channels = [channel.clone()];
        let patterns = [BindPattern::default()];
        let source = if produce {
            RSpaceOperationSource::Produce(&comm.produces[0])
        } else {
            RSpaceOperationSource::Consume(&comm.consume)
        };
        let introduction = if produce {
            build::produce_introduction(&comm.produces[0], &channel, &data, &auth).unwrap()
        } else {
            build::consume_introduction(&comm.consume, &channels, &patterns, &continuation, &auth)
                .unwrap()
        }
        .into_native()
        .unwrap();
        let comm_observation = build::comm(&comm, &continuation, false, &[(&data, false)])
            .unwrap()
            .into_native()
            .unwrap();
        let limit = introduction.measurement.unwrap().introduction_bytes;
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(limit, [0, 1, 1, 0]))
            .unwrap();
        let _scope = budget.enter_comm_accounting_scope();
        let order = || OperationOrder {
            session: budget.deploy_id(),
            path: vec![(0, 0)].into(),
        };
        operation_context::scope(order(), async {
            budget
                .start_native_operation(source, &channels, &[])
                .unwrap();
            if produce {
                budget
                    .reserve_produce_introduction_measured(
                        introduction.event_id,
                        &auth,
                        introduction.measurement.unwrap(),
                        false,
                    )
                    .unwrap();
            } else {
                budget
                    .observe_native_consume_peeks(&std::collections::BTreeSet::new())
                    .unwrap();
                budget
                    .reserve_consume_introduction_measured(
                        introduction.event_id,
                        &auth,
                        introduction.measurement.unwrap(),
                        false,
                    )
                    .unwrap();
            }
            budget.observe_native_comm_source(&comm).unwrap();
            assert!(matches!(
                budget.reserve_comm_authority_measured(
                    comm_observation.event_id,
                    &comm_observation.authority,
                    comm_observation.measurement.unwrap()
                ),
                Err(InterpreterError::OutOfPhlogistonsError)
            ));
            budget.finish_native_operation(source, RSpaceOperationCompletion::Rejected);
        })
        .await;
        let recording = budget.native_budget_recording().unwrap().unwrap();
        assert_eq!(recording.used, limit);
        assert_eq!(recording.attempts.len(), 2);
        assert!(!recording.attempts[1].granted);
        let rows = budget.native_operation_recording().unwrap().unwrap();
        let host = config(limit, [0, 1, 1, 0]).host_work;
        let checked = with_contract(limit, [0, 1, 1, 0], |contract| {
            contract
                .check_operation_journal(
                    recording.session,
                    &recording,
                    rows,
                    journal_limits(),
                    &host,
                )
                .unwrap()
        });
        let trace = checked
            .bind_trace(
                Arc::from([]),
                NativeOperationTraceLimits {
                    events: 0,
                    source_entries: 100,
                    source_bytes: 100_000,
                    telemetry_items: 100,
                    telemetry_bytes: 100_000,
                },
                &host,
            )
            .unwrap();
        let replay = trace.into_replay(host.clone()).unwrap();
        operation_context::scope(order(), async {
            let mut ticket = replay.reserve_current(source).unwrap();
            ticket.authenticate_footprint(&channels, &[]).unwrap();
            let decision = if produce {
                ticket.observe_rho_produce(&comm.produces[0], &channel, &data, &auth)
            } else {
                ticket.observe_rho_consume(
                    &comm.consume,
                    &channels,
                    &patterns,
                    &continuation,
                    &std::collections::BTreeSet::new(),
                    &auth,
                )
            }
            .unwrap();
            assert_eq!(decision, NativeBudgetReplayDecision::Accepted {
                usage: limit
            });
            let mut altered = data.clone();
            altered.random_state.push(8);
            assert!(ticket
                .observe_rho_comm(&comm, &continuation, false, &[(&altered, false)])
                .is_err());
            assert_eq!(replay.completed_usage(), 0);
            assert_eq!(
                ticket
                    .observe_rho_comm(&comm, &continuation, false, &[(&data, false)])
                    .unwrap(),
                NativeBudgetReplayDecision::Denied
            );
            ticket
                .prepare_completion(NativeReplayOutcome::DeniedComm)
                .unwrap()
                .publish();
            assert!(replay.reserve_current(source).is_err());
        })
        .await;
        replay.check_complete().unwrap();
        assert_eq!(replay.completed_usage(), limit);
        assert_eq!(budget.native_phlo_usage(), Some(limit));
        assert_eq!(
            budget
                .native_budget_recording()
                .unwrap()
                .unwrap()
                .attempts
                .len(),
            2
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn native_observation_keeps_exact_payload_and_multi_owner_measurements(
        bytes in prop::collection::vec(any::<u8>(), 0..512), owners in 1usize..17,
    ) {
        let auth = authority(owners);
        let (channel, mut data, continuation, _) = inputs(0);
        data.random_state = bytes;
        let source = Produce::create(&channel, &data, false);
        let observed = build::produce_introduction(&source, &channel, &data, &auth).unwrap().into_native().unwrap();
        prop_assert_eq!(observed.event_id, byte_accounting::produce_introduction_identity(&source));
        prop_assert_eq!(observed.authority, authority::canonical_authority(&auth).unwrap());
        prop_assert_eq!(observed.measurement.unwrap().introduction_bytes, (channel.encoded_len() + data.encoded_len()) as u64 + 64);
        prop_assert!(observed.legacy_amount.is_none());
        let channels = vec![channel; owners];
        let patterns = vec![BindPattern::default(); owners];
        let consume = Consume::create(&channels, &patterns, &continuation, false);
        let observed = build::consume_introduction(&consume, &channels, &patterns, &continuation, &auth).unwrap().into_native().unwrap();
        let messages: usize = channels.iter().map(Message::encoded_len).sum::<usize>() + patterns.iter().map(Message::encoded_len).sum::<usize>() + continuation.encoded_len();
        prop_assert_eq!(observed.measurement.unwrap().introduction_bytes, messages as u64 + 32 + 32 * owners as u64);
    }
}
