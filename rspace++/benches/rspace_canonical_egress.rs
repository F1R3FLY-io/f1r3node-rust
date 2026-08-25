//! Paired release experiment for RSpace DATA checkpoint egress.
//!
//! Fixture construction and the mandatory byte-equivalence audit are outside
//! the timed interval. Retained observations randomize arm order within each
//! pair and count allocations only while the selected production/control path
//! executes. The sparse matrix isolates distinct cardinality, logical
//! multiplicity, and prefix compression without an unnecessary full cross
//! product.

use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rspace_plus_plus::rspace::data_index::canonical_egress_experiment::{
    CanonicalEgressArm, CanonicalEgressFixture, PrefixShape,
};

const SCHEMA: &str = "rspace-canonical-egress-v1";
const SEED: u64 = 0x4131_6026_2026_0824;
const WARMUPS: usize = 5;
const RETAINED_PAIRS: usize = 31;
const PROFILE_ITERATIONS: usize = 1_000;

#[derive(Clone, Copy)]
struct Case {
    distinct: usize,
    multiplicity: u64,
    shape: PrefixShape,
}

const CASES: [Case; 7] = [
    Case {
        distinct: 1,
        multiplicity: 1,
        shape: PrefixShape::Shared,
    },
    Case {
        distinct: 16,
        multiplicity: 1,
        shape: PrefixShape::Shared,
    },
    Case {
        distinct: 256,
        multiplicity: 1,
        shape: PrefixShape::Shared,
    },
    Case {
        distinct: 4_096,
        multiplicity: 1,
        shape: PrefixShape::Shared,
    },
    Case {
        distinct: 256,
        multiplicity: 1,
        shape: PrefixShape::Unshared,
    },
    Case {
        distinct: 16,
        multiplicity: 2,
        shape: PrefixShape::Shared,
    },
    Case {
        distinct: 16,
        multiplicity: 16,
        shape: PrefixShape::Shared,
    },
];

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

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

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, old, new_size) };
        record_allocation(replacement, new_size);
        replacement
    }
}

fn record_allocation(pointer: *mut u8, bytes: usize) {
    if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

struct Observation {
    pair: usize,
    order: &'static str,
    arm: CanonicalEgressArm,
    elapsed_ns: u128,
    allocations: u64,
    allocation_bytes: u64,
    output_len: usize,
    digest: String,
}

fn main() {
    if env::var_os("RSPACE_CANONICAL_EGRESS_PROFILE_ARM").is_some() {
        run_profile();
    } else {
        run_matrix();
    }
}

fn run_matrix() {
    let mut rng = StdRng::seed_from_u64(SEED);
    for case in CASES {
        let fixture = CanonicalEgressFixture::new(case.distinct, case.multiplicity, case.shape);
        let authority = fixture.run(CanonicalEgressArm::CanonicalArena);
        println!(
            "CHECK\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}",
            fixture.shape().label(),
            fixture.distinct(),
            fixture.multiplicity(),
            fixture.logical_cardinality(),
            authority.encoded_datums.len(),
            authority.digest_hex(),
        );

        for warmup in 0..WARMUPS {
            let order = if warmup % 2 == 0 {
                CanonicalEgressArm::ALL
            } else {
                [CanonicalEgressArm::CanonicalArena, CanonicalEgressArm::LegacyTyped]
            };
            for arm in order {
                black_box(fixture.run(arm));
            }
        }

        for pair in 0..RETAINED_PAIRS {
            let legacy_first = rng.gen::<bool>();
            let order = if legacy_first { "AB" } else { "BA" };
            let arms = if legacy_first {
                CanonicalEgressArm::ALL
            } else {
                [CanonicalEgressArm::CanonicalArena, CanonicalEgressArm::LegacyTyped]
            };
            for arm in arms {
                let observation = measure(&fixture, pair, order, arm);
                assert_eq!(observation.output_len, authority.encoded_datums.len());
                assert_eq!(observation.digest, authority.digest_hex());
                println!(
                    "OBS\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    fixture.shape().label(),
                    fixture.distinct(),
                    fixture.multiplicity(),
                    fixture.logical_cardinality(),
                    observation.pair,
                    observation.order,
                    observation.arm.label(),
                    observation.elapsed_ns,
                    observation.allocations,
                    observation.allocation_bytes,
                    observation.output_len,
                );
            }
        }
        eprintln!(
            "measured {} distinct={} multiplicity={} ({} retained pairs)",
            fixture.shape().label(),
            fixture.distinct(),
            fixture.multiplicity(),
            RETAINED_PAIRS,
        );
    }
}

fn measure(
    fixture: &CanonicalEgressFixture,
    pair: usize,
    order: &'static str,
    arm: CanonicalEgressArm,
) -> Observation {
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let outcome = fixture.run(arm);
    let elapsed_ns = started.elapsed().as_nanos();
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let observation = Observation {
        pair,
        order,
        arm,
        elapsed_ns,
        allocations: ALLOC_CALLS.load(Ordering::Relaxed),
        allocation_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
        output_len: outcome.encoded_datums.len(),
        digest: outcome.digest_hex(),
    };
    black_box(outcome);
    observation
}

fn run_profile() {
    let arm = match env::var("RSPACE_CANONICAL_EGRESS_PROFILE_ARM")
        .expect("profile arm")
        .as_str()
    {
        "legacy_typed" => CanonicalEgressArm::LegacyTyped,
        "canonical_arena" => CanonicalEgressArm::CanonicalArena,
        other => panic!("unknown profile arm {other}"),
    };
    let distinct = env_usize("RSPACE_CANONICAL_EGRESS_DISTINCT", 256);
    let multiplicity = env_u64("RSPACE_CANONICAL_EGRESS_MULTIPLICITY", 1);
    let shape = match env::var("RSPACE_CANONICAL_EGRESS_SHAPE")
        .unwrap_or_else(|_| "shared".to_string())
        .as_str()
    {
        "shared" => PrefixShape::Shared,
        "unshared" => PrefixShape::Unshared,
        other => panic!("unknown profile shape {other}"),
    };
    let iterations = env_usize("RSPACE_CANONICAL_EGRESS_ITERATIONS", PROFILE_ITERATIONS);
    let fixture = CanonicalEgressFixture::new(distinct, multiplicity, shape);
    for _ in 0..iterations {
        black_box(fixture.run(arm));
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name).map_or(default, |value| value.parse().expect("valid usize environment value"))
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name).map_or(default, |value| value.parse().expect("valid u64 environment value"))
}
