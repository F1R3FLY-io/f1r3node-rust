// See rspace/src/test/scala/coop/rchain/rspace/history/HistoryRepositorySpec.
// scala

use std::collections::{BTreeSet, HashSet};
use std::ops::Deref;
use std::sync::Arc;

use rand::prelude::SliceRandom;
use rspace_plus_plus::rspace::errors::{HistoryError, RootError};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_slice};
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::hot_store_action::{
    DeleteAction, DeleteCanonicalCollection, DeleteContinuations, DeleteData, DeleteJoins,
    HotStoreAction, InsertAction, InsertCanonicalCollection, InsertCanonicalData,
    InsertContinuations, InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieInsertAction, TrieInsertBinaryConsume, TrieInsertBinaryJoins,
    TrieInsertBinaryProduce,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::{
    encode_continuations, encode_datums, encode_joins,
};
use rspace_plus_plus::rspace::shared::trie_exporter::KeyHash;
use rspace_plus_plus::rspace::state::import_session::{
    StateImportError, StateImportSession, StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use rspace_plus_plus::rspace::state::rspace_state_authority::RSpaceStateAuthority;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use shared::rust::ByteVector;

use crate::history::history_action_tests::{random_blake, zeros_blake};

#[tokio::test]
async fn history_repository_should_process_insert_one_datum() {
    let repo = create_empty_repository();
    let test_datum = datum(1);
    let insert_data = InsertData {
        channel: test_channel_data_prefix(),
        data: vec![test_datum.clone()],
    };

    let next_repo = repo
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(insert_data))])
        .unwrap();
    let history_reader = next_repo.get_history_reader(&next_repo.root());
    let data = history_reader
        .unwrap()
        .base()
        .get_data(&test_channel_data_prefix());
    let fetched = data.first().unwrap().clone();

    assert_eq!(fetched, test_datum);
}

#[tokio::test]
async fn malformed_equal_payloads_of_all_kinds_are_rejected_atomically() {
    let repo = create_empty_repository();
    let initial_root = repo.root();
    let payload = vec![b"same-record".to_vec()];
    let data_key = Blake2b256Hash::new(b"data-key");
    let continuation_key = Blake2b256Hash::new(b"continuation-key");
    let joins_key = Blake2b256Hash::new(b"joins-key");
    let result = repo.do_checkpoint(vec![
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
            TrieInsertBinaryProduce {
                hash: data_key.clone(),
                data: payload.clone(),
            },
        )),
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
            TrieInsertBinaryConsume {
                hash: continuation_key.clone(),
                continuations: payload.clone(),
            },
        )),
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
            TrieInsertBinaryJoins {
                hash: joins_key.clone(),
                joins: payload.clone(),
            },
        )),
    ]);

    assert!(matches!(result, Err(HistoryError::ActionError(_))));
    assert_eq!(repo.authority.current_root().unwrap(), initial_root);
}

#[tokio::test]
async fn canonical_data_action_is_root_equivalent_to_typed_data_action() {
    let channel = test_channel_data_prefix();
    let first = datum(1);
    let second = datum(2);
    let data = vec![second, first.clone(), first];

    let typed = create_empty_repository()
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: channel.clone(),
            data: data.clone(),
        }))])
        .unwrap();
    let canonical = create_empty_repository()
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertCanonicalData(
            InsertCanonicalData::try_from_encoded_datums(
                hash(&channel),
                encode_datums(&data),
                data.len() as u64,
            )
            .expect("typed datum bytes form a valid canonical action"),
        ))])
        .unwrap();

    assert_eq!(canonical.root(), typed.root());
    let fetched = canonical
        .get_history_reader(&canonical.root())
        .expect("canonical checkpoint reader")
        .base()
        .get_data(&channel);
    assert_eq!(encode_datums(&fetched), encode_datums(&data));
}

#[tokio::test]
async fn canonical_continuation_action_is_root_and_delete_equivalent_to_typed_action() {
    let channels = vec![test_channel_continuations_prefix(), "second-channel".to_string()];
    let first = continuation(1);
    let mut second = continuation(2);
    second.persist = false;
    let continuations = vec![second, first];

    let typed = create_empty_repository()
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertContinuations(
            InsertContinuations {
                channels: channels.clone(),
                continuations: continuations.clone(),
            },
        ))])
        .unwrap();
    let canonical = create_empty_repository()
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(
            InsertCanonicalCollection::try_from_encoded_continuations(
                hash_from_slice(&channels),
                encode_continuations(&continuations),
                continuations.len() as u64,
            )
            .expect("typed continuation bytes form a valid canonical action"),
        ))])
        .unwrap();

    assert_eq!(canonical.root(), typed.root());
    let fetched = canonical
        .get_history_reader(&canonical.root())
        .expect("canonical checkpoint reader")
        .base()
        .get_continuations(&channels);
    assert_eq!(encode_continuations(&fetched), encode_continuations(&continuations));

    let typed_deleted = typed
        .checkpoint(vec![HotStoreAction::Delete(DeleteAction::DeleteContinuations(
            DeleteContinuations {
                channels: channels.clone(),
            },
        ))])
        .unwrap();
    let canonical_deleted = canonical
        .checkpoint(vec![HotStoreAction::Delete(DeleteAction::DeleteCanonicalCollection(
            DeleteCanonicalCollection::continuations(hash_from_slice(&channels)),
        ))])
        .unwrap();
    assert_eq!(canonical_deleted.root(), typed_deleted.root());
    assert!(
        canonical_deleted
            .get_history_reader(&canonical_deleted.root())
            .unwrap()
            .base()
            .get_continuations(&channels)
            .is_empty()
    );
}

