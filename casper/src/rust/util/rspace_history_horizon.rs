//! Forward-horizon rspace history reachability calculation.
//!
//! Used by joiner-side LFS sync to determine which rspace post-state roots
//! must be in the joiner's local roots store before transitioning to Running.
//! The calculation is the inverse of `mergeable_channels_gc::is_safe_to_delete`:
//! whereas the GC computes "what's safe to forget," this computes "what's
//! still reachable as a parent of an upcoming proposal."
//!
//! A joiner that has just LFS-synced to LFB at height N needs rspace state
//! for every block that an honest proposer could legitimately reference as
//! a parent: the proposer-side `Estimator::filterDeepParents` filter permits
//! any block within `max_parent_depth + depth_buffer` from the highest tip.
//! For a joiner whose highest tip is the LFB (just after sync), that means
//! every block in the DAG with `height ≥ LFB.height − (max_parent_depth +
//! depth_buffer)` — main chain AND side branches.
//!
//! The validator-side parent-depth check in `validate::parents` rejects any
//! block whose parents fall outside this same horizon, so the set of
//! potentially-validatable blocks is exactly bounded by it. There is no
//! out-of-horizon case to handle: horizon-internal blocks have their roots
//! synced by `sync_forward_horizon`, and out-of-horizon blocks are rejected
//! on consensus rules before validation queries the rspace history.
//!
//! For each in-horizon block we emit BOTH `pre_state_hash` and
//! `post_state_hash`. Single-parent blocks have `pre_state_hash =
//! parent.post_state_hash`, so this is mostly a redundant restatement of
//! a hash that's already collected from the parent. For multi-parent
//! blocks, however, `pre_state_hash` is the merge result computed by the
//! proposer's `dag_merger::merge` via `apply_trie_actions_fn` →
//! `do_checkpoint`'s `store_root`. That merge intermediate is recorded in
//! the proposer's roots_store but is NOT any block's post-state, so a
//! joiner that only collects post-states is left without it. When the
//! joiner later attempts to replay a child of one of those merged blocks
//! (or processes a parallel sibling), the spawn/reset path validates the
//! merge-result root against the joiner's roots_store and fires
//! `RootRepositoryDivergence` if absent. Including pre-states closes
//! that gap — peers serve the radix history for these merge results
//! because their `do_checkpoint` already wrote the trie nodes plus the
//! root tag for them.

use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casper::CasperShardConf;

