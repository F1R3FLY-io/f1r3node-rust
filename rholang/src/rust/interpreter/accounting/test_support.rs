use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
// Added by DR-132 (F4): the live-heap count of a measured runtime.
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use models::rhoapi::*;
use proptest::prelude::*;

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(None) };
    // Added by DR-132 (F4): the live heap that this thread counts into.
    static LIVE_HEAP: Cell<Option<&'static LiveHeap>> = const { Cell::new(None) };
}

struct MeasuredAllocator;

fn measure(bytes: usize) {
    let _ = ALLOCATED.try_with(|total| {
        if let Some(current) = total.get() {
            total.set(Some(current.checked_add(bytes).unwrap()));
        }
    });
}

/// Added by DR-132 (F4): the live heap of the threads of one measured runtime.
/// The deferral tests check that a chain of continuation bodies, re-issues or
/// system-process replies holds no heap for each step of the chain. The heap
/// counts only while a measurement window is open (`transient`).
#[derive(Default)]
pub(crate) struct LiveHeap {
    live: AtomicIsize,
    peak: AtomicIsize,
    open: AtomicBool,
}

impl LiveHeap {
    /// Added by DR-132 (F4): runs `future` in a measurement window, and
    /// returns its output and its transient heap: the peak of the live bytes
    /// above the larger of the start and end levels of the window, as in
    /// dev's `deferred_dispatch_memory_spec`. State that the window keeps,
    /// such as a recording, is in the end level, so it does not count.
    pub(crate) async fn transient<T>(&self, future: impl Future<Output = T>) -> (T, usize) {
        self.live.store(0, Ordering::SeqCst);
        self.peak.store(0, Ordering::SeqCst);
        self.open.store(true, Ordering::SeqCst);
        let output = future.await;
        self.open.store(false, Ordering::SeqCst);
        let peak = self.peak.load(Ordering::SeqCst);
        let end = self.live.load(Ordering::SeqCst).max(0);
        (output, usize::try_from(peak - end).unwrap_or(0))
    }
}

/// Added by DR-132 (F4): adds `bytes` (negative for a release) to the live
/// heap of this thread, if the thread counts one and its window is open.
fn track_live(bytes: isize) {
    let _ = LIVE_HEAP.try_with(|heap| {
        if let Some(heap) = heap.get() {
            if heap.open.load(Ordering::Relaxed) {
                let live = heap.live.fetch_add(bytes, Ordering::Relaxed) + bytes;
                heap.peak.fetch_max(live, Ordering::Relaxed);
            }
        }
    });
}

/// Added by DR-132 (F4): a byte count of a layout as a signed live-heap step.
/// A layout size never exceeds `isize::MAX`.
fn signed(bytes: usize) -> isize { isize::try_from(bytes).unwrap_or(isize::MAX) }

unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            measure(layout.size());
            track_live(signed(layout.size()));
        }
        result
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            measure(layout.size());
            track_live(signed(layout.size()));
        }
        result
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            measure(size);
            track_live(signed(size) - signed(layout.size()));
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        track_live(-signed(layout.size()));
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

/// Added by DR-132 (F4): runs `body` on a new runtime with `workers` worker
/// threads. Each thread of the runtime counts into one live heap, and so does
/// this thread while it drives the body. The body opens a measurement window
/// with `LiveHeap::transient`. Measurements on other runtimes do not
/// interfere.
pub(crate) fn with_live_heap<T, F: Future<Output = T>>(
    workers: usize,
    body: impl FnOnce(&'static LiveHeap) -> F,
) -> T {
    let heap: &'static LiveHeap = Box::leak(Box::default());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .on_thread_start(move || LIVE_HEAP.with(|slot| slot.set(Some(heap))))
        .on_thread_stop(|| LIVE_HEAP.with(|slot| slot.set(None)))
        .build()
        .expect("a measured runtime");
    LIVE_HEAP.with(|slot| slot.set(Some(heap)));
    let output = runtime.block_on(body(heap));
    LIVE_HEAP.with(|slot| slot.set(None));
    drop(runtime);
    output
}

pub(crate) fn term() -> impl Strategy<Value = Par> {
    (prop::collection::vec(any::<u8>(), 0..256), 0_usize..32)
        .prop_map(|(bytes, count)| Par {
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
            prop::collection::vec(child, 0..6).prop_map(|children| Par {
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
