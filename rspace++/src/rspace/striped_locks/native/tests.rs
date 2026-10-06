use std::cell::Cell;

use futures::FutureExt;
use proptest::prelude::*;

use super::*;

fn stripes(count: usize) -> Vec<Arc<tokio::sync::Mutex<()>>> {
    (0..count)
        .map(|_| Arc::new(tokio::sync::Mutex::new(())))
        .collect()
}

proptest! {
    #[test]
    fn preparation_matches_existing_lock_set_and_prepays_backing(
        keys in prop::collection::vec(any::<u64>(), 0..128),
        count in 1usize..64,
    ) {
        let stripes = stripes(count);
        let bytes = Cell::new(0);
        let prepared = prepare(&stripes, &keys, |_, backing| {
            bytes.set(bytes.get() + backing);
            Ok(())
        }).unwrap();
        let mut expected: Vec<_> = keys.iter().map(|key| (*key as usize) % count).collect();
        expected.sort();
        expected.dedup();
        prop_assert_eq!(&prepared.indices, &expected);
        prop_assert!(prepared.indices.windows(2).all(|pair| pair[0] < pair[1]));
        prop_assert!(prepared.indices.iter().all(|index| *index < count));
        prop_assert_eq!(bytes.get(), keys.len() * size_of::<usize>() + expected.len() * size_of::<HeldLock>());
        prop_assert!(prepared.held.capacity() >= expected.len());
        prop_assert!(prepared.held.is_empty());
        let reversed: Vec<_> = keys.iter().copied().rev().collect();
        let second = prepare(&stripes, &reversed, |_, _| Ok(())).unwrap();
        prop_assert_eq!(prepared.indices, second.indices);
    }

    #[test]
    fn every_reservation_cut_prevents_lock_acquisition(
        keys in prop::collection::vec(any::<u64>(), 0..64),
        cut in 0usize..1024,
    ) {
        let stripes = stripes(16);
        let total = Cell::new(0);
        prepare(&stripes, &keys, |_, _| {
            total.set(total.get() + 1);
            Ok(())
        }).unwrap();
        let accepted = cut % total.get();
        let calls = Cell::new(0);
        let result = prepare(&stripes, &keys, |_, _| {
            let call = calls.get();
            calls.set(call + 1);
            if call == accepted { Err(RSpaceError::HostWorkRejected) } else { Ok(()) }
        });
        prop_assert!(matches!(result, Err(RSpaceError::HostWorkRejected)));
        prop_assert_eq!(calls.get(), accepted + 1);
        prop_assert!(stripes.iter().all(|stripe| stripe.try_lock().is_ok()));
    }
}

#[tokio::test]
async fn prepared_locks_hold_exactly_the_existing_stripes() {
    let stripes = stripes(8);
    let guard = prepare(&stripes, &[9, 17, 3, 1], |_, _| Ok(()))
        .unwrap()
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
        prepare(&stripes, &[2, 1, 1], |_, _| Ok(()))
            .unwrap()
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
    let first = prepare(&stripes, &[0, 2], |_, _| Ok(()))
        .unwrap()
        .acquire()
        .await;
    let second = prepare(&stripes, &[1, 3], |_, _| Ok(()))
        .unwrap()
        .acquire()
        .now_or_never();
    assert!(second.is_some());
    drop((first, second));
    assert!(stripes.iter().all(|stripe| stripe.try_lock().is_ok()));
}

#[test]
fn empty_stripe_table_rejects_before_reservation() {
    assert!(prepare(&[], &[0], |_, _| panic!("invalid stripes must not reserve")).is_err());
}

#[test]
fn vector_size_overflow_rejects_before_reservation() {
    assert!(matches!(
        reserve_vector::<u64>(usize::MAX / size_of::<u64>() + 1, &|_, _| panic!(
            "overflow must not reserve"
        )),
        Err(RSpaceError::HostWorkRejected)
    ));
    assert!(matches!(
        reserve_vector::<()>(usize::MAX, &|_, _| panic!("overflow must not reserve")),
        Err(RSpaceError::HostWorkRejected)
    ));
}

fn concurrent_preparation(atomic: bool) {
    use loom::sync::Arc as LoomArc;
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::thread;
    loom::model(move || {
        let required = size_of::<usize>() + size_of::<HeldLock>();
        let remaining = LoomArc::new(AtomicUsize::new(required));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let remaining = remaining.clone();
                thread::spawn(move || {
                    let stripes = stripes(1);
                    prepare(&stripes, &[0], |_, bytes| {
                        if bytes == 0 {
                            return Ok(());
                        }
                        if atomic {
                            remaining
                                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                                    value.checked_sub(bytes)
                                })
                                .map(|_| ())
                                .map_err(|_| RSpaceError::HostWorkRejected)
                        } else {
                            let next = remaining
                                .load(Ordering::Acquire)
                                .checked_sub(bytes)
                                .ok_or(RSpaceError::HostWorkRejected)?;
                            thread::yield_now();
                            remaining.store(next, Ordering::Release);
                            Ok(())
                        }
                    })
                    .is_ok() as usize
                })
            })
            .collect();
        let completed: usize = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .sum();
        assert!(completed * required <= required, "lock preparation exceeds shared backing budget");
    });
}

#[test]
fn loom_lock_preparation_respects_atomic_shared_budget() { concurrent_preparation(true); }

#[test]
#[should_panic(expected = "lock preparation exceeds shared backing budget")]
fn loom_separate_budget_check_and_update_allows_unpaid_lock_preparation() {
    concurrent_preparation(false);
}
