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
    encode_frame, encode_frame_native, encode_frame_native_with_statistics,
    try_visit_soft_checkpoint_par_slots, FrameMemoStatistics, ProjectionPreparationBudget,
    ProjectionPreparationStatistics,
};

const WIDTHS: [usize; 4] = [1, 16, 256, 4_096];
const BODY_BYTES: [usize; 3] = [16, 256, 4_096];
const REUSE_COUNTS: [usize; 5] = [1, 2, 4, 8, 16];
const DEFAULT_REPS: usize = 31;
const DEFAULT_WARMUP: usize = 5;
/// Set while compiling the preregistered production gate. Because this is a
/// compile-time constant, the diagnostic statistics monomorphization and the
/// four persistent-projection controls are unreachable and linker-dead in that
/// binary; the gate therefore measures the exact production code layout.
const PRODUCTION_GATE: bool = option_env!("PAR_OWNER_PRODUCTION_GATE").is_some();

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
    InFrameProduction,
    InFrameMemo,
    Selective,
    Blanket,
    SelectiveTooSmall,
}

impl Arm {
    const ALL: [Self; 6] = [
        Self::Adaptive,
        Self::InFrameProduction,
        Self::InFrameMemo,
        Self::Selective,
        Self::Blanket,
        Self::SelectiveTooSmall,
    ];
    const PRODUCTION: [Self; 2] = [Self::Adaptive, Self::InFrameProduction];

    const fn label(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::InFrameProduction => "in_frame_production",
            Self::InFrameMemo => "in_frame_memo",
            Self::Selective => "selective",
            Self::Blanket => "blanket",
            Self::SelectiveTooSmall => "selective_too_small",
        }
    }

    fn parse(label: &str) -> Option<Self> { Self::ALL.into_iter().find(|arm| arm.label() == label) }
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
    preparation: ProjectionPreparationStatistics,
    frame_memo: FrameMemoStatistics,
    remaining_budget: usize,
}

