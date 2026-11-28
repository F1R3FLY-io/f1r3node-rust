use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash_from_vec;
use rspace_plus_plus::rspace::history::cold_gc::ColdGc;
use rspace_plus_plus::rspace::history::cold_gc_lmdb::LmdbColdGc;
use rspace_plus_plus::rspace::history::cold_store::PersistedData;
use rspace_plus_plus::rspace::history::cold_store_codec::ColdStoreCodec;
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::hot_store_action::{
    HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporterInstance;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;
use shared::rust::store::memory_key_value_store::MemoryEnvironment;

type Repository = Box<dyn HistoryRepository<String, String, String, String> + Send + Sync>;

static CODEC_BUDGET_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn memory_stores() -> RSpaceStore {
    let env = Arc::new(MemoryEnvironment::new());
    RSpaceStore {
        history: Arc::new(env.create_store()),
        roots: Arc::new(env.create_store()),
        cold: Arc::new(env.create_store()),
    }
}

fn lmdb_stores(path: &Path) -> RSpaceStore {
    let env = Arc::new(unsafe {
        heed::EnvOpenOptions::new()
            .map_size(64 * 1024 * 1024)
            .max_dbs(3)
            .open(path)
            .unwrap()
    });
    let mut txn = env.write_txn().unwrap();
    let history = env.create_database(&mut txn, Some("history")).unwrap();
    let roots = env.create_database(&mut txn, Some("roots")).unwrap();
    let cold = env.create_database(&mut txn, Some("cold")).unwrap();
    txn.commit().unwrap();
    RSpaceStore {
        history: Arc::new(LmdbKeyValueStore::new(env.clone(), history)),
        roots: Arc::new(LmdbKeyValueStore::new(env.clone(), roots)),
        cold: Arc::new(LmdbKeyValueStore::new(env, cold)),
    }
}

fn repository(stores: RSpaceStore, enabled: bool) -> Repository {
    let stores = stores.with_cold_compression(enabled);
    HistoryRepositoryInstances::lmdb_repository(stores.history, stores.roots, stores.cold).unwrap()
}

fn channels() -> Vec<String> { vec!["left".into(), "right".into()] }

fn actions() -> Vec<HotStoreAction<String, String, String, String>> {
    let channels = channels();
    let patterns = vec!["*".to_string(); 2];
    let continuation = WaitingContinuation::create(
        &channels,
        &patterns,
        &"continuation-body".repeat(1024),
        true,
        BTreeSet::from([0]),
        patterns.iter().map(String::len).collect(),
        None,
    );
    let datum = Datum::create(&"data".to_string(), "value".to_string(), false, "value".len());
    vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: "data".into(),
            data: vec![datum.clone(), datum],
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: channels.clone(),
            continuations: vec![continuation.clone(), continuation],
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channels[0].clone(),
            joins: vec![channels.clone(), channels],
        })),
    ]
}

fn assert_readable(repo: &dyn HistoryRepository<String, String, String, String>) {
    let reader = repo.get_history_reader(&repo.root()).unwrap();
    let base = reader.base();
    let continuations = base.get_continuations(&channels());
    assert_eq!(continuations.len(), 2);
    assert_eq!(continuations[0], continuations[1]);
    assert!(continuations[0].persist);
    assert_eq!(continuations[0].peeks, BTreeSet::from([0]));
    assert_eq!(continuations[0].continuation, "continuation-body".repeat(1024));
    assert_eq!(base.get_data(&"data".to_string()).len(), 2);
    assert_eq!(base.get_joins(&channels()[0]), vec![channels(), channels()]);
}

fn cold_gc() -> Arc<LmdbColdGc> {
    let env = Arc::new(MemoryEnvironment::new());
    Arc::new(LmdbColdGc::new(Arc::new(env.create_store()), Arc::new(env.create_store())))
}

#[test]
fn cold_gc_factory_preserves_compression_and_installs_decoder_after_reopen() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let gc = cold_gc();
    let expected = repository(memory_stores(), false).checkpoint(actions());
    let root = expected.root();
    {
        let physical = lmdb_stores(dir.path());
        let stores = physical.clone().with_cold_compression(true);
        let repo: Repository = HistoryRepositoryInstances::lmdb_repository_with_cold_gc(
            stores.history,
            stores.roots,
            stores.cold,
            gc.clone(),
        )
        .unwrap();
        let next = repo.checkpoint(actions());
        assert_eq!(next.root(), root);
        assert_readable(next.as_ref());
        assert!(
            physical
                .cold
                .to_map()
                .unwrap()
                .values()
                .any(|v| v.starts_with(b"F1CLZ4\0\0"))
        );
        assert_eq!(next.cold_gc_audit().unwrap().divergence_total, 0);
    }
    let stores = lmdb_stores(dir.path());
    let repo: Repository = HistoryRepositoryInstances::lmdb_repository_with_cold_gc(
        stores.history,
        stores.roots,
        stores.cold.clone(),
        gc,
    )
    .unwrap();
    assert_eq!(repo.root(), root);
    assert_readable(repo.as_ref());
    let keys = stores
        .cold
        .to_map()
        .unwrap()
        .keys()
        .map(|key| bincode::deserialize::<Blake2b256Hash>(key).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        repo.exporter().get_data_items(keys.clone()).unwrap(),
        expected.exporter().get_data_items(keys).unwrap()
    );
    let next = repo.checkpoint(actions());
    assert_eq!(next.root(), root);
    assert_readable(next.as_ref());
    assert_eq!(next.cold_gc_audit().unwrap().divergence_total, 0);
}

