// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManager.scala
// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManagerSyntax.scala

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;
use std::mem::{size_of, size_of_val};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::{Cosigner, Signed};
use dashmap::DashMap;
use hex::ToHex;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, ProcessedUserDeploy,
    SystemDeployData,
};
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::{
    offered_funded_v6_host_work_limits, offered_funded_v6_limits,
};
use models::rust::deploy_envelope::DeployEnvelopeRef;
use models::rust::deploy_id::DeployLookupId;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::native_cost_evidence::NativeCostEvidenceV1;
use models::rust::phlo_intent::{PhloConversionCompositionV2, PhloFundingIntentV2Limits};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::NativeRecordingWireLimits;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::rholang_merging_logic::{
    DeployMergeableData, NumberChannel, RholangMergingLogic,
};
use rholang::rust::interpreter::rho_runtime::{
    self, RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::system_processes::{BlockData, Definition};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_history_record, NativeLeafKind, NativeReadCharge, NativeReadError, NativeReadMeter,
};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::merging_logic::{NumberChannelsDiff, NumberChannelsEndVal};
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::clone_backing::{self, BackingError};
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteVector;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::rust::errors::{CasperError, OfferedCandidateRejection};
use crate::rust::merging::block_index::BlockIndex;
use crate::rust::metrics_constants::{
    BLOCK_INDEX_CACHE_SIZE_METRIC, BLOCK_REPLAY_RUNTIME_EXECUTE_TIME_METRIC,
    BLOCK_REPLAY_RUNTIME_LOCK_WAIT_TIME_METRIC, BLOCK_REPLAY_RUNTIME_SAVE_MERGEABLE_TIME_METRIC,
    CASPER_METRICS_SOURCE, PARENTS_POST_STATE_CACHE_SIZE_METRIC, REPLAY_CACHE_ENTRIES_METRIC,
    REPLAY_CACHE_RETAINED_BYTES_METRIC, RUNTIME_SPAWN_REPLAY_CALLS_METRIC,
    RUNTIME_SPAWN_REPLAY_TIME_METRIC, RUNTIME_SPAWN_TIME_METRIC,
};
use crate::rust::rholang::replay_runtime::{validate_offered_replay_preflight, ReplayRuntimeOps};
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use crate::rust::util::rholang::costacc::direct_wallet_funding::{
    authorize_offered_direct_wallet_funding, authorize_offered_direct_wallet_funding_with_grants,
    CheckedDirectWalletPolicy, DirectWalletFundingLimits, DirectWalletPolicySnapshot,
    NativeFundedExecutionContext, NativeFundedReplayLimits, NativeGrantSettlementInput,
    ReplayedNativeFundedUser,
};
use crate::rust::util::rholang::costacc::genesis_resource_policy::{
    AdoptedResourcePolicy, OFFERED_PRODUCTION_READY,
};
use crate::rust::util::rholang::costacc::offered_acceptance::{
    offered_replay_context_copy_bytes, OfferedAcceptanceBudget,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::{
    usage_delta, OfferedBudgetRecorder, OfferedBudgetUsage, OfferedUsageKind,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    grant_issue_definition, offered_grant_issue_call_limits, offered_grant_transition_limits,
    GrantIssueCallLimits, VerifiedOfferedGrantSources,
};
use crate::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_prepaid_inventory_limits, offered_funded_v6_prepaid_receipt_limits,
    offered_funded_v6_production_limits, offered_funded_v6_recording_limits,
    offered_funded_v6_replay_limits, offered_funded_v6_trace_limits,
};
use crate::rust::util::rholang::replay_cache::{
    InMemoryReplayCache, ReplayCache, ReplayCacheEntry, ReplayCacheKey,
};

type MergeableStore = KeyValueTypedStoreImpl<ByteVector, Vec<DeployMergeableData>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct MergeableKey {
    state_hash: StateHashSerde,
    #[serde(with = "shared::rust::serde_bytes")]
    creator: prost::bytes::Bytes,
    seq_num: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct ExploratoryDeployConfig {
    pub max_concurrent: usize,
    pub phlo_limit: i64,
    pub execution_timeout: Duration,
}

impl ExploratoryDeployConfig {
    /// Rejects a non-positive value rather than clamping it. A clamped `0`
    /// yields a node that answers nothing on this endpoint — one phlogiston
    /// fails every query on cost, a one-millisecond deadline times out every
    /// query — with no diagnostic distinguishing that from a working node.
    pub fn new(
        max_concurrent: usize,
        phlo_limit: i64,
        execution_timeout: Duration,
    ) -> Result<Self, CasperError> {
        if max_concurrent == 0 {
            return Err(CasperError::Other(
                "exploratory-deploy-max-concurrent must be at least 1".to_string(),
            ));
        }
        if phlo_limit <= 0 {
            return Err(CasperError::Other(format!(
                "exploratory-deploy-phlo-limit must be positive, got {}",
                phlo_limit
            )));
        }
        if execution_timeout.is_zero() {
            return Err(CasperError::Other(
                "exploratory-deploy-execution-timeout must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            max_concurrent,
            phlo_limit,
            execution_timeout,
        })
    }

    /// Resolve the operator-facing form of `max_concurrent`: `0` derives the
    /// value from this host's cores (observers are the dedicated servers of
    /// exploratory traffic, and the right ceiling is a host property); an
    /// explicit value is honored, clamped to the semaphore's permit range.
    /// The constructed config always carries a real ceiling — `new` still
    /// rejects 0.
    pub fn resolve(
        max_concurrent: usize,
        phlo_limit: i64,
        execution_timeout: Duration,
    ) -> Result<Self, CasperError> {
        let resolved = if max_concurrent == 0 {
            let cores = std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(4);
            Self::derive_max_concurrent(cores)
        } else {
            max_concurrent.min(Semaphore::MAX_PERMITS)
        };
        Self::new(resolved, phlo_limit, execution_timeout)
    }

    /// Two cores stay reserved for block-following; a floor of two keeps any
    /// host able to serve overlapping clients.
    pub fn derive_max_concurrent(cores: usize) -> usize { cores.saturating_sub(2).max(2) }

    /// Fixture for the test-only constructors. Deliberately not a `Default`
    /// impl: the operator-facing default lives in `defaults.conf` and reaches
    /// the runtime through `create_with_history_config`, so a second
    /// authoritative-looking declaration in Rust could drift from it silently.
    /// These values are not that default — they are only what the test
    /// constructors have always used.
    pub fn for_tests() -> Self {
        Self {
            max_concurrent: 1,
            phlo_limit: 5_000_000,
            execution_timeout: Duration::from_secs(15),
        }
    }
}

pub struct ReplayLock {
    semaphore: Arc<Semaphore>,
    consensus_waiters: std::sync::atomic::AtomicUsize,
    consensus_ready: tokio::sync::Notify,
}

struct ConsensusReplayWaiter<'a>(&'a ReplayLock);

impl Drop for ConsensusReplayWaiter<'_> {
    fn drop(&mut self) {
        self.0
            .consensus_waiters
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        self.0.consensus_ready.notify_waiters();
    }
}

impl ReplayLock {
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            consensus_waiters: std::sync::atomic::AtomicUsize::new(0),
            consensus_ready: tokio::sync::Notify::new(),
        }
    }

    pub async fn acquire_consensus(
        &self,
    ) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        self.consensus_waiters
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let waiter = ConsensusReplayWaiter(self);
        let permit = self.semaphore.clone().acquire_owned().await;
        drop(waiter);
        permit
    }

    pub async fn acquire_reporting(
        &self,
    ) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        loop {
            while self
                .consensus_waiters
                .load(std::sync::atomic::Ordering::Acquire)
                > 0
            {
                let ready = self.consensus_ready.notified();
                tokio::pin!(ready);
                ready.as_mut().enable();
                if self
                    .consensus_waiters
                    .load(std::sync::atomic::Ordering::Acquire)
                    > 0
                {
                    ready.await;
                }
            }
            let permit = self.semaphore.clone().acquire_owned().await?;
            if self
                .consensus_waiters
                .load(std::sync::atomic::Ordering::Acquire)
                == 0
            {
                return Ok(permit);
            }
            drop(permit);
            tokio::task::yield_now().await;
        }
    }
}

impl Default for ReplayLock {
    fn default() -> Self { Self::new() }
}

#[derive(Clone)]
pub struct RuntimeManager {
    pub space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    pub replay_space: ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    pub history_repo: RhoHistoryRepository,
    pub mergeable_store: MergeableStore,
    pub(crate) mergeable_tags: std::sync::Arc<
        std::collections::HashMap<Par, rspace_plus_plus::rspace::merger::merging_logic::MergeType>,
    >,
    // TODO: make proper storage for block indices - OLD
    pub block_index_cache: Arc<DashMap<BlockHash, BlockIndex>>,
    pub block_index_cache_order: Arc<Mutex<VecDeque<BlockHash>>>,
    pub active_validators_cache: Arc<DashMap<StateHash, Vec<Validator>>>,
    pub active_validators_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
    pub bonds_cache: Arc<DashMap<StateHash, Vec<Bond>>>,
    pub bonds_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
    /// Cache for merged parent post-state computation keyed by parent-set snapshot context.
    pub parents_post_state_cache: Arc<DashMap<ParentsPostStateCacheKey, ParentsPostStateCacheVal>>,
    pub parents_post_state_cache_order: Arc<Mutex<VecDeque<ParentsPostStateCacheKey>>>,
    /// Optional replay cache for delta replay optimization
    pub replay_cache: Option<Arc<InMemoryReplayCache>>,
    /// Optional state hash cache for skipping known replays
    replay_lock: Arc<ReplayLock>,
    exploratory_deploy_semaphore: Arc<Semaphore>,
    exploratory_deploy_phlo_limit: i64,
    exploratory_deploy_execution_timeout: Duration,
    pub external_services: ExternalServices,
    /// DR-102: test-only records of the final usage of each offered budget.
    #[cfg(any(test, feature = "test-utils"))]
    offered_budget_recorder: OfferedBudgetRecorder,
}

pub(crate) struct CertifiedOfferedDraft {
    settled_root: [u8; 32],
    envelope_identity: [u8; 32],
    candidate: OfferedProcessedDeploy,
    user_mergeable: NumberChannelsEndVal,
    #[cfg(any(test, feature = "test-utils"))]
    replay_usages: models::rust::host_work::HostWorkUsages,
}

impl CertifiedOfferedDraft {
    pub(crate) fn settled_root(&self) -> [u8; 32] { self.settled_root }

    /// DR-102: the certified mergeable map, which the producer and every
    /// validator store for this deploy.
    pub(crate) fn into_user_mergeable(self) -> NumberChannelsEndVal { self.user_mergeable }

    /// DR-102: the final usage of the replay budget that certify owns.
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn replay_usages(&self) -> models::rust::host_work::HostWorkUsages {
        self.replay_usages
    }

    pub(crate) fn envelope_identity(&self) -> [u8; 32] { self.envelope_identity }

    pub(crate) fn matches_candidate(&self, candidate: &OfferedProcessedDeploy) -> bool {
        self.candidate == *candidate
    }
}

/// D-E4 (DR-111): the charge of the copy of the authority presentations into
/// the candidate of an offered certificate. The copy is released with the
/// certificate, so a block slice copy and cleanup prepays it. An empty slice
/// charges nothing.
fn reserve_presentation_copy(
    presentations: &[models::rhoapi::CostSignature],
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    if presentations.is_empty() {
        return Ok(());
    }
    let meter = |operations: usize, scanned: usize, backing: usize| {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, operations),
            (HostWorkDimension::VerificationBytes, scanned),
            (HostWorkDimension::SearchStateBytes, backing),
        ] {
            budget
                .reserve(
                    dimension,
                    HostWorkUnits::new(u64::try_from(amount).map_err(|_| BackingError::Overflow)?),
                )
                .map_err(|_| BackingError::Rejected)?;
        }
        Ok(())
    };
    clone_backing::reserve_blocks_slice_copy_and_cleanup(presentations, &meter).map_err(|error| {
        CasperError::RuntimeError(format!(
            "offered certificate presentation copy failed: {error:?}"
        ))
    })
}

