// Repeat-deploy carrier index: a dedicated per-signature record of every
// carrier block in the DAG — valid, invalid, and settled alike — kept
// OUTSIDE the deploy-lifecycle tables so it carries no lifecycle
// semantics to violate (rows prune by height, never by terminal writes)
// and shares no keyspace with attacker-influencable rows (the maintainer
// review of PR #382 demonstrated a completeness-marker forgery through an
// unverified `rejected_deploys` sig landing in the events table).
//
// Completeness contract: `insert` records a block's body sigs here BEFORE
// the block becomes DAG-visible. The persisted height watermark W is the
// height since which that contract has held on this database; the fast
// path engages only for scan windows that start at or above W, so no
// startup backfill is ever needed. Rows at heights below the expiration
// window are pruned on floor advances.

use std::sync::Arc;

use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierEntry {
    pub height: i64,
    pub block_hash: ByteString,
}

#[derive(Clone)]
pub struct CarrierIndex {
    carriers: KeyValueTypedStoreImpl<ByteString, Vec<CarrierEntry>>,
    meta: KeyValueTypedStoreImpl<String, i64>,
}

impl CarrierIndex {
    const WATERMARK_KEY: &'static str = "watermark";
    const LAST_PRUNE_KEY: &'static str = "last-prune";
    /// Pruning walks the whole table, so it runs only when the cutoff has
    /// advanced by at least this many blocks since the last walk.
    const PRUNE_STRIDE: i64 = 64;
    const PRUNE_WRITE_CHUNK: usize = 1024;

    pub fn new(carriers_kv: Arc<dyn KeyValueStore>, meta_kv: Arc<dyn KeyValueStore>) -> Self {
        Self {
            carriers: KeyValueTypedStoreImpl::new(carriers_kv),
            meta: KeyValueTypedStoreImpl::new(meta_kv),
        }
    }

    /// In-memory index for test fixtures that build a representation from
    /// raw components.
    pub fn in_memory() -> Self {
        Self {
            carriers: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            meta: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        }
    }

    /// Record one carrier for a sig. Idempotent per (sig, block_hash), so
    /// a crash-then-redelivery re-run writes nothing twice.
    pub fn record_once(
        &self,
        sig: &[u8],
        height: i64,
        block_hash: ByteString,
    ) -> Result<(), KvStoreError> {
        let mut row = self.carriers.get_one(&sig.to_vec())?.unwrap_or_default();
        if row.iter().any(|e| e.block_hash == block_hash) {
            return Ok(());
        }
        row.push(CarrierEntry { height, block_hash });
        self.carriers.put_one(sig.to_vec(), row)
    }

    /// True when the index holds NO carrier for the sig. Sound as an
    /// absence proof only when the caller's scan window starts at or
    /// above the watermark.
    pub fn proves_absence(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        Ok(self
            .carriers
            .get_one(&sig.to_vec())?
            .is_none_or(|row| row.is_empty()))
    }

    pub fn watermark(&self) -> Result<Option<i64>, KvStoreError> {
        self.meta.get_one(&Self::WATERMARK_KEY.to_string())
    }

    /// First-boot initialization: records the height since which every
    /// insert routes through the carrier recording. Returns the effective
    /// watermark (the stored one on every later start).
    pub fn set_watermark_if_absent(&self, w: i64) -> Result<i64, KvStoreError> {
        if let Some(existing) = self.watermark()? {
            return Ok(existing);
        }
        self.meta.put_one(Self::WATERMARK_KEY.to_string(), w)?;
        Ok(w)
    }

