// See casper/src/main/scala/coop/rchain/casper/rholang/RuntimeSyntax.scala

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::OnceLock;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Cosigner, Signed};
use models::casper::{
    CostAuthorityByteEventProto, CostAuthorityEventProto, CostAuthorityResourceProto,
    CostAuthorityWitnessProto,
};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, GPrincipalId, GPrivate, GUnforgeable, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
// `normalizer_env_from_deploy` is replaced by `normalizer_env_from_cosigned_deploy`
// at the only remaining call site (inside `evaluate_cosigned`). The legacy `evaluate`
// path uplifts `Signed<DeployData>` to `Cosigned<DeployData>` via
// `Cosigned::from_single_signer` and delegates, so the legacy env builder is no
// longer reached from runtime.rs.
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::new_freevar_par;
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::accounting;
use rholang::rust::interpreter::accounting::authority::{
    stack_transfer_event_id, AuthorityBornStack, AuthorityEvent, AuthorityStackBirth,
    ResourceMultiset,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::{bootstrap_registry, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    BlockData, DeployAuthority, DeployData as SystemProcessDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::{MergeType, NumberChannelsEndVal};
use rspace_plus_plus::rspace::trace::event::{Event as RSpaceEvent, IOEvent};

#[derive(Clone, Copy)]
enum DefaultCostAuthority {
    Funders,
    Unit,
}

#[derive(Clone, Copy)]
enum AuthorityTraceItem {
    Comm([u8; 32]),
    Produce([u8; 32]),
}

fn causal_authority_events_from_trace(
    trace: impl IntoIterator<Item = AuthorityTraceItem>,
    events: &[AuthorityEvent<[u8; 32]>],
    require_authority_for_every_comm: bool,
) -> Result<Vec<AuthorityEvent<[u8; 32]>>, CasperError> {
    let trace = trace.into_iter().collect::<Vec<_>>();
    let mut by_identity = BTreeMap::new();
    for event in events {
        if by_identity.insert(event.event_id, event.clone()).is_some() {
            return Err(CasperError::InvalidCostSettlement(
                "authority execution produced a duplicate COMM identity".to_string(),
            ));
        }
    }
    let mut ordered = Vec::with_capacity(events.len());
    for item in trace.iter().copied() {
        match item {
            AuthorityTraceItem::Comm(identity) => match by_identity.remove(&identity) {
                Some(event) => ordered.push(event),
                None if require_authority_for_every_comm => {
                    return Err(CasperError::InvalidCostSettlement(
                        "committed COMM trace is missing its authority event".to_string(),
                    ));
                }
                None => {}
            },
            AuthorityTraceItem::Produce(produce_hash) => {
                let mut cell_index = 0u64;
                loop {
                    let identity = stack_transfer_event_id(&produce_hash, cell_index);
                    let Some(event) = by_identity.remove(&identity) else {
                        break;
                    };
                    ordered.push(event);
                    cell_index = cell_index.checked_add(1).ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "cost-stack transfer index overflow".to_string(),
                        )
                    })?;
                }
            }
        }
    }
    if !by_identity.is_empty() {
        let missing = by_identity
            .keys()
            .take(8)
            .map(hex::encode)
            .collect::<Vec<_>>()
            .join(",");
        return Err(CasperError::InvalidCostSettlement(format!(
            "authority execution contains {} event(s) absent from the committed RSpace trace: {}",
            by_identity.len(),
            missing
        )));
    }
    Ok(ordered)
}

pub(crate) fn causal_authority_events(
    deploy_log: &[RSpaceEvent],
    events: &[AuthorityEvent<[u8; 32]>],
) -> Result<Vec<AuthorityEvent<[u8; 32]>>, CasperError> {
    causal_authority_events_from_trace(authority_trace_items(deploy_log), events, true)
}

pub(crate) fn causal_authority_events_from_lifecycle_trace(
    deploy_log: &[RSpaceEvent],
    events: &[AuthorityEvent<[u8; 32]>],
) -> Result<Vec<AuthorityEvent<[u8; 32]>>, CasperError> {
    causal_authority_events_from_trace(authority_trace_items(deploy_log), events, false)
}

fn authority_trace_items(deploy_log: &[RSpaceEvent]) -> Vec<AuthorityTraceItem> {
    let mut trace = Vec::new();
    for event in deploy_log {
        match event {
            RSpaceEvent::Comm(comm) => {
                trace.extend(comm.produces.iter().map(|produce| {
                    AuthorityTraceItem::Produce(
                        produce
                            .hash
                            .bytes()
                            .try_into()
                            .expect("RSpace produce identity length"),
                    )
                }));
                trace.push(AuthorityTraceItem::Comm(
                    comm.cost_identity()
                        .bytes()
                        .try_into()
                        .expect("COMM identity length"),
                ));
            }
            RSpaceEvent::IoEvent(IOEvent::Produce(produce)) => {
                trace.push(AuthorityTraceItem::Produce(
                    produce
                        .hash
                        .bytes()
                        .try_into()
                        .expect("RSpace produce identity length"),
                ));
            }
            RSpaceEvent::IoEvent(IOEvent::Consume(_)) => {}
        }
    }
    trace
}

fn authority_resources_to_proto(
    resources: &rholang::rust::interpreter::accounting::authority::ResourceMultiset<[u8; 32]>,
) -> Vec<CostAuthorityResourceProto> {
    resources
        .0
        .iter()
        .map(|(key, amount)| CostAuthorityResourceProto {
            key: key.to_vec().into(),
            amount: *amount,
        })
        .collect()
}

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_REPLAY_SYSDEPLOY_EVAL_CONSUME_RESULT_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_EVAL_EVALUATE_SOURCE_TIME_METRIC, CASPER_METRICS_SOURCE,
    EVALUATE_SOURCE_WRAPPER_CALLS_METRIC, EVALUATE_SOURCE_WRAPPER_TIME_NS_METRIC,
    EVAL_SYSTEM_DEPLOY_WRAPPER_CALLS_METRIC, EVAL_SYSTEM_DEPLOY_WRAPPER_TIME_NS_METRIC,
};
use crate::rust::util::event_converter;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_result::SystemDeployResult;
use crate::rust::util::rholang::system_deploy_user_error::{
    SystemDeployPlatformFailure, SystemDeployUserError,
};
use crate::rust::util::rholang::tools::Tools;
use crate::rust::util::rholang::{interpreter_util, supply};

/// Process-wide ephemeral identity to sign exploratory deploys.
/// The key pair is generated randomly once per node process, so values derived
/// from it — including the signature, and therefore `rho:rchain:deployId` — are
/// stable within a process but not across restarts or between nodes.
static EXPLORATORY_KEY_PAIR: OnceLock<(PrivateKey, PublicKey)> = OnceLock::new();

fn exploratory_key_pair() -> &'static (PrivateKey, PublicKey) {
    EXPLORATORY_KEY_PAIR.get_or_init(|| Secp256k1.new_key_pair())
}

pub struct RuntimeOps {
    pub runtime: RhoRuntimeImpl,
}

impl RuntimeOps {
    pub fn new(runtime: RhoRuntimeImpl) -> Self { Self { runtime } }
}

#[allow(type_alias_bounds)]
pub type SysEvalResult<S: SystemDeployTrait> =
    (Either<SystemDeployUserError, S::Result>, EvaluateResult);

fn system_deploy_consume_all_pattern() -> BindPattern {
    BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }
}

/// Diagnostic label for a system deploy (closeBlock / slash / checkBalance /
/// redeem — precharge/refund no longer exist under the in-calculus cost
/// accounting, D3). Called lazily inside tracing field evaluation, so it
/// costs nothing unless the event is enabled.
fn system_deploy_kind<S: SystemDeployTrait>(sd: &S) -> &'static str {
    let any = sd.as_any();
    if any.downcast_ref::<CloseBlockDeploy>().is_some() {
        "closeBlock"
    } else if any.downcast_ref::<SlashDeploy>().is_some() {
        "slash"
    } else if any
        .downcast_ref::<crate::rust::util::rholang::costacc::check_balance::CheckBalance>()
        .is_some()
    {
        "checkBalance"
    } else if any
        .downcast_ref::<crate::rust::util::rholang::costacc::redeem_deploy::RedeemDeploy>()
        .is_some()
    {
        "redeem"
    } else {
        "other"
    }
}

impl RuntimeOps {
    /**
     * Because of the history legacy, the emptyStateHash does not really represent an empty trie.
     * The `emptyStateHash` is used as genesis block pre state which the state only contains registry
     * fixed channels in the state.
     */
    pub async fn empty_state_hash(&mut self) -> Result<StateHash, CasperError> {
        self.runtime
            .reset(&RadixHistory::empty_root_node_hash())
            .await?;

        bootstrap_registry(&self.runtime).await;
        let checkpoint = self.runtime.create_checkpoint().await;
        Ok(checkpoint.root.bytes().into())
    }

    /* Compute state with deploys (genesis block) and System deploys (regular block) */

