//! Justification-derived finalized floor — the per-block finalized cut.
//!
//! `floor(B)` is the highest ancestor of B's parents that the clique oracle
//! certifies as finalized when evaluated over B's frozen justification
//! snapshot ([`CliqueOracle::ft_witnessed`]). Every input is contained in the
//! block itself (its signed justifications) or in immutable ancestor metadata,
//! so every honest node derives the same floor for the same block — no
//! node-local finality state participates. This is the linear-finality analog
//! of RChain's per-message fringe: the cut the block's merge builds on.

use std::collections::BTreeMap;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::Bond;
use models::rust::validator::Validator;

use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// The finalized cut a block builds on. Under linear finality this is a single
/// block: the highest witnessed-finalized ancestor across the block's parents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub hash: BlockHash,
    pub block_number: i64,
}

/// The active committee derived from a finalized-floor block's post-state: the
/// PoS bonds at that state, filtered to the currently-active validator set.
///
/// Both the proposer (packaging `block.bonds`, `block_creator.rs`) and the
/// validator (recomputing the bonds cache, `validate.rs::bonds_cache_from_floor`)
/// call THIS ONE function on the same floor state hash, so their committees are
/// identical by construction — the PLAY≡REPLAY the `InvalidBondsCache` check
/// relies on (a block whose `bonds` set differs from the floor committee is
/// rejected). Extracting the two previously byte-identical inline copies into a
/// single helper removes the standing risk that a future edit to one site
/// silently diverges the two (the committee is `Selection.committee_is_floor_bonds`
/// — a pure function of the floor — realized in Rust).
pub async fn floor_committee(
    runtime_manager: &RuntimeManager,
    floor_state: &StateHash,
) -> Result<Vec<Bond>, CasperError> {
    let floor_bonds = runtime_manager.compute_bonds(floor_state).await?;
    let active = runtime_manager.get_active_validators(floor_state).await?;
    Ok(floor_bonds
        .into_iter()
        .filter(|bond| active.contains(&bond.validator))
        .collect())
}

/// Walk depth past which a floor walk is reported as unusually deep (cold
/// start after restart, or a finality stall). Visibility only — the walk
/// always terminates: main-parent chains end at genesis, and genesis is
/// finalized by definition.
const DEEP_WALK_WARN_THRESHOLD: usize = 256;

/// Compute `floor(B)` for a block whose parents and justification snapshot are
/// given. `latest_messages` must be the block's own justifications (validate)
/// or the justification set about to be packaged into the block (propose) —
/// never the live DAG view.
///
/// The floor is computed from two candidate sources and is MONOTONE along
/// ancestry:
///
/// 1. **Inheritance** — every parent's own floor. A child can never carry a
///    lower cut than any parent, so a race sealed at some cut can never be
///    re-litigated by a descendant whose justifications happen to lag behind
///    that cut's finalization. This is RChain's fringe advancement
///    (`calculateFinalization` starts from `latestFringe(parents)` and only
///    moves up); deriving the floor fresh from the oracle per block — without
///    inheritance — allowed exactly that re-litigation.
/// 2. **Advancement** — per parent, the highest main-chain ancestor with
///    `ft_witnessed >= ft_threshold` over the justification snapshot; a block
///    with no main parent is genesis, finalized by definition.
///
/// The floor is the maximum candidate. Both sources are pure functions of the
/// block (parents' floors are themselves block-structural facts), so the
/// result stays node-identical. Linear finality requires every candidate to
/// lie on the floor's own main chain — a violation is a consensus-safety break
/// and is surfaced as an error, never papered over.
pub async fn finalized_floor(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let mut inherited: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        inherited.push(floor_of_block(dag, parent, ftt).await?);
    }
    let (floor, _main_parent_frontier) =
        derive_floor(dag, parents, latest_messages, ftt, inherited).await?;
    Ok(floor)
}

