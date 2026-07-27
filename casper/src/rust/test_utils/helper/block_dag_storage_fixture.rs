// See casper/src/test/scala/coop/rchain/casper/helper/BlockDagStorageFixture.scala
// Moved from casper/tests/helper/block_dag_storage_fixture.rs to casper/src/rust/test_utils/helper/block_dag_storage_fixture.rs
// All imports fixed for library crate context
//
// ## ⚠ This copy used to omit the shared-LMDB lock
//
// There are two copies of this module: this one (a `pub` module of the casper LIBRARY, so
// other crates can use it) and `casper/tests/helper/` (a module of the `casper/tests/mod.rs`
// integration target). They present the same two functions with the same signatures.
//
// The test copy acquired `resources::SHARED_LMDB_LOCK` for the whole fixture body. THIS copy
// acquired nothing — the lock was declared in the test target, so it was not even nameable
// here. Both copies drive the SAME shared LMDB environment, so the two behaved differently
// in the one respect that decides whether concurrent tests corrupt each other:
//
//   Test A: insert(block_A) -> unlock -> get_representation() -> reads a snapshot
//   Test B: insert(block_B) -> unlock -> (writes to the SAME LMDB)
//   Test A: validate()      -> looks up block_B -> "DAG storage is missing hash"
//
// and the unguarded copy was the one another crate would reach for, because the test copy is
// not importable. The lock now lives in the library beside the environment it guards, the
// test lane re-exports it, and both copies take it. See `SHARED_LMDB_LOCK`.

use std::future::Future;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;

use crate::rust::test_utils::util::genesis_builder::GenesisContext;
use crate::rust::test_utils::util::rholang::resources;
use crate::rust::test_utils::util::rholang::resources::SHARED_LMDB_LOCK;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub async fn with_genesis<F, Fut, R>(context: GenesisContext, f: F) -> R
where
    F: FnOnce(KeyValueBlockStore, IndexedBlockDagStorage, RuntimeManager) -> Fut,
    Fut: Future<Output = R>,
{
    // Acquire the global shared-LMDB lock for the whole fixture body, exactly as the test-target
    // copy does. Held for the entire test duration: releasing it earlier would let another
    // fixture write to the shared environment between this test's insert and its read.
    let _lock_guard = SHARED_LMDB_LOCK.lock().await;

    async fn create(
        genesis_context: &GenesisContext,
    ) -> (KeyValueBlockStore, IndexedBlockDagStorage, RuntimeManager) {
        let scope_id = genesis_context.rspace_scope_id.clone();
        let mut kvm = resources::mk_test_rnode_store_manager_shared(scope_id);

        let blocks = KeyValueBlockStore::create_from_kvm(&mut *kvm)
            .await
            .unwrap();
        blocks
            .put(
                genesis_context.genesis_block.block_hash.clone(),
                &genesis_context.genesis_block,
            )
            .expect("Failed to put genesis block");

        let dag = resources::block_dag_storage_from_dyn(&mut *kvm)
            .await
            .unwrap();
        dag.insert(
            &genesis_context.genesis_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
        )
        .expect("Failed to insert genesis block into DAG");

        let indexed_dag = IndexedBlockDagStorage::new(dag);

        let (runtime, _history_repo) =
            resources::mk_runtime_manager_with_history_at(&mut *kvm).await;

        (blocks, indexed_dag, runtime)
    }

    let (blocks, indexed_dag, runtime) = create(&context).await;
    f(blocks, indexed_dag, runtime).await
}

pub async fn with_storage<F, Fut, R>(f: F) -> R
where
    F: FnOnce(KeyValueBlockStore, IndexedBlockDagStorage) -> Fut,
    Fut: Future<Output = R>,
{
    // Acquire the global shared-LMDB lock for the whole fixture body, exactly as the test-target
    // copy does. Held for the entire test duration: releasing it earlier would let another
    // fixture write to the shared environment between this test's insert and its read.
    let _lock_guard = SHARED_LMDB_LOCK.lock().await;

    async fn create() -> (KeyValueBlockStore, IndexedBlockDagStorage) {
        let scope_id = resources::generate_scope_id();
        let mut kvm = resources::mk_test_rnode_store_manager_shared(scope_id);
        let blocks = KeyValueBlockStore::create_from_kvm(&mut *kvm)
            .await
            .unwrap();
        let dag = resources::block_dag_storage_from_dyn(&mut *kvm)
            .await
            .unwrap();
        let indexed_dag = IndexedBlockDagStorage::new(dag);

        (blocks, indexed_dag)
    }

    // Note: init_logger removed - logging should be initialized by test framework
    let (blocks, indexed_dag) = create().await;
    f(blocks, indexed_dag).await
}
