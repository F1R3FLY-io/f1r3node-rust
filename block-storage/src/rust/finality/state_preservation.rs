use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::{
    BlockMetadata, APPLIED_STATE_EFFECTS_PROTOCOL_VERSION, STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION,
};
use models::rust::casper::protocol::casper_message::StateEffectId;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;

#[derive(Default)]
pub struct StateProvenanceCache {
    active: HashMap<(BlockHash, StateEffectId), bool>,
    preservation: HashMap<(BlockHash, BlockHash), bool>,
    ancestry: HashMap<(BlockHash, BlockHash), bool>,
    metadata: HashMap<BlockHash, Arc<BlockMetadata>>,
    validated_exact: HashSet<BlockHash>,
}

pub fn metadata_with_cache(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<Arc<BlockMetadata>, KvStoreError> {
    if let Some(metadata) = cache.metadata.get(block_hash) {
        return Ok(metadata.clone());
    }
    let metadata = Arc::new(dag.lookup_unsafe(block_hash)?);
    cache.metadata.insert(block_hash.clone(), metadata.clone());
    Ok(metadata)
}

pub fn is_dag_ancestor_with_cache(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<bool, KvStoreError> {
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
    metadata: &BlockMetadata,
) -> Result<(), KvStoreError> {
    if metadata.protocol_version < STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION {
        return Err(KvStoreError::InvalidArgument(format!(
            "state-effect provenance is unavailable for block {} at protocol version {}",
            hex::encode(block_hash),
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
) -> Result<Vec<BlockHash>, KvStoreError> {
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
        KvStoreError::InvalidArgument(format!(
            "finalized floor is not materialized for state provenance of block {}",
            hex::encode(block_hash)
        ))
    })?;
    if floor != *block_hash && !inputs.contains(&floor) {
        inputs.push(floor);
    }
    Ok(inputs)
}

fn state_parent(metadata: &BlockMetadata) -> Result<Option<BlockHash>, KvStoreError> {
    if !metadata.merge_base.is_empty() {
        return Ok(Some(metadata.merge_base.clone()));
    }
    match metadata.parents.as_slice() {
        [] => Ok(None),
        [parent] => Ok(Some(parent.clone())),
        _ => Err(KvStoreError::InvalidArgument(format!(
            "protocol-v6 multi-parent block {} has no merge base",
            hex::encode(&metadata.block_hash)
        ))),
    }
}

fn exact_positive_inputs(metadata: &BlockMetadata) -> Result<Vec<BlockHash>, KvStoreError> {
    Ok(state_parent(metadata)?.into_iter().collect())
}

fn validate_exact_state_facts(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    metadata: &BlockMetadata,
    cache: &mut StateProvenanceCache,
) -> Result<Vec<BlockHash>, KvStoreError> {
    let inputs = exact_positive_inputs(metadata)?;
    if cache.validated_exact.contains(block_hash) {
        return Ok(inputs);
    }
    if let Some(parent) = inputs.first() {
        if parent == block_hash || !is_dag_ancestor_with_cache(dag, parent, block_hash, cache)? {
            return Err(KvStoreError::InvalidArgument(format!(
                "state parent {} is not a strict DAG ancestor of protocol-v6 block {}",
                hex::encode(parent),
                hex::encode(block_hash)
            )));
        }
    }
    if !metadata
        .applied_state_effects
        .is_disjoint(&metadata.rejected_state_effects)
    {
        return Err(KvStoreError::InvalidArgument(format!(
            "protocol-v6 block {} both applies and rejects a state effect",
            hex::encode(block_hash)
        )));
    }
    for effect in &metadata.applied_state_effects {
        if effect.source_block_hash == *block_hash
            || !is_dag_ancestor_with_cache(dag, &effect.source_block_hash, block_hash, cache)?
        {
            return Err(KvStoreError::InvalidArgument(format!(
                "applied state effect {}:{} is not from a strict DAG ancestor of block {}",
                hex::encode(&effect.source_block_hash),
                effect.execution_index,
                hex::encode(block_hash)
            )));
        }
        let source = metadata_with_cache(dag, &effect.source_block_hash, cache)?;
        require_effect_provenance(&effect.source_block_hash, &source)?;
        if !source
            .successful_state_effect_indices
            .contains(&effect.execution_index)
        {
            return Err(KvStoreError::InvalidArgument(format!(
                "applied state effect {}:{} has no committed source effect",
                hex::encode(&effect.source_block_hash),
                effect.execution_index
            )));
        }
    }
    cache.validated_exact.insert(block_hash.clone());
    Ok(inputs)
}

pub fn is_state_effect_active_with_cache(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    effect: &StateEffectId,
    cache: &mut StateProvenanceCache,
) -> Result<bool, KvStoreError> {
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
        let exact_positive = metadata.protocol_version >= APPLIED_STATE_EFFECTS_PROTOCOL_VERSION;
        let inputs = if exact_positive {
            validate_exact_state_facts(dag, &current, &metadata, cache)?
        } else {
            state_input_blocks(dag, &current, &metadata.parents, cache)?
        };
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
            let active = if exact_positive {
                own || metadata.applied_state_effects.contains(effect) || inherited
            } else {
                !metadata.rejected_state_effects.contains(effect) && (own || inherited)
            };
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

fn state_lineage_meet(
    dag: &KeyValueDagRepresentation,
    left: &BlockHash,
    right: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<Option<BlockHash>, KvStoreError> {
    let mut left = left.clone();
    let mut right = right.clone();
    loop {
        if left == right {
            return Ok(Some(left));
        }
        let left_metadata = metadata_with_cache(dag, &left, cache)?;
        let right_metadata = metadata_with_cache(dag, &right, cache)?;
        if left_metadata.protocol_version < APPLIED_STATE_EFFECTS_PROTOCOL_VERSION
            || right_metadata.protocol_version < APPLIED_STATE_EFFECTS_PROTOCOL_VERSION
        {
            return Ok(None);
        }
        match left_metadata.block_number.cmp(&right_metadata.block_number) {
            std::cmp::Ordering::Greater => {
                let Some(parent) = validate_exact_state_facts(dag, &left, &left_metadata, cache)?
                    .into_iter()
                    .next()
                else {
                    return Ok(None);
                };
                left = parent;
            }
            std::cmp::Ordering::Less => {
                let Some(parent) = validate_exact_state_facts(dag, &right, &right_metadata, cache)?
                    .into_iter()
                    .next()
                else {
                    return Ok(None);
                };
                right = parent;
            }
            std::cmp::Ordering::Equal => {
                let left_parent = validate_exact_state_facts(dag, &left, &left_metadata, cache)?
                    .into_iter()
                    .next();
                let right_parent = validate_exact_state_facts(dag, &right, &right_metadata, cache)?
                    .into_iter()
                    .next();
                let Some(left_parent) = left_parent else {
                    return Ok(None);
                };
                let Some(right_parent) = right_parent else {
                    return Ok(None);
                };
                left = left_parent;
                right = right_parent;
            }
        }
    }
}

fn segment_introduced_effects(
    dag: &KeyValueDagRepresentation,
    from: &BlockHash,
    meet: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<BTreeSet<StateEffectId>, KvStoreError> {
    let mut effects = BTreeSet::new();
    let mut current = from.clone();
    while current != *meet {
        let metadata = metadata_with_cache(dag, &current, cache)?;
        require_effect_provenance(&current, &metadata)?;
        let state_parent = validate_exact_state_facts(dag, &current, &metadata, cache)?
            .into_iter()
            .next();
        effects.extend(metadata.applied_state_effects.iter().cloned());
        effects.extend(
            metadata
                .successful_state_effect_indices
                .iter()
                .map(|execution_index| StateEffectId {
                    source_block_hash: current.clone(),
                    execution_index: *execution_index,
                }),
        );
        current = state_parent.ok_or_else(|| {
            KvStoreError::InvalidArgument(format!(
                "state lineage of {} reached a root before meet {}",
                hex::encode(from),
                hex::encode(meet)
            ))
        })?;
    }
    Ok(effects)
}

fn exact_positive_state_preserved(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<bool, KvStoreError> {
    let Some(meet) = state_lineage_meet(dag, ancestor, descendant, cache)? else {
        return Ok(false);
    };
    if meet == *ancestor {
        return Ok(true);
    }
    let required = segment_introduced_effects(dag, ancestor, &meet, cache)?;
    if required.is_empty() {
        return Ok(true);
    }
    let carried = segment_introduced_effects(dag, descendant, &meet, cache)?;
    Ok(required.is_subset(&carried))
}

pub fn is_exact_state_contained_with_cache(
    dag: &KeyValueDagRepresentation,
    required: &BlockHash,
    candidate: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<bool, KvStoreError> {
    exact_positive_state_preserved(dag, required, candidate, cache)
}

pub fn is_exact_state_contained(
    dag: &KeyValueDagRepresentation,
    required: &BlockHash,
    candidate: &BlockHash,
) -> Result<bool, KvStoreError> {
    is_exact_state_contained_with_cache(
        dag,
        required,
        candidate,
        &mut StateProvenanceCache::default(),
    )
}

fn potentially_removed_effects(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<BTreeSet<StateEffectId>, KvStoreError> {
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

pub fn is_state_preserved_with_cache(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
    cache: &mut StateProvenanceCache,
) -> Result<bool, KvStoreError> {
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

    let ancestor_metadata = metadata_with_cache(dag, ancestor, cache)?;
    let descendant_metadata = metadata_with_cache(dag, descendant, cache)?;
    if ancestor_metadata.protocol_version >= APPLIED_STATE_EFFECTS_PROTOCOL_VERSION
        && descendant_metadata.protocol_version >= APPLIED_STATE_EFFECTS_PROTOCOL_VERSION
    {
        let preserved = exact_positive_state_preserved(dag, ancestor, descendant, cache)?;
        cache.preservation.insert(key, preserved);
        return Ok(preserved);
    }

    let mut preserved = true;
    for effect in potentially_removed_effects(dag, ancestor, descendant, cache)? {
        if is_state_effect_active_with_cache(dag, ancestor, &effect, cache)?
            && !is_state_effect_active_with_cache(dag, descendant, &effect, cache)?
        {
            preserved = false;
            break;
        }
    }
    cache.preservation.insert(key, preserved);
    Ok(preserved)
}

pub fn is_state_preserved(
    dag: &KeyValueDagRepresentation,
    ancestor: &BlockHash,
    descendant: &BlockHash,
) -> Result<bool, KvStoreError> {
    is_state_preserved_with_cache(
        dag,
        ancestor,
        descendant,
        &mut StateProvenanceCache::default(),
    )
}

pub fn is_state_effect_active(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    effect: &StateEffectId,
) -> Result<bool, KvStoreError> {
    is_state_effect_active_with_cache(
        dag,
        block_hash,
        effect,
        &mut StateProvenanceCache::default(),
    )
}
