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
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;

use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::CliqueOracle;

/// The finalized cut a block builds on. Under linear finality this is a single
/// block: the highest witnessed-finalized ancestor across the block's parents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub hash: BlockHash,
    pub block_number: i64,
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
    ft_threshold: f32,
) -> Result<Floor, CasperError> {
    let mut inherited: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        inherited.push(floor_of_block(dag, parent, ft_threshold).await?);
    }
    let (floor, _main_parent_frontier) =
        derive_floor(dag, parents, latest_messages, ft_threshold, inherited).await?;
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
    ft_threshold: f32,
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
        frontiers.push(parent_frontier(dag, parent, latest_messages, ft_threshold).await?);
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
    ft_threshold: f32,
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
            ft_threshold,
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
    ft_threshold: f32,
) -> Result<Floor, CasperError> {
    if let Some(pivot_hash) = dag.get_cached_frontier(parent)? {
        if let Some(frontier) =
            incremental_frontier(dag, parent, &pivot_hash, latest_messages, ft_threshold).await?
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
    cold_parent_frontier(dag, parent, latest_messages, ft_threshold).await
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
    ft_threshold: f32,
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
    let pivot_ft = CliqueOracle::ft_witnessed(pivot_hash, dag, latest_messages).await?;
    if pivot_ft < ft_threshold {
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
        let ft = CliqueOracle::ft_witnessed(candidate, dag, latest_messages).await?;
        oracle_calls += 1;
        if ft >= ft_threshold {
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
    ft_threshold: f32,
) -> Result<Floor, CasperError> {
    let mut current = parent.clone();
    let mut walked: usize = 0;
    let mut oracle_calls: u64 = 0;
    loop {
        let ft = CliqueOracle::ft_witnessed(&current, dag, latest_messages).await?;
        oracle_calls += 1;
        let finalized = ft >= ft_threshold;
        tracing::debug!(
            target: "f1r3.trace.floor_walk",
            parent = %PrettyPrinter::build_string_bytes(parent),
            current = %PrettyPrinter::build_string_bytes(&current),
            current_number = dag.block_number_unsafe(&current)?,
            ft,
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
