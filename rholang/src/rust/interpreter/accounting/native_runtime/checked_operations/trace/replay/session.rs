use std::fmt::Debug;
use std::hash::Hash;

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::history::native_reader::{NativeReadCharge, NativeReadMeter};
use rspace_plus_plus::rspace::internal::{Datum, Install, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::{
    NativeReplayBoundary as EpochBoundary, NativeReplayEpoch, NativeReplayRestore as EpochRestore,
};
use rspace_plus_plus::rspace::replay_rspace::native_session::{
    NativeReplayExport, NativeReplaySession, NativeSessionCheckpoint,
};
use serde::Serialize;

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityError;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloExecutionError, NativePhloRegionError,
};

mod operations;
mod execution;

#[cfg(test)]
mod errors;

struct SessionEpoch {
    replay: NativeOperationReplay,
    host: HostWorkBudget,
}

impl NativeReadMeter for HostWorkBudget {
    type Error = InterpreterError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        work(
            self,
            HostWorkDimension::VerificationOperations,
            charge.operations,
        )?;
        work(
            self,
            HostWorkDimension::VerificationBytes,
            charge.scanned_bytes,
        )?;
        work(
            self,
            HostWorkDimension::SearchStateBytes,
            charge.backing_bytes,
        )
    }
}

fn error(error: impl Into<NativeReplayError>) -> RSpaceError {
    match error.into() {
        NativeReplayError::Host(error) => error.into(),
        NativeReplayError::Budget(
            NativeBudgetTraceError::Work(FundingSearchError::HostWork(_))
            | NativeBudgetTraceError::Execution(
                NativePhloExecutionError::Work(FundingSearchError::HostWork(_))
                | NativePhloExecutionError::Authority(AuthorityError::HostWorkRejected)
                | NativePhloExecutionError::Region(NativePhloRegionError::Authority(
                    AuthorityError::HostWorkRejected,
                )),
            ),
        ) => RSpaceError::HostWorkRejected,
        other => RSpaceError::InterpreterError(other.to_string()),
    }
}

impl NativeReplayEpoch for SessionEpoch {
    type Boundary = NativeReplayBoundary;

    fn invalidate(&self) { self.replay.invalidate(); }

    fn begin_boundary(&self) -> Result<Self::Boundary, RSpaceError> {
        self.replay.begin_boundary().map_err(error)
    }

    fn reserve_work(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError> {
        reserve_host_work(&self.host, operations, bytes)
    }

    fn reserve_comparison(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError> {
        work(
            &self.host,
            HostWorkDimension::VerificationOperations,
            operations,
        )
        .map_err(error)?;
        work(&self.host, HostWorkDimension::VerificationBytes, bytes).map_err(error)
    }
}

fn reserve_host_work(
    host: &HostWorkBudget,
    operations: usize,
    bytes: usize,
) -> Result<(), RSpaceError> {
    work(host, HostWorkDimension::VerificationOperations, operations).map_err(error)?;
    work(host, HostWorkDimension::VerificationBytes, bytes).map_err(error)?;
    work(host, HostWorkDimension::SearchStateBytes, bytes).map_err(error)
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

pub struct NativeRuntimeReplaySession<C, P, A, K> {
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
        host: HostWorkBudget,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
        P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    {
        self.into_installed_session(history, matcher, host, std::iter::empty())
    }

    pub fn into_installed_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        host: HostWorkBudget,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
        P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    {
        self.build_session(history, matcher, host, installations, None)
    }

    pub(crate) fn into_runtime_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        host: HostWorkBudget,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
        budget: RuntimeBudget,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
        P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    {
        self.build_session(history, matcher, host, installations, Some(budget))
    }

    fn build_session<C, P, A, K>(
        self,
        history: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        host: HostWorkBudget,
        installations: impl IntoIterator<Item = (Vec<C>, Install<P, K>)>,
        budget: Option<RuntimeBudget>,
    ) -> Result<NativeRuntimeReplaySession<C, P, A, K>, RSpaceError>
    where
        C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
        P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
        K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    {
        let replay = self.into_replay(host.clone()).map_err(error)?;
        let replay = match budget {
            Some(budget) => replay.bind_runtime(budget).map_err(error)?,
            None => replay,
        };
        Ok(NativeRuntimeReplaySession {
            inner: NativeReplaySession::new_with_installs(
                history,
                matcher,
                SessionEpoch { replay, host },
                installations,
            )?,
        })
    }
}

impl<C, P, A, K> NativeRuntimeReplaySession<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
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

    pub async fn get_data_with_host_work(
        &self,
        channel: &C,
        host: &HostWorkBudget,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.inner
            .get_data_with_budget(channel, |operations, bytes| {
                reserve_host_work(host, operations, bytes)
            })
            .await
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
