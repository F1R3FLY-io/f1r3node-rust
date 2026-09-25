use std::collections::BTreeMap;

use models::rhoapi::{CostAuthority, CostSignature};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::{self, ResourceMultiset};
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension, NativePhloRegionLimits,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use crate::rust::interpreter::accounting::Sig;

pub(super) fn with_contract<T>(
    limit: u64,
    weights: [u64; 4],
    action: impl FnOnce(NativePhloExecutionContract<'_>) -> T,
) -> T {
    with_priced_contract(limit, weights, 0, action)
}

pub(super) fn native_schedule(weights: [u64; 4], price: u64) -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(weights)
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: price,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

pub(super) fn with_priced_contract<T>(
    limit: u64,
    weights: [u64; 4],
    price: u64,
    action: impl FnOnce(NativePhloExecutionContract<'_>) -> T,
) -> T {
    let descriptor = native_schedule(weights, price);
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [price];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: price,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        limit,
    )
    .unwrap();
    let contract = NativePhloExecutionContract::new(controls, &binding).unwrap();
    action(contract)
}

pub(super) fn config(limit: u64, weights: [u64; 4]) -> NativeRuntimeConfig {
    priced_config(limit, weights, 0)
}

pub(super) fn priced_config(limit: u64, weights: [u64; 4], price: u64) -> NativeRuntimeConfig {
    with_priced_contract(limit, weights, price, |contract| {
        NativeRuntimeConfig::new(
            contract,
            NativeBudgetTraceLimits {
                attempts: 100_000,
                path_segments: 4096,
                regions: NativePhloRegionLimits {
                    regions: 1000,
                    encoded_authority_bytes: 10_000_000,
                },
            },
            HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000))),
        )
    })
}

pub(super) fn journal_limits() -> NativeOperationJournalLimits {
    NativeOperationJournalLimits {
        budget: config(100, [0, 0, 1, 0]).limits,
        operations: 100_000,
        total_path_segments: 1_000_000,
        source_entries: 1_000_000,
        footprint_entries: 1_000_000,
        footprint_bytes: 10_000_000,
        predecessor_edges: 1_000_000,
    }
}

pub(super) fn authority(owners: usize) -> CostAuthority {
    let signatures: Vec<CostSignature> = (0..owners)
        .map(|index| authority::sig_to_cost_signature(&Sig::Ground(vec![index as u8])).unwrap())
        .collect();
    let signature = signatures
        .into_iter()
        .reduce(|left, right| authority::compound_cost_signatures(&left, &right).unwrap())
        .unwrap_or_else(|| authority::sig_to_cost_signature(&Sig::Unit).unwrap());
    CostAuthority {
        regions: vec![authority::cost_region(&signature, b"native runtime", 0).unwrap()],
    }
}

fn reserve(
    budget: &RuntimeBudget,
    auth: &CostAuthority,
    id: u8,
    kind: u8,
    amount: u64,
) -> Result<(), InterpreterError> {
    let raw = ByteCharge {
        introduction_bytes: 0,
        transfer_bytes: amount,
        trace_bytes: 0,
    };
    with_operation(budget, || match kind {
        0 => {
            budget.reserve_produce_introduction_measured([id; 32], auth, raw, id.is_multiple_of(2))
        }
        1 => {
            budget.reserve_consume_introduction_measured([id; 32], auth, raw, id.is_multiple_of(2))
        }
        _ => budget.reserve_comm_authority_measured([id; 32], auth, raw),
    })
}

pub(super) fn with_operation<T>(budget: &RuntimeBudget, call: impl FnOnce() -> T) -> T {
    use std::sync::atomic::{AtomicU64, Ordering};

    use futures::FutureExt;
    use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let order = OperationOrder {
        session: budget.deploy_id(),
        path: vec![(SEQUENCE.fetch_add(1, Ordering::Relaxed), 0)].into(),
    };
    operation_context::scope(order, async { call() })
        .now_or_never()
        .unwrap()
}

