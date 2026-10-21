use super::*;
use crate::rust::interpreter::accounting::native_runtime::{NativePathTrie, PathId};

/// The node of the one-segment path [(id, 0)] in a fixture trie (C7b,
/// DR-86).
fn fixture_path(id: u64) -> PathId {
    static PATHS: std::sync::LazyLock<NativePathTrie> = std::sync::LazyLock::new(|| {
        let mut paths = NativePathTrie::new(128);
        for id in 0..64 {
            paths
                .intern(PathId::ROOT, &[(id, 0)])
                .expect("fixture path fits");
        }
        paths
    });
    PATHS
        .find(&[(id, 0)])
        .expect("fixture paths cover ids below 64")
}

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
            path: fixture_path(id),
            stage: NativeAttemptStage::ProduceIntroduction,
        },
        observation: row(1, 1, false, [charge, 0, 0]),
        granted,
        cause: NativeDenialCause::Budget,
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
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        assert_eq!(checked.total(), 5);
        assert!(matches!(
            checked.finish(),
            Err(NativeBudgetTraceError::Incomplete)
        ));
        for index in [1, 2, 0] {
            let entry = &rows[index];
            let decision = checked
                .consume(&entry.occurrence, &entry.observation)
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
                contract.check_budget_trace([1; 32], rows, trace_limits()),
                Err(NativeBudgetTraceError::Decision)
            ));
        }
        let entry = attempt(0, 1, true);
        let rows = vec![entry.clone(), entry].into();
        assert!(matches!(
            contract.check_budget_trace([1; 32], rows, trace_limits()),
            Err(NativeBudgetTraceError::Duplicate)
        ));
    });
}

#[test]
fn exact_observation_and_session_are_required_without_consuming_on_error() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true)].into();
        assert!(matches!(
            contract.check_budget_trace([2; 32], rows.clone(), trace_limits()),
            Err(NativeBudgetTraceError::Session)
        ));
        let mut wrong_stage = rows[0].clone();
        wrong_stage.occurrence.stage = NativeAttemptStage::Comm;
        assert!(matches!(
            contract.check_budget_trace([1; 32], vec![wrong_stage].into(), trace_limits()),
            Err(NativeBudgetTraceError::Stage)
        ));
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        let mut altered = rows[0].observation.as_ref().clone();
        altered.event_id[0] ^= 1;
        assert!(matches!(
            checked.consume(&rows[0].occurrence, &altered),
            Err(NativeBudgetTraceError::Observation)
        ));
        let mut missing = rows[0].occurrence.clone();
        missing.path = fixture_path(63);
        assert!(matches!(
            checked.consume(&missing, &rows[0].observation),
            Err(NativeBudgetTraceError::Unknown)
        ));
        assert_eq!(checked.remaining(), 1);
        checked
            .consume(&rows[0].occurrence, &rows[0].observation)
            .unwrap();
        assert!(matches!(
            checked.consume(&rows[0].occurrence, &rows[0].observation),
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
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        for index in [2, 1, 0] {
            checked
                .consume(&rows[index].occurrence, &rows[index].observation)
                .unwrap();
        }
        assert_eq!(checked.finish().unwrap(), u64::MAX);
    });
    with_contract([0, 2, 0, 0], u64::MAX, u64::MAX, 0, |contract| {
        let entry = attempt(0, u64::MAX, false);
        let mut checked = contract
            .check_budget_trace([1; 32], vec![entry.clone()].into(), trace_limits())
            .unwrap();
        assert_eq!(
            checked
                .consume(&entry.occurrence, &entry.observation)
                .unwrap(),
            NativeBudgetReplayDecision::Denied
        );
        assert_eq!(checked.finish().unwrap(), 0);
        let forged = NativeBudgetAttempt {
            granted: true,
            ..entry
        };
        assert!(matches!(
            contract.check_budget_trace([1; 32], vec![forged].into(), trace_limits()),
            Err(NativeBudgetTraceError::Decision)
        ));
    });
}

#[test]
fn trace_limits_apply_before_replay() {
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
                contract.check_budget_trace([1; 32], rows.clone(), restricted),
                Err(NativeBudgetTraceError::Limit)
            ));
        }
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
            let mut checked = contract.check_budget_trace([1; 32], rows.clone(), trace_limits()).unwrap();
            let mut order = (0..rows.len()).collect::<Vec<_>>();
            order.sort_by_key(|index| (inputs[*index].1, *index));
            for index in order {
                let entry = &rows[index];
                let decision = checked.consume(&entry.occurrence, &entry.observation).unwrap();
                assert_eq!(matches!(decision, NativeBudgetReplayDecision::Accepted { .. }), entry.granted);
            }
            assert_eq!(u128::from(checked.finish().unwrap()), expected);
            for index in 0..rows.len() {
                let mut corrupted = rows.to_vec();
                corrupted[index].granted = !corrupted[index].granted;
                assert!(matches!(contract.check_budget_trace([1; 32], corrupted.into(), trace_limits()), Err(NativeBudgetTraceError::Decision)));
            }
        });
    }
}

#[test]
fn equal_scalar_cost_does_not_allow_observation_substitution() {
    with_contract([0, 0, 0, 0], 0, 0, 0, |contract| {
        let original = attempt(0, 7, true);
        let mut checked = contract
            .check_budget_trace([1; 32], vec![original.clone()].into(), trace_limits())
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
                    .prepare(Arc::new(altered.clone()), limits())
                    .unwrap()
                    .usage(),
                0
            );
            assert!(matches!(
                checked.consume(&original.occurrence, &altered),
                Err(NativeBudgetTraceError::Observation)
            ));
            assert_eq!(checked.remaining(), 1);
        }
        checked.consume(&original.occurrence, observation).unwrap();
        assert_eq!(checked.finish().unwrap(), 0);
    });
}

#[test]
fn loom_parallel_replay_keeps_the_checked_decisions_and_usage() {
    let (rows, checked) = with_contract([0, 1, 0, 0], 1, 1, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true), attempt(1, 1, false)].into();
        let checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
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
                        .consume(&entry.occurrence, &entry.observation)
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
