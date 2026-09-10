use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use loom::sync::{Arc, Mutex, MutexGuard};
use loom::thread;

#[path = "../../../../block-storage/src/rust/finality/finalization_ledger/recovery_pages.rs"]
#[expect(
    dead_code,
    reason = "Recovery-page scenarios cover the other imported production functions."
)]
mod recovery_pages;
#[path = "../../../../block-storage/src/rust/finality/finalization_ledger/integrity_pages.rs"]
mod integrity_pages;

use integrity_pages::{Captured, Integrity, IntegrityStore};
use recovery_pages::{ExclusiveGate, RecoveryStore};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Head {
    revision: u64,
    digest: u64,
}

struct Gate {
    mutex: Mutex<()>,
    enabled: bool,
}

impl ExclusiveGate for Gate {
    type Guard<'a> = Option<MutexGuard<'a, ()>>;
    fn lock(&self) -> Self::Guard<'_> { self.enabled.then(|| self.mutex.lock().unwrap()) }
}

struct Data {
    genesis: Option<u64>,
    head: Option<Head>,
    non_empty: bool,
    records: BTreeMap<u64, (u64, Head, bool)>,
    projection: u64,
    effects: u64,
    compaction: u64,
    visited: Vec<u64>,
}

#[derive(Clone)]
struct Store {
    gate: Arc<Gate>,
    data: Arc<Mutex<Data>>,
}

impl Store {
    fn new(enabled: bool) -> Self {
        Self {
            gate: Arc::new(Gate {
                mutex: Mutex::new(()),
                enabled,
            }),
            data: Arc::new(Mutex::new(Data {
                genesis: Some(0),
                head: Some(Head {
                    revision: 3,
                    digest: 3,
                }),
                non_empty: true,
                records: (1..=3)
                    .map(|revision| {
                        (
                            revision,
                            (
                                revision - 1,
                                Head {
                                    revision,
                                    digest: revision,
                                },
                                true,
                            ),
                        )
                    })
                    .collect(),
                projection: 0,
                effects: 0,
                compaction: 0,
                visited: Vec::new(),
            })),
        }
    }
}

impl RecoveryStore for Store {
    type Error = String;
    type Gate = Gate;
    type Keys = std::iter::Empty<()>;

    fn gate(&self) -> &Gate { &self.gate }
    fn head_revision(&self) -> Result<Option<u64>, String> {
        Ok(self.data.lock().unwrap().head.map(|head| head.revision))
    }
    fn projection_cursor(&self) -> Result<u64, String> { Ok(self.data.lock().unwrap().projection) }
    fn effects_cursor(&self) -> Result<u64, String> { Ok(self.data.lock().unwrap().effects) }
    fn compaction_cursor(&self) -> Result<u64, String> { Ok(self.data.lock().unwrap().compaction) }
    fn round_complete(&self, _: u64) -> Result<bool, String> {
        Err("audit attempted effect selection".to_string())
    }
    fn write_effects_cursor(&self, _: u64) -> Result<(), String> {
        Err("audit attempted an effects write".to_string())
    }
    fn receipt_keys(&self, _: u64) -> Result<Self::Keys, String> {
        Err("audit attempted receipt enumeration".to_string())
    }
    fn delete_receipts(&self, _: Vec<()>) -> Result<(), String> {
        Err("audit attempted receipt deletion".to_string())
    }
    fn write_compaction_cursor(&self, _: u64) -> Result<(), String> {
        Err("audit attempted a compaction write".to_string())
    }
    fn serialization(message: String) -> String { message }
    fn invalid(message: String) -> String { message }
    fn projection_pending(revision: u64, projected_revision: u64) -> String {
        format!("pending:{revision}:{projected_revision}")
    }
}

impl IntegrityStore for Store {
    type Genesis = u64;
    type Head = Head;

    fn genesis(&self) -> Result<Option<u64>, String> { Ok(self.data.lock().unwrap().genesis) }
    fn head(&self) -> Result<Option<Head>, String> { Ok(self.data.lock().unwrap().head) }
    fn non_empty(&self) -> Result<bool, String> { Ok(self.data.lock().unwrap().non_empty) }
    fn genesis_head(genesis: &u64) -> Head {
        Head {
            revision: 0,
            digest: *genesis,
        }
    }
    fn revision(head: &Head) -> u64 { head.revision }

    fn validate_initialized_endpoints(&self, genesis: &u64, head: &Head) -> Result<(), String> {
        recovery_pages::validate_cursor_bounds(self, head.revision)?;
        if *genesis != 0 || self.record_head(head.revision)? != Some(*head) {
            return Err("invalid initialization".to_string());
        }
        Ok(())
    }

    fn record_head(&self, revision: u64) -> Result<Option<Head>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .records
            .get(&revision)
            .map(|(_, head, _)| *head))
    }

    fn validate_next(&self, expected: &Head, revision: u64) -> Result<Head, String> {
        let mut data = self.data.lock().unwrap();
        data.visited.push(revision);
        let (predecessor, head, valid) = data.records.get(&revision).ok_or("missing record")?;
        if !valid || *predecessor != expected.digest || revision != expected.revision + 1 {
            return Err("invalid record".to_string());
        }
        Ok(*head)
    }
}

struct Audit {
    captured: Captured<u64, Head>,
    failure: Option<String>,
}

impl Audit {
    fn start(store: &Store) -> Result<Self, String> {
        Ok(Self {
            captured: integrity_pages::capture(store)?,
            failure: None,
        })
    }

