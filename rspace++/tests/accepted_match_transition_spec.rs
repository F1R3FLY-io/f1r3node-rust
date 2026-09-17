#![cfg(feature = "accepted-match-transition-experiment")]

use rspace_plus_plus::rspace::hot_store::accepted_match_transition_experiment::{
    AcceptedMatchTransitionExperiment, ExperimentArm, ExperimentCase,
};

#[test]
fn every_registered_cell_has_an_equivalent_or_nonmutating_post_state() {
    for case in ExperimentCase::ALL {
        let audit = AcceptedMatchTransitionExperiment::audit(case);
        assert_eq!(audit.case, case);
    }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
#[test]
fn instrumentation_pins_write_locks_and_pathmap_root_publications() {
    use rspace_plus_plus::rspace::hot_store::accepted_match_transition_experiment::{
        instrumentation_begin, instrumentation_end,
    };

    for case in ExperimentCase::ALL {
        for arm in [ExperimentArm::FrozenSplit, ExperimentArm::Atomic] {
            if !case.supports(arm) {
                continue;
            }
            let mut fixture = AcceptedMatchTransitionExperiment::new(case);
            instrumentation_begin();
            fixture.execute(arm);
            let metrics = instrumentation_end();
            assert_eq!(metrics.write_locks, case.expected_write_locks(arm));
            assert_eq!(metrics.pathmap_root_publications, case.expected_root_publications(arm));
        }
    }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
#[test]
fn instrumentation_is_thread_local_to_the_measured_transition() {
    use rspace_plus_plus::rspace::hot_store::accepted_match_transition_experiment::{
        instrumentation_begin, instrumentation_end,
    };

    instrumentation_begin();
    let child = std::thread::spawn(|| {
        let audit = AcceptedMatchTransitionExperiment::audit(ExperimentCase::Singleton);
        assert_eq!(audit.case, ExperimentCase::Singleton);
    });
    child.join().expect("unmeasured audit thread must finish");
    let mut fixture = AcceptedMatchTransitionExperiment::new(ExperimentCase::Singleton);
    fixture.execute(ExperimentArm::Atomic);
    let metrics = instrumentation_end();
    assert_eq!(metrics.write_locks, 1);
    assert_eq!(metrics.pathmap_root_publications, 1);
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
#[test]
fn concurrent_instrumentation_sessions_keep_independent_counts() {
    use std::sync::{Arc, Barrier};

    use rspace_plus_plus::rspace::hot_store::accepted_match_transition_experiment::{
        instrumentation_begin, instrumentation_end,
    };

    let barrier = Arc::new(Barrier::new(2));
    let handles = [ExperimentCase::Singleton, ExperimentCase::Repeated2].map(|case| {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            instrumentation_begin();
            barrier.wait();
            let mut fixture = AcceptedMatchTransitionExperiment::new(case);
            fixture.execute(ExperimentArm::Atomic);
            let metrics = instrumentation_end();
            (case, metrics)
        })
    });
    for handle in handles {
        let (case, metrics) = handle.join().expect("measured session thread must finish");
        assert_eq!(metrics.write_locks, case.expected_write_locks(ExperimentArm::Atomic));
        assert_eq!(
            metrics.pathmap_root_publications,
            case.expected_root_publications(ExperimentArm::Atomic)
        );
    }
}
