use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex, RwLock};
use loom::thread;

struct Schedule {
    requested: AtomicUsize,
    launched: AtomicUsize,
    completed: AtomicUsize,
    in_flight: AtomicUsize,
    running: AtomicBool,
    retry_ready: AtomicBool,
}

impl Schedule {
    fn new() -> Self {
        Self {
            requested: AtomicUsize::new(0),
            launched: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            running: AtomicBool::new(false),
            retry_ready: AtomicBool::new(false),
        }
    }

    fn request(&self) -> usize { self.requested.fetch_add(1, Ordering::SeqCst) + 1 }

    fn try_start(&self) -> bool {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn release_or_reacquire(&self) -> bool {
        self.running.store(false, Ordering::SeqCst);
        (self.requested.load(Ordering::SeqCst) > self.launched.load(Ordering::SeqCst)
            || self.retry_ready.load(Ordering::SeqCst))
            && self.try_start()
    }

    fn next_coverage(&self) -> Option<usize> {
        let ticket = self.requested.load(Ordering::SeqCst);
        if ticket > self.launched.load(Ordering::SeqCst) {
            self.retry_ready.store(false, Ordering::SeqCst);
            return Some(ticket);
        }
        if self.retry_ready.swap(false, Ordering::SeqCst)
            && self.completed.load(Ordering::SeqCst) < ticket
        {
            return Some(ticket);
        }
        None
    }

    fn launch_latest(&self) -> usize {
        let ticket = self.next_coverage().unwrap();
        self.launched.fetch_max(ticket, Ordering::SeqCst);
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        ticket
    }

    fn succeed(&self, ticket: usize) {
        self.completed.fetch_max(ticket, Ordering::SeqCst);
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if self.completed.load(Ordering::SeqCst) >= self.requested.load(Ordering::SeqCst) {
            self.retry_ready.store(false, Ordering::SeqCst);
        }
    }

    fn fail(&self, ticket: usize) -> bool {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        self.completed.load(Ordering::SeqCst) < ticket
    }

    fn make_retry_ready(&self, ticket: usize) -> bool {
        if self.completed.load(Ordering::SeqCst) >= ticket {
            return false;
        }
        self.retry_ready.store(true, Ordering::SeqCst);
        true
    }
}

struct Ledger {
    head: Mutex<usize>,
    records: AtomicUsize,
    effects: AtomicUsize,
    published: AtomicUsize,
}

struct RecoveryCursors {
    projected: AtomicUsize,
    projection_cursor: Mutex<usize>,
    effects_complete: AtomicUsize,
    effects_cursor: Mutex<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotCapture {
    Stale,
    Corrupt,
    Coherent(ProjectedSnapshot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProjectedSnapshot {
    durable_revision: usize,
    projected_revision: usize,
    dag_floor_revision: usize,
}

struct ProjectedLedger {
    endpoint: Mutex<(usize, usize)>,
    dag_floor: Mutex<usize>,
    projection: RwLock<()>,
}

impl ProjectedLedger {
    fn new() -> Self {
        Self {
            endpoint: Mutex::new((0, 0)),
            dag_floor: Mutex::new(0),
            projection: RwLock::new(()),
        }
    }

    fn append(&self) {
        let mut endpoint = self.endpoint.lock().unwrap();
        endpoint.0 += 1;
    }

    fn project(&self) {
        let head = self.endpoint.lock().unwrap().0;
        {
            let _guard = self.projection.write().unwrap();
            let mut dag_floor = self.dag_floor.lock().unwrap();
            *dag_floor = (*dag_floor).max(head);
        }
        let mut endpoint = self.endpoint.lock().unwrap();
        endpoint.1 = endpoint.1.max(head);
    }

    fn capture(&self) -> SnapshotCapture {
        let before = *self.endpoint.lock().unwrap();
        if before.0 != before.1 {
            return SnapshotCapture::Stale;
        }
        let _guard = self.projection.read().unwrap();
        let dag_floor = *self.dag_floor.lock().unwrap();
        let after = *self.endpoint.lock().unwrap();
        if before != after || after.0 != after.1 {
            return SnapshotCapture::Stale;
        }
        if dag_floor != after.0 {
            return SnapshotCapture::Corrupt;
        }
        SnapshotCapture::Coherent(ProjectedSnapshot {
            durable_revision: after.0,
            projected_revision: after.1,
            dag_floor_revision: dag_floor,
        })
    }
}

const F0: usize = 0;
const F1: usize = 1;
const C: usize = 2;

fn dag_descends(base: usize, candidate: usize) -> bool {
    base == candidate || base == F0 || (base == F1 && candidate == C)
}

fn state_preserves(base: usize, candidate: usize) -> bool {
    base == candidate || base == F0 || (base == F1 && candidate != C)
}

struct BoundLedger {
    head: Mutex<(usize, usize)>,
    callbacks: AtomicUsize,
}

#[derive(Clone, Copy)]
struct FrozenFinalizationEvidence {
    predecessor: (usize, usize),
    requested_target: usize,
    selected_target: usize,
    causal_support: usize,
    state_support: usize,
}

struct TargetBoundLedger {
    head: Mutex<(usize, usize)>,
    live_candidate: AtomicUsize,
    callbacks: AtomicUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RootedLedgerState {
    anchor: Option<usize>,
    head: Option<usize>,
    records: usize,
    cursors: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnsureGenesis {
    Initialized,
    AlreadyCanonical,
}

struct RootedLedger {
    state: Mutex<RootedLedgerState>,
}

impl RootedLedgerState {
    fn pristine() -> Self {
        Self {
            anchor: None,
            head: None,
            records: 0,
            cursors: 0,
        }
    }

    fn prefix(head: usize) -> usize {
        if head == 0 {
            0
        } else {
            (1 << (head + 1)) - 2
        }
    }

    fn valid(&self) -> bool {
        match (self.anchor, self.head, self.cursors) {
            (None, None, 0) => self.records == 0,
            (Some(_), Some(head), 0b111) => self.records == Self::prefix(head),
            _ => false,
        }
    }
}

impl RootedLedger {
    fn new() -> Self {
        Self {
            state: Mutex::new(RootedLedgerState::pristine()),
        }
    }

    fn ensure_genesis(&self, genesis: usize) -> Result<EnsureGenesis, ()> {
        let mut state = self.state.lock().unwrap();
        if *state == RootedLedgerState::pristine() {
            *state = RootedLedgerState {
                anchor: Some(genesis),
                head: Some(0),
                records: 0,
                cursors: 0b111,
            };
            return Ok(EnsureGenesis::Initialized);
        }
        if state.valid() && state.anchor == Some(genesis) {
            return Ok(EnsureGenesis::AlreadyCanonical);
        }
        Err(())
    }

    fn append(&self, candidate: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.valid() || state.anchor.is_none() || state.head != candidate.checked_sub(1) {
            return false;
        }
        state.head = Some(candidate);
        state.records |= 1 << candidate;
        true
    }

    fn snapshot(&self) -> RootedLedgerState { *self.state.lock().unwrap() }
}

impl BoundLedger {
    fn new() -> Self {
        Self {
            head: Mutex::new((0, F0)),
            callbacks: AtomicUsize::new(0),
        }
    }

    fn capture(&self) -> (usize, usize) { *self.head.lock().unwrap() }

    fn try_append(&self, expected: (usize, usize), candidate: usize) -> bool {
        let mut head = self.head.lock().unwrap();
        if *head != expected
            || !dag_descends(expected.1, candidate)
            || !state_preserves(expected.1, candidate)
        {
            return false;
        }
        *head = (expected.0 + 1, candidate);
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        true
    }
}

impl TargetBoundLedger {
    fn new() -> Self {
        Self {
            head: Mutex::new((0, F0)),
            live_candidate: AtomicUsize::new(F1),
            callbacks: AtomicUsize::new(0),
        }
    }

    fn observe_new_candidate(&self, target: usize) {
        self.live_candidate.store(target, Ordering::Release);
    }

    fn try_materialize(&self, evidence: FrozenFinalizationEvidence) -> bool {
        if evidence.requested_target != evidence.selected_target
            || evidence.causal_support <= 8
            || evidence.state_support <= 8
        {
            return false;
        }
        let mut head = self.head.lock().unwrap();
        if *head != evidence.predecessor
            || !dag_descends(evidence.predecessor.1, evidence.selected_target)
            || !state_preserves(evidence.predecessor.1, evidence.selected_target)
        {
            return false;
        }
        *head = (evidence.predecessor.0 + 1, evidence.selected_target);
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        true
    }
}

impl RecoveryCursors {
    fn new() -> Self {
        Self {
            projected: AtomicUsize::new(0),
            projection_cursor: Mutex::new(0),
            effects_complete: AtomicUsize::new(0),
            effects_cursor: Mutex::new(0),
        }
    }

    fn project_next(&self, head: usize) {
        let mut cursor = self.projection_cursor.lock().unwrap();
        if *cursor < head {
            *cursor += 1;
            self.projected.fetch_or(1 << *cursor, Ordering::SeqCst);
        }
    }

    fn complete_effects(&self, round: usize, head: usize) {
        let mut cursor = self.effects_cursor.lock().unwrap();
        self.effects_complete.fetch_or(1 << round, Ordering::SeqCst);
        let completed = self.effects_complete.load(Ordering::SeqCst);
        while *cursor < head && completed & (1 << (*cursor + 1)) != 0 {
            *cursor += 1;
        }
    }
}

impl Ledger {
    fn new() -> Self {
        Self {
            head: Mutex::new(0),
            records: AtomicUsize::new(0),
            effects: AtomicUsize::new(0),
            published: AtomicUsize::new(0),
        }
    }

    fn try_append(&self, expected: usize, candidate: usize) -> bool {
        let mut head = self.head.lock().unwrap();
        if *head != expected || candidate != expected + 1 {
            return false;
        }
        self.records.fetch_or(1 << candidate, Ordering::Release);
        *head = candidate;
        true
    }

    fn apply_effect(&self, round: usize) {
        let head = *self.head.lock().unwrap();
        assert!(round <= head);
        assert_ne!(self.records.load(Ordering::Acquire) & (1 << round), 0);
        self.effects.fetch_or(1 << round, Ordering::AcqRel);
    }

    fn publish(&self, round: usize) { self.published.fetch_max(round, Ordering::AcqRel); }
}

#[test]
fn request_release_race_has_no_lost_wake() {
    loom::model(|| {
        let schedule = Arc::new(Schedule::new());
        schedule.request();
        assert!(schedule.try_start());
        let first = schedule.launch_latest();
        schedule.succeed(first);

        let requester = {
            let schedule = schedule.clone();
            thread::spawn(move || {
                schedule.request();
                schedule.try_start();
            })
        };
        let releaser = {
            let schedule = schedule.clone();
            thread::spawn(move || {
                schedule.release_or_reacquire();
            })
        };
        requester.join().unwrap();
        releaser.join().unwrap();

        if schedule.requested.load(Ordering::SeqCst) > schedule.launched.load(Ordering::SeqCst) {
            assert!(schedule.running.load(Ordering::SeqCst));
        }
    });
}

#[test]
fn failed_worker_never_completes_coverage_and_is_retried() {
    loom::model(|| {
        let schedule = Schedule::new();
        schedule.request();
        let first = schedule.launch_latest();
        assert!(schedule.fail(first));
        assert_eq!(schedule.completed.load(Ordering::SeqCst), 0);
        assert!(schedule.make_retry_ready(first));
        let retry = schedule.launch_latest();
        assert_eq!(retry, first);
        schedule.succeed(retry);
        assert_eq!(schedule.completed.load(Ordering::SeqCst), first);
    });
}

#[test]
fn newer_success_subsumes_concurrent_older_retry() {
    loom::model(|| {
        let schedule = Arc::new(Schedule::new());
        schedule.request();
        let older_ticket = schedule.launch_latest();
        schedule.request();
        let newer_ticket = schedule.launch_latest();

        let older = {
            let schedule = schedule.clone();
            thread::spawn(move || {
                if schedule.fail(older_ticket) {
                    schedule.make_retry_ready(older_ticket);
                }
            })
        };
        let newer = {
            let schedule = schedule.clone();
            thread::spawn(move || schedule.succeed(newer_ticket))
        };
        older.join().unwrap();
        newer.join().unwrap();

        assert_eq!(schedule.completed.load(Ordering::SeqCst), newer_ticket);
        assert_eq!(schedule.next_coverage(), None);
    });
}

#[test]
fn parallel_same_head_append_has_one_winner_and_no_early_effect() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                if ledger.try_append(0, 1) {
                    ledger.apply_effect(1);
                }
            })
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                if ledger.try_append(0, 1) {
                    ledger.apply_effect(1);
                }
            })
        };
        left.join().unwrap();
        right.join().unwrap();

