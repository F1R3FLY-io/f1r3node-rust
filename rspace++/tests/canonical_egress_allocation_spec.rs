#![cfg(feature = "canonical-egress-experiment")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rspace_plus_plus::rspace::data_index::canonical_egress_experiment::{
    CanonicalEgressArm, CanonicalEgressFixture, PrefixShape,
};

const RECORD_COUNTS: [usize; 3] = [16, 256, 4_096];
const MAX_CALLS_EXPONENT: f64 = 0.5;
const MAX_BYTES_EXPONENT: f64 = 1.2;

#[derive(Clone, Copy, Debug, Default)]
struct Allocations {
    calls: u64,
    bytes: u64,
}

thread_local! {
    static TALLY: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct CountingAllocator;

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record_allocation(pointer, layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record_allocation(pointer, layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        record_allocation(replacement, new_size);
        replacement
    }
}

fn record_allocation(pointer: *mut u8, bytes: usize) {
    if pointer.is_null() {
        return;
    }
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let _ = TALLY.try_with(|tally| {
        if let Some(current) = tally.get() {
            tally.set(Some(Allocations {
                calls: current.calls.saturating_add(1),
                bytes: current.bytes.saturating_add(bytes),
            }));
        }
    });
}

fn counted<T>(work: impl FnOnce() -> T) -> (T, Allocations) {
    TALLY.with(|tally| tally.set(Some(Allocations::default())));
    let value = work();
    let allocations = TALLY
        .with(|tally| tally.replace(None))
        .expect("the tally counts while the work runs");
    (value, allocations)
}

#[derive(Debug)]
struct Measurement {
    records: usize,
    output_bytes: usize,
    allocations: Allocations,
}

fn measure(arm: CanonicalEgressArm) -> [Measurement; 3] {
    RECORD_COUNTS.map(|records| {
        let fixture = CanonicalEgressFixture::new(records, 1, PrefixShape::Shared);
        let (outcome, allocations) = counted(|| fixture.run(arm));
        assert_eq!(
            outcome.datum_count,
            u64::try_from(records).expect("a record count fits u64"),
            "the {} arm egresses every record",
            arm.label()
        );
        assert!(
            allocations.calls > 0,
            "the counting allocator counted no allocation of the {} arm at {records} records",
            arm.label()
        );
        Measurement {
            records,
            output_bytes: outcome.encoded_persisted_data.len(),
            allocations,
        }
    })
}

fn log_log_exponent(points: [(f64, f64); 3]) -> f64 {
    let logs = points.map(|(x, y)| (x.ln(), y.ln()));
    let count = logs.len() as f64;
    let mean_x = logs.iter().map(|(x, _)| x).sum::<f64>() / count;
    let mean_y = logs.iter().map(|(_, y)| y).sum::<f64>() / count;
    let covariance: f64 = logs.iter().map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let variance: f64 = logs.iter().map(|(x, _)| (x - mean_x).powi(2)).sum();
    covariance / variance
}

fn calls_exponent(measurements: &[Measurement; 3]) -> f64 {
    log_log_exponent(
        measurements
            .each_ref()
            .map(|measurement| (measurement.records as f64, measurement.allocations.calls as f64)),
    )
}

fn bytes_exponent(measurements: &[Measurement; 3]) -> f64 {
    log_log_exponent(
        measurements.each_ref().map(|measurement| {
            (measurement.output_bytes as f64, measurement.allocations.bytes as f64)
        }),
    )
}

#[test]
fn canonical_arena_egress_meets_both_allocation_rules() {
    let measurements = measure(CanonicalEgressArm::CanonicalArena);
    let calls = calls_exponent(&measurements);
    let bytes = bytes_exponent(&measurements);
    println!(
        "canonical_arena calls exponent {calls:.3}, bytes exponent {bytes:.3}: {measurements:?}"
    );
    assert!(
        calls <= MAX_CALLS_EXPONENT,
        "rule 1 (no per-record materialization): the allocation calls grow with exponent \
         {calls:.3} against the record count, above {MAX_CALLS_EXPONENT}: {measurements:?}"
    );
    assert!(
        bytes <= MAX_BYTES_EXPONENT,
        "rule 2 (linear bytes): the allocated bytes grow with exponent {bytes:.3} against the \
         output bytes, above {MAX_BYTES_EXPONENT}: {measurements:?}"
    );
}

#[test]
fn legacy_typed_egress_violates_rule_one() {
    let measurements = measure(CanonicalEgressArm::LegacyTyped);
    let calls = calls_exponent(&measurements);
    println!("legacy_typed calls exponent {calls:.3}: {measurements:?}");
    assert!(
        calls > MAX_CALLS_EXPONENT,
        "the legacy typed arm reconstructs a typed Datum per record, yet its allocation calls \
         grow with exponent {calls:.3}, within the bound {MAX_CALLS_EXPONENT} of rule 1: \
         {measurements:?}"
    );
}
