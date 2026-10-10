//! DR-120 (gap G9): the interim merge rule of an offered-funded protocol-6 shard.
//!
//! P1 (cost-accounted-rho.tex:344-366) composes the results of different
//! signers "without merge analysis", and only shared data channels take "a
//! natural serialization order". A v6 merge therefore has two paths beneath
//! dev's dispatcher:
//!
//! - `fast::try_compose` composes a scope whose chains provably commute. It
//!   runs no rejection logic, because no chain can lose.
//! - `ordered::ordered_pass` replaces dev's rejection-option search. It walks
//!   the candidate branches once, in the strict order K of `order.rs`, and
//!   then repairs the checks that are not monotone.
//!
//! A legacy shard keeps dev's merger byte for byte (`MergeRule::Dev`).
#![allow(clippy::mutable_key_type)]

pub(crate) mod claims;
pub(crate) mod compose;
pub(crate) mod evidence;
pub(crate) mod fast;
pub(crate) mod ledger;
pub(crate) mod order;
pub(crate) mod ordered;
#[cfg(test)]
mod tests;

use std::collections::HashSet;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::merger::merging_logic;
use shared::rust::hashable_set::HashableSet;

use crate::rust::errors::CasperError;
use crate::rust::merging::dag_merger;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;

/// The reader of a committed state, as `dag_merger::merge` builds it
/// (dag_merger.rs:1447-1451).
pub(crate) type RhoHistoryReader =
    Box<dyn HistoryReader<Blake2b256Hash, Par, BindPattern, ListParWithRandom, TaggedContinuation>>;

/// The merge rule of a shard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeRule {
    /// Dev's merger: the rejection-option search with the unavailable retry.
    Dev,
    /// DR-120: the fast path, or the ordered pass in the order K.
    OfferedV6,
}

/// True when `block` is not the genesis block and holds an offered user deploy.
/// The genesis block has no parents, and it stores its deploys in the legacy
/// format on every shard (genesis.rs:280-284).
fn holds_offered_deploy(block: &BlockMessage) -> bool {
    !block.header.parents_hash_list.is_empty()
        && block
            .body
            .deploys
            .iter()
            .any(|deploy| matches!(deploy, ProcessedUserDeploy::Offered(_)))
}

/// The rule for a merge: OfferedV6 when a parent or a scope block that is not
/// the genesis block holds an offered user deploy, and Dev otherwise.
///
/// The deploy formats decide the rule, so it needs no shard flag (user
/// decision of 2026-10-10). Formats do not mix on a valid chain: v6
/// validation rejects a block with a legacy user deploy
/// (validation_dispatcher.rs:76-82,124-130), and legacy replay refuses an
/// offered term (runtime_manager.rs:1732-1736). The parents are read first,
/// from memory. The scope blocks are read only when no parent decides, in
/// sorted order, so a block that this node does not hold fails identically on
/// every node.
pub(crate) fn rule_for(
    parents: &[BlockMessage],
    scope: &HashSet<BlockHash>,
    block_store: &KeyValueBlockStore,
) -> Result<MergeRule, CasperError> {
    if parents.iter().any(holds_offered_deploy) {
        return Ok(MergeRule::OfferedV6);
    }
    let mut scope_blocks: Vec<&BlockHash> = scope.iter().collect();
    scope_blocks.sort();
    for block_hash in scope_blocks {
        let block = block_store.get(block_hash)?.ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "merge scope block {} not in block store (DAG/store desync)",
                hex::encode(&block_hash[..8.min(block_hash.len())])
            ))
        })?;
        if holds_offered_deploy(&block) {
            return Ok(MergeRule::OfferedV6);
        }
    }
    Ok(MergeRule::Dev)
}

/// The dependency relation of the merge. `merging_logic::depends` evaluates
/// the same expressions as dev's closure at dag_merger.rs:1456-1502.
pub(crate) fn chain_depends(target: &DeployChainIndex, source: &DeployChainIndex) -> bool {
    merging_logic::depends(&target.event_log_index, &source.event_log_index)
}

/// Dev's availability walk over one branch (dag_merger.rs:165-350), with the
/// state-change and mergeable-channel readers of dag_merger.rs:1504-1507 and
/// the base readers of dag_merger.rs:1793-1800.
pub(crate) fn walk_branch(
    branch: HashableSet<DeployChainIndex>,
    depends: &dyn Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    history_reader: &RhoHistoryReader,
) -> Result<
    (
        Option<HashableSet<DeployChainIndex>>,
        HashableSet<DeployChainIndex>,
    ),
    HistoryError,
> {
    dag_merger::split_unavailable_branch_consumes(
        branch,
        &depends,
        &|chain: &DeployChainIndex| Ok(chain.state_changes.clone()),
        &|chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone(),
        &|channel: &Blake2b256Hash| history_reader.get_data_proj_binary(channel),
        &|consume_channels: &Vec<Blake2b256Hash>| {
            let history_pointer = stable_hash_provider::hash_from_hashes(consume_channels);
            history_reader.get_continuations_proj_binary(&history_pointer)
        },
    )
}
