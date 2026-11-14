#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::rust::metrics_constants::{
        ALLOCATOR_TRIM_TOTAL_METRIC, BLOCK_PROCESSOR_METRICS_SOURCE,
    };

    const TRIM_INTERVAL_BLOCKS: u64 = 64;
    static BLOCKS_SINCE_START: AtomicU64 = AtomicU64::new(0);

    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }

    pub fn trim_after_block() {
        let blocks = BLOCKS_SINCE_START.fetch_add(1, Ordering::Relaxed) + 1;
        if blocks.is_multiple_of(TRIM_INTERVAL_BLOCKS) {
            // Return free heap pages to the OS so RSS does not ratchet up.
            unsafe {
                let _ = malloc_trim(0);
            }
            metrics::counter!(ALLOCATOR_TRIM_TOTAL_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .increment(1);
        }
    }
}

/// Every 64th validated block, return free heap pages to the OS (glibc only).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn trim_after_block() { glibc::trim_after_block() }

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn trim_after_block() {}
