mod add_block;
mod api;
mod batch1;
mod batch2;
mod blocks;
mod compute_parents_post_state_regression_spec;
mod differential;
mod engine;
mod finalized_floor;
mod fork_choice;
mod genesis;
mod helper;
mod legacy_corpus;
mod merging;
mod multi_node;
mod repeat_deploy;
mod slashing;
mod sync;
mod util;
mod v6;

pub fn init_logger() { shared::rust::tracing_init::init_for_tests(); }
