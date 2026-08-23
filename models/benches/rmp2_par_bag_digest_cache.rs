//! RMP2 ParBag component-digest cache experiment (pgmcp experiment 5984).
//!
//! The default run emits raw, tab-separated observations for the preregistered
//! Cartesian matrix. It compares the same RMP2 transcript through six cache
//! states: uncached control, cold cache, warm cache, prewarmed copy-on-write
//! (CoW) clones, first access after mutation, and second access after mutation.
//! Fixture construction and cache-state preparation occur outside each timed
//! interval. The three axes are distinct endpoint width, exact canonical
//! protobuf body size, and the multiplicity stored at every endpoint. A
//! process-local counting allocator records allocations made only by the one
//! summary call in each observation.
//!
//! External instruction/data-read and headless Heaptrack profiles isolate one
//! lane by setting `RMP2_CACHE_ARM` to one of the labels printed by
//! [`Arm::label`]. The selected cell is controlled by
//! `RMP2_CACHE_WIDTH`, `RMP2_CACHE_BODY_BYTES`, and
//! `RMP2_CACHE_MULTIPLICITY`; `RMP2_CACHE_ITERATIONS` controls how many
//! independently prepared subjects the profiler executes. Full-matrix retained
//! repetitions and warmups can be overridden with `RMP2_CACHE_REPS` and
//! `RMP2_CACHE_WARMUP`. When `RMP2_CACHE_ARM` is absent, any of the three
//! cell selectors narrows the paired six-lane matrix instead of selecting one
//! arm. This supports an exact diagnostic replication without introducing a
//! second measurement path.
//!
//! Every run fails before measurement unless each resident canonical protobuf
//! body has the requested exact byte length, the cached and uncached summaries
//! are byte-identical, one CoW family initializes exactly once, a warm access
//! performs no initialization, and mutation detaches only its changed root.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Expr;
use models::rust::par_bag::{CanonicalParBagKey, Multiplicity, ParBag};
use models::rust::rholang::match_path::{
    cached_par_bag_match_summary_experiment, observed_par_bag_match_summary_experiment,
    uncached_par_bag_match_summary_experiment, ParBagMatchSummary,
};
use models::rust::rholang::protobuf_encoder;

const WIDTHS: [usize; 5] = [1, 16, 64, 256, 1_024];
const BODY_BYTES: [usize; 3] = [16, 256, 4_096];
const MULTIPLICITIES: [u64; 5] = [1, 2, 4, 8, 32];
const DEFAULT_WARMUP: usize = 5;
const DEFAULT_REPS: usize = 30;
const DEFAULT_PROFILE_ITERATIONS: usize = 1_000;
const TRANSCRIPT_DOMAIN_BYTES: usize = b"RhoMatchPath/ParBag/components/v2".len();
const TRANSCRIPT_RECORD_HEADER_BYTES: usize = 4 + 8 + 8;

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
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            DEALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, old, new_size) };
        if !replacement.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            DEALLOC_BYTES.fetch_add(old.size() as u64, Ordering::Relaxed);
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        replacement
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    NullProfile,
    WarmNullProfile,
    Uncached,
    ColdCache,
    WarmCache,
    PrewarmedCowClones,
    FirstPostMutation,
    SecondPostMutation,
}

impl Arm {
    const ALL: [Self; 6] = [
        Self::Uncached,
        Self::ColdCache,
        Self::WarmCache,
        Self::PrewarmedCowClones,
        Self::FirstPostMutation,
        Self::SecondPostMutation,
    ];

    const PROFILE: [Self; 8] = [
        Self::NullProfile,
        Self::WarmNullProfile,
        Self::Uncached,
        Self::ColdCache,
        Self::WarmCache,
        Self::PrewarmedCowClones,
        Self::FirstPostMutation,
        Self::SecondPostMutation,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::NullProfile => "null_profile",
            Self::WarmNullProfile => "warm_null_profile",
            Self::Uncached => "uncached_control",
            Self::ColdCache => "cold_cache",
            Self::WarmCache => "warm_cache",
            Self::PrewarmedCowClones => "prewarmed_cow_clones",
            Self::FirstPostMutation => "first_post_mutation",
            Self::SecondPostMutation => "second_post_mutation",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::PROFILE.into_iter().find(|arm| arm.label() == value)
    }
}

#[derive(Clone, Copy)]
struct AllocationSample {
    allocations: u64,
    allocated_bytes: u64,
    deallocations: u64,
    deallocated_bytes: u64,
}

