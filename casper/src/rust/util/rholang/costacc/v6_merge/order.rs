//! DR-120: the order K of the ordered pass.
//!
//! K(b) = (not pinned, -loss.max, -loss.sum, lowest source height,
//! `compare_branches`). Settled content comes first, because a merge must
//! never drop it (#341). Prior losses come next, so a chain that keeps losing
//! gains priority (#294). The lowest source height and dev's branch order
//! break the remaining ties.
//!
//! K is a strict total order on disjoint non-empty candidates:
//! `compare_branches` is `Equal` only for two branches with the same sorted
//! chains, and chain equality is the equality of the deploy sets
//! (deploy_chain_index.rs:163-171).

use std::cmp::{Ordering, Reverse};
use std::collections::HashSet;

use crate::rust::merging::conflict_set_merger::{branch_losses, compare_branches, Branch};
use crate::rust::merging::deploy_chain_index::DeployChainIndex;

/// A candidate is pinned when it holds a settled chain.
pub(crate) fn is_pinned(
    candidate: &Branch<DeployChainIndex>,
    pinned: &HashSet<DeployChainIndex>,
) -> bool {
    candidate.0.iter().any(|chain| pinned.contains(chain))
}

/// The numeric part of K: (not pinned, -loss.max, -loss.sum, lowest height).
fn numeric_key(
    candidate: &Branch<DeployChainIndex>,
    pinned: &HashSet<DeployChainIndex>,
) -> (bool, Reverse<u64>, Reverse<u64>, i64) {
    let losses = branch_losses(candidate, &|chain: &DeployChainIndex| {
        chain.prior_rejections
    });
    let lowest_height = candidate
        .0
        .iter()
        .map(|chain| chain.source_block_number)
        .min()
        .unwrap_or(i64::MAX);
    (
        !is_pinned(candidate, pinned),
        Reverse(losses.max),
        Reverse(losses.sum),
        lowest_height,
    )
}

/// K: `Less` means that `a` comes first.
pub(crate) fn k_cmp(
    a: &Branch<DeployChainIndex>,
    b: &Branch<DeployChainIndex>,
    pinned: &HashSet<DeployChainIndex>,
) -> Ordering {
    numeric_key(a, pinned)
        .cmp(&numeric_key(b, pinned))
        .then_with(|| compare_branches(a, b))
}

/// Sorts candidates by K. The result depends only on the set of candidates.
pub(crate) fn sort_by_k(
    candidates: &mut [Branch<DeployChainIndex>],
    pinned: &HashSet<DeployChainIndex>,
) {
    candidates.sort_by(|a, b| k_cmp(a, b, pinned));
}

/// The index of the last candidate by K whose chains satisfy `holds`,
/// preferring unpinned candidates. `candidates` must be sorted by K.
pub(crate) fn last_by_k(
    candidates: &[Branch<DeployChainIndex>],
    pinned: &HashSet<DeployChainIndex>,
    holds: impl Fn(&DeployChainIndex) -> bool,
) -> Option<usize> {
    let matching = |want_pinned: bool| {
        candidates.iter().rposition(|candidate| {
            is_pinned(candidate, pinned) == want_pinned && candidate.0.iter().any(&holds)
        })
    };
    matching(false).or_else(|| matching(true))
}
