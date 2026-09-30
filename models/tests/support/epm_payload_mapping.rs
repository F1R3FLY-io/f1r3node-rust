use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use pathmap::zipper::Zipper;
use proptest::prelude::*;

use super::*;

thread_local! {
    static SPANS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static METER: Cell<Option<AllocationCounts>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AllocationCounts {
    pub allocations: u64,
    pub allocated: u64,
    pub freed: u64,
    pub peak: i128,
    pub overflow: bool,
}

impl AllocationCounts {
    pub(crate) fn outstanding(self) -> i128 { self.allocated as i128 - self.freed as i128 }

    fn record(&mut self, allocated: usize, freed: usize) {
        fn add(total: &mut u64, n: u64, overflow: &mut bool) {
            match total.checked_add(n) {
                Some(next) => *total = next,
                None => *overflow = true,
            }
        }
        add(
            &mut self.allocations,
            u64::from(allocated != 0),
            &mut self.overflow,
        );
        add(&mut self.allocated, allocated as u64, &mut self.overflow);
        add(&mut self.freed, freed as u64, &mut self.overflow);
        self.peak = self.peak.max(self.outstanding());
    }
}

struct MeterGuard;

impl Drop for MeterGuard {
    fn drop(&mut self) { METER.with(|meter| meter.set(None)); }
}

pub(crate) fn allocation_checkpoint() -> AllocationCounts {
    METER.with(|meter| meter.get().expect("active allocation capture"))
}

pub(crate) fn capture_allocations<T>(run: impl FnOnce() -> T) -> (T, AllocationCounts) {
    METER.with(|meter| {
        assert!(meter.get().is_none(), "nested allocation capture");
        meter.set(Some(AllocationCounts::default()));
    });
    let guard = MeterGuard;
    let result = run();
    let counts = allocation_checkpoint();
    drop(guard);
    (result, counts)
}

pub(super) fn observe_span(_: usize, scaffold: bool) {
    SPANS.with(|s| {
        let (events, scaffolds) = s.get();
        s.set((events + 1, scaffolds + usize::from(scaffold)));
    });
}

struct Allocator;
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn record(allocated: usize, freed: usize) {
    let _ = METER.try_with(|meter| {
        if let Some(mut counts) = meter.get() {
            counts.record(allocated, freed);
            meter.set(Some(counts));
        }
    });
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            record(layout.size(), 0);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            record(layout.size(), 0);
        }
        p
    }
    unsafe fn realloc(&self, p: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(p, old, size) };
        if !next.is_null() {
            record(size, old.size());
        }
        next
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        record(0, layout.size());
        unsafe { System.dealloc(p, layout) };
    }
}

#[derive(Clone, Copy, Debug)]
struct Token(u64);
impl payload_seal::Sealed for Token {}
impl EpmPayload for Token {
    fn dispose(self) {}
}

#[derive(Clone, Copy)]
struct Policy;
impl DisposeValue<Token> for Policy {
    fn dispose(&self, _: Token) {}
}
impl rspace_plus_plus::rspace::pathmap_mutation::TeardownBarrier<Token> for Policy {
    fn make_barrier() -> Token { Token(u64::MAX) }
}

fn source(paths: &[(Vec<u8>, bool)]) -> EpmReprOwner<Token, PendingScaffold<Token>> {
    let mut map = PathMap::new();
    for (i, (path, valued)) in paths.iter().enumerate() {
        if *valued {
            map.insert(path, Token(i as u64));
        }
    }
    for (path, valued) in paths {
        if !valued {
            rspace_plus_plus::rspace::pathmap_mutation::NativePathCursor::<_, _, Policy>::new(
                &mut map, path, &Policy,
            )
            .create_path();
        }
    }
    EpmReprOwner::new(
        EPathMapRepr::Map(map),
        PendingScaffold::Repeat(Token(u64::MAX)),
    )
}

fn integer(value: u64) -> Par {
    Par::default().with_exprs(vec![crate::rhoapi::Expr {
        expr_instance: Some(crate::rhoapi::expr::ExprInstance::GInt(value as i64)),
    }])
}

fn topology<V: EpmPayload>(map: &PathMap<V>) -> BTreeSet<Vec<u8>> {
    let mut reader = map.read_zipper();
    let mut frames = vec![reader.child_mask().iter()];
    let mut paths = BTreeSet::new();
    while let Some(children) = frames.last_mut() {
        if let Some(byte) = children.next() {
            reader.descend_to_byte(byte);
            paths.insert(reader.path().to_vec());
            frames.push(reader.child_mask().iter());
        } else {
            frames.pop();
            if !frames.is_empty() {
                reader.ascend_byte();
            }
        }
    }
    paths
}

