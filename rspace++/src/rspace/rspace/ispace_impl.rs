// The ISpace trait implementation: public operation entry points
// (consume/produce/install), checkpointing, and state accessors. The
// locked operation internals live in ops_consume.rs / ops_produce.rs /
// ops_install.rs.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Instant;

use async_trait::async_trait;
use serde::Serialize;

use super::RSpace;
use super::locks::LOCK_SEQUENCE;
use crate::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::hot_store::HotStoreInstances;
use crate::rspace::internal::*;
use crate::rspace::metrics_constants::{
    CHANGES_SPAN, CREATE_CHECKPOINT_SPAN, HISTORY_CHECKPOINT_SPAN, RESET_SPAN,
    REVERT_SOFT_CHECKPOINT_SPAN, RSPACE_METRICS_SOURCE,
};
use crate::rspace::rspace_interface::{ISpace, MaybeConsumeResult, MaybeProduceResult};
use crate::rspace::striped_locks;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{Consume, Event, IOEvent, Produce};

#[async_trait]
impl<C, P, A, K> ISpace<C, P, A, K> for RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
        // Span[F].withMarks("create-checkpoint") from Scala - works because this is NOT
        // async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", CREATE_CHECKPOINT_SPAN).entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-create-checkpoint", "create_checkpoint");

        // Get changes with span
        let changes = {
            let _changes_span =
                tracing::info_span!(target: "f1r3fly.rspace", CHANGES_SPAN).entered();
            self.get_store().changes()
        };

        // Create history checkpoint with span
        let next_history = {
            let _history_span =
                tracing::info_span!(target: "f1r3fly.rspace", HISTORY_CHECKPOINT_SPAN).entered();
            self.get_history_repository().checkpoint(changes)
        };
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        let log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        self.reset_produce_counter();

        let history_repo = self.get_history_repository();
        let history_reader = history_repo.get_history_reader(&history_repo.root())?;

        self.create_new_hot_store(history_reader);
        self.restore_installs();

        // Mark the completion of create-checkpoint
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-create-checkpoint", "create_checkpoint");

        Ok(Checkpoint {
            root: self.get_history_repository().root(),
            log,
        })
    }

    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
        let _span = tracing::info_span!(target: "f1r3fly.rspace", RESET_SPAN).entered();
        let next_history = self.get_history_repository().reset(root)?;
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        *self.event_log.lock().expect("event log lock") = Vec::new();
        self.reset_produce_counter();

        // Striped locks are fixed-size and stateless (Mutex<()>); nothing to
        // clear on reset — they are reused across checkpoints.

        let history_reader = self.get_history_repository().get_history_reader(root)?;
        self.create_new_hot_store(history_reader);
        self.restore_installs();

        Ok(())
    }

    async fn consume_result(
        &self,
        _channel: Vec<C>,
        _pattern: Vec<P>,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        panic!("\nERROR: RSpace consume_result should not be called here");
    }

    async fn get_data(&self, channel: &C) -> Vec<Datum<A>> { self.get_store().get_data(channel) }

    async fn get_waiting_continuations(&self, channels: Vec<C>) -> Vec<WaitingContinuation<P, K>> {
        self.get_store().get_continuations(&channels)
    }

    async fn get_joins(&self, channel: C) -> Vec<Vec<C>> { self.get_store().get_joins(&channel) }

    async fn clear(&self) -> Result<(), RSpaceError> {
        self.reset(&RadixHistory::empty_root_node_hash()).await
    }

    async fn get_root(&self) -> Blake2b256Hash { self.get_history_repository().root() }

    async fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> { self.get_store().to_map() }

    async fn create_soft_checkpoint(&self) -> SoftCheckpoint<C, P, A, K> {
        let cache_snapshot = self.get_store().snapshot();
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let curr_produce_counter = self.take_produce_counter();

        SoftCheckpoint {
            cache_snapshot,
            log: curr_event_log,
            produce_counter: curr_produce_counter,
        }
    }

    async fn take_event_log(&self) -> Log {
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        self.reset_produce_counter();
        curr_event_log
    }

    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<C, P, A, K>,
    ) -> Result<(), RSpaceError> {
        let _span =
            tracing::info_span!(target: "f1r3fly.rspace", REVERT_SOFT_CHECKPOINT_SPAN).entered();
        let history = self.get_history_repository();
        let history_reader = history.get_history_reader(&history.root())?;
        let hot_store = HotStoreInstances::create_from_hs_and_hr(
            checkpoint.cache_snapshot,
            history_reader.base(),
        );

        *self.store.write().expect("store write lock") = Arc::new(hot_store);
        *self.event_log.lock().expect("event log lock") = checkpoint.log;
        self.restore_produce_counter(checkpoint.produce_counter);

        Ok(())
    }

    async fn consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        if channels.is_empty() {
            panic!("RUST ERROR: channels can't be empty");
        }
        if channels.len() != patterns.len() {
            panic!("RUST ERROR: channels.length must equal patterns.length");
        }
        let consume_ref = Consume::create(&channels, &patterns, &continuation, persist);

        let lock_start = Instant::now();
        let channel_hashes: Vec<u64> = channels
            .iter()
            .map(|ch| striped_locks::channel_hash(ch))
            .collect();
        let _lock_guard = self.consume_lock(&channel_hashes).await;
        if tracing::enabled!(target: "f1r3fly.rspace.lock_order", tracing::Level::TRACE) {
            let seq = LOCK_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
            tracing::trace!(target: "f1r3fly.rspace.lock_order", seq = seq, op = "consume", hashes = ?channel_hashes, "lock acquired");
        }
        metrics::counter!("rspace.consume.lock_acquire_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(lock_start.elapsed().as_nanos() as u64);

        metrics::counter!("rspace.consume.calls", "source" => RSPACE_METRICS_SOURCE).increment(1);
        let start = Instant::now();
        let result =
            self.locked_consume(&channels, &patterns, &continuation, persist, &peeks, &consume_ref);
        let duration = start.elapsed();
        metrics::histogram!("comm_consume_time_seconds", "source" => RSPACE_METRICS_SOURCE)
            .record(duration.as_secs_f64());
        result
    }

    async fn produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        let produce_ref = Produce::create(&channel, &data, persist);

        let lock_start = Instant::now();
        let _lock_guard = self.produce_lock(&channel).await;
        if tracing::enabled!(target: "f1r3fly.rspace.lock_order", tracing::Level::TRACE) {
            let seq = LOCK_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
            tracing::trace!(target: "f1r3fly.rspace.lock_order", seq = seq, op = "produce", hash = striped_locks::channel_hash(&channel), "lock acquired");
        }
        metrics::counter!("rspace.produce.lock_acquire_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(lock_start.elapsed().as_nanos() as u64);

        metrics::counter!("rspace.produce.calls", "source" => RSPACE_METRICS_SOURCE).increment(1);
        let start = Instant::now();
        let result = self.locked_produce(channel, data, persist, &produce_ref);
        let duration = start.elapsed();
        metrics::histogram!("comm_produce_time_seconds", "source" => RSPACE_METRICS_SOURCE)
            .record(duration.as_secs_f64());
        result
    }

    async fn install(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        metrics::counter!("rspace.install.calls", "source" => RSPACE_METRICS_SOURCE).increment(1);
        let start = Instant::now();
        let result = self.locked_install_internal(channels, patterns, continuation);
        let duration = start.elapsed();
        metrics::histogram!("install_time_seconds", "source" => RSPACE_METRICS_SOURCE)
            .record(duration.as_secs_f64());
        result
    }

    async fn rig_and_reset(
        &self,
        _start_root: Blake2b256Hash,
        _log: Log,
    ) -> Result<(), RSpaceError> {
        panic!("\nERROR: RSpace rig_and_reset should not be called here");
    }

    async fn rig(&self, _log: Log) -> Result<(), RSpaceError> {
        panic!("\nERROR: RSpace rig should not be called here");
    }

    async fn check_replay_data(&self) -> Result<(), RSpaceError> {
        panic!("\nERROR: RSpace check_replay_data should not be called here");
    }

    async fn is_replay(&self) -> bool { false }

    async fn update_produce(&self, produce_ref: Produce) -> () {
        for event in self.event_log.lock().expect("event log lock").iter_mut() {
            match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    if produce.hash == produce_ref.hash {
                        *produce = produce_ref.clone();
                    }
                }

                Event::Comm(comm) => {
                    for p in comm.produces.iter_mut() {
                        if p.hash == produce_ref.hash {
                            *p = produce_ref.clone();
                        }
                    }
                    if comm
                        .times_repeated
                        .keys()
                        .any(|k| k.hash == produce_ref.hash)
                    {
                        comm.times_repeated = std::mem::take(&mut comm.times_repeated)
                            .into_iter()
                            .map(|(k, v)| {
                                if k.hash == produce_ref.hash {
                                    (produce_ref.clone(), v)
                                } else {
                                    (k, v)
                                }
                            })
                            .collect();
                    }
                }

                _ => continue,
            }
        }
    }
}
