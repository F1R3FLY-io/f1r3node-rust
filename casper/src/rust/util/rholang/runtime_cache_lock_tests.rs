use std::cell::RefCell;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::try_result::TryResult;
use proptest::prelude::*;
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;

type Observer = Box<dyn FnOnce()>;

thread_local! {
    static ORDER_OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
    static INDEX_OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

pub(super) fn before_order_lock() {
    let observer = ORDER_OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer();
    }
}

pub(super) fn before_index_recheck() {
    let observer = INDEX_OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer();
    }
}

struct GuardProbe(Arc<Mutex<Option<bool>>>);

impl GuardProbe {
    fn install<K, V>(map: Arc<DashMap<K, V>>, key: K) -> Self
    where
        K: Eq + Hash + 'static,
        V: 'static,
    {
        let observed = Arc::new(Mutex::new(None));
        let result = observed.clone();
        ORDER_OBSERVER.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Box::new(move || {
                let available = match map.try_get_mut(&key) {
                    TryResult::Present(guard) => {
                        drop(guard);
                        true
                    }
                    TryResult::Locked => false,
                    TryResult::Absent => panic!("the seeded cache entry disappeared"),
                };
                *result.lock().unwrap() = Some(available);
            }));
        });
        Self(observed)
    }

    fn assert_released(&self) {
        assert_eq!(
            *self.0.lock().unwrap(),
            Some(true),
            "the cache shard guard must be released before requesting the order mutex",
        );
    }
}

impl Drop for GuardProbe {
    fn drop(&mut self) {
        ORDER_OBSERVER.with(|slot| slot.borrow_mut().take());
        INDEX_OBSERVER.with(|slot| slot.borrow_mut().take());
    }
}

fn manager() -> RuntimeManager {
    let stores = RSpaceStore {
        history: Arc::new(InMemoryKeyValueStore::new()),
        roots: Arc::new(InMemoryKeyValueStore::new()),
        cold: Arc::new(InMemoryKeyValueStore::new()),
    };
    RuntimeManager::create_with_history(
        stores,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    )
    .0
}

#[test]
fn probe_accepts_touch_without_a_shard_guard() {
    let map = Arc::new(DashMap::new());
    map.insert(1_u8, 2_u8);
    let order = Mutex::new(VecDeque::from([3, 1]));
    let probe = GuardProbe::install(map, 1);
    RuntimeManager::touch_cache_key(&order, &1);
    assert_eq!(*order.lock().unwrap(), VecDeque::from([3, 1]));
    probe.assert_released();
}

#[tokio::test]
async fn active_validator_hit_releases_shard_before_order() {
    let manager = manager();
    let key: StateHash = vec![1; 32].into();
    let value: Vec<Validator> = vec![vec![2; 32].into()];
    manager
        .active_validators_cache
        .insert(key.clone(), value.clone());
    let probe = GuardProbe::install(manager.active_validators_cache.clone(), key.clone());
    assert_eq!(manager.get_active_validators(&key).await.unwrap(), value);
    assert_eq!(
        *manager.active_validators_cache_order.lock().unwrap(),
        VecDeque::from([key])
    );
    probe.assert_released();
}

#[tokio::test]
async fn bond_hit_releases_shard_before_order() {
    let manager = manager();
    let key: StateHash = vec![3; 32].into();
    let value = vec![Bond {
        validator: vec![4; 32].into(),
        stake: 5,
    }];
    manager.bonds_cache.insert(key.clone(), value.clone());
    let probe = GuardProbe::install(manager.bonds_cache.clone(), key.clone());
    assert_eq!(manager.compute_bonds(&key).await.unwrap(), value);
    assert_eq!(
        *manager.bonds_cache_order.lock().unwrap(),
        VecDeque::from([key])
    );
    probe.assert_released();
}

#[tokio::test]
async fn generation_hit_releases_shard_before_order() {
    let manager = manager();
    let key: StateHash = vec![6; 32].into();
    let value = HashMap::from([(vec![7; 32].into(), 8)]);
    manager
        .bond_generations_cache
        .insert(key.clone(), value.clone());
    let probe = GuardProbe::install(manager.bond_generations_cache.clone(), key.clone());
    assert_eq!(manager.compute_bond_generations(&key).await.unwrap(), value);
    assert_eq!(
        *manager.bond_generations_cache_order.lock().unwrap(),
        VecDeque::from([key])
    );
    probe.assert_released();
}

#[tokio::test]
async fn parent_state_hit_releases_shard_before_order() {
    let manager = manager();
    let key = ParentsPostStateCacheKey::new(
        vec![9; 32].into(),
        vec![],
        vec![10; 32].into(),
        vec![],
        false,
        false,
    );
    let state: StateHash = vec![11; 32].into();
    let value = MergedPreState {
        state: state.clone(),
        rejected_user: vec![],
        rejected_state_effects: vec![],
        applied_state_effects: vec![],
        rejected_slashes: vec![],
        applied_from_scope: Default::default(),
        merge_base: Some(vec![12; 32].into()),
    };
    manager.parents_post_state_cache.insert(key.clone(), value);
    let probe = GuardProbe::install(manager.parents_post_state_cache.clone(), key.clone());
    let cached = manager.get_cached_parents_post_state(&key).unwrap();
    assert_eq!(cached.state, state);
    assert_eq!(cached.merge_base, Some(vec![12; 32].into()));
    assert_eq!(
        *manager.parents_post_state_cache_order.lock().unwrap(),
        VecDeque::from([key])
    );
    probe.assert_released();
}