struct Fixture {
    width: usize,
    body_bytes: usize,
    multiplicity: u64,
    transcript_bytes: usize,
    base: ParBag,
    mutation_key: CanonicalParBagKey<Expr>,
    base_summary: ParBagMatchSummary,
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
        let multiplicity =
            Multiplicity::new(multiplicity).expect("every preregistered multiplicity is positive");

        let payload_bytes = byte_array_payload_len(body_bytes);
        let mut base = ParBag::new();
        let mut values = Vec::with_capacity(width);
        for ordinal in 0..width {
            let value = byte_array_expr(payload_bytes, ordinal);
            assert_eq!(
                protobuf_encoder::encoded_len(&value),
                body_bytes,
                "fixture Expr body is not the requested exact size"
            );
            base.insert_many(&value, multiplicity)
                .expect("fixture width and multiplicity fit u64");
            values.push(value);
        }
        assert_eq!(
            base.distinct_len(),
            width,
            "fixture values are not distinct"
        );
        let expected_cardinality = u64::try_from(width)
            .expect("preregistered width fits u64")
            .checked_mul(multiplicity.get())
            .expect("fixture cardinality fits u64");
        assert_eq!(base.cardinality(), expected_cardinality);

        let mut visited = 0usize;
        base.visit_encoded::<Expr>(|entry| {
            assert_eq!(
                entry.payload().len(),
                body_bytes,
                "resident canonical protobuf body changed size"
            );
            assert_eq!(entry.multiplicity(), multiplicity);
            visited += 1;
        });
        assert_eq!(visited, width, "fixture does not expose every endpoint");

        let base_summary = uncached_par_bag_match_summary_experiment(&base);
        let mutation = values
            .into_iter()
            .next()
            .expect("every preregistered width is nonzero");
        let mutation_key = CanonicalParBagKey::compile(&mutation);
        let mut mutated = fresh_cache_root(&base);
        mutate_identity_preserving(&mut mutated, &mutation_key);
        let mutated_summary = uncached_par_bag_match_summary_experiment(&mutated);
        assert_eq!(
            mutated_summary, base_summary,
            "remove/reinsert must invalidate the cache without changing the RMP2 transcript"
        );
        assert_eq!(
            mutated.as_bag().compact_bytes(),
            base.as_bag().compact_bytes(),
            "the post-mutation lane must preserve the exact canonical PathMap"
        );

        let fixture = Self {
            width,
            body_bytes,
            multiplicity: multiplicity.get(),
            transcript_bytes: TRANSCRIPT_DOMAIN_BYTES
                + width * (TRANSCRIPT_RECORD_HEADER_BYTES + body_bytes),
            base,
            mutation_key,
            base_summary,
        };
        fixture.preflight_cache_contract();
        fixture
    }

    fn preflight_cache_contract(&self) {
        let family = fresh_cache_root(&self.base);
        let clone_before_prime = family.clone();
        assert!(family.shares_storage_with(&clone_before_prime));

        let (first, initialized) = observed_par_bag_match_summary_experiment(&family);
        assert!(initialized, "cold CoW family did not initialize its digest");
        assert_eq!(first, self.base_summary);
        let (from_clone, initialized) =
            observed_par_bag_match_summary_experiment(&clone_before_prime);
        assert!(!initialized, "prewarmed CoW clone reopened the transcript");
        assert_eq!(from_clone, first);
        let (warm, initialized) = observed_par_bag_match_summary_experiment(&family);
        assert!(!initialized, "warm cache reopened the transcript");
        assert_eq!(warm, first);

        let mut mutated = family.clone();
        mutate_identity_preserving(&mut mutated, &self.mutation_key);
        assert!(!mutated.shares_storage_with(&family));
        let (post_mutation, initialized) = observed_par_bag_match_summary_experiment(&mutated);
        assert!(initialized, "first post-mutation access did not initialize");
        assert_eq!(post_mutation, self.base_summary);
        let (post_mutation_warm, initialized) = observed_par_bag_match_summary_experiment(&mutated);
        assert!(
            !initialized,
            "second post-mutation access reopened the transcript"
        );
        assert_eq!(post_mutation_warm, post_mutation);
        let (unchanged, initialized) = observed_par_bag_match_summary_experiment(&family);
        assert!(
            !initialized,
            "mutating a clone invalidated the original family"
        );
        assert_eq!(unchanged, first);
    }

    fn expected(&self, _arm: Arm) -> ParBagMatchSummary { self.base_summary }
}