    /// Drop entries below the cutoff (they are below every future scan
    /// window). Strided: the full-table walk runs only when the cutoff
    /// advanced by `PRUNE_STRIDE` since the last walk. Returns the number
    /// of entries removed.
    pub fn prune_below(&self, cutoff: i64) -> Result<u64, KvStoreError> {
        let last = self
            .meta
            .get_one(&Self::LAST_PRUNE_KEY.to_string())?
            .unwrap_or(i64::MIN);
        if last != i64::MIN && cutoff < last.saturating_add(Self::PRUNE_STRIDE) {
            return Ok(0);
        }
        let mut removed: u64 = 0;
        let mut emptied: Vec<ByteString> = Vec::new();
        let mut trimmed: Vec<(ByteString, Vec<CarrierEntry>)> = Vec::new();
        for (sig, row) in self.carriers.to_map()? {
            let kept: Vec<CarrierEntry> =
                row.iter().filter(|e| e.height >= cutoff).cloned().collect();
            if kept.len() == row.len() {
                continue;
            }
            removed += (row.len() - kept.len()) as u64;
            if kept.is_empty() {
                emptied.push(sig);
            } else {
                trimmed.push((sig, kept));
            }
        }
        for chunk in emptied.chunks(Self::PRUNE_WRITE_CHUNK) {
            self.carriers.delete(chunk.to_vec())?;
        }
        for chunk in trimmed.chunks(Self::PRUNE_WRITE_CHUNK) {
            self.carriers.put(chunk.to_vec())?;
        }
        self.meta
            .put_one(Self::LAST_PRUNE_KEY.to_string(), cutoff)?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_idempotent_per_block_and_absence_flips_on_first_carrier() {
        let index = CarrierIndex::in_memory();
        assert!(index.proves_absence(b"sig").expect("probe"));
        index.record_once(b"sig", 5, vec![1; 32]).expect("record");
        index
            .record_once(b"sig", 5, vec![1; 32])
            .expect("re-record");
        assert!(!index.proves_absence(b"sig").expect("probe"));
        let row = index
            .carriers
            .get_one(&b"sig".to_vec())
            .expect("read")
            .expect("row");
        assert_eq!(row.len(), 1, "redelivery must not duplicate");
    }

    #[test]
    fn watermark_is_write_once() {
        let index = CarrierIndex::in_memory();
        assert_eq!(index.watermark().expect("read"), None);
        assert_eq!(index.set_watermark_if_absent(7).expect("set"), 7);
        assert_eq!(index.set_watermark_if_absent(99).expect("re-set"), 7);
        assert_eq!(index.watermark().expect("read"), Some(7));
    }

    #[derive(Clone)]
    struct WriteCountingStore {
        inner: InMemoryKeyValueStore,
        writes: Arc<std::sync::atomic::AtomicUsize>,
        largest_batch: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl KeyValueStore for WriteCountingStore {
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
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.largest_batch
                .fetch_max(kv_pairs.len(), std::sync::atomic::Ordering::SeqCst);
            self.inner.put(kv_pairs)
        }

        fn put_one_if_absent(
            &self,
            key: shared::rust::ByteBuffer,
            value: shared::rust::ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.put_one_if_absent(key, value)
        }

        fn delete(&self, keys: Vec<shared::rust::ByteBuffer>) -> Result<usize, KvStoreError> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.largest_batch
                .fetch_max(keys.len(), std::sync::atomic::Ordering::SeqCst);
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

    #[test]
    fn a_prune_walk_writes_the_carrier_table_in_a_bounded_number_of_batches() {
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let carriers = WriteCountingStore {
            inner: InMemoryKeyValueStore::new(),
            writes: writes.clone(),
            largest_batch: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        let index = CarrierIndex::new(Arc::new(carriers), Arc::new(InMemoryKeyValueStore::new()));
        for i in 0u32..200 {
            let sig = format!("old-{i}").into_bytes();
            index.record_once(&sig, 10, vec![1; 32]).expect("record");
            let sig = format!("mixed-{i}").into_bytes();
            index.record_once(&sig, 10, vec![2; 32]).expect("record");
            index.record_once(&sig, 500, vec![3; 32]).expect("record");
        }
        writes.store(0, std::sync::atomic::Ordering::SeqCst);

        let removed = index.prune_below(400).expect("prune");

        assert_eq!(removed, 400);
        assert!(
            writes.load(std::sync::atomic::Ordering::SeqCst) <= 2,
            "a prune walk must not commit once for each changed row"
        );
        assert!(index.proves_absence(b"old-7").expect("probe"));
        assert!(!index.proves_absence(b"mixed-7").expect("probe"));
    }

    #[test]
    fn a_prune_walk_writes_no_batch_larger_than_the_write_chunk() {
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let largest_batch = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let carriers = WriteCountingStore {
            inner: InMemoryKeyValueStore::new(),
            writes: writes.clone(),
            largest_batch: largest_batch.clone(),
        };
        let index = CarrierIndex::new(Arc::new(carriers), Arc::new(InMemoryKeyValueStore::new()));
        let rows = 2 * CarrierIndex::PRUNE_WRITE_CHUNK + 1;
        for i in 0..rows {
            let sig = format!("old-{i}").into_bytes();
            index.record_once(&sig, 10, vec![1; 32]).expect("record");
        }
        largest_batch.store(0, std::sync::atomic::Ordering::SeqCst);
        writes.store(0, std::sync::atomic::Ordering::SeqCst);

        let removed = index.prune_below(400).expect("prune");

        assert_eq!(removed, rows as u64);
        assert!(
            largest_batch.load(std::sync::atomic::Ordering::SeqCst)
                <= CarrierIndex::PRUNE_WRITE_CHUNK
        );
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(index
            .proves_absence(format!("old-{}", rows - 1).as_bytes())
            .expect("probe"));
    }

    #[test]
    fn prune_drops_below_cutoff_and_strides() {
        let index = CarrierIndex::in_memory();
        index.record_once(b"old", 10, vec![1; 32]).expect("record");
        index
            .record_once(b"mixed", 10, vec![2; 32])
            .expect("record");
        index
            .record_once(b"mixed", 500, vec![3; 32])
            .expect("record");
        index.record_once(b"new", 500, vec![4; 32]).expect("record");

        let removed = index.prune_below(400).expect("prune");
        assert_eq!(removed, 2);
        assert!(
            index.proves_absence(b"old").expect("probe"),
            "empty row deleted"
        );
        assert!(!index.proves_absence(b"mixed").expect("probe"));
        assert!(!index.proves_absence(b"new").expect("probe"));

        index
            .record_once(b"late", 401, vec![5; 32])
            .expect("record");
        let removed = index.prune_below(402).expect("prune inside stride");
        assert_eq!(removed, 0, "a cutoff inside the stride does not walk");
        let removed = index.prune_below(400 + 64).expect("prune at stride");
        assert_eq!(removed, 1, "the stride boundary walks again");
    }
}
