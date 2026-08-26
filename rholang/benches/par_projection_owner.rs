//! End-to-end repeated-owner protobuf projection experiment.
//!
//! Each observation includes explicit preparation (when selected), complete
//! outer `SoftCheckpointProto` encoding, the four-byte FFI prefix, and output
//! allocation. Fixture construction and the independent expected frame are
//! outside the timed interval. Every arm receives a fresh, byte-equivalent
//! fixture so preparation state cannot leak between observations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par};
use models::rspace_plus_plus_types::{
    DatumProto, HotStoreStateProto, JoinProto, SoftCheckpointProto, StoreStateContMapEntry,
    StoreStateDataMapEntry, StoreStateJoinsMapEntry, WaitingContinuationProto,
};
use models::rust::par_bag::ParBag;
use rholang::rust::protobuf_egress::{
    encode_frame, try_visit_soft_checkpoint_par_slots, ProjectionPreparationBudget,
    ProjectionPreparationStatistics,
};

const WIDTHS: [usize; 4] = [1, 16, 256, 4_096];
const BODY_BYTES: [usize; 3] = [16, 256, 4_096];
const REUSE_COUNTS: [usize; 5] = [1, 2, 4, 8, 16];
const DEFAULT_REPS: usize = 31;
const DEFAULT_WARMUP: usize = 5;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if !replacement.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        replacement
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Adaptive,
    Selective,
    Blanket,
    SelectiveTooSmall,
}

impl Arm {
    const ALL: [Self; 4] = [
        Self::Adaptive,
        Self::Selective,
        Self::Blanket,
        Self::SelectiveTooSmall,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::Selective => "selective",
            Self::Blanket => "blanket",
            Self::SelectiveTooSmall => "selective_too_small",
        }
    }
}

struct Fixture {
    message: SoftCheckpointProto,
    expected: Vec<u8>,
    projected_bytes: usize,
    reuse: usize,
}

impl Fixture {
    fn new(width: usize, body_bytes: usize, reuse: usize) -> Self {
        assert!(WIDTHS.contains(&width), "width must be preregistered");
        assert!(
            BODY_BYTES.contains(&body_bytes),
            "body size must be preregistered"
        );
        assert!(
            REUSE_COUNTS.contains(&reuse),
            "reuse count must be preregistered"
        );

        let mut components = ParBag::new();
        for ordinal in 0..width {
            let mut bytes = vec![0x51; body_bytes];
            let ordinal_bytes = (ordinal as u64).to_le_bytes();
            let suffix = bytes.len() - ordinal_bytes.len();
            bytes[suffix..].copy_from_slice(&ordinal_bytes);
            components
                .insert(&Expr {
                    expr_instance: Some(ExprInstance::GByteArray(bytes)),
                })
                .expect("distinct benchmark components fit u64 multiplicity");
        }
        let shared = Par::from_parts(components, vec![0b1010_0101], true);
        let projected_bytes = shared
            .components()
            .protobuf_components_encoded_len()
            .expect("benchmark component projection length is representable");

        let mut continuation_entry = StoreStateContMapEntry {
            key: Vec::new(),
            value: vec![WaitingContinuationProto {
                patterns: vec![BindPattern::default()],
                ..WaitingContinuationProto::default()
            }],
        };
        let mut data_entry = StoreStateDataMapEntry {
            key: None,
            value: vec![DatumProto {
                a: Some(ListParWithRandom::default()),
                ..DatumProto::default()
            }],
        };
        let mut join_entry = StoreStateJoinsMapEntry {
            key: None,
            value: vec![JoinProto::default()],
        };

        for occurrence in 0..reuse {
            match occurrence % 4 {
                0 => continuation_entry.key.push(shared.clone()),
                1 => continuation_entry.value[0].patterns[0]
                    .patterns
                    .push(shared.clone()),
                2 => data_entry.value[0]
                    .a
                    .as_mut()
                    .expect("fixture datum body")
                    .pars
                    .push(shared.clone()),
                3 => join_entry.value[0].join.push(shared.clone()),
                _ => unreachable!(),
            }
        }

        let message = SoftCheckpointProto {
            cache_snapshot: Some(HotStoreStateProto {
                continuations: vec![continuation_entry],
                data: vec![data_entry],
                joins: vec![join_entry],
                ..HotStoreStateProto::default()
            }),
            ..SoftCheckpointProto::default()
        };
        let expected = encode_frame(&message).expect("benchmark frame is representable");
        let anchor = message
            .cache_snapshot
            .as_ref()
            .expect("fixture snapshot")
            .continuations[0]
            .key
            .first()
            .expect("reuse >= 1 gives an anchor");
        let mut slots = 0usize;
        try_visit_soft_checkpoint_par_slots(&message, |value| {
            assert!(value.components().shares_storage_with(anchor.components()));
            slots += 1;
            Ok::<_, std::convert::Infallible>(())
        })
        .unwrap();
        assert_eq!(slots, reuse, "outer fixture reuse count is exact");

        Self {
            message,
            expected,
            projected_bytes,
            reuse,
        }
    }

    fn anchor(&self) -> &Par {
        self.message
            .cache_snapshot
            .as_ref()
            .expect("fixture snapshot")
            .continuations[0]
            .key
            .first()
            .expect("fixture anchor")
    }
}

struct Observation {
    elapsed_ns: u128,
    allocations: u64,
    allocated_bytes: u64,
    pathmap_traversals: u64,
    statistics: ProjectionPreparationStatistics,
    remaining_budget: usize,
}

