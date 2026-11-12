// See rspace/src/main/scala/coop/rchain/rspace/history/instances/RadixHistory.
// scala

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use shared::rust::ByteVector;
use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::HistoryError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history::History;
use crate::rspace::history::history_action::{HistoryAction, HistoryActionTrait};
use crate::rspace::history::radix_tree::{Node, RadixTreeImpl, empty_node, hash_node};
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

/// A backend-neutral, content-derived local history checkpoint.
///
/// Construction is private to this module.  Consumers can inspect it through
/// narrow crate-private accessors, but cannot supply a claimed root digest.
pub struct PreparedHistoryCheckpoint {
    authority: Arc<RSpaceStateAuthority>,
    base: Blake2b256Hash,
    target_root: Blake2b256Hash,
    root_encoding: Option<ByteVector>,
    history_objects: BTreeMap<ByteVector, ByteVector>,
}

impl PreparedHistoryCheckpoint {
    pub(crate) fn authority(&self) -> &Arc<RSpaceStateAuthority> { &self.authority }

    pub(crate) fn base(&self) -> &Blake2b256Hash { &self.base }

    pub(crate) fn target_root(&self) -> &Blake2b256Hash { &self.target_root }

    pub(crate) fn root_encoding(&self) -> Option<&[u8]> { self.root_encoding.as_deref() }

    pub(crate) fn history_objects(&self) -> &BTreeMap<ByteVector, ByteVector> {
        &self.history_objects
    }

    pub(crate) fn changed(&self) -> bool { self.target_root != self.base }
}

pub struct RadixHistory {
    root_hash: Blake2b256Hash,
    root_node: Node,
    imple: RadixTreeImpl,
    store: Arc<dyn KeyValueStore>,
    authority: Option<Arc<RSpaceStateAuthority>>,
}

impl RadixHistory {
    fn load_root_node(imple: &RadixTreeImpl, root: &Blake2b256Hash) -> Result<Node, HistoryError> {
        let is_canonical_empty = root == &Self::empty_root_node_hash();
        Ok(imple.load_node(root.bytes(), Some(is_canonical_empty))?)
    }

    pub fn create(
        root: Blake2b256Hash,
        store: Arc<dyn KeyValueStore>,
    ) -> Result<RadixHistory, HistoryError> {
        let imple = RadixTreeImpl::new(store.clone());
        let node = Self::load_root_node(&imple, &root)?;

        Ok(RadixHistory {
            root_hash: root,
            root_node: node,
            imple,
            store,
            authority: None,
        })
    }

    pub fn create_authorized(
        root: Blake2b256Hash,
        authority: Arc<RSpaceStateAuthority>,
    ) -> Result<RadixHistory, HistoryError> {
        if !authority.contains_verified_root(&root)? {
            return Err(HistoryError::ActionError(format!(
                "cannot construct authorized history for unverified root {root}"
            )));
        }
        let store = authority.history_store()?;
        let imple = RadixTreeImpl::new(store.clone());
        let node = Self::load_root_node(&imple, &root)?;
        Ok(RadixHistory {
            root_hash: root,
            root_node: node,
            imple,
            store,
            authority: Some(authority),
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
}

impl History for RadixHistory {
    fn read(&self, key: ByteVector) -> Result<Option<ByteVector>, HistoryError> {
        let read_result = self.imple.read(&self.root_node, key.as_slice())?;
        Ok(read_result)
    }

    fn process(&self, actions: Vec<HistoryAction>) -> Result<Box<dyn History>, HistoryError> {
        if self.authority.is_some() {
            Err(HistoryError::ActionError(
                "authorized history mutations must use the atomic checkpoint authority".to_string(),
            ))
        } else {
            self.process_detached(actions)
        }
    }

    fn prepare_checkpoint(
        &self,
        actions: Vec<HistoryAction>,
    ) -> Result<(Box<dyn History>, PreparedHistoryCheckpoint), HistoryError> {
        self.prepare_local_checkpoint(actions)
    }

    fn root(&self) -> Blake2b256Hash { self.root_hash.clone() }

    fn reset(&self, root: &Blake2b256Hash) -> Result<Box<dyn History>, HistoryError> {
        if let Some(authority) = &self.authority {
            if !authority.contains_verified_root(root)? {
                return Err(HistoryError::ActionError(format!(
                    "cannot reset authorized history to unverified root {root}"
                )));
            }
        }
        let imple = RadixTreeImpl::new(self.store.clone());
        let node = Self::load_root_node(&imple, root)?;

        Ok(Box::new(RadixHistory {
            root_hash: root.clone(),
            root_node: node,
            imple,
            store: self.store.clone(),
            authority: self.authority.clone(),
        }))
    }
}

impl RadixHistory {
    fn process_detached(
        &self,
        actions: Vec<HistoryAction>,
    ) -> Result<Box<dyn History>, HistoryError> {
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
                let commit = self.imple.commit();
                self.imple.clear_write_cache();
                self.imple.clear_read_cache();
                commit?;
                Ok(Box::new(RadixHistory {
                    root_hash,
                    root_node: new_root_node,
                    imple: RadixTreeImpl::new(self.store.clone()),
                    store: self.store.clone(),
                    authority: self.authority.clone(),
                }))
            }
            None => {
                self.imple.clear_write_cache();
                self.imple.clear_read_cache();
                Ok(Box::new(RadixHistory {
                    root_hash: self.root_hash.clone(),
                    root_node: self.root_node.clone(),
                    imple: RadixTreeImpl::new(self.store.clone()),
                    store: self.store.clone(),
                    authority: self.authority.clone(),
                }))
            }
        }
    }

    fn prepare_local_checkpoint(
        &self,
        actions: Vec<HistoryAction>,
    ) -> Result<(Box<dyn History>, PreparedHistoryCheckpoint), HistoryError> {
        if !self.has_no_duplicates(&actions) {
            return Err(HistoryError::ActionError(
                "Cannot process duplicate actions on one key.".to_string(),
            ));
        }
        let authority = self.authority.as_ref().ok_or_else(|| {
            HistoryError::ActionError(
                "local checkpoint preparation requires an RSpace state authority".to_string(),
            )
        })?;

        let new_root_node_opt = self.imple.make_actions(&self.root_node, actions)?;
        let (target_root, root_node, root_encoding, history_objects) = if let Some(new_root_node) =
            new_root_node_opt
        {
            let target_root =
                Blake2b256Hash::from_bytes(self.imple.save_node(new_root_node.clone()));
            let history_objects = self.imple.take_validated_write_batch()?;
            let root_encoding = history_objects
                .get(&target_root.0)
                .cloned()
                .ok_or_else(|| {
                    HistoryError::ActionError(
                        "prepared history batch does not contain its derived root object"
                            .to_string(),
                    )
                })?;
            (target_root, new_root_node, Some(root_encoding), history_objects)
        } else {
            self.imple.clear_write_cache();
            (self.root_hash.clone(), self.root_node.clone(), None, BTreeMap::new())
        };
        self.imple.clear_read_cache();

        let next = Box::new(RadixHistory {
            root_hash: target_root.clone(),
            root_node,
            imple: RadixTreeImpl::new(self.store.clone()),
            store: self.store.clone(),
            authority: self.authority.clone(),
        });
        let prepared = PreparedHistoryCheckpoint {
            authority: Arc::clone(authority),
            base: self.root_hash.clone(),
            target_root,
            root_encoding,
            history_objects,
        };
        Ok((next, prepared))
    }
}
