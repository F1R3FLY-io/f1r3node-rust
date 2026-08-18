//! Justification-derived finalized floor — the per-block finalized cut.
//!
//! `floor(B)` is the highest sound ancestor of B's parents that holds both the
//! causal clique certificate and the state-preserving clique certificate over
//! B's frozen justification snapshot. Every input is contained in the block
//! itself or in immutable ancestor metadata, so every honest node derives the
//! same floor for the same block. This is the linear-finality analog of
//! RChain's per-message fringe: the cut the block's merge builds on.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{Bond, StateEffectId};
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

#[derive(Default)]
struct StateProvenanceCache {
    active: HashMap<(BlockHash, StateEffectId), bool>,
    preservation: HashMap<(BlockHash, BlockHash), bool>,
    ancestry: HashMap<(BlockHash, BlockHash), bool>,
    metadata: HashMap<BlockHash, Arc<models::rust::block_metadata::BlockMetadata>>,
}

fn metadata_with_cache(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<Arc<models::rust::block_metadata::BlockMetadata>, CasperError> {
    if let Some(metadata) = cache.metadata.get(block_hash) {
        return Ok(metadata.clone());
    }
    let metadata = Arc::new(dag.lookup_unsafe(block_hash)?);
    cache.metadata.insert(block_hash.clone(), metadata.clone());
    Ok(metadata)
}

fn is_dag_ancestor_with_cache(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<bool, CasperError> {
    let key = (ancestor.clone(), descendant.clone());
    if let Some(result) = cache.ancestry.get(&key) {
        return Ok(*result);
    }
    if ancestor == descendant {
        cache.ancestry.insert(key, true);
        return Ok(true);
    }

    let stop_height = dag.block_number_unsafe(ancestor)?;
    let mut visited = HashSet::new();
    let mut stack = vec![descendant.clone()];
    let mut result = false;
    while let Some(current) = stack.pop() {
        if current == *ancestor {
            result = true;
            break;
        }
        if !visited.insert(current.clone()) || dag.block_number_unsafe(&current)? <= stop_height {
            continue;
        }
        let metadata = metadata_with_cache(dag, &current, cache)?;
        stack.extend(metadata.parents.iter().rev().cloned());
    }
    cache.ancestry.insert(key, result);
    Ok(result)
}

fn require_effect_provenance(
    block_hash: &BlockHash,
    metadata: &models::rust::block_metadata::BlockMetadata,
) -> Result<(), CasperError> {
    if metadata.protocol_version < crate::rust::casper::STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION {
        return Err(CasperError::Other(format!(
            "state-effect provenance is unavailable for block {} at protocol version {}",
            PrettyPrinter::build_string_bytes(block_hash),
            metadata.protocol_version
        )));
    }
    Ok(())
}

fn state_input_blocks(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    parents: &[BlockHash],
    cache: &mut StateProvenanceCache,
) -> Result<Vec<BlockHash>, CasperError> {
    if parents.is_empty() {
        return Ok(Vec::new());
    }
    let mut inputs = Vec::with_capacity(parents.len() + 1);
    for candidate in parents {
        let mut covered = false;
        for parent in parents {
            if parent != candidate && is_dag_ancestor_with_cache(dag, candidate, parent, cache)? {
                covered = true;
                break;
            }
        }
        if !covered {
            inputs.push(candidate.clone());
        }
    }
    let floor = dag.get_cached_floor(block_hash)?.ok_or_else(|| {
        CasperError::Other(format!(
            "finalized floor is not materialized for state provenance of block {}",
            PrettyPrinter::build_string_bytes(block_hash)
        ))
    })?;
    if floor != *block_hash && !inputs.contains(&floor) {
        inputs.push(floor);
    }
    Ok(inputs)
}

fn effect_active_with_cache(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    effect: &StateEffectId,
    cache: &mut StateProvenanceCache,
) -> Result<bool, CasperError> {
    let key = (block_hash.clone(), effect.clone());
    if let Some(active) = cache.active.get(&key) {
        return Ok(*active);
    }

    let mut stack = vec![(block_hash.clone(), false)];
    while let Some((current, expanded)) = stack.pop() {
        let current_key = (current.clone(), effect.clone());
        if cache.active.contains_key(&current_key) {
            continue;
        }
        let metadata = metadata_with_cache(dag, &current, cache)?;
        require_effect_provenance(&current, &metadata)?;
        let inputs = state_input_blocks(dag, &current, &metadata.parents, cache)?;
        if expanded {
            let own = current == effect.source_block_hash
                && metadata
                    .successful_state_effect_indices
                    .contains(&effect.execution_index);
            let inherited = inputs.iter().any(|input| {
                cache
                    .active
                    .get(&(input.clone(), effect.clone()))
                    .copied()
                    .unwrap_or(false)
            });
            let active = !metadata.rejected_state_effects.contains(effect) && (own || inherited);
            cache.active.insert(current_key, active);
        } else {
            stack.push((current.clone(), true));
            for input in inputs.iter().rev() {
                if !cache.active.contains_key(&(input.clone(), effect.clone())) {
                    stack.push((input.clone(), false));
                }
            }
        }
    }
    Ok(cache.active.get(&key).copied().unwrap_or(false))
}

fn potentially_removed_effects(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<BTreeSet<StateEffectId>, CasperError> {
    let ancestor_number = dag.block_number_unsafe(ancestor)?;
    let mut effects = BTreeSet::new();
    let mut visited = HashSet::new();
    let mut stack = vec![descendant.clone()];
    while let Some(current) = stack.pop() {
        if current == *ancestor || !visited.insert(current.clone()) {
            continue;
        }
        if dag.block_number_unsafe(&current)? <= ancestor_number {
            continue;
        }
        let metadata = metadata_with_cache(dag, &current, cache)?;
        require_effect_provenance(&current, &metadata)?;
        effects.extend(metadata.rejected_state_effects.iter().cloned());
        for parent in metadata.parents.iter().rev() {
            if parent == ancestor || dag.block_number_unsafe(parent)? > ancestor_number {
                stack.push(parent.clone());
            }
        }
    }
    Ok(effects)
}

pub fn is_state_preserved(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
) -> Result<bool, CasperError> {
    is_state_preserved_with_cache(
        dag,
        ancestor,
        descendant,
        &mut StateProvenanceCache::default(),
    )
}

fn is_state_preserved_with_cache(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<bool, CasperError> {
    let key = (ancestor.clone(), descendant.clone());
    if let Some(preserved) = cache.preservation.get(&key) {
        return Ok(*preserved);
    }
    if ancestor == descendant {
        cache.preservation.insert(key, true);
        return Ok(true);
    }
    if !is_dag_ancestor_with_cache(dag, ancestor, descendant, cache)? {
        cache.preservation.insert(key, false);
        return Ok(false);
    }

    let mut preserved = true;
    for effect in potentially_removed_effects(dag, ancestor, descendant, cache)? {
        if effect_active_with_cache(dag, ancestor, &effect, cache)?
            && !effect_active_with_cache(dag, descendant, &effect, cache)?
        {
            preserved = false;
            break;
        }
    }
    cache.preservation.insert(key, preserved);
    Ok(preserved)
}

pub async fn materialize_finalized_floor(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    ftt: FtThreshold,
) -> Result<(), CasperError> {
    floor_of_block(dag, block_hash, ftt).await?;
    Ok(())
}

pub(crate) async fn materialize_snapshot_floor_closure(
    dag: &KeyValueDagRepresentation,
    block_hashes: impl IntoIterator<Item = BlockHash>,
    ftt: FtThreshold,
) -> Result<(), CasperError> {
    let required = block_hashes.into_iter().collect::<BTreeSet<_>>();
    for block_hash in required {
        materialize_finalized_floor(dag, &block_hash, ftt).await?;
    }
    Ok(())
}

#[cfg(test)]
fn state_supporting_weight_map(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    weight_map: &HashMap<Validator, i64>,
) -> Result<HashMap<Validator, i64>, CasperError> {
    let mut provenance_cache = StateProvenanceCache::default();
    state_supporting_weight_map_with_cache(
        dag,
        target,
        latest_messages,
        weight_map,
        &mut provenance_cache,
    )
}

fn state_supporting_weight_map_with_cache(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    weight_map: &HashMap<Validator, i64>,
    provenance_cache: &mut StateProvenanceCache,
) -> Result<HashMap<Validator, i64>, CasperError> {
    let mut supporting = HashMap::new();
    for (validator, weight) in weight_map {
        let Some(latest) = latest_messages.get(validator) else {
            continue;
        };
        if is_dag_ancestor_with_cache(dag, target, latest, provenance_cache)?
            && is_state_preserved_with_cache(dag, target, latest, provenance_cache).map_err(
                |error| {
                    CasperError::Other(format!(
                        "state-support provenance failed for target {} and latest {}: {error}",
                        PrettyPrinter::build_string_bytes(target),
                        PrettyPrinter::build_string_bytes(latest)
                    ))
                },
            )?
        {
            supporting.insert(validator.clone(), *weight);
        }
    }
    Ok(supporting)
}

pub async fn state_witnessed_exact(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
    strict: bool,
) -> Result<bool, CasperError> {
    state_witnessed_exact_with_cache(
        dag,
        target,
        latest_messages,
        ftt,
        strict,
        &mut StateProvenanceCache::default(),
    )
    .await
}

async fn state_witnessed_exact_with_cache(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
    strict: bool,
    provenance_cache: &mut StateProvenanceCache,
) -> Result<bool, CasperError> {
    if !dag.contains(target) {
        return Ok(false);
    }
    let weight_map = CliqueOracle::get_corresponding_weight_map(target, dag).await?;
    let total_stake = weight_map
        .values()
        .try_fold(0_i64, |sum, weight| sum.checked_add(*weight))
        .ok_or_else(|| CasperError::Other("state-support stake sum overflow".to_string()))?;
    if total_stake <= 0 {
        return Ok(false);
    }
    let supporting = state_supporting_weight_map_with_cache(
        dag,
        target,
        latest_messages,
        &weight_map,
        provenance_cache,
    )?;
    let supporting_stake = supporting
        .values()
        .try_fold(0_i64, |sum, weight| sum.checked_add(*weight))
        .ok_or_else(|| {
            CasperError::Other("state-support agreeing stake sum overflow".to_string())
        })?;
    if (supporting_stake as i128) * 2 <= total_stake as i128 {
        tracing::debug!(
            target: "f1r3.trace.state_oracle",
            candidate = %PrettyPrinter::build_string_bytes(target),
            supporting_stake,
            total_stake,
            decision = false,
            "state-preserving finality verdict"
        );
        return Ok(false);
    }
    let mut run_cache = CliqueOracle::new_run_cache();
    let (decision, _) = CliqueOracle::compute_decision_with_cache(
        target,
        &weight_map,
        &supporting,
        dag,
        &mut run_cache,
        latest_messages,
        ftt.num,
        ftt.den,
        strict,
    )
    .await?;
    tracing::debug!(
        target: "f1r3.trace.state_oracle",
        candidate = %PrettyPrinter::build_string_bytes(target),
        supporting_stake,
        total_stake,
        decision,
        "state-preserving finality verdict"
    );
    Ok(decision)
}

fn state_safe_frontier(
    dag: &KeyValueDagRepresentation,
    raw_frontier: Floor,
) -> Result<Floor, CasperError> {
    let mut chain = vec![raw_frontier.hash];
    while let Some(parent) = dag.main_parent(chain.last().expect("state frontier chain")) {
        chain.push(parent);
    }
    chain.reverse();

    let mut best_hash = chain[0].clone();
    for candidate in chain.into_iter().skip(1) {
        if is_state_preserved(dag, &best_hash, &candidate)? {
            best_hash = candidate;
        }
    }
    Ok(Floor {
        block_number: dag.block_number_unsafe(&best_hash)?,
        hash: best_hash,
    })
}

async fn state_certified_frontier(
    dag: &KeyValueDagRepresentation,
    frontier: Floor,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let mut current = frontier.hash;
    loop {
        let metadata = dag.lookup_unsafe(&current)?;
        if metadata.parents.is_empty()
            || state_witnessed_exact(dag, &current, latest_messages, ftt, false).await?
        {
            return Ok(Floor {
                block_number: metadata.block_number,
                hash: current,
            });
        }
        let Some(base) = dag.main_parent(&current) else {
            return Err(CasperError::Other(format!(
                "state-support frontier cannot descend from non-genesis block {} without a main parent",
                PrettyPrinter::build_string_bytes(&current)
            )));
        };
        current = base;
    }
}

fn candidate_preserves_inherited_floors_with_cache(
    dag: &KeyValueDagRepresentation,
    candidate: &Floor,
    inherited: &[Floor],
    provenance_cache: &mut StateProvenanceCache,
) -> Result<bool, CasperError> {
    for inherited_floor in inherited {
        if candidate.block_number >= inherited_floor.block_number
            && !is_state_preserved_with_cache(
                dag,
                &inherited_floor.hash,
                &candidate.hash,
                provenance_cache,
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn latest_message_coverage(
    dag: &KeyValueDagRepresentation,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    provenance_cache: &mut StateProvenanceCache,
) -> Result<HashMap<BlockHash, BTreeSet<Validator>>, CasperError> {
    let mut queue = BinaryHeap::new();
    let mut queued = HashSet::new();
    let mut processed = HashSet::new();
    let mut coverage: HashMap<BlockHash, BTreeSet<Validator>> = HashMap::new();
    for (validator, latest) in latest_messages {
        let metadata = metadata_with_cache(dag, latest, provenance_cache)?;
        coverage
            .entry(latest.clone())
            .or_default()
            .insert(validator.clone());
        if queued.insert(latest.clone()) {
            queue.push((metadata.block_number, latest.clone()));
        }
    }

    while let Some((_, hash)) = queue.pop() {
        if !queued.remove(&hash) || !processed.insert(hash.clone()) {
            continue;
        }
        let metadata = metadata_with_cache(dag, &hash, provenance_cache)?;
        let current_coverage = coverage.get(&hash).cloned().unwrap_or_default();
        for parent in &metadata.parents {
            let parent_metadata = metadata_with_cache(dag, parent, provenance_cache)?;
            if parent_metadata.block_number >= metadata.block_number {
                return Err(CasperError::Other(format!(
                    "non-descending causal edge in latest-message coverage: {}#{} -> {}#{}",
                    PrettyPrinter::build_string_bytes(&hash),
                    metadata.block_number,
                    PrettyPrinter::build_string_bytes(parent),
                    parent_metadata.block_number,
                )));
            }
            if processed.contains(parent) {
                return Err(CasperError::Other(format!(
                    "late latest-message coverage reached already processed block {}",
                    PrettyPrinter::build_string_bytes(parent),
                )));
            }
            coverage
                .entry(parent.clone())
                .or_default()
                .extend(current_coverage.iter().cloned());
            if queued.insert(parent.clone()) {
                queue.push((parent_metadata.block_number, parent.clone()));
            }
        }
    }
    Ok(coverage)
}

fn corresponding_weight_map_with_cache(
    dag: &KeyValueDagRepresentation,
    target: &BlockHash,
    provenance_cache: &mut StateProvenanceCache,
) -> Result<HashMap<Validator, i64>, CasperError> {
    let target_metadata = metadata_with_cache(dag, target, provenance_cache)?;
    let metadata = if let Some(main_parent) = target_metadata.parents.first() {
        metadata_with_cache(dag, main_parent, provenance_cache)?
    } else {
        target_metadata
    };
    Ok(metadata
        .weight_map
        .iter()
        .map(|(validator, weight)| (validator.clone(), *weight))
        .collect())
}

fn causal_supporting_weight_map(
    target: &BlockHash,
    coverage: &HashMap<BlockHash, BTreeSet<Validator>>,
    weight_map: &HashMap<Validator, i64>,
) -> HashMap<Validator, i64> {
    let supporters = coverage.get(target);
    weight_map
        .iter()
        .filter_map(|(validator, weight)| {
            supporters
                .is_some_and(|validators| validators.contains(validator))
                .then_some((validator.clone(), *weight))
        })
        .collect()
}

async fn dual_certified_universal_frontier(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
    inherited: &[Floor],
    provenance_cache: &mut StateProvenanceCache,
) -> Result<Option<Floor>, CasperError> {
    let mut queue = BinaryHeap::new();
    let mut queued = HashSet::new();
    let mut processed = HashSet::new();
    let mut coverage: HashMap<BlockHash, BTreeSet<usize>> = HashMap::new();
    let latest_coverage = latest_message_coverage(dag, latest_messages, provenance_cache)?;
    let mut clique_cache = CliqueOracle::new_run_cache();
    for (index, parent) in parents.iter().enumerate() {
        let metadata = metadata_with_cache(dag, parent, provenance_cache)?;
        coverage.entry(parent.clone()).or_default().insert(index);
        if queued.insert(parent.clone()) {
            queue.push((metadata.block_number, parent.clone()));
        }
    }

    let mut visited = 0usize;
    while let Some((_, hash)) = queue.pop() {
        if !queued.remove(&hash) || !processed.insert(hash.clone()) {
            continue;
        }
        visited += 1;
        let candidate_metadata = metadata_with_cache(dag, &hash, provenance_cache)?;
        let candidate = Floor {
            hash: candidate_metadata.block_hash.clone(),
            block_number: candidate_metadata.block_number,
        };
        let candidate_is_universal = coverage.get(&hash).map(BTreeSet::len) == Some(parents.len());
        let preserves_inherited = candidate_is_universal
            && candidate_preserves_inherited_floors_with_cache(
                dag,
                &candidate,
                inherited,
                provenance_cache,
            )?;
        let causally_certified = if preserves_inherited {
            let weight_map =
                corresponding_weight_map_with_cache(dag, &candidate.hash, provenance_cache)?;
            let total_stake = weight_map
                .values()
                .try_fold(0_i64, |sum, weight| sum.checked_add(*weight))
                .ok_or_else(|| {
                    CasperError::Other("causal-support stake sum overflow".to_string())
                })?;
            if total_stake <= 0 {
                false
            } else {
                let supporting =
                    causal_supporting_weight_map(&candidate.hash, &latest_coverage, &weight_map);
                CliqueOracle::compute_decision_with_cache(
                    &candidate.hash,
                    &weight_map,
                    &supporting,
                    dag,
                    &mut clique_cache,
                    latest_messages,
                    ftt.num,
                    ftt.den,
                    false,
                )
                .await?
                .0
            }
        } else {
            false
        };
        if causally_certified
            && state_witnessed_exact_with_cache(
                dag,
                &candidate.hash,
                latest_messages,
                ftt,
                false,
                provenance_cache,
            )
            .await?
        {
            tracing::debug!(
                target: "f1r3.trace.floor_walk",
                candidate = %PrettyPrinter::build_string_bytes(&candidate.hash),
                candidate_number = candidate.block_number,
                visited,
                "dual-certified universal frontier"
            );
            return Ok(Some(candidate));
        }

        let current_coverage = coverage.get(&hash).cloned().unwrap_or_default();
        for parent in &candidate_metadata.parents {
            let parent_metadata = metadata_with_cache(dag, parent, provenance_cache)?;
            if parent_metadata.block_number >= candidate_metadata.block_number {
                return Err(CasperError::Other(format!(
                    "non-descending causal edge in universal-floor traversal: {}#{} -> {}#{}",
                    PrettyPrinter::build_string_bytes(&candidate.hash),
                    candidate.block_number,
                    PrettyPrinter::build_string_bytes(parent),
                    parent_metadata.block_number,
                )));
            }
            if processed.contains(parent) {
                return Err(CasperError::Other(format!(
                    "late causal coverage reached already processed universal-floor candidate {}",
                    PrettyPrinter::build_string_bytes(parent),
                )));
            }
            coverage
                .entry(parent.clone())
                .or_default()
                .extend(current_coverage.iter().copied());
            if queued.insert(parent.clone()) {
                queue.push((parent_metadata.block_number, parent.clone()));
            }
        }
    }
    Ok(None)
}

fn can_reuse_linear_parent_universal_frontier(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    inherited: &[Floor],
    provenance_cache: &mut StateProvenanceCache,
) -> Result<bool, CasperError> {
    let ([parent], [inherited_floor]) = (parents, inherited) else {
        return Ok(false);
    };
    if dag.get_cached_floor(parent)?.as_ref() != Some(&inherited_floor.hash) {
        return Ok(false);
    }
    let parent_metadata = metadata_with_cache(dag, parent, provenance_cache)?;
    if parent_metadata.parents.len() != 1 {
        return Ok(false);
    }
    let parent_latest_messages = parent_metadata
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.validator.clone(),
                justification.latest_block_hash.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if parent_latest_messages != *latest_messages {
        return Ok(false);
    }
    for latest in latest_messages.values() {
        if metadata_with_cache(dag, latest, provenance_cache)?.block_number
            >= parent_metadata.block_number
        {
            return Ok(false);
        }
    }
    Ok(true)
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
/// 2. **Advancement** — per parent, derive the highest causally certified
///    main-chain ancestor over the justification snapshot, preserve accepted
///    state effects, and lower it along that main-parent spine until it also has
///    a state-preserving certificate. A block with no main parent is genesis,
///    finalized by definition.
///
/// The floor is the maximum candidate. Both sources are pure functions of the
/// block (parents' floors are themselves block-structural facts), so the
/// result stays node-identical. The selected candidate must be a sound merge
/// base for the complete parent set; an incompatible finalized fork is surfaced
/// as an error, never papered over.
pub async fn finalized_floor(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    materialize_snapshot_floor_closure(
        dag,
        parents.iter().chain(latest_messages.values()).cloned(),
        ftt,
    )
    .await?;
    let mut inherited: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        let hash = dag.get_cached_floor(parent)?.ok_or_else(|| {
            CasperError::Other(format!(
                "snapshot floor closure did not materialize parent {}",
                PrettyPrinter::build_string_bytes(parent)
            ))
        })?;
        inherited.push(Floor {
            block_number: dag.block_number_unsafe(&hash)?,
            hash,
        });
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
    derive_floor_with_cache(
        dag,
        parents,
        latest_messages,
        ftt,
        inherited,
        &mut StateProvenanceCache::default(),
    )
    .await
}

async fn derive_floor_with_cache(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
    inherited: Vec<Floor>,
    provenance_cache: &mut StateProvenanceCache,
) -> Result<(Floor, Floor), CasperError> {
    if parents.is_empty() {
        return Err(CasperError::Other(
            "finalized_floor requires a non-empty parent set; genesis pre-state comes from config"
                .to_string(),
        ));
    }

    let inherited_floors = inherited.clone();
    let mut candidates = inherited;
    let inherited_max = candidates.iter().map(|f| f.block_number).max();
    let mut frontiers: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        frontiers.push(parent_frontier(dag, parent, latest_messages, ftt).await?);
    }
    // parents[0] is the main parent; its frontier over this snapshot is F(B).
    let main_parent_frontier = frontiers[0].clone();
    candidates.extend(frontiers);
    if !can_reuse_linear_parent_universal_frontier(
        dag,
        parents,
        latest_messages,
        &inherited_floors,
        provenance_cache,
    )? {
        if let Some(universal_frontier) = dual_certified_universal_frontier(
            dag,
            parents,
            latest_messages,
            ftt,
            &inherited_floors,
            provenance_cache,
        )
        .await?
        {
            candidates.push(universal_frontier);
        }
    }
    candidates.sort_by(|left, right| {
        left.block_number
            .cmp(&right.block_number)
            .then_with(|| left.hash.cmp(&right.hash))
    });
    candidates.dedup_by(|left, right| left.hash == right.hash);

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
        if !candidate_preserves_inherited_floors_with_cache(
            dag,
            cand,
            &inherited_floors,
            provenance_cache,
        )? {
            continue;
        }
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
    let mut visiting = HashSet::from([block_hash.clone()]);
    let mut provenance_cache = StateProvenanceCache::default();
    while let Some(current) = stack.last().cloned() {
        if dag.get_cached_floor(&current)?.is_some() {
            stack.pop();
            visiting.remove(&current);
            continue;
        }

        let metadata = metadata_with_cache(dag, &current, &mut provenance_cache)?;
        if metadata.parents.is_empty() {
            dag.put_cached_floor(current.clone(), current.clone())?;
            stack.pop();
            visiting.remove(&current);
            continue;
        }

        let mut dependencies = metadata.parents.clone();
        dependencies.extend(
            metadata
                .justifications
                .iter()
                .map(|justification| justification.latest_block_hash.clone()),
        );
        dependencies.sort();
        dependencies.dedup();
        let mut pushed_dependency = false;
        for dependency in dependencies {
            if dag.get_cached_floor(&dependency)?.is_none() {
                if !visiting.insert(dependency.clone()) {
                    return Err(CasperError::Other(format!(
                        "cyclic finalized-floor dependency from block {} to {}",
                        hex::encode(&current),
                        hex::encode(&dependency)
                    )));
                }
                stack.push(dependency);
                pushed_dependency = true;
                break;
            }
        }
        if pushed_dependency {
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
        let (floor, frontier) = derive_floor_with_cache(
            dag,
            &metadata.parents,
            &latest_messages,
            ftt,
            inherited,
            &mut provenance_cache,
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
        visiting.remove(&current);
    }

    let hash = dag
        .get_cached_floor(block_hash)?
        .expect("floor must be cached: the resolution stack drained for this block");
    Ok(Floor {
        block_number: dag.block_number_unsafe(&hash)?,
        hash,
    })
}

/// The highest state-certified floor derived from one parent's causal frontier,
/// over the given justification snapshot.
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
        let parent_metadata = dag.lookup_unsafe(parent)?;
        let parent_latest_messages = parent_metadata
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if parent_latest_messages == *latest_messages {
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_HIT_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            let frontier = state_safe_frontier(dag, Floor {
                block_number: dag.block_number_unsafe(&pivot_hash)?,
                hash: pivot_hash,
            })?;
            return state_certified_frontier(dag, frontier, latest_messages, ftt).await;
        }
        if let Some(frontier) =
            incremental_frontier(dag, parent, &pivot_hash, latest_messages, ftt).await?
        {
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_HIT_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            let frontier = state_safe_frontier(dag, frontier)?;
            return state_certified_frontier(dag, frontier, latest_messages, ftt).await;
        }
    }
    metrics::counter!(
        crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_MISS_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .increment(1);
    let frontier = cold_parent_frontier(dag, parent, latest_messages, ftt).await?;
    let frontier = state_safe_frontier(dag, frontier)?;
    state_certified_frontier(dag, frontier, latest_messages, ftt).await
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
    let pivot_finalized = dag.main_parent(pivot_hash).is_none()
        || CliqueOracle::ft_witnessed_exact(pivot_hash, dag, latest_messages, ftt, false).await?;
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
    trace_frontier(
        parent,
        &best_hash,
        best_number,
        advance as usize,
        "warm-up-walk",
    );
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
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::casper::protocol::casper_message::Justification;
    use parking_lot::RwLock as PlRwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    fn h(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }
    fn val() -> Bytes { Bytes::from(vec![9; 65]) }

    fn effect(source_block_hash: Bytes, execution_index: u32) -> StateEffectId {
        StateEffectId {
            source_block_hash,
            execution_index,
        }
    }

    fn with_successful_effect(mut metadata: BlockMetadata, execution_index: u32) -> BlockMetadata {
        metadata
            .successful_state_effect_indices
            .insert(execution_index);
        metadata
    }

    fn with_rejected_effect(mut metadata: BlockMetadata, rejected: StateEffectId) -> BlockMetadata {
        metadata.rejected_state_effects.insert(rejected);
        metadata
    }

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
            successful_state_effect_indices: Default::default(),
            rejected_state_effects: Default::default(),
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        }
    }

    /// A single-validator linear chain genesis <- b1 <- b2 <- b3, committee {v:1}.
    /// Over the snapshot J = {v -> b2}, the clique oracle finalizes genesis, b1,
    /// and b2 (v's latest message b2 DAG-descends from each), but not b3. So the
    /// frontier of b3 over J is b2.
    fn mk_dag() -> (
        KeyValueDagRepresentation,
        Bytes,
        (Bytes, Bytes, Bytes, Bytes),
    ) {
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
            deploy_occurrence_index: Arc::new(PlRwLock::new(KeyValueTypedStoreImpl::new(
                Arc::new(InMemoryKeyValueStore::new()),
            ))),
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
        assert_eq!(
            floor_cold.hash, b2,
            "derive_floor must select the sound base b2"
        );
        assert_eq!(
            floor_cold, floor_warm,
            "enabling the caches must not change the derived floor (no fork)"
        );
    }

    #[tokio::test]
    async fn finalized_floor_materializes_off_parent_latest_message_provenance() {
        let validator = h(50);
        let (genesis, source, sibling, latest) = (h(0), h(1), h(2), h(3));
        let weights = vec![(validator.clone(), 1)];
        let dag = build_dag(vec![
            md_wm(genesis.clone(), Vec::new(), 0, &validator, weights.clone()),
            with_successful_effect(
                md_wm(
                    source.clone(),
                    vec![genesis.clone()],
                    1,
                    &validator,
                    weights.clone(),
                ),
                0,
            ),
            md_wm(
                sibling.clone(),
                vec![genesis.clone()],
                1,
                &validator,
                weights.clone(),
            ),
            with_rejected_effect(
                md_wm(
                    latest.clone(),
                    vec![source.clone(), sibling],
                    2,
                    &validator,
                    weights,
                ),
                effect(source.clone(), 0),
            ),
        ]);
        let latest_messages = BTreeMap::from([(validator, latest.clone())]);

        let floor = finalized_floor(
            &dag,
            std::slice::from_ref(&source),
            &latest_messages,
            FtThreshold::from_f32_lossy(0.1),
        )
        .await
        .expect("off-parent latest-message provenance must be materialized before selection");

        assert_eq!(floor.hash, genesis);
        assert!(dag.get_cached_floor(&latest).unwrap().is_some());
    }

    #[tokio::test]
    async fn stale_dag_descendant_cannot_advance_inherited_state_floor() {
        let v = h(50);
        let (g, funding, sibling, stale) = (h(0), h(1), h(2), h(3));
        let wm = || vec![(v.clone(), 1)];
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, wm()),
            with_successful_effect(md_wm(funding.clone(), vec![g.clone()], 1, &v, wm()), 0),
            md_wm(sibling.clone(), vec![g.clone()], 1, &v, wm()),
            with_rejected_effect(
                md_wm(
                    stale.clone(),
                    vec![funding.clone(), sibling.clone()],
                    2,
                    &v,
                    wm(),
                ),
                effect(funding.clone(), 0),
            ),
        ]);
        let threshold = FtThreshold::from_f32_lossy(-1.0);
        materialize_finalized_floor(&dag, &stale, threshold)
            .await
            .unwrap();
        assert!(dag.is_dag_ancestor(&funding, &stale).unwrap());
        assert!(!is_state_preserved(&dag, &funding, &stale).unwrap());

        let latest_messages = BTreeMap::from([(v, stale.clone())]);
        let inherited = vec![Floor {
            hash: funding.clone(),
            block_number: 1,
        }];
        let (floor, _) = derive_floor(
            &dag,
            std::slice::from_ref(&stale),
            &latest_messages,
            threshold,
            inherited,
        )
        .await
        .unwrap();
        assert_eq!(floor.hash, funding);
    }

    #[test]
    fn state_frontier_skips_stale_descendant_and_accepts_rebase() {
        let v = h(50);
        let (g, funding, sibling, stale, rebased) = (h(0), h(1), h(2), h(3), h(4));
        let wm = || vec![(v.clone(), 1)];
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, wm()),
            with_successful_effect(md_wm(funding.clone(), vec![g.clone()], 1, &v, wm()), 0),
            md_wm(sibling.clone(), vec![g.clone()], 1, &v, wm()),
            with_rejected_effect(
                md_wm(
                    stale.clone(),
                    vec![funding.clone(), sibling.clone()],
                    2,
                    &v,
                    wm(),
                ),
                effect(funding.clone(), 0),
            ),
            md_wm(rebased.clone(), vec![stale.clone()], 3, &v, wm()),
        ]);
        dag.put_cached_floor(g.clone(), g.clone()).unwrap();
        dag.put_cached_floor(funding.clone(), g.clone()).unwrap();
        dag.put_cached_floor(sibling, g.clone()).unwrap();
        dag.put_cached_floor(stale.clone(), g).unwrap();
        dag.put_cached_floor(rebased.clone(), funding.clone())
            .unwrap();

        assert_eq!(
            state_safe_frontier(&dag, Floor {
                hash: stale,
                block_number: 2,
            },)
            .unwrap()
            .hash,
            funding
        );
        assert_eq!(
            state_safe_frontier(&dag, Floor {
                hash: rebased.clone(),
                block_number: 3,
            },)
            .unwrap()
            .hash,
            rebased
        );
    }

    #[tokio::test]
    async fn causal_merge_vote_cannot_certify_a_rejected_parent_state() {
        let heavy = h(50);
        let source = h(51);
        let other_a = h(52);
        let other_b = h(53);
        let genesis = h(0);
        let rejected_parent = h(1);
        let sibling = h(2);
        let merge = h(3);
        let weights = vec![
            (heavy.clone(), 7),
            (source.clone(), 3),
            (other_a.clone(), 3),
            (other_b.clone(), 3),
        ];
        let dag = build_dag(vec![
            md_wm(genesis.clone(), vec![], 0, &heavy, weights.clone()),
            with_successful_effect(
                md_wm(
                    rejected_parent.clone(),
                    vec![genesis.clone()],
                    1,
                    &source,
                    weights.clone(),
                ),
                0,
            ),
            md_wm(sibling.clone(), vec![genesis.clone()], 1, &other_a, weights),
            with_rejected_effect(
                md_wm(
                    merge.clone(),
                    vec![rejected_parent.clone(), sibling.clone()],
                    2,
                    &heavy,
                    vec![],
                ),
                effect(rejected_parent.clone(), 0),
            ),
        ]);
        seed_state_floors(&dag, [
            (genesis.clone(), genesis.clone()),
            (rejected_parent.clone(), genesis.clone()),
            (sibling.clone(), genesis.clone()),
            (merge.clone(), genesis),
        ]);
        assert!(dag
            .is_dag_ancestor(&rejected_parent, &merge)
            .expect("causal ancestry"));
        assert!(!is_state_preserved(&dag, &rejected_parent, &merge).expect("state ancestry"));

        let latest_messages = BTreeMap::from([
            (heavy, merge),
            (source, rejected_parent.clone()),
            (other_a, sibling.clone()),
            (other_b, sibling),
        ]);
        assert!(!state_witnessed_exact(
            &dag,
            &rejected_parent,
            &latest_messages,
            FtThreshold::from_f32_lossy(0.1),
            false,
        )
        .await
        .expect("state-preserving certificate"));
    }

    #[tokio::test]
    async fn accepted_three_way_merges_retain_state_support_across_repeated_rounds() {
        let validators = [h(50), h(51), h(52)];
        let weights = validators
            .iter()
            .cloned()
            .map(|validator| (validator, 1))
            .collect::<Vec<_>>();
        let (genesis, source, sibling_a, sibling_b) = (h(0), h(1), h(2), h(3));
        let first_round = [h(4), h(5), h(6)];
        let second_round = [h(7), h(8), h(9)];
        let mut blocks = vec![
            md_wm(
                genesis.clone(),
                Vec::new(),
                0,
                &validators[0],
                weights.clone(),
            ),
            with_successful_effect(
                md_wm(
                    source.clone(),
                    vec![genesis.clone()],
                    1,
                    &validators[0],
                    weights.clone(),
                ),
                0,
            ),
            md_wm(
                sibling_a.clone(),
                vec![genesis.clone()],
                1,
                &validators[1],
                weights.clone(),
            ),
            md_wm(
                sibling_b.clone(),
                vec![genesis.clone()],
                1,
                &validators[2],
                weights.clone(),
            ),
        ];
        for (validator, block) in validators.iter().zip(first_round.iter()) {
            blocks.push(md_wm(
                block.clone(),
                vec![source.clone(), sibling_a.clone(), sibling_b.clone()],
                2,
                validator,
                weights.clone(),
            ));
        }
        for (validator, block) in validators.iter().zip(second_round.iter()) {
            let mut metadata = md_wm(
                block.clone(),
                first_round.to_vec(),
                3,
                validator,
                weights.clone(),
            );
            metadata.justifications = validators
                .iter()
                .zip(first_round.iter())
                .map(|(validator, latest_block_hash)| Justification {
                    validator: validator.clone(),
                    latest_block_hash: latest_block_hash.clone(),
                })
                .collect();
            blocks.push(metadata);
        }
        let dag = build_dag(blocks);
        seed_state_floors(
            &dag,
            std::iter::once((genesis.clone(), genesis.clone())).chain(
                [source.clone(), sibling_a, sibling_b]
                    .into_iter()
                    .chain(first_round.iter().cloned())
                    .chain(second_round.iter().cloned())
                    .map(|block| (block, genesis.clone())),
            ),
        );

        for latest in &second_round {
            assert!(dag.is_dag_ancestor(&source, latest).unwrap());
            assert!(is_state_preserved(&dag, &source, latest).unwrap());
        }
        let latest_messages = validators
            .into_iter()
            .zip(second_round)
            .collect::<BTreeMap<_, _>>();
        assert!(state_witnessed_exact(
            &dag,
            &source,
            &latest_messages,
            FtThreshold::from_f32_lossy(0.1),
            true,
        )
        .await
        .unwrap());
    }

    #[test]
    fn state_provenance_is_invariant_under_every_three_parent_order() {
        let validator = h(50);
        let weights = vec![(validator.clone(), 1)];
        let (genesis, source, sibling_a, sibling_b) = (h(0), h(1), h(2), h(3));
        let parent_orders = [
            [source.clone(), sibling_a.clone(), sibling_b.clone()],
            [source.clone(), sibling_b.clone(), sibling_a.clone()],
            [sibling_a.clone(), source.clone(), sibling_b.clone()],
            [sibling_a.clone(), sibling_b.clone(), source.clone()],
            [sibling_b.clone(), source.clone(), sibling_a.clone()],
            [sibling_b.clone(), sibling_a.clone(), source.clone()],
        ];
        let mut blocks = vec![
            md_wm(genesis.clone(), Vec::new(), 0, &validator, weights.clone()),
            with_successful_effect(
                md_wm(
                    source.clone(),
                    vec![genesis.clone()],
                    1,
                    &validator,
                    weights.clone(),
                ),
                0,
            ),
            md_wm(
                sibling_a.clone(),
                vec![genesis.clone()],
                1,
                &validator,
                weights.clone(),
            ),
            md_wm(
                sibling_b.clone(),
                vec![genesis.clone()],
                1,
                &validator,
                weights.clone(),
            ),
        ];
        let mut cases = Vec::new();
        for (index, parents) in parent_orders.into_iter().enumerate() {
            let accepted = h(10 + index as u8);
            let rejected = h(20 + index as u8);
            blocks.push(md_wm(
                accepted.clone(),
                parents.to_vec(),
                2,
                &validator,
                weights.clone(),
            ));
            blocks.push(with_rejected_effect(
                md_wm(
                    rejected.clone(),
                    parents.to_vec(),
                    2,
                    &validator,
                    weights.clone(),
                ),
                effect(source.clone(), 0),
            ));
            cases.push((accepted, rejected));
        }
        let dag = build_dag(blocks);
        seed_state_floors(
            &dag,
            std::iter::once((genesis.clone(), genesis.clone())).chain(
                [source.clone(), sibling_a, sibling_b]
                    .into_iter()
                    .chain(
                        cases
                            .iter()
                            .flat_map(|(accepted, rejected)| [accepted.clone(), rejected.clone()]),
                    )
                    .map(|block| (block, genesis.clone())),
            ),
        );

        for (accepted, rejected) in cases {
            assert!(is_state_preserved(&dag, &source, &accepted).unwrap());
            assert!(!is_state_preserved(&dag, &source, &rejected).unwrap());
        }
    }

    #[test]
    fn unrelated_rejections_in_the_causal_scan_do_not_change_preservation() {
        let validator = h(50);
        let weights = vec![(validator.clone(), 1)];
        let (genesis, source, unrelated, rejected_unrelated, descendant) =
            (h(0), h(1), h(2), h(3), h(4));
        let dag = build_dag(vec![
            md_wm(genesis.clone(), Vec::new(), 0, &validator, weights.clone()),
            with_successful_effect(
                md_wm(
                    source.clone(),
                    vec![genesis.clone()],
                    1,
                    &validator,
                    weights.clone(),
                ),
                0,
            ),
            with_successful_effect(
                md_wm(
                    unrelated.clone(),
                    vec![genesis.clone()],
                    1,
                    &validator,
                    weights.clone(),
                ),
                0,
            ),
            with_rejected_effect(
                md_wm(
                    rejected_unrelated.clone(),
                    vec![unrelated.clone()],
                    2,
                    &validator,
                    weights.clone(),
                ),
                effect(unrelated.clone(), 0),
            ),
            md_wm(
                descendant.clone(),
                vec![source.clone(), rejected_unrelated.clone()],
                3,
                &validator,
                weights,
            ),
        ]);
        seed_state_floors(
            &dag,
            [
                genesis.clone(),
                source.clone(),
                unrelated,
                rejected_unrelated,
                descendant.clone(),
            ]
            .into_iter()
            .map(|block| (block, genesis.clone())),
        );

        assert!(is_state_preserved(&dag, &source, &descendant).unwrap());
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
            successful_state_effect_indices: Default::default(),
            rejected_state_effects: Default::default(),
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
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
        let mut self_justifications = imbl::HashMap::new();
        for b in &blocks {
            dag_set.insert(b.block_hash.clone());
            bnum.insert(b.block_hash.clone(), b.block_number);
            if let Some(main) = b.parents.first() {
                mp.insert(b.block_hash.clone(), main.clone());
            }
            if let Some(justification) = b
                .justifications
                .iter()
                .find(|justification| justification.validator == b.sender)
            {
                self_justifications.insert(
                    b.block_hash.clone(),
                    justification.latest_block_hash.clone(),
                );
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
            self_justification_map: self_justifications,
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            deploy_index: Arc::new(PlRwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                InMemoryKeyValueStore::new(),
            )))),
            deploy_occurrence_index: Arc::new(PlRwLock::new(KeyValueTypedStoreImpl::new(
                Arc::new(InMemoryKeyValueStore::new()),
            ))),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        }
    }

    fn seed_state_floors(
        dag: &KeyValueDagRepresentation,
        floors: impl IntoIterator<Item = (Bytes, Bytes)>,
    ) {
        for (block, floor) in floors {
            dag.put_cached_floor(block, floor).unwrap();
        }
    }

    #[test]
    fn latest_message_coverage_rejects_non_descending_edges() {
        let validator = h(50);
        let parent = h(0);
        let child = h(1);
        let dag = build_dag(vec![
            md_wm(parent.clone(), Vec::new(), 1, &validator, vec![(
                validator.clone(),
                1,
            )]),
            md_wm(child.clone(), vec![parent], 1, &validator, vec![(
                validator.clone(),
                1,
            )]),
        ]);
        let latest_messages = BTreeMap::from([(validator, child)]);
        let result =
            latest_message_coverage(&dag, &latest_messages, &mut StateProvenanceCache::default());
        assert!(matches!(
            result,
            Err(CasperError::Other(message))
                if message.contains("non-descending causal edge")
        ));
    }

    #[test]
    fn universal_frontier_reuse_requires_a_linear_parent_and_unchanged_prior_snapshot() {
        let validator = h(50);
        let genesis = h(0);
        let side = h(1);
        let linear = h(2);
        let merge = h(3);
        let prior = vec![Justification {
            validator: validator.clone(),
            latest_block_hash: genesis.clone(),
        }];
        let mut linear_metadata =
            md_wm(linear.clone(), vec![genesis.clone()], 1, &validator, vec![
                (validator.clone(), 1),
            ]);
        linear_metadata.justifications = prior.clone();
        let mut merge_metadata = md_wm(
            merge.clone(),
            vec![linear.clone(), side.clone()],
            2,
            &validator,
            vec![(validator.clone(), 1)],
        );
        merge_metadata.justifications = prior;
        let dag = build_dag(vec![
            md_wm(genesis.clone(), Vec::new(), 0, &validator, vec![(
                validator.clone(),
                1,
            )]),
            md_wm(side.clone(), vec![genesis.clone()], 1, &validator, vec![(
                validator.clone(),
                1,
            )]),
            linear_metadata,
            merge_metadata,
        ]);
        seed_state_floors(&dag, [
            (genesis.clone(), genesis.clone()),
            (side, genesis.clone()),
            (linear.clone(), genesis.clone()),
            (merge.clone(), genesis.clone()),
        ]);
        let unchanged = BTreeMap::from([(validator.clone(), genesis.clone())]);
        let inherited = [Floor {
            hash: genesis.clone(),
            block_number: 0,
        }];
        assert!(can_reuse_linear_parent_universal_frontier(
            &dag,
            std::slice::from_ref(&linear),
            &unchanged,
            &inherited,
            &mut StateProvenanceCache::default(),
        )
        .unwrap());
        assert!(!can_reuse_linear_parent_universal_frontier(
            &dag,
            std::slice::from_ref(&merge),
            &unchanged,
            &inherited,
            &mut StateProvenanceCache::default(),
        )
        .unwrap());
        let advanced = BTreeMap::from([(validator, linear.clone())]);
        assert!(!can_reuse_linear_parent_universal_frontier(
            &dag,
            std::slice::from_ref(&linear),
            &advanced,
            &inherited,
            &mut StateProvenanceCache::default(),
        )
        .unwrap());
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
        seed_state_floors(&dag, [
            (g.clone(), g.clone()),
            (b1.clone(), g),
            (b2.clone(), b1.clone()),
            (b3.clone(), b2.clone()),
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
        seed_state_floors(&dag, [
            (g.clone(), g.clone()),
            (t.clone(), g.clone()),
            (c.clone(), t.clone()),
            (p1.clone(), c.clone()),
            (p2.clone(), t.clone()),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), c.clone()); // v's frozen latest is c (before it made p1)
        let thr = FtThreshold::from_f32_lossy(0.1);

        let inherited = vec![
            Floor {
                hash: c.clone(),
                block_number: 2,
            },
            Floor {
                hash: t.clone(),
                block_number: 1,
            },
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
            Floor {
                hash: g_a.clone(),
                block_number: 0,
            },
            Floor {
                hash: g_b.clone(),
                block_number: 0,
            },
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
        seed_state_floors(&dag, [
            (g.clone(), g.clone()),
            (t.clone(), g.clone()),
            (c.clone(), t.clone()),
            (p1.clone(), c.clone()),
            (p2.clone(), c.clone()),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), c.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);
        let inherited = vec![
            Floor {
                hash: c.clone(),
                block_number: 2,
            },
            Floor {
                hash: c.clone(),
                block_number: 2,
            },
        ];
        (
            dag,
            j,
            thr,
            vec![p1.clone(), p2.clone()],
            inherited,
            (g, t, c, p1, p2),
        )
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
        assert_eq!(
            floor.hash, c,
            "Case-A selects the common-ancestor finalized candidate c"
        );
        assert!(
            dag.is_dag_ancestor(&floor.hash, &p1)
                .expect("is_dag_ancestor"),
            "the Case-A floor must be a DAG-ancestor of parent p1"
        );
        assert!(
            dag.is_dag_ancestor(&floor.hash, &p2)
                .expect("is_dag_ancestor"),
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
            Floor {
                hash: t.clone(),
                block_number: 1,
            },
            Floor {
                hash: g.clone(),
                block_number: 0,
            },
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

    #[tokio::test]
    async fn derive_floor_promotes_dual_certified_universal_secondary_ancestor() {
        let validators = [h(50), h(51), h(52)];
        let weights = validators
            .iter()
            .cloned()
            .map(|validator| (validator, 10))
            .collect::<Vec<_>>();
        let genesis = h(0);
        let finalized = h(1);
        let side = [h(2), h(3), h(4)];
        let merged = [h(5), h(6), h(7)];
        let tips = [h(8), h(9), h(10)];

        let genesis_justifications = validators
            .iter()
            .map(|validator| Justification {
                validator: validator.clone(),
                latest_block_hash: genesis.clone(),
            })
            .collect::<Vec<_>>();
        let merged_justifications = validators
            .iter()
            .enumerate()
            .map(|(index, validator)| Justification {
                validator: validator.clone(),
                latest_block_hash: merged[index].clone(),
            })
            .collect::<Vec<_>>();

        let mut blocks = vec![md_wm(
            genesis.clone(),
            Vec::new(),
            0,
            &validators[0],
            weights.clone(),
        )];
        let mut finalized_metadata = with_successful_effect(
            md_wm(
                finalized.clone(),
                vec![genesis.clone()],
                1,
                &validators[0],
                weights.clone(),
            ),
            0,
        );
        finalized_metadata.justifications = genesis_justifications.clone();
        blocks.push(finalized_metadata);
        for index in 0..validators.len() {
            let mut side_metadata = md_wm(
                side[index].clone(),
                vec![genesis.clone()],
                1,
                &validators[index],
                weights.clone(),
            );
            side_metadata.justifications = genesis_justifications.clone();
            blocks.push(side_metadata);

            let mut merged_metadata = md_wm(
                merged[index].clone(),
                vec![side[index].clone(), finalized.clone()],
                2,
                &validators[index],
                weights.clone(),
            );
            merged_metadata.justifications = genesis_justifications.clone();
            blocks.push(merged_metadata);
        }
        for index in 0..validators.len() {
            let mut tip_metadata = md_wm(
                tips[index].clone(),
                vec![merged[index].clone()],
                3,
                &validators[index],
                weights.clone(),
            );
            tip_metadata.justifications = merged_justifications.clone();
            blocks.push(tip_metadata);
        }

        let mut rejected_blocks = blocks.clone();
        let finalized_effect = effect(finalized.clone(), 0);
        for rejected_tip in tips.iter().skip(1) {
            let metadata = rejected_blocks
                .iter_mut()
                .find(|metadata| metadata.block_hash == *rejected_tip)
                .expect("rejected tip metadata");
            metadata
                .rejected_state_effects
                .insert(finalized_effect.clone());
        }

        let dag = build_dag(blocks);
        seed_state_floors(
            &dag,
            std::iter::once((genesis.clone(), genesis.clone())).chain(
                std::iter::once((finalized.clone(), genesis.clone())).chain(
                    side.iter()
                        .chain(merged.iter())
                        .chain(tips.iter())
                        .cloned()
                        .map(|block| (block, genesis.clone())),
                ),
            ),
        );
        for tip in &tips {
            assert!(dag.is_dag_ancestor(&finalized, tip).expect("DAG ancestry"));
            assert!(!dag
                .is_in_main_chain(&finalized, tip)
                .expect("main ancestry"));
        }

        let latest_messages = validators
            .iter()
            .cloned()
            .zip(tips.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let threshold = FtThreshold::from_ppm(100_000);
        assert!(CliqueOracle::ft_witnessed_exact(
            &finalized,
            &dag,
            &latest_messages,
            threshold,
            false,
        )
        .await
        .expect("causal certificate"));
        assert!(
            state_witnessed_exact(&dag, &finalized, &latest_messages, threshold, false,)
                .await
                .expect("state certificate")
        );

        for order in [
            [0usize, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let ordered_parents = order.map(|index| tips[index].clone());
            let inherited = ordered_parents
                .iter()
                .map(|_| Floor {
                    hash: genesis.clone(),
                    block_number: 0,
                })
                .collect();
            let (floor, _) = derive_floor(
                &dag,
                &ordered_parents,
                &latest_messages,
                threshold,
                inherited,
            )
            .await
            .expect("derive floor");
            assert_eq!(floor.hash, finalized);
        }

        let rejected_dag = build_dag(rejected_blocks);
        seed_state_floors(
            &rejected_dag,
            std::iter::once((genesis.clone(), genesis.clone())).chain(
                std::iter::once((finalized.clone(), genesis.clone())).chain(
                    side.iter()
                        .chain(merged.iter())
                        .chain(tips.iter())
                        .cloned()
                        .map(|block| (block, genesis.clone())),
                ),
            ),
        );
        assert!(CliqueOracle::ft_witnessed_exact(
            &finalized,
            &rejected_dag,
            &latest_messages,
            threshold,
            false,
        )
        .await
        .expect("causal certificate"));
        assert!(!state_witnessed_exact(
            &rejected_dag,
            &finalized,
            &latest_messages,
            threshold,
            false,
        )
        .await
        .expect("state certificate"));
        let inherited = tips
            .iter()
            .map(|_| Floor {
                hash: genesis.clone(),
                block_number: 0,
            })
            .collect();
        let (floor, _) = derive_floor(&rejected_dag, &tips, &latest_messages, threshold, inherited)
            .await
            .expect("derive floor");
        assert_eq!(floor.hash, genesis);
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

    prop_compose! {
        fn state_lineage_scenario()(stale_len in 1usize..=8, safe_len in 0usize..=8)
            -> (usize, usize) {
            (stale_len, safe_len)
        }
    }

    fn state_effect_recurrence_scenario() -> impl Strategy<Value = Vec<(bool, bool)>> {
        prop::collection::vec((any::<bool>(), any::<bool>()), 1..=12)
    }

    fn universal_floor_scenario() -> impl Strategy<Value = (Vec<usize>, Vec<usize>, [usize; 3])> {
        (
            prop::collection::vec(1usize..=4, 3),
            prop::collection::vec(0usize..=4, 3),
            prop::sample::select(vec![
                [0usize, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ]),
        )
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
                seed_state_floors(
                    &dag,
                    std::iter::once((h(0), h(0)))
                        .chain((1..=depth).map(|i| (h(i as u8), h((i - 1) as u8)))),
                );

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

        #[test]
        fn dual_certified_universal_floor_is_independent_of_branch_shape_and_parent_order(
            (side_depths, tail_depths, order) in universal_floor_scenario()
        ) {
            FLOOR_RUNTIME.block_on(async move {
                let validators = [h(240), h(241), h(242)];
                let weights = validators
                    .iter()
                    .cloned()
                    .map(|validator| (validator, 10))
                    .collect::<Vec<_>>();
                let genesis = h(0);
                let finalized = h(1);
                let genesis_justifications = validators
                    .iter()
                    .map(|validator| Justification {
                        validator: validator.clone(),
                        latest_block_hash: genesis.clone(),
                    })
                    .collect::<Vec<_>>();
                let mut blocks = vec![md_wm(
                    genesis.clone(),
                    Vec::new(),
                    0,
                    &validators[0],
                    weights.clone(),
                )];
                let mut finalized_metadata = with_successful_effect(
                    md_wm(
                        finalized.clone(),
                        vec![genesis.clone()],
                        1,
                        &validators[0],
                        weights.clone(),
                    ),
                    0,
                );
                finalized_metadata.justifications = genesis_justifications.clone();
                blocks.push(finalized_metadata);

                let bases = [10u8, 80, 150];
                let mut anchors = Vec::with_capacity(3);
                let mut tips = Vec::with_capacity(3);
                for index in 0..3 {
                    let mut previous = genesis.clone();
                    for offset in 0..side_depths[index] {
                        let block = h(bases[index] + offset as u8);
                        let mut metadata = md_wm(
                            block.clone(),
                            vec![previous],
                            (offset + 1) as i64,
                            &validators[index],
                            weights.clone(),
                        );
                        metadata.justifications = genesis_justifications.clone();
                        blocks.push(metadata);
                        previous = block;
                    }
                    let merged = h(bases[index] + 8);
                    let mut metadata = md_wm(
                        merged.clone(),
                        vec![previous, finalized.clone()],
                        (side_depths[index] + 1) as i64,
                        &validators[index],
                        weights.clone(),
                    );
                    metadata.justifications = genesis_justifications.clone();
                    blocks.push(metadata);
                    previous = merged;

                    for offset in 0..tail_depths[index] {
                        let block = h(bases[index] + 9 + offset as u8);
                        let mut metadata = md_wm(
                            block.clone(),
                            vec![previous],
                            (side_depths[index] + offset + 2) as i64,
                            &validators[index],
                            weights.clone(),
                        );
                        metadata.justifications = genesis_justifications.clone();
                        blocks.push(metadata);
                        previous = block;
                    }
                    anchors.push(previous.clone());

                    let tip = h(bases[index] + 13);
                    let mut metadata = md_wm(
                        tip.clone(),
                        vec![previous],
                        (side_depths[index] + tail_depths[index] + 2) as i64,
                        &validators[index],
                        weights.clone(),
                    );
                    metadata.justifications = genesis_justifications.clone();
                    blocks.push(metadata);
                    tips.push(tip);
                }

                let frozen_justifications = validators
                    .iter()
                    .enumerate()
                    .map(|(index, validator)| Justification {
                        validator: validator.clone(),
                        latest_block_hash: anchors[index].clone(),
                    })
                    .collect::<Vec<_>>();
                for tip in &tips {
                    blocks
                        .iter_mut()
                        .find(|metadata| metadata.block_hash == *tip)
                        .expect("tip metadata")
                        .justifications = frozen_justifications.clone();
                }

                let block_hashes = blocks
                    .iter()
                    .map(|metadata| metadata.block_hash.clone())
                    .collect::<Vec<_>>();
                let dag = build_dag(blocks);
                seed_state_floors(
                    &dag,
                    block_hashes
                        .iter()
                        .cloned()
                        .map(|block| (block, genesis.clone())),
                );
                let latest_messages = validators
                    .iter()
                    .cloned()
                    .zip(tips.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                let threshold = FtThreshold::from_ppm(100_000);

                let mut provenance_cache = StateProvenanceCache::default();
                let coverage = latest_message_coverage(
                    &dag,
                    &latest_messages,
                    &mut provenance_cache,
                )
                .expect("latest-message coverage");
                for target in &block_hashes {
                    let pairwise_supporters = latest_messages
                        .iter()
                        .filter_map(|(validator, latest)| {
                            dag.is_dag_ancestor(target, latest)
                                .expect("pairwise DAG ancestry")
                                .then_some(validator.clone())
                        })
                        .collect::<BTreeSet<_>>();
                    prop_assert_eq!(
                        coverage.get(target).cloned().unwrap_or_default(),
                        pairwise_supporters.clone(),
                    );

                    let weight_map = corresponding_weight_map_with_cache(
                        &dag,
                        target,
                        &mut provenance_cache,
                    )
                    .expect("cached corresponding weight map");
                    let oracle_weight_map = CliqueOracle::get_corresponding_weight_map(target, &dag)
                        .await
                        .expect("oracle corresponding weight map");
                    prop_assert_eq!(&weight_map, &oracle_weight_map);
                    let optimized_support =
                        causal_supporting_weight_map(target, &coverage, &weight_map);
                    let pairwise_support = weight_map
                        .iter()
                        .filter_map(|(validator, weight)| {
                            pairwise_supporters
                                .contains(validator)
                                .then_some((validator.clone(), *weight))
                        })
                        .collect::<HashMap<_, _>>();
                    prop_assert_eq!(&optimized_support, &pairwise_support);

                    let mut clique_cache = CliqueOracle::new_run_cache();
                    let optimized_decision = CliqueOracle::compute_decision_with_cache(
                        target,
                        &weight_map,
                        &optimized_support,
                        &dag,
                        &mut clique_cache,
                        &latest_messages,
                        threshold.num,
                        threshold.den,
                        false,
                    )
                    .await
                    .expect("optimized causal decision")
                    .0;
                    let pairwise_decision = CliqueOracle::ft_witnessed_exact(
                        target,
                        &dag,
                        &latest_messages,
                        threshold,
                        false,
                    )
                    .await
                    .expect("pairwise causal decision");
                    prop_assert_eq!(optimized_decision, pairwise_decision);
                }

                prop_assert!(CliqueOracle::ft_witnessed_exact(
                    &finalized,
                    &dag,
                    &latest_messages,
                    threshold,
                    false,
                )
                .await
                .expect("causal certificate"));
                prop_assert!(state_witnessed_exact(
                    &dag,
                    &finalized,
                    &latest_messages,
                    threshold,
                    false,
                )
                .await
                .expect("state certificate"));
                for tip in &tips {
                    prop_assert!(dag.is_dag_ancestor(&finalized, tip).expect("DAG ancestry"));
                    prop_assert!(!dag
                        .is_in_main_chain(&finalized, tip)
                        .expect("main ancestry"));
                }

                let ordered_parents = order.map(|index| tips[index].clone());
                let inherited = ordered_parents
                    .iter()
                    .map(|_| Floor {
                        hash: genesis.clone(),
                        block_number: 0,
                    })
                    .collect();
                let (floor, _) = derive_floor(
                    &dag,
                    &ordered_parents,
                    &latest_messages,
                    threshold,
                    inherited,
                )
                .await
                .expect("derive floor");
                prop_assert_eq!(floor.hash, finalized);
                Ok::<(), TestCaseError>(())
            })?;
        }

        #[test]
        fn state_safe_frontier_is_monotone_across_stale_merges_and_rebases(
            (stale_len, safe_len) in state_lineage_scenario()
        ) {
            let validator = h(50);
            let genesis = h(0);
            let funding = h(1);
            let mut blocks = vec![
                md_wm(genesis.clone(), Vec::new(), 0, &validator, vec![(validator.clone(), 1)]),
                with_successful_effect(
                    md_wm(
                        funding.clone(),
                        vec![genesis.clone()],
                        1,
                        &validator,
                        vec![(validator.clone(), 1)],
                    ),
                    0,
                ),
            ];
            let mut previous = funding.clone();
            let mut stale_hashes = Vec::with_capacity(stale_len);
            let mut side_hashes = Vec::with_capacity(stale_len);
            for offset in 0..stale_len {
                let side = h((100 + offset) as u8);
                let stale = h((2 + offset) as u8);
                blocks.push(md_wm(
                    side.clone(),
                    vec![genesis.clone()],
                    1,
                    &validator,
                    vec![(validator.clone(), 1)],
                ));
                blocks.push(with_rejected_effect(
                    md_wm(
                        stale.clone(),
                        vec![previous, side.clone()],
                        (2 + offset) as i64,
                        &validator,
                        vec![(validator.clone(), 1)],
                    ),
                    effect(funding.clone(), 0),
                ));
                previous = stale.clone();
                stale_hashes.push(stale);
                side_hashes.push(side);
            }
            let rebased = h((2 + stale_len) as u8);
            blocks.push(md_wm(
                rebased.clone(),
                vec![previous],
                (2 + stale_len) as i64,
                &validator,
                vec![(validator.clone(), 1)],
            ));
            let mut safe_hashes = vec![rebased.clone()];
            let mut previous = rebased;
            for offset in 0..safe_len {
                let safe = h((3 + stale_len + offset) as u8);
                blocks.push(md_wm(
                    safe.clone(),
                    vec![previous],
                    (3 + stale_len + offset) as i64,
                    &validator,
                    vec![(validator.clone(), 1)],
                ));
                previous = safe.clone();
                safe_hashes.push(safe);
            }

            let dag = build_dag(blocks);
            dag.put_cached_floor(genesis.clone(), genesis.clone()).unwrap();
            dag.put_cached_floor(funding.clone(), genesis.clone()).unwrap();
            for side in &side_hashes {
                dag.put_cached_floor(side.clone(), genesis.clone()).unwrap();
            }
            for stale in &stale_hashes {
                dag.put_cached_floor(stale.clone(), genesis.clone()).unwrap();
            }
            for safe in &safe_hashes {
                dag.put_cached_floor(safe.clone(), funding.clone()).unwrap();
            }

            for (offset, stale) in stale_hashes.iter().enumerate() {
                let frontier = state_safe_frontier(
                    &dag,
                    Floor {
                        hash: stale.clone(),
                        block_number: (2 + offset) as i64,
                    },
                )
                .unwrap();
                prop_assert_eq!(frontier.hash, funding.clone());
                let latest = BTreeMap::from([(validator.clone(), stale.clone())]);
                let weights = HashMap::from([(validator.clone(), 1)]);
                prop_assert!(state_supporting_weight_map(
                    &dag,
                    &funding,
                    &latest,
                    &weights,
                )
                .unwrap()
                .is_empty());
            }
            for (offset, safe) in safe_hashes.iter().enumerate() {
                let frontier = state_safe_frontier(
                    &dag,
                    Floor {
                        hash: safe.clone(),
                        block_number: (2 + stale_len + offset) as i64,
                    },
                )
                .unwrap();
                prop_assert_eq!(frontier.hash, safe.clone());
                prop_assert!(is_state_preserved(&dag, &funding, safe).unwrap());
                let latest = BTreeMap::from([(validator.clone(), safe.clone())]);
                let weights = HashMap::from([(validator.clone(), 1)]);
                let supporting =
                    state_supporting_weight_map(&dag, &funding, &latest, &weights).unwrap();
                prop_assert_eq!(supporting.get(&validator), Some(&1));
            }
        }

        #[test]
        fn active_effect_recurrence_matches_arbitrary_reject_and_restore_sequences(
            steps in state_effect_recurrence_scenario()
        ) {
            let validator = h(50);
            let weights = vec![(validator.clone(), 1)];
            let genesis = h(0);
            let source = h(1);
            let source_effect = effect(source.clone(), 0);
            let mut blocks = vec![
                md_wm(genesis.clone(), Vec::new(), 0, &validator, weights.clone()),
                with_successful_effect(
                    md_wm(
                        source.clone(),
                        vec![genesis.clone()],
                        1,
                        &validator,
                        weights.clone(),
                    ),
                    0,
                ),
            ];
            let mut floors = vec![
                (genesis.clone(), genesis.clone()),
                (source.clone(), genesis.clone()),
            ];
            let mut previous = source.clone();
            let mut expected_active = true;
            let mut expected = Vec::with_capacity(steps.len());
            for (index, (reject, restore)) in steps.into_iter().enumerate() {
                let block = h((index + 2) as u8);
                let metadata = md_wm(
                    block.clone(),
                    vec![previous],
                    (index + 2) as i64,
                    &validator,
                    weights.clone(),
                );
                blocks.push(if reject {
                    with_rejected_effect(metadata, source_effect.clone())
                } else {
                    metadata
                });
                floors.push((
                    block.clone(),
                    if restore {
                        source.clone()
                    } else {
                        genesis.clone()
                    },
                ));
                expected_active = !reject && (expected_active || restore);
                expected.push((block.clone(), expected_active));
                previous = block;
            }

            let dag = build_dag(blocks);
            seed_state_floors(&dag, floors);
            for (block, active) in expected {
                prop_assert_eq!(is_state_preserved(&dag, &source, &block).unwrap(), active);
            }
        }
    }
}
