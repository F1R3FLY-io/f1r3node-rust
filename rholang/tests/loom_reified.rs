//! N7: loom models of four coordination protocols of Reified RSpaces (P9 design v2, section 10,
//! amendment A11).
//!
//! Each test is a shadow model, as in the other loom tests of this crate
//! (`loom_metering_ownership.rs`). The production types use `tokio::sync` and `std::sync`, and
//! loom explores only the interleavings of `loom::sync`. So each model restates one protocol
//! with loom types, and it keeps every step that orders a shared write.
//!
//! The models do not claim a race in native execution. There, every intent carries the
//! footprint key `[2, 0]` (`deterministic_reduction.rs:1049-1051`). So all intents of a frontier
//! fall into one conflict component (`:1069-1099`), and a component runs its intents one at a
//! time, in order (`:1004-1011`). The models assume less: the footprint `[2]‖sid` of a space job
//! (`reified/schedule.rs:84-97`). Under that footprint, the jobs of two spaces run at the same
//! time, and the jobs of one space run one at a time. The routing, ord and dispatch models also
//! let the operations on one space run at the same time, which is a weaker assumption still.
//!
//! | Test | Protocol | Properties |
//! |---|---|---|
//! | `loom_space_routing` | Two jobs of one space, a job of a second space and a host produce take the stripes of their channels. A job takes its stripes in phase A and then in phase B, each set sorted and deduplicated (`striped_locks.rs:41-82`). A host produce takes the stripe of its channel in phase A, and the stripes of its joins in phase B (`rspace/locks.rs:33-57`). | No deadlock. No two jobs update one space at the same time. Each space holds the updates of its own jobs only. |
//! | `loom_seq_guard` | Jobs of a `seq` space check their claim on a channel under the stripe of the space. The commit releases the stripe, and then the job stores its claim (`ephemeral.rs:251-289`, `generic.rs:2267-2270`). The footprint of the space keeps the check and the store together. | Each stored claim descends from the claim that it replaces. One of two sibling lineages uses the channel. A job of another space is not disturbed. |
//! | `loom_ord_assignment` | Operations on one queue read the counter, write the next value and commit their record in one transaction under the stripes of the collection (`counter.rs:165-245`, `generic.rs:644-647`). | Each operation gets a different `ord`. The order of the `ord` values is the commit order (FIFO). |
//! | `loom_handle_dispatch` | Calls are sends on the handle channel `H(sid)`, which a persistent receive serves. A fire on a stored call removes the call in its COMM under the stripe, and the body and the re-issued receive then run at the same time (`ops_consume.rs:124-143`, `reduce.rs:848-913`). | Each call is dispatched once, and its reply reaches its own return channel. The receive waits at the end, and no call stays stored. |
//!
//! Each property has a negative control: the same model with one protocol step changed. Loom
//! finds an interleaving that fails with the named message. The two routing controls detect the
//! deadlock without blocking (`hold_and_wait`), and the routing test checks that the same
//! detector finds no deadlock in the stripe orders of the protocol.
//!
//! | Negative control | Changed step | Failure |
//! |---|---|---|
//! | `loom_space_routing_unsorted_acquisition_deadlocks` | Two jobs of two spaces take their stripes in key order. | each job holds a stripe that the other one needs |
//! | `loom_space_routing_one_phase_escalation_deadlocks` | The host produce takes the stripes of its joins in phase A. | each job holds a stripe that the other one needs |
//! | `loom_seq_guard_without_footprint_breaks_descent` | The jobs do not hold the footprint of their space. | a stored claim that does not descend |
//! | `loom_ord_assignment_bump_without_stripe_duplicates_an_ord` | The operations bump the counter before they take the stripe. | two operations with one `ord` |
//! | `loom_handle_dispatch_removal_outside_the_comm_dispatches_twice` | The body removes the stored call, not the COMM. | a call dispatched twice |
//!
//! Every test explores the executions with at most 3 preemptions and at most 1,000 branches,
//! the bounds of measurement R8 of the P9 design. `LOOM_MAX_PREEMPTIONS` and
//! `LOOM_MAX_BRANCHES` set other bounds. Each positive test prints the number of executions that
//! loom explored.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize as StdAtomicUsize, Ordering as StdOrdering};
use std::sync::Arc as StdArc;

use loom::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex, MutexGuard};
use loom::thread;

/// The preemption bound of measurement R8. `LOOM_MAX_PREEMPTIONS` sets another bound.
const R8_PREEMPTIONS: usize = 3;

