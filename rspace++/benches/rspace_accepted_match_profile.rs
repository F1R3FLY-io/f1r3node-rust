//! Fixed-run, headless profiler for accepted-match transitions.
//!
//! Each fixture is prepared before its individual operation timer and the
//! outcome is audited after instrumentation stops. This produces operation
//! p50/p95/p99 rather than quantiles of Criterion sample averages. The same
//! prebuilt executable is suitable for `perf`, `/usr/bin/time -v`, uProf, and
//! `heaptrack --record-only`; Heaptrack reports are rendered with
//! `heaptrack_print`, never a GUI.

use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use rspace_plus_plus::rspace::hot_store::accepted_match_transition_experiment::{
    AcceptedMatchTransitionExperiment, ExperimentArm, ExperimentCase, ExperimentOutcome,
    instrumentation_begin, instrumentation_end,
};

const SCHEMA: &str = "accepted-match-transition-v1";

struct CountingAllocator;

static ALLOCATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ALLOCATION_ACTIVE.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATION_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ALLOCATION_ACTIVE.load(Ordering::Relaxed) {
            DEALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            DEALLOCATION_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ALLOCATION_ACTIVE.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATION_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        if ALLOCATION_ACTIVE.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATION_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            DEALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            DEALLOCATION_BYTES.fetch_add(old.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, old, new_size) }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AllocationMeasurement {
    calls: u64,
    bytes: u64,
    deallocation_calls: u64,
    deallocation_bytes: u64,
}

fn allocation_begin() {
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    ALLOCATION_BYTES.store(0, Ordering::Relaxed);
    DEALLOCATION_CALLS.store(0, Ordering::Relaxed);
    DEALLOCATION_BYTES.store(0, Ordering::Relaxed);
    assert!(!ALLOCATION_ACTIVE.swap(true, Ordering::SeqCst));
}

fn allocation_end() -> AllocationMeasurement {
    assert!(ALLOCATION_ACTIVE.swap(false, Ordering::SeqCst));
    AllocationMeasurement {
        calls: ALLOCATION_CALLS.load(Ordering::Relaxed),
        bytes: ALLOCATION_BYTES.load(Ordering::Relaxed),
        deallocation_calls: DEALLOCATION_CALLS.load(Ordering::Relaxed),
        deallocation_bytes: DEALLOCATION_BYTES.load(Ordering::Relaxed),
    }
}

fn main() {
    let case = parse_case(&required("ACCEPTED_MATCH_PROFILE_CASE"));
    let arm = parse_arm(&required("ACCEPTED_MATCH_PROFILE_ARM"));
    assert!(case.supports(arm), "stale split control has no valid semantics");
    let iterations = env::var("ACCEPTED_MATCH_PROFILE_ITERATIONS")
        .unwrap_or_else(|_| "10000".to_owned())
        .parse::<usize>()
        .expect("ACCEPTED_MATCH_PROFILE_ITERATIONS must be a positive usize");
    assert!(iterations > 0);

    let audit = AcceptedMatchTransitionExperiment::audit(case);
    let expected = audit.atomic;
    let expected_status = audit.atomic_status;
    let mut durations = Vec::with_capacity(iterations);
    let mut allocation = AllocationMeasurement::default();
    let mut write_locks = 0_u64;
    let mut write_wait_ns = 0_u64;
    let mut write_hold_ns = 0_u64;
    let mut root_publications = 0_u64;
    let mut last_outcome: Option<ExperimentOutcome> = None;

    for _ in 0..iterations {
        let mut fixture = AcceptedMatchTransitionExperiment::new(case);
        allocation_begin();
        instrumentation_begin();
        let started = Instant::now();
        let status = black_box(fixture.execute(black_box(arm)));
        let elapsed = started.elapsed();
        let transition = instrumentation_end();
        let measured_allocation = allocation_end();
        assert_eq!(status, expected_status);
        let outcome = fixture.outcome();
        assert_eq!(outcome, expected);
        last_outcome = Some(outcome);
        durations.push(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
        allocation.calls = allocation.calls.saturating_add(measured_allocation.calls);
        allocation.bytes = allocation.bytes.saturating_add(measured_allocation.bytes);
        allocation.deallocation_calls = allocation
            .deallocation_calls
            .saturating_add(measured_allocation.deallocation_calls);
        allocation.deallocation_bytes = allocation
            .deallocation_bytes
            .saturating_add(measured_allocation.deallocation_bytes);
        write_locks = write_locks.saturating_add(transition.write_locks);
        write_wait_ns = write_wait_ns.saturating_add(transition.write_wait_ns);
        write_hold_ns = write_hold_ns.saturating_add(transition.write_hold_ns);
        root_publications = root_publications.saturating_add(transition.pathmap_root_publications);
    }

    durations.sort_unstable();
    let sum = durations
        .iter()
        .map(|&value| u128::from(value))
        .sum::<u128>();
    let mean = sum / iterations as u128;
    let outcome = last_outcome.expect("a positive iteration count produces an outcome");
    println!(
        concat!(
            "PROFILE\t{}\tcase={}\tarm={}\titerations={}\tstatus={}",
            "\tp50_ns={}\tp95_ns={}\tp99_ns={}\tmean_ns={}",
            "\tallocation_calls={}\tallocation_bytes={}",
            "\tdeallocation_calls={}\tdeallocation_bytes={}",
            "\twrite_locks={}\twrite_wait_ns={}\twrite_hold_ns={}",
            "\tpathmap_root_publications={}\tdigest={}"
        ),
        SCHEMA,
        case.label(),
        arm.label(),
        iterations,
        expected_status.label(),
        quantile(&durations, 50),
        quantile(&durations, 95),
        quantile(&durations, 99),
        mean,
        allocation.calls,
        allocation.bytes,
        allocation.deallocation_calls,
        allocation.deallocation_bytes,
        write_locks,
        write_wait_ns,
        write_hold_ns,
        root_publications,
        outcome.digest_hex(),
    );
}

fn quantile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = percentile.saturating_mul(sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn parse_arm(value: &str) -> ExperimentArm {
    match value {
        "frozen_split" => ExperimentArm::FrozenSplit,
        "atomic" => ExperimentArm::Atomic,
        _ => panic!("unknown accepted-match arm {value:?}"),
    }
}

fn parse_case(value: &str) -> ExperimentCase {
    ExperimentCase::ALL
        .into_iter()
        .find(|case| case.label() == value)
        .unwrap_or_else(|| panic!("unknown accepted-match case {value:?}"))
}
