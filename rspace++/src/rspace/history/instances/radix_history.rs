// See rspace/src/main/scala/coop/rchain/rspace/history/instances/RadixHistory.
// scala

use std::collections::HashSet;
use std::mem::size_of;
use std::sync::Arc;

use shared::rust::ByteVector;
use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::{HistoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{SourceMeter, sort};
use crate::rspace::history::history::{History, NativePreparedHistory};
use crate::rspace::history::history_action::{HistoryAction, HistoryActionTrait};
use crate::rspace::history::radix_tree::{
    NativeRadixBuilder, Node, RadixTreeImpl, empty_node, hash_node,
};

pub struct RadixHistory {
    root_hash: Blake2b256Hash,
    root_node: Node,
    imple: Arc<RadixTreeImpl>,
    store: Arc<dyn KeyValueStore>,
}

impl RadixHistory {
    pub fn create(
        root: Blake2b256Hash,
        store: Arc<dyn KeyValueStore>,
    ) -> Result<RadixHistory, HistoryError> {
        let imple = Arc::new(RadixTreeImpl::new(store.clone()));
        let node = imple.load_node(root.bytes(), Some(true))?;

        Ok(RadixHistory {
            root_hash: root,
            root_node: node,
            imple,
            store,
        })
    }

    pub fn create_store(store: Arc<dyn KeyValueStore>) -> Arc<dyn KeyValueStore> { store }

    pub fn empty_root_node_hash() -> Blake2b256Hash {
        let node_hash_bytes = hash_node(&empty_node()).0;

        Blake2b256Hash::from_bytes(node_hash_bytes)
    }

    fn has_no_duplicates(&self, actions: &Vec<HistoryAction>) -> bool {
        let keys: HashSet<_> = actions.iter().map(|action| action.key()).collect();
        keys.len() == actions.len()
    }

    fn has_no_duplicates_native(
        actions: &[HistoryAction],
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        let backing = actions
            .len()
            .checked_mul(size_of::<&[u8]>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            actions
                .len()
                .checked_add(1)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
            backing,
        )?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(actions.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for action in actions {
            keys.push(match action {
                HistoryAction::Insert(insert) => insert.key.as_slice(),
                HistoryAction::Delete(delete) => delete.key.as_slice(),
            });
        }
        let reserve = |operations, scanned, backing| meter.reserve(operations, scanned, backing);
        sort(&mut keys, |key| key, &reserve)?;
        for pair in keys.windows(2) {
            meter.reserve(1, pair[0].len().min(pair[1].len()), 0)?;
            if pair[0] == pair[1] {
                meter.reserve(1, 0, "Cannot process duplicate actions on one key.".len())?;
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl History for RadixHistory {
    fn read(&self, key: ByteVector) -> Result<Option<ByteVector>, HistoryError> {
        let read_result = self.imple.read(&self.root_node, key.as_slice())?;
        Ok(read_result)
    }

    fn process(&self, actions: Vec<HistoryAction>) -> Result<Box<dyn History>, HistoryError> {
        if !self.has_no_duplicates(&actions) {
            return Err(HistoryError::ActionError(
                "Cannot process duplicate actions on one key.".to_string(),
            ));
        }

        let new_root_node_opt = self.imple.make_actions(&self.root_node, actions)?;

        match new_root_node_opt {
            Some(new_root_node) => {
                let node_hash_bytes = self.imple.save_node(new_root_node.clone());
                let root_hash = Blake2b256Hash::from_bytes(node_hash_bytes);
                // Avoid cloning RadixTreeImpl caches into each checkpointed history instance.
                // A fresh tree backed by the same store preserves correctness and reduces
                // allocator pressure from DashMap clone paths.
                let new_imple = Arc::new(RadixTreeImpl::new(self.store.clone()));
                let new_history = RadixHistory {
                    root_hash,
                    root_node: new_root_node,
                    imple: new_imple,
                    store: self.store.clone(),
                };
                self.imple.commit()?;

                self.imple.clear_write_cache();
                self.imple.clear_read_cache();

                Ok(Box::new(new_history))
            }
            None => Ok(Box::new(RadixHistory {
                root_hash: self.root_hash.clone(),
                root_node: self.root_node.clone(),
                imple: Arc::new(RadixTreeImpl::new(self.store.clone())),
                store: self.store.clone(),
            })),
        }
    }

    fn prepare_native(
        &self,
        actions: Vec<HistoryAction>,
        meter: &dyn SourceMeter,
    ) -> Result<NativePreparedHistory, RSpaceError> {
        if !Self::has_no_duplicates_native(&actions, meter)? {
            return Err(HistoryError::ActionError(
                "Cannot process duplicate actions on one key.".to_string(),
            )
            .into());
        }
        self.imple.reserve_native_new(meter, false)?;
        let tree = RadixTreeImpl::new(self.store.clone());
        let builder = NativeRadixBuilder::new(&tree, meter);
        let updated = builder.make_actions(&self.root_node, actions)?;
        let Some(root_node) = updated else {
            let root_node = builder.clone_root(&self.root_node)?;
            let backing = size_of::<RadixHistory>()
                .checked_add(self.root_hash.0.len())
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(3, self.root_hash.0.len(), backing)?;
            self.imple.reserve_native_new(meter, true)?;
            return Ok(NativePreparedHistory {
                next: Box::new(RadixHistory {
                    root_hash: self.root_hash.clone(),
                    root_node,
                    imple: Arc::new(RadixTreeImpl::new(self.store.clone())),
                    store: self.store.clone(),
                }),
                store: self.store.clone(),
                writes: Vec::new(),
            });
        };
        let root_hash = Blake2b256Hash::from_bytes(tree.save_native_root(&root_node, meter)?);
        let writes = tree.prepare_native_commit(meter)?;
        meter.reserve(2, 0, size_of::<RadixHistory>())?;
        self.imple.reserve_native_new(meter, true)?;
        Ok(NativePreparedHistory {
            next: Box::new(RadixHistory {
                root_hash,
                root_node,
                imple: Arc::new(RadixTreeImpl::new(self.store.clone())),
                store: self.store.clone(),
            }),
            store: self.store.clone(),
            writes,
        })
    }

    fn root_ref(&self) -> &Blake2b256Hash { &self.root_hash }

    fn reset(&self, root: &Blake2b256Hash) -> Result<Box<dyn History>, HistoryError> {
        let imple = Arc::new(RadixTreeImpl::new(self.store.clone()));
        let node = imple.load_node(root.bytes(), Some(true))?;

        Ok(Box::new(RadixHistory {
            root_hash: root.clone(),
            root_node: node,
            imple,
            store: self.store.clone(),
        }))
    }

    fn snapshot_native(&self, meter: &dyn SourceMeter) -> Result<Box<dyn History>, RSpaceError> {
        let root_node = NativeRadixBuilder::new(&self.imple, meter).clone_root(&self.root_node)?;
        let bytes = size_of::<RadixHistory>()
            .checked_add(self.root_hash.0.len())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(2, self.root_hash.0.len(), bytes)?;
        Ok(Box::new(RadixHistory {
            root_hash: self.root_hash.clone(),
            root_node,
            imple: Arc::clone(&self.imple),
            store: Arc::clone(&self.store),
        }))
    }
}
