use std::fmt::Debug;
use std::hash::Hash;

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::internal::{Datum, Install, WaitingContinuation};
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

// Changed by D-C2c (D-S1, DR-96): the native session's digest-keyed store
// holds data, continuations and patterns, which need Clone.
// pub struct NativeRuntimeReplaySession<C, P, A, K> {
pub struct NativeRuntimeReplaySession<C, P: Clone, A: Clone, K: Clone> {
    inner: NativeReplaySession<C, P, A, K, SessionEpoch>,
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
        self.build_session(history, matcher, installations, None)
    }

    pub(crate) fn into_runtime_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
        budget: RuntimeBudget,
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
        self.build_session(history, matcher, installations, Some(budget))
    }

    fn build_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
        budget: Option<RuntimeBudget>,
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
        Ok(NativeRuntimeReplaySession {
            inner: NativeReplaySession::new_with_installs(
                history,
                matcher,
                SessionEpoch { replay },
                installations,
            )?,
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
