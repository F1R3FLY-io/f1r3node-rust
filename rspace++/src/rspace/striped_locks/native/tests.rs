use futures::FutureExt;
use proptest::prelude::*;

use super::*;

fn stripes(count: usize) -> Vec<Arc<tokio::sync::Mutex<()>>> {
    (0..count)
        .map(|_| Arc::new(tokio::sync::Mutex::new(())))
        .collect()
}

proptest! {
    /// Changed by DR-131 S6: the preparation no longer prepays its backing.
    /// The test keeps its checks of the sorted, deduplicated stripe set, of
    /// the empty guard vector and of the independence from the key order.
    #[test]
    fn preparation_matches_existing_lock_set(
        keys in prop::collection::vec(any::<u64>(), 0..128),
        count in 1usize..64,
    ) {
        let stripes = stripes(count);
        let prepared = prepare(&stripes, &keys).expect("lock preparation");
        let mut expected: Vec<_> = keys.iter().map(|key| (*key as usize) % count).collect();
        expected.sort();
        expected.dedup();
        prop_assert_eq!(&prepared.indices, &expected);
        prop_assert!(prepared.indices.windows(2).all(|pair| pair[0] < pair[1]));
        prop_assert!(prepared.indices.iter().all(|index| *index < count));
        prop_assert!(prepared.held.capacity() >= expected.len());
        prop_assert!(prepared.held.is_empty());
        let reversed: Vec<_> = keys.iter().copied().rev().collect();
        let second = prepare(&stripes, &reversed).expect("lock preparation");
        prop_assert_eq!(prepared.indices, second.indices);
    }
}

#[tokio::test]
async fn prepared_locks_hold_exactly_the_existing_stripes() {
    let stripes = stripes(8);
    let guard = prepare(&stripes, &[9, 17, 3, 1])
        .expect("lock preparation")
        .acquire()
        .await;
    for (index, stripe) in stripes.iter().enumerate() {
        assert_eq!(stripe.try_lock().is_err(), [1, 3].contains(&index));
    }
    drop(guard);
    assert!(stripes.iter().all(|stripe| stripe.try_lock().is_ok()));
}

#[tokio::test]
async fn cancellation_releases_the_acquired_prefix() {
    let stripes = stripes(4);
    let blocking = stripes[2].lock().await;
    let mut acquiring = Box::pin(
        prepare(&stripes, &[2, 1, 1])
            .expect("lock preparation")
            .acquire(),
    );
    assert!(acquiring.as_mut().now_or_never().is_none());
    assert!(stripes[1].try_lock().is_err());
    assert!(stripes[0].try_lock().is_ok());
    drop(acquiring);
    assert!(stripes[1].try_lock().is_ok());
    drop(blocking);
    assert!(stripes.iter().all(|stripe| stripe.try_lock().is_ok()));
}

#[tokio::test]
async fn disjoint_lock_sets_remain_concurrently_available() {
    let stripes = stripes(4);
    let first = prepare(&stripes, &[0, 2])
        .expect("lock preparation")
        .acquire()
        .await;
    let second = prepare(&stripes, &[1, 3])
        .expect("lock preparation")
        .acquire()
        .now_or_never();
    assert!(second.is_some());
    drop((first, second));
    assert!(stripes.iter().all(|stripe| stripe.try_lock().is_ok()));
}

/// Changed by DR-131 S6: the preparation has no reservation. The test keeps
/// its check that an empty stripe table is rejected.
#[test]
fn empty_stripe_table_is_rejected() {
    assert!(prepare(&[], &[0]).is_err());
}
