// See rspace/src/test/scala/coop/rchain/rspace/history/HistoryRepositorySpec.
// scala

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use rand::prelude::SliceRandom;
use rspace_plus_plus::rspace::errors::{HistoryError, RadixTreeError, RootError};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::history::checkpoint_writer::{CheckpointWriter, KvCheckpointWriter};
use rspace_plus_plus::rspace::history::history::HistoryInstances;
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::history::history_repository_impl::HistoryRepositoryImpl;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::prune_guard::PruneGuard;
use rspace_plus_plus::rspace::history::root_repository::RootRepository;
use rspace_plus_plus::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use rspace_plus_plus::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, DeleteData, DeleteJoins, HotStoreAction, InsertAction,
    InsertContinuations, InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieDeleteAction, TrieDeleteConsume, TrieDeleteJoins, TrieDeleteProduce,
    TrieInsertAction, TrieInsertBinaryConsume, TrieInsertBinaryJoins, TrieInsertBinaryProduce,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::trie_exporter::{
    KeyHash, NodePath, TrieExporter, TrieNode, Value,
};
use rspace_plus_plus::rspace::shared::trie_importer::TrieImporter;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use shared::rust::ByteVector;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

use crate::history::history_action_tests::{random_blake, zeros_blake};

#[tokio::test]
async fn history_repository_should_process_insert_one_datum() {
    let repo = create_empty_repository();
    let test_datum = datum(1);
    let insert_data = InsertData {
        channel: test_channel_data_prefix(),
        data: vec![test_datum.clone()],
    };

    let next_repo =
        repo.checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(insert_data))]);
    let history_reader = next_repo.get_history_reader(&next_repo.root());
    let data = history_reader
        .unwrap()
        .base()
        .get_data(&test_channel_data_prefix());
    let fetched = data.first().unwrap().clone();

    assert_eq!(fetched, test_datum);
}

#[tokio::test]
async fn history_repository_should_allow_insert_of_joins_datum_continuation_on_same_channel() {
    let repo = create_empty_repository();
    let channel = test_channel_continuations_prefix();

    let test_datum = datum(1);
    let data = InsertData {
        channel: channel.clone(),
        data: vec![test_datum.clone()],
    };

    let test_joins = join(1);
    let joins = InsertJoins {
        channel: channel.clone(),
        joins: test_joins,
    };

    let test_continuation = continuation(1);
    let continuations = InsertContinuations {
        channels: vec![channel.clone()],
        continuations: vec![test_continuation.clone()],
    };

    let next_repo = repo.checkpoint(vec![
        HotStoreAction::Insert(InsertAction::InsertData(data)),
        HotStoreAction::Insert(InsertAction::InsertJoins(joins.clone())),
        HotStoreAction::Insert(InsertAction::InsertContinuations(continuations)),
    ]);
    let history_reader = next_repo.get_history_reader(&next_repo.root());
    let reader = history_reader.as_ref().unwrap().base();

    let fetched_data = reader.get_data(&channel);
    let fetched_continuation = reader.get_continuations(&vec![channel.clone()]);
    let fetched_joins = reader.get_joins(&channel);

    assert_eq!(fetched_data.len(), 1);
    assert_eq!(fetched_data.first().unwrap().clone(), test_datum);

    assert_eq!(fetched_continuation.len(), 1);
    assert_eq!(fetched_continuation.first().unwrap().clone(), test_continuation);

    assert_eq!(fetched_joins.len(), 2);
    assert_eq!(
        HashSet::<String>::from_iter(fetched_joins.into_iter().flatten()),
        HashSet::<String>::from_iter(joins.joins.into_iter().flatten())
    );
}

