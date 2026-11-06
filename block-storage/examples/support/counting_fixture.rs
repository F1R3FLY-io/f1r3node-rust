use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use models::rust::block_hash::BlockHash;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig, GB,
};

struct CountingAllocator;
pub static LIVE: AtomicUsize = AtomicUsize::new(0);
pub static PEAK: AtomicUsize = AtomicUsize::new(0);
static USABLE_LIVE: AtomicUsize = AtomicUsize::new(0);
static USABLE_PEAK: AtomicUsize = AtomicUsize::new(0);
static OVERHEAD_LIVE: AtomicUsize = AtomicUsize::new(0);
static OVERHEAD_PEAK: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn malloc_usable_size(ptr: *mut std::ffi::c_void) -> usize;
}

unsafe fn usable_size(_ptr: *mut u8, _requested: usize) -> usize {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe { malloc_usable_size(_ptr.cast()) }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        _requested
    }
}

fn adjust(live: &AtomicUsize, peak: &AtomicUsize, old: usize, new: usize) {
    if new >= old {
        let delta = new - old;
        let value = live.fetch_add(delta, Ordering::Relaxed) + delta;
        peak.fetch_max(value, Ordering::Relaxed);
    } else {
        live.fetch_sub(old - new, Ordering::Relaxed);
    }
}

fn allocation_changed(old: usize, new: usize, old_usable: usize, new_usable: usize) {
    adjust(&LIVE, &PEAK, old, new);
    adjust(&USABLE_LIVE, &USABLE_PEAK, old_usable, new_usable);
    adjust(
        &OVERHEAD_LIVE,
        &OVERHEAD_PEAK,
        old_usable.saturating_sub(old),
        new_usable.saturating_sub(new),
    );
}

pub fn reset_peaks() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    USABLE_PEAK.store(USABLE_LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    OVERHEAD_PEAK.store(OVERHEAD_LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn usable_json() -> String {
    if cfg!(all(target_os = "linux", target_env = "gnu")) {
        let live = USABLE_LIVE.load(Ordering::Relaxed);
        let peak = USABLE_PEAK.load(Ordering::Relaxed);
        let overhead = OVERHEAD_LIVE.load(Ordering::Relaxed);
        let overhead_peak = OVERHEAD_PEAK.load(Ordering::Relaxed);
        format!("\"allocator_usable_available\":true,\"allocator_usable_live_bytes\":{live},\"allocator_usable_peak_bytes\":{peak},\"allocator_live_overhead_bytes\":{overhead},\"allocator_peak_overhead_bytes\":{overhead_peak},\"allocator_arena_fragmentation\":\"excluded\"")
    } else {
        "\"allocator_usable_available\":false,\"allocator_usable_live_bytes\":null,\"allocator_usable_peak_bytes\":null,\"allocator_live_overhead_bytes\":null,\"allocator_peak_overhead_bytes\":null,\"allocator_arena_fragmentation\":\"unavailable\"".to_string()
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocation_changed(0, layout.size(), 0, unsafe {
                usable_size(ptr, layout.size())
            });
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            allocation_changed(0, layout.size(), 0, unsafe {
                usable_size(ptr, layout.size())
            });
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let usable = unsafe { usable_size(ptr, layout.size()) };
        unsafe { System.dealloc(ptr, layout) };
        allocation_changed(layout.size(), 0, usable, 0);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_usable = unsafe { usable_size(ptr, layout.size()) };
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            allocation_changed(layout.size(), new_size, old_usable, unsafe {
                usable_size(new_ptr, new_size)
            });
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

pub fn manager(path: PathBuf) -> impl KeyValueStoreManager {
    let config = LmdbEnvConfig::new("dagstorage".to_string(), 8 * GB);
    let mapping: HashMap<_, _> = [
        "block-metadata",
        "equivocation-tracker",
        "latest-messages",
        "invalid-blocks",
        "floor-index",
        "frontier-index",
        "deploy-lifecycle-events",
        "deploy-lifecycle-terminal",
        "carrier-index",
        "carrier-index-meta",
        "genesis-hash",
        "dag-membership-v1",
        "dag-height-v1",
        "dag-children-v1",
        "dag-history-state-v1",
        "dag-gc-retry-v1",
        "dag-gc-work-v1",
        "dag-gc-visited-v1",
    ]
    .into_iter()
    .map(|name| (Db::new(name.to_string(), None), config.clone()))
    .collect();
    LmdbDirStoreManager::new(path, mapping)
}

pub fn hash(index: usize) -> BlockHash {
    let mut bytes = vec![0; 32];
    bytes[24..].copy_from_slice(&(index as u64).to_be_bytes());
    bytes.into()
}
