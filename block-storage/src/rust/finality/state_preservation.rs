use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::{BlockMetadata, STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION};
use models::rust::casper::protocol::casper_message::StateEffectId;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;

#[derive(Default)]
pub struct StateProvenanceCache {
    active: HashMap<(BlockHash, StateEffectId), bool>,
    preservation: HashMap<(BlockHash, BlockHash), bool>,
    ancestry: HashMap<(BlockHash, BlockHash), bool>,
    metadata: HashMap<BlockHash, Arc<BlockMetadata>>,
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

fn effect_active_with_cache(
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