#[tokio::test]
async fn history_repository_should_process_insert_and_delete_of_thirty_mixed_elements() {
    let repo = create_empty_repository();

    let data: (Vec<_>, Vec<_>) = (0..=10).map(insert_datum).unzip();
    let joins: (Vec<_>, Vec<_>) = (0..=10).map(insert_join).unzip();
    let conts: (Vec<_>, Vec<_>) = (0..=10).map(insert_continuation).unzip();

    let mut elems: Vec<_> = [&data.0[..], &joins.0[..], &conts.0[..]].concat();
    let mut rng = rand::rng();
    elems.shuffle(&mut rng);

    let data_delete: Vec<_> = data
        .clone()
        .1
        .into_iter()
        .map(|d| {
            HotStoreAction::<String, String, String, String>::Delete(DeleteAction::DeleteData(
                DeleteData { channel: d.channel },
            ))
        })
        .collect();

    let joins_delete: Vec<_> = joins
        .clone()
        .1
        .into_iter()
        .map(|j| {
            HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel: j.channel }))
        })
        .collect();

    let conts_delete: Vec<_> = conts
        .clone()
        .1
        .into_iter()
        .map(|c| {
            HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
                channels: c.channels,
            }))
        })
        .collect();

    let delete_elems: Vec<_> = [&data_delete[..], &joins_delete[..], &conts_delete[..]].concat();

    let next_repo = repo.checkpoint(elems);
    let history_reader = next_repo.get_history_reader(&next_repo.root()).unwrap();
    let next_reader = history_reader.base();

    let fetched_data: Vec<Vec<Datum<String>>> = data
        .1
        .iter()
        .map(|d| next_reader.get_data(&d.channel))
        .collect();
    assert_eq!(
        fetched_data,
        data.1
            .clone()
            .into_iter()
            .map(|d| d.data)
            .collect::<Vec<_>>()
    );

    let fetched_conts: Vec<Vec<WaitingContinuation<String, String>>> = conts
        .1
        .iter()
        .map(|c| next_reader.get_continuations(&c.channels))
        .collect();
    assert_eq!(
        fetched_conts,
        conts
            .1
            .clone()
            .into_iter()
            .map(|c| c.continuations)
            .collect::<Vec<_>>()
    );

    let fetched_joins: Vec<Vec<Vec<String>>> = joins
        .1
        .iter()
        .map(|j| next_reader.get_joins(&j.channel))
        .collect();
    let all_joins = HashSet::<String>::from_iter(fetched_joins.into_iter().flatten().flatten());
    let expected_joins: HashSet<String> = joins
        .clone()
        .1
        .into_iter()
        .flat_map(|j: InsertJoins<String>| j.joins.into_iter())
        .flatten()
        .collect();
    assert_eq!(all_joins, expected_joins);

    let deleted_repo = next_repo.checkpoint(delete_elems);
    let history_reader = deleted_repo
        .get_history_reader(&deleted_repo.root())
        .unwrap();
    let deleted_reader = history_reader.base();

    let fetched_data: Vec<Vec<Datum<String>>> = data
        .1
        .iter()
        .map(|d| next_reader.get_data(&d.channel))
        .collect();
    assert_eq!(
        fetched_data,
        data.1
            .clone()
            .into_iter()
            .map(|d| d.data)
            .collect::<Vec<_>>()
    );

    let fetched_conts: Vec<Vec<WaitingContinuation<String, String>>> = conts
        .1
        .iter()
        .map(|c| next_reader.get_continuations(&c.channels))
        .collect();
    assert_eq!(
        fetched_conts,
        conts
            .1
            .clone()
            .into_iter()
            .map(|c| c.continuations)
            .collect::<Vec<_>>()
    );

    let fetched_joins: Vec<Vec<Vec<String>>> = joins
        .1
        .iter()
        .map(|j| next_reader.get_joins(&j.channel))
        .collect();
    let all_joins = HashSet::<String>::from_iter(fetched_joins.into_iter().flatten().flatten());
    assert_eq!(all_joins, expected_joins);

    let fetched_data: Vec<Vec<Datum<String>>> = data
        .1
        .iter()
        .map(|d| deleted_reader.get_data(&d.channel))
        .collect();
    assert!(fetched_data.iter().flatten().collect::<Vec<_>>().is_empty());

    let fetched_conts: Vec<Vec<WaitingContinuation<String, String>>> = conts
        .1
        .iter()
        .map(|c| deleted_reader.get_continuations(&c.channels))
        .collect();
    assert!(
        fetched_conts
            .iter()
            .flatten()
            .collect::<Vec<_>>()
            .is_empty()
    );

    let fetched_joins: Vec<Vec<Vec<String>>> = joins
        .1
        .iter()
        .map(|j| deleted_reader.get_joins(&j.channel))
        .collect();
    assert!(
        fetched_joins
            .iter()
            .flatten()
            .collect::<Vec<_>>()
            .is_empty()
    );
}

#[tokio::test]
#[allow(clippy::assertions_on_constants)]
async fn history_repository_should_not_allow_switching_to_a_not_existing_root() {
    let repo = create_empty_repository();

    // The absent root keeps its NAME: `RootNotFound` carries the hash so a
    // caller can fetch it instead of parsing prose. Downstream, this is what
    // lets a replay classify the absence as an availability event rather
    // than a verdict.
    match repo.reset(&zeros_blake()) {
        Err(HistoryError::RootError(RootError::RootNotFound(root))) => {
            assert_eq!(root, zeros_blake(), "the error must name the exact root that was not found")
        }
        Ok(_) => assert!(false, "Expected a failure"),
        Err(other) => assert!(false, "Wrong error thrown: {:?}", other),
    }
}

#[tokio::test]
async fn history_repository_should_record_next_root_as_valid() {
    let repo = create_empty_repository();
    let test_datum = datum(1);
    let insert_data = InsertData {
        channel: test_channel_data_prefix(),
        data: vec![test_datum.clone()],
    };

    let next_repo =
        repo.checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(insert_data))]);
    let _ = repo.reset(&RadixHistory::empty_root_node_hash());
    let binding = next_repo.history();
    let next_repo_history = binding.lock().expect("Failed to acquire history lock");
    let _ = repo.reset(&next_repo_history.root());
}

#[test]
fn checkpoint_attribution_preserves_roots_and_records_only_executed_stages() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    for count in [0, 1, 256] {
        let actions: Vec<HotStoreTrieAction<String, String, String, String>> = (0..count)
            .map(|index| {
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                    TrieInsertBinaryProduce {
                        hash: hash(&index),
                        data: vec![vec![index as u8; 8]],
                    },
                ))
            })
            .collect();
        let expected = create_empty_repository()
            .do_checkpoint(actions.clone())
            .root();
        let repo = create_empty_repository();
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let actual = metrics::with_local_recorder(&recorder, || repo.do_checkpoint(actions));
        assert_eq!(actual.root(), expected);
        let snapshot = snapshotter.snapshot().into_vec();
        let samples = |name: &str| -> Vec<f64> {
            snapshot
                .iter()
                .filter(|(key, _, _, _)| key.key().name() == name)
                .flat_map(|(_, _, _, value)| match value {
                    DebugValue::Histogram(values) => {
                        values.iter().map(|value| value.into_inner()).collect()
                    }
                    _ => Vec::new(),
                })
                .collect()
        };
        assert_eq!(samples("history.checkpoint.actions"), vec![count as f64]);
        assert_eq!(samples("history.checkpoint.time").len(), 1);
        for stage in [
            "storage-actions",
            "partition",
            "serialize",
            "leaf-write",
            "history-lock-wait",
            "history-process",
            "root-commit",
        ] {
            let values = samples(&format!("history.checkpoint.{stage}.time"));
            assert_eq!(values.len(), usize::from(count > 0), "{stage}, actions={count}");
            assert!(
                values
                    .iter()
                    .all(|value| value.is_finite() && *value >= 0.0)
            );
        }
        let bytes = samples("history.checkpoint.serialized-bytes");
        assert_eq!(bytes.len(), usize::from(count > 0));
        if count > 0 {
            assert!(bytes[0] > 0.0);
        }
    }
}

