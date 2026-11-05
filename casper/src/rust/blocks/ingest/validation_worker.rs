use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::MissingBlockContext;
use tokio::sync::{mpsc, Semaphore};

use super::ingest_state::WorkOutcome;
use super::ingest_step::IngestStep;
use crate::rust::block_status::{BlockError, InvalidBlock};
use crate::rust::casper::Casper;
use crate::rust::errors::CasperError;
// The block processor's source label is part of the exported names that the
// monitoring rules read.
use crate::rust::metrics_constants::{
    BLOCK_PROCESSOR_METRICS_SOURCE, BLOCK_SIZE_METRIC, BLOCK_VALIDATION_FAILED_METRIC,
    BLOCK_VALIDATION_SUCCESS_METRIC,
};
use crate::rust::util::proto_util;
use crate::rust::ValidBlockProcessing;

/// Lifetime cap on settled-history admissions. Legitimate joins need single
/// digits (the gaps LFS's closure missed); the cap prices the worst case — a
/// bonded attacker citing self-signed junk below the anchor — at bounded,
/// alarmed storage. Past it the block is validated like any other.
pub const SETTLED_ADMISSION_BUDGET: u64 = 512;

/// Ceiling on detached block-hash announces in flight. An announce is
/// best-effort gossip, so past the ceiling further announces are dropped
/// rather than queued.
pub const ANNOUNCE_MAX_IN_FLIGHT: usize = 128;

/// Withdraw a deferral the node is not entitled to.
///
/// `Undecidable` and `AwaitingState` are the absence of a verdict, so they are
/// what an attacker would want a crafted block to induce. They are acceptable
/// only on a node whose history is cut short: a node restored from a sync
/// anchor has nothing below it. A genesis-rooted node (approved block at
/// height zero) holds every block and root it ever needed, so a missing one is
/// local corruption and the block is judged.
pub(crate) fn guard_deferral(
    status: ValidBlockProcessing,
    approved_block_number: i64,
) -> ValidBlockProcessing {
    match status {
        Either::Left(BlockError::Undecidable(hash)) if approved_block_number == 0 => {
            Either::Left(BlockError::BlockException(CasperError::BlockNotHeld(
                hash,
                MissingBlockContext::new("genesis-rooted node refuses deferral"),
            )))
        }
        Either::Left(BlockError::AwaitingState(root)) if approved_block_number == 0 => {
            Either::Left(BlockError::BlockException(CasperError::Other(format!(
                "state root {} missing on a genesis-rooted node — local corruption, not sync",
                root
            ))))
        }
        other => other,
    }
}

/// Whether a block cited by a bonded validator's block and at or below the
/// node's sync anchor is admitted unjudged as settled history. Each condition
/// closes a distinct attack; see the truth-table test.
///
/// `seq_below_senders_latest` refuses live-chain material wearing a sub-anchor
/// height: genuine settled stragglers sit strictly below their sender's latest
/// message, and a sender with no latest message (an unbonded historic author)
/// passes.
pub(crate) fn admit_as_settled(
    block_number: i64,
    approved_block_number: i64,
    budget_remaining: bool,
    seq_below_senders_latest: bool,
) -> bool {
    approved_block_number > 0
        && block_number <= approved_block_number
        && budget_remaining
        && seq_below_senders_latest
}

/// Announces judged blocks' hashes to peers. The announce is one-way gossip,
/// so it runs detached and permit-bounded: awaited inline, one unreachable
/// peer's send timeout would tax every block.
pub struct BlockAnnouncer<T: TransportLayer + Send + Sync> {
    transport: Arc<T>,
    connections: ConnectionsCell,
    conf: RPConf,
    permits: Arc<Semaphore>,
}

impl<T: TransportLayer + Send + Sync + 'static> BlockAnnouncer<T> {
    pub fn new(transport: Arc<T>, connections: ConnectionsCell, conf: RPConf) -> Self {
        Self {
            transport,
            connections,
            conf,
            permits: Arc::new(Semaphore::new(ANNOUNCE_MAX_IN_FLIGHT)),
        }
    }

    /// Announce `block`'s hash to every connected peer without waiting for
    /// the sends; past `ANNOUNCE_MAX_IN_FLIGHT` announces in flight it is
    /// dropped.
    pub fn announce(&self, block: &BlockMessage) {
        let Ok(permit) = self.permits.clone().try_acquire_owned() else {
            tracing::debug!(
                block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                cap = ANNOUNCE_MAX_IN_FLIGHT,
                "dropping block-hash announce: every announce slot is held by a slow peer send"
            );
            return;
        };
        let transport = self.transport.clone();
        let connections = self.connections.clone();
        let conf = self.conf.clone();
        let block_hash: BlockHash = block.block_hash.clone();
        let sender = block.sender.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = transport
                .send_block_hash(&connections, &conf, &block_hash, &sender)
                .await
            {
                tracing::warn!(
                    "Failed to send block hash {} to peers: {}",
                    PrettyPrinter::build_string_bytes(&block_hash),
                    error
                );
            }
        });
    }
}