/// Checks `model` under loom with the bounds of R8: at most [`R8_PREEMPTIONS`] preemptions,
/// and the branch limit of loom (1,000, or `LOOM_MAX_BRANCHES`).
fn check(model: impl Fn() + Sync + Send + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.preemption_bound = builder.preemption_bound.or(Some(R8_PREEMPTIONS));
    builder.check(model);
}

/// Checks `model` as [`check`] does, prints the number of executions that loom explored (R8),
/// and returns it.
fn explore(name: &str, model: impl Fn() + Sync + Send + 'static) -> usize {
    let executions = StdArc::new(StdAtomicUsize::new(0));
    let counter = StdArc::clone(&executions);
    check(move || {
        counter.fetch_add(1, StdOrdering::Relaxed);
        model();
    });
    let explored = executions.load(StdOrdering::Relaxed);
    eprintln!("R8: {name}: {explored} executions");
    explored
}

// ─── Stripe acquisition (`loom_space_routing`)

/// The number of stripes in each phase. Production has 4,096 (`striped_locks.rs:16`). With
/// three stripes, the lock keys of two spaces and of a host channel collide.
const STRIPES: usize = 3;

/// The stripes of one RSpace: phase A and phase B (`rspace.rs:98-99`).
struct Stripes {
    phase_a: Vec<Mutex<()>>,
    phase_b: Vec<Mutex<()>>,
}

impl Stripes {
    fn new() -> Self {
        Self {
            phase_a: (0..STRIPES).map(|_| Mutex::new(())).collect(),
            phase_b: (0..STRIPES).map(|_| Mutex::new(())).collect(),
        }
    }

    fn stripe(&self, (phase, index): (Phase, usize)) -> &Mutex<()> {
        match phase {
            Phase::A => &self.phase_a[index],
            Phase::B => &self.phase_b[index],
        }
    }
}

/// A phase of the stripes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    A,
    B,
}

/// The order in which a job takes the stripes of one phase.
#[derive(Clone, Copy)]
enum Acquisition {
    /// The stripe indices sorted and deduplicated (`striped_locks.rs:47-49`, native
    /// `striped_locks/native.rs:48-55`).
    Sorted,
    /// The stripe indices deduplicated in the order of the keys: the negative control.
    KeyOrder,
}

/// The phase in which a host produce takes the stripes of its joins.
#[derive(Clone, Copy)]
enum Escalation {
    /// Phase B (`rspace/locks.rs:52`, `replay_rspace.rs:130`).
    PhaseB,
    /// Phase A, after the stripe of its own channel: the negative control.
    PhaseA,
}

/// The stripe indices of `keys` in one phase: `key % STRIPES`, deduplicated.
fn stripe_indices(keys: &[u64], acquisition: Acquisition) -> Vec<usize> {
    let mut indices = Vec::with_capacity(keys.len());
    for key in keys {
        let index = (*key as usize) % STRIPES;
        if !indices.contains(&index) {
            indices.push(index);
        }
    }
    match acquisition {
        Acquisition::Sorted => indices.sort_unstable(),
        Acquisition::KeyOrder => {}
    }
    indices
}

/// The stripes of a space job, in the order in which it takes them: every lock key in phase A,
/// and then every lock key in phase B (`striped_locks.rs:74-82`, native
/// `native_session/locks.rs:85-93`).
fn space_job_order(keys: &[u64], acquisition: Acquisition) -> Vec<(Phase, usize)> {
    let indices = stripe_indices(keys, acquisition);
    let mut order = Vec::with_capacity(2 * indices.len());
    order.extend(indices.iter().map(|index| (Phase::A, *index)));
    order.extend(indices.iter().map(|index| (Phase::B, *index)));
    order
}

/// The stripes of a host produce on `channel`, which has a join with the channels `join`, in
/// the order in which it takes them (`rspace/locks.rs:33-57`, `replay_rspace.rs:118-132`): the
/// stripe of its channel in phase A, and then the stripes of the join. The joins of the model
/// do not change, so the order is known before the first stripe.
fn host_produce_order(channel: u64, join: &[u64], escalation: Escalation) -> Vec<(Phase, usize)> {
    let own = stripe_indices(&[channel], Acquisition::Sorted);
    let joined = stripe_indices(join, Acquisition::Sorted);
    let mut order = Vec::with_capacity(own.len() + joined.len());
    order.extend(own.iter().map(|index| (Phase::A, *index)));
    match escalation {
        Escalation::PhaseB => order.extend(joined.iter().map(|index| (Phase::B, *index))),
        Escalation::PhaseA => order.extend(
            joined
                .iter()
                .filter(|index| !own.contains(index))
                .map(|index| (Phase::A, *index)),
        ),
    }
    order
}

