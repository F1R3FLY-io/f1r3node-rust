// Fork-choice verification proptests (LOCAL-ONLY; verification, not consensus code).
// Wired into the `mod` integration-test binary so `cargo test -p casper` runs them,
// and picked up by the fork-choice gate (scripts/check-fork-choice-ALL.sh) via the
// `fork_choice::` filter.
//
//   prop_filter_deep_parents — C12: the concrete `Estimator::filter_deep_parents`
//   conforms to the abstract `within_depth`/`prop_filter` model of GuardBridge.v
//   (soundness + main-retention + completeness + exact-set capstone).

mod prop_filter_deep_parents;
