use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

const BLOCK_ARTIFACT: usize = 1 << 0;
const STATE_ARTIFACT: usize = 1 << 1;
const PARENT_WAITER: usize = 1 << 0;
const SIBLING_WAITER: usize = 1 << 1;
const CHILD_WAITER: usize = 1 << 2;

struct RecoveryState {
    held: AtomicUsize,
    requested: AtomicUsize,
    block_waiters: AtomicUsize,
    state_waiters: AtomicUsize,
    released: AtomicUsize,
    objective_invalid: AtomicBool,
}

impl RecoveryState {
    fn new() -> Self {
        Self {
            held: AtomicUsize::new(0),
            requested: AtomicUsize::new(0),
            block_waiters: AtomicUsize::new(0),
            state_waiters: AtomicUsize::new(0),
            released: AtomicUsize::new(0),
            objective_invalid: AtomicBool::new(false),
        }
    }

    fn waiters(&self, artifact: usize) -> &AtomicUsize {
        match artifact {
            BLOCK_ARTIFACT => &self.block_waiters,
            STATE_ARTIFACT => &self.state_waiters,
            _ => unreachable!(),
        }
    }

    fn drain(&self, artifact: usize) {
        let released = self.waiters(artifact).swap(0, Ordering::AcqRel);
        self.released.fetch_or(released, Ordering::Release);
    }

    fn defer(&self, artifact: usize, waiter: usize) {
        if self.held.load(Ordering::Acquire) & artifact != 0 {
            self.released.fetch_or(waiter, Ordering::Release);
            return;
        }
        self.waiters(artifact).fetch_or(waiter, Ordering::AcqRel);
        self.requested.fetch_or(artifact, Ordering::AcqRel);
        if self.held.load(Ordering::Acquire) & artifact != 0 {
            self.drain(artifact);
        }
    }

    fn recover(&self, artifact: usize) {
        while self.requested.load(Ordering::Acquire) & artifact == 0 {
            thread::yield_now();
        }
        self.held.fetch_or(artifact, Ordering::Release);
        self.drain(artifact);
    }

    fn classify_local_absence(&self) {
        assert!(!self.objective_invalid.load(Ordering::Acquire));
    }
}

#[test]
fn duplicate_block_waiters_survive_registration_recovery_races() {
    loom::model(|| {
        let state = Arc::new(RecoveryState::new());
        let parent = {
            let state = state.clone();
            thread::spawn(move || state.defer(BLOCK_ARTIFACT, PARENT_WAITER))
        };
        let sibling = {
            let state = state.clone();
            thread::spawn(move || state.defer(BLOCK_ARTIFACT, SIBLING_WAITER))
        };
        let recovery = {
            let state = state.clone();
            thread::spawn(move || state.recover(BLOCK_ARTIFACT))
        };

        parent.join().unwrap();
        sibling.join().unwrap();
        recovery.join().unwrap();

        assert_eq!(state.requested.load(Ordering::Acquire), BLOCK_ARTIFACT);
        assert_eq!(state.block_waiters.load(Ordering::Acquire), 0);
        assert_eq!(
            state.released.load(Ordering::Acquire),
            PARENT_WAITER | SIBLING_WAITER
        );
    });
}

#[test]
fn block_and_state_recovery_release_only_exact_waiters() {
    loom::model(|| {
        let state = Arc::new(RecoveryState::new());
        let parent = {
            let state = state.clone();
            thread::spawn(move || state.defer(BLOCK_ARTIFACT, PARENT_WAITER))
        };
        let child = {
            let state = state.clone();
            thread::spawn(move || state.defer(STATE_ARTIFACT, CHILD_WAITER))
        };

        parent.join().unwrap();
        child.join().unwrap();
        state.recover(BLOCK_ARTIFACT);

        assert_eq!(state.released.load(Ordering::Acquire), PARENT_WAITER);
        assert_eq!(state.state_waiters.load(Ordering::Acquire), CHILD_WAITER);

        state.recover(STATE_ARTIFACT);
        assert_eq!(
            state.released.load(Ordering::Acquire),
            PARENT_WAITER | CHILD_WAITER
        );
        assert_eq!(
            state.requested.load(Ordering::Acquire),
            BLOCK_ARTIFACT | STATE_ARTIFACT
        );
    });
}

#[test]
fn validators_recover_without_shared_request_or_release_state() {
    loom::model(|| {
        let genesis = Arc::new(RecoveryState::new());
        let restored = Arc::new(RecoveryState::new());
        let genesis_thread = {
            let genesis = genesis.clone();
            thread::spawn(move || {
                genesis.classify_local_absence();
                genesis.defer(BLOCK_ARTIFACT, PARENT_WAITER);
                genesis.recover(BLOCK_ARTIFACT);
            })
        };
        let restored_thread = {
            let restored = restored.clone();
            thread::spawn(move || {
                restored.classify_local_absence();
                restored.defer(STATE_ARTIFACT, CHILD_WAITER);
                restored.recover(STATE_ARTIFACT);
            })
        };

        genesis_thread.join().unwrap();
        restored_thread.join().unwrap();

        assert_eq!(genesis.requested.load(Ordering::Acquire), BLOCK_ARTIFACT);
        assert_eq!(genesis.released.load(Ordering::Acquire), PARENT_WAITER);
        assert_eq!(restored.requested.load(Ordering::Acquire), STATE_ARTIFACT);
        assert_eq!(restored.released.load(Ordering::Acquire), CHILD_WAITER);
        assert!(!genesis.objective_invalid.load(Ordering::Acquire));
        assert!(!restored.objective_invalid.load(Ordering::Acquire));
    });
}
