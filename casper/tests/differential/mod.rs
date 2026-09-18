// Full-vs-restored differential tests: the same consensus-visible decision
// run against a node holding complete history and against one restored at an
// anchor, asserting the two agree or that the restored node DEFERS.
//
// A restored node may answer "I cannot decide" — `Undecidable`, `BlockNotHeld`,
// `AwaitingState`, `AbsenceHold` — because catch-up resolves it. What it may
// never do is return a DIFFERENT verdict from the full node: that is a
// consensus fork that no retry repairs.
//
// Views are built by listing what a node HOLDS, so the held set of each side
// is explicit at the call site and the two differ only by what was listed.

use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::CasperSnapshot;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;

mod ancestor_walk_across_a_restore_gap;
mod repeat_deploy_across_a_restore_gap;
mod snapshot_parent_candidates;

/// A node's view of the DAG: exactly the blocks passed in are held, indexed as
/// they would be by `insert`. Anything else a walk reaches is absent, which is
/// what a restore horizon looks like from inside a decision.
pub fn view_holding(blocks: &[&BlockMessage]) -> CasperSnapshot {
    let mut snapshot = TestCasperWithSnapshot::create_empty_snapshot();
    for block in blocks {
        let number = block.body.state.block_number;
        snapshot.dag.dag_set.insert(block.block_hash.clone());
        snapshot
            .dag
            .block_number_map
            .insert(block.block_hash.clone(), number);
        if let Some(main_parent) = block.header.parents_hash_list.first() {
            snapshot
                .dag
                .main_parent_map
                .insert(block.block_hash.clone(), main_parent.clone());
        }
        snapshot
            .dag
            .block_metadata_index
            .write()
            .add(BlockMetadata::from_block(block, false, None, None))
            .expect("seed block metadata");
    }
    snapshot
}
