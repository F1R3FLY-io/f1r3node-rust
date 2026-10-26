//! DR-120: the claim checks of the v6 rule (C1, C2).

use std::collections::HashSet;

use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use shared::rust::hashable_set::HashableSet;

use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::system_deploy::is_system_deploy_id;

/// C2: a user deploy id that two chains of the raw chain list carry. Chain
/// equality reads only `deploys_with_cost` (deploy_chain_index.rs:163-171),
/// so a set of chains would collapse two equal-cost copies into one. Dev's
/// dedup reads the raw list too (dag_merger.rs:984-1150). System ids hold
/// their block hash (system_deploy.rs:12-21), so only user ids can repeat.
pub(crate) fn has_repeated_user_id(chains: &[DeployChainIndex]) -> bool {
    let capacity = chains
        .iter()
        .map(|chain| chain.deploys_with_cost.0.len())
        .sum();
    let mut seen: HashSet<&Bytes> = HashSet::with_capacity(capacity);
    chains
        .iter()
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .filter(|deploy| !is_system_deploy_id(&deploy.deploy_id))
        .any(|deploy| !seen.insert(&deploy.deploy_id))
}

/// The channels that `chain` folds: the keys of its mergeable map.
pub(crate) fn folded(chain: &DeployChainIndex) -> impl Iterator<Item = &Blake2b256Hash> {
    chain.event_log_index.number_channels_data.keys()
}

/// The channels that `chain` changes plainly: a datum change outside its own
/// mergeable map. This is dev's test at dag_merger.rs:451-461.
pub(crate) fn plain(chain: &DeployChainIndex) -> impl Iterator<Item = &Blake2b256Hash> {
    chain
        .state_changes
        .datums_changes
        .iter()
        .filter_map(move |entry| {
            let channel = entry.key();
            let change = entry.value();
            match chain
                .event_log_index
                .number_channels_data
                .contains_key(channel)
            {
                true => None,
                false if change.added.is_empty() && change.removed.is_empty() => None,
                false => Some(channel),
            }
        })
}

/// C1: the folded and plain channels of a kept set. A plain change on a
/// channel that the kept set folds loses its value, because the number
/// override writes the base plus the folded diff (rholang_merging_logic.rs:
/// 116-176). Dev rejects such a writer in its keep-one step
/// (dag_merger.rs:437-461). The availability walk already rejects mixing
/// inside one branch (dag_merger.rs:216-229), so these claims compare
/// branches.
#[derive(Default)]
pub(crate) struct FoldedClaims {
    folded: HashSet<Blake2b256Hash>,
    plain: HashSet<Blake2b256Hash>,
}

impl FoldedClaims {
    /// True when `candidate` changes plainly a channel that the kept set
    /// folds, or folds a channel that the kept set changes plainly.
    pub(crate) fn mixes(&self, candidate: &HashableSet<DeployChainIndex>) -> bool {
        candidate.0.iter().any(|chain| {
            plain(chain).any(|channel| self.folded.contains(channel))
                || folded(chain).any(|channel| self.plain.contains(channel))
        })
    }

    pub(crate) fn add(&mut self, candidate: &HashableSet<DeployChainIndex>) {
        for chain in candidate.0.iter() {
            self.folded.extend(folded(chain).cloned());
            self.plain.extend(plain(chain).cloned());
        }
    }
}