/// Core derivation: max over (inherited parent floors ∪ oracle frontiers),
/// with the one-chain safety check. `inherited` must hold the parents' own
/// floors; the caller resolves them so this stays non-recursive.
///
/// Returns `(floor, F(B))` where `F(B)` is the main parent's frontier over this
/// block's snapshot — i.e. `parent_frontier(parents[0], latest_messages)`. Since
/// a block is never witnessed-finalized over its own justifications, this equals
/// the block's OWN frontier `parent_frontier(B, just(B))`, a pure function of the
/// block. `floor_of_block` persists it so later merges resolve their frontiers
/// by an O(advance) up-walk from the cached pivot instead of an O(Δ) down-walk.
async fn derive_floor(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
    inherited: Vec<Floor>,
) -> Result<(Floor, Floor), CasperError> {
    if parents.is_empty() {
        return Err(CasperError::Other(
            "finalized_floor requires a non-empty parent set; genesis pre-state comes from config"
                .to_string(),
        ));
    }

    let mut candidates = inherited;
    let inherited_max = candidates.iter().map(|f| f.block_number).max();
    let mut frontiers: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        frontiers.push(parent_frontier(dag, parent, latest_messages, ftt).await?);
    }
    // parents[0] is the main parent; its frontier over this snapshot is F(B).
    let main_parent_frontier = frontiers[0].clone();
    candidates.extend(frontiers);

    // The floor is the merge base the block being created re-bases every parent onto.
    // Pick the HIGHEST candidate that is a SOUND base, considering candidates from the
    // top down. A candidate `c` is sound when EITHER:
    //
    //   A. `c` is a general DAG-ancestor of EVERY parent (or is one). Then `c` lies below
    //      all inputs, and since the new block merges every parent it descends from `c`
    //      and from every (parent-derived) candidate — nothing finalized is dropped. This
    //      is the multi-parent co-finalization case where two co-finalized siblings are
    //      both DIRECT parents (test_trim_state / run 28135973777): neither sibling is a
    //      base for the other, so the floor descends to their shared finalized cut.
    //
    //   B. every OTHER finalized candidate is compatible with `c` — it lies in `c`'s
    //      general DAG past (a lower cut whose state `c` already captures), or it is
    //      MERGEABLE with `c` via an EXISTING common-descendant parent (run 8c2952a8).
    //      This keeps the highest finalized tip as the floor when it dominates the rest
    //      (the in-place finalization-advance case).
    //
    // The highest candidate satisfying neither A nor B is skipped; if NO candidate is a
    // sound base (no finalized cut common to all parents), that is a genuinely
    // incompatible fork and is surfaced as an error, never papered over.
    let mut ordered: Vec<&Floor> = candidates.iter().collect();
    ordered.sort_by(|a, b| {
        b.block_number
            .cmp(&a.block_number)
            .then_with(|| b.hash.cmp(&a.hash))
    });

    let mut chosen: Option<Floor> = None;
    for cand in ordered {
        // Case A: general-ancestor of every parent.
        let mut covers_all_parents = true;
        for parent in parents {
            if cand.hash != *parent && !dag.is_dag_ancestor(&cand.hash, parent)? {
                covers_all_parents = false;
                break;
            }
        }
        if covers_all_parents {
            chosen = Some(cand.clone());
            break;
        }
        // Case B: every other candidate is in `cand`'s past or mergeable via a parent.
        let mut all_compatible = true;
        for other in &candidates {
            if other.hash == cand.hash || dag.is_dag_ancestor(&other.hash, &cand.hash)? {
                continue;
            }
            let mut mergeable_via_parent = false;
            for parent in parents {
                if dag.is_dag_ancestor(&other.hash, parent)?
                    && dag.is_dag_ancestor(&cand.hash, parent)?
                {
                    mergeable_via_parent = true;
                    break;
                }
            }
            if !mergeable_via_parent {
                all_compatible = false;
                break;
            }
        }
        if all_compatible {
            chosen = Some(cand.clone());
            break;
        }
    }

    let floor = chosen.ok_or_else(|| {
        CasperError::Other(format!(
            "finalized-floor safety violation: no finalized candidate is a sound merge base over \
             parents [{}] (candidates [{}]) — incompatible finalized fork",
            parents
                .iter()
                .map(|p| PrettyPrinter::build_string_bytes(p))
                .collect::<Vec<_>>()
                .join(", "),
            candidates
                .iter()
                .map(|c| format!(
                    "{}#{}",
                    PrettyPrinter::build_string_bytes(&c.hash),
                    c.block_number
                ))
                .collect::<Vec<_>>()
                .join(", "),
        ))
    })?;

    tracing::debug!(
        target: "f1r3.trace.floor_walk",
        candidates = ?candidates.iter().map(|c| format!("{}#{}", PrettyPrinter::build_string_bytes(&c.hash), c.block_number)).collect::<Vec<_>>(),
        chosen = %PrettyPrinter::build_string_bytes(&floor.hash),
        chosen_number = floor.block_number,
        "derive_floor candidates + chosen"
    );

    tracing::debug!(
        target: "f1r3.trace.floor",
        floor = %PrettyPrinter::build_string_bytes(&floor.hash),
        floor_number = floor.block_number,
        inherited_max = inherited_max.unwrap_or(-1),
        parent_count = parents.len(),
        "finalized floor derived (inheritance + advancement)"
    );

    Ok((floor, main_parent_frontier))
}

/// `floor(B)` for an already-inserted block, resolved through the persisted
/// floor cache. On a miss the floor is derived from the block's own metadata
/// (its parents and signed justifications) and cached — the floor is a pure
/// function of the block, so the cache can never go stale.
///
/// Resolution is iterative: ancestors whose floors are not yet cached are
/// pushed onto an explicit stack and computed bottom-up, so inheritance never
/// recurses. In steady state every parent is already cached (each block's
/// floor is computed when it is first merged on), making this a single cache
/// read.
///
/// A block with no parents is genesis: its own floor by definition, the
/// terminal cut of the floor-of-floor recursion.
pub async fn floor_of_block(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let mut stack: Vec<BlockHash> = vec![block_hash.clone()];
    while let Some(current) = stack.last().cloned() {
        if dag.get_cached_floor(&current)?.is_some() {
            stack.pop();
            continue;
        }

        let metadata = dag.lookup_unsafe(&current)?;
        if metadata.parents.is_empty() {
            dag.put_cached_floor(current.clone(), current.clone())?;
            stack.pop();
            continue;
        }

        let mut missing: Vec<BlockHash> = Vec::new();
        for parent in &metadata.parents {
            if dag.get_cached_floor(parent)?.is_none() {
                missing.push(parent.clone());
            }
        }
        if !missing.is_empty() {
            stack.extend(missing);
            continue;
        }

        let mut inherited: Vec<Floor> = Vec::with_capacity(metadata.parents.len());
        for parent in &metadata.parents {
            let hash = dag.get_cached_floor(parent)?.expect(
                "parent floor must be cached: the missing set was empty for this stack entry",
            );
            inherited.push(Floor {
                block_number: dag.block_number_unsafe(&hash)?,
                hash,
            });
        }
        let latest_messages: BTreeMap<Validator, BlockHash> = metadata
            .justifications
            .iter()
            .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
            .collect();
        let (floor, frontier) = derive_floor(
            dag,
            &metadata.parents,
            &latest_messages,
            ftt,
            inherited,
        )
        .await?;

        dag.put_cached_floor(current.clone(), floor.hash.clone())?;
        // Persist F(current) = the block's own frontier over its own snapshot,
        // a pure function of the block. Later merges read this as the up-walk
        // pivot in `parent_frontier`, collapsing the O(Δ²·V) walk ratchet.
        dag.put_cached_frontier(current.clone(), frontier.hash.clone())?;
        tracing::trace!(
            target: "f1r3.trace.floor",
            block = %PrettyPrinter::build_string_bytes(&current),
            floor = %PrettyPrinter::build_string_bytes(&floor.hash),
            floor_number = floor.block_number,
            "floor of inserted block computed and cached"
        );
        stack.pop();
    }

    let hash = dag
        .get_cached_floor(block_hash)?
        .expect("floor must be cached: the resolution stack drained for this block");
    Ok(Floor {
        block_number: dag.block_number_unsafe(&hash)?,
        hash,
    })
}

