#![feature(arbitrary_self_types)]

use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex as IdentityMutex};
use loom::thread;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/admission_identity.rs"]
mod admission_identity;

use admission_identity::AdmissionIdentities;

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
fn concurrent_same_key_claims_never_overlap_or_leak() {
    explore(|| {
        let identities = Arc::new(AdmissionIdentities::<u8, 2>::new());
        let active = Arc::new(AtomicUsize::new(0));
        let workers = [0, 1].map(|_| {
            let identities = identities.clone();
            let active = active.clone();
            thread::spawn(move || {
                if let Some(identity) = identities.try_claim(1) {
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    thread::yield_now();
                    assert!(identities.contains(&1));
                    assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    drop(identity);
                }
            })
        });
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(identities.is_empty());
    });
}

#[test]
fn dropping_an_old_identity_preserves_its_replacement() {
    explore(admission_identity::previous_owner_cannot_release_replacement_identity);
}

#[test]
fn concurrent_releases_preserve_other_keys_with_colliding_shards() {
    explore(|| {
        let identities = Arc::new(AdmissionIdentities::<u8, 1>::new());
        let keep = identities.try_claim(0).unwrap();
        let workers = [1, 2].map(|key| {
            let identities = identities.clone();
            thread::spawn(move || {
                let owner = identities.try_claim(key).unwrap();
                assert!(identities.contains(&0));
                thread::yield_now();
                drop(owner);
                assert!(identities.contains(&0));
            })
        });
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(identities.len(), 1);
        drop(keep);
        assert!(identities.is_empty());
    });
}

#[test]
fn transferring_then_dropping_queued_identity_releases_its_owner() {
    explore(|| {
        let identities = Arc::new(AdmissionIdentities::<u8, 1>::new());
        let queued = Arc::new(IdentityMutex::new(Some(identities.try_claim(1).unwrap())));
        let worker_queue = queued.clone();
        let worker = thread::spawn(move || {
            let item = worker_queue.lock().unwrap().take();
            thread::yield_now();
            drop(item);
        });
        if let Some(replacement) = identities.try_claim(1) {
            assert!(identities.contains(&1));
            drop(replacement);
        }
        worker.join().unwrap();
        assert!(identities.is_empty());
    });
}
