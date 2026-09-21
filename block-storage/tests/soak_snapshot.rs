use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::dag::soak_snapshot::{
    capture, capture_observed, Availability, BlockBody, CaptureLimits, CapturePhase,
    CaptureRequest, EXCLUDED_STORES, SNAPSHOT_SCHEMA_VERSION,
};
use block_storage::rust::key_value_block_store::{BlockDecodeLimits, KeyValueBlockStore};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, Justification};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::store::soak_snapshot::{ReadLimits, SnapshotError};
use tokio::runtime::Runtime;

const DAG_STORES: [&str; 11] = [
    "block-metadata",
    "equivocation-tracker",
    "latest-messages",
    "invalid-blocks",
    "floor-index",
    "frontier-index",
    "deploy-lifecycle-events",
    "deploy-lifecycle-terminal",
    "carrier-index",
    "carrier-index-meta",
    "genesis-hash",
];
const BLOCK_STORES: [&str; 2] = ["blocks", "blocks-approved"];

fn unique_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "f1r3-soak-snapshot-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn env_config(name: &str) -> LmdbEnvConfig {
    LmdbEnvConfig {
        name: name.to_string(),
        max_env_size: 64 * 1024 * 1024,
        max_dbs: 32,
    }
}

fn mapping() -> HashMap<Db, LmdbEnvConfig> {
    let mut map = HashMap::new();
    for name in DAG_STORES {
        map.insert(Db::new(name.to_string(), None), env_config("dagstorage"));
    }
    for name in BLOCK_STORES {
        map.insert(Db::new(name.to_string(), None), env_config("blockstorage"));
    }
    map
}

struct Fixture {
    dir: PathBuf,
    dag: BlockDagKeyValueStorage,
    blocks: KeyValueBlockStore,
    handles: HashMap<&'static str, Arc<dyn KeyValueStore>>,
    chain: Vec<BlockMessage>,
}

impl Fixture {
    fn handle(&self, name: &'static str) -> Arc<dyn KeyValueStore> { self.handles[name].clone() }

    fn dag_env_path(&self) -> String { self.dir.join("dagstorage").display().to_string() }

    fn blocks_env_path(&self) -> String { self.dir.join("blockstorage").display().to_string() }

    fn store_bytes(&self) -> BTreeMap<&'static str, BTreeMap<Vec<u8>, Vec<u8>>> {
        self.handles
            .iter()
            .map(|(name, store)| (*name, store.to_map().unwrap()))
            .collect()
    }

    fn write_from_other_thread(
        &self,
        name: &'static str,
        f: impl FnOnce(Arc<dyn KeyValueStore>) + Send + 'static,
    ) {
        let store = self.handle(name);
        std::thread::spawn(move || f(store)).join().unwrap();
    }
}

fn genesis() -> BlockMessage {
    get_random_block(
        Some(0),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![]),
        Some(vec![]),
        Some(vec![]),
        None,
        Some(vec![]),
        None,
        None,
    )
}

fn child(number: i64, parents: Vec<BlockHash>, justify: Option<BlockHash>) -> BlockMessage {
    let mut block = get_random_block(
        Some(number),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(parents),
        Some(vec![]),
        Some(vec![]),
        None,
        Some(vec![]),
        None,
        None,
    );
    if let Some(latest) = justify {
        block.justifications = vec![Justification {
            validator: block.sender.clone(),
            latest_block_hash: latest,
        }];
    }
    block
}

