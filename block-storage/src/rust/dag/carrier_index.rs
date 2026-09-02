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

use models::rust::deploy_id::DeployLookupId;
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
    carriers: KeyValueTypedStoreImpl<DeployLookupId, Vec<CarrierEntry>>,
    meta: KeyValueTypedStoreImpl<String, i64>,
}

impl CarrierIndex {
    const WATERMARK_KEY: &'static str = "watermark";
    const LAST_PRUNE_KEY: &'static str = "last-prune";
    /// Pruning walks the whole table, so it runs only when the cutoff has
    /// advanced by at least this many blocks since the last walk.
    const PRUNE_STRIDE: i64 = 64;

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
        deploy_id: &DeployLookupId,
        height: i64,
        block_hash: ByteString,
    ) -> Result<(), KvStoreError> {
        let mut row = self.carriers.get_one(deploy_id)?.unwrap_or_default();
        if row.iter().any(|e| e.block_hash == block_hash) {
            return Ok(());
        }
        row.push(CarrierEntry { height, block_hash });
        self.carriers.put_one(deploy_id.clone(), row)
    }

    pub(crate) fn prepare_record_once(
        &self,
        deploy_id: &DeployLookupId,
        height: i64,
        block_hash: ByteString,
    ) -> Result<Option<(ByteString, Option<ByteString>, ByteString)>, KvStoreError> {
        let current = self.carriers.get_one(deploy_id)?;
        if current
            .as_ref()
            .is_some_and(|row| row.iter().any(|entry| entry.block_hash == block_hash))
        {
            return Ok(None);
        }
        let expected = current
            .as_ref()
            .map(|row| self.carriers.encode_value(row))
            .transpose()?;
        let mut replacement = current.unwrap_or_default();
        replacement.push(CarrierEntry { height, block_hash });
        Ok(Some((
            self.carriers.encode_key(deploy_id)?,
            expected,
            self.carriers.encode_value(&replacement)?,
        )))
    }

    pub(crate) fn raw_store(&self) -> &Arc<dyn KeyValueStore> { self.carriers.raw_store() }

    /// True when the index holds NO carrier for the sig. Sound as an
    /// absence proof only when the caller's scan window starts at or
    /// above the watermark.
    pub fn proves_absence(&self, deploy_id: &DeployLookupId) -> Result<bool, KvStoreError> {
        Ok(self
            .carriers
            .get_one(deploy_id)?
            .is_none_or(|row| row.is_empty()))
    }

    pub fn watermark(&self) -> Result<Option<i64>, KvStoreError> {
        self.meta.get_one(&Self::WATERMARK_KEY.to_string())
    }

    /// First-boot initialization: records the height since which every
    /// insert routes through the carrier recording. Returns the effective
    /// watermark (the stored one on every later start).
    pub fn set_watermark_if_absent(&self, w: i64) -> Result<i64, KvStoreError> {
        let key = Self::WATERMARK_KEY.to_string();
        if self.meta.put_one_if_absent(key, w)? {
            return Ok(w);
        }
        self.watermark()?.ok_or_else(|| {
            KvStoreError::KeyNotFound("carrier watermark disappeared after initialization".into())
        })
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
        for (sig, row) in self.carriers.to_map()? {
            let kept: Vec<CarrierEntry> =
                row.iter().filter(|e| e.height >= cutoff).cloned().collect();
            if kept.len() == row.len() {
                continue;
            }
            removed += (row.len() - kept.len()) as u64;
            if kept.is_empty() {
                self.carriers.delete(vec![sig])?;
            } else {
                self.carriers.put_one(sig, kept)?;
            }
        }
        self.meta
            .put_one(Self::LAST_PRUNE_KEY.to_string(), cutoff)?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::thread;

    use models::rust::deploy_id::{DeployIdV6, LegacyDeploySignature};
    use proptest::prelude::*;

    use super::*;

    fn legacy_id(bytes: &[u8]) -> DeployLookupId {
        DeployLookupId::Legacy(LegacyDeploySignature::new(bytes.to_vec()))
    }

    #[test]
    fn record_is_idempotent_per_block_and_absence_flips_on_first_carrier() {
        let index = CarrierIndex::in_memory();
        let deploy_id = legacy_id(b"sig");
        assert!(index.proves_absence(&deploy_id).expect("probe"));
        index
            .record_once(&deploy_id, 5, vec![1; 32])
            .expect("record");
        index
            .record_once(&deploy_id, 5, vec![1; 32])
            .expect("re-record");
        assert!(!index.proves_absence(&deploy_id).expect("probe"));
        let row = index
            .carriers
            .get_one(&deploy_id)
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

    #[test]
    fn concurrent_watermark_initialization_selects_one_durable_value() {
        let index = Arc::new(CarrierIndex::in_memory());
        let barrier = Arc::new(Barrier::new(8));
        let handles = (0..8)
            .map(|watermark| {
                let index = Arc::clone(&index);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    index
                        .set_watermark_if_absent(watermark)
                        .expect("initialize watermark")
                })
            })
            .collect::<Vec<_>>();
        let observed = handles
            .into_iter()
            .map(|handle| handle.join().expect("watermark worker"))
            .collect::<Vec<_>>();
        let stored = index.watermark().expect("read").expect("stored watermark");

        assert!(observed.iter().all(|watermark| *watermark == stored));
        assert!((0..8).contains(&stored));
    }

    #[test]
    fn prune_drops_below_cutoff_and_strides() {
        let index = CarrierIndex::in_memory();
        let old = legacy_id(b"old");
        let mixed = legacy_id(b"mixed");
        let new = DeployLookupId::V6(DeployIdV6::try_from(&[4u8; 32][..]).unwrap());
        let late = legacy_id(b"late");
        index.record_once(&old, 10, vec![1; 32]).expect("record");
        index.record_once(&mixed, 10, vec![2; 32]).expect("record");
        index.record_once(&mixed, 500, vec![3; 32]).expect("record");
        index.record_once(&new, 500, vec![4; 32]).expect("record");

        let removed = index.prune_below(400).expect("prune");
        assert_eq!(removed, 2);
        assert!(
            index.proves_absence(&old).expect("probe"),
            "empty row deleted"
        );
        assert!(!index.proves_absence(&mixed).expect("probe"));
        assert!(!index.proves_absence(&new).expect("probe"));

        index.record_once(&late, 401, vec![5; 32]).expect("record");
        let removed = index.prune_below(402).expect("prune inside stride");
        assert_eq!(removed, 0, "a cutoff inside the stride does not walk");
        let removed = index.prune_below(400 + 64).expect("prune at stride");
        assert_eq!(removed, 1, "the stride boundary walks again");
    }

    proptest! {
        #[test]
        fn pruning_preserves_exactly_the_entries_at_or_above_the_cutoff(
            heights in proptest::collection::vec(0i64..1_000, 0..128),
            cutoff in 0i64..1_000,
        ) {
            let index = CarrierIndex::in_memory();
            let deploy_id = legacy_id(b"property-deploy");
            index.set_watermark_if_absent(17).expect("set watermark");
            for (ordinal, height) in heights.iter().enumerate() {
                let mut block_hash = vec![0u8; 32];
                block_hash[..8].copy_from_slice(&(ordinal as u64).to_le_bytes());
                index
                    .record_once(&deploy_id, *height, block_hash)
                    .expect("record carrier");
            }

            let removed = index.prune_below(cutoff).expect("prune carriers");
            let actual = index
                .carriers
                .get_one(&deploy_id)
                .expect("read carriers")
                .unwrap_or_default()
                .into_iter()
                .map(|entry| entry.height)
                .collect::<Vec<_>>();
            let expected = heights
                .iter()
                .copied()
                .filter(|height| *height >= cutoff)
                .collect::<Vec<_>>();

            prop_assert_eq!(removed as usize, heights.len() - expected.len());
            prop_assert_eq!(actual, expected);
            prop_assert_eq!(index.watermark().expect("read watermark"), Some(17));
        }
    }
}
