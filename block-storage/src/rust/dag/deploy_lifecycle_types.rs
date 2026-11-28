// Row types for the deploy-lifecycle tables on `BlockDagKeyValueStorage`
// (`deploy-lifecycle-events`, `deploy-lifecycle-terminal`).
//
// The events table is a per-sig projection of block BODIES, maintained in
// `insert`'s body pass: inclusion events carry the
// execution outcome and the deploy's validity-window start; rejection
// events carry the record's duplicate flag. The terminal table holds the
// WRITE-ONCE verdicts — Finalized, Expired, Failed are monotone facts that
// can never flip, and `put_deploy_terminal_if_absent` refuses overwrites
// so never-flip is a storage property, not a convention. Event rows are
// pruned when a sig goes terminal (display fields freeze into the terminal
// record), so the events table is structurally bounded to OPEN sigs.
//
// The evaluation logic that writes terminal records lives in the casper
// finality layer (`finality::deploy_lifecycle`) — this layer only indexes
// bodies and enforces write-once.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleEventKind {
    Included { is_failed: bool },
    Rejected { duplicate: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub height: i64,
    pub block_hash: ByteString,
    pub kind: LifecycleEventKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleEvents {
    /// `valid_after_block_number` of the deploy, recorded with the first
    /// inclusion event (identical across copies — the sig covers it).
    /// `None` until an inclusion has been observed (record-only rows).
    pub valid_after: Option<i64>,
    pub events: Vec<LifecycleEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalState {
    Finalized,
    Expired,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRecord {
    pub state: TerminalState,
    /// Display fields, frozen at the terminal write so the event row can
    /// be pruned afterwards.
    pub rejection_count: u32,
    pub latest_height: i64,
    pub latest_block_hash: ByteString,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AppendTiming {
    pub reads: Duration,
    pub put: Duration,
}

/// The two lifecycle tables, bundled so the storage and every
/// representation carry one handle. Callers serialize row mutations
/// through the `RwLock` the owner wraps this in (the insert path and the
/// finality evaluator write; the status API reads).
#[derive(Clone)]
pub struct DeployLifecycleTables {
    events: KeyValueTypedStoreImpl<ByteString, LifecycleEvents>,
    terminal: KeyValueTypedStoreImpl<ByteString, TerminalRecord>,
}

impl DeployLifecycleTables {
    pub fn new(events_kv: Arc<dyn KeyValueStore>, terminal_kv: Arc<dyn KeyValueStore>) -> Self {
        Self {
            events: KeyValueTypedStoreImpl::new(events_kv),
            terminal: KeyValueTypedStoreImpl::new(terminal_kv),
        }
    }

    /// In-memory tables for test fixtures that build a representation from
    /// raw components.
    pub fn in_memory() -> Self {
        Self {
            events: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            terminal: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        }
    }

    pub fn get_events(&self, sig: &[u8]) -> Result<Option<LifecycleEvents>, KvStoreError> {
        let results = self.events.get(&vec![sig.to_vec()])?;
        Ok(results.into_iter().next().flatten())
    }

    /// The sig's most recent canonical appearance — the latest INCLUSION
    /// event by (height, hash) whose block `is_visible` accepts, or the
    /// terminal record's frozen display block once the row is pruned. A
    /// rejection record's block holds the record, not the deploy, so
    /// record-only rows have no appearance. The visibility filter guards
    /// against orphan events from a crash inside `insert`'s ingest-first
    /// window: an event whose block never became DAG-visible must not
    /// resolve as an appearance. The terminal path needs no filter — the
    /// finality evaluator freezes only DAG-visible blocks into the record.
    pub fn canonical_appearance(
        &self,
        sig: &[u8],
        is_visible: &dyn Fn(&[u8]) -> Result<bool, KvStoreError>,
    ) -> Result<Option<ByteString>, KvStoreError> {
        if let Some(terminal) = self.get_terminal(sig)? {
            if terminal.latest_block_hash.is_empty() || !is_visible(&terminal.latest_block_hash)? {
                return Ok(None);
            }
            return Ok(Some(terminal.latest_block_hash));
        }
        let Some(row) = self.get_events(sig)? else {
            return Ok(None);
        };
        let mut latest: Option<&LifecycleEvent> = None;
        for event in &row.events {
            if matches!(event.kind, LifecycleEventKind::Included { .. })
                && is_visible(&event.block_hash)?
                && latest.is_none_or(|previous| {
                    (event.height, &event.block_hash) > (previous.height, &previous.block_hash)
                })
            {
                latest = Some(event);
            }
        }
        Ok(latest.map(|event| event.block_hash.clone()))
    }

    /// Append events for a sig, recording `valid_after` with the first
    /// inclusion. No-op for sigs that already have a terminal record.
    pub fn append_events(
        &self,
        sig: &[u8],
        valid_after: Option<i64>,
        new_events: Vec<LifecycleEvent>,
    ) -> Result<(), KvStoreError> {
        if self.get_terminal(sig)?.is_some() {
            return Ok(());
        }
        let mut row = self.get_events(sig)?.unwrap_or(LifecycleEvents {
            valid_after: None,
            events: Vec::new(),
        });
        if row.valid_after.is_none() {
            row.valid_after = valid_after;
        }
        row.events.extend(new_events);
        self.events.put_one(sig.to_vec(), row)
    }

    /// Every open event row. Rows are pruned at the terminal write, so this
    /// enumerates OPEN sigs, not history.
    pub fn open_rows(&self) -> Result<Vec<(ByteString, LifecycleEvents)>, KvStoreError> {
        self.events.to_map().map(|m| m.into_iter().collect())
    }

    /// Drop the open row for `sig` if `discardable` still holds for its
    /// current contents. The caller holds the tables' write lock, so no event
    /// can be appended between the check and the delete. Returns whether the
    /// row was dropped.
    pub fn discard_events_if(
        &self,
        sig: &[u8],
        discardable: impl Fn(&LifecycleEvents) -> bool,
    ) -> Result<bool, KvStoreError> {
        match self.get_events(sig)? {
            Some(row) if discardable(&row) => {
                self.events.delete(vec![sig.to_vec()])?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Redelivery-idempotent append: a no-op when the row already holds an
    /// event for the same block with the same kind of testimony, so a
    /// crash-then-redelivery re-run never duplicates an event.
    pub fn append_event_once(
        &self,
        sig: &[u8],
        valid_after: Option<i64>,
        event: LifecycleEvent,
    ) -> Result<(), KvStoreError> {
        self.append_events_once_batch(vec![(sig.to_vec(), valid_after, event)])
    }

    pub fn append_events_once_batch(
        &self,
        items: Vec<(ByteString, Option<i64>, LifecycleEvent)>,
    ) -> Result<(), KvStoreError> {
        self.append_events_once_batch_timed(items, &mut AppendTiming::default())
    }

    pub fn append_events_once_batch_timed(
        &self,
        items: Vec<(ByteString, Option<i64>, LifecycleEvent)>,
        timing: &mut AppendTiming,
    ) -> Result<(), KvStoreError> {
        let mut rows: BTreeMap<ByteString, (Option<LifecycleEvents>, bool, bool)> = BTreeMap::new();
        for (sig, valid_after, event) in items {
            let (row, terminal, dirty) = match rows.entry(sig) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let started = Instant::now();
                    let row = self.get_events(entry.key())?;
                    let terminal = self.get_terminal(entry.key())?.is_some();
                    timing.reads += started.elapsed();
                    entry.insert((row, terminal, false))
                }
            };
            let duplicate = row.as_ref().is_some_and(|row| {
                row.events.iter().any(|e| {
                    e.block_hash == event.block_hash
                        && std::mem::discriminant(&e.kind) == std::mem::discriminant(&event.kind)
                })
            });
            if duplicate || *terminal {
                continue;
            }
            let row = row.get_or_insert_with(|| LifecycleEvents {
                valid_after: None,
                events: Vec::new(),
            });
            if row.valid_after.is_none() {
                row.valid_after = valid_after;
            }
            row.events.push(event);
            *dirty = true;
        }
        let writes: Vec<_> = rows
            .into_iter()
            .filter_map(|(sig, (row, _, dirty))| {
                if dirty {
                    row.map(|row| (sig, row))
                } else {
                    None
                }
            })
            .collect();
        if writes.is_empty() {
            return Ok(());
        }
        let started = Instant::now();
        let result = self.events.put(writes);
        timing.put += started.elapsed();
        result
    }

    pub fn get_terminal(&self, sig: &[u8]) -> Result<Option<TerminalRecord>, KvStoreError> {
        let results = self.terminal.get(&vec![sig.to_vec()])?;
        Ok(results.into_iter().next().flatten())
    }

    /// WRITE-ONCE: refuses to overwrite an existing terminal record and
    /// returns the survivor; prunes the event row on a fresh write.
    /// Terminal states are monotone facts — once written, nothing may
    /// flip them, including a buggy second evaluation.
    pub fn put_terminal_if_absent(
        &self,
        sig: &[u8],
        record: TerminalRecord,
    ) -> Result<TerminalRecord, KvStoreError> {
        if let Some(existing) = self.get_terminal(sig)? {
            return Ok(existing);
        }
        self.terminal.put_one(sig.to_vec(), record.clone())?;
        self.events.delete(vec![sig.to_vec()])?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn included(height: i64, is_failed: bool) -> LifecycleEvent {
        LifecycleEvent {
            height,
            block_hash: vec![height as u8; 32],
            kind: LifecycleEventKind::Included { is_failed },
        }
    }

    fn terminal(state: TerminalState) -> TerminalRecord {
        TerminalRecord {
            state,
            rejection_count: 0,
            latest_height: 9,
            latest_block_hash: vec![9; 32],
        }
    }

    #[test]
    fn valid_after_records_once_and_events_accumulate() {
        let tables = DeployLifecycleTables::in_memory();
        tables
            .append_events(b"sig", Some(5), vec![included(6, false)])
            .expect("first append");
        tables
            .append_events(b"sig", Some(7), vec![included(8, true)])
            .expect("second append");
        let row = tables.get_events(b"sig").expect("read").expect("row");
        assert_eq!(
            row.valid_after,
            Some(5),
            "the first inclusion's window start stands"
        );
        assert_eq!(row.events.len(), 2);
    }

    #[test]
    fn terminal_is_write_once_and_prunes_the_event_row() {
        let tables = DeployLifecycleTables::in_memory();
        tables
            .append_events(b"sig", Some(1), vec![included(2, false)])
            .expect("append");
        let first = tables
            .put_terminal_if_absent(b"sig", terminal(TerminalState::Finalized))
            .expect("first terminal");
        assert_eq!(first.state, TerminalState::Finalized);
        let survivor = tables
            .put_terminal_if_absent(b"sig", terminal(TerminalState::Expired))
            .expect("second terminal");
        assert_eq!(
            survivor.state,
            TerminalState::Finalized,
            "a terminal record never flips"
        );
        assert!(
            tables.get_events(b"sig").expect("read").is_none(),
            "the event row is pruned at the terminal write"
        );
        assert!(
            tables.open_rows().expect("open").is_empty(),
            "a terminal sig is no longer open"
        );
    }

    #[test]
    fn append_event_once_is_idempotent_per_block_and_kind() {
        let tables = DeployLifecycleTables::in_memory();
        let event = included(2, false);
        tables
            .append_events(b"sig", Some(1), vec![event.clone()])
            .expect("insert-path append");
        tables
            .append_event_once(b"sig", Some(1), event.clone())
            .expect("backfill re-append");
        let row = tables.get_events(b"sig").expect("read").expect("row");
        assert_eq!(row.events.len(), 1, "backfill must not duplicate events");
        tables
            .append_event_once(b"sig", Some(1), included(5, false))
            .expect("distinct block appends");
        let row = tables.get_events(b"sig").expect("read").expect("row");
        assert_eq!(row.events.len(), 2);
    }

    #[test]
    fn a_discard_rechecks_the_row_it_deletes() {
        let tables = DeployLifecycleTables::in_memory();
        let record = LifecycleEvent {
            height: 3,
            block_hash: vec![3; 32],
            kind: LifecycleEventKind::Rejected { duplicate: false },
        };
        tables
            .append_events(b"sig", None, vec![record])
            .expect("record-only row");
        let record_only = |row: &LifecycleEvents| row.valid_after.is_none();
        assert!(
            tables
                .open_rows()
                .expect("snapshot")
                .iter()
                .all(|(_, row)| record_only(row)),
            "fixture precondition: the snapshot sees a record-only row"
        );

        tables
            .append_events(b"sig", Some(1), vec![included(4, false)])
            .expect("an inclusion lands after the snapshot");
        assert!(
            !tables
                .discard_events_if(b"sig", record_only)
                .expect("discard"),
            "the row gained an inclusion, so it is no longer record-only"
        );
        assert!(
            tables.get_events(b"sig").expect("read").is_some(),
            "the inclusion must survive the discard"
        );

        tables
            .append_events(b"other", None, vec![LifecycleEvent {
                height: 5,
                block_hash: vec![5; 32],
                kind: LifecycleEventKind::Rejected { duplicate: false },
            }])
            .expect("record-only row");
        assert!(tables
            .discard_events_if(b"other", record_only)
            .expect("discard"));
        assert!(tables.get_events(b"other").expect("read").is_none());
    }

    #[test]
    fn append_after_terminal_is_a_noop() {
        let tables = DeployLifecycleTables::in_memory();
        tables
            .put_terminal_if_absent(b"sig", terminal(TerminalState::Expired))
            .expect("terminal");
        tables
            .append_events(b"sig", Some(1), vec![included(2, false)])
            .expect("append after terminal");
        assert!(
            tables.get_events(b"sig").expect("read").is_none(),
            "no event row may reopen behind a terminal record"
        );
    }

    #[derive(Clone)]
    struct PutCountingStore {
        inner: InMemoryKeyValueStore,
        puts: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl KeyValueStore for PutCountingStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(
            &self,
            keys: &Vec<shared::rust::ByteBuffer>,
        ) -> Result<Vec<Option<shared::rust::ByteBuffer>>, KvStoreError> {
            self.inner.get(keys)
        }

        fn put(
            &self,
            kv_pairs: Vec<(shared::rust::ByteBuffer, shared::rust::ByteBuffer)>,
        ) -> Result<(), KvStoreError> {
            self.puts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.put(kv_pairs)
        }

        fn put_one_if_absent(
            &self,
            key: shared::rust::ByteBuffer,
            value: shared::rust::ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.puts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.put_one_if_absent(key, value)
        }

        fn delete_if_value(&self, key: &Vec<u8>, expected: &Vec<u8>) -> Result<bool, KvStoreError> {
            self.inner.delete_if_value(key, expected)
        }

        fn delete(&self, keys: Vec<shared::rust::ByteBuffer>) -> Result<usize, KvStoreError> {
            self.inner.delete(keys)
        }

        fn iterate(
            &self,
            f: fn(shared::rust::ByteBuffer, shared::rust::ByteBuffer),
        ) -> Result<(), KvStoreError> {
            self.inner.iterate(f)
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(
                shared::rust::ByteBuffer,
                shared::rust::ByteBuffer,
            ) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            self.inner.iterate_while(f)
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

        fn to_map(
            &self,
        ) -> Result<
            std::collections::BTreeMap<shared::rust::ByteBuffer, shared::rust::ByteBuffer>,
            KvStoreError,
        > {
            self.inner.to_map()
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    fn block_events(count: u8, height: i64) -> Vec<(ByteString, Option<i64>, LifecycleEvent)> {
        let mut items: Vec<_> = (0..count)
            .map(|i| (vec![b's', i], Some(1), included(height, i % 2 == 0)))
            .collect();
        items.push((vec![b's', 0], None, LifecycleEvent {
            height,
            block_hash: vec![height as u8; 32],
            kind: LifecycleEventKind::Rejected { duplicate: true },
        }));
        items
    }

    #[test]
    fn a_block_appends_its_events_in_one_write() {
        let puts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tables = DeployLifecycleTables::new(
            Arc::new(PutCountingStore {
                inner: InMemoryKeyValueStore::new(),
                puts: puts.clone(),
            }),
            Arc::new(InMemoryKeyValueStore::new()),
        );

        tables
            .append_events_once_batch(block_events(20, 4))
            .expect("append block events");

        assert_eq!(puts.load(std::sync::atomic::Ordering::SeqCst), 1);
        tables
            .append_events_once_batch(block_events(20, 4))
            .expect("redeliver block events");
        assert_eq!(
            puts.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a redelivered block must write nothing"
        );
    }

    fn pre_change_append_event_once(
        tables: &DeployLifecycleTables,
        sig: &[u8],
        valid_after: Option<i64>,
        event: LifecycleEvent,
    ) -> Result<(), KvStoreError> {
        if let Some(row) = tables.get_events(sig)? {
            let duplicate = row.events.iter().any(|e| {
                e.block_hash == event.block_hash
                    && std::mem::discriminant(&e.kind) == std::mem::discriminant(&event.kind)
            });
            if duplicate {
                return Ok(());
            }
        }
        tables.append_events(sig, valid_after, vec![event])
    }

    fn batches_with_repeats() -> Vec<Vec<(ByteString, Option<i64>, LifecycleEvent)>> {
        let mut with_duplicate = block_events(20, 4);
        with_duplicate.push((vec![b's', 1], Some(1), included(4, true)));
        vec![with_duplicate, block_events(12, 5), block_events(20, 4)]
    }

    fn tables_with_terminal_sig() -> DeployLifecycleTables {
        let tables = DeployLifecycleTables::in_memory();
        tables
            .put_terminal_if_absent(&[b's', 3], terminal(TerminalState::Finalized))
            .expect("terminal sig");
        tables
    }

    #[test]
    fn a_block_append_matches_the_pre_change_per_event_writer() {
        let batched = tables_with_terminal_sig();
        let reference = tables_with_terminal_sig();
        for items in batches_with_repeats() {
            batched
                .append_events_once_batch(items.clone())
                .expect("append block events");
            for (sig, valid_after, event) in items {
                pre_change_append_event_once(&reference, &sig, valid_after, event)
                    .expect("append event");
            }
        }

        let rows = |tables: &DeployLifecycleTables| {
            tables
                .open_rows()
                .expect("read")
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(rows(&batched), rows(&reference));
    }

    #[test]
    fn a_block_append_stores_each_event_once_and_skips_terminal_sigs() {
        let tables = tables_with_terminal_sig();
        for items in batches_with_repeats() {
            tables
                .append_events_once_batch(items)
                .expect("append block events");
        }

        assert!(tables.get_events(&[b's', 3]).expect("read").is_none());
        let row = tables.get_events(&[b's', 0]).expect("read").expect("row");
        assert_eq!(row.valid_after, Some(1));
        assert_eq!(row.events, vec![
            included(4, true),
            LifecycleEvent {
                height: 4,
                block_hash: vec![4; 32],
                kind: LifecycleEventKind::Rejected { duplicate: true },
            },
            included(5, true),
            LifecycleEvent {
                height: 5,
                block_hash: vec![5; 32],
                kind: LifecycleEventKind::Rejected { duplicate: true },
            },
        ]);
        let row = tables.get_events(&[b's', 1]).expect("read").expect("row");
        assert_eq!(row.events, vec![included(4, false), included(5, false)]);
        assert!(tables.get_events(&[b's', 15]).expect("read").is_some());
        assert_eq!(
            tables
                .get_events(&[b's', 15])
                .expect("read")
                .expect("row")
                .events,
            vec![included(4, false)]
        );
    }
}
