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