    /// Multi-sig-aware variant of [`Self::compute_state`]. Takes
    /// `Vec<Cosigned<DeployData>>` so multi-signature deploys execute
    /// through signed-source metering and realized settlement at
    /// `play_deploys_for_state_cosigned`. For legacy single-signature
    /// deploys (1-element Cosigned envelopes), behavior is byte-identical.
    pub async fn compute_state_cosigned(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        system_deploys: Vec<crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
            Vec<(ProcessedSystemDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-cosigned-started");
        self.runtime.set_block_data(block_data.clone()).await;
        self.runtime.set_invalid_blocks(invalid_blocks).await;

        let (start_hash, processed_deploys) = self
            .play_deploys_for_state_cosigned(start_hash, terms)
            .await?;

        let (current_hash, processed_system_deploys) = self
            .play_system_deploys_for_state(&start_hash, system_deploys)
            .await?;

        Ok((current_hash, processed_deploys, processed_system_deploys))
    }

    pub(crate) async fn play_system_deploys_for_state(
        &mut self,
        start_hash: &StateHash,
        system_deploys: Vec<crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum>,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedSystemDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        let mut current_hash = start_hash.clone();
        let mut processed_system_deploys = Vec::with_capacity(system_deploys.len());
        for system_deploy_enum in system_deploys.into_iter() {
            let result = match system_deploy_enum {
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Slash(
                    mut slash_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut slash_deploy)
                        .await?
                }
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                    mut close_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut close_deploy)
                        .await?
                }
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Redeem(
                    mut redeem_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut redeem_deploy)
                        .await?
                }
            };
            match result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    mergeable_channels,
                    result: _,
                } => {
                    processed_system_deploys.push((processed_system_deploy, mergeable_channels));
                    current_hash = state_hash;
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Failed { error_msg, .. },
                } => {
                    return Err(CasperError::RuntimeError(format!(
                        "Unexpected system error during cosigned play of system deploy: {}",
                        error_msg
                    )));
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Succeeded { .. },
                } => {
                    return Err(CasperError::RuntimeError(
                        "Unreachable code path. This is likely caused by a bug in the runtime."
                            .to_string(),
                    ));
                }
            }
        }

        Ok((current_hash, processed_system_deploys))
    }

    /**
     * Evaluates deploys and System deploys with checkpoint to get final state hash
     */
    pub async fn compute_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
            Vec<(ProcessedSystemDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        // Using tracing events instead of spans for async context
        // Span[F].traceI("compute-state") equivalent from Scala
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-started");
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }
        if tracing::enabled!(target: "f1r3fly.casper.invalid_blocks", tracing::Level::DEBUG) {
            let entries: Vec<String> = invalid_blocks
                .iter()
                .map(|(bh, v)| {
                    format!(
                        "{}=>{}",
                        hex::encode(&bh[..8.min(bh.len())]),
                        hex::encode(&v[..8.min(v.len())])
                    )
                })
                .collect();
            tracing::debug!(target: "f1r3fly.casper.invalid_blocks", n = invalid_blocks.len(), seq = block_data.seq_num, "PLAY compute_state invalid_blocks: [{}]", entries.join(", "));
        }
        self.runtime.set_block_data(block_data).await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_set_block_data", rss_kb);
        }
        self.runtime.set_invalid_blocks(invalid_blocks).await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_set_invalid_blocks", rss_kb);
        }

        let (start_hash, processed_deploys) =
            self.play_deploys_for_state(start_hash, terms).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_play_deploys_for_state", rss_kb);
        }

        let mut current_hash = start_hash;
        let mut processed_system_deploys = Vec::with_capacity(system_deploys.len());

        for system_deploy_enum in system_deploys {
            // Match on the enum and call appropriate generic method
            let result = match system_deploy_enum {
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Slash(
                    mut slash_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut slash_deploy)
                        .await?
                }
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                    mut close_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut close_deploy)
                        .await?
                }
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Redeem(
                    mut redeem_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut redeem_deploy)
                        .await?
                }
            };

            match result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    mergeable_channels,
                    result: _,
                } => {
                    processed_system_deploys.push((processed_system_deploy, mergeable_channels));
                    current_hash = state_hash;
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Failed { error_msg, .. },
                } => {
                    return Err(CasperError::RuntimeError(format!(
                        "Unexpected system error during play of system deploy: {}",
                        error_msg
                    )))
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Succeeded { .. },
                } => {
                    return Err(CasperError::RuntimeError(
                        "Unreachable code path. This is likely caused by a bug in the runtime."
                            .to_string(),
                    ))
                }
            }
        }

        let post_state_hash = current_hash;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "finish", rss_kb);
        }

        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-finished");
        Ok((post_state_hash, processed_deploys, processed_system_deploys))
    }

    /**
     * Evaluates genesis deploys with checkpoint to get final state hash
     */
    pub async fn compute_genesis(
        &mut self,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_time: i64,
        block_number: i64,
    ) -> Result<
        (
            StateHash,
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        // Using tracing events instead of spans for async context
        // Span[F].traceI("compute-genesis") equivalent from Scala
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-genesis-started");
        self.runtime
            .set_block_data(BlockData {
                time_stamp: block_time,
                block_number,
                sender: PublicKey::from_bytes(&Vec::new()),
                seq_num: 0,
            })
            .await;

        let genesis_pre_state_hash = self.empty_state_hash().await?;
        let play_result = self
            .play_deploys_for_genesis(&genesis_pre_state_hash, terms)
            .await?;

        let (_, processed_deploys) = play_result;
        let post_state_hash = self.runtime.create_checkpoint().await.root.to_bytes_prost();
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-genesis-finished");
        Ok((genesis_pre_state_hash, post_state_hash, processed_deploys))
    }

    /* Deploy evaluators */

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     * */
    /// Multi-signature-aware variant of [`Self::play_deploys_for_state`].
    /// Accepts `Vec<Cosigned<DeployData>>` so multi-signature deploys preserve
    /// their complete authority envelope through execution and realized-cost
    /// settlement. For legacy single-signature deploys (1-element Cosigned
    /// envelopes), behavior is byte-identical to `play_deploys_for_state`.
    pub async fn play_deploys_for_state_cosigned(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        let (state, processed, exhausted) = self
            .play_deploys_for_state_cosigned_internal(start_hash, terms, false, None)
            .await?;
        debug_assert!(exhausted.is_empty());
        Ok((state, processed))
    }

    pub(crate) async fn state_bound_cost_evidence_for_state_cosigned(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        fee_recipient: &PublicKey,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
            crate::rust::util::rholang::acceptance::AdmissionOutcome,
        ),
        CasperError,
    > {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;
        let mut current_root = start_hash.clone();
        let mut accepted = Vec::with_capacity(terms.len());
        let mut outcome = crate::rust::util::rholang::acceptance::AdmissionOutcome::default();
        let mut closed_groups = std::collections::BTreeSet::new();
        let fee_address =
            rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(
                fee_recipient,
            )
            .ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "block proposer has no canonical SystemVault address".to_string(),
                )
            })?
            .to_base58();

        for cosigned in terms {
            let group_key = accounting::funding_sig(&cosigned).lane_hash();
            if closed_groups.contains(&group_key) {
                outcome
                    .rejected
                    .push(crate::rust::util::rholang::acceptance::admission_deploy_id(
                        &cosigned,
                    ));
                continue;
            }
            let pre_state_root: [u8; 32] = current_root.as_ref().try_into().map_err(|_| {
                CasperError::InvalidCostSettlement(
                    "authority reservation pre-state is not Blake2b-256".to_string(),
                )
            })?;
            let mut frontier_by_encoding = BTreeMap::new();
            let mut previous_capacity = None;
            let discovered = loop {
                let frontier = frontier_by_encoding.values().cloned().collect::<Vec<_>>();
                let capacity = {
                    let reader = crate::rust::util::rholang::acceptance::RuntimeOpsSupplyReader {
                        runtime_ops: self,
                        pre_state_root,
                    };
                    crate::rust::util::rholang::acceptance::state_bound_execution_cap_with_frontier(
                        &cosigned, &frontier, &reader,
                    )
                    .await
                };
                let capacity = match capacity {
                    Ok(capacity) => capacity,
                    Err(CasperError::InvalidCostSettlement(reason)) => {
                        tracing::debug!(reason, "state-bound capacity derivation rejected deploy");
                        break None;
                    }
                    Err(error) => return Err(error),
                };
                if previous_capacity.is_some_and(|previous| capacity <= previous) {
                    tracing::debug!(capacity, "state-bound frontier did not increase capacity");
                    break None;
                }
                previous_capacity = Some(capacity);
                let (processed, user_mergeable, exhausted) = self
                    .process_deploy_cosigned_with_budget_and_authority(
                        cosigned.clone(),
                        Cost::create(capacity, "state-bound authority capacity"),
                        None,
                        false,
                    )
                    .await?;
                if exhausted {
                    let before = frontier_by_encoding.len();
                    for authority in self.runtime.cost.authority_frontier() {
                        frontier_by_encoding.insert(authority.encode_to_vec(), authority);
                    }
                    self.runtime
                        .reset(&Blake2b256Hash::from_bytes_prost(&current_root))
                        .await?;
                    if frontier_by_encoding.len() == before {
                        tracing::debug!(
                            capacity,
                            "state-bound exhaustion exposed no new authenticated authority"
                        );
                        break None;
                    }
                    continue;
                }

                let mut witness_proto = processed
                    .authority_cost_witness
                    .as_ref()
                    .ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "state-bound execution is missing its authority witness".to_string(),
                        )
                    })?
                    .clone();
                if witness_proto.pre_state_root.is_empty() {
                    witness_proto.pre_state_root = pre_state_root.to_vec().into();
                }
                let checkpoint = self.runtime.create_checkpoint().await;
                let user_post_state = checkpoint.root.to_bytes_prost();
                if witness_proto.post_state_root.is_empty() {
                    witness_proto.post_state_root = user_post_state.clone();
                }
                let mut witness =
                    crate::rust::util::rholang::acceptance::authority_witness_from_proto(
                        &witness_proto,
                        true,
                    )?;
                witness.pre_state_root = pre_state_root;
                witness.post_state_root = user_post_state.as_ref().try_into().map_err(|_| {
                    CasperError::InvalidCostSettlement(
                        "state-bound user post-state is not Blake2b-256".to_string(),
                    )
                })?;
                break Some((processed, user_mergeable, witness, user_post_state));
            };

            let Some((mut processed, user_mergeable, mut witness, user_post_state)) = discovered
            else {
                self.runtime
                    .reset(&Blake2b256Hash::from_bytes_prost(&current_root))
                    .await?;
                closed_groups.insert(group_key);
                outcome
                    .rejected
                    .push(crate::rust::util::rholang::acceptance::admission_deploy_id(
                        &cosigned,
                    ));
                continue;
            };

            self.runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&current_root))
                .await?;
            let prepared = {
                let reader = crate::rust::util::rholang::acceptance::RuntimeOpsSupplyReader {
                    runtime_ops: self,
                    pre_state_root,
                };
                crate::rust::util::rholang::acceptance::prepare_state_bound_authority_reservation(
                    &cosigned,
                    &witness,
                    &reader,
                    &fee_recipient.bytes,
                )
                .await
            };
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(CasperError::InvalidCostSettlement(reason)) => {
                    tracing::debug!(reason, "state-bound physical reservation rejected deploy");
                    closed_groups.insert(group_key);
                    outcome.rejected.push(
                        crate::rust::util::rholang::acceptance::admission_deploy_id(&cosigned),
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            self.runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&user_post_state))
                .await?;

            let lifecycle = async {
                let reserved_resources = prepared
                    .certificate
                    .allocation
                    .checked_add(&prepared.certificate.byte_allocation)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
                    .checked_add(&prepared.certificate.fee_allocation)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                let mut reserve_allocations = Vec::new();
                for (key, amount) in &reserved_resources.0 {
                    let signature = prepared.signatures.get(key).ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "vault reservation references an unresolved signature".to_string(),
                        )
                    })?;
                    let payer = crate::rust::util::rholang::costacc::vault_payer::vault_payer(
                        signature,
                    )
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                    reserve_allocations.push(
                        crate::rust::util::rholang::costacc::vault_cost_deploy::VaultAllocation::new(
                            payer.address.to_base58(),
                            i64::try_from(*amount).map_err(|_| {
                                CasperError::InvalidCostSettlement(
                                    "vault reservation exceeds the platform range".to_string(),
                                )
                            })?,
                        )?,
                    );
                }
                reserve_allocations.push(
                    crate::rust::util::rholang::costacc::vault_cost_deploy::VaultAllocation::new(
                        fee_address.clone(),
                        crate::rust::util::rholang::costacc::VALIDATOR_HANDLER_COST_PER_DEPLOY,
                    )?,
                );
                let mut mergeable = user_mergeable;
                let mut reserved_inventory = prepared.reserved_inventory()?;
                let mut settlement_signatures = prepared.signatures.clone();
                for birth in &witness.born_stacks {
                    if reserved_inventory
                        .stacks
                        .insert(birth.stack_id, birth.cells.clone())
                        .is_some()
                        || reserved_inventory
                            .born_stacks
                            .insert(birth.stack_id, birth.produce_hash)
                            .is_some()
                    {
                        return Err(CasperError::InvalidCostSettlement(
                            "born authority stack collides with reserved inventory".to_string(),
                        ));
                    }
                    for cell in &birth.cells {
                        let signature = rholang::rust::interpreter::accounting::authority::canonical_cost_signature(cell)
                            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                        let key = rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(&signature)
                            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
                            .lane_hash();
                        match settlement_signatures.get(&key) {
                            Some(existing) if existing != &signature => {
                                return Err(CasperError::InvalidCostSettlement(
                                    "born authority stack signature collides with its lane"
                                        .to_string(),
                                ));
                            }
                            Some(_) => {}
                            None => {
                                settlement_signatures.insert(key, signature);
                            }
                        }
                    }
                }
                let physical_settlement =
                    rholang::rust::interpreter::accounting::authority::allocate_physical_settlement(
                        &witness.events,
                        &settlement_signatures,
                        &reserved_inventory,
                    )
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                rholang::rust::interpreter::accounting::authority::verify_physical_settlement(
                    &witness.events,
                    &settlement_signatures,
                    &reserved_inventory,
                    &physical_settlement.draws,
                )
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                if physical_settlement != prepared.maximum_cost_settlement {
                    return Err(CasperError::InvalidCostSettlement(
                        "retained state-bound execution changed its physical authority settlement"
                            .to_string(),
                    ));
                }
                let after_cost = prepared
                    .inventory
                    .balances
                    .checked_sub(&physical_settlement.balance_debit)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                let byte_settlement = rholang::rust::interpreter::accounting::authority::allocate_quantitative_events(
                    &witness.byte_events,
                    &after_cost,
                )
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                if byte_settlement != prepared.certificate.byte_allocation {
                    return Err(CasperError::InvalidCostSettlement(
                        "retained state-bound execution changed its quantitative byte settlement"
                            .to_string(),
                    ));
                }

                let mut settlement_stacks = prepared
                    .purse_stacks
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                settlement_stacks.extend(
                    self.resolve_authority_born_purse_stacks(&witness.born_stacks)
                        .await?,
                );
                supply::apply_stack_pops(
                    self,
                    &settlement_stacks,
                    &physical_settlement.stack_pops,
                )
                .await?;
                let stack_log = self
                    .runtime
                    .take_event_log()
                    .await
                    .into_iter()
                    .map(event_converter::to_casper_event)
                    .collect::<Vec<_>>();

                let mut settlements = Vec::new();
                for (key, reserved_amount) in &reserved_resources.0 {
                    let signature = prepared.signatures.get(key).ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "vault settlement references an unresolved signature".to_string(),
                        )
                    })?;
                    let payer = crate::rust::util::rholang::costacc::vault_payer::vault_payer(
                        signature,
                    )
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                    let burn = physical_settlement.balance_debit.get(key);
                    let byte_burn = byte_settlement.get(key);
                    let fee = prepared.certificate.fee_allocation.get(key);
                    let total_burn = burn.checked_add(byte_burn).ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "actual vault burn overflows u64".to_string(),
                        )
                    })?;
                    if total_burn
                        .checked_add(fee)
                        .is_none_or(|total| total > *reserved_amount)
                    {
                        return Err(CasperError::InvalidCostSettlement(
                            "actual vault settlement exceeds its reservation".to_string(),
                        ));
                    }
                    settlements.push(
                        crate::rust::util::rholang::costacc::vault_cost_deploy::VaultSettlement::new(
                            payer.address.to_base58(),
                            i64::try_from(total_burn).map_err(|_| {
                                CasperError::InvalidCostSettlement(
                                    "vault burn exceeds the platform range".to_string(),
                                )
                            })?,
                            i64::try_from(fee).map_err(|_| {
                                CasperError::InvalidCostSettlement(
                                    "vault fee exceeds the platform range".to_string(),
                                )
                            })?,
                        )?,
                    );
                }
                settlements.push(
                    crate::rust::util::rholang::costacc::vault_cost_deploy::VaultSettlement::new(
                        fee_address.clone(),
                        crate::rust::util::rholang::costacc::VALIDATOR_HANDLER_COST_PER_DEPLOY,
                        0,
                    )?,
                );
                let mut apply =
                    crate::rust::util::rholang::costacc::vault_cost_deploy::ApplyCostDeploy::new(
                        prepared.certificate.reservation_id,
                        reserve_allocations,
                        settlements,
                        fee_address.clone(),
                        crate::rust::util::rholang::costacc::vault_cost_deploy::lifecycle_random(
                            &prepared.certificate.reservation_id,
                            1,
                        ),
                    )?;
                let (apply_log, apply_result, apply_mergeable) =
                    self.play_system_deploy_internal(&mut apply).await?;
                if let Either::Left(error) = apply_result {
                    tracing::debug!(
                        error = ?error,
                        "state-bound atomic vault application rejected deploy"
                    );
                    return Ok(None);
                }
                mergeable.extend(apply_mergeable);

                witness.certificate_id = prepared.certificate.certificate_id();
                witness.pre_state_root = pre_state_root;
                witness.settlement = physical_settlement.balance_debit.clone();
                witness.byte_settlement = byte_settlement;
                witness.physical_draws = physical_settlement.draws;
                witness
                    .verify_event_authorities()
                    .and_then(|_| {
                        witness.verify_with_settlement(
                            &prepared.certificate,
                            |_, _, _| Ok(witness.settlement.clone()),
                        )
                    })
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;

                let mut lifecycle_log = std::mem::take(&mut processed.deploy_log);
                lifecycle_log.extend(stack_log);
                lifecycle_log.extend(apply_log);
                processed.deploy_log = lifecycle_log;
                processed.authority_funding_certificate = Some(
                    crate::rust::util::rholang::acceptance::authority_certificate_to_proto(
                        &prepared.certificate,
                    ),
                );
                processed.authority_cost_witness = Some(
                    crate::rust::util::rholang::acceptance::authority_witness_to_proto(&witness),
                );
                Ok(Some((processed, mergeable, witness)))
            }
            .await;

            let Some((mut processed, mergeable, mut witness)) = (match lifecycle {
                Ok(result) => result,
                Err(error) => {
                    self.runtime
                        .reset(&Blake2b256Hash::from_bytes_prost(&current_root))
                        .await?;
                    return Err(error);
                }
            }) else {
                self.runtime
                    .reset(&Blake2b256Hash::from_bytes_prost(&current_root))
                    .await?;
                closed_groups.insert(group_key);
                outcome
                    .rejected
                    .push(crate::rust::util::rholang::acceptance::admission_deploy_id(
                        &cosigned,
                    ));
                continue;
            };

            let mergeable = self.get_number_channels_data(&mergeable).await?;
            let checkpoint = self.runtime.create_checkpoint().await;
            let next_root = checkpoint.root.to_bytes_prost();
            processed.pre_state_hash = current_root;
            processed.post_state_hash = next_root.clone();
            witness.post_state_root = next_root.as_ref().try_into().map_err(|_| {
                CasperError::InvalidCostSettlement(
                    "authority settlement post-state is not Blake2b-256".to_string(),
                )
            })?;
            processed.authority_cost_witness =
                Some(crate::rust::util::rholang::acceptance::authority_witness_to_proto(&witness));
            current_root = next_root;
            for (stack_id, pop_count) in &prepared.maximum_cost_settlement.stack_pops {
                let total = outcome.stack_pops.entry(*stack_id).or_default();
                *total = total.checked_add(*pop_count).ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "authority stack pop count overflow".to_string(),
                    )
                })?;
            }
            for (stack_id, stack) in &prepared.purse_stacks {
                if outcome
                    .purse_stacks
                    .insert(*stack_id, stack.clone())
                    .is_some()
                {
                    return Err(CasperError::InvalidCostSettlement(
                        "committed authority outcome contains a duplicate stack identity"
                            .to_string(),
                    ));
                }
            }
            let channels = prepared
                .signatures
                .iter()
                .map(|(key, signature)| {
                    let funding =
                        rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(
                            signature,
                        )
                        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                    Ok((*key, supply::supply_channel(&funding)))
                })
                .collect::<Result<BTreeMap<_, _>, CasperError>>()?;
            crate::rust::util::rholang::acceptance::record_authority_debits(
                &mut outcome.debits,
                &prepared.maximum_cost_settlement.balance_debit,
                &channels,
            )?;
            crate::rust::util::rholang::acceptance::record_authority_debits(
                &mut outcome.debits,
                &prepared.certificate.byte_allocation,
                &channels,
            )?;
            crate::rust::util::rholang::acceptance::record_authority_debits(
                &mut outcome.fee_debits,
                &prepared.certificate.fee_allocation,
                &channels,
            )?;
            outcome.admitted.push(cosigned);
            accepted.push((processed, mergeable));
        }

        Ok((current_root, accepted, outcome))
    }

    async fn play_deploys_for_state_cosigned_internal(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        retain_exhausted: bool,
        execution_caps: Option<&[i64]>,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
            Vec<Bytes>,
        ),
        CasperError,
    > {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "play_deploys_for_state_cosigned.mem step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    step, curr, curr as i64 - prev as i64, curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };

        tracing::info!(target: "f1r3fly.casper.play-deploys-cosigned", "play-deploys-cosigned-started");
        log_mem_step("start");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;
        log_mem_step("after_reset");

        if execution_caps.is_some_and(|caps| caps.len() != terms.len()) {
            return Err(CasperError::InvalidCostSettlement(
                "authority-derived execution capacity count differs from the deploy count"
                    .to_string(),
            ));
        }

        let mut res = Vec::with_capacity(terms.len());
        let mut exhausted = Vec::new();
        let mut current_root = start_hash.clone();
        for (idx, cosigned) in terms.into_iter().enumerate() {
            if mem_profile_enabled {
                let before = format!("before_deploy_{}", idx + 1);
                log_mem_step(&before);
            }
            let primary_sig = cosigned.primary().sig.clone();
            let budget = execution_caps
                .map(|caps| Cost::create(caps[idx], "authority-derived execution capacity"))
                .unwrap_or_else(Cost::unsafe_max);
            let (mut processed, mergeable, did_exhaust) = self
                .process_deploy_cosigned_with_budget_and_authority(cosigned, budget, None, true)
                .await?;
            if did_exhaust {
                if !retain_exhausted {
                    return Err(CasperError::InvalidCostSettlement(format!(
                        "admitted deploy {} exhausted its state-bound execution capacity",
                        hex::encode(&primary_sig)
                    )));
                }
                exhausted.push(primary_sig);
            }
            let mergeable = self.get_number_channels_data(&mergeable).await?;
            let checkpoint = self.runtime.create_checkpoint().await;
            let next_root = checkpoint.root.to_bytes_prost();
            processed.pre_state_hash = current_root;
            processed.post_state_hash = next_root.clone();
            if let Some(witness) = processed.authority_cost_witness.as_mut() {
                witness.pre_state_root = processed.pre_state_hash.clone();
                witness.post_state_root = processed.post_state_hash.clone();
            }
            current_root = next_root;
            res.push((processed, mergeable));
            if mem_profile_enabled {
                let after = format!("after_deploy_{}", idx + 1);
                log_mem_step(&after);
            }
        }

        log_mem_step("after_final_checkpoint");
        Ok((current_root, res, exhausted))
    }

    pub async fn play_deploys_for_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play_deploys", "play-deploys-started");
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_reset", rss_kb);
        }

        let mut res = Vec::with_capacity(terms.len());
        let mut current_root = start_hash.clone();
        for deploy in terms {
            let (mut processed, mergeable) = self.play_ordinary_deploy(deploy).await?;
            let checkpoint = self.runtime.create_checkpoint().await;
            let next_root = checkpoint.root.to_bytes_prost();
            processed.pre_state_hash = current_root;
            processed.post_state_hash = next_root.clone();
            current_root = next_root;
            res.push((processed, mergeable));
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_final_checkpoint", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_final_checkpoint_create_checkpoint", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_final_checkpoint_create_checkpoint", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_final_checkpoint_root_to_bytes", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_final_checkpoint_root_to_bytes", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_final_checkpoint", rss_kb);
        }
        Ok((current_root, res))
    }

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     */
    pub async fn play_deploys_for_genesis(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play_deploys_genesis", "play-deploys-genesis-started");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;

        let mut res = Vec::with_capacity(terms.len());
        let mut current_root = start_hash.clone();
        for cosigned in terms {
            let (mut processed, mergeable, _) = self
                .process_deploy_cosigned_with_budget_and_authority_mode(
                    cosigned,
                    Cost::unsafe_max(),
                    None,
                    DefaultCostAuthority::Unit,
                    true,
                )
                .await?;
            let mergeable = self.get_number_channels_data(&mergeable).await?;
            let checkpoint = self.runtime.create_checkpoint().await;
            let next_root = checkpoint.root.to_bytes_prost();
            processed.pre_state_hash = current_root;
            processed.post_state_hash = next_root.clone();
            current_root = next_root;
            res.push((processed, mergeable));
        }
        Ok((current_root, res))
    }

    /// Evaluates a legacy single-signature deploy under the canonical
    /// reservation and realized-cost settlement protocol. The adapter preserves
    /// the deploy identifier and cost trace by uplifting to a one-signer
    /// `Cosigned<DeployData>` envelope and delegating to the canonical path.
    pub async fn play_ordinary_deploy(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy)
            .map_err(|e| {
                CasperError::RuntimeError(format!("legacy uplift to Cosigned failed: {e}"))
            })?;
        self.play_ordinary_deploy_cosigned(cosigned).await
    }

    /// Multi-signature aware deploy execution with cost accounting.
    ///
    /// D3 (DR-9, OD-1/OD-2): the singular-phlo escrow model is REMOVED. There
    /// is no per-cosigner pre-charge/refund fan-out. Production admission first
    /// evaluates the candidate once with the finite capacity derived from its
    /// authority supply and retains that execution as the block witness.
    /// Exhaustion is a rejection and cannot become a certificate. An admitted
    /// deploy therefore has exact state-bound evidence that its complete cost
    /// fits the capacity, while `total_cost()` records the canonical RSpace
    /// introduction, payload-transfer, trace-byte, and COMM execution cost. The
    /// single supply decrement is applied at block close after that witnessed
    /// user state.
    ///
    /// This is now a thin wrapper over [`Self::process_deploy_cosigned`] (which
    /// owns the INNER soft-checkpoint that rolls back a FAILED user deploy's
    /// effects), plus the mergeable-channel data collection. `cost` on the
    /// returned `ProcessedDeploy` is the canonical weighted `total_cost()`.
    pub async fn play_ordinary_deploy_cosigned(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        tracing::debug!(target: "f1r3fly.casper.play-deploy", "play-deploy-started");
        let primary_pk_hex = hex::encode(&cosigned.primary().pk.bytes);

        // USER DEPLOY (owns its own inner soft-checkpoint for failed-deploy
        // rollback). The admission gate certified and reserved authority; the
        // realized debit is checked and applied at block close.
        tracing::debug!(target: "f1r3fly.casper.user-deploy",
            "user-deploy-started primary_pk={}", primary_pk_hex);
        let (pd, mc) = self.process_deploy_cosigned(cosigned).await?;

        let mut mergeable: HashMap<Par, MergeType> = HashMap::new();
        mergeable.extend(mc);
        let mergeable_channels_data = self.get_number_channels_data(&mergeable).await?;
        Ok((pd, mergeable_channels_data))
    }

    /// Legacy single-signature user-deploy execution. Uplifts to
    /// `Cosigned<DeployData>` and delegates to [`Self::process_deploy_cosigned`]
    /// for byte-identical observable behavior.
    pub async fn process_deploy(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>), CasperError> {
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy)
            .map_err(|e| {
                CasperError::RuntimeError(format!(
                    "legacy uplift to Cosigned failed in process_deploy: {e}"
                ))
            })?;
        self.process_deploy_cosigned(cosigned).await
    }

    /// Multi-signature aware user-deploy execution. Keeps the INNER
    /// soft-checkpoint that wraps the user deploy ONLY — on user-deploy errors
    /// the inner scope reverts the user deploy's effects so a failed deploy
    /// leaves no residue. Admission has reserved authority against Σ⟦s⟧, but
    /// settlement is deferred until the realized cost is known.
    ///
    /// `cost` on the returned `ProcessedDeploy` is the canonical weighted
    /// `total_cost()`: one execution unit per committed COMM plus quantitative
    /// introduction, payload-transfer, and trace bytes. The
    /// `ProcessedDeploy.deploy: Signed<DeployData>` storage shape is
    /// preserved by reconstituting the primary signer's `Signed<DeployData>`
    /// envelope via `Cosigned::into_legacy_signed_unchecked` — invariants
    /// were already enforced at `Cosigned::from_signed_data` construction so
    /// no re-verification is needed.
    pub async fn process_deploy_cosigned(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>), CasperError> {
        let (processed, mergeable, _) = self
            .process_deploy_cosigned_with_budget(cosigned, Cost::unsafe_max())
            .await?;
        Ok((processed, mergeable))
    }

    async fn process_deploy_cosigned_with_budget(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>, bool), CasperError> {
        self.process_deploy_cosigned_with_budget_and_authority(cosigned, budget, None, true)
            .await
    }

    async fn process_deploy_cosigned_with_budget_and_authority(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
        report_exhaustion: bool,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>, bool), CasperError> {
        self.process_deploy_cosigned_with_budget_and_authority_mode(
            cosigned,
            budget,
            authority_allocation,
            DefaultCostAuthority::Funders,
            report_exhaustion,
        )
        .await
    }

    async fn process_deploy_cosigned_with_budget_and_authority_mode(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
        default_authority: DefaultCostAuthority,
        report_exhaustion: bool,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>, bool), CasperError> {
        // INNER soft-checkpoint — wraps the USER DEPLOY only. On a failed user
        // deploy it reverts that deploy's effects (D3: no pre-charge state).
        let fallback = self.runtime.create_soft_checkpoint().await;

        let eval_result = match self
            .evaluate_cosigned_with_budget_and_authority_mode(
                &cosigned,
                budget,
                authority_allocation,
                default_authority,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(fallback).await;
                return Err(error);
            }
        };

        let deploy_log = self.runtime.take_event_log().await;
        let authority_events =
            match causal_authority_events(&deploy_log, &eval_result.authority_events) {
                Ok(events) => events,
                Err(error) => {
                    self.runtime.revert_to_soft_checkpoint(fallback).await;
                    return Err(error);
                }
            };

        let eval_succeeded = eval_result.errors.is_empty();
        let born_stacks = if eval_succeeded {
            match self
                .resolve_authority_stack_births(&eval_result.authority_stack_births)
                .await
            {
                Ok(births) => births,
                Err(error) => {
                    self.runtime.revert_to_soft_checkpoint(fallback).await;
                    return Err(error);
                }
            }
        } else {
            Vec::new()
        };
        let exhausted = eval_result
            .errors
            .iter()
            .any(|error| matches!(error, InterpreterError::OutOfPhlogistonsError));
        let deploy_id = crate::rust::util::rholang::acceptance::admission_deploy_id(&cosigned);
        let preserved = ProcessedDeploy::empty_from_cosigned(&cosigned);

        let deploy_log = deploy_log
            .into_iter()
            .map(event_converter::to_casper_event)
            .collect::<Vec<_>>();
        let deploy_result = ProcessedDeploy {
            deploy: preserved.deploy,
            envelope_commitment: preserved.envelope_commitment,
            cost: Cost::to_proto(eval_result.cost),
            deploy_log,
            is_failed: !eval_succeeded,
            system_deploy_error: None,
            cosigners: preserved.cosigners,
            cosigner_threshold: preserved.cosigner_threshold,
            pre_state_hash: StateHash::new(),
            post_state_hash: StateHash::new(),
            authority_funding_certificate: None,
            authority_cost_witness: Some(CostAuthorityWitnessProto {
                protocol_version: rholang::rust::interpreter::accounting::authority::AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                certificate_id: Bytes::new(),
                pre_state_root: Bytes::new(),
                post_state_root: Bytes::new(),
                events: authority_events
                    .iter()
                    .map(|event| CostAuthorityEventProto {
                        event_id: event.event_id.to_vec().into(),
                        debit: authority_resources_to_proto(&event.debit),
                        authority: Some(event.authority.clone()),
                    })
                    .collect(),
                realized: authority_resources_to_proto(&eval_result.authority_realized),
                settlement: Vec::new(),
                physical_draws: Vec::new(),
                born_stacks: born_stacks
                    .iter()
                    .map(|birth| models::casper::CostAuthorityBornStackProto {
                        stack_id: birth.stack_id.to_vec().into(),
                        produce_hash: birth.produce_hash.to_vec().into(),
                        cells: birth.cells.clone(),
                    })
                    .collect(),
                byte_cost_schedule_version: rholang::rust::interpreter::accounting::byte_accounting::BYTE_COST_SCHEDULE_VERSION,
                byte_cost_schedule_digest: rholang::rust::interpreter::accounting::byte_accounting::byte_cost_schedule_digest().to_vec().into(),
                byte_events: eval_result
                    .authority_byte_events
                    .iter()
                    .map(|event| CostAuthorityByteEventProto {
                        event_id: event.event_id.to_vec().into(),
                        kind: i32::from(event.kind.tag()),
                        authority: Some(event.authority.clone()),
                        amount: event.amount,
                    })
                    .collect(),
                byte_cost: eval_result.quantitative_byte_cost,
                byte_settlement: Vec::new(),
            }),
            admission_status: Default::default(),
        };

        if !eval_succeeded {
            self.runtime.revert_to_soft_checkpoint(fallback).await;
            if !exhausted || report_exhaustion {
                interpreter_util::print_deploy_errors(
                    &Bytes::copy_from_slice(deploy_id.as_bytes()),
                    &eval_result.errors,
                );
            }
        }

        Ok((deploy_result, eval_result.mergeable, exhausted))
    }

    pub(crate) async fn resolve_authority_stack_births(
        &self,
        births: &[AuthorityStackBirth],
    ) -> Result<Vec<AuthorityBornStack>, CasperError> {
        let mut resolved = Vec::with_capacity(births.len());
        for birth in births {
            let head = birth.cells.first().ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "authority stack birth has no resource cells".to_string(),
                )
            })?;
            let signature =
                rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(head)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let channel = supply::supply_channel(&signature);
            let data = self.get_data_datums(&channel).await;
            let inventory = supply::decode_purse_inventory(&data, head)?;
            let matches = inventory
                .stacks
                .into_iter()
                .filter(|stack| {
                    stack.source_hash == birth.produce_hash && stack.stack.cells == birth.cells
                })
                .collect::<Vec<_>>();
            let [stack] = matches.as_slice() else {
                return Err(CasperError::InvalidCostSettlement(
                    "authority stack birth does not identify exactly one live resource".to_string(),
                ));
            };
            resolved.push(AuthorityBornStack {
                stack_id: stack.instance_id,
                produce_hash: birth.produce_hash,
                cells: birth.cells.clone(),
            });
        }
        resolved.sort_by_key(|birth| birth.stack_id);
        if resolved
            .windows(2)
            .any(|pair| pair[0].stack_id == pair[1].stack_id)
        {
            return Err(CasperError::InvalidCostSettlement(
                "authority stack births contain a duplicate resource identity".to_string(),
            ));
        }
        Ok(resolved)
    }

    pub(crate) async fn resolve_authority_born_purse_stacks(
        &self,
        births: &[AuthorityBornStack],
    ) -> Result<Vec<supply::PurseStack>, CasperError> {
        let mut resolved = Vec::with_capacity(births.len());
        for birth in births {
            let head = birth.cells.first().ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "authority born stack has no resource cells".to_string(),
                )
            })?;
            let signature =
                rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(head)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let channel = supply::supply_channel(&signature);
            let data = self.get_data_datums(&channel).await;
            let inventory = supply::decode_purse_inventory(&data, head)?;
            let matches = inventory
                .stacks
                .into_iter()
                .filter(|stack| {
                    stack.instance_id == birth.stack_id
                        && stack.source_hash == birth.produce_hash
                        && stack.stack.cells == birth.cells
                })
                .collect::<Vec<_>>();
            let [stack] = matches.as_slice() else {
                return Err(CasperError::InvalidCostSettlement(
                    "authority born stack is absent or differs from its witness".to_string(),
                ));
            };
            resolved.push(stack.clone());
        }
        Ok(resolved)
    }

    /// Legacy single-signature variant. Thin wrapper around
    /// [`Self::process_deploy_with_mergeable_data_cosigned`].
    pub async fn process_deploy_with_mergeable_data(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy)
            .map_err(|e| {
                CasperError::RuntimeError(format!(
                    "legacy uplift to Cosigned failed in process_deploy_with_mergeable_data: {e}"
                ))
            })?;
        self.process_deploy_with_mergeable_data_cosigned(cosigned)
            .await
    }

    pub async fn process_deploy_with_mergeable_data_cosigned(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let (pd, merge_chs) = self.process_deploy_cosigned(cosigned).await?;
        let data = self.get_number_channels_data(&merge_chs).await?;
        Ok((pd, data))
    }

    pub async fn get_number_channels_data(
        &self,
        channels: &std::collections::HashMap<
            Par,
            rspace_plus_plus::rspace::merger::merging_logic::MergeType,
        >,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let mut result = BTreeMap::new();
        for (channel, merge_type) in channels {
            if let Some((hash, value)) = self.get_number_channel(channel, *merge_type).await? {
                result.insert(hash, (value, *merge_type));
            }
        }
        Ok(result)
    }

    pub fn fold_bitmask_or(values: &[i64]) -> Option<i64> {
        if values.is_empty() {
            return None;
        }
        Some(
            values
                .iter()
                .fold(0i64, |acc, v| ((acc as u64) | (*v as u64)) as i64),
        )
    }

    pub async fn get_number_channel(
        &self,
        channel: &Par,
        merge_type: MergeType,
    ) -> Result<Option<(Blake2b256Hash, i64)>, CasperError> {
        let ch_values = self.runtime.get_data(channel).await;

        if ch_values.is_empty() {
            Ok(None)
        } else {
            let ch_hash = stable_hash_provider::hash(channel);
            if ch_values.len() != 1 {
                let nums: Vec<i64> = ch_values
                    .iter()
                    .filter_map(|datum| {
                        RholangMergingLogic::try_get_number_with_rnd(&datum.a).map(|(n, _)| n)
                    })
                    .collect();

                match merge_type {
                    MergeType::IntegerAdd => {
                        return Err(CasperError::RuntimeError(format!(
                            "number channel {} holds {} values {:?}; IntegerAdd single-value invariant violated",
                            hex::encode(ch_hash.bytes()),
                            ch_values.len(),
                            nums,
                        )));
                    }
                    MergeType::BitmaskOr => {
                        let num = match Self::fold_bitmask_or(&nums) {
                            Some(n) => n,
                            None => return Ok(None),
                        };
                        return Ok(Some((ch_hash, num)));
                    }
                }
            }

            // Single value: opportunistic numeric read. Non-numeric values
            // (e.g., TreeHashMap leaf Maps tagged with the bitmask tag) are
            // skipped here and fall through to the existing conflict path.
            let num_par = &ch_values[0].a;
            match RholangMergingLogic::try_get_number_with_rnd(num_par) {
                Some((num, _)) => Ok(Some((ch_hash, num))),
                None => Ok(None),
            }
        }
    }

    /* System deploy evaluators */

    /**
     * Evaluates System deploy with checkpoint to get final state hash
     */
    pub async fn play_system_deploy<S: SystemDeployTrait>(
        &mut self,
        state_hash: &StateHash,
        system_deploy: &mut S,
    ) -> Result<SystemDeployResult<S::Result>, CasperError> {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
            .await?;

        let (event_log, result, mergeable_channels) =
            self.play_system_deploy_internal(system_deploy).await?;

        match result {
            Either::Right(system_deploy_result) => {
                let final_state_hash = {
                    let checkpoint = self.runtime.create_checkpoint().await;
                    checkpoint.root.to_bytes_prost()
                };
                let mcl = self.get_number_channels_data(&mergeable_channels).await?;
                if let Some(SlashDeploy {
                    invalid_block_hash,
                    equivocation_block_hash,
                    pk,
                    target_activation_epoch,
                    target_bond_generation,
                    initial_rand: _,
                }) = system_deploy.as_any().downcast_ref::<SlashDeploy>()
                {
                    let system_deploy = if let Some(equivocation_block_hash) =
                        equivocation_block_hash
                    {
                        SystemDeployData::create_equivocation_slash(
                            invalid_block_hash.clone(),
                            equivocation_block_hash.clone(),
                            pk.clone(),
                            *target_activation_epoch,
                            *target_bond_generation,
                        )
                    } else {
                        SystemDeployData::create_slash(
                            invalid_block_hash.clone(),
                            pk.clone(),
                            *target_activation_epoch,
                            *target_bond_generation,
                        )
                    };
                    Ok(SystemDeployResult::play_succeeded(
                        state_hash.clone(),
                        final_state_hash,
                        event_log,
                        system_deploy,
                        mcl,
                        system_deploy_result,
                    ))
                } else if let Some(CloseBlockDeploy { .. }) =
                    system_deploy.as_any().downcast_ref::<CloseBlockDeploy>()
                {
                    Ok(SystemDeployResult::play_succeeded(
                        state_hash.clone(),
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_close(),
                        mcl,
                        system_deploy_result,
                    ))
                } else if let Some(redeem) = system_deploy
                    .as_any()
                    .downcast_ref::<crate::rust::util::rholang::costacc::redeem_deploy::RedeemDeploy>()
                {
                    // Cost-Accounted Rho Stage-C redemption: persist the FULL
                    // authorization material (validator, outcome, multisig
                    // keyset/quorum, cosigner authorizations) so replay re-runs
                    // the DR-12 quorum verification byte-identically to play.
                    use crate::rust::util::rholang::costacc::redeem_deploy::RedemptionOutcome;
                    let (outcome_tag, penalty) = match &redeem.outcome {
                        RedemptionOutcome::Vindicated => ("Vindicated".to_string(), 0_i64),
                        RedemptionOutcome::Guilty { penalty } => ("Guilty".to_string(), *penalty),
                        RedemptionOutcome::Burned => ("Burned".to_string(), 0_i64),
                    };
                    let authorizations = redeem
                        .authorizations
                        .iter()
                        .map(|a| models::rust::casper::protocol::casper_message::RedemptionAuthorizationData {
                            public_key: a.public_key.clone().into(),
                            signature: a.signature.clone().into(),
                        })
                        .collect();
                    Ok(SystemDeployResult::play_succeeded(
                        state_hash.clone(),
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_redeem(
                            redeem.validator_pk.clone().into(),
                            redeem.target_bond_generation,
                            outcome_tag,
                            penalty,
                            redeem.pos_multi_sig_public_keys.clone(),
                            redeem.pos_multi_sig_quorum,
                            authorizations,
                        ),
                        mcl,
                        system_deploy_result,
                    ))
                } else {
                    Ok(SystemDeployResult::play_succeeded(
                        state_hash.clone(),
                        final_state_hash,
                        event_log,
                        SystemDeployData::Empty,
                        mcl,
                        system_deploy_result,
                    ))
                }
            }

            Either::Left(usr_err) => {
                self.runtime
                    .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
                    .await?;
                Ok(SystemDeployResult::play_failed(event_log, usr_err))
            }
        }
    }

    pub async fn play_system_deploy_internal<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<
        (
            Vec<Event>,
            Either<SystemDeployUserError, S::Result>,
            HashMap<Par, MergeType>,
        ),
        CasperError,
    > {
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        // Get System deploy result / throw fatal errors for unexpected results
        let (result_or_system_deploy_error, eval_result) =
            self.eval_system_deploy(system_deploy).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_eval_system_deploy", rss_kb);
        }

        let log = self.runtime.take_event_log().await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_take_event_log", rss_kb);
        }
        let log = log
            .into_iter()
            .map(event_converter::to_casper_event)
            .collect::<Vec<_>>();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_convert_event_log", rss_kb);
        }

        Ok((log, result_or_system_deploy_error, eval_result.mergeable))
    }

    /**
     * Evaluates System deploy (applicative errors are fatal)
     */
    pub async fn eval_system_deploy<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<SysEvalResult<S>, CasperError> {
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", kind = system_deploy_kind(system_deploy), "eval_system_deploy ENTER (eval system source, then consume its result)");
        let wrapper_pre_start = Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        let wrapper_pre = wrapper_pre_start.elapsed();
        let eval_result = self.evaluate_system_source(system_deploy).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_evaluate_system_source", rss_kb);
        }

        let wrapper_mid_start = Instant::now();
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", n_eval_errors = eval_result.errors.len(), "eval_system_deploy: system source evaluated");
        if !eval_result.errors.is_empty() {
            tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", "eval_system_deploy: UnexpectedSystemErrors (system deploy eval ERRORED)");
            return Err(CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::UnexpectedSystemErrors(eval_result.errors),
            ));
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_error_check", rss_kb);
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_consume_system_result", rss_kb);
        }
        let wrapper_mid = wrapper_mid_start.elapsed();
        let consumed = self.consume_system_result(system_deploy).await?;
        let wrapper_post_start = Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_consume_system_result", rss_kb);
        }
        let r = match consumed {
            Some((_, vec_list)) => match vec_list.as_slice() {
                [ListParWithRandom { pars, .. }] if pars.len() == 1 => {
                    let extracted = system_deploy.extract_result(&pars[0]);
                    if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb()
                    {
                        tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_extract_result", rss_kb);
                    }
                    Ok(extracted)
                }
                _ => Err(CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::UnexpectedResult(
                        vec_list.iter().flat_map(|lp| lp.pars.clone()).collect(),
                    ),
                )),
            },
            None => Err(CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::ConsumeFailed,
            )),
        }?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_match_result", rss_kb);
        }
        metrics::counter!(EVAL_SYSTEM_DEPLOY_WRAPPER_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(EVAL_SYSTEM_DEPLOY_WRAPPER_TIME_NS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(
                (wrapper_pre + wrapper_mid + wrapper_post_start.elapsed()).as_nanos() as u64,
            );

        Ok((r, eval_result))
    }

    /**
     * Evaluates exploratory (read-only) deploy
     */
    pub async fn play_exploratory_deploy_with_phlo_limit(
        &mut self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
        phlo_limit: i64,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let data = DeployData {
            term,
            language: "rholang".to_string(),
            time_stamp: 0,
            valid_after_block_number: 0,
            shard_id: String::new(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let (ephemeral_sk, ephemeral_pk) = exploratory_key_pair().clone();
        let deploy = Signed::create_unbound(
            data,
            deployer.unwrap_or(ephemeral_pk),
            ephemeral_sk,
            Box::new(Secp256k1),
        )?;
        let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy)
            .map_err(|error| {
                CasperError::RuntimeError(format!(
                    "exploratory deploy uplift to Cosigned failed: {error}"
                ))
            })?;
        let eval_res = self
            .evaluate_cosigned_with_budget(
                &cosigned,
                Cost::create(phlo_limit.max(0), "exploratory deploy limit"),
            )
            .await?;
        if !eval_res.errors.is_empty() {
            return Err(CasperError::InterpreterError(eval_res.errors[0].clone()));
        }
        let cost = eval_res.cost.value.max(0) as u64;
        Ok((self.get_data_par(&return_name).await, cost))
    }

    pub async fn play_exploratory_deploy_v61_with_phlo_limit(
        &mut self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
        shard_id: String,
        phlo_limit: i64,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let data = DeployData {
            term,
            language: "rholang".to_string(),
            time_stamp: 0,
            valid_after_block_number: 0,
            shard_id,
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let (_, ephemeral_pk) = exploratory_key_pair().clone();
        let public_key = deployer.unwrap_or(ephemeral_pk);
        let signer = Cosigner {
            pk: public_key.clone(),
            sig: Bytes::new(),
            sig_algorithm: Box::new(Secp256k1),
        };
        let commitment =
            Cosigned::<DeployData>::envelope_commitment_for_presence(&data, &[signer], 1, &[1])
                .map_err(|error| {
                    CasperError::RuntimeError(format!(
                        "protocol-v6 exploratory identity construction failed: {error}"
                    ))
                })?;
        let deploy_id: [u8; 32] = commitment
            .as_ref()
            .try_into()
            .expect("protocol-v6 deploy identity width");
        let mut seed = b"f1r3node:user-deploy-unforgeable:v6".to_vec();
        seed.extend_from_slice(&deploy_id);
        let mut return_rand = Tools::rng(&seed);
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: return_rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);

        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        self.runtime
            .set_deploy_data(SystemProcessDeployData {
                timestamp: data.time_stamp,
                authority: DeployAuthority::Principal(GPrincipalId {
                    key_family: 1,
                    public_key: public_key.bytes.to_vec(),
                }),
                deploy_id: deploy_id.to_vec(),
            })
            .await;
        self.runtime.cost.set_unmetered(false);
        self.runtime.cost.set_deploy_id_funded(
            deploy_id,
            accounting::funding_sig_single(&accounting::principal_ground_v61(&public_key.bytes)),
        );
        let normalizer_env = models::rust::normalizer_env::normalizer_env_from_v61_single_signer(
            &deploy_id,
            &public_key,
        );
        let eval_res = self
            .runtime
            .evaluate_with_authority(
                &data.term,
                Cost::create(phlo_limit.max(0), "exploratory deploy limit"),
                normalizer_env,
                Tools::rng(&seed),
                None,
            )
            .await
            .map_err(CasperError::InterpreterError)?;
        if !eval_res.errors.is_empty() {
            return Err(CasperError::InterpreterError(eval_res.errors[0].clone()));
        }
        let cost = eval_res.cost.value.max(0) as u64;
        Ok((self.get_data_par(&return_name).await, cost))
    }

    pub async fn play_exploratory_deploy(
        &mut self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let deploy_result = async {
            // D3: a deploy carries no phlo price/limit — exploratory execution
            // is metered by the in-calculus cost accounting, not a deploy field.
            let data = DeployData {
                term,
                language: "rholang".to_string(),
                time_stamp: 0,
                valid_after_block_number: 0,
                shard_id: String::new(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            };

            let (ephemeral_sk, ephemeral_pk) = exploratory_key_pair().clone();
            let deploy = Signed::create_unbound(
                data,
                deployer.unwrap_or(ephemeral_pk),
                ephemeral_sk,
                Box::new(Secp256k1),
            )?;

            // Create return channel as first private name created in deploy term
            let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
            let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: rand.next().into_iter().map(|b| b as u8).collect(),
                })),
            }]);

            // Execute deploy on top of specified block hash
            self.capture_results_with_name(hash, &deploy, &return_name)
                .await
        };

        deploy_result.await
    }

    /// Lenient exploratory query: a runtime execution failure degrades to an
    /// empty result (logged, not propagated). Appropriate for display/API
    /// reads (bonds, active validators) — NEVER for consensus-level reads,
    /// where "failed" and "absent" must stay distinguishable
    /// (see [`Self::play_exploratory_par_strict`]).
    pub async fn play_exploratory_par(
        &mut self,
        par: Par,
        hash: &StateHash,
    ) -> Result<Vec<Par>, CasperError> {
        self.play_exploratory_par_with_mode(par, hash, false).await
    }

    /// Strict variant: a runtime injection failure PROPAGATES as an error
    /// instead of degrading to an empty result. Required for consensus-level
    /// reads (the protocol fault-tolerance threshold) where "query failed"
    /// must never be conflated with "value genuinely absent" — the lenient
    /// empty-result degradation would silently route a transient execution
    /// failure into the local-config fallback and re-open node-local
    /// divergence.
    pub async fn play_exploratory_par_strict(
        &mut self,
        par: Par,
        hash: &StateHash,
    ) -> Result<Vec<Par>, CasperError> {
        self.play_exploratory_par_with_mode(par, hash, true).await
    }

    pub async fn play_query_par_current_strict(&self, par: Par) -> Result<Vec<Par>, CasperError> {
        let mut runtime = self.runtime.clone();
        let fallback = runtime.create_soft_checkpoint().await;
        let rand = Blake2b512Random::create_from_bytes(&[0u8; 128]);
        let mut return_rand = rand.clone();
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: return_rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);
        let result = {
            let _unmetered_scope = runtime.cost.enter_unmetered_scope();
            match runtime.inj(par, Env::new(), rand).await {
                Ok(()) => Ok(RuntimeOps::new(runtime.clone())
                    .get_data_par(&return_name)
                    .await),
                Err(error) => Err(CasperError::RuntimeError(format!(
                    "current-state query execution failed: {error:?}"
                ))),
            }
        };
        runtime.revert_to_soft_checkpoint(fallback).await;
        result
    }

    async fn play_exploratory_par_with_mode(
        &mut self,
        par: Par,
        hash: &StateHash,
        strict: bool,
    ) -> Result<Vec<Par>, CasperError> {
        use crate::rust::metrics_constants::{
            BONDS_CACHE_GET_DATA_TIME_METRIC, BONDS_CACHE_INJ_TIME_METRIC,
            BONDS_CACHE_RESET_TIME_METRIC, CASPER_METRICS_SOURCE,
        };
        let __reset_start = std::time::Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_reset", rss_kb);
        }
        self.runtime.cost().set(Cost::unsafe_max());
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_set_cost", rss_kb);
        }
        metrics::histogram!(BONDS_CACHE_RESET_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(__reset_start.elapsed().as_secs_f64());

        let rand = Blake2b512Random::create_from_bytes(&[0u8; 128]);
        let mut return_rand = rand.clone();
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: return_rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_build_return_name", rss_kb);
        }

        let __inj_start = std::time::Instant::now();
        let result = match self.runtime.inj(par, Env::new(), rand).await {
            Ok(()) => {
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_inj_ok", rss_kb);
                }
                metrics::histogram!(BONDS_CACHE_INJ_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__inj_start.elapsed().as_secs_f64());
                let __get_data_start = std::time::Instant::now();
                let data = self.get_data_par(&return_name).await;
                metrics::histogram!(BONDS_CACHE_GET_DATA_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__get_data_start.elapsed().as_secs_f64());
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_get_data_par", rss_kb);
                }
                Ok(data)
            }
            Err(err) => {
                metrics::histogram!(BONDS_CACHE_INJ_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__inj_start.elapsed().as_secs_f64());
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_inj_err", rss_kb);
                }
                tracing::error!(error = ?err, strict, "play_exploratory_par failed");
                if strict {
                    Err(CasperError::RuntimeError(format!(
                        "exploratory query execution failed (strict mode): {:?}",
                        err
                    )))
                } else {
                    Ok(Vec::new())
                }
            }
        };

        let _ = self.runtime.take_event_log().await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_take_event_log", rss_kb);
        }
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_post_query_reset", rss_kb);
        }

        result
    }

    /* Checkpoints */

    /**
     * Creates soft checkpoint with rollback if result is false.
     */
    pub async fn with_soft_transaction<A, F, Fut>(&mut self, action: F) -> Result<A, CasperError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(A, bool), CasperError>>,
    {
        let fallback = self.runtime.create_soft_checkpoint().await;

        match action().await {
            Ok((value, true)) => Ok(value),
            Ok((value, false)) => {
                self.runtime.revert_to_soft_checkpoint(fallback).await;
                Ok(value)
            }
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(fallback).await;
                Err(error)
            }
        }
    }

    /* Evaluates and captures results */

    // Return channel on which result is captured is the first name
    // in the deploy term `new return in { return!(42) }`
    pub async fn capture_results(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
    ) -> Result<Vec<Par>, CasperError> {
        // Create return channel as first unforgeable name created in deploy term
        let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);

        let (data, _token_cost) = self
            .capture_results_with_name(start, deploy, &return_name)
            .await?;
        Ok(data)
    }

    pub async fn capture_results_with_name(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        name: &Par,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        self.capture_results_with_errors(start, deploy, name).await
    }

    pub async fn capture_results_with_errors(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        name: &Par,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start))
            .await?;

        let eval_res = self.evaluate(deploy).await?;
        if !eval_res.errors.is_empty() {
            return Err(CasperError::InterpreterError(eval_res.errors[0].clone()));
        }

        let cost = eval_res.cost.value.max(0) as u64;
        Ok((self.get_data_par(name).await, cost))
    }

    /* Evaluates Rholang source code */

    /// Legacy single-signature evaluate. Preserves byte-identical
    /// observable behavior for existing on-chain deploys (same `deploy_id`,
    /// same `Sig::Quote` value, same normalizer env). Multi-signature
    /// dispatch happens in [`Self::evaluate_cosigned`] which this
    /// method delegates to via legacy uplift.
    pub async fn evaluate(
        &mut self,
        deploy: &Signed<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        let cosigned =
            crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy.clone())
                .map_err(|e| {
                    CasperError::RuntimeError(format!(
                        "legacy uplift to Cosigned failed in evaluate: {e}"
                    ))
                })?;
        self.evaluate_cosigned(&cosigned).await
    }

    pub(crate) async fn evaluate_genesis(
        &mut self,
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        self.evaluate_cosigned_with_budget_and_authority_mode(
            cosigned,
            Cost::unsafe_max(),
            None,
            DefaultCostAuthority::Unit,
        )
        .await
    }

    /// Multi-signature aware deploy evaluation. Single source of truth for
    /// the signature install + normalizer-env construction logic.
    ///
    /// Single-sig deploys (`!cosigned.is_compound()`) route through the
    /// legacy `set_deploy_signature` (legacy `DEPLOY_SIGNATURE_DOMAIN`) so
    /// existing on-chain deploy_ids are preserved bit-for-bit. Multi-sig
    /// deploys route through `set_deploy_signatures` (compound domain
    /// separator) folding all signers into a left-associated `Sig::And` tree.
    ///
    /// The normalizer env is built via `normalizer_env_from_cosigned_deploy`
    /// in both cases — for single-sig that produces a one-element
    /// `rho:system:cosigners` list, observably equivalent to the legacy
    /// `normalizer_env_from_deploy(signed)` output (Cosigned uplift
    /// equivalence verified by
    /// `cosigned_envelope_legacy_uplift_yields_single_element_cosigners`).
    pub async fn evaluate_cosigned(
        &mut self,
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        self.evaluate_cosigned_with_budget(cosigned, Cost::unsafe_max())
            .await
    }

    pub(crate) async fn evaluate_cosigned_with_budget(
        &mut self,
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
    ) -> Result<EvaluateResult, CasperError> {
        self.evaluate_cosigned_with_budget_and_authority(cosigned, budget, None)
            .await
    }

    pub(crate) async fn evaluate_cosigned_with_budget_and_authority(
        &mut self,
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
    ) -> Result<EvaluateResult, CasperError> {
        self.evaluate_cosigned_with_budget_and_authority_mode(
            cosigned,
            budget,
            authority_allocation,
            DefaultCostAuthority::Funders,
        )
        .await
    }

    async fn evaluate_cosigned_with_budget_and_authority_mode(
        &mut self,
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
        default_authority: DefaultCostAuthority,
    ) -> Result<EvaluateResult, CasperError> {
        let deploy_data = SystemProcessDeployData::from_cosigned(cosigned);
        self.runtime.set_deploy_data(deploy_data).await;
        self.runtime.cost.set_unmetered(false);

        // Decouple the wire-signature deploy identity from the funding
        // authority: verified signer public keys select canonical SystemVault
        // payers, while nested signed regions and located stacks refine the
        // authority during reduction. `funding_sig` is the shared derivation
        // used by admission and replay and excludes unsigned threshold
        // placeholders.
        match default_authority {
            DefaultCostAuthority::Funders => {
                let funding = accounting::funding_sig(cosigned);
                if cosigned.is_envelope_bound() {
                    let deploy_id: [u8; 32] = cosigned
                        .envelope_commitment()
                        .expect("validated protocol-v6 envelope identity")
                        .as_ref()
                        .try_into()
                        .expect("protocol-v6 deploy identity width");
                    self.runtime.cost.set_deploy_id_funded(deploy_id, funding);
                } else if cosigned.is_compound() {
                    let sigs: Vec<&[u8]> =
                        cosigned.signers().iter().map(|s| s.sig.as_ref()).collect();
                    self.runtime
                        .cost
                        .set_deploy_signatures_funded(&sigs, funding);
                } else {
                    self.runtime
                        .cost
                        .set_deploy_signature_funded(&cosigned.primary().sig, funding);
                }
            }
            DefaultCostAuthority::Unit => self.runtime.cost.reset_for_system_deploy(),
        }

        // Production bounded play and replay pass the same finite
        // authority-derived capacity here. The unbounded default remains only
        // for non-consensus exploratory and system-facing callers that do not
        // produce an admitted user-deploy certificate.
        let normalizer_env =
            models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(cosigned);
        let initial_rand = Tools::user_deploy_rng(cosigned);
        let result = self
            .runtime
            .evaluate_with_authority(
                &cosigned.data.term,
                budget,
                normalizer_env,
                initial_rand,
                authority_allocation,
            )
            .await;

        match result {
            Ok(eval_result) => Ok(eval_result),
            Err(e) => Err(CasperError::InterpreterError(e)),
        }
    }

    pub async fn evaluate_system_source<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<EvaluateResult, CasperError> {
        self.runtime.cost.reset_for_system_deploy();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        // Using tracing events for async - Span[F].traceI("evaluate-system-source") from Scala
        tracing::debug!(target: "f1r3fly.casper.evaluate_system_source", "evaluate-system-source-started");
        let eval_start = Instant::now();
        let wrapper_pre_start = eval_start;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_build_env", rss_kb);
        }
        let env = system_deploy.env();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_build_env", rss_kb);
        }
        let rand = system_deploy.rand().clone();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_clone_rand", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_runtime_evaluate", rss_kb);
        }
        let wrapper_pre = wrapper_pre_start.elapsed();
        let result = {
            // System deploys perform protocol maintenance and settlement work
            // outside user-runtime metering. The scoped guard is deliberately
            // used here so panics, early returns, and async errors cannot leak
            // unmetered mode into the next user deploy.
            let _unmetered_scope = self.runtime.cost.enter_unmetered_scope();
            self.runtime
                .evaluate(
                    S::source(),
                    Cost::unsafe_max(),
                    env,
                    // `evaluate` owns the random seed state for this run, so the
                    // cloned deploy seed is passed by value with the rest of the
                    // immutable system-deploy inputs.
                    rand,
                )
                .await
        };
        let result = result?;
        let wrapper_post_start = Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_runtime_evaluate", rss_kb);
        }
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_EVALUATE_SOURCE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(eval_start.elapsed().as_secs_f64());
        metrics::counter!(EVALUATE_SOURCE_WRAPPER_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(EVALUATE_SOURCE_WRAPPER_TIME_NS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment((wrapper_pre + wrapper_post_start.elapsed()).as_nanos() as u64);
        Ok(result)
    }

    pub async fn get_data_par(&self, channel: &Par) -> Vec<Par> {
        self.runtime
            .get_data(channel)
            .await
            .into_iter()
            .flat_map(|datum| datum.a.pars)
            .collect()
    }

    pub async fn get_data_datums(
        &self,
        channel: &Par,
    ) -> Vec<rspace_plus_plus::rspace::internal::Datum<ListParWithRandom>> {
        self.runtime.get_data(channel).await
    }

    pub async fn get_continuation_par(&self, channels: Vec<Par>) -> Vec<(Vec<BindPattern>, Par)> {
        self.runtime
            .get_continuations(channels)
            .await
            .into_iter()
            .filter_map(|wk| {
                if let Some(TaggedCont::ParBody(par_body)) = wk.continuation.tagged_cont {
                    Some((wk.patterns, par_body.body.unwrap()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn consume_result(
        &mut self,
        channel: Par,
        pattern: BindPattern,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, CasperError> {
        Ok(self
            .runtime
            .consume_result(vec![channel], vec![pattern])
            .await?)
    }

    pub async fn consume_system_result<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, CasperError> {
        let consume_start = Instant::now();
        let return_channel = system_deploy.return_channel()?;
        let result = self
            .consume_result(return_channel, system_deploy_consume_all_pattern())
            .await;
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_CONSUME_RESULT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(consume_start.elapsed().as_secs_f64());
        result
    }

    /* Read only Rholang evaluator helpers */

    pub async fn get_active_validators(
        &mut self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        let validators_pars = self
            .play_exploratory_par(Self::activate_validator_query_par().clone(), start_hash)
            .await?;

        if validators_pars.is_empty() {
            tracing::warn!(
                "No result from getActiveValidators query for state {}; treating as no active validators",
                PrettyPrinter::build_string_bytes(start_hash)
            );
            return Ok(Vec::new());
        }

        if validators_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from query of current bonds in state {}: {}",
                PrettyPrinter::build_string_bytes(start_hash),
                validators_pars.len()
            )));
        }

        let validators = Self::to_validator_vec(validators_pars[0].to_owned())?;
        let vlds: Vec<String> = validators.iter().map(|v| hex::encode(v)).collect();
        tracing::info!(
            "*** ACTIVE VALIDATORS FOR StateHash {}: {}",
            hex::encode(start_hash),
            vlds.join("\n")
        );

        Ok(validators)
    }

    pub async fn compute_bonds(&mut self, hash: &StateHash) -> Result<Vec<Bond>, CasperError> {
        let bonds_pars = self
            .play_exploratory_par(Self::bonds_query_par().clone(), hash)
            .await?;

        if bonds_pars.is_empty() {
            tracing::warn!(
                "No result from getBonds query for state {}; treating as empty bonds",
                PrettyPrinter::build_string_bytes(hash)
            );
            return Ok(Vec::new());
        }

        if bonds_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from query of current bonds in state {}: {}",
                PrettyPrinter::build_string_bytes(hash),
                bonds_pars.len()
            )));
        }

        Self::to_bond_vec(bonds_pars[0].to_owned())
    }

    pub async fn compute_bond_generations(
        &mut self,
        hash: &StateHash,
    ) -> Result<HashMap<Validator, i64>, CasperError> {
        let generation_pars = self
            .play_exploratory_par(Self::bond_generations_query_par().clone(), hash)
            .await?;

        if generation_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of bond-generation results for state {}: {}",
                PrettyPrinter::build_string_bytes(hash),
                generation_pars.len()
            )));
        }

        Self::to_bond_generation_map(generation_pars[0].to_owned())
    }

    fn activate_validator_query_source() -> String {
        r#"
          new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getActiveValidators", *return)
          }
        }
      "#
        .to_string()
    }

    /// Reads the protocol fault-tolerance threshold (parts-per-million) from
    /// the PoS contract at `start_hash`. Returns `None` when the contract does
    /// not expose the getter (a chain whose genesis predates the parameter) —
    /// the caller falls back to its local configuration in that case.
    pub async fn get_fault_tolerance_threshold_ppm(
        &mut self,
        start_hash: &StateHash,
    ) -> Result<Option<i64>, CasperError> {
        // STRICT query: a runtime execution failure must PROPAGATE (failing
        // node startup) rather than degrade to an empty result — the lenient
        // path's `Ok(vec![])`-on-error would be indistinguishable from "the
        // getter does not exist" and silently route a transient failure into
        // the local-config fallback, re-opening node-local floor divergence.
        // `None` is returned only after a SUCCESSFUL query with no result.
        let ppm_pars = self
            .play_exploratory_par_strict(Self::fault_tolerance_ppm_query_par().clone(), start_hash)
            .await?;

        if ppm_pars.is_empty() {
            tracing::warn!(
                "No result from getFaultToleranceThresholdPpm query for state {}; \
                 genesis predates the on-chain protocol FTT — falling back to local config",
                PrettyPrinter::build_string_bytes(start_hash)
            );
            return Ok(None);
        }
        if ppm_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from getFaultToleranceThresholdPpm query in state {}: {}",
                PrettyPrinter::build_string_bytes(start_hash),
                ppm_pars.len()
            )));
        }

        let par = &ppm_pars[0];
        match par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::GInt(ppm)) => {
                // RANGE GATE (θ = ppm/1e6 ∈ [-1, 1]). This is the guard that
                // discharges the `-den <= num <= den` hypothesis of the Rocq
                // `FtExact.ft_exact_no_overflow` / `ft_decides_exact` decision
                // `2q·den ⋛ S·(den+num)`. It is NOT decorative:
                //   * ppm < -1e6 ⇒ den+num < 0 ⇒ rhs < 0 <= lhs ⇒ the oracle
                //     returns true for ANY q, bypassing the fault-tolerance
                //     threshold shard-wide;
                //   * ppm > 1e6 ⇒ rhs > 2·S·den >= lhs ⇒ nothing ever
                //     finalizes (liveness halt).
                // `ft_decides_exact`'s `debug_assert!` cannot be relied on: it
                // compiles out in release, which is how CI runs. Reject at the
                // single read choke point so no caller can observe an
                // out-of-range protocol threshold.
                if !(-1_000_000..=1_000_000).contains(ppm) {
                    return Err(CasperError::RuntimeError(format!(
                        "on-chain fault-tolerance-threshold ppm out of range [-1000000, 1000000] \
                         in state {}: {}",
                        PrettyPrinter::build_string_bytes(start_hash),
                        ppm
                    )));
                }
                Ok(Some(*ppm))
            }
            other => Err(CasperError::RuntimeError(format!(
                "getFaultToleranceThresholdPpm returned a non-integer value in state {}: {:?}",
                PrettyPrinter::build_string_bytes(start_hash),
                other
            ))),
        }
    }

    fn fault_tolerance_ppm_query_source() -> String {
        r#"
          new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getFaultToleranceThresholdPpm", *return)
          }
        }
      "#
        .to_string()
    }

    fn fault_tolerance_ppm_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::fault_tolerance_ppm_query_source())
                .expect("Failed to compile fault tolerance ppm query source")
        })
    }

    fn activate_validator_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::activate_validator_query_source())
                .expect("Failed to compile active validator query source")
        })
    }

    fn bonds_query_source() -> String {
        r#"
        new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getBonds", *return)
          }
        }
      "#
        .to_string()
    }

    fn bonds_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::bonds_query_source())
                .expect("Failed to compile bonds query source")
        })
    }

    fn bond_generations_query_source() -> String {
        r#"
        new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getBondGenerations", *return)
          }
        }
      "#
        .to_string()
    }

    fn bond_generations_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::bond_generations_query_source())
                .expect("Failed to compile bond-generations query source")
        })
    }

    fn to_validator_vec(validators_par: Par) -> Result<Vec<Validator>, CasperError> {
        if validators_par.exprs.is_empty() {
            return Ok(Vec::new());
        }

        let ps = match validators_par.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::ESetBody(set) => ParSetTypeMapper::eset_to_par_set(set.clone()).ps,
            _ => SortedParHashSet::create_from_empty(),
        };

        ps.map_iter(|v| {
            if v.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Validator in bonds map wasn't a single string.".to_string(),
                ))
            } else {
                match v.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GByteArray(g_byte_array) => Ok(g_byte_array.clone().into()),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GByteArray in validator data".to_string(),
                    )),
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()
    }

    fn to_bond_vec(bonds_map: Par) -> Result<Vec<Bond>, CasperError> {
        if bonds_map.exprs.is_empty() {
            return Ok(Vec::new());
        }

        let ps = match bonds_map.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::EMapBody(map) => ParMapTypeMapper::emap_to_par_map(map.clone()).ps,
            _ => SortedParMap::create_from_empty(),
        };

        ps.map_iter(|(validator, bond)| {
            if validator.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Validator in bonds map wasn't a single string.".to_string(),
                ))
            } else if bond.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Stake in bonds map wasn't a single string.".to_string(),
                ))
            } else {
                let validator_name = match validator.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GByteArray(g_byte_array) => Ok(g_byte_array.clone().into()),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GByteArray in validator data".to_string(),
                    )),
                }?;

                let stake_amount = match bond.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GInt(g_int) => Ok(*g_int),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GInt in stake data".to_string(),
                    )),
                }?;

                Ok(Bond {
                    validator: validator_name,
                    stake: stake_amount,
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()
    }

    fn to_bond_generation_map(
        generations_map: Par,
    ) -> Result<HashMap<Validator, i64>, CasperError> {
        if generations_map.exprs.is_empty() {
            return Ok(HashMap::new());
        }

        let ps = match generations_map.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::EMapBody(map) => ParMapTypeMapper::emap_to_par_map(map.clone()).ps,
            _ => SortedParMap::create_from_empty(),
        };

        ps.map_iter(|(validator, generation)| {
            if validator.exprs.len() != 1 || generation.exprs.len() != 1 {
                return Err(CasperError::RuntimeError(
                    "Malformed bond-generation map entry".to_string(),
                ));
            }
            let validator = match validator.exprs[0].expr_instance.as_ref().unwrap() {
                ExprInstance::GByteArray(value) => value.clone().into(),
                _ => {
                    return Err(CasperError::RuntimeError(
                        "Expected GByteArray in bond-generation validator".to_string(),
                    ))
                }
            };
            let generation = match generation.exprs[0].expr_instance.as_ref().unwrap() {
                ExprInstance::GInt(value) if *value >= 0 => *value,
                _ => {
                    return Err(CasperError::RuntimeError(
                        "Expected nonnegative GInt in bond-generation value".to_string(),
                    ))
                }
            };
            Ok((validator, generation))
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_bitmask_or_empty_returns_none() {
        assert_eq!(RuntimeOps::fold_bitmask_or(&[]), None);
    }

    #[test]
    fn fold_bitmask_or_single_returns_value() {
        assert_eq!(RuntimeOps::fold_bitmask_or(&[42]), Some(42));
    }

    #[test]
    fn fold_bitmask_or_returns_or_fold_not_max() {
        let a = 0b00010001i64;
        let b = 0b00100010i64;
        assert_eq!(RuntimeOps::fold_bitmask_or(&[a, b]), Some(0b00110011));
        let c = 0b01000000i64;
        assert_eq!(RuntimeOps::fold_bitmask_or(&[a, b, c]), Some(0b01110011));
    }

    #[test]
    fn fold_bitmask_or_commutes() {
        let xs = [0b0001_0001i64, 0b0010_0010, 0b0100_0100, 0b1000_1000];
        let mut ys = xs;
        ys.reverse();
        assert_eq!(
            RuntimeOps::fold_bitmask_or(&xs),
            RuntimeOps::fold_bitmask_or(&ys),
        );
    }

    #[test]
    fn fold_bitmask_or_negative_high_bits_preserved() {
        let neg = i64::MIN;
        let pos = 0b1010i64;
        let folded = RuntimeOps::fold_bitmask_or(&[neg, pos]).unwrap();
        assert_eq!(folded as u64, (neg as u64) | (pos as u64));
        assert_ne!(folded & i64::MIN, 0, "sign bit must remain set");
    }

    fn authority_event(identity: u8) -> AuthorityEvent<[u8; 32]> {
        AuthorityEvent {
            event_id: [identity; 32],
            authority: models::rhoapi::CostAuthority::default(),
            debit: ResourceMultiset::default(),
        }
    }

    #[test]
    fn authority_events_follow_committed_causal_order() {
        let events = vec![authority_event(1), authority_event(2)];
        let ordered = causal_authority_events_from_trace(
            [
                AuthorityTraceItem::Comm([2; 32]),
                AuthorityTraceItem::Comm([1; 32]),
            ],
            &events,
            true,
        )
        .unwrap();

        assert_eq!(ordered[0].event_id, [2; 32]);
        assert_eq!(ordered[1].event_id, [1; 32]);
    }

    #[test]
    fn authority_event_order_requires_an_exact_identity_bijection() {
        let events = vec![authority_event(1), authority_event(2)];

        assert!(causal_authority_events_from_trace(
            [AuthorityTraceItem::Comm([1; 32])],
            &events,
            true,
        )
        .is_err());
        assert!(causal_authority_events_from_trace(
            [
                AuthorityTraceItem::Comm([1; 32]),
                AuthorityTraceItem::Comm([3; 32]),
            ],
            &events,
            true,
        )
        .is_err());
        assert!(causal_authority_events_from_trace(
            [
                AuthorityTraceItem::Comm([1; 32]),
                AuthorityTraceItem::Comm([2; 32]),
            ],
            &[authority_event(1), authority_event(1)],
            true,
        )
        .is_err());
    }

    #[test]
    fn authority_events_select_the_user_subset_from_a_lifecycle_trace() {
        let events = vec![authority_event(1), authority_event(2)];
        let ordered = causal_authority_events_from_trace(
            [
                AuthorityTraceItem::Comm([9; 32]),
                AuthorityTraceItem::Comm([2; 32]),
                AuthorityTraceItem::Comm([8; 32]),
                AuthorityTraceItem::Comm([1; 32]),
            ],
            &events,
            false,
        )
        .unwrap();
        assert_eq!(
            ordered
                .iter()
                .map(|event| event.event_id)
                .collect::<Vec<_>>(),
            vec![[2; 32], [1; 32]]
        );
        assert!(causal_authority_events_from_trace(
            [AuthorityTraceItem::Comm([9; 32])],
            &events,
            false,
        )
        .is_err());
    }

    #[test]
    fn stack_transfer_events_follow_their_produce_before_later_comms() {
        let produce_hash = [7; 32];
        let first = stack_transfer_event_id(&produce_hash, 0);
        let second = stack_transfer_event_id(&produce_hash, 1);
        let comm = [9; 32];
        let events = vec![
            AuthorityEvent {
                event_id: comm,
                authority: models::rhoapi::CostAuthority::default(),
                debit: ResourceMultiset::default(),
            },
            AuthorityEvent {
                event_id: second,
                authority: models::rhoapi::CostAuthority::default(),
                debit: ResourceMultiset::default(),
            },
            AuthorityEvent {
                event_id: first,
                authority: models::rhoapi::CostAuthority::default(),
                debit: ResourceMultiset::default(),
            },
        ];
        let ordered = causal_authority_events_from_trace(
            [
                AuthorityTraceItem::Produce(produce_hash),
                AuthorityTraceItem::Comm(comm),
            ],
            &events,
            true,
        )
        .unwrap();

        assert_eq!(
            ordered
                .iter()
                .map(|event| event.event_id)
                .collect::<Vec<_>>(),
            vec![first, second, comm]
        );
    }

    #[test]
    fn matched_produces_precede_their_comm_in_the_authority_trace() {
        use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
        use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

        let first = Produce::new(
            Blake2b256Hash::new(b"channel-a"),
            Blake2b256Hash::new(b"produce-a"),
            false,
        );
        let second = Produce::new(
            Blake2b256Hash::new(b"channel-b"),
            Blake2b256Hash::new(b"produce-b"),
            false,
        );
        let comm = COMM {
            consume: Consume {
                channel_hashes: vec![Blake2b256Hash::new(b"channel-a")],
                hash: Blake2b256Hash::new(b"consume"),
                persistent: false,
            },
            produces: vec![first.clone(), second.clone()],
            peeks: std::collections::BTreeSet::new(),
            times_repeated: BTreeMap::from([(first.clone(), 1), (second.clone(), 1)]),
        };
        let comm_identity: [u8; 32] = comm.cost_identity().bytes().try_into().unwrap();

        let trace = authority_trace_items(&[RSpaceEvent::Comm(comm)]);
        assert!(matches!(
            trace.as_slice(),
            [
                AuthorityTraceItem::Produce(first_hash),
                AuthorityTraceItem::Produce(second_hash),
                AuthorityTraceItem::Comm(actual_comm)
            ] if first_hash == first.hash.bytes().as_slice()
                && second_hash == second.hash.bytes().as_slice()
                && actual_comm == &comm_identity
        ));
    }
}
