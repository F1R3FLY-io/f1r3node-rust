// See casper/src/main/scala/coop/rchain/casper/util/rholang/ReplayCache.scala

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{Event, Peek, ProduceEvent};

/// Cache key: parent state + block identity (sender, seqNum) + replay payload fingerprint.
/// Including a payload fingerprint prevents unsafe cache hits for mutated deploy content
/// that happens to share (parent, sender, seqNum).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplayCacheKey {
    pub parent_state: StateHash,
    pub sender_pk: Vec<u8>,
    pub seq_num: i64,
    pub payload_hash: Vec<u8>,
}

impl ReplayCacheKey {
    pub fn new(
        parent_state: StateHash,
        sender_pk: Vec<u8>,
        seq_num: i64,
        payload_hash: Vec<u8>,
    ) -> Self {
        Self {
            parent_state,
            sender_pk,
            seq_num,
            payload_hash,
        }
    }

    fn heap_bytes(&self) -> usize {
        self.parent_state
            .len()
            .saturating_add(self.sender_pk.len())
            .saturating_add(self.payload_hash.len())
    }
}

/// Cached replay result containing event log and post-state hash.
#[derive(Clone, Debug)]
pub struct ReplayCacheEntry {
    pub event_log: Arc<Vec<Event>>,
    pub post_state: StateHash,
    retained_bytes: usize,
}

impl ReplayCacheEntry {
    pub fn new(event_log: Vec<Event>, post_state: StateHash) -> Self {
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

struct ReplayCacheState {
    map: IndexMap<ReplayCacheKey, ReplayCacheEntry>,
    retained_bytes: usize,
}

/// Uses lengths rather than capacities so two equal keys always charge the
/// same amount — the byte accounting must be reproducible from the stored
/// (key, entry) pair alone when an eviction credits it back.
fn charged_bytes(key: &ReplayCacheKey, entry: &ReplayCacheEntry) -> usize {
    key.heap_bytes().saturating_add(entry.retained_bytes())
}

/// Simple in-memory LRU replay cache (thread-safe).
pub struct InMemoryReplayCache {
    state: Mutex<ReplayCacheState>,
    max_entries: usize,
    max_bytes: usize,
}

impl InMemoryReplayCache {
    pub fn new(max_entries: usize) -> Self { Self::with_limits(max_entries, usize::MAX) }

    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            state: Mutex::new(ReplayCacheState {
                map: IndexMap::with_capacity(max_entries),
                retained_bytes: 0,
            }),
            max_entries,
            max_bytes,
        }
    }

    /// Create with default capacity (1024 entries).
    pub fn default_capacity() -> Self { Self::new(1024) }

    pub fn stats(&self) -> (usize, usize) {
        let state = self.state.lock().expect("ReplayCache lock poisoned");
        (state.map.len(), state.retained_bytes)
    }

    pub fn len(&self) -> usize { self.stats().0 }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn retained_bytes(&self) -> usize { self.stats().1 }
}

impl ReplayCache for InMemoryReplayCache {
    fn get(&self, key: &ReplayCacheKey) -> Option<ReplayCacheEntry> {
        let mut state = self.state.lock().expect("ReplayCache lock poisoned");
        // Move-to-back keeps the stored key: re-inserting the caller's clone
        // could swap a compact key for one backed by a larger shared
        // allocation, silently changing what the cache actually retains.
        let index = state.map.get_index_of(key)?;
        let last = state.map.len() - 1;
        state.map.move_index(index, last);
        state.map.get_index(last).map(|(_, entry)| entry.clone())
    }

    fn put(&self, key: ReplayCacheKey, entry: ReplayCacheEntry) -> bool {
        let charged = charged_bytes(&key, &entry);
        let mut state = self.state.lock().expect("ReplayCache lock poisoned");

        if self.max_entries == 0 || charged > self.max_bytes {
            return false;
        }

        if let Some((replaced_key, replaced)) = state.map.shift_remove_entry(&key) {
            state.retained_bytes = state
                .retained_bytes
                .saturating_sub(charged_bytes(&replaced_key, &replaced));
        }

        state.retained_bytes = state.retained_bytes.saturating_add(charged);
        state.map.insert(key, entry);

        while state.map.len() > self.max_entries || state.retained_bytes > self.max_bytes {
            let Some((removed_key, removed)) = state.map.shift_remove_index(0) else {
                state.retained_bytes = 0;
                break;
            };
            state.retained_bytes = state
                .retained_bytes
                .saturating_sub(charged_bytes(&removed_key, &removed));
        }

        true
    }

    fn clear(&self) {
        let mut state = self.state.lock().expect("ReplayCache lock poisoned");
        state.map.clear();
        state.retained_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(parent: &str, sender: &str, seq: i64) -> ReplayCacheKey {
        ReplayCacheKey::new(
            parent.as_bytes().to_vec().into(),
            sender.as_bytes().to_vec(),
            seq,
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

        fn prop_key(i: u8) -> ReplayCacheKey { make_key("prop-parent", "prop-sender", i as i64) }

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
                for op in ops {
                    match op {
                        Op::Put { key, payload_len, with_event } => {
                            let entry = sized_entry(payload_len, with_event);
                            let charged = charged_bytes(&prop_key(key), &entry);
                            let admitted = cache.put(prop_key(key), entry);
                            prop_assert_eq!(
                                admitted,
                                max_entries > 0 && charged <= max_bytes
                            );
                        }
                        Op::Get { key } => {
                            if cache.get(&prop_key(key)).is_some() {
                                let state = cache.state.lock().unwrap();
                                let (last_key, _) =
                                    state.map.get_index(state.map.len() - 1).unwrap();
                                prop_assert!(last_key == &prop_key(key));
                            }
                        }
                        Op::Clear => {
                            cache.clear();
                            prop_assert_eq!(cache.stats(), (0, 0));
                        }
                    }
                    let state = cache.state.lock().unwrap();
                    prop_assert!(state.map.len() <= max_entries);
                    prop_assert!(state.retained_bytes <= max_bytes);
                    let live_sum: usize = state
                        .map
                        .iter()
                        .map(|(key, entry)| charged_bytes(key, entry))
                        .sum();
                    prop_assert_eq!(state.retained_bytes, live_sum);
                }
            }
        }
    }
}