    fn page(&mut self, store: &Store, budget: usize) -> Result<bool, String> {
        integrity_pages::validate_page(
            store,
            Integrity {
                snapshot: &self.captured.snapshot,
                validated: &mut self.captured.validated,
                failure: &mut self.failure,
                complete: &mut self.captured.complete,
            },
            NonZeroUsize::new(budget).unwrap(),
        )
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

#[test]
fn two_native_audit_kernels_keep_their_captured_head_during_append() {
    explore(|| {
        let store = Store::new(true);
        {
            let mut data = store.data.lock().unwrap();
            data.head = Some(Head {
                revision: 2,
                digest: 2,
            });
            data.records.remove(&3);
        }
        let first = Audit::start(&store).unwrap();
        {
            let mut data = store.data.lock().unwrap();
            let head = Head {
                revision: 3,
                digest: 3,
            };
            data.records.insert(3, (2, head, true));
            data.head = Some(head);
        }
        let second = Audit::start(&store).unwrap();
        let readers = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, mut audit)| {
                let store = store.clone();
                thread::spawn(move || {
                    assert!(!audit.page(&store, index + 1).unwrap());
                    audit
                })
            })
            .collect::<Vec<_>>();
        let writer = {
            let store = store.clone();
            thread::spawn(move || {
                let _guard = store.gate.lock();
                let mut data = store.data.lock().unwrap();
                let head = Head {
                    revision: 4,
                    digest: 4,
                };
                data.records.insert(4, (3, head, true));
                data.head = Some(head);
            })
        };
        let audits = readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect::<Vec<_>>();
        writer.join().unwrap();
        for (index, mut audit) in audits.into_iter().enumerate() {
            let target = index as u64 + 2;
            while !audit.page(&store, 2).unwrap() {}
            assert_eq!(
                audit.captured.validated,
                Some(Head {
                    revision: target,
                    digest: target
                })
            );
            assert_eq!(audit.captured.snapshot.unwrap().1.revision, target);
        }
        let mut visited = store.data.lock().unwrap().visited.clone();
        visited.sort_unstable();
        assert_eq!(visited, vec![1, 1, 2, 2, 3]);
    });
}

fn concurrent_cursor_updates(enabled: bool) {
    let store = Store::new(enabled);
    let mut audit = Audit::start(&store).unwrap();
    let reader = {
        let store = store.clone();
        thread::spawn(move || {
            assert!(
                audit.page(&store, 1).is_ok(),
                "negative control: audit observed incoherent cursor reads"
            );
        })
    };
    let writer = thread::spawn(move || {
        let _guard = store.gate.lock();
        store.data.lock().unwrap().projection = 1;
        store.data.lock().unwrap().effects = 1;
    });
    reader.join().unwrap();
    writer.join().unwrap();
}

#[test]
fn audit_page_guard_preserves_coherent_cursors() { explore(|| concurrent_cursor_updates(true)); }

#[test]
#[should_panic(expected = "negative control: audit observed incoherent cursor reads")]
fn missing_audit_gate_can_reject_a_valid_concurrent_ledger() {
    explore(|| concurrent_cursor_updates(false));
}

#[test]
fn failed_page_cannot_publish_a_partial_prefix_or_resume_after_repair() {
    explore(|| {
        let store = Store::new(true);
        let mut audit = Audit::start(&store).unwrap();
        store.data.lock().unwrap().records.get_mut(&2).unwrap().2 = false;
        assert_eq!(audit.page(&store, 3), Err("invalid record".to_string()));
        assert_eq!(
            audit.captured.validated,
            Some(Head {
                revision: 0,
                digest: 0
            })
        );
        assert!(!audit.captured.complete);
        assert!(store.gate.mutex.try_lock().is_ok());
        let previous_reads = store.data.lock().unwrap().visited.len();
        store.data.lock().unwrap().records.get_mut(&2).unwrap().2 = true;
        assert_eq!(audit.page(&store, 3), Err("invalid record".to_string()));
        assert_eq!(store.data.lock().unwrap().visited.len(), previous_reads);
        let mut restarted = Audit::start(&store).unwrap();
        assert_eq!(restarted.captured.validated.unwrap().revision, 0);
        assert!(restarted.page(&store, 3).unwrap());
    });
}

#[test]
fn changed_audit_identity_is_terminal_and_empty_storage_is_distinct() {
    for change in 0..4 {
        explore(move || {
            let store = Store::new(true);
            let mut audit = Audit::start(&store).unwrap();
            assert!(!audit.page(&store, 1).unwrap());
            {
                let mut data = store.data.lock().unwrap();
                match change {
                    0 => data.genesis = Some(1),
                    1 => data.head = None,
                    2 => data.head.as_mut().unwrap().digest = 99,
                    _ => {
                        data.head = Some(Head {
                            revision: 4,
                            digest: 4,
                        });
                        data.records.get_mut(&3).unwrap().1.digest = 99;
                    }
                }
            }
            assert!(audit.page(&store, 1).is_err());
            assert!(!audit.captured.complete);
            assert_eq!(audit.captured.validated.unwrap().revision, 1);
        });
    }
    for non_empty in [false, true] {
        explore(move || {
            let store = Store::new(true);
            {
                let mut data = store.data.lock().unwrap();
                data.genesis = None;
                data.head = None;
                data.non_empty = non_empty;
                data.records.clear();
            }
            let result = Audit::start(&store);
            if non_empty {
                assert!(result.is_err());
            } else {
                assert!(result.unwrap().captured.complete);
            }
        });
    }
}
