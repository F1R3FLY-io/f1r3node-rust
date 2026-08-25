//! Preregistered lifecycle experiment for a secondary identity-order PathMap.
//!
//! The frozen control is the production arena-plus-descriptor-sort DATA egress.
//! The treatment maintains one per-channel `PathMap<Multiplicity>` whose
//! relative paths are complete canonical Datum identities. Fixture creation,
//! arm construction, and mandatory equivalence audits remain outside each
//! timed interval. Every measured mutation returns its fixture to the same
//! logical state.

use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rspace_plus_plus::rspace::data_index::canonical_egress_experiment::{
    CanonicalEgressFixture, IdentityOrderArm, IdentityOrderFixture, PrefixShape,
};

const SCHEMA: &str = "rspace-identity-order-index-v1";
const SEED: u64 = 0x4131_6028_2026_0825;
const WARMUPS: usize = 5;
const RETAINED_PAIRS: usize = 82;
const PROFILE_ITERATIONS: usize = 1_000;

#[derive(Clone, Copy)]
struct Case {
    distinct: usize,
    multiplicity: u64,
    shape: PrefixShape,
}

const CASES: [Case; 9] = [
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
        distinct: 4_096,
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
    Case {
        distinct: 256,
        multiplicity: 1,
        shape: PrefixShape::Opposed,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Egress,
    InsertRemove,
    CloneUpdate,
    RebuildEgress,
}

impl Operation {
    const ALL: [Self; 4] =
        [Self::Egress, Self::InsertRemove, Self::CloneUpdate, Self::RebuildEgress];

    const fn label(self) -> &'static str {
        match self {
            Self::Egress => "egress",
            Self::InsertRemove => "insert_remove",
            Self::CloneUpdate => "clone_update",
            Self::RebuildEgress => "rebuild_egress",
        }
    }
}

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

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
        record_deallocation(pointer, layout.size());
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, old, new_size) };
        if !replacement.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            DEALLOC_BYTES
                .fetch_add(u64::try_from(old.size()).unwrap_or(u64::MAX), Ordering::Relaxed);
        }
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

fn record_deallocation(pointer: *mut u8, bytes: usize) {
    if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
        DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        DEALLOC_BYTES.fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    logical: u64,
    output_len: usize,
    digest: String,
}

struct Observation {
    elapsed_ns: u128,
    allocations: u64,
    allocation_bytes: u64,
    deallocations: u64,
    deallocation_bytes: u64,
    outcome: Outcome,
}

fn main() {
    if env::var_os("RSPACE_IDENTITY_ORDER_PROFILE").is_some() {
        run_profile();
    } else {
        run_matrix();
    }
}

fn run_matrix() {
    let mut rng = StdRng::seed_from_u64(SEED);
    for case in CASES {
        let fixture = IdentityOrderFixture::new(case.distinct, case.multiplicity, case.shape);
        for operation in Operation::ALL {
            let authority = execute(&fixture, operation, IdentityOrderArm::ArenaSort);
            let treatment = execute(&fixture, operation, IdentityOrderArm::MaintainedPathMap);
            assert_eq!(treatment, authority, "lifecycle arms must be observationally equal");
            println!(
                "CHECK\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                fixture.shape().label(),
                fixture.distinct(),
                fixture.multiplicity(),
                fixture.logical_cardinality(),
                operation.label(),
                authority.logical,
                authority.output_len,
                authority.digest,
            );

            for warmup in 0..WARMUPS {
                let arms = if warmup % 2 == 0 {
                    IdentityOrderArm::ALL
                } else {
                    [IdentityOrderArm::MaintainedPathMap, IdentityOrderArm::ArenaSort]
                };
                for arm in arms {
                    black_box(execute(&fixture, operation, arm));
                }
            }

            for pair in 0..RETAINED_PAIRS {
                let control_first = rng.gen::<bool>();
                let order = if control_first { "AB" } else { "BA" };
                let arms = if control_first {
                    IdentityOrderArm::ALL
                } else {
                    [IdentityOrderArm::MaintainedPathMap, IdentityOrderArm::ArenaSort]
                };
                for arm in arms {
                    let observation = measure(&fixture, operation, arm);
                    assert_eq!(observation.outcome, authority);
                    println!(
                        "OBS\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        fixture.shape().label(),
                        fixture.distinct(),
                        fixture.multiplicity(),
                        fixture.logical_cardinality(),
                        operation.label(),
                        pair,
                        order,
                        arm.label(),
                        observation.elapsed_ns,
                        observation.allocations,
                        observation.allocation_bytes,
                        observation.deallocations,
                        observation.deallocation_bytes,
                        observation.outcome.output_len,
                        observation.outcome.digest,
                    );
                }
            }
            eprintln!(
                "measured {} distinct={} multiplicity={} operation={} ({} retained pairs)",
                fixture.shape().label(),
                fixture.distinct(),
                fixture.multiplicity(),
                operation.label(),
                RETAINED_PAIRS,
            );
        }
    }
}

