use std::cell::Cell;

use super::*;
use crate::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::native_reader::{
    NativeLeafKind, NativeReadCharge, NativeReadMeter, decode_history_record,
};
use crate::rspace::history::radix_tree::{Item, empty_node, encode};

struct ChargeCounter<'a> {
    operations: &'a Cell<usize>,
    scanned: &'a Cell<usize>,
}

impl NativeReadMeter for ChargeCounter<'_> {
    type Error = RSpaceError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        self.operations
            .set(self.operations.get() + charge.operations);
        self.scanned.set(self.scanned.get() + charge.scanned_bytes);
        Ok(())
    }
}

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

#[tokio::test]
async fn cold_typed_rows_prepay_cleanup_before_cache_handoff() {
    let history = retained_history().await;
    let session = isolated(history.clone());
    let channel = "data".to_owned();
    let projection = hash(&channel);
    let key: [u8; 32] = projection.0.as_slice().try_into().unwrap();
    let frame_operations = Cell::new(0);
    let frame_scanned = Cell::new(0);
    let decode_operations = Cell::new(0);
    let decode_scanned = Cell::new(0);
    let rows_count = Cell::new(0);
    history
        .native_history_reader(session.root)
        .with_records(
            NativeLeafKind::Data,
            &key,
            &ChargeCounter {
                operations: &frame_operations,
                scanned: &frame_scanned,
            },
            |rows| {
                rows_count.set(rows.len());
                for row in rows.iter() {
                    // Changed by D-S2 (DR-95): the session decodes its rows in
                    // History mode, so the reference decode uses it too.
                    // let datum: Datum<String> = decode_record(row, &ChargeCounter {
                    let datum: Datum<String> = decode_history_record(row, &ChargeCounter {
                        operations: &decode_operations,
                        scanned: &decode_scanned,
                    })
                    .unwrap();
                    assert_eq!(datum.a, "retained");
                }
                Ok::<_, RSpaceError>(())
            },
        )
        .unwrap()
        .unwrap();
    let actual_operations = Cell::new(0);
    let actual_scanned = Cell::new(0);
    let values = session
        .read_records::<Datum<String>>(
            NativeLeafKind::Data,
            projection,
            &|operations, scanned, _| {
                actual_operations.set(actual_operations.get() + operations);
                actual_scanned.set(actual_scanned.get() + scanned);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(values.len(), rows_count.get());
    assert!(decode_operations.get() > 0);
    assert!(decode_scanned.get() > 0);
    assert_eq!(
        actual_operations.get(),
        frame_operations.get() + 2 * rows_count.get() + 1 + 2 * decode_operations.get()
    );
    assert_eq!(actual_scanned.get(), frame_scanned.get() + 2 * decode_scanned.get());
    let limit = actual_scanned.get() - 1;
    let used = Cell::new(0usize);
    let rejected = session.read_records::<Datum<String>>(
        NativeLeafKind::Data,
        hash(&channel),
        &|_, scanned, _| {
            let next = used.get() + scanned;
            if next > limit {
                Err(RSpaceError::HostWorkRejected)
            } else {
                used.set(next);
                Ok(())
            }
        },
    );
    assert!(matches!(rejected, Err(RSpaceError::HostWorkRejected)));
    assert!(session.space.get_store().snapshot().data_flat().is_empty());
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
    assert!(session.space.get_store().snapshot().data_flat().is_empty());
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

#[tokio::test]
async fn metered_current_root_reader_preserves_legacy_projection_values() {
    let history = retained_history().await;
    let root = history.root();
    let calls = AtomicUsize::new(0);
    let native = history
        .get_current_history_reader_native(&|_, _, _| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .unwrap();
    let legacy = history.get_history_reader(&root).unwrap();
    assert_eq!(native.root(), root);
    assert!(calls.load(Ordering::Relaxed) > 0);
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

#[tokio::test]
async fn every_cold_read_reservation_cut_leaves_no_partial_typed_cache() {
    let history = retained_history().await;
    let root = history.root();
    for kind in 0..3 {
        let baseline = isolated(history.clone());
        let before = baseline.epoch.calls.load(Ordering::Relaxed);
        query(&baseline, kind).await.unwrap();
        let calls = baseline.epoch.calls.load(Ordering::Relaxed) - before;
        for accepted in 0..calls {
            let session = isolated(history.clone());
            *session.epoch.remaining_calls.lock().unwrap() = Some(accepted);
            assert!(query(&session, kind).await.is_err(), "kind={kind}, cut={accepted}");
            let state = session.space.get_store().snapshot();
            assert!(state.data_flat().is_empty());
            assert!(state.joins_flat().is_empty());
            assert!(state.continuations_flat().is_empty());
            assert_eq!(*session.epoch.state.lock().unwrap(), (vec![], 0, false, false));
            for lock in session
                .space
                .phase_a_locks
                .iter()
                .chain(session.space.phase_b_locks.iter())
            {
                assert!(lock.try_lock().is_ok());
            }
            *session.epoch.remaining_calls.lock().unwrap() = None;
            query(&session, kind).await.unwrap();
            assert_eq!(history.root(), root);
        }
    }
}

#[tokio::test]
async fn every_current_root_snapshot_reservation_cut_rejects_before_session_creation() {
    let history = retained_history().await;
    let root = history.root();
    let baseline_epoch = Epoch::default();
    let baseline =
        Session::new(history.clone(), Arc::new(Box::new(Matcher)), baseline_epoch.clone()).unwrap();
    let calls = baseline_epoch.calls.load(Ordering::Relaxed);
    assert!(calls > 1);
    assert!(baseline_epoch.bytes.load(Ordering::Relaxed) > 0);
    query(&baseline, 0).await.unwrap();
    for accepted in 0..calls {
        let epoch = Epoch::default();
        *epoch.remaining_calls.lock().unwrap() = Some(accepted);
        assert!(
            Session::new(history.clone(), Arc::new(Box::new(Matcher)), epoch).is_err(),
            "accepted={accepted}"
        );
        assert_eq!(history.root(), root);
    }
    let independent = isolated(history);
    for kind in 0..3 {
        query(&independent, kind).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_native_readers_keep_separate_caches_and_exhaustion() {
    let history = retained_history().await;
    let left = isolated(history.clone());
    let right = isolated(history);
    left.epoch.reject_work.store(true, Ordering::Release);
    let (failed, successful) = tokio::join!(query(&left, 0), query(&right, 0));
    assert!(failed.is_err());
    successful.unwrap();
    assert!(left.space.get_store().snapshot().data_flat().is_empty());
    assert_eq!(right.space.get_store().snapshot().data_flat().len(), 1);
    let inspection = Epoch::default();
    assert_eq!(
        left.get_data_with_budget(&"data".to_owned(), |operations, bytes| {
            inspection.reserve_work(operations, bytes)
        })
        .await
        .unwrap()[0]
            .a,
        "retained"
    );
    assert!(left.get_data(&"data".to_owned()).await.is_err());
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
        assert!(session.space.get_store().snapshot().data_flat().is_empty());
        assert_eq!(history.root(), root);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn metered_continuation_projection_preserves_channel_order_and_duplicates(
        channels in prop::collection::vec("[a-z]{0,12}", 0..20),
    ) {
        let epoch = Epoch::default();
        let actual = crate::rspace::hashing::native_source::channels_hash(&channels, &|operations, scanned, backing| {
            epoch.reserve_comparison(operations, scanned)?;
            epoch.reserve_work(0, backing)
        }).unwrap();
        prop_assert_eq!(actual, hash_from_vec(&channels));
    }
}
