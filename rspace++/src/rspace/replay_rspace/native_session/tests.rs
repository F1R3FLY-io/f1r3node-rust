use std::sync::atomic::AtomicUsize;

use futures::FutureExt;
use proptest::prelude::*;

use super::*;
use crate::rspace::hashing::native_source::channel_key;
use crate::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use crate::rspace::history::native_reader::{NativeLeafKind, decode_datum};
use crate::rspace::replay_rspace::native_epoch::{
    NativeCandidateIdentity, NativeOperationEpoch, NativeOperationPublication,
    NativeOperationTicket, NativeReplayDecision, NativeReplayOutcome,
};
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::{ProduceCommitGuard, RSpaceOperationSource};
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

mod installation;
mod export;
mod history;

#[derive(Clone, Default)]
struct Epoch {
    codec: Option<EventCodecEpoch>,
    state: Arc<Mutex<(Vec<u64>, u64, bool, bool)>>,
    reject_restore: Arc<AtomicBool>,
    reject_complete: Arc<AtomicBool>,
    panic_publish: Arc<AtomicBool>,
    published: Arc<AtomicUsize>,
    pending: Arc<AtomicBool>,
}

struct Boundary(Epoch);
struct Restore(Boundary, usize);

impl NativeReplayEpoch for Epoch {
    type Boundary = Boundary;

    fn codec_epoch(&self) -> EventCodecEpoch { self.codec.unwrap_or(EventCodecEpoch::Legacy) }

    fn invalidate(&self) { self.state.lock().unwrap().3 = true; }

    fn denial(&self) -> RSpaceError { RSpaceError::OutOfPhlogistons }