fn measure(fixture: &Fixture, arm: Arm) -> Observation {
    let mut budget = match arm {
        Arm::Adaptive => ProjectionPreparationBudget::new(0, 0),
        Arm::Selective | Arm::Blanket => {
            ProjectionPreparationBudget::new(fixture.projected_bytes, fixture.projected_bytes)
        }
        Arm::SelectiveTooSmall => {
            ProjectionPreparationBudget::new(fixture.projected_bytes, fixture.projected_bytes - 1)
        }
    };

    ParBag::reset_protobuf_pathmap_traversals_experiment();
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    let started = Instant::now();
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);

    match arm {
        Arm::Adaptive => {}
        Arm::Selective | Arm::SelectiveTooSmall => {
            budget
                .prepare(black_box(fixture.anchor()))
                .expect("benchmark preparation length is representable");
        }
        Arm::Blanket => {
            try_visit_soft_checkpoint_par_slots(&fixture.message, |value| {
                budget.prepare(black_box(value)).map(|_| ())
            })
            .expect("benchmark preparation length is representable");
        }
    }
    let output = encode_frame(black_box(&fixture.message)).expect("benchmark frame encodes");

    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let elapsed_ns = started.elapsed().as_nanos();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    assert_eq!(output, fixture.expected);
    let statistics = budget.statistics();
    match arm {
        Arm::Adaptive => {
            assert_eq!(statistics.attempts, 0);
            assert_eq!(
                ParBag::take_protobuf_pathmap_traversals_experiment(),
                fixture.reuse as u64
            );
            return Observation {
                elapsed_ns,
                allocations: ALLOCATIONS.load(Ordering::Relaxed),
                allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
                pathmap_traversals: fixture.reuse as u64,
                statistics,
                remaining_budget: budget.remaining_new_retention_bytes(),
            };
        }
        Arm::Selective => {
            assert_eq!(statistics.prepared, 1);
            assert_eq!(statistics.refused_too_large, 0);
        }
        Arm::Blanket => {
            assert_eq!(statistics.prepared, 1);
            assert_eq!(statistics.already_prepared, fixture.reuse - 1);
        }
        Arm::SelectiveTooSmall => {
            assert_eq!(statistics.prepared, 0);
            assert_eq!(statistics.refused_too_large, 1);
        }
    }
    let pathmap_traversals = ParBag::take_protobuf_pathmap_traversals_experiment();
    let expected_traversals = match arm {
        Arm::Selective | Arm::Blanket => 1,
        Arm::SelectiveTooSmall => fixture.reuse as u64,
        Arm::Adaptive => unreachable!(),
    };
    assert_eq!(pathmap_traversals, expected_traversals);
    Observation {
        elapsed_ns,
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        pathmap_traversals,
        statistics,
        remaining_budget: budget.remaining_new_retention_bytes(),
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a usize: {error}"))
        })
        .unwrap_or(default)
}

fn selected_values(name: &str, values: &[usize], default: usize) -> Vec<usize> {
    match std::env::var(name) {
        Ok(value) => {
            let selected = value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a usize: {error}"));
            assert!(values.contains(&selected), "{name} is not preregistered");
            vec![selected]
        }
        Err(std::env::VarError::NotPresent) => vec![default],
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn selected_or_all(name: &str, values: &[usize]) -> Vec<usize> {
    match std::env::var(name) {
        Ok(value) => {
            let selected = value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a usize: {error}"));
            assert!(values.contains(&selected), "{name} is not preregistered");
            vec![selected]
        }
        Err(std::env::VarError::NotPresent) => values.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn main() {
    let reps = env_usize("PAR_OWNER_REPS", DEFAULT_REPS);
    let warmup = env_usize("PAR_OWNER_WARMUP", DEFAULT_WARMUP);
    assert!(reps != 0, "PAR_OWNER_REPS must be nonzero");
    println!(
        "META\tformat=par-projection-owner-v1\treps={reps}\twarmup={warmup}\tordering=balanced-four-arm-rotation\ttimed=prepare-plus-complete-frame\tfixture=soft-checkpoint-shared-aux"
    );
    println!(
        "HEADER\twidth\tbody_bytes\treuse\toutput_bytes\tprojected_bytes\tarm\tsample\tposition\telapsed_ns\tallocations\tallocated_bytes\tpathmap_traversals\tattempts\tprepared\talready_prepared\trefused_too_large\tnewly_retained_bytes\tremaining_budget"
    );
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        println!("LOADAVG\t{}", loadavg.trim());
    }

    for width in selected_values("PAR_OWNER_WIDTH", &WIDTHS, 16) {
        for body_bytes in selected_values("PAR_OWNER_BODY_BYTES", &BODY_BYTES, 256) {
            for reuse in selected_or_all("PAR_OWNER_REUSE", &REUSE_COUNTS) {
                for repetition in 0..(warmup + reps) {
                    for position in 0..Arm::ALL.len() {
                        let arm = Arm::ALL[(repetition + position) % Arm::ALL.len()];
                        let fixture = Fixture::new(width, body_bytes, reuse);
                        let observation = measure(&fixture, arm);
                        if repetition >= warmup {
                            let statistics = observation.statistics;
                            println!(
                                "OBS\t{width}\t{body_bytes}\t{reuse}\t{}\t{}\t{}\t{}\t{position}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                fixture.expected.len(),
                                fixture.projected_bytes,
                                arm.label(),
                                repetition - warmup,
                                observation.elapsed_ns,
                                observation.allocations,
                                observation.allocated_bytes,
                                observation.pathmap_traversals,
                                statistics.attempts,
                                statistics.prepared,
                                statistics.already_prepared,
                                statistics.refused_too_large,
                                statistics.newly_retained_bytes,
                                observation.remaining_budget,
                            );
                        }
                    }
                }
            }
        }
    }
}
