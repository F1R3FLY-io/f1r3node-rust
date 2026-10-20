use loom::sync::{Arc, Mutex, RwLock};
use loom::thread;

// Added by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the child-process seam of the negative controls below.
#[path = "../../../../shared/tests/support/panic_child.rs"]
mod panic_child;

use panic_child::PanicChild;

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
// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the test expected a panic. A child process now runs the subject, and
// this process checks the exit status and the stderr of the child.
// #[should_panic(expected = "operation callbacks crossed observer ownership")]
fn reloading_observer_during_an_operation_violates_ownership() {
    let child = PanicChild::new(
        module_path!(),
        "reloading_observer_during_an_operation_violates_ownership",
    );
    if !child.is_child() {
        child.expect_panic("operation callbacks crossed observer ownership");
        return;
    }
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    child.subject_reached();
    model.check(|| verify_capture(false));
}
