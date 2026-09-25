use futures::FutureExt;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::*;

fn channel(id: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(id)),
    }])
}

fn scope<T>(budget: &RuntimeBudget, index: usize, action: impl FnOnce() -> T) -> T {
    operation_context::scope(
        OperationOrder {
            session: budget.deploy_id(),
            path: vec![(index as u64, 0)].into(),
        },
        async { action() },
    )
    .now_or_never()
    .unwrap()
}

fn introduce(budget: &RuntimeBudget, source: &Produce) {
    budget
        .reserve_produce_introduction_measured(
            source.hash.0.as_slice().try_into().unwrap(),
            &authority(1),
            ByteCharge {
                introduction_bytes: 0,
                transfer_bytes: 1,
                trace_bytes: 0,
            },
            source.persistent,
        )
        .unwrap();
}

#[test]
fn native_consume_metadata_rejects_invalid_indexes_and_repeated_capture_before_debit() {
    for invalid in [vec![-1], vec![1], vec![0]] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(100, [0, 0, 1, 0]))
            .unwrap();
        let _accounting = budget.enter_comm_accounting_scope();
        let channels = vec![channel(0)];
        let source = Consume::create(&channels, &vec![0u8], &7u8, false);
        scope(&budget, 0, || {
            budget
                .start_native_operation(RSpaceOperationSource::Consume(&source), &channels, &[])
                .unwrap();
            if invalid == [0] {
                budget
                    .observe_native_consume_peeks(&std::collections::BTreeSet::new())
                    .unwrap();
            }
            assert!(budget
                .observe_native_consume_peeks(&invalid.into_iter().collect())
                .is_err());
        });
        assert_eq!(budget.native_phlo_usage(), Some(0));
        assert!(budget.native_operation_recording().is_err());
    }
}

#[test]
fn native_operation_recording_rejects_incomplete_duplicate_and_mismatched_lifecycles() {
    for failure in 0..7 {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(100, [0, 0, 1, 0]))
            .unwrap();
        let _accounting = budget.enter_comm_accounting_scope();
        let channel = channel(1);
        let source = Produce::create(&channel, &7u8, false);
        scope(&budget, 0, || {
            let reference = RSpaceOperationSource::Produce(&source);
            budget
                .start_native_operation(reference, std::slice::from_ref(&channel), &[])
                .unwrap();
            assert!(budget.native_operation_recording().is_err());
            if failure == 0 {
                return;
            }
            if failure == 1 {
                assert!(budget.start_native_operation(reference, &[], &[]).is_err());
                return;
            }
            if failure != 2 {
                introduce(&budget, &source);
            }
            let mut changed = source.clone();
            changed.persistent = !changed.persistent;
            let final_source = if failure == 3 {
                RSpaceOperationSource::Produce(&changed)
            } else {
                reference
            };
            budget.finish_native_operation(
                final_source,
                if failure == 4 {
                    RSpaceOperationCompletion::Matched
                } else if failure == 6 {
                    RSpaceOperationCompletion::Rejected
                } else {
                    RSpaceOperationCompletion::Stored
                },
            );
            if failure == 5 {
                assert!(budget.native_operation_recording().is_ok());
                budget.finish_native_operation(reference, RSpaceOperationCompletion::Stored);
            }
        });
        assert!(
            budget.native_operation_recording().is_err(),
            "failure case {failure}"
        );
    }
}

#[test]
fn native_operation_recording_allows_independent_operations_to_overlap() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(100, [0, 0, 1, 0]))
        .unwrap();
    let _accounting = budget.enter_comm_accounting_scope();
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|threads| {
        for index in 0..4 {
            let budget = &budget;
            let barrier = &barrier;
            threads.spawn(move || {
                scope(budget, index, || {
                    let channel = channel(index as i64);
                    let source = Produce::create(&channel, &7u8, false);
                    let reference = RSpaceOperationSource::Produce(&source);
                    budget
                        .start_native_operation(reference, &[channel], &[])
                        .unwrap();
                    introduce(budget, &source);
                    barrier.wait();
                    budget.finish_native_operation(reference, RSpaceOperationCompletion::Stored);
                })
            });
        }
    });
    let records = budget.native_operation_recording().unwrap().unwrap();
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|row| row.predecessors.is_empty()));
    assert!(records.iter().all(|row| row.budget_end == 4));
    assert_eq!(budget.native_phlo_usage(), Some(4));
}

#[test]
fn native_operation_recording_rejects_overlapping_conflicting_operations() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(100, [0, 0, 1, 0]))
        .unwrap();
    let _accounting = budget.enter_comm_accounting_scope();
    let channel = channel(1);
    let source = Produce::create(&channel, &7u8, false);
    scope(&budget, 0, || {
        budget
            .start_native_operation(
                RSpaceOperationSource::Produce(&source),
                std::slice::from_ref(&channel),
                &[],
            )
            .unwrap();
        introduce(&budget, &source);
    });
    scope(&budget, 1, || {
        assert!(budget
            .start_native_operation(RSpaceOperationSource::Produce(&source), &[channel], &[])
            .is_err());
    });
    assert!(budget.native_operation_recording().is_err());
    assert_eq!(budget.native_phlo_usage(), Some(1));
}

