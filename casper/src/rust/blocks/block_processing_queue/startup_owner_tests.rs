use std::future::{poll_fn, ready, Ready};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;

use proptest::prelude::*;
use tokio::sync::oneshot;

use super::*;

type Owner = StartupOwner<usize, Root, &'static str>;
type Ticket = StartupTicket<usize, Root, &'static str>;
type Registration = StartupRegistration<usize, Root, &'static str>;
type Context = StartupContext<usize, Root, &'static str>;
type NoCallback = fn() -> Ready<Result<(), &'static str>>;

struct Root {
    owner: Weak<Owner>,
    live: Arc<AtomicUsize>,
    stop_published: Option<Arc<AtomicBool>>,
    drop_gates: Vec<(
        std::sync::mpsc::SyncSender<()>,
        std::sync::mpsc::Receiver<()>,
    )>,
}

impl Root {
    fn new(owner: &Arc<Owner>, live: &Arc<AtomicUsize>) -> Self {
        live.fetch_add(1, Ordering::SeqCst);
        Self {
            owner: Arc::downgrade(owner),
            live: live.clone(),
            stop_published: None,
            drop_gates: Vec::new(),
        }
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if let Some(stopped) = &self.stop_published {
            assert!(
                stopped.load(Ordering::SeqCst),
                "root destroyed before stop signal"
            );
        }
        if let Some(owner) = self.owner.upgrade() {
            assert!(
                owner.state_is_unlocked_for_test(),
                "root destroyed under owner lock"
            );
        }
        for (entered, resume) in &self.drop_gates {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        }
        assert!(self.live.fetch_sub(1, Ordering::SeqCst) > 0);
    }
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
}

type FieldOwner = StartupOwner<usize, RootPair, &'static str>;

struct FieldRoot {
    owner: Weak<FieldOwner>,
    live: Arc<AtomicUsize>,
    gate: Option<(
        std::sync::mpsc::SyncSender<()>,
        std::sync::mpsc::Receiver<()>,
    )>,
    panic_after_drop: bool,
}

impl Drop for FieldRoot {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            let state = owner
                .state
                .try_lock()
                .expect("field destruction holds no controller guard");
            assert_eq!(state.leases.pending().unwrap().phase, LeasePhase::Retiring);
        }
        if let Some((entered, resume)) = &self.gate {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        }
        assert!(self.live.fetch_sub(1, Ordering::SeqCst) > 0);
        assert!(!self.panic_after_drop, "source destructor unwind");
    }
}

struct RootPair {
    source: FieldRoot,
    selected: FieldRoot,
    panic_before_fields: bool,
}

impl Drop for RootPair {
    fn drop(&mut self) {
        assert!(Arc::ptr_eq(&self.source.live, &self.selected.live));
        assert!(!self.panic_before_fields, "pair destructor unwind");
    }
}

#[test]
fn separate_root_fields_finish_destruction_before_lease_reuse() {
    let owner = Arc::new(FieldOwner::default());
    let casper = Arc::new(1);
    let prepared = owner.prepare(&casper);
    let context = prepared.handle();
    let (_registration, cleanup) = owner
        .publish(prepared, |registration| registration)
        .unwrap();
    cleanup.finish();
    let live = Arc::new(AtomicUsize::new(0));
    let (source_tx, source_rx) = std::sync::mpsc::sync_channel(0);
    let (source_resume_tx, source_resume_rx) = std::sync::mpsc::sync_channel(0);
    let (selected_tx, selected_rx) = std::sync::mpsc::sync_channel(0);
    let (selected_resume_tx, selected_resume_rx) = std::sync::mpsc::sync_channel(0);
    let mut ticket = context.request_startup(false).unwrap();
    ticket
        .try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| {
            live.fetch_add(2, Ordering::SeqCst);
            Ok(RootPair {
                source: FieldRoot {
                    owner: Arc::downgrade(&owner),
                    live: live.clone(),
                    gate: Some((source_tx, source_resume_rx)),
                    panic_after_drop: false,
                },
                selected: FieldRoot {
                    owner: Arc::downgrade(&owner),
                    live: live.clone(),
                    gate: Some((selected_tx, selected_resume_rx)),
                    panic_after_drop: false,
                },
                panic_before_fields: false,
            })
        })
        .unwrap();
    let prepared = owner.prepare(&casper);
    let context = prepared.handle();
    let (_replacement_registration, cleanup) = owner
        .publish(prepared, |registration| registration)
        .unwrap();
    let retiring = std::thread::spawn(move || cleanup.finish());
    source_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let mut new = context.request_startup(false).unwrap();
    assert!(new.try_reserve_snapshot().unwrap().is_none());
    assert_eq!(live.load(Ordering::SeqCst), 2);
    source_resume_tx.send(()).unwrap();
    selected_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(new.try_reserve_snapshot().unwrap().is_none());
    assert_eq!(live.load(Ordering::SeqCst), 1);
    selected_resume_tx.send(()).unwrap();
    retiring.join().unwrap();
    assert_eq!(live.load(Ordering::SeqCst), 0);
    drop(new.try_reserve_snapshot().unwrap().unwrap());
}

