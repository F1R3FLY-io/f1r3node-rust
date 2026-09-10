use std::collections::HashSet;

use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::{CanonicalBytesArena, CanonicalLocatedByte};

fn observations(source: &SourceTreeWindow<'_>) -> Vec<CanonicalLocatedByte> {
    (0..source.len())
        .map(|offset| {
            CanonicalLocatedByte::new(
                source.byte_at(offset).unwrap(),
                source.provenance_at(offset).unwrap(),
            )
        })
        .collect()
}

fn assert_shape(source: &SourceTreeWindow<'_>) -> usize {
    let mut visited = HashSet::new();
    let mut pending = source.root.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if !visited.insert(Arc::as_ptr(node)) {
            continue;
        }
        assert!(node.len > 0);
        match node.payload.as_ref().unwrap() {
            SourcePayload::Borrowed {
                bytes,
                start,
                len,
                absolute_start,
                ..
            } => {
                assert_eq!(node.height, 0);
                assert_eq!(node.len, *len);
                assert!(start.checked_add(*len).unwrap() <= bytes.len());
                assert!(absolute_start
                    .checked_add(*start)
                    .unwrap()
                    .checked_add(*len)
                    .is_some());
            }
            SourcePayload::Frozen(bytes) => {
                assert_eq!(node.height, 0);
                assert_eq!(node.len, bytes.len());
            }
            SourcePayload::Branch2(children) => {
                assert_eq!(children[0].height, children[1].height);
                assert_eq!(
                    usize::from(node.height),
                    usize::from(children[0].height) + 1
                );
                assert_eq!(
                    node.len,
                    children.iter().map(|child| child.len).sum::<usize>()
                );
                pending.extend(children);
            }
            SourcePayload::Branch3(children) => {
                assert!(children
                    .iter()
                    .all(|child| child.height == children[0].height));
                assert_eq!(
                    usize::from(node.height),
                    usize::from(children[0].height) + 1
                );
                assert_eq!(
                    node.len,
                    children.iter().map(|child| child.len).sum::<usize>()
                );
                pending.extend(children);
            }
        }
    }
    if let Some(root) = &source.root {
        assert!(source.start.checked_add(source.len).unwrap() <= root.len);
        assert!(usize::from(root.height) < usize::BITS as usize);
    } else {
        assert_eq!(source.len(), 0);
    }
    visited.len()
}

