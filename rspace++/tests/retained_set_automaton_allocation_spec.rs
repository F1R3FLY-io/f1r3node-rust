//! Isolated construction-allocation slope gate for the retained PathMap
//! automaton.
//!
//! Candidate inputs are built before counting starts. The measured region is
//! only immutable automaton compilation, so unrelated test allocation and the
//! caller's candidate vectors cannot hide superlinear retained-state growth.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::RetainedSetAutomaton;

struct CountingAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        // SAFETY: the unchanged request is delegated to System.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        // SAFETY: the unchanged request is delegated to System.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` and `layout` came from System through this wrapper.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size, Ordering::Relaxed);
        }
        // SAFETY: the live System allocation is forwarded unchanged.
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
}

fn waiting(path: Vec<u8>) -> WaitingContinuation<Exact, ()> {
    WaitingContinuation {
        patterns: Arc::new(vec![Exact(path)]),
        ..WaitingContinuation::default()
    }
}

fn candidates(cardinality: usize) -> Vec<WaitingContinuation<Exact, ()>> {
    (0..cardinality)
        .map(|index| waiting(format!("shared/retained/record/{index:08x}").into_bytes()))
        .collect()
}

fn measure_compile(
    candidates: &[WaitingContinuation<Exact, ()>],
) -> (RetainedSetAutomaton, usize, usize) {
    ALLOCATIONS.store(0, Ordering::SeqCst);
    ALLOCATED_BYTES.store(0, Ordering::SeqCst);
    assert!(!COUNTING.swap(true, Ordering::SeqCst));
    let automaton = RetainedSetAutomaton::compile(&ExactMatcher, candidates);
    assert!(COUNTING.swap(false, Ordering::SeqCst));
    (automaton, ALLOCATIONS.load(Ordering::SeqCst), ALLOCATED_BYTES.load(Ordering::SeqCst))
}

#[test]
fn doubling_live_terminals_has_a_linear_construction_allocation_slope() {
    let small_candidates = candidates(1_024);
    let large_candidates = candidates(2_048);
    let (small, small_allocations, small_bytes) = measure_compile(&small_candidates);
    let (large, large_allocations, large_bytes) = measure_compile(&large_candidates);

    assert_eq!(small.stats().live_terminal_count, 1_024);
    assert_eq!(large.stats().live_terminal_count, 2_048);
    assert_eq!(small.stats().dead_terminal_count, 0);
    assert_eq!(large.stats().dead_terminal_count, 0);

    // A factor of three leaves allocator-bin and PathMap branch-shape margin
    // while still rejecting quadratic growth when the live set doubles.
    assert!(
        large_allocations <= small_allocations.saturating_mul(3),
        "allocation count grew superlinearly: {small_allocations} -> {large_allocations}"
    );
    assert!(
        large_bytes <= small_bytes.saturating_mul(3),
        "allocated bytes grew superlinearly: {small_bytes} -> {large_bytes}"
    );
}