/// Takes the stripes of `order`, in that order, and blocks on each (`striped_locks.rs:51-69`).
fn take_in_order<'a>(stripes: &'a Stripes, order: &[(Phase, usize)]) -> Vec<MutexGuard<'a, ()>> {
    order
        .iter()
        .map(|stripe| stripes.stripe(*stripe).lock().expect("a stripe lock"))
        .collect()
}

/// The stripes of a deadlock probe: one flag for each stripe, `true` while a job holds it. A
/// job takes a flag with a compare-and-swap and releases it with a store.
///
/// The probe uses atomics, not mutexes, for a technical reason. In loom 0.7.2, the release of a
/// mutex is not a scheduling point, and the partial-order reduction records no access for it.
/// So loom never runs the `try_lock` of one job between the failed `try_lock` of the other job
/// and its release, and the cycle below stays unexplored.
struct ProbeStripes {
    phase_a: [AtomicBool; STRIPES],
    phase_b: [AtomicBool; STRIPES],
}

impl ProbeStripes {
    fn new() -> Self {
        Self {
            phase_a: std::array::from_fn(|_| AtomicBool::new(false)),
            phase_b: std::array::from_fn(|_| AtomicBool::new(false)),
        }
    }

    fn flag(&self, (phase, index): (Phase, usize)) -> &AtomicBool {
        match phase {
            Phase::A => &self.phase_a[index],
            Phase::B => &self.phase_b[index],
        }
    }
}

