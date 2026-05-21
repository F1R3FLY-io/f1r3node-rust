//! Dependency-free pendants and buffer queries.
//!
//! Phase 3 Step 5 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use std::collections::{HashMap, HashSet};

use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;

use super::block_admission::admit_dag_contains;
use super::types::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;

pub(crate) fn buffer_get_dependency_free_from_buffer<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<BlockMessage>, CasperError> {
    let equivocation_hashes: HashSet<BlockHash> = this
        .block_dag_storage
        .access_equivocations_tracker(|tracker| {
            let equivocation_records = tracker.data()?;
            let hashes: HashSet<BlockHash> = equivocation_records
                .iter()
                .flat_map(|record| record.equivocation_detected_block_hashes.iter())
                .cloned()
                .collect();
            Ok(hashes)
        })?;

    let invalid_block_hashes: HashSet<BlockHash> = this
        .block_dag_storage
        .get_representation()?
        .invalid_blocks_map()?
        .into_keys()
        .collect();

    // Build candidate set from both pendants and buffered children.
    let mut candidate_hashes: HashSet<BlockHash> = HashSet::new();

    let pendants = this.casper_buffer_storage.get_pendants();
    for pendant_serde in pendants.iter() {
        candidate_hashes.insert(BlockHash::from(pendant_serde.0.clone()));
    }

    let buffer_dag = this.casper_buffer_storage.to_doubly_linked_dag();
    for (child_hash, _) in buffer_dag.child_to_parent_adjacency_list.iter() {
        candidate_hashes.insert(BlockHash::from(child_hash.0.clone()));
    }

    // C14 / Perf-5: read each candidate block from store exactly once
    // and reuse the materialized `BlockMessage` across the dependency
    // check and the final result construction. Prior to this commit
    // every candidate was read three times — once for `.is_some()`,
    // once to inspect `dependencies_hashes_of`, and once to build the
    // result Vec. The middle read also carried a defensive "block
    // disappeared from store between is_some() and get()" branch that
    // was unreachable (the buffer's state lock isn't held across the
    // gap; a concurrent eviction is in principle observable) but
    // becomes trivially unreachable when each block is read only
    // once.
    let mut blocks_in_store: HashMap<BlockHash, BlockMessage> =
        HashMap::with_capacity(candidate_hashes.len());
    for candidate_hash in candidate_hashes {
        if let Some(block) = this.block_store.get(&candidate_hash)? {
            blocks_in_store.insert(candidate_hash, block);
        }
    }

    let mut dep_free_keys: Vec<BlockHash> = Vec::with_capacity(blocks_in_store.len());
    for (candidate_hash, block) in &blocks_in_store {
        let all_deps = proto_util::dependencies_hashes_of(block);
        let all_deps_available = all_deps.into_iter().all(|dep| {
            admit_dag_contains(this, &dep)
                || equivocation_hashes.contains(&dep)
                || invalid_block_hashes.contains(&dep)
        });

        if all_deps_available {
            dep_free_keys.push(candidate_hash.clone());
        }
    }

    let mut result: Vec<BlockMessage> = Vec::with_capacity(dep_free_keys.len());
    for hash in dep_free_keys {
        if let Some(block) = blocks_in_store.remove(&hash) {
            result.push(block);
        }
    }

    Ok(result)
}

pub(crate) fn buffer_get_all_from_buffer<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<BlockMessage>, CasperError> {
    let dag = this.casper_buffer_storage.to_doubly_linked_dag();
    let all_hashes = dag
        .child_to_parent_adjacency_list
        .keys()
        .map(|hash| BlockHash::from(hash.clone()));

    let mut blocks = Vec::new();
    for hash in all_hashes {
        if let Some(block) = this.block_store.get(&hash)? {
            blocks.push(block);
        }
    }

    Ok(blocks)
}
