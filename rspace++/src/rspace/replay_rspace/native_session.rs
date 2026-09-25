use std::mem::size_of;
use std::sync::atomic::AtomicBool;

use tokio::sync::RwLock;

use super::native_epoch::{NativeReplayBoundary, NativeReplayEpoch, NativeReplayRestore};
use super::*;
use crate::rspace::hot_store::HotStoreState;

mod backing;
mod publication;
mod operations;
mod result;
mod locks;
use backing::tree_backing;
use publication::PublicationGuard;

type EpochCheckpoint<E> = <<E as NativeReplayEpoch>::Boundary as NativeReplayBoundary>::Checkpoint;

pub struct NativeSessionCheckpoint<C, P, A, K, E>
where
    C: Eq + Hash,
    P: Clone,
    A: Clone,
    K: Clone,
    E: NativeReplayEpoch,
{
    identity: Arc<()>,
    epoch: EpochCheckpoint<E>,
    state: HotStoreState<C, P, A, K>,
    log: Log,
    counters: BTreeMap<Produce, i32>,
    waiting: i64,
}

pub struct NativeReplaySession<C, P, A, K, E> {
    space: ReplayRSpace<C, P, A, K>,
    epoch: E,
    identity: Arc<()>,
    gate: RwLock<()>,
    unavailable: AtomicBool,
}

#[derive(Debug)]
pub struct NativeReplayExport<E> {
    root: Blake2b256Hash,
    evidence: E,
}

impl<E> NativeReplayExport<E> {
    pub fn root(&self) -> &Blake2b256Hash { &self.root }

    pub fn evidence(&self) -> &E { &self.evidence }

    pub fn into_parts(self) -> (Blake2b256Hash, E) { (self.root, self.evidence) }
}

fn unavailable() -> RSpaceError {
    RSpaceError::InterpreterError("native replay session is closed or poisoned".to_owned())
}

fn foreign_checkpoint() -> RSpaceError {
    RSpaceError::InterpreterError("native replay checkpoint belongs to another session".to_owned())
}

fn backing<T>(count: usize) -> Result<usize, RSpaceError> {
    count.checked_mul(size_of::<T>()).ok_or_else(|| {
        RSpaceError::InterpreterError("native replay checkpoint size overflow".to_owned())
    })
}

fn reserve_produce<E: NativeReplayEpoch>(epoch: &E, produce: &Produce) -> Result<(), RSpaceError> {
    epoch.reserve_work(1, produce.channel_hash.0.len())?;
    epoch.reserve_work(1, produce.hash.0.len())?;
    epoch.reserve_work(
        produce.output_value.len(),
        backing::<Vec<u8>>(produce.output_value.len())?,
    )?;
    for bytes in &produce.output_value {
        epoch.reserve_work(1, bytes.len())?;
    }
    Ok(())
}

fn reserve_tree<K, V, E: NativeReplayEpoch>(epoch: &E, entries: usize) -> Result<(), RSpaceError> {
    let (operations, bytes) = tree_backing::<K, V>(entries).ok_or_else(|| {
        RSpaceError::InterpreterError("native replay checkpoint tree size overflow".to_owned())
    })?;
    epoch.reserve_work(operations, bytes)
}

fn reserve_consume<E: NativeReplayEpoch>(epoch: &E, consume: &Consume) -> Result<(), RSpaceError> {
    epoch.reserve_work(1, consume.hash.0.len())?;
    epoch.reserve_work(
        consume.channel_hashes.len(),
        backing::<Blake2b256Hash>(consume.channel_hashes.len())?,
    )?;
    for channel in &consume.channel_hashes {
        epoch.reserve_work(1, channel.0.len())?;
    }
    Ok(())
}