fn fixture(tag: &str) -> Fixture {
    let dir = unique_dir(tag);
    let runtime = Runtime::new().unwrap();
    let (dag, blocks, handles) = runtime.block_on(async {
        let mut kvm = LmdbDirStoreManager::new(dir.clone(), mapping());
        let dag = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
        let mut handles: HashMap<&'static str, Arc<dyn KeyValueStore>> = HashMap::new();
        for name in DAG_STORES.iter().chain(BLOCK_STORES.iter()) {
            handles.insert(name, kvm.store(name.to_string()).await.unwrap());
        }
        (dag, blocks, handles)
    });
    drop(runtime);

    let g = genesis();
    let b1 = child(1, vec![g.block_hash.clone()], Some(g.block_hash.clone()));
    let b2 = child(2, vec![b1.block_hash.clone()], Some(b1.block_hash.clone()));
    let b3 = child(
        3,
        vec![
            b2.block_hash.clone(),
            g.block_hash.clone(),
            b1.block_hash.clone(),
        ],
        Some(b2.block_hash.clone()),
    );
    let chain = vec![g, b1, b2, b3];
    for (index, block) in chain.iter().enumerate() {
        let mode = if index == 0 {
            InsertMode::Approved
        } else {
            InsertMode::Normal
        };
        dag.insert(block, mode).unwrap();
        blocks.put_block_message(block).unwrap();
    }

    Fixture {
        dir,
        dag,
        blocks,
        handles,
        chain,
    }
}

fn limits() -> CaptureLimits {
    CaptureLimits {
        read: ReadLimits {
            max_value_bytes: 1 << 20,
            max_total_bytes: 1 << 26,
            max_records: 10_000,
            max_operations: 100_000,
        },
        block_decode: BlockDecodeLimits {
            max_compressed_bytes: 1 << 20,
            max_decompressed_bytes: 1 << 24,
            max_expansion_ratio: 255,
        },
        max_blocks: 1_000,
        max_validators: 1_000,
        max_edges: 10_000,
        max_work: 100_000,
        lock_wait: Duration::from_secs(5),
    }
}

fn request<'a>(bodies: &'a [BlockHash]) -> CaptureRequest<'a> {
    CaptureRequest {
        limits: limits(),
        bodies,
    }
}

fn metadata_store(
    handle: Arc<dyn KeyValueStore>,
) -> KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> {
    KeyValueTypedStoreImpl::new(handle)
}

fn floor_store(
    handle: Arc<dyn KeyValueStore>,
) -> KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> {
    KeyValueTypedStoreImpl::new(handle)
}

#[test]
fn invalid_limits_are_rejected_before_guards_or_reads() {
    let fx = fixture("limits");
    let phases = Arc::new(Mutex::new(Vec::new()));
    let mut bad = limits();
    bad.max_work = 0;
    let observed = phases.clone();
    let result = capture_observed(
        &fx.dag,
        &fx.blocks,
        &CaptureRequest {
            limits: bad,
            bodies: &[],
        },
        move |phase| observed.lock().unwrap().push(phase),
    );
    assert!(matches!(result, Err(SnapshotError::InvalidLimits(_))));
    assert!(phases.lock().unwrap().is_empty());

    let mut zero_wait = limits();
    zero_wait.lock_wait = Duration::ZERO;
    assert!(matches!(
        CaptureLimits::validate(&zero_wait),
        Err(SnapshotError::InvalidLimits(_))
    ));
}

#[test]
fn in_memory_backend_is_unsupported() {
    let runtime = Runtime::new().unwrap();
    let (dag, blocks) = runtime.block_on(async {
        let mut kvm = InMemoryStoreManager::new();
        let dag = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
        (dag, blocks)
    });
    drop(runtime);
    let g = genesis();
    dag.insert(&g, InsertMode::Approved).unwrap();
    let result = capture(&dag, &blocks, &request(&[]));
    assert!(matches!(result, Err(SnapshotError::Unsupported(_))));
}

