use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

fn bump(bytes: usize) {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    let _ = BYTES.try_with(|count| count.set(count.get() + bytes));
}

pub struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn read() -> (usize, usize) {
    (
        ALLOCATIONS.try_with(Cell::get).unwrap_or(0),
        BYTES.try_with(Cell::get).unwrap_or(0),
    )
}

pub fn measure<R>(body: impl FnOnce() -> R) -> (R, usize, usize) {
    let (allocations_before, bytes_before) = read();
    let result = body();
    let (allocations_after, bytes_after) = read();
    (
        result,
        allocations_after - allocations_before,
        bytes_after - bytes_before,
    )
}

pub fn assert_subquadratic(
    label: &str,
    low_depth: usize,
    low: usize,
    high_depth: usize,
    high: usize,
) -> Result<(), String> {
    let depth_ratio = high_depth / low_depth;
    (high <= low.saturating_mul(depth_ratio + depth_ratio / 2))
        .then_some(())
        .ok_or_else(|| {
            format!(
                "{label} grew from {low} at depth {low_depth} to {high} at depth {high_depth}; \
                 this exceeds a 1.5x envelope around linear encoded-input growth"
            )
        })
}
