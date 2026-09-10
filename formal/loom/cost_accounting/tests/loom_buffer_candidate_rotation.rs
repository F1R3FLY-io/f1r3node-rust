use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/candidate_rotation.rs"]
mod candidate_rotation;

use candidate_rotation::CandidateRotation;

#[test]
fn recovery_and_expiry_keep_independent_orders() {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(|| {
        let mut rotation = CandidateRotation::new();
        for key in [1u16, 2, 3] {
            rotation.insert(key);
        }
        let rotation = Arc::new(Mutex::new(rotation));
        let recovery = {
            let rotation = rotation.clone();
            thread::spawn(move || {
                for key in [1, 2, 3] {
                    assert_eq!(rotation.lock().unwrap().next(), Some(key));
                }
            })
        };
        for key in [1, 2, 3] {
            assert_eq!(rotation.lock().unwrap().next_expiry(), Some(key));
        }
        recovery.join().unwrap();
    });
}

#[test]
fn expiry_covers_surviving_members_during_recovery_and_mutation() {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(|| {
        let mut rotation = CandidateRotation::new();
        rotation.insert(1u16);
        rotation.insert(2);
        let count = rotation.len();
        let rotation = Arc::new(Mutex::new(rotation));
        let recovery = {
            let rotation = rotation.clone();
            thread::spawn(move || {
                rotation.lock().unwrap().next();
                rotation.lock().unwrap().next();
            })
        };
        let mutation = {
            let rotation = rotation.clone();
            thread::spawn(move || {
                rotation.lock().unwrap().remove(&1);
                rotation.lock().unwrap().insert(3);
            })
        };
        let mut served = false;
        for _ in 0..count {
            served |= rotation.lock().unwrap().next_expiry() == Some(2);
        }
        assert!(served);
        recovery.join().unwrap();
        mutation.join().unwrap();
        let mut rotation = rotation.lock().unwrap();
        let recovery: std::collections::HashSet<_> =
            (0..2).map(|_| rotation.next().unwrap()).collect();
        let expiry: std::collections::HashSet<_> =
            (0..2).map(|_| rotation.next_expiry().unwrap()).collect();
        assert_eq!(recovery, std::collections::HashSet::from([2, 3]));
        assert_eq!(expiry, recovery);
    });
}

#[test]
fn concurrent_arrivals_and_retirement_preserve_existing_candidate_service() {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(|| {
        let mut rotation = CandidateRotation::new();
        rotation.insert(1u16);
        rotation.insert(2);
        let rotation = Arc::new(Mutex::new(rotation));
        let first = {
            let rotation = rotation.clone();
            thread::spawn(move || {
                rotation.lock().unwrap().insert(3);
                assert!(!rotation.lock().unwrap().insert(2));
            })
        };
        let second = {
            let rotation = rotation.clone();
            thread::spawn(move || {
                assert!(rotation.lock().unwrap().remove(&1));
                rotation.lock().unwrap().insert(4);
            })
        };
        let mut served = false;
        for _ in 0..2 {
            served |= rotation.lock().unwrap().next() == Some(2);
        }
        assert!(served, "new arrivals overtook an existing candidate");
        first.join().unwrap();
        second.join().unwrap();
        let mut final_state = rotation.lock().unwrap();
        assert_eq!(final_state.len(), 3);
        let members: std::collections::HashSet<_> =
            (0..3).map(|_| final_state.next().unwrap()).collect();
        assert_eq!(members, std::collections::HashSet::from([2, 3, 4]));
    });
}
