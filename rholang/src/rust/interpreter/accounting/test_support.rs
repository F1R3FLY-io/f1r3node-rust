use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use models::rhoapi::*;
use proptest::prelude::*;

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(None) };
}

struct MeasuredAllocator;

fn measure(bytes: usize) {
    let _ = ALLOCATED.try_with(|total| {
        if let Some(current) = total.get() {
            total.set(Some(current.checked_add(bytes).unwrap()));
        }
    });
}

unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            measure(layout.size());
        }
        result
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            measure(layout.size());
        }
        result
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            measure(size);
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

struct Measurement;

impl Drop for Measurement {
    fn drop(&mut self) { ALLOCATED.with(|total| total.set(None)); }
}

/// Bytes allocated on this thread while `action` runs, measured by the
/// test binary's global allocator.
pub(crate) fn measured<T>(action: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATED.with(|total| {
        assert!(total.get().is_none(), "measurements do not nest");
        total.set(Some(0));
    });
    let measurement = Measurement;
    let value = action();
    let bytes = ALLOCATED.with(|total| total.get().expect("active measurement"));
    drop(measurement);
    (value, bytes)
}

pub(crate) fn term() -> impl Strategy<Value = Par> {
    (prop::collection::vec(any::<u8>(), 0..256), 0_usize..32)
        // Changed by the merge of feature/cost-accounted-rho: F1R3Lang's `Par` has a
        // generated iterative `Drop`, so struct-update syntax cannot move the other
        // fields out of the base (E0509). `models::par_from_default!` assigns them.
        .prop_map(|(bytes, count)| models::par_from_default! {
            exprs: vec![
                Expr {
                    expr_instance: Some(expr::ExprInstance::GByteArray(bytes))
                };
                count
            ],
            locally_free: vec![7; count],
            ..Default::default()
        })
        .prop_recursive(3, 64, 6, |child| {
            // Changed by the merge of feature/cost-accounted-rho: F1R3Lang's `Par` has a
            // generated iterative `Drop`, so struct-update syntax cannot move the other
            // fields out of the base (E0509). `models::par_from_default!` assigns them.
            prop::collection::vec(child, 0..6).prop_map(|children| models::par_from_default! {
                sends: vec![Send {
                    data: children.clone(),
                    ..Default::default()
                }],
                news: vec![New {
                    injections: children
                        .into_iter()
                        .enumerate()
                        .map(|(index, child)| (index.to_string(), child))
                        .collect(),
                    ..Default::default()
                }],
                ..Default::default()
            })
        })
}
