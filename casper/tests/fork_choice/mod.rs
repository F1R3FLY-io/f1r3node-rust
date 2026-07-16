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
//   argmax (Rank.v `rank_head_is_argmax`/`rank_selects_heaviest`), and the ghost main
//   parent `tips[0]` is deterministic + heaviest (GuardBridge.v
//   `main_parent_first_deterministic`, snapshot.rs:128-142).

mod prop_bound;
mod prop_estimator_determinism;
mod prop_filter_deep_parents;
mod prop_ghost_argmax;
mod prop_lca;
