use super::*;

fn trace_limits() -> NativeBudgetTraceLimits {
    NativeBudgetTraceLimits {
        attempts: 1024,
        path_segments: 128,
        regions: limits(),
    }
}

fn attempt(id: u64, charge: u64, granted: bool) -> NativeBudgetAttempt {
    NativeBudgetAttempt {
        occurrence: NativeBudgetOccurrence {
            session: [1; 32],
            path: vec![(id, 0)],
            stage: NativeAttemptStage::ProduceIntroduction,
        },
        observation: row(1, 1, false, [charge, 0, 0]),
        granted,
    }
}

#[test]
fn denied_attempt_keeps_its_original_prefix_decision_after_reordering() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![
            attempt(0, 4, true),
            attempt(1, 2, false),
            attempt(2, 1, true),
        ]
        .into();
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget())
            .unwrap();
        assert_eq!(checked.total(), 5);
        assert!(matches!(
            checked.finish(),
            Err(NativeBudgetTraceError::Incomplete)
        ));
        for index in [1, 2, 0] {
            let entry = &rows[index];
            let decision = checked
                .consume(&entry.occurrence, &entry.observation, &budget())
                .unwrap();
            assert_eq!(
                matches!(decision, NativeBudgetReplayDecision::Accepted { .. }),
                entry.granted
            );
        }
        assert_eq!(checked.finish().unwrap(), 5);
        assert_eq!(checked.remaining(), 0);
    });
}

#[test]
fn forged_denials_and_duplicate_occurrences_are_rejected() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        for charge in [0, 1, 5] {
            let rows = vec![attempt(0, charge, false)].into();
            assert!(matches!(
                contract.check_budget_trace([1; 32], rows, trace_limits(), &budget()),
                Err(NativeBudgetTraceError::Decision)
            ));
        }
        let entry = attempt(0, 1, true);
        let rows = vec![entry.clone(), entry].into();
        assert!(matches!(
            contract.check_budget_trace([1; 32], rows, trace_limits(), &budget()),
            Err(NativeBudgetTraceError::Duplicate)
        ));
    });
}

#[test]
fn exact_observation_and_session_are_required_without_consuming_on_error() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true)].into();
        assert!(matches!(
            contract.check_budget_trace([2; 32], rows.clone(), trace_limits(), &budget()),
            Err(NativeBudgetTraceError::Session)
        ));
        let mut wrong_stage = rows[0].clone();
        wrong_stage.occurrence.stage = NativeAttemptStage::Comm;
        assert!(matches!(
            contract.check_budget_trace(
                [1; 32],
                vec![wrong_stage].into(),
                trace_limits(),
                &budget()
            ),
            Err(NativeBudgetTraceError::Stage)
        ));
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget())
            .unwrap();
        let mut altered = rows[0].observation.as_ref().clone();
        altered.event_id[0] ^= 1;
        assert!(matches!(
            checked.consume(&rows[0].occurrence, &altered, &budget()),
            Err(NativeBudgetTraceError::Observation)
        ));
        let mut missing = rows[0].occurrence.clone();
        missing.path.push((1, 1));
        assert!(matches!(
            checked.consume(&missing, &rows[0].observation, &budget()),
            Err(NativeBudgetTraceError::Unknown)
        ));
        assert_eq!(checked.remaining(), 1);
        checked
            .consume(&rows[0].occurrence, &rows[0].observation, &budget())
            .unwrap();
        assert!(matches!(
            checked.consume(&rows[0].occurrence, &rows[0].observation, &budget()),
            Err(NativeBudgetTraceError::Consumed)
        ));
        assert_eq!(checked.finish().unwrap(), 1);
    });
}

#[test]
fn full_width_addition_and_preparation_overflow_are_denials() {
    with_contract([0, 1, 0, 0], u64::MAX, u64::MAX, 0, |contract| {
        let rows: Arc<[_]> = vec![
            attempt(0, u64::MAX, true),
            attempt(1, 1, false),
            attempt(2, 0, true),
        ]
        .into();
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget())
            .unwrap();
        for index in [2, 1, 0] {
            checked
                .consume(&rows[index].occurrence, &rows[index].observation, &budget())
                .unwrap();
        }
        assert_eq!(checked.finish().unwrap(), u64::MAX);
    });
    with_contract([0, 2, 0, 0], u64::MAX, u64::MAX, 0, |contract| {
        let entry = attempt(0, u64::MAX, false);
        let mut checked = contract
            .check_budget_trace(
                [1; 32],
                vec![entry.clone()].into(),
                trace_limits(),
                &budget(),
            )
            .unwrap();
        assert_eq!(
            checked
                .consume(&entry.occurrence, &entry.observation, &budget())
                .unwrap(),
            NativeBudgetReplayDecision::Denied
        );
        assert_eq!(checked.finish().unwrap(), 0);
        let forged = NativeBudgetAttempt {
            granted: true,
            ..entry
        };
        assert!(matches!(
            contract.check_budget_trace([1; 32], vec![forged].into(), trace_limits(), &budget()),
            Err(NativeBudgetTraceError::Decision)
        ));
    });
}

#[test]
fn trace_limits_and_host_rejection_apply_before_replay() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true)].into();
        for restricted in [
            NativeBudgetTraceLimits {
                attempts: 0,
                ..trace_limits()
            },
            NativeBudgetTraceLimits {
                path_segments: 0,
                ..trace_limits()
            },
        ] {
            assert!(matches!(
                contract.check_budget_trace([1; 32], rows.clone(), restricted, &budget()),
                Err(NativeBudgetTraceError::Limit)
            ));
        }
        let denied_budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        assert!(contract
            .check_budget_trace([1; 32], Arc::from([]), trace_limits(), &denied_budget)
            .is_err());
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget())
            .unwrap();
        assert!(checked
            .consume(&rows[0].occurrence, &rows[0].observation, &denied_budget)
            .is_err());
        assert_eq!(checked.remaining(), 1);
    });
}

