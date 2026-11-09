//! What a node knows about validator liveness: its own view of every
//! validator's bucket, and what its last finalized block's state records of
//! claims, ejections and stake.

use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::validator::Validator;

use crate::rust::economics::bucket_view::{LeakyBucketParams, LeakyBucketProposal, ViewSnapshot};
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::rholang::runtime::FloorState;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub struct ValidatorLiveness {
    pub params: LeakyBucketParams,
    /// Node-local: depends on which blocks this node accepted, and in what
    /// order.
    pub local_view: ViewSnapshot,
    /// What this node would offer and vote in its next block; a node without
    /// a validator identity proposes nothing.
    pub decision: Option<LeakyBucketProposal>,
    pub finalized_state: FinalizedLiveness,
}

/// The state of a node's last finalized block.
pub struct FinalizedLiveness {
    pub block_hash: BlockHash,
    pub block_number: i64,
    pub state: Arc<FloorState>,
    /// Each ejected validator and the last block of its grace, in identity
    /// order.
    pub grace_ends: Vec<(Validator, i64)>,
}

pub async fn validator_liveness(engine_cell: &EngineCell) -> eyre::Result<ValidatorLiveness> {
    let engine = engine_cell.get().await;
    let casper = engine
        .with_casper()
        .ok_or_else(|| eyre::eyre!("casper is not available yet"))?;
    let view = casper.bucket_view();
    let local_view = view.snapshot();
    let decision = casper.get_validator().map(|identity| {
        view.proposal(&prost::bytes::Bytes::copy_from_slice(
            &identity.public_key.bytes,
        ))
    });
    let finalized = casper.last_finalized_block().await?;
    let finalized_state = finalized_liveness(
        &casper.runtime_manager(),
        finalized.block_hash.clone(),
        finalized.body.state.block_number,
        &finalized.body.state.post_state_hash,
    )
    .await?;
    Ok(ValidatorLiveness {
        params: *view.params(),
        local_view,
        decision,
        finalized_state,
    })
}

async fn finalized_liveness(
    runtime_manager: &RuntimeManager,
    block_hash: BlockHash,
    block_number: i64,
    post_state_hash: &BlockHash,
) -> eyre::Result<FinalizedLiveness> {
    let state = runtime_manager.floor_state(post_state_hash).await?;
    let mut grace_ends = runtime_manager
        .claim_book(post_state_hash)
        .await?
        .grace_ends
        .clone();
    grace_ends.sort();
    Ok(FinalizedLiveness {
        block_hash,
        block_number,
        state,
        grace_ends,
    })
}
