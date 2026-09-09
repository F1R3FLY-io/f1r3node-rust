use imbl::OrdSet;
use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../block-storage/src/rust/util/ordered_snapshot.rs"]
mod ordered_snapshot;
use ordered_snapshot::OrderedSnapshot;

#[test]
fn captured_root_is_independent_of_concurrent_membership_changes() {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1000;
    builder.check(|| {
        let live = Arc::new(Mutex::new([1u8, 2, 3].into_iter().collect::<OrdSet<u8>>()));
        let observed = live.clone();
        let reader = thread::spawn(move || {
            let (snapshot, expected) = {
                let guard = observed.lock().unwrap();
                (
                    OrderedSnapshot::new(guard.clone()),
                    guard.iter().copied().collect::<Vec<_>>(),
                )
            };
            assert_eq!(snapshot.original_len(), expected.len());
            assert_eq!(snapshot.shared_values().len(), expected.len());
            thread::yield_now();
            assert_eq!(snapshot.collect::<Vec<_>>(), expected);
        });
        let mutated = live.clone();
        let writer = thread::spawn(move || {
            mutated.lock().unwrap().remove(&1);
            thread::yield_now();
            mutated.lock().unwrap().insert(4);
            thread::yield_now();
            mutated.lock().unwrap().insert(1);
        });
        reader.join().unwrap();
        writer.join().unwrap();
    });
}
