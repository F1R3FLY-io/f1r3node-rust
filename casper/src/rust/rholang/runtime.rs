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
use crypto::rust::signatures::signed::Signed;
use models::casper::{
    CostAuthorityByteEventProto, CostAuthorityEventProto, CostAuthorityResourceProto,
    CostAuthorityWitnessProto,
};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, GPrivate, GUnforgeable, ListParWithRandom, Par, TaggedContinuation,
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
// Slice 30: WAL root computation for per-deploy observability.  In
// slice 30b this hash goes on-chain via a proto extension of
// ProcessedDeploy (hard fork); until then it's logged for operator
// diagnostics.
use rholang::rust::interpreter::io::lock::{DeployScope, LockRegistry};
use rholang::rust::interpreter::io::snapshot::compute_wal_root;
use rholang::rust::interpreter::io::wal::{Wal, WalEntry, WalMark};
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::{bootstrap_registry, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    BlockData, DeployData as SystemProcessDeployData,
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

/// RAII per-deploy resource guard.  Two responsibilities:
///
/// 1. **WAL boundary** (C-30-1 review fix, 2026 slice 30 round 2):
///    captures `Wal::begin_deploy` on construction and guarantees
///    `Wal::take_deploy_entries` runs on **every** exit path (success,
///    `?`-propagated error, panic), preventing per-deploy WAL entries
///    from leaking into the next deploy's slice.
/// 2. **Lock deploy-end sweep** (Phase 8 slice 8a step 5, 2026-08-13):
///    calls `LockRegistry::release_all_for_deploy(&deploy_scope)` on
///    Drop, sweeping every lock the deploy acquired but didn't release
///    via `token!release()` or `File.close`.  Spec §Explicit locks
///    MUST auto-release at deploy end for cross-validator determinism
///    (a MAY leak would diverge the lock table across validators that
///    make different MAY choices).
///
/// # WAL correctness
///
/// The pre-round-2 code drained only on the two `Ok`-return paths;
/// five `?` operators between `begin_deploy` and the drain sites
/// could early-return with WAL entries still in the per-runtime
/// buffer, poisoning the next deploy's `take_deploy_entries(mark)`
/// with entries the previous deploy contributed (Critical finding
/// C-30-1).  This guard closes the gap by making drain-on-drop the
/// default and drain-and-attach the explicit opt-in via
/// `take_and_commit`.
///
/// # Lock-sweep correctness
///
/// The Drop-based sweep symmetrically closes any leaked locks on all
/// three exit paths (success, `?`-propagated error, panic).  The
/// sweep is scope-scoped: it clears only entries whose
/// `RangeEntry.deploy` / `SequentialEntry.deploy` matches this
/// guard's `deploy_scope`, so a prior deploy's locks (if any survived
/// its own sweep — shouldn't happen absent a bug) are unaffected.
/// The `current_scope_cell` is set at construction so concurrent
/// lock-native handlers record the correct scope; cleared on drop.
pub(crate) struct WalDeployScope {
    /// Arc-shared clone of the per-runtime WAL — cheap; shares the
    /// underlying `Arc<Mutex<Vec<WalEntry>>>` so appends and drains
    /// see the same buffer.
    wal: Wal,
    mark: WalMark,
    /// Once `take_and_commit` runs, Drop skips the discard-drain
    /// (the entries are already in the caller's hands).
    committed: bool,
    /// Phase 8 slice 8a step 5: manager-shared range-lock registry
    /// (Arc-cloned).  Drop sweeps every lock acquired under this
    /// deploy's scope via `release_all_for_deploy(&deploy_scope)`
    /// AFTER the WAL discard-drain, so both the WAL contribution
    /// AND any leaked locks are cleaned up atomically at deploy end.
    lock_registry: LockRegistry,
    /// Phase 8 slice 8a step 5: the deploy-derived scope this guard
    /// is responsible for.  Blake2b256(deploy.sig) for user deploys;
    /// a state-hash-derived value for system deploys.  MUST be
    /// non-sentinel (`!= [0; 32]`); the `release_all_for_deploy`
    /// debug_assert guards against sentinel input.
    deploy_scope: DeployScope,
    /// Phase 8 slice 8a step 5: per-runtime "current deploy scope"
    /// cell shared with `FileHandleTable::current_deploy_scope`.
    /// Constructor sets it to `deploy_scope`; Drop clears back to
    /// `[0; 32]` after the sweep.  Handlers read this cell at
    /// acquire time so the LockRegistry entry records the correct
    /// scope for the eventual sweep.
    current_scope_cell: std::sync::Arc<std::sync::RwLock<DeployScope>>,
}

impl WalDeployScope {
    // H-2 fix (2026-08-06): now `pub(crate)` so `replay_runtime`
    // can use the same guard.  Pre-fix, replay didn't wrap deploys
    // in a WAL scope — `journal_read`/`journal_write` on the
    // replay branch appended to the follower's per-runtime WAL
    // without draining, causing unbounded growth across a block
    // (hits `MAX_WAL_ENTRIES` only on follower → follower fails
    // deploys the leader accepted) and making per-deploy WAL
    // comparison impossible.
    /// Legacy constructor — used only by pre-step-5 unit tests that
    /// don't exercise the lock-sweep behavior.  Uses a per-instance
    /// throwaway `LockRegistry` and a fresh scope cell, so Drop's
    /// `release_all_for_deploy` is a no-op (the registry is empty).
    /// New code MUST use `new_with_lock_sweep` and pass a real
    /// deploy-derived scope.
    #[cfg(test)]
    pub(crate) fn new(wal: Wal) -> Self {
        // Test-only fallback scope: any non-sentinel value avoids
        // the debug_assert in release_all_for_deploy.  `[0xAA; 32]`
        // is arbitrary but recognizable in test-failure diagnostics.
        Self::new_with_lock_sweep(
            wal,
            LockRegistry::new(),
            [0xAAu8; 32],
            std::sync::Arc::new(std::sync::RwLock::new([0u8; 32])),
        )
    }

    /// Phase 8 slice 8a step 5 — production constructor with lock
    /// sweep.  Sets `current_scope_cell` to `deploy_scope` so
    /// concurrent lock-native handlers record their acquires under
    /// this deploy's scope.  On Drop, sweeps `deploy_scope`'s locks
    /// from the shared `LockRegistry` and clears the cell back to
    /// sentinel.
    ///
    /// `deploy_scope` MUST be non-sentinel (`!= [0; 32]`); a sentinel
    /// scope would trip the `release_all_for_deploy` debug_assert
    /// on drop.  Blake2b256 output always produces a non-sentinel
    /// value under any real input (chance of collision with `[0; 32]`
    /// is 2⁻²⁵⁶ — negligible), so the enforcement is by construction
    /// via how callers derive scopes.
    pub(crate) fn new_with_lock_sweep(
        wal: Wal,
        lock_registry: LockRegistry,
        deploy_scope: DeployScope,
        current_scope_cell: std::sync::Arc<std::sync::RwLock<DeployScope>>,
    ) -> Self {
        // Promoted from debug_assert to assert during step-5 review
        // (2026-08-13) for release-build defense-in-depth: the 3 call
        // sites (process_deploy_cosigned_with_budget_and_authority_mode,
        // play_system_deploy, replay_deploy_e) derive via
        // Blake2b256 → non-sentinel by
        // construction, but a future refactor that introduces a
        // sentinel-scope path would silently pass in release builds
        // and end up sweeping every stray sentinel-scoped entry in
        // Drop.  The one-comparison cost is negligible; keep the
        // guard on for all build modes.
        assert!(
            deploy_scope != [0u8; 32],
            "WalDeployScope::new_with_lock_sweep called with sentinel scope [0; 32]; \
             this would trip the release_all_for_deploy guard on Drop.  Callers \
             must derive a non-sentinel scope (Blake2b256 of deploy identifier)."
        );
        let mark = wal.begin_deploy();
        // Publish this deploy's scope to the shared cell so
        // concurrent-in-this-deploy lock-native calls record it.
        *current_scope_cell
            .write()
            .expect("current_deploy_scope RwLock poisoned") = deploy_scope;
        Self {
            wal,
            mark,
            committed: false,
            lock_registry,
            deploy_scope,
            current_scope_cell,
        }
    }

    /// Success path: drain entries in canonical (deploy_log) order
    /// and mark committed so Drop is a no-op.
    ///
    /// Slice 30c H-R3 integration: instead of returning entries in
    /// insertion order (scheduler-dependent under Par), walk the
    /// deploy's `event_log` Produces to derive a canonical order.
    /// Log order is frozen when the leader publishes the block; all
    /// validators consume the same log verbatim during replay, so
    /// the output is byte-identical across validators and across
    /// re-executions on the same validator regardless of tokio
    /// scheduling.  Resolves H-R3.
    pub(crate) fn take_and_commit(&mut self, deploy_log: &[Event]) -> Vec<WalEntry> {
        self.committed = true;
        let hashes = produce_channel_hashes(deploy_log);
        self.wal
            .take_deploy_entries_in_log_order(self.mark, &hashes)
    }
}

/// Slice 30c H-R3 integration: extract each Produce event's
/// `channels_hash` from a deploy's event log, in log order.  These
/// hashes are the keys the log-order WAL drain matches against the
/// per-entry ack-channel sidecar recorded by `Wal::append_with_ack`.
///
/// ConsumeEvents and CommEvents are skipped — the WAL sidecar is
/// keyed by ack-channel hash, which appears in the Produce that
/// publishes the syscall's reply.
pub(crate) fn produce_channel_hashes(deploy_log: &[Event]) -> Vec<[u8; 32]> {
    let mut out = Vec::with_capacity(deploy_log.len());
    for e in deploy_log {
        if let Event::Produce(pe) = e {
            // `channels_hash` is bytes::Bytes; expected to be exactly
            // 32 bytes (Blake2b256).  Anything else is malformed;
            // skip silently rather than panic — the drain has
            // defense-in-depth to append unmatched entries.
            if pe.channels_hash.len() == 32 {
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&pe.channels_hash);
                out.push(buf);
            }
        }
    }
    out
}

