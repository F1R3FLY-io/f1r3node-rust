use loom::sync::{Arc, Mutex, RwLock};
use loom::thread;

#[derive(Default)]
struct Observer {
    events: Mutex<Vec<(usize, usize)>>,
}

fn verify_capture(capture: bool) {
    let original = Arc::new(Observer::default());
    let replacement = Arc::new(Observer::default());
    let active = Arc::new(RwLock::new(original.clone()));
    let worker_active = active.clone();
    let worker = thread::spawn(move || {
        let observer = worker_active.read().unwrap().clone();
        observer.events.lock().unwrap().push((0, 0));
        for stage in [1, 2] {
            let current = if capture {
                observer.clone()
            } else {
                worker_active.read().unwrap().clone()
            };
            current.events.lock().unwrap().push((0, stage));
        }
        observer.events.lock().unwrap().push((0, 3));
    });
    let second_active = active.clone();
    let second = thread::spawn(move || {
        let observer = second_active.read().unwrap().clone();
        for stage in 0..4 {
            observer.events.lock().unwrap().push((1, stage));
        }
    });
    *active.write().unwrap() = replacement.clone();
    worker.join().unwrap();
    second.join().unwrap();
    for operation in 0..2 {
        let original_events: Vec<_> = original
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| *id == operation)
            .map(|(_, stage)| *stage)
            .collect();
        let replacement_events: Vec<_> = replacement
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| *id == operation)
            .map(|(_, stage)| *stage)
            .collect();
        assert!(
            (original_events == vec![0, 1, 2, 3] && replacement_events.is_empty())
                || (replacement_events == vec![0, 1, 2, 3] && original_events.is_empty()),
            "operation callbacks crossed observer ownership"
        );
    }
}

#[test]
fn concurrent_operations_retain_their_captured_observers() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    model.check(|| verify_capture(true));
}

#[test]
#[should_panic(expected = "operation callbacks crossed observer ownership")]
fn reloading_observer_during_an_operation_violates_ownership() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    model.check(|| verify_capture(false));
}
