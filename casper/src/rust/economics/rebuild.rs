//! Rebuilding a node's bucket view from the blocks it holds.
//!
//! The view lives in memory, so a node that restarts, or that joined from a
//! snapshot, holds blocks it never observed. Each is replayed through the
//! same observation an accepted block gets, oldest first, after the view has
//! learned what the node's finalized block records.

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;

use super::bucket_view::{BucketView, Observation};
use super::reward::fresh_in;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Begins `view` at the last finalized block's state, crediting from the
/// retention window below that block, then observes every valid block held
/// from there upwards, by height and then by hash.
///
/// A block with a parent, or a previous block of its sender, that this node
/// does not hold is passed over: its reward cannot be recomputed.
pub async fn rebuild(
    view: &BucketView,
    dag: &KeyValueDagRepresentation,
    blocks: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
) -> Result<(), CasperError> {
    let finalized = held(blocks, &dag.last_finalized_block())?;
    let floor = runtime_manager
        .floor_state(&finalized.body.state.post_state_hash)
        .await?;
    let from = view.params().retention_window_blocks.map_or(0, |window| {
        finalized
            .body
            .state
            .block_number
            .saturating_sub(window)
            .max(0)
    });
    view.begin(&floor, finalized.body.state.block_number, from);
    for mut height in dag.topo_sort(from, None)? {
        height.sort();
        for hash in height {
            let metadata = dag.lookup_unsafe(&hash)?;
            if metadata.is_invalid() || metadata.parents.is_empty() {
                continue;
            }
            let mut parents = Vec::with_capacity(metadata.parents.len());
            for parent in &metadata.parents {
                match dag.lookup(parent)? {
                    Some(parent) => parents.push(parent),
                    None => break,
                }
            }
            if parents.len() != metadata.parents.len() {
                continue;
            }
            let fresh = match fresh_in(
                dag,
                &metadata.sender,
                &metadata.justifications,
                metadata.block_number,
                view.params().reward.previous_reach,
            ) {
                Ok(fresh) => fresh,
                Err(CasperError::BlockNotHeld(..)) => continue,
                Err(error) => return Err(error),
            };
            view.observe(&Observation::of(&held(blocks, &hash)?, &parents, fresh));
        }
    }
    Ok(())
}

fn held(
    blocks: &KeyValueBlockStore,
    hash: &BlockHash,
) -> Result<models::rust::casper::protocol::casper_message::BlockMessage, CasperError> {
    blocks.get(hash)?.ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "the body of held block {} is missing",
            hex::encode(hash)
        ))
    })
}