impl Drop for WalDeployScope {
    fn drop(&mut self) {
        if !self.committed {
            // Discard-drain: entries produced during a failed deploy
            // are removed from the per-runtime buffer so the next
            // deploy's `begin_deploy` mark points at an empty tail.
            // Logged at debug level so an incident review can find
            // leaked entries.
            let leaked = self.wal.take_deploy_entries(self.mark);
            if !leaked.is_empty() {
                tracing::debug!(
                    target: "f1r3fly.casper.fs_wal",
                    n_entries = leaked.len(),
                    "fs-wal discard-drain on error path (deploy did not commit its WAL)"
                );
            }
        }
        // Phase 8 slice 8a step 5 + 8b sub-3 + 8b sub-6 review-fix
        // (2026-08-12): cancel THIS deploy's parked wait:true acquires
        // FIRST, then sweep THIS deploy's held locks.
        //
        // ## Ordering rationale (sub-6 review-fix)
        //
        // The prior order (release-first, cancel-second) had a subtle
        // same-deploy leak: `release_all_for_deploy` internally calls
        // `wake_waiters(state)` after removing this deploy's held
        // entries, which admits ANY parked waiter whose range now
        // fits — INCLUDING a waiter whose own `deploy` field matches
        // the dropping scope.  Once admitted, the waiter is promoted
        // to `state.ranges` with `deploy == self.deploy_scope`.  The
        // subsequent `cancel_all_waiters_for_deploy` finds nothing to
        // cancel (waiter is now held, not parked).  The now-held
        // lock's deploy is dead → nothing ever releases it → leaks
        // past deploy boundary forever.
        //
        // Reversed order:
        //   1. cancel_all_waiters_for_deploy(deploy) — kills THIS
        //      deploy's parked waiters (Err(Cancelled) on oneshot);
        //      they can never be admitted.
        //   2. release_all_for_deploy(deploy) — sweeps THIS deploy's
        //      held locks + wake_waiters admits ONLY other-deploy
        //      waiters (this deploy's are gone).
        //
        // Runs on EVERY exit path (success via take_and_commit + Drop,
        // `?`-propagated error via Drop alone, panic-unwind via Drop
        // alone).  Symmetrical with the WAL discard-drain above.
        //
        // Sentinel guards: both `cancel_all_waiters_for_deploy` and
        // `release_all_for_deploy` panic on `[0; 32]`.
        // new_with_lock_sweep's assert! rejects sentinel scopes at
        // construction; legacy test constructor uses `[0xAA; 32]` —
        // also non-sentinel.
        let n_cancelled = self
            .lock_registry
            .cancel_all_waiters_for_deploy(&self.deploy_scope);
        if n_cancelled > 0 {
            tracing::debug!(
                target: "f1r3fly.casper.fs_locks",
                n_waiters = n_cancelled,
                "deploy-end auto-cancel: signalled Cancelled to parked wait:true acquires"
            );
        }
        let n_released = self
            .lock_registry
            .release_all_for_deploy(&self.deploy_scope);
        if n_released > 0 {
            tracing::debug!(
                target: "f1r3fly.casper.fs_locks",
                n_locks = n_released,
                "deploy-end auto-release: swept locks the caller did not release"
            );
        }
        // Clear the per-runtime "current scope" cell back to sentinel
        // so between-deploy handler calls (test-path only under
        // normal operation) see the sentinel value.
        *self
            .current_scope_cell
            .write()
            .expect("current_deploy_scope RwLock poisoned") = [0u8; 32];
    }
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
        terms: Vec<Signed<DeployData>>,
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
                outcome.rejected.push(cosigned.primary().sig.clone());
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
                let (processed, user_mergeable, _fs_wal, exhausted) = self
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
                outcome.rejected.push(cosigned.primary().sig.clone());
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
                    outcome.rejected.push(cosigned.primary().sig.clone());
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
                outcome.rejected.push(cosigned.primary().sig.clone());
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
            let (mut processed, mergeable, _fs_wal, did_exhaust) = self
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
        // Merged flow: cost-accounted's per-deploy checkpoint chain
        // tracking + fileio's per-block WAL aggregation (H-30-2 slice-
        // 30b).  `play_ordinary_deploy` returns each deploy's WAL
        // contribution as the 3rd tuple element; we accumulate them
        // in block order for the post-block WAL root computation,
        // while also chaining each deploy's pre/post state via
        // checkpoints for state-hash continuity (cost-accounted's
        // addition).  The per-deploy WalDeployScope now lives inside
        // `process_deploy_cosigned_with_budget_and_authority_mode`,
        // sharing the same atomic-deploy boundary as the inner
        // soft-checkpoint.
        let mut current_root = start_hash.clone();
        let mut block_fs_wal: Vec<WalEntry> = Vec::new();
        for deploy in terms {
            let (mut pd, mc, fs_wal) = self.play_ordinary_deploy(deploy).await?;
            if !fs_wal.is_empty() {
                block_fs_wal.extend(fs_wal);
            }
            let checkpoint = self.runtime.create_checkpoint().await;
            let next_root = checkpoint.root.to_bytes_prost();
            pd.pre_state_hash = current_root;
            pd.post_state_hash = next_root.clone();
            current_root = next_root;
            res.push((pd, mc));
        }
        if !block_fs_wal.is_empty() {
            let root = compute_wal_root(&block_fs_wal);
            // H-30b-4 review note (slice 30b round 2): OPERATOR-VISIBLE
            // LOG SCHEMA — do not rename these fields without
            // coordinating with dashboards.
            tracing::info!(
                target: "f1r3fly.casper.fs_wal",
                n_entries = block_fs_wal.len(),
                block_wal_root = %hex::encode(&root[..8]),
                "per-block consensus WAL slice computed"
            );
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
        // H-1 fix (2026-08-06) — slice 30c Phase B: cache the
        // per-block WAL slice keyed by the post-state hash (`current_root`
        // after all per-deploy checkpoints from cost-accounted's flow).
        // A finalized block carries this same `post_state_hash` in
        // `block.body.state.post_state_hash`, so
        // `finalization_runner::new_lfb_found_effect` can look up the
        // slice by that key and snapshot on cadence hits.
        if !block_fs_wal.is_empty() {
            let block_number = self.runtime.block_data_ref.read().await.block_number;
            const MAX_PENDING_WAL_SLICES: usize = 1024;
            let mut slices = self.runtime.pending_wal_slices.write().await;
            if slices.len() >= MAX_PENDING_WAL_SLICES {
                if let Some(oldest_key) = slices
                    .iter()
                    .min_by_key(|(_, (bn, _))| *bn)
                    .map(|(k, _)| k.clone())
                {
                    slices.remove(&oldest_key);
                    tracing::warn!(
                        target: "f1r3fly.casper.fs_wal",
                        cap = MAX_PENDING_WAL_SLICES,
                        "pending_wal_slices cache full; evicting oldest entry.  Deep-fork scenario or stalled finalizer?"
                    );
                }
            }
            slices.insert(current_root.to_vec(), (block_number, block_fs_wal));
        }
        Ok((current_root, res))
    }

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     */
    pub async fn play_deploys_for_genesis(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play_deploys_genesis", "play-deploys-genesis-started");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;

        // Slice 31 + H-P7-5 review fix: RAII exemption for the URN
        // filter.  Genesis composition needs `rho:io:fs:native:*`
        // URNs available so the FsGenesis deploy can bind `fsRead`,
        // `fsWrite`, etc.  The Drop impl re-enables the filter on
        // ALL exit paths — including panics and tokio-task
        // cancellation — so a subsequent user deploy on this runtime
        // cannot inherit the exemption even under adversarial or
        // buggy execution.  Guard holds an Arc<AtomicBool> clone of
        // the flag (not a runtime borrow), so mutable-runtime access
        // inside the loop is unblocked.
        let _filter_exemption = self.runtime.exempt_fs_native_urn_filter();
        let mut res = Vec::with_capacity(terms.len());
        let mut current_root = start_hash.clone();
        for deploy in terms {
            let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy)
                .map_err(|error| {
                CasperError::RuntimeError(format!(
                    "legacy uplift to Cosigned failed in genesis: {error}"
                ))
            })?;
            let (mut processed, mergeable, _fs_wal, _) = self
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
        // Cost-accounted merge: `current_root` already reflects the
        // final state hash (updated after each deploy's checkpoint in
        // the loop above), so no redundant final checkpoint needed.
        // `_filter_exemption` RAII-drops at end of scope naturally.
        Ok((current_root, res))
    }

