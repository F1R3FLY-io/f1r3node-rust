use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

impl IndexKey for u64 {
    fn comparison_work(&self) -> (usize, usize) { (1, 8) }
}

fn host() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn verify<K: IndexKey + std::fmt::Debug, V: PartialEq + std::fmt::Debug>(
    index: &NativeIndex<K, V>,
    expected: &BTreeMap<K, V>,
) {
    fn walk<K: IndexKey + std::fmt::Debug, V>(
        index: &NativeIndex<K, V>,
        current: Option<usize>,
        parent: Option<usize>,
        lower: Option<&K>,
        upper: Option<&K>,
        visited: &mut BTreeSet<usize>,
    ) -> usize {
        let Some(id) = current else {
            return 0;
        };
        assert!(visited.insert(id), "index has repeated links");
        let node = &index.nodes[id];
        assert_eq!(node.parent, parent);
        if let Some(lower) = lower {
            assert!(lower < &node.key);
        }
        if let Some(upper) = upper {
            assert!(&node.key < upper);
        }
        let left = walk(index, node.left, Some(id), lower, Some(&node.key), visited);
        let right = walk(index, node.right, Some(id), Some(&node.key), upper, visited);
        assert_eq!(node.height, 1 + left.max(right));
        assert!(left.abs_diff(right) <= 1, "index lost AVL balance");
        node.height
    }
    let mut visited = BTreeSet::new();
    let height = walk(index, index.root, None, None, None, &mut visited);
    assert_eq!(visited.len(), index.nodes.len());
    assert!(height < height_bound(index.len()));
    assert_eq!(index.len(), expected.len());
    assert_eq!(
        index.keys().collect::<BTreeSet<_>>(),
        expected.keys().collect()
    );
    let budget = host();
    for (key, value) in expected {
        assert_eq!(index.get(key, &budget).unwrap(), Some(value));
        assert_eq!(index.get_prepaid(key), Some(value));
    }
}

#[test]
fn native_index_preserves_every_small_insertion_permutation() {
    fn permutations(prefix: &mut Vec<u64>, remaining: &mut Vec<u64>) {
        if remaining.is_empty() {
            let mut index = NativeIndex::default();
            let mut expected = BTreeMap::new();
            let budget = host();
            for key in prefix.iter().copied() {
                let stable: Vec<_> = index.nodes.iter().map(|node| node.key).collect();
                let prepared = index.prepare_insert(key, key + 10, &budget).unwrap();
                assert_eq!(index.len(), expected.len());
                assert_eq!(index.commit(prepared), expected.insert(key, key + 10));
                for (position, key) in stable.iter().enumerate() {
                    assert_eq!(&index.nodes[position].key, key);
                }
                verify(&index, &expected);
            }
            return;
        }
        for offset in 0..remaining.len() {
            let key = remaining.remove(offset);
            prefix.push(key);
            permutations(prefix, remaining);
            prefix.pop();
            remaining.insert(offset, key);
        }
    }
    permutations(&mut Vec::new(), &mut (0..6).collect());
}

#[derive(Clone, Debug)]
struct CountedKey {
    value: u64,
    calls: Arc<AtomicUsize>,
}

impl PartialEq for CountedKey {
    fn eq(&self, other: &Self) -> bool { self.value == other.value }
}
impl Eq for CountedKey {}
impl PartialOrd for CountedKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for CountedKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.calls.fetch_add(1, AtomicOrdering::Relaxed);
        self.value.cmp(&other.value)
    }
}
impl IndexKey for CountedKey {
    fn comparison_work(&self) -> (usize, usize) { (1, 8) }
}

#[test]
fn native_index_stops_before_unfunded_comparison_or_growth() {
    let calls = Arc::new(AtomicUsize::new(0));
    let key = CountedKey {
        value: 1,
        calls: Arc::clone(&calls),
    };
    let mut index = NativeIndex::default();
    let prepared = index.prepare_insert(key.clone(), 3, &host()).unwrap();
    index.commit(prepared);
    calls.store(0, AtomicOrdering::Relaxed);
    let empty = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(index.get(&key, &empty).is_err());
    assert_eq!(calls.load(AtomicOrdering::Relaxed), 0);
    assert_eq!(index.len(), 1);
    let mut empty_index = NativeIndex::default();
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000));
    limits.set(HostWorkDimension::SearchStateBytes, HostWorkLimit::new(0));
    let limited = HostWorkBudget::new(limits);
    assert!(empty_index.prepare_insert(1u64, 3, &limited).is_err());
    assert_eq!(empty_index.len(), 0);
    assert_eq!(empty_index.logical_capacity, 0);
    assert!(empty_index.root.is_none());
}

#[test]
fn native_index_prepaid_completion_survives_independent_growth() {
    let mut index = NativeIndex::default();
    let budget = host();
    let prepared = index.prepare_insert(0u64, 7, &budget).unwrap();
    index.commit(prepared);
    reserve_lookup(0u64.comparison_work(), 512, &budget).unwrap();
    for key in 1..512 {
        let prepared = index.prepare_insert(key, 0, &budget).unwrap();
        index.commit(prepared);
    }
    let before = budget.report();
    assert_eq!(index.get_prepaid(&0), Some(&7));
    assert_eq!(budget.report(), before);
    assert!(index.height(index.root) < height_bound(512));
}

