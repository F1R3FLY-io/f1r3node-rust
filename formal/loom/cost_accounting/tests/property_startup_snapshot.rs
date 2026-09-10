use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use imbl::OrdSet;
use proptest::prelude::*;

#[path = "../../../../block-storage/src/rust/util/ordered_snapshot.rs"]
mod ordered_snapshot;
use ordered_snapshot::OrderedSnapshot;

proptest! {
    #[test]
    fn immutable_cursor_matches_eager_reference_during_live_mutation(
        initial in proptest::collection::vec(any::<u16>(), 0..512),
        operations in proptest::collection::vec((any::<bool>(), any::<u16>(), any::<bool>()), 0..1024),
    ) {
        let expected: BTreeSet<_> = initial.iter().copied().collect();
        let mut live: OrdSet<_> = initial.into_iter().collect();
        let mut snapshot = OrderedSnapshot::new(live.clone());
        prop_assert_eq!(snapshot.original_len(), expected.len());
        prop_assert_eq!(snapshot.shared_values().len(), expected.len());
        let mut visited = Vec::new();
        for (insert, key, visit) in operations {
            if insert { live.insert(key); } else { live.remove(&key); }
            if visit {
                if let Some(key) = snapshot.next() { visited.push(key); }
            }
        }
        visited.extend(snapshot.by_ref());
        prop_assert_eq!(visited, expected.into_iter().collect::<Vec<_>>());
        prop_assert_eq!(snapshot.next(), None);
        prop_assert_eq!(snapshot.next(), None);
    }

    #[test]
    fn bounded_pages_preserve_all_hashes_and_extreme_keys(
        initial in proptest::collection::vec(any::<u64>(), 0..512),
        page in 1usize..65,
    ) {
        let mut values: OrdSet<_> = initial.into_iter().collect();
        values.insert(0);
        values.insert(u64::MAX);
        let expected = values.iter().copied().collect::<Vec<_>>();
        let mut cursor = OrderedSnapshot::new(values);
        let mut visited = Vec::new();
        loop {
            let mut count = 0;
            for _ in 0..page {
                match cursor.next() {
                    Some(key) => { visited.push(key); count += 1; }
                    None => break,
                }
            }
            prop_assert!(count <= page);
            if count == 0 { break; }
        }
        prop_assert_eq!(visited, expected);
    }
}

#[derive(Debug)]
struct CountedKey {
    value: usize,
    clones: Arc<AtomicUsize>,
}

impl Clone for CountedKey {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::SeqCst);
        Self {
            value: self.value,
            clones: self.clones.clone(),
        }
    }
}

impl PartialEq for CountedKey {
    fn eq(&self, other: &Self) -> bool { self.value == other.value }
}
impl Eq for CountedKey {}
impl PartialOrd for CountedKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for CountedKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.value.cmp(&other.value) }
}

#[test]
fn snapshot_capture_and_cursor_do_not_copy_the_full_set() {
    let clones = Arc::new(AtomicUsize::new(0));
    let values: OrdSet<CountedKey> = (0..65_536)
        .map(|value| CountedKey {
            value,
            clones: clones.clone(),
        })
        .collect();
    clones.store(0, Ordering::SeqCst);
    let mut snapshot = OrderedSnapshot::new(values.clone());
    assert_eq!(clones.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot.original_len(), 65_536);
    for expected in 0..1024 {
        clones.store(0, Ordering::SeqCst);
        assert_eq!(snapshot.next().unwrap().value, expected);
        assert_eq!(clones.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn live_removal_does_not_erase_snapshot_membership() {
    let mut live: OrdSet<u32> = [1u32, 2, 3].into_iter().collect();
    let snapshot = OrderedSnapshot::new(live.clone());
    live.remove(&1);
    live.remove(&2);
    live.insert(2);
    live.insert(4);
    assert_eq!(snapshot.collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(live.iter().copied().collect::<Vec<_>>(), vec![2, 3, 4]);
}
