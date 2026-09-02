use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, RwLock};
use std::thread;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_implicits::{get_random_block, protocol_v6_processed_deploy_gen};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedDeploy};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use shared::rust::ByteBuffer;

#[derive(Clone, Default)]
struct AdmissionFaults {
    late_atomic_failure: Arc<AtomicBool>,
}

impl AdmissionFaults {
    fn fail_next_atomic_commit_late(&self) {
        self.late_atomic_failure.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct FaultInjectingStore {
    inner: InMemoryKeyValueStore,
    faults: AdmissionFaults,
}

impl KeyValueStore for FaultInjectingStore {
    fn as_any(&self) -> &dyn Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        self.inner.get(keys)
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        self.inner.put(kv_pairs)
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(f)
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }

    fn strict_atomic_mutate(
        &self,
        mutations: &[AtomicStoreMutation<'_>],
    ) -> Result<(), KvStoreError> {
        let stores = mutations
            .iter()
            .map(|mutation| {
                mutation
                    .store
                    .as_any()
                    .downcast_ref::<FaultInjectingStore>()
                    .ok_or_else(|| {
                        KvStoreError::AtomicityUnavailable(
                            "fault transaction includes another backend".to_string(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut inner_mutations = mutations
            .iter()
            .zip(&stores)
            .map(|(mutation, store)| AtomicStoreMutation {
                store: &store.inner,
                key: mutation.key.clone(),
                operation: mutation.operation.clone(),
            })
            .collect::<Vec<_>>();

        if self
            .faults
            .late_atomic_failure
            .swap(false, Ordering::SeqCst)
        {
            inner_mutations.push(AtomicStoreMutation {
                store: &self.inner,
                key: b"carrier-admission-late-failure".to_vec(),
                operation: AtomicStoreOperation::CompareAndSwap {
                    expected: Some(vec![0xff]),
                    replacement: None,
                },
            });
        }

        self.inner.strict_atomic_mutate(&inner_mutations)
    }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

struct FaultInjectingStoreManager {
    stores: HashMap<String, FaultInjectingStore>,
    coordinator: Arc<RwLock<()>>,
    faults: AdmissionFaults,
}

impl FaultInjectingStoreManager {
    fn new() -> (Self, AdmissionFaults) {
        let faults = AdmissionFaults::default();
        (
            Self {
                stores: HashMap::new(),
                coordinator: Arc::new(RwLock::new(())),
                faults: faults.clone(),
            },
            faults,
        )
    }
}

#[async_trait]
impl KeyValueStoreManager for FaultInjectingStoreManager {
    async fn store(&mut self, name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        let store = self
            .stores
            .entry(name.clone())
            .or_insert_with(|| FaultInjectingStore {
                inner: InMemoryKeyValueStore::new_with_coordinator(self.coordinator.clone()),
                faults: self.faults.clone(),
            });
        Ok(Arc::new(store.clone()))
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> {
        self.stores.clear();
        Ok(())
    }
}

fn sample_deploy() -> ProcessedDeploy {
    let mut runner = TestRunner::deterministic();
    protocol_v6_processed_deploy_gen()
        .new_tree(&mut runner)
        .unwrap()
        .current()
}

fn block(
    version: i64,
    number: i64,
    parents: Vec<Vec<u8>>,
    deploys: Vec<ProcessedDeploy>,
) -> BlockMessage {
    let mut block = get_random_block(
        Some(number),
        Some(number as i32),
        None,
        None,
        None,
        Some(version),
        None,
        Some(parents.into_iter().map(Into::into).collect()),
        Some(vec![]),
        Some(deploys),
        None,
        Some(vec![]),
        None,
        None,
    );
    block.header.sender_bond_generation = Some(BondGeneration::GENESIS);
    block.body.state.bond_generations.clear();
    block
}

fn genesis() -> BlockMessage { block(6, 0, vec![], vec![]) }

#[tokio::test]
async fn v6_late_atomic_admission_failure_has_no_partial_projection_and_exact_retry() {
    let (mut manager, faults) = FaultInjectingStoreManager::new();
    let storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
    let genesis = genesis();
    storage
        .insert(&genesis, InsertMode::ApprovedGenesis)
        .unwrap();
    let deploy = sample_deploy();
    let deploy_id =
        DeployLookupId::V6(DeployIdV6::try_from(deploy.envelope_commitment.as_ref()).unwrap());
    let candidate = block(6, 1, vec![genesis.block_hash.to_vec()], vec![deploy]);

    faults.fail_next_atomic_commit_late();
    assert!(matches!(
        storage.insert(&candidate, InsertMode::Normal),
        Err(KvStoreError::TransactionConflict(_))
    ));

    let failed = storage.get_representation().unwrap();
    assert!(!failed.contains(&candidate.block_hash));
    assert!(failed
        .block_metadata_index
        .read()
        .get(&candidate.block_hash)
        .unwrap()
        .is_none());
    assert!(failed.carrier_index_proves_absence(&deploy_id).unwrap());
    assert!(failed
        .lookup_deploy_occurrences(&deploy_id)
        .unwrap()
        .is_empty());
    assert!(failed
        .deploy_lifecycle_events(&deploy_id)
        .unwrap()
        .is_none());

    storage.insert(&candidate, InsertMode::Normal).unwrap();
    let retried = storage.get_representation().unwrap();
    assert!(retried.contains(&candidate.block_hash));
    assert!(retried
        .block_metadata_index
        .read()
        .get(&candidate.block_hash)
        .unwrap()
        .is_some());
    assert!(!retried.carrier_index_proves_absence(&deploy_id).unwrap());
    assert_eq!(
        retried.lookup_deploy_occurrences(&deploy_id).unwrap(),
        [candidate.block_hash.clone()].into_iter().collect()
    );
    assert_eq!(
        retried
            .deploy_lifecycle_events(&deploy_id)
            .unwrap()
            .unwrap()
            .events
            .len(),
        1
    );
    assert_eq!(retried.prune_carriers_below(2).unwrap(), 1);
}

#[tokio::test]
async fn concurrent_insert_and_prune_preserve_a_carrier_at_the_cutoff() {
    let mut manager = InMemoryStoreManager::new();
    let storage = Arc::new(BlockDagKeyValueStorage::new(&mut manager).await.unwrap());
    let genesis = genesis();
    storage
        .insert(&genesis, InsertMode::ApprovedGenesis)
        .unwrap();
    let deploy = sample_deploy();
    let deploy_id =
        DeployLookupId::V6(DeployIdV6::try_from(deploy.envelope_commitment.as_ref()).unwrap());
    let candidate = block(6, 100, vec![genesis.block_hash.to_vec()], vec![deploy]);
    let representation = storage.get_representation().unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let insert_storage = Arc::clone(&storage);
    let insert_barrier = Arc::clone(&barrier);
    let insert_candidate = candidate.clone();
    let insert = thread::spawn(move || {
        insert_barrier.wait();
        insert_storage.insert(&insert_candidate, InsertMode::Normal)
    });
    let prune_barrier = Arc::clone(&barrier);
    let prune = thread::spawn(move || {
        prune_barrier.wait();
        representation.prune_carriers_below(100)
    });

    insert.join().unwrap().unwrap();
    prune.join().unwrap().unwrap();

    let current = storage.get_representation().unwrap();
    assert!(!current.carrier_index_proves_absence(&deploy_id).unwrap());
    assert_eq!(
        current.lookup_deploy_occurrences(&deploy_id).unwrap(),
        [candidate.block_hash].into_iter().collect()
    );
    assert_eq!(current.prune_carriers_below(164).unwrap(), 1);
}
