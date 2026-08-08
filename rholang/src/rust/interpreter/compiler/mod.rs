pub mod bound_context;
pub mod bound_map;
pub mod bound_map_chain;
pub mod compiler;
pub mod exports;
pub mod free_context;
pub mod free_map;
pub mod id_context;
pub mod normalize;
/// The normalizer's explicit pushdown machine: value/work/continuation
/// alphabet, the single drive loop, and its invariants.
pub mod normalize_drive;
pub mod normalizer;
/// The **recursive oracle twin** of the normalizer SCC, verbatim as of
/// `6ccf71f2`. Test-only: it is the Θ(depth) implementation the converted
/// machine is differentiated against, and it must never be reachable from
/// production. See `docs/design/audits/theta-depth-traversals-2026-07-26.md` §8.
#[cfg(test)]
#[rustfmt::skip]
#[path = "../../../../tests/support/normalize_recursive.rs"]
pub mod normalize_recursive;
/// The differential that discharges the conversion's neutrality obligation:
/// the machine against the recursive oracle, on encoded term bytes plus the
/// final free map and binding chain.
#[cfg(test)]
#[path = "../../../../tests/support/normalize_differential.rs"]
pub mod normalize_differential;
pub mod receive_binds_sort_matcher;
pub mod span_utils;
pub mod utils;
