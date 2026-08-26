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
                                    || propose_result.is_recovery_deferred() =>
                                {
                                    tracing::info!(status = %propose_result.propose_status, "Propose finished")
                                }
                                None => {
                                    tracing::error!(
                                        status = %propose_result.propose_status,
                                        "propose failed"
                                    )
                                }
                            }
                        }
                        Err(error) => {
                            tracing::error!(%error, "propose call failed");
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
                            if let Some(sender) = result_sender.take() {
                                let _ = sender.send(ProposerResult::failure(
                                    ProposeStatus::Failure(ProposeFailure::BugError),
                                    failure_seq_number,
                                ));
                            }
                            let error_result: (ProposeResult, Option<BlockMessage>) =
                                (ProposeResult::failure(ProposeFailure::BugError), None);
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