/// The highest witnessed-finalized block on one parent's main chain, over the
/// given justification snapshot.
///
/// Two paths, both yielding the identical frontier — the cache is a transparent
/// optimization, proven so by L-ANC + L-SNAP (see
/// `docs/theory/finalized-floor/finalized-floor-verification.md`):
///
/// * **Warm** ([`incremental_frontier`]) — when `parent`'s own frontier
///   `F(parent)` is cached (persisted by [`floor_of_block`] on insertion).
///   `F(parent)` is the frontier over `parent`'s OWN snapshot; the snapshot here
///   (`latest_messages` = the child's justifications) is a superset, so by L-SNAP
///   the true frontier sits at height ≥ `F(parent)`. We take it as a pivot and
///   walk UP the spine toward `parent`, advancing while each block stays
///   finalized. By L-ANC finalization is downward-closed on the spine, so the
///   walk stops at the first non-finalized block after only O(advance) oracle
///   calls — amortized O(1). The band itself is collected with cheap
///   `main_parent` hops (no oracle calls).
///
/// * **Cold** ([`cold_parent_frontier`]) — no cached pivot, the pivot is off
///   `parent`'s spine, the committee changed across the band (L-ANC's premise
///   fails), or the pivot no longer finalizes over the larger snapshot (L-SNAP's
///   premise fails): the original top-down walk from `parent`, one oracle call
///   per step down to the first finalized block (or genesis).
async fn parent_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    if let Some(pivot_hash) = dag.get_cached_frontier(parent)? {
        if let Some(frontier) =
            incremental_frontier(dag, parent, &pivot_hash, latest_messages, ftt).await?
        {
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_HIT_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            return Ok(frontier);
        }
    }
    metrics::counter!(
        crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_MISS_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .increment(1);
    cold_parent_frontier(dag, parent, latest_messages, ftt).await
}