struct PreparedLane {
    arm: Arm,
    bag: ParBag,
}

impl PreparedLane {
    fn new(fixture: &Fixture, arm: Arm) -> Self {
        let bag = match arm {
            Arm::NullProfile | Arm::Uncached | Arm::ColdCache => fresh_cache_root(&fixture.base),
            Arm::WarmNullProfile | Arm::WarmCache => {
                let bag = fresh_cache_root(&fixture.base);
                assert_eq!(
                    cached_par_bag_match_summary_experiment(&bag),
                    fixture.base_summary
                );
                bag
            }
            Arm::PrewarmedCowClones => {
                let bag = fresh_cache_root(&fixture.base);
                assert_eq!(
                    cached_par_bag_match_summary_experiment(&bag),
                    fixture.base_summary
                );
                bag.clone()
            }
            Arm::FirstPostMutation | Arm::SecondPostMutation => {
                let bag = fresh_cache_root(&fixture.base);
                assert_eq!(
                    cached_par_bag_match_summary_experiment(&bag),
                    fixture.base_summary
                );
                let mut changed = bag.clone();
                mutate_identity_preserving(&mut changed, &fixture.mutation_key);
                if arm == Arm::SecondPostMutation {
                    assert_eq!(
                        cached_par_bag_match_summary_experiment(&changed),
                        fixture.base_summary
                    );
                }
                changed
            }
        };
        Self { arm, bag }
    }

    fn execute(&self, expected: ParBagMatchSummary) -> u64 {
        let summary = match self.arm {
            Arm::NullProfile | Arm::WarmNullProfile => {
                return black_box(fold_summary(0x9e37_79b9_7f4a_7c15u64, black_box(expected)));
            }
            Arm::Uncached => uncached_par_bag_match_summary_experiment(black_box(&self.bag)),
            _ => cached_par_bag_match_summary_experiment(black_box(&self.bag)),
        };
        black_box(fold_summary(0x9e37_79b9_7f4a_7c15u64, black_box(summary)))
    }
}

fn byte_array_payload_len(body_bytes: usize) -> usize {
    let key_bytes = prost::encoding::key_len(25);
    (0..=body_bytes)
        .find(|payload_bytes| {
            key_bytes + prost::encoding::encoded_len_varint(*payload_bytes as u64) + payload_bytes
                == body_bytes
        })
        .unwrap_or_else(|| panic!("no GByteArray Expr has an exact {body_bytes}-byte body"))
}

fn byte_array_expr(payload_bytes: usize, ordinal: usize) -> Expr {
    let mut payload = vec![0xa5; payload_bytes];
    let ordinal = (ordinal as u64).to_be_bytes();
    payload[..ordinal.len()].copy_from_slice(&ordinal);
    Expr {
        expr_instance: Some(ExprInstance::GByteArray(payload)),
    }
}

fn fresh_cache_root(source: &ParBag) -> ParBag {
    ParBag::try_from_bag(source.as_bag().clone())
        .expect("an internally constructed ParBag remains canonical")
}

fn mutate_identity_preserving(bag: &mut ParBag, key: &CanonicalParBagKey<Expr>) {
    let before = bag.count_key(key);
    assert!(before != 0, "mutation key must be resident");
    assert_eq!(
        bag.remove_key(key).map_or(0, Multiplicity::get),
        before - 1,
        "remove must decrement exactly one occurrence"
    );
    assert_eq!(
        bag.insert_key(key).map(Multiplicity::get),
        Ok(before),
        "reinsert must restore the original multiplicity"
    );
}

fn fold_summary(mut checksum: u64, summary: ParBagMatchSummary) -> u64 {
    checksum = checksum.rotate_left(11) ^ summary.distinct();
    checksum = checksum.rotate_left(17) ^ summary.cardinality();
    for word in summary.digest().chunks_exact(8) {
        checksum = checksum.rotate_left(23)
            ^ u64::from_be_bytes(word.try_into().expect("digest chunk has width eight"));
    }
    checksum
}

fn reset_allocation_counters() {
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    DEALLOC_CALLS.store(0, Ordering::Relaxed);
    DEALLOC_BYTES.store(0, Ordering::Relaxed);
}

