mod add_block;
mod api;
mod batch1;
mod batch2;
mod blocks;
mod compute_parents_post_state_regression_spec;
mod engine;
mod finalized_floor;
mod fork_choice;
mod genesis;
mod helper;
mod merging;
mod multi_node;
mod repeat_deploy;
mod slashing;
mod sync;
mod test_node_fixture;
mod util;

pub fn init_logger() { shared::rust::tracing_init::init_for_tests(); }
