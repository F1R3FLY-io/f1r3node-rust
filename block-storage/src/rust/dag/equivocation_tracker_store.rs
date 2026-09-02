// See block-storage/src/main/scala/coop/rchain/blockstorage/dag/EquivocationTrackerStore.scala

use std::collections::{BTreeSet, HashSet};

use models::rust::block_hash::BlockHashSerde;
use models::rust::bond_generation::BondGeneration;
use models::rust::equivocation_record::{EquivocationRecord, SequenceNumber};
use models::rust::validator::ValidatorSerde;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

#[derive(Clone)]
pub struct EquivocationTrackerStore {
    pub store: KeyValueTypedStoreImpl<
        (ValidatorSerde, BondGeneration, SequenceNumber),
        BTreeSet<BlockHashSerde>,
    >,
}

impl EquivocationTrackerStore {
    pub fn new(
        store: KeyValueTypedStoreImpl<
            (ValidatorSerde, BondGeneration, SequenceNumber),
            BTreeSet<BlockHashSerde>,
        >,
    ) -> Self {
        Self { store }
    }

    pub fn add(&self, record: EquivocationRecord) -> Result<(), KvStoreError> {
        self.store.put_one(
            (
                ValidatorSerde(record.equivocator),
                record.equivocator_bond_generation,
                record.equivocation_base_block_seq_num,
            ),
            record
                .equivocation_detected_block_hashes
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    pub(crate) fn ensure_identity(
        &self,
        equivocator: models::rust::validator::Validator,
        generation: BondGeneration,
        base_sequence_number: SequenceNumber,
    ) -> Result<(), KvStoreError> {
        let key = (
            ValidatorSerde(equivocator),
            generation,
            base_sequence_number,
        );
        if self.store.get_one(&key)?.is_none() {
            self.store.put_one(key, BTreeSet::new())?;
        }
        Ok(())
    }

    pub fn add_all(&mut self, records: Vec<EquivocationRecord>) -> Result<(), KvStoreError> {
        self.store.put(
            records
                .into_iter()
                .map(|record| {
                    (
                        (
                            ValidatorSerde(record.equivocator),
                            record.equivocator_bond_generation,
                            record.equivocation_base_block_seq_num,
                        ),
                        record
                            .equivocation_detected_block_hashes
                            .into_iter()
                            .map(Into::into)
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    pub fn data(&self) -> Result<HashSet<EquivocationRecord>, KvStoreError> {
        self.store.to_map().map(|map| {
            map.into_iter()
                .map(
                    |(
                        (equivocator, equivocator_bond_generation, equivocation_base_block_seq_num),
                        equivocation_detected_block_hashes,
                    )| {
                        EquivocationRecord::new(
                            equivocator.into(),
                            equivocator_bond_generation,
                            equivocation_base_block_seq_num,
                            equivocation_detected_block_hashes
                                .into_iter()
                                .map(Into::into)
                                .collect(),
                        )
                    },
                )
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rust::block_hash::BlockHash;
    use models::rust::validator::Validator;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;

    fn tracker() -> EquivocationTrackerStore {
        EquivocationTrackerStore::new(KeyValueTypedStoreImpl::new(Arc::new(
            InMemoryKeyValueStore::new(),
        )))
    }

    fn record(
        validator_byte: u8,
        generation: i64,
        seq_num: SequenceNumber,
        hash_byte: u8,
    ) -> EquivocationRecord {
        EquivocationRecord::new(
            Validator::from(vec![validator_byte; 65]),
            BondGeneration::new(generation).unwrap(),
            seq_num,
            std::collections::BTreeSet::from([BlockHash::from(vec![hash_byte; 32])]),
        )
    }

    #[test]
    fn data_is_empty_on_a_fresh_store() {
        assert_eq!(tracker().data().unwrap(), HashSet::new());
    }

    #[test]
    fn add_then_data_round_trips_a_record() {
        let tracker = tracker();
        let record = record(1, 2, 0, 0xAA);
        tracker.add(record.clone()).unwrap();
        assert_eq!(tracker.data().unwrap(), HashSet::from([record]));
    }

    #[test]
    fn add_overwrites_the_record_for_the_same_validator_generation_and_seq_num() {
        let tracker = tracker();
        tracker.add(record(1, 2, 0, 0xAA)).unwrap();
        let updated = record(1, 2, 0, 0xBB);
        tracker.add(updated.clone()).unwrap();
        assert_eq!(tracker.data().unwrap(), HashSet::from([updated]));
    }

    #[test]
    fn distinct_bond_generations_have_distinct_records() {
        let tracker = tracker();
        let first = record(1, 2, 0, 0xAA);
        let second = record(1, 3, 0, 0xBB);
        tracker.add(first.clone()).unwrap();
        tracker.add(second.clone()).unwrap();
        assert_eq!(tracker.data().unwrap(), HashSet::from([first, second]));
    }

    #[test]
    fn add_all_stores_every_record() {
        let mut tracker = tracker();
        let records = vec![
            record(1, 0, 0, 0xAA),
            record(2, 1, 3, 0xBB),
            record(3, 2, 7, 0xCC),
        ];
        tracker.add_all(records.clone()).unwrap();
        assert_eq!(
            tracker.data().unwrap(),
            records.into_iter().collect::<HashSet<_>>()
        );
    }
}
