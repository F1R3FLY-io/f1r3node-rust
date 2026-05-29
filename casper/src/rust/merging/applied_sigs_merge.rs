// applied_sigs merge + post-state aggregation. See
// workspace/projects/system-integration/notes/applied-sigs-design.md
// §3 (merge rule) + §4 (post-state aggregation).
//
// The applied_sigs map tracks user-deploy sigs ever applied in a
// state's ancestry (modulo merge rollback and lifespan expiry). It
// drives the simplified `repeat_deploy` lookup and the proposer-side
// deploy-selection filter — the structural once-per-deploy guarantee
// that retires the `resolve_at_parents` LFB-dependence class.
//
// Pure functions. No I/O, no logging beyond invariant-violation warns.

use std::collections::{HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

/// Compute the merged pre-state's `applied_sigs` from this merge's
/// parents, this merge's rejected-deploys set, and the merge engine's
/// kept_chain_sigs overlay (sigs from chains the merge kept that are
/// NOT in any individual parent's applied_sigs).
///
/// ```text
/// merged_pre.applied_sigs =
///     ∪ parents.applied_sigs
///     ∪ kept_chain_sigs           ← Phase 1 completion (bug-d fix)
///     − this_merge_rejected_deploys
///     − { sig : height < current_height − deploy_lifespan }
/// ```
///
/// **`kept_chain_sigs` overlay:** the merge engine's
/// `dag_merger::merge` returns `kept_chain_sigs` — sigs from chains
/// the merge kept that may not appear in any direct parent's
/// `applied_sigs`. Without this overlay, the proposer's Filter 2 (and
/// the validator's `repeat_deploy`) misses cases where a deploy was
/// applied in a chain that's in the merge scope but not a direct
/// parent. The block then includes that deploy in body, executes it
/// against a merge-result trie that already contains its prior
/// writes, and triggers MULTIDATUM / BUG FOUND (the bug-d empirical
/// signature). Verified against bonding attempt 9: 15 slip-through
/// blocks where `post_size < pre_size + body_deploys` correlate with
/// the 180 BUG FOUND events.
///
/// **Tie-break on parent disagreement:** when two parents carry the
/// same sig at different heights, the result takes `min(heights)`.
/// The earliest application is the canonical record (matches
/// lifespan-GC semantics — the sig is gone once its earliest
/// application is older than `deploy_lifespan`). Disagreement
/// indicates an upstream merge-invariant violation and is logged at
/// `warn` level with target `f1r3.trace.applied_sigs`.
///
/// **Determinism:** the result is a bit-identical function of
/// `(parents, this_merge_rejected_deploys, kept_chain_sigs,
/// current_height, deploy_lifespan)`. Two nodes with identical inputs
/// produce identical outputs regardless of `parents` iteration order.
pub fn merge_pre_state(
    parents: &[&HashMap<Bytes, i64>],
    this_merge_rejected_deploys: &HashSet<Bytes>,
    kept_chain_sigs: &HashMap<Bytes, i64>,
    current_height: i64,
    deploy_lifespan: i64,
) -> HashMap<Bytes, i64> {
    let mut merged: HashMap<Bytes, i64> = HashMap::new();

    // Union of parents with min-heights tie-break on disagreement.
    for parent in parents {
        for (sig, height) in parent.iter() {
            match merged.get_mut(sig) {
                Some(existing) if *existing != *height => {
                    let picked = (*existing).min(*height);
                    tracing::warn!(
                        target: "f1r3.trace.applied_sigs",
                        "[APPLIED-SIGS-PARENT-DISAGREEMENT] sig={} h1={} h2={} picked={}",
                        short_hex(sig),
                        existing,
                        height,
                        picked,
                    );
                    *existing = picked;
                }
                Some(_) => {} // matching height, no-op
                None => {
                    merged.insert(sig.clone(), *height);
                }
            }
        }
    }

    // Overlay kept_chain_sigs from the merge engine. Sigs from kept
    // chains that aren't in any parent's applied_sigs get added here
    // — without this, Phase 1's deploy filters miss kept-chain sigs
    // and let duplicates through (the bug-d / BUG FOUND mechanism).
    // On overlap with parents, take min(heights) — earlier
    // application is canonical for lifespan-GC purposes.
    for (sig, height) in kept_chain_sigs.iter() {
        match merged.get_mut(sig) {
            Some(existing) => {
                let picked = (*existing).min(*height);
                *existing = picked;
            }
            None => {
                merged.insert(sig.clone(), *height);
            }
        }
    }

    // Subtract this-merge's rejected_deploys (the §3 rollback step).
    for sig in this_merge_rejected_deploys {
        merged.remove(sig);
    }

    // Subtract lifespan-expired entries. A sig whose recorded height
    // is older than `current_height - deploy_lifespan` is GC'd; the
    // existing `Expired` finalization-state check (independent of
    // applied_sigs) prevents re-application of expired deploys, so
    // this purely bounds memory.
    let floor = (current_height - deploy_lifespan).max(0);
    merged.retain(|_sig, height| *height >= floor);

    merged
}

/// Aggregate a block's post-state applied_sigs from its merged
/// pre-state and the sigs in `body.deploys`.
///
/// ```text
/// post.applied_sigs = merged_pre.applied_sigs ∪ { sig: current_height | sig ∈ body.deploys }
/// ```
///
/// **Includes `is_failed=true` deploys** — identity is consumed by
/// execution regardless of success: pre-charge ran, user paid, sig is
/// terminal (`DeployFinalizationState::Failed`). Re-execution would
/// re-charge phlo for work the user already paid for. See
/// applied-sigs-design.md §11 #5.
///
/// Sigs already present in `merged_pre` are updated to `current_height`
/// — the most recent application is the canonical height. (In practice
/// `repeat_deploy` rejects blocks that re-include an applied sig, so a
/// sig appearing in both `merged_pre` and `body.deploys` is a malformed
/// block; this function does the conservative thing if asked anyway.)
pub fn aggregate_post_state(
    mut merged_pre: HashMap<Bytes, i64>,
    body_deploys_sigs: impl IntoIterator<Item = Bytes>,
    current_height: i64,
) -> HashMap<Bytes, i64> {
    for sig in body_deploys_sigs {
        merged_pre.insert(sig, current_height);
    }
    merged_pre
}

/// Reduce a parent set to its **maximal antichain** — drop any parent
/// that is an ancestor of another parent in the set (following ANY
/// parent edge, not just the main parent). The merge layer's fast
/// path does the analogous reduction for state (see
/// `interpreter_util.rs::compute_parents_post_state` line ~813,
/// using `with_ancestors_capped`).
///
/// Without this dedup, a parent set `[merge_block, block_a]` where
/// `merge_block.parents_hash_list = [block_a, block_b]` (block_a is
/// merge_block's SECONDARY parent, not main) causes the union of
/// parents' applied_sigs to wrongly re-introduce sigs that
/// `merge_block` already subtracted via its merge — because
/// `is_in_main_chain(block_a, merge_block)` returns false (block_a
/// not on the main-parent chain), so a main-chain-only dedup keeps
/// both. `recovery_cycle_spec` triggers this when the proposer picks
/// the validator's own latest plus a multi-parent merge from
/// another validator.
///
/// Uses `dag.ancestors` (BFS over all-parent edges) bounded by the
/// parent set itself (the only matches that matter are other parents).
/// Returns indices of parents to KEEP. Lookup failures keep the
/// parent — conservative.
pub fn effective_parent_indices(
    parents: &[BlockHash],
    dag: &KeyValueDagRepresentation,
) -> Vec<usize> {
    let n = parents.len();
    let mut keep = vec![true; n];
    let parent_set: HashSet<BlockHash> = parents.iter().cloned().collect();
    for j in 0..n {
        // Collect the set of OTHER parents that are ancestors of
        // parents[j] via any-parent BFS. Filter early to keep the walk
        // bounded — once we've found all other parents, we can stop
        // visiting deeper.
        let ancestors_of_j = match dag.ancestors(parents[j].clone(), |hash| parent_set.contains(hash)) {
            Ok(set) => set,
            Err(_) => continue,
        };
        for i in 0..n {
            if i == j || !keep[i] {
                continue;
            }
            if ancestors_of_j.contains(&parents[i]) {
                keep[i] = false;
            }
        }
    }
    (0..n).filter(|i| keep[*i]).collect()
}

fn short_hex(bytes: &Bytes) -> String {
    hex::encode(&bytes[..bytes.len().min(8)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn map(entries: &[(&str, i64)]) -> HashMap<Bytes, i64> {
        entries.iter().map(|(s, h)| (sig(s), *h)).collect()
    }

    #[test]
    fn empty_parents_yields_empty_map() {
        let result = merge_pre_state(&[], &HashSet::new(), &HashMap::new(), 10, 50);
        assert!(result.is_empty());
    }

    #[test]
    fn single_parent_passes_through() {
        let p = map(&[("a", 1), ("b", 2)]);
        let result = merge_pre_state(&[&p], &HashSet::new(), &HashMap::new(), 10, 50);
        assert_eq!(result, p);
    }

    #[test]
    fn two_parents_union() {
        let p1 = map(&[("a", 1)]);
        let p2 = map(&[("b", 2)]);
        let result = merge_pre_state(&[&p1, &p2], &HashSet::new(), &HashMap::new(), 10, 50);
        assert_eq!(result, map(&[("a", 1), ("b", 2)]));
    }

    #[test]
    fn parent_disagreement_takes_min_height() {
        let p1 = map(&[("a", 5)]);
        let p2 = map(&[("a", 3)]);
        let result = merge_pre_state(&[&p1, &p2], &HashSet::new(), &HashMap::new(), 10, 50);
        assert_eq!(*result.get(&sig("a")).unwrap(), 3);
    }

    #[test]
    fn parent_disagreement_min_is_order_invariant() {
        let p1 = map(&[("a", 5)]);
        let p2 = map(&[("a", 3)]);
        let r1 = merge_pre_state(&[&p1, &p2], &HashSet::new(), &HashMap::new(), 10, 50);
        let r2 = merge_pre_state(&[&p2, &p1], &HashSet::new(), &HashMap::new(), 10, 50);
        assert_eq!(r1, r2);
    }

    #[test]
    fn rejected_deploys_are_subtracted() {
        let p = map(&[("a", 1), ("b", 2), ("c", 3)]);
        let rejected: HashSet<Bytes> = [sig("b")].into_iter().collect();
        let result = merge_pre_state(&[&p], &rejected, &HashMap::new(), 10, 50);
        assert_eq!(result, map(&[("a", 1), ("c", 3)]));
    }

    #[test]
    fn kept_chain_sigs_overlay_adds_sigs_not_in_parents() {
        // The bug-d fix: the merge engine returns kept_chain_sigs for sigs
        // that are in kept chains but not in any direct parent's
        // applied_sigs. Without the overlay, Phase 1's filters miss these.
        let p1 = map(&[("a", 1)]);
        let p2 = map(&[("b", 2)]);
        let kept: HashMap<Bytes, i64> = map(&[("d", 5)]);
        let result = merge_pre_state(&[&p1, &p2], &HashSet::new(), &kept, 10, 50);
        assert_eq!(
            result,
            map(&[("a", 1), ("b", 2), ("d", 5)]),
            "sig d came from a kept chain (not in any parent) — overlay must \
             include it so Filter 2 and repeat_deploy catch re-inclusion."
        );
    }

    #[test]
    fn kept_chain_sigs_overlay_takes_min_height_on_overlap() {
        // If a sig is in both a parent and kept_chain_sigs, take the
        // earliest application — matches lifespan-GC semantics.
        let p = map(&[("a", 5)]);
        let kept: HashMap<Bytes, i64> = map(&[("a", 3)]);
        let result = merge_pre_state(&[&p], &HashSet::new(), &kept, 10, 50);
        assert_eq!(*result.get(&sig("a")).unwrap(), 3);
    }

    #[test]
    fn lifespan_expired_entries_are_dropped() {
        let p = map(&[("old", 1), ("new", 9)]);
        // current_height=10, lifespan=5 → floor=5 → "old" at height 1 expires.
        let result = merge_pre_state(&[&p], &HashSet::new(), &HashMap::new(), 10, 5);
        assert_eq!(result, map(&[("new", 9)]));
    }

    #[test]
    fn lifespan_floor_clamps_at_zero() {
        let p = map(&[("a", 0), ("b", 1)]);
        // current_height=3, lifespan=10 → floor would be -7 → clamped to 0.
        let result = merge_pre_state(&[&p], &HashSet::new(), &HashMap::new(), 3, 10);
        assert_eq!(result, p);
    }

    #[test]
    fn aggregate_post_state_adds_body_sigs() {
        let pre = map(&[("a", 1)]);
        let body_sigs = vec![sig("b"), sig("c")];
        let result = aggregate_post_state(pre, body_sigs, 5);
        assert_eq!(result, map(&[("a", 1), ("b", 5), ("c", 5)]));
    }

    #[test]
    fn aggregate_post_state_overwrites_existing_height() {
        let pre = map(&[("a", 1)]);
        let body_sigs = vec![sig("a")];
        // Conservatively updates to current_height — repeat_deploy should
        // have rejected this block, but if asked, take the latest.
        let result = aggregate_post_state(pre, body_sigs, 9);
        assert_eq!(*result.get(&sig("a")).unwrap(), 9);
    }

    #[test]
    fn recovery_scenario_end_to_end() {
        // Parent block M applied D (height 1). D was NOT in LFB (the
        // base) — this is the legitimate-recovery case where D's
        // application was never finalized.
        let m_post = map(&[("D", 1)]);
        let empty_base = HashMap::new();
        // Block M' is the merge that REJECTED D's branch.
        // merged_pre for M' = M.applied_sigs − {D} = {} (D not in base, subtract proceeds).
        let mprime_rejected: HashSet<Bytes> = [sig("D")].into_iter().collect();
        let mprime_pre = merge_pre_state(&[&m_post], &mprime_rejected, &empty_base, 2, 50);
        assert!(mprime_pre.is_empty(), "rejected D must be absent from merged_pre");
        // M' post-state with empty body (just the rejection).
        let mprime_post = aggregate_post_state(mprime_pre, std::iter::empty(), 2);
        assert!(mprime_post.is_empty());
        // Block N re-includes D (legitimate recovery).
        let n_pre = merge_pre_state(&[&mprime_post], &HashSet::new(), &empty_base, 3, 50);
        assert!(n_pre.is_empty(), "N's pre-state should not contain D — recovery is legal");
        // After N executes, D is re-applied at height 3.
        let n_post = aggregate_post_state(n_pre, vec![sig("D")], 3);
        assert_eq!(*n_post.get(&sig("D")).unwrap(), 3);
    }

    #[test]
    fn bonding_scenario_end_to_end() {
        // Parent block M applied D (height 1). D's branch SURVIVED a
        // subsequent merge — no rejection. Block N then tries to
        // re-include D (the bonding-bug case).
        let m_post = map(&[("D", 1)]);
        // Block N's merge has NO rejection of D (its parent chain kept D).
        let n_pre = merge_pre_state(&[&m_post], &HashSet::new(), &HashMap::new(), 2, 50);
        assert!(n_pre.contains_key(&sig("D")),
                "D still applied in N's merged pre-state — repeat_deploy MUST reject");
    }
}
