//! Representation-level allocation gate for the PathMap-backed Bag.
//!
//! This test binary owns its counting allocator, so unrelated workspace tests
//! cannot contaminate the measurements. The gate compares one logical
//! occurrence with `u64::MAX` indistinguishable occurrences at the same path.
//! A multiplicity is one fixed-width trie value; logical cardinality must not
//! allocate or serialize one object per occurrence.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rspace_plus_plus::rspace::bag::{Bag, Multiplicity};

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
        // SAFETY: this allocator delegates the unchanged allocation request to
        // the process System allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        // SAFETY: this allocator delegates the unchanged allocation request to
        // the process System allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from the System allocator through this exact
        // wrapper and `layout` is the corresponding allocation layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size, Ordering::Relaxed);
        }
        // SAFETY: `pointer` and `layout` identify a live System allocation, and
        // the request is forwarded without changing either value.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    ALLOCATIONS.store(0, Ordering::SeqCst);
    ALLOCATED_BYTES.store(0, Ordering::SeqCst);
    assert!(!COUNTING.swap(true, Ordering::SeqCst));
    let result = operation();
    assert!(COUNTING.swap(false, Ordering::SeqCst));
    (result, ALLOCATIONS.load(Ordering::SeqCst), ALLOCATED_BYTES.load(Ordering::SeqCst))
}

fn singleton(path: &[u8], multiplicity: Multiplicity) -> Bag {
    let mut bag = Bag::new();
    bag.insert_many(path, multiplicity).unwrap();
    bag
}

#[test]
fn logical_cardinality_never_materializes_occurrence_sized_storage() {
    assert_eq!(std::mem::size_of::<Multiplicity>(), std::mem::size_of::<u64>());
    assert_eq!(std::mem::align_of::<Multiplicity>(), std::mem::align_of::<u64>());

    let path = vec![0xa5; 256];
    let (one, one_allocations, one_bytes) = measure(|| singleton(&path, Multiplicity::ONE));
    let (maximum, maximum_allocations, maximum_bytes) =
        measure(|| singleton(&path, Multiplicity::new(u64::MAX).unwrap()));

    assert_eq!(one_allocations, maximum_allocations);
    assert_eq!(one_bytes, maximum_bytes);
    assert_eq!(one.distinct_len(), 1);
    assert_eq!(maximum.distinct_len(), 1);
    assert_eq!(one.iter().count(), 1);
    assert_eq!(maximum.iter().count(), 1);
    assert_eq!(one.cardinality(), 1);
    assert_eq!(maximum.cardinality(), u64::MAX);
    assert!(!maximum.cardinality_is_saturated());

    let (one_image, one_image_allocations, one_image_bytes) = measure(|| one.compact_bytes());
    let (maximum_image, maximum_image_allocations, maximum_image_bytes) =
        measure(|| maximum.compact_bytes());
    // ACTree03 branchless-varint encodes a `u64` in one to nine bytes. The
    // largest multiplicity may therefore add eight bytes, but never storage
    // proportional to the number of logical occurrences.
    assert_eq!(maximum_image.len() - one_image.len(), 8);
    assert_eq!(one_image_allocations, maximum_image_allocations);
    assert_eq!(maximum_image_bytes - one_image_bytes, 8);
}
