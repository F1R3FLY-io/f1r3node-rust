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
use crate::rspace::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};
use crate::rspace::serializers::serializers::{encode_continuations_for_epoch, encode_datum};

struct ForbiddenReader;

impl<T: Clone> HistoryReaderBase<T, T, T, T> for ForbiddenReader {
    fn get_data_proj(&self, _: &T) -> Result<Vec<Datum<T>>, crate::rspace::errors::RSpaceError> {
        panic!("legacy datum read")
    }
    fn get_continuations_proj(
        &self,
        _: &Vec<T>,
    ) -> Result<Vec<WaitingContinuation<T, T>>, crate::rspace::errors::RSpaceError> {
        panic!("legacy continuation read")
    }
    fn get_joins_proj(&self, _: &T) -> Result<Vec<Vec<T>>, crate::rspace::errors::RSpaceError> {
        panic!("legacy join read")
    }
}

type History<T = String> = Arc<Box<dyn HistoryRepository<T, T, T, T> + Send + Sync>>;

fn isolated(history: History) -> Session { isolated_for_epoch(history, EventCodecEpoch::Legacy) }

fn isolated_for_epoch(history: History, codec: EventCodecEpoch) -> Session {
    let session = Session::new(history, Arc::new(Box::new(Matcher)), Epoch {
        codec: Some(codec),
        ..Epoch::default()
    })
    .expect("create isolated native session");
    *session.space.store.write().unwrap() =
        Arc::new(HotStoreInstances::create_from_hr(Box::new(ForbiddenReader)));
    session
}

#[derive(Debug)]
struct UndecodedRecord;

async fn framed_history<T>(
    kind: NativeLeafKind,
    payload: Option<&[u8]>,
    corrupt_hash: bool,
) -> (History<T>, [u8; 32])
where
    T: Clone
        + std::fmt::Debug
        + Default
        + serde::Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + StableHashSerialize
        + std::hash::Hash
        + Ord
        + Send
        + Sync
        + 'static,
{
    let mut stores = InMemoryStoreManager::new();
    let storage = stores
        .r_space_stores()
        .await
        .expect("create history stores");
    let nodes = storage.history.clone();
    let leaves = storage.cold.clone();
    let (play, _) = RSpace::<T, T, T, T>::create_with_replay(storage, Arc::new(Box::new(Matcher)))
        .expect("create history fixture");
    let (prefix, tag, projection) = match kind {
        NativeLeafKind::Data => (0, 1_u32, hash(&"data".to_owned())),
        NativeLeafKind::Continuations => (1, 2_u32, hash_from_vec(&vec!["data".to_owned()])),
        NativeLeafKind::Joins => (2, 0_u32, hash(&"data".to_owned())),
    };
    let key = digest_bytes(projection.clone());
    let history = play.get_history_repository();
    let Some(payload) = payload else {
        return (history, key);
    };
    let body = bincode::serialize(payload).expect("encode leaf payload");
    let pointer = if corrupt_hash {
        Blake2b256Hash::new(b"different leaf")
    } else {
        Blake2b256Hash::new(&body)
    };
    let mut stored = tag.to_le_bytes().to_vec();
    stored.extend_from_slice(&body);
    leaves
        .put_one(pointer.0.clone(), stored)
        .expect("store framed leaf");
    let mut node = empty_node();
    node[prefix] = Item::Leaf {
        prefix: projection.0,
        value: pointer.0,
    };
    let bytes = encode(&node);
    let root = Blake2b256Hash::new(&bytes);
    nodes
        .put_one(root.0.clone(), bytes)
        .expect("store history node");
    history.record_root(&root).expect("record fixture root");
    (Arc::new(history.reset(&root).expect("select fixture root")), key)
}

