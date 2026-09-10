// See casper/src/main/scala/coop/rchain/casper/util/rholang/ReplayCache.scala

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{Event, Peek, ProduceEvent};
use models::rust::validator::Validator;
use rholang::rust::interpreter::system_processes::BlockData;

use super::replay_cache_state::ReplayCacheState;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplayCacheContext {
    sender_pk: Vec<u8>,
    seq_num: i32,
    timestamp: i64,
    height: i64,
    invalid_blocks_hash: [u8; 32],
}

impl ReplayCacheContext {
    pub fn new(block_data: &BlockData, invalid_blocks: &HashMap<BlockHash, Validator>) -> Self {
        let mut entries: Vec<_> = invalid_blocks.iter().collect();
        entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let invalid_blocks_hash = Blake2b256::hash_stream(|update| {
            update(b"f1r3node:replay-invalid-blocks:v1");
            update(&(entries.len() as u64).to_le_bytes());
            for (block, validator) in entries {
                update(&(block.len() as u64).to_le_bytes());
                update(block.as_ref());
                update(&(validator.len() as u64).to_le_bytes());
                update(validator.as_ref());
            }
        });
        Self {
            sender_pk: block_data.sender.bytes.to_vec(),
            seq_num: block_data.seq_num,
            timestamp: block_data.time_stamp,
            height: block_data.block_number,
            invalid_blocks_hash: invalid_blocks_hash
                .try_into()
                .expect("Blake2b256 produces 32 bytes"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplayCacheKey {
    pub parent_state: StateHash,
    pub context: ReplayCacheContext,
    pub payload_hash: Vec<u8>,
}

impl ReplayCacheKey {
    pub fn new(
        parent_state: StateHash,
        context: ReplayCacheContext,
        payload_hash: Vec<u8>,
    ) -> Self {
        Self {
            parent_state,
            context,
            payload_hash,
        }
    }

    fn heap_bytes(&self) -> usize {
        self.parent_state
            .len()
            .saturating_add(self.context.sender_pk.len())
            .saturating_add(self.payload_hash.len())
    }

    fn into_owned(mut self) -> Self {
        self.parent_state = owned_bytes(&self.parent_state);
        self
    }
}

/// `prost::bytes::Bytes` is a refcounted slice: a short slice keeps its whole
/// backing allocation (typically a decoded block's buffer) alive, so
/// length-based accounting under-counts what a cached value really retains.
/// Copying at cache boundary makes the private buffer exactly `len` bytes —
/// the accounting becomes exact and the decode buffer is released.
fn owned_bytes(bytes: &prost::bytes::Bytes) -> prost::bytes::Bytes {
    prost::bytes::Bytes::copy_from_slice(bytes)
}

/// Cached replay result containing event log and post-state hash.
#[derive(Clone, Debug)]
pub struct ReplayCacheEntry {
    pub event_log: Arc<Vec<Event>>,
    pub post_state: StateHash,
    retained_bytes: usize,
}

impl ReplayCacheEntry {
    pub fn new(mut event_log: Vec<Event>, post_state: StateHash) -> Self {
        for event in &mut event_log {
            Self::normalize_event(event);
        }
        let post_state = owned_bytes(&post_state);
        let retained_bytes = event_log
            .capacity()
            .saturating_mul(std::mem::size_of::<Event>())
            .saturating_add(event_log.iter().map(Self::event_heap_bytes).sum::<usize>())
            .saturating_add(post_state.len());
        Self {
            event_log: Arc::new(event_log),
            post_state,
            retained_bytes,
        }
    }

    fn normalize_event(event: &mut Event) {
        match event {
            Event::Produce(produce) => Self::normalize_produce(produce),
            Event::Consume(consume) => {
                for channels_hash in &mut consume.channels_hashes {
                    *channels_hash = owned_bytes(channels_hash);
                }
                consume.hash = owned_bytes(&consume.hash);
            }
            Event::Comm(comm) => {
                for channels_hash in &mut comm.consume.channels_hashes {
                    *channels_hash = owned_bytes(channels_hash);
                }
                comm.consume.hash = owned_bytes(&comm.consume.hash);
                for produce in &mut comm.produces {
                    Self::normalize_produce(produce);
                }
            }
        }
    }

    fn normalize_produce(produce: &mut ProduceEvent) {
        produce.channels_hash = owned_bytes(&produce.channels_hash);
        produce.hash = owned_bytes(&produce.hash);
        for value in &mut produce.output_value {
            *value = owned_bytes(value);
        }
    }

    fn event_heap_bytes(event: &Event) -> usize {
        match event {
            Event::Produce(produce) => Self::produce_heap_bytes(produce),
            Event::Consume(consume) => consume
                .channels_hashes
                .capacity()
                .saturating_mul(std::mem::size_of::<prost::bytes::Bytes>())
                .saturating_add(
                    consume
                        .channels_hashes
                        .iter()
                        .map(prost::bytes::Bytes::len)
                        .sum::<usize>(),
                )
                .saturating_add(consume.hash.len()),
            Event::Comm(comm) => comm
                .consume
                .channels_hashes
                .capacity()
                .saturating_mul(std::mem::size_of::<prost::bytes::Bytes>())
                .saturating_add(
                    comm.consume
                        .channels_hashes
                        .iter()
                        .map(prost::bytes::Bytes::len)
                        .sum::<usize>(),
                )
                .saturating_add(comm.consume.hash.len())
                .saturating_add(
                    comm.produces
                        .capacity()
                        .saturating_mul(std::mem::size_of::<ProduceEvent>()),
                )
                .saturating_add(
                    comm.produces
                        .iter()
                        .map(Self::produce_heap_bytes)
                        .sum::<usize>(),
                )
                .saturating_add(
                    comm.peeks
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Peek>()),
                ),
        }
    }

    fn produce_heap_bytes(produce: &ProduceEvent) -> usize {
        produce
            .channels_hash
            .len()
            .saturating_add(produce.hash.len())
            .saturating_add(
                produce
                    .output_value
                    .capacity()
                    .saturating_mul(std::mem::size_of::<prost::bytes::Bytes>()),
            )
            .saturating_add(
                produce
                    .output_value
                    .iter()
                    .map(prost::bytes::Bytes::len)
                    .sum::<usize>(),
            )
    }

    fn retained_bytes(&self) -> usize { self.retained_bytes }
}

/// Trait for replay caching operations.
pub trait ReplayCache: Send + Sync {
    fn get(&self, key: &ReplayCacheKey) -> Option<ReplayCacheEntry>;
    fn put(&self, key: ReplayCacheKey, entry: ReplayCacheEntry) -> bool;
    fn clear(&self);
}

/// Uses lengths rather than capacities so two equal keys always charge the
/// same amount — the byte accounting must be reproducible from the stored
/// (key, entry) pair alone when an eviction credits it back.
fn charged_bytes(key: &ReplayCacheKey, entry: &ReplayCacheEntry) -> usize {
    key.heap_bytes().saturating_add(entry.retained_bytes())
}

/// Simple in-memory LRU replay cache (thread-safe).
pub struct InMemoryReplayCache {
    state: Mutex<ReplayCacheState<ReplayCacheKey, ReplayCacheEntry>>,
    max_entries: usize,
    max_bytes: usize,
}

impl InMemoryReplayCache {
    pub fn new(max_entries: usize) -> Self { Self::with_limits(max_entries, usize::MAX) }

    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            state: Mutex::new(ReplayCacheState::new(max_entries)),
            max_entries,
            max_bytes,
        }
    }

    /// Create with default capacity (1024 entries).
    pub fn default_capacity() -> Self { Self::new(1024) }

    pub fn stats(&self) -> (usize, usize) {
        let state = self.state.lock().expect("ReplayCache lock poisoned");
        state.stats()
    }

    pub fn len(&self) -> usize { self.stats().0 }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn retained_bytes(&self) -> usize { self.stats().1 }
}

impl ReplayCache for InMemoryReplayCache {
    fn get(&self, key: &ReplayCacheKey) -> Option<ReplayCacheEntry> {
        let mut state = self.state.lock().expect("ReplayCache lock poisoned");
        state.get(key)
    }

