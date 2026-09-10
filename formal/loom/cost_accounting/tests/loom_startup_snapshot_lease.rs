use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_snapshot_lease.rs"]
mod startup_snapshot_lease;
use startup_snapshot_lease::{LeasePhase, LeaseRole, SnapshotLeases};

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
fn concurrent_capture_reservations_cover_construction_through_destruction() {
    explore(|| {
        let kernel = Arc::new(Mutex::new(SnapshotLeases::default()));
        let physical = Arc::new(AtomicUsize::new(0));
        let tasks = [1, 2].map(|id| {
            let kernel = kernel.clone();
            let physical = physical.clone();
            thread::spawn(move || {
                if !kernel.lock().unwrap().reserve(id, id) {
                    return;
                }
                assert!(kernel.lock().unwrap().begin_capture(&id));
                assert_eq!(physical.fetch_add(1, Ordering::SeqCst), 0);
                thread::yield_now();
                assert!(kernel.lock().unwrap().finish_capture(&id));
                assert!(kernel.lock().unwrap().abandon_capture(&id));
                thread::yield_now();
                assert_eq!(physical.fetch_sub(1, Ordering::SeqCst), 1);
                let mut guard = kernel.lock().unwrap();
                assert!(guard.destroyed(LeaseRole::Pending, &id));
                assert!(guard.release(LeaseRole::Pending, &id));
            })
        });
        for task in tasks {
            task.join().unwrap();
        }
        assert_eq!(physical.load(Ordering::SeqCst), 0);
        assert!(kernel.lock().unwrap().pending().is_none());
    });
}

#[test]
fn activation_waits_for_actual_active_episode_destruction() {
    explore(|| {
        let mut state = SnapshotLeases::default();
        assert!(state.reserve(1, 1));
        assert!(state.begin_capture(&1));
        assert!(state.finish_capture(&1));
        assert!(state.store(&1, &1));
        assert_eq!(state.activate(&1), Some(1));
        assert!(state.reserve(2, 2));
        assert!(state.begin_capture(&2));
        assert!(state.finish_capture(&2));
        assert!(state.store(&2, &2));
        let kernel = Arc::new(Mutex::new(state));
        let physical_active = Arc::new(AtomicUsize::new(1));
        let retired_kernel = kernel.clone();
        let retired_physical = physical_active.clone();
        let retiring = thread::spawn(move || {
            assert!(retired_kernel.lock().unwrap().retire_active(&1));
            thread::yield_now();
            assert_eq!(retired_physical.fetch_sub(1, Ordering::SeqCst), 1);
            let mut guard = retired_kernel.lock().unwrap();
            assert!(guard.destroyed(LeaseRole::Active, &1));
            assert!(guard.release(LeaseRole::Active, &1));
        });
        let new_kernel = kernel.clone();
        let new_physical = physical_active.clone();
        let activating = thread::spawn(move || {
            let activated = new_kernel.lock().unwrap().activate(&2);
            if activated.is_some() {
                assert_eq!(new_physical.fetch_add(1, Ordering::SeqCst), 0);
            }
            activated
        });
        retiring.join().unwrap();
        let activated = activating.join().unwrap();
        let mut guard = kernel.lock().unwrap();
        if activated.is_none() {
            assert_eq!(guard.activate(&2), Some(2));
            assert_eq!(physical_active.fetch_add(1, Ordering::SeqCst), 0);
        }
        assert_eq!(guard.active().unwrap().identity, 2);
        assert!(!guard.release(LeaseRole::Active, &1));
    });
}

#[test]
fn stale_pending_release_cannot_release_the_same_identity_after_transfer() {
    explore(|| {
        let mut state = SnapshotLeases::default();
        state.reserve(1, 1);
        state.begin_capture(&1);
        state.finish_capture(&1);
        state.store(&1, &1);
        state.activate(&1).unwrap();
        let kernel = Arc::new(Mutex::new(state));
        let stale_kernel = kernel.clone();
        let stale = thread::spawn(move || {
            let mut guard = stale_kernel.lock().unwrap();
            assert!(!guard.abandon_capture(&1));
            assert!(!guard.retire_stored(&1));
            assert!(!guard.destroyed(LeaseRole::Pending, &1));
            assert!(!guard.release(LeaseRole::Pending, &1));
        });
        let active_kernel = kernel.clone();
        let active = thread::spawn(move || {
            assert!(active_kernel.lock().unwrap().retire_active(&1));
            thread::yield_now();
            let mut guard = active_kernel.lock().unwrap();
            assert_eq!(guard.active().unwrap().phase, LeasePhase::Retiring);
            assert!(guard.destroyed(LeaseRole::Active, &1));
            assert!(guard.release(LeaseRole::Active, &1));
        });
        stale.join().unwrap();
        active.join().unwrap();
        assert!(kernel.lock().unwrap().active().is_none());
    });
}
