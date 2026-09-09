use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_completion.rs"]
mod startup_completion;
use startup_completion::{StartupCompletion, StartupKey, StartupPhase as Phase};

type Kernel = StartupCompletion<u8, u8>;
type Key = StartupKey<u8, u8>;

fn key(context: u8, request: u8) -> Key { Key { context, request } }

fn ready() -> (Kernel, Key) {
    let mut control = Kernel::default();
    let id = key(1, 1);
    assert!(control.publish_context(1));
    assert!(control.install(id.clone(), true));
    assert_eq!(control.activate(), Some(id.clone()));
    assert!(control.presence_complete(&id));
    assert!(control.scan_complete(&id));
    assert!(control.retire_active(&id));
    (control, id)
}

fn explore(test: impl Fn() + Send + Sync + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

#[test]
fn stop_races_with_authorization_and_success_at_the_same_lock_boundary() {
    explore(|| {
        let (kernel, id) = ready();
        let control = Arc::new(Mutex::new(kernel));
        let finishing = control.clone();
        let callback = thread::spawn(move || {
            let authorized = {
                let mut guard = finishing.lock().unwrap();
                let expected = !guard.is_stopped();
                let result = guard.authorize_callback(&id);
                assert_eq!(result, expected);
                result
            };
            thread::yield_now();
            let observed = {
                let mut guard = finishing.lock().unwrap();
                let result = guard.callback_succeeded(&id);
                assert_eq!(result, authorized && !guard.is_stopped());
                result
            };
            thread::yield_now();
            let mut guard = finishing.lock().unwrap();
            let expected = observed && !guard.is_stopped();
            let success = guard.commit_success(&id);
            assert_eq!(success, expected);
            success
        });
        let stopping = control.clone();
        let stopper = thread::spawn(move || assert!(stopping.lock().unwrap().stop()));
        let success = callback.join().unwrap();
        stopper.join().unwrap();
        let guard = control.lock().unwrap();
        assert!(guard.is_stopped());
        assert!(guard.context().is_none());
        assert_eq!(
            guard.current().unwrap().phase,
            if success {
                Phase::Succeeded
            } else {
                Phase::Cancelled
            }
        );
    });
}

#[test]
fn replacement_can_follow_authorization_but_cannot_receive_old_success() {
    explore(|| {
        let (kernel, old) = ready();
        let control = Arc::new(Mutex::new(kernel));
        let finishing = control.clone();
        let callback = thread::spawn(move || {
            let authorized = finishing.lock().unwrap().authorize_callback(&old);
            thread::yield_now();
            let observed = {
                let mut guard = finishing.lock().unwrap();
                let result = guard.callback_succeeded(&old);
                assert_eq!(result, authorized && guard.context() == Some(&1));
                result
            };
            thread::yield_now();
            let mut guard = finishing.lock().unwrap();
            let same_context = guard.context() == Some(&1);
            let success = guard.commit_success(&old);
            assert_eq!(success, observed && same_context);
            success
        });
        let replacing = control.clone();
        let replacement = thread::spawn(move || {
            let mut guard = replacing.lock().unwrap();
            assert!(guard.publish_context(2));
            assert!(guard.install(key(2, 2), false));
        });
        callback.join().unwrap();
        replacement.join().unwrap();
        let guard = control.lock().unwrap();
        assert_eq!(guard.current().unwrap().key, key(2, 2));
        assert_eq!(guard.current().unwrap().phase, Phase::Pending);
    });
}

#[test]
fn old_retirement_and_cancellation_cannot_clear_new_active_work() {
    explore(|| {
        let mut kernel = Kernel::default();
        kernel.publish_context(1);
        kernel.install(key(1, 1), true);
        kernel.activate().unwrap();
        kernel.install(key(1, 2), false);
        let control = Arc::new(Mutex::new(kernel));
        let retiring = control.clone();
        let old = thread::spawn(move || {
            assert!(retiring.lock().unwrap().retire_active(&key(1, 1)));
            thread::yield_now();
            let mut guard = retiring.lock().unwrap();
            assert!(!guard.cancel(&key(1, 1)));
            assert!(!guard.retire_active(&key(1, 1)));
        });
        let activating = control.clone();
        let new = thread::spawn(move || activating.lock().unwrap().activate());
        old.join().unwrap();
        let activated = new.join().unwrap();
        let mut guard = control.lock().unwrap();
        if activated.is_none() {
            assert_eq!(guard.activate(), Some(key(1, 2)));
        }
        assert_eq!(guard.active(), Some(&key(1, 2)));
        assert_eq!(guard.current().unwrap().phase, Phase::Presence);
    });
}

#[test]
fn two_callback_attempts_consume_only_one_permission() {
    explore(|| {
        let (kernel, id) = ready();
        let control = Arc::new(Mutex::new(kernel));
        let attempts = [0, 1].map(|_| {
            let control = control.clone();
            let id = id.clone();
            thread::spawn(move || control.lock().unwrap().authorize_callback(&id))
        });
        let permissions = attempts
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>();
        assert_eq!(permissions, 1);
        let mut guard = control.lock().unwrap();
        assert!(guard.callback_succeeded(&id));
        assert!(guard.commit_success(&id));
        assert!(!guard.commit_success(&id));
    });
}

#[test]
fn revoked_context_cannot_finish_scan_or_overwrite_failure() {
    explore(|| {
        let mut kernel = Kernel::default();
        kernel.publish_context(1);
        kernel.install(key(1, 1), true);
        kernel.activate().unwrap();
        let control = Arc::new(Mutex::new(kernel));
        let advancing = control.clone();
        let worker = thread::spawn(move || {
            let mut guard = advancing.lock().unwrap();
            let live = guard.context() == Some(&1);
            assert_eq!(guard.presence_complete(&key(1, 1)), live);
            assert_eq!(guard.fail(&key(1, 1)), live);
        });
        let revoking = control.clone();
        let revoked = thread::spawn(move || assert!(revoking.lock().unwrap().revoke_context(&1)));
        worker.join().unwrap();
        revoked.join().unwrap();
        let mut guard = control.lock().unwrap();
        assert!(!guard.scan_complete(&key(1, 1)));
        assert!(!guard.authorize_callback(&key(1, 1)));
        assert!(!guard.commit_success(&key(1, 1)));
        assert!(matches!(
            guard.current().unwrap().phase,
            Phase::Cancelled | Phase::Failed
        ));
    });
}