fn measure(lane: &PreparedLane, expected: ParBagMatchSummary) -> (u128, AllocationSample, u64) {
    reset_allocation_counters();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    let started = Instant::now();
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    let checksum = lane.execute(expected);
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let elapsed = started.elapsed().as_nanos();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);

    assert_eq!(
        checksum,
        fold_summary(0x9e37_79b9_7f4a_7c15u64, expected),
        "lane computed a different summary or skipped work"
    );
    (
        elapsed,
        AllocationSample {
            allocations: ALLOC_CALLS.load(Ordering::Relaxed),
            allocated_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
            deallocations: DEALLOC_CALLS.load(Ordering::Relaxed),
            deallocated_bytes: DEALLOC_BYTES.load(Ordering::Relaxed),
        },
        checksum,
    )
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

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "yes" => true,
            "0" | "false" | "no" => false,
            _ => panic!("{name}={value:?} is not a boolean"),
        },
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn print_metadata(mode: &str, reps: usize, warmup: usize) {
    println!(
        "META\tformat=rmp2-par-bag-digest-cache-v1\tmode={mode}\treps={reps}\twarmup={warmup}\twidths=1,16,64,256,1024\tbody_bytes=16,256,4096\tmultiplicities=1,2,4,8,32"
    );
    println!(
        "HEADER\twidth\tbody_bytes\ttranscript_bytes\tmultiplicity\tarm\tsample\telapsed_ns\tallocations\tallocated_bytes\tdeallocations\tdeallocated_bytes\tnet_allocated_bytes\tchecksum"
    );
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        let fields = loadavg.split_whitespace().collect::<Vec<_>>();
        if fields.len() == 5 {
            println!(
                "LOADAVG\tone_minute={}\tfive_minutes={}\tfifteen_minutes={}\trunnable={}\tlast_pid={}",
                fields[0], fields[1], fields[2], fields[3], fields[4]
            );
        }
    }
}

fn print_observation(
    fixture: &Fixture,
    arm: Arm,
    sample: usize,
    elapsed: u128,
    allocations: AllocationSample,
    checksum: u64,
) {
    let net = i128::from(allocations.allocated_bytes) - i128::from(allocations.deallocated_bytes);
    println!(
        "OBS\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{checksum:016x}",
        fixture.width,
        fixture.body_bytes,
        fixture.transcript_bytes,
        fixture.multiplicity,
        arm.label(),
        sample,
        elapsed,
        allocations.allocations,
        allocations.allocated_bytes,
        allocations.deallocations,
        allocations.deallocated_bytes,
        net,
    );
}

