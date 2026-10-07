use loom::sync::{Arc, Mutex};
use loom::thread;

const IDLE: usize = 0;
const RESTORING: usize = 1;
const RUNNING: usize = 2;
const TERMINAL: usize = 3;
const MAX_FAILURES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RestoreLease(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureDisposition {
    Retry,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RestoreState {
    phase: usize,
    failures: usize,
    generation: usize,
    channels_ready: bool,
    estimator_available: bool,
    terminal_publications: usize,
}

impl RestoreState {
    fn new() -> Self {
        Self {
            phase: IDLE,
            failures: 0,
            generation: 0,
            channels_ready: true,
            estimator_available: true,
            terminal_publications: 0,
        }
    }

    fn begin(&mut self) -> Option<RestoreLease> {
        if self.phase != IDLE || !self.channels_ready {
            return None;
        }
        self.generation += 1;
        self.phase = RESTORING;
        self.channels_ready = false;
        Some(RestoreLease(self.generation))
    }

    fn record_failure(&mut self, lease: RestoreLease) -> Option<FailureDisposition> {
        if self.phase != RESTORING || self.generation != lease.0 {
            return None;
        }
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            self.phase = TERMINAL;
            self.terminal_publications += 1;
            Some(FailureDisposition::Terminal)
        } else {
            Some(FailureDisposition::Retry)
        }
    }

    fn release_retry(&mut self, lease: RestoreLease) -> bool {
        if self.phase != RESTORING || self.generation != lease.0 || self.failures >= MAX_FAILURES {
            return false;
        }
        self.channels_ready = true;
        self.phase = IDLE;
        true
    }

    fn commit_running(&mut self, lease: RestoreLease) -> bool {
        if self.phase != RESTORING || self.generation != lease.0 {
            return false;
        }
        self.estimator_available = false;
        self.phase = RUNNING;
        true
    }

    fn terminate_active(&mut self, lease: RestoreLease) -> bool {
        if self.phase != RESTORING || self.generation != lease.0 {
            return false;
        }
        self.phase = TERMINAL;
        self.terminal_publications += 1;
        true
    }

    fn terminate_retry_request(&mut self, lease: RestoreLease) -> bool {
        if self.phase != IDLE || self.generation != lease.0 {
            return false;
        }
        self.channels_ready = false;
        self.phase = TERMINAL;
        self.terminal_publications += 1;
        true
    }
}

fn release_after_failure(state: &mut RestoreState, lease: RestoreLease) {
    assert_eq!(state.record_failure(lease), Some(FailureDisposition::Retry));
    assert!(state.release_retry(lease));
}

#[test]
fn duplicate_approved_blocks_have_one_restore_owner() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        let left = state.clone();
        let right = state.clone();

        let first = thread::spawn(move || left.lock().unwrap().begin());
        let second = thread::spawn(move || right.lock().unwrap().begin());

        let leases = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(leases.iter().filter(|lease| lease.is_some()).count(), 1);
        let final_state = state.lock().unwrap();
        assert_eq!(final_state.phase, RESTORING);
        assert_eq!(final_state.generation, 1);
    });
}

#[test]
fn stale_generation_cannot_terminate_active_restore() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        let stale = state.lock().unwrap().begin().unwrap();
        release_after_failure(&mut state.lock().unwrap(), stale);
        let active = state.lock().unwrap().begin().unwrap();

        let stale_state = state.clone();
        let stale_completion =
            thread::spawn(move || stale_state.lock().unwrap().terminate_retry_request(stale));
        let active_state = state.clone();
        let active_completion =
            thread::spawn(move || active_state.lock().unwrap().commit_running(active));

        assert!(!stale_completion.join().unwrap());
        assert!(active_completion.join().unwrap());
        let final_state = state.lock().unwrap();
        assert_eq!(final_state.phase, RUNNING);
        assert_eq!(final_state.generation, active.0);
        assert_eq!(final_state.terminal_publications, 0);
    });
}