#[tokio::test]
async fn canonical_join_action_preserves_duplicate_multiplicity_and_typed_roots() {
    let channel = test_channel_joins_prefix();
    let repeated = vec!["repeated".to_string(), "suffix".to_string()];
    let other = vec!["other".to_string()];
    let joins = vec![repeated.clone(), other, repeated];

    let typed = create_empty_repository()
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channel.clone(),
            joins: joins.clone(),
        }))])
        .unwrap();
    let canonical = create_empty_repository()
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(
            InsertCanonicalCollection::try_from_encoded_joins(
                hash(&channel),
                encode_joins(&joins),
                joins.len() as u64,
            )
            .expect("typed join bytes form a valid canonical action"),
        ))])
        .unwrap();

    assert_eq!(canonical.root(), typed.root());
    let fetched = canonical
        .get_history_reader(&canonical.root())
        .expect("canonical checkpoint reader")
        .base()
        .get_joins(&channel);
    assert_eq!(encode_joins(&fetched), encode_joins(&joins));
    assert_eq!(fetched.len(), joins.len());

    let typed_deleted = typed
        .checkpoint(vec![HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins {
            channel: channel.clone(),
        }))])
        .unwrap();
    let canonical_deleted = canonical
        .checkpoint(vec![HotStoreAction::Delete(DeleteAction::DeleteCanonicalCollection(
            DeleteCanonicalCollection::joins(hash(&channel)),
        ))])
        .unwrap();
    assert_eq!(canonical_deleted.root(), typed_deleted.root());
    assert!(
        canonical_deleted
            .get_history_reader(&canonical_deleted.root())
            .unwrap()
            .base()
            .get_joins(&channel)
            .is_empty()
    );
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

    let next_repo = repo
        .checkpoint(vec![
            HotStoreAction::Insert(InsertAction::InsertData(data)),
            HotStoreAction::Insert(InsertAction::InsertJoins(joins.clone())),
            HotStoreAction::Insert(InsertAction::InsertContinuations(continuations)),
        ])
        .unwrap();
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
    let mut rng = rand::thread_rng();
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

    let next_repo = repo.checkpoint(elems).unwrap();
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

    let deleted_repo = next_repo.checkpoint(delete_elems).unwrap();
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
async fn history_repository_should_not_allow_switching_to_a_not_existing_root() {
    let repo = create_empty_repository();

    match repo.reset(&zeros_blake()) {
        Err(HistoryError::RootError(RootError::UnknownRootError(err))) => {
            assert!(
                err.contains("unknown verified root"),
                "Expected error containing 'unknown verified root', got: {}",
                err
            )
        }
        Ok(_) => assert!(false, "Expected a failure"),
        _ => assert!(false, "Wrong error thrown"),
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

    let next_repo = repo
        .checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(insert_data))])
        .unwrap();
    let _ = repo.reset(&RadixHistory::empty_root_node_hash());
    let next_root = next_repo.root();
    let _ = repo.reset(&next_root).unwrap();
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

fn continuation(s: i32) -> WaitingContinuation<String, String> {
    WaitingContinuation {
        patterns: std::sync::Arc::new(vec![format!("pattern-{}", s)]),
        continuation: std::sync::Arc::new(format!("cont-{}", s)),
        persist: true,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: vec![random_blake()],
            hash: random_blake(),
            persistent: true,
        },
    }
}

fn datum(s: i32) -> Datum<String> {
    Datum {
        a: std::sync::Arc::new(format!("data-{}", s)),
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

type DynTestHistoryRepository =
    dyn HistoryRepository<String, String, String, String> + Send + Sync + 'static;

struct TestHistoryRepository {
    repository: Box<DynTestHistoryRepository>,
    authority: Arc<RSpaceStateAuthority>,
}

impl Deref for TestHistoryRepository {
    type Target = DynTestHistoryRepository;

    fn deref(&self) -> &Self::Target { self.repository.as_ref() }
}

fn create_empty_repository() -> TestHistoryRepository {
    let authority = RSpaceStateAuthority::in_memory();
    let repository =
        HistoryRepositoryInstances::<String, String, String, String>::repository(authority.clone())
            .expect("authority-backed history repository");
    TestHistoryRepository {
        repository,
        authority,
    }
}

struct EmptyImporter;

impl RSpaceImporter for EmptyImporter {
    fn begin_import(
        &self,
        _spec: StateImportSpec,
    ) -> Result<Box<dyn StateImportSession>, StateImportError> {
        Err(StateImportError::InternalInvariant(
            "the history-repository fixture has no state-import stores".to_string(),
        ))
    }

    fn get_history_item(&self, _hash: KeyHash) -> Result<Option<ByteVector>, StateImportError> {
        Ok(None)
    }
}

#[test]
fn repository_only_importer_mock_rejects_mutation_authority_without_panicking() {
    let root = RadixHistory::empty_root_node_hash();
    let spec = rspace_plus_plus::rspace::state::import_session::StateImportSpec::new(
        [root.clone()],
        rspace_plus_plus::rspace::state::import_session::StateImportPublicationMode::Activate(root),
    )
    .unwrap();
    let error = match EmptyImporter.begin_import(spec) {
        Ok(_) => panic!("repository-only fixture unexpectedly granted import authority"),
        Err(error) => error,
    };
    assert!(matches!(error, StateImportError::InternalInvariant(_)));
}