fn run_matrix() {
    let reps = env_usize("RMP2_CACHE_REPS", DEFAULT_REPS);
    let warmup = env_usize("RMP2_CACHE_WARMUP", DEFAULT_WARMUP);
    assert!(reps != 0, "RMP2_CACHE_REPS must be nonzero");
    print_metadata("matrix", reps, warmup);

    let widths = selected_matrix_usizes("RMP2_CACHE_WIDTH", &WIDTHS);
    let body_sizes = selected_matrix_usizes("RMP2_CACHE_BODY_BYTES", &BODY_BYTES);
    let multiplicities = selected_matrix_u64s("RMP2_CACHE_MULTIPLICITY", &MULTIPLICITIES);
    println!(
        "SELECTION\twidths={}\tbody_bytes={}\tmultiplicities={}",
        join_usizes(&widths),
        join_usizes(&body_sizes),
        join_u64s(&multiplicities),
    );

    for width in widths {
        for &body_bytes in &body_sizes {
            for &multiplicity in &multiplicities {
                let fixture = Fixture::new(width, body_bytes, multiplicity);
                for repetition in 0..(warmup + reps) {
                    let lanes = Arm::ALL.map(|arm| PreparedLane::new(&fixture, arm));
                    for slot in 0..Arm::ALL.len() {
                        let arm_index = (repetition + slot) % Arm::ALL.len();
                        let lane = &lanes[arm_index];
                        let expected = fixture.expected(lane.arm);
                        let (elapsed, allocations, checksum) = measure(lane, expected);
                        if repetition >= warmup {
                            print_observation(
                                &fixture,
                                lane.arm,
                                repetition - warmup,
                                elapsed,
                                allocations,
                                checksum,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn selected_matrix_usizes<const N: usize>(name: &str, allowed: &[usize; N]) -> Vec<usize> {
    match std::env::var(name) {
        Ok(value) => vec![selected_usize(
            name,
            value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not an integer: {error}")),
            allowed,
        )],
        Err(std::env::VarError::NotPresent) => allowed.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn selected_matrix_u64s<const N: usize>(name: &str, allowed: &[u64; N]) -> Vec<u64> {
    match std::env::var(name) {
        Ok(value) => vec![selected_u64(
            name,
            value
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a u64: {error}")),
            allowed,
        )],
        Err(std::env::VarError::NotPresent) => allowed.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn join_usizes(values: &[usize]) -> String {
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn join_u64s(values: &[u64]) -> String {
    values
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn selected_usize<const N: usize>(name: &str, value: usize, allowed: &[usize; N]) -> usize {
    assert!(
        allowed.contains(&value),
        "{name}={value} is outside the preregistered matrix {allowed:?}"
    );
    value
}

fn selected_u64<const N: usize>(name: &str, value: u64, allowed: &[u64; N]) -> u64 {
    assert!(
        allowed.contains(&value),
        "{name}={value} is outside the preregistered matrix {allowed:?}"
    );
    value
}

fn run_profile(arm_name: &str) {
    let arm = Arm::parse(arm_name).unwrap_or_else(|| {
        panic!(
            "unknown RMP2_CACHE_ARM={arm_name:?}; expected one of {}",
            Arm::PROFILE.map(Arm::label).join(",")
        )
    });
    let width = selected_usize(
        "RMP2_CACHE_WIDTH",
        env_usize("RMP2_CACHE_WIDTH", 1_024),
        &WIDTHS,
    );
    let body_bytes = selected_usize(
        "RMP2_CACHE_BODY_BYTES",
        env_usize("RMP2_CACHE_BODY_BYTES", 4_096),
        &BODY_BYTES,
    );
    let multiplicity = selected_u64(
        "RMP2_CACHE_MULTIPLICITY",
        env_u64("RMP2_CACHE_MULTIPLICITY", 32),
        &MULTIPLICITIES,
    );
    let iterations = env_usize("RMP2_CACHE_ITERATIONS", DEFAULT_PROFILE_ITERATIONS);
    let reuse_prepared = env_bool("RMP2_CACHE_REUSE_PREPARED", false);
    let quiet = env_bool("RMP2_CACHE_PROFILE_QUIET", false);
    assert!(iterations != 0, "RMP2_CACHE_ITERATIONS must be nonzero");

    if reuse_prepared {
        assert!(
            matches!(arm, Arm::WarmNullProfile | Arm::WarmCache),
            "RMP2_CACHE_REUSE_PREPARED is valid only for matched warm profile lanes"
        );
    }

    print_metadata(
        if reuse_prepared {
            "profile-reused-preparation"
        } else {
            "profile"
        },
        iterations,
        0,
    );
    let fixture = Fixture::new(width, body_bytes, multiplicity);
    let mut aggregate = 0u64;
    if reuse_prepared {
        let lane = PreparedLane::new(&fixture, arm);
        for sample in 0..iterations {
            let (elapsed, allocations, checksum) = measure(&lane, fixture.expected(arm));
            aggregate = aggregate.rotate_left(7) ^ checksum;
            if !quiet {
                print_observation(&fixture, arm, sample, elapsed, allocations, checksum);
            }
        }
        if quiet {
            println!(
                "PROFILE_SUMMARY\tarm={}\titerations={iterations}\taggregate={aggregate:016x}",
                arm.label()
            );
        }
        return;
    }
    let lanes = (0..iterations)
        .map(|_| PreparedLane::new(&fixture, arm))
        .collect::<Vec<_>>();
    for (sample, lane) in lanes.iter().enumerate() {
        let (elapsed, allocations, checksum) = measure(lane, fixture.expected(arm));
        aggregate = aggregate.rotate_left(7) ^ checksum;
        if !quiet {
            print_observation(&fixture, arm, sample, elapsed, allocations, checksum);
        }
    }
    if quiet {
        println!(
            "PROFILE_SUMMARY\tarm={}\titerations={iterations}\taggregate={aggregate:016x}",
            arm.label()
        );
    }
}

fn main() {
    match std::env::var("RMP2_CACHE_ARM") {
        Ok(arm) => run_profile(&arm),
        Err(std::env::VarError::NotPresent) => run_matrix(),
        Err(error) => panic!("RMP2_CACHE_ARM is not valid Unicode: {error}"),
    }
}