proptest! {
    #[test]
    fn checked_prefix_properties_survive_arbitrary_replay_permutations(
        limit in any::<u64>(),
        inputs in prop::collection::vec((any::<u64>(), any::<u64>()), 0..48),
    ) {
        with_contract([0, 1, 0, 0], limit, limit, 0, |contract| {
            let mut expected = 0_u128;
            let rows: Arc<[_]> = inputs.iter().enumerate().map(|(id, (charge, _))| {
                let next = expected + u128::from(*charge);
                let granted = next <= u128::from(limit);
                if granted { expected = next; }
                attempt(id as u64, *charge, granted)
            }).collect::<Vec<_>>().into();
            let mut checked = contract.check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget()).unwrap();
            let mut order = (0..rows.len()).collect::<Vec<_>>();
            order.sort_by_key(|index| (inputs[*index].1, *index));
            for index in order {
                let entry = &rows[index];
                let decision = checked.consume(&entry.occurrence, &entry.observation, &budget()).unwrap();
                assert_eq!(matches!(decision, NativeBudgetReplayDecision::Accepted { .. }), entry.granted);
            }
            assert_eq!(u128::from(checked.finish().unwrap()), expected);
            for index in 0..rows.len() {
                let mut corrupted = rows.to_vec();
                corrupted[index].granted = !corrupted[index].granted;
                assert!(matches!(contract.check_budget_trace([1; 32], corrupted.into(), trace_limits(), &budget()), Err(NativeBudgetTraceError::Decision)));
            }
        });
    }
}

#[test]
fn host_rejection_during_indexing_and_observation_comparison_is_atomic() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![
            attempt(2, 1, true),
            attempt(1, 1, true),
            attempt(0, 1, true),
        ]
        .into();
        let mut indexing_limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
        indexing_limits.set(
            HostWorkDimension::VerificationOperations,
            HostWorkLimit::new(103),
        );
        let indexing_budget = HostWorkBudget::new(indexing_limits);
        assert!(matches!(
            contract.check_budget_trace([1; 32], rows.clone(), trace_limits(), &indexing_budget),
            Err(NativeBudgetTraceError::Work(_))
        ));
        assert_eq!(
            indexing_budget
                .usage(HostWorkDimension::VerificationOperations)
                .get(),
            103
        );
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget())
            .unwrap();
        let mut comparison_limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
        comparison_limits.set(HostWorkDimension::VerificationBytes, HostWorkLimit::new(0));
        let comparison_budget = HostWorkBudget::new(comparison_limits);
        assert!(matches!(
            checked.consume(
                &rows[0].occurrence,
                &rows[0].observation,
                &comparison_budget
            ),
            Err(NativeBudgetTraceError::Work(_))
        ));
        assert!(comparison_budget.is_rejected());
        assert_eq!(checked.remaining(), rows.len());
        for entry in rows.iter() {
            checked
                .consume(&entry.occurrence, &entry.observation, &budget())
                .unwrap();
        }
        assert_eq!(checked.finish().unwrap(), 3);
    });
}

#[test]
fn equal_scalar_cost_does_not_allow_observation_substitution() {
    with_contract([0, 0, 0, 0], 0, 0, 0, |contract| {
        let original = attempt(0, 7, true);
        let mut checked = contract
            .check_budget_trace(
                [1; 32],
                vec![original.clone()].into(),
                trace_limits(),
                &budget(),
            )
            .unwrap();
        let observation = original.observation.as_ref();
        let mut mutations = vec![observation.clone(); 6];
        mutations[0].event_id[0] ^= 1;
        mutations[1].kind = AuthorityByteEventKind::ConsumeIntroduction;
        mutations[2]
            .measurement
            .as_mut()
            .unwrap()
            .introduction_bytes += 1;
        mutations[3].authority.regions[0].instance_id[0] ^= 1;
        mutations[4].authority.regions[0]
            .signature
            .as_mut()
            .unwrap()
            .value = Some(Value::Ground(vec![8]));
        mutations[5].legacy_amount = Some(0);
        for altered in mutations {
            assert_eq!(
                contract
                    .prepare(Arc::new(altered.clone()), limits(), &budget())
                    .unwrap()
                    .usage(),
                0
            );
            assert!(matches!(
                checked.consume(&original.occurrence, &altered, &budget()),
                Err(NativeBudgetTraceError::Observation)
            ));
            assert_eq!(checked.remaining(), 1);
        }
        checked
            .consume(&original.occurrence, observation, &budget())
            .unwrap();
        assert_eq!(checked.finish().unwrap(), 0);
    });
}

#[test]
fn loom_parallel_replay_keeps_the_checked_decisions_and_usage() {
    let (rows, checked) = with_contract([0, 1, 0, 0], 1, 1, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true), attempt(1, 1, false)].into();
        let checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits(), &budget())
            .unwrap();
        (rows, checked)
    });
    loom::model(move || {
        let shared = loom::sync::Arc::new(loom::sync::Mutex::new(checked.clone()));
        let workers = rows
            .iter()
            .cloned()
            .map(|entry| {
                let shared = shared.clone();
                loom::thread::spawn(move || {
                    let decision = shared
                        .lock()
                        .unwrap()
                        .consume(&entry.occurrence, &entry.observation, &budget())
                        .unwrap();
                    assert_eq!(
                        matches!(decision, NativeBudgetReplayDecision::Accepted { .. }),
                        entry.granted
                    );
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(shared.lock().unwrap().finish().unwrap(), 1);
    });
}
