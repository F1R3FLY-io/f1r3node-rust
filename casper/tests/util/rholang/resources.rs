// See casper/src/test/scala/coop/rchain/casper/util/rholang/Resources.scala

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::errors::CasperError;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::storage::rnode_key_value_store_manager::rnode_db_mapping;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use dashmap::DashSet;
use lazy_static::lazy_static;
use models::rhoapi::Par;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use parking_lot::RwLock;
use prost::bytes::Bytes;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{LmdbDirStoreManager, GB};
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use tempfile::{Builder, TempDir};

use crate::init_logger;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

static CACHED_GENESIS: OnceLock<Arc<Mutex<Option<GenesisContext>>>> = OnceLock::new();

lazy_static! {
    /// Serializes the block-DAG storage fixtures, which share the cached
    /// genesis RSpace stores.
    pub static ref SHARED_LMDB_LOCK: Mutex<()> = Mutex::new(());
}

static TEST_LMDB_ROOT: OnceLock<TempDir> = OnceLock::new();

extern "C" fn remove_test_lmdb_root() {
    if let Some(root) = TEST_LMDB_ROOT.get() {
        if let Err(e) = std::fs::remove_dir_all(root.path()) {
            eprintln!("Failed to remove {}: {}", root.path().display(), e);
        }
    }
}

/// Each test scope opens its own LMDB environments, so concurrent tests hold
/// more files than common default soft limits allow.
fn raise_open_file_limit() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit only writes the provided struct.
    let read = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) };
    assert_eq!(read, 0, "getrlimit(RLIMIT_NOFILE) failed");
    limit.rlim_cur = max_open_files(limit.rlim_max);
    // SAFETY: setrlimit only reads the provided struct.
    let written = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) };
    assert_eq!(written, 0, "setrlimit(RLIMIT_NOFILE) failed");
}

#[cfg(not(target_os = "macos"))]
fn max_open_files(hard_limit: libc::rlim_t) -> libc::rlim_t { hard_limit }