#[test]
fn destructor_unwind_drops_remaining_fields_before_releasing_the_lease() {
    for panic_in_source in [false, true] {
        let owner = Arc::new(FieldOwner::default());
        let casper = Arc::new(1);
        let prepared = owner.prepare(&casper);
        let context = prepared.handle();
        let (_registration, cleanup) = owner
            .publish(prepared, |registration| registration)
            .unwrap();
        cleanup.finish();
        let live = Arc::new(AtomicUsize::new(0));
        let mut ticket = context.request_startup(false).unwrap();
        ticket
            .try_reserve_snapshot()
            .unwrap()
            .unwrap()
            .capture(|| {
                live.fetch_add(2, Ordering::SeqCst);
                Ok(RootPair {
                    source: FieldRoot {
                        owner: Arc::downgrade(&owner),
                        live: live.clone(),
                        gate: None,
                        panic_after_drop: panic_in_source,
                    },
                    selected: FieldRoot {
                        owner: Arc::downgrade(&owner),
                        live: live.clone(),
                        gate: None,
                        panic_after_drop: false,
                    },
                    panic_before_fields: !panic_in_source,
                })
            })
            .unwrap();
        let (_, cleanup) = owner.clear_and_publish(|| ());
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cleanup.finish())).is_err()
        );
        assert_eq!(live.load(Ordering::SeqCst), 0);
        assert!(owner.state.lock().unwrap().leases.pending().is_none());
    }
}

