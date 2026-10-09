use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

use crate::helper::block_util::generate_validator;

const BLOCK_METADATA_STORE: &str = "block-metadata";

#[derive(Clone)]
struct CountingStore {
    inner: Arc<dyn KeyValueStore>,
    reads: Arc<AtomicUsize>,
}

impl KeyValueStore for CountingStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<Option<Vec<u8>>>, KvStoreError> {
        self.reads.fetch_add(keys.len(), Ordering::SeqCst);
        self.inner.get(keys)
    }

    fn put(&self, kv_pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), KvStoreError> {
        self.inner.put(kv_pairs)
    }

    fn put_one_if_absent(&self, key: Vec<u8>, value: Vec<u8>) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }

    fn delete(&self, keys: Vec<Vec<u8>>) -> Result<usize, KvStoreError> { self.inner.delete(keys) }

    fn iterate(&self, f: fn(Vec<u8>, Vec<u8>)) -> Result<(), KvStoreError> { self.inner.iterate(f) }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(Vec<u8>, Vec<u8>) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, KvStoreError> { self.inner.to_map() }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

struct CountingStoreManager {
    inner: InMemoryStoreManager,
    metadata_reads: Arc<AtomicUsize>,
}

#[async_trait]
impl KeyValueStoreManager for CountingStoreManager {
    async fn store(&mut self, name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        let store = self.inner.store(name.clone()).await?;
        if name == BLOCK_METADATA_STORE {
            Ok(Arc::new(CountingStore {
                inner: store,
                reads: self.metadata_reads.clone(),
            }))
        } else {
            Ok(store)
        }
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> { self.inner.shutdown().await }
}

fn block(
    number: i64,
    sender: &Validator,
    parents: Vec<BlockHash>,
    justifications: Vec<Justification>,
    bonds: &[Bond],
) -> BlockMessage {
    block_implicits::get_random_block(
        Some(number),
        Some(number as i32),
        None,
        None,
        Some(sender.clone()),
        None,
        Some(number),
        Some(parents),
        Some(justifications),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(bonds.to_vec()),
        Some("root".to_string()),
        None,
    )
}

async fn metadata_reads_for_one_oracle_call(validator_count: usize) -> usize {
    let metadata_reads = Arc::new(AtomicUsize::new(0));
    let mut kvm = CountingStoreManager {
        inner: InMemoryStoreManager::new(),
        metadata_reads: metadata_reads.clone(),
    };
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
        .await
        .expect("dag storage");

    let validators: Vec<Validator> = (0..validator_count)
        .map(|i| generate_validator(Some(&format!("metadata-reads {}", i))))
        .collect();
    let bonds: Vec<Bond> = validators
        .iter()
        .map(|validator| Bond {
            validator: validator.clone(),
            stake: 10,
        })
        .collect();

    let genesis = block(0, &validators[0], Vec::new(), Vec::new(), &bonds);
    dag_storage
        .insert(&genesis, InsertMode::Approved)
        .expect("insert genesis");

    let mut latest: Vec<BlockHash> = vec![genesis.block_hash.clone(); validator_count];
    let mut target: Option<BlockHash> = None;
    for height in 1..=4 {
        let justifications: Vec<Justification> = validators
            .iter()
            .zip(latest.iter())
            .map(|(validator, hash)| Justification {
                validator: validator.clone(),
                latest_block_hash: hash.clone(),
            })
            .collect();
        let mut parents = vec![latest[0].clone()];
        for hash in &latest[1..] {
            if !parents.contains(hash) {
                parents.push(hash.clone());
            }
        }
        let round: Vec<BlockMessage> = validators
            .iter()
            .map(|validator| {
                block(
                    height,
                    validator,
                    parents.clone(),
                    justifications.clone(),
                    &bonds,
                )
            })
            .collect();
        for b in &round {
            dag_storage
                .insert(b, InsertMode::Normal)
                .expect("insert block");
        }
        latest = round.iter().map(|b| b.block_hash.clone()).collect();
        if height == 1 {
            target = Some(latest[0].clone());
        }
    }

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    let snapshot: BTreeMap<Validator, BlockHash> = validators
        .iter()
        .cloned()
        .zip(latest.iter().cloned())
        .collect();
    let target = target.expect("target block");

    metadata_reads.store(0, Ordering::SeqCst);
    let finalized = CliqueOracle::ft_witnessed_exact(
        &target,
        &dag,
        &snapshot,
        FtThreshold::from_f32_lossy(0.1),
        false,
    )
    .await
    .expect("oracle verdict");
    assert!(finalized, "a unanimous committee must finalize the target");
    metadata_reads.load(Ordering::SeqCst)
}

#[tokio::test]
async fn clique_oracle_metadata_reads_grow_linearly_with_committee() {
    let mut reads: HashMap<usize, usize> = HashMap::new();
    for validator_count in [5, 10, 20] {
        let count = metadata_reads_for_one_oracle_call(validator_count).await;
        assert!(
            count <= 4 * validator_count,
            "one oracle call read {} block-metadata rows for a committee of {}: the pairwise \
             disagreement walk must not deserialize metadata per validator pair",
            count,
            validator_count
        );
        reads.insert(validator_count, count);
    }
    assert!(
        reads[&20] <= 5 * reads[&5],
        "metadata reads must scale linearly with the committee, got {:?}",
        reads
    );
}