fn borrowed(bytes: &[u8], source: u32, offset: usize) -> SourceTreeWindow<'_> {
    SourceTreeWindow::from_borrowed(bytes, source, offset).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn concatenation_and_every_split_preserve_bytes_origins_and_balance(
        chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..13), 0..25),
        prepend in any::<bool>(),
    ) {
        let mut root = SourceTreeWindow::empty();
        let mut expected = Vec::new();
        for (id, chunk) in chunks.iter().enumerate() {
            let piece = borrowed(chunk, id as u32, id * 101);
            let located = observations(&piece);
            if prepend {
                root = piece.concat(&root).unwrap();
                expected.splice(0..0, located);
            } else {
                root = root.concat(&piece).unwrap();
                expected.extend(located);
            }
            assert_shape(&root);
        }
        prop_assert_eq!(observations(&root), expected.clone());
        prop_assert_eq!(root.located_bytes().collect::<Vec<_>>(), expected.clone());
        prop_assert_eq!(root.bytes().collect::<Vec<_>>(), expected.iter().map(|value| value.byte).collect::<Vec<_>>());
        for cut in 0..=root.len() {
            let (left, right) = root.split(cut).unwrap();
            assert_shape(&left);
            assert_shape(&right);
            prop_assert_eq!(observations(&left), expected[..cut].to_vec());
            prop_assert_eq!(observations(&right), expected[cut..].to_vec());
            let joined = left.concat(&right).unwrap();
            assert_shape(&joined);
            prop_assert!(root.content_eq(&joined));
            prop_assert_eq!(observations(&joined), expected.clone());
            let mut cursor = root.located_bytes();
            prop_assert!(cursor.skip_exact(cut));
            let before = cursor.clone().collect::<Vec<_>>();
            prop_assert!(!cursor.skip_exact(cursor.len() + 1));
            prop_assert_eq!(&cursor.collect::<Vec<_>>(), &before);
            prop_assert_eq!(&before, &expected[cut..]);
        }
        prop_assert!(root.split(root.len() + 1).is_none());
        prop_assert!(root.byte_at(root.len()).is_none());
        prop_assert!(root.provenance_at(root.len()).is_none());
    }

    #[test]
    fn normalized_windows_compose_without_losing_original_sources(
        bytes in prop::collection::vec(any::<u8>(), 1..160),
        cuts in prop::collection::vec(any::<usize>(), 1..20),
    ) {
        let original = borrowed(&bytes, 7, 123);
        let mut joined = SourceTreeWindow::empty();
        let mut expected = Vec::new();
        for pair in cuts.windows(2) {
            let a = pair[0] % (bytes.len() + 1);
            let b = pair[1] % (bytes.len() + 1);
            let range = a.min(b)..a.max(b);
            let window = original.subview(range.clone()).unwrap();
            if !window.is_empty() {
                prop_assert!(Arc::ptr_eq(original.root.as_ref().unwrap(), window.root.as_ref().unwrap()));
            }
            expected.extend((range).map(|index| CanonicalLocatedByte::new(
                bytes[index], CanonicalByteProvenance::new(7, 123 + index),
            )));
            joined = joined.concat(&window).unwrap();
            assert_shape(&joined);
        }
        prop_assert_eq!(observations(&joined), expected);
        prop_assert_eq!(observations(&original).len(), bytes.len());
    }
}

#[test]
fn frozen_windows_keep_page_owners_after_arena_and_parent_release() {
    let child = {
        let mut arena = CanonicalBytesArena::new();
        let mut root = arena.empty();
        for offset in 0..193 {
            root = arena
                .append(
                    root,
                    CanonicalLocatedByte::new(
                        (offset % 251) as u8,
                        CanonicalByteProvenance::new(19, 900 + offset),
                    ),
                )
                .unwrap();
        }
        let source = SourceTreeWindow::from_frozen(arena.freeze(root));
        let left = source.subview(3..70).unwrap();
        let right = source.subview(120..190).unwrap();
        left.concat(&right).unwrap()
    };
    assert_shape(&child);
    let expected = (3..70)
        .chain(120..190)
        .map(|offset| {
            CanonicalLocatedByte::new(
                (offset % 251) as u8,
                CanonicalByteProvenance::new(19, 900 + offset),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(observations(&child), expected);
}

#[test]
fn coalescing_requires_original_owner_and_exact_adjacency() {
    let bytes = vec![1, 2, 3, 4];
    let other = bytes.clone();
    let root = borrowed(&bytes, 8, 20);
    assert!(root.content_eq(&borrowed(&other, 99, 0)));
    assert!(!root.content_eq(&borrowed(&[1, 2, 3, 5], 8, 20)));
    let left = root.subview(0..2).unwrap();
    let right = root.subview(2..4).unwrap();
    assert_eq!(
        observations(&left.try_coalesce(&right).unwrap()),
        observations(&root)
    );
    assert!(left.try_coalesce(&root.subview(1..4).unwrap()).is_none());
    assert!(left
        .try_coalesce(&borrowed(&other, 8, 20).subview(2..4).unwrap())
        .is_none());
    assert_eq!(root.contiguous_bytes(1..3), Some(&bytes[1..3]));
    assert!(root.contiguous_bytes(3..5).is_none());
}

#[test]
fn checked_lengths_refuse_first_unrepresentable_shared_tree_without_expansion() {
    let atom = [17u8];
    let mut root = borrowed(&atom, 1, 0);
    for exponent in 1..usize::BITS {
        root = root.concat(&root).unwrap();
        assert_eq!(root.len(), 1usize << exponent);
        assert_eq!(assert_shape(&root), exponent as usize + 1);
    }
    assert!(root.concat(&root).is_none());
    assert!(root.subview(1..usize::MAX).is_none());
    assert!(SourceTreeWindow::from_borrowed(&atom, 1, usize::MAX).is_none());
    assert_eq!(root.byte_at(root.len() - 1), Some(17));
    assert_eq!(
        root.provenance_at(root.len() - 1),
        Some(CanonicalByteProvenance::new(1, 0))
    );
    for cut in [1, root.len() / 3, root.len() / 2, root.len() - 1] {
        let (left, right) = root.split(cut).unwrap();
        assert_eq!(left.len(), cut);
        assert_eq!(right.len(), root.len() - cut);
        assert_shape(&left);
        assert_shape(&right);
        assert_eq!(left.byte_at(left.len() - 1), Some(17));
        assert_eq!(right.byte_at(right.len() - 1), Some(17));
        let joined = left.concat(&right).unwrap();
        assert_eq!(joined.len(), root.len());
        assert_shape(&joined);
    }
}

#[test]
fn concurrent_shared_root_release_retains_surviving_source_on_small_stacks() {
    let atom = [29u8];
    let mut root = borrowed(&atom, 3, 71);
    for _ in 1..usize::BITS {
        root = root.concat(&root).unwrap();
    }
    let survivor = root.subview(root.len() - 1..root.len()).unwrap();
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| {
                let copy = root.clone();
                let barrier = &barrier;
                std::thread::Builder::new()
                    .stack_size(64 * 1024)
                    .spawn_scoped(scope, move || {
                        barrier.wait();
                        drop(copy);
                    })
                    .unwrap()
            })
            .collect::<Vec<_>>();
        drop(root);
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert_eq!(observations(&survivor), vec![CanonicalLocatedByte::new(
        29,
        CanonicalByteProvenance::new(3, 71),
    )]);
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn_scoped(scope, move || drop(survivor))
            .unwrap()
            .join()
            .unwrap();
    });
}