    /// Evaluates a legacy single-signature deploy under the canonical
    /// reservation and realized-cost settlement protocol. The adapter preserves
    /// the deploy identifier and cost trace by uplifting to a one-signer
    /// `Cosigned<DeployData>` envelope and delegating to the canonical path.
    pub async fn play_ordinary_deploy(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal, Vec<WalEntry>), CasperError> {
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
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal, Vec<WalEntry>), CasperError> {
        tracing::debug!(target: "f1r3fly.casper.play-deploy", "play-deploy-started");
        let primary_pk_hex = hex::encode(&cosigned.primary().pk.bytes);

        // USER DEPLOY (owns its own inner soft-checkpoint for failed-deploy
        // rollback + WalDeployScope for the consensus WAL). The admission
        // gate certified and reserved authority; the realized debit is
        // checked and applied at block close.
        tracing::debug!(target: "f1r3fly.casper.user-deploy",
            "user-deploy-started primary_pk={}", primary_pk_hex);
        let (pd, mc, fs_wal) = self.process_deploy_cosigned(cosigned).await?;

        let mut mergeable: HashMap<Par, MergeType> = HashMap::new();
        mergeable.extend(mc);
        let mergeable_channels_data = self.get_number_channels_data(&mergeable).await?;
        Ok((pd, mergeable_channels_data, fs_wal))
    }

    /// Legacy single-signature user-deploy execution. Uplifts to
    /// `Cosigned<DeployData>` and delegates to [`Self::process_deploy_cosigned`]
    /// for byte-identical observable behavior.
    pub async fn process_deploy(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>, Vec<WalEntry>), CasperError> {
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
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>, Vec<WalEntry>), CasperError> {
        let (processed, mergeable, fs_wal, _) = self
            .process_deploy_cosigned_with_budget(cosigned, Cost::unsafe_max())
            .await?;
        Ok((processed, mergeable, fs_wal))
    }

    async fn process_deploy_cosigned_with_budget(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
    ) -> Result<
        (
            ProcessedDeploy,
            HashMap<Par, MergeType>,
            Vec<WalEntry>,
            bool,
        ),
        CasperError,
    > {
        self.process_deploy_cosigned_with_budget_and_authority(cosigned, budget, None, true)
            .await
    }

    async fn process_deploy_cosigned_with_budget_and_authority(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
        budget: Cost,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
        report_exhaustion: bool,
    ) -> Result<
        (
            ProcessedDeploy,
            HashMap<Par, MergeType>,
            Vec<WalEntry>,
            bool,
        ),
        CasperError,
    > {
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
    ) -> Result<
        (
            ProcessedDeploy,
            HashMap<Par, MergeType>,
            Vec<WalEntry>,
            bool,
        ),
        CasperError,
    > {
        // WalDeployScope — opens BEFORE the soft-checkpoint so the
        // atomic-deploy boundary spans both the RSpace state (owned by
        // the inner soft-checkpoint) and the per-runtime consensus WAL
        // + LockRegistry sweep (owned by wal_scope). Any early return
        // via `?` or explicit `return Err(...)` runs wal_scope's Drop,
        // which discards WAL entries produced by the failed deploy so
        // they do not leak into the next deploy's slice, and sweeps
        // this deploy's range-lock acquires from the shared
        // LockRegistry. On success, `take_and_commit(&deploy_log)`
        // drains the deploy's WAL contribution in canonical log-order
        // (H-R3) for the block emitter to aggregate.
        //
        // Step 5: derive deploy_scope from the primary signer's
        // signature so every lock acquired under this deploy is
        // tagged with a unique 32-byte identifier.
        let deploy_scope: DeployScope = {
            let h =
                crypto::rust::hash::blake2b256::Blake2b256::hash(cosigned.primary().sig.to_vec());
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        };
        let mut wal_scope = WalDeployScope::new_with_lock_sweep(
            self.runtime.fs_handles.wal.clone(),
            self.runtime.fs_handles.lock_registry.clone(),
            deploy_scope,
            self.runtime.fs_handles.current_deploy_scope.clone(),
        );

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
        let primary_sig = cosigned.primary().sig.clone();
        let is_compound = cosigned.is_compound();
        let extracted_threshold = cosigned.cosigner_threshold() as i32;
        // For multi-sig deploys (§1.9): extract cosigner data BEFORE the
        // `into_legacy_signed_unchecked` consumes the envelope, so the
        // ProcessedDeploy carries the full cosigner list through block storage
        // and replay. D3 (DR-9): no per-signer phlo_share.
        let extracted_cosigners: Vec<models::casper::CompoundSigner> = if is_compound {
            cosigned
                .signers()
                .iter()
                .skip(1)
                .map(|c| models::casper::CompoundSigner {
                    pk: c.pk.bytes.clone().into(),
                    sig: c.sig.clone(),
                    sig_algorithm: c.sig_algorithm.name(),
                })
                .collect()
        } else {
            Vec::new()
        };
        // Reconstitute the legacy Signed<DeployData> shape for the
        // `ProcessedDeploy.deploy` field. For single-sig (legacy uplift),
        // this returns a byte-identical legacy envelope. For multi-sig,
        // the additional cosigners survive via the `cosigners` field
        // alongside, NOT through the inner Signed shape.
        let legacy_signed = cosigned.into_legacy_signed_unchecked();

        let deploy_log = deploy_log
            .into_iter()
            .map(event_converter::to_casper_event)
            .collect::<Vec<_>>();

        // Slice 30 (C-30-1 round-2 fix) + H-R3 integration:
        // drain the deploy's per-runtime WAL contribution in
        // canonical log-order.  Success only — a failed evaluation
        // leaves `wal_scope` un-committed so its Drop discards the
        // entries, mirroring the soft-checkpoint revert on the
        // RSpace side.
        let fs_wal = if eval_succeeded {
            let drained = wal_scope.take_and_commit(&deploy_log);
            if !drained.is_empty() {
                let wal_root = compute_wal_root(&drained);
                tracing::debug!(
                    target: "f1r3fly.casper.fs_wal",
                    deploy_sig = hex::encode(&primary_sig).as_str(),
                    n_entries = drained.len(),
                    wal_root = %hex::encode(&wal_root[..8]),
                    "fs-wal per-deploy drain (committed)"
                );
            }
            drained
        } else {
            Vec::new()
        };

        let deploy_result = ProcessedDeploy {
            deploy: legacy_signed,
            cost: Cost::to_proto(eval_result.cost),
            deploy_log,
            is_failed: !eval_succeeded,
            system_deploy_error: None,
            cosigners: extracted_cosigners,
            cosigner_threshold: extracted_threshold,
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
                interpreter_util::print_deploy_errors(&primary_sig, &eval_result.errors);
            }
        }