/// Takes the probe stripe `flag` when it is free. Returns whether the job took it.
fn try_take(flag: &AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// One job of a deadlock probe. The job takes the first stripe of `order` and waits for it. It
/// then probes each later stripe once. When a probe finds its stripe taken, the job would wait
/// there with its stripes. So it raises its flag in `waiting`, reads the flag of the other job,
/// lowers its flag, and only then releases its stripes. Returns whether the other job was
/// waiting too. Every read of a flag is a read-modify-write, which reads the latest value.
fn probe_in_order(
    stripes: &ProbeStripes,
    order: &[(Phase, usize)],
    waiting: &[AtomicBool; 2],
    job: usize,
) -> bool {
    let mut held = Vec::with_capacity(order.len());
    let mut cycle = false;
    for (step, stripe) in order.iter().enumerate() {
        let flag = stripes.flag(*stripe);
        let taken = match step {
            0 => {
                while !try_take(flag) {
                    thread::yield_now();
                }
                true
            }
            _ => try_take(flag),
        };
        match taken {
            true => held.push(flag),
            false => {
                waiting[job].store(true, Ordering::SeqCst);
                cycle = waiting[1 - job].fetch_or(false, Ordering::SeqCst);
                waiting[job].store(false, Ordering::SeqCst);
                break;
            }
        }
    }
    for flag in held {
        flag.store(false, Ordering::SeqCst);
    }
    cycle
}

/// Whether the two jobs with the stripe orders `orders` can each hold a stripe that the other
/// one needs. A job that sees the flag of the other job is inside its own waiting window, and so
/// is the other job. Each one then holds the stripe whose probe failed for the other one,
/// because a job releases its stripes only after its window. With blocking locks, that
/// interleaving deadlocks.
///
/// The controls detect the cycle and do not block, for a technical reason. When loom reports a
/// deadlock while other threads hold loom guards, the drop of those guards panics again, and
/// the test process aborts.
fn hold_and_wait(orders: [Vec<(Phase, usize)>; 2]) -> bool {
    let stripes = Arc::new(ProbeStripes::new());
    let waiting = Arc::new([AtomicBool::new(false), AtomicBool::new(false)]);
    let mut job = 0;
    let [first, second] = orders.map(|order| {
        let stripes = Arc::clone(&stripes);
        let waiting = Arc::clone(&waiting);
        let index = job;
        job += 1;
        thread::spawn(move || probe_in_order(&stripes, &order, &waiting, index))
    });
    let first = first.join().expect("the first job");
    let second = second.join().expect("the second job");
    first || second
}

/// The state of one space or one host channel: the jobs that updated it, as bits, and the
/// number of jobs inside an update.
struct Region {
    inside: AtomicUsize,
    updates: AtomicUsize,
}

impl Region {
    fn new() -> Self {
        Self {
            inside: AtomicUsize::new(0),
            updates: AtomicUsize::new(0),
        }
    }

    /// One update by `job`. No other job may be inside an update of this region.
    fn update(&self, job: usize) {
        let others = self.inside.fetch_add(1, Ordering::SeqCst);
        assert_eq!(others, 0, "two jobs update one space at the same time");
        self.updates.fetch_or(1 << job, Ordering::SeqCst);
        self.inside.fetch_sub(1, Ordering::SeqCst);
    }

    fn updates(&self) -> usize { self.updates.load(Ordering::SeqCst) }
}

/// A job that takes the stripes of `order`, updates `region`, and releases the stripes. A
/// space job releases its stripes at its commit (`space_play.rs:327-338`, native
/// `native_session/space.rs:234-274`, `space_store_handle.rs:918-919`).
fn locked_update(stripes: &Stripes, order: &[(Phase, usize)], region: &Region, job: usize) {
    let held = take_in_order(stripes, order);
    region.update(job);
    drop(held);
}

/// The lock keys of the routing model. A key `k` maps to the stripe `k % 3`.
///
/// | Channel | Key | Stripe |
/// |---|---|---|
/// | `C(s1)`, the anchor of space 1 | 0 | 0 |
/// | `D(s1,k)`, a collection of space 1 | 5 | 2 |
/// | `C(s2)`, the anchor of space 2 | 2 | 2 |
/// | `D(s2,k)`, a collection of space 2 | 3 | 0 |
/// | `h`, a host channel | 8 | 2 |
/// | `g`, a host channel with a join on `h` | 6 | 0 |
///
/// So space 2 and the host produce meet the stripes of space 1 in the opposite key order.
const SPACE_1: [u64; 2] = [0, 5];
const SPACE_1_REVERSED: [u64; 2] = [5, 0];
const SPACE_2: [u64; 2] = [2, 3];
const HOST_CHANNEL: u64 = 8;
const HOST_JOIN: [u64; 2] = [8, 6];

const CAN_DEADLOCK: &str = "two jobs can each hold a stripe that the other one needs";

/// No deadlock, mutual exclusion on one space and the isolation of two spaces under the
/// two-phase stripe acquisition. The second model checks that the probes of the controls find
/// no cycle in the stripe orders of the protocol.
#[test]
fn loom_space_routing() {
    explore("loom_space_routing", || {
        let stripes = Arc::new(Stripes::new());
        let first = Arc::new(Region::new());
        let second = Arc::new(Region::new());
        let host = Arc::new(Region::new());
        let jobs = [
            (
                space_job_order(&SPACE_1, Acquisition::Sorted),
                Arc::clone(&first),
                0,
            ),
            (
                space_job_order(&SPACE_1_REVERSED, Acquisition::Sorted),
                Arc::clone(&first),
                1,
            ),
            (
                space_job_order(&SPACE_2, Acquisition::Sorted),
                Arc::clone(&second),
                2,
            ),
            (
                host_produce_order(HOST_CHANNEL, &HOST_JOIN, Escalation::PhaseB),
                Arc::clone(&host),
                3,
            ),
        ]
        .map(|(order, region, job)| {
            let stripes = Arc::clone(&stripes);
            thread::spawn(move || locked_update(&stripes, &order, &region, job))
        });
        for job in jobs {
            job.join().expect("a job");
        }
        assert_eq!(
            first.updates(),
            0b0011,
            "space 1 holds the updates of its two jobs"
        );
        assert_eq!(
            second.updates(),
            0b0100,
            "space 2 holds the update of its job"
        );
        assert_eq!(
            host.updates(),
            0b1000,
            "the host channel holds the update of the produce"
        );
    });
    explore("loom_space_routing (probes)", || {
        let two_spaces = hold_and_wait([
            space_job_order(&SPACE_1, Acquisition::Sorted),
            space_job_order(&SPACE_2, Acquisition::Sorted),
        ]);
        assert!(!two_spaces, "{CAN_DEADLOCK}: two spaces");
        let escalation = hold_and_wait([
            space_job_order(&SPACE_1, Acquisition::Sorted),
            host_produce_order(HOST_CHANNEL, &HOST_JOIN, Escalation::PhaseB),
        ]);
        assert!(!escalation, "{CAN_DEADLOCK}: a space and a host produce");
    });
}

/// Negative control: two jobs of two spaces that take their stripes in key order can each hold
/// a stripe that the other one needs.
#[test]
#[should_panic(expected = "two jobs can each hold a stripe that the other one needs")]
fn loom_space_routing_unsorted_acquisition_deadlocks() {
    check(|| {
        let cycle = hold_and_wait([
            space_job_order(&SPACE_1, Acquisition::KeyOrder),
            space_job_order(&SPACE_2, Acquisition::KeyOrder),
        ]);
        assert!(!cycle, "{CAN_DEADLOCK}: two spaces");
    });
}

/// Negative control: a host produce that takes the stripes of its joins in phase A can hold a
/// stripe that a space job needs while it needs a stripe of the space job.
#[test]
#[should_panic(expected = "two jobs can each hold a stripe that the other one needs")]
fn loom_space_routing_one_phase_escalation_deadlocks() {
    check(|| {
        let cycle = hold_and_wait([
            space_job_order(&SPACE_1, Acquisition::Sorted),
            host_produce_order(HOST_CHANNEL, &HOST_JOIN, Escalation::PhaseA),
        ]);
        assert!(!cycle, "{CAN_DEADLOCK}: a space and a host produce");
    });
}

// ─── The claims of a `seq` space (`loom_seq_guard`)

/// A participant path, one segment for each split (`PathSegment` in production).
type Path = Vec<u8>;

/// The claim book of a deploy (`VolatileRegistry`, `ephemeral.rs:76-95`): the claim of each
/// channel behind the lock of the book.
struct ClaimBook {
    claims: Mutex<BTreeMap<u8, Path>>,
}

impl ClaimBook {
    fn new() -> Self {
        Self {
            claims: Mutex::new(BTreeMap::new()),
        }
    }

    /// The claim of `channel` (`claim_of`, `ephemeral.rs:168-170`).
    fn claim_of(&self, channel: u8) -> Option<Path> {
        self.claims
            .lock()
            .expect("the claim book")
            .get(&channel)
            .cloned()
    }

    /// Sets the claim of `channel` to `path` (`store_claims`, `ephemeral.rs:173-178`). Returns
    /// whether `path` descends from the claim that it replaces.
    fn store(&self, channel: u8, path: &[u8]) -> bool {
        let replaced = self
            .claims
            .lock()
            .expect("the claim book")
            .insert(channel, path.to_vec());
        match replaced {
            None => true,
            Some(claim) => path.starts_with(&claim),
        }
    }
}

/// One `seq` space: the footprint key `[2]‖sid` of its jobs, which makes its jobs run one at a
/// time, and the stripe of its anchor `C(sid)`, which every job of the space takes
/// (`generic.rs:644-647`).
struct SeqSpace {
    footprint: Mutex<()>,
    stripe: Mutex<()>,
}

impl SeqSpace {
    fn new() -> Self {
        Self {
            footprint: Mutex::new(()),
            stripe: Mutex::new(()),
        }
    }
}

/// Whether a job holds the footprint of its space from its check to its store.
#[derive(Clone, Copy)]
enum Footprint {
    Held,
    /// The negative control.
    Absent,
}

/// The result of one job on a `seq` channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeqUse {
    /// The job committed. `descends` tells whether its stored claim descends from the claim
    /// that it replaced.
    Committed { descends: bool },
    /// The claim was not admitted: `SeqConcurrent`.
    Concurrent,
}

