use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct PendingWork {
    fresh_ready: bool,
    retry_ready: bool,
    terminal: bool,
}

impl PendingWork {
    fn ready(self) -> bool { !self.terminal && (self.fresh_ready || self.retry_ready) }
}

#[test]
fn fresh_to_retry_custody_transfer_cannot_create_false_idle() {
    loom::model(|| {
        let work = Arc::new(Mutex::new(PendingWork {
            fresh_ready: true,
            retry_ready: false,
            terminal: false,
        }));
        let transfer = {
            let work = work.clone();
            thread::spawn(move || {
                let mut work = work.lock().unwrap();
                work.retry_ready = true;
                work.fresh_ready = false;
            })
        };
        let heartbeat = {
            let work = work.clone();
            thread::spawn(move || work.lock().unwrap().ready())
        };

        transfer.join().unwrap();
        assert!(heartbeat.join().unwrap());
        assert!(work.lock().unwrap().ready());
    });
}

#[test]
fn terminalization_dominates_both_pending_pools() {
    loom::model(|| {
        let work = Arc::new(Mutex::new(PendingWork {
            fresh_ready: true,
            retry_ready: true,
            terminal: false,
        }));
        let terminalize = {
            let work = work.clone();
            thread::spawn(move || work.lock().unwrap().terminal = true)
        };
        terminalize.join().unwrap();
        assert!(!work.lock().unwrap().ready());
    });
}
