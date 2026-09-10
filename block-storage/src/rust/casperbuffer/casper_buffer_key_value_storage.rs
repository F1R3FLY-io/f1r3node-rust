// See block-storage/src/main/scala/coop/rchain/blockstorage/casperbuffer/CasperBufferKeyValueStorage.scala
// See block-storage/src/test/scala/coop/rchain/blockstorage/casperbuffer/CasperBufferStorageTest.scala

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use models::rust::block_hash::BlockHashSerde;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{
    strict_atomic_mutate, AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::rust::util::doubly_linked_dag_operations::BlockDependencyDag;
use crate::rust::util::ordered_snapshot::OrderedSnapshot;

mod buffer_transaction;
mod candidate_rotation;

#[cfg(test)]
mod persistence_tests;

use buffer_transaction::commit_then_publish;
use candidate_rotation::CandidateRotation;

use super::pending_request_policy::PendingRequestPolicy;

/**
 * @param parentsStore - persistent map {hash -> parents set}
 * @param blockDependencyDag - in-memory dependency DAG, recreated from parentsStore on node startup
 */
#[derive(Clone)]
pub struct CasperBufferKeyValueStorage {
    parents_store: KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>>,
    pending_policy_store: Arc<dyn KeyValueStore>,
    block_dependency_dag: Arc<Mutex<BlockDependencyDag>>,
    explicit_blocks: Arc<Mutex<HashSet<BlockHashSerde>>>,
    scan_candidates: Arc<Mutex<CandidateRotation<BlockHashSerde>>>,
    first_seen_ms: Arc<dashmap::DashMap<BlockHashSerde, u64>>,
    last_prune_ms: Arc<AtomicU64>,
    state_lock: Arc<RwLock<()>>,
}

impl CasperBufferKeyValueStorage {
    pub const PENDING_POLICY_NAMESPACE: &'static str = "pending-request-policy-v1";
    const CERTIFICATE_DEPENDENCY_PREFIX: u8 = 0xff;

    fn certificate_dependency_key(digest: &BlockHashSerde) -> Result<BlockHashSerde, KvStoreError> {
        if digest.0.len() != models::rust::block_hash::LENGTH {
            return Err(KvStoreError::InvalidArgument(format!(
                "finalization certificate digest must be {} bytes",
                models::rust::block_hash::LENGTH
            )));
        }
        let mut key = Vec::with_capacity(models::rust::block_hash::LENGTH + 1);
        key.push(Self::CERTIFICATE_DEPENDENCY_PREFIX);
        key.extend_from_slice(&digest.0);
        Ok(BlockHashSerde(key.into()))
    }

    fn certificate_digest_from_dependency_key(key: &BlockHashSerde) -> Option<BlockHashSerde> {
        (key.0.len() == models::rust::block_hash::LENGTH + 1
            && key.0.first().copied() == Some(Self::CERTIFICATE_DEPENDENCY_PREFIX))
        .then(|| BlockHashSerde(key.0.slice(1..)))
    }

    pub fn is_block_candidate(key: &BlockHashSerde) -> bool {
        Self::certificate_digest_from_dependency_key(key).is_none()
    }

    pub async fn new_from_kvm(
        kvm: &mut (impl KeyValueStoreManager + ?Sized),
    ) -> Result<Self, KvStoreError> {
        let parents_store_kv = kvm.store("parents-map".to_string()).await?;
        let parents_store: KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>> =
            KeyValueTypedStoreImpl::new(parents_store_kv);

        let pending_policy_store = kvm
            .store(Self::PENDING_POLICY_NAMESPACE.to_string())
            .await?;
        Self::new_from_kv_store(parents_store, pending_policy_store).await
    }

    pub async fn new_from_kv_store(
        kv_store: KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>>,
        pending_policy_store: Arc<dyn KeyValueStore>,
    ) -> Result<Self, KvStoreError> {
        let parents_map = kv_store.to_map()?;
        let explicit_blocks = parents_map.keys().cloned().collect();
        let mut in_mem_store = BlockDependencyDag::empty();
        for (key, parents) in parents_map {
            if parents.is_empty() {
                in_mem_store.dependency_free.insert(key);
            } else {
                for parent in parents {
                    in_mem_store.add(parent, key.clone());
                }
            }
        }
        let restored_at = Self::now_millis();
        let first_seen_ms = dashmap::DashMap::new();
        let mut candidate_keys = Vec::new();
        for key in in_mem_store
            .child_to_parent_adjacency_list
            .keys()
            .chain(in_mem_store.dependency_free.iter())
        {
            first_seen_ms.insert(key.clone(), restored_at);
            if Self::certificate_digest_from_dependency_key(key).is_none() {
                candidate_keys.push(key.clone());
            }
        }
        candidate_keys.sort();
        let mut scan_candidates = CandidateRotation::new();
        for key in candidate_keys {
            scan_candidates.insert(key);
        }

        Ok(Self {
            parents_store: kv_store,
            pending_policy_store,
            block_dependency_dag: Arc::new(Mutex::new(in_mem_store)),
            explicit_blocks: Arc::new(Mutex::new(explicit_blocks)),
            scan_candidates: Arc::new(Mutex::new(scan_candidates)),
            first_seen_ms: Arc::new(first_seen_ms),
            last_prune_ms: Arc::new(AtomicU64::new(0)),
            state_lock: Arc::new(RwLock::new(())),
        })
    }

    fn read_guard(&self) -> RwLockReadGuard<'_, ()> {
        self.state_lock.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Acquire the buffer's exclusive state lock. Visible at `pub(crate)`
    /// so the atomic buffer-DAG transition helper (`dag::buffer_dag_transition`)
    /// can take both locks in a documented order across stores.
    pub(crate) fn write_guard(&self) -> RwLockWriteGuard<'_, ()> {
        self.state_lock.write().unwrap_or_else(|e| e.into_inner())
    }

    fn insert_scan_candidate(&self, hash: &BlockHashSerde) {
        if Self::certificate_digest_from_dependency_key(hash).is_none() {
            self.scan_candidates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(hash.clone());
        }
    }

    pub fn scan_candidate_count(&self) -> usize {
        let _guard = self.read_guard();
        self.scan_candidates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn next_scan_candidate(&self) -> Option<BlockHashSerde> {
        let _guard = self.read_guard();
        self.scan_candidates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .next()
    }

    pub fn next_expiry_scan_batch(&self, remaining: usize) -> Vec<BlockHashSerde> {
        let _guard = self.read_guard();
        let mut candidates = self
            .scan_candidates
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        (0..remaining.min(64).min(candidates.len()))
            .filter_map(|_| candidates.next_expiry())
            .collect()
    }

    fn persist_rows(
        &self,
        rows: &[(BlockHashSerde, HashSet<BlockHashSerde>)],
        removed: Option<&BlockHashSerde>,
    ) -> Result<(), KvStoreError> {
        let store = self.parents_store.raw_store().as_ref();
        let mut mutations = rows
            .iter()
            .map(|(key, parents)| {
                Ok(AtomicStoreMutation {
                    store,
                    key: self.parents_store.encode_key(key)?,
                    operation: AtomicStoreOperation::Put(self.parents_store.encode_value(parents)?),
                })
            })
            .collect::<Result<Vec<_>, KvStoreError>>()?;
        if let Some(key) = removed {
            mutations.push(AtomicStoreMutation {
                store,
                key: self.parents_store.encode_key(key)?,
                operation: AtomicStoreOperation::Delete,
            });
            mutations.push(AtomicStoreMutation {
                store: self.pending_policy_store.as_ref(),
                key: self.parents_store.encode_key(key)?,
                operation: AtomicStoreOperation::Delete,
            });
        }
        strict_atomic_mutate(&mutations)
    }

    fn add_relation_unlocked(
        &self,
        parent: BlockHashSerde,
        child: BlockHashSerde,
    ) -> Result<(), KvStoreError> {
        let mut dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut explicit = self
            .explicit_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut parents = dag
            .child_to_parent_adjacency_list
            .get(&child)
            .cloned()
            .unwrap_or_default();
        parents.insert(parent.clone());
        commit_then_publish(
            [(child.clone(), parents)],
            |rows| self.persist_rows(rows, None),
            |_| {
                self.track_hash_first_seen(&parent);
                self.track_hash_first_seen(&child);
                self.insert_scan_candidate(&parent);
                self.insert_scan_candidate(&child);
                explicit.insert(child.clone());
                dag.add(parent, child);
            },
        )
    }

    /// Remove a block hash from the casper buffer assuming the caller
    /// already holds `state_lock.write()`. Visible at `pub(crate)` so
    /// the atomic buffer-DAG transition helper can perform the buffer
    /// half of the (dag.insert, buffer.remove) pair under a shared
    /// critical section.
    pub(crate) fn remove_unlocked(&self, hash: BlockHashSerde) -> Result<(), KvStoreError> {
        let mut dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut explicit = self
            .explicit_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let old_parents = dag
            .child_to_parent_adjacency_list
            .get(&hash)
            .cloned()
            .unwrap_or_default();
        let changes: Vec<_> = dag
            .parent_to_child_adjacency_list
            .get(&hash)
            .into_iter()
            .flatten()
            .filter(|child| *child != &hash)
            .map(|child| {
                let mut parents = dag
                    .child_to_parent_adjacency_list
                    .get(child)
                    .cloned()
                    .unwrap_or_default();
                parents.remove(&hash);
                (child.clone(), parents)
            })
            .collect();
        commit_then_publish(
            changes,
            |rows| self.persist_rows(rows, Some(&hash)),
            |rows| {
                explicit.remove(&hash);
                dag.child_to_parent_adjacency_list.remove(&hash);
                dag.parent_to_child_adjacency_list.remove(&hash);
                dag.dependency_free.remove(&hash);
                self.first_seen_ms.remove(&hash);
                self.scan_candidates
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&hash);
                for parent in old_parents {
                    if parent == hash {
                        continue;
                    }
                    if let Some(children) = dag.parent_to_child_adjacency_list.get_mut(&parent) {
                        children.remove(&hash);
                        if children.is_empty() {
                            dag.parent_to_child_adjacency_list.remove(&parent);
                            if !explicit.contains(&parent)
                                && !dag.child_to_parent_adjacency_list.contains_key(&parent)
                            {
                                dag.dependency_free.remove(&parent);
                                self.first_seen_ms.remove(&parent);
                                self.scan_candidates
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .remove(&parent);
                            }
                        }
                    }
                }
                for (child, parents) in rows {
                    if parents.is_empty() {
                        dag.child_to_parent_adjacency_list.remove(&child);
                        dag.dependency_free.insert(child);
                    } else {
                        dag.child_to_parent_adjacency_list.insert(child, parents);
                    }
                }
            },
        )
    }

    pub fn add_relation(
        &self,
        parent: BlockHashSerde,
        child: BlockHashSerde,
    ) -> Result<(), KvStoreError> {
        let _guard = self.write_guard();
        self.add_relation_unlocked(parent, child)
    }

    pub fn add_dependencies(
        &self,
        child: BlockHashSerde,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
    ) -> Result<(), KvStoreError> {
        self.publish_dependencies(child, blocks, certificates, None)
    }

    pub fn publish_pending_request(
        &self,
        child: BlockHashSerde,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
        expected: Option<&PendingRequestPolicy>,
        updated: &PendingRequestPolicy,
    ) -> Result<(), KvStoreError> {
        self.publish_dependencies(child, blocks, certificates, Some((expected, updated)))
    }

    fn policy_mutation<'a>(
        &'a self,
        child: &BlockHashSerde,
        expected: Option<&PendingRequestPolicy>,
        updated: &PendingRequestPolicy,
    ) -> Result<AtomicStoreMutation<'a>, KvStoreError> {
        Self::validate_policy_update(expected, updated)?;
        Ok(AtomicStoreMutation {
            store: self.pending_policy_store.as_ref(),
            key: self.parents_store.encode_key(child)?,
            operation: AtomicStoreOperation::CompareAndSwap {
                expected: expected.map(PendingRequestPolicy::encode).transpose()?,
                replacement: Some(updated.encode()?),
            },
        })
    }

    fn validate_policy_update(
        expected: Option<&PendingRequestPolicy>,
        updated: &PendingRequestPolicy,
    ) -> Result<(), KvStoreError> {
        let revision = expected.map_or(Some(1), |policy| policy.revision.checked_add(1));
        if revision != Some(updated.revision) {
            return Err(KvStoreError::InvalidArgument(
                "pending policy update requires the next checked revision".into(),
            ));
        }
        if let Some(previous) = expected {
            if updated.initial_timestamp != previous.initial_timestamp
                || (previous.requested_as_dependency && !updated.requested_as_dependency)
                || updated.retry_attempts < previous.retry_attempts
                || updated.peer_requery_attempts < previous.peer_requery_attempts
            {
                return Err(KvStoreError::InvalidArgument(
                    "pending policy update cannot reset request history".into(),
                ));
            }
        }
        Ok(())
    }

    fn publish_dependencies(
        &self,
        child: BlockHashSerde,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
        policy: Option<(Option<&PendingRequestPolicy>, &PendingRequestPolicy)>,
    ) -> Result<(), KvStoreError> {
        let mut submitted = blocks;
        for digest in certificates {
            submitted.insert(Self::certificate_dependency_key(&digest)?);
        }
        let _guard = self.write_guard();
        let existing = self.parents_store.get_one(&child)?;
        if policy.is_none()
            && existing
                .as_ref()
                .is_some_and(|parents| submitted.is_subset(parents))
        {
            return Ok(());
        }
        let mut parents = existing.unwrap_or_default();
        parents.extend(submitted);
        let mut dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut explicit = self
            .explicit_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        commit_then_publish(
            [(child.clone(), parents)],
            |rows| {
                if let Some((expected, updated)) = policy {
                    let mutation = self.policy_mutation(&rows[0].0, expected, updated)?;
                    strict_atomic_mutate(&[
                        AtomicStoreMutation {
                            store: self.parents_store.raw_store().as_ref(),
                            key: self.parents_store.encode_key(&rows[0].0)?,
                            operation: AtomicStoreOperation::Put(
                                self.parents_store.encode_value(&rows[0].1)?,
                            ),
                        },
                        mutation,
                    ])
                } else {
                    self.persist_rows(rows, None)
                }
            },
            |rows| {
                explicit.insert(child.clone());
                self.track_hash_first_seen(&child);
                self.insert_scan_candidate(&child);
                let [(_, parents)] = rows;
                if parents.is_empty() {
                    dag.dependency_free.insert(child);
                } else {
                    for parent in parents {
                        self.track_hash_first_seen(&parent);
                        self.insert_scan_candidate(&parent);
                        dag.add(parent, child.clone());
                    }
                }
            },
        )
    }

    pub fn pending_request_policy(
        &self,
        child: &BlockHashSerde,
    ) -> Result<Option<PendingRequestPolicy>, KvStoreError> {
        let _guard = self.read_guard();
        let mut policy = None;
        self.pending_policy_store.with_value(
            &self.parents_store.encode_key(child)?,
            &mut |bytes| {
                policy = bytes.map(PendingRequestPolicy::decode).transpose()?;
                Ok(())
            },
        )?;
        if policy.is_some() && self.parents_store.get_one(child)?.is_none() {
            return Err(KvStoreError::InvalidArgument(
                "pending request policy has no durable dependency row".into(),
            ));
        }
        Ok(policy)
    }

    pub fn update_pending_request_policy(
        &self,
        child: &BlockHashSerde,
        expected: &PendingRequestPolicy,
        updated: &PendingRequestPolicy,
    ) -> Result<(), KvStoreError> {
        let mutation = self.policy_mutation(child, Some(expected), updated)?;
        self.apply_pending_policy_mutation(child, mutation)
    }

    pub fn renew_pending_request_policy(
        &self,
        child: &BlockHashSerde,
        expected: &PendingRequestPolicy,
        sweep_time: u64,
    ) -> Result<PendingRequestPolicy, KvStoreError> {
        let mut updated = expected.renewed_retry_budget(sweep_time)?.ok_or_else(|| {
            KvStoreError::InvalidArgument("retry renewal requires an expired quarantine".into())
        })?;
        updated.advance_revision()?;
        self.apply_pending_policy_mutation(child, AtomicStoreMutation {
            store: self.pending_policy_store.as_ref(),
            key: self.parents_store.encode_key(child)?,
            operation: AtomicStoreOperation::CompareAndSwap {
                expected: Some(expected.encode()?),
                replacement: Some(updated.encode()?),
            },
        })?;
        Ok(updated)
    }

    pub fn quarantine_pending_request_policy(
        &self,
        child: &BlockHashSerde,
        expected: &PendingRequestPolicy,
        effective: &PendingRequestPolicy,
        budget: u32,
        deadline: u64,
    ) -> Result<PendingRequestPolicy, KvStoreError> {
        let mut advancing = effective.clone();
        advancing.revision = expected.revision;
        advancing.advance_revision()?;
        Self::validate_policy_update(Some(expected), &advancing)?;
        let updated = advancing.quarantined_retry_budget(budget, deadline)?;
        self.apply_pending_policy_mutation(child, AtomicStoreMutation {
            store: self.pending_policy_store.as_ref(),
            key: self.parents_store.encode_key(child)?,
            operation: AtomicStoreOperation::CompareAndSwap {
                expected: Some(expected.encode()?),
                replacement: Some(updated.encode()?),
            },
        })?;
        Ok(updated)
    }

    fn apply_pending_policy_mutation(
        &self,
        child: &BlockHashSerde,
        mutation: AtomicStoreMutation<'_>,
    ) -> Result<(), KvStoreError> {
        let _guard = self.write_guard();
        let key = self.parents_store.encode_key(child)?;
        let mut captured = None;
        self.parents_store
            .raw_store()
            .with_value(&key, &mut |bytes| {
                captured = bytes.map(<[u8]>::to_vec);
                Ok(())
            })?;
        let bytes = captured.ok_or_else(|| {
            KvStoreError::TransactionConflict("pending dependency row was retired".into())
        })?;
        self.parents_store.decode_value(&bytes)?;
        strict_atomic_mutate(&[
            AtomicStoreMutation {
                store: self.parents_store.raw_store().as_ref(),
                key,
                operation: AtomicStoreOperation::CompareAndSwap {
                    expected: Some(bytes.clone()),
                    replacement: Some(bytes),
                },
            },
            mutation,
        ])
    }

    pub fn contains_durable_row(&self, block: &BlockHashSerde) -> Result<bool, KvStoreError> {
        let _guard = self.read_guard();
        Ok(self.parents_store.get_one(block)?.is_some())
    }

    pub fn add_certificate_relation(
        &self,
        digest: BlockHashSerde,
        child: BlockHashSerde,
    ) -> Result<(), KvStoreError> {
        let dependency = Self::certificate_dependency_key(&digest)?;
        let _guard = self.write_guard();
        self.add_relation_unlocked(dependency, child)
    }

    pub fn resolve_certificate_dependency(
        &self,
        digest: BlockHashSerde,
    ) -> Result<(), KvStoreError> {
        let dependency = Self::certificate_dependency_key(&digest)?;
        let _guard = self.write_guard();
        if self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .parent_to_child_adjacency_list
            .contains_key(&dependency)
        {
            self.remove_unlocked(dependency)?;
        }
        Ok(())
    }

    pub fn get_missing_certificate_dependencies(&self) -> HashSet<BlockHashSerde> {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.dependency_free
            .iter()
            .filter_map(Self::certificate_digest_from_dependency_key)
            .collect()
    }

    pub fn requested_as_certificate_dependency(&self, digest: &BlockHashSerde) -> bool {
        let Ok(dependency) = Self::certificate_dependency_key(digest) else {
            return false;
        };
        let _guard = self.read_guard();
        self.block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .parent_to_child_adjacency_list
            .contains_key(&dependency)
    }

    pub fn is_waiting_on_certificate(&self, block_hash: &BlockHashSerde) -> bool {
        let _guard = self.read_guard();
        self.block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .child_to_parent_adjacency_list
            .get(block_hash)
            .is_some_and(|parents| {
                parents
                    .iter()
                    .any(|parent| Self::certificate_digest_from_dependency_key(parent).is_some())
            })
    }

    pub fn put_pendant(&self, block: BlockHashSerde) -> Result<(), KvStoreError> {
        let _guard = self.write_guard();
        let mut dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut explicit = self
            .explicit_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if explicit.contains(&block) {
            return Ok(());
        }
        commit_then_publish(
            [(block.clone(), HashSet::new())],
            |rows| self.persist_rows(rows, None),
            |_| {
                explicit.insert(block.clone());
                self.track_hash_first_seen(&block);
                self.insert_scan_candidate(&block);
                dag.dependency_free.insert(block);
            },
        )
    }

    pub fn remove(&self, hash: BlockHashSerde) -> Result<(), KvStoreError> {
        let _guard = self.write_guard();
        self.remove_unlocked(hash)
    }

    pub fn get_parents(&self, block_hash: &BlockHashSerde) -> Option<HashSet<BlockHashSerde>> {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.child_to_parent_adjacency_list.get(block_hash).cloned()
    }

    pub fn get_children(&self, block_hash: &BlockHashSerde) -> Option<HashSet<BlockHashSerde>> {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.parent_to_child_adjacency_list.get(block_hash).cloned()
    }

    pub fn get_pendants(&self) -> HashSet<BlockHashSerde> {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.dependency_free
            .iter()
            .filter(|hash| Self::certificate_digest_from_dependency_key(hash).is_none())
            .cloned()
            .collect()
    }

    pub fn snapshot_pendant_candidates(&self) -> OrderedSnapshot<BlockHashSerde> {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        OrderedSnapshot::new(dag.dependency_free.clone())
    }

    // Block is considered to be in CasperBuffer when there is a records about its parents
    pub fn contains(&self, block_hash: &BlockHashSerde) -> bool {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.child_to_parent_adjacency_list.contains_key(block_hash)
    }

    pub fn to_doubly_linked_dag(&self) -> BlockDependencyDag {
        let _guard = self.read_guard();
        self.block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn requested_as_dependency(&self, block_hash: &BlockHashSerde) -> bool {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.parent_to_child_adjacency_list.contains_key(block_hash)
    }

    pub fn size(&self) -> usize {
        let _guard = self.read_guard();
        self.block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .child_to_parent_adjacency_list
            .len()
    }

    pub fn approx_node_count(&self) -> usize {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.child_to_parent_adjacency_list
            .len()
            .saturating_add(dag.dependency_free.len())
    }

    pub fn is_pendant(&self, block_hash: &BlockHashSerde) -> bool {
        let _guard = self.read_guard();
        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        dag.dependency_free.contains(block_hash)
    }

    fn dependency_free_nodes_with_age_ms(&self, now_ms: u64) -> Vec<(u64, BlockHashSerde)> {
        let mut nodes = Vec::new();
        let mut seen = HashSet::new();

        let dag = self
            .block_dependency_dag
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for hash in dag.dependency_free.iter() {
            if let Some(children) = dag.parent_to_child_adjacency_list.get(hash) {
                for child in children {
                    if seen.insert(child.clone()) {
                        let seen_ms = self
                            .first_seen_ms
                            .get(child)
                            .map(|seen| *seen)
                            .unwrap_or(now_ms);
                        nodes.push((now_ms.saturating_sub(seen_ms), child.clone()));
                    }
                }
            } else if seen.insert(hash.clone()) {
                let seen_ms = self
                    .first_seen_ms
                    .get(hash)
                    .map(|seen| *seen)
                    .unwrap_or(now_ms);
                nodes.push((now_ms.saturating_sub(seen_ms), hash.clone()));
            }
        }

        nodes
    }

    pub fn enforce_limits(
        &self,
        max_approx_nodes: usize,
        stale_ttl_ms: u64,
        max_prune_batch: usize,
        prune_interval_ms: u64,
    ) -> Result<(usize, usize), KvStoreError> {
        let now = Self::now_millis();
        let last_prune = self.last_prune_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last_prune) < prune_interval_ms {
            return Ok((0, 0));
        }
        self.last_prune_ms.store(now, Ordering::Relaxed);

        let mut stale_candidates: Vec<(u64, BlockHashSerde)> = self
            .dependency_free_nodes_with_age_ms(now)
            .into_iter()
            .filter(|(age_ms, _)| *age_ms >= stale_ttl_ms)
            .collect();
        stale_candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        let mut stale_pruned = 0usize;
        for (_, hash) in stale_candidates.into_iter().take(max_prune_batch) {
            match self.remove(hash) {
                Ok(_) => stale_pruned += 1,
                Err(KvStoreError::InvalidArgument(_)) => {}
                Err(e) => return Err(e),
            }
        }

        let mut overflow_pruned = 0usize;
        let mut approx_nodes = self.approx_node_count();
        let mut attempts = 0usize;
        while overflow_pruned < max_prune_batch
            && attempts < max_prune_batch
            && approx_nodes > max_approx_nodes
        {
            let mut oldest_nodes: Vec<(u64, BlockHashSerde)> = self
                .dependency_free_nodes_with_age_ms(now)
                .into_iter()
                .collect();
            oldest_nodes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

            let Some((_, hash)) = oldest_nodes.into_iter().next() else {
                break;
            };

            let removed = self.remove(hash);
            attempts += 1;

            match removed {
                Ok(_) => {
                    overflow_pruned += 1;
                    approx_nodes = self.approx_node_count();
                }
                Err(KvStoreError::InvalidArgument(_)) => {}
                Err(e) => return Err(e),
            }
        }

        Ok((stale_pruned, overflow_pruned))
    }

    fn track_hash_first_seen(&self, hash: &BlockHashSerde) {
        self.first_seen_ms
            .entry(hash.clone())
            .or_insert_with(Self::now_millis);
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use models::rust::block_hash::BlockHashSerde;
    use proptest::prelude::*;
    use proptest::test_runner::{TestCaseError, TestCaseResult};
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn create_block_hash(data: &[u8]) -> BlockHashSerde {
        BlockHashSerde(Bytes::copy_from_slice(data))
    }

    proptest! {
        #[test]
        fn certificate_dependency_namespace_is_disjoint_and_round_trips(bytes in any::<[u8; models::rust::block_hash::LENGTH]>()) {
            let digest = BlockHashSerde(Bytes::copy_from_slice(&bytes));
            let dependency = CasperBufferKeyValueStorage::certificate_dependency_key(&digest)
                .expect("certificate dependency key");

            prop_assert_eq!(dependency.0.len(), models::rust::block_hash::LENGTH + 1);
            prop_assert_eq!(dependency.0[0], CasperBufferKeyValueStorage::CERTIFICATE_DEPENDENCY_PREFIX);
            prop_assert_ne!(dependency.clone(), digest.clone());
            prop_assert_eq!(
                CasperBufferKeyValueStorage::certificate_digest_from_dependency_key(&dependency),
                Some(digest)
            );
        }

        #[test]
        fn pruning_waiters_preserves_every_surviving_parent_obligation(
            child_count in 2_usize..9,
            requested_prune_count in 1_usize..9,
        ) {
            let prune_count = requested_prune_count.min(child_count - 1);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let result: TestCaseResult = runtime.block_on(async move {
                let mut kvm = InMemoryStoreManager::new();
                let store = kvm
                    .store("parents-map".to_string())
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let typed_store = KeyValueTypedStoreImpl::new(store);
                let buffer = CasperBufferKeyValueStorage::new_from_kv_store(typed_store, kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into()).await.unwrap())
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let parent = create_block_hash(&[250; models::rust::block_hash::LENGTH]);
                for index in 0..child_count {
                    let child = create_block_hash(&[index as u8; models::rust::block_hash::LENGTH]);
                    buffer
                        .add_relation(parent.clone(), child)
                        .map_err(|error| TestCaseError::fail(error.to_string()))?;
                }

                let (stale_pruned, _) = buffer
                    .enforce_limits(usize::MAX, 0, prune_count, 0)
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(stale_pruned, prune_count);

                let survivors = buffer
                    .get_children(&parent)
                    .ok_or_else(|| TestCaseError::fail("missing surviving dependency edges"))?;
                prop_assert_eq!(survivors.len(), child_count - prune_count);
                for survivor in survivors {
                    prop_assert!(!buffer.is_pendant(&survivor));
                    prop_assert_eq!(
                        buffer.get_parents(&survivor),
                        Some(HashSet::from([parent.clone()]))
                    );
                }
                Ok(())
            });
            result?;
        }
    }

    #[tokio::test]
    async fn casper_buffer_storage_should_work() -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);

        let a = create_block_hash(b"A");
        let b = create_block_hash(b"B");
        let c = create_block_hash(b"C");
        let d = create_block_hash(b"D");

        typed_store.put_one(c.clone(), HashSet::from([d.clone()]))?;

        let casper_buffer = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;

        // CasperBufferStorage be able to restore state on startup
        let c_parents = casper_buffer.get_parents(&c);
        assert!(c_parents.is_some());
        assert!(c_parents.unwrap().contains(&d));

        let d_children = casper_buffer.get_children(&d);
        assert!(d_children.is_some());
        assert!(d_children.unwrap().contains(&c));

        // Add relation should change parents set and children set
        casper_buffer.add_relation(a.clone(), b.clone())?;

        let b_parents = casper_buffer.get_parents(&b);
        assert!(b_parents.is_some());
        assert!(b_parents.unwrap().contains(&a));

        let a_children = casper_buffer.get_children(&a);
        assert!(a_children.is_some());
        assert!(a_children.unwrap().contains(&b));

        // Block that has no parents should be pendant
        casper_buffer.add_relation(a.clone(), b.clone())?;
        assert!(casper_buffer.is_pendant(&a));

        let h1 = casper_buffer.parents_store.get_one(&b)?;
        assert!(h1.is_some());
        assert!(h1.unwrap().contains(&a));
        casper_buffer.remove(a.clone())?;
        let h2 = casper_buffer.parents_store.get_one(&b)?;
        assert_eq!(h2, Some(HashSet::new()));

        // When removed hash A is the last parent for hash B, B should be pendant
        assert!(casper_buffer.is_pendant(&b));

        Ok(())
    }

    #[tokio::test]
    async fn casper_buffer_put_pendant_stays_dependency_free() -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let casper_buffer = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;

        let block = create_block_hash(b"dependent_block");
        let temp_block = BlockHashSerde(prost::bytes::Bytes::from_static(b"tempblock"));
        casper_buffer.put_pendant(block.clone())?;

        assert!(!casper_buffer.contains(&block));
        assert!(casper_buffer.is_pendant(&block));
        assert!(!casper_buffer.contains(&temp_block));
        assert!(!casper_buffer.is_pendant(&temp_block));
        assert!(casper_buffer.get_parents(&block).is_none());
        assert!(casper_buffer.get_children(&temp_block).is_none());

        let pendants = casper_buffer.get_pendants();
        assert!(pendants.contains(&block));
        Ok(())
    }

    #[tokio::test]
    async fn certificate_dependencies_are_typed_persistent_and_resolved_atomically(
    ) -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let digest = BlockHashSerde(Bytes::from(vec![7; models::rust::block_hash::LENGTH]));
        let block = BlockHashSerde(Bytes::from(vec![8; models::rust::block_hash::LENGTH]));

        let first = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store.clone(),
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;
        first.add_certificate_relation(digest.clone(), block.clone())?;
        assert_eq!(
            first.get_missing_certificate_dependencies(),
            HashSet::from([digest.clone()])
        );
        assert!(first.get_pendants().is_empty());
        drop(first);

        let restored = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;
        assert!(restored.requested_as_certificate_dependency(&digest));
        assert_eq!(
            restored.get_missing_certificate_dependencies(),
            HashSet::from([digest.clone()])
        );
        assert!(restored.get_pendants().is_empty());

        restored.resolve_certificate_dependency(digest.clone())?;
        assert!(!restored.requested_as_certificate_dependency(&digest));
        assert!(restored.get_missing_certificate_dependencies().is_empty());
        assert_eq!(restored.get_pendants(), HashSet::from([block]));
        Ok(())
    }

    #[tokio::test]
    async fn certificate_dependency_pruning_removes_the_waiting_block_not_the_proof_obligation(
    ) -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let buffer = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;
        let digest = BlockHashSerde(Bytes::from(vec![9; models::rust::block_hash::LENGTH]));
        let block = BlockHashSerde(Bytes::from(vec![10; models::rust::block_hash::LENGTH]));

        buffer.add_certificate_relation(digest.clone(), block.clone())?;
        let (stale_pruned, _) = buffer.enforce_limits(usize::MAX, 0, 1, 0)?;

        assert_eq!(stale_pruned, 1);
        assert!(!buffer.requested_as_certificate_dependency(&digest));
        assert!(!buffer.contains(&block));
        assert!(!buffer.is_pendant(&block));
        Ok(())
    }

    #[tokio::test]
    async fn block_dependency_pruning_retires_a_waiter_without_waking_its_sibling(
    ) -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let buffer = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;
        let parent = create_block_hash(b"missing-parent");
        let first = create_block_hash(b"first-child");
        let second = create_block_hash(b"second-child");

        buffer.add_relation(parent.clone(), first.clone())?;
        buffer.add_relation(parent.clone(), second.clone())?;
        let (stale_pruned, _) = buffer.enforce_limits(usize::MAX, 0, 1, 0)?;

        assert_eq!(stale_pruned, 1);
        let children = buffer
            .get_children(&parent)
            .expect("one waiting child must retain the missing-parent evidence");
        assert_eq!(children.len(), 1);
        let survivor = children.iter().next().expect("surviving child");
        assert!(!buffer.is_pendant(survivor));
        assert_eq!(buffer.get_parents(survivor), Some(HashSet::from([parent])));
        assert_ne!(buffer.contains(&first), buffer.contains(&second));
        Ok(())
    }

    #[tokio::test]
    async fn certificate_dependency_rejects_non_digest_keys() -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let buffer = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;
        let short = create_block_hash(b"short");
        let block = BlockHashSerde(Bytes::from(vec![11; models::rust::block_hash::LENGTH]));

        assert!(buffer.add_certificate_relation(short, block).is_err());
        assert!(buffer.get_missing_certificate_dependencies().is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn casper_buffer_remove_repairs_stale_parent_links() -> Result<(), KvStoreError> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await?;
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let casper_buffer = CasperBufferKeyValueStorage::new_from_kv_store(
            typed_store,
            kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                .await
                .unwrap(),
        )
        .await?;

        let block = create_block_hash(b"orphan");
        let valid_parent = create_block_hash(b"valid-parent");
        let stale_parent = create_block_hash(b"stale-parent");

        casper_buffer.add_relation(valid_parent.clone(), block.clone())?;

        {
            let mut dag = casper_buffer
                .block_dependency_dag
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let parents = dag
                .child_to_parent_adjacency_list
                .get_mut(&block)
                .expect("expected block to have parent links");
            parents.insert(stale_parent.clone());
        }

        let before = casper_buffer
            .get_parents(&block)
            .expect("expected pendant-parent linkage");
        assert!(before.contains(&valid_parent));
        assert!(before.contains(&stale_parent));
        assert!(casper_buffer.first_seen_ms.get(&valid_parent).is_some());

        casper_buffer.remove(block.clone())?;

        assert!(!casper_buffer.contains(&block));
        assert!(casper_buffer.get_parents(&block).is_none());
        assert!(casper_buffer.parents_store.get_one(&block)?.is_none());
        assert!(!casper_buffer.requested_as_dependency(&valid_parent));
        assert!(casper_buffer.first_seen_ms.get(&valid_parent).is_none());

        Ok(())
    }
}