#[test]
fn capture_copies_the_held_dag_with_ordered_parents_and_availability_states() {
    let fx = fixture("copy");
    let b3 = &fx.chain[3];
    let unknown = BlockHash::from(vec![0xEEu8; 32]);
    let bodies = vec![b3.block_hash.clone(), unknown.clone()];
    let phases = Arc::new(Mutex::new(Vec::new()));
    let observed = phases.clone();
    let snapshot = capture_observed(&fx.dag, &fx.blocks, &request(&bodies), move |phase| {
        observed.lock().unwrap().push(phase)
    })
    .unwrap();

    assert_eq!(*phases.lock().unwrap(), vec![
        CapturePhase::GuardsHeld,
        CapturePhase::StateCopied,
        CapturePhase::RowsRead,
        CapturePhase::Validated,
        CapturePhase::GuardsReleased,
    ]);
    assert_eq!(snapshot.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snapshot.coverage.held_blocks, 4);
    assert!(snapshot.coverage.complete_held_dag);
    assert_eq!(snapshot.coverage.requested_bodies, 2);
    assert_eq!(snapshot.dag_set.len(), 4);
    assert_eq!(snapshot.blocks.len(), 4);
    assert_eq!(snapshot.transactions.len(), 2);
    let environments: Vec<&str> = snapshot
        .transactions
        .iter()
        .map(|txn| txn.environment.as_str())
        .collect();
    assert!(environments.contains(&fx.dag_env_path().as_str()));
    assert!(environments.contains(&fx.blocks_env_path().as_str()));
    for txn in &snapshot.transactions {
        assert_eq!(txn.last_txn_id_after_validation, Some(txn.txn_id));
    }

    let captured_b3 = &snapshot.blocks[&b3.block_hash];
    assert_eq!(captured_b3.parents, b3.header.parents_hash_list);
    assert_eq!(captured_b3.metadata.parents, b3.header.parents_hash_list);
    assert_eq!(captured_b3.floor, Availability::Absent);
    assert_eq!(captured_b3.frontier, Availability::Absent);
    assert_eq!(
        snapshot.main_parent_map[&b3.block_hash],
        b3.header.parents_hash_list[0]
    );
    assert_eq!(
        snapshot.self_justification_map[&b3.block_hash],
        fx.chain[2].block_hash
    );
    assert_eq!(
        snapshot.last_finalized_block,
        Some((fx.chain[0].block_hash.clone(), 0))
    );
    assert!(snapshot
        .finalized_block_set
        .contains(&fx.chain[0].block_hash));
    assert_eq!(snapshot.latest_messages.len(), 4);
    assert!(snapshot.invalid_blocks.is_empty());
    assert_eq!(snapshot.insertion_generation, fx.dag.current_generation());

    assert_eq!(snapshot.bodies.len(), 2);
    match &snapshot.bodies[&b3.block_hash] {
        BlockBody::Held(block) => assert_eq!(block, b3),
        other => panic!("expected held body, got {other:?}"),
    }
    assert_eq!(snapshot.bodies[&unknown], BlockBody::NotHeld);
    assert_eq!(snapshot.digest().len(), 32);
    assert_eq!(snapshot.digest_hex().len(), 64);
}

#[test]
fn present_cache_seeds_are_captured_and_absent_seeds_stay_absent() {
    let fx = fixture("seeds");
    let b1 = fx.chain[1].block_hash.clone();
    let g = fx.chain[0].block_hash.clone();
    floor_store(fx.handle("floor-index"))
        .put_one(BlockHashSerde(b1.clone()), BlockHashSerde(g.clone()))
        .unwrap();

    let snapshot = capture(&fx.dag, &fx.blocks, &request(&[])).unwrap();
    assert_eq!(snapshot.blocks[&b1].floor, Availability::Present(g));
    assert_eq!(snapshot.blocks[&b1].frontier, Availability::Absent);
    assert_eq!(
        snapshot.blocks[&fx.chain[2].block_hash].floor,
        Availability::Absent
    );
}

#[test]
fn finalization_row_write_without_generation_change_is_rejected() {
    let fx = fixture("finalization");
    let b2 = fx.chain[2].clone();
    let generation_before = fx.dag.current_generation();
    let bytes_before = fx.store_bytes();

    let target = b2.block_hash.clone();
    let fx_ref = &fx;
    let result = capture_observed(&fx.dag, &fx.blocks, &request(&[]), move |phase| {
        if phase == CapturePhase::RowsRead {
            let hash = target.clone();
            fx_ref.write_from_other_thread("block-metadata", move |store| {
                let typed = metadata_store(store);
                let mut metadata = typed.get_unsafe(&BlockHashSerde(hash.clone())).unwrap();
                metadata.finalized = true;
                metadata.fault_tolerance_value = 0.9;
                typed.put_one(BlockHashSerde(hash), metadata).unwrap();
            });
        }
    });

    match result {
        Err(SnapshotError::EnvironmentChanged { environment, .. }) => {
            assert_eq!(environment, fx.dag_env_path());
        }
        other => panic!("expected environment change, got {:?}", other.map(|_| ())),
    }
    assert_eq!(fx.dag.current_generation(), generation_before);

    let bytes_after = fx.store_bytes();
    assert_ne!(
        bytes_before["block-metadata"],
        bytes_after["block-metadata"]
    );
    for name in DAG_STORES.iter().chain(BLOCK_STORES.iter()) {
        if *name != "block-metadata" {
            assert_eq!(bytes_before[name], bytes_after[name]);
        }
    }
}

