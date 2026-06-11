//! Canonical floor-state recursion — `FS(F)` as a pure function of the cut.
//!
//! The sealed finalized state for a floor F is
//! `FS(F) = merge(closure(F) \ closure(floor(F))  onto  FS(floor(F)))`,
//! where `floor(F)` is F's own justification-derived floor
//! ([`super::floor::floor_of_block`]). The predecessor cut is a block-structural
//! fact — never the node's previous LFB — so there is exactly ONE recursion path
//! from any floor down to genesis, and `FS(F)` is bit-identical on every node
//! whether it is materialized at finalization time or on a read miss. (The
//! prior experiment's seal folded one step from the node-local LFB while its
//! read path folded from arbitrary anchors; the divergence between those two
//! folds was the verified FS path-dependence root cause.)
//!
//! Because the value is a pure function of the cut, the read path persists on a
//! miss (write-through): there is no separate "seal at finalization" mechanism
//! to keep consistent with.

use std::collections::{BTreeSet, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::floor_data::FloorData;
use models::rust::block::state_hash::StateHashSerde;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::floor::{floor_of_block, Floor};
use crate::rust::errors::CasperError;
use crate::rust::merging::dag_merger;
use crate::rust::system_deploy::is_system_deploy_id;
use crate::rust::util::rholang::interpreter_util::build_block_index;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Build the enforcement window for a merge based at `base_floor`.
///
/// Every chain in `scope` executed against `FS(floor(X))` of its own source
/// block X, which can lag the merge base. `F*` — the minimum floor across the
/// scope — bounds everything any scope chain could have missed: finalized
/// chains in `closure(base_floor) \ closure(F*)` are the potential
/// counterparties (this also covers straddler cones, whose own floors sit at
/// or below their fork region). Window chains are partitioned by the sealed
/// `rejected_deploys` record into accepted (conflict counterparties +
/// duplicate-sig sources) and rejected (depends-sources).
///
/// Empty in the common case where every scope block already built on
/// `base_floor`.
pub async fn enforcement_window(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    base_floor: &Floor,
    base_fs: &FloorData,
    scope: &HashSet<BlockHash>,
    ft_threshold: f32,
) -> Result<dag_merger::FinalContext, CasperError> {
    let mut f_star: Floor = base_floor.clone();
    for hash in scope {
        let block_floor = floor_of_block(dag, hash, ft_threshold).await?;
        let lower = block_floor.block_number < f_star.block_number
            || (block_floor.block_number == f_star.block_number && block_floor.hash < f_star.hash);
        if lower {
            f_star = block_floor;
        }
    }

    if f_star.hash == base_floor.hash {
        return Ok(dag_merger::FinalContext {
            accepted_chains: Vec::new(),
            rejected_chains: Vec::new(),
            enforce_sigs: HashSet::new(),
        });
    }

    let f_star_closure = floor_closure(dag, runtime_manager, &f_star.hash)?;
    let mut window: HashSet<BlockHash> =
        dag.ancestors(base_floor.hash.clone(), |h| !f_star_closure.contains(h))?;
    window.insert(base_floor.hash.clone());
    window.retain(|h| !f_star_closure.contains(h));

    let mut window_sorted: Vec<BlockHash> = window.into_iter().collect();
    window_sorted.sort();

    let rejected_sigs: HashSet<prost::bytes::Bytes> =
        base_fs.rejected_deploys.iter().cloned().collect();
    let mut accepted_chains = Vec::new();
    let mut rejected_chains = Vec::new();
    let mut enforce_sigs: HashSet<prost::bytes::Bytes> = HashSet::new();
    for block in &window_sorted {
        for chain in build_block_index(runtime_manager, block_store, block)?.deploy_chains {
            let seal_rejected = chain
                .deploys_with_cost
                .0
                .iter()
                .any(|deploy| rejected_sigs.contains(&deploy.deploy_id));
            if seal_rejected {
                rejected_chains.push(chain);
            } else {
                for deploy in chain.deploys_with_cost.0.iter() {
                    if !is_system_deploy_id(&deploy.deploy_id) {
                        enforce_sigs.insert(deploy.deploy_id.clone());
                    }
                }
                accepted_chains.push(chain);
            }
        }
    }

    tracing::debug!(
        target: "f1r3.trace.fs_floor",
        event = "enforcement_window",
        base_floor = %PrettyPrinter::build_string_bytes(&base_floor.hash),
        base_floor_number = base_floor.block_number,
        f_star = %PrettyPrinter::build_string_bytes(&f_star.hash),
        f_star_number = f_star.block_number,
        window_blocks = window_sorted.len(),
        accepted_chains = accepted_chains.len(),
        rejected_chains = rejected_chains.len(),
        enforce_sigs = enforce_sigs.len(),
        "built finalized enforcement window"
    );

    Ok(dag_merger::FinalContext {
        accepted_chains,
        rejected_chains,
        enforce_sigs,
    })
}

/// Materialized `closure(floor)` — the floor block with all its ancestors —
/// through the RuntimeManager cache: one BFS per floor advance, shared by the
/// seal and the stage-2 conflict-scope computation.
pub fn floor_closure(
    dag: &KeyValueDagRepresentation,
    runtime_manager: &RuntimeManager,
    floor_hash: &BlockHash,
) -> Result<std::sync::Arc<HashSet<BlockHash>>, CasperError> {
    if let Some(cached) = runtime_manager.get_cached_floor_closure(floor_hash) {
        return Ok(cached);
    }
    let closure = std::sync::Arc::new(dag.with_ancestors(floor_hash.clone(), |_| true)?);
    tracing::debug!(
        target: "f1r3.trace.fs_floor",
        event = "floor_closure_materialized",
        floor = %PrettyPrinter::build_string_bytes(floor_hash),
        size = closure.len(),
        "materialized floor closure"
    );
    runtime_manager.put_cached_floor_closure(floor_hash.clone(), closure.clone());
    Ok(closure)
}

/// `FS(floor_hash)`, from the store when present, else computed by the
/// canonical recursion and persisted write-through.
///
/// The descent resolves the floor-of-floor chain down to the nearest stored
/// floor state (terminating at genesis, whose post-state IS its finalized
/// state), then folds back up sealing one cut at a time. Every step of both
/// directions is a pure function of block-structural facts, so concurrent or
/// repeated computation of the same cut always stores the same value.
pub async fn floor_state_get_or_compute(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    floor_hash: &BlockHash,
    ft_threshold: f32,
) -> Result<FloorData, CasperError> {
    if let Some(stored) = dag.get_floor_state(floor_hash)? {
        tracing::trace!(
            target: "f1r3.trace.fs_floor",
            event = "fs_hit",
            floor = %PrettyPrinter::build_string_bytes(floor_hash),
            fs_block = stored.block_number,
            "floor state store hit"
        );
        return Ok(stored);
    }

    // Descend the floor-of-floor chain collecting cuts with no stored state,
    // until a stored state (or genesis) provides the fold base.
    let mut unsealed: Vec<BlockHash> = Vec::new();
    let mut cursor = floor_hash.clone();
    let base = loop {
        if let Some(stored) = dag.get_floor_state(&cursor)? {
            break stored;
        }
        let metadata = dag.lookup_unsafe(&cursor)?;
        if metadata.parents.is_empty() {
            // Genesis terminal: its post-state IS the finalized state.
            let genesis = block_store.get_unsafe(&cursor);
            let seed = FloorData {
                state_hash: StateHashSerde(genesis.body.state.post_state_hash.clone()),
                rejected_deploys: Vec::new(),
                block_number: genesis.body.state.block_number,
            };
            dag.put_floor_state(cursor.clone(), seed.clone())?;
            tracing::debug!(
                target: "f1r3.trace.fs_floor",
                event = "fs_genesis_seed",
                genesis = %PrettyPrinter::build_string_bytes(&cursor),
                "seeded floor state at genesis post-state"
            );
            break seed;
        }
        unsealed.push(cursor.clone());
        cursor = floor_of_block(dag, &cursor, ft_threshold).await?.hash;
    };

    if !unsealed.is_empty() {
        tracing::info!(
            target: "f1r3.trace.fs_floor",
            event = "fs_fold",
            target_floor = %PrettyPrinter::build_string_bytes(floor_hash),
            base = %PrettyPrinter::build_string_bytes(&cursor),
            base_fs_block = base.block_number,
            fold_depth = unsealed.len(),
            "floor state miss; folding up the floor chain"
        );
    }

    // Fold back up, sealing each cut onto its predecessor's state.
    let mut state = base;
    for cut in unsealed.into_iter().rev() {
        state = seal_floor_cut(
            dag,
            block_store,
            runtime_manager,
            &cut,
            &state,
            ft_threshold,
        )
        .await?;
        dag.put_floor_state(cut, state.clone())?;
    }
    Ok(state)
}

/// Seal one floor cut: merge the newly finalized cone
/// `closure(cut) \ closure(floor(cut))` onto `FS(floor(cut))`.
///
/// `prev_state` must be the floor state of `floor(cut)` — the caller resolves
/// the chain; this function performs exactly one canonical step. Rejection
/// decisions made by the seal accumulate into `rejected_deploys` (deduplicated,
/// ordered), so a deploy rejected at any cut stays enforceable at later cuts.
async fn seal_floor_cut(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    cut: &BlockHash,
    prev_state: &FloorData,
    ft_threshold: f32,
) -> Result<FloorData, CasperError> {
    let cut_number = dag.block_number_unsafe(cut)?;
    let prev = floor_of_block(dag, cut, ft_threshold).await?;

    // scope = closure(cut) \ closure(prev): the newly finalized cone. The
    // predecessor closure is downward-closed, so gating the ancestor walk on
    // non-membership yields exactly the difference.
    let prev_closure = floor_closure(dag, runtime_manager, &prev.hash)?;
    let mut scope: HashSet<BlockHash> =
        dag.ancestors(cut.clone(), |h| !prev_closure.contains(h))?;
    scope.insert(cut.clone());
    scope.retain(|h| !prev_closure.contains(h));

    if scope.is_empty() {
        // floor(cut) is a strict ancestor of cut, so the cut itself is always
        // in the difference; an empty scope means the DAG indices are corrupt.
        return Err(CasperError::Other(format!(
            "floor seal invariant violated: empty seal scope for cut {} (#{}) over floor {} (#{})",
            PrettyPrinter::build_string_bytes(cut),
            cut_number,
            PrettyPrinter::build_string_bytes(&prev.hash),
            prev.block_number,
        )));
    }

    tracing::debug!(
        target: "f1r3.trace.fs_floor",
        event = "seal_entry",
        cut = %PrettyPrinter::build_string_bytes(cut),
        cut_number,
        prev_floor = %PrettyPrinter::build_string_bytes(&prev.hash),
        prev_fs_block = prev_state.block_number,
        base_state = %PrettyPrinter::build_string_bytes(&prev_state.state_hash.0),
        scope = scope.len(),
        "sealing newly finalized cone onto FS(floor(cut))"
    );

    // Finalized decisions made at-or-below the previous floor are enforced on
    // the seal too: scope chains may have executed before those decisions.
    let final_context = enforcement_window(
        dag,
        block_store,
        runtime_manager,
        &prev,
        prev_state,
        &scope,
        ft_threshold,
    )
    .await?;

    let base_state = Blake2b256Hash::from_bytes_prost(&prev_state.state_hash.0);
    let (sealed_state, rejected_user, _rejected_slash) = dag_merger::merge(
        dag,
        &prev.hash,
        &base_state,
        |hash: &BlockHash| Ok(build_block_index(runtime_manager, block_store, hash)?.deploy_chains),
        &runtime_manager.history_repo,
        dag_merger::cost_optimal_rejection_alg(),
        Some(scope),
        // The scope IS the explicit finalized closure; late-block filtering is
        // a live-view concept and must not participate.
        true,
        Some(&final_context),
    )?;

    let mut rejected: BTreeSet<prost::bytes::Bytes> =
        prev_state.rejected_deploys.iter().cloned().collect();
    let carried = rejected.len();
    rejected.extend(rejected_user.into_iter().map(|(sig, _src)| sig));
    let sealed_state_bytes = sealed_state.to_bytes_prost();

    tracing::info!(
        target: "f1r3.trace.fs_floor",
        event = "seal_result",
        cut = %PrettyPrinter::build_string_bytes(cut),
        cut_number,
        fs_state = %PrettyPrinter::build_string_bytes(&sealed_state_bytes),
        rejected_total = rejected.len(),
        rejected_carried = carried,
        "sealed floor state"
    );

    Ok(FloorData {
        state_hash: StateHashSerde(sealed_state_bytes),
        rejected_deploys: rejected.into_iter().collect(),
        block_number: cut_number,
    })
}
