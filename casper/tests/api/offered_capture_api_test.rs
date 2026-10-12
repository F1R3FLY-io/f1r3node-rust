//! H2 (bug 11419, DR-127): the capture of measured purse stacks keys its heads
//! by channel. One lane can appear in two authority forms: `foo` written as a
//! signature is a ground atom, and a name bound to `@"foo"` is a quoted
//! process. Both forms name one channel and one lane. The capture keeps the
//! first head of each channel and accepts a later head of the other form.
//!
//! The module is a child of `offered_funded_api_test`, so it shares that
//! module's fixtures.

use rholang::rust::interpreter::compiler::compiler::Compiler;

use crate::helper::offered_deploy::{
    owner_offer_with_lanes, selected_schedule, PermittedLane, OFFER_PHLO_LIMIT, TEST_SHARD,
};
use crate::helper::test_node::TestNode;

/// Added by H2 (bug 11419, DR-127): the client permits lane foo. Its term
/// signs one layer with `foo` and another with a name bound to `@"foo"`, and
/// the offer lists no slot. Before H2, the capture rejected the second form
/// after execution with "one measured purse channel has conflicting heads",
/// and the producer quarantined the offer with no charge. Now the run
/// settles, and every validator holds the same receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lane_in_two_authority_forms_settles() {
    const TERM: &str = "{% @\"foo-ground\"!(true) %}[foo] | \
                        new x in { x!(\"foo\") | for (n <- x) { {% @\"foo-name\"!(true) %}[n] } }";
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("a three-node network starts");
    let owner = genesis.genesis_vaults[0].clone();
    let lane = Compiler::source_to_adt("foo :: ()")
        .expect("the stack literal compiles")
        .cost_stacks[0]
        .cells[0]
        .clone();
    let every_class = (0..selected_schedule().classes.len()).collect::<Vec<_>>();
    let offer = owner_offer_with_lanes(
        &owner,
        1,
        TERM.to_string(),
        0,
        TEST_SHARD.to_string(),
        OFFER_PHLO_LIMIT,
        &[PermittedLane {
            signature: &lane,
            classes: &every_class,
        }],
    );
    let (_, receipt) = super::propose_offer(&mut nodes, offer)
        .await
        .unwrap_or_else(|reason| panic!("the offer settles: {reason}"));
    assert!(receipt.phlo_used > 0, "the run pays for its work");
    assert!(receipt.phlo_used + receipt.retained_phlo <= receipt.phlo_limit);
}
