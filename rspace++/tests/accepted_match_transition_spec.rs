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
