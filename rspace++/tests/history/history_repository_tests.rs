// See rspace/src/test/scala/coop/rchain/rspace/history/HistoryRepositorySpec.
// scala

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use rand::prelude::SliceRandom;
use rspace_plus_plus::rspace::errors::{HistoryError, RootError};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::history::checkpoint_writer::{CheckpointWriter, KvCheckpointWriter};
use rspace_plus_plus::rspace::history::history::HistoryInstances;
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::history::history_repository_impl::HistoryRepositoryImpl;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
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
        rspace_exporter: Arc::new(EmptyExporter),
        rspace_importer: Arc::new(EmptyImporter),
        _marker: std::marker::PhantomData,
    }
}

fn create_inmem_cold_store() -> Arc<dyn KeyValueStore> { Arc::new(InMemoryKeyValueStore::new()) }

struct EmptyExporter;

impl RSpaceExporter for EmptyExporter {
    fn get_root(&self) -> Result<KeyHash, RootError> { todo!() }
}

impl TrieExporter for EmptyExporter {
    fn get_nodes(&self, _start_path: NodePath, _skip: i32, _take: i32) -> Vec<TrieNode<KeyHash>> {
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
}

impl TrieImporter for EmptyImporter {
    fn set_history_items(&self, _data: Vec<(KeyHash, Value)>) { todo!() }

    fn set_data_items(&self, _data: Vec<(KeyHash, Value)>) { todo!() }

    fn set_root(&self, _key: &KeyHash) { todo!() }
}
