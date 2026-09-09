use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../casper/src/rust/engine/block_retriever/dependency_provenance.rs"]
mod dependency_provenance;

#[test]
fn concurrent_announcement_and_dependency_preserve_exact_provenance() {
    loom::model(|| {
        let state = Arc::new(Mutex::new((false, 7u32, u64::MAX)));
        let workers = [false, true].map(|dependency| {
            let state = state.clone();
            thread::spawn(move || {
                let mut request = state.lock().unwrap();
                request.0 =
                    dependency_provenance::merge_dependency_provenance(request.0, dependency);
            })
        });
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(*state.lock().unwrap(), (true, 7, u64::MAX));
    });
}

#[test]
#[should_panic(expected = "stale announcement erased dependency provenance")]
fn unlocked_snapshot_is_an_unsafe_control() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(false));
        let announcement_state = state.clone();
        let announcement = thread::spawn(move || {
            let observed = *announcement_state.lock().unwrap();
            thread::yield_now();
            *announcement_state.lock().unwrap() =
                dependency_provenance::merge_dependency_provenance(observed, false);
        });
        let dependency_state = state.clone();
        let dependency = thread::spawn(move || {
            let mut request = dependency_state.lock().unwrap();
            *request = dependency_provenance::merge_dependency_provenance(*request, true);
        });
        announcement.join().unwrap();
        dependency.join().unwrap();
        assert!(
            *state.lock().unwrap(),
            "stale announcement erased dependency provenance"
        );
    });
}
