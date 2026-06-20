//! Canonical floor-state recursion — `FS(F)` as a function of the finalized cut.
//!
//! The sealed finalized state for a finalized cut F is
//! `FS(F) = merge(closure(F) \ closure(prev(F))  onto  FS(prev(F)))`,
//! where `prev(F)` is the justification-derived floor of F ([`floor_of_block`]) —
//! NOT the node-local finalized frontier. The floor is a pure function of F's signed
//! justifications plus immutable ancestor metadata, so every node seals F from the
//! same predecessor and `FS` is node-identical by construction. The floor lags F by
//! the witnessing depth, so the recursion strictly descends to genesis (whose floor
//! is itself, the terminal cut).
//!
//! Each step folds a newly finalized cone onto its predecessor's state, down to
//! genesis (whose post-state IS its finalized state). Because the value is a
//! function of the cut, the read path persists on a miss (write-through): there
//! is no separate "seal at finalization" mechanism to keep consistent with.
//!
//! The seal base MUST NOT key on node-local `is_finalized`: under finalization lag
//! (e.g. contention) two nodes finalize the same floor F to different local frontiers,
//! so an `is_finalized`-keyed predecessor walk seals F from different cuts. The
//! base-dependent recovery dedup (`recovered_deploy_effect_in_base` reads the running
//! base) then skips different re-proposals on each path and folds a divergent `FS` —
//! the verified #71 cascade (divergent FS -> divergent multi-parent pre-state ->
//! InvalidTransaction -> slash -> finalization stall). Chaining on the justification
//! floor removes that node-local input entirely.

use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::floor_data::FloorData;
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::state_change_merger;

