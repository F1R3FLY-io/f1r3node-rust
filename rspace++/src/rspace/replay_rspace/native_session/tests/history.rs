use std::cell::Cell;

use super::*;
use crate::rspace::hashing::native_source::{GroupKeys, channel_key, channels_hash_from_keys};
use crate::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::native_reader::{
    NativeLeafKind, decode_continuation, decode_datum, decode_join,
};
use crate::rspace::history::radix_tree::{Item, empty_node, encode};

struct ForbiddenReader;

impl HistoryReaderBase<String, String, String, String> for ForbiddenReader {
    fn get_data_proj(&self, _: &String) -> Vec<Datum<String>> { panic!("legacy datum read") }
    fn get_continuations_proj(&self, _: &Vec<String>) -> Vec<WaitingContinuation<String, String>> {
        panic!("legacy continuation read")
    }
    fn get_joins_proj(&self, _: &String) -> Vec<Vec<String>> { panic!("legacy join read") }
}

type History = Arc<Box<dyn HistoryRepository<String, String, String, String> + Send + Sync>>;

fn isolated(history: History) -> Session {
    let session = Session::new(history, Arc::new(Box::new(Matcher)), Epoch::default()).unwrap();
    *session.space.store.write().unwrap() =
        Arc::new(HotStoreInstances::create_from_hr(Box::new(ForbiddenReader)));
    session
}

async fn retained_history() -> History {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    play.produce("data".to_owned(), "retained".to_owned(), true)
        .await
        .unwrap();
    play.consume(
        vec!["left".to_owned(), "right".to_owned()],
        vec!["p".to_owned(), "p".to_owned()],
        "joined".to_owned(),
        false,
        BTreeSet::new(),
    )
    .await
    .unwrap();
    play.create_checkpoint().await.unwrap();
    play.get_history_repository()
}

/// Changed by DR-131 S6: the test was
/// `cold_typed_rows_prepay_cleanup_before_cache_handoff`. The charge and
/// rejection checks went with host work. The typed decoder is deleted, so
/// the reference decode is bincode, the stored format. The test keeps its
/// checks that the session decodes every stored row and that the read puts
/// nothing in the cache.
#[tokio::test]
async fn cold_rows_decode_without_a_cache_handoff() {
    let history = retained_history().await;
    let session = isolated(history.clone());
    let channel = "data".to_owned();
    let projection = hash(&channel);
    let key: [u8; 32] = projection.0.as_slice().try_into().unwrap();
    let rows_count = Cell::new(0);
    history
        .native_history_reader(session.root)
        .with_records(NativeLeafKind::Data, &key, |rows| {
            rows_count.set(rows.len());
            for row in rows.iter() {
                let datum: Datum<String> = bincode::deserialize(row).unwrap();
                assert_eq!(datum.a, "retained");
            }
            Ok::<_, RSpaceError>(())
        })
        .unwrap()
        .unwrap();
    let values = session
        .read_records(
            NativeLeafKind::Data,
            // Changed by D-C2d (D-S1, DR-96): the reader takes the digest's
            // 32 bytes.
            // projection,
            &key,
            decode_datum::<String>,
        )
        .unwrap();
    assert_eq!(values.len(), rows_count.get());
    // Changed by D-C2c (D-S1, DR-96): the native session caches in its
    // digest-keyed store.
    // assert!(session.space.get_store().snapshot().data_flat().is_empty());
    assert_eq!(session.store.entry_counts()[0], 0);
}

async fn query(session: &Session, kind: usize) -> Result<(), RSpaceError> {
    match kind {
        0 => {
            let values = session.get_data(&"data".to_owned()).await?;
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].a, "retained");
            assert!(values[0].persist);
        }
        1 => {
            let values = session.get_joins(&"left".to_owned()).await?;
            assert_eq!(values, [vec!["left".to_owned(), "right".to_owned()]]);
        }
        _ => {
            let values = session
                .get_continuations(&["left".to_owned(), "right".to_owned()])
                .await?;
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].continuation, "joined");
            assert_eq!(values[0].patterns, ["p", "p"]);
        }
    }
    Ok(())
}