#[tokio::test]
async fn checkpoint_with_no_actions_returns_repository_at_same_root() {
    let repo = create_empty_repository();
    let root_before = repo.root();
    let next = repo.checkpoint(vec![]);
    assert_eq!(next.root(), root_before);

    let next_after_empty_trie_actions = repo.do_checkpoint(vec![]);
    assert_eq!(next_after_empty_trie_actions.root(), root_before);
}

#[tokio::test]
async fn do_checkpoint_binary_trie_actions_roundtrip_and_delete() {
    let repo = create_empty_repository();
    let data_key = random_blake();
    let cont_key = random_blake();
    let joins_key = random_blake();
    let data_values = vec![vec![1u8; 8], vec![2u8; 8]];
    let cont_values = vec![vec![3u8; 8]];
    let join_values = vec![vec![4u8; 8]];

    let next = repo.do_checkpoint(vec![
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
            TrieInsertBinaryProduce {
                hash: data_key.clone(),
                data: data_values.clone(),
            },
        )),
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
            TrieInsertBinaryConsume {
                hash: cont_key.clone(),
                continuations: cont_values.clone(),
            },
        )),
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
            TrieInsertBinaryJoins {
                hash: joins_key.clone(),
                joins: join_values.clone(),
            },
        )),
    ]);

    let reader = next.get_history_reader(&next.root()).unwrap();
    assert_eq!(reader.get_data_proj_binary(&data_key).unwrap(), data_values);
    assert_eq!(reader.get_continuations_proj_binary(&cont_key).unwrap(), cont_values);
    assert_eq!(reader.get_joins_proj_binary(&joins_key).unwrap(), join_values);
    assert_eq!(reader.root(), next.root());

    let deleted = next.do_checkpoint(vec![
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(
            TrieDeleteProduce {
                hash: data_key.clone(),
            },
        )),
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteConsume(
            TrieDeleteConsume {
                hash: cont_key.clone(),
            },
        )),
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(TrieDeleteJoins {
            hash: joins_key.clone(),
        })),
    ]);

    let deleted_reader = deleted.get_history_reader(&deleted.root()).unwrap();
    assert!(
        deleted_reader
            .get_data_proj_binary(&data_key)
            .unwrap()
            .is_empty()
    );
    assert!(
        deleted_reader
            .get_continuations_proj_binary(&cont_key)
            .unwrap()
            .is_empty()
    );
    assert!(
        deleted_reader
            .get_joins_proj_binary(&joins_key)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn record_root_makes_root_visible_to_contains_root() {
    let repo = create_empty_repository();
    let new_root = random_blake();
    assert!(!repo.contains_root(&new_root).unwrap());

    repo.record_root(&new_root).unwrap();
    assert!(repo.contains_root(&new_root).unwrap());
    assert!(
        repo.contains_root(&RadixHistory::empty_root_node_hash())
            .unwrap()
    );
}

#[test]
fn reset_does_not_move_the_current_root_pointer() {
    let repo = create_empty_repository();
    let (first, _) = insert_datum(1);
    let (second, _) = insert_datum(2);
    let first_root = repo.checkpoint(vec![first]).root();
    let second_root = repo.checkpoint(vec![second]).root();
    let current = || {
        repo.roots_repository
            .lock()
            .unwrap()
            .roots_store
            .current_root()
            .unwrap()
    };
    assert_eq!(current(), Some(second_root.clone()));

    let next = repo.reset(&first_root).unwrap();

    assert_eq!(next.root(), first_root);
    assert_eq!(current(), Some(second_root));
}

struct FailingCheckpointWriter;

impl CheckpointWriter for FailingCheckpointWriter {
    fn write(
        &self,
        _nodes: Vec<(ByteVector, ByteVector)>,
        _root: &Blake2b256Hash,
    ) -> Result<(), KvStoreError> {
        Err(KvStoreError::IoError("injected checkpoint write failure".to_string()))
    }
}

#[test]
fn failed_checkpoint_write_leaves_the_store_at_the_previous_root() {
    let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let repo = empty_repository_over(
        history_store.clone(),
        roots_store.clone(),
        Arc::new(FailingCheckpointWriter),
    );
    let previous_root = repo.root();
    let history_before = history_store.to_map().unwrap();
    let (insert, _) = insert_datum(1);

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        repo.checkpoint(vec![insert]).root()
    }));

    assert!(outcome.is_err());
    assert_eq!(
        RootsStoreInstances::roots_store(roots_store)
            .current_root()
            .unwrap(),
        Some(previous_root)
    );
    assert_eq!(history_store.to_map().unwrap(), history_before);
}

fn repository_over_in_memory_stores()
-> (HistoryRepositoryImpl<String, String, String, String>, Arc<dyn KeyValueStore>) {
    let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let writer = Arc::new(KvCheckpointWriter {
        history: history_store.clone(),
        roots: roots_store.clone(),
    });
    (empty_repository_over(history_store.clone(), roots_store, writer), history_store)
}

