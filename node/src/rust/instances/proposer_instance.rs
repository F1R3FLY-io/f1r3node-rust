use std::sync::Arc;
use std::time::{Duration, Instant};

use casper::rust::blocks::proposer::propose_result::{
    ProposeFailure, ProposeResult, ProposeStatus,
};
use casper::rust::blocks::proposer::proposer::{
    ProductionProposer, ProposeReturnType, ProposerResult,
};
use casper::rust::casper::Casper;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::errors::CasperError;
use casper::rust::metrics_constants::{PROPOSER_QUEUE_PENDING_METRIC, VALIDATOR_METRICS_SOURCE};
use casper::rust::ProposeRequestKind;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use tokio::sync::{mpsc, oneshot};

use super::proposer_coalescer::{FinishOutcome, ProposerCoalescer};

const PROPOSER_RESULT_QUEUE_CAPACITY: usize = 64;
const PROPOSER_MIN_INTERVAL: Duration = Duration::from_millis(250);

#[cfg(test)]
fn should_retry_immediately_on_trigger(result: &ProposeResult, is_async: bool) -> bool {
    let _ = (result, is_async);
    false
}

pub(crate) struct ProposeQueueEntry {
    pub(crate) request_kind: ProposeRequestKind,
    pub(crate) result_sender: oneshot::Sender<ProposerResult>,
    pub(crate) coalescer: Arc<ProposerCoalescer>,
}

pub struct ProposerInstance<T: TransportLayer + Send + Sync + 'static> {
    pub(crate) propose_requests_queue_rx: mpsc::Receiver<ProposeQueueEntry>,
    pub proposer: Arc<tokio::sync::Mutex<ProductionProposer<T>>>,
    pub state: Arc<tokio::sync::RwLock<casper::rust::state::instances::ProposerState>>,
    pub engine_cell: Arc<EngineCell>,
}

