use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

struct ValidatorRecovery {
    identity: usize,
    buffered: AtomicBool,
}

impl ValidatorRecovery {
    fn new(identity: usize) -> Self {
        Self {
            identity,
            buffered: AtomicBool::new(false),
        }
    }

    fn validate_received_rejection(&self, carrier_owner: usize) {
        if self.identity == carrier_owner {
            self.buffered.store(true, Ordering::Release);
        }
    }

    fn retry_authorized(&self, floor_settled: bool) -> bool {
        floor_settled && self.buffered.load(Ordering::Acquire)
    }
}

#[test]
fn received_merge_transfers_retry_custody_only_to_the_carrier_owner() {
    loom::model(|| {
        let owner = Arc::new(ValidatorRecovery::new(1));
        let peer = Arc::new(ValidatorRecovery::new(2));

        let owner_validation = {
            let owner = owner.clone();
            thread::spawn(move || owner.validate_received_rejection(1))
        };
        let peer_validation = {
            let peer = peer.clone();
            thread::spawn(move || peer.validate_received_rejection(1))
        };

        owner_validation.join().unwrap();
        peer_validation.join().unwrap();

        assert!(owner.retry_authorized(true));
        assert!(!peer.retry_authorized(true));
        assert!(!owner.retry_authorized(false));
    });
}

#[test]
fn distinct_carrier_owners_recover_without_a_global_retry_lock() {
    loom::model(|| {
        let first = Arc::new(ValidatorRecovery::new(1));
        let second = Arc::new(ValidatorRecovery::new(2));

        let first_validation = {
            let first = first.clone();
            thread::spawn(move || {
                first.validate_received_rejection(1);
                first.validate_received_rejection(2);
            })
        };
        let second_validation = {
            let second = second.clone();
            thread::spawn(move || {
                second.validate_received_rejection(1);
                second.validate_received_rejection(2);
            })
        };

        first_validation.join().unwrap();
        second_validation.join().unwrap();

        assert!(first.retry_authorized(true));
        assert!(second.retry_authorized(true));
    });
}

#[test]
fn concurrent_legacy_and_v6_dispositions_do_not_alias() {
    loom::model(|| {
        let active = Arc::new(AtomicUsize::new(0));
        let rejected = Arc::new(AtomicUsize::new(0));
        let legacy_active = active.clone();
        let legacy = thread::spawn(move || {
            legacy_active.fetch_or(0b01, Ordering::AcqRel);
        });
        let v6_active = active.clone();
        let v6_rejected = rejected.clone();
        let v6 = thread::spawn(move || {
            v6_active.fetch_or(0b10, Ordering::AcqRel);
            v6_rejected.fetch_or(0b10, Ordering::AcqRel);
        });

        legacy.join().unwrap();
        v6.join().unwrap();

        let active_domains = active.load(Ordering::Acquire);
        let rejected_domains = rejected.load(Ordering::Acquire);
        assert_eq!(active_domains, 0b11);
        assert_eq!(rejected_domains, 0b10);
        assert_eq!(active_domains & !rejected_domains, 0b01);
    });
}
