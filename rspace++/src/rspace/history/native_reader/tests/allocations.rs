use std::alloc::{GlobalAlloc, Layout, System};

use super::*;

thread_local! {
    static REQUESTED: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn record(bytes: usize) {
    let _ = REQUESTED.try_with(|value| {
        if let Some(total) = value.get() {
            value.set(Some(total + bytes));
        }
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

struct Recording;

impl Drop for Recording {
    fn drop(&mut self) { REQUESTED.with(|value| value.set(None)); }
}

fn measure<T>(action: impl FnOnce() -> T) -> (T, usize) {
    REQUESTED.with(|value| {
        assert!(value.get().is_none());
        value.set(Some(0));
    });
    let recording = Recording;
    let result = action();
    let bytes = REQUESTED.with(|value| value.get().unwrap());
    drop(recording);
    (result, bytes)
}

fn allocation_case(
    nodes: &dyn KeyValueStore,
    leaves: &dyn KeyValueStore,
    serialized: bool,
) -> (usize, usize) {
    let rows = vec![vec![42_u8; 1_000_000]];
    let payload = bincode::serialize(&rows).unwrap();
    let (hash, bytes) = frame(NativeLeafKind::Data, &payload, &[]);
    let root = install(nodes, leaves, NativeLeafKind::Data, hash, bytes, serialized);
    let reader = NativeHistoryReader::new(root, nodes, leaves);
    let meter = Meter::new(usize::MAX);
    let (result, actual) = measure(|| {
        reader.with_records(NativeLeafKind::Data, &[7; 32], &meter, |view| {
            let mut rows = view.iter();
            let row = rows.next().unwrap();
            assert_eq!(row.len(), 1_000_000);
            assert_eq!(row[0], 42);
            assert!(rows.next().is_none());
            Ok(())
        })
    });
    result.unwrap().unwrap();
    (actual, meter.backing.load(Ordering::Relaxed))
}

#[test]
fn in_memory_framing_never_copies_the_cold_payload() {
    for serialized in [false, true] {
        let (actual, reserved) = allocation_case(
            &InMemoryKeyValueStore::new(),
            &InMemoryKeyValueStore::new(),
            serialized,
        );
        assert_eq!(actual, 40);
        assert!(actual <= reserved);
    }
}

#[test]
fn lmdb_framing_and_key_codec_fit_the_reserved_backing() {
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/verification/native-history-tests");
    std::fs::create_dir_all(&parent).unwrap();
    for serialized in [false, true] {
        let dir = tempfile::Builder::new()
            .prefix("borrowed-reader-")
            .tempdir_in(&parent)
            .unwrap();
        let env = Arc::new(unsafe {
            heed::EnvOpenOptions::new()
                .map_size(16 * 1024 * 1024)
                .max_dbs(2)
                .open(dir.path())
                .unwrap()
        });
        let mut transaction = env.write_txn().unwrap();
        let nodes_db = env
            .create_database(&mut transaction, Some("nodes"))
            .unwrap();
        let leaves_db = env
            .create_database(&mut transaction, Some("leaves"))
            .unwrap();
        transaction.commit().unwrap();
        let nodes = LmdbKeyValueStore::new(Arc::clone(&env), nodes_db);
        let leaves = LmdbKeyValueStore::new(Arc::clone(&env), leaves_db);
        let (actual, reserved) = allocation_case(&nodes, &leaves, serialized);
        assert!(actual <= reserved, "{actual} allocated bytes exceed {reserved} reserved bytes");
    }
}

#[test]
fn host_rejection_precedes_reader_allocation() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let reader = NativeHistoryReader::new([0; 32], &nodes, &leaves);
    let meter = Meter::new(0);
    let (result, bytes) =
        measure(|| reader.with_records(NativeLeafKind::Data, &[0; 32], &meter, |_| Ok(())));
    assert!(matches!(result, Err(NativeReadError::Host("exhausted"))));
    assert_eq!(bytes, 0);
}
