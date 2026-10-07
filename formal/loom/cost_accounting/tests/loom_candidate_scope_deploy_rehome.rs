use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

#[derive(Clone, Copy)]
struct CandidateAuthorization {
    on_self_chain: bool,
    active_in_candidate_scope: bool,
    selected_recovery: bool,
}

impl CandidateAuthorization {
    fn should_package(self) -> bool {
        !self.on_self_chain || self.selected_recovery || !self.active_in_candidate_scope
    }
}

#[test]
fn captured_rehome_authorization_survives_concurrent_floor_advance() {
    loom::model(|| {
        let live_scope = Arc::new(AtomicBool::new(false));
        let packaged = Arc::new(AtomicBool::new(false));
        let authorization = CandidateAuthorization {
            on_self_chain: true,
            active_in_candidate_scope: live_scope.load(Ordering::Acquire),
            selected_recovery: false,
        };

        let advance_scope = live_scope.clone();
        let advance = thread::spawn(move || {
            advance_scope.store(true, Ordering::Release);
        });

        let packaged_flag = packaged.clone();
        let package = thread::spawn(move || {
            if authorization.should_package() {
                packaged_flag.store(true, Ordering::Release);
            }
        });

        advance.join().unwrap();
        package.join().unwrap();
        assert!(packaged.load(Ordering::Acquire));
    });
}

#[test]
fn parallel_validator_rehomes_commute() {
    loom::model(|| {
        let published = Arc::new(AtomicUsize::new(0));
        let left = published.clone();
        let right = published.clone();
        let authorization = CandidateAuthorization {
            on_self_chain: true,
            active_in_candidate_scope: false,
            selected_recovery: false,
        };

        let left_thread = thread::spawn(move || {
            if authorization.should_package() {
                left.fetch_or(0b01, Ordering::AcqRel);
            }
        });
        let right_thread = thread::spawn(move || {
            if authorization.should_package() {
                right.fetch_or(0b10, Ordering::AcqRel);
            }
        });

        left_thread.join().unwrap();
        right_thread.join().unwrap();
        assert_eq!(published.load(Ordering::Acquire), 0b11);
    });
}

#[test]
fn captured_active_occurrence_suppresses_duplicate_during_cleanup() {
    loom::model(|| {
        let live_scope = Arc::new(AtomicBool::new(true));
        let packaged = Arc::new(AtomicBool::new(false));
        let authorization = CandidateAuthorization {
            on_self_chain: true,
            active_in_candidate_scope: live_scope.load(Ordering::Acquire),
            selected_recovery: false,
        };

        let cleanup_scope = live_scope.clone();
        let cleanup = thread::spawn(move || {
            cleanup_scope.store(false, Ordering::Release);
        });
        let packaged_flag = packaged.clone();
        let package = thread::spawn(move || {
            if authorization.should_package() {
                packaged_flag.store(true, Ordering::Release);
            }
        });

        cleanup.join().unwrap();
        package.join().unwrap();
        assert!(!packaged.load(Ordering::Acquire));
    });
}
