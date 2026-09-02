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

use std::sync::Arc;

use models::rust::deploy_id::DeployLookupId;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleEventKind {
    Included {
        is_failed: bool,
    },
    Rejected {
        duplicate: bool,
        /// The CARRIER this record adjudicated — the block whose body held the
        /// copy the forming merge rejected. Carried through from the block's
        /// `rejected_deploys` so the register decides coverage by equality
        /// rather than re-inferring it from ancestry and a distance bound.
        carrier: ByteString,
    },
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

/// The two lifecycle tables, bundled so the storage and every
/// representation carry one handle. Callers serialize row mutations
/// through the `RwLock` the owner wraps this in (the insert path and the
/// finality evaluator write; the status API reads).
#[derive(Clone)]
pub struct DeployLifecycleTables {
    events: KeyValueTypedStoreImpl<DeployLookupId, LifecycleEvents>,
    terminal: KeyValueTypedStoreImpl<DeployLookupId, TerminalRecord>,
}

impl DeployLifecycleTables {
    pub(crate) fn events_store(&self) -> &KeyValueTypedStoreImpl<DeployLookupId, LifecycleEvents> {
        &self.events
    }

    pub(crate) fn terminal_store(&self) -> &KeyValueTypedStoreImpl<DeployLookupId, TerminalRecord> {
        &self.terminal
    }

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

    pub fn get_events(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<LifecycleEvents>, KvStoreError> {
        let results = self.events.get(&vec![deploy_id.clone()])?;
        Ok(results.into_iter().next().flatten())
    }

    /// The deploy's most recent canonical appearance is the latest inclusion
    /// event by (height, hash) whose block `is_visible` accepts, or the
    /// terminal record's frozen display block once the row is pruned. A
    /// rejection record's block holds the record, not the deploy, so
    /// record-only rows have no appearance. The visibility filter guards
    /// against orphan events from a crash inside `insert`'s ingest-first
    /// window: an event whose block never became DAG-visible must not
    /// resolve as an appearance. The terminal path needs no filter — the
    /// finality evaluator writes terminals from DAG facts (floor-closure
    /// guarded), so the frozen block was visible at the write.
    pub fn canonical_appearance(
        &self,
        deploy_id: &DeployLookupId,
        is_visible: &dyn Fn(&[u8]) -> bool,
    ) -> Result<Option<ByteString>, KvStoreError> {
        if let Some(terminal) = self.get_terminal(deploy_id)? {
            if terminal.latest_block_hash.is_empty() {
                return Ok(None);
            }
            return Ok(Some(terminal.latest_block_hash));
        }
        let Some(row) = self.get_events(deploy_id)? else {
            return Ok(None);
        };
        Ok(row
            .events
            .iter()
            .filter(|e| matches!(e.kind, LifecycleEventKind::Included { .. }))
            .filter(|e| is_visible(&e.block_hash))
            .max_by(|a, b| {
                a.height
                    .cmp(&b.height)
                    .then_with(|| a.block_hash.cmp(&b.block_hash))
            })
            .map(|e| e.block_hash.clone()))
    }

    /// Append events for a sig, recording `valid_after` with the first
    /// inclusion. No-op for sigs that already have a terminal record.
    pub fn append_events(
        &self,
        deploy_id: &DeployLookupId,
        valid_after: Option<i64>,
        new_events: Vec<LifecycleEvent>,
    ) -> Result<(), KvStoreError> {
        if self.get_terminal(deploy_id)?.is_some() {
            return Ok(());
        }
        let mut row = self.get_events(deploy_id)?.unwrap_or(LifecycleEvents {
            valid_after: None,
            events: Vec::new(),
        });
        if row.valid_after.is_none() {
            row.valid_after = valid_after;
        }
        row.events.extend(new_events);
        self.events.put_one(deploy_id.clone(), row)
    }

    /// Every sig with an open event row — the schedule rebuild's input.
    /// Rows are pruned at the terminal write, so this enumerates OPEN
    /// sigs, not history.
    pub fn open_sigs(&self) -> Result<Vec<DeployLookupId>, KvStoreError> {
        self.events.to_map().map(|m| m.into_keys().collect())
    }

    /// Redelivery-idempotent append is a no-op when the row already holds an
    /// event for the same block with the same kind of testimony, so a
    /// crash-then-redelivery re-run never duplicates an event.
    pub fn append_event_once(
        &self,
        deploy_id: &DeployLookupId,
        valid_after: Option<i64>,
        event: LifecycleEvent,
    ) -> Result<(), KvStoreError> {
        if let Some(row) = self.get_events(deploy_id)? {
            let duplicate = row.events.iter().any(|e| {
                e.block_hash == event.block_hash
                    && std::mem::discriminant(&e.kind) == std::mem::discriminant(&event.kind)
            });
            if duplicate {
                return Ok(());
            }
        }
        self.append_events(deploy_id, valid_after, vec![event])
    }

    pub fn get_terminal(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<TerminalRecord>, KvStoreError> {
        let results = self.terminal.get(&vec![deploy_id.clone()])?;
        Ok(results.into_iter().next().flatten())
    }

    /// WRITE-ONCE: refuses to overwrite an existing terminal record and
    /// returns the survivor; prunes the event row on a fresh write.
    /// Terminal states are monotone facts — once written, nothing may
    /// flip them, including a buggy second evaluation.
    pub fn put_terminal_if_absent(
        &self,
        deploy_id: &DeployLookupId,
        record: TerminalRecord,
    ) -> Result<TerminalRecord, KvStoreError> {
        if let Some(existing) = self.get_terminal(deploy_id)? {
            return Ok(existing);
        }
        self.terminal.put_one(deploy_id.clone(), record.clone())?;
        self.events.delete(vec![deploy_id.clone()])?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use models::rust::deploy_id::LegacyDeploySignature;

    use super::*;

    fn deploy_id() -> DeployLookupId {
        DeployLookupId::Legacy(LegacyDeploySignature::new(b"sig".to_vec()))
    }

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
        let deploy_id = deploy_id();
        tables
            .append_events(&deploy_id, Some(5), vec![included(6, false)])
            .expect("first append");
        tables
            .append_events(&deploy_id, Some(7), vec![included(8, true)])
            .expect("second append");
        let row = tables.get_events(&deploy_id).expect("read").expect("row");
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
        let deploy_id = deploy_id();
        tables
            .append_events(&deploy_id, Some(1), vec![included(2, false)])
            .expect("append");
        let first = tables
            .put_terminal_if_absent(&deploy_id, terminal(TerminalState::Finalized))
            .expect("first terminal");
        assert_eq!(first.state, TerminalState::Finalized);
        let survivor = tables
            .put_terminal_if_absent(&deploy_id, terminal(TerminalState::Expired))
            .expect("second terminal");
        assert_eq!(
            survivor.state,
            TerminalState::Finalized,
            "a terminal record never flips"
        );
        assert!(
            tables.get_events(&deploy_id).expect("read").is_none(),
            "the event row is pruned at the terminal write"
        );
        assert!(
            tables.open_sigs().expect("open").is_empty(),
            "a terminal sig is no longer open"
        );
    }

    #[test]
    fn append_event_once_is_idempotent_per_block_and_kind() {
        let tables = DeployLifecycleTables::in_memory();
        let deploy_id = deploy_id();
        let event = included(2, false);
        tables
            .append_events(&deploy_id, Some(1), vec![event.clone()])
            .expect("insert-path append");
        tables
            .append_event_once(&deploy_id, Some(1), event.clone())
            .expect("backfill re-append");
        let row = tables.get_events(&deploy_id).expect("read").expect("row");
        assert_eq!(row.events.len(), 1, "backfill must not duplicate events");
        tables
            .append_event_once(&deploy_id, Some(1), included(5, false))
            .expect("distinct block appends");
        let row = tables.get_events(&deploy_id).expect("read").expect("row");
        assert_eq!(row.events.len(), 2);
    }

    #[test]
    fn append_after_terminal_is_a_noop() {
        let tables = DeployLifecycleTables::in_memory();
        let deploy_id = deploy_id();
        tables
            .put_terminal_if_absent(&deploy_id, terminal(TerminalState::Expired))
            .expect("terminal");
        tables
            .append_events(&deploy_id, Some(1), vec![included(2, false)])
            .expect("append after terminal");
        assert!(
            tables.get_events(&deploy_id).expect("read").is_none(),
            "no event row may reopen behind a terminal record"
        );
    }
}
