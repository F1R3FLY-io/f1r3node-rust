use std::collections::BTreeMap;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExecutionKey {
    pre_state: usize,
    post_state: usize,
    creator: usize,
    sequence: usize,
    payload: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Evidence {
    diff: isize,
}

struct EvidenceStore {
    entries: Mutex<BTreeMap<ExecutionKey, Evidence>>,
}

struct PublicationState {
    durable: BTreeMap<ExecutionKey, Evidence>,
    cache: BTreeMap<ExecutionKey, Evidence>,
}

struct ValidatedEvidenceStore {
    state: Mutex<PublicationState>,
}

impl EvidenceStore {
    fn new() -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    fn publish_local_replay(&self, key: ExecutionKey, evidence: Evidence) {
        self.entries.lock().unwrap().insert(key, evidence);
    }

    fn publish_peer_response(&self, _key: ExecutionKey, _evidence: Evidence) -> bool { false }

    fn get(&self, key: ExecutionKey) -> Option<Evidence> {
        self.entries.lock().unwrap().get(&key).copied()
    }

    fn retire(&self, key: ExecutionKey) -> Option<Evidence> {
        self.entries.lock().unwrap().remove(&key)
    }

    fn snapshot(&self) -> BTreeMap<ExecutionKey, Evidence> { self.entries.lock().unwrap().clone() }
}

impl ValidatedEvidenceStore {
    fn new() -> Self {
        Self {
            state: Mutex::new(PublicationState {
                durable: BTreeMap::new(),
                cache: BTreeMap::new(),
            }),
        }
    }

    fn publish_authenticated_replay(
        &self,
        authenticated: bool,
        declared_post_root: usize,
        computed_post_root: usize,
        key: ExecutionKey,
        evidence: Evidence,
    ) -> bool {
        if !authenticated || declared_post_root != computed_post_root {
            return false;
        }

        let mut state = self.state.lock().unwrap();
        match state.durable.get(&key) {
            Some(existing) if *existing != evidence => false,
            _ => {
                state.durable.insert(key, evidence);
                state.cache.insert(key, evidence);
                true
            }
        }
    }

    fn snapshot(
        &self,
    ) -> (
        BTreeMap<ExecutionKey, Evidence>,
        BTreeMap<ExecutionKey, Evidence>,
    ) {
        let state = self.state.lock().unwrap();
        (state.durable.clone(), state.cache.clone())
    }
}

fn equivocations() -> ((ExecutionKey, Evidence), (ExecutionKey, Evidence)) {
    (
        (
            ExecutionKey {
                pre_state: 1,
                post_state: 9,
                creator: 3,
                sequence: 4,
                payload: 5,
            },
            Evidence { diff: 7 },
        ),
        (
            ExecutionKey {
                pre_state: 2,
                post_state: 9,
                creator: 3,
                sequence: 4,
                payload: 6,
            },
            Evidence { diff: -11 },
        ),
    )
}

#[test]
fn concurrent_equivocation_replays_keep_distinct_authenticated_entries() {
    loom::model(|| {
        let store = Arc::new(EvidenceStore::new());
        let ((left_key, left_evidence), (right_key, right_evidence)) = equivocations();
        let left = {
            let store = store.clone();
            thread::spawn(move || store.publish_local_replay(left_key, left_evidence))
        };
        let right = {
            let store = store.clone();
            thread::spawn(move || store.publish_local_replay(right_key, right_evidence))
        };

        left.join().unwrap();
        right.join().unwrap();
        assert_eq!(store.get(left_key), Some(left_evidence));
        assert_eq!(store.get(right_key), Some(right_evidence));
        assert_eq!(store.snapshot().len(), 2);
    });
}

#[test]
fn peer_evidence_cannot_race_with_or_overwrite_local_replay() {
    loom::model(|| {
        let store = Arc::new(EvidenceStore::new());
        let ((key, canonical), _) = equivocations();
        let local = {
            let store = store.clone();
            thread::spawn(move || store.publish_local_replay(key, canonical))
        };
        let peer = {
            let store = store.clone();
            thread::spawn(move || {
                assert!(!store.publish_peer_response(key, Evidence { diff: 99 }));
            })
        };

        local.join().unwrap();
        peer.join().unwrap();
        assert_eq!(store.get(key), Some(canonical));
        assert_eq!(store.snapshot().len(), 1);
    });
}

#[test]
fn only_authenticated_exact_root_replay_publishes_durable_and_cached_evidence() {
    loom::model(|| {
        let store = Arc::new(ValidatedEvidenceStore::new());
        let ((key, canonical), _) = equivocations();
        let exact = {
            let store = store.clone();
            thread::spawn(move || store.publish_authenticated_replay(true, 9, 9, key, canonical))
        };
        let mismatched = {
            let store = store.clone();
            thread::spawn(move || {
                store.publish_authenticated_replay(true, 9, 10, key, Evidence { diff: 91 })
            })
        };
        let unauthenticated = {
            let store = store.clone();
            thread::spawn(move || {
                store.publish_authenticated_replay(false, 9, 9, key, Evidence { diff: 92 })
            })
        };

        assert!(exact.join().unwrap());
        assert!(!mismatched.join().unwrap());
        assert!(!unauthenticated.join().unwrap());
        let (durable, cache) = store.snapshot();
        assert_eq!(durable.get(&key), Some(&canonical));
        assert_eq!(cache.get(&key), Some(&canonical));
        assert_eq!(durable, cache);
    });
}

#[test]
fn validators_with_opposite_arrival_orders_converge() {
    loom::model(|| {
        let first = Arc::new(EvidenceStore::new());
        let second = Arc::new(EvidenceStore::new());
        let ((left_key, left_evidence), (right_key, right_evidence)) = equivocations();
        let left_then_right = {
            let store = first.clone();
            thread::spawn(move || {
                store.publish_local_replay(left_key, left_evidence);
                store.publish_local_replay(right_key, right_evidence);
            })
        };
        let right_then_left = {
            let store = second.clone();
            thread::spawn(move || {
                store.publish_local_replay(right_key, right_evidence);
                store.publish_local_replay(left_key, left_evidence);
            })
        };

        left_then_right.join().unwrap();
        right_then_left.join().unwrap();
        assert_eq!(first.snapshot(), second.snapshot());
        assert_eq!(first.get(left_key), Some(left_evidence));
        assert_eq!(first.get(right_key), Some(right_evidence));
    });
}

#[test]
fn finalized_execution_retirement_preserves_distinct_equivocation() {
    loom::model(|| {
        let store = Arc::new(EvidenceStore::new());
        let ((retired_key, retired_evidence), (live_key, live_evidence)) = equivocations();
        store.publish_local_replay(retired_key, retired_evidence);
        store.publish_local_replay(live_key, live_evidence);

        let retire = {
            let store = store.clone();
            thread::spawn(move || store.retire(retired_key))
        };
        let replay_live = {
            let store = store.clone();
            thread::spawn(move || store.publish_local_replay(live_key, live_evidence))
        };

        assert_eq!(retire.join().unwrap(), Some(retired_evidence));
        replay_live.join().unwrap();
        assert_eq!(store.get(retired_key), None);
        assert_eq!(store.get(live_key), Some(live_evidence));
        assert_eq!(store.snapshot().len(), 1);
    });
}
