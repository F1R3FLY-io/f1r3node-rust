use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct DagConsensusView {
    floor: usize,
    latest: usize,
}

#[test]
fn floor_and_latest_are_captured_from_one_atomic_dag_view() {
    loom::model(|| {
        let dag = Arc::new(Mutex::new(DagConsensusView {
            floor: 0,
            latest: 1,
        }));
        let writer_dag = Arc::clone(&dag);
        let reader_dag = Arc::clone(&dag);

        let writer = thread::spawn(move || {
            let mut view = writer_dag.lock().unwrap();
            view.floor = 2;
            view.latest = 3;
        });
        let reader = thread::spawn(move || {
            let captured = *reader_dag.lock().unwrap();
            assert!(
                (captured.floor == 0 && captured.latest == 1)
                    || (captured.floor == 2 && captured.latest == 3)
            );
        });

        writer.join().unwrap();
        reader.join().unwrap();
    });
}

#[test]
fn captured_floor_is_an_evidence_root_even_when_latest_is_stale() {
    loom::model(|| {
        let dag = Arc::new(Mutex::new(DagConsensusView {
            floor: 2,
            latest: 1,
        }));
        let writer_dag = Arc::clone(&dag);
        let reader_dag = Arc::clone(&dag);

        let writer = thread::spawn(move || {
            let mut view = writer_dag.lock().unwrap();
            view.latest = 3;
        });
        let reader = thread::spawn(move || {
            let captured = *reader_dag.lock().unwrap();
            let evidence_roots = [captured.floor, captured.latest];
            assert!(evidence_roots.contains(&captured.floor));
            if captured.latest == 1 {
                assert_eq!(evidence_roots, [2, 1]);
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();
    });
}