fn compare(paths: Vec<(Vec<u8>, bool)>) {
    let owner = source(&paths);
    let sibling = EpmReprOwner::new(
        owner.as_repr().clone(),
        PendingScaffold::Repeat(Token(u64::MAX)),
    );
    let mut expected = BTreeMap::new();
    for (i, (path, valued)) in paths.iter().enumerate() {
        if *valued {
            expected.insert(path.clone(), i as u64);
        }
    }
    let expected_paths: BTreeSet<_> = paths
        .iter()
        .flat_map(|(p, _)| (1..=p.len()).map(|n| p[..n].to_vec()))
        .collect();
    let mut trace = Vec::new();
    let mapped = owner
        .try_map_resident::<()>(|v| {
            trace.push(v.0);
            Ok(integer(v.0))
        })
        .unwrap();
    assert_eq!(trace, expected.values().copied().collect::<Vec<_>>());
    let EPathMapRepr::Map(map) = mapped.as_repr() else {
        panic!("lost map mode")
    };
    assert_eq!(topology(map), expected_paths);
    assert_eq!(map.val_count(), expected.len());
    for (key, value) in expected {
        assert_eq!(map.get(&key), Some(&integer(value)));
    }
    let EPathMapRepr::Map(map) = sibling.as_repr() else {
        unreachable!()
    };
    assert_eq!(topology(map), expected_paths);
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/epm_payload_mapping.regressions")
        ))),
        ..ProptestConfig::default()
    })]
    #[test]
    fn ordered_mapping_preserves_all_bindings_and_topology(
        paths in prop::collection::vec((prop::collection::vec(0u8..8, 0..16), any::<bool>()), 0..24),
    ) { compare(paths); }
}

#[test]
fn modes_root_prefix_dense_siblings_and_repeated_values_are_exact() {
    let mut set = PathMap::new();
    set.insert(b"a", ());
    set.create_path(b"abcd");
    let expected_paths = topology(&set);
    let owner = EpmReprOwner::new(EPathMapRepr::Set(set), PendingScaffold::Repeat(Token(0)));
    let mapped = owner
        .try_map_resident::<()>(|_| panic!("set callback"))
        .unwrap();
    let EPathMapRepr::Set(set) = mapped.as_repr() else {
        panic!("lost set mode")
    };
    assert_eq!(topology(set), expected_paths);
    assert!(set.contains(b"a"));
    assert_eq!(set.val_count(), 1);
    for repr in [
        EPathMapRepr::Empty,
        EPathMapRepr::Set(PathMap::new()),
        EPathMapRepr::Map(PathMap::new()),
    ] {
        let mode = repr.mode();
        let owner = EpmReprOwner::new(repr, PendingScaffold::Repeat(Token(0)));
        let mapped = owner
            .try_map_resident::<()>(|_| panic!("empty callback"))
            .unwrap();
        assert_eq!(mapped.as_repr().mode(), mode);
    }
    let mut paths = vec![
        (vec![], true),
        (vec![0], true),
        (vec![0, 1], true),
        (vec![4, 9], false),
    ];
    paths.extend((0..=255).map(|byte| (vec![255, byte], byte % 2 == 0)));
    compare(paths);
    let mut map = PathMap::new();
    for key in [b"".as_slice(), b"a", b"ab", b"b"] {
        map.insert(key, Token(7));
    }
    let owner = EpmReprOwner::new(EPathMapRepr::Map(map), PendingScaffold::Repeat(Token(0)));
    let mut callbacks = 0;
    let mapped = owner
        .try_map_resident::<()>(|v| {
            callbacks += 1;
            Ok(integer(v.0))
        })
        .unwrap();
    assert_eq!(callbacks, 4);
    let EPathMapRepr::Map(map) = mapped.as_repr() else {
        unreachable!()
    };
    assert_eq!(map.val_count(), 4);
}

#[test]
fn first_error_and_panic_stop_callbacks_without_publishing() {
    let paths: Vec<_> = (0..8).map(|v| (vec![v], true)).collect();
    for at in 0..8 {
        let mut calls = 0;
        let result = source(&paths).try_map_resident(|v| {
            let index = calls;
            calls += 1;
            if index == at {
                return Err(index);
            }
            Ok(integer(v.0))
        });
        assert_eq!(calls, at + 1);
        assert!(matches!(result, Err(error) if error == at));
        observe_payload_mapping_unwind(&format!("callback:{at}"), "injected callback interruption");
    }
}

