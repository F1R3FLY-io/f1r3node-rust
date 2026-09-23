use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pathmap::utils::BitMask;
use pathmap::zipper::{Zipper, ZipperForking};
use proptest::prelude::*;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token(u64);

impl payload_seal::Sealed for Token {}
impl EpmPayload for Token {
    fn dispose(self) {}
}

#[derive(Clone, Copy)]
struct FixturePolicy;

impl<V: EpmPayload> DisposeValue<V> for FixturePolicy {
    fn dispose(&self, value: V) { value.dispose(); }
}

impl rspace_plus_plus::rspace::pathmap_mutation::TeardownBarrier<Token> for FixturePolicy {
    fn make_barrier() -> Token { Token(u64::MAX) }
}

impl rspace_plus_plus::rspace::pathmap_mutation::TeardownBarrier<()> for FixturePolicy {
    fn make_barrier() {}
}

fn create_path<V: EpmPayload>(map: &mut PathMap<V>, path: &[u8])
where FixturePolicy: rspace_plus_plus::rspace::pathmap_mutation::TeardownBarrier<V> {
    rspace_plus_plus::rspace::pathmap_mutation::NativePathCursor::<V, _, FixturePolicy>::new(
        map,
        path,
        &FixturePolicy,
    )
    .create_path();
}

fn borrowed_entries<V: Clone + Send + Sync + Unpin + 'static>(
    map: &PathMap<V>,
) -> Vec<(Vec<u8>, V)> {
    let mut reader = map.read_zipper();
    let mut entries = Vec::new();
    if let Some(value) = reader.val() {
        entries.push((Vec::new(), value.clone()));
    }
    while reader.to_next_val() {
        entries.push((reader.path().to_vec(), reader.val().unwrap().clone()));
    }
    entries
}

fn remaining(cursor: &mut EpmOwnedCursor<Token>, paths: &[Vec<u8>]) -> Vec<(Vec<u8>, Token)> {
    let PendingWriterState::Map(writer, _) = &mut cursor.writer.state else {
        unreachable!()
    };
    let focus = writer.path().to_vec();
    writer.reset();
    let mut reader = writer.fork_read_zipper();
    let expected_paths: BTreeSet<Vec<u8>> = paths
        .iter()
        .flat_map(|path| (1..=path.len()).map(|length| path[..length].to_vec()))
        .collect();
    let mut actual_paths = BTreeSet::new();
    if descend_first(&mut reader) {
        'walk: loop {
            actual_paths.insert(reader.path().to_vec());
            if descend_first(&mut reader) {
                continue;
            }
            loop {
                if reader.to_next_sibling_byte() {
                    break;
                }
                reader.ascend_byte();
                if reader.at_root() {
                    break 'walk;
                }
            }
        }
    }
    assert_eq!(actual_paths, expected_paths);
    for path in paths {
        reader.reset();
        reader.descend_to(path);
        assert!(reader.path_exists(), "lost original path {path:?}");
    }
    reader.reset();
    let mut entries = Vec::new();
    if let Some(value) = reader.val() {
        entries.push((Vec::new(), *value));
    }
    while reader.to_next_val() {
        entries.push((reader.path().to_vec(), *reader.val().unwrap()));
    }
    drop(reader);
    writer.descend_to(focus);
    entries
}

fn descend_first(zipper: &mut impl ZipperMoving) -> bool {
    let Some(byte) = zipper.child_mask().iter().next() else {
        return false;
    };
    zipper.descend_to_byte(byte);
    true
}