use super::floor::{floor_of_block, Floor};
use crate::rust::errors::CasperError;
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
        cursor = floor_of_block(dag, &cursor, ft_threshold).await?.hash;
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
    // Used to derive the node-identical justification floor of `cut` — the seal base.
    ft_threshold: f32,
) -> Result<FloorData, CasperError> {
    let cut_number = dag.block_number_unsafe(cut)?;
    let prev_hash = floor_of_block(dag, cut, ft_threshold).await?.hash;
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

    // FS(cut) = the deterministic op-fold of the newly-finalized cone onto
    // FS(prev_finalized_cut) (the #71 fix). A keep-one merge would drop concurrent
    // writes to a single-value cell (e.g. the PoS state cell); instead each finalized
    // block's COMMITTED diff is folded onto the running FS in topological order — number
    // channels by their commutative delta, non-foldable Map/Set/Int cells by a recursive
    // structural 3-way merge — so every co-finalized concurrent write is preserved.
    // Folding committed diffs is pure trie work (no reducer), so FS is node-identical
    // across nodes. FS is the canonical finalized state every consumer reads; it
    // intentionally differs from a block's committed (lagging-floor) post-state. User and
    // system deploy chains are both folded; per-signature dedup (the "flip") and the
    // cross-cut base-check keep each finalized effect applied exactly once (see below).
    let mut numbered: Vec<(i64, BlockHash)> = scope
        .iter()
        .map(|h| Ok::<_, CasperError>((dag.block_number_unsafe(h)?, h.clone())))
        .collect::<Result<Vec<_>, _>>()?;
    numbered.sort_by(|(na, ha), (nb, hb)| na.cmp(nb).then_with(|| ha.cmp(hb)));

    // ── Read-only diff probe (f1r3.trace.seal_diff) ──────────────────────────────
    // For each cone block, dump every NON-foldable datum-channel change (the value
    // cells the seal cannot delta-fold via a tagged number channel) as fingerprints
    // of the removed/added datums, alongside the seal-base value at that channel.
    // Two co-finalized writers carrying the SAME `removed` fingerprint as `base`
    // prove a whole-value diff-apply would stale-consume the second (the structural
    // delta is then mandatory). Purely diagnostic; mutates nothing.
    {
        let fp = |bytes: &[u8]| -> String {
            hex::encode(&Blake2b256Hash::new(bytes).bytes()[..6])
        };
        let base_hash = Blake2b256Hash::from_bytes_prost(&prev_state.state_hash.0);
        let base_reader = runtime_manager
            .history_repo
            .get_history_reader(&base_hash)
            .map_err(CasperError::HistoryError)?;
        for (block_number, block_hash) in &numbered {
            let idx = match crate::rust::util::rholang::interpreter_util::build_block_index(
                runtime_manager,
                block_store,
                block_hash,
            ) {
                Ok(idx) => idx,
                Err(e) => {
                    tracing::debug!(target: "f1r3.trace.seal_diff", block_number, error = %e, "probe: block index unavailable");
                    continue;
                }
            };
            for chain in &idx.deploy_chains {
                for e in chain.state_changes.datums_changes.iter() {
                    let ch = e.key();
                    if chain.event_log_index.number_channels_data.contains_key(ch) {
                        continue; // foldable (tagged number channel) — not the value-cell case
                    }
                    let change = e.value();
                    if change.removed.is_empty() && change.added.is_empty() {
                        continue;
                    }
                    let removed: Vec<String> = change.removed.iter().map(|d| fp(d)).collect();
                    let added: Vec<String> = change.added.iter().map(|d| fp(d)).collect();
                    let base_vals: Vec<String> = base_reader
                        .get_data_proj_binary(ch)
                        .map(|v| v.iter().map(|d| fp(d)).collect())
                        .unwrap_or_default();
                    tracing::debug!(
                        target: "f1r3.trace.seal_diff",
                        event = "seal_diff_probe",
                        cut = %PrettyPrinter::build_string_bytes(cut),
                        block_number,
                        channel = %hex::encode(&ch.bytes()[..6]),
                        removed = %removed.join(","),
                        added = %added.join(","),
                        base = %base_vals.join(","),
                        "non-foldable datum-channel diff vs seal base"
                    );
                }
            }
        }
    }

    // FS(cut) is the deterministic op-fold of the finalized cone: PLAY each finalized
    // deploy in topological order onto the running FS. Sequencing turns concurrent
    // writes into a linear chain, so it folds with NO content-awareness — commutative
    // number channels fold their commutative delta, non-foldable Map/Set/Int cells
    // fold STRUCTURALLY (recursive 3-way merge deriving the key/element/arithmetic
    // delta), so co-finalized concurrent writes are preserved instead of dropped or
    // stale-consumed. Determinism comes from applying COMMITTED diffs (pure trie ops,
    // no reducer). Two dedups keep each finalized effect applied EXACTLY ONCE:
    //   - `played_sigs`: a sig kept in more than one block in THIS cone (the "flip").
    //   - the base-check: a recovery re-proposal whose effect is already folded into
    //     FS(prev_cut) from an EARLIER cut. Structural map/set merges are idempotent,
    //     but Int-delta folds are not, so the check still prevents a cross-cut
    //     double-count (e.g. a numeric `+N` counted twice).
    let mut played_sigs: HashSet<prost::bytes::Bytes> = HashSet::new();
    let mut fs_state: StateHash = prev_state.state_hash.0.clone();
    let mut chains_applied = 0usize;
    let mut chains_skipped = 0usize;
    for (_block_number, block_hash) in &numbered {
        let idx = crate::rust::util::rholang::interpreter_util::build_block_index(
            runtime_manager,
            block_store,
            block_hash,
        )?;
        for chain in &idx.deploy_chains {
            let sigs: Vec<prost::bytes::Bytes> = chain
                .deploys_with_cost
                .0
                .iter()
                .map(|d| d.deploy_id.clone())
                .collect();
            // Within-cut dedup: a deploy-chain whose sig was already folded (the "flip":
            // the same deploy kept in more than one cone block).
            if sigs.iter().any(|s| played_sigs.contains(s)) {
                chains_skipped += 1;
                continue;
            }
            // Cross-cut dedup: a recovery re-proposal whose effect is already folded into
            // FS from an earlier cut. Value-cell merges are idempotent, but Int-delta
            // folds are not, so the session-20 base-check still matters here.
            let base_hash = Blake2b256Hash::from_bytes_prost(&fs_state);
            let already_in_base = sigs.iter().any(|sig| {
                !crate::rust::system_deploy::is_system_deploy_id(sig)
                    && crate::rust::util::rholang::interpreter_util::recovered_deploy_effect_in_base(
                        dag,
                        block_store,
                        runtime_manager,
                        &base_hash,
                        sig,
                    )
                    .unwrap_or(false)
            });
            if already_in_base {
                chains_skipped += 1;
                continue;
            }

            // Apply this chain's COMMITTED diff onto the running FS via the trie
            // machinery: foldable number channels fold through the merge primitive;
            // non-foldable Map/Set/Int cells fold structurally (recursive 3-way merge,
            // preserving co-finalized concurrent writes); everything else applies its
            // recorded remove/add through `make_trie_action` (stale-consume backstop).
            let base_reader = std::sync::Arc::new(
                runtime_manager
                    .history_repo
                    .get_history_reader(&base_hash)
                    .map_err(CasperError::HistoryError)?,
            );
            let reader_for_fold = std::sync::Arc::clone(&base_reader);
            let actions = state_change_merger::compute_trie_actions(
                &chain.state_changes,
                &*base_reader,
                &chain.event_log_index.number_channels_data,
                move |hash: &Blake2b256Hash, channel_changes, number_chs| {
                    if let Some(number_ch_val) = number_chs.get(hash) {
                        let (diff, merge_type) = *number_ch_val;
                        let base_get_data = |h: &Blake2b256Hash| reader_for_fold.get_data(h);
                        Ok(Some(RholangMergingLogic::calculate_number_channel_merge(
                            hash,
                            diff,
                            merge_type,
                            channel_changes,
                            base_get_data,
                        )?))
                    } else {
                        let base_get_data = |h: &Blake2b256Hash| reader_for_fold.get_data(h);
                        RholangMergingLogic::calculate_map_channel_merge(
                            hash,
                            channel_changes,
                            base_get_data,
                        )
                    }
                },
            )
            .map_err(CasperError::HistoryError)?;
            let new_root = runtime_manager
                .history_repo
                .reset(&base_hash)
                .map(|repo| repo.do_checkpoint(actions))
                .map(|checkpoint| checkpoint.root())
                .map_err(CasperError::HistoryError)?;
            fs_state = prost::bytes::Bytes::copy_from_slice(&new_root.bytes());
            for s in sigs {
                played_sigs.insert(s);
            }
            chains_applied += 1;
        }
    }

    // The seal no longer keep-one rejects anything — every finalized deploy is played —
    // so the rejected/accepted ledgers carry forward unchanged. They are
    // retained-but-unconsumed (the FloorFateResolver that read them was removed); a
    // follow-up cleanup can drop them from the FloorData model.
    let committed_state_bytes = block_store
        .get_unsafe(cut)
        .body
        .state
        .post_state_hash
        .clone();
    if committed_state_bytes != fs_state {
        tracing::debug!(
            target: "f1r3.trace.fs_floor",
            event = "seal_state_divergence",
            cut = %PrettyPrinter::build_string_bytes(cut),
            cut_number,
            committed = %PrettyPrinter::build_string_bytes(&committed_state_bytes),
            fs = %PrettyPrinter::build_string_bytes(&fs_state),
            "FS (deterministic seal re-execution) differs from committed block post-state"
        );
    }

    tracing::debug!(
        target: "f1r3.trace.fs_floor",
        event = "seal_result",
        cut = %PrettyPrinter::build_string_bytes(cut),
        cut_number,
        prev_floor = %PrettyPrinter::build_string_bytes(&prev.hash),
        prev_fs_block = prev.block_number,
        fs_state = %PrettyPrinter::build_string_bytes(&fs_state),
        cone_blocks = numbered.len(),
        chains_applied,
        chains_skipped,
        "sealed floor state (deterministic structural diff-fold of the finalized cone)"
    );

    Ok(FloorData {
        state_hash: StateHashSerde(fs_state),
        rejected_deploys: prev_state.rejected_deploys.clone(),
        accepted_deploys: prev_state.accepted_deploys.clone(),
        block_number: cut_number,
    })
}
