// See block-storage/src/test/scala/coop/rchain/blockstorage/dag/IndexedBlockDagStorage.scala

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, InsertMode, KeyValueDagRepresentation,
};

fn insert_mode(equivocates: Option<BlockHash>) -> InsertMode {
    match equivocates {
        Some(sibling) => InsertMode::Equivocation { sibling },
        None => InsertMode::Normal,
    }
}

pub struct IndexedBlockDagStorage {
    underlying: BlockDagKeyValueStorage,
    id_to_blocks: DashMap<i64, BlockMessage>,
    current_id: Arc<Mutex<i64>>,
}

impl IndexedBlockDagStorage {
    pub fn new(underlying: BlockDagKeyValueStorage) -> Self {
        Self {
            underlying,
            id_to_blocks: DashMap::new(),
            current_id: Arc::new(Mutex::new(-1)),
        }
    }

    pub fn get_representation(&self) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // P2-12: shared lock for pure-read snapshot path.
        let _lock_guard = self.underlying.global_lock.read();
        self.underlying.get_representation_internal()
    }

    pub fn commit_lfb<V>(
        &self,
        directly: BlockHash,
        ft_value: f32,
        validate: V,
    ) -> Result<crate::rust::dag::finalization_commit::CommitOutcome, KvStoreError>
    where
        V: FnOnce(
            &KeyValueDagRepresentation,
        ) -> Result<(), crate::rust::dag::finalization_commit::CommitRefusal>,
    {
        self.underlying.commit_lfb(directly, ft_value, validate)
    }

    pub fn insert(
        &mut self,
        block: &BlockMessage,
        mode: InsertMode,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // P2-12: insert mutates; exclusive lock.
        let _lock_guard = self.underlying.global_lock.write();
        self.underlying.insert_internal(block, mode)
    }

    pub fn insert_indexed(
        &mut self,
        block: &BlockMessage,
        genesis: &BlockMessage,
        equivocates: Option<BlockHash>,
    ) -> Result<BlockMessage, KvStoreError> {
        // P2-12: insert mutates; exclusive lock.
        let _lock_guard = self.underlying.global_lock.write();

        // Use internal methods to avoid re-acquiring lock
        self.underlying
            .insert_internal(genesis, InsertMode::Approved)?;
        let dag = self.underlying.get_representation_internal()?;
        let next_creator_seq_num = if block.seq_num == 0 {
            dag.latest_message(&block.sender)?
                .map_or(-1, |b| b.sequence_number)
                + 1
        } else {
            block.seq_num
        };

        let mut current_id = self.current_id.lock().unwrap();
        let next_id = if block.seq_num == 0 {
            *current_id + 1
        } else {
            block.seq_num.into()
        };

        let mut new_post_state = block.body.state.clone();
        new_post_state.block_number = next_id;

        let mut modified_block = block.clone();
        modified_block.seq_num = next_creator_seq_num;
        modified_block.body.state = new_post_state;

        self.underlying
            .insert_internal(&modified_block, insert_mode(equivocates))?;
        self.id_to_blocks.insert(next_id, modified_block.clone());
        *current_id = next_id;

        Ok(modified_block)
    }

    pub fn inject(
        &mut self,
        index: i64,
        block: BlockMessage,
        equivocates: Option<BlockHash>,
    ) -> Result<(), KvStoreError> {
        // P2-12: insert mutates; exclusive lock.
        let _lock_guard = self.underlying.global_lock.write();
        self.id_to_blocks.insert(index, block.clone());
        self.underlying
            .insert_internal(&block, insert_mode(equivocates))?;

        Ok(())
    }

    pub async fn record_directly_finalized<F, Fut>(
        &mut self,
        block_hash: BlockHash,
        ft_value: f32,
        finalization_effect: F,
    ) -> Result<(), KvStoreError>
    where
        F: FnMut(&HashSet<BlockHash>) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        self.underlying
            .record_directly_finalized(block_hash, ft_value, finalization_effect)
            .await
    }

    pub fn lookup_by_id(&self, id: i64) -> Result<Option<BlockMessage>, KvStoreError> {
        // DashMap is already thread-safe, so no additional lock needed
        Ok(self.id_to_blocks.get(&id).map(|b| b.clone()))
    }

    pub fn lookup_by_id_unsafe(&self, id: i64) -> BlockMessage {
        // DashMap is already thread-safe, so no additional lock needed
        self.id_to_blocks.get(&id).unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use models::rust::block_implicits::get_random_block;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;
    use crate::rust::dag::block_dag_key_value_storage::InsertMode;

    fn block(number: i64, seq_num: i32, parents: Vec<BlockHash>) -> BlockMessage {
        get_random_block(
            Some(number),
            Some(seq_num),
            None,
            None,
            None,
            None,
            None,
            Some(parents),
            Some(vec![]),
            None,
            None,
            Some(vec![]),
            None,
            None,
        )
    }

    async fn storage() -> IndexedBlockDagStorage {
        let mut kvm = InMemoryStoreManager::new();
        IndexedBlockDagStorage::new(BlockDagKeyValueStorage::new(&mut kvm).await.unwrap())
    }

    #[tokio::test]
    async fn insert_indexed_assigns_sequential_ids_and_serves_lookups() {
        let mut storage = storage().await;
        let genesis = block(0, 0, vec![]);

        let first = storage
            .insert_indexed(
                &block(0, 0, vec![genesis.block_hash.clone()]),
                &genesis,
                None,
            )
            .unwrap();
        assert_eq!(first.body.state.block_number, 0);
        assert_eq!(first.seq_num, 0);

        let second = storage
            .insert_indexed(
                &block(0, 0, vec![genesis.block_hash.clone()]),
                &genesis,
                None,
            )
            .unwrap();
        assert_eq!(second.body.state.block_number, 1);

        assert_eq!(storage.lookup_by_id(0).unwrap(), Some(first.clone()));
        assert_eq!(storage.lookup_by_id(1).unwrap(), Some(second.clone()));
        assert_eq!(storage.lookup_by_id(99).unwrap(), None);
        assert_eq!(storage.lookup_by_id_unsafe(1), second.clone());

        let dag = storage.get_representation().unwrap();
        assert!(dag.contains(&genesis.block_hash).unwrap());
        assert!(dag.contains(&first.block_hash).unwrap());
        assert!(dag.contains(&second.block_hash).unwrap());
    }

    #[tokio::test]
    async fn insert_indexed_keeps_explicit_sequence_numbers() {
        let mut storage = storage().await;
        let genesis = block(0, 0, vec![]);

        let inserted = storage
            .insert_indexed(
                &block(0, 3, vec![genesis.block_hash.clone()]),
                &genesis,
                None,
            )
            .unwrap();
        assert_eq!(inserted.seq_num, 3);
        assert_eq!(inserted.body.state.block_number, 3);
        assert_eq!(storage.lookup_by_id(3).unwrap(), Some(inserted));
    }

    #[tokio::test]
    async fn insert_indexed_invalid_lands_in_the_invalid_set() {
        let mut storage = storage().await;
        let genesis = block(0, 0, vec![]);

        let inserted = storage
            .insert_indexed(
                &block(0, 0, vec![genesis.block_hash.clone()]),
                &genesis,
                Some(BlockHash::from(vec![6; 32])),
            )
            .unwrap();

        let dag = storage.get_representation().unwrap();
        assert!(dag
            .invalid_blocks()
            .expect("invalid blocks")
            .iter()
            .any(|meta| meta.block_hash == inserted.block_hash));
    }

    #[tokio::test]
    async fn inject_registers_a_block_at_an_explicit_index() {
        let mut storage = storage().await;
        let genesis = block(0, 0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();

        let injected = block(1, 1, vec![genesis.block_hash.clone()]);
        storage.inject(7, injected.clone(), None).unwrap();

        assert_eq!(storage.lookup_by_id(7).unwrap(), Some(injected.clone()));
        let dag = storage.get_representation().unwrap();
        assert!(dag.contains(&injected.block_hash).unwrap());
    }

    #[tokio::test]
    async fn record_directly_finalized_marks_held_ancestry() {
        let mut storage = storage().await;
        let genesis = block(0, 0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();
        let b1 = block(1, 1, vec![genesis.block_hash.clone()]);
        storage.insert(&b1, InsertMode::Normal).unwrap();
        let b2 = block(2, 2, vec![b1.block_hash.clone()]);
        storage.insert(&b2, InsertMode::Normal).unwrap();

        storage
            .record_directly_finalized(b2.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();

        let dag = storage.get_representation().unwrap();
        assert_eq!(dag.last_finalized_block(), b2.block_hash);
        assert!(dag.is_finalized(&b1.block_hash).unwrap());
        assert!(dag.is_finalized(&b2.block_hash).unwrap());
    }
}