async fn check_block_index_hit(recheck: bool) {
    let manager = manager();
    let key: BlockHash = vec![13; 32].into();
    let value = BlockIndex {
        block_hash: key.clone(),
        deploy_chains: vec![],
    };
    let inserted = Arc::new(AtomicBool::new(false));
    if recheck {
        let map = manager.block_index_cache.clone();
        let cache_key = key.clone();
        let observed = inserted.clone();
        INDEX_OBSERVER.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                map.insert(cache_key, value);
                observed.store(true, Ordering::SeqCst);
            }));
        });
    } else {
        manager.block_index_cache.insert(key.clone(), value);
    }
    let probe = GuardProbe::install(manager.block_index_cache.clone(), key.clone());
    let root = manager.history_repo.root();
    let cached = manager
        .get_or_compute_block_index(&key, 0, &vec![], &vec![], &root, &root, &vec![])
        .unwrap();
    assert_eq!(cached.block_hash, key);
    assert!(cached.deploy_chains.is_empty());
    assert_eq!(inserted.load(Ordering::SeqCst), recheck);
    assert_eq!(
        *manager.block_index_cache_order.lock().unwrap(),
        VecDeque::from([key])
    );
    probe.assert_released();
}

#[tokio::test]
async fn block_index_fast_hit_releases_shard_before_order() { check_block_index_hit(false).await; }

#[tokio::test]
async fn block_index_recheck_releases_shard_before_order() { check_block_index_hit(true).await; }

proptest! {
    #[test]
    fn cache_touch_preserves_unique_order_without_a_shard_guard(
        keys in proptest::collection::btree_set(any::<u8>(), 0..64),
        key in any::<u8>(),
        reverse in any::<bool>(),
    ) {
        let mut initial = keys.into_iter().collect::<VecDeque<_>>();
        if reverse {
            initial.make_contiguous().reverse();
        }
        let mut expected = initial.clone();
        expected.retain(|existing| *existing != key);
        expected.push_back(key);
        let map = Arc::new(DashMap::new());
        map.insert(key, key);
        let order = Mutex::new(initial);
        let probe = GuardProbe::install(map.clone(), key);
        RuntimeManager::touch_cache_key(&order, &key);
        prop_assert_eq!(&*order.lock().unwrap(), &expected);
        prop_assert_eq!(map.get(&key).map(|entry| *entry), Some(key));
        probe.assert_released();
    }

    #[test]
    fn generic_eviction_skips_stale_order_and_preserves_owned_values(
        values in proptest::collection::btree_map(any::<u8>(), any::<u64>(), 0..32),
        ordered_keys in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let map = DashMap::new();
        for (key, value) in &values {
            map.insert(*key, *value);
        }
        let copies = map.iter().map(|entry| (*entry.key(), *entry.value()))
            .collect::<BTreeMap<_, _>>();
        let victim_position = ordered_keys.iter().position(|key| values.contains_key(key));
        let victim = victim_position.map(|position| ordered_keys[position]);
        let order = Mutex::new(VecDeque::from(ordered_keys.clone()));
        RuntimeManager::evict_fifo_entry(&map, &order);
        let mut expected_values = values.clone();
        if let Some(victim) = victim {
            expected_values.remove(&victim);
        }
        let remaining = map.iter().map(|entry| (*entry.key(), *entry.value()))
            .collect::<BTreeMap<_, _>>();
        prop_assert_eq!(remaining, expected_values);
        let expected_order = victim_position.map(|position| {
            ordered_keys[position + 1..].iter().copied().collect::<VecDeque<_>>()
        }).unwrap_or_default();
        prop_assert_eq!(&*order.lock().unwrap(), &expected_order);
        prop_assert_eq!(copies, values);
    }

    #[test]
    fn validator_hit_returns_an_owned_copy_across_eviction(
        key_bytes in any::<[u8; 32]>(),
        validators in proptest::collection::vec(any::<[u8; 32]>(), 0..16),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let manager = manager();
            let key: StateHash = key_bytes.to_vec().into();
            let value = validators.into_iter().map(|v| v.to_vec().into()).collect::<Vec<Validator>>();
            manager.active_validators_cache.insert(key.clone(), value.clone());
            let probe = GuardProbe::install(manager.active_validators_cache.clone(), key.clone());
            let copied = manager.get_active_validators(&key).await.unwrap();
            probe.assert_released();
            RuntimeManager::evict_fifo_entry(
                &manager.active_validators_cache,
                &manager.active_validators_cache_order,
            );
            assert!(manager.active_validators_cache.is_empty());
            assert!(manager.active_validators_cache_order.lock().unwrap().is_empty());
            assert_eq!(copied, value);
        });
    }

    #[test]
    fn block_index_hit_and_eviction_preserve_owned_identity_and_byte_accounting(
        key_bytes in any::<[u8; 32]>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let manager = manager();
            let key: BlockHash = key_bytes.to_vec().into();
            let root = manager.history_repo.root();
            let first = manager.get_or_compute_block_index(
                &key, 0, &vec![], &vec![], &root, &root, &vec![],
            ).unwrap();
            let charged = first.retained_bytes();
            assert_eq!(manager.block_index_cache_retained_bytes.load(Ordering::Acquire), charged);
            let probe = GuardProbe::install(manager.block_index_cache.clone(), key.clone());
            let copied = manager.get_or_compute_block_index(
                &key, 0, &vec![], &vec![], &root, &root, &vec![],
            ).unwrap();
            probe.assert_released();
            assert!(manager.evict_block_index_entry());
            assert!(manager.block_index_cache.is_empty());
            assert_eq!(manager.block_index_cache_retained_bytes.load(Ordering::Acquire), 0);
            assert_eq!(copied.block_hash, first.block_hash);
            assert!(copied.deploy_chains.is_empty());
        });
    }
}