async fn boundary_query(session: &Session, kind: NativeLeafKind) -> Result<(), RSpaceError> {
    match kind {
        NativeLeafKind::Data => session.get_data(&"data".to_owned()).await.map(|_| ()),
        NativeLeafKind::Continuations => session
            .get_continuations(&["data".to_owned()])
            .await
            .map(|_| ()),
        NativeLeafKind::Joins => session.get_joins(&"data".to_owned()).await.map(|_| ()),
    }
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
struct LegacyOnly(String);

impl StableHashSerialize for LegacyOnly {}

impl ColdStoreDecode for LegacyOnly {
    fn cold_decode_prefix(_: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Err(ColdStoreDecodeError::Legacy("unsupported current test schema".to_owned()))
    }

    fn cold_decode_legacy_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        String::cold_decode_legacy_prefix(bytes).map(|(value, consumed)| (Self(value), consumed))
    }

    fn cold_decode_cost_v1_prefix(_: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Err(ColdStoreDecodeError::Legacy("unsupported v1 test schema".to_owned()))
    }
}

type UnsupportedSession =
    NativeReplaySession<LegacyOnly, LegacyOnly, LegacyOnly, LegacyOnly, Epoch>;

fn unsupported_session(history: History<LegacyOnly>, codec: EventCodecEpoch) -> UnsupportedSession {
    let session = UnsupportedSession::new(history, Arc::new(Box::new(Matcher)), Epoch {
        codec: Some(codec),
        ..Epoch::default()
    })
    .expect("create schema-bound session");
    *session.space.store.write().expect("store lock") =
        Arc::new(HotStoreInstances::create_from_hr(Box::new(ForbiddenReader)));
    session
}

async fn unsupported_query(
    session: &UnsupportedSession,
    kind: NativeLeafKind,
) -> Result<(), RSpaceError> {
    let channel = LegacyOnly("data".to_owned());
    match kind {
        NativeLeafKind::Data => session.get_data(&channel).await.map(|_| ()),
        NativeLeafKind::Continuations => session.get_continuations(&[channel]).await.map(|_| ()),
        NativeLeafKind::Joins => session.get_joins(&channel).await.map(|_| ()),
    }
}

fn valid_schema_row(kind: NativeLeafKind, value: &str) -> Vec<u8> {
    let channel = "data".to_owned();
    match kind {
        NativeLeafKind::Data => encode_datum(&Datum::create(&channel, value.to_owned(), false)),
        NativeLeafKind::Continuations => {
            let waiting = WaitingContinuation::create(
                &vec![channel],
                &vec![value.to_owned()],
                &value.to_owned(),
                false,
                BTreeSet::new(),
            );
            let leaf = encode_continuations_for_epoch(EventCodecEpoch::Legacy, &[waiting])
                .expect("encode continuation leaf");
            bincode::deserialize::<Vec<Vec<u8>>>(&leaf)
                .expect("decode continuation rows")
                .pop()
                .expect("one continuation")
        }
        NativeLeafKind::Joins => bincode::serialize(&vec![value.to_owned()]).expect("encode join"),
    }
}

#[tokio::test]
async fn absent_and_zero_row_history_never_enters_a_decoder_for_any_epoch() {
    for kind in [NativeLeafKind::Data, NativeLeafKind::Continuations, NativeLeafKind::Joins] {
        for payload in [None, Some(&[0_u8; 8][..])] {
            let (history, key) = framed_history::<String>(kind, payload, false).await;
            for codec in [EventCodecEpoch::Legacy, EventCodecEpoch::CostV1, EventCodecEpoch::CostV2]
            {
                let session = isolated_for_epoch(history.clone(), codec);
                let values = session
                    .read_records::<UndecodedRecord>(kind, &key, |_| {
                        panic!("empty history must not invoke a decoder")
                    })
                    .expect("empty history");
                assert!(values.is_empty());
                assert_eq!(session.store.entry_counts(), [0; 5]);
                boundary_query(&session, kind)
                    .await
                    .expect("query empty history");
            }
        }
    }
}

