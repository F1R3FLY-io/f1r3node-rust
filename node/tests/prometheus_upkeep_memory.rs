use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use node::rust::diagnostics::new_prometheus_reporter::NewPrometheusReporter;

struct LiveHeapCounter;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for LiveHeapCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: LiveHeapCounter = LiveHeapCounter;

#[test]
fn production_upkeep_releases_raw_observations_before_any_scrape() {
    const INITIAL_SAMPLES: usize = 1_000_000;
    const MIN_BUFFER_BYTES: usize = 8 * 1024 * 1024;
    const RETAINED_LIMIT_BYTES: usize = 1024 * 1024;

    let reporter = NewPrometheusReporter::initialize().unwrap();
    let histogram = metrics::histogram!("upkeep_memory_regression");
    let baseline = LIVE_BYTES.load(Ordering::Relaxed);
    for _ in 0..INITIAL_SAMPLES {
        histogram.record(0.5);
    }
    let buffered_bytes = LIVE_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(
        buffered_bytes >= MIN_BUFFER_BYTES,
        "the raw buffer must be populated before upkeep: {buffered_bytes} bytes"
    );

    let upkeep = reporter.start_upkeep().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut samples = INITIAL_SAMPLES;
    loop {
        let retained_bytes = LIVE_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
        if retained_bytes <= RETAINED_LIMIT_BYTES {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "raw observations remain before scrape: before={buffered_bytes}, after={retained_bytes} bytes"
        );
        for _ in 0..128 {
            histogram.record(0.5);
            samples += 1;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let output = reporter.scrape_data();
    for expected in [
        format!("upkeep_memory_regression_count {samples}"),
        format!("upkeep_memory_regression_sum {}", samples as f64 * 0.5),
        format!("upkeep_memory_regression_bucket{{le=\"0.5\"}} {samples}"),
        format!("upkeep_memory_regression_bucket{{le=\"+Inf\"}} {samples}"),
    ] {
        assert!(output.lines().any(|line| line == expected), "{output}");
    }
    drop(upkeep);
}
