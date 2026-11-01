pub mod bag;
pub(crate) mod guarded_pathmap;
pub mod candidate_order;
#[cfg(test)]
#[path = "../../tests/data_index/candidate_digest_override.rs"]
pub(crate) mod candidate_digest_test_hook;
pub mod data_index;
pub mod hashing;
pub mod pathmap_codec;
pub mod pathmap_mutation;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only unit tests exercise this substrate until task 7931 wires it into RSpace"
    )
)]
pub(crate) mod persistence;
pub mod history;
pub mod hot_store;
pub mod internal;
pub mod logging;
pub mod rho_path;
pub mod ordered_receive;
pub mod rspace;
mod space_matcher;
pub mod checkpoint;
pub mod hot_store_action;
pub mod shared;
pub mod hot_store_trie_action;
pub mod state;
pub mod serializers;
pub mod trace;
pub mod r#match;
pub mod util;
pub mod rspace_interface;
pub mod tuplespace_interface;
pub mod replay_rspace_interface;
pub mod errors;
pub mod replay_rspace;
pub(crate) mod record_index;
pub mod receive_accelerator;
#[cfg(feature = "record-order-index-experiment")]
#[doc(hidden)]
pub use record_index::order_index_experiment;
pub mod retained_set_automaton;
pub mod merger;
pub mod reporting_rspace;
pub mod reporting_transformer;
pub mod metrics_constants;