/// Warm frontier: resolve `parent`'s frontier over the (larger) `latest_messages`
/// snapshot by an incremental UP-walk from the cached pivot `F(parent)`. Returns
/// `Ok(None)` when a determinism guard trips, signalling the caller to fall back
/// to the cold walk (which yields the identical result); the cache thus never
/// changes the derived frontier, only the work done to find it.
async fn incremental_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    pivot_hash: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Option<Floor>, CasperError> {
    let pivot_number = dag.block_number_unsafe(pivot_hash)?;

    // Collect the spine band [parent .. pivot] with cheap `main_parent` hops
    // (NO oracle calls). `spine[0]` = parent (top); the tail descends the main
    // spine down to the block reached at the pivot's height.
    let mut spine: Vec<BlockHash> = Vec::new();
    spine.push(parent.clone());
    spine.extend(dag.main_parent_chain(parent.clone(), pivot_number)?);
    // The pivot must be exactly the bottom of the band; otherwise it is not on
    // `parent`'s main spine (a fork at equal height) — fall back to cold.
    match spine.last() {
        Some(last) if last == pivot_hash => {}
        _ => return Ok(None),
    }

    // L-ANC guard: the committee (corresponding weight map, exactly what
    // `ft_witnessed` uses) must be constant across the band, else finalization
    // need not be downward-closed and the up-walk could disagree with the cold
    // walk. This is O(band) cheap metadata reads — bounded by the floor-distance
    // backstop — and never an oracle call.
    let pivot_committee = CliqueOracle::get_corresponding_weight_map(pivot_hash, dag).await?;
    for block in &spine {
        let committee = CliqueOracle::get_corresponding_weight_map(block, dag).await?;
        if committee != pivot_committee {
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_INCREMENTAL_GUARD_FALLBACK_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            return Ok(None);
        }
    }

    // L-SNAP guard: the pivot must still be witnessed-finalized over the larger
    // snapshot. It was finalized over `parent`'s own snapshot, and a superset can
    // only raise the fault tolerance — but a bonding event in the band can break
    // that monotonicity, so we verify rather than assume.
    let mut oracle_calls: u64 = 1;
    // A9 exact ≥-semantics (floor path): the pivot must still be witnessed-
    // finalized over the larger snapshot. `strict=false` ⇒ (2q−S)/S ≥ θ.
    let pivot_finalized =
        CliqueOracle::ft_witnessed_exact(pivot_hash, dag, latest_messages, ftt, false).await?;
    if !pivot_finalized {
        metrics::counter!(
            crate::rust::metrics_constants::FLOOR_INCREMENTAL_GUARD_FALLBACK_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .increment(1);
        return Ok(None);
    }

    // Up-walk: from just above the pivot toward `parent`, advancing while each
    // block stays finalized. By L-ANC (constant committee, verified above) the
    // finalized blocks form a downward-closed prefix, so the first non-finalized
    // block ends it and the highest finalized block is the frontier.
    let mut best_hash = pivot_hash.clone();
    let mut best_number = pivot_number;
    let mut advance: u64 = 0;
    for candidate in spine[..spine.len() - 1].iter().rev() {
        // A9 exact ≥-semantics (floor path): advance while each block stays
        // witnessed-finalized over the snapshot.
        let finalized =
            CliqueOracle::ft_witnessed_exact(candidate, dag, latest_messages, ftt, false).await?;
        oracle_calls += 1;
        if finalized {
            best_hash = candidate.clone();
            best_number = dag.block_number_unsafe(candidate)?;
            advance += 1;
        } else {
            break;
        }
    }

    metrics::counter!(
        crate::rust::metrics_constants::FLOOR_WALK_ORACLE_CALLS_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .increment(oracle_calls);
    metrics::histogram!(
        crate::rust::metrics_constants::FLOOR_FRONTIER_ADVANCE_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .record(advance as f64);
    trace_frontier(parent, &best_hash, best_number, advance as usize, "warm-up-walk");
    Ok(Some(Floor {
        hash: best_hash,
        block_number: best_number,
    }))
}

/// Cold frontier: the top-down walk from `parent`, one clique-oracle call per
/// step, returning the first witnessed-finalized block (or genesis). Used on a
/// cache miss or when a warm-path determinism guard trips; also the genesis
/// terminator. Always terminates — main-parent chains end at genesis.
async fn cold_parent_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let mut current = parent.clone();
    let mut walked: usize = 0;
    let mut oracle_calls: u64 = 0;
    loop {
        // A9 exact ≥-semantics (floor path): first witnessed-finalized block down
        // the main-parent chain is the frontier.
        let finalized =
            CliqueOracle::ft_witnessed_exact(&current, dag, latest_messages, ftt, false).await?;
        oracle_calls += 1;
        tracing::debug!(
            target: "f1r3.trace.floor_walk",
            parent = %PrettyPrinter::build_string_bytes(parent),
            current = %PrettyPrinter::build_string_bytes(&current),
            current_number = dag.block_number_unsafe(&current)?,
            finalized,
            walked,
            "floor walk step"
        );
        if finalized {
            let block_number = dag.block_number_unsafe(&current)?;
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_WALK_ORACLE_CALLS_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(oracle_calls);
            trace_frontier(
                parent,
                &current,
                block_number,
                walked,
                "witnessed-finalized",
            );
            return Ok(Floor {
                hash: current,
                block_number,
            });
        }
        match dag.main_parent(&current) {
            Some(main_parent) => {
                current = main_parent;
                walked += 1;
                if walked == DEEP_WALK_WARN_THRESHOLD {
                    tracing::warn!(
                        target: "f1r3.trace.floor",
                        parent = %PrettyPrinter::build_string_bytes(parent),
                        walked,
                        "floor walk unusually deep; finality is lagging or this is a cold start"
                    );
                }
            }
            None => {
                // No main parent: `current` is genesis, finalized by definition.
                let block_number = dag.block_number_unsafe(&current)?;
                metrics::counter!(
                    crate::rust::metrics_constants::FLOOR_WALK_ORACLE_CALLS_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(oracle_calls);
                trace_frontier(parent, &current, block_number, walked, "genesis");
                return Ok(Floor {
                    hash: current,
                    block_number,
                });
            }
        }
    }
}

fn trace_frontier(
    parent: &BlockHash,
    frontier: &BlockHash,
    frontier_number: i64,
    walked: usize,
    kind: &str,
) {
    tracing::trace!(
        target: "f1r3.trace.floor",
        parent = %PrettyPrinter::build_string_bytes(parent),
        frontier = %PrettyPrinter::build_string_bytes(frontier),
        frontier_number,
        walked,
        kind,
        "per-parent finalized frontier"
    );
}

#[cfg(test)]
mod frontier_determinism_tests {
    //! The determinism linchpin's "tested" leg: on a real finalizing DAG the WARM
    //! up-walk (`incremental_frontier` from a cached pivot) returns the identical
    //! frontier as the COLD down-walk (`cold_parent_frontier`), and the floor a
    //! block derives is invariant to whether the caches are cold or warm
    //! (transparency ⇒ no fork). Complements the axiom-free Rocq proof
    //! (Floor.frontier_cache_transparent) and the 400+-block soak.
    use super::*;

    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use parking_lot::RwLock as PlRwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    fn h(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }
    fn val() -> Bytes { Bytes::from(vec![9; 65]) }

    fn md(hash: Bytes, parents: Vec<Bytes>, num: i64, v: &Bytes) -> BlockMetadata {
        let mut wm = BTreeMap::new();
        wm.insert(v.clone(), 1i64);
        BlockMetadata {
            block_hash: hash,
            parents,
            sender: v.clone(),
            justifications: vec![],
            weight_map: wm,
            block_number: num,
            sequence_number: num as i32,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
        }
    }

    /// A single-validator linear chain genesis <- b1 <- b2 <- b3, committee {v:1}.
    /// Over the snapshot J = {v -> b2}, the clique oracle finalizes genesis, b1,
    /// and b2 (v's latest message b2 DAG-descends from each), but not b3. So the
    /// frontier of b3 over J is b2.
    fn mk_dag() -> (KeyValueDagRepresentation, Bytes, (Bytes, Bytes, Bytes, Bytes)) {
        let v = val();
        let (g, b1, b2, b3) = (h(0), h(1), h(2), h(3));

        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        bms.add(md(g.clone(), vec![], 0, &v)).unwrap();
        bms.add(md(b1.clone(), vec![g.clone()], 1, &v)).unwrap();
        bms.add(md(b2.clone(), vec![b1.clone()], 2, &v)).unwrap();
        bms.add(md(b3.clone(), vec![b2.clone()], 3, &v)).unwrap();

        let mut dag_set = imbl::HashSet::new();
        for x in [&g, &b1, &b2, &b3] {
            dag_set.insert(x.clone());
        }
        let mut bnum = imbl::HashMap::new();
        bnum.insert(g.clone(), 0);
        bnum.insert(b1.clone(), 1);
        bnum.insert(b2.clone(), 2);
        bnum.insert(b3.clone(), 3);
        let mut mp = imbl::HashMap::new();
        mp.insert(b1.clone(), g.clone());
        mp.insert(b2.clone(), b1.clone());
        mp.insert(b3.clone(), b2.clone());

        let dag = KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: bnum,
            main_parent_map: mp,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            deploy_index: Arc::new(PlRwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                InMemoryKeyValueStore::new(),
            )))),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        };
        (dag, v, (g, b1, b2, b3))
    }

    #[tokio::test]
    async fn warm_up_walk_equals_cold_down_walk() {
        let (dag, v, (_g, b1, b2, b3)) = mk_dag();
        let mut j = BTreeMap::new();
        j.insert(v, b2.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);

        // Cold: top-down from b3 → first finalized is b2.
        let cold = cold_parent_frontier(&dag, &b3, &j, thr).await.unwrap();
        assert_eq!(cold.hash, b2, "cold frontier of b3 over J must be b2");

        // Warm: from a pivot BELOW the true frontier (b1) → the up-walk must
        // advance to b2 and stop (b3 not finalized), matching the cold result.
        let warm = incremental_frontier(&dag, &b3, &b1, &j, thr).await.unwrap();
        assert!(
            warm.is_some(),
            "warm path must apply (committee constant across the band, pivot finalized)"
        );
        assert_eq!(
            warm.unwrap().hash,
            cold.hash,
            "warm up-walk must equal the cold down-walk (frontier cache is transparent)"
        );
    }

    #[tokio::test]
    async fn finalized_floor_is_cache_transparent() {
        let (dag, v, (_g, _b1, b2, b3)) = mk_dag();
        let mut j = BTreeMap::new();
        j.insert(v, b2.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);

        // First call populates the floor/frontier caches (cold internally);
        // the second reads them (warm). The derived floor must be identical, and
        // it must be the sound Case-A base b2 (the highest finalized ancestor of
        // the single parent b3).
        let floor_cold = finalized_floor(&dag, &[b3.clone()], &j, thr).await.unwrap();
        let floor_warm = finalized_floor(&dag, &[b3.clone()], &j, thr).await.unwrap();
        assert_eq!(floor_cold.hash, b2, "derive_floor must select the sound base b2");
        assert_eq!(
            floor_cold, floor_warm,
            "enabling the caches must not change the derived floor (no fork)"
        );
    }

    // ---- Phase-7 W7.2: guard-trip, Case-B soundness, incompatible-fork Err ----

    fn md_wm(
        hash: Bytes,
        parents: Vec<Bytes>,
        num: i64,
        sender: &Bytes,
        wm: Vec<(Bytes, i64)>,
    ) -> BlockMetadata {
        let mut weight_map = BTreeMap::new();
        for (validator, weight) in wm {
            weight_map.insert(validator, weight);
        }
        BlockMetadata {
            block_hash: hash,
            parents,
            sender: sender.clone(),
            justifications: vec![],
            weight_map,
            block_number: num,
            sequence_number: num as i32,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
        }
    }

    /// Assemble a DAG from an explicit block list, deriving `dag_set`,
    /// `block_number_map`, and `main_parent_map` (parents[0]) from the metadata.
    fn build_dag(blocks: Vec<BlockMetadata>) -> KeyValueDagRepresentation {
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let mut dag_set = imbl::HashSet::new();
        let mut bnum = imbl::HashMap::new();
        let mut mp = imbl::HashMap::new();
        for b in &blocks {
            dag_set.insert(b.block_hash.clone());
            bnum.insert(b.block_hash.clone(), b.block_number);
            if let Some(main) = b.parents.first() {
                mp.insert(b.block_hash.clone(), main.clone());
            }
        }
        for b in blocks {
            bms.add(b).unwrap();
        }
        KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: bnum,
            main_parent_map: mp,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            deploy_index: Arc::new(PlRwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                InMemoryKeyValueStore::new(),
            )))),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        }
    }

    /// Guard-trip: a committee CHANGE inside the band (a bonding / re-stake between
    /// the pivot and the parent) breaks L-ANC's constant-committee premise. The warm
    /// up-walk MUST decline (`Ok(None)`) rather than serve a frontier computed under
    /// an inconsistent committee, and the dispatcher MUST fall back to the cold walk,
    /// yielding the identical frontier. This is the "tested" leg of the guard whose
    /// soundness `GuardBridge.chain_adj_AdjDC` derives in Rocq.
    #[tokio::test]
    async fn guard_trip_committee_change_falls_back_to_cold() {
        let v = h(50);
        let (g, b1, b2, b3) = (h(0), h(1), h(2), h(3));
        // v's weight changes 1 -> 2 at b2, so committee(b3) = wm(b2) = {v:2} differs
        // from pivot_committee = committee(b1) = wm(g) = {v:1}.
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, vec![(v.clone(), 1)]),
            md_wm(b1.clone(), vec![g.clone()], 1, &v, vec![(v.clone(), 1)]),
            md_wm(b2.clone(), vec![b1.clone()], 2, &v, vec![(v.clone(), 2)]),
            md_wm(b3.clone(), vec![b2.clone()], 3, &v, vec![(v.clone(), 2)]),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), b2.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);

        // Warm up-walk from pivot b1 must DECLINE (committee changes at b3 in the band).
        let warm = incremental_frontier(&dag, &b3, &b1, &j, thr).await.unwrap();
        assert!(
            warm.is_none(),
            "incremental_frontier must return Ok(None) on a committee change in the band"
        );

        // Seed the pivot so the dispatcher attempts (and must abandon) the warm path;
        // it must fall back to the cold walk and return the identical frontier.
        dag.put_cached_frontier(b3.clone(), b1.clone()).unwrap();
        let dispatched = parent_frontier(&dag, &b3, &j, thr).await.unwrap();
        let cold = cold_parent_frontier(&dag, &b3, &j, thr).await.unwrap();
        assert_eq!(
            dispatched, cold,
            "on a guard trip the dispatched frontier must equal the cold walk (transparent)"
        );
    }

    /// Case-B (in-place finalization-advance dominates): the highest finalized
    /// candidate `c` is NOT a general ancestor of every parent (Case-A fails), yet
    /// every other candidate lies in `c`'s DAG past, so `c` is a sound base and is
    /// chosen. DAG: g <- t <- c <- p1 (validator v) plus t <- p2 (validator w, not in
    /// the committee). Over j={v:c}, c and t finalize but p1, p2 do not, so
    /// frontier(p1)=c and frontier(p2)=t. `c` is not an ancestor of p2, but t (p2's
    /// frontier) is in c's past ⇒ Case-B selects c. Mirrors Selection.case_b_compatible.
    #[tokio::test]
    async fn derive_floor_case_b_selects_dominating_finalized_tip() {
        let v = h(50);
        let w = h(51);
        let (g, t, c, p1, p2) = (h(0), h(1), h(2), h(3), h(4));
        let wm = || vec![(v.clone(), 1)]; // committee is always {v:1}; w never votes
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, wm()),
            md_wm(t.clone(), vec![g.clone()], 1, &v, wm()),
            md_wm(c.clone(), vec![t.clone()], 2, &v, wm()),
            md_wm(p1.clone(), vec![c.clone()], 3, &v, wm()),
            md_wm(p2.clone(), vec![t.clone()], 2, &w, wm()),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), c.clone()); // v's frozen latest is c (before it made p1)
        let thr = FtThreshold::from_f32_lossy(0.1);

        let inherited = vec![
            Floor { hash: c.clone(), block_number: 2 },
            Floor { hash: t.clone(), block_number: 1 },
        ];
        let (floor, _f) = derive_floor(&dag, &[p1.clone(), p2.clone()], &j, thr, inherited)
            .await
            .unwrap();
        assert_eq!(
            floor.hash, c,
            "Case-B must select the dominating finalized tip c (Case-A fails: c is not an \
             ancestor of p2; but t — the only other candidate — is in c's past)"
        );
    }

    /// Incompatible-fork Err: two parents with NO common finalized candidate must be
    /// surfaced as an error, never papered over with an unsound base. Modeled with a
    /// deliberately DISCONNECTED DAG (independent roots g_a, g_b) — the only shape in
    /// which two honest cuts are truly incompatible, since a `{1,1}` quorum can never
    /// finalize competing forks (both validators would have to agree). Each parent's
    /// frontier is its own root, so no candidate is a common ancestor and neither
    /// Case-A nor Case-B holds ⇒ the safety error fires (Selection.select_none_correct).
    #[tokio::test]
    async fn derive_floor_incompatible_fork_errors() {
        let v = h(50);
        let w = h(51);
        let (g_a, a1, g_b, b1) = (h(0), h(1), h(5), h(6));
        let dag = build_dag(vec![
            md_wm(g_a.clone(), vec![], 0, &v, vec![(v.clone(), 1)]),
            md_wm(a1.clone(), vec![g_a.clone()], 1, &v, vec![(v.clone(), 1)]),
            md_wm(g_b.clone(), vec![], 0, &w, vec![(w.clone(), 1)]),
            md_wm(b1.clone(), vec![g_b.clone()], 1, &w, vec![(w.clone(), 1)]),
        ]);
        let j = BTreeMap::new(); // nothing finalizes by quorum; frontiers fall to the roots
        let thr = FtThreshold::from_f32_lossy(0.1);

        let inherited = vec![
            Floor { hash: g_a.clone(), block_number: 0 },
            Floor { hash: g_b.clone(), block_number: 0 },
        ];
        let result = derive_floor(&dag, &[a1.clone(), b1.clone()], &j, thr, inherited).await;
        match result {
            Err(CasperError::Other(msg)) => assert!(
                msg.contains("incompatible finalized fork"),
                "expected the incompatible-fork safety error, got: {msg}"
            ),
            other => panic!("expected Err(incompatible fork), got {other:?}"),
        }
    }

    /// A shared Case-A DAG: `g <- t <- c`, with BOTH parents `p1` (v) and `p2` (w)
    /// children of `c`, so `c` is a common ancestor of `{p1,p2}`. The committee is
    /// `{v:1}` (w never votes) and `j={v:c}`, so the whole chain `g,t,c` finalizes and
    /// `c` is the highest finalized common ancestor. Returns `(dag, j, thr, [p1,p2],
    /// inherited, g,t,c,p1,p2)`.
    #[allow(clippy::type_complexity)]
    fn case_a_fixture() -> (
        KeyValueDagRepresentation,
        BTreeMap<Bytes, Bytes>,
        FtThreshold,
        Vec<Bytes>,
        Vec<Floor>,
        (Bytes, Bytes, Bytes, Bytes, Bytes),
    ) {
        let v = h(50);
        let w = h(51);
        let (g, t, c, p1, p2) = (h(0), h(1), h(2), h(3), h(4));
        let wm = || vec![(v.clone(), 1)];
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, wm()),
            md_wm(t.clone(), vec![g.clone()], 1, &v, wm()),
            md_wm(c.clone(), vec![t.clone()], 2, &v, wm()),
            md_wm(p1.clone(), vec![c.clone()], 3, &v, wm()),
            md_wm(p2.clone(), vec![c.clone()], 3, &w, wm()),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), c.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);
        let inherited = vec![
            Floor { hash: c.clone(), block_number: 2 },
            Floor { hash: c.clone(), block_number: 2 },
        ];
        (dag, j, thr, vec![p1.clone(), p2.clone()], inherited, (g, t, c, p1, p2))
    }

    /// T-LIN (`Selection.case_a_common_ancestor`): when the highest finalized candidate
    /// is a DAG-ancestor of EVERY parent (Case-A), `derive_floor` selects it AND the
    /// result is a genuine common ancestor of all parents — asserted via the sibling
    /// `is_dag_ancestor` primitive. (Previously only the Case-B pick and the
    /// incompatible-fork `Err` were tested; the Case-A common-ancestor property was not.)
    #[tokio::test]
    async fn derive_floor_case_a_floor_is_common_ancestor_of_all_parents() {
        let (dag, j, thr, parents, inherited, (_g, _t, c, p1, p2)) = case_a_fixture();
        let (floor, _f) = derive_floor(&dag, &parents, &j, thr, inherited)
            .await
            .expect("derive_floor");
        assert_eq!(floor.hash, c, "Case-A selects the common-ancestor finalized candidate c");
        assert!(
            dag.is_dag_ancestor(&floor.hash, &p1).expect("is_dag_ancestor"),
            "the Case-A floor must be a DAG-ancestor of parent p1"
        );
        assert!(
            dag.is_dag_ancestor(&floor.hash, &p2).expect("is_dag_ancestor"),
            "the Case-A floor must be a DAG-ancestor of parent p2"
        );
    }

    /// Maximality / T-DET (`Selection.select_highest_sound`): the inheritance+advancement
    /// case. The two parents carry LAGGING inherited floors (`t@1`, `g@0` — older cuts),
    /// but the justification snapshot `j={v:c}` has since finalized `c`, so advancement
    /// surfaces `c@2` as a frontier candidate. The candidate multiset is therefore
    /// genuinely `{g@0, t@1, c@2}` — three sound bases of distinct height — and
    /// `derive_floor` must pick the strict MAXIMUM (`c@2`), never a lagging inherited cut.
    /// This is exactly the "a child never carries a lower cut than any parent, and
    /// advances to the highest newly-finalized candidate" contract (docstring on
    /// `finalized_floor`). `g` and `t` are shown to be sound competitors (common
    /// ancestors of both parents) and strictly lower, so the choice is a real maximum.
    #[tokio::test]
    async fn derive_floor_selects_highest_sound_finalized_candidate() {
        let (dag, j, thr, parents, _inherited, (g, t, _c, p1, p2)) = case_a_fixture();
        // Override the inherited floors with the LAGGING cuts the parents carried before
        // c finalized, forcing g@0 and t@1 into the candidate sort alongside advancement's c@2.
        let lagging = vec![
            Floor { hash: t.clone(), block_number: 1 },
            Floor { hash: g.clone(), block_number: 0 },
        ];
        let (floor, _f) = derive_floor(&dag, &parents, &j, thr, lagging)
            .await
            .expect("derive_floor");
        assert_eq!(
            floor.block_number, 2,
            "advancement selects the highest sound candidate c (num 2), not the lagging inherited t=1 or g=0"
        );
        for (lower, lower_num) in [(&t, 1i64), (&g, 0)] {
            assert!(
                dag.is_dag_ancestor(lower, &p1).expect("anc")
                    && dag.is_dag_ancestor(lower, &p2).expect("anc"),
                "the lagging candidate (num {lower_num}) is ALSO a common ancestor of both parents (a competing sound base)"
            );
            assert!(
                lower_num < floor.block_number,
                "and is strictly lower-numbered than the chosen floor (maximality)"
            );
        }
    }

    /// T-FIN (`Selection.select_finalized` / `GuardBridge.upgo_finalized`): the floor
    /// `derive_floor` returns is itself `Finalized` over the justification snapshot — it
    /// clears the exact FT threshold (floor path, `≥`) per the same clique oracle the
    /// node runs (`CliqueOracle::ft_witnessed_exact`). Confirms the result is a genuinely
    /// finalized cut, not merely a well-formed ancestor.
    #[tokio::test]
    async fn derive_floor_result_is_finalized_over_justifications() {
        let (dag, j, thr, parents, inherited, _hashes) = case_a_fixture();
        let (floor, _f) = derive_floor(&dag, &parents, &j, thr, inherited)
            .await
            .expect("derive_floor");
        let finalized = crate::rust::safety::clique_oracle::CliqueOracle::ft_witnessed_exact(
            &floor.hash,
            &dag,
            &j,
            thr,
            false,
        )
        .await
        .expect("ft_witnessed_exact");
        assert!(
            finalized,
            "the derive_floor result must be Finalized over the justification snapshot (T-FIN)"
        );
    }

    // ---- Phase-4 T-DET maximality: derive_floor picks the HIGHEST sound candidate ----

    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;

    lazy_static::lazy_static! {
        // Shared Tokio runtime for the `#[test]` proptest (it cannot be `#[tokio::test]`);
        // mirrors casper/tests/fork_choice/prop_estimator_determinism.rs.
        static ref FLOOR_RUNTIME: tokio::runtime::Runtime =
            tokio::runtime::Runtime::new().expect("tokio runtime");
    }

    prop_compose! {
        /// A single-validator linear chain `g=b0 <- b1 <- … <- b_depth` ({v:1}
        /// committee), a frontier index `k ∈ [1, depth)` (so `frontier(b_depth over
        /// {v:b_k}) = b_k`, strictly below the parent), and a random `inherited_mask`
        /// selecting which strict ancestors `b_0..b_{depth-1}` are also fed as inherited
        /// floors. Every candidate is a DAG-ancestor of the single parent `b_depth`
        /// (linear chain) ⇒ every candidate is a Case-A sound base ⇒ maximality alone
        /// decides the floor.
        fn chain_scenario()(depth in 2usize..=6)(
            k in 1usize..depth,
            inherited_mask in prop::collection::vec(any::<bool>(), depth),
            depth in Just(depth),
        ) -> (usize, usize, Vec<bool>) {
            (depth, k, inherited_mask)
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 24, max_shrink_iters: 8, ..ProptestConfig::default() })]

        // maximality / T-DET (`Selection.select_highest_sound`): over a
        // descending-sorted candidate set of Case-A sound bases (inherited parent floors
        // ∪ the per-parent frontier), `derive_floor` returns the candidate of GREATEST
        // block number — the highest sound base. This is the general (proptest) form of
        // the hand-built `derive_floor_selects_highest_sound_finalized_candidate` example:
        // the candidate multiset is randomly `{inherited b_i} ∪ {frontier b_k}`, and the
        // chosen floor must always be its maximum, regardless of whether inheritance or
        // advancement supplies it (monotonicity in BOTH directions).
        #[test]
        fn derive_floor_selects_highest_sound_candidate_over_chain((depth, k, mask) in chain_scenario()) {
            FLOOR_RUNTIME.block_on(async move {
                let v = h(50);
                let mut blocks: Vec<BlockMetadata> = Vec::with_capacity(depth + 1);
                for i in 0..=depth {
                    let parents = if i == 0 { vec![] } else { vec![h((i - 1) as u8)] };
                    blocks.push(md_wm(h(i as u8), parents, i as i64, &v, vec![(v.clone(), 1)]));
                }
                let dag = build_dag(blocks);
                let parent = h(depth as u8);

                // frontier(parent over {v:b_k}) = b_k (single-validator chain).
                let mut j = BTreeMap::new();
                j.insert(v.clone(), h(k as u8));
                let thr = FtThreshold::from_f32_lossy(0.1);

                // Inherited candidates: the selected strict ancestors b_0..b_{depth-1}.
                let mut inherited: Vec<Floor> = Vec::new();
                for (i, &selected) in mask.iter().enumerate().take(depth) {
                    if selected {
                        inherited.push(Floor { hash: h(i as u8), block_number: i as i64 });
                    }
                }

                let (floor, _f) = derive_floor(&dag, &[parent], &j, thr, inherited.clone())
                    .await
                    .expect("derive_floor");

                // Oracle: the maximum block number over inherited ∪ {frontier k}. All
                // candidates are distinct chain positions, so the max hash is unambiguous.
                let expected_num = inherited
                    .iter()
                    .map(|f| f.block_number)
                    .chain(std::iter::once(k as i64))
                    .max()
                    .expect("candidate set is non-empty (frontier always present)");
                prop_assert_eq!(
                    floor.block_number, expected_num,
                    "derive_floor must select the HIGHEST sound candidate's block number"
                );
                prop_assert_eq!(
                    floor.hash, h(expected_num as u8),
                    "derive_floor must select the chain block at the highest candidate number"
                );
                Ok::<(), TestCaseError>(())
            })?;
        }
    }
}