/// Compute the set of rspace roots a joiner needs in its local roots store
/// before transitioning to Running. Walks every block in the DAG with
/// `height ≥ min_block_number` and emits both its `pre_state_hash` and
/// `post_state_hash`, deduped, ordered by descending block_number (LFB-side
/// first — most likely to be referenced as a parent by an early incoming
/// block). Within a single block the post-state is emitted before the
/// pre-state so the joiner imports the "outer" state before its merge
/// intermediate.
///
/// `min_block_number` MUST be the same bound the caller gives the block
/// requester and the mergeable-channel replay — [`lfs_min_block_number`].
/// Deriving a narrower one here is what broke joiner sync: the replay resets
/// rspace history to the pre-state of every block from that bound upward, so
/// any block it reaches whose roots were not synced fails
/// `validate_and_set_current_root` and the node never reaches Running. The
/// parent-reachability window is only one of the two constraints in that
/// bound, and it is the narrower one whenever `deploy_lifespan` exceeds
/// `max_parent_depth + depth_buffer`.
///
/// Including pre-states is what fixes multi-parent merge intermediates:
/// these hashes only ever exist as the result of the proposer's
/// `dag_merger::merge` (recorded via `do_checkpoint`'s `store_root`), and
/// would otherwise never reach the joiner because they are not any block's
/// `post_state_hash`. See module-level docs for the full rationale.
///
/// Returns an empty vec if the LFB is at depth 0 (genesis) or if the
/// horizon would extend below genesis (clamped to height 0).
pub fn compute_forward_horizon_roots(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    lfb: &BlockMessage,
    casper_shard_conf: &CasperShardConf,
    min_block_number: i64,
) -> Result<Vec<Blake2b256Hash>, KvStoreError> {
    if casper_shard_conf.max_parent_depth == i32::MAX {
        // Depth check disabled — joiner can validate against any historical
        // block. Fall back to no horizon sync; caller can opt into full
        // replay (`disable-lfs = true`) instead.
        return Ok(Vec::new());
    }

    let lfb_height = lfb.body.state.block_number;
    let min_height = std::cmp::max(0, min_block_number);

    if min_height > lfb_height {
        return Ok(Vec::new());
    }

    // topo_sort returns Vec<Vec<BlockHash>> — one inner vec per height,
    // covering all blocks at that height (main chain + side branches).
    // Ordering: ascending by height. Reverse to get LFB-side first.
    let layers = dag.topo_sort(min_height, Some(lfb_height))?;

    let mut roots: Vec<Blake2b256Hash> = Vec::new();
    let mut seen: HashSet<Blake2b256Hash> = HashSet::new();
    let mut visited = 0usize;
    let mut absent = 0usize;
    for layer in layers.iter().rev() {
        for block_hash in layer {
            visited += 1;
            let block = match block_store.get(block_hash)? {
                Some(b) => b,
                None => {
                    absent += 1;
                    tracing::warn!(
                        "compute_forward_horizon_roots: block {} in DAG but missing from block_store",
                        models::rust::casper::pretty_printer::PrettyPrinter::build_string_bytes(
                            block_hash
                        )
                    );
                    continue;
                }
            };
            let post = Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash);
            if seen.insert(post.clone()) {
                roots.push(post);
            }
            let pre = Blake2b256Hash::from_bytes_prost(&block.body.state.pre_state_hash);
            if seen.insert(pre.clone()) {
                roots.push(pre);
            }
        }
    }

    // The walk's inputs and yield. A root the joiner later resets to but never
    // requested is invisible without this: `visited` says how much of the
    // window the DAG actually held, which a downloaded-block count cannot.
    tracing::debug!(
        target: "f1r3.trace.lfs",
        min_height,
        lfb_height,
        layers = layers.len(),
        blocks_visited = visited,
        blocks_absent_from_store = absent,
        roots = roots.len(),
        emitted = %roots
            .iter()
            .map(|r| hex::encode(&r.bytes()[..8]))
            .collect::<Vec<_>>()
            .join(","),
        "forward-horizon walk"
    );
    Ok(roots)
}

/// Compute the LFS lower-bound block number for joiner-side block download.
///
/// `lfs_block_requester::stream` clamps how far back from LFB it will accept
/// blocks during sync. The bound has two contributing constraints:
///
///   1. `deploy_lifespan` — blocks within this window of LFB carry deploys
///      that may still be reproposable (the original Scala-era reason).
///   2. `max_parent_depth + mergeable_channels_gc_depth_buffer` — the
///      forward-horizon: any block within this window of LFB may still be
///      legitimately referenced as a parent by an upcoming proposal, so the
///      joiner needs its DAG metadata even if it's older than
///      `deploy_lifespan` from LFB.
///
/// Take the lower of the two bounds (= older floor) so LFS coverage spans
/// both windows. The `i32::MAX` sentinel for `max_parent_depth` disables
/// the parent-depth check — in that mode the forward-horizon is unbounded
/// and `deploy_lifespan` alone determines the floor.
///
/// Negative results clamp to 0 (genesis) so the bound is always a valid
/// block number.
pub fn lfs_min_block_number(
    start_block_number: i64,
    deploy_lifespan: i64,
    max_parent_depth: i32,
    depth_buffer: i32,
) -> i64 {
    let lifespan_bound = std::cmp::max(0, start_block_number - deploy_lifespan);
    let horizon_bound = if max_parent_depth as i64 >= i32::MAX as i64 {
        // Sentinel: depth check disabled. Forward-horizon is unbounded,
        // so the lifespan bound alone determines the floor.
        lifespan_bound
    } else {
        std::cmp::max(
            0,
            start_block_number - (max_parent_depth as i64) - (depth_buffer as i64),
        )
    };
    std::cmp::min(lifespan_bound, horizon_bound)
}