        assert_eq!(*ledger.head.lock().unwrap(), 1);
        assert_eq!(ledger.records.load(Ordering::Acquire), 1 << 1);
        assert_eq!(ledger.effects.load(Ordering::Acquire), 1 << 1);
    });
}

#[test]
fn effect_retry_and_out_of_order_publication_are_idempotent() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        assert!(ledger.try_append(0, 1));
        assert!(ledger.try_append(1, 2));
        let older = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                ledger.apply_effect(1);
                ledger.publish(1);
            })
        };
        let newer = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                ledger.apply_effect(2);
                ledger.apply_effect(2);
                ledger.publish(2);
            })
        };
        older.join().unwrap();
        newer.join().unwrap();

        assert_eq!(ledger.effects.load(Ordering::Acquire), (1 << 1) | (1 << 2));
        assert_eq!(ledger.published.load(Ordering::Acquire), 2);
    });
}

#[test]
fn concurrent_recovery_advances_only_contiguous_durable_prefixes() {
    loom::model(|| {
        let recovery = Arc::new(RecoveryCursors::new());
        let first = {
            let recovery = recovery.clone();
            thread::spawn(move || {
                recovery.project_next(2);
                recovery.complete_effects(1, 2);
            })
        };
        let second = {
            let recovery = recovery.clone();
            thread::spawn(move || {
                recovery.project_next(2);
                recovery.complete_effects(2, 2);
            })
        };
        first.join().unwrap();
        second.join().unwrap();

        assert_eq!(*recovery.projection_cursor.lock().unwrap(), 2);
        assert_eq!(
            recovery.projected.load(Ordering::SeqCst),
            (1 << 1) | (1 << 2)
        );
        assert_eq!(*recovery.effects_cursor.lock().unwrap(), 2);
        assert_eq!(
            recovery.effects_complete.load(Ordering::SeqCst),
            (1 << 1) | (1 << 2)
        );
    });
}