#[test]
fn native_index_growth_charges_logical_backing_and_moves_even_with_allocator_excess() {
    let budget = host();
    let mut values: Vec<u64> = Vec::with_capacity(64);
    let mut logical = 0;
    reserve_vector(&mut values, &mut logical, 1, &budget).unwrap();
    assert_eq!(logical, 4);
    assert_eq!(budget.usage(HostWorkDimension::SearchStateBytes).get(), 32);
    values.extend(0..4);
    reserve_vector(&mut values, &mut logical, 1, &budget).unwrap();
    assert_eq!(logical, 8);
    assert_eq!(budget.usage(HostWorkDimension::SearchStateBytes).get(), 96);
    assert_eq!(budget.usage(HostWorkDimension::VerificationBytes).get(), 32);
    assert_eq!(values, vec![0, 1, 2, 3]);
}

#[test]
fn native_index_growth_work_rejection_does_not_allocate_or_publish() {
    let mut values: Vec<u64> = (0..4).collect();
    let original_capacity = values.capacity();
    let mut logical = 4;
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000));
    limits.set(HostWorkDimension::VerificationBytes, HostWorkLimit::new(31));
    assert!(reserve_vector(&mut values, &mut logical, 1, &HostWorkBudget::new(limits)).is_err());
    assert_eq!(logical, 4);
    assert_eq!(values.capacity(), original_capacity);
    assert_eq!(values, vec![0, 1, 2, 3]);
}

#[test]
fn native_index_rejects_stale_single_and_batch_preparations_before_mutation() {
    let mut index = NativeIndex::default();
    let budget = host();
    let single = index.prepare_insert(1u64, 1, &budget).unwrap();
    let batch = index
        .prepare_batch(vec![(2u64, 2), (3u64, 3)], &budget)
        .unwrap();
    let newer = index.prepare_insert(4u64, 4, &budget).unwrap();
    index.commit(newer);
    let revision = index.revision;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| index.commit(single))).is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| index.commit_batch(batch)))
            .is_err()
    );
    assert_eq!(index.revision, revision);
    verify(&index, &BTreeMap::from([(4u64, 4)]));
}

#[test]
fn native_index_long_prefix_comparisons_reserve_the_complete_key_bound() {
    let mut index = NativeIndex::default();
    let budget = host();
    let key: Arc<[u8]> = vec![3; 8192].into();
    let prepared = index.prepare_insert(Arc::clone(&key), 9, &budget).unwrap();
    index.commit(prepared);
    let mut changed = key.to_vec();
    *changed.last_mut().unwrap() = 4;
    let changed: Arc<[u8]> = changed.into();
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000));
    limits.set(
        HostWorkDimension::VerificationBytes,
        HostWorkLimit::new(8191),
    );
    assert!(index.get(&changed, &HostWorkBudget::new(limits)).is_err());
    limits.set(
        HostWorkDimension::VerificationBytes,
        HostWorkLimit::new(8192),
    );
    let exact = HostWorkBudget::new(limits);
    assert_eq!(index.get(&changed, &exact).unwrap(), None);
    assert_eq!(
        exact.usage(HostWorkDimension::VerificationBytes).get(),
        8192
    );
    assert_eq!(index.get_prepaid(&key), Some(&9));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn native_index_matches_reference_for_arbitrary_batches(
        updates in prop::collection::vec(prop::collection::vec((0u64..64, any::<u64>()), 0..12), 0..24),
    ) {
        let mut index = NativeIndex::default();
        let mut expected = BTreeMap::new();
        let budget = host();
        for batch in updates {
            let stable: Vec<_> = index.nodes.iter().map(|node| node.key).collect();
            let prepared = index.prepare_batch(batch.clone(), &budget).unwrap();
            verify(&index, &expected);
            let work_before = budget.report();
            index.commit_batch(prepared);
            prop_assert_eq!(budget.report(), work_before);
            expected.extend(batch);
            for (position, key) in stable.iter().enumerate() {
                prop_assert_eq!(&index.nodes[position].key, key);
            }
            verify(&index, &expected);
        }
    }

    #[test]
    fn native_index_matches_reference_after_every_generated_update(
        updates in prop::collection::vec((0u64..128, any::<u64>()), 0..256),
    ) {
        let mut index = NativeIndex::default();
        let mut expected = BTreeMap::new();
        let budget = host();
        for (key, value) in updates {
            let prepared = index.prepare_insert(key, value, &budget).unwrap();
            let work_before = budget.report();
            prop_assert_eq!(index.commit(prepared), expected.insert(key, value));
            prop_assert_eq!(budget.report(), work_before);
            verify(&index, &expected);
        }
    }

    #[test]
    fn native_index_occurrence_keys_preserve_long_paths_and_stages(
        prefix in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
        suffixes in prop::collection::vec((any::<u64>(), 0u8..3), 1..32),
    ) {
        let mut index = NativeIndex::default();
        let mut expected = BTreeMap::new();
        let budget = host();
        for (value, stage) in suffixes {
            let mut path = prefix.clone();
            path.push((value, 0));
            let key = NativeBudgetOccurrence {
                session: [7; 32], path,
                stage: [NativeAttemptStage::ProduceIntroduction, NativeAttemptStage::ConsumeIntroduction, NativeAttemptStage::Comm][stage as usize],
            };
            let prepared = index.prepare_insert(key.clone(), value, &budget).unwrap();
            prop_assert_eq!(index.commit(prepared), expected.insert(key, value));
            verify(&index, &expected);
        }
    }
}