#[test]
fn cache_row_write_without_global_lock_is_rejected() {
    let fx = fixture("cache");
    let b1 = fx.chain[1].block_hash.clone();
    let g = fx.chain[0].block_hash.clone();
    let generation_before = fx.dag.current_generation();

    let fx_ref = &fx;
    let result = capture_observed(&fx.dag, &fx.blocks, &request(&[]), move |phase| {
        if phase == CapturePhase::RowsRead {
            let (b1, g) = (b1.clone(), g.clone());
            fx_ref.write_from_other_thread("frontier-index", move |store| {
                floor_store(store)
                    .put_one(BlockHashSerde(b1), BlockHashSerde(g))
                    .unwrap();
            });
        }
    });
    assert!(matches!(
        result,
        Err(SnapshotError::EnvironmentChanged { .. })
    ));
    assert_eq!(fx.dag.current_generation(), generation_before);
}

#[test]
fn block_store_environment_change_is_reported_separately() {
    let fx = fixture("blocks-env");
    let extra = child(9, vec![fx.chain[0].block_hash.clone()], None);
    let fx_ref = &fx;
    let result = capture_observed(&fx.dag, &fx.blocks, &request(&[]), move |phase| {
        if phase == CapturePhase::RowsRead {
            let block = extra.clone();
            fx_ref.write_from_other_thread("blocks", move |store| {
                let approved: Arc<dyn KeyValueStore> = store.clone();
                KeyValueBlockStore::new(store, approved)
                    .put_block_message(&block)
                    .unwrap();
            });
        }
    });
    match result {
        Err(SnapshotError::EnvironmentChanged { environment, .. }) => {
            assert_eq!(environment, fx.blocks_env_path());
        }
        other => panic!(
            "expected block environment change, got {:?}",
            other.map(|_| ())
        ),
    }
}

#[test]
fn captured_data_survives_later_live_mutations() {
    let fx = fixture("survive");
    let snapshot = capture(
        &fx.dag,
        &fx.blocks,
        &request(&[fx.chain[3].block_hash.clone()]),
    )
    .unwrap();
    let digest = snapshot.digest();
    let blocks_before = snapshot.blocks.clone();
    let bodies_before = snapshot.bodies.clone();

    let b4 = child(
        4,
        vec![fx.chain[3].block_hash.clone()],
        Some(fx.chain[3].block_hash.clone()),
    );
    fx.dag.insert(&b4, InsertMode::Normal).unwrap();
    fx.blocks.put_block_message(&b4).unwrap();
    let live = fx.dag.get_representation().unwrap();
    live.floor_index
        .put_one(
            BlockHashSerde(fx.chain[3].block_hash.clone()),
            BlockHashSerde(fx.chain[0].block_hash.clone()),
        )
        .unwrap();
    metadata_store(fx.handle("block-metadata"))
        .put_one(BlockHashSerde(fx.chain[1].block_hash.clone()), {
            let mut m = BlockMetadata::from_block(&fx.chain[1], false, None, None);
            m.finalized = true;
            m
        })
        .unwrap();

    assert_eq!(snapshot.digest(), digest);
    assert_eq!(snapshot.blocks, blocks_before);
    assert_eq!(snapshot.bodies, bodies_before);
    assert_eq!(snapshot.dag_set.len(), 4);
    assert!(!snapshot.dag_set.contains(&b4.block_hash));

    let fresh = capture(&fx.dag, &fx.blocks, &request(&[])).unwrap();
    assert_ne!(fresh.digest(), digest);
    assert_eq!(fresh.dag_set.len(), 5);
}

