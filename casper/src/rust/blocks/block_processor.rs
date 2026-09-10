// See casper/src/main/scala/coop/rchain/casper/blocks/BlockProcessor.scala

/*
 * ARCHITECTURAL CHOICE: Trait-based Dependency Injection
 *
 * This implementation uses trait-based dependency injection instead of functional closures
 * because Rust's ownership model and async system work better with traits than with complex
 * closure captures. Traits provide zero-cost abstractions, better testability, and seamless
 * async support while maintaining the same flexibility as the original Scala version.
 */

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, CertifiedSenderAuthority, KeyValueDagRepresentation,
    ValidatedSettledHistoryAdmission,
};
use block_storage::rust::dag::buffer_dag_transition::atomic_insert_settled_then_buffer;
use block_storage::rust::finality::SETTLED_RECOVERY_EPISODE_CAPACITY;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, CasperMessage};
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::env;
use tokio::sync::mpsc;

use crate::rust::block_status::{
    BlockError, CertifiedBlockValidation, InvalidBlock, ValidationDeferral,
};
use crate::rust::casper::{Casper, CasperSnapshot};
use crate::rust::engine::block_retriever::{AdmitHashReason, BlockRetriever, RequestTracking};
use crate::rust::engine::runtime_state_requester::StateRootFetchCommand;
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_PROCESSING_STORAGE_TIME_METRIC, BLOCK_PROCESSING_VALIDATION_SETUP_TIME_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE, BLOCK_SIZE_METRIC, BLOCK_VALIDATION_FAILED_METRIC,
    BLOCK_VALIDATION_LOCAL_FAULT_DEFERRED_METRIC, BLOCK_VALIDATION_SUCCESS_METRIC,
    BLOCK_VALIDATION_TIME_METRIC,
};
use crate::rust::util::proto_util;
use crate::rust::validate::Validate;
use crate::rust::ValidBlockProcessing;

/// Logic for processing incoming blocks
/// Blocks created by node itself are not held here, but in Proposer.
#[derive(Clone)]
pub struct BlockProcessor<T: TransportLayer + Send + Sync> {
    dependencies: BlockProcessorDependencies<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOwnership {
    Pending,
    Terminal,
    Missing,
}

pub struct PublicationInterest {
    hash: BlockHash,
    casper: Arc<dyn Casper + Send + Sync>,
    requested_as_dependency: bool,
}

impl PublicationInterest {
    fn check_binding(
        &self,
        casper: &Arc<dyn Casper + Send + Sync>,
        hash: &BlockHash,
    ) -> Result<(), CasperError> {
        if self.hash == hash && Arc::ptr_eq(&self.casper, casper) {
            Ok(())
        } else {
            Err(CasperError::RuntimeError(
                "Publication evidence has a different block or Casper context".into(),
            ))
        }
    }
}

pub struct AcceptedBlockPublication {
    interest: PublicationInterest,
}

pub enum StoredPublicationPreparation {
    Terminal,
    Missing,
    Accepted {
        block: Box<BlockMessage>,
        evidence: AcceptedBlockPublication,
    },
}

/// What must happen to a block once validation has returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PostValidation {
    /// The block was judged. Drop it from the buffer and stop tracking it.
    Settled,
    /// The block was NOT judged: validation needed a block this node does not
    /// hold. Keep it buffered against the named dependency and fetch that, or
    /// the block is dropped un-judged and the gap it needs is never requested.
    AwaitingBlock(BlockHash),
    /// The block was NOT judged: replay needed a state root this node does
    /// not hold. Keep it buffered as a pendant — the pendant scan retries it
    /// after each processed block, throttled by the missing-dependency
    /// attempts machinery — and hand the root to the state requester.
    AwaitingState(rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash),
}

/// Withdraw the deferral if this node has no hole in its history.
///
/// `Undecidable` is the one outcome that is not a verdict, so it is also the
/// one an attacker would want: a block that induces it is never judged, never
/// recorded invalid, and produces no evidence. That is only acceptable when the
/// node truly cannot know — which is exactly when its own history is cut short.
///
/// A node built from genesis holds a complete main-parent spine, so a block it
/// cannot find is corruption and must be judged as before. A node restored from
/// a sync anchor has nothing below that anchor and never will:
/// `last_approved_block` is written once — at the genesis ceremony or at LFS
/// restore — and never advances, so its height is a durable statement about
/// what that node can answer, not a transient flag.
pub(crate) fn guard_deferral(
    status: ValidBlockProcessing,
    approved_block_number: i64,
) -> ValidBlockProcessing {
    match status {
        Either::Left(BlockError::Undecidable(hash)) if approved_block_number == 0 => {
            Either::Left(BlockError::BlockException(CasperError::BlockNotHeld(hash)))
        }
        // Same rule for the state artifact: a genesis-rooted node computed or
        // imported every root it ever needed, so a missing one is corruption
        // and must be judged — deferring would hand a crafted block a
        // permanent non-verdict on any full node.
        Either::Left(BlockError::AwaitingState(root)) if approved_block_number == 0 => {
            Either::Left(BlockError::BlockException(CasperError::Other(format!(
                "state root {} missing on a genesis-rooted node — local corruption, not sync",
                root
            ))))
        }
        other => other,
    }
}

pub(crate) fn guard_certified_deferral(
    validation: CertifiedBlockValidation,
    approved_block_number: i64,
) -> CertifiedBlockValidation {
    if approved_block_number != 0 {
        return validation;
    }
    match validation {
        CertifiedBlockValidation::MissingDependency(ValidationDeferral::AwaitingBlock(hash)) => {
            CertifiedBlockValidation::LocalFault(CasperError::BlockNotHeld(hash))
        }
        CertifiedBlockValidation::MissingDependency(ValidationDeferral::AwaitingState(root)) => {
            use rholang::rust::interpreter::errors::InterpreterError;
            use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RootError};

            CertifiedBlockValidation::LocalFault(CasperError::InterpreterError(
                InterpreterError::RSpaceError(RSpaceError::HistoryError(HistoryError::RootError(
                    RootError::RootNotFound(root),
                ))),
            ))
        }
        other => other,
    }
}

/// Whether an arriving block is settled history to be admitted unjudged —
/// the LFS door, opened at runtime.
///
/// A restored node's own restore inserted hundreds of blocks hash-checked and
/// unexecuted; a straggler from the same settled region — cited by gossip the
/// restore could not have known about — is the same kind of block and gets the
/// same treatment. Judging it instead is what broke: the node-state validation
/// checks assume dependency-ordered insertion, which the restore itself
/// bypassed, so the verdicts they produce on old blocks are statements about
/// this node's restore, not about the block.
///
/// Each condition closes a distinct attack; see the truth-table test.
///
/// `seq_below_senders_latest` requires the block's sequence number to sit
/// strictly below the sender's current latest message. Genuine settled
/// stragglers always do — settled history predates the anchor's
/// justification frontier — while a block at-or-above that frontier is
/// live-chain material wearing a sub-anchor height (the CI run 32588262605
/// pollution shape: shared validator keys, foreign seq 40 against a live
/// seq-5 head). A sender with NO latest message passes the condition:
/// deep settled history is routinely authored by since-unbonded validators
/// with no live slot, and live material always has one — refusing on an
/// absent slot re-wedges the restore gaps the door exists to close.
pub(crate) fn admit_as_settled(
    block_number: i64,
    approved_block_number: i64,
    solicited_by_bonded: bool,
    budget_remaining: bool,
    seq_below_senders_latest: bool,
) -> bool {
    approved_block_number > 0
        && block_number <= approved_block_number
        && solicited_by_bonded
        && budget_remaining
        && seq_below_senders_latest
}

/// What the block-processing loop should do with a block whose validation
/// attempt returned a hard `Err` (no verdict, not a typed deferral).
///
/// `Retry` keeps the block buffered — a transient fault heals on a later
/// harvest, and the failure quarantine paces those retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationFailureDisposition {
    Retry,
    RetainAndQuarantine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettledAdmissionResult {
    NotSolicited,
    NotEligible,
    DuplicateInFlight,
    AlreadyAdmitted,
    Admitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettledTicketState {
    InFlight(u64),
    Admitted,
}

struct SettledTicketGuard {
    registry: Arc<Mutex<HashMap<BlockHash, SettledTicketState>>>,
    block_hash: BlockHash,
    claim_id: u64,
    committed: bool,
}

impl SettledTicketGuard {
    fn commit(&mut self) -> Result<(), CasperError> {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match registry.get(&self.block_hash) {
            Some(SettledTicketState::InFlight(claim_id)) if *claim_id == self.claim_id => {
                registry.insert(self.block_hash.clone(), SettledTicketState::Admitted);
                self.committed = true;
                Ok(())
            }
            _ => Err(CasperError::RuntimeError(
                "settled-ticket claim changed before durable commit".to_string(),
            )),
        }
    }
}

impl Drop for SettledTicketGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if matches!(
            registry.get(&self.block_hash),
            Some(SettledTicketState::InFlight(claim_id)) if *claim_id == self.claim_id
        ) {
            registry.remove(&self.block_hash);
        }
    }
}

fn next_settled_claim_id(sequence: &AtomicU64) -> Result<u64, CasperError> {
    sequence
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| {
            CasperError::RuntimeError("settled-ticket claim sequence exhausted".to_string())
        })?
        .checked_add(1)
        .ok_or_else(|| {
            CasperError::RuntimeError("settled-ticket claim sequence exhausted".to_string())
        })
}

