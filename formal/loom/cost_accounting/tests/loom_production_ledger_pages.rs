use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use loom::sync::{Arc, Mutex, MutexGuard};
use loom::thread;

#[path = "../../../../block-storage/src/rust/finality/finalization_ledger/recovery_pages.rs"]
mod recovery_pages;
#[path = "../../../../block-storage/src/rust/finality/finalization_ledger/effect_observation.rs"]
mod effect_observation;

use recovery_pages::{
    Advancement, Compaction, ExclusiveGate, ReceiptFactory, ReceiptKeys, RecoveryStore, Window,
};

struct Gate {
    lock: Mutex<()>,
    enabled: bool,
}

impl ExclusiveGate for Gate {
    type Guard<'a> = Option<MutexGuard<'a, ()>>;

    fn lock(&self) -> Self::Guard<'_> { self.enabled.then(|| self.lock.lock().unwrap()) }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Key {
    Effect(u64, u8, usize),
    Marker(u64),
}

struct Factory;

impl ReceiptFactory for Factory {
    type Block = u8;
    type Key = Key;
    const KINDS: usize = 4;

    fn effect(revision: u64, block: &u8, kind: usize) -> Key { Key::Effect(revision, *block, kind) }
    fn complete(revision: u64) -> Key { Key::Marker(revision) }
}

type Keys = ReceiptKeys<std::vec::IntoIter<u8>, Factory>;

struct Data {
    head: u64,
    projection: u64,
    effects: u64,
    compaction: u64,
    receipts: BTreeSet<Key>,
    marker_reads: Vec<u64>,
    deletions: Vec<Vec<Key>>,
    fail_effects: bool,
    fail_delete: bool,
    fail_compaction: bool,
}

#[derive(Clone)]
struct Store {
    gate: Arc<Gate>,
    data: Arc<Mutex<Data>>,
}

impl Store {
    fn new(rounds: u64, effects: u64, enabled: bool) -> Self {
        Self {
            gate: Arc::new(Gate {
                lock: Mutex::new(()),
                enabled,
            }),
            data: Arc::new(Mutex::new(Data {
                head: rounds,
                projection: rounds,
                effects,
                compaction: 0,
                receipts: (1..=rounds)
                    .flat_map(|revision| Keys::new(revision, vec![0].into_iter()))
                    .collect(),
                marker_reads: Vec::new(),
                deletions: Vec::new(),
                fail_effects: false,
                fail_delete: false,
                fail_compaction: false,
            })),
        }
    }
}

impl RecoveryStore for Store {
    type Error = String;
    type Gate = Gate;
    type Keys = Keys;

    fn gate(&self) -> &Gate { &self.gate }
    fn head_revision(&self) -> Result<Option<u64>, String> {
        Ok(Some(self.data.lock().unwrap().head))
    }
    fn projection_cursor(&self) -> Result<u64, String> { Ok(self.data.lock().unwrap().projection) }
    fn effects_cursor(&self) -> Result<u64, String> { Ok(self.data.lock().unwrap().effects) }
    fn compaction_cursor(&self) -> Result<u64, String> { Ok(self.data.lock().unwrap().compaction) }

    fn round_complete(&self, revision: u64) -> Result<bool, String> {
        let mut data = self.data.lock().unwrap();
        data.marker_reads.push(revision);
        Ok(data.receipts.contains(&Key::Marker(revision)))
    }

    fn write_effects_cursor(&self, revision: u64) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        if data.fail_effects {
            return Err("effects-write".to_string());
        }
        assert!(
            revision >= data.effects,
            "negative control: stale worker regressed effects cursor"
        );
        assert!(revision <= data.projection);
        for round in data.effects + 1..=revision {
            assert!(data.receipts.contains(&Key::Marker(round)));
        }
        data.effects = revision;
        Ok(())
    }

    fn receipt_keys(&self, revision: u64) -> Result<Keys, String> {
        assert!(revision <= self.data.lock().unwrap().head);
        Ok(Keys::new(revision, vec![0].into_iter()))
    }

    fn delete_receipts(&self, keys: Vec<Key>) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        if data.fail_delete {
            return Err("receipt-delete".to_string());
        }
        for key in &keys {
            match key {
                Key::Effect(revision, _, _) => assert!(*revision <= data.effects),
                Key::Marker(revision) => {
                    assert!(*revision <= data.effects);
                    assert!(!data
                        .receipts
                        .iter()
                        .any(|entry| matches!(entry, Key::Effect(r, _, _) if r == revision)));
                }
            }
            data.receipts.remove(key);
        }
        data.deletions.push(keys);
        Ok(())
    }

    fn write_compaction_cursor(&self, revision: u64) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        if data.fail_compaction {
            return Err("compaction-write".to_string());
        }
        assert!(revision >= data.compaction && revision <= data.effects);
        assert!(!data.receipts.iter().any(|key| match key {
            Key::Effect(round, _, _) | Key::Marker(round) => *round <= revision,
        }));
        data.compaction = revision;
        Ok(())
    }

    fn serialization(message: String) -> String { message }
    fn invalid(message: String) -> String { message }
    fn projection_pending(revision: u64, projected_revision: u64) -> String {
        format!("projection-pending:{revision}:{projected_revision}")
    }
}