/// One program job that uses `channel` of `space` from the participant path `path`. It checks
/// its claim under the stripe of the space (`SeqClaims::claim`, `ephemeral.rs:251-284`): the
/// claim is admitted when the channel has no claim or its claim is a prefix of `path`
/// (`:277-278`). The commit releases the stripe (`space_store_handle.rs:918-919`), and then the
/// finish closure stores the claim (`SeqClaims::store`, `ephemeral.rs:289`;
/// `generic.rs:2267-2270`).
fn seq_job(
    space: &SeqSpace,
    book: &ClaimBook,
    channel: u8,
    path: &[u8],
    footprint: Footprint,
) -> SeqUse {
    let _component = match footprint {
        Footprint::Held => Some(space.footprint.lock().expect("the footprint of the space")),
        Footprint::Absent => None,
    };
    let admitted = {
        let _stripe = space.stripe.lock().expect("the stripe of the anchor");
        match book.claim_of(channel) {
            None => true,
            Some(claim) => path.starts_with(&claim),
        }
    };
    match admitted {
        true => SeqUse::Committed {
            descends: book.store(channel, path),
        },
        false => SeqUse::Concurrent,
    }
}

/// The channel `u` of the first `seq` space and the channel `v` of the second.
const CHANNEL_U: u8 = 1;
const CHANNEL_V: u8 = 2;

/// The results of the seq model: the lineage `[1]` then `[1, 0]` on `u`, the sibling `[2]` on
/// `u`, the job `[1]` on `v`, and the final claims of `u` and `v`.
struct SeqRun {
    lineage: [SeqUse; 2],
    sibling: SeqUse,
    other: SeqUse,
    claim_u: Option<Path>,
    claim_v: Option<Path>,
}

impl SeqRun {
    fn uses(&self) -> [SeqUse; 4] { [self.lineage[0], self.lineage[1], self.sibling, self.other] }
}