/// Classify a validation outcome for post-processing.
///
/// Everything except `Undecidable` is a verdict and is settled. `Undecidable`
/// is the absence of one, so the block must survive to be retried: dropping it
/// loses the block, and the missing hash it names is the only thing that can
/// unstick the node.
pub(crate) fn post_validation(status: &ValidBlockProcessing) -> PostValidation {
    match status {
        Either::Left(BlockError::Undecidable(missing)) => {
            PostValidation::AwaitingBlock(missing.clone())
        }
        Either::Left(BlockError::AwaitingState(root)) => {
            PostValidation::AwaitingState(root.clone())
        }
        _ => PostValidation::Settled,
    }
}

/// Lifetime cap on settled-history admissions. Legitimate joins need single
/// digits (the gaps LFS's closure missed); the cap prices the worst case — a
/// BONDED attacker citing self-signed junk below the anchor — at bounded,
/// alarmed storage. Past it the node degrades to today's deferral, loudly.
pub(crate) const SETTLED_ADMISSION_BUDGET: u64 = SETTLED_RECOVERY_EPISODE_CAPACITY;

const CASPER_BUFFER_PRUNE_INTERVAL_MS: u64 = 5_000;
const CASPER_BUFFER_STALE_TTL_MS: u64 = 180_000;
const CASPER_BUFFER_MAX_APPROX_NODES: usize = 16_384;
const CASPER_BUFFER_MAX_PRUNE_BATCH: usize = 512;
const CASPER_BUFFER_MAX_APPROX_NODES_ENV: &str = "F1R3_CASPER_BUFFER_MAX_APPROX_NODES";
const CASPER_BUFFER_STALE_TTL_MS_ENV: &str = "F1R3_CASPER_BUFFER_STALE_TTL_MS";
const CASPER_BUFFER_MAX_PRUNE_BATCH_ENV: &str = "F1R3_CASPER_BUFFER_MAX_PRUNE_BATCH";
const CASPER_BUFFER_PRUNE_INTERVAL_MS_ENV: &str = "F1R3_CASPER_BUFFER_PRUNE_INTERVAL_MS";
const CASPER_BUFFER_STALE_PRUNED_METRIC: &str = "casper.buffer.stale-pruned";
const CASPER_BUFFER_OVERFLOW_PRUNED_METRIC: &str = "casper.buffer.overflow-pruned";
const CASPER_BUFFER_APPROX_NODES_METRIC: &str = "casper.buffer.approx-nodes";
const CASPER_BUFFER_DEPENDENCY_LOOP_PRUNED_METRIC: &str = "casper.buffer.dependency-loop-pruned";
const MISSING_DEPENDENCY_ATTEMPTS_MAX_DEFAULT: u32 = 32;
const MISSING_DEPENDENCY_ATTEMPTS_MAX_ENV: &str = "F1R3_MISSING_DEPENDENCY_ATTEMPTS_MAX";
const VALIDATION_ERROR_ATTEMPTS_MAX_DEFAULT: u32 = 32;
const VALIDATION_ERROR_ATTEMPTS_MAX_ENV: &str = "F1R3_VALIDATION_ERROR_ATTEMPTS_MAX";
const MISSING_DEPENDENCY_QUARANTINE_MS_DEFAULT: u64 = 120_000;
const MISSING_DEPENDENCY_QUARANTINE_MS_ENV: &str = "F1R3_MISSING_DEPENDENCY_QUARANTINE_MS";
static CASPER_BUFFER_MAX_APPROX_NODES_CFG: OnceLock<usize> = OnceLock::new();
static CASPER_BUFFER_STALE_TTL_MS_CFG: OnceLock<u64> = OnceLock::new();
static CASPER_BUFFER_MAX_PRUNE_BATCH_CFG: OnceLock<usize> = OnceLock::new();
static CASPER_BUFFER_PRUNE_INTERVAL_MS_CFG: OnceLock<u64> = OnceLock::new();
static MISSING_DEPENDENCY_ATTEMPTS_MAX_CFG: OnceLock<u32> = OnceLock::new();
static VALIDATION_ERROR_ATTEMPTS_MAX_CFG: OnceLock<u32> = OnceLock::new();
static MISSING_DEPENDENCY_QUARANTINE_MS_CFG: OnceLock<u64> = OnceLock::new();