#[test]
fn native_runtime_uses_native_weights_and_full_width_without_legacy_double_charge() {
    let budget = RuntimeBudget::new(Cost::create(1, "native test"));
    budget
        .reset_for_native_execution(config(u64::MAX, [0, 0, 1, 0]))
        .unwrap();
    let scope = budget.enter_comm_accounting_scope();
    let auth = authority(1);
    reserve(&budget, &auth, 0, 0, u64::MAX).unwrap();
    reserve(&budget, &auth, 0, 0, u64::MAX).unwrap();
    assert_eq!(budget.native_phlo_usage(), Some(u64::MAX));
    assert_eq!(budget.byte_observations().rows.len(), 1);
    assert!(budget.byte_observations().legacy_events().is_empty());
    assert!(matches!(
        reserve(&budget, &auth, 1, 2, 1),
        Err(InterpreterError::OutOfPhlogistonsError)
    ));
    assert_eq!(budget.native_phlo_usage(), Some(u64::MAX));
    assert_eq!(budget.byte_observations().rows.len(), 1);
    assert!(budget
        .reset_for_native_execution(config(0, [0; 4]))
        .is_err());
    drop(scope);
    assert_eq!(budget.total_cost().value, 0);
    budget
        .reset_for_native_execution(config(0, [0; 4]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    with_operation(&budget, || {
        budget.reserve_comm_authority_measured([2; 32], &auth, ByteCharge {
            introduction_bytes: u64::MAX,
            transfer_bytes: u64::MAX,
            trace_bytes: u64::MAX,
        })
    })
    .unwrap();
    assert_eq!(budget.native_phlo_usage(), Some(0));
}

#[test]
fn native_runtime_rejects_missing_measurements_and_preserves_usage_across_partial_clear() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(6, [2, 0, 0, 0]))
        .unwrap();
    let scope = budget.enter_comm_accounting_scope();
    let auth = authority(3);
    assert!(with_operation(&budget, || budget.reserve_comm_identity([3; 32])).is_err());
    assert!(with_operation(&budget, || budget
        .reserve_comm_authority_identity([3; 32], &auth))
    .is_err());
    assert!(with_operation(&budget, || budget
        .reserve_produce_introduction_identity([3; 32], &auth, 0, false))
    .is_err());
    assert!(budget.byte_observations().rows.is_empty());
    assert_eq!(budget.native_phlo_usage(), Some(0));
    assert!(budget.native_budget_recording().is_err());
    drop(scope);
    budget
        .reset_for_native_execution(config(6, [2, 0, 0, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    reserve(&budget, &auth, 3, 2, 0).unwrap();
    let before = budget.byte_observations();
    reserve(&budget, &auth, 3, 2, 0).unwrap();
    assert_eq!(budget.byte_observations(), before);
    assert!(reserve(&budget, &auth, 3, 2, 1).is_err());
    assert_eq!(budget.native_phlo_usage(), Some(6));
    budget.install_authority_allocation(ResourceMultiset::default());
    assert_eq!(budget.native_phlo_usage(), Some(6));
    assert!(!budget.byte_observations().has_complete_measurements());
    assert!(reserve(&budget, &auth, 4, 2, 0).is_err());
    assert_eq!(budget.native_phlo_usage(), Some(6));
    assert_eq!(before.rows.len(), 1);
}

#[test]
fn native_runtime_parallel_acceptance_never_overdraws_and_retains_exact_receipts() {
    for owners in [1, 3, 65] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(owners as u64 * 6, [2, 0, 1, 0]))
            .unwrap();
        let scope = budget.enter_comm_accounting_scope();
        let auth = authority(owners);
        std::thread::scope(|threads| {
            let barrier = Arc::new(std::sync::Barrier::new(8));
            for worker in 0..8 {
                let budget = budget.clone();
                let auth = &auth;
                let barrier = Arc::clone(&barrier);
                threads.spawn(move || {
                    barrier.wait();
                    let _ = reserve(&budget, auth, worker % 3, 2, (worker % 2) as u64);
                });
            }
        });
        let observed = budget.byte_observations();
        let sum: u64 = observed
            .rows
            .iter()
            .map(|row| (2 + row.measurement.unwrap().transfer_bytes) * owners as u64)
            .sum();
        assert_eq!(budget.native_phlo_usage(), Some(sum));
        assert!(sum <= owners as u64 * 6);
        assert_eq!(budget.authority_events().len(), observed.rows.len());
        drop(scope);
    }
}

#[tokio::test]
async fn native_runtime_real_comm_replays_native_usage_and_complete_measurements() {
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use crate::rust::interpreter::rho_runtime::RhoRuntime;
    use crate::rust::interpreter::test_utils::resources::create_runtimes;

    for (limit, weights) in [(0, [0; 4]), (1_000_000, [2, 3, 5, 7])] {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager.r_space_stores().await.unwrap();
        let (mut play, mut replay, _) = create_runtimes(stores, false, &mut Vec::new()).await;
        for runtime in [&play, &replay] {
            runtime
                .cost
                .set_deploy_signature_funded(b"native-play-replay", Sig::Ground(vec![7]));
        }
        let root = play.get_root().await;
        let term = r#"new x in { x!(7) | for (@value <- x) { @"native-result"!(value) } }"#;
        let rand = Blake2b512Random::create_from_bytes(b"native-play-replay");
        let result = play
            .evaluate_with_native_phlo(
                term,
                HashMap::new(),
                rand.clone(),
                None,
                config(limit, weights),
            )
            .await
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.byte_observations.has_complete_measurements());
        assert!(result
            .byte_observations
            .rows
            .iter()
            .any(|row| row.kind == authority::AuthorityByteEventKind::Comm));
        assert!(result.native_phlo_usage.unwrap() <= limit);
        if limit > 0 {
            assert!(result.native_phlo_usage.unwrap() > 0);
        }
        let checkpoint = play.create_checkpoint().await;
        replay.reset(&root).await.unwrap();
        replay.rig(checkpoint.log).await.unwrap();
        let replayed = replay
            .evaluate_with_native_phlo(term, HashMap::new(), rand, None, config(limit, weights))
            .await
            .unwrap();
        assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
        replay.check_replay_data().await.unwrap();
        assert_eq!(result.native_phlo_usage, replayed.native_phlo_usage);
        assert_eq!(result.byte_observations, replayed.byte_observations);
        let recorded = result.native_budget_recording.as_ref().unwrap();
        let replay_recorded = replayed.native_budget_recording.as_ref().unwrap();
        assert_eq!(recorded.session, play.cost.deploy_id());
        assert_eq!(recorded.session, replay_recorded.session);
        assert_eq!(Some(recorded.used), result.native_phlo_usage);
        assert_eq!(recorded.used, replay_recorded.used);
        for (recording, observations) in [
            (recorded, &result.byte_observations),
            (replay_recorded, &replayed.byte_observations),
        ] {
            assert!(recording.attempts.iter().all(|row| row.granted));
            let observed = recording
                .attempts
                .iter()
                .map(|row| row.observation.as_ref())
                .collect::<Vec<_>>();
            assert_eq!(
                observed,
                observations
                    .rows
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>()
            );
            assert!(recording.attempts.iter().all(|row| {
                row.occurrence.session == recording.session && !row.occurrence.path.is_empty()
            }));
        }
        let by_occurrence = |recording: &NativeBudgetRecording| {
            recording
                .attempts
                .iter()
                .map(|row| {
                    (
                        row.occurrence.clone(),
                        (row.observation.clone(), row.granted),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(by_occurrence(recorded), by_occurrence(replay_recorded));
        let operations = result.native_operation_recording.as_ref().unwrap();
        let replay_operations = replayed.native_operation_recording.as_ref().unwrap();
        assert!(!operations.is_empty());
        let operation_sources = |rows: &[NativeOperationRecord]| {
            rows.iter()
                .map(|row| {
                    (
                        (row.occurrence.session, row.occurrence.path.clone()),
                        (
                            row.source.clone(),
                            row.comm.as_ref().map(|comm| comm.source.clone()),
                            row.completion,
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            operation_sources(operations),
            operation_sources(replay_operations)
        );
        for (rows, budget) in [(operations, recorded), (replay_operations, replay_recorded)] {
            with_contract(limit, weights, |contract| {
                let host =
                    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
                let checked = contract
                    .check_operation_journal(
                        budget.session,
                        budget,
                        Arc::clone(rows),
                        journal_limits(),
                        &host,
                    )
                    .unwrap();
                assert_eq!(checked.total(), budget.used);
                assert_eq!(checked.operation_count(), rows.len());
            });
            let mut links = std::collections::BTreeSet::new();
            for row in rows.iter() {
                assert!(row.budget_start <= row.budget_end);
                assert!(row.budget_end <= budget.attempts.len());
                for link in std::iter::once(row.introduction)
                    .chain(row.comm.as_ref().map(|comm| comm.observation))
                {
                    match link {
                        NativeObservationLink::Attempt(index) => {
                            assert!(links.insert((0, index)));
                            assert!((row.budget_start..row.budget_end).contains(&index));
                            assert_eq!(
                                budget.attempts[index].occurrence.path.as_slice(),
                                row.occurrence.path.as_ref()
                            );
                        }
                        NativeObservationLink::Retry(index) => {
                            assert!(links.insert((1, index)));
                            assert!((row.budget_start..=row.budget_end)
                                .contains(&budget.retries[index].fresh_before));
                            assert_eq!(
                                budget.retries[index].occurrence.path.as_slice(),
                                row.occurrence.path.as_ref()
                            );
                        }
                    }
                }
            }
            assert_eq!(links.len(), budget.attempts.len() + budget.retries.len());
        }
        assert_eq!(checkpoint.root, replay.create_checkpoint().await.root);
    }
}

#[tokio::test]
async fn native_runtime_ceiling_stops_comm_before_its_continuation_runs() {
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{Expr, Par};

    use crate::rust::interpreter::rho_runtime::RhoRuntime;
    use crate::rust::interpreter::test_utils::resources::with_runtime;

    with_runtime("native-limit-", |mut runtime| async move {
        runtime
            .cost
            .set_deploy_signature_funded(b"native-limit", Sig::Ground(vec![7]));
        let result = runtime
            .evaluate_with_native_phlo(
                r#"new x in { x!(7) | for (@value <- x) { @"native-result"!(value) } }"#,
                HashMap::new(),
                Blake2b512Random::create_from_bytes(b"native-limit"),
                None,
                config(0, [1, 0, 0, 0]),
            )
            .await
            .unwrap();
        assert!(
            result
                .errors
                .iter()
                .any(|error| matches!(error, InterpreterError::OutOfPhlogistonsError)),
            "{:?}",
            result.errors
        );
        assert_eq!(result.native_phlo_usage, Some(0));
        let recording = result.native_budget_recording.as_ref().unwrap();
        assert_eq!(recording.used, 0);
        let operations = result.native_operation_recording.as_ref().unwrap();
        with_contract(0, [1, 0, 0, 0], |contract| {
            let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
            let checked = contract.check_operation_journal(
                recording.session, recording, Arc::clone(operations), journal_limits(), &host,
            ).unwrap();
            assert_eq!(checked.total(), 0);
        });
        let denied: Vec<_> = operations.iter().filter(|row| {
            row.completion == rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion::Rejected
        }).collect();
        assert_eq!(denied.len(), 1);
        let comm = denied[0].comm.as_ref().unwrap();
        let NativeObservationLink::Attempt(index) = comm.observation else {
            panic!("denied COMM cannot reference an accepted retry");
        };
        assert!(!recording.attempts[index].granted);
        assert_eq!(comm.source.produces.len(), 1);
        assert!(!comm.source.consume.persistent);
        assert!(recording.attempts.iter().any(|row| {
            !row.granted
                && row.occurrence.stage == super::super::native_phlo_rules::NativeAttemptStage::Comm
        }));
        assert_eq!(
            recording.attempts.iter().filter(|row| row.granted).count(),
            result.byte_observations.rows.len()
        );
        assert!(result
            .byte_observations
            .rows
            .iter()
            .all(|row| row.kind != authority::AuthorityByteEventKind::Comm));
        let output = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("native-result".to_owned())),
        }]);
        assert!(runtime.get_data(&output).await.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_runtime_host_rejection_restores_state_and_excludes_economic_receipts() {
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rust::host_work::HostWorkDimension;

    use crate::rust::interpreter::rho_runtime::RhoRuntime;
    use crate::rust::interpreter::test_utils::resources::with_runtime;

    with_runtime("native-host-reject-", |mut runtime| async move {
        runtime
            .cost
            .set_deploy_signature_funded(b"native-host-reject", Sig::Ground(vec![7]));
        let root = runtime.get_root().await;
        let source = r#"new x in { x!(0) | for (_ <- x) { @"changed"!(1 + 2) } }"#;
        let rand = Blake2b512Random::create_from_bytes(b"native-host-reject");
        let settings = config(1_000_000, [2, 3, 5, 7]);
        let complete_host = settings.host_work.clone();
        let complete = runtime
            .evaluate_with_native_phlo(source, HashMap::new(), rand.clone(), None, settings)
            .await
            .unwrap();
        assert!(complete.errors.is_empty(), "{:?}", complete.errors);
        let calls = complete_host.usage(HostWorkDimension::PrimitiveCalls).get();
        assert!(calls > 0);
        let mut rejected_after_usage = false;
        for allowed in 0..calls {
            runtime.reset(&root).await.unwrap();
            let before = runtime.get_hot_changes().await;
            let _ = runtime.take_event_log().await;
            let mut settings = config(1_000_000, [2, 3, 5, 7]);
            let mut limits = settings.host_work.limits();
            limits.set(
                HostWorkDimension::PrimitiveCalls,
                HostWorkLimit::new(allowed),
            );
            settings.host_work = HostWorkBudget::new(limits);
            let host = settings.host_work.clone();
            let result = runtime
                .evaluate_with_native_phlo(source, HashMap::new(), rand.clone(), None, settings)
                .await
                .unwrap();
            assert_eq!(result.errors, vec![InterpreterError::HostWorkRejected]);
            assert!(host.is_rejected());
            rejected_after_usage |= runtime.cost.native_phlo_usage().unwrap() > 0;
            assert_eq!(result.native_phlo_usage, None);
            assert!(result.native_budget_recording.is_none());
            assert!(result.byte_observations.rows.is_empty());
            assert!(!result.byte_observations.has_complete_measurements());
            assert_eq!(runtime.get_hot_changes().await, before);
            assert!(runtime.take_event_log().await.is_empty());
        }
        assert!(rejected_after_usage);
    })
    .await;
}

#[tokio::test]
async fn native_runtime_reuse_isolates_parse_failure_and_legacy_evaluation() {
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;

    use crate::rust::interpreter::rho_runtime::RhoRuntime;
    use crate::rust::interpreter::test_utils::resources::with_runtime;

    with_runtime("native-reuse-", |mut runtime| async move {
        runtime
            .cost
            .set_deploy_signature_funded(b"native-reuse", Sig::Ground(vec![7]));
        let term = r#"new x in { x!(7) | for (@value <- x) { @"native-result"!(value) } }"#;
        let rand = Blake2b512Random::create_from_bytes(b"native-reuse");
        let first = runtime
            .evaluate_with_native_phlo(
                term,
                HashMap::new(),
                rand.clone(),
                None,
                config(1_000_000, [2, 3, 5, 7]),
            )
            .await
            .unwrap();
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        assert!(first.native_phlo_usage.unwrap() > 0);
        let saved = first.byte_observations.clone();
        let invalid = runtime
            .evaluate_with_native_phlo(
                "new {",
                HashMap::new(),
                rand.clone(),
                None,
                config(0, [1, 0, 0, 0]),
            )
            .await
            .unwrap();
        assert!(matches!(invalid.errors.as_slice(), [
            InterpreterError::ParserError(_)
        ]));
        assert_eq!(invalid.native_phlo_usage, None);
        assert!(invalid.native_budget_recording.is_none());
        assert!(invalid.byte_observations.rows.is_empty());
        assert_eq!(runtime.cost.native_phlo_usage(), Some(0));
        assert!(runtime.cost.byte_observations().rows.is_empty());
        let legacy = runtime
            .evaluate(term, Cost::unsafe_max(), HashMap::new(), rand)
            .await
            .unwrap();
        assert!(legacy.errors.is_empty(), "{:?}", legacy.errors);
        assert_eq!(legacy.native_phlo_usage, None);
        assert!(legacy.native_budget_recording.is_none());
        assert!(legacy.cost.value > 0);
        assert_eq!(first.byte_observations, saved);
    })
    .await;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn native_runtime_histories_refine_idempotent_and_nonpersistent_acceptance(
        limit in 0_u64..1000,
        operations in prop::collection::vec((0_u8..8, 0_u8..3, 0_u64..12), 0..40),
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget.reset_for_native_execution(config(limit, [2, 0, 5, 0])).unwrap();
        let _scope = budget.enter_comm_accounting_scope();
        let auth = authority(3);
        let mut expected_usage = 0_u64;
        let mut expected_rows = 0;
        let mut identities = BTreeMap::new();
        let mut invalid = false;
        for (id, kind, raw) in operations {
            let idempotent = kind == 2 || id.is_multiple_of(2);
            let cost = (u64::from(kind == 2) * 2 + raw * 5) * 3;
            let prior = if idempotent { identities.get(&(id, kind)).copied() } else { None };
            let accepted = !invalid && match prior {
                Some(old_raw) => old_raw == raw,
                None => expected_usage + cost <= limit,
            };
            prop_assert_eq!(reserve(&budget, &auth, id, kind, raw).is_ok(), accepted);
            invalid |= prior.is_some_and(|old_raw| old_raw != raw);
            if accepted && prior.is_none() {
                expected_usage += cost;
                expected_rows += 1;
                if idempotent { identities.insert((id, kind), raw); }
            }
            prop_assert_eq!(budget.native_phlo_usage(), Some(expected_usage));
            prop_assert_eq!(budget.byte_observations().rows.len(), expected_rows);
            if invalid {
                prop_assert!(budget.native_budget_recording().is_err());
            } else {
                let recording = budget.native_budget_recording().unwrap().unwrap();
                prop_assert_eq!(recording.used, expected_usage);
                prop_assert_eq!(recording.attempts.iter().filter(|attempt| attempt.granted).count(), expected_rows);
                for retry in recording.retries.iter() {
                    let original = &recording.attempts[retry.accepted_attempt];
                    prop_assert!(original.granted);
                    prop_assert_eq!(&original.observation, &retry.observation);
                    prop_assert_ne!(&original.occurrence, &retry.occurrence);
                    prop_assert!(retry.accepted_attempt < retry.fresh_before);
                    prop_assert!(retry.fresh_before <= recording.attempts.len());
                }
            }
        }
        if !invalid {
            let recording = budget.native_budget_recording().unwrap().unwrap();
            let settings = config(limit, [2, 0, 5, 0]);
            with_contract(limit, [2, 0, 5, 0], |contract| {
                let mut checked = contract.check_budget_trace(
                    recording.session,
                    recording.attempts.clone(),
                    settings.limits,
                    &settings.host_work,
                ).unwrap();
                assert_eq!(checked.total(), recording.used);
                for attempt in recording.attempts.iter().rev() {
                    let decision = checked.consume(
                        &attempt.occurrence, &attempt.observation, &settings.host_work,
                    ).unwrap();
                    assert_eq!(
                        matches!(decision, super::super::native_phlo_rules::NativeBudgetReplayDecision::Accepted { .. }),
                        attempt.granted,
                    );
                }
                assert_eq!(checked.finish().unwrap(), recording.used);
            });
        }
    }
}

#[path = "tests/recording.rs"]
mod recording_tests;

#[path = "tests/operations.rs"]
mod operation_tests;

#[path = "tests/operation_sources.rs"]
mod operation_source_tests;

#[path = "tests/observation_construction.rs"]
mod observation_construction_tests;