        Ok((deploy_result, eval_result.mergeable, fs_wal, exhausted))
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
        let (pd, merge_chs, _fs_wal) = self.process_deploy_cosigned(cosigned).await?;
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

        // Slice 30c F-30b-8 fix: wrap the standalone system deploy
        // path with a WalDeployScope so any Consensus WAL entries a
        // system deploy would produce are drained + discarded per-
        // deploy rather than leaking into the next user deploy's
        // slice.  Currently no system deploy (CloseBlock, Slash,
        // PreCharge, Refund) touches Consensus caps at all — they
        // dispatch only to PoS/vault Rholang contracts, none of
        // which invoke fs-native URNs.  If a future system deploy
        // is written to touch Consensus caps, its WAL entries
        // land in `_leaked_entries` here, get logged at warn, and
        // are discarded rather than silently attributed to the
        // next user deploy.  A follow-up slice can extend the
        // block-level WAL aggregator to include system-deploy
        // contributions (needs a proto extension: system deploys
        // don't have per-deploy WAL attribution on
        // `ProcessedSystemDeploy` today).
        // Step 5: system deploys don't have a signature (they're
        // internal PoS/vault/system operations), so derive scope from
        // `state_hash` prefixed by a system-deploy marker.  System
        // deploys currently don't touch fs-native URNs — the sweep is
        // a no-op — but a non-sentinel scope is required to avoid
        // tripping the `release_all_for_deploy` debug_assert on drop.
        let deploy_scope: DeployScope = {
            let mut input = b"phase8-system-deploy:".to_vec();
            input.extend_from_slice(state_hash);
            let h = crypto::rust::hash::blake2b256::Blake2b256::hash(input);
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        };
        let _wal_scope = WalDeployScope::new_with_lock_sweep(
            self.runtime.fs_handles.wal.clone(),
            self.runtime.fs_handles.lock_registry.clone(),
            deploy_scope,
            self.runtime.fs_handles.current_deploy_scope.clone(),
        );

        let (event_log, result, mergeable_channels) =
            self.play_system_deploy_internal(system_deploy).await?;

        let final_state_hash = {
            let checkpoint = self.runtime.create_checkpoint().await;
            checkpoint.root.to_bytes_prost()
        };