/// Lower the LFS download floor so the responder's floor seed is usable.
///
/// The seed names two blocks by number precisely because the receiver has to
/// size its window before it holds anything to look up. Both must land in the
/// window, and so must the block below the lower of them: resolving the anchor's
/// frontier walks the band between anchor and frontier, and the clique oracle
/// takes each band block's corresponding weight map from that block's own MAIN
/// PARENT. A window stopping exactly at the frontier is one lookup short, and
/// that lookup is the difference between deriving forward and deferring forever.
///
/// Never narrows: `unseeded` also serves the deploy-lifespan and forward-horizon
/// windows, which the seed knows nothing about.
pub fn lfs_seeded_min_block_number(unseeded: i64, floor_number: i64, frontier_number: i64) -> i64 {
    let seed_reach = std::cmp::min(floor_number, frontier_number) - 1;
    std::cmp::max(0, std::cmp::min(unseeded, seed_reach))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seed only helps if the blocks it names are downloaded, and one block
    /// deeper than that: resolving the frontier reads each band block's own MAIN
    /// PARENT for its corresponding weight map, so a window that stops exactly
    /// at the frontier leaves the oracle one lookup short.
    #[test]
    fn a_seed_widens_the_window_to_one_below_the_blocks_it_names() {
        // Anchor 87 with the test-shard geometry: unseeded bound 37.
        assert_eq!(lfs_min_block_number(87, 50, 15, 10), 37);
        // A floor/frontier at 30 must pull the window down to 29, not 30.
        assert_eq!(lfs_seeded_min_block_number(37, 30, 33), 29);
        // The LOWER of the two names the bound.
        assert_eq!(lfs_seeded_min_block_number(37, 33, 30), 29);
    }

    /// A seed inside the window the node was going to download anyway must not
    /// shrink it. The unseeded bound serves the deploy-lifespan and
    /// forward-horizon windows, which the seed knows nothing about.
    #[test]
    fn a_seed_above_the_bound_never_narrows_the_window() {
        assert_eq!(lfs_seeded_min_block_number(37, 80, 82), 37);
        assert_eq!(lfs_seeded_min_block_number(0, 80, 82), 0);
    }

    /// Genesis is a valid floor, and block -1 is not a valid bound.
    #[test]
    fn a_seed_at_genesis_clamps_to_zero() {
        assert_eq!(lfs_seeded_min_block_number(37, 0, 0), 0);
    }

    #[test]
    fn min_block_number_takes_lifespan_when_horizon_wider() {
        // start=200, lifespan=50 -> lifespan_bound=150
        // max_parent_depth=10, buffer=0 -> horizon_bound=190
        // min(150, 190) = 150 (lifespan tighter)
        assert_eq!(lfs_min_block_number(200, 50, 10, 0), 150);
    }

    #[test]
    fn min_block_number_takes_horizon_when_lifespan_wider() {
        // start=200, lifespan=50 -> lifespan_bound=150
        // max_parent_depth=100, buffer=10 -> horizon_bound=90
        // min(150, 90) = 90 (horizon tighter — this is the §2.14 case:
        // forward-horizon goes deeper than deploy_lifespan)
        assert_eq!(lfs_min_block_number(200, 50, 100, 10), 90);
    }

    #[test]
    fn min_block_number_clamps_to_zero_at_genesis() {
        // start=10, lifespan=50 -> max(0, 10-50) = 0
        assert_eq!(lfs_min_block_number(10, 50, 100, 10), 0);
    }

    #[test]
    fn min_block_number_clamps_horizon_to_zero() {
        // start=10, lifespan=5 -> lifespan_bound=5
        // max_parent_depth=100, buffer=10 -> horizon raw=10-110=-100 -> clamped 0
        // min(5, 0) = 0
        assert_eq!(lfs_min_block_number(10, 5, 100, 10), 0);
    }

    #[test]
    fn min_block_number_disabled_depth_falls_back_to_lifespan() {
        // max_parent_depth=i32::MAX (sentinel) → horizon_bound=lifespan_bound
        // The function returns lifespan_bound directly (no horizon clamping).
        assert_eq!(lfs_min_block_number(200, 50, i32::MAX, 10), 150);
        assert_eq!(lfs_min_block_number(200, 50, i32::MAX, 0), 150);
    }

    #[test]
    fn min_block_number_zero_buffer() {
        // No buffer → pure max_parent_depth.
        // start=300, lifespan=50, max_parent_depth=200 -> horizon=100
        // min(250, 100) = 100
        assert_eq!(lfs_min_block_number(300, 50, 200, 0), 100);
    }
}

// Integration coverage for the full `compute_forward_horizon_roots`
// reachability calc lives in `casper/tests/util/rspace_history_horizon_test.rs`
// (alongside the rest of the casper-test mod tree) where the test fixtures
// `with_storage` and `create_chain` are available.