#[test]
fn concurrent_append_projection_and_capture_never_publish_a_mixed_snapshot() {
    loom::model(|| {
        let ledger = Arc::new(ProjectedLedger::new());
        let append = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.append())
        };
        let project = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.project())
        };
        let capture = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.capture())
        };
        append.join().unwrap();
        project.join().unwrap();
        let outcome = capture.join().unwrap();

        assert_ne!(outcome, SnapshotCapture::Corrupt);
        if let SnapshotCapture::Coherent(snapshot) = outcome {
            assert_eq!(snapshot.durable_revision, snapshot.projected_revision);
            assert_eq!(snapshot.projected_revision, snapshot.dag_floor_revision);
            assert!(snapshot.durable_revision <= ledger.endpoint.lock().unwrap().0);
        }
    });
}

#[test]
fn coherent_capture_remains_a_prefix_after_later_projection() {
    loom::model(|| {
        let ledger = ProjectedLedger::new();
        let captured = ledger.capture();
        ledger.append();
        ledger.project();

        assert_eq!(
            captured,
            SnapshotCapture::Coherent(ProjectedSnapshot {
                durable_revision: 0,
                projected_revision: 0,
                dag_floor_revision: 0,
            })
        );
        assert_eq!(*ledger.endpoint.lock().unwrap(), (1, 1));
        assert_eq!(*ledger.dag_floor.lock().unwrap(), 1);
    });
}