fn observe_payload_mapping_unwind(case: &str, diagnostic: &str) {
    let scratch = shared::rust::test_scratch::acquire("epm-payload-mapping-unwind-");
    let marker = scratch.path().join("verified");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rust::epathmap_trie_codec::payload_mapping_tests::payload_mapping_unwind_child",
            "--nocapture",
        ])
        .env("EPM_PAYLOAD_MAPPING_CASE", case)
        .env("EPM_PAYLOAD_MAPPING_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the EPathMap payload mapping child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(
                failure.kind,
                shared::rust::test_process::ProbeFailureKind::Panicked
            );
            assert!(failure.stderr.contains(diagnostic), "{}", failure.stderr);
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the EPathMap payload mapping child did not fail")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), b"verified");
}

#[test]
#[ignore = "isolated EPathMap payload mapping unwind child"]
fn payload_mapping_unwind_child() {
    let Ok(case) = std::env::var("EPM_PAYLOAD_MAPPING_CASE") else {
        return;
    };
    let marker =
        std::env::var_os("EPM_PAYLOAD_MAPPING_MARKER").expect("EPathMap payload mapping marker");
    let marker: std::path::PathBuf = marker.into();
    match case.as_str() {
        callback if callback.starts_with("callback:") => {
            let at: usize = callback[9..].parse().expect("callback stop index");
            assert!(at < 8);
            struct CallbackCountOnDrop {
                calls: std::rc::Rc<Cell<usize>>,
                expected: usize,
                marker: std::path::PathBuf,
            }
            impl Drop for CallbackCountOnDrop {
                fn drop(&mut self) {
                    assert_eq!(self.calls.get(), self.expected);
                    std::fs::write(&self.marker, b"verified").unwrap();
                }
            }
            let calls = std::rc::Rc::new(Cell::new(0));
            let _checked = CallbackCountOnDrop {
                calls: std::rc::Rc::clone(&calls),
                expected: at + 1,
                marker,
            };
            let paths: Vec<_> = (0..8).map(|v| (vec![v], true)).collect();
            let _ = source(&paths).try_map_resident::<()>(|v| {
                let index = calls.get();
                calls.set(index + 1);
                if index == at {
                    panic!("injected callback interruption");
                }
                Ok(integer(v.0))
            });
        }
        "meter_unwind" | "meter_nested" => {
            struct MeterClearedOnDrop {
                marker: std::path::PathBuf,
            }
            impl Drop for MeterClearedOnDrop {
                fn drop(&mut self) {
                    assert!(METER.with(Cell::get).is_none());
                    std::fs::write(&self.marker, b"verified").unwrap();
                }
            }
            struct OuterMeterOnDrop;
            impl Drop for OuterMeterOnDrop {
                fn drop(&mut self) {
                    assert!(METER.with(Cell::get).is_some());
                }
            }
            let _checked = MeterClearedOnDrop { marker };
            if case == "meter_unwind" {
                let _ = capture_allocations(|| panic!("meter unwind"));
            } else {
                let _ = capture_allocations(|| {
                    let _outer = OuterMeterOnDrop;
                    capture_allocations(|| ());
                });
            }
        }
        "deep" => {
            std::thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(move || {
                    struct ReachedOnDrop(std::path::PathBuf);
                    impl Drop for ReachedOnDrop {
                        fn drop(&mut self) { std::fs::write(&self.0, b"verified").unwrap(); }
                    }
                    let _reached = ReachedOnDrop(marker);
                    let mut paths = Vec::new();
                    let mut prefix = Vec::new();
                    for i in 0..8192 {
                        prefix.push(1);
                        paths.push((prefix.clone(), i % 1024 == 0));
                        *prefix.last_mut().unwrap() = 0;
                    }
                    let _ = source(&paths).try_map_resident::<()>(|_| panic!("deep callback"));
                })
                .unwrap()
                .join()
                .unwrap();
        }
        _ => panic!("unknown EPathMap payload mapping child case"),
    }
}

#[test]
fn compressed_stems_have_bounded_frames_scaffolds_and_retained_heap() {
    for length in [64, 1024, 8192] {
        let paths = vec![(vec![1; length], false)];
        SPANS.with(|s| s.set((0, 0)));
        let mapped = source(&paths)
            .try_map_resident::<()>(|_| panic!("dangling callback"))
            .unwrap();
        assert_eq!(SPANS.with(Cell::get), (2, 1));
        let ((), counts) = capture_allocations(|| drop(mapped));
        assert!(!counts.overflow);
        let (allocated, freed) = (counts.allocated, counts.freed);
        let retained = freed
            .checked_sub(allocated)
            .expect("drop frees retained trie storage");
        eprintln!("mapping stem bytes={length} retained_heap_bytes={retained}");
        assert!(
            retained < (length * 8 + 4096) as u64,
            "unary prefix fragmented: {retained}"
        );
    }
}

