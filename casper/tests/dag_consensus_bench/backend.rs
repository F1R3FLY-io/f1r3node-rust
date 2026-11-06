use crate::helper::test_node::{ShardOverrides, TestNode};
use crate::util::genesis_builder::GenesisContext;

pub async fn network(genesis: GenesisContext, window: i64, depth: i32) -> Vec<TestNode> {
    TestNode::create_network_with_overrides(genesis, 4, ShardOverrides {
        max_parent_depth: Some(depth),
        dag_hot_window_depth: Some(window),
        finalization_rate: Some(0),
        ..Default::default()
    })
    .await
    .expect("benchmark network")
}

pub fn resident_blocks(node: &TestNode) -> usize {
    node.block_dag_storage
        .get_representation()
        .unwrap()
        .resident_counts()
        .blocks
}

pub fn bounded_backend() -> bool { true }
