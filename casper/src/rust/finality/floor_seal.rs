//! Advance-only finalized-state ledger — `FS(B)` as a function of the finalized cut.
//!
//! The sealed finalized state for a finalized block B is built FORWARD from its
//! immediate main parent:
//! `FS(B) = apply(closure(B) \ closure(main_parent(B))  onto  FS(main_parent(B)))`,
//! recursing down the main-parent chain to genesis (whose post-state IS its finalized
//! state). Each step applies only the block's INCREMENTAL delta — its own block plus its
//! co-finalized secondary-parent subtrees — ACCEPTED inclusions only, once each.
//!
//! Why build-forward (not re-fold a lagging cone): every block enters exactly ONE delta
//! (the cut where it first joins the main-chain closure), so a finalized op is applied
//! once and then lives below every later main parent — never re-derived. `FS` is therefore
//! MONOTONE by construction: a value, once applied, cannot be dropped by a later cut (only
//! a finalized delete/slash op removes it). The prior design re-folded the whole
//! `closure(cut) \ closure(floor)` cone onto `FS(floor)` every cut, with a cone-rebuilt
//! skip set — so a larger cone could re-skip an already-applied inclusion. That stateless
//! re-derivation was the verified flicker (a finalized value present at one cut, gone at
//! the next). Advancing from the immediate parent removes the re-derivation entirely.
//!
//! Node-identity: `main_parent(B)` is a pure function of B's signed parents (NOT node-local
//! `is_finalized`), and the delta + accepted-only fold + dedup are pure functions of
//! `closure(B)`, with no reducer (recorded committed diffs only). So every node computes
//! the same `FS(B)`. This preserves the #71 guard — a node-local base would diverge
//! (different recovery dedup per node -> divergent FS -> divergent multi-parent pre-state ->
//! InvalidTransaction -> slash -> finalization stall); chaining on the main parent keeps the
//! base node-identical, exactly as the justification floor did, but without the lag that
//! forced the re-derivation.
//!
//! Because the value is a pure function of the cut, the read path persists on a miss
//! (write-through); there is no separate "seal at finalization" mechanism to keep
//! consistent with.