/// Runs the seq model once.
fn seq_run(footprint: Footprint) -> SeqRun {
    let book = Arc::new(ClaimBook::new());
    let first = Arc::new(SeqSpace::new());
    let second = Arc::new(SeqSpace::new());
    let lineage = {
        let (book, first) = (Arc::clone(&book), Arc::clone(&first));
        thread::spawn(move || {
            [
                seq_job(&first, &book, CHANNEL_U, &[1], footprint),
                seq_job(&first, &book, CHANNEL_U, &[1, 0], footprint),
            ]
        })
    };
    let sibling = {
        let (book, first) = (Arc::clone(&book), Arc::clone(&first));
        thread::spawn(move || seq_job(&first, &book, CHANNEL_U, &[2], footprint))
    };
    let other = {
        let (book, second) = (Arc::clone(&book), Arc::clone(&second));
        thread::spawn(move || seq_job(&second, &book, CHANNEL_V, &[1], footprint))
    };
    SeqRun {
        lineage: lineage.join().expect("the lineage"),
        sibling: sibling.join().expect("the sibling"),
        other: other.join().expect("the job of the other space"),
        claim_u: book.claim_of(CHANNEL_U),
        claim_v: book.claim_of(CHANNEL_V),
    }
}

const DOES_NOT_DESCEND: &str = "a stored claim does not descend from the claim that it replaces";

/// The claim rule with the check and the store under the footprint of the space.
#[test]
fn loom_seq_guard() {
    explore("loom_seq_guard", || {
        let run = seq_run(Footprint::Held);
        assert!(
            run.uses()
                .iter()
                .all(|used| *used != SeqUse::Committed { descends: false }),
            "{DOES_NOT_DESCEND}"
        );
        let committed = SeqUse::Committed { descends: true };
        match (run.lineage, run.sibling) {
            ([first, child], SeqUse::Concurrent) if first == committed && child == committed => {
                assert_eq!(run.claim_u, Some(vec![1, 0]), "the lineage holds u");
            }
            ([SeqUse::Concurrent, SeqUse::Concurrent], sibling) if sibling == committed => {
                assert_eq!(run.claim_u, Some(vec![2]), "the sibling holds u");
            }
            (lineage, sibling) => {
                panic!("both lineages use u: lineage {lineage:?}, sibling {sibling:?}")
            }
        }
        assert_eq!(run.other, committed, "the job of the other space commits");
        assert_eq!(
            run.claim_v,
            Some(vec![1]),
            "the other space holds its own claim"
        );
    });
}

/// Negative control: without the footprint, two siblings both pass the check before either
/// stores its claim, and the second store replaces a claim that is not its prefix.
#[test]
#[should_panic(expected = "a stored claim does not descend from the claim that it replaces")]
fn loom_seq_guard_without_footprint_breaks_descent() {
    check(|| {
        let run = seq_run(Footprint::Absent);
        assert!(
            run.uses()
                .iter()
                .all(|used| *used != SeqUse::Committed { descends: false }),
            "{DOES_NOT_DESCEND}"
        );
    });
}

// ─── The `ord` of an ordered collection (`loom_ord_assignment`)

/// A record of a queue: its `ord` and the job that enqueued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    ord: u64,
    job: usize,
}

/// One queue collection: the counter record `S(sid,k)`, and, behind the stripe of the lock keys
/// of the collection, its records in commit order (`generic.rs:644-647`).
struct Collection {
    counter: AtomicU64,
    records: Mutex<Vec<Record>>,
}

impl Collection {
    fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
            records: Mutex::new(Vec::with_capacity(2)),
        }
    }
}

/// Where an operation bumps the counter.
#[derive(Clone, Copy)]
enum Bump {
    /// In its transaction, under the stripes (`counter::bump`, `counter.rs:165-245`).
    UnderStripe,
    /// Before it takes the stripes: the negative control.
    BeforeStripe,
}

/// Reads the counter and writes the next value. The value read is the `ord` of the operation.
fn bump_counter(collection: &Collection) -> u64 {
    let ord = collection.counter.load(Ordering::SeqCst);
    collection.counter.store(ord + 1, Ordering::SeqCst);
    ord
}

/// One enqueue: it bumps the counter and commits its record with the `ord` of the bump. Returns
/// the `ord`.
fn enqueue(collection: &Collection, job: usize, bump: Bump) -> u64 {
    match bump {
        Bump::UnderStripe => {
            let mut records = collection
                .records
                .lock()
                .expect("the stripes of the collection");
            let ord = bump_counter(collection);
            records.push(Record { ord, job });
            ord
        }
        Bump::BeforeStripe => {
            let ord = bump_counter(collection);
            let mut records = collection
                .records
                .lock()
                .expect("the stripes of the collection");
            records.push(Record { ord, job });
            ord
        }
    }
}

