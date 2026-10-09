//! The native certification harness of decision 47 (design v2, section 6).
//!
//! Proposer self-certification and validator replay certify an offered deploy in five steps:
//! the journal check, the trace binding, the native replay environment, its evaluation and the
//! completeness check (`casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/
//! replay.rs`). Before these steps, the proposer encodes the budget recording and the operation
//! journal into the cost evidence of the offer, and the validator decodes them
//! (`casper/src/rust/util/rholang/costacc/offered_evidence.rs`).
//!
//! This harness runs the same steps at the v6 limits of the offered funded protocol. The limits
//! come from `models::rust::cost_protocol_limits`. The harness restates the derivations of
//! `casper/src/rust/util/rholang/costacc/production_limits.rs`, because a rholang test cannot
//! depend on casper.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::cost_protocol_limits::{
    offered_funded_v6_host_work_limits, offered_funded_v6_limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
    NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{
    decode_native_budget_recording, decode_native_operation_journal,
    encode_native_budget_recording, encode_native_operation_journal, funding_sig_compound,
    NativeBudgetRecording, NativeObservationLink, NativeOperationJournalLimits,
    NativeOperationRecord, NativeOperationTraceLimits, NativeRecordingWireLimits,
    NativeRuntimeConfig, RuntimeBudget, Sig,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::reified::mirror_reconciler::FinalizedState;
use rholang::rust::interpreter::rho_runtime::{
    create_native_replay_env, create_rho_runtime, NativeReplayEnvironment, RhoHistoryRepository,
    RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::spaces::namespace::{self, Durability, ReifiedId};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hot_store::HotStoreState;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::MB;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
use rspace_plus_plus::rspace::space_store_handle::SUB_OPERATION_TAG;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

use super::session::Injections;

/// The deploy signature of every native run.
pub const DEPLOY: &[u8] = b"rrs-native-spaces";
/// The seed of the deploy's random state, the same in native play and native replay.
pub const SEED: &[u8] = b"rrs-native-spaces-random";
/// A native phlo limit that no program of the suites reaches.
pub const AMPLE_LIMIT: u64 = 1_000_000;
/// The weights of the compute, introduction, transfer and trace classes, and the price.
pub const WEIGHTS: [u64; 4] = [0, 1, 1, 0];
const PRICE: u64 = 3;
/// The public channel that the programs forward their results to.
pub const OUT: &str = "rrs-native-out";
/// The map size of the LMDB store of a chain that a restart test opens (P7).
const LMDB_MAP_SIZE: usize = 256 * MB;
/// The path segments of the v6 native trace: `V6_NATIVE_PATH_SEGMENTS` of
/// `casper/src/rust/util/rholang/costacc/production_limits.rs`.
const V6_NATIVE_PATH_SEGMENTS: usize = 1_024;

// ─── The v6 limits (restated from `production_limits.rs`) ──────────────────────────────────────

/// `offered_funded_v6_trace_limits`: the budget trace limits of native play and replay.
pub fn trace_limits() -> NativeBudgetTraceLimits {
    let protocol = offered_funded_v6_limits();
    NativeBudgetTraceLimits {
        attempts: protocol.deploy_log_events,
        path_segments: V6_NATIVE_PATH_SEGMENTS,
        regions: NativePhloRegionLimits {
            regions: protocol.envelope.payload.funding.authority_nodes,
            encoded_authority_bytes: protocol.funding_case.key_bytes,
        },
    }
}

/// `offered_funded_v6_recording_limits`: the wire limits of the recording in the evidence.
pub fn recording_limits() -> NativeRecordingWireLimits {
    let protocol = offered_funded_v6_limits();
    NativeRecordingWireLimits {
        wire: PhloWireLimits {
            total_bytes: protocol.evidence.field_bytes,
            field_bytes: protocol.evidence.field_bytes,
        },
        budget: trace_limits(),
        operations: protocol.deploy_log_events,
        source_entries: protocol.deploy_log_items,
        footprint_entries: protocol.deploy_log_items,
        footprint_bytes: protocol.evidence.field_bytes,
        predecessor_edges: protocol.deploy_log_items,
        total_path_segments: protocol.deploy_log_items,
    }
}

/// `offered_funded_v6_replay_limits().journal`: the limits of the journal check.
pub fn journal_limits() -> NativeOperationJournalLimits {
    let protocol = offered_funded_v6_limits();
    let recording = recording_limits();
    NativeOperationJournalLimits {
        budget: recording.budget,
        operations: recording.operations,
        total_path_segments: protocol.deploy_log_items,
        source_entries: recording.source_entries,
        footprint_entries: recording.footprint_entries,
        footprint_bytes: recording.footprint_bytes,
        predecessor_edges: recording.predecessor_edges,
    }
}

/// `offered_funded_v6_replay_limits().trace`: the limits of the trace binding.
pub fn trace_bind_limits() -> NativeOperationTraceLimits {
    let protocol = offered_funded_v6_limits();
    NativeOperationTraceLimits {
        events: protocol.deploy_log_events,
        source_entries: protocol.deploy_log_items,
        source_bytes: protocol.deploy_log_bytes,
        telemetry_items: protocol.deploy_log_items,
        telemetry_bytes: protocol.deploy_log_bytes,
    }
}

/// A fresh host-work budget of the v6 profile, as production gives each play and each replay.
pub fn host_work() -> HostWorkBudget { HostWorkBudget::new(offered_funded_v6_host_work_limits()) }

// ─── The native phlo contract ──────────────────────────────────────────────────────────────────

fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(WEIGHTS)
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: PRICE,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// Runs `action` with the native phlo contract of a deploy whose phlo limit is `limit`.
pub fn with_contract<T>(
    limit: u64,
    action: impl FnOnce(NativePhloExecutionContract<'_>) -> T,
) -> T {
    let descriptor = schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("the test schedule binds");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [PRICE];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: PRICE,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        limit,
    )
    .expect("the test controls check");
    let contract =
        NativePhloExecutionContract::new(controls, &binding).expect("the test contract builds");
    action(contract)
}

/// The native runtime configuration of a deploy whose phlo limit is `limit`.
pub fn config(limit: u64, host: HostWorkBudget) -> NativeRuntimeConfig {
    with_contract(limit, |contract| {
        NativeRuntimeConfig::new(contract, trace_limits(), host)
    })
}

pub fn random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(SEED) }

pub fn funding() -> Sig { funding_sig_compound(&[&1u64.to_be_bytes()]) }

// ─── Play ──────────────────────────────────────────────────────────────────────────────────────

/// A fresh runtime over the empty state, with the history at the empty root, and its history.
/// The system processes are installed in the hot store only, as the native replay environment
/// installs them again.
async fn fresh_runtime() -> (RhoRuntimeImpl, RhoHistoryRepository) {
    let (runtime, space) = fresh_space_runtime().await;
    (runtime, space.get_history_repository())
}

/// The play space of a native runtime. A clone shares the history handle of the space, so it
/// gives the repository of the latest checkpoint.
type PlaySpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// A fresh runtime over the empty state, and a clone of its play space.
async fn fresh_space_runtime() -> (RhoRuntimeImpl, PlaySpace) {
    let mut stores = InMemoryStoreManager::new();
    let (space, _) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory play and replay space");
    space.create_checkpoint().await.expect("the empty root");
    let runtime = space_runtime(space.clone()).await;
    (runtime, space)
}

/// A play runtime over `space`, with the system processes installed in its hot store.
async fn space_runtime(space: PlaySpace) -> RhoRuntimeImpl {
    create_rho_runtime(
        space,
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await
    .expect("a play runtime")
}

/// The values on [`OUT`], one Debug form for each datum, sorted.
async fn outputs_of(runtime: &RhoRuntimeImpl) -> Vec<String> {
    let mut outputs: Vec<String> = out_values_of(runtime)
        .await
        .iter()
        .map(|pars| format!("{pars:?}"))
        .collect();
    outputs.sort_unstable();
    outputs
}

/// The pars of each datum on [`OUT`], in the order of their Debug forms.
async fn out_values_of(runtime: &RhoRuntimeImpl) -> Vec<Vec<Par>> {
    let channel = new_gstring_par(OUT.to_string(), Vec::new(), false);
    let mut values: Vec<Vec<Par>> = runtime
        .get_data(&channel)
        .await
        .into_iter()
        .map(|datum| datum.a.pars)
        .collect();
    values.sort_by_cached_key(|pars| format!("{pars:?}"));
    values
}

/// The play of one deploy: its result, its event log, the history of its pre-state, its
/// post-state root, its outputs as Debug forms and as pars, and its random state.
pub struct Played {
    pub result: EvaluateResult,
    pub log: Vec<Event>,
    pub history: RhoHistoryRepository,
    pub root: Blake2b256Hash,
    pub outputs: Vec<String>,
    pub out: Vec<Vec<Par>>,
    pub random: Blake2b512Random,
    /// The volatile channels (ids with an ephemeral KIND) that hold records, waiting
    /// continuations or join entries in the hot store after the evaluation and before the
    /// checkpoint. The sweep at the end of each root evaluation leaves none (P7, design v3
    /// VR-2).
    pub residue: Vec<Par>,
    /// The durable space channels that hold records, waiting continuations or join entries in
    /// the hot store after the evaluation and before the checkpoint (P7).
    pub durable: Vec<Par>,
    /// The normalizer env of the play: the ids that the test injects through `rho:rrs:test:*`
    /// URNs (P7). Native replay normalizes the term with the same env.
    pub injections: Injections,
}

/// Plays `term` as a native deploy whose phlo limit is `limit`, on a fresh runtime.
pub async fn native_play(term: &str, limit: u64) -> Played {
    native_play_with(term, limit, host_work()).await
}

/// Plays `term` as a native deploy whose phlo limit is `limit` and whose host-work budget is
/// `host`, on a fresh runtime.
pub async fn native_play_with(term: &str, limit: u64, host: HostWorkBudget) -> Played {
    let (mut runtime, history) = fresh_runtime().await;
    runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
    play_on(
        &mut runtime,
        history,
        term,
        limit,
        host,
        random(),
        Injections::new(),
    )
    .await
}

/// Plays `term` like [`native_play_with`], and returns the result of the evaluation, an error
/// included. A deploy that the host-work check rejects after its last operation evaluates to
/// `Err(HostWorkRejected)`, so [`native_play_with`] cannot play it.
pub async fn native_play_result(
    term: &str,
    limit: u64,
    host: HostWorkBudget,
) -> Result<EvaluateResult, InterpreterError> {
    let (mut runtime, _) = fresh_runtime().await;
    runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
    runtime
        .evaluate_with_native_phlo(term, HashMap::new(), random(), None, config(limit, host))
        .await
}

/// Plays `term` as a native deploy with the random state `random` and the injected ids
/// `injections` on `runtime`, from its current root, and checkpoints the post-state into
/// `history`.
async fn play_on(
    runtime: &mut RhoRuntimeImpl,
    history: RhoHistoryRepository,
    term: &str,
    limit: u64,
    host: HostWorkBudget,
    random: Blake2b512Random,
    injections: Injections,
) -> Played {
    let result = runtime
        .evaluate_with_native_phlo(
            term,
            injections.clone(),
            random.clone(),
            None,
            config(limit, host),
        )
        .await
        .expect("native play evaluates");
    let soft = runtime.create_soft_checkpoint().await;
    let log = soft.log.into_iter().collect();
    let outputs = outputs_of(runtime).await;
    let out = out_values_of(runtime).await;
    let residue = hot_store_residue(&soft.cache_snapshot, Durability::Ephemeral);
    let durable = hot_store_residue(&soft.cache_snapshot, Durability::Durable);
    let root = runtime.create_checkpoint().await.root;
    Played {
        result,
        log,
        history,
        root,
        outputs,
        out,
        random,
        residue,
        durable,
        injections,
    }
}

// Changed by P7 step 5: the readers below listed the rows of `get_hot_changes`, and
// `HotStore::to_map` builds a row only for a channel that holds data. So a channel with waiting
// continuations alone, or with join entries alone, never reached the residue check. The readers
// now read every map of a hot-store snapshot (`hot_store_residue`).
//
// /// The volatile channels that hold records or waiting continuations in the hot store of
// /// `runtime`, sorted by their Debug form (P7, design v3 VR-2).
// pub async fn volatile_residue(runtime: &RhoRuntimeImpl) -> Vec<Par> {
//     space_residue(runtime, Durability::Ephemeral).await
// }
//
// /// The space-scoped channels of the durability `durability` that hold records or waiting
// /// continuations in the hot store of `runtime`, sorted by their Debug form (P7).
// pub async fn space_residue(runtime: &RhoRuntimeImpl, durability: Durability) -> Vec<Par> {
//     let mut residue: Vec<Par> = runtime
//         .get_hot_changes()
//         .await
//         .into_iter()
//         .filter(|(_, row)| !row.data.is_empty() || !row.wks.is_empty())
//         .flat_map(|(channels, _)| channels)
//         .filter(|channel| space_durability(channel) == Some(durability))
//         .collect();
//     residue.sort_by_cached_key(|channel| format!("{channel:?}"));
//     residue.dedup();
//     residue
// }

/// A snapshot of every map of a hot store: data, waiting continuations and join entries.
pub type HotState = HotStoreState<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The space-scoped channels of the durability `durability` that hold records, waiting
/// continuations or join entries in the hot-store snapshot `state`, sorted by their Debug form
/// (P7, design v3 VR-2). A swept channel keeps an empty entry (F31), which does not count.
pub fn hot_store_residue(state: &HotState, durability: Durability) -> Vec<Par> {
    let mut residue: Vec<Par> = Vec::new();
    for shard in state.data.iter() {
        for (channel, data) in shard.iter() {
            if !data.is_empty() {
                residue.push(channel.clone());
            }
        }
    }
    for shard in state.continuations.iter() {
        for (channels, waiters) in shard.iter() {
            if !waiters.is_empty() {
                residue.extend(channels.iter().cloned());
            }
        }
    }
    for shard in state.joins.iter() {
        for (channel, joins) in shard.iter() {
            if !joins.is_empty() {
                residue.push(channel.clone());
                residue.extend(joins.iter().flatten().cloned());
            }
        }
    }
    residue.retain(|channel| space_durability(channel) == Some(durability));
    residue.sort_by_cached_key(|channel| format!("{channel:?}"));
    residue.dedup();
    residue
}

/// The durability of a space-scoped channel, or `None` for any other channel.
fn space_durability(channel: &Par) -> Option<Durability> {
    match namespace::reified_id_of(channel)? {
        ReifiedId::Config(space)
        | ReifiedId::SpaceCounter(space)
        | ReifiedId::CollectionCounter(space, _)
        | ReifiedId::Data(space, _)
        | ReifiedId::PathIndex(space, _)
        | ReifiedId::Allocator(space)
        | ReifiedId::Handle(space)
        | ReifiedId::Name(space, _) => Some(space.durability),
        ReifiedId::Op(_) | ReifiedId::Matcher(..) | ReifiedId::Body(..) | ReifiedId::Reserved(..) => {
            None
        }
    }
}

/// A native runtime that plays deploys one after another, as consecutive blocks do. Each deploy
/// starts from the checkpoint root of the deploy before it, with the random state of its label,
/// and its play certifies from that root.
pub struct NativeChain {
    runtime: RhoRuntimeImpl,
    space: PlaySpace,
}

impl NativeChain {
    /// A chain over the empty state.
    pub async fn new() -> Self {
        let (runtime, space) = fresh_space_runtime().await;
        runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
        Self { runtime, space }
    }

    /// Plays `term` with the random state of `label` and the phlo limit `limit`. The play
    /// keeps the history repository of the latest checkpoint, the pre-state of the deploy.
    pub async fn play(&mut self, term: &str, label: &str, limit: u64) -> Played {
        self.play_with(term, label, limit, host_work(), Injections::new())
            .await
    }

    /// Plays `term` like [`NativeChain::play`], with the host-work budget `host` and the
    /// injected ids `injections`. The caller keeps a clone of `host` to read the host work of
    /// the play (P7).
    pub async fn play_with(
        &mut self,
        term: &str,
        label: &str,
        limit: u64,
        host: HostWorkBudget,
        injections: Injections,
    ) -> Played {
        let random = Blake2b512Random::create_from_bytes(label.as_bytes());
        let history = self.space.get_history_repository();
        play_on(
            &mut self.runtime,
            history,
            term,
            limit,
            host,
            random,
            injections,
        )
        .await
    }

    /// A chain at the root `root` of this chain's history with a new runtime, as a node that
    /// restarts reads its state: a spawned space with an empty hot store (`RSpace::spawn`), and
    /// a new reducer with no state of this chain's runtime (P7, design v3 RH-1).
    pub async fn restarted(&self, root: &Blake2b256Hash) -> Self {
        let space = self
            .space
            .spawn()
            .expect("spawn a space over the history of the chain");
        Self::at(space, root).await
    }

    /// A chain over the LMDB store in `directory`: at the root `root` of the store, or at the
    /// empty root of a new store when `root` is `None`. Each child process of a restart test
    /// opens the store that an earlier child wrote (P7, `reified_restart_rehydrate.rs`).
    pub async fn open(directory: &Path, root: Option<&Blake2b256Hash>) -> Self {
        let mut manager = mk_rspace_store_manager(directory.to_path_buf(), LMDB_MAP_SIZE);
        let stores = manager
            .r_space_stores()
            .await
            .expect("open the LMDB RSpace stores");
        let (space, _) =
            RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
                stores,
                Arc::new(Box::new(Matcher)),
            )
            .expect("an LMDB play and replay space");
        match root {
            Some(root) => Self::at(space, root).await,
            None => {
                space.create_checkpoint().await.expect("the empty root");
                let runtime = space_runtime(space.clone()).await;
                runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
                Self { runtime, space }
            }
        }
    }

    /// A chain over `space` with a new runtime reset to `root`.
    async fn at(space: PlaySpace, root: &Blake2b256Hash) -> Self {
        let mut runtime = space_runtime(space.clone()).await;
        runtime
            .reset(root)
            .await
            .expect("reset the runtime to the root");
        runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
        Self { runtime, space }
    }

    /// Resets the runtime of the chain to `root`, as a rollback to an earlier root does (P7,
    /// design v3 RH-2).
    pub async fn reset(&mut self, root: &Blake2b256Hash) {
        self.runtime
            .reset(root)
            .await
            .expect("reset the chain to the root");
    }

    /// The space-scoped channels of the durability `durability` that hold records or waiting
    /// continuations in the hot store of the chain's runtime (P7).
    pub async fn space_channels(&self, durability: Durability) -> Vec<Par> {
        hot_store_residue(&self.space.get_store().snapshot(), durability)
    }

    /// The history repository of the latest checkpoint or reset of the chain: the pre-state of
    /// its next deploy, as a validator holds it before replay (P7).
    pub fn history(&self) -> RhoHistoryRepository { self.space.get_history_repository() }

    /// The pars of each datum on the string channel `channel`.
    pub async fn values(&self, channel: &str) -> Vec<Vec<Par>> {
        self.runtime
            .get_data(&new_gstring_par(channel.to_string(), Vec::new(), false))
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }

    /// The data on `channel`.
    pub async fn data(
        &self,
        channel: &Par,
    ) -> Vec<rspace_plus_plus::rspace::internal::Datum<ListParWithRandom>> {
        self.runtime.get_data(channel).await
    }

    /// The waiting continuations on `channel` (P7).
    pub async fn waiters(
        &self,
        channel: &Par,
    ) -> Vec<rspace_plus_plus::rspace::internal::WaitingContinuation<BindPattern, TaggedContinuation>>
    {
        self.runtime.get_continuations(vec![channel.clone()]).await
    }

    /// The finalized state of the chain, as the mirror reconciler reads it (P6 step 7c). It
    /// shares the history of the chain, so it reads every root that the chain checkpointed.
    pub fn finalized_state(&self) -> Arc<ChainState> {
        Arc::new(ChainState {
            space: self.space.clone(),
            roots: Mutex::new(BTreeMap::new()),
        })
    }

    /// The volatile channels in the hot store of the chain's runtime (P7, design v3 VR-2). The
    /// space of the chain shares the hot store of its runtime.
    pub async fn residue(&self) -> Vec<Par> {
        hot_store_residue(&self.space.get_store().snapshot(), Durability::Ephemeral)
    }

    /// Injects `term` into the runtime of the chain through `RhoRuntime::inj`, as the
    /// exploratory deploys of Casper do: an evaluation without a deploy result.
    pub async fn explore(&self, term: &str) -> Result<(), InterpreterError> {
        let parsed = Compiler::source_to_adt(term).expect("the program parses");
        self.runtime.inj(parsed, Env::new(), random()).await
    }
}

/// The finalized state of a [`NativeChain`]: a runtime at any root of the chain, as the runtime
/// manager of a node spawns it, and the post-state root of each block that a test finalizes.
pub struct ChainState {
    space: PlaySpace,
    roots: Mutex<BTreeMap<Vec<u8>, Blake2b256Hash>>,
}

impl ChainState {
    /// Records the block `block_hash` with the post-state root `root`.
    pub fn finalize(&self, block_hash: &[u8], root: &Blake2b256Hash) {
        self.roots
            .lock()
            .expect("the roots of the chain state")
            .insert(block_hash.to_vec(), root.clone());
    }
}

#[async_trait]
impl FinalizedState for ChainState {
    async fn post_state(&self, block_hash: &[u8]) -> Result<Option<Blake2b256Hash>, String> {
        Ok(self
            .roots
            .lock()
            .expect("the roots of the chain state")
            .get(block_hash)
            .cloned())
    }

    async fn runtime_at(&self, root: &Blake2b256Hash) -> Result<RhoRuntimeImpl, String> {
        let space = self.space.spawn().map_err(|error| error.to_string())?;
        let mut runtime = create_rho_runtime(
            space,
            Arc::new(default_mergeable_tags()),
            false,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await
        .map_err(|error| error.to_string())?;
        runtime
            .reset(root)
            .await
            .map_err(|error| error.to_string())?;
        Ok(runtime)
    }
}

// ─── Certification ─────────────────────────────────────────────────────────────────────────────

/// The budget recording and the operation journal of `played`, encoded into the evidence wire
/// at the v6 recording limits and decoded from it, as the proposer commits them and the
/// validator reads them.
pub fn committed_evidence(
    played: &Played,
) -> (NativeBudgetRecording, Arc<[NativeOperationRecord]>) {
    let recording = played
        .result
        .native_budget_recording
        .clone()
        .expect("native play records its budget");
    let journal = played
        .result
        .native_operation_recording
        .clone()
        .expect("native play records its operations");
    let (budget_bytes, journal_bytes) = evidence_bytes(played);
    let (decoded, decoded_journal) = decode_evidence(&budget_bytes, &journal_bytes);
    assert_eq!(decoded.used, recording.used, "the evidence keeps the usage");
    assert_eq!(
        decoded.session, recording.session,
        "the evidence keeps the session"
    );
    assert_eq!(
        decoded_journal.as_ref(),
        journal.as_ref(),
        "the evidence keeps the journal rows"
    );
    (decoded, decoded_journal)
}

/// The budget recording and the operation journal of `played`, encoded into the evidence wire
/// at the v6 recording limits, as the proposer commits them: the bytes of the budget recording
/// and the bytes of the journal.
pub fn evidence_bytes(played: &Played) -> (Vec<u8>, Vec<u8>) {
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    let journal = played
        .result
        .native_operation_recording
        .as_ref()
        .expect("native play records its operations");
    let wire = recording_limits();
    let encoder = host_work();
    let budget_bytes = encode_native_budget_recording(recording, wire, &encoder)
        .expect("the budget recording fits the v6 evidence wire");
    let journal_bytes = encode_native_operation_journal(journal, &recording.paths, wire, &encoder)
        .expect("the operation journal fits the v6 evidence wire");
    (budget_bytes, journal_bytes)
}

/// The budget recording and the operation journal that a validator decodes from the evidence
/// bytes of [`evidence_bytes`].
pub fn decode_evidence(
    budget_bytes: &[u8],
    journal_bytes: &[u8],
) -> (NativeBudgetRecording, Arc<[NativeOperationRecord]>) {
    let wire = recording_limits();
    let decoder = host_work();
    let mut decoded = decode_native_budget_recording(budget_bytes, wire, &decoder)
        .expect("the budget recording decodes");
    let journal = decode_native_operation_journal(journal_bytes, &mut decoded, wire, &decoder)
        .expect("the operation journal decodes");
    (decoded, journal)
}

/// The native replay environment of `played` over `journal`, at the v6 limits, before its
/// evaluation. Returns the rejection of the journal check or of the trace binding.
pub async fn replay_environment(
    played: &Played,
    limit: u64,
    recording: &NativeBudgetRecording,
    journal: Arc<[NativeOperationRecord]>,
) -> Result<NativeReplayEnvironment, String> {
    replay_environment_with(played, limit, recording, journal, host_work()).await
}

/// [`replay_environment`] with the host-work budget `host` of the certification.
pub async fn replay_environment_with(
    played: &Played,
    limit: u64,
    recording: &NativeBudgetRecording,
    journal: Arc<[NativeOperationRecord]>,
    host: HostWorkBudget,
) -> Result<NativeReplayEnvironment, String> {
    replay_environment_from(
        played.history.clone(),
        played.log.clone(),
        limit,
        recording,
        journal,
        host,
    )
    .await
}

/// [`replay_environment_with`] from the parts of a play that a validator holds: the history of
/// the pre-state and the event log of the block (P7: a process that did not play).
pub async fn replay_environment_from(
    history: RhoHistoryRepository,
    log: Vec<Event>,
    limit: u64,
    recording: &NativeBudgetRecording,
    journal: Arc<[NativeOperationRecord]>,
    host: HostWorkBudget,
) -> Result<NativeReplayEnvironment, String> {
    let trace = with_contract(limit, |contract| {
        contract.check_operation_journal(
            recording.session,
            recording,
            journal,
            journal_limits(),
            &host,
        )
    })
    .map_err(|error| format!("the journal check rejects the journal: {error}"))?
    .bind_trace(log.into(), trace_bind_limits(), &host)
    .map_err(|error| format!("the trace binding rejects the log: {error}"))?;
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, funding());
    budget
        .reset_for_native_execution(config(limit, host.clone()))
        .expect("the replay budget enters native execution");
    Ok(create_native_replay_env(
        trace,
        history,
        Arc::new(Box::new(Matcher)),
        host,
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
    )
    .await
    .expect("the native replay environment builds"))
}

/// The certification of a play: the result of native replay, the completeness check, the
/// completed usage and the exported post-state root.
pub struct Certification {
    pub evaluation: Result<EvaluateResult, InterpreterError>,
    pub complete: Result<(), String>,
    pub usage: Result<u64, String>,
    pub root: Result<Blake2b256Hash, String>,
}

/// Evaluates `term` in `environment` and finishes the certification.
pub async fn finish(environment: NativeReplayEnvironment, term: &str) -> Certification {
    finish_with(environment, term, random()).await
}

/// Evaluates `term` in `environment` with the random state `random` of its play, and finishes
/// the certification.
pub async fn finish_with(
    environment: NativeReplayEnvironment,
    term: &str,
    random: Blake2b512Random,
) -> Certification {
    finish_injected(environment, term, random, Injections::new()).await
}

/// [`finish_with`] for a term with the injected ids `injections` of its play (P7).
pub async fn finish_injected(
    mut environment: NativeReplayEnvironment,
    term: &str,
    random: Blake2b512Random,
    injections: Injections,
) -> Certification {
    let parsed =
        Compiler::source_to_adt_with_normalizer_env(term, injections).expect("the program parses");
    let evaluation = environment.evaluate(parsed, random).await;
    let complete = environment
        .check_complete()
        .await
        .map_err(|error| error.to_string());
    let usage = environment
        .completed_usage()
        .await
        .map_err(|error| error.to_string());
    let root = environment
        .export()
        .await
        .map(|exported| exported.root().clone())
        .map_err(|error| error.to_string());
    Certification {
        evaluation,
        complete,
        usage,
        root,
    }
}

/// Certifies `played` through its committed evidence, as validator replay does.
pub async fn certify(played: &Played, term: &str, limit: u64) -> Certification {
    certify_with(played, term, limit, host_work()).await
}

/// Certifies `played` through its committed evidence with the host-work budget `host`.
pub async fn certify_with(
    played: &Played,
    term: &str,
    limit: u64,
    host: HostWorkBudget,
) -> Certification {
    let (recording, journal) = committed_evidence(played);
    let environment = replay_environment_with(played, limit, &recording, journal, host)
        .await
        .expect("the committed evidence checks");
    finish_injected(
        environment,
        term,
        played.random.clone(),
        played.injections.clone(),
    )
    .await
}

/// Checks that `certification` reproduces `played`: the errors of play, a complete journal, the
/// phlo usage of play, the same authority events and byte observations, and the post-state root
/// of play.
pub fn assert_reproduces(label: &str, played: &Played, certification: &Certification) {
    let evaluation = certification
        .evaluation
        .as_ref()
        .unwrap_or_else(|error| panic!("{label}: native replay evaluates: {error:?}"));
    assert_eq!(evaluation.errors, played.result.errors, "{label}: errors");
    assert_eq!(
        certification.complete,
        Ok(()),
        "{label}: replay consumes every journal row of play"
    );
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    assert_eq!(
        certification.usage,
        Ok(recording.used),
        "{label}: replay uses the phlo of play"
    );
    assert_eq!(
        evaluation.native_phlo_usage, played.result.native_phlo_usage,
        "{label}: native phlo usage"
    );
    assert_eq!(
        evaluation.authority_events, played.result.authority_events,
        "{label}: authority events"
    );
    assert_eq!(
        evaluation.byte_observations.rows, played.result.byte_observations.rows,
        "{label}: byte observations"
    );
    assert_eq!(
        certification.root.as_ref(),
        Ok(&played.root),
        "{label}: replay reaches the post-state root of play"
    );
}

/// True when the certification rejects the play: native replay fails, its deploy fails, the
/// journal is incomplete, or the post-state root is not exported.
pub fn rejected(certification: &Certification) -> bool {
    match &certification.evaluation {
        Err(_) => true,
        Ok(evaluation) => {
            !evaluation.errors.is_empty()
                || certification.complete.is_err()
                || certification.root.is_err()
        }
    }
}

// ─── Event logs ────────────────────────────────────────────────────────────────────────────────

/// The tagged bincode bytes of an event log, as a block carries it to another process: `0` and
/// a COMM, or `1` and an IO event (P7, `reified_restart_rehydrate.rs`).
pub fn encode_log(log: &[Event]) -> Vec<u8> {
    let entries: Vec<(u8, Vec<u8>)> = log
        .iter()
        .map(|event| match event {
            Event::Comm(comm) => (0, bincode::serialize(comm).expect("serialize a COMM")),
            Event::IoEvent(io) => (1, bincode::serialize(io).expect("serialize an IO event")),
        })
        .collect();
    bincode::serialize(&entries).expect("serialize a log")
}

/// The event log of the bytes of [`encode_log`].
pub fn decode_log(bytes: &[u8]) -> Vec<Event> {
    let entries: Vec<(u8, Vec<u8>)> = bincode::deserialize(bytes).expect("decode a log");
    entries
        .into_iter()
        .map(|(tag, bytes)| match tag {
            0 => Event::Comm(bincode::deserialize(&bytes).expect("decode a COMM")),
            1 => Event::IoEvent(bincode::deserialize(&bytes).expect("decode an IO event")),
            other => panic!("unknown log entry tag {other}"),
        })
        .collect()
}

// ─── Journal rows ──────────────────────────────────────────────────────────────────────────────

/// The channels of the footprint of `row`, decoded from their bincode bytes.
pub fn footprint_channels(row: &NativeOperationRecord) -> Vec<Par> {
    row.footprint
        .iter()
        .map(|bytes| bincode::deserialize(bytes).expect("a footprint entry is a channel"))
        .collect()
}

/// True when `channel` is a channel of a native space: a Reified id other than an op channel.
pub fn is_space_channel(channel: &Par) -> bool {
    !matches!(
        namespace::reified_id_of(channel),
        None | Some(ReifiedId::Op(_))
    )
}

/// True when `row` is a staged space operation: its path ends with a sub-order segment
/// `(i, SUB_OPERATION_TAG)` (decision 19, D3).
pub fn is_space_row(recording: &NativeBudgetRecording, row: &NativeOperationRecord) -> bool {
    recording
        .paths
        .last_segment(row.occurrence.path)
        .is_some_and(|(_, tag)| tag == SUB_OPERATION_TAG)
}

/// The budget attempts that `row` links to: its introduction and the observation of its COMM.
pub fn row_attempts(row: &NativeOperationRecord) -> Vec<usize> {
    let mut attempts = Vec::with_capacity(2);
    if let NativeObservationLink::Attempt(index) = row.introduction {
        attempts.push(index);
    }
    if let Some(comm) = &row.comm {
        if let NativeObservationLink::Attempt(index) = comm.observation {
            attempts.push(index);
        }
    }
    attempts
}
