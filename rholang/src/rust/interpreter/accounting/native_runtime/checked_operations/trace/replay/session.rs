use std::fmt::Debug;
use std::hash::Hash;

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::internal::{Datum, Install, WaitingContinuation};
// Added by DR-134 (DR-70): the logger of a reporting session.
use rspace_plus_plus::rspace::logging::RSpaceLogger;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::{
    NativeReplayBoundary as EpochBoundary, NativeReplayEpoch, NativeReplayRestore as EpochRestore,
};
use rspace_plus_plus::rspace::replay_rspace::native_session::{
    NativeReplayExport, NativeReplaySession, NativeSessionCheckpoint,
};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::Serialize;

use super::*;
use crate::rust::interpreter::execution_space::EvaluationProgress;

mod operations;
mod execution;

#[cfg(test)]
mod errors;

struct SessionEpoch {
    replay: NativeOperationReplay,
}

fn error(error: impl Into<NativeReplayError>) -> RSpaceError {
    match error.into() {
        NativeReplayError::Interpreter(error) => error.into(),
        other => RSpaceError::InterpreterError(other.to_string()),
    }
}

impl NativeReplayEpoch for SessionEpoch {
    type Boundary = NativeReplayBoundary;

    fn invalidate(&self) { self.replay.invalidate(); }

    /// Added by DR-113: a replayed denial takes the class of the bound
    /// source, as the play denial does.
    fn denial(&self) -> RSpaceError {
        RSpaceError::from(
            crate::rust::interpreter::accounting::native_runtime::exhaustion_error(
                self.replay.bound_source(),
            ),
        )
    }

    fn begin_boundary(&self) -> Result<Self::Boundary, RSpaceError> {
        self.replay.begin_boundary().map_err(error)
    }
}

impl EpochBoundary for NativeReplayBoundary {
    type Checkpoint = NativeReplayCheckpoint;
    type Restore = NativeReplayRestore;
    type Evidence = NativeReplayAccountingSnapshot;

    fn checkpoint(&self) -> Self::Checkpoint { NativeReplayBoundary::checkpoint(self) }

    fn prepare_restore(self, checkpoint: &Self::Checkpoint) -> Result<Self::Restore, RSpaceError> {
        NativeReplayBoundary::prepare_restore(self, checkpoint).map_err(error)
    }

    fn check_complete(&self) -> Result<(), RSpaceError> {
        NativeReplayBoundary::check_complete(self).map_err(error)
    }

    fn completed_usage(&self) -> Result<u64, RSpaceError> {
        NativeReplayBoundary::completed_usage(self).map_err(error)
    }

    fn completed_evidence(&self) -> Result<Self::Evidence, RSpaceError> {
        NativeReplayBoundary::completed_evidence(self).map_err(error)
    }

    fn close(self) { NativeReplayBoundary::close(self) }
}

impl EpochRestore for NativeReplayRestore {
    fn publish(self) { NativeReplayRestore::publish(self) }
}

/// Added by DR-132 (D2-B): native replay counts the live tasks of each
/// evaluation for its stall rule (`progress.rs`).
impl EvaluationProgress for NativeOperationReplay {
    /// Only a replay with an idle progress word, no open boundary and no
    /// reserved slot begins an evaluation. A stalled or cancelled replay
    /// never begins again.
    fn begin(&self) -> Result<(), RSpaceError> {
        let state = self.inner.state.lock().expect("native replay state");
        state.open().map_err(error)?;
        if !state.quiescent() || !self.inner.progress.begin() {
            return Err(error(NativeReplayError::Busy));
        }
        Ok(())
    }

    fn enter(&self) { self.inner.progress.enter(); }

    /// The finish that leaves every live task parked declares the stall
    /// under the state lock, and the waiters wake after the unlock. The drop
    /// path tolerates a poisoned lock.
    fn finish(&self) {
        if self.inner.progress.finish() {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if progress::stall(&mut state, &self.inner.progress) {
                drop(state);
                self.inner.changed.notify_waiters();
            }
        }
    }

