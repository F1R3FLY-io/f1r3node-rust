use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use pathmap::zipper::Zipper;
use proptest::prelude::*;

use super::*;

thread_local! {
    static SPANS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static METER: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
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
        if let Some((a, f)) = meter.get() {
            meter.set(Some((a + allocated, f + freed)));
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
        for panic in [false, true] {
            let mut calls = 0;
            let result = catch_unwind(AssertUnwindSafe(|| {
                source(&paths).try_map_resident(|v| {
                    let index = calls;
                    calls += 1;
                    if index == at {
                        if panic {
                            panic!("injected callback interruption");
                        }
                        return Err(index);
                    }
                    Ok(integer(v.0))
                })
            }));
            assert_eq!(calls, at + 1);
            if panic {
                assert!(result.is_err());
            } else {
                assert!(matches!(result, Ok(Err(error)) if error == at));
            }
        }
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
        METER.with(|m| m.set(Some((0, 0))));
        drop(mapped);
        let (allocated, freed) = METER.with(|m| m.replace(None).unwrap());
        let retained = freed
            .checked_sub(allocated)
            .expect("drop frees retained trie storage");
        eprintln!("mapping stem bytes={length} retained_heap_bytes={retained}");
        assert!(
            retained < length * 8 + 4096,
            "unary prefix fragmented: {retained}"
        );
    }
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
            assert!(catch_unwind(AssertUnwindSafe(|| {
                let _ = source(&paths).try_map_resident::<()>(|_| panic!("deep callback"));
            }))
            .is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}