#[test]
fn scratch_views_share_no_mutable_stores_with_each_other_or_production() {
    let fx = fixture("scratch");
    let snapshot = capture(&fx.dag, &fx.blocks, &request(&[])).unwrap();
    let view_a = snapshot.scratch_view().unwrap();
    let view_b = snapshot.scratch_view().unwrap();
    assert_eq!(view_a.excluded_stores, &EXCLUDED_STORES);
    assert!(!view_a.observes_durable_work());

    let b2 = fx.chain[2].block_hash.clone();
    let g = fx.chain[0].block_hash.clone();
    view_a
        .representation
        .floor_index
        .put_one(BlockHashSerde(b2.clone()), BlockHashSerde(g.clone()))
        .unwrap();
    assert_eq!(
        view_a
            .representation
            .floor_index
            .get_one(&BlockHashSerde(b2.clone()))
            .unwrap(),
        Some(BlockHashSerde(g.clone()))
    );
    assert_eq!(
        view_b
            .representation
            .floor_index
            .get_one(&BlockHashSerde(b2.clone()))
            .unwrap(),
        None
    );
    assert_eq!(
        floor_store(fx.handle("floor-index"))
            .get_one(&BlockHashSerde(b2.clone()))
            .unwrap(),
        None
    );

    {
        let mut index = view_a.representation.block_metadata_index.write();
        let mut extra =
            BlockMetadata::from_block(&child(7, vec![g.clone()], None), false, None, None);
        extra.finalized = true;
        index.add(extra.clone()).unwrap();
        assert!(index.contains(&extra.block_hash));
        assert!(!view_b
            .representation
            .block_metadata_index
            .read()
            .contains(&extra.block_hash));
        assert!(!fx
            .dag
            .get_representation()
            .unwrap()
            .contains(&extra.block_hash));
    }

    assert_eq!(view_b.representation.dag_set.len(), 4);
    assert_eq!(view_b.representation.last_finalized_block_hash, g);
    assert!(view_b
        .representation
        .block_metadata_index
        .read()
        .contains(&b2));
    assert_eq!(
        view_b.representation.latest_messages_map.len(),
        snapshot.latest_messages.len()
    );
    assert!(view_b
        .representation
        .lifecycle
        .read()
        .canonical_appearance(&[1u8; 32], &|_| true)
        .unwrap()
        .is_none());
}

#[test]
fn production_store_bytes_are_unchanged_by_successful_and_rejected_captures() {
    let fx = fixture("bytes");
    let before = fx.store_bytes();
    capture(
        &fx.dag,
        &fx.blocks,
        &request(&[fx.chain[2].block_hash.clone()]),
    )
    .unwrap();
    assert_eq!(fx.store_bytes(), before);

    let mut tight = limits();
    tight.max_blocks = 1;
    let rejected = capture(&fx.dag, &fx.blocks, &CaptureRequest {
        limits: tight,
        bodies: &[],
    });
    assert_eq!(
        rejected.err(),
        Some(SnapshotError::LimitExceeded {
            kind: "held blocks",
            limit: 1,
            observed: 4,
        })
    );
    assert_eq!(fx.store_bytes(), before);

    let mut tight_edges = limits();
    tight_edges.max_edges = 2;
    let rejected_edges = capture(&fx.dag, &fx.blocks, &CaptureRequest {
        limits: tight_edges,
        bodies: &[],
    });
    assert!(matches!(
        rejected_edges,
        Err(SnapshotError::LimitExceeded { .. })
    ));
    assert_eq!(fx.store_bytes(), before);

    let mut tight_work = limits();
    tight_work.max_work = 2;
    let rejected_work = capture(&fx.dag, &fx.blocks, &CaptureRequest {
        limits: tight_work,
        bodies: &[],
    });
    assert_eq!(
        rejected_work.err(),
        Some(SnapshotError::LimitExceeded {
            kind: "capture work",
            limit: 2,
            observed: 3,
        })
    );
    assert_eq!(fx.store_bytes(), before);
}

