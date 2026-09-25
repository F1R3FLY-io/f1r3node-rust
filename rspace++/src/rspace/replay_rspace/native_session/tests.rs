use std::sync::atomic::AtomicUsize;

use futures::FutureExt;
use proptest::prelude::*;

use super::*;
use crate::rspace::rspace::RSpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

mod installation;
mod export;

#[tokio::test]
async fn legacy_query_lock_path_has_no_native_reservation() {
    let session = session().await;
    let channel = "unpaid legacy query".to_owned();
    session.epoch.reject_work.store(true, Ordering::Release);
    let calls = session.epoch.calls.load(Ordering::Relaxed);
    let _channels = session
        .space
        .consume_lock(&[striped_locks::channel_hash(&channel)])
        .await;
    assert!(session.space.get_store().get_data(&channel).is_empty());
    assert_eq!(calls, session.epoch.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn explicit_read_budget_cannot_restore_execution_authority_or_bypass_closure() {
    let session = session().await;
    put(&session, "retained").await;
    session.epoch.reject_work.store(true, Ordering::Release);
    let state = session.epoch.state.lock().unwrap().clone();
    let calls = session.epoch.calls.load(Ordering::Relaxed);
    let channel = "c".to_owned();
    assert!(session.get_data(&channel).await.is_err());
    assert!(
        session
            .get_data_with_budget(&channel, |_, _| Err(RSpaceError::HostWorkRejected))
            .await
            .is_err()
    );
    let inspection = Epoch::default();
    let data = session
        .get_data_with_budget(&channel, |operations, bytes| {
            inspection.reserve_work(operations, bytes)
        })
        .await
        .unwrap();
    assert_eq!(data.into_iter().map(|datum| datum.a).collect::<Vec<_>>(), ["retained"]);
    assert_eq!(state, *session.epoch.state.lock().unwrap());
    assert_eq!(calls, session.epoch.calls.load(Ordering::Relaxed));
    assert!(session.epoch.reject_work.load(Ordering::Acquire));
    assert!(inspection.calls.load(Ordering::Relaxed) > 0);
    assert!(inspection.bytes.load(Ordering::Relaxed) > 0);
    assert!(session.get_data(&channel).await.is_err());
    session.close().await.unwrap();
    assert!(
        session
            .get_data_with_budget(&channel, |_, _| Ok(()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn query_lock_preparation_rejects_exhausted_host_budget() {
    let session = session().await;
    let channel = "unpaid query".to_owned();
    session.epoch.reject_work.store(true, Ordering::Release);
    assert!(session.get_data(&channel).await.is_err());
    assert!(session.get_joins(&channel).await.is_err());
    assert!(session.get_continuations(&[channel]).await.is_err());
    assert!(
        session
            .space
            .phase_a_locks
            .iter()
            .all(|lock| lock.try_lock().is_ok())
    );
    assert!(
        session
            .space
            .phase_b_locks
            .iter()
            .all(|lock| lock.try_lock().is_ok())
    );
}

#[tokio::test]
async fn every_two_phase_lock_preparation_cut_releases_all_guards() {
    let session = session().await;
    let channel = "two phase preparation".to_owned();
    for produce in [false, true] {
        let before = session.epoch.calls.load(Ordering::Relaxed);
        let result = if produce {
            session.produce_lock(&channel).await
        } else {
            session.consume_lock(&[0, 1, 1]).await
        };
        drop(result.unwrap());
        let calls = session.epoch.calls.load(Ordering::Relaxed) - before;
        for accepted in 0..calls {
            *session.epoch.remaining_calls.lock().unwrap() = Some(accepted);
            let result = if produce {
                session.produce_lock(&channel).await
            } else {
                session.consume_lock(&[0, 1, 1]).await
            };
            assert!(result.is_err(), "produce={produce}, cut={accepted}");
            assert!(
                session
                    .space
                    .phase_a_locks
                    .iter()
                    .all(|lock| lock.try_lock().is_ok())
            );
            assert!(
                session
                    .space
                    .phase_b_locks
                    .iter()
                    .all(|lock| lock.try_lock().is_ok())
            );
        }
        *session.epoch.remaining_calls.lock().unwrap() = None;
    }
}

#[derive(Clone, Default)]
struct Epoch {
    state: Arc<Mutex<(Vec<u64>, u64, bool, bool)>>,
    reject_work: Arc<AtomicBool>,
    reject_restore: Arc<AtomicBool>,
    reject_complete: Arc<AtomicBool>,
    panic_publish: Arc<AtomicBool>,
    work: Arc<AtomicUsize>,
    bytes: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    remaining_calls: Arc<Mutex<Option<usize>>>,
}

struct Boundary(Epoch);
struct Restore(Boundary, usize);

impl NativeReplayEpoch for Epoch {
    type Boundary = Boundary;

    fn invalidate(&self) { self.state.lock().unwrap().3 = true; }

    fn begin_boundary(&self) -> Result<Boundary, RSpaceError> {
        let mut state = self.state.lock().unwrap();
        if state.2 || state.3 {
            return Err(unavailable());
        }
        state.2 = true;
        Ok(Boundary(self.clone()))
    }

    fn reserve_work(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError> {
        if self.reject_work.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        if let Some(remaining) = self.remaining_calls.lock().unwrap().as_mut() {
            if *remaining == 0 {
                return Err(unavailable());
            }
            *remaining -= 1;
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.work.fetch_add(operations, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(())
    }

    fn reserve_comparison(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError> {
        self.reserve_work(operations, bytes)
    }
}

impl NativeReplayBoundary for Boundary {
    type Checkpoint = (usize, u64);
    type Restore = Restore;
    type Evidence = u64;

    fn checkpoint(&self) -> Self::Checkpoint {
        let state = self.0.state.lock().unwrap();
        (state.0.len(), state.0.last().copied().unwrap_or(0))
    }

    fn prepare_restore(self, token: &Self::Checkpoint) -> Result<Restore, RSpaceError> {
        if self.0.reject_restore.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        let actual = {
            let state = self.0.state.lock().unwrap();
            if token.0 == 0 {
                Some(0)
            } else {
                state.0.get(token.0 - 1).copied()
            }
        };
        if actual != Some(token.1) {
            return Err(foreign_checkpoint());
        }
        Ok(Restore(self, token.0))
    }

    fn check_complete(&self) -> Result<(), RSpaceError> {
        if self.0.reject_complete.load(Ordering::Acquire) {
            Err(unavailable())
        } else {
            Ok(())
        }
    }

    fn completed_usage(&self) -> Result<u64, RSpaceError> {
        Ok(self.0.state.lock().unwrap().0.iter().sum())
    }

    fn completed_evidence(&self) -> Result<Self::Evidence, RSpaceError> { self.completed_usage() }

    fn close(self) { self.0.state.lock().unwrap().3 = true; }
}

impl Drop for Boundary {
    fn drop(&mut self) { self.0.state.lock().unwrap().2 = false; }
}

impl NativeReplayRestore for Restore {
    fn publish(self) {
        assert!(!self.0.0.panic_publish.load(Ordering::Acquire), "injected publication panic");
        self.0.0.state.lock().unwrap().0.truncate(self.1);
    }
}

#[derive(Clone)]
struct Matcher;

impl Match<String, String, String> for Matcher {
    fn get(&self, _: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
}

type Session = NativeReplaySession<String, String, String, String, Epoch>;

#[tokio::test]
async fn produce_update_checks_every_field_and_precharges_comparisons() {
    let session = session().await;
    let original =
        Produce::create(&"c", &"data", false).mark_as_non_deterministic(vec![vec![1, 2]]);
    session
        .validate_produce_update(&original, &original)
        .await
        .unwrap();
    for field in 0..6 {
        let mut changed = original.clone();
        match field {
            0 => changed.hash = Produce::create(&"other", &"data", false).hash,
            1 => changed.channel_hash = Produce::create(&"other", &"data", false).channel_hash,
            2 => changed.persistent = !changed.persistent,
            3 => changed.is_deterministic = !changed.is_deterministic,
            4 => changed.failed = !changed.failed,
            _ => changed.output_value[0][0] ^= 1,
        }
        if field != 0 {
            assert_eq!(original, changed);
        }
        assert!(
            session
                .validate_produce_update(&original, &changed)
                .await
                .is_err()
        );
    }
    let before = session.epoch.calls.load(Ordering::Relaxed);
    session
        .validate_produce_update(&original, &original)
        .await
        .unwrap();
    let calls = session.epoch.calls.load(Ordering::Relaxed) - before;
    assert!(calls > 1);
    for accepted in 0..calls {
        *session.epoch.remaining_calls.lock().unwrap() = Some(accepted);
        assert!(
            session
                .validate_produce_update(&original, &original)
                .await
                .is_err()
        );
        assert!(session.epoch.state.lock().unwrap().0.is_empty());
    }
    *session.epoch.remaining_calls.lock().unwrap() = None;
    session.close().await.unwrap();
    assert!(
        session
            .validate_produce_update(&original, &original)
            .await
            .is_err()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn producer_output_authentication_matches_exact_payloads(
        expected in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..64), 0..8),
        actual in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..64), 0..8),
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let session = session().await;
            let original = Produce::create(&"c", &"data", false).mark_as_non_deterministic(expected.clone());
            let updated = Produce { output_value: actual.clone(), ..original.clone() };
            assert_eq!(session.validate_produce_update(&original, &updated).await.is_ok(), expected == actual);
        });
    }
}

async fn session() -> Session {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    Session::new(play.get_history_repository(), Arc::new(Box::new(Matcher)), Epoch::default())
        .unwrap()
}

async fn put(session: &Session, value: &str) { put_at(session, "c", value).await; }

async fn put_at(session: &Session, channel: &str, value: &str) {
    let _shared = session.gate.read().await;
    session.ensure_open().unwrap();
    session
        .space
        .produce(channel.into(), value.into(), false)
        .await
        .unwrap();
    session
        .space
        .event_log
        .lock()
        .unwrap()
        .push(Event::IoEvent(IOEvent::Produce(Produce::create(
            &channel.to_owned(),
            &value.to_owned(),
            false,
        ))));
    let mut state = session.epoch.state.lock().unwrap();
    assert!(!state.2 && !state.3);
    state.1 += 1;
    let serial = state.1;
    state.0.push(serial);
}

async fn values(session: &Session) -> Vec<String> {
    session
        .get_data(&"c".into())
        .await
        .unwrap()
        .into_iter()
        .map(|datum| datum.a)
        .collect()
}

fn counters(session: &Session) -> i32 {
    session.space.produce_counter.lock().unwrap().values().sum()
}

#[tokio::test]
async fn checkpoint_preserves_counters_and_restores_store_log_and_ledger_together() {
    let session = session().await;
    put(&session, "first").await;
    assert_eq!(session.space.event_log.lock().unwrap().len(), 1);
    let before_log = format!("{:?}", session.space.event_log.lock().unwrap());
    let checkpoint = session.checkpoint().await.unwrap();
    assert_eq!(counters(&session), 1);
    assert_eq!(format!("{:?}", session.space.event_log.lock().unwrap()), before_log);
    put(&session, "second").await;
    assert_eq!(values(&session).await, ["second", "first"]);
    session.restore(checkpoint).await.unwrap();
    assert_eq!(values(&session).await, ["first"]);
    assert_eq!(counters(&session), 1);
    assert_eq!(session.epoch.state.lock().unwrap().0.len(), 1);
    assert_eq!(format!("{:?}", session.space.event_log.lock().unwrap()), before_log);
}

#[tokio::test]
async fn foreign_and_abandoned_checkpoints_preserve_both_current_states() {
    let session = session().await;
    let other = self::session().await;
    let root = session.checkpoint().await.unwrap();
    put(&session, "abandoned").await;
    let abandoned = session.checkpoint().await.unwrap();
    session.restore(root).await.unwrap();
    put(&session, "current").await;
    assert!(session.restore(abandoned).await.is_err());
    let foreign = other.checkpoint().await.unwrap();
    assert!(session.restore(foreign).await.is_err());
    assert_eq!(values(&session).await, ["current"]);
    assert_eq!(counters(&session), 1);
    assert_eq!(session.epoch.state.lock().unwrap().0.len(), 1);
}

#[tokio::test]
async fn rejected_or_cancelled_preparation_preserves_state_and_releases_the_gate() {
    let session = session().await;
    let failed = session.checkpoint().await.unwrap();
    let cancelled = session.checkpoint().await.unwrap();
    put(&session, "current").await;
    session.epoch.reject_restore.store(true, Ordering::Release);
    assert!(session.restore(failed).await.is_err());
    session.epoch.reject_restore.store(false, Ordering::Release);
    let shared = session.gate.read().await;
    assert!(session.restore(cancelled).now_or_never().is_none());
    drop(shared);
    assert_eq!(values(&session).await, ["current"]);
    assert_eq!(session.epoch.state.lock().unwrap().0.len(), 1);
    session.checkpoint().await.unwrap();
}

#[tokio::test]
async fn capture_waits_for_all_shared_operation_leases() {
    let session = session().await;
    let first = session.gate.read().await;
    let second = session.gate.read().await;
    let mut capture = Box::pin(session.checkpoint());
    assert!(capture.as_mut().now_or_never().is_none());
    drop(first);
    assert!(capture.as_mut().now_or_never().is_none());
    drop(second);
    capture.await.unwrap();
}

#[tokio::test]
async fn each_checkpoint_reservation_failure_preserves_live_state() {
    let session = session().await;
    put(&session, "first").await;
    put(&session, "second").await;
    let log = format!("{:?}", session.space.event_log.lock().unwrap());
    let before = session.epoch.calls.load(Ordering::Relaxed);
    let checkpoint = session.checkpoint().await.unwrap();
    let reservations = session.epoch.calls.load(Ordering::Relaxed) - before;
    assert!(reservations > 1);
    for accepted in 0..reservations {
        *session.epoch.remaining_calls.lock().unwrap() = Some(accepted);
        assert!(session.checkpoint().await.is_err());
        assert!(session.get_data(&"c".into()).await.is_err());
        *session.epoch.remaining_calls.lock().unwrap() = None;
        assert_eq!(values(&session).await, ["second", "first"]);
        assert_eq!(counters(&session), 2);
        assert_eq!(format!("{:?}", session.space.event_log.lock().unwrap()), log);
        let ledger = session.epoch.state.lock().unwrap();
        assert_eq!(ledger.0.len(), 2);
        assert!(!ledger.2);
    }
    *session.epoch.remaining_calls.lock().unwrap() = None;
    session.restore(checkpoint).await.unwrap();
    session.checkpoint().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_capture_keeps_tuple_metadata_and_ledger_on_the_same_boundary() {
    for shared_channel in [false, true] {
        for _ in 0..16 {
            let session = Arc::new(session().await);
            let start = Arc::new(tokio::sync::Barrier::new(3));
            let mut workers = Vec::new();
            for index in 0..2 {
                let session = Arc::clone(&session);
                let start = Arc::clone(&start);
                workers.push(tokio::spawn(async move {
                    start.wait().await;
                    let channel = if shared_channel || index == 0 {
                        "c"
                    } else {
                        "d"
                    };
                    put_at(&session, channel, &index.to_string()).await;
                }));
            }
            start.wait().await;
            tokio::task::yield_now().await;
            let checkpoint = session.checkpoint().await.unwrap();
            let expected_count: i32 = checkpoint.counters.values().sum();
            for worker in workers {
                worker.await.unwrap();
            }
            assert_eq!(counters(&session), 2);
            session.restore(checkpoint).await.unwrap();
            let c = session.get_data(&"c".into()).await.unwrap();
            let d = session.get_data(&"d".into()).await.unwrap();
            assert_eq!(c.len() + d.len(), expected_count as usize);
            assert_eq!(counters(&session), expected_count);
            assert_eq!(session.epoch.state.lock().unwrap().0.len(), expected_count as usize);
            assert_eq!(session.space.event_log.lock().unwrap().len(), expected_count as usize);
        }
    }
}

#[tokio::test]
async fn restore_needs_no_new_host_work_and_close_rejects_every_access() {
    let session = session().await;
    let root = session.checkpoint().await.unwrap();
    let second_root = session.checkpoint().await.unwrap();
    put(&session, "later").await;
    session.epoch.reject_work.store(true, Ordering::Release);
    assert!(session.checkpoint().await.is_err());
    let work = session.epoch.work.load(Ordering::Relaxed);
    session.restore(root).await.unwrap();
    assert_eq!(work, session.epoch.work.load(Ordering::Relaxed));
    assert!(session.get_data(&"c".into()).await.is_err());
    session.epoch.reject_work.store(false, Ordering::Release);
    assert!(values(&session).await.is_empty());
    session.check_complete().await.unwrap();
    session.close().await.unwrap();
    assert!(session.get_data(&"c".into()).await.is_err());
    assert!(session.restore(second_root).await.is_err());
    assert!(session.checkpoint().await.is_err());
    assert!(session.check_complete().await.is_err());
    assert!(session.close().await.is_err());
}

#[tokio::test]
async fn publication_unwind_permanently_closes_the_session() {
    let session = session().await;
    let root = session.checkpoint().await.unwrap();
    put(&session, "later").await;
    session.epoch.panic_publish.store(true, Ordering::Release);
    assert!(
        std::panic::AssertUnwindSafe(session.restore(root))
            .catch_unwind()
            .await
            .is_err()
    );
    assert!(session.get_data(&"c".into()).await.is_err());
    assert!(session.checkpoint().await.is_err());
    assert!(session.check_complete().await.is_err());
}

#[test]
fn publication_unwind_closes_before_ticket_cleanup() {
    struct Ticket<'a>(&'a AtomicBool);
    impl Drop for Ticket<'_> {
        fn drop(&mut self) {
            assert!(self.0.load(Ordering::Acquire));
        }
    }
    let closed = AtomicBool::new(false);
    let outcome = std::panic::catch_unwind(|| {
        let _ticket = Ticket(&closed);
        let _publication = PublicationGuard::new(&closed);
        panic!("publication failed");
    });
    assert!(outcome.is_err());
    assert!(closed.load(Ordering::Acquire));
    PublicationGuard::new(&closed).complete();
    assert!(closed.load(Ordering::Acquire));
}

proptest! {
    #[test]
    fn generated_publication_histories_preserve_closure(
        initially_closed in any::<bool>(),
        aborts in prop::collection::vec(any::<bool>(), 0..512),
    ) {
        let closed = AtomicBool::new(initially_closed);
        let mut expected = initially_closed;
        for aborted in aborts {
            let publication = PublicationGuard::new(&closed);
            if aborted { drop(publication); } else { publication.complete(); }
            expected |= aborted;
            prop_assert_eq!(closed.load(Ordering::Acquire), expected);
        }
    }
}

#[test]
fn metadata_reservation_includes_every_source_and_output_payload() {
    let epoch = Epoch::default();
    let mut produce = Produce::create(&"c", &"v", false);
    produce.output_value = vec![vec![], vec![1; 31], vec![2; 7]];
    let consume = Consume::create(&vec!["c"], &vec!["p"], &"k", false);
    let comm = COMM {
        consume,
        produces: vec![produce.clone()],
        peeks: BTreeSet::from([0]),
        times_repeated: BTreeMap::from([(produce.clone(), 3)]),
    };
    let log = vec![Event::IoEvent(IOEvent::Produce(produce.clone())), Event::Comm(comm)];
    reserve_log(&epoch, &log).unwrap();
    let bytes = epoch.bytes.load(Ordering::Relaxed);
    let smaller = Epoch::default();
    let mut changed = log;
    let Event::IoEvent(IOEvent::Produce(p)) = &mut changed[0] else {
        unreachable!()
    };
    p.output_value[1].pop();
    reserve_log(&smaller, &changed).unwrap();
    assert_eq!(bytes - smaller.bytes.load(Ordering::Relaxed), 1);
    assert!(bytes >= 3 * (64 + 38 + 3 * size_of::<Vec<u8>>()));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_checkpoint_histories_preserve_the_exact_tuple_prefix(commands in prop::collection::vec(0u8..4, 1..60)) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let session = session().await;
            let mut expected = Vec::new();
            let mut checkpoints = Vec::new();
            for (index, command) in commands.into_iter().enumerate() {
                match command {
                    0 | 1 => {
                        let value = index.to_string();
                        put(&session, &value).await;
                        expected.push(value);
                    }
                    2 => checkpoints.push((session.checkpoint().await.unwrap(), expected.clone())),
                    _ => {
                        if let Some((checkpoint, prefix)) = checkpoints.pop() {
                            let valid = expected.starts_with(&prefix);
                            prop_assert_eq!(session.restore(checkpoint).await.is_ok(), valid);
                            if valid { expected = prefix; }
                        }
                    }
                }
                prop_assert_eq!(values(&session).await, expected.iter().rev().cloned().collect::<Vec<_>>());
                prop_assert_eq!(counters(&session) as usize, expected.len());
                prop_assert_eq!(session.epoch.state.lock().unwrap().0.len(), expected.len());
                prop_assert_eq!(session.space.event_log.lock().unwrap().len(), expected.len());
            }
            Ok(())
        })?;
    }
}

#[derive(Clone, Default)]
struct CountAlloc(Arc<AtomicUsize>);

unsafe impl std::alloc::Allocator for CountAlloc {
    fn allocate(
        &self,
        layout: std::alloc::Layout,
    ) -> Result<std::ptr::NonNull<[u8]>, std::alloc::AllocError> {
        let allocation = std::alloc::Global.allocate(layout)?;
        self.0.fetch_add(layout.size(), Ordering::Relaxed);
        Ok(allocation)
    }

    unsafe fn deallocate(&self, ptr: std::ptr::NonNull<u8>, layout: std::alloc::Layout) {
        unsafe { std::alloc::Global.deallocate(ptr, layout) }
    }
}

#[tokio::test]
async fn counter_snapshot_reservation_covers_actual_tree_allocation() {
    let session = session().await;
    let before = session.epoch.bytes.load(Ordering::Relaxed);
    let empty = session.checkpoint().await.unwrap();
    let empty_bytes = session.epoch.bytes.load(Ordering::Relaxed) - before;
    let produce = Produce::create(&"c", &"v", false);
    session
        .space
        .produce_counter
        .lock()
        .unwrap()
        .insert(produce.clone(), 1);
    let before = session.epoch.bytes.load(Ordering::Relaxed);
    let populated = session.checkpoint().await.unwrap();
    let populated_bytes = session.epoch.bytes.load(Ordering::Relaxed) - before;
    let allocator = CountAlloc::default();
    let mut map = BTreeMap::new_in(allocator.clone());
    map.insert(produce.clone(), 1_i32);
    allocator.0.store(0, Ordering::Relaxed);
    let copied = map.clone();
    let actual =
        allocator.0.load(Ordering::Relaxed) + produce.channel_hash.0.len() + produce.hash.0.len();
    assert!(
        populated_bytes - empty_bytes >= actual,
        "reserved {} bytes but cloned at least {actual} bytes",
        populated_bytes - empty_bytes
    );
    drop((empty, populated, copied));
}

fn assert_tree_clone_bound<K: Clone + Ord, V: Clone>(
    map: &BTreeMap<K, V, CountAlloc>,
    allocator: &CountAlloc,
) {
    allocator.0.store(0, Ordering::Relaxed);
    let copied = map.clone();
    let actual = allocator.0.load(Ordering::Relaxed);
    let (_, reserved) = tree_backing::<K, V>(map.len()).unwrap();
    assert!(
        actual <= reserved,
        "{actual} allocated bytes exceed {reserved} reserved bytes for {} entries",
        map.len()
    );
    drop(copied);
}

#[test]
fn tree_backing_covers_node_splits_merges_and_aligned_keys() {
    #[repr(align(128))]
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Aligned(u64);

    let allocator = CountAlloc::default();
    for count in [0, 1, 2, 5, 11, 12, 64, 512, 4096] {
        let mut map = BTreeMap::new_in(allocator.clone());
        for key in 0..count {
            map.insert(Aligned(key), [0_u8; 33]);
        }
        assert_tree_clone_bound(&map, &allocator);
        for key in (0..count).step_by(2) {
            map.remove(&Aligned(key));
        }
        assert_tree_clone_bound(&map, &allocator);
        map.clear();
        assert_tree_clone_bound(&map, &allocator);
    }
}

#[test]
fn tree_backing_rejects_overflow_before_any_reservation() {
    let epoch = Epoch::default();
    assert!(reserve_tree::<Produce, i32, _>(&epoch, usize::MAX).is_err());
    assert_eq!(epoch.work.load(Ordering::Relaxed), 0);
    assert_eq!(epoch.bytes.load(Ordering::Relaxed), 0);
    assert_eq!(tree_backing::<Produce, i32>(0), Some((0, 0)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn generated_tree_shapes_fit_reserved_backing(commands in prop::collection::vec((any::<u16>(), any::<bool>()), 0..500)) {
        let allocator = CountAlloc::default();
        let mut map = BTreeMap::new_in(allocator.clone());
        for (key, insert) in commands {
            if insert { map.insert(key, key as i32); } else { map.remove(&key); }
            assert_tree_clone_bound(&map, &allocator);
        }
    }

    #[test]
    fn tree_backing_is_monotone_and_covers_clone_and_drop(entries in 0usize..100_000, extra in 0usize..100_000) {
        let (work, bytes) = tree_backing::<Produce, i32>(entries).unwrap();
        let (larger_work, larger_bytes) = tree_backing::<Produce, i32>(entries + extra).unwrap();
        prop_assert!(work <= larger_work);
        prop_assert!(bytes <= larger_bytes);
        prop_assert!(work >= 2 * entries);
        prop_assert_eq!(bytes == 0, entries == 0);
    }
}
