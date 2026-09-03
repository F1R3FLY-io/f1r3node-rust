use std::sync::Arc;

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::radix_tree::{Item, Node, RadixTreeImpl, empty_node};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_store::KeyValueStore;

#[test]
fn print_tree_handles_a_deep_history_chain_without_native_recursion() {
    std::thread::Builder::new()
        .name("radix-tree-print-depth".to_owned())
        .stack_size(256 * 1024)
        .spawn(|| {
            let store = Arc::new(InMemoryKeyValueStore::new());
            let tree = RadixTreeImpl::new(store.clone());

            let mut leaf = empty_node();
            leaf[0] = Item::Leaf {
                prefix: Vec::new(),
                value: vec![0; 32],
            };
            let leaf_bytes = bincode::serialize::<Node>(&leaf).expect("serialize leaf node");
            let mut child_key = Blake2b256Hash::new(&leaf_bytes).bytes();
            store
                .put_one(child_key.clone(), leaf_bytes)
                .expect("store leaf node");

            for _ in 0..4096 {
                let mut parent = empty_node();
                parent[0] = Item::NodePtr {
                    prefix: Vec::new(),
                    ptr: child_key,
                };
                let parent_bytes = bincode::serialize::<Node>(&parent).expect("serialize parent");
                child_key = Blake2b256Hash::new(&parent_bytes).bytes();
                store
                    .put_one(child_key.clone(), parent_bytes)
                    .expect("store parent node");
            }

            let mut root = empty_node();
            root[0] = Item::NodePtr {
                prefix: Vec::new(),
                ptr: child_key,
            };
            let rendered = tree.print_tree(&root, Vec::new()).expect("print deep tree");
            assert!(rendered.contains("Leaf at prefix:"));
            assert_eq!(rendered.matches("NodePtr at ").count(), 4097);
        })
        .expect("spawn fixed-stack radix printer probe")
        .join()
        .expect("fixed-stack radix printer probe panicked");
}
