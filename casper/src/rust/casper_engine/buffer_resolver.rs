//! Dependency-free pendants and buffer queries.
//!
//! Phase 3 Step 5 — extracted from `multi_parent_casper_impl.rs`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use std::collections::HashSet;

use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;

use super::block_admission::admit_dag_contains;
use super::types::MultiParentCasperImpl;

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
        })
        .map_err(|e| CasperError::RuntimeError(e.to_string()))?;

    let invalid_block_hashes: HashSet<BlockHash> = this
        .block_dag_storage
        .get_representation()
        .map_err(|e| CasperError::RuntimeError(e.to_string()))?
        .invalid_blocks_map()
        .map_err(|e| CasperError::RuntimeError(e.to_string()))?
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

    // Keep only candidates that exist in block store.
    let mut candidates_stored = Vec::new();
    for candidate_hash in candidate_hashes {
        if this.block_store.get(&candidate_hash)?.is_some() {
            candidates_stored.push(candidate_hash);
        }
    }

    // Filter to dependency-free candidates by real block dependencies.
    let mut dep_free_pendants = Vec::new();
    for candidate_hash in candidates_stored {
        // P2-7: surface missing block as a typed error.
        let block = this.block_store.get(&candidate_hash)?.ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "block {} disappeared from store between is_some() and get()",
                PrettyPrinter::build_string_bytes(&candidate_hash)
            ))
        })?;
        let all_deps = proto_util::dependencies_hashes_of(&block);
        let all_deps_available = all_deps.into_iter().all(|dep| {
            admit_dag_contains(this, &dep)
                || equivocation_hashes.contains(&dep)
                || invalid_block_hashes.contains(&dep)
        });

        if all_deps_available {
            dep_free_pendants.push(candidate_hash);
        }
    }

    // Get the actual BlockMessages.
    let result = dep_free_pendants
        .into_iter()
        .map(|hash| this.block_store.get(&hash))
        .collect::<Result<Option<Vec<_>>, _>>()?
        .ok_or_else(|| CasperError::RuntimeError("Failed to get blocks from store".to_string()))?;

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
