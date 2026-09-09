use loom::sync::{Arc, RwLock};
use loom::thread;

#[path = "../../../../block-storage/src/rust/dag/admitted_metadata.rs"]
mod admitted_metadata;

fn pending_publication_and_admission(keep_guard: bool, lose_metadata: bool) {
    let mut model = loom::model::Builder::new();
    model.max_threads = 4;
    model.max_branches = 1000;
    model.max_permutations = None;
    model.max_duration = None;
    model.preemption_bound = None;
    model.checkpoint_file = None;
    model.check(move || {
        let dag = Arc::new(RwLock::new((false, false)));
        let pending = Arc::new(RwLock::new(false));
        let publications: Vec<_> = (0..2)
            .map(|_| {
                let dag = dag.clone();
                let pending = pending.clone();
                thread::spawn(move || {
                    let guard = dag.read().unwrap();
                    let admitted = match *guard {
                        (false, _) => Ok(false),
                        (true, true) => Ok(true),
                        (true, false) => Err(()),
                    };
                    let publish = || {
                        thread::yield_now();
                        *pending.write().unwrap() = true;
                        Ok::<_, ()>(())
                    };
                    let result = if keep_guard {
                        admitted_metadata::publish_if_unadmitted(guard, || admitted, publish)
                    } else {
                        drop(guard);
                        admitted_metadata::publish_if_unadmitted((), || admitted, publish)
                    };
                    assert_eq!(
                        result,
                        admitted.map(|terminal| if terminal { None } else { Some(()) })
                    );
                })
            })
            .collect();
        let admitting = {
            let dag = dag.clone();
            let pending = pending.clone();
            thread::spawn(move || {
                {
                    let mut terminal = dag.write().unwrap();
                    *terminal = (true, true);
                    *pending.write().unwrap() = false;
                }
                if lose_metadata {
                    dag.write().unwrap().1 = false;
                }
            })
        };
        for publication in publications {
            publication.join().unwrap();
        }
        admitting.join().unwrap();
        assert!(dag.read().unwrap().0);
        assert!(
            !*pending.read().unwrap(),
            "terminal admission acquired pending ownership"
        );
    });
}

#[test]
fn pending_publication_keeps_the_dag_read_guard_until_commit() {
    pending_publication_and_admission(true, false);
}

#[test]
fn metadata_loss_does_not_reopen_terminal_publication() {
    pending_publication_and_admission(true, true);
}

#[test]
#[should_panic(expected = "terminal admission acquired pending ownership")]
fn released_dag_guard_exposes_terminal_republication() {
    pending_publication_and_admission(false, false);
}

#[test]
fn publication_and_row_replacement_keep_read_witnesses_exact() {
    let mut model = loom::model::Builder::new();
    model.max_threads = 3;
    model.max_branches = 1000;
    model.check(|| {
        let state = Arc::new(RwLock::new((false, Ok::<bool, ()>(false))));
        let publishing = state.clone();
        let writer = thread::spawn(move || {
            publishing.write().unwrap().1 = Ok(true);
            {
                let mut state = publishing.write().unwrap();
                state.1 = Ok(true);
                state.0 = true;
            }
            publishing.write().unwrap().1 = Err(());
        });
        let reading = state.clone();
        let reader = thread::spawn(move || {
            for _ in 0..2 {
                let state = reading.read().unwrap();
                let actual = admitted_metadata::metadata_present(state.0, || state.1);
                let expected = if state.0 { state.1 } else { Ok(false) };
                assert_eq!(actual, expected);
            }
        });
        writer.join().unwrap();
        reader.join().unwrap();
        let state = state.read().unwrap();
        assert_eq!(
            admitted_metadata::metadata_present(state.0, || state.1),
            Err(())
        );
    });
}