struct MergeableReadMeter<'a>(&'a HostWorkBudget);

impl NativeReadMeter for MergeableReadMeter<'_> {
    type Error = CasperError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, charge.operations),
            (HostWorkDimension::VerificationBytes, charge.scanned_bytes),
            (HostWorkDimension::SearchStateBytes, charge.backing_bytes),
        ] {
            let units = u64::try_from(amount).map_err(|_| {
                CasperError::RuntimeError("offered mergeable read work overflows".into())
            })?;
            self.0
                .reserve(dimension, HostWorkUnits::new(units))
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        }
        Ok(())
    }
}

fn mergeable_read_error(error: NativeReadError<CasperError>) -> CasperError {
    match error {
        NativeReadError::Host(error) | NativeReadError::Consumer(error) => error,
        NativeReadError::Store(error) => {
            CasperError::RuntimeError(format!("offered mergeable history read failed: {error}"))
        }
        NativeReadError::Invalid(error) => {
            CasperError::RuntimeError(format!("offered mergeable history is invalid: {error:?}"))
        }
    }
}

fn offered_vec_clone_bytes<T>(items: &[T]) -> Option<usize> {
    items.len().checked_mul(size_of::<T>())
}

fn offered_event_clone_bytes(event: &Event) -> Option<usize> {
    match event {
        Event::Produce(produce) => offered_vec_clone_bytes(&produce.output_value),
        Event::Consume(consume) => offered_vec_clone_bytes(&consume.channels_hashes),
        Event::Comm(comm) => {
            let mut total = offered_vec_clone_bytes(&comm.consume.channels_hashes)?
                .checked_add(offered_vec_clone_bytes(&comm.produces)?)?
                .checked_add(offered_vec_clone_bytes(&comm.peeks)?)?;
            for produce in &comm.produces {
                total = total.checked_add(offered_vec_clone_bytes(&produce.output_value)?)?;
            }
            Some(total)
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ParentsPostStateCacheKey {
    pub sorted_parent_hashes: Vec<BlockHash>,
    // Snapshot LFB participates in visible-ancestor filtering, so cache key must include it.
    pub snapshot_lfb_hash: BlockHash,
    // The finalized-floor merge base is derived from the block's frozen
    // justification snapshot (finality/floor.rs), so identical parent sets
    // under different justification maps can merge from different floors.
    // Sorted (validator, latest_block_hash) pairs keep such contexts from
    // sharing a cache entry.
    pub sorted_latest_messages: Vec<(Validator, BlockHash)>,
    // Whether the computation ran with a rejected-deploy buffer attached.
    // Buffer population is a side effect of the merge, not part of the
    // cached value — a bufferless computation (exploratory deploy) must
    // never satisfy a lookup from the create/validate path, or that
    // path's buffer populate is silently skipped.
    pub buffer_populated: bool,
}

/// The merged pre-state a block builds on, with every fact the merge
/// derived alongside it. One struct through the derivation, the
/// parents-post-state cache, and the checkpoint path — the facts travel
/// together or not at all: a consumer holding the state without the
/// applied set cannot tell which deploys' effects that state already
/// contains, and executing one of them again double-applies it.
#[derive(Clone, Debug)]
pub struct MergedPreState {
    pub state: StateHash,
    /// Rejected user deploys as full records — each names the carrier it
    /// adjudicated and carries the formation-time duplicate flag. These
    /// travel to the block body as-is; the record IS the consensus content.
    pub rejected_user: Vec<models::rust::casper::protocol::casper_message::RejectedDeploy>,
    pub rejected_slashes: Vec<crate::rust::merging::rejected_slash::RejectedSlash>,
    /// User sigs whose chains the merge APPLIED from scope: their effects
    /// are in `state`, so executing any of them on top would double-apply.
    /// Empty on the non-merging shapes (genesis, single parent, covering
    /// parent), where effects arrive via a parent's post-state instead.
    pub applied_from_scope: std::collections::HashSet<prost::bytes::Bytes>,
    /// The block whose committed state `state` derives from: on the merged
    /// path the main parent, or the floor when the main parent's state does
    /// not hold the floor's settled content. `None` where the header already
    /// determines it (genesis, single parent, covering parent).
    pub merge_base: Option<BlockHash>,
}

pub type ParentsPostStateCacheVal = MergedPreState;

impl RuntimeManager {
    /// DR-102: test-only records of the final usage of each offered budget.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn offered_budget_usages(&self) -> Vec<OfferedBudgetUsage> {
        self.offered_budget_recorder.records()
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn record_offered_budget_usage(
        &self,
        kind: OfferedUsageKind,
        deploy_id: &[u8],
        pre_state_root: &[u8],
        usages: models::rust::host_work::HostWorkUsages,
    ) {
        self.offered_budget_recorder
            .record(kind, deploy_id, pre_state_root, usages);
    }

    pub async fn read_direct_offered_wallet_snapshot<'a>(
        &self,
        envelope: &'a models::rust::deploy_envelope::DeployEnvelope,
        original_root: &StateHash,
        verified: Option<&VerifiedOfferedGrantSources>,
        budget: &HostWorkBudget,
    ) -> Result<
        DirectWalletPolicySnapshot<'a, models::rust::signed_phlo_deploy::OfferedFundedDeploy>,
        CasperError,
    > {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(CasperError::RuntimeError(
                "rooted offered wallet capture requires a signed offered envelope".into(),
            ));
        };
        let protocol = offered_funded_v6_limits();
        for (dimension, amount) in [
            (
                HostWorkDimension::VerificationBytes,
                protocol.envelope.payload.signing.total_bytes,
            ),
            (
                HostWorkDimension::SearchStateBytes,
                protocol.envelope.payload.funding.wire.total_bytes,
            ),
        ] {
            budget
                .reserve(
                    dimension,
                    HostWorkUnits::new(u64::try_from(amount).map_err(|_| {
                        CasperError::RuntimeError("offered wallet preflight bound overflows".into())
                    })?),
                )
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        }
        let limits = DirectWalletFundingLimits {
            members: protocol.envelope.members,
            funding: protocol.envelope.payload.funding,
        };
        let authorization = match verified {
            Some(verified) => {
                authorize_offered_direct_wallet_funding_with_grants(signed, limits, verified)
            }
            None => authorize_offered_direct_wallet_funding(signed, limits),
        }
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        authorization
            .read_policy_snapshot(
                self,
                original_root.clone(),
                std::num::NonZeroUsize::MIN,
                budget,
            )
            .await
            .map_err(|error| CasperError::RuntimeError(error.to_string()))
    }

    const MAX_BLOCK_INDEX_CACHE_ENTRIES: usize = 128;
    const MAX_PARENTS_POST_STATE_CACHE_ENTRIES: usize = 64;
    const MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES: usize = 256;
    const MAX_BONDS_CACHE_ENTRIES: usize = 64;
    const MAX_REPLAY_CACHE_ENTRIES: usize = 192;
    const MAX_REPLAY_CACHE_BYTES: usize = 32 * 1024 * 1024;
    const MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES: usize = 1_536;

    fn collect_replay_logs(
        usr_processed: &[ProcessedDeploy],
        sys_processed: &[ProcessedSystemDeploy],
    ) -> Vec<Event> {
        let user_log_len: usize = usr_processed.iter().map(|pd| pd.deploy_log.len()).sum();
        let sys_log_len: usize = sys_processed
            .iter()
            .map(|psd| match psd {
                ProcessedSystemDeploy::Succeeded { event_list, .. } => event_list.len(),
                ProcessedSystemDeploy::Failed { event_list, .. } => event_list.len(),
            })
            .sum();

        let mut all_logs = Vec::with_capacity(user_log_len + sys_log_len);

        for pd in usr_processed {
            all_logs.extend(pd.deploy_log.iter().cloned());
        }

        for psd in sys_processed {
            match psd {
                ProcessedSystemDeploy::Succeeded { event_list, .. } => {
                    all_logs.extend(event_list.iter().cloned());
                }
                ProcessedSystemDeploy::Failed { event_list, .. } => {
                    all_logs.extend(event_list.iter().cloned());
                }
            }
        }

        all_logs
    }

    fn replay_payload_hash(
        usr_processed: &[ProcessedDeploy],
        sys_processed: &[ProcessedSystemDeploy],
        is_genesis: bool,
    ) -> Vec<u8> {
        #[inline]
        fn push_len_prefixed(bytes: &mut Vec<u8>, data: &[u8]) {
            bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
            bytes.extend_from_slice(data);
        }

        // Fingerprint replay-relevant payload so cache keys stay safe under adversarial input.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(usr_processed.len() as u64).to_le_bytes());
        for pd in usr_processed {
            push_len_prefixed(&mut bytes, &pd.deploy.sig);
            bytes.extend_from_slice(&pd.cost.cost.to_le_bytes());
            bytes.push(u8::from(pd.is_failed));
            match &pd.system_deploy_error {
                Some(err) => {
                    bytes.push(1);
                    push_len_prefixed(&mut bytes, err.as_bytes());
                }
                None => bytes.push(0),
            }
        }
        bytes.extend_from_slice(&(sys_processed.len() as u64).to_le_bytes());
        for psd in sys_processed {
            match psd {
                ProcessedSystemDeploy::Succeeded { system_deploy, .. } => {
                    bytes.push(0);
                    match system_deploy {
                        SystemDeployData::Slash {
                            invalid_block_hash,
                            issuer_public_key,
                            target_activation_epoch,
                        } => {
                            bytes.push(0);
                            push_len_prefixed(&mut bytes, invalid_block_hash);
                            push_len_prefixed(&mut bytes, &issuer_public_key.bytes);
                            // Little-endian is consensus-determined for this
                            // hash-affecting encoding — every node must agree
                            // on the bytes fed into the post-state hash. Do
                            // not switch to big-endian or `to_be_bytes`.
                            bytes.extend_from_slice(&target_activation_epoch.to_le_bytes());
                        }
                        SystemDeployData::CloseBlockSystemDeployData => {
                            bytes.push(1);
                        }
                        SystemDeployData::Empty => {
                            bytes.push(2);
                        }
                    }
                }
                ProcessedSystemDeploy::Failed { error_msg, .. } => {
                    bytes.push(1);
                    push_len_prefixed(&mut bytes, error_msg.as_bytes());
                }
            }
        }
        bytes.push(u8::from(is_genesis));
        Blake2b256::hash(bytes)
    }

    fn max_block_index_cache_entries() -> usize { Self::MAX_BLOCK_INDEX_CACHE_ENTRIES }

    fn max_parents_post_state_cache_entries() -> usize {
        Self::MAX_PARENTS_POST_STATE_CACHE_ENTRIES
    }

    fn max_active_validators_cache_entries() -> usize { Self::MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES }

    fn max_bonds_cache_entries() -> usize { Self::MAX_BONDS_CACHE_ENTRIES }

    fn max_replay_cache_entries() -> usize { Self::MAX_REPLAY_CACHE_ENTRIES }

    fn max_replay_cache_bytes() -> usize { Self::MAX_REPLAY_CACHE_BYTES }

    fn max_replay_cache_event_log_entries() -> usize { Self::MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES }

    fn record_replay_cache_metrics(cache: &InMemoryReplayCache) -> (usize, usize) {
        let stats = cache.stats();
        metrics::gauge!(REPLAY_CACHE_ENTRIES_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(stats.0 as f64);
        metrics::gauge!(REPLAY_CACHE_RETAINED_BYTES_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(stats.1 as f64);
        stats
    }

    fn try_acquire_exploratory_deploy_permit_with(
        semaphore: Arc<Semaphore>,
    ) -> Option<OwnedSemaphorePermit> {
        semaphore.try_acquire_owned().ok()
    }

    pub fn try_acquire_exploratory_deploy_permit(&self) -> Option<OwnedSemaphorePermit> {
        Self::try_acquire_exploratory_deploy_permit_with(self.exploratory_deploy_semaphore.clone())
    }

    pub fn replay_lock(&self) -> Arc<ReplayLock> { self.replay_lock.clone() }

    pub fn exploratory_deploy_execution_timeout_value(&self) -> Duration {
        self.exploratory_deploy_execution_timeout
    }

    pub fn trim_allocator() {
        #[cfg(target_os = "linux")]
        unsafe {
            unsafe extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            let _ = malloc_trim(0);
        }
    }

    fn touch_cache_key<K>(order: &Mutex<VecDeque<K>>, key: &K)
    where K: Eq + Clone {
        // LRU touch is O(n) due VecDeque::position/remove. This is intentional for now:
        // these caches are tightly bounded (64-256 entries by default), so linear touch
        // remains cheaper than introducing additional synchronized index maps.
        if let Ok(mut guard) = order.lock() {
            if let Some(pos) = guard.iter().position(|existing| existing == key) {
                guard.remove(pos);
            }
            guard.push_back(key.clone());
        }
    }

    fn evict_fifo_entry<K, V>(map: &DashMap<K, V>, order: &Mutex<VecDeque<K>>)
    where K: Eq + Hash + Clone {
        if let Ok(mut guard) = order.lock() {
            while let Some(evict_key) = guard.pop_front() {
                if map.remove(&evict_key).is_some() {
                    break;
                }
            }
        }
    }

    pub async fn spawn_runtime(&self) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_runtime_with(Vec::new(), self.external_services.clone())
            .await
    }

    pub async fn spawn_offered_runtime(
        &self,
        limits: GrantIssueCallLimits,
        budget: HostWorkBudget,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_runtime_with(
            vec![grant_issue_definition(limits, budget)],
            ExternalServices::noop(),
        )
        .await
    }

    /// Added by DR-114: the offered play runtime calls the node's external
    /// services once per recorded call. Every replay keeps the noop services,
    /// because replay produces the record and never calls a service.
    pub async fn spawn_offered_play_runtime(
        &self,
        limits: GrantIssueCallLimits,
        budget: HostWorkBudget,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_runtime_with(
            vec![grant_issue_definition(limits, budget)],
            self.external_services.clone(),
        )
        .await
    }

    async fn spawn_runtime_with(
        &self,
        mut definitions: Vec<Definition>,
        services: ExternalServices,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        let start = std::time::Instant::now();
        let new_space = self.space.spawn().expect("Failed to spawn RSpace");
        let runtime = rho_runtime::create_rho_runtime(
            new_space,
            self.mergeable_tags.clone(),
            true,
            &mut definitions,
            services,
        )
        .await?;
        metrics::histogram!(RUNTIME_SPAWN_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        Ok(runtime)
    }

    pub async fn spawn_replay_runtime(&self) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_replay_runtime_with(Vec::new(), self.external_services.clone())
            .await
    }

    pub async fn spawn_offered_replay_runtime(
        &self,
        limits: GrantIssueCallLimits,
        budget: HostWorkBudget,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_replay_runtime_with(
            vec![grant_issue_definition(limits, budget)],
            ExternalServices::noop(),
        )
        .await
    }

    async fn spawn_replay_runtime_with(
        &self,
        mut definitions: Vec<Definition>,
        services: ExternalServices,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        let start = std::time::Instant::now();
        let new_replay_space = self
            .replay_space
            .spawn()
            .expect("Failed to spawn ReplayRSpace");

        let runtime = rho_runtime::create_replay_rho_runtime(
            new_replay_space,
            self.mergeable_tags.clone(),
            true,
            &mut definitions,
            services,
        )
        .await?;
        metrics::counter!(RUNTIME_SPAWN_REPLAY_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::histogram!(RUNTIME_SPAWN_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        Ok(runtime)
    }

    pub async fn compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<(StateHash, Vec<ProcessedDeploy>, Vec<ProcessedSystemDeploy>), CasperError> {
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);

        // Block data used for mergeable key
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;

        let (state_hash, usr_deploy_res, sys_deploy_res) = runtime_ops
            .compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks,
                None,
            )
            .await?;

        let (usr_processed, usr_mergeable): (Vec<ProcessedDeploy>, Vec<NumberChannelsEndVal>) =
            usr_deploy_res.into_iter().unzip();
        let (sys_processed, sys_mergeable): (
            Vec<ProcessedSystemDeploy>,
            Vec<NumberChannelsEndVal>,
        ) = sys_deploy_res.into_iter().unzip();
        let replay_cache_event_log_cap = Self::max_replay_cache_event_log_entries();

        // Concat user and system deploys mergeable channel maps
        let mergeable_chs = usr_mergeable
            .into_iter()
            .chain(sys_mergeable.into_iter())
            .collect();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state_hash = Blake2b256Hash::from_bytes_prost(&state_hash);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            post_state_hash,
            sender.bytes.clone(),
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        )?;

        // Cache replay result for potential replay shortcut (including event logs)
        if let Some(ref cache) = self.replay_cache {
            let all_logs = Self::collect_replay_logs(&usr_processed, &sys_processed);
            let replay_payload_hash =
                Self::replay_payload_hash(&usr_processed, &sys_processed, false);

            if !all_logs.is_empty() && all_logs.len() <= replay_cache_event_log_cap {
                let key = ReplayCacheKey::new(
                    start_hash.clone(),
                    sender.bytes.to_vec(),
                    seq_num as i64,
                    replay_payload_hash,
                );
                let entry = ReplayCacheEntry::new(all_logs, state_hash.clone());
                let replay_cached = cache.put(key, entry);
                let (cache_entries, cache_retained_bytes) =
                    Self::record_replay_cache_metrics(cache);
                tracing::debug!("[CACHE] Replay cache admission for sender seq={}: cached={}, entries={}, retained_bytes={}", seq_num, replay_cached, cache_entries, cache_retained_bytes);
            } else if !all_logs.is_empty() {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    all_logs.len()
                );
            }
        }

        Ok((state_hash, usr_processed, sys_processed))
    }

    pub async fn compute_state_with_bonds_envelopes(
        &self,
        start_hash: &StateHash,
        terms: Vec<PendingDeployCandidate>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        play_budget: Option<std::time::Duration>,
        adopted: Option<&AdoptedResourcePolicy>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedUserDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
        ),
        CasperError,
    > {
        if terms
            .iter()
            .all(|candidate| matches!(candidate, PendingDeployCandidate::Legacy(_)))
        {
            let legacy = terms
                .into_iter()
                .map(|candidate| match candidate {
                    PendingDeployCandidate::Legacy(deploy) => deploy,
                    PendingDeployCandidate::Envelope(_) => unreachable!(),
                })
                .collect();
            let (state, deploys, system_deploys, bonds) = self
                .compute_state_with_bonds(
                    start_hash,
                    legacy,
                    system_deploys,
                    block_data,
                    invalid_blocks,
                    play_budget,
                )
                .await?;
            return Ok((
                state,
                deploys
                    .into_iter()
                    .map(ProcessedUserDeploy::Legacy)
                    .collect(),
                system_deploys,
                bonds,
            ));
        }

        let adopted = adopted.ok_or_else(|| {
            CasperError::RuntimeError(
                "offered proposal requires authenticated approved-genesis policy".into(),
            )
        })?;
        if !adopted.offered_funded_v6_active() {
            return Err(CasperError::RuntimeError(
                "offered proposal format is inactive under approved genesis".into(),
            ));
        }
        let offered_budget = HostWorkBudget::new(offered_funded_v6_host_work_limits());

        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        // DR-102 (DR-72): a host-work rejection of the execution budget after the
        // candidate step is attributable to the candidate, so the proposer
        // quarantines the envelope instead of failing every later proposal.
        let candidate_id = match terms.as_slice() {
            [PendingDeployCandidate::Envelope(envelope)] => match envelope.identity() {
                DeployLookupId::V6(deploy_id) => Some(*deploy_id),
                _ => None,
            },
            _ => None,
        };
        let runtime = self
            .spawn_offered_runtime(offered_grant_issue_call_limits(), offered_budget.clone())
            .await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let (state_hash, user_results, system_results) = runtime_ops
            .compute_state_envelopes(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks.unwrap_or_default(),
                play_budget,
                Some(adopted),
                self,
                Some(&offered_budget),
            )
            .await?;
        let (user_processed, user_mergeable): (Vec<_>, Vec<_>) = user_results.into_iter().unzip();
        let (system_processed, system_mergeable): (Vec<_>, Vec<_>) =
            system_results.into_iter().unzip();
        let candidate_rejection = |error: CasperError| match candidate_id {
            Some(deploy_id) if offered_budget.is_rejected() => {
                CasperError::OfferedCandidateRejected(OfferedCandidateRejection {
                    deploy_id,
                    pre_state_root: start_hash.clone(),
                    reason: error.to_string(),
                })
            }
            _ => error,
        };
        #[cfg(any(test, feature = "test-utils"))]
        let execution_before = offered_budget.usages();
        let mergeable_count = user_mergeable
            .len()
            .checked_add(system_mergeable.len())
            .ok_or_else(|| CasperError::RuntimeError("offered mergeable count overflows".into()))?;
        offered_budget
            .reserve(
                HostWorkDimension::SearchStateBytes,
                HostWorkUnits::new(
                    u64::try_from(
                        mergeable_count
                            .checked_mul(size_of::<NumberChannelsEndVal>())
                            .ok_or_else(|| {
                                CasperError::RuntimeError("offered mergeable size overflows".into())
                            })?,
                    )
                    .map_err(|_| {
                        CasperError::RuntimeError("offered mergeable size overflows".into())
                    })?,
                ),
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))
            .map_err(candidate_rejection)?;
        let bonds = runtime_ops.compute_bonds(&state_hash).await?;
        self.save_mergeable_channels_metered(
            Blake2b256Hash::from_bytes_prost(&state_hash),
            sender.bytes.clone(),
            seq_num,
            user_mergeable.into_iter().chain(system_mergeable).collect(),
            &Blake2b256Hash::from_bytes_prost(start_hash),
            &offered_budget,
        )
        .map_err(candidate_rejection)?;
        #[cfg(any(test, feature = "test-utils"))]
        if let Some(deploy_id) = candidate_id {
            self.offered_budget_recorder.record(
                OfferedUsageKind::ProducerMergeableExecution,
                deploy_id.as_ref(),
                start_hash,
                usage_delta(execution_before, offered_budget.usages()),
            );
        }
        Ok((state_hash, user_processed, system_processed, bonds))
    }

    pub async fn compute_state_with_bonds(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        play_budget: Option<std::time::Duration>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
        ),
        CasperError,
    > {
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let runtime = self.spawn_runtime().await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_spawn_runtime", rss_kb);
        }
        let mut runtime_ops = RuntimeOps::new(runtime);

        // Block data used for mergeable key
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;

        let (state_hash, usr_deploy_res, sys_deploy_res) = runtime_ops
            .compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks,
                play_budget,
            )
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_compute_state", rss_kb);
        }

        let (usr_processed, usr_mergeable): (Vec<ProcessedDeploy>, Vec<NumberChannelsEndVal>) =
            usr_deploy_res.into_iter().unzip();
        let (sys_processed, sys_mergeable): (
            Vec<ProcessedSystemDeploy>,
            Vec<NumberChannelsEndVal>,
        ) = sys_deploy_res.into_iter().unzip();
        let replay_cache_event_log_cap = Self::max_replay_cache_event_log_entries();

        // Concat user and system deploys mergeable channel maps
        let mergeable_chs = usr_mergeable
            .into_iter()
            .chain(sys_mergeable.into_iter())
            .collect();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state_hash = Blake2b256Hash::from_bytes_prost(&state_hash);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            post_state_hash,
            sender.bytes.clone(),
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        )?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_save_mergeable_channels", rss_kb);
        }

        // Cache replay result for potential replay shortcut (including event logs)
        if let Some(ref cache) = self.replay_cache {
            let all_logs = Self::collect_replay_logs(&usr_processed, &sys_processed);
            let replay_payload_hash =
                Self::replay_payload_hash(&usr_processed, &sys_processed, false);

            if !all_logs.is_empty() && all_logs.len() <= replay_cache_event_log_cap {
                let key = ReplayCacheKey::new(
                    start_hash.clone(),
                    sender.bytes.to_vec(),
                    seq_num as i64,
                    replay_payload_hash,
                );
                let entry = ReplayCacheEntry::new(all_logs, state_hash.clone());
                let replay_cached = cache.put(key, entry);
                let (cache_entries, cache_retained_bytes) =
                    Self::record_replay_cache_metrics(cache);
                tracing::debug!("[CACHE] Replay cache admission for sender seq={}: cached={}, entries={}, retained_bytes={}", seq_num, replay_cached, cache_entries, cache_retained_bytes);
            } else if !all_logs.is_empty() {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    all_logs.len()
                );
            }
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_cache_updates", rss_kb);
        }

        // Reuse the same spawned runtime for bonds query to avoid a second runtime init.
        let bonds = runtime_ops.compute_bonds(&state_hash).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_compute_bonds", rss_kb);
        }
        drop(runtime_ops);
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_drop_runtime_ops", rss_kb);
        }

        Ok((state_hash, usr_processed, sys_processed, bonds))
    }

    pub async fn compute_genesis(
        &self,
        terms: Vec<Signed<DeployData>>,
        block_time: i64,
        block_number: i64,
    ) -> Result<(StateHash, StateHash, Vec<ProcessedDeploy>), CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);

        let (pre_state, state_hash, processed) = runtime_ops
            .compute_genesis(terms, block_time, block_number)
            .await?;
        let (processed_deploys, mergeable_chs) = processed.into_iter().unzip();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(&pre_state);
        let post_state_hash = Blake2b256Hash::from_bytes_prost(&state_hash);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            post_state_hash,
            prost::bytes::Bytes::new(),
            0,
            mergeable_chs,
            &pre_state_hash,
        )?;

        Ok((pre_state, state_hash, processed_deploys))
    }

    pub async fn replay_compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool, // FIXME have a better way of knowing this. Pass the replayDeploy function maybe? - OLD
    ) -> Result<StateHash, CasperError> {
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let replay_payload_hash = Self::replay_payload_hash(&terms, &system_deploys, is_genesis);

        // Step 1: Check replay cache (deterministic replay delta)
        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            sender.bytes.to_vec(),
            seq_num as i64,
            replay_payload_hash,
        );
        if let Some(ref cache) = self.replay_cache {
            if let Some(entry) = cache.get(&replay_cache_key) {
                // A replay that returns Ok must leave the mergeable entry for its
                // post-state persisted — `ensure_mergeable_entry` exists precisely
                // to rebuild a collected entry and has no other way to do it. The
                // cache carries only the event log and the post-state, so honoring
                // the shortcut while the entry is absent returns success having
                // rebuilt nothing; the caller then reports the entry still missing
                // and that error becomes a slashable verdict against the block's
                // proposer. Verify presence first and fall through to the full
                // replay (which persists) when it is gone.
                let mergeable_key = MergeableKey {
                    state_hash: StateHashSerde(entry.post_state.clone()),
                    creator: sender.bytes.clone(),
                    seq_num,
                };
                let mergeable_key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
                    CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
                })?;

                if self.mergeable_store.contains_key(mergeable_key_encoded)? {
                    tracing::info!("[CACHE] ReplayCache hit for sender seq={}", seq_num);

                    // Rig the replay runtime with cached event log
                    let replay_runtime = self.spawn_replay_runtime().await?;
                    let rspace_events: Vec<_> = entry
                        .event_log
                        .iter()
                        .map(crate::rust::util::event_converter::to_rspace_event)
                        .collect();
                    replay_runtime.rig(rspace_events).await?;

                    return Ok(entry.post_state);
                }

                tracing::warn!(
                    "[CACHE] ReplayCache hit without mergeable entry for sender seq={}; falling back to full replay",
                    seq_num
                );
            }
        }

        // Step 2: Full replay (cache miss)
        let lock_start = std::time::Instant::now();
        let replay_permit = self.replay_lock.acquire_consensus().await;
        metrics::histogram!(BLOCK_REPLAY_RUNTIME_LOCK_WAIT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(lock_start.elapsed().as_secs_f64());
        let _replay_permit = replay_permit
            .map_err(|error| CasperError::Other(format!("Replay semaphore closed: {}", error)))?;
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let replay_runtime = self.spawn_replay_runtime().await?;
        let runtime_ops = RuntimeOps::new(replay_runtime);
        let mut replay_runtime_ops = ReplayRuntimeOps::new(runtime_ops);

        let execute_start = std::time::Instant::now();
        let replay_result = replay_runtime_ops
            .replay_compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                Some(invalid_blocks),
                is_genesis,
            )
            .await;
        metrics::histogram!(BLOCK_REPLAY_RUNTIME_EXECUTE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(execute_start.elapsed().as_secs_f64());
        let (state_hash, mergeable_chs) = replay_result?;

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state = state_hash.to_bytes_prost();

        // Phase 9 (G-1): surface persistence failure as a typed
        // `CasperError` instead of a finalization-task panic. A panic
        // here would crash the runtime_manager future with an
        // unhelpful "task panicked" log; the typed Result lets the
        // caller decide how to react (likely abort the proposal, not
        // the whole node).
        let save_start = std::time::Instant::now();
        let save_result = self.save_mergeable_channels(
            state_hash.clone(),
            sender.bytes,
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        );
        metrics::histogram!(BLOCK_REPLAY_RUNTIME_SAVE_MERGEABLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(save_start.elapsed().as_secs_f64());
        save_result.map_err(|e| {
            CasperError::RuntimeError(format!("Failed to save mergeable channels: {:?}", e))
        })?;

        Ok(post_state)
    }

    // Changed by DR-102: certify owns the replay budget, so every role charges
    // exactly the certified replay to it, and it borrows the block context.
    // pub(crate) async fn certify_offered_draft(
    //     &self,
    //     processed: &OfferedProcessedDeploy,
    //     start_hash: &StateHash,
    //     block_data: &BlockData,
    //     invalid_blocks: HashMap<BlockHash, Validator>,
    //     adopted: &AdoptedResourcePolicy,
    //     budget: &HostWorkBudget,
    // ) -> Result<CertifiedOfferedDraft, CasperError> {
    pub(crate) async fn certify_offered_draft(
        &self,
        processed: &OfferedProcessedDeploy,
        start_hash: &StateHash,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<CertifiedOfferedDraft, CasperError> {
        let replay_budget = HostWorkBudget::new(offered_funded_v6_host_work_limits());
        let budget = &replay_budget;
        if !adopted.offered_funded_v6_active() {
            return Err(CasperError::RuntimeError(
                "offered-funded replay is not active for this block".to_string(),
            ));
        }
        let protocol = offered_funded_v6_limits();
        let evidence_bytes = processed.native_cost_evidence_bytes().len();
        if evidence_bytes > protocol.evidence.total_bytes
            || processed.deploy_log().len() > protocol.deploy_log_events
        {
            return Err(CasperError::RuntimeError(
                "offered replay candidate exceeds protocol limits".to_string(),
            ));
        }
        for (dimension, amount) in [
            (HostWorkDimension::VerificationBytes, evidence_bytes),
            (HostWorkDimension::VerificationOperations, evidence_bytes),
            (
                HostWorkDimension::VerificationOperations,
                processed.deploy_log().len(),
            ),
        ] {
            budget
                .reserve(
                    dimension,
                    HostWorkUnits::new(u64::try_from(amount).map_err(|_| {
                        CasperError::RuntimeError("offered replay work overflows".to_string())
                    })?),
                )
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        }
        let outer =
            NativeCostEvidenceV1::decode(processed.native_cost_evidence_bytes(), protocol.evidence)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let owned_sections = outer
            .funding_case
            .len()
            .checked_add(outer.prepaid_delta.len())
            .and_then(|bytes| bytes.checked_add(outer.wallet_settlement.len()))
            .and_then(|bytes| bytes.checked_mul(16))
            .ok_or_else(|| {
                CasperError::RuntimeError("offered replay work overflows".to_string())
            })?;
        budget
            .reserve(
                HostWorkDimension::SearchStateBytes,
                HostWorkUnits::new(u64::try_from(owned_sections).map_err(|_| {
                    CasperError::RuntimeError("offered replay work overflows".to_string())
                })?),
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        processed
            .validate(protocol.evidence)
            .map_err(CasperError::RuntimeError)?;
        let evidence = processed
            .evidence(protocol.evidence)
            .map_err(CasperError::RuntimeError)?;
        let original_root: [u8; 32] = start_hash.as_ref().try_into().map_err(|_| {
            CasperError::RuntimeError("offered replay requires a 32-byte original root".to_string())
        })?;
        if evidence.original_funding_root != original_root {
            return Err(CasperError::RuntimeError(
                "offered replay funding root differs from block pre-state".to_string(),
            ));
        }
        let DeployEnvelopeRef::OfferedFunded(signed) = processed.envelope().view() else {
            return Err(CasperError::RuntimeError(
                "offered replay requires a signed funded envelope".to_string(),
            ));
        };
        let funding = protocol.envelope.payload.funding;
        budget
            .reserve(
                HostWorkDimension::VerificationBytes,
                HostWorkUnits::new(u64::try_from(funding.wire.total_bytes).map_err(|_| {
                    CasperError::RuntimeError("offered replay funding bound overflows".to_string())
                })?),
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let candidate_limits = OfferedCandidateLimits {
            funding: PhloFundingIntentV2Limits {
                wire: funding.wire,
                base: funding,
                grant_uses: funding.wire.total_bytes / 8,
                grant_id_bytes: funding.wire.field_bytes,
                quote_evidence_bytes: funding.wire.field_bytes,
            },
            max_phlo_limit: i64::MAX as u64,
        };
        let prepared = prepare_offered_candidate(
            signed,
            adopted,
            original_root,
            original_root,
            candidate_limits,
        )?;
        if !matches!(
            prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(CasperError::RuntimeError(
                "offered replay conversion requires authenticated composition".to_string(),
            ));
        }
        let grant_context = if prepared.intent.grant_uses.is_empty() {
            None
        } else {
            let authenticated_time = u64::try_from(block_data.time_stamp).map_err(|_| {
                CasperError::RuntimeError(
                    "offered replay grant use requires nonnegative block time".to_string(),
                )
            })?;
            let grant_limits = offered_grant_transition_limits();
            let grant_snapshot = self.capture_offered_grants_from_signed(
                original_root,
                signed,
                grant_limits,
                budget,
            )?;
            let verified = grant_snapshot.verified_source_payers(
                original_root,
                signed,
                authenticated_time,
                grant_limits,
                budget,
            )?;
            Some((grant_snapshot, verified, authenticated_time))
        };
        let snapshot = self
            .read_direct_offered_wallet_snapshot(
                processed.envelope(),
                start_hash,
                grant_context.as_ref().map(|(_, verified, _)| verified),
                budget,
            )
            .await?;
        if snapshot.wallets().pre_state_root() != original_root {
            return Err(CasperError::RuntimeError(
                "offered replay wallet snapshot belongs to another root".to_string(),
            ));
        }
        let selected = prepared.selected_terms(adopted)?;
        let schedule = PhloScheduleBinding::new(
            selected.schedule(),
            models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS,
        )
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if evidence.genesis_policy_commitment != adopted.genesis_policy_commitment()?
            || evidence.schedule_commitment != schedule.schedule().commitment
            || evidence.phlo_price != schedule.schedule().actual_price
            || evidence.phlo_limit != prepared.phlo_limit
            || evidence.phlo_price != prepared.phlo_price
            || evidence.fee_rev != prepared.fee_rev
            || evidence.envelope_commitment != prepared.envelope_identity
        {
            return Err(CasperError::RuntimeError(
                "offered replay evidence differs from adopted policy or selected schedule"
                    .to_string(),
            ));
        }
        // DR-102: every role charges the copy of the block context here, before
        // the copy. The producer charged it before certify and the validator
        // did not charge it.
        budget
            .reserve(
                HostWorkDimension::SearchStateBytes,
                HostWorkUnits::new(
                    u64::try_from(offered_replay_context_copy_bytes(
                        block_data,
                        invalid_blocks,
                    )?)
                    .map_err(|_| {
                        CasperError::RuntimeError(
                            "offered replay context copy overflows".to_string(),
                        )
                    })?,
                ),
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let witness = self
            .replay_native_offered_witness(
                &prepared,
                processed,
                adopted,
                &schedule,
                NativeFundedExecutionContext {
                    block_data: block_data.clone(),
                    // Changed by DR-102: certify borrows the block context.
                    // invalid_blocks,
                    invalid_blocks: invalid_blocks.clone(),
                    trace: offered_funded_v6_trace_limits(),
                    host_work: budget.clone(),
                },
                candidate_limits,
                offered_funded_v6_recording_limits(),
                offered_funded_v6_replay_limits(),
            )
            .await?;
        let settlement = witness
            .settle_replayed_candidate(
                self,
                processed,
                &snapshot,
                grant_context
                    .as_ref()
                    .map(|(grant_snapshot, verified, authenticated_time)| {
                        NativeGrantSettlementInput {
                            snapshot: grant_snapshot,
                            verified,
                            authenticated_time: *authenticated_time,
                            limits: offered_grant_transition_limits(),
                        }
                    }),
                offered_funded_v6_production_limits(),
                offered_funded_v6_prepaid_inventory_limits(),
                offered_funded_v6_prepaid_receipt_limits(),
                budget,
            )
            .await?;
        let settled_root = settlement.final_root;
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_runtime(offered_grant_issue_call_limits(), budget.clone())
                .await?,
        );
        runtime
            .runtime
            .reset(&Blake2b256Hash::from_bytes(settled_root.to_vec()))
            .await?;
        if runtime.runtime.get_root().await.bytes() != settled_root {
            return Err(CasperError::RuntimeError(
                "offered certificate mergeable runtime has wrong root".to_string(),
            ));
        }
        let user_mergeable = runtime
            .get_number_channels_data_metered(&settlement.mergeable, budget)
            .await?;
        let event_backing = processed
            .deploy_log()
            .iter()
            .try_fold(0usize, |total, event| {
                total.checked_add(offered_event_clone_bytes(event)?)
            })
            .ok_or_else(|| {
                CasperError::RuntimeError("offered certificate event copy overflows".to_string())
            })?;
        let body = signed.data.body();
        // Changed by D-O1 (DR-111): the copy charges a block slice copy and
        // cleanup in `reserve_presentation_copy`.
        // let presentation_meter = |operations: usize, scanned: usize, backing: usize| {
        //     for (dimension, amount) in [
        //         (HostWorkDimension::VerificationOperations, operations),
        //         (HostWorkDimension::VerificationBytes, scanned),
        //         (HostWorkDimension::SearchStateBytes, backing),
        //     ] {
        //         budget
        //             .reserve(
        //                 dimension,
        //                 HostWorkUnits::new(
        //                     u64::try_from(amount).map_err(|_| BackingError::Overflow)?,
        //                 ),
        //             )
        //             .map_err(|_| BackingError::Rejected)?;
        //     }
        //     Ok(())
        // };
        // if !body.authority_presentations.is_empty() {
        //     clone_backing::reserve_slice_copy_and_cleanup(
        //         &body.authority_presentations,
        //         &presentation_meter,
        //     )
        //     .map_err(|error| {
        //         CasperError::RuntimeError(format!(
        //             "offered certificate presentation copy failed: {error:?}"
        //         ))
        //     })?;
        // }
        reserve_presentation_copy(&body.authority_presentations, budget)?;
        let signer_backing = signed.signers().iter().try_fold(0usize, |total, signer| {
            total
                .checked_add(size_of::<Cosigner>())?
                .checked_add(size_of_val(signer.sig_algorithm.as_ref()).max(1))
        });
        let clone_bound = body
            .term
            .len()
            .checked_add(body.language.len())
            .and_then(|n| n.checked_add(body.shard_id.len()))
            .and_then(|n| n.checked_add(signed.data.funding_intent().len()))
            .and_then(|n| n.checked_add(signed.data.signing_payload_len()))
            .and_then(|n| n.checked_add(signer_backing?))
            .and_then(|n| n.checked_add(processed.identity_bytes().len()))
            .and_then(|n| n.checked_add(processed.native_cost_evidence_bytes().len()))
            .and_then(|n| {
                n.checked_add(
                    processed
                        .deploy_log()
                        .len()
                        .checked_mul(size_of::<Event>())?,
                )
            })
            .and_then(|n| n.checked_add(event_backing))
            .ok_or_else(|| {
                CasperError::RuntimeError("offered certificate copy bound overflows".to_string())
            })?;
        for dimension in [
            HostWorkDimension::SearchStateBytes,
            HostWorkDimension::VerificationBytes,
        ] {
            budget
                .reserve(
                    dimension,
                    HostWorkUnits::new(u64::try_from(clone_bound).map_err(|_| {
                        CasperError::RuntimeError(
                            "offered certificate copy work overflows".to_string(),
                        )
                    })?),
                )
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        }
        Ok(CertifiedOfferedDraft {
            settled_root,
            envelope_identity: prepared.envelope_identity,
            candidate: processed.clone(),
            user_mergeable,
            #[cfg(any(test, feature = "test-utils"))]
            replay_usages: replay_budget.usages(),
        })
    }

    async fn replay_offered_system_continuation(
        &self,
        certificate: &CertifiedOfferedDraft,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
        budget: &HostWorkBudget,
    ) -> Result<(Blake2b256Hash, Vec<NumberChannelsEndVal>), CasperError> {
        let candidate_root = StateHash::copy_from_slice(&certificate.settled_root());
        let runtime = self
            .spawn_offered_replay_runtime(offered_grant_issue_call_limits(), budget.clone())
            .await?;
        let mut replay = ReplayRuntimeOps::new_from_runtime(runtime);
        replay
            .replay_compute_state(
                &candidate_root,
                Vec::new(),
                system_deploys,
                block_data,
                Some(invalid_blocks),
                false,
            )
            .await
    }

    pub async fn replay_compute_state_envelopes(
        &self,
        start_hash: &StateHash,
        terms: Vec<models::rust::casper::protocol::casper_message::ProcessedUserDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
    ) -> Result<StateHash, CasperError> {
        self.replay_compute_state_envelopes_with_policy(
            start_hash,
            terms,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
            None,
            None,
        )
        .await
    }

    pub async fn replay_compute_state_envelopes_with_policy(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedUserDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
        adopted: Option<&AdoptedResourcePolicy>,
        expected_post_state: Option<&StateHash>,
    ) -> Result<StateHash, CasperError> {
        if validate_offered_replay_preflight(start_hash, &terms)? {
            let adopted = adopted.ok_or_else(|| {
                CasperError::RuntimeError(
                    "offered-funded replay requires authenticated adopted resource policy"
                        .to_string(),
                )
            })?;
            if is_genesis || !adopted.offered_funded_v6_active() {
                return Err(CasperError::RuntimeError(
                    "offered-funded replay is not active for this block".to_string(),
                ));
            }
            let [ProcessedUserDeploy::Offered(processed)] = terms.as_slice() else {
                return Err(CasperError::RuntimeError(
                    "offered-funded replay requires one isolated user candidate".to_string(),
                ));
            };
            // Changed by DR-102: certify owns the replay budget. The work that this
            // validator does after the certified replay to accept the block is
            // charged to its own acceptance budget, as the producer charges its
            // counterpart to its execution budget.
            // let budget = HostWorkBudget::new(offered_funded_v6_host_work_limits());
            // let certificate = self
            //     .certify_offered_draft(
            //         processed,
            //         start_hash,
            //         block_data,
            //         invalid_blocks.clone().unwrap_or_default(),
            //         adopted,
            //         &budget,
            //     )
            //     .await?;
            let acceptance = OfferedAcceptanceBudget::new();
            let budget = acceptance.meter().clone();
            let no_invalid_blocks = HashMap::new();
            let certificate = self
                .certify_offered_draft(
                    processed,
                    start_hash,
                    block_data,
                    invalid_blocks.as_ref().unwrap_or(&no_invalid_blocks),
                    adopted,
                )
                .await?;
            #[cfg(any(test, feature = "test-utils"))]
            let replay_usages = certificate.replay_usages();
            let (post_root, system_mergeable) = self
                .replay_offered_system_continuation(
                    &certificate,
                    system_deploys,
                    block_data,
                    invalid_blocks.unwrap_or_default(),
                    &budget,
                )
                .await?;
            let expected = expected_post_state.ok_or_else(|| {
                CasperError::RuntimeError(
                    "offered block replay requires authenticated expected post-state".to_string(),
                )
            })?;
            if post_root.to_bytes_prost() != *expected {
                return Err(CasperError::RuntimeError(
                    "offered block replay post-state differs from commitment".to_string(),
                ));
            }
            if !OFFERED_PRODUCTION_READY {
                return Err(CasperError::RuntimeError(
                    "offered-funded replay remains closed pending block-level qualification"
                        .to_string(),
                ));
            }
            let channel_count = system_mergeable.len().checked_add(1).ok_or_else(|| {
                CasperError::RuntimeError("offered mergeable result count overflows".to_string())
            })?;
            budget
                .reserve(
                    HostWorkDimension::SearchStateBytes,
                    HostWorkUnits::new(
                        u64::try_from(
                            channel_count
                                .checked_mul(size_of::<NumberChannelsEndVal>())
                                .ok_or_else(|| {
                                    CasperError::RuntimeError(
                                        "offered mergeable result allocation overflows".to_string(),
                                    )
                                })?,
                        )
                        .map_err(|_| {
                            CasperError::RuntimeError(
                                "offered mergeable result work overflows".to_string(),
                            )
                        })?,
                    ),
                )
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let mut mergeable_values = Vec::new();
            mergeable_values
                .try_reserve_exact(channel_count)
                .map_err(|_| {
                    CasperError::RuntimeError(
                        "offered mergeable result allocation failed".to_string(),
                    )
                })?;
            mergeable_values.push(certificate.user_mergeable);
            mergeable_values.extend(system_mergeable);
            self.save_mergeable_channels_metered(
                post_root.clone(),
                block_data.sender.bytes.clone(),
                block_data.seq_num,
                mergeable_values,
                &Blake2b256Hash::from_bytes_prost(start_hash),
                &budget,
            )
            .map_err(|error| {
                CasperError::RuntimeError(format!(
                    "Failed to save offered mergeable channels: {:?}",
                    error
                ))
            })?;
            #[cfg(any(test, feature = "test-utils"))]
            {
                self.offered_budget_recorder.record(
                    OfferedUsageKind::ValidatorReplay,
                    processed.identity_bytes(),
                    start_hash,
                    replay_usages,
                );
                self.offered_budget_recorder.record(
                    OfferedUsageKind::ValidatorAcceptance,
                    processed.identity_bytes(),
                    start_hash,
                    budget.usages(),
                );
            }
            return Ok(post_root.to_bytes_prost());
        }
        let legacy = terms
            .into_iter()
            .map(|term| match term {
                models::rust::casper::protocol::casper_message::ProcessedUserDeploy::Legacy(
                    deploy,
                ) => Ok(deploy),
                models::rust::casper::protocol::casper_message::ProcessedUserDeploy::Offered(_) => {
                    Err(CasperError::RuntimeError(
                        "offered-funded replay requires independent native validation".to_string(),
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.replay_compute_state(
            start_hash,
            legacy,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
        )
        .await
    }

    pub async fn replay_offered_native_user_checked<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<
            'a,
            models::rust::signed_phlo_deploy::OfferedFundedDeploy,
        >,
        processed: &OfferedProcessedDeploy,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
        candidate_limits: OfferedCandidateLimits,
        recording_limits: NativeRecordingWireLimits,
        replay_limits: NativeFundedReplayLimits,
    ) -> Result<ReplayedNativeFundedUser<'policy, 'a>, CasperError> {
        let evidence_limits: PhloWireLimits = offered_funded_v6_limits().evidence;
        processed
            .validate(evidence_limits)
            .map_err(CasperError::RuntimeError)?;
        let evidence = processed
            .evidence(evidence_limits)
            .map_err(CasperError::RuntimeError)?;
        let DeployEnvelopeRef::OfferedFunded(signed) = processed.envelope().view() else {
            return Err(CasperError::RuntimeError(
                "native replay requires an offered funded envelope".to_string(),
            ));
        };
        let preflight = prepare_offered_candidate(
            signed,
            adopted,
            evidence.original_funding_root,
            evidence.original_funding_root,
            candidate_limits,
        )?;
        if !adopted.offered_funded_v6_active()
            || evidence.genesis_policy_commitment != adopted.genesis_policy_commitment()?
            || evidence.schedule_commitment != schedule.schedule().commitment
            || evidence.phlo_price != schedule.schedule().actual_price
            || evidence.phlo_limit != preflight.phlo_limit
            || evidence.phlo_price != preflight.phlo_price
            || evidence.fee_rev != preflight.fee_rev
            || evidence.envelope_commitment != preflight.envelope_identity
            || evidence.original_funding_root != policy.snapshot().wallets().pre_state_root()
        {
            return Err(CasperError::RuntimeError(
                "offered replay differs from authenticated genesis, schedule, or funding root"
                    .to_string(),
            ));
        }
        let mut replayed = self
            .replay_native_funded_user_from_processed(
                policy,
                processed,
                adopted,
                schedule,
                context,
                evidence_limits,
                recording_limits,
                replay_limits,
            )
            .await?;
        let covered = replayed
            .user_event_count()
            .checked_add(replayed.grant_issue_event_count())
            .ok_or_else(|| {
                CasperError::RuntimeError("offered replay event boundary overflows".to_string())
            })?;
        if covered > processed.deploy_log().len() {
            return Err(CasperError::RuntimeError(
                "offered native replay omits committed grant events".to_string(),
            ));
        }
        let root = replayed.runtime().runtime.create_checkpoint().await.root;
        if root.bytes() != replayed.expected_settlement_runtime_root() {
            replayed
                .runtime()
                .runtime
                .reset(&Blake2b256Hash::from_bytes(
                    evidence.original_funding_root.to_vec(),
                ))
                .await?;
            return Err(CasperError::RuntimeError(
                "offered native replay checkpoint differs from committed settlement root"
                    .to_string(),
            ));
        }
        Ok(replayed)
    }

    pub async fn capture_results(
        &self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
    ) -> Result<Vec<Par>, CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.capture_results(start, deploy).await?;
        Ok(computed)
    }

    pub async fn get_active_validators(
        &self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        if let Some(cached) = self.active_validators_cache.get(start_hash) {
            Self::touch_cache_key(&self.active_validators_cache_order, start_hash);
            return Ok(cached.clone());
        }

        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_active_validators(start_hash).await?;

        let max_entries = Self::max_active_validators_cache_entries();
        if self.active_validators_cache.len() >= max_entries {
            Self::evict_fifo_entry(
                &self.active_validators_cache,
                &self.active_validators_cache_order,
            );
        }
        self.active_validators_cache
            .insert(start_hash.clone(), computed.clone());
        Self::touch_cache_key(&self.active_validators_cache_order, start_hash);

        Ok(computed)
    }

    /// On-chain protocol fault-tolerance threshold (ppm) at `start_hash`, or
    /// `None` when the chain's genesis predates the parameter. Read once at
    /// casper construction (`hash_set_casper`) — not cached here.
    pub async fn get_fault_tolerance_threshold_ppm(
        &self,
        start_hash: &StateHash,
    ) -> Result<Option<i64>, CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .get_fault_tolerance_threshold_ppm(start_hash)
            .await
    }

    /// On-chain consensus parameters `(max-parent-depth, deploy-lifespan,
    /// min-phlo-price)` at `start_hash`, or `None` when the chain's genesis
    /// predates them. Read once at casper construction (`hash_set_casper`) —
    /// not cached here.
    pub async fn get_genesis_resource_policy(
        &self,
        start_hash: &StateHash,
    ) -> Result<models::rust::phlo_schedule::PhloGenesisPolicy, CasperError> {
        let mut runtime_ops = RuntimeOps::new(self.spawn_runtime().await?);
        runtime_ops.get_genesis_resource_policy(start_hash).await
    }

    pub async fn find_genesis_resource_policy(
        &self,
        start_hash: &StateHash,
    ) -> Result<Option<models::rust::phlo_schedule::PhloGenesisPolicy>, CasperError> {
        let mut runtime_ops = RuntimeOps::new(self.spawn_runtime().await?);
        runtime_ops.find_genesis_resource_policy(start_hash).await
    }

    pub async fn get_consensus_parameters(
        &self,
        start_hash: &StateHash,
    ) -> Result<Option<(i32, i64, i64)>, CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops.get_consensus_parameters(start_hash).await
    }

    pub async fn compute_bonds(&self, hash: &StateHash) -> Result<Vec<Bond>, CasperError> {
        if let Some(cached) = self.bonds_cache.get(hash) {
            Self::touch_cache_key(&self.bonds_cache_order, hash);
            return Ok(cached.clone());
        }

        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.compute_bonds(hash).await?;

        let max_entries = Self::max_bonds_cache_entries();
        if self.bonds_cache.len() >= max_entries {
            Self::evict_fifo_entry(&self.bonds_cache, &self.bonds_cache_order);
        }
        self.bonds_cache.insert(hash.clone(), computed.clone());
        Self::touch_cache_key(&self.bonds_cache_order, hash);

        Ok(computed)
    }

    // Executes deploy as user deploy with immediate rollback
    pub async fn play_exploratory_deploy(
        &self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .play_exploratory_deploy_with_phlo_limit(
                term,
                hash,
                deployer,
                self.exploratory_deploy_phlo_limit,
            )
            .await
    }

    pub async fn get_data(&self, hash: StateHash, channel: &Par) -> Result<Vec<Par>, CasperError> {
        let mut runtime = self.spawn_runtime().await?;

        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&hash))
            .await?;

        let runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_data_par(channel).await;
        Ok(computed)
    }

    pub async fn get_continuation(
        &self,
        hash: StateHash,
        channels: Vec<Par>,
    ) -> Result<Vec<(Vec<BindPattern>, Par)>, CasperError> {
        let mut runtime = self.spawn_runtime().await?;

        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&hash))
            .await?;

        let runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_continuation_par(channels).await;
        Ok(computed)
    }

    pub fn get_history_repo(&self) -> RhoHistoryRepository { self.history_repo.clone() }

    /// Check whether a post-state root is recorded in the local rspace
    /// roots store. Used by joiner-side LFS forward-horizon sync to skip
    /// roots that have already been imported. Pure lookup — no side effects.
    pub fn has_root(&self, root: &Blake2b256Hash) -> Result<bool, CasperError> {
        self.history_repo
            .contains_root(root)
            .map_err(|e| CasperError::RuntimeError(format!("has_root lookup failed: {:?}", e)))
    }

    /// Get or compute BlockIndex with caching
    pub fn get_or_compute_block_index<
        D: crate::rust::merging::block_index::ProcessedIndexDeploy,
    >(
        &self,
        block_hash: &BlockHash,
        block_number: i64,
        usr_processed_deploys: &[D],
        sys_processed_deploys: &Vec<ProcessedSystemDeploy>,
        pre_state_hash: &Blake2b256Hash,
        post_state_hash: &Blake2b256Hash,
        mergeable_chs: &Vec<NumberChannelsDiff>,
    ) -> Result<BlockIndex, CasperError> {
        if let Some(cached) = self.block_index_cache.get(block_hash) {
            Self::touch_cache_key(&self.block_index_cache_order, block_hash);
            metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
                .set(self.block_index_cache.len() as f64);
            return Ok(cached.clone());
        }

        // Cache miss - compute the BlockIndex.
        let block_index = crate::rust::merging::block_index::new(
            block_hash,
            block_number,
            usr_processed_deploys,
            sys_processed_deploys,
            pre_state_hash,
            post_state_hash,
            &self.history_repo,
            mergeable_chs,
        )?;

        // Keep index cache bounded for long-running validators.
        // Avoid DashMap re-entrant calls while holding an entry guard.
        let max_entries = Self::max_block_index_cache_entries();
        if self.block_index_cache.len() >= max_entries {
            Self::evict_fifo_entry(&self.block_index_cache, &self.block_index_cache_order);
        }

        self.block_index_cache
            .insert(block_hash.clone(), block_index.clone());
        Self::touch_cache_key(&self.block_index_cache_order, block_hash);
        metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.block_index_cache.len() as f64);
        Ok(block_index)
    }

    /// Remove BlockIndex from cache (used during finalization)
    pub fn remove_block_index_cache(&self, block_hash: &BlockHash) {
        self.block_index_cache.remove(block_hash);
        metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.block_index_cache.len() as f64);
    }

    pub fn get_cached_parents_post_state(
        &self,
        key: &ParentsPostStateCacheKey,
    ) -> Option<ParentsPostStateCacheVal> {
        let result = self.parents_post_state_cache.get(key).map(|entry| {
            Self::touch_cache_key(&self.parents_post_state_cache_order, key);
            entry.value().clone()
        });
        metrics::gauge!(PARENTS_POST_STATE_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.parents_post_state_cache.len() as f64);
        result
    }

    pub fn put_cached_parents_post_state(
        &self,
        key: ParentsPostStateCacheKey,
        value: ParentsPostStateCacheVal,
    ) {
        // Keep cache bounded with simple eviction strategy.
        let max_entries = Self::max_parents_post_state_cache_entries();
        if self.parents_post_state_cache.len() >= max_entries {
            Self::evict_fifo_entry(
                &self.parents_post_state_cache,
                &self.parents_post_state_cache_order,
            );
        }
        self.parents_post_state_cache.insert(key.clone(), value);
        Self::touch_cache_key(&self.parents_post_state_cache_order, &key);
        metrics::gauge!(PARENTS_POST_STATE_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.parents_post_state_cache.len() as f64);
    }

    /**
     * Load mergeable channels from store
     */
    pub fn load_mergeable_channels(
        &self,
        state_hash_bs: &StateHash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
    ) -> Result<Vec<NumberChannelsDiff>, CasperError> {
        let state_hash = Blake2b256Hash::from_bytes_prost(state_hash_bs);
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(state_hash.to_bytes_prost()),
            creator: creator.clone(),
            seq_num,
        };

        let get_key =
            bincode::serialize(&mergeable_key).expect("Failed to serialize mergeable key");

        let res = self.mergeable_store.get_one(&get_key)?;

        match res {
            Some(res) => {
                let res_map = res
                    .into_iter()
                    .map(|x| {
                        x.channels
                            .into_iter()
                            .map(|y| (y.hash, (y.diff, y.merge_type)))
                            .collect::<BTreeMap<_, _>>()
                    })
                    .collect::<Vec<_>>();
                Ok(res_map)
            }
            None => {
                let msg = format!(
                    "Missing mergeable entry for state {} (creator={}, seq={})",
                    state_hash.bytes().encode_hex::<String>(),
                    creator.encode_hex::<String>(),
                    seq_num
                );
                tracing::error!(state_hash = %state_hash.bytes().encode_hex::<String>(), creator = %creator.encode_hex::<String>(), seq_num, "mergeable entry missing for block");
                Err(CasperError::KvStoreError(KvStoreError::KeyNotFound(msg)))
            }
        }
    }

    /// Build the mergeable-store key bytes for a block.
    pub fn mergeable_key_bytes_for_block(
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
    ) -> Result<Vec<u8>, CasperError> {
        let key = MergeableKey {
            state_hash: StateHashSerde(block.body.state.post_state_hash.clone()),
            creator: block.sender.clone(),
            seq_num: block.seq_num,
        };
        bincode::serialize(&key)
            .map_err(|e| CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string())))
    }

    /// Look up a block's mergeable-channels entry and return its over-the-wire
    /// byte form. Returns `(key_bytes, Some(value_bytes))` if present,
    /// `(key_bytes, None)` if absent. Re-serializes via bincode at the typed
    /// store boundary so the wire format is independent of LMDB's internal
    /// encoding.
    pub fn get_mergeable_entry_bytes(
        &self,
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), CasperError> {
        let key_bytes = Self::mergeable_key_bytes_for_block(block)?;
        let value: Option<Vec<DeployMergeableData>> = self.mergeable_store.get_one(&key_bytes)?;
        let value_bytes = value
            .map(|v| bincode::serialize(&v))
            .transpose()
            .map_err(|e| {
                CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
            })?;
        Ok((key_bytes, value_bytes))
    }

    /// Store a mergeable-channels entry received over the wire. Decodes the
    /// transported bytes and writes via the typed store. Empty `value_bytes`
    /// signals "peer had no entry" and is a no-op.
    pub fn put_mergeable_entry_bytes(
        &self,
        key_bytes: Vec<u8>,
        value_bytes: Vec<u8>,
    ) -> Result<(), CasperError> {
        if value_bytes.is_empty() {
            return Ok(());
        }
        let value: Vec<DeployMergeableData> = bincode::deserialize(&value_bytes).map_err(|e| {
            CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
        })?;
        self.mergeable_store
            .put_one(key_bytes, value)
            .map_err(CasperError::from)
    }

    /// True iff this node already holds the mergeable-channels entry for `block`.
    /// Its presence is a byproduct of whether this node executed/replayed the
    /// block: a block imported via LFS without replay — or rejected locally and
    /// never replayed — lacks it, while the multi-parent merge requires it for
    /// every scope block. Without a recompute that makes merge validity node-local.
    pub fn has_mergeable_entry(
        &self,
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
    ) -> Result<bool, CasperError> {
        let key = Self::mergeable_key_bytes_for_block(block)?;
        let value: Option<Vec<DeployMergeableData>> = self.mergeable_store.get_one(&key)?;
        Ok(value.is_some())
    }

    /// Materialize the mergeable-channels entry for `block` by replaying it,
    /// unless already present. The mergeable diffs are a deterministic function
    /// of the block's content, so a full replay reconstructs exactly the entry
    /// the proposer stored — making mergeable presence a function of consensus
    /// data on every node rather than of local execution history.
    ///
    /// `invalid_blocks` MUST be the block's own invalid-block set (the set its
    /// original validation used); otherwise the replay computes a different
    /// post-state and the entry would be stored under the wrong key.
    pub async fn ensure_mergeable_entry(
        &self,
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<(), CasperError> {
        if self.has_mergeable_entry(block)? {
            return Ok(());
        }

        let block_data = BlockData::from_block(block);
        let is_genesis = block.header.parents_hash_list.is_empty();
        let computed_post_state = self
            .replay_compute_state_envelopes(
                &block.body.state.pre_state_hash,
                block.body.deploys.clone(),
                block.body.system_deploys.clone(),
                &block_data,
                Some(invalid_blocks),
                is_genesis,
            )
            .await?;

        // The entry is keyed by post-state, so a replay that reproduces a
        // different one stores it where nobody looks. Name that case: it is a
        // replay-determinism failure, not a storage failure, and the two need
        // very different responses.
        if computed_post_state != block.body.state.post_state_hash {
            return Err(CasperError::RuntimeError(format!(
                "recompute for block {} (seq={}) produced post-state {} but the block declares {}; \
                 the mergeable entry was stored under the computed key",
                hex::encode(&block.block_hash),
                block.seq_num,
                hex::encode(&computed_post_state),
                hex::encode(&block.body.state.post_state_hash),
            )));
        }

        // Fail closed: the full-replay path persists the entry. If it is still
        // absent the merge would diverge across nodes, so surface it.
        if !self.has_mergeable_entry(block)? {
            return Err(CasperError::RuntimeError(format!(
                "mergeable entry still absent after recompute for block {} (seq={})",
                hex::encode(&block.block_hash),
                block.seq_num,
            )));
        }

        Ok(())
    }

    /// Delete mergeable channels entry keyed by (post-state-hash, creator, seq-num).
    /// Returns `true` if the entry existed prior to deletion.
    pub fn delete_mergeable_channels(
        &self,
        state_hash_bs: &StateHash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
    ) -> Result<bool, CasperError> {
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(state_hash_bs.clone()),
            creator,
            seq_num,
        };
        let encoded_key =
            bincode::serialize(&mergeable_key).expect("Failed to serialize mergeable key");
        let existed = self.mergeable_store.contains_key(encoded_key.clone())?;
        if existed {
            self.mergeable_store.delete(vec![encoded_key])?;
        }
        Ok(existed)
    }

    /**
     * Converts final mergeable (number) channel values and save to mergeable store.
     *
     * Tuple (postStateHash, creator, seqNum) is used as a key, preStateHash is used to
     * read initial value to get the difference.
     */
    fn save_mergeable_channels(
        &self,
        post_state_hash: Blake2b256Hash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
        channels_data: Vec<NumberChannelsEndVal>,
        // Used to calculate value difference from final values
        pre_state_hash: &Blake2b256Hash,
    ) -> Result<(), CasperError> {
        // Calculate difference values from final values on number channels
        let diffs = self.convert_number_channels_to_diff(channels_data, pre_state_hash)?;

        // Convert to storage types
        let deploy_channels = diffs
            .into_iter()
            .map(|data| {
                let channels: Vec<NumberChannel> = data
                    .into_iter()
                    .map(|(hash, (diff, merge_type))| NumberChannel {
                        hash,
                        diff,
                        merge_type,
                    })
                    .collect::<Vec<_>>();

                DeployMergeableData { channels }
            })
            .collect();

        // Key is composed from post-state hash and block creator with seq number
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(post_state_hash.to_bytes_prost()),
            creator,
            seq_num,
        };

        let key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
            CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
        })?;

        // Save to mergeable channels store
        self.mergeable_store.put_one(key_encoded, deploy_channels)?;

        Ok(())
    }

    fn save_mergeable_channels_metered(
        &self,
        post_state_hash: Blake2b256Hash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
        channels_data: Vec<NumberChannelsEndVal>,
        pre_state_hash: &Blake2b256Hash,
        budget: &HostWorkBudget,
    ) -> Result<(), CasperError> {
        let entries = channels_data.iter().try_fold(0_usize, |count, map| {
            count.checked_add(map.len()).ok_or_else(|| {
                CasperError::RuntimeError("offered mergeable entry count overflows".into())
            })
        })?;
        let bytes = entries
            .checked_mul(1024)
            .and_then(|count| count.checked_add(channels_data.len().checked_mul(512)?))
            .and_then(|count| count.checked_add(creator.len().checked_mul(4)?))
            .and_then(|count| count.checked_add(512))
            .ok_or_else(|| {
                CasperError::RuntimeError("offered mergeable work size overflows".into())
            })?;
        let scanned = entries
            .checked_mul(256)
            .and_then(|count| count.checked_add(creator.len().checked_mul(2)?))
            .and_then(|count| count.checked_add(256))
            .ok_or_else(|| {
                CasperError::RuntimeError("offered mergeable scan size overflows".into())
            })?;
        let operations = entries
            .checked_mul(256)
            .and_then(|count| count.checked_add(channels_data.len().checked_mul(16)?))
            .and_then(|count| count.checked_add(32))
            .ok_or_else(|| {
                CasperError::RuntimeError("offered mergeable operation count overflows".into())
            })?;
        MergeableReadMeter(budget).reserve(NativeReadCharge {
            operations,
            scanned_bytes: scanned,
            backing_bytes: bytes,
        })?;
        let root: [u8; 32] = pre_state_hash.0.as_slice().try_into().map_err(|_| {
            CasperError::RuntimeError("offered mergeable pre-state hash is invalid".into())
        })?;
        if !self
            .history_repo
            .contains_root(pre_state_hash)
            .map_err(|error| {
                CasperError::RuntimeError(format!("offered mergeable root lookup failed: {error}"))
            })?
        {
            return Err(CasperError::RuntimeError(
                "offered mergeable pre-state root is not registered".into(),
            ));
        }
        let reader = self.history_repo.native_history_reader(root);
        let meter = MergeableReadMeter(budget);
        let unique_channels = channels_data
            .iter()
            .flat_map(|map| map.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut initial_values: BTreeMap<Blake2b256Hash, i64> = BTreeMap::new();
        for channel in unique_channels {
            let hash: [u8; 32] = channel.0.as_slice().try_into().map_err(|_| {
                CasperError::RuntimeError("offered mergeable channel hash is invalid".into())
            })?;
            let value = reader
                .with_records(NativeLeafKind::Data, &hash, &meter, |records| {
                    if records.len() > 1 {
                        return Err(CasperError::RuntimeError(
                            "offered mergeable channel has multiple pre-state values".into(),
                        ));
                    }
                    let Some(raw) = records.iter().next() else {
                        return Ok(0);
                    };
                    // Changed by D-S2 (DR-95): the row decodes in History mode,
                    // which charges each node once and reserves only real
                    // allocations.
                    // let datum: Datum<ListParWithRandom> =
                    //     decode_record(raw, &meter).map_err(mergeable_read_error)?;
                    let datum: Datum<ListParWithRandom> =
                        decode_history_record(raw, &meter).map_err(mergeable_read_error)?;
                    RholangMergingLogic::try_get_number_with_rnd(&datum.a)
                        .map(|(number, _)| number)
                        .ok_or_else(|| {
                            CasperError::RuntimeError(
                                "offered mergeable pre-state channel is non-numeric".into(),
                            )
                        })
                })
                .map_err(mergeable_read_error)?
                .unwrap_or(0);
            initial_values.insert(channel, value);
        }
        let diffs =
            RholangMergingLogic::calculate_num_channel_diff(channels_data, move |channel| {
                initial_values.get(channel).copied()
            });
        let deploy_channels = diffs
            .into_iter()
            .map(|data| DeployMergeableData {
                channels: data
                    .into_iter()
                    .map(|(hash, (diff, merge_type))| NumberChannel {
                        hash,
                        diff,
                        merge_type,
                    })
                    .collect(),
            })
            .collect();
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(post_state_hash.to_bytes_prost()),
            creator,
            seq_num,
        };
        let key_encoded = bincode::serialize(&mergeable_key).map_err(|error| {
            CasperError::KvStoreError(KvStoreError::SerializationError(error.to_string()))
        })?;
        self.mergeable_store.put_one(key_encoded, deploy_channels)?;
        Ok(())
    }

    /**
     * Converts number channels final values to difference values. Excludes channels without an initial value.
     *
     * @param channelsData Final values
     * @param preStateHash Inital state
     * @return Map with values as difference on number channel
     */
    pub fn convert_number_channels_to_diff(
        &self,
        channels_data: Vec<NumberChannelsEndVal>,
        // Used to calculate value difference from final values
        pre_state_hash: &Blake2b256Hash,
    ) -> Result<Vec<NumberChannelsDiff>, CasperError> {
        let history_repo = self.history_repo.clone();
        let reader = history_repo
            .get_history_reader(pre_state_hash)
            .map_err(|e| {
                CasperError::RuntimeError(format!(
                    "Failed to get history reader for pre-state hash: {:?}",
                    e
                ))
            })?;

        // Build a one-shot base-value map to avoid repeatedly creating history readers per key.
        let unique_channels = channels_data
            .iter()
            .flat_map(|m| m.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut initial_values: BTreeMap<Blake2b256Hash, i64> = BTreeMap::new();
        for ch in unique_channels {
            let data = reader.get_data(&ch).map_err(|e| {
                CasperError::RuntimeError(format!(
                    "Error getting data for channel {:?}: {:?}",
                    ch, e
                ))
            })?;
            if data.len() > 1 {
                return Err(CasperError::RuntimeError(format!(
                    "Expected at most one value for number channel {:?}, found {}",
                    ch,
                    data.len()
                )));
            }
            // None = channel doesn't exist (legitimate; start from 0). Some-but-non-numeric
            // is an invariant violation (channel-type stability is a contract-level
            // guarantee — interior nodes always numeric, leaves always Map). Treat as
            // hard failure so the merge is rejected rather than silently substituting 0.
            let value = match data.first() {
                None => 0,
                Some(datum) => match RholangMergingLogic::try_get_number_with_rnd(&datum.a) {
                    Some((n, _)) => n,
                    None => {
                        return Err(CasperError::RuntimeError(format!(
                            "Pre-state value for number channel {:?} is non-numeric; \
                             channel-type invariant violated",
                            ch,
                        )));
                    }
                },
            };
            initial_values.insert(ch, value);
        }

        // Calculate difference values from final values on number channels. The diff is
        // the wrapping group inverse (see calculate_num_channel_diff): it faithfully
        // recovers each deploy's intended delta even when execution overflowed. Over-large
        // deltas are rejected downstream at merge (combine checked_add / apply checked_add).
        Ok(RholangMergingLogic::calculate_num_channel_diff(
            channels_data,
            move |ch| initial_values.get(ch).copied(),
        ))
    }

    /**
     * This is a hard-coded value for `emptyStateHash` which is calculated by
     * [[coop.rchain.casper.rholang.RuntimeOps.emptyStateHash]].
     * Because of the value is actually the same all
     * the time. For some situations, we can just use the value directly for better performance.
     */
    pub fn empty_state_hash_fixed() -> StateHash {
        hex::decode("b38db9a0203b6b9cf5987024f325b83da33be5c1b820b3f86fd979578f2985d5")
            .unwrap()
            .into()
    }

    pub fn create_with_space_config(
        rspace: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        replay_rspace: ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        history_repo: RhoHistoryRepository,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
        exploratory_deploy_config: ExploratoryDeployConfig,
    ) -> RuntimeManager {
        let replay_cache_size = Self::max_replay_cache_entries();

        RuntimeManager {
            space: rspace,
            replay_space: replay_rspace,
            history_repo,
            mergeable_store,
            mergeable_tags,
            block_index_cache: Arc::new(DashMap::new()),
            block_index_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            active_validators_cache: Arc::new(DashMap::new()),
            active_validators_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            bonds_cache: Arc::new(DashMap::new()),
            bonds_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            parents_post_state_cache: Arc::new(DashMap::new()),
            parents_post_state_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            replay_cache: (replay_cache_size > 0).then(|| {
                Arc::new(InMemoryReplayCache::with_limits(
                    replay_cache_size,
                    Self::max_replay_cache_bytes(),
                ))
            }),
            replay_lock: Arc::new(ReplayLock::new()),
            exploratory_deploy_semaphore: Arc::new(Semaphore::new(
                exploratory_deploy_config.max_concurrent,
            )),
            exploratory_deploy_phlo_limit: exploratory_deploy_config.phlo_limit,
            exploratory_deploy_execution_timeout: exploratory_deploy_config.execution_timeout,
            external_services,
            #[cfg(any(test, feature = "test-utils"))]
            offered_budget_recorder: OfferedBudgetRecorder::default(),
        }
    }

    pub fn create_with_store(
        store: RSpaceStore,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
    ) -> RuntimeManager {
        let (rt_manager, _) =
            Self::create_with_history(store, mergeable_store, mergeable_tags, external_services);
        rt_manager
    }

    /// Test-only entry point: supplies `ExploratoryDeployConfig::for_tests()`.
    /// Production construction goes through `create_with_history_config` with
    /// the operator's configuration.
    pub fn create_with_history(
        store: RSpaceStore,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
    ) -> (RuntimeManager, RhoHistoryRepository) {
        Self::create_with_history_config(
            store,
            mergeable_store,
            mergeable_tags,
            external_services,
            ExploratoryDeployConfig::for_tests(),
        )
    }

    pub fn create_with_history_config(
        store: RSpaceStore,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
        exploratory_deploy_config: ExploratoryDeployConfig,
    ) -> (RuntimeManager, RhoHistoryRepository) {
        let (rspace, replay_rspace) =
            RSpace::create_with_replay(store, Arc::new(Box::new(Matcher)))
                .expect("Failed to create RSpaceWithReplay");

        let history_repo = rspace.get_history_repository();

        let runtime_manager = RuntimeManager::create_with_space_config(
            rspace,
            replay_rspace,
            history_repo.clone(),
            mergeable_store,
            mergeable_tags,
            external_services,
            exploratory_deploy_config,
        );

        (runtime_manager, history_repo)
    }

    /**
     * Creates connection to [[MergeableStore]] database.
     *
     * Mergeable (number) channels store is used in [[RuntimeManager]] implementation.
     * This function provides default instantiation.
     */
    pub async fn mergeable_store(
        kvm: &mut dyn KeyValueStoreManager,
    ) -> Result<MergeableStore, KvStoreError> {
        let store = kvm.store("mergeable-channel-cache".to_string()).await?;

        Ok(KeyValueTypedStoreImpl::new(store))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::Semaphore;

    use super::{ExploratoryDeployConfig, ReplayLock, RuntimeManager};

    /// D-E4 (DR-111): the copy of the authority presentations reserves one
    /// block slice copy and the release of the copy, in the walker's
    /// dimensions. An empty slice reserves nothing. The exact mirror at two
    /// sizes is the growth check. The copy accepts its exact credit and is
    /// rejected one unit short in each dimension that it uses.
    #[test]
    fn presentation_copy_charges_a_block_slice_copy_and_cleanup() {
        use std::cell::Cell;

        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::CostSignature;
        use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
        use rholang::rust::interpreter::host_work::HostWorkBudget;
        use shared::rust::clone_backing::{self, BackingError};

        use super::reserve_presentation_copy;

        let walker = [
            HostWorkDimension::VerificationOperations,
            HostWorkDimension::VerificationBytes,
            HostWorkDimension::SearchStateBytes,
        ];
        let walk = |presentations: &[CostSignature]| {
            let used = Cell::new([0u64; 3]);
            let meter =
                |operations: usize, scanned: usize, backing: usize| -> Result<(), BackingError> {
                    let [o, s, b] = used.get();
                    used.set([
                        o + operations as u64,
                        s + scanned as u64,
                        b + backing as u64,
                    ]);
                    Ok(())
                };
            clone_backing::reserve_blocks_slice_copy_and_cleanup(presentations, &meter)
                .expect("an unlimited walk");
            used.get()
        };
        let limited = |units: [u64; 3]| {
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(0));
            for (dimension, limit) in walker.into_iter().zip(units) {
                limits.set(dimension, HostWorkLimit::new(limit));
            }
            HostWorkBudget::new(limits)
        };
        let ground = |size: usize| CostSignature {
            value: Some(Value::Ground(vec![8; size])),
        };

        let empty = limited([0; 3]);
        reserve_presentation_copy(&[], &empty).expect("an empty copy reserves nothing");
        for dimension in HostWorkDimension::ALL {
            assert_eq!(empty.usage(dimension).get(), 0, "{dimension:?}");
        }

        for presentations in [vec![ground(32)], vec![ground(32), ground(4_096)]] {
            let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
            reserve_presentation_copy(&presentations, &budget).expect("an unlimited budget");
            let expected = walk(&presentations);
            for dimension in HostWorkDimension::ALL {
                let units = walker
                    .iter()
                    .position(|walked| *walked == dimension)
                    .map_or(0, |index| expected[index]);
                assert_eq!(budget.usage(dimension).get(), units, "{dimension:?}");
            }
        }

        let presentations = vec![ground(32), ground(4_096)];
        let expected = walk(&presentations);
        reserve_presentation_copy(&presentations, &limited(expected)).expect("the exact credit");
        for dimension in 0..3 {
            if expected[dimension] > 0 {
                let mut short = expected;
                short[dimension] -= 1;
                assert!(
                    reserve_presentation_copy(&presentations, &limited(short)).is_err(),
                    "dimension {dimension}"
                );
            }
        }
    }

    #[test]
    fn exploratory_deploy_config_rejects_non_positive_values() {
        assert!(ExploratoryDeployConfig::new(0, 5_000_000, Duration::from_secs(15)).is_err());
        assert!(ExploratoryDeployConfig::new(1, 0, Duration::from_secs(15)).is_err());
        assert!(ExploratoryDeployConfig::new(1, -1, Duration::from_secs(15)).is_err());
        assert!(ExploratoryDeployConfig::new(1, 5_000_000, Duration::ZERO).is_err());

        let valid =
            ExploratoryDeployConfig::new(2, 42, Duration::from_millis(500)).expect("valid config");
        assert_eq!(valid.max_concurrent, 2);
        assert_eq!(valid.phlo_limit, 42);
        assert_eq!(valid.execution_timeout, Duration::from_millis(500));
    }

    #[test]
    fn exploratory_deploy_concurrency_derivation() {
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(1), 2);
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(4), 2);
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(8), 6);
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(16), 14);

        let derived = ExploratoryDeployConfig::resolve(0, 5_000_000, Duration::from_secs(15))
            .expect("the sentinel derives, never rejects");
        assert!(derived.max_concurrent >= 2);

        let explicit = ExploratoryDeployConfig::resolve(5, 5_000_000, Duration::from_secs(15))
            .expect("explicit value");
        assert_eq!(explicit.max_concurrent, 5);

        let clamped =
            ExploratoryDeployConfig::resolve(usize::MAX, 5_000_000, Duration::from_secs(15))
                .expect("an oversized explicit value clamps instead of panicking");
        assert_eq!(clamped.max_concurrent, Semaphore::MAX_PERMITS);

        assert!(
            ExploratoryDeployConfig::resolve(0, 0, Duration::from_secs(15)).is_err(),
            "the sentinel does not bypass the other range checks"
        );
    }

    #[tokio::test]
    async fn consensus_replay_has_priority_over_queued_reporting() {
        let lock = Arc::new(ReplayLock::new());
        let first_consensus = lock.acquire_consensus().await.expect("Replay lock closed");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let reporting_lock = lock.clone();
        let reporting_tx = tx.clone();
        let reporting = tokio::spawn(async move {
            let _permit = reporting_lock
                .acquire_reporting()
                .await
                .expect("Replay lock closed");
            reporting_tx.send("reporting").expect("Receiver closed");
        });
        tokio::time::sleep(Duration::from_millis(10)).await;

        let consensus_lock = lock.clone();
        let consensus = tokio::spawn(async move {
            let _permit = consensus_lock
                .acquire_consensus()
                .await
                .expect("Replay lock closed");
            tx.send("consensus").expect("Receiver closed");
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(first_consensus);

        assert_eq!(rx.recv().await, Some("consensus"));
        assert_eq!(rx.recv().await, Some("reporting"));
        consensus.await.expect("Consensus task failed");
        reporting.await.expect("Reporting task failed");
    }

    #[tokio::test]
    async fn cancelled_consensus_waiter_releases_reporting() {
        let lock = Arc::new(ReplayLock::new());
        let reporting_permit = lock.acquire_reporting().await.expect("Replay lock closed");
        let consensus_lock = lock.clone();
        let consensus = tokio::spawn(async move { consensus_lock.acquire_consensus().await });
        tokio::time::sleep(Duration::from_millis(10)).await;
        consensus.abort();
        consensus
            .await
            .expect_err("Consensus task was not cancelled");
        drop(reporting_permit);

        let _permit = tokio::time::timeout(Duration::from_secs(1), lock.acquire_reporting())
            .await
            .expect("Reporting remained blocked")
            .expect("Replay lock closed");
    }

    #[test]
    fn exploratory_deploy_permit_is_bounded_and_released() {
        let semaphore = Arc::new(Semaphore::new(1));
        let first = RuntimeManager::try_acquire_exploratory_deploy_permit_with(semaphore.clone());
        assert!(first.is_some());

        let second = RuntimeManager::try_acquire_exploratory_deploy_permit_with(semaphore.clone());
        assert!(second.is_none());

        drop(first);

        let third = RuntimeManager::try_acquire_exploratory_deploy_permit_with(semaphore);
        assert!(third.is_some());
    }
}