#[test]
fn held_block_without_metadata_row_is_incomplete_evidence() {
    let fx = fixture("incomplete");
    let b1 = fx.chain[1].block_hash.clone();
    metadata_store(fx.handle("block-metadata"))
        .delete(vec![BlockHashSerde(b1.clone())])
        .unwrap();
    assert!(fx.dag.get_representation().unwrap().contains(&b1));

    let result = capture(&fx.dag, &fx.blocks, &request(&[]));
    match result {
        Err(SnapshotError::Incomplete(message)) => {
            assert!(message.contains(&hex::encode(&b1)));
        }
        other => panic!("expected incomplete evidence, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn held_block_without_body_row_is_incomplete_but_unknown_block_is_not_held() {
    let fx = fixture("body");
    let b2 = fx.chain[2].block_hash.clone();
    fx.handle("blocks").delete(vec![b2.to_vec()]).unwrap();

    let result = capture(&fx.dag, &fx.blocks, &request(std::slice::from_ref(&b2)));
    assert!(matches!(result, Err(SnapshotError::Incomplete(_))));

    let unknown = BlockHash::from(vec![0x11u8; 32]);
    let snapshot = capture(
        &fx.dag,
        &fx.blocks,
        &request(std::slice::from_ref(&unknown)),
    )
    .unwrap();
    assert_eq!(snapshot.bodies[&unknown], BlockBody::NotHeld);
}

#[test]
fn bounded_block_decoding_rejects_oversized_lengths_before_allocation() {
    let limits = BlockDecodeLimits {
        max_compressed_bytes: 64,
        max_decompressed_bytes: 1024,
        max_expansion_ratio: 4,
    };

    let mut declared_huge = Vec::new();
    prost::encoding::encode_varint(1 << 40, &mut declared_huge);
    declared_huge.extend_from_slice(&[0u8; 8]);
    assert_eq!(
        KeyValueBlockStore::decode_block_bounded(&declared_huge, &limits).err(),
        Some(SnapshotError::LimitExceeded {
            kind: "decompressed block bytes",
            limit: 1024,
            observed: 1 << 40,
        })
    );

    let mut declared_expanded = Vec::new();
    prost::encoding::encode_varint(1000, &mut declared_expanded);
    declared_expanded.extend_from_slice(&[0u8; 8]);
    assert_eq!(
        KeyValueBlockStore::decode_block_bounded(&declared_expanded, &limits).err(),
        Some(SnapshotError::LimitExceeded {
            kind: "decode expansion bytes",
            limit: 32,
            observed: 1000,
        })
    );

    let too_large = vec![0u8; 65];
    assert_eq!(
        KeyValueBlockStore::decode_block_bounded(&too_large, &limits).err(),
        Some(SnapshotError::LimitExceeded {
            kind: "compressed block bytes",
            limit: 64,
            observed: 65,
        })
    );

    let mut garbage = Vec::new();
    prost::encoding::encode_varint(16, &mut garbage);
    garbage.extend_from_slice(&[0xFFu8; 8]);
    assert!(matches!(
        KeyValueBlockStore::decode_block_bounded(&garbage, &limits),
        Err(SnapshotError::Malformed(_))
    ));

    assert!(matches!(
        KeyValueBlockStore::decode_block_bounded(&[], &limits),
        Err(SnapshotError::Malformed(_))
    ));

    let zero = BlockDecodeLimits {
        max_compressed_bytes: 0,
        max_decompressed_bytes: 1,
        max_expansion_ratio: 1,
    };
    assert!(matches!(
        KeyValueBlockStore::decode_block_bounded(&[0u8], &zero),
        Err(SnapshotError::InvalidLimits(_))
    ));
}

#[test]
fn stored_block_round_trips_through_bounded_decoding() {
    let fx = fixture("roundtrip");
    let b1 = &fx.chain[1];
    let raw = fx
        .handle("blocks")
        .get_one(&b1.block_hash.to_vec())
        .unwrap()
        .unwrap();
    let decoded = KeyValueBlockStore::decode_block_bounded(&raw, &limits().block_decode).unwrap();
    assert_eq!(&decoded, b1);
}