#[test]
fn stop_during_capture_retains_the_inflight_lease_until_the_builder_ends() {
    let (owner, _casper, _registration, context, live) = setup();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(0);
    let builder_owner = owner.clone();
    let builder_live = live.clone();
    let builder = std::thread::spawn(move || {
        let mut ticket = context.request_startup(false).unwrap();
        ticket.try_reserve_snapshot().unwrap().unwrap().capture(|| {
            let root = Root::new(&builder_owner, &builder_live);
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            Ok(root)
        })
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    owner.stop();
    assert_eq!(
        owner.state.lock().unwrap().leases.pending().unwrap().phase,
        LeasePhase::Capturing
    );
    assert_eq!(live.load(Ordering::SeqCst), 1);
    assert!(owner.activate().unwrap().is_none());
    resume_tx.send(()).unwrap();
    assert_eq!(builder.join().unwrap(), Err(StartupError::Stopped));
    assert!(owner.state.lock().unwrap().leases.pending().is_none());
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

fn publish(owner: &Arc<Owner>, casper: &Arc<usize>) -> (Registration, Context) {
    let prepared = owner.prepare(casper);
    let handle = prepared.handle();
    let (registration, cleanup) = owner
        .publish(prepared, |registration| {
            assert!(owner.state.try_lock().is_err());
            registration
        })
        .unwrap();
    cleanup.finish();
    (registration, handle)
}

#[test]
fn context_handles_distinguish_republication_of_the_same_casper() {
    let owner = Arc::new(Owner::default());
    let casper = Arc::new(7usize);
    assert!(owner.current_context().unwrap().is_none());
    let (old_registration, old) = publish(&owner, &casper);
    let observed = owner.current_context().unwrap().unwrap();
    assert!(old.is_current().unwrap());
    assert!(Arc::ptr_eq(&observed.context().unwrap().unwrap(), &casper));
    let (new_registration, new) = publish(&owner, &casper);
    assert!(!old.is_current().unwrap());
    assert!(old.context().unwrap().is_none());
    assert!(observed.context().unwrap().is_none());
    drop(old_registration);
    assert!(new.is_current().unwrap());
    drop(new_registration);
    assert!(!new.is_current().unwrap());
    assert!(owner.current_context().unwrap().is_none());
}

#[test]
fn context_handles_do_not_keep_the_owner_or_casper_alive() {
    let owner = Arc::new(Owner::default());
    let casper = Arc::new(7usize);
    let (registration, handle) = publish(&owner, &casper);
    let weak_casper = Arc::downgrade(&casper);
    drop(casper);
    assert!(weak_casper.upgrade().is_none());
    assert!(handle.context().unwrap().is_none());
    owner.stop();
    assert!(!handle.is_current().unwrap());
    let weak_owner = Arc::downgrade(&owner);
    drop(owner);
    assert!(weak_owner.upgrade().is_none());
    assert!(!handle.is_current().unwrap());
    assert!(handle.context().unwrap().is_none());
    drop(registration);
}

fn setup() -> (
    Arc<Owner>,
    Arc<usize>,
    Registration,
    Context,
    Arc<AtomicUsize>,
) {
    let owner = Arc::new(Owner::default());
    let casper = Arc::new(11);
    let (registration, context) = publish(&owner, &casper);
    (
        owner,
        casper,
        registration,
        context,
        Arc::new(AtomicUsize::new(0)),
    )
}

fn submit(
    owner: &Arc<Owner>,
    context: &Context,
    live: &Arc<AtomicUsize>,
    callback: bool,
) -> Ticket {
    let mut ticket = context.request_startup(callback).unwrap();
    ticket
        .try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| Ok(Root::new(owner, live)))
        .unwrap();
    ticket
}

fn make_ready(owner: &Arc<Owner>) {
    let active = owner.activate().unwrap().unwrap();
    assert!(active.presence_complete().unwrap());
    assert!(active.scan_complete().unwrap());
    drop(active);
}

#[test]
fn capture_permission_survives_replacement_until_the_builder_and_roots_retire() {
    let (owner, casper, _registration, context, live) = setup();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(0);
    let builder_owner = owner.clone();
    let builder_live = live.clone();
    let builder = std::thread::spawn(move || {
        let mut ticket = context.request_startup(false).unwrap();
        ticket.try_reserve_snapshot().unwrap().unwrap().capture(|| {
            let root = Root::new(&builder_owner, &builder_live);
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            Ok(root)
        })
    });
    entered_rx.recv().unwrap();
    let (_replacement_registration, replacement) = publish(&owner, &casper);
    let mut ticket = replacement.request_startup(false).unwrap();
    for _ in 0..128 {
        assert!(ticket.try_reserve_snapshot().unwrap().is_none());
        assert!(owner.activate().unwrap().is_none());
        assert_eq!(live.load(Ordering::SeqCst), 1);
    }
    resume_tx.send(()).unwrap();
    assert_eq!(builder.join().unwrap(), Err(StartupError::Cancelled));
    assert_eq!(live.load(Ordering::SeqCst), 0);
    ticket
        .try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| Ok(Root::new(&owner, &live)))
        .unwrap();
    drop(ticket);
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[test]
fn pending_lease_covers_source_and_selected_root_destruction_outside_publication_guards() {
    let (owner, casper, _registration, context, live) = setup();
    let (source_tx, source_rx) = std::sync::mpsc::sync_channel(0);
    let (source_resume_tx, source_resume_rx) = std::sync::mpsc::sync_channel(0);
    let (selected_tx, selected_rx) = std::sync::mpsc::sync_channel(0);
    let (selected_resume_tx, selected_resume_rx) = std::sync::mpsc::sync_channel(0);
    let mut old = context.request_startup(false).unwrap();
    old.try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| {
            let mut root = Root::new(&owner, &live);
            root.drop_gates = vec![
                (source_tx, source_resume_rx),
                (selected_tx, selected_resume_rx),
            ];
            Ok(root)
        })
        .unwrap();
    let prepared = owner.prepare(&casper);
    let replacement = prepared.handle();
    let (_registration, cleanup) = owner
        .publish(prepared, |registration| registration)
        .unwrap();
    let retiring = std::thread::spawn(move || cleanup.finish());
    source_rx.recv().unwrap();
    let mut ticket = replacement.request_startup(false).unwrap();
    assert!(owner.context().unwrap().is_some());
    assert!(ticket.try_reserve_snapshot().unwrap().is_none());
    assert_eq!(live.load(Ordering::SeqCst), 1);
    source_resume_tx.send(()).unwrap();
    selected_rx.recv().unwrap();
    assert!(ticket.try_reserve_snapshot().unwrap().is_none());
    assert_eq!(live.load(Ordering::SeqCst), 1);
    selected_resume_tx.send(()).unwrap();
    retiring.join().unwrap();
    assert_eq!(live.load(Ordering::SeqCst), 0);
    ticket
        .try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| Ok(Root::new(&owner, &live)))
        .unwrap();
    drop(ticket);
}

