use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sample {
    pub allocated_bytes: u128,
    pub freed_bytes: u128,
    pub allocations: u128,
    pub live_delta: i128,
    pub peak_delta: i128,
    pub phase_peak: i128,
    pub max_request_bytes: usize,
    pub phase_max_request_bytes: usize,
}

impl Sample {
    fn allocate(&mut self, bytes: usize) {
        self.allocated_bytes += bytes as u128;
        self.allocations += 1;
        self.live_delta += bytes as i128;
        self.peak_delta = self.peak_delta.max(self.live_delta);
        self.phase_peak = self.phase_peak.max(self.live_delta);
        self.max_request_bytes = self.max_request_bytes.max(bytes);
        self.phase_max_request_bytes = self.phase_max_request_bytes.max(bytes);
    }

    fn free(&mut self, bytes: usize) {
        self.freed_bytes += bytes as u128;
        self.live_delta -= bytes as i128;
    }
}

thread_local! {
    static ACTIVE: Cell<Option<Sample>> = const { Cell::new(None) };
    static MARKERS: Cell<[Option<Sample>; 4]> = const { Cell::new([None; 4]) };
}

struct ProbeAllocator;

#[global_allocator]
static ALLOCATOR: ProbeAllocator = ProbeAllocator;

fn update(change: impl FnOnce(&mut Sample)) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut sample) = active.get() {
            change(&mut sample);
            active.set(Some(sample));
        }
    });
}

unsafe impl GlobalAlloc for ProbeAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            update(|sample| sample.allocate(layout.size()));
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            update(|sample| sample.allocate(layout.size()));
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        update(|sample| sample.free(layout.size()));
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            update(|sample| {
                sample.free(layout.size());
                sample.allocate(size);
            });
        }
        replacement
    }
}

struct Capture;

impl Drop for Capture {
    fn drop(&mut self) { ACTIVE.with(|active| active.set(None)); }
}

pub(crate) fn measure<T>(operation: impl FnOnce() -> T) -> (T, Sample) {
    ACTIVE.with(|active| {
        assert!(active.get().is_none(), "allocation captures cannot nest");
        active.set(Some(Sample::default()));
    });
    MARKERS.with(|markers| markers.set([None; 4]));
    let capture = Capture;
    let value = operation();
    let sample = ACTIVE.with(|active| active.get().unwrap());
    drop(capture);
    (value, sample)
}

pub(crate) fn checkpoint() -> Sample {
    ACTIVE.with(|active| {
        let sample = active
            .get()
            .expect("allocation checkpoint requires an active capture");
        active.set(Some(Sample {
            phase_peak: sample.live_delta,
            phase_max_request_bytes: 0,
            ..sample
        }));
        sample
    })
}

pub(crate) fn mark(index: usize) {
    if ACTIVE.with(|active| active.get().is_none()) {
        return;
    }
    let sample = checkpoint();
    MARKERS.with(|markers| {
        let mut values = markers.get();
        assert!(values[index].is_none(), "allocation phase marker repeated");
        values[index] = Some(sample);
        markers.set(values);
    });
}

pub(crate) fn markers() -> [Option<Sample>; 4] { MARKERS.with(Cell::get) }

pub(crate) fn interval(before: Sample, after: Sample) -> Sample {
    Sample {
        allocated_bytes: after.allocated_bytes - before.allocated_bytes,
        freed_bytes: after.freed_bytes - before.freed_bytes,
        allocations: after.allocations - before.allocations,
        live_delta: after.live_delta - before.live_delta,
        peak_delta: (after.phase_peak - before.live_delta).max(0),
        phase_peak: (after.phase_peak - before.live_delta).max(0),
        max_request_bytes: after.phase_max_request_bytes,
        phase_max_request_bytes: after.phase_max_request_bytes,
    }
}

pub(crate) fn report(phase: &str, items: usize, payload: usize, sample: Sample) {
    println!(
        "RESOURCE phase={phase} items={items} payload_bytes={payload} allocations={} allocated_bytes={} freed_bytes={} live_delta_bytes={} peak_delta_bytes={} phase_peak_bytes={} max_request_bytes={}",
        sample.allocations, sample.allocated_bytes, sample.freed_bytes,
        sample.live_delta, sample.peak_delta, sample.phase_peak, sample.max_request_bytes,
    );
}

#[test]
fn allocation_probe_accounts_for_reallocation_and_phase_boundaries() {
    let (phase, total) = measure(|| unsafe {
        let first = Layout::from_size_align(64, 8).unwrap();
        let pointer = ALLOCATOR.alloc_zeroed(first);
        assert!(!pointer.is_null());
        assert_eq!(*pointer, 0);
        let pointer = ALLOCATOR.realloc(pointer, first, 128);
        assert!(!pointer.is_null());
        let phase = checkpoint();
        ALLOCATOR.dealloc(pointer, Layout::from_size_align(128, 8).unwrap());
        phase
    });
    assert_eq!(phase.allocations, 2);
    assert_eq!(phase.allocated_bytes, 192);
    assert_eq!(phase.freed_bytes, 64);
    assert_eq!(phase.live_delta, 128);
    assert_eq!(total.live_delta, 0);
    assert_eq!(total.peak_delta, 128);
    assert_eq!(total.phase_peak, 128);
    assert_eq!(total.allocated_bytes, total.freed_bytes);
}

#[test]
fn allocation_probe_deactivates_after_unwind_and_keeps_threads_separate() {
    let panic = std::panic::catch_unwind(|| measure(|| panic!("probe unwind")));
    assert!(panic.is_err());
    let (_, sample) = measure(|| ());
    assert_eq!(sample.allocated_bytes, 0);
    assert_eq!(sample.freed_bytes, 0);
    std::thread::spawn(|| {
        assert!(ACTIVE.with(|active| active.get().is_none()));
        let (_, sample) = measure(|| ());
        assert_eq!(sample.allocations, 0);
    })
    .join()
    .unwrap();
}