fn reserve_log<E: NativeReplayEpoch>(epoch: &E, log: &Log) -> Result<(), RSpaceError> {
    epoch.reserve_work(log.len(), backing::<Event>(log.len())?)?;
    for event in log {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => reserve_produce(epoch, produce)?,
            Event::IoEvent(IOEvent::Consume(consume)) => reserve_consume(epoch, consume)?,
            Event::Comm(comm) => {
                reserve_consume(epoch, &comm.consume)?;
                epoch
                    .reserve_work(comm.produces.len(), backing::<Produce>(comm.produces.len())?)?;
                for produce in &comm.produces {
                    reserve_produce(epoch, produce)?;
                }
                reserve_tree::<i32, (), _>(epoch, comm.peeks.len())?;
                reserve_tree::<Produce, i32, _>(epoch, comm.times_repeated.len())?;
                for produce in comm.times_repeated.keys() {
                    reserve_produce(epoch, produce)?;
                }
            }
        }
    }
    Ok(())
}

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    E: NativeReplayEpoch,
{
    pub fn new(
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        epoch: E,
    ) -> Result<Self, RSpaceError> {
        let reader = history.get_history_reader(&history.root())?;
        let store = HotStoreInstances::create_from_hr(reader.base());
        Ok(Self {
            space: ReplayRSpace::apply(history, Arc::new(store), matcher),
            epoch,
            identity: Arc::new(()),
            gate: RwLock::new(()),
            unavailable: AtomicBool::new(false),
        })
    }

    pub fn new_with_installs(
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        epoch: E,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
    ) -> Result<Self, RSpaceError> {
        let session = Self::new(history, matcher, epoch)?;
        for (channels, install) in installations {
            if channels.is_empty() || channels.len() != install.patterns.len() {
                return Err(RSpaceError::BugFoundError(
                    "native replay installation requires nonempty paired channels and patterns"
                        .to_owned(),
                ));
            }
            session.epoch.reserve_work(1, 0)?;
            let matched = session.space.locked_install_internal(
                channels,
                install.patterns,
                install.continuation,
            )?;
            if matched.is_some() {
                return Err(RSpaceError::BugFoundError(
                    "native replay installation cannot execute a COMM".to_owned(),
                ));
            }
        }
        Ok(session)
    }

    fn ensure_open(&self) -> Result<(), RSpaceError> {
        if self.unavailable.load(Ordering::Acquire) {
            Err(unavailable())
        } else {
            Ok(())
        }
    }

    fn produce_source(&self, channel: &C, data: &A, persist: bool) -> Result<Produce, RSpaceError>
    where E: super::native_epoch::NativeOperationEpoch<C, P, A, K> {
        self.ensure_open()?;
        self.epoch.prepare_produce_source(channel, data)?;
        crate::rspace::hashing::native_source::produce(
            channel,
            data,
            persist,
            &|operations, scanned, backing| {
                self.epoch.reserve_comparison(operations, scanned)?;
                self.epoch.reserve_work(0, backing)
            },
        )
    }

    fn consume_source(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persist: bool,
    ) -> Result<Consume, RSpaceError>
    where
        E: super::native_epoch::NativeOperationEpoch<C, P, A, K>,
    {
        self.ensure_open()?;
        self.epoch
            .prepare_consume_source(channels, patterns, continuation)?;
        crate::rspace::hashing::native_source::consume(
            channels,
            patterns,
            continuation,
            persist,
            &|operations, scanned, backing| {
                self.epoch.reserve_comparison(operations, scanned)?;
                self.epoch.reserve_work(0, backing)
            },
        )
    }

    pub async fn checkpoint(&self) -> Result<NativeSessionCheckpoint<C, P, A, K, E>, RSpaceError> {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        let boundary = self.epoch.begin_boundary()?;
        let (shards, bytes) = HotStoreState::<C, P, A, K>::snapshot_layout();
        self.epoch
            .reserve_work(shards.saturating_mul(2).saturating_add(16), bytes)?;
        let log = self.space.event_log.lock().expect("native replay log");
        let counters = self
            .space
            .produce_counter
            .lock()
            .expect("native replay counters");
        reserve_log(&self.epoch, &log)?;
        reserve_tree::<Produce, i32, _>(&self.epoch, counters.len())?;
        for produce in counters.keys() {
            reserve_produce(&self.epoch, produce)?;
        }
        Ok(NativeSessionCheckpoint {
            identity: Arc::clone(&self.identity),
            epoch: boundary.checkpoint(),
            state: self.space.get_store().snapshot(),
            log: log.clone(),
            counters: counters.clone(),
            waiting: self
                .space
                .replay_waiting_continuations_estimate
                .load(Ordering::Relaxed),
        })
    }

    pub async fn restore(
        &self,
        checkpoint: NativeSessionCheckpoint<C, P, A, K, E>,
    ) -> Result<(), RSpaceError> {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        if !Arc::ptr_eq(&self.identity, &checkpoint.identity) {
            return Err(foreign_checkpoint());
        }
        let restore = self
            .epoch
            .begin_boundary()?
            .prepare_restore(&checkpoint.epoch)?;
        let store = self.space.get_store();
        let mut log = self.space.event_log.lock().expect("native replay log");
        let mut counters = self
            .space
            .produce_counter
            .lock()
            .expect("native replay counters");
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        store.set_state(checkpoint.state);
        *log = checkpoint.log;
        *counters = checkpoint.counters;
        self.space
            .replay_waiting_continuations_estimate
            .store(checkpoint.waiting, Ordering::Relaxed);
        restore.publish();
        publication.complete();
        Ok(())
    }

    pub async fn check_complete(&self) -> Result<(), RSpaceError> {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        self.epoch.begin_boundary()?.check_complete()
    }

    pub async fn completed_usage(&self) -> Result<u64, RSpaceError> {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        self.epoch.begin_boundary()?.completed_usage()
    }

    pub async fn completed_evidence(
        &self,
    ) -> Result<<E::Boundary as NativeReplayBoundary>::Evidence, RSpaceError> {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        let boundary = self.epoch.begin_boundary()?;
        boundary.check_complete()?;
        boundary.completed_evidence()
    }

    pub async fn export(
        &self,
    ) -> Result<NativeReplayExport<<E::Boundary as NativeReplayBoundary>::Evidence>, RSpaceError>
    {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        let boundary = self.epoch.begin_boundary()?;
        boundary.check_complete()?;
        let evidence = boundary.completed_evidence()?;
        self.epoch.reserve_work(1, 0)?;
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        let history = self.space.get_history_repository();
        let changes = self.space.get_store().changes();
        let persisted = history.checkpoint(changes);
        let root = persisted.root();
        self.unavailable.store(true, Ordering::Release);
        boundary.close();
        publication.complete();
        Ok(NativeReplayExport { root, evidence })
    }

    pub async fn validate_produce_update(
        &self,
        original: &Produce,
        updated: &Produce,
    ) -> Result<(), RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        let items = original
            .output_value
            .len()
            .checked_add(updated.output_value.len())
            .and_then(|items| items.checked_add(8))
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.epoch.reserve_comparison(items, 0)?;
        for bytes in
            [&original.hash.0, &original.channel_hash.0, &updated.hash.0, &updated.channel_hash.0]
                .into_iter()
                .chain(original.output_value.iter())
                .chain(updated.output_value.iter())
        {
            self.epoch.reserve_comparison(1, bytes.len())?;
        }
        if super::native_directive::exact_produce(original, updated) {
            Ok(())
        } else {
            Err(RSpaceError::InterpreterError(
                "native replay external output differs from recorded output".to_owned(),
            ))
        }
    }

    pub async fn close(&self) -> Result<(), RSpaceError> {
        let _exclusive = self.gate.write().await;
        self.ensure_open()?;
        let boundary = self.epoch.begin_boundary()?;
        self.unavailable.store(true, Ordering::Release);
        boundary.close();
        Ok(())
    }

    pub async fn get_data(&self, channel: &C) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.get_data_with_budget(channel, |operations, bytes| {
            self.epoch.reserve_work(operations, bytes)
        })
        .await
    }

    pub async fn get_data_with_budget(
        &self,
        channel: &C,
        reserve: impl Fn(usize, usize) -> Result<(), RSpaceError> + Send + Sync,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        reserve(1, 0)?;
        let hashes = [striped_locks::channel_hash(channel)];
        let _channels = self.consume_lock_with(&hashes, &reserve).await?;
        self.ensure_open()?;
        Ok(self.space.get_store().get_data(channel))
    }

    pub async fn get_joins(&self, channel: &C) -> Result<Vec<Vec<C>>, RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        self.epoch.reserve_work(1, 0)?;
        let hashes = [striped_locks::channel_hash(channel)];
        let _channels = self.consume_lock(&hashes).await?;
        self.ensure_open()?;
        Ok(self.space.get_store().get_joins(channel))
    }

    pub async fn get_continuations(
        &self,
        channels: &[C],
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        let hashes = self.channel_hashes(channels, channels.len())?;
        let _channels = self.consume_lock(&hashes).await?;
        self.ensure_open()?;
        Ok(self.space.get_store().get_continuations(channels))
    }
}

#[cfg(test)]
mod tests;