fn explore(test: impl Fn() + Sync + Send + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 4;
    builder.max_branches = 5_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

fn advance_once(store: &Store, window: Window, budget: usize) -> (u64, bool) {
    let mut cursor = window.cursor;
    let mut finished = false;
    let mut failure = None;
    let result = recovery_pages::advance_page(
        store,
        Advancement {
            target: window.target,
            cursor: &mut cursor,
            finished: &mut finished,
            failure: &mut failure,
        },
        NonZeroUsize::new(budget).unwrap(),
    )
    .unwrap();
    assert_eq!(result, finished);
    assert!(failure.is_none());
    (cursor, finished)
}

fn two_cursor_workers(enabled: bool) {
    let store = Store::new(2, 0, enabled);
    let first_window = recovery_pages::begin_advancement(&store).unwrap();
    let second_window = recovery_pages::begin_advancement(&store).unwrap();
    let first = {
        let store = store.clone();
        thread::spawn(move || advance_once(&store, first_window, 1))
    };
    let second = {
        let store = store.clone();
        thread::spawn(move || advance_once(&store, second_window, 2))
    };
    first.join().unwrap();
    assert_eq!(second.join().unwrap(), (2, true));
    let data = store.data.lock().unwrap();
    assert_eq!(data.effects, 2);
    assert!(data.marker_reads.len() <= 3);
    assert!(data.marker_reads.iter().all(|round| *round <= 2));
}

#[test]
fn shared_cursor_pages_resume_current_progress_with_two_workers() {
    explore(|| two_cursor_workers(true));
}

#[test]
#[should_panic(expected = "negative control: stale worker regressed effects cursor")]
fn missing_page_gate_exposes_a_stale_cursor_write() { explore(|| two_cursor_workers(false)); }

#[test]
fn captured_effect_target_does_not_follow_later_projection() {
    explore(|| {
        let store = Store::new(2, 0, true);
        store.data.lock().unwrap().projection = 1;
        let selected = recovery_pages::select_effects(&store).unwrap();
        assert_eq!(selected, Window {
            cursor: 0,
            target: 1
        });
        assert_eq!(
            recovery_pages::require_projection(&store, 2),
            Err("projection-pending:2:1".to_string())
        );
        assert!(recovery_pages::require_projection(&store, 0).is_err());
        let writer = {
            let store = store.clone();
            thread::spawn(move || {
                let _guard = store.gate.lock();
                let mut data = store.data.lock().unwrap();
                data.head = 3;
                data.projection = 3;
                data.receipts.extend(Keys::new(3, vec![0].into_iter()));
            })
        };
        let reader = {
            let store = store.clone();
            thread::spawn(move || {
                assert_eq!(advance_once(&store, selected, 2), (1, true));
                recovery_pages::require_projection(&store, 1).unwrap();
            })
        };
        writer.join().unwrap();
        reader.join().unwrap();
        let data = store.data.lock().unwrap();
        assert_eq!(data.effects, 1);
        assert_eq!(data.marker_reads, vec![1]);
    });
}

fn compact_all(store: &Store, window: Window, budget: usize) {
    let mut cursor = window.cursor;
    let mut pending = None;
    let mut failure = None;
    for _ in 0..6 {
        if recovery_pages::compact_page(
            store,
            Compaction {
                target: window.target,
                cursor: &mut cursor,
                pending: &mut pending,
                failure: &mut failure,
            },
            NonZeroUsize::new(budget).unwrap(),
        )
        .unwrap()
        {
            assert_eq!(cursor, 1);
            assert!(pending.is_none());
            return;
        }
    }
    panic!("one-round compaction exceeded its finite page bound");
}

#[test]
fn shared_compactors_delete_the_marker_last_and_preserve_completion() {
    explore(|| {
        let store = Store::new(1, 1, true);
        let first_window = recovery_pages::begin_compaction(&store).unwrap();
        let second_window = recovery_pages::begin_compaction(&store).unwrap();
        let first = {
            let store = store.clone();
            thread::spawn(move || compact_all(&store, first_window, 1))
        };
        let second = {
            let store = store.clone();
            thread::spawn(move || compact_all(&store, second_window, 2))
        };
        first.join().unwrap();
        second.join().unwrap();
        assert!(effect_observation::effect_is_complete(
            1,
            || store.effects_cursor(),
            || -> Result<bool, String> { panic!("covered cursor must not read a receipt") },
        )
        .unwrap());
        let data = store.data.lock().unwrap();
        assert_eq!(data.effects, 1);
        assert!(data.receipts.is_empty());
        assert_eq!(data.compaction, 1);
        assert!(data.deletions.iter().all(|page| page.len() <= 2));
    });
}

#[test]
fn completion_gap_and_failed_write_preserve_unpublished_progress() {
    explore(|| {
        let store = Store::new(2, 0, true);
        store.data.lock().unwrap().receipts.remove(&Key::Marker(1));
        let window = recovery_pages::begin_advancement(&store).unwrap();
        assert_eq!(advance_once(&store, window, 2), (0, true));
        {
            let mut data = store.data.lock().unwrap();
            data.receipts.insert(Key::Marker(1));
            data.fail_effects = true;
        }
        let mut cursor = 0;
        let mut finished = false;
        let mut failure = None;
        let error = recovery_pages::advance_page(
            &store,
            Advancement {
                target: 2,
                cursor: &mut cursor,
                finished: &mut finished,
                failure: &mut failure,
            },
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, "effects-write");
        assert_eq!(cursor, 0);
        assert!(!finished);
        assert_eq!(store.effects_cursor().unwrap(), 0);
        assert!(store.gate.lock.try_lock().is_ok());
        store.data.lock().unwrap().fail_effects = false;
        assert_eq!(advance_once(&store, window, 2), (2, true));
        assert_eq!(
            recovery_pages::advance_page(
                &store,
                Advancement {
                    target: 2,
                    cursor: &mut cursor,
                    finished: &mut finished,
                    failure: &mut failure,
                },
                NonZeroUsize::new(2).unwrap()
            ),
            Err(error)
        );
        assert_eq!(cursor, 0);
    });
}

#[test]
fn failed_compaction_pages_require_fresh_durable_recovery() {
    for fail_delete in [false, true] {
        explore(move || {
            let store = Store::new(1, 1, true);
            let window = recovery_pages::begin_compaction(&store).unwrap();
            {
                let mut data = store.data.lock().unwrap();
                data.fail_delete = fail_delete;
                data.fail_compaction = !fail_delete;
            }
            let mut cursor = 0;
            let mut pending = None;
            let mut failure = None;
            let error = recovery_pages::compact_page(
                &store,
                Compaction {
                    target: 1,
                    cursor: &mut cursor,
                    pending: &mut pending,
                    failure: &mut failure,
                },
                NonZeroUsize::new(5).unwrap(),
            )
            .unwrap_err();
            assert_eq!(
                error,
                if fail_delete {
                    "receipt-delete"
                } else {
                    "compaction-write"
                }
            );
            assert_eq!(cursor, 0);
            assert_eq!(store.compaction_cursor().unwrap(), 0);
            assert!(store.gate.lock.try_lock().is_ok());
            {
                let mut data = store.data.lock().unwrap();
                assert_eq!(data.receipts.len(), if fail_delete { 5 } else { 0 });
                data.fail_delete = false;
                data.fail_compaction = false;
            }
            compact_all(&store, window, 1);
            assert_eq!(
                recovery_pages::compact_page(
                    &store,
                    Compaction {
                        target: 1,
                        cursor: &mut cursor,
                        pending: &mut pending,
                        failure: &mut failure,
                    },
                    NonZeroUsize::new(1).unwrap()
                ),
                Err(error)
            );
        });
    }
}
