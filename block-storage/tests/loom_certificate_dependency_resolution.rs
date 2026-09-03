use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Debug)]
struct State {
    requested: bool,
    certificate: Option<u64>,
    dependency: bool,
    pendant: bool,
    queued: bool,
    enqueue_count: u8,
}

fn handle_response(state: &Arc<Mutex<State>>, scan: &Arc<Mutex<()>>, digest: u64) {
    if !state.lock().unwrap().requested {
        return;
    }
    {
        let mut state = state.lock().unwrap();
        match state.certificate {
            Some(existing) => assert_eq!(existing, digest),
            None => state.certificate = Some(digest),
        }
    }
    {
        let mut state = state.lock().unwrap();
        if state.dependency {
            state.dependency = false;
            state.pendant = true;
        }
    }
    state.lock().unwrap().requested = false;
    let _scan = scan.lock().unwrap();
    let mut state = state.lock().unwrap();
    if state.pendant && !state.queued {
        state.pendant = false;
        state.queued = true;
        state.enqueue_count = state.enqueue_count.saturating_add(1);
    }
}

#[test]
fn duplicate_responses_resolve_once_and_enqueue_once_under_every_interleaving() {
    loom::model(|| {
        let digest = 7;
        let state = Arc::new(Mutex::new(State {
            requested: true,
            certificate: None,
            dependency: true,
            pendant: false,
            queued: false,
            enqueue_count: 0,
        }));
        let scan = Arc::new(Mutex::new(()));
        let first = {
            let state = state.clone();
            let scan = scan.clone();
            thread::spawn(move || handle_response(&state, &scan, digest))
        };
        let second = {
            let state = state.clone();
            let scan = scan.clone();
            thread::spawn(move || handle_response(&state, &scan, digest))
        };
        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.certificate, Some(digest));
        assert!(!state.dependency);
        assert!(!state.requested);
        assert!(state.queued);
        assert_eq!(state.enqueue_count, 1);
    });
}

#[test]
fn unsolicited_response_cannot_mutate_storage_buffer_or_queue() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(State {
            requested: false,
            certificate: None,
            dependency: false,
            pendant: false,
            queued: false,
            enqueue_count: 0,
        }));
        let scan = Arc::new(Mutex::new(()));
        let worker = {
            let state = state.clone();
            let scan = scan.clone();
            thread::spawn(move || handle_response(&state, &scan, 7))
        };
        worker.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.certificate, None);
        assert!(!state.dependency);
        assert!(!state.pendant);
        assert!(!state.queued);
        assert_eq!(state.enqueue_count, 0);
    });
}