/// macOS rejects a soft limit above `kern.maxfilesperproc`, even when the hard
/// limit is unlimited.
#[cfg(target_os = "macos")]
fn max_open_files(hard_limit: libc::rlim_t) -> libc::rlim_t {
    let mut per_process: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: the name is NUL-terminated and `size` matches the output buffer.
    let read = unsafe {
        libc::sysctlbyname(
            c"kern.maxfilesperproc".as_ptr(),
            (&mut per_process as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(read, 0, "sysctlbyname(kern.maxfilesperproc) failed");
    hard_limit.min(per_process as libc::rlim_t)
}

/// Parent of every test LMDB directory. Cached genesis scopes live in statics,
/// which are never dropped, so the root is removed at process exit instead.
fn test_lmdb_root() -> &'static Path {
    TEST_LMDB_ROOT
        .get_or_init(|| {
            raise_open_file_limit();
            let root = Builder::new()
                .prefix("casper-test-lmdb-")
                .tempdir()
                .expect("Failed to create test LMDB root");
            // SAFETY: the callback is a plain function that captures no state.
            let registered = unsafe { libc::atexit(remove_test_lmdb_root) };
            assert_eq!(registered, 0, "Failed to register test LMDB root cleanup");
            root
        })
        .path()
}

/// An LMDB data directory owned by a test, removed when the last clone drops.
#[derive(Clone)]
pub struct TestScope(Arc<TempDir>);

impl TestScope {
    pub fn new() -> Self {
        Self(Arc::new(
            Builder::new()
                .prefix("scope-")
                .tempdir_in(test_lmdb_root())
                .expect("Failed to create test LMDB dir"),
        ))
    }

    pub fn path(&self) -> &Path { self.0.path() }
}

impl Default for TestScope {
    fn default() -> Self { Self::new() }
}

/// A store manager that keeps its data directory alive for as long as it can
/// open environments in it.
pub struct TestStoreManager {
    manager: Box<dyn KeyValueStoreManager>,
    _scope: TestScope,
}

impl Deref for TestStoreManager {
    type Target = dyn KeyValueStoreManager;

    fn deref(&self) -> &Self::Target { &*self.manager }
}

impl DerefMut for TestStoreManager {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *self.manager }
}

#[allow(clippy::await_holding_lock)]
pub async fn genesis_context() -> Result<GenesisContext, CasperError> {
    let genesis_arc = CACHED_GENESIS
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();

    let mut genesis_guard = genesis_arc.lock().unwrap();

    if genesis_guard.is_none() {
        let mut genesis_builder = GenesisBuilder::new();
        let new_genesis = genesis_builder.build_genesis_with_parameters(None).await?;
        *genesis_guard = Some(new_genesis);
    }

    Ok(genesis_guard.as_ref().unwrap().clone())
}

pub async fn with_runtime_manager<F, Fut, R>(f: F) -> Result<R, CasperError>
where
    F: FnOnce(RuntimeManager, GenesisContext, BlockMessage) -> Fut,
    Fut: Future<Output = R>,
{
    init_logger();
    let genesis_context = genesis_context().await?;
    let genesis_block = genesis_context.genesis_block.clone();

    // Use the genesis scope to access all genesis data including RSpace history
    // This ensures tests can reset to the genesis state root hash
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    // Use create_with_history to ensure tests can reset to genesis state root hash
    let (runtime_manager, _history_repo) = mk_runtime_manager_with_history_at(&mut *kvm).await;

    Ok(f(runtime_manager, genesis_context, genesis_block).await)
}

/// Creates the production store layout in `scope`'s directory.
pub fn mk_test_rnode_store_manager(scope: &TestScope) -> TestStoreManager {
    // Production env map sizes reach terabytes of address space; tests hold
    // many directories open at once, so each env is capped.
    let limit_size = 4 * GB;

    let db_mapping = rnode_db_mapping(None)
        .into_iter()
        .map(|(db, mut conf)| {
            conf.max_env_size = conf.max_env_size.min(limit_size);
            (db, conf)
        })
        .collect();

    TestStoreManager {
        manager: Box::new(LmdbDirStoreManager::new(
            scope.path().to_path_buf(),
            db_mapping,
        )),
        _scope: scope.clone(),
    }
}

/// Creates a store manager for a new node in its own directory, seeded with
/// the genesis block. The node's RSpace stores come from the genesis scope.
pub async fn mk_test_node_store_manager(
    genesis_context: &GenesisContext,
) -> Result<TestStoreManager, CasperError> {
    let mut new_kvm = mk_test_rnode_store_manager(&TestScope::new());

    // Copy genesis block to the new scope's block store
    let new_block_store = KeyValueBlockStore::create_from_kvm(&mut *new_kvm).await?;
    new_block_store.put(
        genesis_context.genesis_block.block_hash.clone(),
        &genesis_context.genesis_block,
    )?;

    // Copy genesis DAG metadata to the new scope's DAG storage
    let new_dag_storage = block_dag_storage_from_dyn(&mut *new_kvm)
        .await
        .map_err(|e| CasperError::RuntimeError(format!("Failed to create DAG storage: {:?}", e)))?;
    new_dag_storage.insert(
        &genesis_context.genesis_block,
        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
    )?;

    Ok(new_kvm)
}

/// Opens the genesis scope, where genesis RSpace history and roots live.
///
/// Note: Multiple tests using this will share the same RSpace state.
pub fn mk_test_rnode_store_manager_from_genesis(
    genesis_context: &GenesisContext,
) -> TestStoreManager {
    mk_test_rnode_store_manager(&genesis_context.rspace_scope)
}

type MergeableStore = shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl<
    shared::rust::ByteVector,
    Vec<rholang::rust::interpreter::merging::rholang_merging_logic::DeployMergeableData>,
>;

pub async fn mergeable_store_from_dyn(
    kvm: &mut dyn KeyValueStoreManager,
) -> Result<MergeableStore, shared::rust::store::key_value_store::KvStoreError> {
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    let store = kvm
        .store("mergeable-channel-cache".to_string())
        .await
        .map_err(|e| {
            shared::rust::store::key_value_store::KvStoreError::IoError(format!(
                "Failed to get mergeable store: {:?}",
                e
            ))
        })?;
    Ok(KeyValueTypedStoreImpl::new(store))
}

pub async fn block_dag_storage_from_dyn(
    kvm: &mut dyn KeyValueStoreManager,
) -> Result<
    block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage,
    shared::rust::store::key_value_store::KvStoreError,
> {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use block_storage::rust::dag::equivocation_tracker_store::EquivocationTrackerStore;
    use models::rust::block_hash::BlockHashSerde;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::equivocation_record::SequenceNumber;
    use models::rust::validator::ValidatorSerde;
    use parking_lot::RwLock;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    let block_metadata_kv_store = kvm.store("block-metadata".to_string()).await.map_err(|e| {
        shared::rust::store::key_value_store::KvStoreError::IoError(format!(
            "Failed to get block-metadata store: {:?}",
            e
        ))
    })?;
    let block_metadata_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
        KeyValueTypedStoreImpl::new(block_metadata_kv_store);
    let block_metadata_store = BlockMetadataStore::new(block_metadata_db);

    let equivocation_tracker_kv_store = kvm
        .store("equivocation-tracker".to_string())
        .await
        .map_err(|e| {
            shared::rust::store::key_value_store::KvStoreError::IoError(format!(
                "Failed to get equivocation-tracker store: {:?}",
                e
            ))
        })?;
    let equivocation_tracker_db: KeyValueTypedStoreImpl<
        (ValidatorSerde, SequenceNumber),
        BTreeSet<BlockHashSerde>,
    > = KeyValueTypedStoreImpl::new(equivocation_tracker_kv_store);
    let equivocation_tracker_store = EquivocationTrackerStore::new(equivocation_tracker_db);

    let latest_messages_kv_store = kvm
        .store("latest-messages".to_string())
        .await
        .map_err(|e| {
            shared::rust::store::key_value_store::KvStoreError::IoError(format!(
                "Failed to get latest-messages store: {:?}",
                e
            ))
        })?;
    let latest_messages_db: KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde> =
        KeyValueTypedStoreImpl::new(latest_messages_kv_store);

    let invalid_blocks_kv_store = kvm.store("invalid-blocks".to_string()).await.map_err(|e| {
        shared::rust::store::key_value_store::KvStoreError::IoError(format!(
            "Failed to get invalid-blocks store: {:?}",
            e
        ))
    })?;
    let invalid_blocks_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
        KeyValueTypedStoreImpl::new(invalid_blocks_kv_store);

    Ok(BlockDagKeyValueStorage::from_parts(
        Arc::new(RwLock::new(())),
        latest_messages_db,
        Arc::new(RwLock::new(block_metadata_store)),
        invalid_blocks_db,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        equivocation_tracker_store,
        Arc::new(AtomicU64::new(0)),
    ))
}

pub async fn key_value_deploy_storage_from_dyn(
    kvm: &mut dyn KeyValueStoreManager,
) -> Result<
    block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage,
    shared::rust::store::key_value_store::KvStoreError,
> {
    use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
    use shared::rust::ByteString;

    let deploy_storage_kv_store = kvm.store("deploy_storage".to_string()).await.map_err(|e| {
        shared::rust::store::key_value_store::KvStoreError::IoError(format!(
            "Failed to get deploy_storage store: {:?}",
            e
        ))
    })?;
    let deploy_storage_db: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>> =
        KeyValueTypedStoreImpl::new(deploy_storage_kv_store);

    Ok(KeyValueDeployStorage {
        store: deploy_storage_db,
    })
}

pub async fn key_value_rejected_deploy_buffer_from_dyn(
    kvm: &mut dyn KeyValueStoreManager,
) -> Result<
    block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer,
    shared::rust::store::key_value_store::KvStoreError,
> {
    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
    use shared::rust::ByteString;

    let buffer_kv_store = kvm
        .store("rejected_deploy_buffer".to_string())
        .await
        .map_err(|e| {
            shared::rust::store::key_value_store::KvStoreError::IoError(format!(
                "Failed to get rejected_deploy_buffer store: {:?}",
                e
            ))
        })?;
    let buffer_db: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>> =
        KeyValueTypedStoreImpl::new(buffer_kv_store);

    Ok(KeyValueRejectedDeployBuffer { store: buffer_db })
}

pub async fn casper_buffer_storage_from_dyn(
    kvm: &mut dyn KeyValueStoreManager,
) -> Result<
    block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage,
    shared::rust::store::key_value_store::KvStoreError,
> {
    use std::collections::HashSet;

    use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
    use models::rust::block_hash::BlockHashSerde;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    let parents_store_kv = kvm.store("parents-map".to_string()).await.map_err(|e| {
        shared::rust::store::key_value_store::KvStoreError::IoError(format!(
            "Failed to get parents-map store: {:?}",
            e
        ))
    })?;
    let parents_store: KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>> =
        KeyValueTypedStoreImpl::new(parents_store_kv);

    CasperBufferKeyValueStorage::new_from_kv_store(parents_store)
        .await
        .map_err(|e| {
            shared::rust::store::key_value_store::KvStoreError::IoError(format!(
                "Failed to create CasperBufferKeyValueStorage: {:?}",
                e
            ))
        })
}

pub async fn mk_runtime_manager(
    _prefix: &str,
    mergeable_tags: Option<
        std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
    >,
) -> RuntimeManager {
    let mut kvm = mk_test_rnode_store_manager(&TestScope::new());

    mk_runtime_manager_at(&mut *kvm, mergeable_tags).await
}

pub async fn mk_runtime_manager_at(
    kvm: &mut dyn KeyValueStoreManager,
    mergeable_tags: Option<
        std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
    >,
) -> RuntimeManager {
    let mergeable_tags =
        mergeable_tags.unwrap_or_else(|| std::sync::Arc::new(Genesis::default_mergeable_tags()));

    let r_store = kvm.r_space_stores().await.unwrap();
    let m_store = mergeable_store_from_dyn(kvm).await.unwrap();
    RuntimeManager::create_with_store(
        r_store,
        m_store,
        mergeable_tags,
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    )
}

pub async fn mk_runtime_manager_with_history_at(
    kvm: &mut dyn KeyValueStoreManager,
) -> (RuntimeManager, RhoHistoryRepository) {
    let r_store = kvm.r_space_stores().await.unwrap();
    let m_store = mergeable_store_from_dyn(kvm).await.unwrap();
    let (rt_manager, history_repo) = RuntimeManager::create_with_history(
        r_store,
        m_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );
    (rt_manager, history_repo)
}

pub fn new_key_value_dag_representation() -> KeyValueDagRepresentation {
    let block_metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));

    KeyValueDagRepresentation {
        dag_set: imbl::HashSet::new(),
        latest_messages_map: imbl::HashMap::new(),
        child_map: imbl::HashMap::new(),
        height_map: imbl::OrdMap::new(),
        block_number_map: imbl::HashMap::new(),
        main_parent_map: imbl::HashMap::new(),
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        last_finalized_block_hash: BlockHash::new(),
        finalized_blocks_set: imbl::HashSet::new(),
        block_metadata_index: Arc::new(RwLock::new(BlockMetadataStore::new(block_metadata_store))),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(RwLock::new(
            block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(),
        )),
        carrier_index: Arc::new(RwLock::new(
            block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
        )),
    }
}

pub fn mk_dummy_casper_snapshot() -> CasperSnapshot {
    let dag = new_key_value_dag_representation();

    CasperSnapshot {
        dag,
        last_finalized_block: Bytes::new(),
        parents: Vec::new(),
        justifications: HashSet::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num: 0,
        max_seq_nums: HashMap::new(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf::new(),
            bonds_map: HashMap::new(),
            active_validators: Vec::new(),
        },
    }
}