    fn close(&self) { self.inner.progress.close(); }

    fn identity(&self) -> usize { NativeOperationReplay::identity(self) }
}

// Changed by D-C2c (D-S1, DR-96): the native session's digest-keyed store
// holds data, continuations and patterns, which need Clone.
// pub struct NativeRuntimeReplaySession<C, P, A, K> {
pub struct NativeRuntimeReplaySession<C, P: Clone, A: Clone, K: Clone> {
    inner: NativeReplaySession<C, P, A, K, SessionEpoch>,
    /// Added by DR-132 (D2-B): the progress of each evaluation on this
    /// session.
    progress: Arc<dyn EvaluationProgress>,
    /// Added by DR-132 (D2-B): the replay, so tests can read its progress.
    #[cfg(test)]
    replay: NativeOperationReplay,
}

pub struct NativeRuntimeReplayCheckpoint<C: Eq + Hash, P: Clone, A: Clone, K: Clone> {
    inner: NativeSessionCheckpoint<C, P, A, K, SessionEpoch>,
}

impl CheckedNativeOperationTrace {
    pub fn into_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
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
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        A: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        K: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
    {
        self.into_installed_session(history, matcher, std::iter::empty())
    }

    pub fn into_installed_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
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
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        A: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        K: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
    {
        // Changed by DR-134 (DR-70): an installed session has no logger.
        // self.build_session(history, matcher, installations, None)
        self.build_session(history, matcher, installations, None, None)
    }

    // Disabled by DR-134 (DR-70): `create_native_replay_env` and
    // `create_native_replay_env_with_logger` both call
    // `into_runtime_session_with_logger`, so this wrapper has no caller.
    //     pub(crate) fn into_runtime_session<C, P, A, K>(
    //         self,
    //         history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
    //         matcher: Arc<Box<dyn Match<P, A, K>>>,
    //         installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
    //         budget: RuntimeBudget,
    //     ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    //     where
    //         C: Clone
    //             + Debug
    //             + Default
    //             + Serialize
    //             + serde::de::DeserializeOwned
    //             + ColdStoreDecode
    //             + Hash
    //             + Ord
    //             + Eq
    //             + 'static
    //             + Sync
    //             + Send,
    //         P: Clone
    //             + Debug
    //             + Default
    //             + Serialize
    //             + serde::de::DeserializeOwned
    //             + ColdStoreDecode
    //             + 'static
    //             + Sync
    //             + Send,
    //         A: Clone
    //             + Debug
    //             + Default
    //             + Serialize
    //             + serde::de::DeserializeOwned
    //             + ColdStoreDecode
    //             + 'static
    //             + Sync
    //             + Send,
    //         K: Clone
    //             + Debug
    //             + Default
    //             + Serialize
    //             + serde::de::DeserializeOwned
    //             + ColdStoreDecode
    //             + 'static
    //             + Sync
    //             + Send,
    //     {
    //         // Changed by DR-134 (DR-70): validation and play replay without a
    //         // logger.
    //         // self.build_session(history, matcher, installations, Some(budget))
    //         self.into_runtime_session_with_logger(history, matcher, installations, budget, None)
    //     }

    /// Added by DR-134 (DR-70): `into_runtime_session` whose session logs its
    /// stored and matched operations to `logger`, for block reports.
    pub(crate) fn into_runtime_session_with_logger<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
        budget: RuntimeBudget,
        logger: Option<Box<dyn RSpaceLogger<C, P, A, K>>>,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
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
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        A: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        K: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
    {
        self.build_session(history, matcher, installations, Some(budget), logger)
    }

