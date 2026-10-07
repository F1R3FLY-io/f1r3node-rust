use loom::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct LocalPublication {
    head: u64,
    effects_through: u64,
    revision: u64,
}

struct LiveRecoveryNode {
    known_height: AtomicU64,
    requested_finalization: AtomicU64,
    completed_finalization: AtomicU64,
    recovery_active: AtomicBool,
    publication: Mutex<LocalPublication>,
}

impl LiveRecoveryNode {
    fn new() -> Self {
        Self {
            known_height: AtomicU64::new(0),
            requested_finalization: AtomicU64::new(0),
            completed_finalization: AtomicU64::new(0),
            recovery_active: AtomicBool::new(false),
            publication: Mutex::new(LocalPublication {
                head: 0,
                effects_through: 0,
                revision: 0,
            }),
        }
    }

    fn advertise_and_admit_tip(&self, tip: u64) {
        self.known_height.fetch_max(tip, Ordering::AcqRel);
        if self.recovery_active.load(Ordering::Acquire) {
            self.requested_finalization.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn request_recovery(&self) {
        self.recovery_active.store(true, Ordering::Release);
        self.requested_finalization.fetch_add(1, Ordering::AcqRel);
    }

    fn run_local_finalizer(&self) {
        let ticket = self.requested_finalization.load(Ordering::Acquire);
        if ticket <= self.completed_finalization.load(Ordering::Acquire) {
            return;
        }
        let target = self.known_height.load(Ordering::Acquire);
        let mut publication = self.publication.lock().unwrap();
        if target > publication.head {
            publication.head = target;
            publication.effects_through = target;
            publication.revision += 1;
        }
        self.completed_finalization
            .fetch_max(ticket, Ordering::AcqRel);
    }

    fn publication(&self) -> LocalPublication { *self.publication.lock().unwrap() }
}

#[test]
fn remote_tip_and_local_finalizer_race_preserves_atomic_local_publication() {
    loom::model(|| {
        let node = Arc::new(LiveRecoveryNode::new());
        node.request_recovery();
        let admission = {
            let node = node.clone();
            thread::spawn(move || node.advertise_and_admit_tip(2))
        };
        let finalizer = {
            let node = node.clone();
            thread::spawn(move || node.run_local_finalizer())
        };
        admission.join().unwrap();
        finalizer.join().unwrap();
        node.run_local_finalizer();
        let publication = node.publication();
        assert_eq!(publication.head, publication.effects_through);
        assert!(publication.head <= node.known_height.load(Ordering::Acquire));
    });
}

#[test]
fn duplicate_and_reordered_tips_are_idempotent_advice() {
    loom::model(|| {
        let node = Arc::new(LiveRecoveryNode::new());
        node.request_recovery();
        let lower = {
            let node = node.clone();
            thread::spawn(move || {
                node.advertise_and_admit_tip(1);
                node.advertise_and_admit_tip(1);
            })
        };
        let higher = {
            let node = node.clone();
            thread::spawn(move || node.advertise_and_admit_tip(2))
        };
        lower.join().unwrap();
        higher.join().unwrap();
        node.run_local_finalizer();
        let publication = node.publication();
        assert_eq!(node.known_height.load(Ordering::Acquire), 2);
        assert_eq!(publication.head, 2);
        assert_eq!(publication.effects_through, 2);
        assert_eq!(publication.revision, 1);
    });
}

#[test]
fn finalizer_retry_after_post_capture_admission_reaches_the_new_tip() {
    loom::model(|| {
        let node = Arc::new(LiveRecoveryNode::new());
        node.request_recovery();
        let first = {
            let node = node.clone();
            thread::spawn(move || node.run_local_finalizer())
        };
        let admission = {
            let node = node.clone();
            thread::spawn(move || node.advertise_and_admit_tip(2))
        };
        first.join().unwrap();
        admission.join().unwrap();
        node.run_local_finalizer();
        let publication = node.publication();
        assert_eq!(publication.head, 2);
        assert_eq!(publication.effects_through, 2);
        assert!(
            node.completed_finalization.load(Ordering::Acquire)
                >= node.requested_finalization.load(Ordering::Acquire)
        );
    });
}

#[test]
fn validators_recover_without_a_shared_publication_lock() {
    loom::model(|| {
        let first = Arc::new(LiveRecoveryNode::new());
        let second = Arc::new(LiveRecoveryNode::new());
        first.request_recovery();
        second.request_recovery();
        first.advertise_and_admit_tip(1);
        second.advertise_and_admit_tip(2);
        let first_worker = {
            let first = first.clone();
            thread::spawn(move || first.run_local_finalizer())
        };
        let second_worker = {
            let second = second.clone();
            thread::spawn(move || second.run_local_finalizer())
        };
        first_worker.join().unwrap();
        second_worker.join().unwrap();
        assert_eq!(first.publication().head, 1);
        assert_eq!(second.publication().head, 2);
    });
}