pub fn validation_error_attempts_max() -> u32 {
    *VALIDATION_ERROR_ATTEMPTS_MAX_CFG.get_or_init(|| {
        env::var_or_filtered(
            VALIDATION_ERROR_ATTEMPTS_MAX_ENV,
            VALIDATION_ERROR_ATTEMPTS_MAX_DEFAULT,
            |value: &u32| *value > 0,
        )
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum CasperDependency {
    Block(BlockHash),
    FinalizationCertificate(BlockHash),
}

impl CasperDependency {
    fn bytes(&self) -> &BlockHash {
        match self {
            Self::Block(hash) | Self::FinalizationCertificate(hash) => hash,
        }
    }
}

fn casper_buffer_max_approx_nodes() -> usize {
    *CASPER_BUFFER_MAX_APPROX_NODES_CFG.get_or_init(|| {
        env::var_or(
            CASPER_BUFFER_MAX_APPROX_NODES_ENV,
            CASPER_BUFFER_MAX_APPROX_NODES,
        )
    })
}

fn casper_buffer_stale_ttl_ms() -> u64 {
    *CASPER_BUFFER_STALE_TTL_MS_CFG
        .get_or_init(|| env::var_or(CASPER_BUFFER_STALE_TTL_MS_ENV, CASPER_BUFFER_STALE_TTL_MS))
}

fn casper_buffer_max_prune_batch() -> usize {
    *CASPER_BUFFER_MAX_PRUNE_BATCH_CFG.get_or_init(|| {
        env::var_or(
            CASPER_BUFFER_MAX_PRUNE_BATCH_ENV,
            CASPER_BUFFER_MAX_PRUNE_BATCH,
        )
    })
}

fn casper_buffer_prune_interval_ms() -> u64 {
    *CASPER_BUFFER_PRUNE_INTERVAL_MS_CFG.get_or_init(|| {
        env::var_or(
            CASPER_BUFFER_PRUNE_INTERVAL_MS_ENV,
            CASPER_BUFFER_PRUNE_INTERVAL_MS,
        )
    })
}

fn missing_dependency_attempts_max() -> u32 {
    *MISSING_DEPENDENCY_ATTEMPTS_MAX_CFG.get_or_init(|| {
        env::var_or_filtered(
            MISSING_DEPENDENCY_ATTEMPTS_MAX_ENV,
            MISSING_DEPENDENCY_ATTEMPTS_MAX_DEFAULT,
            |v: &u32| *v > 0,
        )
    })
}

fn missing_dependency_quarantine_ms() -> u64 {
    *MISSING_DEPENDENCY_QUARANTINE_MS_CFG.get_or_init(|| {
        env::var_or_filtered(
            MISSING_DEPENDENCY_QUARANTINE_MS_ENV,
            MISSING_DEPENDENCY_QUARANTINE_MS_DEFAULT,
            |v: &u64| *v > 0,
        )
    })
}

impl<T: TransportLayer + Send + Sync> BlockProcessor<T> {
    pub fn new(dependencies: BlockProcessorDependencies<T>) -> Self { Self { dependencies } }

    /// The height this node was started from. Zero means genesis — a complete
    /// spine, so nothing below it can legitimately be absent.
    fn approved_block_number(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
    ) -> Result<i64, CasperError> {
        casper
            .get_approved_block()
            .map(|approved| proto_util::block_number(approved))
    }

    /// check if block should be processed
    pub fn check_if_of_interest(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        Ok(self.capture_publication_interest(casper, block)?.is_some())
    }

    pub fn capture_publication_interest(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
    ) -> Result<Option<PublicationInterest>, CasperError> {
        let already_processed = casper.contains(&block.block_hash);

        let shard_of_interest = casper.get_approved_block().map(|approved_block| {
            approved_block
                .shard_id
                .eq_ignore_ascii_case(&block.shard_id)
        })?;

        let version_of_interest = Validate::version(block, casper.get_version());

        let old_block = casper.get_approved_block().map(|approved_block| {
            proto_util::block_number(block) < proto_util::block_number(approved_block)
        })?;

        // A block this node requested to satisfy a missing dependency is of
        // interest whatever its height. Dropping it as "old" is why a joiner
        // can never acquire pre-anchor history: it requests the dependency,
        // receives it, discards it here, and the dependent block retries
        // forever — 23,643 attempts on one block before the shard's finality
        // stalled behind the joiner's idle stake. The `old_block` filter still
        // does its real job, since unsolicited gossip is never in this set.
        let requested_as_dependency = self
            .dependencies
            .was_requested_as_dependency(&block.block_hash)?;

        let interested = !already_processed
            && shard_of_interest
            && version_of_interest
            && (!old_block || requested_as_dependency);
        Ok(interested.then(|| PublicationInterest {
            hash: block.block_hash.clone(),
            casper,
            requested_as_dependency,
        }))
    }

    pub async fn check_and_store_for_publication(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
        interest: PublicationInterest,
    ) -> Result<(bool, Option<AcceptedBlockPublication>), CasperError> {
        interest.check_binding(&casper, &block.block_hash)?;
        let well_formed = self.check_if_well_formed_and_store(block).await?;
        let evidence = (well_formed && block.has_valid_content_hash())
            .then_some(AcceptedBlockPublication { interest });
        Ok((well_formed, evidence))
    }

    fn publication_provenance(
        casper: &Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
        evidence: Option<&AcceptedBlockPublication>,
    ) -> Result<bool, CasperError> {
        if let Some(evidence) = evidence {
            evidence.interest.check_binding(casper, &block.block_hash)?;
            if !block.has_valid_content_hash() {
                return Err(CasperError::RuntimeError(
                    "Publication body does not match its verified identity".into(),
                ));
            }
            Ok(evidence.interest.requested_as_dependency)
        } else {
            Ok(false)
        }
    }

    /// check block format and store if check passed
    pub async fn check_if_well_formed_and_store(
        &self,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        let valid_format = Validate::format_of_fields(block);
        let valid_sig = Validate::block_signature(block);
        let is_valid = valid_format && valid_sig;

        if is_valid {
            // Time storage operation
            let storage_start = Instant::now();
            self.dependencies.store_block(block).await?;
            metrics::histogram!(BLOCK_PROCESSING_STORAGE_TIME_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .record(storage_start.elapsed().as_secs_f64());
        }

        Ok(is_valid)
    }

    /// check if block has all dependencies available and can be validated
    pub async fn check_dependencies_with_effects(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        self.check_dependencies_with_publication(casper, block, None)
            .await
    }

    pub async fn check_dependencies_with_publication(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
        evidence: Option<&AcceptedBlockPublication>,
    ) -> Result<bool, CasperError> {
        let provenance = Self::publication_provenance(&casper, block, evidence)?;
        self.dependencies.prune_casper_buffer_if_needed()?;
        self.dependencies
            .sweep_expired_missing_dependency_quarantine()?;
        self.dependencies
            .sweep_orphaned_missing_dependency_attempts()?;
        self.dependencies
            .sweep_orphaned_missing_dependency_quarantine()?;
        self.dependencies
            .sweep_expired_validation_error_quarantine()?;
        self.dependencies
            .sweep_orphaned_validation_error_attempts()?;

        if self
            .dependencies
            .is_missing_dependency_quarantined(&block.block_hash)?
        {
            tracing::debug!(
                "Skipping block {} due to missing-dependency quarantine ({}ms).",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                missing_dependency_quarantine_ms()
            );
            metrics::counter!(CASPER_BUFFER_DEPENDENCY_LOOP_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE, "reason" => "quarantine")
                .increment(1);
            // Keep buffered block graph intact while quarantined.
            // Dropping buffered blocks here can break dependency chains and stall finality.
            let restored = if let Some(evidence) = evidence {
                self.restore_accepted_publication(casper.clone(), &block.block_hash, evidence)
                    .await?;
                true
            } else {
                self.restore_stored_buffer_ownership(casper.clone(), &block.block_hash)
                    .await?
            };
            if restored {
                return Ok(false);
            }
        }

        let (is_ready, deps_to_fetch, deps_in_buffer) = self
            .dependencies
            .get_non_validated_dependencies(casper.clone(), block)
            .await?;
        if is_ready {
            self.dependencies
                .clear_missing_dependency_attempts(&block.block_hash)?;
            // store pendant block in buffer, it will be removed once block is validated and added to DAG
            self.dependencies
                .commit_to_buffer_with_provenance(block, None, provenance)
                .await?;
        } else {
            if self
                .dependencies
                .register_missing_dependency_attempt(&block.block_hash)?
            {
                tracing::warn!(
                    "Throttling block {} after {} missing-dependency checks (keeping in buffer).",
                    PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                    missing_dependency_attempts_max()
                );
                metrics::counter!(CASPER_BUFFER_DEPENDENCY_LOOP_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE, "reason" => "attempts")
                    .increment(1);
                self.dependencies
                    .mark_missing_dependency_quarantine(&block.block_hash)?;
            }

            // associate parents with new block in casper buffer
            let mut all_deps = deps_to_fetch.clone();
            all_deps.extend(deps_in_buffer.clone());
            self.dependencies
                .commit_to_buffer_with_provenance(block, Some(all_deps), provenance)
                .await?;
            self.dependencies
                .request_missing_dependencies(&deps_to_fetch)
                .await?;
            // Recovery path: if dependency graph is stuck in buffer (no fresh deps to fetch),
            // force a network re-request for buffered dependencies.
            if deps_to_fetch.is_empty() && !deps_in_buffer.is_empty() {
                self.dependencies
                    .recover_stale_buffer_dependencies(&deps_in_buffer)
                    .await?;
            }
        }

        Ok(is_ready)
    }

    async fn retain_deferred_validation(
        &self,
        casper: &Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
        deferral: &ValidationDeferral,
        evidence: Option<&AcceptedBlockPublication>,
    ) -> Result<(), CasperError> {
        let provenance = Self::publication_provenance(casper, block, evidence)?;
        if matches!(deferral, ValidationDeferral::AlreadyBuffered) {
            return self
                .dependencies
                .commit_to_buffer_with_provenance(block, None, provenance)
                .await;
        }
        match post_validation(&Either::Left(deferral.status())) {
            PostValidation::Settled => Ok(()),
            PostValidation::AwaitingBlock(missing) => {
                let deps = HashSet::from([CasperDependency::Block(missing.clone())]);
                self.dependencies
                    .commit_to_buffer_with_provenance(block, Some(deps.clone()), provenance)
                    .await?;
                self.dependencies
                    .request_missing_dependencies(&deps)
                    .await?;
                Ok(())
            }
            PostValidation::AwaitingState(root) => {
                if self
                    .dependencies
                    .register_missing_dependency_attempt(&block.block_hash)?
                {
                    self.dependencies
                        .mark_missing_dependency_quarantine(&block.block_hash)?;
                }
                self.dependencies
                    .commit_to_buffer_with_provenance(block, None, provenance)
                    .await?;
                self.dependencies
                    .request_state_root(&root, &block.block_hash);
                Ok(())
            }
        }
    }

    /// validate block and invoke all effects required
    pub async fn validate_with_effects(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
        // this option is required for tests, as sometimes block without parents available are added, so
        // CasperSnapshot cannot be constructed
        snapshot_opt: Option<CasperSnapshot>,
    ) -> Result<ValidBlockProcessing, CasperError> {
        self.validate_with_publication(casper, block, snapshot_opt, None)
            .await
    }

    pub async fn validate_with_publication(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
        snapshot_opt: Option<CasperSnapshot>,
        evidence: Option<&AcceptedBlockPublication>,
    ) -> Result<ValidBlockProcessing, CasperError> {
        let provenance = Self::publication_provenance(&casper, block, evidence)?;
        // Record block size
        let block_size = block.to_proto().encode_to_vec().len();
        metrics::histogram!(BLOCK_SIZE_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(block_size as f64);

        // Time validation setup
        let setup_start = Instant::now();
        let mut snapshot = match snapshot_opt {
            Some(snapshot) => snapshot,
            None => match self
                .dependencies
                .get_casper_state_snapshot(casper.clone())
                .await
            {
                Ok(snapshot) => snapshot,
                // The snapshot walks the same history the floor does, so it hits
                // the same edge first on a node whose history is short. Report it
                // as the absence of a verdict rather than erroring the block out
                // of the pipeline un-judged and untracked — but only if this node
                // is entitled to defer at all.
                Err(CasperError::BlockNotHeld(missing)) => {
                    let guarded = guard_deferral(
                        Either::Left(BlockError::Undecidable(missing.clone())),
                        self.approved_block_number(casper.clone())?,
                    );
                    if !matches!(guarded, Either::Left(BlockError::Undecidable(_))) {
                        return Err(CasperError::BlockNotHeld(missing));
                    }
                    tracing::warn!(
                        "Snapshot for block {} needs {}, which this node does not hold.",
                        PrettyPrinter::build_string_bytes(&block.block_hash),
                        PrettyPrinter::build_string_bytes(&missing)
                    );
                    let deps = HashSet::from([CasperDependency::Block(missing.clone())]);
                    self.dependencies
                        .commit_to_buffer_with_provenance(block, Some(deps.clone()), provenance)
                        .await?;
                    self.dependencies
                        .request_missing_dependencies(&deps)
                        .await?;
                    return Ok(guarded);
                }
                Err(err) => return Err(err),
            },
        };
        metrics::histogram!(BLOCK_PROCESSING_VALIDATION_SETUP_TIME_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(setup_start.elapsed().as_secs_f64());

        // Time block validation
        let validation_start = Instant::now();
        let validation = self
            .dependencies
            .validate_block(casper.clone(), &mut snapshot, block)
            .await?;
        let validation =
            guard_certified_deferral(validation, self.approved_block_number(casper.clone())?);
        let status = validation.status();
        metrics::histogram!(BLOCK_VALIDATION_TIME_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(validation_start.elapsed().as_secs_f64());

        // Record validation outcome
        let _ = match &validation {
            CertifiedBlockValidation::Accepted {
                sender_authority,
                admission_outcome,
                ..
            } => {
                metrics::counter!(BLOCK_VALIDATION_SUCCESS_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                    .increment(1);
                self.dependencies
                    .effects_for_valid_block(casper, block, sender_authority, admission_outcome)
                    .await
            }
            CertifiedBlockValidation::ObjectiveRejected {
                invalid,
                sender_authority,
                admission_outcome,
            } => {
                metrics::counter!(BLOCK_VALIDATION_FAILED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                    .increment(1);
                self.dependencies
                    .effects_for_invalid_block(
                        casper,
                        block,
                        invalid,
                        &snapshot,
                        sender_authority,
                        admission_outcome,
                    )
                    .await
            }
            CertifiedBlockValidation::UnattributableRejected { .. } => {
                metrics::counter!(BLOCK_VALIDATION_FAILED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                    .increment(1);
                Ok(snapshot.dag.clone())
            }
            CertifiedBlockValidation::LocalFault(err) => {
                tracing::error!(
                    "Block {} validation was inconclusive because of a local fault ({}); deferring it to bounded recovery without recording invalidity.",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    err
                );
                match BlockError::from_validation_error(err.clone()) {
                    BlockError::Undecidable(hash) => {
                        self.retain_deferred_validation(
                            &casper,
                            block,
                            &ValidationDeferral::AwaitingBlock(hash),
                            evidence,
                        )
                        .await?;
                    }
                    BlockError::AwaitingState(root) => {
                        self.retain_deferred_validation(
                            &casper,
                            block,
                            &ValidationDeferral::AwaitingState(root),
                            evidence,
                        )
                        .await?;
                    }
                    _ => {
                        self.dependencies
                            .recover_after_local_validation_fault(&block.block_hash)
                            .await?;
                    }
                }
                return Ok(status);
            }
            CertifiedBlockValidation::CasperBusy => {
                self.dependencies
                    .recover_after_local_validation_fault(&block.block_hash)
                    .await?;
                return Ok(status);
            }
            CertifiedBlockValidation::MissingDependency(deferral) => {
                self.retain_deferred_validation(&casper, block, deferral, evidence)
                    .await?;
                return Ok(status);
            }
            CertifiedBlockValidation::AlreadyProcessed => Ok(snapshot.dag.clone()),
        }?;

        // once block is validated and effects are invoked, it should be removed from buffer
        self.dependencies.remove_from_buffer(block).await?;
        self.dependencies.ack_processed(block).await?;
        Ok(status)
    }

    /// Equivalent to Scala's: ackProcessed = (b: BlockMessage) => BlockRetriever[F].ackInCasper(b.blockHash)
    pub async fn ack_processed(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.dependencies.ack_processed(block).await
    }

    pub async fn ack_received(&self, hash: BlockHash) -> Result<(), CasperError> {
        self.dependencies.block_retriever.ack_receive(hash).await
    }

    pub fn record_received(&self, hash: BlockHash) -> Result<RequestTracking, CasperError> {
        self.dependencies.block_retriever.record_received(hash)
    }

    pub fn reopen_after_local_failure(
        &self,
        hash: BlockHash,
    ) -> Result<RequestTracking, CasperError> {
        self.dependencies
            .block_retriever
            .reopen_after_local_failure(hash)
    }

    pub fn retry_ownership(&self, hash: &BlockHash) -> Result<RetryOwnership, CasperError> {
        if self
            .dependencies
            .block_dag_storage
            .get_representation()?
            .contains(hash)
        {
            return Ok(RetryOwnership::Terminal);
        }
        if self
            .dependencies
            .casper_buffer
            .pending_request_policy(&BlockHashSerde(hash.clone()))?
            .is_some()
        {
            return Ok(RetryOwnership::Pending);
        }
        Ok(RetryOwnership::Missing)
    }

    pub async fn restore_stored_buffer_ownership(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        match self.prepare_stored_publication(casper.clone(), hash)? {
            StoredPublicationPreparation::Terminal => Ok(true),
            StoredPublicationPreparation::Missing => Ok(false),
            StoredPublicationPreparation::Accepted { block, evidence } => {
                self.publish_prepared_block(casper, &block, &evidence)
                    .await?;
                Ok(true)
            }
        }
    }

    fn publication_already_terminal(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        if self
            .dependencies
            .block_dag_storage
            .get_representation()?
            .contains(hash)
        {
            self.dependencies
                .block_retriever
                .forget_hash_tracking(hash)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn verified_stored_publication_body(
        &self,
        hash: &BlockHash,
    ) -> Result<Option<BlockMessage>, CasperError> {
        let Some(block) = self.dependencies.block_store.get_detached(hash)? else {
            return Ok(None);
        };
        if block.block_hash != hash
            || !block.has_valid_content_hash()
            || !Validate::format_of_fields(&block)
            || !Validate::block_signature(&block)
        {
            return Err(CasperError::RuntimeError(
                "Stored block identity is invalid during buffer ownership repair".to_string(),
            ));
        }
        Ok(Some(block))
    }

    pub fn prepare_stored_publication(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        hash: &BlockHash,
    ) -> Result<StoredPublicationPreparation, CasperError> {
        if self.publication_already_terminal(hash)? {
            return Ok(StoredPublicationPreparation::Terminal);
        }
        let requested_as_dependency = self.dependencies.was_requested_as_dependency(hash)?;
        let Some(block) = self.verified_stored_publication_body(hash)? else {
            return Ok(StoredPublicationPreparation::Missing);
        };
        Ok(StoredPublicationPreparation::Accepted {
            block: Box::new(block),
            evidence: AcceptedBlockPublication {
                interest: PublicationInterest {
                    hash: hash.clone(),
                    casper,
                    requested_as_dependency,
                },
            },
        })
    }

    pub async fn restore_accepted_publication(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        hash: &BlockHash,
        evidence: &AcceptedBlockPublication,
    ) -> Result<(), CasperError> {
        evidence.interest.check_binding(&casper, hash)?;
        if self.publication_already_terminal(hash)? {
            return Ok(());
        }
        let block = self
            .verified_stored_publication_body(hash)?
            .ok_or_else(|| {
                CasperError::RuntimeError(
                    "Accepted publication body disappeared from storage".into(),
                )
            })?;
        self.publish_prepared_block(casper, &block, evidence).await
    }

    pub async fn publish_prepared_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: &BlockMessage,
        evidence: &AcceptedBlockPublication,
    ) -> Result<(), CasperError> {
        let provenance = Self::publication_provenance(&casper, block, Some(evidence))?;
        let (_, mut missing, buffered) = self
            .dependencies
            .get_non_validated_dependencies(casper, block)
            .await?;
        missing.extend(buffered);
        self.dependencies
            .commit_to_buffer_with_provenance(block, Some(missing), provenance)
            .await
    }

    pub fn forget_hash_tracking(&self, hash: &BlockHash) -> Result<(), CasperError> {
        self.dependencies.block_retriever.forget_hash_tracking(hash)
    }

    /// See [`BlockProcessorDependencies::try_admit_settled`].
    pub async fn try_admit_settled(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<SettledAdmissionResult, CasperError> {
        self.dependencies.try_admit_settled(casper, block).await
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn fail_next_settled_insert(&self) { self.dependencies.fail_next_settled_insert(); }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn settled_admission_count(
        &self,
        shard_id: &str,
        protocol_version: i64,
    ) -> Result<u64, CasperError> {
        let episode = self
            .dependencies
            .block_dag_storage
            .current_recovery_episode(shard_id, protocol_version)?;
        Ok(self
            .dependencies
            .block_dag_storage
            .settled_recovery_usage(&episode)?)
    }

    /// Remove block hash from CasperBuffer dependency graph.
    pub async fn remove_from_buffer(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.dependencies.remove_from_buffer(block).await
    }

    /// Best-effort purge for stale/uninteresting blocks to prevent infinite buffer requeue loops.
    pub async fn purge_from_buffer_and_ack(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.dependencies.remove_from_buffer(block).await?;
        self.dependencies.ack_processed(block).await
    }

    /// See [`BlockProcessorDependencies::note_validation_failure`].
    pub fn note_validation_failure(
        &self,
        block_hash: &BlockHash,
    ) -> Result<ValidationFailureDisposition, CasperError> {
        self.dependencies.note_validation_failure(block_hash)
    }

    /// See [`BlockProcessorDependencies::is_validation_failure_quarantined`].
    pub fn is_validation_failure_quarantined(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        self.dependencies
            .is_validation_failure_quarantined(block_hash)
    }

    /// See [`BlockProcessorDependencies::clear_validation_failures`].
    pub fn clear_validation_failures(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
        self.dependencies.clear_validation_failures(block_hash)
    }
}

/// Unified dependencies structure - equivalent to Scala companion object approach
/// Contains all dependencies needed for block processing in one place
#[derive(Clone)]
pub struct BlockProcessorDependencies<T: TransportLayer + Send + Sync> {
    block_store: KeyValueBlockStore,
    casper_buffer: CasperBufferKeyValueStorage,
    block_dag_storage: BlockDagKeyValueStorage,
    block_retriever: BlockRetriever<T>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
    casper_buffer_last_prune_ms: Arc<AtomicU64>,
    missing_dependency_attempts: Arc<Mutex<HashMap<BlockHash, u32>>>,
    missing_dependency_quarantine_until: Arc<Mutex<HashMap<BlockHash, u64>>>,
    /// Hard validation `Err`s per buffered block, bounding the
    /// fail→pendant→fail loop the way `missing_dependency_attempts` bounds
    /// the dependency-check loop. Deadlines are `Instant`s: the quarantine
    /// paces retries, so a wall-clock step (NTP correction) must neither
    /// void an active quarantine nor extend one for hours.
    validation_error_attempts: Arc<Mutex<HashMap<BlockHash, u32>>>,
    validation_error_quarantine_until: Arc<Mutex<HashMap<BlockHash, std::time::Instant>>>,
    settled_ticket_registry: Arc<Mutex<HashMap<BlockHash, SettledTicketState>>>,
    settled_ticket_claim_sequence: Arc<AtomicU64>,
    #[cfg(any(test, feature = "test-utils"))]
    settled_insert_failures: Arc<AtomicU64>,
    /// Names missing state roots to the runtime state requester. `None` only
    /// in test constructions; without it a missing root still defers safely,
    /// it just never heals.
    state_root_fetch_tx: Option<mpsc::Sender<StateRootFetchCommand>>,
}

impl<T: TransportLayer + Send + Sync> BlockProcessorDependencies<T> {
    pub fn new(
        block_store: KeyValueBlockStore,
        block_dag_storage: BlockDagKeyValueStorage,
        block_retriever: BlockRetriever<T>,
        transport: Arc<T>,
        connections_cell: ConnectionsCell,
        conf: RPConf,
        state_root_fetch_tx: Option<mpsc::Sender<StateRootFetchCommand>>,
    ) -> Result<Self, CasperError> {
        Ok(Self {
            block_store,
            casper_buffer: block_retriever.casper_buffer().clone(),
            block_dag_storage,
            block_retriever,
            transport,
            connections_cell,
            conf,
            casper_buffer_last_prune_ms: Arc::new(AtomicU64::new(0)),
            missing_dependency_attempts: Arc::new(Mutex::new(HashMap::new())),
            missing_dependency_quarantine_until: Arc::new(Mutex::new(HashMap::new())),
            validation_error_attempts: Arc::new(Mutex::new(HashMap::new())),
            validation_error_quarantine_until: Arc::new(Mutex::new(HashMap::new())),
            settled_ticket_registry: Arc::new(Mutex::new(HashMap::new())),
            settled_ticket_claim_sequence: Arc::new(AtomicU64::new(0)),
            #[cfg(any(test, feature = "test-utils"))]
            settled_insert_failures: Arc::new(AtomicU64::new(0)),
            state_root_fetch_tx,
        })
    }

    /// Name a missing root to the state requester, if one is wired.
    fn request_state_root(&self, root: &Blake2b256Hash, owner: &BlockHash) {
        match &self.state_root_fetch_tx {
            Some(tx) => {
                if tx
                    .try_send(StateRootFetchCommand::Acquire {
                        root: root.clone(),
                        owner: owner.clone(),
                    })
                    .is_err()
                {
                    tracing::warn!(
                        %root,
                        "state requester queue full or closed; the root stays absent and \
                         its dependents keep deferring"
                    );
                }
            }
            None => tracing::warn!(
                %root,
                "no state requester wired; the root stays absent and its dependents \
                 keep deferring"
            ),
        }
    }

    async fn release_state_root_owner(&self, owner: &BlockHash) {
        if let Some(tx) = &self.state_root_fetch_tx {
            if tx
                .send(StateRootFetchCommand::ReleaseOwner(owner.clone()))
                .await
                .is_err()
            {
                tracing::warn!(
                    block = %PrettyPrinter::build_string_bytes(owner),
                    "state requester closed before owner cleanup"
                );
            }
        }
    }

    // Public getters for tests
    pub fn transport(&self) -> &Arc<T> { &self.transport }

    pub fn casper_buffer(&self) -> &CasperBufferKeyValueStorage { &self.casper_buffer }

    fn prune_casper_buffer_if_needed(&self) -> Result<(), CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last_prune = self.casper_buffer_last_prune_ms.load(Ordering::Relaxed);
        let prune_interval_ms = casper_buffer_prune_interval_ms();
        if now_ms.saturating_sub(last_prune) < prune_interval_ms {
            return Ok(());
        }
        self.casper_buffer_last_prune_ms
            .store(now_ms, Ordering::Relaxed);

        let (stale_pruned, overflow_pruned) = self.casper_buffer.enforce_limits(
            casper_buffer_max_approx_nodes(),
            casper_buffer_stale_ttl_ms(),
            casper_buffer_max_prune_batch(),
            prune_interval_ms,
        )?;
        let approx_nodes = self.casper_buffer.approx_node_count();

        metrics::gauge!(CASPER_BUFFER_APPROX_NODES_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .set(approx_nodes as f64);
        if stale_pruned > 0 {
            metrics::counter!(CASPER_BUFFER_STALE_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .increment(stale_pruned as u64);
        }
        if overflow_pruned > 0 {
            metrics::counter!(CASPER_BUFFER_OVERFLOW_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .increment(overflow_pruned as u64);
        }
        if stale_pruned > 0 || overflow_pruned > 0 {
            tracing::warn!(
                "Pruned CasperBuffer entries: stale={}, overflow={}, approx_nodes={}",
                stale_pruned,
                overflow_pruned,
                approx_nodes
            );
        }

        Ok(())
    }

    /// Equivalent to Scala's: storeBlock = (b: BlockMessage) => BlockStore[F].put(b)
    pub async fn store_block(&self, block: &BlockMessage) -> Result<(), CasperError> {
        if block.header.version >= crate::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION
            && block.header.finalized_floor.is_some()
            && block.finalized_floor_certificate.is_none()
        {
            self.block_store
                .put_block_message_awaiting_certificate(block)?;
        } else {
            self.block_store.put_block_message(block)?;
        }
        Ok(())
    }

    /// Equivalent to Scala's: getCasperStateSnapshot = (c: Casper[F]) => c.getSnapshot
    pub async fn get_casper_state_snapshot(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
    ) -> Result<CasperSnapshot, CasperError> {
        casper.get_snapshot().await
    }

    /// Equivalent to Scala's: getNonValidatedDependencies = (c: Casper[F], b: BlockMessage) => { ... }
    pub async fn get_non_validated_dependencies(
        &self,
        _casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<(bool, HashSet<CasperDependency>, HashSet<CasperDependency>), CasperError> {
        let dag = self.block_dag_storage.get_representation()?;
        let mut block_with_certificate = block.clone();
        let missing_certificate = if block.header.version
            >= crate::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION
            && block.finalized_floor_certificate.is_none()
        {
            match block.header.finalized_floor.as_ref() {
                Some(commitment) => match self
                    .block_store
                    .get_finalization_certificate(&commitment.certificate_digest)?
                {
                    Some(certificate) => {
                        block_with_certificate.finalized_floor_certificate = Some(certificate);
                        None
                    }
                    None => Some(commitment.certificate_digest.clone()),
                },
                None => None,
            }
        } else {
            None
        };
        let (deps_validated, deps_missing) =
            proto_util::dependency_metadata_partition(&block_with_certificate, &dag)?;

        let mut missing: HashSet<CasperDependency> = deps_missing
            .into_iter()
            .map(CasperDependency::Block)
            .collect();
        if let Some(digest) = missing_certificate {
            missing.insert(CasperDependency::FinalizationCertificate(digest));
        }

        let deps_in_buffer_all: Vec<CasperDependency> = missing
            .iter()
            .filter(|dependency| match dependency {
                CasperDependency::Block(hash) => {
                    let hash = BlockHashSerde(hash.clone());
                    self.casper_buffer.contains(&hash) || self.casper_buffer.is_pendant(&hash)
                }
                CasperDependency::FinalizationCertificate(digest) => self
                    .casper_buffer
                    .requested_as_certificate_dependency(&BlockHashSerde(digest.clone())),
            })
            .cloned()
            .collect();

        let deps_in_buffer = deps_in_buffer_all;

        let deps_to_fetch: Vec<CasperDependency> = missing
            .iter()
            .filter(|&dep| !deps_in_buffer.contains(dep))
            .cloned()
            .collect();

        let ready = deps_to_fetch.is_empty() && deps_in_buffer.is_empty();

        if !ready {
            tracing::debug!(
                "Block {} waiting on missing dependencies. To fetch: {}. In buffer: {}. Validated: {}.",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                PrettyPrinter::build_string_hashes(
                    &deps_to_fetch
                        .iter()
                        .map(|dependency| dependency.bytes().to_vec())
                        .collect::<Vec<_>>()
                ),
                PrettyPrinter::build_string_hashes(
                    &deps_in_buffer
                        .iter()
                        .map(|dependency| dependency.bytes().to_vec())
                        .collect::<Vec<_>>()
                ),
                PrettyPrinter::build_string_hashes(
                    &deps_validated
                        .iter()
                        .map(|h| h.as_ref().to_vec())
                        .collect::<Vec<_>>()
                )
            );
        }

        Ok((
            ready,
            deps_to_fetch.into_iter().collect(),
            deps_in_buffer.into_iter().collect(),
        ))
    }

    /// Equivalent to Scala's: commitToBuffer = (b: BlockMessage, deps: Option[Set[BlockHash]]) => { ... }
    pub async fn commit_to_buffer(
        &self,
        block: &BlockMessage,
        deps: Option<HashSet<CasperDependency>>,
    ) -> Result<(), CasperError> {
        self.commit_to_buffer_with_provenance(block, deps, false)
            .await
    }

    async fn commit_to_buffer_with_provenance(
        &self,
        block: &BlockMessage,
        deps: Option<HashSet<CasperDependency>>,
        requested_as_dependency: bool,
    ) -> Result<(), CasperError> {
        let mut blocks = HashSet::new();
        let mut certificates = HashSet::new();
        for dependency in deps.into_iter().flatten() {
            match dependency {
                CasperDependency::Block(hash) => {
                    blocks.insert(BlockHashSerde(hash));
                }
                CasperDependency::FinalizationCertificate(digest) => {
                    certificates.insert(BlockHashSerde(digest));
                }
            }
        }
        let published = self
            .block_dag_storage
            .publish_if_unadmitted(&block.block_hash, || {
                self.block_retriever.publish_pending_with_provenance(
                    block.block_hash.clone(),
                    blocks,
                    certificates,
                    requested_as_dependency,
                )
            })?;
        if published.is_none() {
            self.block_retriever
                .forget_hash_tracking(&block.block_hash)?;
        }
        Ok(())
    }

    /// Equivalent to Scala's: removeFromBuffer = (b: BlockMessage) => casperBuffer.remove(b.blockHash)
    pub async fn remove_from_buffer(&self, block: &BlockMessage) -> Result<(), CasperError> {
        let block_hash_serde = BlockHashSerde(block.block_hash.clone());
        self.casper_buffer.remove(block_hash_serde)?;
        self.clear_missing_dependency_attempts(&block.block_hash)?;
        self.clear_missing_dependency_quarantine(&block.block_hash)?;
        self.release_state_root_owner(&block.block_hash).await;

        Ok(())
    }

    fn sweep_orphaned_missing_dependency_attempts(&self) -> Result<(), CasperError> {
        let to_clear: Vec<BlockHash> = {
            let attempts = self.missing_dependency_attempts.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_attempts lock".to_string(),
                )
            })?;

            attempts
                .keys()
                .filter_map(|block_hash| {
                    let block_hash_serde = BlockHashSerde(block_hash.clone());
                    let is_active = self.casper_buffer.contains(&block_hash_serde)
                        || self.casper_buffer.is_pendant(&block_hash_serde);

                    if is_active {
                        None
                    } else {
                        Some(block_hash.clone())
                    }
                })
                .collect()
        };

        if to_clear.is_empty() {
            return Ok(());
        }

        let mut attempts = self.missing_dependency_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire missing_dependency_attempts lock".to_string(),
            )
        })?;

        for block_hash in to_clear {
            attempts.remove(&block_hash);
        }

        Ok(())
    }

    fn sweep_orphaned_missing_dependency_quarantine(&self) -> Result<(), CasperError> {
        let to_clear: Vec<BlockHash> = {
            let quarantine: Vec<BlockHash> = self
                .missing_dependency_quarantine_until
                .lock()
                .map_err(|_| {
                    CasperError::RuntimeError(
                        "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                    )
                })?
                .keys()
                .cloned()
                .collect();

            quarantine
                .into_iter()
                .filter_map(|block_hash| {
                    let block_hash_serde = BlockHashSerde(block_hash.clone());
                    let is_active = self.casper_buffer.contains(&block_hash_serde)
                        || self.casper_buffer.is_pendant(&block_hash_serde);

                    if is_active {
                        None
                    } else {
                        Some(block_hash)
                    }
                })
                .collect()
        };

        if to_clear.is_empty() {
            return Ok(());
        }

        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;

        for block_hash in to_clear {
            quarantine.remove(&block_hash);
        }

        Ok(())
    }

    fn register_missing_dependency_attempt(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        let mut attempts = self.missing_dependency_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire missing_dependency_attempts lock".to_string(),
            )
        })?;
        let next = attempts.entry(block_hash.clone()).or_insert(0);
        *next = next.saturating_add(1);
        Ok(*next >= missing_dependency_attempts_max())
    }

    fn clear_missing_dependency_attempts(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
        let mut attempts = self.missing_dependency_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire missing_dependency_attempts lock".to_string(),
            )
        })?;
        attempts.remove(block_hash);
        Ok(())
    }

    fn clear_missing_dependency_quarantine(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(), CasperError> {
        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        quarantine.remove(block_hash);
        Ok(())
    }

    fn mark_missing_dependency_quarantine(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(), CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let until = now_ms.saturating_add(missing_dependency_quarantine_ms());
        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        quarantine.insert(block_hash.clone(), until);
        Ok(())
    }

    fn is_missing_dependency_quarantined(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        Ok(quarantine
            .get(block_hash)
            .copied()
            .is_some_and(|until| now_ms < until))
    }

    /// Record one hard validation `Err` for a buffered block and decide its
    /// fate by pacing further retries through the failure quarantine.
    ///
    /// The quarantine is stamped in both dispositions — between retries it
    /// paces the pendant harvest, and after the cap it starts a fresh bounded
    /// retry episode without deleting an unresolved dependency node.
    pub fn note_validation_failure(
        &self,
        block_hash: &BlockHash,
    ) -> Result<ValidationFailureDisposition, CasperError> {
        let reached_cap = {
            let mut attempts = self.validation_error_attempts.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire validation_error_attempts lock".to_string(),
                )
            })?;
            let next = attempts.entry(block_hash.clone()).or_insert(0);
            *next = next.saturating_add(1);
            *next >= validation_error_attempts_max()
        };

        let until = std::time::Instant::now()
            + std::time::Duration::from_millis(missing_dependency_quarantine_ms());
        let mut quarantine = self.validation_error_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire validation_error_quarantine_until lock".to_string(),
            )
        })?;
        quarantine.insert(block_hash.clone(), until);

        Ok(if reached_cap {
            ValidationFailureDisposition::RetainAndQuarantine
        } else {
            ValidationFailureDisposition::Retry
        })
    }

    /// Whether the pendant harvest should skip this hash because its last
    /// validation attempt hard-failed within the quarantine window.
    pub fn is_validation_failure_quarantined(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        let now = std::time::Instant::now();
        let quarantine = self.validation_error_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire validation_error_quarantine_until lock".to_string(),
            )
        })?;
        Ok(quarantine
            .get(block_hash)
            .copied()
            .is_some_and(|until| now < until))
    }

    /// A settled verdict ends the failure ledger for this hash.
    pub fn clear_validation_failures(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
        {
            let mut attempts = self.validation_error_attempts.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire validation_error_attempts lock".to_string(),
                )
            })?;
            attempts.remove(block_hash);
        }
        let mut quarantine = self.validation_error_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire validation_error_quarantine_until lock".to_string(),
            )
        })?;
        quarantine.remove(block_hash);
        Ok(())
    }

    fn sweep_expired_validation_error_quarantine(&self) -> Result<(), CasperError> {
        let now = std::time::Instant::now();
        let mut quarantine = self.validation_error_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire validation_error_quarantine_until lock".to_string(),
            )
        })?;
        quarantine.retain(|_, until| *until > now);
        Ok(())
    }

    fn sweep_orphaned_validation_error_attempts(&self) -> Result<(), CasperError> {
        let to_clear: Vec<BlockHash> = {
            let attempts = self.validation_error_attempts.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire validation_error_attempts lock".to_string(),
                )
            })?;

            attempts
                .keys()
                .filter_map(|block_hash| {
                    let block_hash_serde = BlockHashSerde(block_hash.clone());
                    let is_active = self.casper_buffer.contains(&block_hash_serde)
                        || self.casper_buffer.is_pendant(&block_hash_serde);

                    if is_active {
                        None
                    } else {
                        Some(block_hash.clone())
                    }
                })
                .collect()
        };

        if to_clear.is_empty() {
            return Ok(());
        }

        let mut attempts = self.validation_error_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire validation_error_attempts lock".to_string(),
            )
        })?;

        for block_hash in to_clear {
            attempts.remove(&block_hash);
        }

        Ok(())
    }

    fn sweep_expired_missing_dependency_quarantine(&self) -> Result<(), CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        quarantine.retain(|_, until| *until > now_ms);
        Ok(())
    }

    pub fn was_requested_as_dependency(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        self.block_retriever.was_requested_as_dependency(hash)
    }

    /// Admit an arriving block as settled history if [`admit_as_settled`]'s
    /// conditions hold: inserted into the DAG hash-checked and unjudged, the
    /// same treatment LFS restore gave every block it downloaded. Returns
    /// whether the block was admitted; a `false` sends it down the ordinary
    /// judged path.
    ///
    /// Insertion cannot touch consensus state: `InsertMode::SettledHistory`
    /// leaves latest messages exactly as they were, and every verdict channel
    /// is untouched because the block never enters validation.
    pub async fn try_admit_settled(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<SettledAdmissionResult, CasperError> {
        let anchor = casper.get_approved_block()?.clone();
        let approved_block_number = proto_util::block_number(&anchor);
        if approved_block_number <= 0 || proto_util::block_number(block) > approved_block_number {
            return Ok(SettledAdmissionResult::NotEligible);
        }
        if let Some(metadata) = self
            .block_dag_storage
            .get_representation()?
            .lookup(&block.block_hash)?
        {
            let Some(record) = metadata.settled_history_admission.as_ref() else {
                return Ok(SettledAdmissionResult::NotEligible);
            };
            if record.anchor_block_hash() != &anchor.block_hash {
                return Err(CasperError::RuntimeError(
                    "settled-history admission uses a non-approved anchor".to_string(),
                ));
            }
            let citer = self
                .block_store
                .get(record.citer_block_hash())?
                .ok_or_else(|| {
                    CasperError::RuntimeError(
                        "settled-history admission citer is missing".to_string(),
                    )
                })?;
            ValidatedSettledHistoryAdmission::from_record(record, block, &anchor, &citer)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            self.reconcile_settled_cleanup(block).await?;
            return Ok(SettledAdmissionResult::AlreadyAdmitted);
        }

        let Some(proof) = self.settled_history_proof(block, &anchor)? else {
            return Ok(SettledAdmissionResult::NotSolicited);
        };

        let seq_below_senders_latest = {
            let representation = self.block_dag_storage.get_representation()?;
            match representation.latest_message_hash(&block.sender) {
                Some(latest_hash) => match representation.lookup(&latest_hash)? {
                    Some(latest_meta) => block.seq_num < latest_meta.sequence_number,
                    None => return Err(CasperError::BlockNotHeld(latest_hash)),
                },
                None => true,
            }
        };
        if !admit_as_settled(
            proto_util::block_number(block),
            approved_block_number,
            true,
            true,
            seq_below_senders_latest,
        ) {
            return Ok(SettledAdmissionResult::NotEligible);
        }
        self.block_store.put_block_message(block)?;

        let mut ticket = match self.claim_settled_ticket(&block.block_hash)? {
            Ok(ticket) => ticket,
            Err(result) => return Ok(result),
        };
        let charge = self
            .block_dag_storage
            .prepare_settled_recovery_charge(block, &proof)?;

        #[cfg(any(test, feature = "test-utils"))]
        if self
            .settled_insert_failures
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                (remaining > 0).then(|| remaining - 1)
            })
            .is_ok()
        {
            return Err(CasperError::RuntimeError(
                "injected settled-history insertion failure".to_string(),
            ));
        }

        let insertion = atomic_insert_settled_then_buffer(
            &self.block_dag_storage,
            block,
            &proof,
            &charge,
            &self.casper_buffer,
        );
        let (representation, buffer_cleanup_error) = match insertion {
            Ok(result) => result,
            Err(shared::rust::store::key_value_store::KvStoreError::RecoveryBudgetExhausted {
                ..
            }) => return Ok(SettledAdmissionResult::NotEligible),
            Err(error) => return Err(error.into()),
        };
        drop(representation);
        ticket.commit()?;
        let admitted = self
            .block_dag_storage
            .settled_recovery_usage(&charge.episode)?;
        if admitted == SETTLED_ADMISSION_BUDGET / 2 {
            tracing::warn!(
                admitted,
                budget = SETTLED_ADMISSION_BUDGET,
                "Settled-history admissions at half budget; a healthy join needs single \
                 digits — investigate what keeps citing unheld settled blocks"
            );
        }
        tracing::info!(
            block = %PrettyPrinter::build_string_bytes(&block.block_hash),
            block_number = proto_util::block_number(block),
            anchor_number = approved_block_number,
            admitted,
            "Admitted solicited block as settled history (below this node's sync anchor)"
        );
        if let Some(error) = buffer_cleanup_error {
            tracing::warn!(
                block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                error = %error,
                "settled-history DAG commit succeeded before buffer cleanup"
            );
            if let Err(retry_error) = self.reconcile_settled_cleanup(block).await {
                tracing::warn!(
                    block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    error = %retry_error,
                    "settled-history cleanup retry remains pending"
                );
            }
        }
        self.request_state_root(
            &Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash),
            &block.block_hash,
        );
        self.request_state_root(
            &Blake2b256Hash::from_bytes_prost(&block.body.state.pre_state_hash),
            &block.block_hash,
        );
        if let Err(error) = self.ack_processed(block).await {
            tracing::warn!(
                block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                error = %error,
                "settled-history commit succeeded before retriever acknowledgement"
            );
        }
        Ok(SettledAdmissionResult::Admitted)
    }

    async fn reconcile_settled_cleanup(&self, block: &BlockMessage) -> Result<(), CasperError> {
        match self
            .casper_buffer
            .remove(BlockHashSerde(block.block_hash.clone()))
        {
            Ok(())
            | Err(shared::rust::store::key_value_store::KvStoreError::InvalidArgument(_)) => {}
            Err(error) => return Err(error.into()),
        }
        self.block_retriever
            .forget_hash_tracking(&block.block_hash)?;
        Ok(())
    }

    fn claim_settled_ticket(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Result<SettledTicketGuard, SettledAdmissionResult>, CasperError> {
        let claim_id = next_settled_claim_id(&self.settled_ticket_claim_sequence)?;
        let mut registry = self
            .settled_ticket_registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match registry.get(block_hash) {
            Some(SettledTicketState::InFlight(_)) => {
                return Ok(Err(SettledAdmissionResult::DuplicateInFlight))
            }
            Some(SettledTicketState::Admitted) => {
                return Ok(Err(SettledAdmissionResult::AlreadyAdmitted))
            }
            None => {
                registry.insert(block_hash.clone(), SettledTicketState::InFlight(claim_id));
            }
        }
        drop(registry);
        Ok(Ok(SettledTicketGuard {
            registry: self.settled_ticket_registry.clone(),
            block_hash: block_hash.clone(),
            claim_id,
            committed: false,
        }))
    }

    fn settled_history_proof(
        &self,
        target: &BlockMessage,
        anchor: &BlockMessage,
    ) -> Result<Option<ValidatedSettledHistoryAdmission>, CasperError> {
        let Some(children) = self
            .casper_buffer
            .get_children(&BlockHashSerde(target.block_hash.clone()))
        else {
            return Ok(None);
        };
        let mut child_hashes = children.into_iter().map(|hash| hash.0).collect::<Vec<_>>();
        child_hashes.sort();
        for child_hash in child_hashes {
            let Some(citer) = self.block_store.get(&child_hash)? else {
                continue;
            };
            if !Validate::format_of_fields(&citer) || !Validate::block_signature(&citer) {
                continue;
            }
            if !proto_util::dependencies_hashes_of(&citer).contains(&target.block_hash) {
                continue;
            }
            let Some(stake) = anchor
                .body
                .state
                .bonds
                .iter()
                .find(|bond| bond.validator == citer.sender && bond.stake > 0)
                .map(|bond| bond.stake)
            else {
                continue;
            };
            let Some(generation) = anchor
                .body
                .state
                .bond_generations
                .iter()
                .find(|entry| entry.validator == citer.sender)
                .map(|entry| entry.generation)
            else {
                continue;
            };
            if citer.header.sender_bond_generation != Some(generation) {
                continue;
            }
            let proof =
                ValidatedSettledHistoryAdmission::new(target, anchor, &citer, generation, stake)
                    .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            return Ok(Some(proof));
        }
        Ok(None)
    }

    #[cfg(any(test, feature = "test-utils"))]
    fn fail_next_settled_insert(&self) {
        self.settled_insert_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Equivalent to Scala's: requestMissingDependencies = (deps: Set[BlockHash]) => { ... }
    pub async fn request_missing_dependencies(
        &self,
        deps: &HashSet<CasperDependency>,
    ) -> Result<(), CasperError> {
        let mut first_error = None;
        for dep in deps {
            let result = match dep {
                CasperDependency::Block(hash) => self
                    .block_retriever
                    .admit_hash(
                        hash.clone(),
                        None,
                        AdmitHashReason::MissingDependencyRequested,
                    )
                    .await
                    .map(|_| ()),
                CasperDependency::FinalizationCertificate(digest) => self
                    .block_retriever
                    .request_finalization_certificate(digest.clone())
                    .await
                    .map(|_| ()),
            };
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Recovery helper for deadlock scenarios where dependencies remain in CasperBuffer
    /// but there are no newly discovered hashes to fetch.
    pub async fn recover_stale_buffer_dependencies(
        &self,
        deps: &HashSet<CasperDependency>,
    ) -> Result<(), CasperError> {
        let mut first_error = None;
        for dep in deps {
            let result = match dep {
                CasperDependency::Block(hash) => {
                    self.block_retriever.recover_dependency(hash.clone()).await
                }
                CasperDependency::FinalizationCertificate(digest) => self
                    .block_retriever
                    .request_finalization_certificate(digest.clone())
                    .await
                    .map(|_| ()),
            };
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub async fn recover_after_local_validation_fault(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(), CasperError> {
        let block_hash_serde = BlockHashSerde(block_hash.clone());
        self.casper_buffer.remove(block_hash_serde)?;

        metrics::counter!(
            BLOCK_VALIDATION_LOCAL_FAULT_DEFERRED_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .increment(1);

        self.block_retriever
            .recover_dependency(block_hash.clone())
            .await
    }

    /// Equivalent to Scala's: validateBlock = (c: Casper[F], s: CasperSnapshot[F], b: BlockMessage) => c.validate(b, s)
    pub async fn validate_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        snapshot: &mut CasperSnapshot,
        block: &BlockMessage,
    ) -> Result<CertifiedBlockValidation, CasperError> {
        casper.validate(block, snapshot).await
    }

    /// Equivalent to Scala's: ackProcessed = (b: BlockMessage) => BlockRetriever[F].ackInCasper(b.blockHash)
    pub async fn ack_processed(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.block_retriever
            .ack_in_casper(block.block_hash.clone())
            .await?;

        Ok(())
    }

    /// Equivalent to Scala's: effectsForInvalidBlock = (c: Casper[F], b: BlockMessage, r: InvalidBlock, s: CasperSnapshot[F]) => { ... }
    pub async fn effects_for_invalid_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
        invalid_block: &InvalidBlock,
        snapshot: &CasperSnapshot,
        certificate: &CertifiedSenderAuthority,
        outcome: &models::rust::block_metadata::CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        let dag = casper.handle_invalid_block(
            block,
            invalid_block,
            &snapshot.dag,
            certificate,
            outcome,
        )?;

        // Equivalent to Scala's: CommUtil[F].sendBlockHash(b.blockHash, b.sender)
        if let Err(err) = self
            .transport
            .send_block_hash(
                &self.connections_cell,
                &self.conf,
                &block.block_hash,
                &block.sender,
            )
            .await
        {
            tracing::warn!(
                "Failed to send block hash {} to sender during invalid-block effects: {}",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                err
            );
        }

        Ok(dag)
    }

    /// Equivalent to Scala's: effectsForValidBlock = (c: Casper[F], b: BlockMessage) => { ... }
    pub async fn effects_for_valid_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
        certificate: &CertifiedSenderAuthority,
        outcome: &models::rust::block_metadata::CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        let dag = {
            casper
                .handle_valid_block(block, certificate, outcome)
                .await?
        };

        // Equivalent to Scala's: CommUtil[F].sendBlockHash(b.blockHash, b.sender)
        if let Err(err) = self
            .transport
            .send_block_hash(
                &self.connections_cell,
                &self.conf,
                &block.block_hash,
                &block.sender,
            )
            .await
        {
            tracing::warn!(
                "Failed to send block hash {} to sender during valid-block effects: {}",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                err
            );
        }

        Ok(dag)
    }
}

/// Constructor function equivalent to Scala's companion object apply method
/// Creates unified dependencies and BlockProcessor
pub fn new_block_processor<T: TransportLayer + Send + Sync>(
    block_store: KeyValueBlockStore,
    block_dag_storage: BlockDagKeyValueStorage,
    block_retriever: BlockRetriever<T>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
    state_root_fetch_tx: Option<mpsc::Sender<StateRootFetchCommand>>,
) -> Result<BlockProcessor<T>, CasperError> {
    let dependencies = BlockProcessorDependencies::new(
        block_store,
        block_dag_storage,
        block_retriever,
        transport,
        connections_cell,
        conf,
        state_root_fetch_tx,
    )?;

    Ok(BlockProcessor::new(dependencies))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::rust::block_status::ValidBlock;

    /// A block validation could not judge must not be cleaned up like one it
    /// did. Settling it drops it from the buffer, and because an invalid block
    /// counts as a satisfied dependency, the next block in line then becomes
    /// "ready" and is mis-handled the same way — the gap is never fetched and
    /// the node never catches up.
    #[test]
    fn an_undecidable_block_waits_for_the_block_it_named() {
        let missing = BlockHash::from(b"the-block-we-lack".to_vec());

        assert_eq!(
            post_validation(&Either::Left(BlockError::Undecidable(missing.clone()))),
            PostValidation::AwaitingBlock(missing),
            "an undecidable block must stay buffered against the block it needs"
        );
    }

    /// Deferral is only honest for a node that actually has a hole in its
    /// history. A node built from genesis holds a complete spine, so a block it
    /// cannot find is corruption, and it must still judge — otherwise any
    /// crafted block that induces the error would buy its proposer a permanent
    /// non-verdict: never judged, never invalid, no evidence, sitting in the
    /// buffer while every honest node judges it normally.
    ///
    /// A node restored from a sync anchor is truncated for the life of its data
    /// directory: `last_approved_block` is written once, at genesis ceremony or
    /// at restore, and never advances. Its anchor height is therefore a durable
    /// statement that history below it will never arrive.
    #[test]
    fn only_a_node_with_a_hole_in_its_history_may_defer() {
        let missing = BlockHash::from(b"below-my-anchor".to_vec());
        let undecidable = || Either::Left(BlockError::Undecidable(missing.clone()));

        assert_eq!(
            guard_deferral(undecidable(), 87),
            Either::Left(BlockError::Undecidable(missing.clone())),
            "a node restored at an anchor genuinely cannot judge below it"
        );

        assert!(
            matches!(
                guard_deferral(undecidable(), 0),
                Either::Left(BlockError::BlockException(CasperError::BlockNotHeld(_)))
            ),
            "a genesis-rooted node has the whole spine, so a missing block is corruption \
             and must be judged — deferring here is an escape hatch for crafted blocks"
        );
    }

    #[test]
    fn certified_deferral_guard_preserves_recovery_identity() {
        let missing = BlockHash::from(vec![0x51; 32]);
        let truncated = guard_certified_deferral(
            CertifiedBlockValidation::MissingDependency(ValidationDeferral::AwaitingBlock(
                missing.clone(),
            )),
            87,
        );
        assert!(matches!(
            truncated,
            CertifiedBlockValidation::MissingDependency(ValidationDeferral::AwaitingBlock(hash))
                if hash == missing
        ));

        let genesis = guard_certified_deferral(
            CertifiedBlockValidation::MissingDependency(ValidationDeferral::AwaitingBlock(
                missing.clone(),
            )),
            0,
        );
        assert!(matches!(
            genesis,
            CertifiedBlockValidation::LocalFault(CasperError::BlockNotHeld(hash))
                if hash == missing
        ));

        let root = Blake2b256Hash::from_bytes(vec![0x52; 32]);
        let genesis_state = guard_certified_deferral(
            CertifiedBlockValidation::MissingDependency(ValidationDeferral::AwaitingState(
                root.clone(),
            )),
            0,
        );
        let CertifiedBlockValidation::LocalFault(error) = genesis_state else {
            panic!("genesis-rooted state absence must remain a local fault");
        };
        assert_eq!(
            BlockError::from_validation_error(error),
            BlockError::AwaitingState(root)
        );
    }

    /// Settled-history admission is the LFS door opened at runtime. A restored
    /// node judges old blocks with checks that assume dependency-ordered
    /// insertion — an assumption its own restore already broke for 298 blocks —
    /// so a straggler from the same settled region must come through the same
    /// door those 298 did: hash-checked, inserted, never judged. Each condition
    /// closes a distinct attack:
    ///
    ///   - only a truncated node (a genesis-rooted node judges everything, so
    ///     no crafted block can buy an unjudged admission there);
    ///   - only at-or-below the anchor (live consensus is always judged);
    ///   - only when solicited by a bonded validator's signature-checked block
    ///     (an unbonded attacker's citations open nothing);
    ///   - only within budget (a staked attacker buys bounded, alarmed storage,
    ///     never unbounded growth — past the budget the node degrades to
    ///     today's deferral, loudly);
    ///   - only seq-strictly-below the sender's latest message (settled
    ///     history predates the anchor's justification frontier; a higher
    ///     seq is live-chain material wearing a sub-anchor height).
    #[test]
    fn settled_history_admission_has_five_conditions() {
        assert!(
            admit_as_settled(9, 87, true, true, true),
            "a below-anchor block solicited by a bonded citer on a truncated node is settled history"
        );
        assert!(
            admit_as_settled(87, 87, true, true, true),
            "the anchor's own height is inside the settled cut"
        );
        assert!(
            !admit_as_settled(88, 87, true, true, true),
            "above the anchor is live consensus and must be judged"
        );
        assert!(
            !admit_as_settled(9, 0, true, true, true),
            "a genesis-rooted node judges everything — same discriminator as guard_deferral"
        );
        assert!(
            !admit_as_settled(9, 87, false, true, true),
            "a citation from an unbonded sender opens no door"
        );
        assert!(
            !admit_as_settled(9, 87, true, false, true),
            "budget exhausted falls back to deferral, never silent growth"
        );
        assert!(
            !admit_as_settled(9, 87, true, true, false),
            "a seq at-or-above the sender's latest message is not settled history"
        );
    }

    /// Every actual verdict — valid, invalid, or a genuine storage fault — is
    /// settled. Only the absence of a verdict waits.
    #[test]
    fn every_verdict_settles() {
        for status in [
            Either::Right(ValidBlock::Valid),
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction)),
            Either::Left(BlockError::BlockException(CasperError::RuntimeError(
                "disk".into(),
            ))),
            Either::Left(BlockError::MissingBlocks),
        ] {
            assert_eq!(
                post_validation(&status),
                PostValidation::Settled,
                "{status:?} is a verdict and must be settled, not retried"
            );
        }
    }

    proptest! {
        #[test]
        fn settled_ticket_registry_matches_the_durable_commit(commit in any::<bool>()) {
            let block_hash = BlockHash::from(vec![0xA5; 32]);
            let registry = Arc::new(Mutex::new(HashMap::from([(
                block_hash.clone(),
                SettledTicketState::InFlight(1),
            )])));
            {
                let mut ticket = SettledTicketGuard {
                    registry: registry.clone(),
                    block_hash: block_hash.clone(),
                    claim_id: 1,
                    committed: false,
                };
                if commit {
                    prop_assert!(ticket.commit().is_ok());
                }
            }

            if commit {
                prop_assert_eq!(
                    registry.lock().unwrap().get(&block_hash).copied(),
                    Some(SettledTicketState::Admitted)
                );
            } else {
                prop_assert!(!registry.lock().unwrap().contains_key(&block_hash));
            }
        }
    }

    #[test]
    fn settled_ticket_claim_sequence_fails_closed_at_exhaustion() {
        let sequence = AtomicU64::new(u64::MAX - 1);
        assert_eq!(next_settled_claim_id(&sequence).unwrap(), u64::MAX);
        assert!(matches!(
            next_settled_claim_id(&sequence),
            Err(CasperError::RuntimeError(message))
                if message == "settled-ticket claim sequence exhausted"
        ));
    }
}
