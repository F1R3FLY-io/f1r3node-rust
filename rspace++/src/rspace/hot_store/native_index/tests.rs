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

#[test]
fn restore_rewinds_the_entry_and_the_key_count() {
    let index = DigestShards::<u64, u64>::new();
    let key_a = StoreKey([0x11; 32]);
    let key_b = StoreKey([0x22; 32]);

    {
        let mut shard = index.write(&key_a);
        index
            .insert_new(&mut shard, key_a, Arc::new(1), 100, false)
            .expect("insert at the snapshot point");
    }
    let snapshot = index.snapshot();
    assert_eq!(index.key_count(), 1);

    // Mutate past the snapshot: add key_b and rewrite key_a. restore() must
    // undo both.
    {
        let mut shard = index.write(&key_b);
        index
            .insert_new(&mut shard, key_b, Arc::new(2), 200, false)
            .expect("insert past the snapshot");
    }
    {
        let mut shard = index.write(&key_a);
        DigestShards::replace(&mut shard, &key_a, 999);
    }
    assert_eq!(index.key_count(), 2);
    assert_eq!(DigestShards::get(&index.read(&key_a), &key_a).map(|e| e.value), Some(999));

    index.restore(snapshot);
    assert_eq!(index.key_count(), 1, "the restored key count is the snapshot's");
    assert_eq!(
        DigestShards::get(&index.read(&key_a), &key_a).map(|e| e.value),
        Some(100),
        "the restored value is the snapshot's",
    );
    assert!(
        DigestShards::get(&index.read(&key_b), &key_b).is_none(),
        "a post-snapshot key is absent after restore",
    );
}

#[test]
fn snapshot_is_independent_of_later_writes() {
    let index = DigestShards::<u64, u64>::new();
    let key = StoreKey([0x33; 32]);
    {
        let mut shard = index.write(&key);
        index
            .insert_new(&mut shard, key, Arc::new(1), 7, false)
            .expect("insert");
    }
    let snapshot = index.snapshot();

    // Mutate the live index; the snapshot's shards are owned clones and must
    // keep the original value.
    {
        let mut shard = index.write(&key);
        DigestShards::replace(&mut shard, &key, 99);
    }
    let snap_shard = &snapshot.shards[key.shard()];
    assert_eq!(
        snap_shard.get(&key).map(|e| e.value),
        Some(7),
        "the snapshot's shard must not observe the later replace",
    );
}

#[test]
fn read_shard_and_write_shard_reach_the_same_entry_as_the_key_path() {
    let index = DigestShards::<u64, u64>::new();
    let key = StoreKey([0x44; 32]);
    let shard_index = key.shard();
    {
        let mut shard = index.write_shard(shard_index);
        index
            .insert_new(&mut shard, key, Arc::new(5), 42, true)
            .expect("insert via write_shard");
    }
    let via_shard_read = {
        let read = index.read_shard(shard_index);
        DigestShards::get(&read, &key).map(|e| e.value)
    };
    let via_key_read = {
        let read = index.read(&key);
        DigestShards::get(&read, &key).map(|e| e.value)
    };
    assert_eq!(via_shard_read, Some(42));
    assert_eq!(
        via_shard_read, via_key_read,
        "read_shard(shard_index) and read(key) must observe the same entry",
    );
}
