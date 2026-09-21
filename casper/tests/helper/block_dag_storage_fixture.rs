// See casper/src/test/scala/coop/rchain/casper/helper/BlockDagStorageFixture.scala

use std::future::Future;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;

use crate::init_logger;
use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources;

#[allow(clippy::await_holding_lock)]
pub async fn with_genesis<F, Fut, R>(context: GenesisContext, f: F) -> R
where
    F: FnOnce(KeyValueBlockStore, IndexedBlockDagStorage, RuntimeManager) -> Fut,
    Fut: Future<Output = R>,
{
    let _lock_guard = resources::SHARED_LMDB_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    async fn create(
        genesis_context: &GenesisContext,
    ) -> (KeyValueBlockStore, IndexedBlockDagStorage, RuntimeManager) {
        let mut kvm = resources::mk_test_node_store_manager(genesis_context)
            .await
            .unwrap();

        let blocks = KeyValueBlockStore::create_from_kvm(&mut *kvm)
            .await
            .unwrap();

        let dag = resources::block_dag_storage_from_dyn(&mut *kvm)
            .await
            .unwrap();
        dag.insert(
            &genesis_context.genesis_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
        )
        .expect("Failed to insert genesis block into DAG");

        let indexed_dag = IndexedBlockDagStorage::new(dag);

        let rspace_store = resources::mk_test_rnode_store_manager_from_genesis(genesis_context)
            .r_space_stores()
            .await
            .unwrap();
        let mergeable_store = resources::mergeable_store_from_dyn(&mut *kvm)
            .await
            .unwrap();
        let (runtime, _history_repo) = RuntimeManager::create_with_history(
            rspace_store,
            mergeable_store,
            std::sync::Arc::new(Genesis::default_mergeable_tags()),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        );

        (blocks, indexed_dag, runtime)
    }

    let (blocks, indexed_dag, runtime) = create(&context).await;
    f(blocks, indexed_dag, runtime).await
}

#[allow(clippy::await_holding_lock)]
pub async fn with_storage<F, Fut, R>(f: F) -> R
where
    F: FnOnce(KeyValueBlockStore, IndexedBlockDagStorage) -> Fut,
    Fut: Future<Output = R>,
{
    let _lock_guard = resources::SHARED_LMDB_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    async fn create() -> (KeyValueBlockStore, IndexedBlockDagStorage) {
        let mut kvm = resources::mk_test_rnode_store_manager(&resources::TestScope::new());
        let blocks = KeyValueBlockStore::create_from_kvm(&mut *kvm)
            .await
            .unwrap();
        let dag = resources::block_dag_storage_from_dyn(&mut *kvm)
            .await
            .unwrap();
        let indexed_dag = IndexedBlockDagStorage::new(dag);

        (blocks, indexed_dag)
    }

    init_logger();

    let (blocks, indexed_dag) = create().await;
    f(blocks, indexed_dag).await
}