#[test]
fn checkpoint_rejects_a_stored_node_with_a_different_value_and_accepts_an_equal_one() {
    let (reference, reference_store) = repository_over_in_memory_stores();
    let (insert, _) = insert_datum(1);
    let new_root = reference.checkpoint(vec![insert.clone()]).root();
    let root_key = new_root.bytes().to_vec();
    let root_node = reference_store.get_one(&root_key).unwrap().unwrap();

    let (equal, equal_store) = repository_over_in_memory_stores();
    equal_store
        .put(vec![(root_key.clone(), root_node.clone())])
        .unwrap();
    assert_eq!(equal.checkpoint(vec![insert.clone()]).root(), new_root);

    let (conflicting, conflicting_store) = repository_over_in_memory_stores();
    conflicting_store
        .put(vec![(root_key, b"a different node".to_vec())])
        .unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        conflicting.checkpoint(vec![insert]).root()
    }));
    let message = outcome
        .err()
        .and_then(|payload| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(message.contains("CollisionError"), "{message}");
}

struct RecordingCheckpointWriter {
    inner: KvCheckpointWriter,
    roots_repository: Mutex<Option<Arc<Mutex<RootRepository>>>>,
    roots_lock_held_at_write: Mutex<Vec<bool>>,
}

impl CheckpointWriter for RecordingCheckpointWriter {
    fn write(
        &self,
        nodes: Vec<(ByteVector, ByteVector)>,
        root: &Blake2b256Hash,
    ) -> Result<(), KvStoreError> {
        let held = self
            .roots_repository
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|roots| roots.try_lock().is_err());
        self.roots_lock_held_at_write.lock().unwrap().push(held);
        self.inner.write(nodes, root)
    }
}

fn repository_with_recording_writer()
-> (HistoryRepositoryImpl<String, String, String, String>, Arc<RecordingCheckpointWriter>) {
    let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let writer = Arc::new(RecordingCheckpointWriter {
        inner: KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        },
        roots_repository: Mutex::new(None),
        roots_lock_held_at_write: Mutex::new(Vec::new()),
    });
    let repo = empty_repository_over(history_store, roots_store, writer.clone());
    *writer.roots_repository.lock().unwrap() = Some(repo.roots_repository.clone());
    (repo, writer)
}

#[test]
fn checkpoint_writes_its_state_while_the_roots_mutex_is_free() {
    let (repo, writer) = repository_with_recording_writer();
    let (first, _) = insert_datum(1);
    let (second, _) = insert_datum(2);

    repo.checkpoint(vec![first]).checkpoint(vec![second]);

    assert_eq!(*writer.roots_lock_held_at_write.lock().unwrap(), vec![false, false]);
}

#[test]
fn empty_checkpoint_writes_nothing_and_keeps_the_current_root() {
    let (repo, writer) = repository_with_recording_writer();
    let current_root = repo.root();

    let next = repo.checkpoint(Vec::new());

    assert_eq!(next.root(), current_root);
    assert!(writer.roots_lock_held_at_write.lock().unwrap().is_empty());
}

#[test]
fn lock_site_metrics_count_each_call_site_separately() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    let repo = create_empty_repository();
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        let (insert, _) = insert_datum(1);
        let next = repo.checkpoint(vec![insert]);
        let root = next.root();
        repo.record_root(&root).unwrap();
        assert!(repo.contains_root(&root).unwrap());
        repo.reset(&root).unwrap();
        repo.get_history_reader(&root).unwrap();
        repo.get_history_reader_struct(&root).unwrap();
    });
    let snapshot = snapshotter.snapshot().into_vec();
    let counter = |name: &str| -> Option<u64> {
        snapshot
            .iter()
            .find(|(key, _, _, _)| key.key().name() == name)
            .and_then(|(_, _, _, value)| match value {
                DebugValue::Counter(value) => Some(*value),
                _ => None,
            })
    };
    for (lock, site, calls) in [
        ("roots_repository", "record_root", 1),
        ("roots_repository", "contains_root", 1),
        ("roots_repository", "reset", 1),
        ("current_history", "checkpoint", 1),
        ("current_history", "reset", 1),
        ("current_history", "history_reader", 2),
        ("current_history", "root", 1),
    ] {
        let prefix = format!("history.repository.{lock}.{site}");
        assert_eq!(counter(&format!("{prefix}.calls")), Some(calls), "{prefix}");
        assert!(counter(&format!("{prefix}.wait_ns")).is_some(), "{prefix}");
        assert!(counter(&format!("{prefix}.hold_ns")).is_some(), "{prefix}");
    }
    assert_eq!(counter("history.repository.roots_repository.checkpoint.calls"), None);
    assert_eq!(counter("history.repository.roots_repository.lock_calls"), Some(3));
    assert_eq!(counter("history.repository.current_history.lock_calls"), Some(5));
}

#[tokio::test]
async fn history_reader_generic_getters_return_inserted_values() {
    let repo = create_empty_repository();
    let channel = "generic-channel".to_string();
    let test_datum = datum(7);
    let test_continuation = continuation(7);
    let test_joins = vec![vec![channel.clone()]];

    let next = repo.checkpoint(vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: channel.clone(),
            data: vec![test_datum.clone()],
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: vec![channel.clone()],
            continuations: vec![test_continuation.clone()],
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channel.clone(),
            joins: test_joins.clone(),
        })),
    ]);

    let reader = next.get_history_reader_struct(&next.root()).unwrap();

    assert_eq!(reader.get_data_proj_generic(&channel), vec![test_datum.clone()]);
    assert_eq!(reader.get_continuations_proj_generic(&vec![channel.clone()]), vec![
        test_continuation.clone()
    ]);
    assert_eq!(reader.get_joins_proj_generic(&channel), test_joins);

    let data_key = hash(&channel);
    let cont_key = hash_from_vec(&vec![channel.clone()]);
    assert_eq!(reader.get_data_proj(&data_key).unwrap(), vec![test_datum]);
    assert_eq!(reader.get_continuations_proj(&cont_key).unwrap(), vec![test_continuation]);
    assert_eq!(reader.get_joins_proj(&data_key).unwrap(), test_joins);
    assert!(!reader.get_data_proj_binary(&data_key).unwrap().is_empty());
    assert!(
        !reader
            .get_continuations_proj_binary(&cont_key)
            .unwrap()
            .is_empty()
    );
    assert!(!reader.get_joins_proj_binary(&data_key).unwrap().is_empty());
}