#[test]
fn cold_gc_sweep_guard_blocks_checkpoint_leaf_publication() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.blocking_lock();
    for enabled in [false, true] {
        let physical = memory_stores();
        let stores = physical.clone().with_cold_compression(enabled);
        let gc = cold_gc();
        let guard = gc.prune_guard().unwrap();
        let repo: Repository = HistoryRepositoryInstances::lmdb_repository_with_cold_gc(
            stores.history,
            stores.roots,
            stores.cold,
            gc,
        )
        .unwrap();
        let sweep = guard.enter_sweep();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let checkpoint = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx.send(repo.checkpoint(actions())).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let blocked = matches!(
            done_rx.recv_timeout(Duration::from_millis(250)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        let unpublished = physical.cold.to_map().unwrap().is_empty();
        drop(sweep);
        checkpoint.join().unwrap();
        assert!(blocked, "checkpoint must wait for the sweep guard");
        assert!(unpublished, "cold writes must wait for the sweep guard");
        let next = done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_readable(next.as_ref());
        assert_eq!(next.cold_gc_audit().unwrap().divergence_total, 0);
    }
}

#[test]
fn raw_and_compressed_checkpoints_have_identical_roots_and_canonical_exports() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.blocking_lock();
    let raw_stores = memory_stores();
    let compressed_stores = memory_stores();
    let raw = repository(raw_stores.clone(), false).checkpoint(actions());
    let compressed = repository(compressed_stores.clone(), true).checkpoint(actions());
    assert_eq!(raw.root(), compressed.root());
    assert_readable(raw.as_ref());
    assert_readable(compressed.as_ref());
    assert_eq!(raw_stores.history.to_map().unwrap(), compressed_stores.history.to_map().unwrap());
    assert!(compressed_stores.cold.size_bytes() < raw_stores.cold.size_bytes());
    let keys: Vec<_> = raw_stores
        .cold
        .to_map()
        .unwrap()
        .keys()
        .map(|key| bincode::deserialize::<Blake2b256Hash>(key).unwrap())
        .collect();
    assert_eq!(
        raw.exporter().get_data_items(keys.clone()).unwrap(),
        compressed.exporter().get_data_items(keys).unwrap()
    );
    let empty = repository(memory_stores(), true);
    assert_eq!(empty.root(), empty.checkpoint(vec![]).root());
}

#[test]
fn canonical_state_transfers_in_both_compression_directions() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.blocking_lock();
    for (source_enabled, destination_enabled) in [(false, true), (true, false), (true, true)] {
        let source = repository(memory_stores(), source_enabled).checkpoint(actions());
        let destination_stores = memory_stores();
        let destination = repository(destination_stores.clone(), destination_enabled);
        let start_path = vec![(source.root(), None)];
        let exported = RSpaceExporterItems::get_history_and_data(
            source.exporter(),
            start_path.clone(),
            0,
            1000,
        )
        .unwrap();
        let history_items = exported.0.items;
        let data_items = exported.1.items;
        RSpaceImporterInstance::validate_state_items(
            &history_items,
            &data_items,
            start_path,
            &exported.0.last_path,
            1000,
            0,
            destination.importer(),
        )
        .unwrap();
        for (_, value) in &data_items {
            bincode::deserialize::<PersistedData>(value).unwrap();
        }
        let importer = destination.importer();
        importer.set_history_items(history_items);
        importer.set_data_items(data_items.clone());
        importer.set_root(&source.root());
        let imported = destination.reset(&source.root()).unwrap();
        assert_readable(imported.as_ref());
        assert_eq!(imported.root(), source.root());
        assert_eq!(
            imported
                .exporter()
                .get_data_items(data_items.iter().map(|(hash, _)| hash.clone()).collect())
                .unwrap(),
            data_items
        );
        assert_eq!(
            importer.has_data_items(
                &data_items
                    .iter()
                    .map(|(hash, _)| hash.clone())
                    .collect::<Vec<_>>()
            ),
            vec![true; data_items.len()]
        );
        assert!(
            destination_stores
                .cold
                .to_map()
                .unwrap()
                .keys()
                .all(|key| key.len() == 32)
        );
        let next_source = source.checkpoint(actions());
        let next_destination = imported.checkpoint(actions());
        assert_eq!(next_source.root(), next_destination.root());
    }
}