    fn put(&self, key: ReplayCacheKey, entry: ReplayCacheEntry) -> bool {
        let key = key.into_owned();
        let mut state = self.state.lock().expect("ReplayCache lock poisoned");
        state.put(key, entry, self.max_entries, self.max_bytes, charged_bytes)
    }

    fn clear(&self) {
        let mut state = self.state.lock().expect("ReplayCache lock poisoned");
        state.clear();
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn replay_cache_context_binds_every_input(
            timestamp in any::<i64>(),
            height in any::<i64>(),
            sequence in any::<i32>(),
            entries in proptest::collection::btree_map(0u8..200, any::<u16>(), 1..12),
        ) {
            let mut data = BlockData::empty();
            data.time_stamp = timestamp;
            data.block_number = height;
            data.seq_num = sequence;
            let invalid: HashMap<BlockHash, Validator> = entries.iter().map(|(key, value)| (
                vec![*key; 32].into(), value.to_le_bytes().to_vec().into(),
            )).collect();
            let context = ReplayCacheContext::new(&data, &invalid);
            let original = ReplayCacheKey::new(vec![1; 32].into(), context, vec![2; 32]);
            let cache = InMemoryReplayCache::with_limits(16, 65536);
            prop_assert!(cache.put(original.clone(), make_entry("expected")));

            for axis in 0..8 {
                let mut changed_data = data.clone();
                let mut changed_invalid = invalid.clone();
                let mut parent = vec![1; 32];
                let mut payload = vec![2; 32];
                match axis {
                    0 => parent[0] ^= 1,
                    1 => payload[0] ^= 1,
                    2 => changed_data.sender = crypto::rust::public_key::PublicKey::from_bytes(&[1]),
                    3 => changed_data.seq_num = sequence.wrapping_add(1),
                    4 => changed_data.time_stamp = timestamp.wrapping_add(1),
                    5 => changed_data.block_number = height.wrapping_add(1),
                    6 => { changed_invalid.insert(vec![255; 32].into(), vec![3; 32].into()); }
                    7 => {
                        let (key, value) = entries.first_key_value().unwrap();
                        changed_invalid.insert(
                            vec![*key; 32].into(),
                            value.wrapping_add(1).to_le_bytes().to_vec().into(),
                        );
                    }
                    _ => unreachable!(),
                }
                let changed = ReplayCacheKey::new(
                    parent.into(), ReplayCacheContext::new(&changed_data, &changed_invalid), payload,
                );
                prop_assert_ne!(&original, &changed, "context axis {}", axis);
                prop_assert!(cache.get(&changed).is_none(), "context axis {}", axis);
            }
            prop_assert_eq!(cache.get(&original).unwrap().post_state, make_entry("expected").post_state);
        }

        #[test]
        fn replay_cache_invalid_map_order_is_irrelevant(
            entries in proptest::collection::btree_map(any::<u8>(), any::<u16>(), 0..32),
        ) {
            let encode = |(key, value): (&u8, &u16)| -> (BlockHash, Validator) {
                (vec![*key; 32].into(), value.to_le_bytes().to_vec().into())
            };
            let forward = entries.iter().map(encode).collect();
            let reverse = entries.iter().rev().map(encode).collect();
            prop_assert_eq!(
                ReplayCacheContext::new(&BlockData::empty(), &forward),
                ReplayCacheContext::new(&BlockData::empty(), &reverse),
            );
        }
    }

