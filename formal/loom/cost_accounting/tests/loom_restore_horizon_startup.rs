use loom::sync::{Arc, RwLock};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Raw,
    Running,
}

#[derive(Clone, Copy)]
struct StartupState {
    phase: Phase,
    latest: usize,
}

#[test]
fn running_snapshot_cannot_observe_a_stale_latest_message() {
    loom::model(|| {
        let state = Arc::new(RwLock::new(StartupState {
            phase: Phase::Raw,
            latest: 1,
        }));
        let reconcile = {
            let state = state.clone();
            thread::spawn(move || {
                let mut state = state.write().unwrap();
                state.latest = 2;
                state.phase = Phase::Running;
            })
        };
        let capture = {
            let state = state.clone();
            thread::spawn(move || {
                let state = state.read().unwrap();
                if state.phase == Phase::Running {
                    assert_eq!(state.latest, 2);
                }
            })
        };

        reconcile.join().unwrap();
        capture.join().unwrap();
        let state = state.read().unwrap();
        assert_eq!(state.phase, Phase::Running);
        assert_eq!(state.latest, 2);
    });
}

#[test]
fn concurrent_captures_observe_one_published_startup_state() {
    loom::model(|| {
        let state = Arc::new(RwLock::new(StartupState {
            phase: Phase::Raw,
            latest: 1,
        }));
        let reconcile = {
            let state = state.clone();
            thread::spawn(move || {
                let mut state = state.write().unwrap();
                state.latest = 2;
                state.phase = Phase::Running;
            })
        };
        let capture = |state: Arc<RwLock<StartupState>>| {
            thread::spawn(move || {
                let state = state.read().unwrap();
                assert!(
                    (state.phase == Phase::Raw && state.latest == 1)
                        || (state.phase == Phase::Running && state.latest == 2)
                );
            })
        };
        let first = capture(state.clone());
        let second = capture(state.clone());

        reconcile.join().unwrap();
        first.join().unwrap();
        second.join().unwrap();
    });
}
