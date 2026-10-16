use std::sync::Arc;

use super::*;

/// Changed by DR-131 S6: the index charges and the key limit are deleted
/// with host work. A present key at `insert_new` is a broken invariant of
/// the store (F-11), and the store keeps its entry and its key count.
#[test]
fn a_present_key_is_a_broken_invariant() {
    let index = DigestShards::<u64, u64>::new();
    let key = StoreKey([3; 32]);
    let mut shard = index.write(&key);
    index
        .insert_new(&mut shard, key, Arc::new(1), 10, false)
        .expect("an absent key");
    assert!(matches!(
        index.insert_new(&mut shard, key, Arc::new(1), 11, false),
        Err(RSpaceError::BugFoundError(_))
    ));
    assert_eq!(DigestShards::get(&shard, &key).map(|entry| entry.value), Some(10));
    assert_eq!(index.key_count(), 1);
}
