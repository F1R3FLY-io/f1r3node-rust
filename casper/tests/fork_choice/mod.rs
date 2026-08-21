// Fork-choice verification proptests (LOCAL-ONLY; verification, not consensus code).
// Wired into the `mod` integration-test binary so `cargo test -p casper` runs them,
// and picked up by the fork-choice gate (scripts/check-fork-choice-ALL.sh) via the
// `fork_choice::` filter.
//
//   prop_filter_deep_parents — C12: the concrete `Estimator::filter_deep_parents`
//   conforms to the abstract `within_depth`/`prop_filter` model of GuardBridge.v
//   (soundness + main-retention + completeness + exact-set capstone).
//
//   prop_estimator_determinism — DETERMINISM + SCORE-MONOID + FILTER(T-10): the real
//   `Estimator` on a fixed multi-validator DAG returns order-independent tips, and an
//   invalid latest message is excluded (MainTheorem.v/Score.v/slashing ForkChoice.v).
//
//   prop_lca — the real `DagOperations::lowest_universal_common_ancestor_many` on RANDOM
//   well-formed DAGs: converges, is a common ancestor of every input, and is the LOWEST
//   such (max block number), cross-checked against `is_dag_ancestor` (Lca.v).
//
//   prop_bound — the `Estimator::apply(max_parents, depth)` B2/B3/B4 seams: sentinel /
//   usize-safe caps, head-preserving truncation, score-overflow typed Err, empty-tips
//   typed Err (Bound.v).
//
//   prop_ghost_argmax — T-GHOST + T-MP: the real `Estimator` on RANDOM weighted fork
//   DAGs returns the heaviest-subtree ranking (score DESC, hash ASC) led by the GHOST
//   argmax (Rank.v `rank_head_is_argmax`/`rank_selects_heaviest`), and the GHOST head
//   `tips[0]` is deterministic + heaviest (GuardBridge.v
//   `ghost_sort_first_deterministic`, snapshot.rs:317-331).
//
//   NOTE: `tips[0]` is the GHOST head, NOT necessarily the block's MAIN PARENT —
//   snapshot.rs:332 then runs `prefer_deploy_support_main_parent` (:124-185), which
//   can PROMOTE a deploy-carrying branch over it (GuardBridge.v seam (3),
//   `pipeline_head_may_differ_from_ghost`). That second stage is a private fn, so its
//   proptests live in-module in snapshot.rs's `mod tests` (`deploy_support_*`), not
//   here. See docs/theory/fork-choice/fork-choice-verification.md §6.2.

//   merged_sibling_scores — the MULTI-PARENT case the proptests above never
//   build: every fixture in this directory is single-parent, and on a
//   single-parent DAG "score every DAG ancestor" and "score every main-parent
//   ancestor" coincide. This one separates them.
//
//   heaviest_subtree_descent — the DEPTH-2 case the proptests above never
//   build: at depth 1 a tip's own score IS its subtree weight, so ranking
//   tips by score and descending the heaviest subtree coincide. At depth 2
//   they separate: the head must come from the majority-weight BRANCH, not
//   from whichever tip hash-sorts first. Pinned to CI instance ucc-i6
//   (run 32404488936), where the hash-ordered head reverted a 200-of-300
//   finality certificate with zero equivocations.

mod heaviest_subtree_descent;
mod merged_sibling_scores;
mod prop_bound;
mod prop_estimator_determinism;
mod prop_filter_deep_parents;
mod prop_ghost_argmax;
mod prop_lca;