fn measure(fixture: &Fixture, arm: Arm) -> Observation {
    let mut budget = match arm {
        Arm::Adaptive | Arm::InFrameProduction | Arm::InFrameMemo => {
            ProjectionPreparationBudget::new(0, 0)
        }
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
        Arm::Adaptive | Arm::InFrameProduction | Arm::InFrameMemo => {}
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
    let (output, frame_memo) = match arm {
        Arm::InFrameProduction => (
            encode_frame_native(black_box(&fixture.message))
                .expect("production native benchmark frame encodes"),
            None,
        ),
        Arm::InFrameMemo => {
            let (output, statistics) =
                encode_frame_native_with_statistics(black_box(&fixture.message))
                    .expect("native benchmark frame encodes");
            (output, Some(statistics))
        }
        Arm::Adaptive | Arm::Selective | Arm::Blanket | Arm::SelectiveTooSmall => (
            encode_frame(black_box(&fixture.message)).expect("benchmark frame encodes"),
            None,
        ),
    };

    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let elapsed_ns = started.elapsed().as_nanos();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    assert_eq!(output, fixture.expected);
    let preparation = budget.statistics();
    let frame_memo = frame_memo.unwrap_or_default();
    match arm {
        Arm::Adaptive => {
            assert_eq!(preparation.attempts, 0);
            assert_eq!(
                ParBag::take_protobuf_pathmap_traversals_experiment(),
                fixture.reuse as u64
            );
            return Observation {
                elapsed_ns,
                allocations: ALLOCATIONS.load(Ordering::Relaxed),
                allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
                pathmap_traversals: fixture.reuse as u64,
                preparation,
                frame_memo,
                remaining_budget: budget.remaining_new_retention_bytes(),
            };
        }
        Arm::InFrameMemo => {
            assert_eq!(preparation, ProjectionPreparationStatistics::default());
            assert_eq!(frame_memo.census.unique_owners, 1);
            assert_eq!(
                frame_memo.census.repeated_owners,
                usize::from(fixture.reuse > 1)
            );
            assert_eq!(
                frame_memo.direct_only_occurrences,
                usize::from(fixture.reuse == 1)
            );
            assert_eq!(
                frame_memo.range_recorded_owners,
                usize::from(fixture.reuse > 1)
            );
            assert_eq!(frame_memo.range_copied_occurrences, fixture.reuse - 1);
            assert!(!frame_memo.memo_owner_spilled);
            assert_eq!(
                frame_memo.direct_only_component_bytes,
                fixture.projected_bytes * usize::from(fixture.reuse == 1)
            );
            assert_eq!(
                frame_memo.range_recorded_component_bytes,
                fixture.projected_bytes * usize::from(fixture.reuse > 1)
            );
            assert_eq!(
                frame_memo.range_copied_component_bytes,
                fixture
                    .projected_bytes
                    .checked_mul(fixture.reuse - 1)
                    .expect("preregistered fixture copy count fits usize")
            );
        }
        Arm::InFrameProduction => {
            assert_eq!(preparation, ProjectionPreparationStatistics::default());
            assert_eq!(frame_memo, FrameMemoStatistics::default());
        }
        Arm::Selective => {
            assert_eq!(preparation.prepared, 1);
            assert_eq!(preparation.refused_too_large, 0);
        }
        Arm::Blanket => {
            assert_eq!(preparation.prepared, 1);
            assert_eq!(preparation.already_prepared, fixture.reuse - 1);
        }
        Arm::SelectiveTooSmall => {
            assert_eq!(preparation.prepared, 0);
            assert_eq!(preparation.refused_too_large, 1);
        }
    }
    let pathmap_traversals = ParBag::take_protobuf_pathmap_traversals_experiment();
    let expected_traversals = match arm {
        Arm::InFrameProduction | Arm::InFrameMemo | Arm::Selective | Arm::Blanket => 1,
        Arm::SelectiveTooSmall => fixture.reuse as u64,
        Arm::Adaptive => unreachable!(),
    };
    assert_eq!(pathmap_traversals, expected_traversals);
    Observation {
        elapsed_ns,
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        pathmap_traversals,
        preparation,
        frame_memo,
        remaining_budget: budget.remaining_new_retention_bytes(),
    }
}

/// Minimal timing wrapper for the compile-time production gate.
///
/// It deliberately cannot call the diagnostic statistics API or any retained-
/// projection control. Both arms still cover the complete private FFI frame,
/// allocation counters, byte equality, and emission-only PathMap traversals.
fn measure_production_gate(fixture: &Fixture, arm: Arm) -> Observation {
    assert!(Arm::PRODUCTION.contains(&arm));
    ParBag::reset_protobuf_pathmap_traversals_experiment();
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    let started = Instant::now();
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);

    let output = match arm {
        Arm::Adaptive => {
            encode_frame(black_box(&fixture.message)).expect("control benchmark frame encodes")
        }
        Arm::InFrameProduction => encode_frame_native(black_box(&fixture.message))
            .expect("production native benchmark frame encodes"),
        Arm::InFrameMemo | Arm::Selective | Arm::Blanket | Arm::SelectiveTooSmall => {
            unreachable!("production gate admits exactly two arms")
        }
    };

    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let elapsed_ns = started.elapsed().as_nanos();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    assert_eq!(output, fixture.expected);
    let pathmap_traversals = ParBag::take_protobuf_pathmap_traversals_experiment();
    let expected_traversals = match arm {
        Arm::Adaptive => fixture.reuse as u64,
        Arm::InFrameProduction => 1,
        Arm::InFrameMemo | Arm::Selective | Arm::Blanket | Arm::SelectiveTooSmall => {
            unreachable!("production gate admits exactly two arms")
        }
    };
    assert_eq!(pathmap_traversals, expected_traversals);
    Observation {
        elapsed_ns,
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        pathmap_traversals,
        preparation: ProjectionPreparationStatistics::default(),
        frame_memo: FrameMemoStatistics::default(),
        remaining_budget: 0,
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

fn selected_arms() -> Vec<Arm> {
    let available = if PRODUCTION_GATE {
        &Arm::PRODUCTION[..]
    } else {
        &Arm::ALL[..]
    };
    match std::env::var("PAR_OWNER_PROFILE_ARM") {
        Ok(label) => vec![Arm::parse(&label)
            .filter(|arm| available.contains(arm))
            .unwrap_or_else(|| {
                panic!(
                    "PAR_OWNER_PROFILE_ARM={label:?} is not available in this benchmark protocol"
                )
            })],
        Err(std::env::VarError::NotPresent) => available.to_vec(),
        Err(error) => panic!("PAR_OWNER_PROFILE_ARM is not valid Unicode: {error}"),
    }
}

fn profile_iterations() -> Option<usize> {
    match std::env::var("PAR_OWNER_PROFILE_ITERATIONS") {
        Ok(value) => Some(value.parse::<usize>().unwrap_or_else(|error| {
            panic!("PAR_OWNER_PROFILE_ITERATIONS={value:?} is not a usize: {error}")
        })),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => panic!("PAR_OWNER_PROFILE_ITERATIONS is not valid Unicode: {error}"),
    }
}

/// Low-noise whole-encoder loop for external profilers.
///
/// Fixture construction, expected-byte construction, warmup, and stdout are
/// outside the sampled loop. A rolling checksum and an exact aggregate
/// PathMap-traversal assertion prevent a profiler run from becoming vacuous.
fn run_profile_only(
    iterations: usize,
    warmup: usize,
    arm: Arm,
    width: usize,
    body_bytes: usize,
    reuse: usize,
) {
    assert!(
        iterations != 0,
        "PAR_OWNER_PROFILE_ITERATIONS must be nonzero"
    );
    assert!(
        matches!(arm, Arm::Adaptive | Arm::InFrameProduction),
        "profile-only mode compares the legacy and production encoders"
    );
    let fixture = Fixture::new(width, body_bytes, reuse);
    let encode = |fixture: &Fixture| match arm {
        Arm::Adaptive => {
            encode_frame(black_box(&fixture.message)).expect("profile control frame encodes")
        }
        Arm::InFrameProduction => encode_frame_native(black_box(&fixture.message))
            .expect("profile production frame encodes"),
        Arm::InFrameMemo | Arm::Selective | Arm::Blanket | Arm::SelectiveTooSmall => {
            unreachable!("profile-only mode admits exactly two arms")
        }
    };

    for _ in 0..warmup {
        let output = encode(&fixture);
        assert_eq!(output, fixture.expected);
        black_box(output);
    }

    ParBag::reset_protobuf_pathmap_traversals_experiment();
    let mut checksum = 0u64;
    for _ in 0..iterations {
        let output = encode(&fixture);
        assert_eq!(output, fixture.expected);
        checksum = checksum
            .rotate_left(9)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .wrapping_add(output.len() as u64)
            ^ u64::from(output[0])
            ^ (u64::from(output[output.len() - 1]) << 8);
        black_box(&output);
    }
    black_box(checksum);

    let pathmap_traversals = ParBag::take_protobuf_pathmap_traversals_experiment();
    let expected_traversals = match arm {
        Arm::Adaptive => (reuse as u64)
            .checked_mul(iterations as u64)
            .expect("profile control traversal count is representable"),
        Arm::InFrameProduction => iterations as u64,
        Arm::InFrameMemo | Arm::Selective | Arm::Blanket | Arm::SelectiveTooSmall => {
            unreachable!("profile-only mode admits exactly two arms")
        }
    };
    assert_eq!(pathmap_traversals, expected_traversals);
    println!(
        "PROFILE\tarm={}\twidth={width}\tbody_bytes={body_bytes}\treuse={reuse}\titerations={iterations}\twarmup={warmup}\toutput_bytes={}\tpathmap_traversals={pathmap_traversals}\tchecksum={checksum}",
        arm.label(),
        fixture.expected.len(),
    );
}

fn main() {
    let warmup = env_usize("PAR_OWNER_WARMUP", DEFAULT_WARMUP);
    let arms = selected_arms();
    if let Some(iterations) = profile_iterations() {
        assert_eq!(
            arms.len(),
            1,
            "profile-only mode requires PAR_OWNER_PROFILE_ARM"
        );
        let width = selected_values("PAR_OWNER_WIDTH", &WIDTHS, 16)[0];
        let body_bytes = selected_values("PAR_OWNER_BODY_BYTES", &BODY_BYTES, 256)[0];
        let reuse = selected_values("PAR_OWNER_REUSE", &REUSE_COUNTS, 1)[0];
        run_profile_only(iterations, warmup, arms[0], width, body_bytes, reuse);
        return;
    }

    let reps = env_usize("PAR_OWNER_REPS", DEFAULT_REPS);
    let ordering = if arms.len() == 1 {
        "single-arm-profile"
    } else if PRODUCTION_GATE {
        "balanced-two-arm-production-rotation"
    } else {
        "balanced-six-arm-rotation"
    };
    assert!(reps != 0, "PAR_OWNER_REPS must be nonzero");
    println!(
        "META\tformat=par-projection-owner-v5\treps={reps}\twarmup={warmup}\tordering={ordering}\ttimed=policy-plus-complete-frame\tfixture=soft-checkpoint-shared-aux\ttraversal_counter=emission-only"
    );
    println!(
        "HEADER\twidth\tbody_bytes\treuse\toutput_bytes\tprojected_bytes\tarm\tsample\tposition\telapsed_ns\tallocations\tallocated_bytes\tpathmap_traversals\tattempts\tprepared\talready_prepared\trefused_too_large\tnewly_retained_bytes\tremaining_budget\tpar_occurrences\tcomponent_empty_occurrences\teligible_occurrences\tunique_owners\trepeated_occurrences\trepeated_owners\tunique_component_bytes\treusable_component_bytes\trepeated_component_bytes\tcensus_owner_spilled\ttranscript_events\ttranscript_spilled\tdirect_only_occurrences\trange_recorded_owners\trange_copied_occurrences\tmemo_owner_spilled\tdirect_only_component_bytes\trange_recorded_component_bytes\trange_copied_component_bytes"
    );
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        println!("LOADAVG\t{}", loadavg.trim());
    }

    for width in selected_values("PAR_OWNER_WIDTH", &WIDTHS, 16) {
        for body_bytes in selected_values("PAR_OWNER_BODY_BYTES", &BODY_BYTES, 256) {
            for reuse in selected_or_all("PAR_OWNER_REUSE", &REUSE_COUNTS) {
                for repetition in 0..(warmup + reps) {
                    for position in 0..arms.len() {
                        let arm = if arms.len() == 1 {
                            arms[0]
                        } else {
                            arms[(repetition + position) % arms.len()]
                        };
                        let fixture = Fixture::new(width, body_bytes, reuse);
                        let observation = if PRODUCTION_GATE {
                            measure_production_gate(&fixture, arm)
                        } else {
                            measure(&fixture, arm)
                        };
                        if repetition >= warmup {
                            let preparation = observation.preparation;
                            let frame_memo = observation.frame_memo;
                            println!(
                                concat!(
                                    "OBS\t{width}\t{body_bytes}\t{reuse}",
                                    "\t{}\t{}\t{}\t{}\t{position}",
                                    "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                    "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                    "\t{}\t{}\t{}",
                                    "\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                ),
                                fixture.expected.len(),
                                fixture.projected_bytes,
                                arm.label(),
                                repetition - warmup,
                                observation.elapsed_ns,
                                observation.allocations,
                                observation.allocated_bytes,
                                observation.pathmap_traversals,
                                preparation.attempts,
                                preparation.prepared,
                                preparation.already_prepared,
                                preparation.refused_too_large,
                                preparation.newly_retained_bytes,
                                observation.remaining_budget,
                                frame_memo.census.par_occurrences,
                                frame_memo.census.component_empty_occurrences,
                                frame_memo.census.eligible_occurrences,
                                frame_memo.census.unique_owners,
                                frame_memo.census.repeated_occurrences,
                                frame_memo.census.repeated_owners,
                                frame_memo.census.unique_component_bytes,
                                frame_memo.census.reusable_component_bytes,
                                frame_memo.census.repeated_component_bytes,
                                frame_memo.census_owner_spilled,
                                frame_memo.transcript_events,
                                frame_memo.transcript_spilled,
                                frame_memo.direct_only_occurrences,
                                frame_memo.range_recorded_owners,
                                frame_memo.range_copied_occurrences,
                                frame_memo.memo_owner_spilled,
                                frame_memo.direct_only_component_bytes,
                                frame_memo.range_recorded_component_bytes,
                                frame_memo.range_copied_component_bytes,
                                width = width,
                                body_bytes = body_bytes,
                                reuse = reuse,
                                position = position,
                            );
                        }
                    }
                }
            }
        }
    }
}