/// Validates dispatched blocks and admits settled history, with the effects
/// of each verdict: the DAG insert, and the block-hash announce to peers.
pub struct Validation<T: TransportLayer + Send + Sync> {
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
    announcer: BlockAnnouncer<T>,
    settled_admissions: AtomicU64,
    state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
}

impl<T: TransportLayer + Send + Sync + 'static> Validation<T> {
    pub fn new(
        block_store: KeyValueBlockStore,
        dag: BlockDagKeyValueStorage,
        transport: Arc<T>,
        connections: ConnectionsCell,
        conf: RPConf,
        state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
    ) -> Self {
        Self {
            block_store,
            dag,
            announcer: BlockAnnouncer::new(transport, connections, conf),
            settled_admissions: AtomicU64::new(0),
            state_root_fetch_tx,
        }
    }

    pub fn block_store(&self) -> &KeyValueBlockStore { &self.block_store }

    /// Validate `block` and carry out the verdict's effects.
    pub async fn validate(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
    ) -> WorkOutcome {
        match self.judge(casper, block).await {
            Ok((_, outcome)) => outcome,
            Err(error) => {
                tracing::error!(
                    block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    %error,
                    "block validation failed without a verdict"
                );
                WorkOutcome::Failed
            }
        }
    }

    /// Validate `block`, carry out the verdict's effects, and return the
    /// verdict with what block ingest does next.
    pub async fn judge(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
    ) -> Result<(ValidBlockProcessing, WorkOutcome), CasperError> {
        metrics::histogram!(BLOCK_SIZE_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(block.to_proto().encoded_len() as f64);
        let approved_block_number = proto_util::block_number(casper.get_approved_block()?);
        let mut snapshot = match IngestStep::ValidationSetup
            .time_async(casper.get_snapshot())
            .await
        {
            Ok(snapshot) => snapshot,
            Err(CasperError::BlockNotHeld(missing, site)) => {
                let guarded = guard_deferral(
                    Either::Left(BlockError::Undecidable(missing.clone())),
                    approved_block_number,
                );
                if !matches!(guarded, Either::Left(BlockError::Undecidable(_))) {
                    return Err(CasperError::BlockNotHeld(missing, site));
                }
                tracing::warn!(
                    "Snapshot for block {} needs {}, which this node does not hold. Walk: {}",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    PrettyPrinter::build_string_bytes(&missing),
                    site.accessor()
                );
                return Ok((guarded, WorkOutcome::AwaitingBlock(missing)));
            }
            Err(error) => return Err(error),
        };

        let status = IngestStep::Validation
            .time_async(casper.validate(block, &mut snapshot))
            .await?;
        let status = guard_deferral(status, approved_block_number);
        let verdict_metric = match &status {
            Either::Right(_) => BLOCK_VALIDATION_SUCCESS_METRIC,
            Either::Left(_) => BLOCK_VALIDATION_FAILED_METRIC,
        };
        metrics::counter!(verdict_metric, "source" => BLOCK_PROCESSOR_METRICS_SOURCE).increment(1);
        let outcome = match &status {
            Either::Right(_) => {
                casper.handle_valid_block(block).await?;
                self.announcer.announce(block);
                WorkOutcome::Settled
            }
            Either::Left(BlockError::Undecidable(missing)) => {
                tracing::warn!(
                    "Block {} could not be judged: this node does not hold {}.",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    PrettyPrinter::build_string_bytes(missing)
                );
                WorkOutcome::AwaitingBlock(missing.clone())
            }
            Either::Left(BlockError::AwaitingState(root)) => {
                tracing::warn!(
                    block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    %root,
                    "Block could not be judged: this node does not hold the state root its \
                     replay starts from."
                );
                WorkOutcome::AwaitingState(root.clone())
            }
            Either::Left(BlockError::Invalid(invalid)) => {
                self.judge_invalid(casper.as_ref(), block, invalid, &snapshot)?
            }
            // A block that raised an exception is recorded as an invalid
            // transaction, so blocks built on it are not stalled behind it.
            Either::Left(BlockError::BlockException(error)) => {
                tracing::warn!(
                    "Block {} raised BlockException ({}); recording as InvalidTransaction.",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    error
                );
                self.judge_invalid(
                    casper.as_ref(),
                    block,
                    &InvalidBlock::InvalidTransaction,
                    &snapshot,
                )?
            }
            Either::Left(other) => {
                tracing::warn!(
                    "Block {} validation returned {:?}; dropped.",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    other
                );
                WorkOutcome::Rejected {
                    invalid_sender: false,
                }
            }
        };
        tracing::info!(
            "Block {} validated {:?}.",
            PrettyPrinter::build_string_bytes(&block.block_hash),
            status
        );
        crate::rust::util::allocator::trim_after_block();
        Ok((status, outcome))
    }

    fn judge_invalid(
        &self,
        casper: &(dyn Casper + Send + Sync),
        block: &BlockMessage,
        invalid: &InvalidBlock,
        snapshot: &crate::rust::casper::CasperSnapshot,
    ) -> Result<WorkOutcome, CasperError> {
        casper.handle_invalid_block(block, invalid, &snapshot.dag)?;
        self.announcer.announce(block);
        Ok(if self.dag.contains(&block.block_hash) {
            WorkOutcome::Settled
        } else {
            WorkOutcome::Rejected {
                invalid_sender: *invalid == InvalidBlock::InvalidSender,
            }
        })
    }

    /// Admit `block` unjudged as settled history, or report that it does not
    /// qualify. Block ingest dispatches this only for a block cited by a
    /// validator bonded at the anchor.
    pub async fn admit_settled(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
    ) -> WorkOutcome {
        match self.try_admit_settled(casper, block) {
            Ok(true) => WorkOutcome::Settled,
            Ok(false) => WorkOutcome::NotSettledHistory,
            Err(error) => {
                tracing::error!(
                    block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    %error,
                    "settled-history admission failed"
                );
                WorkOutcome::Failed
            }
        }
    }

    fn try_admit_settled(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        let approved_block_number = proto_util::block_number(casper.get_approved_block()?);
        let seq_below_senders_latest = match self.dag.latest_message_hash(&block.sender)? {
            Some(latest_hash) => match self.dag.lookup(&latest_hash)? {
                Some(latest) => block.seq_num < latest.sequence_number,
                None => false,
            },
            None => true,
        };
        if !admit_as_settled(
            proto_util::block_number(block),
            approved_block_number,
            self.settled_admissions.load(Ordering::Relaxed) < SETTLED_ADMISSION_BUDGET,
            seq_below_senders_latest,
        ) {
            return Ok(false);
        }
        // Reserve the budget slot atomically, so concurrent admissions cannot
        // overshoot the bound.
        let Ok(reserved) = self.settled_admissions.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |admitted| (admitted < SETTLED_ADMISSION_BUDGET).then_some(admitted + 1),
        ) else {
            return Ok(false);
        };
        let admitted = self
            .block_store
            .persist_block(block)
            .map_err(CasperError::from)
            .and_then(|()| {
                self.dag
                    .insert(block, InsertMode::SettledHistory)
                    .map_err(CasperError::from)
            });
        if let Err(error) = admitted {
            self.settled_admissions.fetch_sub(1, Ordering::Relaxed);
            return Err(error);
        }
        let admitted = reserved + 1;
        if admitted == SETTLED_ADMISSION_BUDGET / 2 {
            tracing::warn!(
                admitted,
                budget = SETTLED_ADMISSION_BUDGET,
                "Settled-history admissions at half budget; a healthy join needs single digits \
                 — investigate what keeps citing unheld settled blocks"
            );
        }
        tracing::info!(
            block = %PrettyPrinter::build_string_bytes(&block.block_hash),
            block_number = proto_util::block_number(block),
            anchor_number = approved_block_number,
            admitted,
            "Admitted solicited block as settled history (below this node's sync anchor)"
        );
        // A settled block is a legal parent whose state its children's replay
        // reads; fetch its declared roots before a child asks for them.
        for root in [
            &block.body.state.post_state_hash,
            &block.body.state.pre_state_hash,
        ] {
            self.request_state_root(Blake2b256Hash::from_bytes_prost(root));
        }
        Ok(true)
    }

    fn request_state_root(&self, root: Blake2b256Hash) {
        match &self.state_root_fetch_tx {
            Some(tx) => {
                if tx.try_send(root.clone()).is_err() {
                    tracing::warn!(%root, "state requester queue full or closed; the eager root fetch is skipped");
                }
            }
            None => {
                tracing::debug!(%root, "no state requester wired; the eager root fetch is skipped")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settled_history_admission_needs_every_condition() {
        assert!(admit_as_settled(5, 10, true, true));
        assert!(
            !admit_as_settled(5, 0, true, true),
            "a genesis-rooted node holds all history"
        );
        assert!(
            !admit_as_settled(11, 10, true, true),
            "above the anchor is live material"
        );
        assert!(!admit_as_settled(5, 10, false, true), "the budget is spent");
        assert!(
            !admit_as_settled(5, 10, true, false),
            "at or above the sender's latest message"
        );
    }

    #[test]
    fn only_a_node_with_a_hole_in_its_history_may_defer() {
        let missing = BlockHash::from(vec![7u8; 32]);
        let undecidable = || Either::Left(BlockError::Undecidable(missing.clone()));
        assert!(matches!(
            guard_deferral(undecidable(), 0),
            Either::Left(BlockError::BlockException(CasperError::BlockNotHeld(..)))
        ));
        assert!(matches!(
            guard_deferral(undecidable(), 5),
            Either::Left(BlockError::Undecidable(_))
        ));
        let root = Blake2b256Hash::new(b"root");
        assert!(matches!(
            guard_deferral(Either::Left(BlockError::AwaitingState(root.clone())), 0),
            Either::Left(BlockError::BlockException(_))
        ));
        assert!(matches!(
            guard_deferral(Either::Left(BlockError::AwaitingState(root)), 5),
            Either::Left(BlockError::AwaitingState(_))
        ));
    }
}
