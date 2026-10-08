use std::mem::size_of;
use std::sync::atomic::AtomicBool;

use shared::rust::clone_backing::CloneBacking;
use tokio::sync::RwLock;

use super::native_epoch::{NativeReplayBoundary, NativeReplayEpoch, NativeReplayRestore};
use super::*;
use crate::rspace::hashing::native_source::{GroupKeys, StoreKey, channel_key};
use crate::rspace::hot_store::HotStoreState;
use crate::rspace::hot_store::native_store::{NativeHotStore, NativeStoreState};

#[cfg(test)]
mod backing;
mod publication;
mod operations;
mod result;
mod locks;
mod history;
mod installation;
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
    // Changed by D-C2c (D-S1, DR-96): the native session uses its
    // digest-keyed store.
    // state: HotStoreState<C, P, A, K>,
    state: NativeStoreState<C, P, A, K>,
    log: Log,
    counters: BTreeMap<Produce, i32>,
    waiting: i64,
}

pub struct NativeReplaySession<C, P: Clone, A: Clone, K: Clone, E> {
    space: ReplayRSpace<C, P, A, K>,
    /// D-C2c (D-S1, DR-96): the session's digest-keyed store.
    store: Arc<NativeHotStore<C, P, A, K>>,
    root: [u8; 32],
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

fn reserve_checkpoint_metadata<E: NativeReplayEpoch>(
    epoch: &E,
    log: &Log,
    counters: &BTreeMap<Produce, i32>,
) -> Result<(), RSpaceError> {
    let meter = |operations, scanned, backing| {
        epoch.reserve_comparison(operations, scanned)?;
        epoch.reserve_work(0, backing)
    };
    // Changed by D-O1 (DR-108): block accounting charges inline bytes
    // once per enclosing block.
    // crate::rspace::native_backing::reserve_copy_and_cleanup(log, &meter)?;
    // crate::rspace::native_backing::reserve_copy_and_cleanup(counters, &meter)
    crate::rspace::native_backing::reserve_blocks_copy_and_cleanup(log, &meter)?;
    crate::rspace::native_backing::reserve_blocks_copy_and_cleanup(counters, &meter)
}

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    E: NativeReplayEpoch,
{
    pub fn new(
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        epoch: E,
    ) -> Result<Self, RSpaceError> {
        let meter = |operations, scanned, backing| {
            epoch.reserve_comparison(operations, scanned)?;
            epoch.reserve_work(0, backing)
        };
        let reader = history.get_current_history_reader_native(&meter)?;
        meter(1, 0, 32)?;
        let root = reader
            .root()
            .0
            .try_into()
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        let base = reader.base_metered(&meter)?;
        let cache_shards =
            HotStoreInstances::native_cache_shards().ok_or(RSpaceError::HostWorkRejected)?;
        let (shards, bytes) = HotStoreState::<C, P, A, K>::snapshot_layout();
        let (store_operations, store_bytes) =
            HotStoreInstances::native_constructor_layout::<C, P, A, K>(cache_shards)
                .ok_or(RSpaceError::HostWorkRejected)?;
        let (lock_operations, lock_bytes) =
            crate::rspace::striped_locks::native::constructor_layout()
                .ok_or(RSpaceError::HostWorkRejected)?;
        let (replay_operations, replay_bytes) =
            ReplayRSpace::<C, P, A, K>::native_constructor_layout(cache_shards)
                .ok_or(RSpaceError::HostWorkRejected)?;
        let (native_store_operations, native_store_bytes) =
            NativeHotStore::<C, P, A, K>::constructor_layout()
                .ok_or(RSpaceError::HostWorkRejected)?;
        // Changed by D-C2c (D-S1, DR-96): the session also builds its
        // digest-keyed store.
        // let fixed_operations = store_operations
        //     .checked_add(lock_operations)
        //     .and_then(|value| value.checked_add(replay_operations))
        //     .and_then(|value| value.checked_add(2))
        //     .ok_or(RSpaceError::HostWorkRejected)?;
        let fixed_operations = store_operations
            .checked_add(lock_operations)
            .and_then(|value| value.checked_add(replay_operations))
            .and_then(|value| value.checked_add(native_store_operations))
            .and_then(|value| value.checked_add(2))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let operations = shards
            .checked_mul(2)
            .and_then(|value| value.checked_add(fixed_operations.checked_mul(2)?))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let wrapper_bytes =
            crate::rspace::native_backing::arc_allocation_bytes::<Box<dyn HotStore<C, P, A, K>>>()
                .and_then(|value| {
                    value.checked_add(crate::rspace::native_backing::arc_allocation_bytes::<()>()?)
                })
                .ok_or(RSpaceError::HostWorkRejected)?;
        let bytes = bytes
            .checked_add(store_bytes)
            .and_then(|value| value.checked_add(lock_bytes))
            .and_then(|value| value.checked_add(replay_bytes))
            .and_then(|value| value.checked_add(wrapper_bytes))
            .and_then(|value| value.checked_add(native_store_bytes))
            .ok_or(RSpaceError::HostWorkRejected)?;
        epoch.reserve_work(operations, bytes)?;
        let store = HotStoreInstances::create_from_hr_native(base, cache_shards);
        Ok(Self {
            space: ReplayRSpace::apply_native(history, Arc::new(store), matcher, cache_shards),
            store: Arc::new(NativeHotStore::new()),
            root,
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
            session.install(channels, install)?;
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

    /// The consume source and the store keys of its channels, in channel
    /// order (D-C2c, D-S1, DR-96).
    fn consume_source(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persist: bool,
    ) -> Result<(Consume, Vec<StoreKey>), RSpaceError>
    where
        E: super::native_epoch::NativeOperationEpoch<C, P, A, K>,
    {
        self.ensure_open()?;
        self.epoch
            .prepare_consume_source(channels, patterns, continuation)?;
        // Changed by D-C2c (D-S1, DR-96): the source also returns the keys of
        // its channels.
        // crate::rspace::hashing::native_source::consume(
        crate::rspace::hashing::native_source::consume_keys(
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
        // Changed by D-C2c (D-S1, DR-96): the native session uses its
        // digest-keyed store.
        // let (shards, bytes) = HotStoreState::<C, P, A, K>::snapshot_layout();
        let (shards, bytes) = NativeHotStore::<C, P, A, K>::snapshot_layout();
        let operations = shards
            .checked_mul(2)
            .and_then(|count| count.checked_add(16))
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.epoch.reserve_work(operations, bytes)?;
        let log = self.space.event_log.lock().expect("native replay log");
        let counters = self
            .space
            .produce_counter
            .lock()
            .expect("native replay counters");
        reserve_checkpoint_metadata(&self.epoch, &log, &counters)?;
        Ok(NativeSessionCheckpoint {
            identity: Arc::clone(&self.identity),
            epoch: boundary.checkpoint(),
            // Changed by D-C2c (D-S1, DR-96): the native session uses its
            // digest-keyed store.
            // state: self.space.get_store().snapshot(),
            state: self.store.snapshot(),
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
        // Changed by D-C2c (D-S1, DR-96): the native session uses its
        // digest-keyed store.
        // let store = self.space.get_store();
        let mut log = self.space.event_log.lock().expect("native replay log");
        let mut counters = self
            .space
            .produce_counter
            .lock()
            .expect("native replay counters");
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        // store.set_state(checkpoint.state);
        self.store.restore(checkpoint.state);
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
        let preparation =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        let prepared_result = (|| {
            // Changed by D-C2c (D-S1, DR-96): the native session uses its
            // digest-keyed store.
            // let changes =
            //     self.space
            //         .get_store()
            //         .changes_metered(&|operations, scanned, backing| {
            //             self.history_reserve(operations, scanned, backing)
            //         })?;
            // Changed by D-C3 (D-S3, DR-97): the export borrows cached values
            // and emits dirty keys only.
            // let changes = self.store.changes(&|operations, scanned, backing| {
            //     self.history_reserve(operations, scanned, backing)
            // })?;
            // let history = self.space.get_history_repository();
            // let prepared = history
            //     .prepare_native_checkpoint(changes, &|operations, scanned, backing| {
            //         self.history_reserve(operations, scanned, backing)
            //     })?;
            let reserve =
                |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
            let dirty = self.store.dirty_entries(&reserve)?;
            let changes = dirty.actions(&reserve)?;
            let history = self.space.get_history_repository();
            let prepared = history.prepare_native_checkpoint_borrowed(&changes, &reserve)?;
            let root = prepared
                .root
                .as_ref()
                .map_or(self.root.as_slice(), |root| root.0.as_slice());
            self.history_reserve(1, root.len(), root.len())?;
            let export_root = Blake2b256Hash::from_bytes(root.to_vec());
            Ok::<_, RSpaceError>((history, prepared, export_root))
        })();
        preparation.complete();
        let (history, prepared, root) = prepared_result?;
        let export = NativeReplayExport { root, evidence };
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        history.commit_native_checkpoint(prepared)?;
        self.unavailable.store(true, Ordering::Release);
        boundary.close();
        publication.complete();
        Ok(export)
    }

    pub async fn validate_produce_update(
        &self,
        original: &Produce,
        updated: &Produce,
    ) -> Result<(), RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        let meter = |operations, scanned, backing| {
            self.epoch.reserve_comparison(operations, scanned)?;
            self.epoch.reserve_work(0, backing)
        };
        if super::native_directive::metered_exact_produce(original, updated, &meter)? {
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
        self.get_data_prepared(channel, |operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })
        .await
    }

    pub async fn get_data_with_budget(
        &self,
        channel: &C,
        reserve: impl Fn(usize, usize) -> Result<(), RSpaceError> + Send + Sync,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.get_data_prepared(channel, |operations, scanned: usize, backing| {
            reserve(
                operations,
                scanned
                    .checked_add(backing)
                    .ok_or(RSpaceError::HostWorkRejected)?,
            )
        })
        .await
    }

    async fn get_data_prepared(
        &self,
        channel: &C,
        reserve: impl Fn(usize, usize, usize) -> Result<(), RSpaceError> + Send + Sync,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        reserve(1, 0, 0)?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // crate::rspace::native_backing::inspect(channel, &reserve)?;
        crate::rspace::native_backing::inspect_blocks(channel, &reserve)?;
        let hashes = [striped_locks::channel_hash(channel)];
        let _channels = self
            .consume_lock_with(&hashes, |operations, bytes| reserve(operations, 0, bytes))
            .await?;
        self.ensure_open()?;
        // Added by D-E1 (DR-108): a block inspection prepays one traversal.
        // The lock hash reads the channel once, and the store key encodes it
        // a second time.
        crate::rspace::native_backing::inspect_blocks(channel, &reserve)?;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the channel key.
        // self.read_data_with(channel, &reserve)
        let key = channel_key(channel, &reserve)?;
        self.read_data_with(channel, key, &reserve)
    }

    pub async fn get_joins(&self, channel: &C) -> Result<Vec<Vec<C>>, RSpaceError> {
        let _shared = self.gate.read().await;
        self.ensure_open()?;
        self.epoch.reserve_work(1, 0)?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // crate::rspace::native_backing::inspect(
        //     channel,
        //     &|operations, scanned, backing| {
        //         self.history_reserve(operations, scanned, backing)
        //     },
        // )?;
        crate::rspace::native_backing::inspect_blocks(channel, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })?;
        let hashes = [striped_locks::channel_hash(channel)];
        let _channels = self.consume_lock(&hashes).await?;
        self.ensure_open()?;
        // Added by D-E1 (DR-108): a block inspection prepays one traversal.
        // The lock hash reads the channel once, and the store key encodes it
        // a second time.
        crate::rspace::native_backing::inspect_blocks(channel, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })?;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the channel key.
        // self.read_joins(channel)
        let key = channel_key(channel, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })?;
        self.read_joins(channel, key)
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
        // Added by D-E1 (DR-108): a block inspection prepays one traversal.
        // The lock hashes read each channel once, and the group keys encode
        // each channel a second time.
        crate::rspace::native_backing::inspect_blocks_slice(
            channels,
            &|operations, scanned, backing| self.history_reserve(operations, scanned, backing),
        )?;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the group keys.
        // self.read_continuations(channels)
        let keys = GroupKeys::build(channels, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })?;
        self.read_continuations(channels, &keys)
    }
}

#[cfg(test)]
mod tests;