    fn begin_boundary(&self) -> Result<Boundary, RSpaceError> {
        let mut state = self.state.lock().unwrap();
        if state.2 || state.3 {
            return Err(unavailable());
        }
        state.2 = true;
        Ok(Boundary(self.clone()))
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

impl<T: Clone> Match<T, T, T> for Matcher {
    fn get(&self, _: &T, datum: &T) -> Option<T> { Some(datum.clone()) }
}

type Session = NativeReplaySession<String, String, String, String, Epoch>;

struct StoredTicket {
    published: Arc<AtomicUsize>,
    pending: Arc<AtomicBool>,
    prepared: bool,
}

impl Drop for StoredTicket {
    fn drop(&mut self) {
        if !self.prepared {
            self.pending.store(false, Ordering::Release);
        }
    }
}

struct StoredPublication {
    published: Arc<AtomicUsize>,
    pending: Arc<AtomicBool>,
}

impl Drop for StoredPublication {
    fn drop(&mut self) { self.pending.store(false, Ordering::Release); }
}

impl NativeOperationPublication for StoredPublication {
    fn publish(&mut self) { self.published.fetch_add(1, Ordering::Relaxed); }
}

impl NativeOperationTicket<String, String, String, String> for StoredTicket {
    type Authority = ();
    type Publication = StoredPublication;

    fn outcome(&self) -> NativeReplayOutcome { NativeReplayOutcome::Stored }
    fn candidate_identity(&self) -> Option<&dyn NativeCandidateIdentity> { None }
    fn authenticate_footprint(
        &mut self,
        _: &[String],
        _: &[Vec<String>],
    ) -> Result<(), RSpaceError> {
        Ok(())
    }
    fn observe_produce(
        &mut self,
        _: &Produce,
        _: &String,
        _: &String,
        _: &(),
    ) -> Result<NativeReplayDecision, RSpaceError> {
        Ok(NativeReplayDecision::Granted)
    }
    fn observe_consume(
        &mut self,
        _: &Consume,
        _: &[String],
        _: &[String],
        _: &String,
        _: &BTreeSet<i32>,
        _: &(),
    ) -> Result<NativeReplayDecision, RSpaceError> {
        Err(RSpaceError::ProduceCommitDenied)
    }
    fn observe_comm(
        &mut self,
        _: &COMM,
        _: &String,
        _: bool,
        _: &[crate::rspace::internal::ConsumeCandidate<String, String>],
    ) -> Result<NativeReplayDecision, RSpaceError> {
        Err(RSpaceError::ProduceCommitDenied)
    }
    fn returned_produce(&self, source: &Produce) -> Result<Produce, RSpaceError> {
        Ok(source.clone())
    }
    fn prepare(mut self, outcome: NativeReplayOutcome) -> Result<Self::Publication, RSpaceError> {
        if outcome != NativeReplayOutcome::Stored {
            return Err(RSpaceError::ProduceCommitDenied);
        }
        self.prepared = true;
        Ok(StoredPublication {
            published: self.published.clone(),
            pending: self.pending.clone(),
        })
    }
}

#[async_trait::async_trait]
impl NativeOperationEpoch<String, String, String, String> for Epoch {
    type Ticket = StoredTicket;

    async fn wait_ready(&self, _: RSpaceOperationSource<'_>) -> Result<(), RSpaceError> { Ok(()) }
    fn begin_operation(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<Self::Ticket>, RSpaceError> {
        match source {
            RSpaceOperationSource::Produce(_) if !self.pending.swap(true, Ordering::AcqRel) => {
                Ok(Some(StoredTicket {
                    published: self.published.clone(),
                    pending: self.pending.clone(),
                    prepared: false,
                }))
            }
            RSpaceOperationSource::Produce(_) => Ok(None),
            RSpaceOperationSource::Consume(_) => Err(RSpaceError::ProduceCommitDenied),
            RSpaceOperationSource::Resting(_) => Err(RSpaceError::ProduceCommitDenied),
        }
    }
}

struct PublicationAuthority {
    allow: AtomicBool,
    calls: AtomicUsize,
}

impl ProduceCommitGuard for PublicationAuthority {
    fn with_commit(&self, commit: Box<dyn FnOnce() + '_>) -> Result<(), RSpaceError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if !self.allow.load(Ordering::Acquire) {
            return Err(RSpaceError::OutOfPhlogistons);
        }
        commit();
        Ok(())
    }
}

#[tokio::test]
async fn native_guard_refusal_keeps_store_counter_and_ticket_unpublished() {
    let session = session().await;
    let guard = PublicationAuthority {
        allow: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    };
    assert!(matches!(
        session
            .produce_with_authority_guarded(
                "c".to_owned(),
                "guarded".to_owned(),
                false,
                |_| Ok::<(), RSpaceError>(()),
                &guard,
            )
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    ));
    assert!(values(&session).await.is_empty());
    assert_eq!(counters(&session), 0);
    assert_eq!(session.epoch.published.load(Ordering::Relaxed), 0);
    assert!(!session.epoch.pending.load(Ordering::Acquire));
    assert_eq!(guard.calls.load(Ordering::Relaxed), 1);

    guard.allow.store(true, Ordering::Release);
    assert!(
        session
            .produce_with_authority_guarded(
                "c".to_owned(),
                "guarded".to_owned(),
                false,
                |_| Ok::<(), RSpaceError>(()),
                &guard,
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(values(&session).await, ["guarded"]);
    assert_eq!(counters(&session), 1);
    assert_eq!(session.epoch.published.load(Ordering::Relaxed), 1);
    assert!(!session.epoch.pending.load(Ordering::Acquire));
    assert_eq!(guard.calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn produce_update_checks_every_field() {
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

/// D-C2c (DR-96): the store key of a channel.
fn free_key(channel: &String) -> crate::rspace::hashing::native_source::StoreKey {
    channel_key(channel).expect("a channel key")
}

async fn put_at(session: &Session, channel: &str, value: &str) {
    let _shared = session.gate.read().await;
    session.ensure_open().unwrap();
    // Changed by D-C2c (D-S1, DR-96): the session's state is its digest-keyed
    // store, so the fixture fills the channel's entry and puts the datum
    // there, as a stored produce does.
    // session
    //     .space
    //     .produce(channel.into(), value.into(), false)
    //     .await
    //     .unwrap();
    let owned = channel.to_owned();
    let key = free_key(&owned);
    session
        .store
        .data_view(
            &owned,
            key,
            // Changed by D-C2d (D-S1, DR-96): the reader takes the key's digest.
            // &|| session.read_records(NativeLeafKind::Data, hash(&owned, &free)?, &free),
            &|| session.read_records(NativeLeafKind::Data, &key.0, decode_datum::<String>),
        )
        .unwrap();
    let datum = Datum::create(&owned, value.to_owned(), false);
    session
        .space
        .increment_produce_counter(&datum.source, false);
    // Changed by D-C2e (D-S1, DR-96): the publication takes the channel.
    // session.store.put_datum(key, datum, &free).unwrap();
    session.store.put_datum(&owned, key, datum).unwrap();
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
        .map(|datum| datum.a.as_ref().clone())
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

/// Changed by DR-131 S6: the test was
/// `restore_needs_no_new_host_work_and_close_rejects_every_access`. The
/// host-work checks went with host work. The test keeps its checks that the
/// restore returns to the checkpoint and that the close rejects every
/// access.
#[tokio::test]
async fn restore_returns_to_the_checkpoint_and_close_rejects_every_access() {
    let session = session().await;
    let root = session.checkpoint().await.unwrap();
    let second_root = session.checkpoint().await.unwrap();
    put(&session, "later").await;
    session.restore(root).await.unwrap();
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

/// Defect D1 and user decision 6 (2026-10-16) fixtures: admits a commit only
/// when the matched data equal `admitted`, in bind order, as a `where` guard
/// reads the matched data.
#[derive(Clone)]
struct AdmitsExactly(Vec<String>);

impl Match<String, String, String> for AdmitsExactly {
    fn get(&self, _: &String, datum: &String) -> Option<String> { Some(datum.clone()) }

    fn check_commit(&self, _: &String, matched: &[&String]) -> bool {
        matched.iter().copied().eq(self.0.iter())
    }

    /// Every commit compares the matched data with a fixed list, so every
    /// continuation can veto, as for a receive with a `where` guard. With
    /// DR-129's E3-a skip, the Stored check would skip such a continuation.
    fn commit_may_veto(&self, _: &String) -> bool { true }
}

/// Defect D1 fixtures: a session whose commit admits only `admitted`.
async fn guarded_session(admitted: &[&str]) -> Session {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a play space");
    let matcher = AdmitsExactly(admitted.iter().map(|value| (*value).to_owned()).collect());
    Session::new(play.get_history_repository(), Arc::new(Box::new(matcher)), Epoch::default())
        .expect("a native session")
}

/// Defect D1 fixtures: stores a waiting consume of `channels` in the session,
/// as a stored consume does.
async fn wait_at(session: &Session, channels: &[&str], continuation: &str) {
    let _shared = session.gate.read().await;
    session.ensure_open().expect("an open session");
    let channels: Vec<String> = channels
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect();
    let patterns = vec!["any".to_owned(); channels.len()];
    let continuation = continuation.to_owned();
    let (source, channel_keys) = session
        .consume_source(&channels, &patterns, &continuation, false)
        .expect("a consume source");
    let keys = GroupKeys::from_channel_keys(channel_keys);
    session
        .prefetch_continuations(&channels, &keys)
        .expect("a continuation prefetch");
    for (channel, channel_key) in channels.iter().zip(&keys.channels) {
        session
            .read_joins(channel, *channel_key)
            .expect("a join read");
    }
    session
        .store
        .store_consume(&channels, &keys, WaitingContinuation {
            patterns: Arc::new(patterns),
            continuation: Arc::new(continuation),
            persist: false,
            peeks: BTreeSet::new(),
            source,
        })
        .expect("a stored consume");
}

/// Defect D1 and user decision 6 (2026-10-16): a Stored ticket cannot hide a
/// guarded join that a produce completes only after the guard rejects the
/// first assignment. The join names `c` twice, `one` rests, and the produce
/// of `two` arrives. The incoming datum comes first, so the first assignment
/// is `[two, one]`, and the guard admits only `[one, two]`. The search finds
/// that assignment, so the session refuses the ticket and keeps its state.
/// With a guard that admits no assignment, the same ticket stores the datum.
#[tokio::test]
async fn stored_ticket_cannot_suppress_a_guard_backtrack_match() {
    for (admitted, refused) in [(&["one", "two"][..], true), (&[][..], false)] {
        let session = guarded_session(admitted).await;
        put_at(&session, "c", "one").await;
        wait_at(&session, &["c", "c"], "k").await;
        let before = counters(&session);
        let result = session
            .produce("c".to_owned(), "two".to_owned(), false, &())
            .await;
        let mut stored = values(&session).await;
        stored.sort();
        if refused {
            assert!(
                matches!(
                    &result,
                    Err(RSpaceError::InterpreterError(message))
                        if message == "native session outcome differs from actual execution"
                ),
                "the session accepted a Stored ticket for a match: {result:?}"
            );
            assert_eq!(stored, ["one"]);
            assert_eq!(counters(&session), before);
            assert_eq!(session.epoch.published.load(Ordering::Relaxed), 0);
        } else {
            assert!(
                result
                    .expect("the Stored ticket stores the datum")
                    .is_none(),
                "a guard that admits no assignment leaves the produce stored"
            );
            assert_eq!(stored, ["one", "two"]);
            assert_eq!(counters(&session), before + 1);
            assert_eq!(session.epoch.published.load(Ordering::Relaxed), 1);
        }
        assert!(!session.epoch.pending.load(Ordering::Acquire));
    }
}