/// One dequeue: it bumps the counter too (decision 31) and takes the head, the record with the
/// least `ord` (decision 11). Returns its `ord` and the record that it took.
fn dequeue(collection: &Collection, bump: Bump) -> (u64, Option<Record>) {
    let take_head = |records: &mut Vec<Record>| {
        let head = records
            .iter()
            .enumerate()
            .min_by_key(|(_, record)| record.ord)
            .map(|(index, _)| index);
        head.map(|index| records.remove(index))
    };
    match bump {
        Bump::UnderStripe => {
            let mut records = collection
                .records
                .lock()
                .expect("the stripes of the collection");
            let ord = bump_counter(collection);
            (ord, take_head(&mut records))
        }
        Bump::BeforeStripe => {
            let ord = bump_counter(collection);
            let mut records = collection
                .records
                .lock()
                .expect("the stripes of the collection");
            (ord, take_head(&mut records))
        }
    }
}

/// The results of the ord model: the `ord` of each operation, the record that the dequeue
/// took, the records left in commit order, and the commit order of the enqueues.
struct OrdRun {
    ords: [u64; 3],
    taken: Option<Record>,
    left: Vec<Record>,
}

/// Runs the ord model once: two enqueues and a dequeue on one queue.
fn ord_run(bump: Bump) -> OrdRun {
    let collection = Arc::new(Collection::new());
    let enqueues = [0, 1].map(|job| {
        let collection = Arc::clone(&collection);
        thread::spawn(move || enqueue(&collection, job, bump))
    });
    let dequeue = {
        let collection = Arc::clone(&collection);
        thread::spawn(move || dequeue(&collection, bump))
    };
    let [first, second] = enqueues.map(|enqueue| enqueue.join().expect("an enqueue"));
    let (dequeued, taken) = dequeue.join().expect("the dequeue");
    let left = collection.records.lock().expect("the records").clone();
    OrdRun {
        ords: [first, second, dequeued],
        taken,
        left,
    }
}

const SHARED_ORD: &str = "two operations of one collection share an ord";

/// The counter bump under the stripes gives distinct `ord` values in commit order.
#[test]
fn loom_ord_assignment() {
    explore("loom_ord_assignment", || {
        let run = ord_run(Bump::UnderStripe);
        let [first, second, third] = run.ords;
        assert!(
            first != second && first != third && second != third,
            "{SHARED_ORD}"
        );
        assert!(
            run.left.windows(2).all(|pair| pair[0].ord < pair[1].ord),
            "the records are not in commit order"
        );
        match run.taken {
            Some(taken) => {
                let enqueued_before = [first, second].into_iter().filter(|ord| *ord < third).min();
                assert_eq!(
                    Some(taken.ord),
                    enqueued_before,
                    "the dequeue takes the first committed record"
                );
                assert_eq!(run.left.len(), 1, "one record is left");
            }
            None => {
                assert!(
                    first > third && second > third,
                    "the dequeue finds the queue empty only before both enqueues"
                );
                assert_eq!(run.left.len(), 2, "both records are left");
            }
        }
    });
}

/// Negative control: two operations that bump the counter before they take the stripes can
/// read one value.
#[test]
#[should_panic(expected = "two operations of one collection share an ord")]
fn loom_ord_assignment_bump_without_stripe_duplicates_an_ord() {
    check(|| {
        let run = ord_run(Bump::BeforeStripe);
        let [first, second, third] = run.ords;
        assert!(
            first != second && first != third && second != third,
            "{SHARED_ORD}"
        );
    });
}

// ─── Handle calls (`loom_handle_dispatch`)

/// The number of calls of the dispatch model.
const CALLS: usize = 2;

/// A call of a handle method. Its id is also the index of its return channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Call(usize);

/// The handle channel `H(sid)` behind its stripe: the stored calls, and whether the persistent
/// handle receive waits on it.
struct HandleChannel {
    calls: Vec<Call>,
    waiting: bool,
}

/// A handle: its channel and the return channel of each call.
struct Handle {
    channel: Mutex<HandleChannel>,
    replies: Vec<Mutex<Vec<Call>>>,
}

impl Handle {
    /// A handle whose channel holds the calls `stored`, sent before the receive.
    fn new(stored: Vec<Call>) -> Self {
        Self {
            channel: Mutex::new(HandleChannel {
                calls: stored,
                waiting: false,
            }),
            replies: (0..CALLS)
                .map(|_| Mutex::new(Vec::with_capacity(1)))
                .collect(),
        }
    }
}

/// Where a fire on a stored call removes the call.
#[derive(Clone, Copy)]
enum Removal {
    /// In the COMM, under the stripe (`ops_consume.rs:138, 192-202`).
    InComm,
    /// In the body, after the stripe is released: the negative control.
    InBody,
}