#[tokio::test]
async fn unsupported_nonempty_history_refuses_without_cache_or_legacy_fallback() {
    for kind in [NativeLeafKind::Data, NativeLeafKind::Continuations, NativeLeafKind::Joins] {
        for rows in [vec![Vec::<u8>::new()], vec![vec![0], vec![255; 3]], vec![valid_schema_row(
            kind, "retained",
        )]] {
            let payload = bincode::serialize(&rows).expect("encode nonempty rows");
            let (history, _) = framed_history::<LegacyOnly>(kind, Some(&payload), false).await;
            for codec in [EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
                let session = unsupported_session(history.clone(), codec);
                for _ in 0..2 {
                    assert_eq!(
                        unsupported_query(&session, kind).await,
                        Err(RSpaceError::InterpreterError(
                            "native history: TypedRecord".to_owned()
                        )),
                    );
                    assert_eq!(session.store.entry_counts(), [0; 5]);
                }
            }
        }
        let payload =
            bincode::serialize(&vec![valid_schema_row(kind, "legacy")]).expect("encode legacy row");
        let (history, _) = framed_history::<LegacyOnly>(kind, Some(&payload), false).await;
        unsupported_query(&unsupported_session(history, EventCodecEpoch::Legacy), kind)
            .await
            .expect("legacy schema remains supported");
    }
}

