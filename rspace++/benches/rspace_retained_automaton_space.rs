use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::{
    RetainedSetAutomaton, RetainedSetAutomatonStats,
};
use rspace_plus_plus::rspace::rho_path::RhoPathV1;

struct CountingAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static FREED_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if COUNTING.load(Ordering::Relaxed) {
            FREED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size, Ordering::Relaxed);
            FREED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[derive(Clone, Default)]
struct Exact(Vec<u8>);

struct ExactMatcher;

impl Match<Exact, Vec<u8>, ()> for ExactMatcher {
    fn get(&self, pattern: &Exact, datum: &Vec<u8>) -> Option<Vec<u8>> {
        (pattern.0 == *datum).then(|| datum.clone())
    }

    fn retained_pattern_path(&self, pattern: &Exact) -> Option<Vec<u8>> { Some(pattern.0.clone()) }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> { Some(datum.clone()) }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }
}

struct FlatDispatchMatcher;

impl Match<Exact, Vec<u8>, ()> for FlatDispatchMatcher {
    fn get(&self, pattern: &Exact, datum: &Vec<u8>) -> Option<Vec<u8>> {
        ExactMatcher.get(pattern, datum)
    }

    fn retained_pattern_path(&self, pattern: &Exact) -> Option<Vec<u8>> {
        ExactMatcher.retained_pattern_path(pattern)
    }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> {
        ExactMatcher.retained_datum_path(datum)
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool {
        ExactMatcher.supports_rho_path_candidate_pruning()
    }

    fn retained_flat_dispatch_experiment(&self) -> bool { true }
}

#[derive(Clone, Copy)]
enum Corpus {
    Diverse,
    Shared,
}

impl Corpus {
    const fn name(self) -> &'static str {
        match self {
            Self::Diverse => "diverse",
            Self::Shared => "shared",
        }
    }

