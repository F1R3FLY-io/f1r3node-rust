use loom::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use loom::sync::Arc;
use loom::thread;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/recovery_signal_state.rs"]
mod recovery_signal_state;
use recovery_signal_state::{RecoverySignalState, RecoveryWake};

fn explore(test: impl Fn() + Send + Sync + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

#[test]
fn active_pass_handoff_retains_demand_from_parallel_producers() {
    explore(|| {
        let signal = Arc::new(RecoverySignalState::default());
        let producers = [false, true].map(|proposal| {
            let signal = signal.clone();
            thread::spawn(move || signal.request(proposal))
        });
        let mut successor = RecoveryWake::Idle.merge(signal.take());
        thread::yield_now();
        successor = successor.merge(signal.take());
        for producer in producers {
            producer.join().unwrap();
        }
        successor = successor.merge(signal.take());
        assert_eq!(successor, RecoveryWake::Work { proposal: true });
        assert_eq!(signal.take(), RecoveryWake::Idle);
    });
}

#[test]
fn stop_dominates_local_demand_during_concurrent_handoff() {
    explore(|| {
        let signal = Arc::new(RecoverySignalState::default());
        signal.request(false);
        let producing = signal.clone();
        let producer = thread::spawn(move || producing.request(true));
        let stopping = signal.clone();
        let stopper = thread::spawn(move || stopping.stop());
        let successor = RecoveryWake::Idle.merge(signal.take());
        producer.join().unwrap();
        stopper.join().unwrap();
        assert_eq!(successor.merge(signal.take()), RecoveryWake::Stopped);
        assert!(!signal.request(true));
    });
}

#[test]
fn simultaneous_requests_coalesce_without_losing_proposal() {
    explore(|| {
        let state = Arc::new(RecoverySignalState::default());
        let threads = [false, true].map(|proposal| {
            let state = state.clone();
            thread::spawn(move || state.request(proposal))
        });
        let notified = threads
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>();
        assert_eq!(notified, 1);
        assert_eq!(state.take(), RecoveryWake::Work { proposal: true });
        assert_eq!(state.take(), RecoveryWake::Idle);
    });
}

#[test]
fn stop_cannot_be_overwritten_by_racing_requests_or_takes() {
    explore(|| {
        let state = Arc::new(RecoverySignalState::default());
        let producer_state = state.clone();
        let producer = thread::spawn(move || {
            producer_state.request(true);
            producer_state.request(false);
        });
        let stopper_state = state.clone();
        let stopper = thread::spawn(move || stopper_state.stop());
        let _ = state.take();
        producer.join().unwrap();
        assert!(stopper.join().unwrap());
        assert!(state.is_stopped());
        assert!(!state.request(true));
        assert!(!state.stop());
        assert_eq!(state.take(), RecoveryWake::Stopped);
    });
}

#[test]
fn producer_racing_with_take_is_observed_in_this_or_next_take() {
    explore(|| {
        let state = Arc::new(RecoverySignalState::default());
        assert!(state.request(false));
        let producer_state = state.clone();
        let producer = thread::spawn(move || producer_state.request(true));
        let first = state.take();
        producer.join().unwrap();
        let second = state.take();
        assert!(matches!(first, RecoveryWake::Work { .. }));
        assert!(
            first == RecoveryWake::Work { proposal: true }
                || second == RecoveryWake::Work { proposal: true }
        );
        assert_eq!(state.take(), RecoveryWake::Idle);
    });
}

#[test]
fn notification_survives_request_between_idle_check_and_wait() {
    explore(|| {
        let state = Arc::new(RecoverySignalState::default());
        let notice = Arc::new(AtomicBool::new(false));
        let producer_state = state.clone();
        let producer_notice = notice.clone();
        let producer = thread::spawn(move || {
            let required = producer_state.request(false);
            if required {
                producer_notice.swap(true, Ordering::SeqCst);
            }
            required
        });
        let first = state.take();
        thread::yield_now();
        let woke = notice
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        let second = if woke {
            state.take()
        } else {
            RecoveryWake::Idle
        };
        let required = producer.join().unwrap();
        let final_notice = notice.load(Ordering::SeqCst);
        let remaining = state.take();
        assert!(
            matches!(first, RecoveryWake::Work { .. })
                || matches!(second, RecoveryWake::Work { .. })
                || final_notice,
            "first={first:?}, woke={woke}, second={second:?}, final_notice={final_notice}, remaining={remaining:?}, required={required}"
        );
    });
}