#[test]
fn active_retirement_blocks_activation_but_does_not_block_one_pending_capture() {
    let (owner, _casper, _registration, context, live) = setup();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(0);
    let mut old = context.request_startup(false).unwrap();
    old.try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| {
            let mut root = Root::new(&owner, &live);
            root.drop_gates.push((entered_tx, resume_rx));
            Ok(root)
        })
        .unwrap();
    let active = owner.activate().unwrap().unwrap();
    let retiring = std::thread::spawn(move || drop(active));
    entered_rx.recv().unwrap();
    let new = submit(&owner, &context, &live, false);
    assert_eq!(live.load(Ordering::SeqCst), 2);
    assert!(owner.activate().unwrap().is_none());
    resume_tx.send(()).unwrap();
    retiring.join().unwrap();
    assert_eq!(live.load(Ordering::SeqCst), 1);
    make_ready(&owner);
    drop(new);
}

#[tokio::test]
async fn lease_availability_wakes_the_capture_waiter_without_competing_with_the_driver() {
    let (owner, casper, _registration, context, live) = setup();
    let old = submit(&owner, &context, &live, false);
    let prepared = owner.prepare(&casper);
    let replacement = prepared.handle();
    let (_registration, cleanup) = owner
        .publish(prepared, |registration| registration)
        .unwrap();
    let mut ticket = replacement.request_startup(false).unwrap();
    let builds = AtomicUsize::new(0);
    let mut capture = Box::pin(ticket.capture_snapshot(|| {
        builds.fetch_add(1, Ordering::SeqCst);
        Ok(Root::new(&owner, &live))
    }));
    poll_fn(|cx| {
        assert!(capture.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    owner.changed().await;
    cleanup.finish();
    owner.changed().await;
    capture.await.unwrap();
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    drop(old);
    drop(ticket);
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[test]
fn unused_capture_failure_and_builder_unwind_release_only_their_lease() {
    let (owner, _casper, _registration, context, live) = setup();
    let mut ticket = context.request_startup(false).unwrap();
    drop(ticket.try_reserve_snapshot().unwrap().unwrap());
    assert!(owner.state.lock().unwrap().leases.pending().is_none());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ticket
            .try_reserve_snapshot()
            .unwrap()
            .unwrap()
            .capture(|| -> Result<Root, &'static str> {
                let _root = Root::new(&owner, &live);
                panic!("builder unwind");
            })
    }))
    .is_err());
    assert_eq!(live.load(Ordering::SeqCst), 0);
    assert!(owner.state.lock().unwrap().leases.pending().is_none());
    assert_eq!(
        ticket
            .try_reserve_snapshot()
            .unwrap()
            .unwrap()
            .capture(|| Err("capture failed")),
        Err(StartupError::Scan("capture failed"))
    );
    assert_eq!(ticket.phase(), StartupPhase::Failed);
    assert!(owner.state.lock().unwrap().leases.pending().is_none());
    assert!(matches!(
        ticket.try_reserve_snapshot(),
        Err(StartupError::Scan("capture failed"))
    ));
}

#[test]
fn transferred_lease_rejects_stale_pending_role_release_and_repeated_capture() {
    let (owner, _casper, _registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, false);
    let identity = owner
        .state
        .lock()
        .unwrap()
        .leases
        .pending()
        .unwrap()
        .identity
        .clone();
    {
        let mut state = owner.state.lock().unwrap();
        assert!(!state.leases.begin_capture(&identity));
        assert!(!state.leases.release(LeaseRole::Pending, &identity));
    }
    let active = owner.activate().unwrap().unwrap();
    {
        let mut state = owner.state.lock().unwrap();
        assert_eq!(state.leases.active().unwrap().identity, identity);
        assert!(!state.leases.destroyed(LeaseRole::Pending, &identity));
        assert!(!state.leases.release(LeaseRole::Pending, &identity));
        assert!(!state.leases.abandon_capture(&identity));
    }
    drop(active);
    let new = submit(&owner, &context, &live, false);
    assert!(!owner
        .state
        .lock()
        .unwrap()
        .leases
        .release(LeaseRole::Pending, &identity));
    drop(ticket);
    assert_eq!(live.load(Ordering::SeqCst), 1);
    drop(new);
}