#[tokio::test]
async fn checkpoint_handles_large_action_batches() {
    let repo = create_empty_repository();
    let datums: Vec<Datum<String>> = (0..300).map(datum).collect();
    let actions: Vec<HotStoreAction<String, String, String, String>> = datums
        .iter()
        .enumerate()
        .map(|(i, d)| {
            HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                channel: format!("bulk-channel-{}", i),
                data: vec![d.clone()],
            }))
        })
        .collect();

    let next = repo.checkpoint(actions);
    let reader = next.get_history_reader(&next.root()).unwrap().base();

    for i in [0usize, 137, 299] {
        let fetched = reader.get_data(&format!("bulk-channel-{}", i));
        assert_eq!(fetched, vec![datums[i].clone()]);
    }
}

fn test_channel_data_prefix() -> String { "channel-data".to_string() }

fn test_channel_joins_prefix() -> String { "channel-joins".to_string() }

fn test_channel_continuations_prefix() -> String { "channel-continuations".to_string() }

fn insert_datum(
    s: i32,
) -> (HotStoreAction<String, String, String, String>, InsertData<String, String>) {
    let insert = InsertData {
        channel: format!("{}{}", test_channel_data_prefix(), s),
        data: vec![datum(s)],
    };

    (HotStoreAction::Insert(InsertAction::InsertData(insert.clone())), insert)
}

fn insert_join(s: i32) -> (HotStoreAction<String, String, String, String>, InsertJoins<String>) {
    let insert = InsertJoins {
        channel: format!("{}{}", test_channel_joins_prefix(), s),
        joins: join(s),
    };

    (HotStoreAction::Insert(InsertAction::InsertJoins(insert.clone())), insert)
}

#[allow(clippy::type_complexity)]
fn insert_continuation(
    s: i32,
) -> (HotStoreAction<String, String, String, String>, InsertContinuations<String, String, String>) {
    let insert = InsertContinuations {
        channels: vec![format!("{}{}", test_channel_continuations_prefix(), s)],
        continuations: vec![continuation(s)],
    };

    (HotStoreAction::Insert(InsertAction::InsertContinuations(insert.clone())), insert)
}

fn join(s: i32) -> Vec<Vec<String>> {
    vec![vec![format!("abc{}", s), format!("def{}", s)], vec![
        format!("wer{}", s),
        format!("tre{}", s),
    ]]
}

pub fn continuation(s: i32) -> WaitingContinuation<String, String> {
    WaitingContinuation {
        patterns: vec![format!("pattern-{}", s)],
        continuation: format!("cont-{}", s),
        persist: true,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: vec![random_blake()],
            hash: random_blake(),
            persistent: true,
        },
        pattern_sizes: vec![1],
    }
}

pub fn datum(s: i32) -> Datum<String> {
    Datum {
        a: format!("data-{}", s),
        persist: false,
        source: Produce {
            channel_hash: random_blake(),
            hash: random_blake(),
            persistent: false,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        },
        size: format!("data-{}", s).len(),
    }
}

pub fn create_empty_repository() -> HistoryRepositoryImpl<String, String, String, String> {
    let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let writer = Arc::new(KvCheckpointWriter {
        history: history_store.clone(),
        roots: roots_store.clone(),
    });
    empty_repository_over(history_store, roots_store, writer)
}

fn empty_repository_over(
    history_store: Arc<dyn KeyValueStore>,
    roots_store: Arc<dyn KeyValueStore>,
    checkpoint_writer: Arc<dyn CheckpointWriter>,
) -> HistoryRepositoryImpl<String, String, String, String> {
    let past_roots = RootRepository {
        roots_store: Box::new(RootsStoreInstances::roots_store(roots_store.clone())),
    };
    let empty_history =
        HistoryInstances::create(RadixHistory::empty_root_node_hash(), history_store.clone())
            .unwrap();

    let _ = past_roots.commit(&RadixHistory::empty_root_node_hash());

    HistoryRepositoryImpl {
        current_history: Arc::new(Mutex::new(Box::new(empty_history))),
        roots_repository: Arc::new(Mutex::new(past_roots)),
        checkpoint_writer,
        leaf_store: create_inmem_cold_store(),
        history_kv_store: history_store,
        rspace_exporter: Arc::new(EmptyExporter),
        rspace_importer: Arc::new(EmptyImporter),
        prune_guard: Arc::new(PruneGuard::new()),
        _marker: std::marker::PhantomData,
    }
}

fn create_inmem_cold_store() -> Arc<dyn KeyValueStore> { Arc::new(InMemoryKeyValueStore::new()) }

struct EmptyExporter;

impl RSpaceExporter for EmptyExporter {
    fn get_root(&self) -> Result<KeyHash, RootError> { todo!() }
}

impl TrieExporter for EmptyExporter {
    fn get_nodes(
        &self,
        _start_path: NodePath,
        _skip: i32,
        _take: i32,
    ) -> Result<Vec<TrieNode<KeyHash>>, RadixTreeError> {
        todo!()
    }

    fn get_history_items(
        &self,
        _keys: Vec<KeyHash>,
    ) -> Result<Vec<(KeyHash, Value)>, KvStoreError> {
        todo!()
    }

    fn get_data_items(&self, _keys: Vec<KeyHash>) -> Result<Vec<(KeyHash, Value)>, KvStoreError> {
        todo!()
    }
}

struct EmptyImporter;

impl RSpaceImporter for EmptyImporter {
    fn get_history_item(&self, _hash: KeyHash) -> Option<ByteVector> { todo!() }

