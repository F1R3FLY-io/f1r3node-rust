use std::sync::Arc;

use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use rspace_plus_plus::rspace::hot_store_action::{HotStoreAction, InsertAction, InsertData};
use rspace_plus_plus::rspace::shared::rspace_store_manager::get_or_create_rspace_store;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

use crate::history::history_repository_tests::datum;

type Repo = Box<dyn HistoryRepository<String, String, String, String> + Send + Sync>;

fn batch(seed: u64, size: u64) -> Vec<HotStoreAction<String, String, String, String>> {
    (0..size)
        .map(|index| {
            HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                channel: format!("channel-{seed}-{index}"),
                data: vec![datum(index as i32)],
            }))
        })
        .collect()
}

fn roots_store_counters(f: impl FnOnce()) -> (u64, u64) {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, f);
    let mut reads = 0;
    let mut writes = 0;
    for (key, _, _, value) in snapshotter.snapshot().into_vec() {
        if let DebugValue::Counter(value) = value {
            match key.key().name() {
                "history.roots_store.reads" => reads += value,
                "history.roots_store.writes" => writes += value,
                _ => {}
            }
        }
    }
    (reads, writes)
}

fn environment_txn_id(store: &Arc<dyn KeyValueStore>) -> usize {
    store
        .as_any()
        .downcast_ref::<LmdbKeyValueStore>()
        .expect("the store is an LMDB store")
        .env
        .info()
        .last_txn_id
}

#[test]
fn checkpoint_makes_the_new_state_durable_in_one_history_environment_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 28).unwrap();
    let history = store.history.clone();
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let before = environment_txn_id(&history);

    let next = repo.checkpoint(batch(1, 4));

    assert_eq!(environment_txn_id(&history) - before, 1);
    assert_ne!(next.root(), repo.root());
}

#[test]
fn repository_reopened_after_a_checkpoint_starts_at_the_new_root_and_reads_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 28).unwrap();
    let (history, roots, cold) = (store.history.clone(), store.roots.clone(), store.cold.clone());
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let new_root = repo.checkpoint(batch(1, 4)).root();
    drop(repo);

    let reopened: Repo = HistoryRepositoryInstances::lmdb_repository(history, roots, cold).unwrap();
    let data = reopened
        .get_history_reader(&new_root)
        .unwrap()
        .base()
        .get_data(&"channel-1-2".to_string());

    assert_eq!(reopened.root(), new_root);
    assert_eq!(data.iter().map(|datum| datum.a.clone()).collect::<Vec<_>>(), vec![
        "data-2".to_string()
    ]);
}

#[test]
fn checkpoint_larger_than_the_read_cache_completes_and_reads_back_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 30).unwrap();
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();

    let next = repo.checkpoint(batch(7, 20_000));
    let reader = next.get_history_reader(&next.root()).unwrap();

    for index in [0, 9_999, 19_999] {
        let data = reader.base().get_data(&format!("channel-7-{index}"));
        assert_eq!(data.iter().map(|datum| datum.a.clone()).collect::<Vec<_>>(), vec![format!(
            "data-{index}"
        )]);
    }
}

#[test]
fn lmdb_roots_store_reset_reads_once_and_never_writes() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 28).unwrap();
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let first = repo.checkpoint(batch(1, 4));
    let second = first.checkpoint(batch(2, 4));
    let roots = [repo.root(), first.root(), second.root()];

    let (reads, writes) = roots_store_counters(|| {
        for root in &roots {
            repo.reset(root).unwrap();
        }
    });

    assert_eq!((reads, writes), (3, 0));
    assert!(repo.reset(&Blake2b256Hash::new(b"absent root")).is_err());
}

#[test]
fn lmdb_roots_store_record_root_commits_both_keys_in_one_write() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 28).unwrap();
    let roots_kv = store.roots.clone();
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let root = Blake2b256Hash::new(b"recorded root");

    let before = environment_txn_id(&roots_kv);

    repo.record_root(&root).unwrap();

    assert_eq!(environment_txn_id(&roots_kv) - before, 1);
    assert!(repo.contains_root(&root).unwrap());
    assert_eq!(
        RootsStoreInstances::roots_store(roots_kv)
            .current_root()
            .unwrap(),
        Some(root)
    );
}

#[test]
fn lmdb_repository_reopens_at_the_last_committed_root_after_resets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let store = get_or_create_rspace_store(path.to_str().unwrap(), 1 << 28).unwrap();
    let (history, roots, cold) = (store.history.clone(), store.roots.clone(), store.cold.clone());
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let first = repo.checkpoint(batch(1, 4));
    let second = first.checkpoint(batch(2, 4));
    second.reset(&first.root()).unwrap();

    let reopened: Repo = HistoryRepositoryInstances::lmdb_repository(history, roots, cold).unwrap();

    assert_eq!(reopened.root(), second.root());
}