#[test]
fn source_window_is_three_native_words() {
    assert_eq!(
        std::mem::size_of::<SourceTreeWindow<'_>>(),
        3 * std::mem::size_of::<usize>()
    );
}

#[test]
fn retained_leaf_cursors_avoid_frontier_allocation_and_preserve_skip_origins() {
    let bytes = [1, 2, 3, 4, 5];
    let root = borrowed(&bytes, 31, 70);
    let mut cursor = root.located_bytes();
    assert_eq!(cursor.0.pending.capacity(), 0);
    assert!(cursor.skip_exact(2));
    drop(root);
    assert_eq!(cursor.collect::<Vec<_>>(), (2..5).map(|at| CanonicalLocatedByte::new(
        bytes[at], CanonicalByteProvenance::new(31, 70 + at),
    )).collect::<Vec<_>>());
    let cursor = {
        let mut arena = CanonicalBytesArena::new();
        let mut root = arena.empty();
        for at in 0..193 {
            root = arena.append(root, CanonicalLocatedByte::new(at as u8,
                CanonicalByteProvenance::new(32, 1000 + at))).unwrap();
        }
        let source = SourceTreeWindow::from_frozen(arena.freeze(root));
        let mut cursor = source.located_bytes();
        assert_eq!(cursor.0.pending.capacity(), 0);
        assert!(cursor.skip_exact(127));
        cursor
    };
    assert_eq!(cursor.collect::<Vec<_>>(), (127..193).map(|at| CanonicalLocatedByte::new(
        at as u8, CanonicalByteProvenance::new(32, 1000 + at),
    )).collect::<Vec<_>>());
}