#[tokio::test]
async fn empty_history_boundary_preserves_malformed_and_hash_failures() {
    for kind in [NativeLeafKind::Data, NativeLeafKind::Continuations, NativeLeafKind::Joins] {
        for (payload, corrupt_hash, fault) in
            [(vec![0_u8; 7], false, "Truncated"), (vec![0_u8; 8], true, "LeafHash")]
        {
            let (history, key) = framed_history::<String>(kind, Some(&payload), corrupt_hash).await;
            for codec in [EventCodecEpoch::Legacy, EventCodecEpoch::CostV1, EventCodecEpoch::CostV2]
            {
                let session = isolated_for_epoch(history.clone(), codec);
                let expected = RSpaceError::InterpreterError(format!("native history: {fault}"));
                let result = session.read_records::<UndecodedRecord>(kind, &key, |_| {
                    panic!("invalid history must not invoke a decoder")
                });
                assert_eq!(result.expect_err("invalid authenticated history"), expected);
                assert_eq!(boundary_query(&session, kind).await, Err(expected));
                assert_eq!(session.store.entry_counts(), [0; 5]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_nonempty_rows_keep_unsupported_epoch_decoding_closed(
        kind_index in 0usize..3,
        cost_v2 in any::<bool>(),
        values in prop::collection::vec("[a-z]{0,48}", 1..5),
    ) {
        let kind = [NativeLeafKind::Data, NativeLeafKind::Continuations, NativeLeafKind::Joins][kind_index];
        let codec = if cost_v2 { EventCodecEpoch::CostV2 } else { EventCodecEpoch::CostV1 };
        let rows: Vec<_> = values.iter().map(|value| valid_schema_row(kind, value)).collect();
        let payload = bincode::serialize(&rows).expect("encode generated records");
        let runtime = tokio::runtime::Builder::new_current_thread().build().expect("property runtime");
        runtime.block_on(async {
            let (history, _) = framed_history::<LegacyOnly>(kind, Some(&payload), false).await;
            let legacy = unsupported_session(history.clone(), EventCodecEpoch::Legacy);
            unsupported_query(&legacy, kind).await.expect("valid legacy schema");
            let session = unsupported_session(history, codec);
            prop_assert_eq!(
                unsupported_query(&session, kind).await,
                Err(RSpaceError::InterpreterError("native history: TypedRecord".to_owned())),
            );
            prop_assert_eq!(session.store.entry_counts(), [0; 5]);
            Ok(())
        })?;
    }

    #[test]
    fn continuation_projection_preserves_channel_order_and_duplicates(
        channels in prop::collection::vec("[a-z]{0,12}", 0..20),
    ) {
        let keys = GroupKeys::build(&channels).expect("group keys");
        let projection = channels_hash_from_keys(&keys.channels);
        let reference = hash_from_vec(&channels);
        prop_assert_eq!(projection.as_slice(), reference.0.as_slice());
    }
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
                let datum: crate::rspace::serializers::serializers::DatumOracleDe<String> =
                    bincode::deserialize(row).expect("value-shaped record oracle");
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
            assert_eq!(values[0].a.as_ref(), "retained");
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
            assert_eq!(values[0].continuation.as_ref(), "joined");
            assert_eq!(values[0].patterns.as_ref(), &["p".to_owned(), "p".to_owned()]);
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
        a: Arc::new("valid first row".to_owned()),
        persist: false,
        source: Produce::create(&"data", &"valid first row", false),
    };
    let rows = vec![crate::rspace::serializers::serializers::encode_datum(&datum), vec![255; 3]];
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

// Added by the merge of feature/cost-accounted-rho (finding R2-03): native
// replay decodes each history row under the codec epoch of its session, as
// play's `EpochHistoryReader` does (`history_reader.rs`). The test value has
// a different layout in each epoch, and each decoder checks the tag of its
// own layout, so a row read under another epoch fails at its first value.
const LEGACY_LAYOUT_TAG: u8 = 0xA0;
const COST_V1_LAYOUT_TAG: u8 = 0xC1;
const COST_V2_LAYOUT_TAG: u8 = 0xC2;

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
struct EpochLayout {
    text: String,
    cost: bool,
}

impl EpochLayout {
    fn ordinary(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cost: false,
        }
    }

    fn priced(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cost: true,
        }
    }

    fn tagged(&self, tag: u8, with_cost: bool) -> Vec<u8> {
        let mut bytes = vec![tag];
        bytes.extend(bincode::serialize(&self.text).expect("encode the text"));
        if with_cost {
            bytes.extend(bincode::serialize(&self.cost).expect("encode the cost flag"));
        }
        bytes
    }

    fn decode_tagged(
        bytes: &[u8],
        tag: u8,
        with_cost: bool,
    ) -> Result<(Self, usize), ColdStoreDecodeError> {
        match bytes.first() {
            Some(found) if *found == tag => {}
            found => {
                return Err(ColdStoreDecodeError::Legacy(format!(
                    "layout tag {found:?} is not {tag:#04x}"
                )));
            }
        }
        let (text, used) =
            crate::rspace::serializers::cold_store_decode::legacy_prefix::<String>(&bytes[1..])?;
        let consumed = 1 + used;
        match with_cost {
            false => Ok((Self { text, cost: false }, consumed)),
            true => {
                let (cost, used) = crate::rspace::serializers::cold_store_decode::legacy_prefix::<
                    bool,
                >(&bytes[consumed..])?;
                Ok((Self { text, cost }, consumed + used))
            }
        }
    }
}

impl StableHashSerialize for EpochLayout {
    fn stable_hash_bytes(&self) -> Vec<u8> { self.tagged(COST_V2_LAYOUT_TAG, true) }

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { self.tagged(LEGACY_LAYOUT_TAG, false) }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { self.tagged(COST_V1_LAYOUT_TAG, true) }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> { Ok(self.cost) }
}

impl ColdStoreDecode for EpochLayout {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Self::decode_tagged(bytes, COST_V2_LAYOUT_TAG, true)
    }

    fn cold_decode_legacy_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Self::decode_tagged(bytes, LEGACY_LAYOUT_TAG, false)
    }

    fn cold_decode_cost_v1_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Self::decode_tagged(bytes, COST_V1_LAYOUT_TAG, true)
    }
}

type LayoutSession = NativeReplaySession<EpochLayout, EpochLayout, EpochLayout, EpochLayout, Epoch>;

fn layout_session(history: History<EpochLayout>, codec: EventCodecEpoch) -> LayoutSession {
    let session = LayoutSession::new(history, Arc::new(Box::new(Matcher)), Epoch {
        codec: Some(codec),
        ..Epoch::default()
    })
    .expect("create an epoch-bound session");
    *session.space.store.write().expect("store lock") =
        Arc::new(HotStoreInstances::create_from_hr(Box::new(ForbiddenReader)));
    session
}

