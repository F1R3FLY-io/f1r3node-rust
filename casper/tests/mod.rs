mod add_block;
mod api;
mod batch1;
mod batch2;
mod blocks;
mod engine;
mod genesis;
mod helper;
mod merging;
mod multi_node;
mod sync;
mod util;

pub fn init_logger() { shared::rust::tracing_init::init_for_tests(); }
