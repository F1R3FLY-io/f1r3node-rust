pub mod bundle_sort_matcher;
pub mod connective_sort_matcher;
pub mod expr_sort_matcher;
pub mod if_sort_matcher;
pub mod match_sort_matcher;
pub mod new_sort_matcher;
pub mod ordering;
pub mod par_sort_matcher;
pub mod receive_sort_matcher;
pub mod score_tree;
pub mod send_sort_matcher;
/// Leg-2 Stage C-2: the sorter's per-arm table, shared by the production
/// driver and its recursive oracle so that the two cannot drift.
pub mod sort_combine;
/// Leg-2 Stage C-2: the explicit-worklist driver — the production traversal.
pub mod sort_drive;
/// Leg-2 Stage C-2: the recursive oracle twin and the differential that
/// compares it against the driver. `#[cfg(test)]`; see its module docs.
#[cfg(test)]
#[path = "../../../../tests/support/sort_recursive.rs"]
pub mod sort_recursive;
pub mod sortable;
pub mod unforgeable_sort_matcher;
pub mod var_sort_matcher;