/// The history that a play space of `epoch` writes at its checkpoint. A
/// persistent datum rests on an ordinary channel, and a join waits on two
/// ordinary channels. In the two cost epochs, a datum and a join also use a
/// cost-bearing channel, whose key is domain-tagged and differs by epoch.
async fn epoch_history(epoch: EventCodecEpoch) -> History<EpochLayout> {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) =
        RSpace::<EpochLayout, EpochLayout, EpochLayout, EpochLayout>::create_with_replay_for_epoch(
            epoch,
            stores
                .r_space_stores()
                .await
                .expect("create history stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("create the play space");
    play.produce(EpochLayout::ordinary("data"), EpochLayout::ordinary("retained"), true)
        .await
        .expect("produce on an ordinary channel");
    play.consume(
        vec![EpochLayout::ordinary("left"), EpochLayout::ordinary("right")],
        vec![EpochLayout::ordinary("p"), EpochLayout::ordinary("p")],
        EpochLayout::ordinary("joined"),
        false,
        BTreeSet::new(),
    )
    .await
    .expect("consume on ordinary channels");
    match epoch {
        EventCodecEpoch::Legacy => {}
        EventCodecEpoch::CostV1 | EventCodecEpoch::CostV2 => {
            play.produce(EpochLayout::priced("cost-data"), EpochLayout::priced("priced"), true)
                .await
                .expect("produce on a cost-bearing channel");
            play.consume(
                vec![EpochLayout::priced("cost-left"), EpochLayout::ordinary("right")],
                vec![EpochLayout::priced("q"), EpochLayout::ordinary("q")],
                EpochLayout::priced("cost-joined"),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("consume on a cost-bearing channel");
        }
    }
    play.create_checkpoint()
        .await
        .expect("checkpoint the play space");
    play.get_history_repository()
}

/// The value that both readers decode, or `None` when both refuse. A
/// refusal by one reader alone fails the test.
fn agree<T: PartialEq + std::fmt::Debug>(
    native: Result<T, RSpaceError>,
    epoch_reader: Result<T, RSpaceError>,
    context: &str,
) -> Option<T> {
    match (native, epoch_reader) {
        (Ok(native), Ok(epoch_reader)) => {
            assert_eq!(native, epoch_reader, "{context}: the readers decode different values");
            Some(native)
        }
        (Err(_), Err(_)) => None,
        (native, epoch_reader) => {
            panic!(
                "{context}: one reader refused: native {native:?}, epoch reader {epoch_reader:?}"
            )
        }
    }
}

#[tokio::test]
async fn native_history_reads_agree_with_the_epoch_reader_in_every_epoch() {
    const EPOCHS: [EventCodecEpoch; 3] =
        [EventCodecEpoch::Legacy, EventCodecEpoch::CostV1, EventCodecEpoch::CostV2];
    let data_channel = EpochLayout::ordinary("data");
    let priced_channel = EpochLayout::priced("cost-data");
    let right = EpochLayout::ordinary("right");
    let priced_left = EpochLayout::priced("cost-left");
    let ordinary_group = vec![EpochLayout::ordinary("left"), right.clone()];
    let priced_group = vec![priced_left.clone(), right.clone()];
    for written in EPOCHS {
        let history = epoch_history(written).await;
        let root = history.root();
        for read in EPOCHS {
            let context = format!("written {written:?}, read {read:?}");
            let session = layout_session(history.clone(), read);
            let reader = history
                .get_history_reader_for_epoch(&root, read)
                .expect("an epoch reader");
            let data = agree(
                session.get_data(&data_channel).await,
                reader.get_data(&data_channel),
                &context,
            );
            let priced_data = agree(
                session.get_data(&priced_channel).await,
                reader.get_data(&priced_channel),
                &context,
            );
            let joins = agree(session.get_joins(&right).await, reader.get_joins(&right), &context);
            let priced_joins = agree(
                session.get_joins(&priced_left).await,
                reader.get_joins(&priced_left),
                &context,
            );
            let continuations = agree(
                session.get_continuations(&ordinary_group).await,
                reader.get_continuations(&ordinary_group),
                &context,
            );
            let priced_continuations = agree(
                session.get_continuations(&priced_group).await,
                reader.get_continuations(&priced_group),
                &context,
            );
            match (written == read, read) {
                (true, EventCodecEpoch::Legacy) => {
                    let data = data.expect("the Legacy datum decodes");
                    assert_eq!(data.len(), 1, "{context}");
                    assert_eq!(data[0].a.as_ref(), &EpochLayout::ordinary("retained"), "{context}");
                    assert!(data[0].persist, "{context}");
                    assert_eq!(joins, Some(vec![ordinary_group.clone()]), "{context}");
                    let continuations = continuations.expect("the Legacy continuation decodes");
                    assert_eq!(continuations.len(), 1, "{context}");
                    assert_eq!(
                        continuations[0].continuation.as_ref(),
                        &EpochLayout::ordinary("joined"),
                        "{context}"
                    );
                    assert!(
                        priced_data.is_none() &&
                            priced_joins.is_none() &&
                            priced_continuations.is_none(),
                        "{context}: Legacy refuses a cost-bearing key"
                    );
                }
                (true, _) => {
                    let data = data.expect("the datum decodes");
                    assert_eq!(data.len(), 1, "{context}");
                    assert_eq!(data[0].a.as_ref(), &EpochLayout::ordinary("retained"), "{context}");
                    let priced_data = priced_data.expect("the cost-bearing datum decodes");
                    assert_eq!(priced_data.len(), 1, "{context}");
                    assert_eq!(
                        priced_data[0].a.as_ref(),
                        &EpochLayout::priced("priced"),
                        "{context}"
                    );
                    let joins = joins.expect("the joins decode");
                    assert_eq!(joins.len(), 2, "{context}");
                    assert!(
                        joins.contains(&ordinary_group) && joins.contains(&priced_group),
                        "{context}"
                    );
                    assert_eq!(priced_joins, Some(vec![priced_group.clone()]), "{context}");
                    let continuations = continuations.expect("the continuation decodes");
                    assert_eq!(continuations.len(), 1, "{context}");
                    assert_eq!(
                        continuations[0].continuation.as_ref(),
                        &EpochLayout::ordinary("joined"),
                        "{context}"
                    );
                    let priced_continuations =
                        priced_continuations.expect("the cost-bearing continuation decodes");
                    assert_eq!(priced_continuations.len(), 1, "{context}");
                    assert_eq!(
                        priced_continuations[0].continuation.as_ref(),
                        &EpochLayout::priced("cost-joined"),
                        "{context}"
                    );
                }
                (false, _) => {
                    // Negative control: an ordinary key is the same in every
                    // epoch, so both readers find the rows and refuse their
                    // layout. A cost-bearing key differs by epoch, so it
                    // finds no row, and Legacy refuses it.
                    assert!(
                        data.is_none() && joins.is_none() && continuations.is_none(),
                        "{context}: a row written under another epoch must not decode"
                    );
                    match read {
                        EventCodecEpoch::Legacy => assert!(
                            priced_data.is_none() &&
                                priced_joins.is_none() &&
                                priced_continuations.is_none(),
                            "{context}: Legacy refuses a cost-bearing key"
                        ),
                        EventCodecEpoch::CostV1 | EventCodecEpoch::CostV2 => {
                            assert_eq!(priced_data, Some(Vec::new()), "{context}");
                            assert_eq!(priced_joins, Some(Vec::new()), "{context}");
                            assert_eq!(priced_continuations, Some(Vec::new()), "{context}");
                        }
                    }
                }
            }
        }
    }
}