#[tokio::test]
async fn cold_and_warm_native_queries_never_use_the_legacy_reader() {
    let history = retained_history().await;
    let root = history.root();
    let session = isolated(history.clone());
    let checkpoint = session.checkpoint().await.unwrap();
    for _ in 0..2 {
        for kind in 0..3 {
            query(&session, kind).await.unwrap();
        }
    }
    session.restore(checkpoint).await.unwrap();
    // Changed by D-C2c (D-S1, DR-96): the native session caches in its
    // digest-keyed store.
    // assert!(session.space.get_store().snapshot().data_flat().is_empty());
    assert_eq!(session.store.entry_counts()[0], 0);
    for kind in 0..3 {
        query(&session, kind).await.unwrap();
    }
    assert_eq!(session.completed_usage().await.unwrap(), 0);
    assert_eq!(history.root(), root);
    session.close().await.unwrap();
    for kind in 0..3 {
        assert!(query(&session, kind).await.is_err());
    }
}

/// Changed by DR-131 S6: the test was
/// `metered_current_root_reader_preserves_legacy_projection_values`. The
/// charge check went with host work. The test keeps its checks of the root
/// and of the projections.
#[tokio::test]
async fn current_root_reader_preserves_legacy_projection_values() {
    let history = retained_history().await;
    let root = history.root();
    let native = history.get_current_history_reader_native();
    let legacy = history.get_history_reader(&root).unwrap();
    assert_eq!(native.root(), root);
    assert_eq!(
        native.get_data_proj_generic(&"data".to_owned()),
        legacy.get_data_proj_generic(&"data".to_owned())
    );
    assert_eq!(
        native.get_joins_proj_generic(&"left".to_owned()),
        legacy.get_joins_proj_generic(&"left".to_owned())
    );
    assert_eq!(
        native.get_continuations_proj_generic(&vec!["left".to_owned(), "right".to_owned()]),
        legacy.get_continuations_proj_generic(&vec!["left".to_owned(), "right".to_owned()])
    );
}

#[tokio::test]
async fn cold_reads_remain_on_the_session_root_after_repository_root_changes() {
    let history = retained_history().await;
    let old_root = history.root();
    let session = isolated(history.clone());
    let empty_root = RadixHistory::empty_root_node_hash();
    {
        let handle = history.history();
        let mut current = handle.lock().unwrap();
        *current = current.reset(&empty_root).unwrap();
    }
    assert_eq!(history.root(), empty_root);
    assert_ne!(old_root, empty_root);
    for kind in 0..3 {
        query(&session, kind).await.unwrap();
    }
    let later = isolated(history);
    assert!(later.get_data(&"data".to_owned()).await.unwrap().is_empty());
    assert!(
        later
            .get_joins(&"left".to_owned())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        later
            .get_continuations(&["left".to_owned(), "right".to_owned()])
            .await
            .unwrap()
            .is_empty()
    );
}

/// Changed by DR-131 S6: the test was
/// `independent_native_readers_keep_separate_caches_and_exhaustion`. The
/// exhaustion went with host work. The test keeps its check that a read of
/// one session fills only that session's cache.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_native_readers_keep_separate_caches() {
    let history = retained_history().await;
    let left = isolated(history.clone());
    let right = isolated(history);
    query(&right, 0).await.unwrap();
    // Changed by D-C2c (D-S1, DR-96): the native session caches in its
    // digest-keyed store.
    // assert!(left.space.get_store().snapshot().data_flat().is_empty());
    // assert_eq!(right.space.get_store().snapshot().data_flat().len(), 1);
    assert_eq!(left.store.entry_counts()[0], 0);
    assert_eq!(right.store.entry_counts()[0], 1);
}

#[tokio::test]
async fn malformed_authenticated_records_never_publish_a_partial_cache_or_fall_back() {
    let mut stores = InMemoryStoreManager::new();
    let storage = stores.r_space_stores().await.unwrap();
    let nodes = storage.history.clone();
    let leaves = storage.cold.clone();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        storage,
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    let datum = Datum {
        a: "valid first row".to_owned(),
        persist: false,
        source: Produce::create(&"data", &"valid first row", false),
    };
    let rows = vec![bincode::serialize(&datum).unwrap(), vec![255; 3]];
    let payload = bincode::serialize(&rows).unwrap();
    let body = bincode::serialize(&payload).unwrap();
    let pointer = Blake2b256Hash::new(&body);
    let mut stored = 1_u32.to_le_bytes().to_vec();
    stored.extend_from_slice(&body);
    leaves.put_one(pointer.0.clone(), stored).unwrap();
    let mut node = empty_node();
    node[0] = Item::Leaf {
        prefix: hash(&"data".to_owned()).0,
        value: pointer.0,
    };
    let bytes = encode(&node);
    let root = Blake2b256Hash::new(&bytes);
    nodes.put_one(root.0.clone(), bytes).unwrap();
    let history = play.get_history_repository();
    history.record_root(&root).unwrap();
    let history = Arc::new(history.reset(&root).unwrap());
    let session = isolated(history.clone());
    for _ in 0..3 {
        let error = session.get_data(&"data".to_owned()).await.unwrap_err();
        assert_eq!(error, RSpaceError::InterpreterError("native history: TypedRecord".to_owned()));
        // Changed by D-C2c (D-S1, DR-96): the native session caches in its
        // digest-keyed store.
        // assert!(session.space.get_store().snapshot().data_flat().is_empty());
        assert_eq!(session.store.entry_counts()[0], 0);
        assert_eq!(history.root(), root);
    }
}