proptest! {
    #[test]
    fn allocation_trace_matches_signed_reference(events in prop::collection::vec((0usize..65536, 0usize..65536), 0..256), split in any::<usize>()) {
        let mut counts = AllocationCounts::default();
        let mut balance = 0i128;
        let mut peak = 0i128;
        let mut requested = 0u64;
        let mut freed = 0u64;
        let mut calls = 0u64;
        for &(a, f) in &events {
            let old_peak = counts.peak;
            counts.record(a, f);
            balance += a as i128 - f as i128;
            peak = peak.max(balance);
            requested += a as u64;
            freed += f as u64;
            calls += u64::from(a != 0);
            prop_assert_eq!(counts.outstanding(), balance);
            prop_assert_eq!(counts.peak, peak);
            prop_assert!(counts.peak >= old_peak && counts.peak >= balance);
        }
        prop_assert_eq!(counts.allocated, requested);
        prop_assert_eq!(counts.freed, freed);
        prop_assert_eq!(counts.allocations, calls);
        prop_assert!(!counts.overflow);
        let at = split % (events.len() + 1);
        let mut composed = AllocationCounts::default();
        for part in [&events[..at], &events[at..]] {
            for &(a, f) in part { composed.record(a, f); }
        }
        prop_assert_eq!(counts, composed);
    }
}

#[test]
fn allocation_meter_tracks_lifecycle_and_reallocation() {
    let ((live, grown, shrunk), complete) = capture_allocations(|| unsafe {
        let layout = Layout::from_size_align(137, 8).unwrap();
        let pointer = std::alloc::alloc(layout);
        assert!(!pointer.is_null());
        let live = allocation_checkpoint();
        let pointer = std::alloc::realloc(pointer, layout, 1024);
        assert!(!pointer.is_null());
        let grown = allocation_checkpoint();
        let layout = Layout::from_size_align(1024, 8).unwrap();
        let pointer = std::alloc::realloc(pointer, layout, 64);
        assert!(!pointer.is_null());
        let shrunk = allocation_checkpoint();
        std::alloc::dealloc(pointer, Layout::from_size_align(64, 8).unwrap());
        (live, grown, shrunk)
    });
    assert_eq!(
        (live.allocations, live.outstanding(), live.peak),
        (1, 137, 137)
    );
    assert_eq!(
        (grown.allocations, grown.outstanding(), grown.peak),
        (2, 1024, 1024)
    );
    assert_eq!(
        (shrunk.allocations, shrunk.outstanding(), shrunk.peak),
        (3, 64, 1024)
    );
    assert_eq!(
        (complete.outstanding(), complete.allocated, complete.freed),
        (0, 1225, 1225)
    );
    assert!(!complete.overflow);
}

#[test]
fn allocation_meter_scopes_restore_after_unwind_and_reject_nesting() {
    assert_eq!(capture_allocations(|| ()).1, AllocationCounts::default());
    observe_payload_mapping_unwind("meter_unwind", "meter unwind");
    observe_payload_mapping_unwind("meter_nested", "nested allocation capture");
    assert!(METER.with(Cell::get).is_none());
}

#[test]
fn allocation_meter_is_thread_local_and_observes_external_frees() {
    let left = std::thread::spawn(|| capture_allocations(|| std::hint::black_box(vec![0u8; 137])));
    let right = std::thread::spawn(|| capture_allocations(|| std::hint::black_box(vec![0u8; 251])));
    let (left, lc) = left.join().unwrap();
    let (right, rc) = right.join().unwrap();
    assert_eq!(lc.outstanding(), 137);
    assert_eq!(rc.outstanding(), 251);
    assert_eq!(capture_allocations(|| drop(left)).1.outstanding(), -137);
    assert_eq!(capture_allocations(|| drop(right)).1.outstanding(), -251);
}

#[test]
fn allocation_meter_overflow_invalidates_instead_of_wrapping() {
    let mut counts = AllocationCounts {
        allocated: u64::MAX,
        ..Default::default()
    };
    counts.record(1, 0);
    assert!(counts.overflow);
    assert_eq!(counts.allocated, u64::MAX);
}

#[test]
fn deep_branching_success_error_and_unwind_are_stack_safe() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut paths = Vec::new();
            let mut prefix = Vec::new();
            for i in 0..8192 {
                prefix.push(1);
                paths.push((prefix.clone(), i % 1024 == 0));
                *prefix.last_mut().unwrap() = 0;
            }
            for failure in [false, true] {
                let mut calls = 0;
                let mapped = source(&paths).try_map_resident(|_| {
                    calls += 1;
                    if failure && calls == 4 {
                        return Err(());
                    }
                    Ok(Par::default())
                });
                assert_eq!(mapped.is_err(), failure);
                drop(mapped);
            }
        })
        .unwrap()
        .join()
        .unwrap();
    observe_payload_mapping_unwind("deep", "deep callback");
}