    fn values(self, cardinality: usize) -> Vec<Vec<u8>> {
        (0..cardinality)
            .map(|index| match self {
                Self::Diverse => {
                    format!("{:02x}/{:08x}", index % 251, index.wrapping_mul(0x9E37_79B1))
                        .into_bytes()
                }
                Self::Shared => format!("resting/shared-prefix/record-{index:08x}").into_bytes(),
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
enum Arm {
    PathMap,
    FlatDispatch,
}

impl Arm {
    const fn name(self) -> &'static str {
        match self {
            Self::PathMap => "pathmap",
            Self::FlatDispatch => "flat_dispatch",
        }
    }

    fn compile(self, candidates: &[WaitingContinuation<Exact, ()>]) -> RetainedSetAutomaton {
        match self {
            Self::PathMap => RetainedSetAutomaton::compile(&ExactMatcher, candidates),
            Self::FlatDispatch => RetainedSetAutomaton::compile(&FlatDispatchMatcher, candidates),
        }
    }
}

struct CompileAllocation {
    allocations: usize,
    allocated_bytes: usize,
    live_bytes: i128,
}

const COLUMNS: [&str; 17] = [
    "arm",
    "corpus",
    "size",
    "compile_allocations",
    "compile_allocated_bytes",
    "live_bytes_after_compile",
    "candidate_count",
    "indexed_candidate_count",
    "pattern_count",
    "indexed_pattern_count",
    "fallback_pattern_count",
    "terminal_count",
    "distinct_terminal_path_count",
    "shared_terminal_count",
    "live_terminal_count",
    "dead_terminal_count",
    "maximum_terminal_depth",
];

fn waiting(pattern: Exact) -> WaitingContinuation<Exact, ()> {
    WaitingContinuation {
        patterns: Arc::new(vec![pattern]),
        ..WaitingContinuation::default()
    }
}

fn measure_compile(
    arm: Arm,
    candidates: &[WaitingContinuation<Exact, ()>],
) -> (RetainedSetAutomaton, CompileAllocation) {
    ALLOCATIONS.store(0, Ordering::SeqCst);
    ALLOCATED_BYTES.store(0, Ordering::SeqCst);
    FREED_BYTES.store(0, Ordering::SeqCst);
    assert!(!COUNTING.swap(true, Ordering::SeqCst));
    let automaton = arm.compile(candidates);
    assert!(COUNTING.swap(false, Ordering::SeqCst));
    let allocated_bytes = ALLOCATED_BYTES.load(Ordering::SeqCst);
    let freed_bytes = FREED_BYTES.load(Ordering::SeqCst);
    let live_bytes = i128::try_from(allocated_bytes).expect("an allocated byte count fits in i128") -
        i128::try_from(freed_bytes).expect("a freed byte count fits in i128");
    let allocation = CompileAllocation {
        allocations: ALLOCATIONS.load(Ordering::SeqCst),
        allocated_bytes,
        live_bytes,
    };
    (automaton, allocation)
}

fn preflight(
    corpus: Corpus,
    cardinality: usize,
    candidates: &[WaitingContinuation<Exact, ()>],
    resting: &[u8],
) {
    let pathmap = Arm::PathMap.compile(candidates);
    let flat_dispatch = Arm::FlatDispatch.compile(candidates);
    assert_eq!(
        flat_dispatch.stats(),
        pathmap.stats(),
        "space preflight: flat-dispatch stats equal the PathMap stats for the {} corpus at \
         {cardinality}",
        corpus.name()
    );
    let mut resting_bag = Bag::new();
    resting_bag
        .insert(&RhoPathV1::relative_datum_key(resting, b"stable/resting-identity"))
        .expect("one resting occurrence");
    for fresh in [resting, b"fresh/does-not-match-any-retained-pattern".as_slice()] {
        for bag in [None, Some(&resting_bag)] {
            assert_eq!(
                flat_dispatch.matching_candidates_with_resting(fresh, &[0], bag),
                pathmap.matching_candidates_with_resting(fresh, &[0], bag),
                "space preflight: flat-dispatch selections equal the PathMap selections for the \
                 {} corpus at {cardinality}",
                corpus.name()
            );
        }
    }
}

fn row(
    arm: Arm,
    corpus: Corpus,
    cardinality: usize,
    allocation: &CompileAllocation,
    stats: RetainedSetAutomatonStats,
) -> String {
    [
        arm.name().to_string(),
        corpus.name().to_string(),
        cardinality.to_string(),
        allocation.allocations.to_string(),
        allocation.allocated_bytes.to_string(),
        allocation.live_bytes.to_string(),
        stats.candidate_count.to_string(),
        stats.indexed_candidate_count.to_string(),
        stats.pattern_count.to_string(),
        stats.indexed_pattern_count.to_string(),
        stats.fallback_pattern_count.to_string(),
        stats.terminal_count.to_string(),
        stats.distinct_terminal_path_count.to_string(),
        stats.shared_terminal_count.to_string(),
        stats.live_terminal_count.to_string(),
        stats.dead_terminal_count.to_string(),
        stats.maximum_terminal_depth.to_string(),
    ]
    .join("\t")
}

fn main() {
    let preflight_only = std::env::args()
        .skip(1)
        .any(|argument| argument == "--test");
    if !preflight_only {
        println!("{}", COLUMNS.join("\t"));
    }
    for corpus in [Corpus::Diverse, Corpus::Shared] {
        for cardinality in [16usize, 256, 4_096] {
            let values = corpus.values(cardinality);
            let resting = values[cardinality - 1].clone();
            let candidates = values
                .into_iter()
                .map(|value| waiting(Exact(value)))
                .collect::<Vec<_>>();
            preflight(corpus, cardinality, &candidates, &resting);
            if preflight_only {
                continue;
            }
            for arm in [Arm::PathMap, Arm::FlatDispatch] {
                let (automaton, allocation) = measure_compile(arm, &candidates);
                println!("{}", row(arm, corpus, cardinality, &allocation, automaton.stats()));
                drop(automaton);
            }
        }
    }
}