#[test]
fn running_commit_is_permanent() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        let lease = state.lock().unwrap().begin().unwrap();
        assert!(state.lock().unwrap().commit_running(lease));
        let committed = *state.lock().unwrap();

        let first_state = state.clone();
        let first = thread::spawn(move || {
            let mut state = first_state.lock().unwrap();
            assert!(!state.terminate_active(lease));
            assert_eq!(state.record_failure(lease), None);
        });
        let second_state = state.clone();
        let second = thread::spawn(move || {
            let mut state = second_state.lock().unwrap();
            assert!(!state.terminate_retry_request(lease));
            assert_eq!(state.begin(), None);
        });

        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(*state.lock().unwrap(), committed);
    });
}

#[test]
fn stale_generation_preserves_newer_idle_state() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        let stale = state.lock().unwrap().begin().unwrap();
        release_after_failure(&mut state.lock().unwrap(), stale);
        let current = state.lock().unwrap().begin().unwrap();
        release_after_failure(&mut state.lock().unwrap(), current);
        let idle = *state.lock().unwrap();

        let stale_state = state.clone();
        let stale_completion =
            thread::spawn(move || stale_state.lock().unwrap().terminate_retry_request(stale));
        let stale_active_state = state.clone();
        let stale_active_completion =
            thread::spawn(move || stale_active_state.lock().unwrap().terminate_active(stale));

        assert!(!stale_completion.join().unwrap());
        assert!(!stale_active_completion.join().unwrap());
        assert_eq!(*state.lock().unwrap(), idle);
        assert_eq!(idle.phase, IDLE);
        assert_eq!(idle.generation, current.0);
    });
}

#[test]
fn matching_current_retry_terminalizes_once() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        let lease = state.lock().unwrap().begin().unwrap();
        release_after_failure(&mut state.lock().unwrap(), lease);

        let left = state.clone();
        let right = state.clone();
        let first = thread::spawn(move || left.lock().unwrap().terminate_retry_request(lease));
        let second = thread::spawn(move || right.lock().unwrap().terminate_retry_request(lease));

        let publications = usize::from(first.join().unwrap()) + usize::from(second.join().unwrap());
        assert_eq!(publications, 1);
        let final_state = state.lock().unwrap();
        assert_eq!(final_state.phase, TERMINAL);
        assert_eq!(final_state.generation, lease.0);
        assert_eq!(final_state.terminal_publications, 1);
    });
}

#[test]
fn duplicate_stale_completions_are_effect_free() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        let stale = state.lock().unwrap().begin().unwrap();
        release_after_failure(&mut state.lock().unwrap(), stale);
        let active = state.lock().unwrap().begin().unwrap();
        let active_state = *state.lock().unwrap();

        let retry_state = state.clone();
        let retry =
            thread::spawn(move || retry_state.lock().unwrap().terminate_retry_request(stale));
        let active_failure_state = state.clone();
        let active_failure =
            thread::spawn(move || active_failure_state.lock().unwrap().terminate_active(stale));
        let record_state = state.clone();
        let record = thread::spawn(move || record_state.lock().unwrap().record_failure(stale));

        assert!(!retry.join().unwrap());
        assert!(!active_failure.join().unwrap());
        assert_eq!(record.join().unwrap(), None);
        assert_eq!(*state.lock().unwrap(), active_state);
        assert!(state.lock().unwrap().commit_running(active));
    });
}

#[test]
fn failure_budget_has_one_terminal_boundary() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoreState::new()));
        for failure in 1..=MAX_FAILURES {
            let lease = state.lock().unwrap().begin().unwrap();
            let disposition = state.lock().unwrap().record_failure(lease).unwrap();
            if failure < MAX_FAILURES {
                assert_eq!(disposition, FailureDisposition::Retry);
                assert!(state.lock().unwrap().release_retry(lease));
            } else {
                assert_eq!(disposition, FailureDisposition::Terminal);
            }
        }
        let final_state = state.lock().unwrap();
        assert_eq!(final_state.phase, TERMINAL);
        assert_eq!(final_state.failures, MAX_FAILURES);
        assert_eq!(final_state.terminal_publications, 1);
        assert!(!final_state.channels_ready);
        assert!(final_state.estimator_available);
    });
}