#[test]
fn private_allocations_are_fresh_and_old_destructors_cannot_revoke_replacements() {
    let (owner, casper, old_registration, old_context, live) = setup();
    let old = submit(&owner, &old_context, &live, false);
    let old_key = old.request.key.clone();
    let (new_registration, new_context) = publish(&owner, &casper);
    assert_eq!(old.phase(), StartupPhase::Cancelled);
    assert_ne!(old_context.identity, new_context.identity);
    assert_eq!(new_context.identity, new_context.clone().identity);
    let new = submit(&owner, &new_context, &live, false);
    assert_ne!(old_key.request, new.request.key.request);
    drop(old_registration);
    drop(old);
    assert_eq!(new.phase(), StartupPhase::Pending);
    assert_eq!(
        owner.state.lock().unwrap().completion.context(),
        Some(&new_context.identity)
    );
    assert_eq!(live.load(Ordering::SeqCst), 1);
    drop(new_registration);
    assert_eq!(new.phase(), StartupPhase::Cancelled);
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[test]
fn pending_replacement_retains_only_one_root_and_active_retirement_is_exact() {
    let (owner, _casper, _registration, context, live) = setup();
    let original = submit(&owner, &context, &live, false);
    let mut active = owner.activate().unwrap().unwrap();
    assert_eq!(active.work_mut().live.load(Ordering::SeqCst), 1);
    let mut pending = submit(&owner, &context, &live, false);
    assert_eq!(original.phase(), StartupPhase::Cancelled);
    for _ in 0..128 {
        let replacement = submit(&owner, &context, &live, false);
        assert_eq!(pending.phase(), StartupPhase::Cancelled);
        drop(pending);
        pending = replacement;
        assert_eq!(live.load(Ordering::SeqCst), 2);
        assert!(owner.activate().unwrap().is_none());
    }
    assert!(!active.presence_complete().unwrap());
    assert!(!active.scan_complete().unwrap());
    drop(original);
    assert_eq!(live.load(Ordering::SeqCst), 2);
    drop(active);
    assert_eq!(live.load(Ordering::SeqCst), 1);
    let active = owner.activate().unwrap().unwrap();
    assert_eq!(
        owner.state.lock().unwrap().completion.active(),
        Some(&active.key)
    );
    drop(pending);
    assert_eq!(live.load(Ordering::SeqCst), 1);
    drop(active);
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn both_scan_phases_precede_callback_construction_and_actual_return() {
    let (owner, _casper, _registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, true);
    let constructed = Arc::new(AtomicUsize::new(0));
    let count = constructed.clone();
    let (release, released) = oneshot::channel();
    let mut completion = Box::pin(ticket.finish(Some(move || {
        count.fetch_add(1, Ordering::SeqCst);
        async move {
            released.await.unwrap();
            Ok(())
        }
    })));
    poll_fn(|cx| {
        assert!(completion.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    let active = owner.activate().unwrap().unwrap();
    assert!(!active.scan_complete().unwrap());
    assert!(active.presence_complete().unwrap());
    poll_fn(|cx| {
        assert!(completion.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    assert!(active.scan_complete().unwrap());
    poll_fn(|cx| {
        assert!(completion.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(constructed.load(Ordering::SeqCst), 1);
    assert_eq!(
        owner
            .state
            .lock()
            .unwrap()
            .completion
            .current()
            .unwrap()
            .phase,
        StartupPhase::Authorized
    );
    assert!(owner.context().unwrap().is_some());
    drop(active);
    release.send(()).unwrap();
    assert_eq!(completion.await, Ok(()));
    assert_eq!(
        owner
            .state
            .lock()
            .unwrap()
            .completion
            .current()
            .unwrap()
            .phase,
        StartupPhase::Succeeded
    );
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn callback_error_and_configuration_mismatch_cannot_complete_startup() {
    for mismatch in [false, true] {
        let (owner, _casper, _registration, context, live) = setup();
        let ticket = submit(&owner, &context, &live, !mismatch);
        make_ready(&owner);
        let result = ticket.finish(Some(|| ready(Err("callback failed")))).await;
        assert_eq!(
            result,
            Err(if mismatch {
                StartupError::CallbackConfiguration
            } else {
                StartupError::Callback("callback failed")
            })
        );
        assert_eq!(
            owner
                .state
                .lock()
                .unwrap()
                .completion
                .current()
                .unwrap()
                .phase,
            StartupPhase::Failed
        );
    }
    let (owner, _casper, _registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, true);
    make_ready(&owner);
    assert_eq!(
        ticket.finish(None::<NoCallback>).await,
        Err(StartupError::CallbackConfiguration)
    );
}

#[tokio::test]
async fn replacement_after_ready_notification_cannot_authorize_old_callback() {
    let (owner, casper, _old_registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, true);
    make_ready(&owner);
    let (_new_registration, replacement) = publish(&owner, &casper);
    let new = submit(&owner, &replacement, &live, false);
    let constructed = AtomicUsize::new(0);
    let result = ticket
        .finish(Some(|| {
            constructed.fetch_add(1, Ordering::SeqCst);
            ready(Ok(()))
        }))
        .await;
    assert_eq!(result, Err(StartupError::Cancelled));
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    assert_eq!(new.phase(), StartupPhase::Pending);
}

#[tokio::test]
async fn replacement_during_callback_cancels_its_future_and_cannot_complete_new_ticket() {
    let (owner, casper, _old_registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, true);
    make_ready(&owner);
    let dropped = Arc::new(AtomicBool::new(false));
    let probe = DropFlag(dropped.clone());
    let (release, released) = oneshot::channel::<()>();
    let mut completion = Box::pin(ticket.finish(Some(move || async move {
        let _probe = probe;
        released.await.unwrap();
        Ok(())
    })));
    poll_fn(|cx| {
        assert!(completion.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let (_new_registration, replacement) = publish(&owner, &casper);
    let new = submit(&owner, &replacement, &live, false);
    assert_eq!(completion.await, Err(StartupError::Cancelled));
    assert!(dropped.load(Ordering::SeqCst));
    assert!(release.send(()).is_err());
    assert_eq!(new.phase(), StartupPhase::Pending);
}

#[tokio::test]
async fn cancellation_destroys_unpolled_waiting_and_suspended_callback_futures() {
    for stage in 0..3 {
        let (owner, _casper, _registration, context, live) = setup();
        let ticket = submit(&owner, &context, &live, true);
        let dropped = Arc::new(AtomicBool::new(false));
        let probe = DropFlag(dropped.clone());
        let mut completion = Box::pin(ticket.finish(Some(move || async move {
            let _probe = probe;
            std::future::pending::<Result<(), &'static str>>().await
        })));
        if stage == 2 {
            make_ready(&owner);
        }
        if stage > 0 {
            poll_fn(|cx| {
                assert!(completion.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }
        drop(completion);
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(
            owner
                .state
                .lock()
                .unwrap()
                .completion
                .current()
                .unwrap()
                .phase,
            StartupPhase::Cancelled
        );
        assert_eq!(live.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn scan_failure_and_abandoned_work_wake_the_actual_ticket() {
    for stage in 0..3 {
        let (owner, _casper, _registration, context, live) = setup();
        let ticket = submit(&owner, &context, &live, false);
        let mut waiting = Box::pin(ticket.finish(None::<NoCallback>));
        poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let active = owner.activate().unwrap().unwrap();
        if stage == 1 {
            assert!(active.presence_complete().unwrap());
        }
        if stage < 2 {
            assert!(active.fail("storage failed").unwrap());
        }
        drop(active);
        assert_eq!(
            waiting.await,
            Err(if stage == 2 {
                StartupError::ScanAbandoned
            } else {
                StartupError::Scan("storage failed")
            })
        );
        assert_eq!(live.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn stop_and_owner_destruction_wake_waiters_without_retaining_casper() {
    for destroy_owner in [false, true] {
        let (owner, casper, _registration, context, live) = setup();
        let weak = Arc::downgrade(&casper);
        let ticket = submit(&owner, &context, &live, false);
        let mut waiting = Box::pin(ticket.finish(None::<NoCallback>));
        poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(casper);
        assert!(weak.upgrade().is_none());
        assert!(owner.context().unwrap().is_none());
        if !destroy_owner {
            owner.stop();
            assert!(owner.is_stopped());
            assert!(owner.activate().unwrap().is_none());
            assert!(matches!(
                context.request_startup(false),
                Err(StartupError::Stopped)
            ));
        }
        drop(owner);
        assert_eq!(waiting.await, Err(StartupError::Stopped));
        assert_eq!(live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn parked_active_work_does_not_retain_casper_and_stop_preserves_active_ownership() {
    let (owner, casper, _registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, false);
    let active = owner.activate().unwrap().unwrap();
    assert!(active.context().unwrap().is_some());
    let weak = Arc::downgrade(&casper);
    drop(casper);
    assert!(weak.upgrade().is_none());
    assert!(active.context().unwrap().is_none());
    owner.stop();
    assert_eq!(ticket.phase(), StartupPhase::Cancelled);
    assert!(owner.state.lock().unwrap().completion.active().is_some());
    assert_eq!(live.load(Ordering::SeqCst), 1);
    drop(active);
    assert!(owner.state.lock().unwrap().completion.active().is_none());
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn committed_success_survives_replacement_before_caller_observation() {
    let (owner, casper, _registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, false);
    make_ready(&owner);
    let (committed, observed) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = ticket.finish(None::<NoCallback>).await;
        committed.send(()).unwrap();
        released.await.unwrap();
        result
    });
    observed.await.unwrap();
    let (_new_registration, replacement) = publish(&owner, &casper);
    let new = submit(&owner, &replacement, &live, false);
    release.send(()).unwrap();
    assert_eq!(task.await.unwrap(), Ok(()));
    assert_eq!(new.phase(), StartupPhase::Pending);
}

#[tokio::test]
async fn publication_rejects_wrong_owner_and_stopped_owner_without_swapping() {
    let (owner, casper, _registration, _context, _live) = setup();
    let other = Arc::new(Owner::default());
    let prepared = other.prepare(&casper);
    assert!(matches!(
        owner.publish(prepared, |_| panic!("wrong owner swap")),
        Err(StartupError::WrongOwner)
    ));
    owner.stop();
    let prepared = owner.prepare(&casper);
    assert!(matches!(
        owner.publish(prepared, |_| panic!("stopped owner swap")),
        Err(StartupError::Stopped)
    ));
    let (_, cleanup) = owner.clear_and_publish(|| assert!(owner.state.try_lock().is_err()));
    cleanup.finish();
    owner.changed().await;
    assert!(owner.is_stopped());
}

proptest! {
    #[test]
    fn owned_event_histories_preserve_ticket_results_and_root_bounds(
        events in prop::collection::vec((0u8..11, any::<usize>()), 0..192),
        stop_at in prop::option::of(0usize..192),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (owner, casper, registration, mut context, live) = setup();
        let mut registration = Some(registration);
        let mut tickets: Vec<Option<Ticket>> = Vec::new();
        let mut expected: Vec<StartupPhase> = Vec::new();
        let mut current: Option<usize> = None;
        let mut active: Option<(usize, ActiveStartup<usize, Root, &'static str>)> = None;
        let mut stopped = false;
        let mut published = true;
        for (position, (event, selector)) in events.into_iter().enumerate() {
            if stop_at == Some(position) {
                stopped = true;
                published = false;
                if let Some(index) = current {
                    if !expected[index].is_terminal() { expected[index] = StartupPhase::Cancelled; }
                }
                owner.stop();
            }
            let selected = if tickets.is_empty() { None } else { Some(selector % tickets.len()) };
            match event {
                0 if !stopped && published => {
                    if let Some(index) = current {
                        if !expected[index].is_terminal() { expected[index] = StartupPhase::Cancelled; }
                    }
                    current = Some(tickets.len());
                    tickets.push(Some(submit(&owner, &context, &live, false)));
                    expected.push(StartupPhase::Pending);
                }
                1 if !stopped => {
                    if let Some(index) = current {
                        if !expected[index].is_terminal() { expected[index] = StartupPhase::Cancelled; }
                    }
                    let (new_registration, new_context) = publish(&owner, &casper);
                    registration = Some(new_registration);
                    context = new_context;
                    published = true;
                }
                2 => {
                    let should_activate = !stopped && published && active.is_none() && current.is_some_and(|index| expected[index] == StartupPhase::Pending);
                    let work = owner.activate().unwrap();
                    prop_assert_eq!(work.is_some(), should_activate);
                    if let Some(work) = work {
                        let index = current.unwrap();
                        expected[index] = StartupPhase::Presence;
                        active = Some((index, work));
                    }
                }
                3 | 4 => if let Some((index, work)) = active.as_ref() {
                    let from = if event == 3 { StartupPhase::Presence } else { StartupPhase::Admission };
                    let to = if event == 3 { StartupPhase::Admission } else { StartupPhase::Ready };
                    let allowed = !stopped && published && current == Some(*index) && expected[*index] == from;
                    let result = if event == 3 { work.presence_complete() } else { work.scan_complete() };
                    prop_assert_eq!(result.unwrap(), allowed);
                    if allowed { expected[*index] = to; }
                },
                5 => if let Some(index) = selected {
                    if tickets[index].is_some() && current == Some(index) && !expected[index].is_terminal() {
                        expected[index] = StartupPhase::Cancelled;
                    }
                    drop(tickets[index].take());
                },
                6 => if let Some((index, work)) = active.take() {
                    if current == Some(index) && matches!(expected[index], StartupPhase::Presence | StartupPhase::Admission) {
                        expected[index] = StartupPhase::Failed;
                    }
                    drop(work);
                },
                7 => if let Some((index, work)) = active.as_ref() {
                    let allowed = !stopped && published && current == Some(*index) && !expected[*index].is_terminal();
                    prop_assert_eq!(work.fail("generated failure").unwrap(), allowed);
                    if allowed { expected[*index] = StartupPhase::Failed; }
                },
                8 => if let Some(index) = selected {
                    if tickets[index].is_some() && (expected[index].is_terminal() || expected[index] == StartupPhase::Ready) {
                        let ticket = tickets[index].take().unwrap();
                        let result = runtime.block_on(ticket.finish(None::<NoCallback>));
                        if expected[index] == StartupPhase::Ready { expected[index] = StartupPhase::Succeeded; }
                        prop_assert_eq!(result.is_ok(), expected[index] == StartupPhase::Succeeded);
                    }
                },
                9 => {
                    if let Some(index) = current {
                        if !expected[index].is_terminal() { expected[index] = StartupPhase::Cancelled; }
                    }
                    let (_, cleanup) = owner.clear_and_publish(|| ());
                    cleanup.finish();
                    published = false;
                    drop(registration.take());
                }
                _ => {}
            }
            for (index, ticket) in tickets.iter().enumerate() {
                if let Some(ticket) = ticket { prop_assert_eq!(ticket.phase(), expected[index]); }
            }
            let pending_count = usize::from(current.is_some_and(|index| expected[index] == StartupPhase::Pending));
            prop_assert_eq!(live.load(Ordering::SeqCst), pending_count + usize::from(active.is_some()));
            prop_assert_eq!(owner.is_stopped(), stopped);
        }
        drop(tickets);
        drop(active);
        drop(registration);
        prop_assert_eq!(live.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn poisoned_owner_rejects_publication_and_clear_cancels_waiting_startup() {
    let (owner, casper, _registration, context, live) = setup();
    let ticket = submit(&owner, &context, &live, false);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.with_locked_state_for_test(|| panic!("poison fixture"));
    }))
    .is_err());
    assert!(matches!(
        owner.publish(owner.prepare(&casper), |_| panic!("poisoned publication")),
        Err(StartupError::Poisoned)
    ));
    let (_, cleanup) = owner.clear_and_publish(|| ());
    owner.state.clear_poison();
    cleanup.finish();
    assert!(owner.is_stopped());
    assert_eq!(
        ticket.finish(None::<NoCallback>).await,
        Err(StartupError::Cancelled)
    );
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deferred_stop_publishes_cancellation_before_destroying_pending_roots() {
    let (owner, _casper, _registration, context, live) = setup();
    let stopped = Arc::new(AtomicBool::new(false));
    let mut ticket = context.request_startup(false).unwrap();
    ticket
        .try_reserve_snapshot()
        .unwrap()
        .unwrap()
        .capture(|| {
            let mut root = Root::new(&owner, &live);
            root.stop_published = Some(stopped.clone());
            Ok(root)
        })
        .unwrap();
    let cleanup = owner.stop_deferred();
    assert!(owner.is_stopped());
    assert_eq!(ticket.phase(), StartupPhase::Cancelled);
    assert_eq!(live.load(Ordering::SeqCst), 1);
    stopped.store(true, Ordering::SeqCst);
    cleanup.finish();
    assert_eq!(live.load(Ordering::SeqCst), 0);
    assert_eq!(
        ticket.finish(None::<NoCallback>).await,
        Err(StartupError::Stopped)
    );
}
