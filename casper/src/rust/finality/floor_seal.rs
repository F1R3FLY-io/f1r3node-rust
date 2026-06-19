//! Canonical floor-state recursion — `FS(F)` as a function of the finalized cut.
//!
//! The sealed finalized state for a finalized cut F is
//! `FS(F) = merge(closure(F) \ closure(prev(F))  onto  FS(prev(F)))`,
//! where `prev(F)` is the previous finalized cut below F on its main-parent
//! chain ([`previous_finalized_cut`]) — the actual finalized frontier, not the
//! lagging justification floor. Chaining the seal on the finalized frontier puts
//! every write finalized at-or-below `prev(F)` into the base, so the seal never
//! re-litigates an already-finalized write and `FS` grows monotonically.
//!
//! Each step folds a newly finalized cone onto its predecessor's state, down to
//! genesis (whose post-state IS its finalized state). Because the value is a
//! function of the cut, the read path persists on a miss (write-through): there
//! is no separate "seal at finalization" mechanism to keep consistent with.
//!
//! Open question: `previous_finalized_cut` keys on node-local `is_finalized`, so
//! whether the resulting predecessor (and thus `FS`) is node-identical is not yet
//! settled — tracked in notes/merge-lifecycle.md, separate from this module's
//! recursion logic.

use std::collections::{BTreeSet, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::floor_data::{FloorData, SealedAcceptance, SealedRejection};
use models::rust::block::state_hash::StateHashSerde;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::floor::Floor;
use crate::rust::errors::CasperError;
use crate::rust::merging::dag_merger;
use crate::rust::util::rholang::interpreter_util::build_block_index;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

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

/// The previous finalized cut below `cut` on its main-parent chain — the base the
/// seal chains onto.
///
/// Unlike `floor_of_block` (justification-derived, lagging the cut by the witnessing
/// depth), this is the actual finalized frontier below `cut`. Chaining the seal here
/// puts every write finalized at-or-below it INTO the base, so the seal never
/// re-litigates an already-finalized write — `FS` grows monotonically (the #71 fix).
///
/// Node-identical: every `FS` read is of a finalized cut (`floor ≤ LFB`), and
/// finalization advances the whole cone, so when `cut` is finalized every finalized
/// ancestor is already finalized. The main-parent walk to the first `is_finalized`
/// block therefore yields the same predecessor on every node. Genesis (no main
/// parent) is the terminal cut.
fn previous_finalized_cut(
    dag: &KeyValueDagRepresentation,
    cut: &BlockHash,
) -> Result<BlockHash, CasperError> {
    const DEEP_WALK_WARN: usize = 256;
    let mut current = cut.clone();
    let mut walked: usize = 0;
    loop {
        match dag.main_parent(&current) {
            Some(parent) => {
                if dag.is_finalized(&parent) {
                    return Ok(parent);
                }
                current = parent;
                walked += 1;
                if walked == DEEP_WALK_WARN {
                    tracing::warn!(
                        target: "f1r3.trace.fs_floor",
                        cut = %PrettyPrinter::build_string_bytes(cut),
                        walked,
                        "previous_finalized_cut walk unusually deep; finalization lagging or cold start"
                    );
                }
            }
            None => {
                // `current` is genesis: the terminal finalized cut.
                return Ok(current);
            }
        }
    }
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
                accepted_deploys: Vec::new(),
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
        cursor = previous_finalized_cut(dag, &cursor)?;
    };

    if !unsealed.is_empty() {
        tracing::debug!(
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
    // Vestigial: the seal operates on already-finalized cuts (previous_finalized_cut
    // walks is_finalized; the merge needs no FT), so no fault tolerance is computed here.
    _ft_threshold: f32,
) -> Result<FloorData, CasperError> {
    let cut_number = dag.block_number_unsafe(cut)?;
    let prev_hash = previous_finalized_cut(dag, cut)?;
    let prev = Floor {
        block_number: dag.block_number_unsafe(&prev_hash)?,
        hash: prev_hash,
    };

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

    // Every seal-scope block's mergeable entry must be loadable before the
    // merge builds indices; recompute any this node never replayed.
    crate::rust::util::rholang::interpreter_util::ensure_scope_mergeable_present(
        block_store,
        runtime_manager,
        dag,
        &scope,
    )
    .await?;

    // No enforcement window. The base is FS(prev_finalized_cut), so every decision
    // finalized at-or-below `prev` is already baked into the base; the scope is only
    // the newly-finalized delta. keep-one + single-value serialization on the sealed
    // base resolve the delta, and a delta chain that depends on a finalized-rejected
    // chain hits a stale-consume and is dropped — so the enforcement window is
    // redundant here, and omitting it avoids its over-rejection (the Step-4 hazard)
    // now that the re-merge result IS FS.
    let base_state = Blake2b256Hash::from_bytes_prost(&prev_state.state_hash.0);
    let (sealed_state, applied_user, rejected_user, _rejected_slash) = dag_merger::merge(
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
        None,
    )?;

    // DIAG (mechanism discriminator): a kept chain whose sig is ALREADY in
    // FS(prev).accepted is re-applying a finalized deploy's effect — the
    // content-twin source if this is mechanism 1 (recovery re-proposing an
    // already-accepted deploy). Silence ⇒ the double is a distinct-deploy
    // re-create (mechanism 2).
    {
        let prev_accepted: HashSet<prost::bytes::Bytes> = prev_state
            .accepted_deploys
            .iter()
            .map(|a| a.sig.clone())
            .collect();
        let reapplied: Vec<String> = applied_user
            .iter()
            .filter(|(sig, _)| prev_accepted.contains(sig))
            .map(|(sig, _)| hex::encode(&sig[..sig.len().min(8)]))
            .collect();
        if !reapplied.is_empty() {
            tracing::warn!(
                target: "f1r3.trace.fs_floor",
                event = "seal_reapply",
                cut_number,
                reapplied = %reapplied.join(","),
                "seal kept chains re-apply already-FS-accepted deploys (content-twin source)"
            );
        }
    }

    let mut rejected: BTreeSet<SealedRejection> =
        prev_state.rejected_deploys.iter().cloned().collect();
    let carried = rejected.len();
    rejected.extend(rejected_user.into_iter().map(|(sig, src)| SealedRejection {
        sig,
        host: BlockHashSerde(src),
    }));

    // Accumulate kept-chain acceptances the same way: a deploy accepted into FS
    // at any cut stays accepted at every later cut. This ledger is retained but
    // no longer consumed — the FloorFateResolver that read it was removed in the
    // buffer-drain change; exactly-once is now the recovery-buffer invariant
    // (purge-on-accept) plus `Validate::repeat_deploy`. Kept here to avoid a
    // serialized-FloorData model change; remove in the follow-up cleanup.
    let mut accepted: BTreeSet<SealedAcceptance> =
        prev_state.accepted_deploys.iter().cloned().collect();
    let accepted_carried = accepted.len();
    accepted.extend(applied_user.into_iter().map(|(sig, src)| SealedAcceptance {
        sig,
        host: BlockHashSerde(src),
    }));
    // FS(cut) is the chained re-merge: the newly-finalized delta merged onto
    // FS(prev_finalized_cut). Because the base is the finalized frontier (not the
    // lagging justification floor), the re-merge never re-litigates an already-
    // finalized write, so FS grows monotonically — a finalized entry is never
    // dropped by a descendant cut (the #71 fix). The block's own committed
    // post-state may differ (it merged on its lagging floor); that is the block's
    // working state, while FS is the canonical finalized state every consumer reads.
    let remerge_state_bytes = sealed_state.to_bytes_prost();
    let committed_state_bytes = block_store
        .get_unsafe(cut)
        .body
        .state
        .post_state_hash
        .clone();

    if committed_state_bytes != remerge_state_bytes {
        // Diagnostic: FS (chained on the finalized frontier) intentionally differs
        // from the block's committed post-state (merged on the lagging floor).
        tracing::debug!(
            target: "f1r3.trace.fs_floor",
            event = "seal_state_divergence",
            cut = %PrettyPrinter::build_string_bytes(cut),
            cut_number,
            committed = %PrettyPrinter::build_string_bytes(&committed_state_bytes),
            fs = %PrettyPrinter::build_string_bytes(&remerge_state_bytes),
            "FS (chained finalized seal) differs from committed block post-state"
        );
    }

    tracing::debug!(
        target: "f1r3.trace.fs_floor",
        event = "seal_result",
        cut = %PrettyPrinter::build_string_bytes(cut),
        cut_number,
        prev_floor = %PrettyPrinter::build_string_bytes(&prev.hash),
        prev_fs_block = prev.block_number,
        fs_state = %PrettyPrinter::build_string_bytes(&remerge_state_bytes),
        rejected_total = rejected.len(),
        rejected_carried = carried,
        accepted_total = accepted.len(),
        accepted_carried,
        "sealed floor state (chained on previous finalized cut)"
    );

    Ok(FloorData {
        state_hash: StateHashSerde(remerge_state_bytes),
        rejected_deploys: rejected.into_iter().collect(),
        accepted_deploys: accepted.into_iter().collect(),
        block_number: cut_number,
    })
}