    #[test]
    fn replay_cache_invalid_map_encoding_separates_key_and_value() {
        let left = HashMap::from([(vec![1].into(), vec![2, 3].into())]);
        let right = HashMap::from([(vec![1, 2].into(), vec![3].into())]);
        assert_ne!(
            ReplayCacheContext::new(&BlockData::empty(), &left),
            ReplayCacheContext::new(&BlockData::empty(), &right),
        );
    }

    #[test]
    fn concurrent_cache_callers_keep_distinct_runtime_contexts() {
        let cache = Arc::new(InMemoryReplayCache::with_limits(2, 65536));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let callers: Vec<_> = (1..=2)
            .map(|timestamp| {
                let cache = cache.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut data = BlockData::empty();
                    data.time_stamp = timestamp;
                    let key = ReplayCacheKey::new(
                        vec![1; 32].into(),
                        ReplayCacheContext::new(&data, &HashMap::new()),
                        vec![2; 32],
                    );
                    let post_state: StateHash = vec![timestamp as u8; 32].into();
                    assert!(cache.put(
                        key.clone(),
                        ReplayCacheEntry::new(Vec::new(), post_state.clone())
                    ));
                    barrier.wait();
                    assert_eq!(cache.get(&key).unwrap().post_state, post_state);
                })
            })
            .collect();
        for caller in callers {
            caller.join().unwrap();
        }
        assert_eq!(cache.len(), 2);
    }

    fn make_key(parent: &str, sender: &str, seq: i32) -> ReplayCacheKey {
        let mut block_data = BlockData::empty();
        block_data.sender = crypto::rust::public_key::PublicKey::from_bytes(sender.as_bytes());
        block_data.seq_num = seq;
        ReplayCacheKey::new(
            parent.as_bytes().to_vec().into(),
            ReplayCacheContext::new(&block_data, &HashMap::new()),
            vec![0u8; 32],
        )
    }

    fn make_entry(post: &str) -> ReplayCacheEntry {
        ReplayCacheEntry::new(vec![], post.as_bytes().to_vec().into())
    }

    fn make_event_entry() -> ReplayCacheEntry {
        ReplayCacheEntry::new(
            vec![Event::Produce(ProduceEvent {
                channels_hash: prost::bytes::Bytes::from_static(b"channel"),
                hash: prost::bytes::Bytes::from_static(b"hash"),
                persistent: false,
                times_repeated: 1,
                is_deterministic: true,
                output_value: vec![prost::bytes::Bytes::from_static(b"payload")],
                failed: false,
            })],
            prost::bytes::Bytes::from_static(b"post"),
        )
    }

    #[test]
    fn test_store_and_retrieve() {
        let cache = InMemoryReplayCache::default_capacity();
        let key = make_key("parent", "sender", 1);
        let entry = make_entry("post-state");

        cache.put(key.clone(), entry.clone());
        let result = cache.get(&key);
        assert!(result.is_some());
    }

    #[test]
    fn test_miss_for_unknown_key() {
        let cache = InMemoryReplayCache::default_capacity();
        let key = make_key("unknown", "sender", 42);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_eviction_when_over_capacity() {
        let cache = InMemoryReplayCache::new(2);

        let k1 = make_key("p1", "a", 1);
        let k2 = make_key("p2", "b", 2);
        let k3 = make_key("p3", "c", 3);
        let e = make_entry("post");

        cache.put(k1.clone(), e.clone());
        cache.put(k2.clone(), e.clone());
        cache.put(k3.clone(), e.clone());

        // k1 should be evicted
        assert!(cache.get(&k1).is_none());
        assert!(cache.get(&k2).is_some());
        assert!(cache.get(&k3).is_some());
    }

    #[test]
    fn test_clear() {
        let cache = InMemoryReplayCache::default_capacity();
        let key = make_key("p", "s", 5);
        let entry = make_entry("post");

        cache.put(key.clone(), entry);
        cache.clear();
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.retained_bytes(), 0);
    }

    #[test]
    fn test_eviction_when_over_byte_capacity() {
        let k1 = make_key("p1", "a", 1);
        let k2 = make_key("p2", "b", 2);
        let c1 = charged_bytes(&k1, &make_entry("four"));
        let c2 = charged_bytes(&k2, &make_entry("five"));
        let cache = InMemoryReplayCache::with_limits(10, c1 + c2 - 1);

        cache.put(k1.clone(), make_entry("four"));
        cache.put(k2.clone(), make_entry("five"));

        assert!(cache.get(&k1).is_none());
        assert!(cache.get(&k2).is_some());
        assert_eq!(cache.stats(), (1, c2));
    }

    #[test]
    fn test_byte_capacity_eviction_honors_lru_access() {
        let k1 = make_key("p1", "a", 1);
        let k2 = make_key("p2", "b", 2);
        let k3 = make_key("p3", "c", 3);
        let c1 = charged_bytes(&k1, &make_entry("four"));
        let c2 = charged_bytes(&k2, &make_entry("five"));
        let c3 = charged_bytes(&k3, &make_entry("six!"));
        let cache = InMemoryReplayCache::with_limits(10, c1 + c2);

        cache.put(k1.clone(), make_entry("four"));
        cache.put(k2.clone(), make_entry("five"));
        assert!(cache.get(&k1).is_some());
        cache.put(k3.clone(), make_entry("six!"));

        assert!(cache.get(&k2).is_none());
        assert!(cache.get(&k1).is_some());
        assert!(cache.get(&k3).is_some());
        assert_eq!(cache.stats(), (2, c1 + c3));
    }

    #[test]
    fn test_oversized_entry_is_not_cached() {
        let key = make_key("p", "s", 1);
        let charged = charged_bytes(&key, &make_entry("four"));
        let cache = InMemoryReplayCache::with_limits(10, charged - 1);

        assert!(!cache.put(key.clone(), make_entry("four")));

        assert!(cache.get(&key).is_none());
        assert_eq!(cache.stats(), (0, 0));
    }

    #[test]
    fn test_entry_construction_copies_shared_bytes_backing() {
        let backing = prost::bytes::Bytes::from(vec![7u8; 4096]);
        let slice = backing.slice(0..8);
        let post_slice = backing.slice(8..16);

        let entry = ReplayCacheEntry::new(
            vec![Event::Produce(ProduceEvent {
                channels_hash: slice.clone(),
                hash: slice.clone(),
                persistent: false,
                times_repeated: 1,
                is_deterministic: true,
                output_value: vec![slice.clone()],
                failed: false,
            })],
            post_slice.clone(),
        );

        let Event::Produce(produce) = &entry.event_log[0] else {
            panic!("expected produce event");
        };
        assert_eq!(produce.channels_hash.as_ref(), slice.as_ref());
        assert_ne!(
            produce.channels_hash.as_ref().as_ptr(),
            slice.as_ref().as_ptr()
        );
        assert_ne!(
            produce.output_value[0].as_ref().as_ptr(),
            slice.as_ref().as_ptr()
        );
        assert_eq!(entry.post_state.as_ref(), post_slice.as_ref());
        assert_ne!(
            entry.post_state.as_ref().as_ptr(),
            post_slice.as_ref().as_ptr()
        );
    }

    #[test]
    fn test_put_copies_shared_key_backing() {
        let backing = prost::bytes::Bytes::from(vec![9u8; 4096]);
        let parent_slice = backing.slice(0..8);
        let mut key = make_key("p", "s", 1);
        key.parent_state = parent_slice.clone();
        let cache = InMemoryReplayCache::default_capacity();

        assert!(cache.put(key.clone(), make_entry("post")));

        let state = cache.state.lock().unwrap();
        let (stored_key, _) = state.entries().next().unwrap();
        assert_eq!(stored_key.parent_state.as_ref(), parent_slice.as_ref());
        assert_ne!(
            stored_key.parent_state.as_ref().as_ptr(),
            parent_slice.as_ref().as_ptr()
        );
    }

    #[test]
    fn test_key_bytes_count_toward_capacity() {
        let key = make_key("p", "s", 1);
        let entry = make_entry("four");
        let cache = InMemoryReplayCache::with_limits(10, entry.retained_bytes());

        assert!(!cache.put(key.clone(), entry));
        assert_eq!(cache.stats(), (0, 0));
    }

    #[test]
    fn test_event_heap_bytes_count_toward_capacity() {
        let key = make_key("p", "s", 1);
        let entry = make_event_entry();
        let charged = charged_bytes(&key, &entry);
        let cache = InMemoryReplayCache::with_limits(10, charged - 1);

        assert!(!cache.put(key.clone(), entry.clone()));
        assert!(cache.get(&key).is_none());

        let cache = InMemoryReplayCache::with_limits(10, charged);
        assert!(cache.put(key.clone(), entry));
        assert_eq!(cache.stats(), (1, charged));
    }

    #[test]
    fn test_rejected_replacement_preserves_cached_entry() {
        let key = make_key("p", "s", 1);
        let c_four = charged_bytes(&key, &make_entry("four"));
        let cache = InMemoryReplayCache::with_limits(10, c_four);

        assert!(cache.put(key.clone(), make_entry("four")));
        assert!(!cache.put(key.clone(), make_entry("oversized")));

        assert_eq!(cache.get(&key).unwrap().post_state.as_ref(), b"four");
        assert_eq!(cache.stats(), (1, c_four));
    }

    #[test]
    fn test_replacement_updates_retained_bytes() {
        let key = make_key("p", "s", 1);
        let c_larger = charged_bytes(&key, &make_entry("larger"));
        let cache = InMemoryReplayCache::with_limits(10, c_larger);

        assert!(cache.put(key.clone(), make_entry("four")));
        assert!(cache.put(key.clone(), make_entry("larger")));

        assert_eq!(cache.get(&key).unwrap().post_state.as_ref(), b"larger");
        assert_eq!(cache.stats(), (1, c_larger));
    }

    #[test]
    fn test_zero_entry_capacity_rejects_admission() {
        let cache = InMemoryReplayCache::with_limits(0, usize::MAX);
        let key = make_key("p", "s", 1);

        assert!(!cache.put(key.clone(), make_entry("post")));
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.stats(), (0, 0));
    }

    mod invariants {
        use proptest::prelude::*;

        use super::*;

        #[derive(Debug, Clone)]
        enum Op {
            Put {
                key: u8,
                payload_len: usize,
                with_event: bool,
            },
            Get {
                key: u8,
            },
            Clear,
        }

        fn op_strategy() -> impl Strategy<Value = Op> {
            prop_oneof![
                (0u8..4, 0usize..64, any::<bool>()).prop_map(|(key, payload_len, with_event)| {
                    Op::Put {
                        key,
                        payload_len,
                        with_event,
                    }
                }),
                (0u8..4).prop_map(|key| Op::Get { key }),
                Just(Op::Clear),
            ]
        }

        fn prop_key(i: u8) -> ReplayCacheKey {
            make_key("prop-parent", "prop-sender", i32::from(i))
        }

        fn sized_entry(payload_len: usize, with_event: bool) -> ReplayCacheEntry {
            let event_log = if with_event {
                vec![Event::Produce(ProduceEvent {
                    channels_hash: prost::bytes::Bytes::from_static(b"channel"),
                    hash: prost::bytes::Bytes::from_static(b"hash"),
                    persistent: false,
                    times_repeated: 1,
                    is_deterministic: true,
                    output_value: vec![prost::bytes::Bytes::from(vec![0u8; payload_len])],
                    failed: false,
                })]
            } else {
                vec![]
            };
            ReplayCacheEntry::new(event_log, prost::bytes::Bytes::from(vec![0u8; payload_len]))
        }

        proptest! {
            // The invariants that make the byte cap an actual bound on
            // retained memory, for every reachable op sequence: len never
            // exceeds max_entries, retained_bytes never exceeds max_bytes,
            // the accounting equals the sum over live entries (no drift
            // through the eviction/replacement saturating arithmetic),
            // admission follows the documented contract, and a hit moves its
            // key to most-recently-used position.
            #[test]
            fn cache_invariants_hold_for_any_op_sequence(
                max_entries in 0usize..5,
                max_bytes in 0usize..512,
                ops in prop::collection::vec(op_strategy(), 1..48),
            ) {
                let cache = InMemoryReplayCache::with_limits(max_entries, max_bytes);
                let mut reference: Vec<(ReplayCacheKey, ReplayCacheEntry)> = Vec::new();
                for op in ops {
                    match op {
                        Op::Put { key, payload_len, with_event } => {
                            let entry = sized_entry(payload_len, with_event);
                            let charged = charged_bytes(&prop_key(key), &entry);
                            if max_entries > 0 && charged <= max_bytes {
                                reference.retain(|(stored, _)| stored != &prop_key(key));
                                reference.push((prop_key(key), entry.clone()));
                                while reference.len() > max_entries
                                    || reference.iter().map(|(k, v)| charged_bytes(k, v)).sum::<usize>() > max_bytes
                                {
                                    reference.remove(0);
                                }
                            }
                            let admitted = cache.put(prop_key(key), entry);
                            prop_assert_eq!(
                                admitted,
                                max_entries > 0 && charged <= max_bytes
                            );
                        }
                        Op::Get { key } => {
                            let expected = reference.iter().position(|(stored, _)| stored == &prop_key(key))
                                .map(|index| {
                                    let pair = reference.remove(index);
                                    let value = pair.1.clone();
                                    reference.push(pair);
                                    value
                                });
                            let actual = cache.get(&prop_key(key));
                            prop_assert_eq!(actual.as_ref().map(|v| &v.post_state), expected.as_ref().map(|v| &v.post_state));
                            prop_assert_eq!(actual.as_ref().map(|v| &v.event_log), expected.as_ref().map(|v| &v.event_log));
                            if actual.is_some() {
                                let state = cache.state.lock().unwrap();
                                let (last_key, _) = state.entries().next_back().unwrap();
                                prop_assert!(last_key == &prop_key(key));
                            }
                        }
                        Op::Clear => {
                            reference.clear();
                            cache.clear();
                            prop_assert_eq!(cache.stats(), (0, 0));
                        }
                    }
                    let state = cache.state.lock().unwrap();
                    prop_assert!(state.stats().0 <= max_entries);
                    prop_assert!(state.stats().1 <= max_bytes);
                    let live_sum: usize = state
                        .entries()
                        .map(|(key, entry)| charged_bytes(key, entry))
                        .sum();
                    prop_assert_eq!(state.stats().1, live_sum);
                    prop_assert_eq!(state.stats().0, reference.len());
                    for ((key, entry), (expected_key, expected_entry)) in state.entries().zip(&reference) {
                        prop_assert_eq!(key, expected_key);
                        prop_assert_eq!(&entry.post_state, &expected_entry.post_state);
                        prop_assert_eq!(&entry.event_log, &expected_entry.event_log);
                    }
                }
            }
        }
    }
}