    fn build_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
        budget: Option<RuntimeBudget>,
        // Added by DR-134 (DR-70).
        logger: Option<Box<dyn RSpaceLogger<C, P, A, K>>>,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
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
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        A: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
        K: Clone
            + Debug
            + Default
            + Serialize
            + serde::de::DeserializeOwned
            + ColdStoreDecode
            + 'static
            + Sync
            + Send,
    {
        let replay = self.into_replay().map_err(error)?;
        let replay = match budget {
            Some(budget) => replay.bind_runtime(budget).map_err(error)?,
            None => replay,
        };
        // Changed by DR-132 (D2-B): the session keeps the progress of the
        // replay.
        // Ok(NativeRuntimeReplaySession {
        //     inner: NativeReplaySession::new_with_installs(
        //         history,
        //         matcher,
        //         SessionEpoch { replay },
        //         installations,
        //     )?,
        // })
        let progress: Arc<dyn EvaluationProgress> = Arc::new(replay.clone());
        #[cfg(test)]
        let observed = replay.clone();
        // Changed by DR-134 (DR-70): the session takes the optional logger.
        Ok(NativeRuntimeReplaySession {
            inner: NativeReplaySession::new_with_installs_and_logger(
                history,
                matcher,
                SessionEpoch { replay },
                installations,
                logger,
            )?,
            progress,
            #[cfg(test)]
            replay: observed,
        })
    }
}

impl<C, P, A, K> NativeRuntimeReplaySession<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
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
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
{
    /// Added by DR-132 (D2-B): the progress of each evaluation on this
    /// session.
    pub(crate) fn evaluation_progress(&self) -> Arc<dyn EvaluationProgress> {
        Arc::clone(&self.progress)
    }

    /// Added by DR-132 (D2-B): the progress word is idle.
    #[cfg(test)]
    pub(crate) fn progress_idle(&self) -> bool { self.replay.progress_idle() }

    /// Added by DR-132 (D2-B): replay declared a stall.
    #[cfg(test)]
    pub(crate) fn progress_stalled(&self) -> bool { self.replay.progress_stalled() }

    /// Added by DR-132 (D2-B): the live tasks and the parked waiters.
    #[cfg(test)]
    pub(crate) fn progress_counts(&self) -> (u64, u64) { self.replay.progress_counts() }

    /// Added by DR-132 (D2-B): poisons the state lock of the replay.
    #[cfg(test)]
    pub(crate) fn poison_state(&self) { self.replay.poison_state() }

    pub async fn checkpoint(
        &self,
    ) -> Result<NativeRuntimeReplayCheckpoint<C, P, A, K>, RSpaceError> {
        Ok(NativeRuntimeReplayCheckpoint {
            inner: self.inner.checkpoint().await?,
        })
    }

    pub async fn restore(
        &self,
        checkpoint: NativeRuntimeReplayCheckpoint<C, P, A, K>,
    ) -> Result<(), RSpaceError> {
        self.inner.restore(checkpoint.inner).await
    }

    pub async fn check_complete(&self) -> Result<(), RSpaceError> {
        self.inner.check_complete().await
    }

    pub async fn completed_usage(&self) -> Result<u64, RSpaceError> {
        self.inner.completed_usage().await
    }

    pub async fn completed_evidence(&self) -> Result<NativeReplayAccountingSnapshot, RSpaceError> {
        self.inner.completed_evidence().await
    }

    pub async fn export(
        &self,
    ) -> Result<NativeReplayExport<NativeReplayAccountingSnapshot>, RSpaceError> {
        self.inner.export().await
    }

    pub async fn close(&self) -> Result<(), RSpaceError> { self.inner.close().await }

    pub async fn get_data(&self, channel: &C) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.inner.get_data(channel).await
    }

    pub async fn get_joins(&self, channel: &C) -> Result<Vec<Vec<C>>, RSpaceError> {
        self.inner.get_joins(channel).await
    }

    pub async fn get_continuations(
        &self,
        channels: &[C],
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        self.inner.get_continuations(channels).await
    }
}