    fn has_data_items(&self, _hashes: &[KeyHash]) -> Vec<bool> { todo!() }
}

impl TrieImporter for EmptyImporter {
    fn set_history_items(&self, _data: Vec<(KeyHash, Value)>) { todo!() }

    fn set_data_items(&self, _data: Vec<(KeyHash, Value)>) { todo!() }

    fn set_root(&self, _key: &KeyHash) { todo!() }
}

// --- prune_history_atomic -------------------------------------------------

/// The atomic prune covers mark + sweep in one pass against the
/// backing store. These tests pin the per-node correctness of
/// `HistoryRepositoryImpl::prune_history_atomic` — the pre-#1045-fix
/// code did mark and sweep in two separate steps, which admitted the
/// race that could delete a new block's just-written trie nodes.
mod prune_history_atomic {
    use rspace_plus_plus::rspace::history::radix_tree::{Item, empty_node, encode, hash_node};

    use super::*;

    fn put_node(
        store: &Arc<dyn KeyValueStore>,
        node: &rspace_plus_plus::rspace::history::radix_tree::Node,
    ) -> Blake2b256Hash {
        let (hash_bytes, _) = hash_node(node);
        store.put(vec![(hash_bytes.clone(), encode(node))]).unwrap();
        Blake2b256Hash::from_bytes(hash_bytes)
    }

    fn leaf_node(
        slot: usize,
        prefix: Vec<u8>,
        value: Vec<u8>,
    ) -> rspace_plus_plus::rspace::history::radix_tree::Node {
        let mut n = empty_node();
        n[slot] = Item::Leaf { prefix, value };
        n
    }

    fn pointer_node(
        slot: usize,
        ptr: Vec<u8>,
    ) -> rspace_plus_plus::rspace::history::radix_tree::Node {
        let mut n = empty_node();
        n[slot] = Item::NodePtr {
            prefix: vec![],
            ptr,
        };
        n
    }

    #[test]
    fn retains_reachable_nodes_and_sweeps_the_rest() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // Two chains: root_a -> child_a -> leaf_a, and root_b -> leaf_b.
        // Retaining only root_a should keep {root_a, child_a, leaf_a}
        // and sweep {root_b, leaf_b}.
        let leaf_a = put_node(&history_store, &leaf_node(0, vec![0x01], vec![0xaa; 32]));
        let child_a = put_node(&history_store, &pointer_node(1, leaf_a.bytes()));
        let root_a = put_node(&history_store, &pointer_node(2, child_a.bytes()));