fn exercise(map: PathMap<Token>, paths: &[Vec<u8>], stop: usize) {
    let expected = borrowed_entries(&map);
    let actual: Vec<_> =
        EpmOwnedCursor::new(map.clone(), PendingScaffold::Repeat(Token(u64::MAX))).collect();
    assert_eq!(actual, expected);
    let mut cursor = EpmOwnedCursor::new(map.clone(), PendingScaffold::Repeat(Token(u64::MAX)));
    let mut prefix = Vec::new();
    for count in 0..=stop.min(expected.len()) {
        assert_eq!(prefix, expected[..count]);
        assert_eq!(remaining(&mut cursor, paths), expected[count..]);
        if count < stop.min(expected.len()) {
            prefix.push(cursor.next().unwrap());
        }
    }
    drop(cursor);
    assert_eq!(borrowed_entries(&map), expected);
    for path in paths {
        assert!(map.path_exists_at(path));
    }
    let mut cursor = EpmOwnedCursor::new(map, PendingScaffold::Repeat(Token(u64::MAX)));
    assert_eq!(cursor.by_ref().collect::<Vec<_>>(), expected);
    for _ in 0..4 {
        assert!(cursor.next().is_none());
    }
}

#[test]
fn root_prefix_siblings_and_dangling_paths_are_preserved() {
    for keys in [
        vec![],
        vec![b"".to_vec()],
        [b"".as_slice(), b"a", b"a/child", b"b", b"b/child", b"z"]
            .into_iter()
            .map(Vec::from)
            .collect(),
    ] {
        let mut map = PathMap::new();
        for (index, key) in keys.iter().enumerate() {
            map.insert(key, Token(index as u64 % 2));
        }
        let mut paths = keys;
        for path in [b"a/dangling".as_slice(), b"b/dangling", b"c/topology"] {
            create_path(&mut map, path);
            paths.push(path.to_vec());
        }
        for stop in 0..=map.val_count() {
            exercise(map.clone(), &paths, stop);
        }
    }
    exercise(PathMap::new(), &[], 0);
    let mut root = PathMap::new();
    root.insert([], Token(9));
    exercise(root, &[Vec::new()], 1);
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/epm_owned_cursor.regressions")
        ))),
        ..ProptestConfig::default()
    })]
    #[test]
    fn arbitrary_prefixes_partition_the_source_and_preserve_shared_roots(
        entries in prop::collection::vec((prop::collection::vec(0u8..8, 0..14), 0u64..4), 0..48),
        dangling in prop::collection::vec(prop::collection::vec(0u8..8, 1..14), 0..16),
        stop in 0usize..64,
    ) {
        let expected: BTreeMap<_, _> = entries.into_iter().collect();
        let mut map = PathMap::new();
        let mut set = PathMap::new();
        for (key, value) in &expected {
            map.insert(key, Token(*value));
            set.insert(key, ());
        }
        let mut paths: Vec<_> = expected.keys().cloned().collect();
        for path in dangling {
            create_path(&mut map, &path);
            create_path(&mut set, &path);
            paths.push(path);
        }
        prop_assert_eq!(borrowed_entries(&map), expected.iter()
            .map(|(key, value)| (key.clone(), Token(*value))).collect::<Vec<_>>());
        exercise(map, &paths, stop);
        let mut set_cursor = OwnedEPathMapSetEntries::new(set.clone());
        prop_assert_eq!(set_cursor.by_ref().map(|(key, ())| key).collect::<Vec<_>>(),
            expected.keys().cloned().collect::<Vec<_>>());
        prop_assert!(set_cursor.next().is_none());
        prop_assert!(set_cursor.next().is_none());
        drop(set_cursor);
        prop_assert_eq!(set.val_count(), expected.len());
    }
}

#[derive(Default)]
struct Counts {
    created: AtomicUsize,
    dropped: AtomicUsize,
}

struct Tracked(Arc<Counts>);

impl Tracked {
    fn new(counts: &Arc<Counts>) -> Self {
        counts.created.fetch_add(1, Ordering::Relaxed);
        Self(Arc::clone(counts))
    }
}

impl Clone for Tracked {
    fn clone(&self) -> Self { Self::new(&self.0) }
}

impl Drop for Tracked {
    fn drop(&mut self) { self.0.dropped.fetch_add(1, Ordering::Relaxed); }
}

impl payload_seal::Sealed for Tracked {}
impl EpmPayload for Tracked {
    fn dispose(self) { drop(self); }
}

