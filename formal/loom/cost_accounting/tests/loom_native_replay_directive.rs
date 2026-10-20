use loom::sync::{Arc, Mutex};
use loom::thread;

// Added by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the child-process seam of the negative controls below.
#[path = "../../../../shared/tests/support/panic_child.rs"]
mod panic_child;

use panic_child::PanicChild;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Store {
    tuples: Vec<u8>,
    counters: usize,
    bindings: usize,
}

fn select(store: &Store, wanted: &[u8]) -> Vec<usize> {
    wanted
        .iter()
        .map(|value| {
            store
                .tuples
                .iter()
                .position(|datum| datum == value)
                .unwrap()
        })
        .collect()
}

fn publish(
    store: &mut Store,
    mut indices: Vec<usize>,
    wanted: &[u8],
    denied: bool,
    ascending: bool,
) {
    let before = store.clone();
    if !denied {
        indices.sort_unstable();
        if !ascending {
            indices.reverse();
        }
        let mut removed = Vec::new();
        for index in indices {
            assert!(index < store.tuples.len(), "stale candidate index");
            removed.push(store.tuples.remove(index));
        }
        removed.sort_unstable();
        assert_eq!(removed, wanted, "retired different tuples");
        store.counters += 1;
        store.bindings -= 1;
    } else {
        assert_eq!(*store, before);
    }
}

fn check(shared_guard: bool, ascending: bool, denied: bool) {
    let store = Arc::new(Mutex::new(Store {
        tuples: vec![1, 2, 3, 4],
        counters: 0,
        bindings: 2,
    }));
    let workers: Vec<_> = [vec![1, 2], vec![3, 4]]
        .into_iter()
        .enumerate()
        .map(|(index, wanted)| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                if shared_guard {
                    let mut state = store.lock().unwrap();
                    let candidates = select(&state, &wanted);
                    publish(
                        &mut state,
                        candidates,
                        &wanted,
                        denied && index == 0,
                        ascending,
                    );
                } else {
                    let candidates = select(&store.lock().unwrap(), &wanted);
                    thread::yield_now();
                    publish(
                        &mut store.lock().unwrap(),
                        candidates,
                        &wanted,
                        denied && index == 0,
                        ascending,
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let state = store.lock().unwrap();
    assert_eq!(state.tuples, if denied { vec![1, 2] } else { vec![] });
    assert_eq!(state.counters, if denied { 1 } else { 2 });
    assert_eq!(state.bindings, usize::from(denied));
}

#[test]
fn selected_indices_remain_valid_until_publication() {
    for denied in [false, true] {
        loom::model(move || check(true, false, denied));
    }
}

#[test]
// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the test expected a panic. A child process now runs the subject, and
// this process checks the exit status and the stderr of the child.
// #[should_panic(expected = "retired different tuples")]
fn ascending_retirement_changes_the_selected_tuple_set() {
    let child = PanicChild::new(
        module_path!(),
        "ascending_retirement_changes_the_selected_tuple_set",
    );
    if !child.is_child() {
        child.expect_panic("retired different tuples");
        return;
    }
    child.subject_reached();
    loom::model(|| check(true, true, false));
}

#[test]
// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the test expected a panic. A child process now runs the subject, and
// this process checks the exit status and the stderr of the child.
// #[should_panic(expected = "stale candidate index")]
fn releasing_the_guard_before_publication_invalidates_selection() {
    let child = PanicChild::new(
        module_path!(),
        "releasing_the_guard_before_publication_invalidates_selection",
    );
    if !child.is_child() {
        child.expect_panic("stale candidate index");
        return;
    }
    child.subject_reached();
    loom::model(|| check(false, false, false));
}