fn execute(fixture: &IdentityOrderFixture, operation: Operation, arm: IdentityOrderArm) -> Outcome {
    match operation {
        Operation::Egress => {
            let outcome = fixture.run_egress(arm);
            Outcome {
                logical: outcome.datum_count,
                output_len: outcome.encoded_datums.len(),
                digest: outcome.digest_hex(),
            }
        }
        Operation::InsertRemove => {
            let mut mutation = fixture.mutation_fixture(arm);
            Outcome {
                logical: mutation.run_insert_remove_cycle(),
                output_len: 0,
                digest: String::new(),
            }
        }
        Operation::CloneUpdate => Outcome {
            logical: fixture.run_clone_update(arm),
            output_len: 0,
            digest: String::new(),
        },
        Operation::RebuildEgress => {
            let outcome = fixture.run_rebuild_egress(arm);
            Outcome {
                logical: outcome.datum_count,
                output_len: outcome.encoded_datums.len(),
                digest: outcome.digest_hex(),
            }
        }
    }
}

fn measure(
    fixture: &IdentityOrderFixture,
    operation: Operation,
    arm: IdentityOrderArm,
) -> Observation {
    // The mutation fixture represents already-resident state and is cloned
    // before the timer. CloneUpdate deliberately performs its clones inside
    // `execute`, because clone plus one CoW write is the operation under test.
    let mut mutation =
        (operation == Operation::InsertRemove).then(|| fixture.mutation_fixture(arm));
    reset_allocations();
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let outcome = if let Some(mutation) = mutation.as_mut() {
        Outcome {
            logical: mutation.run_insert_remove_cycle(),
            output_len: 0,
            digest: String::new(),
        }
    } else {
        execute(fixture, operation, arm)
    };
    let elapsed_ns = started.elapsed().as_nanos();
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    Observation {
        elapsed_ns,
        allocations: ALLOC_CALLS.load(Ordering::Relaxed),
        allocation_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
        deallocations: DEALLOC_CALLS.load(Ordering::Relaxed),
        deallocation_bytes: DEALLOC_BYTES.load(Ordering::Relaxed),
        outcome,
    }
}

fn reset_allocations() {
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    DEALLOC_CALLS.store(0, Ordering::Relaxed);
    DEALLOC_BYTES.store(0, Ordering::Relaxed);
}

fn run_profile() {
    let profile = env::var("RSPACE_IDENTITY_ORDER_PROFILE").expect("profile operation");
    let arm = match env::var("RSPACE_IDENTITY_ORDER_ARM")
        .unwrap_or_else(|_| "identity_pathmap".to_string())
        .as_str()
    {
        "arena_sort" => IdentityOrderArm::ArenaSort,
        "identity_pathmap" => IdentityOrderArm::MaintainedPathMap,
        other => panic!("unknown identity-order arm {other}"),
    };
    let distinct = env_usize("RSPACE_IDENTITY_ORDER_DISTINCT", 4_096);
    let multiplicity = env_u64("RSPACE_IDENTITY_ORDER_MULTIPLICITY", 1);
    let shape = match env::var("RSPACE_IDENTITY_ORDER_SHAPE")
        .unwrap_or_else(|_| "shared".to_string())
        .as_str()
    {
        "shared" => PrefixShape::Shared,
        "unshared" => PrefixShape::Unshared,
        "opposed" => PrefixShape::Opposed,
        other => panic!("unknown identity-order shape {other}"),
    };
    if profile == "retained" && arm == IdentityOrderArm::ArenaSort {
        black_box(CanonicalEgressFixture::new(distinct, multiplicity, shape));
        return;
    }
    let fixture = IdentityOrderFixture::new(distinct, multiplicity, shape);
    if profile == "retained" {
        black_box(fixture);
        return;
    }
    let operation = match profile.as_str() {
        "egress" => Operation::Egress,
        "insert_remove" => Operation::InsertRemove,
        "clone_update" => Operation::CloneUpdate,
        "rebuild_egress" => Operation::RebuildEgress,
        other => panic!("unknown identity-order profile operation {other}"),
    };
    let iterations = env_usize("RSPACE_IDENTITY_ORDER_ITERATIONS", PROFILE_ITERATIONS);
    for _ in 0..iterations {
        black_box(execute(&fixture, operation, arm));
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name).map_or(default, |value| value.parse().expect("valid usize environment value"))
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name).map_or(default, |value| value.parse().expect("valid u64 environment value"))
}
