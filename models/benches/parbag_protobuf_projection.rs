//! ParBag protobuf egress projection-policy experiment.
//!
//! The three mechanisms share the production RPB3 zipper encoder:
//!
//! - `direct` writes resident protobuf bodies to the final sink and retains no
//!   second image;
//! - `cold_cached` constructs the current before/after-root projections;
//! - `warm_cached` reuses those projections;
//! - `cold_adaptive` takes the proposed cache-hit-or-direct route with no
//!   existing projection;
//! - `warm_adaptive` exercises the same route with a shared projection.
//!
//! Fixture construction, cache preparation, output allocation, and byte
//! comparison are outside the timed interval. A counting allocator therefore
//! isolates allocations caused by the egress policy itself. Every observation
//! is rejected unless it is byte-identical to the direct RPB3 authority.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Expr;
use models::rust::par_bag::{Multiplicity, ParBag};
use models::rust::rholang::protobuf_encoder;

const WIDTHS: [usize; 4] = [1, 16, 256, 4_096];
const BODY_BYTES: [usize; 3] = [16, 256, 4_096];
const MULTIPLICITIES: [u64; 3] = [1, 2, 16];
const DEFAULT_WARMUP: usize = 5;
const DEFAULT_REPS: usize = 31;
const DEFAULT_PROFILE_ITERATIONS: usize = 1_000;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, old, new_size) };
        if !replacement.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        replacement
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Direct,
    ColdCached,
    WarmCached,
    ColdAdaptive,
    WarmAdaptive,
}

impl Arm {
    const ALL: [Self; 5] = [
        Self::Direct,
        Self::ColdCached,
        Self::WarmCached,
        Self::ColdAdaptive,
        Self::WarmAdaptive,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::ColdCached => "cold_cached",
            Self::WarmCached => "warm_cached",
            Self::ColdAdaptive => "cold_adaptive",
            Self::WarmAdaptive => "warm_adaptive",
        }
    }

    fn parse(value: &str) -> Option<Self> { Self::ALL.into_iter().find(|arm| arm.label() == value) }
}

struct Fixture {
    base: ParBag,
    expected: Vec<u8>,
}

impl Fixture {
    fn new(width: usize, body_bytes: usize, multiplicity: u64) -> Self {
        assert!(WIDTHS.contains(&width), "width must be preregistered");
        assert!(
            BODY_BYTES.contains(&body_bytes),
            "body size must be preregistered"
        );
        assert!(
            MULTIPLICITIES.contains(&multiplicity),
            "multiplicity must be preregistered"
        );
        let multiplicity = Multiplicity::new(multiplicity).expect("positive multiplicity");
        let payload_bytes = byte_array_payload_len(body_bytes);
        let mut base = ParBag::new();
        for ordinal in 0..width {
            let value = byte_array_expr(payload_bytes, ordinal);
            assert_eq!(protobuf_encoder::encoded_len(&value), body_bytes);
            base.insert_many(&value, multiplicity)
                .expect("fixture cardinality fits u64");
        }
        assert_eq!(base.distinct_len(), width);
        let mut expected = Vec::with_capacity(
            base.protobuf_components_encoded_len()
                .expect("fixture expansion is representable"),
        );
        base.encode_protobuf_components_direct(&mut expected)
            .expect("direct fixture authority");
        let fixture = Self { base, expected };
        fixture.preflight();
        fixture
    }

    fn fresh(&self) -> ParBag {
        ParBag::try_from_bag(self.base.as_bag().clone())
            .expect("internally constructed fixture remains canonical")
    }

    fn prepared(&self, arm: Arm) -> ParBag {
        let bag = self.fresh();
        assert_eq!(
            bag.protobuf_projection_cache_state_experiment(),
            (false, false),
            "fresh imported roots must not carry a projection"
        );
        if matches!(arm, Arm::WarmCached | Arm::WarmAdaptive) {
            let mut scratch = Vec::with_capacity(self.expected.len());
            bag.encode_protobuf_components_cached_experiment(&mut scratch)
                .expect("prewarm cached projection");
            assert_eq!(scratch, self.expected);
            assert_eq!(
                bag.protobuf_projection_cache_state_experiment(),
                (true, true)
            );
        }
        bag
    }

    fn execute(&self, arm: Arm, bag: &ParBag, output: &mut Vec<u8>) {
        match arm {
            Arm::Direct => bag.encode_protobuf_components_direct(output),
            Arm::ColdCached | Arm::WarmCached => {
                bag.encode_protobuf_components_cached_experiment(output)
            }
            Arm::ColdAdaptive | Arm::WarmAdaptive => {
                bag.encode_protobuf_components_adaptive_experiment(output)
            }
        }
        .expect("fixture output is representable");
    }

    fn preflight(&self) {
        for arm in Arm::ALL {
            let bag = self.prepared(arm);
            let before = bag.protobuf_projection_cache_state_experiment();
            let mut output = Vec::with_capacity(self.expected.len());
            self.execute(arm, &bag, &mut output);
            assert_eq!(output, self.expected, "{} changed bytes", arm.label());
            let after = bag.protobuf_projection_cache_state_experiment();
            match arm {
                Arm::Direct | Arm::ColdAdaptive => assert_eq!(after, before),
                Arm::ColdCached => assert_eq!(after, (true, true)),
                Arm::WarmCached | Arm::WarmAdaptive => assert_eq!(after, before),
            }
        }
    }
}

fn byte_array_payload_len(body_bytes: usize) -> usize {
    let key_bytes = prost::encoding::key_len(25);
    (8..=body_bytes)
        .find(|payload_bytes| {
            key_bytes + prost::encoding::encoded_len_varint(*payload_bytes as u64) + payload_bytes
                == body_bytes
        })
        .unwrap_or_else(|| panic!("no GByteArray Expr has an exact {body_bytes}-byte body"))
}

