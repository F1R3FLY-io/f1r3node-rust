//! Justification-derived finalized floor — the per-block finalized cut.
//!
//! `floor(B)` is the highest ancestor of B's parents that the clique oracle
//! certifies as finalized when evaluated over B's frozen justification
//! snapshot ([`CliqueOracle::ft_witnessed`]). Every input is contained in the
//! block itself (its signed justifications) or in immutable ancestor metadata,
//! so every honest node derives the same floor for the same block — no
//! node-local finality state participates. This is the linear-finality analog
//! of RChain's per-message fringe: the cut the block's merge builds on.

use std::collections::{BTreeMap, HashSet};

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
    derive_floor(dag, parents, latest_messages, ft_threshold, inherited).await
}

/// Core derivation: max over (inherited parent floors ∪ oracle frontiers),
/// with the one-chain safety check. `inherited` must hold the parents' own
/// floors; the caller resolves them so this stays non-recursive.
async fn derive_floor(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ft_threshold: f32,
    inherited: Vec<Floor>,
) -> Result<Floor, CasperError> {
    if parents.is_empty() {
        return Err(CasperError::Other(
            "finalized_floor requires a non-empty parent set; genesis pre-state comes from config"
                .to_string(),
        ));
    }

    let mut candidates = inherited;
    let inherited_max = candidates.iter().map(|f| f.block_number).max();
    for parent in parents {
        candidates.push(parent_frontier(dag, parent, latest_messages, ft_threshold).await?);
    }

    let floor = candidates
        .iter()
        .max_by(|a, b| {
            a.block_number
                .cmp(&b.block_number)
                .then_with(|| a.hash.cmp(&b.hash))
        })
        .expect("candidates is non-empty: one frontier per parent and parents is non-empty")
        .clone();

    tracing::debug!(
        target: "f1r3.trace.floor_walk",
        candidates = ?candidates.iter().map(|c| format!("{}#{}", PrettyPrinter::build_string_bytes(&c.hash), c.block_number)).collect::<Vec<_>>(),
        chosen = %PrettyPrinter::build_string_bytes(&floor.hash),
        chosen_number = floor.block_number,
        "derive_floor candidates + chosen"
    );

    // Linear-finality safety: every other finalized candidate must be COMPATIBLE
    // with the chosen floor. Two cases are compatible:
    //
    //   1. The candidate lies in the floor's GENERAL DAG past (reachable via any
    //      parent path) — a lower cut merged in as a secondary parent; its state
    //      is preserved.
    //   2. The candidate is MERGEABLE with the floor — some parent has merged
    //      BOTH (each is a general DAG-ancestor of that parent), so the parent is
    //      a common descendant and both states coexist. This is the multi-parent
    //      co-finalization case: equal-weight siblings that every block merges are
    //      co-finalized, neither lying on the other's ancestry, yet neither lost.
    //      The earlier rule recognized only case 1 and rejected case 2 as a
    //      "safety violation", wedging the proposer (integration run 8c2952a8).
    //
    // A candidate that is neither is a finalized block with no common descendant
    // — a genuinely incompatible fork — and is surfaced as an error, never
    // papered over.
    for candidate in &candidates {
        if candidate.hash == floor.hash
            || is_general_ancestor(dag, &candidate.hash, candidate.block_number, &floor.hash)?
        {
            continue;
        }
        let mergeable_via_parent = {
            let mut found = false;
            for parent in parents {
                if is_general_ancestor(dag, &candidate.hash, candidate.block_number, parent)?
                    && is_general_ancestor(dag, &floor.hash, floor.block_number, parent)?
                {
                    found = true;
                    break;
                }
            }
            found
        };
        if !mergeable_via_parent {
            return Err(CasperError::Other(format!(
                "finalized-floor safety violation: finalized cut {} (#{}) is neither in floor {} \
                 (#{})'s past nor merged with it by any parent — incompatible finalized fork",
                PrettyPrinter::build_string_bytes(&candidate.hash),
                candidate.block_number,
                PrettyPrinter::build_string_bytes(&floor.hash),
                floor.block_number,
            )));
        }
    }

    tracing::debug!(
        target: "f1r3.trace.floor",
        floor = %PrettyPrinter::build_string_bytes(&floor.hash),
        floor_number = floor.block_number,
        inherited_max = inherited_max.unwrap_or(-1),
        parent_count = parents.len(),
        "finalized floor derived (inheritance + advancement)"
    );

    Ok(floor)
}

/// True if `candidate` (at height `candidate_number`) is a general DAG ancestor
/// of `descendant` — reachable by following ANY parent, not just the main
/// parent. Walks up from `descendant`, pruning branches once they drop to or
/// below the candidate's height, so the search covers only the cone between
/// them (cheap when the floor is near the candidate, as in steady state).
fn is_general_ancestor(
    dag: &KeyValueDagRepresentation,
    candidate: &BlockHash,
    candidate_number: i64,
    descendant: &BlockHash,
) -> Result<bool, CasperError> {
    if candidate == descendant {
        return Ok(true);
    }
    let mut seen: HashSet<BlockHash> = HashSet::new();
    let mut stack: Vec<BlockHash> = vec![descendant.clone()];
    while let Some(hash) = stack.pop() {
        if !seen.insert(hash.clone()) {
            continue;
        }
        if hash == *candidate {
            return Ok(true);
        }
        let meta = dag.lookup_unsafe(&hash)?;
        // At or below the candidate's height (and not the candidate itself):
        // every parent is strictly lower, so this branch cannot reach it.
        if meta.block_number <= candidate_number {
            continue;
        }
        for parent in meta.parents {
            if !seen.contains(&parent) {
                stack.push(parent);
            }
        }
    }
    Ok(false)
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
        let floor = derive_floor(
            dag,
            &metadata.parents,
            &latest_messages,
            ft_threshold,
            inherited,
        )
        .await?;

        dag.put_cached_floor(current.clone(), floor.hash.clone())?;
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

/// The highest witnessed-finalized block on one parent's main chain.
async fn parent_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ft_threshold: f32,
) -> Result<Floor, CasperError> {
    let mut current = parent.clone();
    let mut walked: usize = 0;
    loop {
        let ft = CliqueOracle::ft_witnessed(&current, dag, latest_messages).await?;
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