/// D-S3 (D-C3, DR-97): clean cold fills are not exported. The full export of
/// the same entries re-inserts the stored leaves and deletes an absent key,
/// which keeps the root. The dirty export has no action, and the session's
/// export keeps the root.
#[tokio::test]
async fn clean_cold_fill_is_not_exported() {
    let history = retained_history().await;
    let root = history.root();
    let session = isolated(history.clone());
    for kind in 0..3 {
        query(&session, kind).await.expect("a cold fill");
    }
    let absent = session
        .get_data(&"absent".to_owned())
        .await
        .expect("an empty cold fill");
    assert!(absent.is_empty());
    let counts = session.store.entry_counts();
    assert_eq!([counts[0], counts[1], counts[3]], [2, 1, 1]);
    let dirty = session.store.dirty_entries();
    let actions = dirty.actions();
    assert!(actions.is_empty());
    let full = session.store.changes();
    assert_eq!(full.len(), 4);
    let prepared = history
        .prepare_native_checkpoint(full)
        .expect("the full export");
    assert_eq!(prepared.cold_actions.len(), 3);
    assert_eq!(prepared.root, Some(root.clone()));
    let borrowed = history
        .prepare_native_checkpoint_borrowed(&actions)
        .expect("the dirty export");
    assert!(borrowed.cold_actions.is_empty());
    assert!(borrowed.prepared_history.is_none());
    let export = session.export().await.expect("the export");
    assert_eq!(export.root(), &root);
    assert_eq!(history.root(), root);
}

fn digest_bytes(digest: Blake2b256Hash) -> [u8; 32] {
    digest.0.as_slice().try_into().expect("a 32-byte digest")
}

/// D-S1 (D-C2d, DR-96): the cold fills read the history by the store keys.
/// For data, joins and continuations they return the records that a read by
/// the legacy projections returns, which hashed every channel again. The
/// continuation projection does not depend on the channel order.
#[tokio::test]
async fn cold_fill_reads_same_records_as_legacy() {
    let history = retained_history().await;
    let session = isolated(history);
    let data = "data".to_owned();
    let legacy_data = session
        .read_records(NativeLeafKind::Data, &digest_bytes(hash(&data)), decode_datum::<String>)
        .unwrap();
    assert_eq!(legacy_data.len(), 1);
    let data_key = channel_key(&data).unwrap();
    assert_eq!(session.read_data_with(&data, data_key).unwrap(), legacy_data);
    let group = vec!["left".to_owned(), "right".to_owned()];
    let legacy_continuations = session
        .read_records(
            NativeLeafKind::Continuations,
            &digest_bytes(hash_from_vec(&group)),
            decode_continuation::<String, String>,
        )
        .unwrap();
    assert_eq!(legacy_continuations.len(), 1);
    let keys = GroupKeys::build(&group).unwrap();
    assert_eq!(session.read_continuations(&group, &keys).unwrap(), legacy_continuations);
    let reversed: Vec<String> = group.iter().rev().cloned().collect();
    let reversed_keys = GroupKeys::build(&reversed).unwrap();
    assert_eq!(
        channels_hash_from_keys(&reversed_keys.channels),
        channels_hash_from_keys(&keys.channels)
    );
    for channel in &group {
        let legacy_joins = session
            .read_records(
                NativeLeafKind::Joins,
                &digest_bytes(hash(channel)),
                decode_join::<String>,
            )
            .unwrap();
        assert_eq!(legacy_joins, vec![group.clone()]);
        let key = channel_key(channel).unwrap();
        assert_eq!(session.read_joins(channel, key).unwrap(), legacy_joins);
    }
}