fn balanced(counts: &Counts) {
    assert_eq!(
        counts.created.load(Ordering::Relaxed),
        counts.dropped.load(Ordering::Relaxed)
    );
}

#[test]
fn partial_drop_and_callback_unwind_balance_payloads() {
    for stop in 0..=6 {
        for panic in [false, true] {
            let counts = Arc::new(Counts::default());
            let mut map = PathMap::new();
            for key in [b"".as_slice(), b"a", b"a/b", b"a/b/c", b"a/c", b"b"] {
                map.insert(key, Tracked::new(&counts));
            }
            map.create_path(b"a/b/dangling");
            let mut cursor =
                EpmOwnedCursor::new(map, PendingScaffold::Repeat(Tracked::new(&counts)));
            let result = catch_unwind(AssertUnwindSafe(move || {
                for _ in 0..stop {
                    let (_, value) = cursor.next().unwrap();
                    drop(value);
                }
                if panic {
                    panic!("injected consumer failure");
                }
            }));
            assert_eq!(result.is_err(), panic);
            balanced(&counts);
        }
    }
}

#[test]
fn deep_branching_partial_consumption_is_stack_safe() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for values in [false, true] {
                for stop in [0, 1, 4096, 8192] {
                    let counts = Arc::new(Counts::default());
                    let mut owner = EpmMapOwner {
                        owned: Some((
                            PathMap::new(),
                            PendingScaffold::Repeat(Tracked::new(&counts)),
                        )),
                    };
                    let map = &mut owner.owned.as_mut().unwrap().0;
                    let mut key = Vec::new();
                    for _ in 0..8192 {
                        key.push(1);
                        if values {
                            map.insert(&key, Tracked::new(&counts));
                        } else {
                            map.create_path(&key);
                        }
                        *key.last_mut().unwrap() = 0;
                    }
                    let (writer, scaffold) = owner.into_writer_parts();
                    let mut cursor = EpmOwnedCursor {
                        writer: PendingWriter {
                            state: PendingWriterState::Map(writer, scaffold),
                        },
                        started: false,
                        exhausted: false,
                    };
                    for _ in 0..stop {
                        if !values {
                            break;
                        }
                        drop(cursor.next().unwrap());
                    }
                    if values && stop == 1 {
                        let PendingWriterState::Map(writer, _) = &cursor.writer.state else {
                            unreachable!()
                        };
                        assert_eq!(writer.path().len(), 8192);
                    }
                    drop(cursor);
                    balanced(&counts);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn public_consuming_boundaries_keep_modes_and_guard_callback_unwind() {
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::Expr;
    use crate::rust::rhoapi_ext::{EPathMap, OwnedEPathMapEntries};

    fn integer(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    assert!(matches!(
        EPathMap::default().into_owned_parts().entries,
        OwnedEPathMapEntries::Empty
    ));
    let set = || EPathMap::from_set_iter([integer(1), integer(2)], vec![1], true, None);
    let map = || {
        EPathMap::new_map(
            [(integer(1), integer(3)), (integer(2), integer(4))],
            vec![2],
            false,
            None,
        )
    };
    let parts = set().into_owned_parts();
    assert_eq!(parts.locally_free, vec![1]);
    assert!(parts.connective_used);
    assert!(matches!(parts.entries, OwnedEPathMapEntries::Set(_)));
    assert_eq!(parts.entries.count(), 2);
    let parts = map().into_owned_parts();
    assert_eq!(parts.locally_free, vec![2]);
    assert!(!parts.connective_used);
    assert!(matches!(parts.entries, OwnedEPathMapEntries::Map(_)));
    assert_eq!(parts.entries.count(), 2);
    assert!(map().into_raw_set_entries(|_| unreachable!()).is_err());
    assert!(set().into_raw_map_entries(|_, _| unreachable!()).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| {
        set()
            .into_raw_set_entries(|_| panic!("injected set callback failure"))
            .unwrap();
    }))
    .is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| {
        map()
            .into_raw_map_entries(|_, _| panic!("injected map callback failure"))
            .unwrap();
    }))
    .is_err());
}