fn byte_array_expr(payload_bytes: usize, ordinal: usize) -> Expr {
    let mut payload = vec![0xa5; payload_bytes];
    payload[..8].copy_from_slice(&(ordinal as u64).to_be_bytes());
    Expr {
        expr_instance: Some(ExprInstance::GByteArray(payload)),
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not an integer: {error}"))
        })
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a u64: {error}"))
        })
        .unwrap_or(default)
}

fn selected_usizes(name: &str, values: &[usize]) -> Vec<usize> {
    match std::env::var(name) {
        Ok(value) => {
            let selected = value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not an integer: {error}"));
            assert!(values.contains(&selected), "{name} is not preregistered");
            vec![selected]
        }
        Err(std::env::VarError::NotPresent) => values.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn selected_u64s(name: &str, values: &[u64]) -> Vec<u64> {
    match std::env::var(name) {
        Ok(value) => {
            let selected = value
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a u64: {error}"));
            assert!(values.contains(&selected), "{name} is not preregistered");
            vec![selected]
        }
        Err(std::env::VarError::NotPresent) => values.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn measure(fixture: &Fixture, arm: Arm) -> (u128, u64, u64) {
    let bag = fixture.prepared(arm);
    let mut output = Vec::with_capacity(fixture.expected.len());
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    let started = Instant::now();
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    fixture.execute(arm, black_box(&bag), black_box(&mut output));
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let elapsed = started.elapsed().as_nanos();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    assert_eq!(output, fixture.expected);
    (
        elapsed,
        ALLOC_CALLS.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
}

fn print_metadata(mode: &str, reps: usize, warmup: usize) {
    println!(
        "META\tformat=parbag-protobuf-projection-v1\tmode={mode}\treps={reps}\twarmup={warmup}\twidths=1,16,256,4096\tbody_bytes=16,256,4096\tmultiplicities=1,2,16"
    );
    println!(
        "HEADER\twidth\tbody_bytes\tmultiplicity\toutput_bytes\tarm\tsample\telapsed_ns\tallocations\tallocated_bytes"
    );
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        println!("LOADAVG\t{}", loadavg.trim());
    }
}

fn run_matrix() {
    let reps = env_usize("PARBAG_PROTOBUF_REPS", DEFAULT_REPS);
    let warmup = env_usize("PARBAG_PROTOBUF_WARMUP", DEFAULT_WARMUP);
    assert!(reps != 0, "PARBAG_PROTOBUF_REPS must be nonzero");
    print_metadata("matrix", reps, warmup);
    for width in selected_usizes("PARBAG_PROTOBUF_WIDTH", &WIDTHS) {
        for body_bytes in selected_usizes("PARBAG_PROTOBUF_BODY_BYTES", &BODY_BYTES) {
            for multiplicity in selected_u64s("PARBAG_PROTOBUF_MULTIPLICITY", &MULTIPLICITIES) {
                let fixture = Fixture::new(width, body_bytes, multiplicity);
                for repetition in 0..(warmup + reps) {
                    // Rotate the first arm to counter first-in-batch thermal and
                    // scheduler effects without changing the prepared fixture.
                    for slot in 0..Arm::ALL.len() {
                        let arm = Arm::ALL[(repetition + slot) % Arm::ALL.len()];
                        let (elapsed, allocations, allocated_bytes) = measure(&fixture, arm);
                        if repetition >= warmup {
                            println!(
                                "OBS\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                width,
                                body_bytes,
                                multiplicity,
                                fixture.expected.len(),
                                arm.label(),
                                repetition - warmup,
                                elapsed,
                                allocations,
                                allocated_bytes,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn run_profile(arm: Arm) {
    let width = env_usize("PARBAG_PROTOBUF_WIDTH", 256);
    let body_bytes = env_usize("PARBAG_PROTOBUF_BODY_BYTES", 256);
    let multiplicity = env_u64("PARBAG_PROTOBUF_MULTIPLICITY", 1);
    let iterations = env_usize("PARBAG_PROTOBUF_ITERATIONS", DEFAULT_PROFILE_ITERATIONS);
    assert!(WIDTHS.contains(&width));
    assert!(BODY_BYTES.contains(&body_bytes));
    assert!(MULTIPLICITIES.contains(&multiplicity));
    assert!(iterations != 0);
    print_metadata("profile", iterations, 0);
    let fixture = Fixture::new(width, body_bytes, multiplicity);
    let bag = fixture.prepared(arm);
    let mut checksum = 0u64;
    for _ in 0..iterations {
        let mut output = Vec::with_capacity(fixture.expected.len());
        fixture.execute(arm, black_box(&bag), black_box(&mut output));
        checksum = checksum.rotate_left(7) ^ output.len() as u64;
        checksum ^= u64::from(output.first().copied().unwrap_or(0));
    }
    println!(
        "PROFILE\tarm={}\twidth={}\tbody_bytes={}\tmultiplicity={}\titerations={}\toutput_bytes={}\tchecksum={checksum:016x}",
        arm.label(), width, body_bytes, multiplicity, iterations, fixture.expected.len()
    );
}

fn main() {
    match std::env::var("PARBAG_PROTOBUF_ARM") {
        Ok(value) => run_profile(
            Arm::parse(&value)
                .unwrap_or_else(|| panic!("PARBAG_PROTOBUF_ARM={value:?} is not registered")),
        ),
        Err(std::env::VarError::NotPresent) => run_matrix(),
        Err(error) => panic!("PARBAG_PROTOBUF_ARM is not valid Unicode: {error}"),
    }
}
