use std::sync::atomic::{AtomicU8, Ordering};

use proptest::prelude::*;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/recovery_signal_state.rs"]
mod recovery_signal_state;
#[path = "../../../../casper/src/rust/blocks/block_processing_queue/recovery_pass.rs"]
mod recovery_pass;
use recovery_pass::RecoveryPass;
use recovery_signal_state::{RecoverySignalState, RecoveryWake};

proptest! {
    #[test]
    fn arbitrary_handoff_partitions_preserve_requests_and_stop(
        groups in prop::collection::vec((prop::collection::vec(any::<bool>(), 0..32), any::<bool>()), 0..64),
    ) {
        let signal = RecoverySignalState::default();
        let mut successor = RecoveryWake::Idle;
        let mut requests = Vec::new();
        let mut stopped = false;
        for (group, stop) in groups {
            for proposal in group {
                signal.request(proposal);
                if !stopped { requests.push(proposal); }
            }
            if stop { signal.stop(); stopped = true; }
            let received = signal.take();
            let after_take = signal.take();
            prop_assert_eq!(after_take, if stopped { RecoveryWake::Stopped } else { RecoveryWake::Idle });
            successor = successor.merge(received);
            let expected = if stopped { RecoveryWake::Stopped }
                else if requests.is_empty() { RecoveryWake::Idle }
                else { RecoveryWake::Work { proposal: requests.iter().any(|value| *value) } };
            prop_assert_eq!(successor, expected);
            prop_assert_eq!(successor.merge(after_take), successor);
        }
    }

    #[test]
    fn signal_histories_match_stop_preserving_reference(operations in prop::collection::vec((0_u8..3, any::<bool>()), 0..1024)) {
        let signal = RecoverySignalState::default();
        let mut stopped = false;
        let mut requests = Vec::new();
        for (operation, proposal) in operations {
            match operation {
                0 => {
                    let expected_notice = !stopped && requests.is_empty();
                    prop_assert_eq!(signal.request(proposal), expected_notice);
                    if !stopped { requests.push(proposal); }
                }
                1 => {
                    let expected = if stopped { RecoveryWake::Stopped }
                        else if requests.is_empty() { RecoveryWake::Idle }
                        else { RecoveryWake::Work { proposal: requests.iter().any(|request| *request) } };
                    prop_assert_eq!(signal.take(), expected);
                    requests.clear();
                }
                _ => {
                    prop_assert_eq!(signal.stop(), !stopped);
                    stopped = true;
                    requests.clear();
                }
            }
            prop_assert_eq!(signal.is_stopped(), stopped);
        }
    }

    #[test]
    fn page_partitions_preserve_visits_and_error_history(
        errors in prop::collection::vec(any::<bool>(), 0..512),
        page in 1_usize..128,
        proposal in any::<bool>(),
    ) {
        let mut pass = RecoveryPass::new(errors.len(), proposal);
        let mut visited = 0;
        let mut any_error = false;
        for chunk in errors.chunks(page) {
            prop_assert!(!pass.proposal_ready());
            for &error in chunk {
                prop_assert!(pass.visit());
                visited += 1;
                if error { pass.fail(); }
                any_error |= error;
                prop_assert_eq!(pass.remaining() + visited, errors.len());
                if pass.remaining() != 0 || any_error { prop_assert!(!pass.proposal_ready()); }
            }
        }
        prop_assert_eq!(pass.remaining(), 0);
        prop_assert_eq!(pass.proposal_ready(), proposal && !any_error);
        prop_assert!(!pass.visit());
    }

    #[test]
    fn pass_counter_accepts_full_machine_range(candidates in any::<usize>(), visits in 0_usize..1024) {
        let mut pass = RecoveryPass::new(candidates, true);
        for visited in 0..visits {
            prop_assert_eq!(pass.visit(), visited < candidates);
            prop_assert_eq!(pass.remaining(), candidates.saturating_sub(visited + 1));
        }
    }
}

#[test]
fn wake_merge_exhaustively_matches_the_proven_algebra() {
    let states = [
        RecoveryWake::Idle,
        RecoveryWake::Work { proposal: false },
        RecoveryWake::Work { proposal: true },
        RecoveryWake::Stopped,
    ];
    for left in states {
        assert_eq!(left.merge(RecoveryWake::Idle), left);
        assert_eq!(RecoveryWake::Idle.merge(left), left);
        assert_eq!(left.merge(left), left);
        assert_eq!(left.merge(RecoveryWake::Stopped), RecoveryWake::Stopped);
        assert_eq!(RecoveryWake::Stopped.merge(left), RecoveryWake::Stopped);
        for right in states {
            assert_eq!(left.merge(right), right.merge(left));
            let signal = RecoverySignalState::default();
            match right {
                RecoveryWake::Idle => {}
                RecoveryWake::Work { proposal } => {
                    signal.request(proposal);
                }
                RecoveryWake::Stopped => {
                    signal.stop();
                }
            }
            let successor = left.merge(signal.take());
            assert_eq!(successor.merge(signal.take()), left.merge(right));
            for third in states {
                assert_eq!(
                    left.merge(right).merge(third),
                    left.merge(right.merge(third))
                );
            }
        }
    }
}

#[test]
fn a_capacity_wake_preserves_proposal_demand_after_a_failed_pass() {
    let signal = RecoverySignalState::default();
    let mut pass = RecoveryPass::new(1, false);
    signal.request(true);
    let successor = RecoveryWake::Idle.merge(signal.take());
    pass.fail();
    assert!(pass.visit());
    assert!(!pass.proposal_ready());
    signal.request(false);
    assert_eq!(successor.merge(signal.take()), RecoveryWake::Work {
        proposal: true
    });
}

#[test]
fn later_page_error_and_blocked_pass_never_authorize_proposal() {
    let mut pass = RecoveryPass::new(3, true);
    assert!(pass.visit());
    assert!(!pass.proposal_ready());
    assert_eq!(pass.remaining(), 2);
    assert!(pass.visit());
    pass.fail();
    assert!(pass.visit());
    assert!(!pass.proposal_ready());
}