#[test]
fn corrupt_raw_priority_key_never_uses_valid_serialized_fallback() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.blocking_lock();
    let stores = memory_stores();
    let repo = repository(stores.clone(), true).checkpoint(actions());
    let logical = ColdStoreCodec::new(stores.cold.clone(), false);
    let (key, canonical) = logical
        .to_map()
        .unwrap()
        .into_iter()
        .find(|(_, value)| {
            matches!(
                bincode::deserialize::<PersistedData>(value).unwrap(),
                PersistedData::Continuations(_)
            )
        })
        .unwrap();
    let leaf_hash: Blake2b256Hash = bincode::deserialize(&key).unwrap();
    stores
        .cold
        .put_one(leaf_hash.bytes(), b"F1CLZ4\0\0broken".to_vec())
        .unwrap();
    assert!(
        repo.exporter()
            .get_data_items(vec![leaf_hash.clone()])
            .is_err()
    );
    let reader = repo.get_history_reader(&repo.root()).unwrap();
    assert!(
        reader
            .get_continuations(&hash_from_vec(&channels()))
            .is_err()
    );
    stores.cold.delete(vec![leaf_hash.bytes()]).unwrap();
    assert_eq!(
        repo.exporter()
            .get_data_items(vec![leaf_hash.clone()])
            .unwrap(),
        vec![(leaf_hash, canonical)]
    );
    assert_readable(repo.as_ref());
}

#[test]
fn lmdb_restart_and_aborted_transaction_preserve_mixed_rows_and_retained_roots() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root;
    let empty_root;
    {
        let stores = lmdb_stores(dir.path());
        let repo = repository(stores.clone(), true);
        empty_root = repo.root();
        let next = repo.checkpoint(actions());
        root = next.root();
        assert_readable(next.as_ref());
        let logical = ColdStoreCodec::new(stores.cold.clone(), false);
        let (key, canonical) = logical.to_map().unwrap().into_iter().next().unwrap();
        let physical = stores.cold.get_one(&key).unwrap();
        let lmdb = stores
            .cold
            .as_any()
            .downcast_ref::<LmdbKeyValueStore>()
            .unwrap();
        {
            let mut txn = lmdb.env.write_txn().unwrap();
            lmdb.db
                .put(&mut txn, &key, &b"interrupted replacement".to_vec())
                .unwrap();
        }
        assert_eq!(stores.cold.get_one(&key).unwrap(), physical);
        assert_eq!(logical.get_one(&key).unwrap(), Some(canonical));
        lmdb.env.force_sync().unwrap();
    }
    {
        let stores = lmdb_stores(dir.path());
        let repo = repository(stores.clone(), false);
        assert_eq!(repo.root(), root);
        assert_readable(repo.as_ref());
        assert_eq!(repo.reset(&empty_root).unwrap().root(), empty_root);
        let next = repo.checkpoint(actions());
        assert_eq!(next.root(), root);
        assert_readable(next.as_ref());
        let logical = ColdStoreCodec::new(stores.cold.clone(), false);
        logical
            .iterate_while(&mut |_, value| {
                assert!(bincode::deserialize::<PersistedData>(&value).is_ok());
                Ok(true)
            })
            .unwrap();
    }
}

#[derive(Clone)]
struct Wildcard;

impl Match<String, String, String> for Wildcard {
    fn get(&self, _: &String, datum: &String) -> Option<String> { Some(datum.clone()) }

    fn pattern_size(&self, pattern: &String) -> usize { pattern.len() }

    fn data_size(&self, datum: &String) -> usize { datum.len() }
}

#[tokio::test]
async fn persisted_multi_channel_peek_replays_to_the_same_root() {
    let _budget_guard = CODEC_BUDGET_TEST_LOCK.lock().await;
    let mut roots = Vec::new();
    for enabled in [false, true] {
        let stores = memory_stores().with_cold_compression(enabled);
        let (space, replay) = RSpace::<String, String, String, String>::create_with_replay(
            stores,
            Arc::new(Box::new(Wildcard)),
        )
        .unwrap();
        space
            .consume(
                channels(),
                vec!["*".into(); 2],
                "continuation-body".repeat(1024),
                true,
                BTreeSet::from([0]),
            )
            .await
            .unwrap();
        let initial = space.create_checkpoint().await.unwrap();
        assert!(
            space
                .produce("left".into(), "first".into(), false)
                .await
                .unwrap()
                .is_none()
        );
        let result = space
            .produce("right".into(), "second".into(), false)
            .await
            .unwrap()
            .unwrap();
        let checkpoint = space.create_checkpoint().await.unwrap();
        replay
            .rig_and_reset(initial.root, checkpoint.log)
            .await
            .unwrap();
        assert!(
            replay
                .produce("left".into(), "first".into(), false)
                .await
                .unwrap()
                .is_none()
        );
        let replay_result = replay
            .produce("right".into(), "second".into(), false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.0, replay_result.0);
        assert_eq!(result.1, replay_result.1);
        assert_eq!(replay.create_checkpoint().await.unwrap().root, checkpoint.root);
        roots.push(checkpoint.root);
    }
    assert_eq!(roots[0], roots[1]);
}