/// The body of the handle receive: it forwards the call to `HANDLE_OP`, which replies on the
/// return channel of the call (I-2).
fn dispatch(handle: &Handle, call: Call) {
    handle.replies[call.0]
        .lock()
        .expect("a return channel")
        .push(call);
}

/// A call: a send on `H(sid)` (`ops_produce.rs:69-112`). When the receive waits, the COMM fires
/// the body at once and keeps the persistent receive (`ops_produce.rs:202-205`). Otherwise the
/// call is stored.
fn send_call(handle: &Handle, call: Call) {
    let fired = {
        let mut channel = handle.channel.lock().expect("the stripe of H(sid)");
        match channel.waiting {
            true => Some(call),
            false => {
                channel.calls.push(call);
                None
            }
        }
    };
    if let Some(call) = fired {
        dispatch(handle, call);
    }
}

/// The persistent receive on `H(sid)`. A fire on a stored call runs the body, and the
/// re-issued receive runs at the same time (`reduce.rs:848-913`). With no stored call, the
/// receive waits (`ops_consume.rs:144-152`). The receive fires at most `CALLS` times, which
/// bounds the threads of the negative control. With the removal in the COMM, the bound never
/// applies: each fire removes one of the `CALLS` calls.
fn handle_receive(handle: &Arc<Handle>, removal: Removal) {
    let mut bodies = Vec::with_capacity(CALLS);
    loop {
        let fired = {
            let mut channel = handle.channel.lock().expect("the stripe of H(sid)");
            match (channel.calls.first().copied(), removal) {
                (None, _) => {
                    channel.waiting = true;
                    None
                }
                (Some(_), _) if bodies.len() == CALLS => None,
                (Some(call), Removal::InComm) => {
                    channel.calls.remove(0);
                    Some(call)
                }
                (Some(call), Removal::InBody) => Some(call),
            }
        };
        let Some(call) = fired else {
            break;
        };
        let handle = Arc::clone(handle);
        bodies.push(thread::spawn(move || {
            if let Removal::InBody = removal {
                let mut channel = handle.channel.lock().expect("the stripe of H(sid)");
                if let Some(position) = channel.calls.iter().position(|stored| *stored == call) {
                    channel.calls.remove(position);
                }
            }
            dispatch(&handle, call);
        }));
    }
    for body in bodies {
        body.join().expect("a body");
    }
}

/// The results of the dispatch model: the replies on each return channel, and the state of
/// the handle channel at the end.
struct DispatchRun {
    replies: Vec<Vec<Call>>,
    waiting: bool,
    stored: Vec<Call>,
}

/// Runs the dispatch model once: call 0 is stored before the receive, and call 1 is sent at
/// the same time as the receive.
fn dispatch_run(removal: Removal) -> DispatchRun {
    let handle = Arc::new(Handle::new(vec![Call(0)]));
    let caller = {
        let handle = Arc::clone(&handle);
        thread::spawn(move || send_call(&handle, Call(1)))
    };
    let receive = {
        let handle = Arc::clone(&handle);
        thread::spawn(move || handle_receive(&handle, removal))
    };
    caller.join().expect("the caller");
    receive.join().expect("the receive");
    let channel = handle.channel.lock().expect("the stripe of H(sid)");
    DispatchRun {
        replies: handle
            .replies
            .iter()
            .map(|replies| replies.lock().expect("a return channel").clone())
            .collect(),
        waiting: channel.waiting,
        stored: channel.calls.clone(),
    }
}

const DISPATCHED_TWICE: &str = "a call is dispatched twice";

/// Each call is dispatched once, and its reply reaches its own return channel.
#[test]
fn loom_handle_dispatch() {
    explore("loom_handle_dispatch", || {
        let run = dispatch_run(Removal::InComm);
        assert!(
            run.replies.iter().all(|replies| replies.len() <= 1),
            "{DISPATCHED_TWICE}"
        );
        for (index, replies) in run.replies.iter().enumerate() {
            assert_eq!(
                replies.as_slice(),
                [Call(index)],
                "call {index} has one reply, on its own return channel"
            );
        }
        assert!(run.waiting, "the persistent receive waits at the end");
        assert!(run.stored.is_empty(), "no call stays stored");
    });
}

/// Negative control: a fire that leaves the stored call to its body lets the re-issued
/// receive fire on the same call.
#[test]
#[should_panic(expected = "a call is dispatched twice")]
fn loom_handle_dispatch_removal_outside_the_comm_dispatches_twice() {
    check(|| {
        let run = dispatch_run(Removal::InBody);
        assert!(
            run.replies.iter().all(|replies| replies.len() <= 1),
            "{DISPATCHED_TWICE}"
        );
    });
}