        match result {
            Either::Right(system_deploy_result) => {
                let mcl = self.get_number_channels_data(&mergeable_channels).await?;
                if let Some(SlashDeploy {
                    invalid_block_hash,
                    pk,
                    target_activation_epoch,
                    initial_rand: _,
                }) = system_deploy.as_any().downcast_ref::<SlashDeploy>()
                {
                    Ok(SystemDeployResult::play_succeeded(
                        state_hash.clone(),
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_slash(
                            invalid_block_hash.clone(),
                            pk.clone(),
                            *target_activation_epoch,
                        ),
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

            Either::Left(usr_err) => Ok(SystemDeployResult::play_failed(event_log, usr_err)),
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
        deploy: &Signed<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        let cosigned =
            crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy.clone())
                .map_err(|e| {
                    CasperError::RuntimeError(format!(
                        "legacy uplift to Cosigned failed in genesis replay: {e}"
                    ))
                })?;
        self.evaluate_cosigned_with_budget_and_authority_mode(
            &cosigned,
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
                if cosigned.is_compound() {
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

        let primary = cosigned.primary();
        // Production bounded play and replay pass the same finite
        // authority-derived capacity here. The unbounded default remains only
        // for non-consensus exploratory and system-facing callers that do not
        // produce an admitted user-deploy certificate.
        let result = self
            .runtime
            .evaluate_with_authority(
                &cosigned.data.term,
                budget,
                models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(cosigned),
                Tools::unforgeable_name_rng(&primary.pk, cosigned.data.time_stamp),
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
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;

    /// H-1 fix regression pin (2026-08-06) — slice 30c Phase B.
    /// `play_deploys_for_state` MUST NOT call `SnapshotWriter::
    /// maybe_write` directly (that was the pre-H-1 per-block trigger
    /// that forked snapshot writes on sibling non-finalized DAG
    /// tips).  Post-H-1, the slice is cached in
    /// `pending_wal_slices` and the actual write happens in
    /// `finalization_runner::new_lfb_found_effect` after the block
    /// finalizes.
    ///
    /// Source-scan pin: a future refactor that re-introduces
    /// `writer.maybe_write(...)` inside `play_deploys_for_state`'s
    /// body would trip this test.  Cheap to run; catches the
    /// class of regression that would silently reintroduce the
    /// per-block trigger.
    #[test]
    fn play_deploys_for_state_does_not_call_maybe_write_directly() {
        let src = include_str!("runtime.rs");
        // Locate the `pub async fn play_deploys_for_state` body by
        // string scan.  Look between the `pub async fn` header and
        // the matching function-closing `Ok((final_root, res))`
        // return (the terminal expression of this function — see
        // line ~499 of this file).
        let start_idx = src
            .find("pub async fn play_deploys_for_state")
            .expect("play_deploys_for_state must exist in this file");
        let end_marker = "Ok((final_root, res))";
        let body_end = src[start_idx..]
            .find(end_marker)
            .expect("terminal return must exist inside play_deploys_for_state");
        let body = &src[start_idx..start_idx + body_end];
        // Match call-syntax (`.maybe_write(`) rather than the bare
        // symbol — the H-1 docstring in this function references
        // "SnapshotWriter::maybe_write" as explanation, which is
        // fine; the anti-pattern is the invocation itself.
        assert!(
            !body.contains(".maybe_write("),
            "play_deploys_for_state must NOT call SnapshotWriter::maybe_write \
             directly — H-1 fix moved the trigger to finalization_runner's \
             new_lfb_found_effect so snapshots reflect only the finalized \
             chain, not sibling non-finalized DAG tips.  If you need to \
             re-add per-block snapshotting, first reason carefully about \
             cross-fork snapshot content divergence."
        );
        // Positive: the fix inserts into `pending_wal_slices` at
        // end of the function.  Verify that path is present.
        assert!(
            body.contains("pending_wal_slices"),
            "play_deploys_for_state must cache the per-block WAL slice into \
             pending_wal_slices for finalization_runner to pick up"
        );
    }

    /// H-2 fix regression pin (2026-08-06): replay path wraps each
    /// deploy in `WalDeployScope`.  Pre-fix, `replay_deploy_e` did
    /// not scope the WAL for the deploy, so follower-side journal
    /// appends (from the `is_replay` branches of fs_read /
    /// fs_write / fs_truncate) accumulated across every deploy in a
    /// block until the runtime was reset.  Consequences: unbounded
    /// growth (hit `MAX_WAL_ENTRIES` on follower only), no per-
    /// deploy comparison against the leader's committed slice.
    /// Post-fix, `replay_deploy_e` uses `WalDeployScope::new` +
    /// `take_and_commit` (discard-drain on error via Drop) so the
    /// follower's per-deploy lifecycle mirrors the leader's.
    #[test]
    fn replay_deploy_e_wraps_in_wal_deploy_scope() {
        let src = include_str!("replay_runtime.rs");
        // Locate `pub async fn replay_deploy_e` and confirm it
        // constructs a `WalDeployScope` and calls
        // `take_and_commit`.  Search the whole file since the
        // helper is invoked by fully-qualified path — a future
        // refactor that renames the helper would still trip this
        // if either the type or the method name shifted.
        assert!(
            src.contains("WalDeployScope::new"),
            "H-2 regression: replay_runtime.rs must construct a WalDeployScope \
             (per-deploy WAL drain guard).  Pre-H-2 the follower's WAL grew \
             unboundedly across a block."
        );
        assert!(
            src.contains("take_and_commit"),
            "H-2 regression: replay_runtime.rs must call take_and_commit on \
             the deploy's WalDeployScope so the per-deploy slice is drained \
             in canonical log order (matching the leader's commitment)."
        );
    }

    /// H-1 companion pin: `new_lfb_found_effect` in the finalization
    /// runner MUST consume from `pending_wal_slices` and call
    /// `maybe_write`.  If a refactor drops the LFB hook, snapshots
    /// stop firing entirely rather than resurrecting the per-block
    /// hazard.
    #[test]
    fn finalization_runner_new_lfb_found_effect_writes_snapshots() {
        let src = include_str!("../engine/multi_parent_casper/finalization_runner.rs");
        assert!(
            src.contains("pending_wal_slices"),
            "finalization_runner must consume pending_wal_slices \
             (H-1 fix: LFB-triggered snapshot write)"
        );
        assert!(
            src.contains("maybe_write"),
            "finalization_runner must call SnapshotWriter::maybe_write \
             on cache hits (H-1 fix)"
        );
    }

    fn mk_entry(tag: &str) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: std::path::PathBuf::from(format!("/{tag}")),
            extra_path: None,
            offset: None,
            length: Some(tag.len() as u64),
            payload_ref: Some(PayloadRef::hash(tag.as_bytes())),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    // ---------------------------------------------------------------
    // C-30-1 / C-30-2 round-2 review-fix tests: WalDeployScope
    // ---------------------------------------------------------------

    /// C-30-2: success path drains via `take_and_commit` and hands
    /// entries to the caller; the underlying Wal is empty after.
    #[test]
    fn wal_deploy_scope_take_and_commit_returns_entries_and_empties_wal() {
        let wal = Wal::new();
        wal.append(mk_entry("pre")).unwrap();
        let mut scope = WalDeployScope::new(wal.clone());
        wal.append(mk_entry("a")).unwrap();
        wal.append(mk_entry("b")).unwrap();
        let entries = scope.take_and_commit(&[]);
        assert_eq!(entries.len(), 2, "committed entries include only post-mark");
        assert_eq!(wal.len(), 1, "pre-mark entry survives the drain");
        assert_eq!(entries[0].path, std::path::PathBuf::from("/a"));
        assert_eq!(entries[1].path, std::path::PathBuf::from("/b"));
        // Dropping a committed scope is a no-op.
        drop(scope);
        assert_eq!(wal.len(), 1, "Drop after commit does not double-drain");
    }

    /// C-30-1: the *core* invariant.  Dropping a scope WITHOUT
    /// commit discards entries appended during its lifetime.  This
    /// pins the fix for the `?`-early-return leak that the pre-fix
    /// code had.
    #[test]
    fn wal_deploy_scope_drop_without_commit_discards_entries() {
        let wal = Wal::new();
        wal.append(mk_entry("pre")).unwrap();
        {
            let _scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("leak-1")).unwrap();
            wal.append(mk_entry("leak-2")).unwrap();
            // `_scope` drops here without commit — discard-drain.
        }
        assert_eq!(
            wal.len(),
            1,
            "Drop without commit MUST discard post-mark entries so the next \
             deploy's begin_deploy sees an empty tail (C-30-1 regression pin)"
        );
        let remaining = wal.snapshot();
        assert_eq!(remaining[0].path, std::path::PathBuf::from("/pre"));
    }

    /// C-30-1 simulated `?`-early-return: a function that
    /// constructs a scope, appends, then returns Err before
    /// committing.  Verifies the scope's Drop still discards.
    #[test]
    fn wal_deploy_scope_early_return_pattern_does_not_leak() {
        let wal = Wal::new();

        fn simulated_deploy(wal: Wal) -> Result<(), &'static str> {
            let _scope = WalDeployScope::new(wal.clone());
            wal.append(WalEntry {
                op: WalOp::Truncate,
                path: std::path::PathBuf::from("/leaked"),
                extra_path: None,
                offset: Some(0),
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            })
            .unwrap();
            // Simulate a `?`-propagated error: return without commit.
            Err("simulated deploy failure")
        }

        let r = simulated_deploy(wal.clone());
        assert!(r.is_err(), "test precondition: simulated deploy fails");
        assert_eq!(
            wal.len(),
            0,
            "early-return via error must discard-drain (C-30-1 fix)"
        );
    }

    /// Sequential deploys: deploy A commits, deploy B commits — each
    /// sees only its own entries.  Pins per-deploy isolation.
    #[test]
    fn wal_deploy_scope_sequential_deploys_are_isolated() {
        let wal = Wal::new();
        let a_entries = {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("a1")).unwrap();
            wal.append(mk_entry("a2")).unwrap();
            scope.take_and_commit(&[])
        };
        let b_entries = {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("b1")).unwrap();
            scope.take_and_commit(&[])
        };
        assert_eq!(a_entries.len(), 2);
        assert_eq!(b_entries.len(), 1);
        assert_eq!(a_entries[0].path, std::path::PathBuf::from("/a1"));
        assert_eq!(b_entries[0].path, std::path::PathBuf::from("/b1"));
        assert_eq!(wal.len(), 0, "both deploys drained; WAL is empty");
    }

    /// Failed deploy followed by successful deploy: the successful
    /// deploy's commit MUST NOT include the failed deploy's entries.
    #[test]
    fn wal_deploy_scope_failed_deploy_does_not_pollute_next_deploy() {
        let wal = Wal::new();
        {
            let _scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("failed")).unwrap();
            // Drop without commit — discard-drain.
        }
        let b_entries = {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("clean")).unwrap();
            scope.take_and_commit(&[])
        };
        assert_eq!(
            b_entries.len(),
            1,
            "next deploy's slice must be exactly its own contributions"
        );
        assert_eq!(b_entries[0].path, std::path::PathBuf::from("/clean"));
        assert!(!b_entries
            .iter()
            .any(|e| e.path == std::path::Path::new("/failed")));
    }

    // ---------------------------------------------------------------
    // Phase 8 slice 8a step 5 — WalDeployScope::end auto-release hook.
    //
    // Tests below verify that WalDeployScope's Drop calls
    // LockRegistry::release_all_for_deploy(&deploy_scope) so any
    // locks the deploy acquired but did NOT release get swept at
    // deploy end (spec §Explicit locks MUST auto-release).  The
    // sweep is deploy-scope-scoped: locks acquired under OTHER
    // deploy scopes (e.g., prior/next deploys, if they interleave in
    // the shared LockRegistry) are unaffected.
    // ---------------------------------------------------------------

    use rholang::rust::interpreter::io::lock::{
        AcquireOutcome, HolderId, LockError, LockMode, LockRegistry, WaitPolicy,
    };

    fn holder(byte: u8) -> HolderId { HolderId::from_bytes([byte; 32]) }

    /// Deploy-end sweep: a lock acquired under this deploy's scope
    /// gets released when the WalDeployScope drops (caller neither
    /// released it explicitly nor closed the File cap).
    #[test]
    fn wal_deploy_scope_drop_releases_leaked_locks() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xA1u8; 32];