#[test]
fn subtree_skip_is_constant_work_before_descent_and_failed_copy_is_atomic() {
    let atom = [53u8];
    let mut root = borrowed(&atom, 11, 91);
    for _ in 1..usize::BITS {
        root = root.concat(&root).unwrap();
    }
    let mut cursor = root.located_bytes();
    assert_eq!(cursor.0.pending.len(), 1);
    assert!(cursor.skip_exact(root.len() - 1));
    assert_eq!(cursor.0.pending.len(), 1);
    assert_eq!(cursor.len(), 1);
    assert_eq!(cursor.next(), Some(CanonicalLocatedByte::new(53,
        CanonicalByteProvenance::new(11, 91))));
    assert_eq!(cursor.next(), None);
    let mut output = [99; 4];
    assert!(!root.copy_exact(usize::MAX - 1, &mut output));
    assert_eq!(output, [99; 4]);
    assert!(root.copy_exact(root.len() - 4, &mut output));
    assert_eq!(output, [53; 4]);
}

#[test]
fn maximally_shared_ternary_teardown_keeps_combined_frontier_bounded() {
    let atom = [61u8];
    let mut root = borrowed(&atom, 13, 101);
    while root.len().checked_mul(3).is_some() {
        let node = root.root.as_ref().unwrap();
        root = SourceTreeWindow::from_root(Some(node3(
            Arc::clone(node), Arc::clone(node), Arc::clone(node),
        ).unwrap()));
    }
    assert_shape(&root);
    let mut pending = vec![Arc::clone(root.root.as_ref().unwrap())];
    let mut seen = HashSet::new();
    let mut high_water = pending.len();
    while let Some(node) = pending.pop() {
        if !seen.insert(Arc::as_ptr(&node)) {
            continue;
        }
        match node.payload.as_ref().unwrap() {
            SourcePayload::Branch3(children) => {
                assert!(pending.len() + children.len() <= RELEASE_CAPACITY);
                pending.extend(children.iter().cloned());
                high_water = high_water.max(pending.len());
            }
            SourcePayload::Borrowed { .. } => {}
            _ => panic!("unexpected ternary fixture node"),
        }
    }
    assert_eq!(high_water, 1 + 2 * root.height());
    std::thread::scope(|scope| {
        std::thread::Builder::new().stack_size(64 * 1024)
            .spawn_scoped(scope, move || drop(root)).unwrap().join().unwrap();
    });
}

#[test]
fn simultaneous_final_root_releases_reclaim_each_shared_physical_node() {
    let atom = [41u8];
    let mut root = borrowed(&atom, 5, 111);
    for _ in 1..usize::BITS {
        root = root.concat(&root).unwrap();
    }
    let mut visited = HashSet::new();
    let mut nodes = Vec::new();
    let mut pending = vec![root.root.as_ref().unwrap()];
    while let Some(node) = pending.pop() {
        if !visited.insert(Arc::as_ptr(node)) {
            continue;
        }
        nodes.push(Arc::downgrade(node));
        match node.payload.as_ref().unwrap() {
            SourcePayload::Branch2(children) => pending.extend(children),
            SourcePayload::Branch3(children) => pending.extend(children),
            _ => {}
        }
    }
    drop(pending);
    let roots = (0..4).map(|_| root.clone()).collect::<Vec<_>>();
    drop(root);
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles = roots
            .into_iter()
            .map(|copy| {
                let barrier = &barrier;
                std::thread::Builder::new()
                    .stack_size(64 * 1024)
                    .spawn_scoped(scope, move || {
                        barrier.wait();
                        drop(copy);
                    })
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert_eq!(nodes.len(), usize::BITS as usize);
    assert!(nodes.iter().all(|node| node.upgrade().is_none()));
}

#[test]
fn failed_uniqueness_checks_do_not_have_the_into_inner_payload_contract() {
    struct Probe(Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for Probe {
        fn drop(&mut self) { self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
    }
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let first = Arc::new(Probe(Arc::clone(&drops)));
    let second = Arc::clone(&first);
    let first = Arc::try_unwrap(first).err().unwrap();
    let second = Arc::try_unwrap(second).err().unwrap();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(first);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(second);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);

    let first = Arc::new(Probe(Arc::clone(&drops)));
    let second = Arc::clone(&first);
    assert!(Arc::into_inner(first).is_none());
    let payload = Arc::into_inner(second).unwrap();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(payload);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