#[test]
fn native_operation_completion_accepts_exactly_model_outcomes() {
    for intro in [None, Some(false), Some(true)] {
        for comm in [None, Some(false), Some(true)] {
            if comm.is_some() && intro != Some(true) {
                continue;
            }
            for completion in [
                RSpaceOperationCompletion::Stored,
                RSpaceOperationCompletion::Matched,
                RSpaceOperationCompletion::Rejected,
            ] {
                let budget = RuntimeBudget::new(Cost::unsafe_max());
                let limit = u64::from(intro == Some(true));
                budget
                    .reset_for_native_execution(config(limit, [0, 0, 1, 0]))
                    .unwrap();
                let _accounting = budget.enter_comm_accounting_scope();
                let channel = channel(1);
                let source = Produce::create(&channel, &7u8, false);
                scope(&budget, 0, || {
                    let reference = RSpaceOperationSource::Produce(&source);
                    budget
                        .start_native_operation(reference, std::slice::from_ref(&channel), &[])
                        .unwrap();
                    if let Some(granted) = intro {
                        let result = budget.reserve_produce_introduction_measured(
                            source.hash.0.as_slice().try_into().unwrap(),
                            &authority(1),
                            ByteCharge {
                                introduction_bytes: 0,
                                transfer_bytes: 1,
                                trace_bytes: 0,
                            },
                            false,
                        );
                        assert_eq!(result.is_ok(), granted);
                    }
                    if let Some(granted) = comm {
                        let event = COMM {
                            consume: Consume::create(&vec![channel], &vec![0u8], &7u8, false),
                            produces: vec![source.clone()],
                            peeks: Default::default(),
                            times_repeated: Default::default(),
                        };
                        budget.observe_native_comm_source(&event).unwrap();
                        let result = budget.reserve_comm_authority_measured(
                            event.cost_identity().0.as_slice().try_into().unwrap(),
                            &authority(1),
                            ByteCharge {
                                introduction_bytes: 0,
                                transfer_bytes: u64::from(!granted),
                                trace_bytes: 0,
                            },
                        );
                        assert_eq!(result.is_ok(), granted);
                    }
                    budget.finish_native_operation(reference, completion);
                });
                let expected = match completion {
                    RSpaceOperationCompletion::Stored => intro == Some(true) && comm.is_none(),
                    RSpaceOperationCompletion::Matched => intro == Some(true) && comm == Some(true),
                    RSpaceOperationCompletion::Rejected => {
                        intro == Some(false) || comm == Some(false)
                    }
                };
                assert_eq!(
                    budget.native_operation_recording().is_ok(),
                    expected,
                    "intro={intro:?}, comm={comm:?}, completion={completion:?}"
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn native_operation_journal_matches_channel_dependencies_and_budget_cuts(
        footprints in prop::collection::vec(prop::collection::vec(0i64..8, 1..5), 1..32),
        persistent in any::<bool>(),
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget.reset_for_native_execution(config(100, [0, 0, 1, 0])).unwrap();
        let _accounting = budget.enter_comm_accounting_scope();
        let mut last = BTreeMap::new();
        let mut expected = Vec::new();
        let mut accepted = std::collections::BTreeSet::new();
        let mut fresh = 0;
        let mut retries = 0;
        for (index, ids) in footprints.iter().enumerate() {
            let channels: Vec<_> = ids.iter().copied().map(channel).collect();
            let source = Produce::create(&channels[0], &7u8, persistent);
            let predecessors = ids.iter().filter_map(|id| last.get(id).copied())
                .collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
            for id in ids {
                last.insert(*id, index);
            }
            let start = fresh;
            let link = if persistent && !accepted.insert(ids[0]) {
                let link = NativeObservationLink::Retry(retries);
                retries += 1;
                link
            } else {
                let link = NativeObservationLink::Attempt(fresh);
                fresh += 1;
                link
            };
            expected.push((predecessors, start, fresh, link));
            scope(&budget, index, || {
                let reference = RSpaceOperationSource::Produce(&source);
                budget.start_native_operation(reference, &channels[..1], &[channels.clone()]).unwrap();
                introduce(&budget, &source);
                budget.finish_native_operation(reference, RSpaceOperationCompletion::Stored);
            });
        }
        let records = budget.native_operation_recording().unwrap().unwrap();
        let charges = budget.native_budget_recording().unwrap().unwrap();
        with_contract(100, [0, 0, 1, 0], |contract| {
            let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
            let checked = contract.check_operation_journal(
                charges.session, &charges, Arc::clone(&records), journal_limits(), &host,
            ).unwrap();
            assert_eq!(checked.total(), charges.used);
        });
        prop_assert_eq!(records.len(), expected.len());
        prop_assert_eq!(charges.attempts.len(), fresh);
        prop_assert_eq!(charges.retries.len(), retries);
        for (index, (row, (predecessors, start, end, link))) in records.iter().zip(expected).enumerate() {
            prop_assert_eq!(row.predecessors.as_ref(), predecessors);
            prop_assert_eq!(row.budget_start, start);
            prop_assert_eq!(row.budget_end, end);
            prop_assert_eq!(row.introduction, link);
            prop_assert_eq!(row.occurrence.path.as_ref(), &[(index as u64, 0)]);
            prop_assert!(row.comm.is_none());
            prop_assert_eq!(row.completion, RSpaceOperationCompletion::Stored);
            prop_assert!(row.footprint.windows(2).all(|pair| pair[0] < pair[1]));
            for predecessor in row.predecessors.iter() {
                prop_assert!(records[*predecessor].budget_end <= row.budget_start);
            }
        }
    }
}