        let leaf_b = put_node(&history_store, &leaf_node(3, vec![0x02], vec![0xbb; 32]));
        let root_b = put_node(&history_store, &pointer_node(4, leaf_b.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        let result = repo
            .prune_history_atomic(std::slice::from_ref(&root_a))
            .expect("atomic prune");

        assert_eq!(result.marked, 3, "root_a + child_a + leaf_a");
        assert_eq!(result.swept, 2, "root_b and leaf_b are unreachable");
        assert!(history_store.get_one(&root_a.bytes()).unwrap().is_some());
        assert!(history_store.get_one(&child_a.bytes()).unwrap().is_some());
        assert!(history_store.get_one(&leaf_a.bytes()).unwrap().is_some());
        assert!(history_store.get_one(&root_b.bytes()).unwrap().is_none());
        assert!(history_store.get_one(&leaf_b.bytes()).unwrap().is_none());
    }

    #[test]
    fn shared_subtrees_are_marked_once_and_retained() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // Two roots sharing one child. Retaining both keeps the shared
        // subtree once; the swept set is empty.
        let shared = put_node(&history_store, &empty_node());
        let root_a = put_node(&history_store, &pointer_node(0, shared.bytes()));
        let root_b = put_node(&history_store, &pointer_node(1, shared.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        let result = repo
            .prune_history_atomic(&[root_a.clone(), root_b.clone()])
            .expect("atomic prune");

        assert_eq!(result.marked, 3, "root_a, root_b, and shared counted once each");
        assert_eq!(result.swept, 0);
        assert!(history_store.get_one(&shared.bytes()).unwrap().is_some());
    }

    #[test]
    fn missing_root_subtree_is_tolerated() {
        // A joiner that has a root tag but has not yet imported the
        // subtree. The prune must not fail; it marks the root hash and
        // stops the walk at the first missing descendant. Downstream
        // readers of such a subtree then get the typed
        // `RadixTreeError::MissingNode` (not a panic) and can decide to
        // fetch from peers.
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // root references a child that is not in the store.
        let phantom_child = vec![0xCD; 32];
        let root = put_node(&history_store, &pointer_node(0, phantom_child));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        let result = repo
            .prune_history_atomic(std::slice::from_ref(&root))
            .expect("prune must tolerate a missing subtree, not panic");

        // The walk marks the root AND the phantom child hash (so a later
        // root referencing the same phantom short-circuits), but only the
        // root is actually in the store. Nothing is swept.
        assert_eq!(result.marked, 2);
        assert_eq!(result.swept, 0);
        assert!(history_store.get_one(&root.bytes()).unwrap().is_some());
    }

    #[test]
    fn empty_roots_list_is_a_total_sweep() {
        // Edge case: caller passes no retained roots. Everything is
        // unreachable, so everything in the store is swept. This is the
        // honest answer to "nothing is retained" — callers who do not
        // want this must not pass an empty list.
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        let n1 = put_node(&history_store, &empty_node());
        let n2 = put_node(&history_store, &pointer_node(1, n1.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        let result = repo.prune_history_atomic(&[]).expect("atomic prune");

        assert_eq!(result.marked, 0);
        assert_eq!(result.swept, 2);
        assert!(history_store.get_one(&n1.bytes()).unwrap().is_none());
        assert!(history_store.get_one(&n2.bytes()).unwrap().is_none());
    }

    /// A root pinned during the pass (via the `PruneGuard`'s FIFO) MUST
    /// survive even when the caller's retained-root set does not name it.
    /// This is the issue-#1061 race cause (2a): a block's new state root
    /// has been written to the trie store but the DAG has not yet caught
    /// up, so the casper-side `compute_gc_retention_roots` walk misses it.
    /// `do_checkpoint` pins the new root; `prune_history_atomic` reads the
    /// pinned snapshot and marks it before sweeping.
    #[test]
    fn pinned_root_from_checkpoint_survives_sweep_with_unrelated_caller_set() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // A root the DAG walk sees.
        let dag_leaf = put_node(&history_store, &leaf_node(0, vec![0x01], vec![0xaa; 32]));
        let dag_root = put_node(&history_store, &pointer_node(2, dag_leaf.bytes()));

        // A root that `do_checkpoint` has just committed but the DAG has
        // not yet picked up: pre-#1061 the sweep would delete its leaf.
        let pinned_leaf = put_node(&history_store, &leaf_node(1, vec![0x02], vec![0xbb; 32]));
        let pinned_root = put_node(&history_store, &pointer_node(3, pinned_leaf.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        repo.prune_guard.pin(pinned_root.clone());

        let result = repo
            .prune_history_atomic(std::slice::from_ref(&dag_root))
            .expect("atomic prune");

        // Both subtrees survive: the caller supplied dag_root and the
        // pinned FIFO supplied pinned_root. The mark set covers all 4
        // nodes; nothing is swept.
        assert_eq!(result.marked, 4);
        assert_eq!(result.swept, 0);
        assert!(
            history_store
                .get_one(&pinned_root.bytes())
                .unwrap()
                .is_some()
        );
        assert!(
            history_store
                .get_one(&pinned_leaf.bytes())
                .unwrap()
                .is_some()
        );
    }

    /// End-to-end: `do_checkpoint` must pin its new root automatically so
    /// that a subsequent sweep (with no caller-supplied retained roots)
    /// does not wipe the just-written state. This exercises the real
    /// writer path, not a manual `prune_guard.pin`.
    #[test]
    fn do_checkpoint_pins_new_root_automatically() {
        let repo = create_empty_repository();
        let test_datum = datum(1);
        let insert = InsertData {
            channel: test_channel_data_prefix(),
            data: vec![test_datum.clone()],
        };
        let next_repo =
            repo.checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(insert))]);

        let new_root = next_repo.root();

        // Direct check: the pin landed in the shared FIFO. `repo` and
        // `next_repo` share the same `Arc<PruneGuard>`.
        assert!(
            repo.prune_guard
                .pinned_snapshot()
                .iter()
                .any(|h| h == &new_root),
            "do_checkpoint must pin its new root"
        );

        // Caller supplies no retention roots; only the pinned FIFO can
        // save the new state. Pre-#1061 the write would be swept and
        // the subsequent read would fail.
        let result = next_repo.prune_history_atomic(&[]).expect("atomic prune");
        assert!(
            result.marked >= 1,
            "pinned new root must mark at least itself (got {} marked)",
            result.marked
        );

        // The new root is still in the store, and reading the datum works.
        let reader = next_repo
            .get_history_reader(&new_root)
            .expect("reader after sweep");
        let read_back = reader.base().get_data(&test_channel_data_prefix());
        assert_eq!(read_back.first().cloned(), Some(test_datum));
    }

    /// `reset` must pin its target root. The test isolates `reset`'s pin
    /// from `checkpoint`'s by using a root that `checkpoint` never saw:
    /// the empty-root node is written directly to the trie store and
    /// recorded in the roots store, so neither constructor nor checkpoint
    /// pinned it. We assert the pin lands only after `reset` is called.
    #[test]
    fn reset_pins_target_root_automatically() {
        use rspace_plus_plus::rspace::history::radix_tree::empty_node;

        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // Put the empty-root node directly in the trie store — bypasses the
        // checkpoint path so this hash is NOT in the pinned FIFO.
        let empty = put_node(&history_store, &empty_node());

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        // record_root makes it a valid reset target.
        repo.record_root(&empty).expect("record root");

        // Precondition: neither constructor nor any prior call pinned this
        // hash. `create_empty_repository`/`empty_repository_over` record
        // the empty root via `past_roots.commit`, but that writes only to
        // the roots store — not to the `PruneGuard`'s FIFO.
        assert!(
            !repo
                .prune_guard
                .pinned_snapshot()
                .iter()
                .any(|h| h == &empty),
            "precondition: target root must not already be pinned"
        );

        // Reset. `HistoryRepositoryImpl::reset` must pin before loading.
        let reset_repo = repo.reset(&empty).expect("reset to recorded root");

        // `reset_repo` is a trait object, but `repo` and `reset_repo` share
        // the same `Arc<PruneGuard>`, so observe through `repo`.
        assert!(
            repo.prune_guard
                .pinned_snapshot()
                .iter()
                .any(|h| h == &empty),
            "reset must pin its target root"
        );

        // End-to-end: a sweep with an empty caller set sees the pin and
        // keeps the root.
        let result = reset_repo.prune_history_atomic(&[]).expect("atomic prune");
        assert_eq!(result.marked, 1);
        assert_eq!(result.swept, 0);
        assert!(history_store.get_one(&empty.bytes()).unwrap().is_some());
    }

    /// Issue #1061 cause (2b): a new state can reference a node whose hash
    /// matches an older dead node. The writer's content-addressed write
    /// path (`pending_writes`) skips keys already in the store, so if the
    /// sweep deletes the already-present copy, the new state has nothing
    /// to point at. The `PruneGuard`'s write lock serialises the sweep
    /// out of that window, but a checkpoint that commits BEFORE the pass
    /// still has to have pinned its root so the sweep retains the shared
    /// node via the mark walk.
    #[test]
    fn pinned_root_retains_content_addressed_shared_node() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // A leaf shared between an older-but-dead root and a new root.
        // Content addressing means both roots point at the same bytes;
        // the "new" write skipped the leaf's put because it was already
        // present. Only the new root is pinned.
        let shared = put_node(&history_store, &leaf_node(5, vec![0x03], vec![0xcc; 32]));
        let dead_root = put_node(&history_store, &pointer_node(0, shared.bytes()));
        let new_root = put_node(&history_store, &pointer_node(1, shared.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        repo.prune_guard.pin(new_root.clone());

        // Caller supplies no roots — only the pinned FIFO retains.
        let result = repo.prune_history_atomic(&[]).expect("atomic prune");

        // The pinned new_root marks itself and the shared leaf; dead_root
        // is swept.
        assert_eq!(result.marked, 2);
        assert_eq!(result.swept, 1);
        assert!(history_store.get_one(&new_root.bytes()).unwrap().is_some());
        assert!(history_store.get_one(&shared.bytes()).unwrap().is_some());
        assert!(history_store.get_one(&dead_root.bytes()).unwrap().is_none());
    }

    /// Issue #1098: a root pinned before any pass has run, with no
    /// caller-supplied retained root referring to it, is eventually
    /// evicted by `PIN_RETENTION_PASSES`-bounded age eviction and its
    /// subtree is swept. This is the lever that keeps the mark set from
    /// growing without bound under long-running load.
    ///
    /// `PIN_RETENTION_PASSES` is a module-private constant in
    /// `history_repository_impl`; the test runs a conservative number of
    /// passes (6, well above the current retention + 1) so it does not
    /// fail if the constant is later tuned slightly.
    #[test]
    fn pin_is_evicted_after_retention_window_and_its_subtree_is_swept() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        // One pinned root with a child, reachable ONLY via the pin — no
        // caller retention root will reference it.
        let doomed_leaf = put_node(&history_store, &leaf_node(0, vec![0x01], vec![0xaa; 32]));
        let doomed_root = put_node(&history_store, &pointer_node(2, doomed_leaf.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        repo.prune_guard.pin(doomed_root.clone());

        // Precondition: the pin exists and nothing has been swept.
        assert!(
            repo.prune_guard
                .pinned_snapshot()
                .iter()
                .any(|h| h == &doomed_root)
        );

        // Run passes until the pin is evicted and the subtree is swept.
        // Bail out as soon as both doomed nodes are gone — keeps the test
        // tolerant to small changes in the retention constant.
        let mut evicted_pass: Option<usize> = None;
        for pass in 1..=6 {
            let result = repo.prune_history_atomic(&[]).expect("atomic prune");
            if history_store
                .get_one(&doomed_root.bytes())
                .unwrap()
                .is_none() &&
                history_store
                    .get_one(&doomed_leaf.bytes())
                    .unwrap()
                    .is_none()
            {
                evicted_pass = Some(pass);
                // The pass that swept the subtree must also report the
                // eviction via `pins_evicted` — the hook operators use to
                // see the mark set contracting.
                assert!(
                    result.pins_evicted >= 1,
                    "the pass that swept the pin must report pins_evicted >= 1 (got {})",
                    result.pins_evicted
                );
                break;
            }
        }
        assert!(
            evicted_pass.is_some(),
            "pin must be evicted within 6 passes under any reasonable retention setting"
        );
    }

    /// Issue #1098: a pin that is refreshed every pass must survive
    /// indefinitely. This models a root that `reset` keeps hitting during
    /// a long replay — age eviction must NOT drop it while it is actively
    /// in use.
    #[test]
    fn repinned_root_survives_age_eviction_indefinitely() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        let leaf = put_node(&history_store, &leaf_node(0, vec![0x01], vec![0xaa; 32]));
        let root = put_node(&history_store, &pointer_node(2, leaf.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);

        for _ in 0..8 {
            // Simulate a writer / replay pinning this same root every pass.
            repo.prune_guard.pin(root.clone());
            let result = repo.prune_history_atomic(&[]).expect("atomic prune");
            assert_eq!(result.swept, 0, "refreshed pin must retain its subtree");
            assert!(history_store.get_one(&root.bytes()).unwrap().is_some());
            assert!(history_store.get_one(&leaf.bytes()).unwrap().is_some());
        }
    }

    /// Issue #1098: `prune_history_atomic` returns the new observability
    /// counters so the casper-side driver can log them. This test does
    /// not assert exact values beyond basic shape — the unit tests on
    /// `PruneGuard::begin_pass` and the batched sweep cover the exact
    /// arithmetic.
    #[test]
    fn prune_history_atomic_reports_pin_and_batch_counters() {
        let history_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let roots_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let writer = Arc::new(KvCheckpointWriter {
            history: history_store.clone(),
            roots: roots_store.clone(),
        });

        let leaf = put_node(&history_store, &leaf_node(0, vec![0x01], vec![0xaa; 32]));
        let root = put_node(&history_store, &pointer_node(2, leaf.bytes()));

        let repo = empty_repository_over(history_store.clone(), roots_store, writer);
        repo.prune_guard.pin(root.clone());

        let result = repo.prune_history_atomic(&[]).expect("atomic prune");
        assert_eq!(result.marked, 2);
        assert_eq!(result.swept, 0);
        assert_eq!(result.pins_initial, 1);
        assert_eq!(
            result.marked_from_pins, 2,
            "the root and leaf are both reachable from the single pin"
        );
        assert!(result.batches >= 1, "a non-empty store must run at least one delete batch");
        assert_eq!(result.pins_mid_pass_added, 0, "no concurrent writers here");
    }
}
