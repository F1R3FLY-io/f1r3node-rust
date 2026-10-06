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
mod merging;
mod multi_node;
// ⚠ A DIRECTORY, like every other entry in this list. The three multi-sig
// specs used to be declared here as bare `tests/*.rs` files, which cargo ALSO
// auto-discovers as standalone integration targets — so each was compiled
// twice and every test in them ran twice. See `multi_sig/mod.rs`.
mod multi_sig;
mod repeat_deploy;
mod slashing;
mod sync;
mod util;

pub fn init_logger() { shared::rust::tracing_init::init_for_tests(); }