#[test]
fn projection_lag_is_retryable_and_post_projection_capture_is_coherent() {
    loom::model(|| {
        let ledger = ProjectedLedger::new();
        ledger.append();
        assert_eq!(ledger.capture(), SnapshotCapture::Stale);
        ledger.project();
        assert_eq!(
            ledger.capture(),
            SnapshotCapture::Coherent(ProjectedSnapshot {
                durable_revision: 1,
                projected_revision: 1,
                dag_floor_revision: 1,
            })
        );
    });
}

#[test]
fn stable_fully_projected_floor_mismatch_is_corruption() {
    loom::model(|| {
        let ledger = ProjectedLedger::new();
        ledger.append();
        ledger.project();
        *ledger.dag_floor.lock().unwrap() = 0;
        assert_eq!(ledger.capture(), SnapshotCapture::Corrupt);
    });
}

#[test]
fn parallel_bound_certificates_have_one_state_preserving_winner() {
    loom::model(|| {
        let ledger = Arc::new(BoundLedger::new());
        let captured = ledger.capture();
        let first = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.try_append(captured, F1))
        };
        let second = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.try_append(captured, C))
        };
        let first_committed = first.join().unwrap();
        let second_committed = second.join().unwrap();

        assert_ne!(first_committed, second_committed);
        let current = ledger.capture();
        assert_eq!(current.0, 1);
        assert!(state_preserves(F0, current.1));
        assert_eq!(ledger.callbacks.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn stale_certificate_cannot_late_bind_across_state_regression() {
    loom::model(|| {
        let ledger = Arc::new(BoundLedger::new());
        let captured = ledger.capture();
        let first_committed = Arc::new(AtomicBool::new(false));
        let first = {
            let ledger = ledger.clone();
            let first_committed = first_committed.clone();
            thread::spawn(move || {
                assert!(ledger.try_append(captured, F1));
                first_committed.store(true, Ordering::SeqCst);
            })
        };
        let stale = {
            let ledger = ledger.clone();
            let first_committed = first_committed.clone();
            thread::spawn(move || {
                while !first_committed.load(Ordering::SeqCst) {
                    thread::yield_now();
                }
                assert!(dag_descends(F1, C));
                assert!(!state_preserves(F1, C));
                assert!(!ledger.try_append(captured, C));
            })
        };
        first.join().unwrap();
        stale.join().unwrap();

        assert_eq!(ledger.capture(), (1, F1));
        assert_eq!(ledger.callbacks.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn frozen_target_cannot_mix_with_a_concurrent_latest_message_arrival() {
    loom::model(|| {
        let ledger = Arc::new(TargetBoundLedger::new());
        let evidence = FrozenFinalizationEvidence {
            predecessor: (0, F0),
            requested_target: F1,
            selected_target: F1,
            causal_support: 12,
            state_support: 12,
        };
        let materializer = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.try_materialize(evidence))
        };
        let arrival = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.observe_new_candidate(C))
        };
        assert!(materializer.join().unwrap());
        arrival.join().unwrap();

        assert_eq!(*ledger.head.lock().unwrap(), (1, F1));
        assert_eq!(ledger.live_candidate.load(Ordering::Acquire), C);
        assert_eq!(ledger.callbacks.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn parallel_exact_genesis_assertions_have_one_atomic_initializer() {
    loom::model(|| {
        let ledger = Arc::new(RootedLedger::new());
        let initialized = Arc::new(AtomicUsize::new(0));
        let already = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let ledger = ledger.clone();
            let initialized = initialized.clone();
            let already = already.clone();
            handles.push(thread::spawn(move || {
                match ledger.ensure_genesis(1).unwrap() {
                    EnsureGenesis::Initialized => {
                        initialized.fetch_add(1, Ordering::SeqCst);
                    }
                    EnsureGenesis::AlreadyCanonical => {
                        already.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(initialized.load(Ordering::SeqCst), 1);
        assert_eq!(already.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.snapshot(), RootedLedgerState {
            anchor: Some(1),
            head: Some(0),
            records: 0,
            cursors: 0b111,
        });
    });
}

#[test]
fn conflicting_genesis_assertion_is_inert_during_exact_retry() {
    loom::model(|| {
        let ledger = Arc::new(RootedLedger::new());
        assert_eq!(ledger.ensure_genesis(1), Ok(EnsureGenesis::Initialized));
        let exact = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.ensure_genesis(1))
        };
        let conflict = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.ensure_genesis(2))
        };

        assert_eq!(exact.join().unwrap(), Ok(EnsureGenesis::AlreadyCanonical));
        assert_eq!(conflict.join().unwrap(), Err(()));
        assert_eq!(ledger.snapshot(), RootedLedgerState {
            anchor: Some(1),
            head: Some(0),
            records: 0,
            cursors: 0b111,
        });
    });
}

#[test]
fn exact_genesis_retry_cannot_reset_a_parallel_append() {
    loom::model(|| {
        let ledger = Arc::new(RootedLedger::new());
        assert_eq!(ledger.ensure_genesis(1), Ok(EnsureGenesis::Initialized));
        let retry = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.ensure_genesis(1))
        };
        let append = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.append(1))
        };

        assert_eq!(retry.join().unwrap(), Ok(EnsureGenesis::AlreadyCanonical));
        assert!(append.join().unwrap());
        assert_eq!(ledger.snapshot(), RootedLedgerState {
            anchor: Some(1),
            head: Some(1),
            records: 1 << 1,
            cursors: 0b111,
        });
    });
}

#[test]
fn concurrent_restart_observes_pristine_or_fully_rooted_bootstrap() {
    loom::model(|| {
        let ledger = Arc::new(RootedLedger::new());
        let bootstrap = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.ensure_genesis(1))
        };
        let restart = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.snapshot())
        };

        assert!(bootstrap.join().unwrap().is_ok());
        let observed = restart.join().unwrap();
        assert!(observed.valid());
        assert!(
            observed == RootedLedgerState::pristine()
                || observed
                    == RootedLedgerState {
                        anchor: Some(1),
                        head: Some(0),
                        records: 0,
                        cursors: 0b111,
                    }
        );
    });
}
