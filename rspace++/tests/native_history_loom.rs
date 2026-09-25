use loom::sync::Arc;
use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::thread;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::native_reader::{
    NativeHistoryReader, NativeLeafKind, NativeReadCharge, NativeReadError, NativeReadMeter,
};
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct Meter {
    remaining: AtomicUsize,
    atomic: bool,
}

impl NativeReadMeter for Meter {
    type Error = ();

    fn reserve(&self, _: NativeReadCharge) -> Result<(), ()> {
        if self.atomic {
            self.remaining
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
                .map(|_| ())
                .map_err(|_| ())
        } else {
            let remaining = self.remaining.load(Ordering::Acquire);
            let next = remaining.checked_sub(1).ok_or(())?;
            thread::yield_now();
            self.remaining.store(next, Ordering::Release);
            Ok(())
        }
    }
}

fn fixture() -> (RSpaceStore, [u8; 32]) {
    let stores = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(InMemoryStoreManager::new().r_space_stores())
        .unwrap();
    let payload = [1_u64.to_le_bytes().as_slice(), 1_u64.to_le_bytes().as_slice(), &[42]].concat();
    let leaf = [17_u64.to_le_bytes().as_slice(), &payload].concat();
    let leaf_hash = Blake2b256Hash::new(&leaf);
    stores
        .cold
        .put_one(leaf_hash.0.clone(), [1_u32.to_le_bytes().as_slice(), &leaf].concat())
        .unwrap();
    let node = [&[0, 32][..], &[7; 32], &leaf_hash.0].concat();
    let root = Blake2b256Hash::new(&node);
    stores.history.put_one(root.0.clone(), node).unwrap();
    (stores, root.0.try_into().unwrap())
}

fn check(stores: &RSpaceStore, root: [u8; 32], limit: usize, atomic: bool) {
    let meter = Arc::new(Meter {
        remaining: AtomicUsize::new(limit),
        atomic,
    });
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let stores = stores.clone();
            let meter = Arc::clone(&meter);
            thread::Builder::new()
                .stack_size(1024 * 1024)
                .spawn(move || {
                    let reader = NativeHistoryReader::new(
                        root,
                        stores.history.as_ref(),
                        stores.cold.as_ref(),
                    );
                    let result = reader.with_records(
                        NativeLeafKind::Data,
                        &[7; 32],
                        meter.as_ref(),
                        |rows| {
                            assert_eq!(rows.len(), 1);
                            assert_eq!(rows.iter().next(), Some(&[42][..]));
                            Ok(())
                        },
                    );
                    match result {
                        Ok(Some(())) => 1,
                        Err(NativeReadError::Host(())) => 0,
                        other => panic!("unexpected reader result: {other:?}"),
                    }
                })
                .unwrap()
        })
        .collect();
    let completed: usize = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .sum();
    assert!(completed * 5 <= limit, "consumer ran without all five reservations");
    if limit == 10 {
        assert_eq!(completed, 2);
    }
    assert!(meter.remaining.load(Ordering::Acquire) <= limit);
}

fn model(check: impl Fn() + Send + Sync + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.preemption_bound = Some(builder.preemption_bound.unwrap_or(3));
    builder.check(check);
}

#[test]
fn concurrent_production_reads_require_every_reservation() {
    let (stores, root) = fixture();
    for limit in 0..=10 {
        let stores = stores.clone();
        model(move || check(&stores, root, limit, true));
    }
}

#[test]
#[should_panic(expected = "consumer ran without all five reservations")]
fn non_atomic_meter_can_authorize_unfunded_reads() {
    let (stores, root) = fixture();
    model(move || check(&stores, root, 5, false));
}
