//! Stateless block-event helpers and the deploy-future predicate.
//!
//! Phase 3 Step 1 — extracted from `multi_parent_casper_impl.rs`. These
//! items are pure functions that do not touch `MultiParentCasperImpl` state.
//!
//! * `created_event` / `added_event` / `finalised_event` — `pub` because
//!   external callers (e.g. `blocks/proposer/proposer.rs`) import them
//!   through the `multi_parent_casper_impl::` re-export shim.
//! * `pending_deploy_is_future_for_next_block` — `pub(super)` so the
//!   sibling `dispatch` module's `has_pending_deploys_in_storage_for_snapshot`
//!   can call it without exposing it crate-wide.

use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::shared::f1r3fly_event::{DeployEvent, F1r3flyEvent};

/// Returns `true` when a pending deploy's `valid_after_block_number`
/// strictly exceeds the latest block in the DAG — i.e. the deploy cannot
/// yet be included in the *next* block. Caller:
/// [`super::dispatch::MultiParentCasper::has_pending_deploys_in_storage_for_snapshot`].
///
/// `pub(super)` because the only caller is in a sibling sub-module of
/// `casper_engine`. Phase 7 (C-2) demoted this from `pub(crate)` once the
/// Phase 3 migration's intermediate caller (in the now-shim
/// `multi_parent_casper_impl`) was removed.
#[inline]
pub(super) fn pending_deploy_is_future_for_next_block(
    latest_block_number: i64,
    valid_after_block_number: i64,
) -> bool {
    valid_after_block_number > latest_block_number
}

/// Extract common block event data.
fn block_event(
    block: &BlockMessage,
) -> (
    String,
    i64,
    i64,
    Vec<String>,
    Vec<(String, String)>,
    Vec<DeployEvent>,
    String,
    i32,
) {
    let block_hash = hex::encode(block.block_hash.clone());

    let parent_hashes = block
        .header
        .parents_hash_list
        .iter()
        .map(hex::encode)
        .collect::<Vec<_>>();

    let justification_hashes = block
        .justifications
        .iter()
        .map(|j| {
            (
                hex::encode(j.validator.clone()),
                hex::encode(j.latest_block_hash.clone()),
            )
        })
        .collect::<Vec<_>>();

    // Build DeployEvent with full information
    let deploys = block
        .body
        .deploys
        .iter()
        .map(|pd| {
            DeployEvent::new(
                hex::encode(pd.deploy.sig.clone()),
                pd.cost.cost as i64,
                hex::encode(pd.deploy.pk.bytes.clone()),
                pd.is_failed,
            )
        })
        .collect::<Vec<_>>();

    let block_number = block.body.state.block_number;
    let timestamp = block.header.timestamp;
    let creator = hex::encode(block.sender.clone());
    let seq_num = block.seq_num;

    (
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    )
}

/// Create BlockCreated event for a block.
pub fn created_event(block: &BlockMessage) -> F1r3flyEvent {
    let (
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    ) = block_event(block);
    F1r3flyEvent::block_created(
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    )
}

/// Create BlockAdded event for a block.
pub fn added_event(block: &BlockMessage) -> F1r3flyEvent {
    let (
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    ) = block_event(block);
    F1r3flyEvent::block_added(
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    )
}

/// Create BlockFinalised event for a block.
pub fn finalised_event(block: &BlockMessage) -> F1r3flyEvent {
    let (
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    ) = block_event(block);
    F1r3flyEvent::block_finalised(
        block_hash,
        block_number,
        timestamp,
        parent_hashes,
        justification_hashes,
        deploys,
        creator,
        seq_num,
    )
}

#[cfg(test)]
mod pending_deploy_tests {
    use super::pending_deploy_is_future_for_next_block;

    #[test]
    fn pending_deploy_equal_to_latest_block_is_not_future_for_next_block() {
        assert!(!pending_deploy_is_future_for_next_block(100, 100));
    }

    #[test]
    fn pending_deploy_above_latest_block_is_future_for_next_block() {
        assert!(pending_deploy_is_future_for_next_block(100, 101));
    }

    #[test]
    fn pending_deploy_below_latest_block_is_not_future_for_next_block() {
        assert!(!pending_deploy_is_future_for_next_block(100, 99));
    }
}