use std::collections::{HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::floor_data::{FloorData, SealedAcceptance, SealedRejection};
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::state_change_merger;

use super::floor::Floor;
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
        // Advance-only ledger: chain on the IMMEDIATE main parent (a node-identical block
        // fact), not the lagging justification floor. `FS(block) = FS(main_parent) ⊕` the
        // block's incremental delta, so a finalized value applied once is never re-derived
        // by a later cut (build-forward) — which is what removes the flicker. main_parent is
        // a pure function of the signed block, so the base stays node-identical (no
        // node-local `is_finalized` input; the #71 divergence guard holds).
        cursor = dag.main_parent(&cursor).ok_or_else(|| {
            CasperError::Other(format!(
                "main_parent missing for non-genesis cut {}",
                PrettyPrinter::build_string_bytes(&cursor),
            ))
        })?;
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

/// Advance one cut: apply the incremental delta `closure(cut) \ closure(main_parent(cut))`
/// onto `FS(main_parent(cut))`.
///
/// `prev_state` must be the finalized state of `main_parent(cut)` — the caller resolves the
/// main-parent chain; this function performs exactly one forward step. Because the delta is
/// only the blocks newly reachable through `cut` (its own block + co-finalized
/// secondary-parent subtrees), each finalized op is applied in exactly one step and never
/// re-derived, so the ledger is monotone.
/// Retention window for the FloorData reporting ledgers (accepted/rejected deploy
/// sigs). Past the deploy lifespan below the floor a deploy is terminally
/// finalized-or-expired, so its reporting entry is prunable. Generous so a still-
/// queryable deploy is never dropped early, while bounding the cumulative ledger so
/// it cannot grow without limit across cuts.
const LEDGER_RETENTION_BLOCKS: i64 = 1024;

async fn seal_floor_cut(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    cut: &BlockHash,
    prev_state: &FloorData,
    // Retained for signature stability; the advance-only base is the main parent, so the
    // justification-floor derivation (which needed the FT threshold) is no longer used here.
    _ft_threshold: f32,
) -> Result<FloorData, CasperError> {
    let cut_number = dag.block_number_unsafe(cut)?;
    // Advance-only base = the cut's main parent (node-identical), not its justification
    // floor. The delta `closure(cut) \ closure(main_parent)` is the cut's own block plus its
    // co-finalized secondary-parent subtrees — applied ACCEPTED-only, once, onto
    // FS(main_parent). Because every block enters exactly one such delta (the cut where it
    // first joins the main-chain closure), a finalized op is evaluated once and then lives
    // below every later main_parent — never re-folded, so FS is monotone by construction.
    let prev_hash = dag.main_parent(cut).ok_or_else(|| {
        CasperError::Other(format!(
            "main_parent missing for non-genesis cut {}",
            PrettyPrinter::build_string_bytes(cut),
        ))
    })?;
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
    if tracing::enabled!(target: "f1r3.trace.seal_diff", tracing::Level::DEBUG) {
        let fp = |bytes: &[u8]| -> String { hex::encode(&Blake2b256Hash::new(bytes).bytes()[..6]) };
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

    // FS(cut) = the advance-only op-fold of THIS cut's incremental delta onto FS(main_parent).
    // Apply each ACCEPTED finalized inclusion's committed diff once, in topological order:
    // number channels fold their commutative delta; non-foldable Map/Set/Int cells fold
    // STRUCTURALLY (recursive 3-way merge), preserving co-finalized concurrent writes. Pure
    // trie work (no reducer) ⇒ node-identical.
    //
    // Dedup keeps each finalized effect applied EXACTLY ONCE via three orthogonal checks
    // (restored from `2b9af7d6` — the session-36 persistent-ledger swap over-skipped and is
    // reverted; the sig-rng base-check is the cross-cut dedup, immune to the build-forward
    // cone/delta split because it reads the actual running FS, not carried verdicts):
    //   - `rejected_inclusions` (sig, host): construction keep-one'd losers in THIS cone —
    //     skip from that host; the effect lands via its ACCEPTED recovery inclusion.
    //   - `played_sigs`: within-cut dedup (the "flip": a sig kept in >1 cone block).
    //   - base-check (below): a recovery re-proposal whose sig-derived (pre-charge) cell is
    //     already in FS from an earlier cut — non-idempotent Int-delta folds would double-count.
    let mut rejected_inclusions: HashSet<(prost::bytes::Bytes, BlockHash)> = HashSet::new();
    for (_n, bh) in &numbered {
        for rd in &block_store.get_unsafe(bh).body.rejected_deploys {
            rejected_inclusions.insert((rd.sig.clone(), rd.host.clone()));
        }
    }
    let mut played_sigs: HashSet<prost::bytes::Bytes> = HashSet::new();
    // Reporting ledger: every sig whose effect we fold into FS this cut, cumulative
    // over cuts (carried prev ledger + this cut's applied chains), pruned below. This
    // is the authoritative effect-level deploy ledger that deploy_finalization_status
    // reads; the seal's DEDUP still uses the running-FS base-check above (not this
    // ledger), so populating it cannot reintroduce the persistent-ledger over-skip.
    let mut accepted_ledger: Vec<SealedAcceptance> = prev_state.accepted_deploys.clone();
    // CloseBlock is stamped on every block (count = DAG width), so concurrent co-finalized
    // siblings carry replica CloseBlocks. Apply one per height so its replicated epoch-reward
    // (committedRewards) and withdrawal-payout (posVault) effects are not folded once per sibling.
    let mut closeblock_heights: HashSet<i64> = HashSet::new();
    let mut fs_state: StateHash = prev_state.state_hash.0.clone();
    let mut chains_applied = 0usize;
    let mut chains_skipped = 0usize;
    // Flatten the cone's deploy chains and order them by the shared keep-one order (CloseBlock
    // PRIMARY so the non-recoverable epoch deploy wins, then block# producer-before-consumer,
    // then cost/hash/sigs). The fold below is gated by a whole-cell keep-one: the seal cone is
    // the UNION of all finalized blocks across forks, so it can hold concurrent writers to one
    // single-value cell that no single construction merge ever saw together (each finalized on
    // its own fork). Folding both splits a coupled entity (the orphan); instead apply ONE writer
    // per cell and route the rest to recovery, exactly as construction's serialize keep-one does
    // within a single merge.
    let mut ordered_chains: Vec<(
        i64,
        BlockHash,
        crate::rust::merging::deploy_chain_index::DeployChainIndex,
    )> = Vec::new();
    for (block_number, block_hash) in &numbered {
        let idx = crate::rust::util::rholang::interpreter_util::build_block_index(
            runtime_manager,
            block_store,
            block_hash,
        )?;
        for chain in idx.deploy_chains {
            ordered_chains.push((*block_number, block_hash.clone(), chain));
        }
    }
    ordered_chains.sort_by(|(_, _, a), (_, _, b)| {
        crate::rust::merging::dag_merger::serialize_keep_one_order(a, b)
    });

    // Foldable (mergeable number) channels compose via the dispatcher fold and are exempt from
    // the keep-one — only non-foldable single-value cells serialize.
    let mut foldable: HashSet<Blake2b256Hash> = HashSet::new();
    for (_, _, chain) in &ordered_chains {
        for (ch, _) in chain.event_log_index.number_channels_data.iter() {
            foldable.insert(ch.clone());
        }
    }
    // Running available-datum multiset per non-foldable channel, seeded lazily from the FIXED
    // floor base (prev FS) and advanced only for chains actually applied — so it mirrors the
    // running fs_state's single-value cells (each gets exactly one folded writer under keep-one).
    let avail_base_hash = Blake2b256Hash::from_bytes_prost(&prev_state.state_hash.0);
    let avail_reader = runtime_manager
        .history_repo
        .get_history_reader(&avail_base_hash)
        .map_err(CasperError::HistoryError)?;
    let mut available: HashMap<Blake2b256Hash, Vec<Vec<u8>>> = HashMap::new();
    // Seal keep-one losers (sig, host): concurrent single-value-cell writers the seal did NOT
    // apply this cut. Routed to recovery (re-executed against the updated FS a cut later), and
    // recorded in the rejected ledger below.
    let mut seal_rejected: Vec<(prost::bytes::Bytes, BlockHash)> = Vec::new();

    for (block_number, block_hash, chain) in &ordered_chains {
        let sigs: Vec<prost::bytes::Bytes> = chain
            .deploys_with_cost
            .0
            .iter()
            .map(|d| d.deploy_id.clone())
            .collect();
        // Construction keep-one'd at THIS host: its effect lands via its accepted recovery
        // inclusion, not this rejected original (folding the loser double-applies/stale-consumes).
        if sigs
            .iter()
            .any(|s| rejected_inclusions.contains(&(s.clone(), block_hash.clone())))
        {
            chains_skipped += 1;
            continue;
        }
        // Within-cut "flip": the same deploy kept in more than one cone block.
        if sigs.iter().any(|s| played_sigs.contains(s)) {
            chains_skipped += 1;
            continue;
        }
        // Cross-cut: a recovery re-proposal whose sig-derived (pre-charge/own) cell is already
        // folded into FS from an earlier cut. Value-cell merges are idempotent, but Int-delta
        // folds are not, so this base-check prevents a cross-cut double-count. System deploys
        // (CloseBlock/Slash) are excluded — they share no per-deploy cell, deduped by height.
        // Skipped (not rejected): the effect is already in FS, so it needs no recovery.
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
        // CloseBlock dedup-by-height: a CloseBlock chain whose height was already applied is
        // a replica sibling — skip it. Covers BOTH its committedRewards Map write AND its
        // posVault withdrawal number-channel transfer (a tagged number channel the value-type
        // fold would otherwise sum across siblings). First at a height inserts and applies;
        // replicas find the height present and skip.
        if sigs
            .iter()
            .any(|s| crate::rust::system_deploy::is_close_block_deploy_id(s))
            && !closeblock_heights.insert(*block_number)
        {
            chains_skipped += 1;
            continue;
        }
        // Whole-cell keep-one: serializable iff every non-foldable channel this chain consumes
        // from still has those datums available in the running cell state. A concurrent
        // single-value-cell writer whose consumed datum an earlier winner already took is
        // REJECTED to recovery — NOT folded, because folding two writers into one coupled cell
        // splits the entity (the orphan). The CloseBlock-PRIMARY order makes the non-recoverable
        // epoch deploy the winner; the user writer it displaces re-executes against the updated FS.
        let mut serializable = true;
        for e in chain.state_changes.datums_changes.iter() {
            let ch = e.key();
            if foldable.contains(ch) {
                continue;
            }
            let removed = &e.value().removed;
            if removed.is_empty() {
                continue;
            }
            if !available.contains_key(ch) {
                let base = avail_reader
                    .get_data_proj_binary(ch)
                    .map_err(CasperError::HistoryError)?;
                available.insert(ch.clone(), base);
            }
            if !crate::rust::merging::dag_merger::is_sub_multiset(
                removed,
                available.get(ch).expect("seeded above"),
            ) {
                serializable = false;
                break;
            }
        }
        if !serializable {
            for s in &sigs {
                seal_rejected.push((s.clone(), block_hash.clone()));
            }
            tracing::info!(
                target: "f1r3.trace.fs_floor",
                event = "seal_serialize_reject",
                block = *block_number,
                sigs = ?sigs.iter().map(|s| hex::encode(&s[..s.len().min(8)])).collect::<Vec<_>>(),
                "seal keep-one: chain's consumed single-value-cell datum no longer available (cross-fork concurrent writer) — rejecting to recovery"
            );
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
        // Advance `available` for this applied chain so later chains see its writes:
        // available = (available -- removed) ++ added, per touched non-foldable channel.
        for e in chain.state_changes.datums_changes.iter() {
            let ch = e.key();
            if foldable.contains(ch) {
                continue;
            }
            if !available.contains_key(ch) {
                let base = avail_reader
                    .get_data_proj_binary(ch)
                    .map_err(CasperError::HistoryError)?;
                available.insert(ch.clone(), base);
            }
            let avail = available.get_mut(ch).expect("seeded above");
            let mut next =
                rspace_plus_plus::rspace::merger::state_change::StateChange::multiset_diff(
                    avail,
                    &e.value().removed,
                );
            next.extend(e.value().added.iter().cloned());
            *avail = next;
        }
        for s in sigs {
            accepted_ledger.push(SealedAcceptance {
                sig: s.clone(),
                host: BlockHashSerde(block_hash.clone()),
                host_number: *block_number,
            });
            played_sigs.insert(s);
        }
        chains_applied += 1;
    }

    // The seal now keep-ones single-value cells: `seal_rejected` holds the concurrent cross-fork
    // writers it did NOT fold this cut. They feed the rejected ledger (below) and the recovery
    // buffer (in compute_parents_post_state, gated by the running-FS base-check), so each loser
    // re-executes against the updated FS and lands a cut later — monotone, no orphan.

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

    // Rejected ledger: this cut's construction keep-one'd inclusions AND the seal's own
    // keep-one losers (sig, host), extending the carried set. host_number (from the DAG) drives
    // retention pruning. Both feed the recovery buffer; the seal losers re-execute against the
    // updated FS, the construction losers land via their accepted recovery inclusion.
    let mut rejected_ledger: Vec<SealedRejection> = prev_state.rejected_deploys.clone();
    for (sig, host) in rejected_inclusions.iter().chain(seal_rejected.iter()) {
        rejected_ledger.push(SealedRejection {
            sig: sig.clone(),
            host: BlockHashSerde(host.clone()),
            host_number: dag.block_number_unsafe(host).unwrap_or(cut_number),
        });
    }
    // Bound both ledgers: drop entries whose host is far below the floor — past the
    // retention window a deploy is terminally finalized-or-expired, so its reporting
    // entry is no longer needed. Sort+dedup keeps the cumulative carry idempotent.
    let retention_floor = cut_number - LEDGER_RETENTION_BLOCKS;
    accepted_ledger.retain(|a| a.host_number >= retention_floor);
    rejected_ledger.retain(|r| r.host_number >= retention_floor);
    accepted_ledger.sort();
    accepted_ledger.dedup();
    rejected_ledger.sort();
    rejected_ledger.dedup();

    Ok(FloorData {
        state_hash: StateHashSerde(fs_state),
        rejected_deploys: rejected_ledger,
        accepted_deploys: accepted_ledger,
        block_number: cut_number,
    })
}