        // Acquire a lock under this deploy scope, then drop the
        // scope guard without releasing.
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            lock_registry
                .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy_scope)
                .expect("acquire under fresh scope must succeed");
            assert_eq!(
                lock_registry.held_locks(),
                1,
                "lock must be held BEFORE drop"
            );
            // _scope drops here — Drop calls release_all_for_deploy.
        }
        assert_eq!(
            lock_registry.held_locks(),
            0,
            "WalDeployScope::drop MUST sweep the deploy's leaked locks \
             (spec §Explicit locks MUST auto-release at deploy end)"
        );
    }

    /// Deploy-end sweep is scoped: locks acquired under OTHER
    /// deploy scopes survive.  Simulates two deploys sharing a
    /// LockRegistry (as would happen across two sequential deploys
    /// on the same runtime).
    #[test]
    fn wal_deploy_scope_drop_only_releases_matching_scope() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_a: DeployScope = [0xA1u8; 32];
        let deploy_b: DeployScope = [0xB2u8; 32];

        // Deploy B acquires first (not yet in scope guard).
        lock_registry
            .try_acquire_range((1, 42), 200, 100, LockMode::Read, holder(2), deploy_b)
            .expect("deploy B's acquire must succeed");

        // Deploy A runs in its scope guard, acquires another lock,
        // then drops — sweep must only clear A's, leaving B's alone.
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_a,
                current_scope_cell.clone(),
            );
            lock_registry
                .try_acquire_range((1, 43), 0, 100, LockMode::Write, holder(1), deploy_a)
                .expect("deploy A's acquire must succeed");
            assert_eq!(lock_registry.held_locks(), 2);
        }
        assert_eq!(
            lock_registry.held_locks(),
            1,
            "sweep MUST be scope-scoped: only deploy A's lock cleared, \
             deploy B's survives"
        );
    }

    /// Constructor sets the current-scope cell so concurrent
    /// handler calls record the correct scope; Drop clears it back
    /// to sentinel.
    #[test]
    fn wal_deploy_scope_publishes_scope_to_shared_cell() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xC3u8; 32];

        // Before the guard: sentinel.
        assert_eq!(*current_scope_cell.read().unwrap(), [0u8; 32]);

        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            // Inside the guard: real scope.
            assert_eq!(
                *current_scope_cell.read().unwrap(),
                deploy_scope,
                "constructor must publish scope to the shared cell so \
                 concurrent lock-native handlers can read it at acquire time"
            );
        }
        // After drop: back to sentinel.
        assert_eq!(
            *current_scope_cell.read().unwrap(),
            [0u8; 32],
            "Drop MUST clear the scope cell back to sentinel so \
             between-deploy handler calls see no live scope"
        );
    }

    /// Multiple locks under one deploy scope all get swept.
    #[test]
    fn wal_deploy_scope_sweeps_multiple_locks_under_same_deploy() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xD4u8; 32];

        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            // Acquire 5 range locks + 1 sequential (on different inode).
            for i in 0..5 {
                lock_registry
                    .try_acquire_range(
                        (1, 42),
                        i * 200,
                        100,
                        LockMode::Read,
                        holder(1),
                        deploy_scope,
                    )
                    .expect("range acquire must succeed");
            }
            lock_registry
                .try_acquire_sequential((1, 43), holder(2), deploy_scope)
                .expect("sequential acquire must succeed");
            assert_eq!(lock_registry.held_locks(), 6);
        }
        assert_eq!(
            lock_registry.held_locks(),
            0,
            "sweep MUST release EVERY lock under the deploy scope, \
             both range and sequential"
        );
    }

    // ---------------------------------------------------------------
    // Step-5 review gap tests (2026-08-13, follow-up on the whole-
    // step-5 review).  Address the four coverage gaps flagged in the
    // review by pinning:
    //   (1) Panic-path sweep — Drop runs on panic-unwind too.
    //   (2) Sentinel-scope construction panic (matches the runtime
    //       assert! promoted from debug_assert in commit 6f537099).
    //   (3) Static-analysis pins on the 3 WalDeployScope call sites
    //       (process_deploy_cosigned_with_budget_and_authority_mode,
    //       play_system_deploy, replay_deploy_e) using
    //       `new_with_lock_sweep`, not legacy `new(wal)`.
    // Gap #2 from the review (handler-level current_deploy_scope
    // read pin) lives in rholang/src/rust/interpreter/io/handlers.rs
    // — that's where the handlers being pinned live.
    // ---------------------------------------------------------------

    /// **Gap 1: panic-path sweep.**  Drop runs on panic-unwind too
    /// per Rust semantics — but a regression that gated the sweep on
    /// `!committed` (mirroring the WAL discard-drain's gating) would
    /// silently skip sweep on error paths.  This test simulates a
    /// deploy-body panic and asserts the sweep still fires.
    ///
    /// Spec §Explicit locks: "Implementations MUST auto-release at
    /// deploy-end" — MUST covers ALL exit paths including panic-
    /// unwind.  A leaked lock past deploy-end would poison the next
    /// deploy's LockRegistry state cross-validator (each validator's
    /// panic-timing could differ), which is exactly the divergence
    /// the MAY→MUST promotion targeted.
    #[test]
    fn wal_deploy_scope_sweeps_on_panic_unwind() {
        use std::panic;
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xE5u8; 32];

        let registry_snapshot = lock_registry.clone();
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal,
                lock_registry,
                deploy_scope,
                current_scope_cell,
            );
            registry_snapshot
                .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy_scope)
                .unwrap();
            assert_eq!(registry_snapshot.held_locks(), 1);
            // Simulate a deploy-body panic — Drop runs on unwind.
            panic!("simulated deploy-body panic");
        }));
        assert!(
            result.is_err(),
            "catch_unwind must have captured the simulated panic"
        );
        assert_eq!(
            registry_snapshot.held_locks(),
            0,
            "WalDeployScope::drop MUST sweep leaked locks even on panic-unwind \
             path (spec §Explicit locks MUST auto-release runs on ALL exit \
             paths; a regression gating sweep on !committed would be caught \
             here)"
        );
    }

    /// **Gap 4: sentinel-scope construction panic.**  Pin the
    /// `assert!(deploy_scope != [0; 32])` guard promoted from
    /// `debug_assert!` in commit 6f537099.  Panic message matches
    /// the assert!'s message text so a future refactor changing the
    /// message trips this test.
    #[test]
    #[should_panic(
        expected = "WalDeployScope::new_with_lock_sweep called with sentinel scope [0; 32]"
    )]
    fn wal_deploy_scope_new_with_sentinel_scope_panics() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        // Sentinel scope MUST panic in ALL build modes (assert!,
        // not debug_assert!).  A future refactor that reverts to
        // debug_assert! would still pass this test in debug mode
        // but fail in release; both build modes' CI runs would
        // catch the regression.
        let _scope =
            WalDeployScope::new_with_lock_sweep(wal, lock_registry, [0u8; 32], current_scope_cell);
    }

    // ---------------------------------------------------------------
    // Slice 8b sub-3 (2026-08-12): WalDeployScope::drop MUST also
    // sweep parked `wait: true` waiters via
    // `cancel_all_waiters_for_deploy` so no waiter is leaked past
    // deploy end.  Symmetrical with the step-5
    // release_all_for_deploy sweep; the two invocations touch
    // disjoint sets (held locks vs. parked waiters) but share the
    // same "deploy end = clean up this deploy's registry footprint"
    // invariant.
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn wal_deploy_scope_drop_cancels_parked_waiters_for_this_deploy() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xC3u8; 32];

        // A different deploy pre-acquires the conflicting lock so
        // our `wait: true` acquire will actually park (rather than
        // immediately admit).
        let pre_deploy: DeployScope = [0xEEu8; 32];
        lock_registry
            .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(9), pre_deploy)
            .expect("pre-deploy conflict-holder acquire must succeed");

        // Park a waiter under `deploy_scope` inside the WalDeployScope
        // guard's lifetime.  Save the admit receiver so we can
        // assert what it signals on drop.
        let admit_rx = {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            let outcome = lock_registry
                .try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Write,
                    holder(1),
                    deploy_scope,
                    WaitPolicy::Wait,
                )
                .expect("wait:true acquire on conflict must return Parked, not error");
            let admit_rx = match outcome {
                AcquireOutcome::Parked { admit, .. } => admit,
                AcquireOutcome::Immediate(_) => panic!("expected Parked, got Immediate"),
            };
            assert_eq!(
                lock_registry.parked_waiters(),
                1,
                "waiter must be parked before scope drops"
            );
            admit_rx
            // _scope drops here — Drop MUST call
            // cancel_all_waiters_for_deploy(&deploy_scope), which
            // signals Err(Cancelled) on our admit oneshot.
        };
        assert_eq!(
            lock_registry.parked_waiters(),
            0,
            "sub-3 regression: WalDeployScope::drop MUST cancel this \
             deploy's parked waiters — a leak would leave the parked \
             entry in the registry indefinitely (and the caller's \
             await would hang forever if the pre_deploy holder \
             never releases)"
        );
        // The admit oneshot must have received Cancelled.
        let outcome = admit_rx
            .await
            .expect("admit sender must not drop before send");
        assert_eq!(
            outcome,
            Err(LockError::Cancelled),
            "sub-3 regression: the parked native's admit await MUST \
             see Err(Cancelled) so it can produce an FSERR_CANCELLED \
             reply to the caller"
        );
    }

    #[tokio::test]
    async fn wal_deploy_scope_drop_leaves_other_deploys_waiters_intact() {
        // Sweep is scope-scoped: waiters parked under a DIFFERENT
        // deploy scope survive this deploy's drop.
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_a: DeployScope = [0xA1u8; 32];
        let deploy_b: DeployScope = [0xB2u8; 32];

        // Pre-holder to force parking.
        let pre_deploy: DeployScope = [0xEEu8; 32];
        lock_registry
            .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(9), pre_deploy)
            .expect("pre acquire");

        // Deploy B parks a waiter (out-of-scope-guard OK — waiter
        // just carries the deploy_b tag).
        let _b_rx = {
            let outcome = lock_registry
                .try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Write,
                    holder(2),
                    deploy_b,
                    WaitPolicy::Wait,
                )
                .expect("deploy B park");
            match outcome {
                AcquireOutcome::Parked { admit, .. } => admit,
                _ => panic!("expected Parked"),
            }
        };
        assert_eq!(lock_registry.parked_waiters(), 1);

        // Deploy A enters its scope guard, parks a waiter, then
        // drops.  Only A's waiter should be cancelled.
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_a,
                current_scope_cell.clone(),
            );
            let _a_outcome = lock_registry
                .try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Write,
                    holder(1),
                    deploy_a,
                    WaitPolicy::Wait,
                )
                .expect("deploy A park");
            assert_eq!(lock_registry.parked_waiters(), 2);
        }
        assert_eq!(
            lock_registry.parked_waiters(),
            1,
            "sub-3 regression: sweep MUST be scope-scoped — only \
             deploy A's parked waiter cancelled, deploy B's survives"
        );
    }

    /// Source-scan pin: `WalDeployScope::drop` MUST invoke
    /// `cancel_all_waiters_for_deploy` alongside
    /// `release_all_for_deploy`.  A regression that removed the
    /// waiter-sweep call would leave `wait: true` acquires stranded
    /// at deploy end — the parked native's await would hang forever
    /// unless something else signalled the oneshot (nothing else in
    /// the current architecture does).
    #[test]
    fn wal_deploy_scope_drop_calls_cancel_all_waiters_for_deploy() {
        let src = include_str!("runtime.rs");
        let impl_start = src
            .find("impl Drop for WalDeployScope")
            .expect("runtime.rs missing impl Drop for WalDeployScope");
        // 4KB window (was 3KB; sub-6 review-fix expanded the drop
        // body's rationale comment for the reversed ordering).
        let window = &src[impl_start..std::cmp::min(impl_start + 4000, src.len())];
        assert!(
            window.contains("cancel_all_waiters_for_deploy"),
            "sub-3 regression: WalDeployScope::drop MUST invoke \
             cancel_all_waiters_for_deploy — removing this call would \
             leak wait:true acquires past deploy end"
        );
        assert!(
            window.contains("release_all_for_deploy")
                && window.contains("cancel_all_waiters_for_deploy"),
            "sub-3 regression: both release AND cancel sweeps must \
             fire on drop"
        );
    }

    /// **Sub-6 review-fix B1 source-scan pin**: cancel MUST precede
    /// release in `WalDeployScope::drop`.  Reversed order (release
    /// first) has a same-deploy admission-then-leak bug — a waiter
    /// tagged with this deploy_scope can be admitted by
    /// wake_waiters (invoked internally by release_all_for_deploy)
    /// before the cancel_all sees it, creating a held lock scoped to
    /// a dying deploy that leaks past drop.
    #[test]
    fn wal_deploy_scope_drop_cancels_before_release() {
        let src = include_str!("runtime.rs");
        let impl_start = src
            .find("impl Drop for WalDeployScope")
            .expect("runtime.rs missing impl Drop for WalDeployScope");
        let window = &src[impl_start..std::cmp::min(impl_start + 4000, src.len())];
        let cancel_pos = window
            .find("cancel_all_waiters_for_deploy(&self.deploy_scope)")
            .expect("cancel_all_waiters_for_deploy invocation not found");
        let release_pos = window
            .find("release_all_for_deploy(&self.deploy_scope)")
            .expect("release_all_for_deploy invocation not found");
        assert!(
            cancel_pos < release_pos,
            "sub-6 review-fix B1 regression: cancel_all_waiters_for_\
             deploy MUST precede release_all_for_deploy — reversing \
             would allow same-deploy waiters to be admitted via \
             wake_waiters and leak past drop"
        );
    }

    /// **Sub-6 review-fix B1 behavioral test**: exercise the exact
    /// same-deploy admission-then-leak path that the reversed
    /// ordering fixes.  A waiter with `deploy = self.deploy_scope`
    /// that would be admissible after this deploy's release must
    /// NOT be admitted — it must be cancelled first.
    #[tokio::test]
    async fn wal_deploy_scope_drop_no_same_deploy_admission_leak() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xD4u8; 32];

        // Cap A (holder 1) acquires W under deploy_scope.
        // Cap B (holder 2, different holder, same deploy!) parks W
        // wait:true on the same range — conflicts with A.
        //
        // Under reversed order (post-fix):
        //   1. cancel_all_waiters_for_deploy(deploy_scope) —
        //      cancels B (its deploy matches)
        //   2. release_all_for_deploy(deploy_scope) — sweeps A;
        //      wake_waiters finds no B (already cancelled); nothing
        //      else to admit
        // Result: registry is EMPTY after drop.
        //
        // Under the pre-fix release-first order:
        //   1. release_all_for_deploy(deploy_scope) — sweeps A;
        //      wake_waiters admits B (same deploy, different holder
        //      → different holder passes conflict → admits with
        //      deploy = deploy_scope)
        //   2. cancel_all_waiters_for_deploy(deploy_scope) — finds
        //      nothing parked (B was just admitted)
        // Result: B's admitted range LEAKS with a dead deploy_scope.
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            lock_registry
                .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy_scope)
                .expect("A acquires");
            let _b_outcome = lock_registry
                .try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Write,
                    holder(2),
                    deploy_scope,
                    WaitPolicy::Wait,
                )
                .expect("B parks");
            assert_eq!(lock_registry.held_locks(), 1);
            assert_eq!(lock_registry.parked_waiters(), 1);
        }
        // After drop: no leak.  Both A (released) and B (cancelled)
        // are gone.
        assert_eq!(
            lock_registry.held_locks(),
            0,
            "sub-6 B1 regression: same-deploy waiter must not be \
             admitted-and-leaked through release-first ordering"
        );
        assert_eq!(lock_registry.parked_waiters(), 0);
        assert_eq!(lock_registry.tracked_files(), 0);
    }

    /// **Gap 3a: user-deploy path pin.**  Verify
    /// `process_deploy_cosigned_with_budget_and_authority_mode`
    /// constructs its `WalDeployScope` via `new_with_lock_sweep` (not
    /// legacy `new`) AND threads `lock_registry` +
    /// `current_deploy_scope` from `fs_handles`.  A regression that
    /// reverted to legacy `new(wal)` would leave the sweep un-plumbed
    /// for the user-deploy path.  Post cost-accounted merge, the
    /// WalDeployScope moved from the deleted
    /// `play_deploy_with_cost_accounting` into
    /// `process_deploy_cosigned_with_budget_and_authority_mode` so it
    /// shares the atomic-deploy boundary with the inner
    /// soft-checkpoint.
    #[test]
    fn user_deploy_path_uses_new_with_lock_sweep() {
        let src = include_str!("runtime.rs");
        assert!(
            src.contains("async fn process_deploy_cosigned_with_budget_and_authority_mode"),
            "runtime.rs missing process_deploy_cosigned_with_budget_and_authority_mode definition"
        );
        assert!(
            src.contains("WalDeployScope::new_with_lock_sweep("),
            "step 5 regression: user-deploy path must construct \
             WalDeployScope via new_with_lock_sweep — a revert to legacy `new` \
             would leave the sweep un-plumbed"
        );
        assert!(
            src.contains("fs_handles.lock_registry.clone()"),
            "step 5 regression: user-deploy path must pass \
             fs_handles.lock_registry to new_with_lock_sweep so the sweep \
             targets the shared registry"
        );
        assert!(
            src.contains("fs_handles.current_deploy_scope.clone()"),
            "step 5 regression: user-deploy path must pass \
             fs_handles.current_deploy_scope to new_with_lock_sweep so the \
             handlers can read the deploy scope at acquire time"
        );
    }

    /// **Gap 3b: system-deploy path pin.**  System deploys use a
    /// state-hash-derived scope with the "phase8-system-deploy:"
    /// prefix so system-deploy scope space doesn't collide with
    /// user-deploy scope space.  This test pins both the construction
    /// call AND the prefix.
    #[test]
    fn play_system_deploy_uses_new_with_lock_sweep() {
        let src = include_str!("runtime.rs");
        assert!(
            src.contains("pub async fn play_system_deploy"),
            "runtime.rs missing play_system_deploy definition"
        );
        assert!(
            src.contains("phase8-system-deploy:"),
            "step 5 regression: system-deploy scope derivation must include \
             the \"phase8-system-deploy:\" prefix so system-deploy scope space \
             is disjoint from user-deploy scope space (Blake2b256 domain \
             separation)"
        );
    }

    /// **Gap 3c: replay-path pin.**  Follower replay uses the SAME
    /// Blake2b256(deploy.sig) scope derivation as the leader — a
    /// divergence in this derivation would be silent under is_replay
    /// (LockRegistry state stays empty on the follower) but would be
    /// load-bearing if a future refactor removes the is_replay short-
    /// circuit.  This test pins the identical derivation.
    #[test]
    fn replay_deploy_e_uses_new_with_lock_sweep() {
        let src = include_str!("replay_runtime.rs");
        assert!(
            src.contains("WalDeployScope::new_with_lock_sweep("),
            "step 5 regression: replay_deploy_e must construct WalDeployScope \
             via new_with_lock_sweep — a revert to legacy `new` would leave \
             the follower's sweep un-plumbed (invisible under is_replay but \
             a load-bearing divergence trap if is_replay short-circuit \
             changes)"
        );
        assert!(
            src.contains("processed_deploy.deploy.sig.to_vec()"),
            "step 5 regression: replay-path scope derivation must use \
             processed_deploy.deploy.sig to match the leader's derivation \
             byte-for-byte"
        );
    }

    // ---------------------------------------------------------------
    // C-30b-1 round-2 review-fix test: the play_deploys_for_state
    // aggregation composition.  Pre-fix, the extend + compute_wal_root
    // + maybe_write loop had zero test coverage; a refactor that
    // swapped `extend`→`push` (nesting Vec<Vec<...>>), reordered
    // per-deploy WALs, or called maybe_write on the wrong slice would
    // compile and pass every existing test.  This test exercises the
    // SAME composition — WalDeployScope drain × N deploys, aggregate
    // into a block-scope Vec, compute the root, hand to SnapshotWriter
    // — WITHOUT the full RuntimeManager weight.
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn play_deploys_aggregation_composition_writes_correct_snapshot() {
        use rholang::rust::interpreter::io::snapshot::{
            compute_wal_root, read_snapshot_bytes, snapshot_path, SnapshotWriter,
        };

        let snapshot_dir = tempfile::tempdir().unwrap();
        let wal = Wal::new();
        let writer = SnapshotWriter {
            dir: snapshot_dir.path().to_path_buf(),
            cadence: 1, // every block
            retain: 10,
            signer_sk: None,
        };

        // Simulate play_deploys_for_state's block-scan loop with 3
        // deploys.  Each deploy: WalDeployScope::new → append via WAL
        // → take_and_commit → extend aggregator.
        let mut block_fs_wal: Vec<WalEntry> = Vec::new();
        for (deploy_idx, tag) in [("deploy0", 3), ("deploy1", 1), ("deploy2", 2)]
            .iter()
            .enumerate()
        {
            let (label, n_entries) = tag;
            let mut scope = WalDeployScope::new(wal.clone());
            for i in 0..*n_entries {
                let path = format!("/{label}-e{i}");
                wal.append(mk_entry(&path[1..])).unwrap();
            }
            let fs_wal = scope.take_and_commit(&[]);
            assert_eq!(
                fs_wal.len(),
                *n_entries,
                "deploy {deploy_idx} committed the wrong number of entries"
            );
            block_fs_wal.extend(fs_wal);
        }
        assert_eq!(
            block_fs_wal.len(),
            6,
            "aggregation must accumulate ALL per-deploy contributions"
        );
        // Insertion order (deploy0's 3 entries, then deploy1's 1, then deploy2's 2)
        // MUST be preserved — the WAL root is order-sensitive.
        assert_eq!(
            block_fs_wal[0].path,
            std::path::PathBuf::from("/deploy0-e0")
        );
        assert_eq!(
            block_fs_wal[3].path,
            std::path::PathBuf::from("/deploy1-e0")
        );
        assert_eq!(
            block_fs_wal[4].path,
            std::path::PathBuf::from("/deploy2-e0")
        );

        // Compute the block WAL root — same call play_deploys_for_state
        // makes.
        let block_root = compute_wal_root(&block_fs_wal);

        // maybe_write on a cadence=1 block writes.
        let block_number = 5i64;
        let write_root = writer
            .maybe_write(block_number, &block_fs_wal)
            .expect("maybe_write must succeed with valid writer")
            .expect("cadence=1, non-empty entries: must produce a snapshot");
        assert_eq!(
            write_root, block_root,
            "SnapshotWriter's write must produce the SAME root as compute_wal_root on the same slice"
        );

        // The snapshot file exists at the content-addressed path.
        let snap_path = snapshot_path(snapshot_dir.path(), &write_root);
        assert!(snap_path.exists(), "snapshot file must be written to disk");
        // Read back + verify root.
        let bytes = read_snapshot_bytes(snapshot_dir.path(), &write_root).unwrap();
        assert!(!bytes.is_empty());

        // Per-runtime WAL was drained (no leftover entries).
        assert_eq!(
            wal.len(),
            0,
            "after 3 deploys × take_and_commit, per-runtime WAL is empty"
        );
    }

    /// C-30b-1 companion: pin the aggregation-order invariant.  If a
    /// future refactor accidentally reversed per-deploy order (e.g.,
    /// via a `.rev()`) or bucketed via a HashMap iteration, the block
    /// WAL root would flip.  This test asserts the exact root for a
    /// canonical 2-deploy sequence.
    #[tokio::test]
    async fn play_deploys_aggregation_preserves_deploy_scan_order() {
        use rholang::rust::interpreter::io::snapshot::compute_wal_root;

        let wal = Wal::new();
        let mut block_fs_wal: Vec<WalEntry> = Vec::new();

        // Deploy A appends "a".
        {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("a")).unwrap();
            block_fs_wal.extend(scope.take_and_commit(&[]));
        }
        // Deploy B appends "b".
        {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("b")).unwrap();
            block_fs_wal.extend(scope.take_and_commit(&[]));
        }
        let root_ab = compute_wal_root(&block_fs_wal);

        // Redo with reversed deploy order — different root.
        let wal2 = Wal::new();
        let mut block_fs_wal2: Vec<WalEntry> = Vec::new();
        {
            let mut scope = WalDeployScope::new(wal2.clone());
            wal2.append(mk_entry("b")).unwrap();
            block_fs_wal2.extend(scope.take_and_commit(&[]));
        }
        {
            let mut scope = WalDeployScope::new(wal2.clone());
            wal2.append(mk_entry("a")).unwrap();
            block_fs_wal2.extend(scope.take_and_commit(&[]));
        }
        let root_ba = compute_wal_root(&block_fs_wal2);

        assert_ne!(
            root_ab, root_ba,
            "deploy-scan order MUST be part of the block WAL root — \
             a refactor that reorders would silently fork consensus"
        );
    }

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