impl<T: TransportLayer + Send + Sync + 'static> ProposerInstance<T> {
    pub(crate) fn new(
        propose_requests_queue_rx: mpsc::Receiver<ProposeQueueEntry>,
        proposer: Arc<tokio::sync::Mutex<ProductionProposer<T>>>,
        state: Arc<tokio::sync::RwLock<casper::rust::state::instances::ProposerState>>,
        engine_cell: Arc<EngineCell>,
    ) -> Self {
        Self {
            propose_requests_queue_rx,
            proposer,
            state,
            engine_cell,
        }
    }

    pub fn create(
        self,
    ) -> Result<mpsc::Receiver<(ProposeResult, Option<BlockMessage>)>, CasperError> {
        let (result_tx, result_rx) = mpsc::channel(PROPOSER_RESULT_QUEUE_CAPACITY);

        tokio::spawn(async move {
            let Self {
                mut propose_requests_queue_rx,
                proposer,
                state,
                engine_cell,
            } = self;
            let mut last_propose_started_at: Option<Instant> = None;

            while let Some(entry) = propose_requests_queue_rx.recv().await {
                metrics::gauge!(
                    PROPOSER_QUEUE_PENDING_METRIC,
                    "source" => VALIDATOR_METRICS_SOURCE
                )
                .set(0.0);

                let mut request_kind = entry.request_kind;
                let mut result_sender = Some(entry.result_sender);
                let coalescer = entry.coalescer;
                let mut forced_follow_up = false;

                loop {
                    if !forced_follow_up {
                        if let Some(last_started) = last_propose_started_at {
                            let elapsed = last_started.elapsed();
                            if elapsed < PROPOSER_MIN_INTERVAL {
                                tokio::time::sleep(PROPOSER_MIN_INTERVAL - elapsed).await;
                            }
                        }
                    }

                    let engine = engine_cell.get().await;
                    let Some(current_casper) = engine.with_casper() else {
                        if let Some(sender) = result_sender.take() {
                            let _ = sender.send(ProposerResult::empty());
                        }
                        coalescer.cancel();
                        break;
                    };
                    let casper: Arc<dyn Casper + Send + Sync> = current_casper;

                    last_propose_started_at = Some(Instant::now());
                    tracing::info!(request_kind = ?request_kind, "Propose started");

                    let (curr_result_tx, curr_result_rx) = oneshot::channel();
                    {
                        let mut state_guard = state.write().await;
                        state_guard.curr_propose_result = Some(curr_result_rx);
                    }

                    let mut proposer_guard = proposer.lock().await;
                    let validator_public_key = proposer_guard.validator.public_key.bytes.clone();
                    let propose_result = proposer_guard
                        .propose(casper.clone(), request_kind.clone())
                        .await;
                    drop(proposer_guard);

                    match propose_result {
                        Ok(ProposeReturnType {
                            propose_result,
                            block_message_opt,
                            propose_result_to_send,
                        }) => {
                            if let Some(sender) = result_sender.take() {
                                let _ = sender.send(propose_result_to_send);
                            }

                            let result_copy = (propose_result.clone(), block_message_opt.clone());
                            {
                                let mut state_guard = state.write().await;
                                state_guard.latest_propose_result = Some(result_copy.clone());
                                state_guard.curr_propose_result = None;
                            }
                            let _ = curr_result_tx.send(result_copy);

                            match block_message_opt {
                                Some(block) => {
                                    let block_string =
                                        PrettyPrinter::build_string_block_message(&block, true);
                                    tracing::info!(
                                        status = ?propose_result.propose_status,
                                        block = %block_string,
                                        "Propose finished"
                                    );
                                    if let Err(error) =
                                        result_tx.send((propose_result, Some(block))).await
                                    {
                                        tracing::error!(%error, "propose result send failed");
                                    }
                                }
                                None if propose_result.is_no_new_deploys()
                                    || propose_result.is_deferred() =>
                                {
                                    tracing::info!(status = %propose_result.propose_status, "Propose finished")
                                }
                                None => {
                                    if propose_result.is_no_new_deploys() {
                                        tracing::info!("Propose: {}", propose_result.propose_status)
                                    } else {
                                        tracing::error!(
                                            status = %propose_result.propose_status,
                                            "propose failed"
                                        )
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "propose call failed");

                            let (classified_failure, block_to_request) = classify_propose_error(&e);
                            if let Some(hash) = block_to_request {
                                if let Err(req_err) =
                                    casper.request_block_from_peers(hash.clone()).await
                                {
                                    tracing::warn!(
                                        error = %req_err,
                                        block = %hex::encode(&hash[..hash.len().min(8)]),
                                        "failed to request the block the propose walk needs"
                                    );
                                }
                            }

                            let failure_seq_number = match casper.get_snapshot().await {
                                Ok(snapshot) => snapshot
                                    .max_seq_nums
                                    .get(&validator_public_key)
                                    .map(|sequence| *sequence + 1)
                                    .unwrap_or(1)
                                    as i32,
                                Err(snapshot_error) => {
                                    tracing::warn!(
                                        %snapshot_error,
                                        "Failed to get Casper snapshot for failure sequence number"
                                    );
                                    -1
                                }
                            };

                            // Always resolve requester oneshot with a failure result.
                            // Dropping this sender causes "channel closed" at caller and
                            // unnecessarily breaks heartbeat liveness flow.
                            if let Some(sender) = result_sender.take() {
                                let _ = sender.send(ProposerResult::failure(
                                    ProposeStatus::Failure(classified_failure.clone()),
                                    failure_seq_number,
                                ));
                            }

                            // Runtime propose errors are internal failures and should not be
                            // reported as NoNewDeploys / InternalDeployError.
                            let error_result: (ProposeResult, Option<BlockMessage>) =
                                (ProposeResult::failure(classified_failure), None);

                            // Send to both channels
                            let _ = curr_result_tx.send(error_result);
                            state.write().await.curr_propose_result = None;
                        }
                    }

                    match coalescer.finish() {
                        FinishOutcome::Idle => break,
                        FinishOutcome::PendingFollowUp => {
                            request_kind = ProposeRequestKind::PendingDeploy;
                            forced_follow_up = true;
                        }
                    }
                }
            }

            tracing::info!("Propose requests queue closed, stopping proposer");
            Result::<(), CasperError>::Ok(())
        });

        Ok(result_rx)
    }
}

/// Classify a propose-call error into the failure the channels report and
/// the block to request, if any. `BlockNotHeld` carries the floor
/// machinery's fetch-and-retry contract; everything else is a genuine bug
/// the heartbeat should back off on.
pub(crate) fn classify_propose_error(
    error: &CasperError,
) -> (ProposeFailure, Option<models::rust::block_hash::BlockHash>) {
    match error {
        CasperError::BlockNotHeld(hash) => (
            ProposeFailure::MissingBlock(hash.clone()),
            Some(hash.clone()),
        ),
        _ => (ProposeFailure::BugError, None),
    }
}

#[cfg(test)]
mod tests {
    use casper::rust::blocks::proposer::propose_result::CheckProposeConstraintsFailure;

    use super::*;

    /// `BlockNotHeld` is availability, not a bug: the classification must
    /// name the block so the caller requests it, and must not report the
    /// backoff-escalating BugError.
    #[test]
    fn block_not_held_classifies_as_missing_block_to_request() {
        let hash = models::rust::block_hash::BlockHash::from(vec![0x5a; 32]);
        let (failure, request) = classify_propose_error(&CasperError::BlockNotHeld(hash.clone()));
        assert_eq!(
            request.as_ref(),
            Some(&hash),
            "the named block must be requested"
        );
        assert!(
            matches!(failure, ProposeFailure::MissingBlock(h) if h == hash),
            "availability failure, never BugError"
        );
    }

    /// Every other propose error keeps the bug classification (and its
    /// backoff) — the availability carve-out must not widen.
    #[test]
    fn other_errors_stay_bug_classified() {
        let (failure, request) =
            classify_propose_error(&CasperError::RuntimeError("genuinely broken".to_string()));
        assert!(request.is_none());
        assert!(matches!(failure, ProposeFailure::BugError));
    }

    #[test]
    fn should_not_retry_internal_deploy_error_immediately() {
        let result = ProposeResult::failure(ProposeFailure::InternalDeployError);
        assert!(
            !should_retry_immediately_on_trigger(&result, true),
            "InternalDeployError should not trigger immediate retry"
        );
    }

    #[test]
    fn should_not_retry_not_enough_new_blocks_for_async_propose() {
        let result = ProposeResult::failure(ProposeFailure::CheckConstraintsFailure(
            CheckProposeConstraintsFailure::NotEnoughNewBlocks,
        ));
        assert!(
            !should_retry_immediately_on_trigger(&result, true),
            "NotEnoughNewBlocks should not trigger immediate async retry"
        );
    }
}
