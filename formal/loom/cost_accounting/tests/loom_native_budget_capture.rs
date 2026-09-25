use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

fn check_capture(pending_first: bool, abandoned: bool) {
    let pending = Arc::new(AtomicUsize::new(1));
    let invalid = Arc::new(AtomicBool::new(false));
    let writer_pending = Arc::clone(&pending);
    let writer_invalid = Arc::clone(&invalid);
    let writer = thread::spawn(move || {
        if abandoned {
            writer_invalid.store(true, Ordering::Release);
        }
        writer_pending.fetch_sub(1, Ordering::AcqRel);
    });
    let complete = if pending_first {
        let pending_count = pending.load(Ordering::Acquire);
        pending_count == 0 && !invalid.load(Ordering::Acquire)
    } else {
        let invalidated = invalid.load(Ordering::Acquire);
        !invalidated && pending.load(Ordering::Acquire) == 0
    };
    assert!(
        !abandoned || !complete,
        "abandoned preparation exported evidence"
    );
    writer.join().unwrap();
    assert_eq!(pending.load(Ordering::Acquire), 0);
    assert_eq!(invalid.load(Ordering::Acquire), abandoned);
}

#[test]
fn pending_acquire_precedes_invalidation_read() {
    for abandoned in [true, false] {
        loom::model(move || check_capture(true, abandoned));
    }
}

#[test]
#[should_panic(expected = "abandoned preparation exported evidence")]
fn reading_invalidation_first_can_export_an_abandoned_preparation() {
    loom::model(|| check_capture(false, true));
}

fn check_recording_failure(invalidate_before_unlock: bool) {
    let state = Arc::new(Mutex::new((false, 0usize, 0usize)));
    let invalid = Arc::new(AtomicBool::new(false));
    let failed_state = Arc::clone(&state);
    let failed_invalid = Arc::clone(&invalid);
    let failure = thread::spawn(move || {
        {
            let mut state = failed_state.lock().unwrap();
            state.0 = true;
            state.2 = state.1;
            if invalidate_before_unlock {
                failed_invalid.store(true, Ordering::Release);
            }
        }
        thread::yield_now();
        failed_invalid.store(true, Ordering::Release);
    });
    let publication = thread::spawn(move || {
        let mut state = state.lock().unwrap();
        if !invalid.load(Ordering::Acquire) {
            state.1 += 1;
        }
        assert!(
            !state.0 || state.1 == state.2,
            "preparation published after recording failure"
        );
    });
    failure.join().unwrap();
    publication.join().unwrap();
}

#[test]
fn recording_failure_invalidates_before_authority_unlock() {
    loom::model(|| check_recording_failure(true));
}

#[test]
#[should_panic(expected = "preparation published after recording failure")]
fn deferred_ticket_drop_permits_publication_after_recording_failure() {
    loom::model(|| check_recording_failure(false));
}
