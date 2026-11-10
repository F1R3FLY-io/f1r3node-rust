use std::collections::HashSet;

use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::{CanonicalBytesArena, CanonicalLocatedByte};
use crate::rust::rholang::indexed_protobuf::IndexedByteRegion;
use crate::rust::rholang::protobuf_cursor::CanonicalByteRange;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SourcePhysicalInventory {
    pub(crate) nodes: HashSet<usize>,
    pub(crate) node_storage_bytes: usize,
    pub(crate) borrowed_slices: HashSet<(usize, usize)>,
    pub(crate) frozen_indexes: std::collections::HashMap<usize, usize>,
    pub(crate) byte_pages: std::collections::HashMap<usize, (usize, usize)>,
    pub(crate) provenance_pages: std::collections::HashMap<usize, (usize, usize)>,
}

impl SourcePhysicalInventory {
    pub(crate) fn observe(&mut self, source: &SourceTreeWindow<'_>) {
        let mut pending = source.root.iter().collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            if !self.nodes.insert(Arc::as_ptr(node) as usize) {
                continue;
            }
            self.node_storage_bytes += std::mem::size_of_val(node.as_ref());
            match node.payload.as_ref().unwrap() {
                SourcePayload::Borrowed(value) => {
                    let bytes = value.root_slice();
                    self.borrowed_slices
                        .insert((bytes.as_ptr() as usize, bytes.len()));
                }
                SourcePayload::Frozen(bytes) => bytes.observe_physical_storage(self),
                SourcePayload::Branch2(children) => pending.extend(children),
                SourcePayload::Branch3(children) => pending.extend(children),
            }
        }
    }
}

#[test]
fn physical_inventory_deduplicates_shared_roots_without_expanding_logical_bytes() {
    let input = [7];
    let mut root = borrowed(&input, 3, usize::MAX);
    for _ in 0..usize::BITS - 2 {
        root = root.concat(&root).unwrap();
    }
    let mut physical = SourcePhysicalInventory::default();
    physical.observe(&root);
    assert_eq!(physical.borrowed_slices.len(), 1);
    assert!(physical.nodes.len() < 4 * usize::BITS as usize);
    assert!(physical.byte_pages.is_empty());
    let nodes = physical.nodes.len();
    let storage = physical.node_storage_bytes;
    for view in [
        root.clone(),
        root.subview(0..1).unwrap(),
        root.subview(root.len() - 1..root.len()).unwrap(),
    ] {
        physical.observe(&view);
    }
    assert_eq!(physical.nodes.len(), nodes);
    assert_eq!(physical.node_storage_bytes, storage);
}

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
            SourcePayload::Borrowed(value) => {
                assert_eq!(node.height, 0);
                assert_eq!(node.len, value.len());
                assert!(
                    value.start().checked_add(value.len()).unwrap() <= value.root_slice().len()
                );
                assert!(value
                    .absolute_start()
                    .checked_add(value.start())
                    .unwrap()
                    .checked_add(value.len() - 1)
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

fn property_config() -> ProptestConfig {
    ProptestConfig {
        cases: 128,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/canonical_source_tree.regressions"
            )),
        )),
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn retained_view_identity_follows_operation_lineage_and_checked_ranges(
        bytes in prop::collection::vec(any::<u8>(), 1..33),
        first in any::<u8>(),
        second in any::<u8>(),
        split in any::<u8>(),
    ) {
        let length = bytes.len();
        let a = usize::from(first) % (length + 1);
        let b = usize::from(second) % (length + 1);
        let (start, end) = (a.min(b), a.max(b));
        let cut = usize::from(split) % (length + 1);
        let root = borrowed(&bytes, 7, 400);
        let independent = borrowed(&bytes, 7, 400);
        let left = root.subview(0..cut).unwrap();
        let right = root.subview(cut..length).unwrap();
        let joined = left.try_coalesce(&right).unwrap();
        let independent_right = independent.subview(cut..length).unwrap();
        let independently_joined = left.try_coalesce(&independent_right).unwrap();
        let composed = root.concat(&independent).unwrap();
        let composed_again = root.concat(&independent).unwrap();

        let cases = vec![
            (root.clone(), 0usize, 0usize, length),
            (root.subview(start..end).unwrap(), 0, start, end - start),
            (root.subview(start..end).unwrap(), 0, start, end - start),
            (left, 0, 0, cut),
            (right, 0, cut, length - cut),
            (joined, 0, 0, length),
            (independent.clone(), 1, 0, length),
            (independent.subview(start..end).unwrap(), 1, start, end - start),
            (independently_joined, 2, 0, length),
            (composed.clone(), 3, 0, length * 2),
            (composed.subview(start..end).unwrap(), 3, start, end - start),
            (composed_again, 4, 0, length * 2),
        ];
        for (index, (actual, lineage, offset, width)) in cases.iter().enumerate() {
            prop_assert_eq!(actual.start, *offset);
            prop_assert_eq!(actual.len(), *width);
            for (other, other_lineage, other_offset, other_width) in &cases[index..] {
                let expected = lineage == other_lineage
                    && offset == other_offset
                    && width == other_width;
                prop_assert_eq!(actual.same_view(other), expected);
                prop_assert_eq!(other.same_view(actual), expected);
            }
        }
        prop_assert_eq!(observations(&root), observations(&independent));
        prop_assert!(!root.same_view(&independent));

        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (offset, byte) in bytes.iter().copied().enumerate() {
            handle = arena.append(
                handle,
                CanonicalLocatedByte::new(
                    byte,
                    CanonicalByteProvenance::new(7, 400 + offset),
                ),
            ).unwrap();
        }
        let frozen = arena.freeze(handle);
        let frozen_clone = frozen.clone();
        prop_assert!(frozen.same_view(&frozen_clone));
        let first_frozen_root = SourceTreeWindow::from_frozen(frozen);
        let second_frozen_root = SourceTreeWindow::from_frozen(frozen_clone);
        prop_assert_eq!(observations(&first_frozen_root), observations(&second_frozen_root));
        prop_assert!(!first_frozen_root.same_view(&second_frozen_root));
        prop_assert!(first_frozen_root.same_view(&first_frozen_root.clone()));
    }

    #[test]
    fn indexed_composed_identity_retains_root_and_outer_range_layers(
        bytes in prop::collection::vec(any::<u8>(), 1..33),
        first in any::<u8>(),
        second in any::<u8>(),
        source in any::<u32>(),
        base in 0usize..100_000,
    ) {
        let length = bytes.len();
        let a = usize::from(first) % (length + 1);
        let b = usize::from(second) % (length + 1);
        let (start, end) = (a.min(b), a.max(b));
        let root = borrowed(&bytes, source, base);
        let root_end = CanonicalByteProvenance::new(source, base + length);
        let indexed = IndexedByteRegion::from_source(root, root_end);
        let range = CanonicalByteRange { start, end };
        let direct = indexed.subview(range).unwrap();
        let natural_end = direct.end_position().physical;
        let normalized = indexed.subview_with_end(range, natural_end).unwrap();
        let direct_bytes = direct.located_bytes().collect::<Vec<_>>();
        let normalized_bytes = normalized.located_bytes().collect::<Vec<_>>();
        prop_assert_eq!(direct_bytes, normalized_bytes);
        prop_assert_eq!(direct.end_position(), normalized.end_position());
        prop_assert!(direct.source_window().unwrap().same_view(&normalized.source_window().unwrap()));
        prop_assert_eq!(direct.same_view(&normalized), start == 0 && end == length);
        prop_assert!(direct.same_view(&direct.clone()));
        prop_assert!(normalized.same_view(&normalized.clone()));

        let changed_end = CanonicalByteProvenance::new(source ^ 1, natural_end.offset);
        let changed = indexed.subview_with_end(range, changed_end).unwrap();
        prop_assert_eq!(changed.located_bytes().collect::<Vec<_>>(), direct.located_bytes().collect::<Vec<_>>());
        prop_assert_eq!(changed.end_position().physical, changed_end);
        prop_assert!(!direct.same_view(&changed));
        prop_assert!(!normalized.same_view(&changed));
    }

    #[test]
    fn projected_mixed_shared_dag_matches_live_bytes_origins_and_explicit_ends(
        chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..9), 2..9),
        first_cut in any::<u8>(),
        second_cut in any::<u8>(),
        split_seed in any::<u8>(),
        range_start in any::<u8>(),
        range_end in any::<u8>(),
        end_offset in 0usize..100_000,
    ) {
        let mut mixed = SourceTreeWindow::empty();
        for (index, chunk) in chunks.iter().enumerate() {
            let piece = if index % 2 == 0 {
                borrowed(chunk, index as u32 + 1, index * 101)
            } else {
                let mut arena = CanonicalBytesArena::new();
                let mut handle = arena.empty();
                for (at, byte) in chunk.iter().copied().enumerate() {
                    let origin = CanonicalByteProvenance::new(
                        500 + index as u32 + (at % 3) as u32,
                        10_000 + index * 100 + at * 7,
                    );
                    handle = arena.append(handle, CanonicalLocatedByte::new(byte, origin)).unwrap();
                }
                SourceTreeWindow::from_frozen(arena.freeze(handle))
            };
            mixed = mixed.concat(&piece).unwrap();
        }
        let shared = mixed.concat(&mixed).unwrap();
        match shared.root.as_ref().unwrap().payload.as_ref() {
            Some(SourcePayload::Branch2([left, right])) => {
                prop_assert!(Arc::ptr_eq(left, right));
            }
            _ => prop_assert!(false, "the doubled root must share its child Arc"),
        }

        let a = usize::from(first_cut) % (shared.len() + 1);
        let b = usize::from(second_cut) % (shared.len() + 1);
        let selected = shared.subview(a.min(b)..a.max(b)).unwrap();
        let cut = usize::from(split_seed) % (selected.len() + 1);
        let (left, right) = selected.split(cut).unwrap();
        let rejoined = left.concat(&right).unwrap();
        let projection = SourceDagProjection::capture(
            &shared,
            &[mixed.clone(), selected.clone(), left.clone(), right.clone(), rejoined.clone()],
        );
        let windows = [
            (&shared, projection.root),
            (&mixed, projection.pending[0]),
            (&selected, projection.pending[1]),
            (&left, projection.pending[2]),
            (&right, projection.pending[3]),
            (&rejoined, projection.pending[4]),
        ];
        for (actual, image) in windows {
            assert_shape(actual);
            let expected = projection.window_located_bytes(image);
            prop_assert_eq!(actual.located_bytes().collect::<Vec<_>>(), expected.clone());
            for (at, entry) in expected.iter().enumerate() {
                prop_assert_eq!(actual.byte_at(at), Some(entry.byte));
                prop_assert_eq!(actual.provenance_at(at), Some(entry.provenance));
            }
            prop_assert_eq!(actual.byte_at(expected.len()), None);
            prop_assert_eq!(actual.provenance_at(expected.len()), None);

            let root_end = CanonicalByteProvenance::new(30_000, end_offset);
            let indexed = IndexedByteRegion::from_source(actual.clone(), root_end);
            for (at, entry) in expected.iter().enumerate() {
                prop_assert_eq!(indexed.byte_at(at), Some(entry.byte));
                prop_assert_eq!(indexed.position_at(at).unwrap().physical, entry.provenance);
            }
            prop_assert_eq!(indexed.position_at(expected.len()).unwrap().physical, root_end);

            let x = usize::from(range_start) % (expected.len() + 1);
            let y = usize::from(range_end) % (expected.len() + 1);
            let (start, end) = (x.min(y), x.max(y));
            let range = CanonicalByteRange { start, end };
            let subview = indexed.subview(range).unwrap();
            let natural_end = expected.get(end).map_or(root_end, |entry| entry.provenance);
            prop_assert_eq!(subview.position_at(end - start).unwrap().physical, natural_end);
            prop_assert_eq!(subview.located_bytes().collect::<Vec<_>>(), expected[start..end].to_vec());

            let explicit_end = CanonicalByteProvenance::new(40_000, end_offset);
            let overridden = indexed.subview_with_end(range, explicit_end).unwrap();
            prop_assert_eq!(overridden.position_at(end - start).unwrap().physical, explicit_end);
            prop_assert_eq!(overridden.located_bytes().collect::<Vec<_>>(), expected[start..end].to_vec());
        }
    }
}

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn retained_chunks_match_scalar_interleavings_and_atomic_copy(
        chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..17), 1..13),
        skips in prop::collection::vec(0usize..33, 1..33),
    ) {
        let mut source = SourceTreeWindow::empty();
        for (index, bytes) in chunks.iter().enumerate() {
            let part = if index % 2 == 0 {
                borrowed(bytes, index as u32, 100)
            } else {
                let mut arena = CanonicalBytesArena::new();
                let mut handle = arena.empty();
                for (at, byte) in bytes.iter().enumerate() {
                    handle = arena.append(handle, CanonicalLocatedByte::new(*byte,
                        CanonicalByteProvenance::new(index as u32, at * 3))).unwrap();
                }
                SourceTreeWindow::from_frozen(arena.freeze(handle))
            };
            source = source.concat(&part).unwrap();
        }
        let expected = chunks.concat();
        let mut cursor = source.bytes();
        let mut at = 0;
        for (step, width) in skips.into_iter().enumerate() {
            if at == expected.len() { break; }
            let chunk = cursor.chunk().unwrap().to_vec();
            prop_assert!(!chunk.is_empty());
            prop_assert_eq!(&chunk[..], &expected[at..at + chunk.len()]);
            prop_assert_eq!(cursor.chunk().unwrap(), &chunk[..]);
            prop_assert_eq!(cursor.len(), expected.len() - at);
            prop_assert_eq!(cursor.clone().take(chunk.len()).collect::<Vec<_>>(), chunk);
            if step % 2 == 0 {
                prop_assert_eq!(cursor.next(), Some(expected[at]));
                at += 1;
            } else {
                let width = width.min(cursor.len());
                prop_assert!(cursor.skip_exact(width));
                at += width;
            }
        }
        prop_assert_eq!(cursor.collect::<Vec<_>>(), expected[at..].to_vec());
        for start in 0..=expected.len() {
            let width = 17.min(expected.len() - start);
            let mut output = vec![0; width];
            prop_assert!(source.copy_exact(start, &mut output));
            prop_assert_eq!(&output[..], &expected[start..start + width]);
        }
        for start in [expected.len(), expected.len() + 1, usize::MAX] {
            let mut output = [99; 2];
            prop_assert!(!source.copy_exact(start, &mut output));
            prop_assert_eq!(output, [99; 2]);
        }
        prop_assert!(source.copy_exact(source.len(), &mut []));
        prop_assert!(!source.copy_exact(source.len() + 1, &mut []));
    }
}

#[test]
fn frozen_chunks_preserve_truncated_page_seams_and_cached_seeks() {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for at in 0..513 {
        handle = arena
            .append(
                handle,
                CanonicalLocatedByte::new(at as u8, CanonicalByteProvenance::new(5, at * 7)),
            )
            .unwrap();
    }
    let root = SourceTreeWindow::from_frozen(arena.freeze(handle));
    let source = root.subview(17..500).unwrap();
    let expected = (17..500).map(|at| at as u8).collect::<Vec<_>>();
    let mut cursor = source.bytes();
    drop((root, arena));
    let mut at = 0;
    while let Some(chunk) = cursor.chunk() {
        assert!(!chunk.is_empty());
        assert_eq!(chunk, &expected[at..at + chunk.len()]);
        let width = chunk.len();
        assert!(cursor.skip_exact(width));
        at += width;
    }
    assert_eq!(at, expected.len());
    assert_eq!(cursor.next(), None);
    assert!(cursor.chunk().is_none());
    for start in [0, 1, 46, 47, 48, 127, 255, 482] {
        let mut cursor = source.bytes();
        assert!(cursor.skip_exact(start));
        let chunk = cursor.chunk().unwrap().to_vec();
        assert!(cursor.skip_exact(0));
        assert_eq!(cursor.chunk().unwrap(), chunk);
        assert_eq!(cursor.next(), Some(expected[start]));
        if start + 1 < expected.len() {
            let suffix = cursor.chunk().unwrap();
            assert_eq!(suffix, &expected[start + 1..start + 1 + suffix.len()]);
        }
    }
}

#[test]
fn chunk_equality_handles_unequal_partitions_and_every_mismatch() {
    let bytes = (0..129).map(|at| at as u8).collect::<Vec<_>>();
    let partition = |bytes: &[u8], width| {
        let mut arena = CanonicalBytesArena::new();
        let mut result = SourceTreeWindow::empty();
        for chunk in bytes.chunks(width) {
            let mut handle = arena.empty();
            for byte in chunk {
                handle = arena
                    .append(
                        handle,
                        CanonicalLocatedByte::new(*byte, CanonicalByteProvenance::new(17, 29)),
                    )
                    .unwrap();
            }
            result = result
                .concat(&SourceTreeWindow::from_frozen(arena.freeze(handle)))
                .unwrap();
        }
        result
    };
    let left = partition(&bytes, 13);
    let right = partition(&bytes, 19);
    assert!(left.content_eq(&right));
    assert!(right.content_eq(&left));
    assert!(left.content_eq(&borrowed(&bytes, 99, 37)));
    assert!(!left.content_eq(&right.subview(0..128).unwrap()));
    for at in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert!(!left.content_eq(&partition(&changed, 19)));
    }
    let atom = [3];
    let mut shared = borrowed(&atom, 3, usize::MAX);
    for _ in 1..usize::BITS {
        shared = shared.concat(&shared).unwrap();
    }
    let view = shared.subview(1..shared.len() - 1).unwrap();
    assert!(shared.content_eq(&shared.clone()));
    assert!(view.content_eq(&view.clone()));
    assert!(SourceTreeWindow::empty().content_eq(&SourceTreeWindow::empty()));
}

proptest! {
    #![proptest_config(property_config())]

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

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn borrowed_leaf_located_lookup_uses_one_checked_slot(
        bytes in prop::collection::vec(any::<u8>(), 1..65),
        source in any::<u32>(),
        start_sample in any::<u16>(),
        end_sample in any::<u16>(),
    ) {
        let start = usize::from(start_sample) % bytes.len();
        let end = start + 1 + usize::from(end_sample) % (bytes.len() - start);
        let len = end - start;
        for absolute_start in [0, usize::MAX - (end - 1)] {
            let shape = verified_score_store::source_borrowed_leaf_shape(
                &bytes, start, end, absolute_start,
            ).unwrap();
            prop_assert_eq!(shape.len, len);
            prop_assert_eq!(shape.height, 0);
            let window = SourceTreeWindow::from_borrowed_range(
                &bytes, start..end, source, absolute_start,
            ).unwrap();
            prop_assert_eq!(window.len(), shape.len);
            let SourcePayload::Borrowed(leaf) = window.root.as_ref().unwrap().payload.as_ref().unwrap()
            else {
                unreachable!()
            };
            prop_assert_eq!(leaf.shape(), shape);
            prop_assert_eq!(leaf.root_slice().as_ptr(), bytes.as_ptr());
            prop_assert_eq!(leaf.root_slice().len(), bytes.len());
            for from in [0, len / 2, len - 1, len] {
                for to in [from, len] {
                    let anchor = absolute_start.checked_add(start + from);
                    let selected = leaf.selected_slice(from, to);
                    let child = leaf.subleaf(from, to);
                    prop_assert_eq!(selected.is_some(), anchor.is_some());
                    prop_assert_eq!(child.is_some(), anchor.is_some());
                    if let (Some((selected_bytes, selected_anchor)), Some(child)) = (selected, child) {
                        prop_assert_eq!(selected_bytes, &bytes[start + from..start + to]);
                        prop_assert_eq!(selected_anchor, anchor.unwrap());
                        prop_assert_eq!(child.root_slice().as_ptr(), bytes.as_ptr());
                        prop_assert_eq!(child.root_slice().len(), bytes.len());
                        prop_assert_eq!(child.len(), to - from);
                        prop_assert_eq!(child.start(), start + from);
                        prop_assert_eq!(child.source(), source);
                    }
                }
            }
            prop_assert!(leaf.selected_slice(1, 0).is_none());
            prop_assert!(leaf.subleaf(1, 0).is_none());
            prop_assert!(leaf.selected_slice(0, len + 1).is_none());
            prop_assert!(leaf.subleaf(0, len + 1).is_none());
            for at in 0..len {
                let expected_offset = absolute_start + start + at;
                let expected = (bytes[start + at], source, expected_offset);
                prop_assert_eq!(
                    verified_score_store::source_borrowed_located_at(
                        &bytes, start, len, source, absolute_start, at,
                    ),
                    Some(expected),
                );
                prop_assert_eq!(leaf.located_at(at), Some(expected));
                prop_assert_eq!(window.byte_at(at), Some(expected.0));
                prop_assert_eq!(
                    window.provenance_at(at),
                    Some(CanonicalByteProvenance::new(expected.1, expected.2)),
                );
            }
            prop_assert!(verified_score_store::source_borrowed_located_at(
                &bytes, start, len, source, absolute_start, len,
            ).is_none());
            prop_assert!(leaf.located_at(len).is_none());
            prop_assert!(window.byte_at(len).is_none());
            prop_assert!(window.provenance_at(len).is_none());
        }
        if end > 1 {
            let overflow_start = usize::MAX - (end - 2);
            prop_assert!(verified_score_store::source_borrowed_leaf_shape(
                &bytes, start, end, overflow_start,
            ).is_none());
            prop_assert!(SourceBorrowedLeaf::new(
                &bytes, start, end, source, overflow_start,
            ).is_none());
            prop_assert!(SourceTreeWindow::from_borrowed_range(
                &bytes, start..end, source, overflow_start,
            ).is_none());
            prop_assert!(verified_score_store::source_borrowed_located_at(
                &bytes, start, len, source, overflow_start, len - 1,
            ).is_none());
        }
        prop_assert!(verified_score_store::source_borrowed_leaf_shape(
            &bytes, end, start, 0,
        ).is_none());
        prop_assert!(verified_score_store::source_borrowed_leaf_shape(
            &bytes, start, bytes.len() + 1, 0,
        ).is_none());
    }
}

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn verified_source_shapes_route_every_boundary_and_reject_mismatch(
        first_len in 1usize..1025,
        middle_len in 1usize..1025,
        last_len in 1usize..1025,
        height in 0u8..u8::MAX,
        sampled in any::<u16>(),
    ) {
        let first = SourceNodeShape { len: first_len, height };
        let middle = SourceNodeShape { len: middle_len, height };
        let last = SourceNodeShape { len: last_len, height };
        let parent2 = verified_score_store::source_branch2_shape(first, middle).unwrap();
        let total2 = first_len + middle_len;
        prop_assert_eq!(parent2.len, total2);
        prop_assert_eq!(parent2.height, height + 1);
        for at in [0, first_len - 1, first_len, total2 - 1, usize::from(sampled) % total2] {
            let (child, local) = verified_score_store::source_branch2_descend(
                parent2, first, middle, at,
            ).unwrap();
            let (expected_child, expected_local) = if at < first_len {
                (0, at)
            } else {
                (1, at - first_len)
            };
            prop_assert_eq!((child, local), (expected_child, expected_local));
        }
        prop_assert!(verified_score_store::source_branch2_descend(
            parent2, first, middle, total2,
        ).is_none());

        let parent3 = verified_score_store::source_branch3_shape(first, middle, last).unwrap();
        let total3 = total2 + last_len;
        prop_assert_eq!(parent3.len, total3);
        prop_assert_eq!(parent3.height, height + 1);
        for at in [
            0, first_len - 1, first_len, total2 - 1, total2, total3 - 1,
            usize::from(sampled) % total3,
        ] {
            let (child, local) = verified_score_store::source_branch3_descend(
                parent3, first, middle, last, at,
            ).unwrap();
            let (expected_child, expected_local) = if at < first_len {
                (0, at)
            } else if at < total2 {
                (1, at - first_len)
            } else {
                (2, at - total2)
            };
            prop_assert_eq!((child, local), (expected_child, expected_local));
        }
        prop_assert!(verified_score_store::source_branch3_descend(
            parent3, first, middle, last, total3,
        ).is_none());

        let wrong_len = SourceNodeShape { len: total2 + 1, ..parent2 };
        let wrong_height = SourceNodeShape { height, ..parent2 };
        prop_assert!(verified_score_store::source_branch2_descend(
            wrong_len, first, middle, 0,
        ).is_none());
        prop_assert!(verified_score_store::source_branch2_descend(
            wrong_height, first, middle, 0,
        ).is_none());
        let mismatched_child = SourceNodeShape { height: height + 1, ..middle };
        let overflowing_child = SourceNodeShape { len: usize::MAX, ..first };
        prop_assert!(verified_score_store::source_branch2_shape(
            first, mismatched_child,
        ).is_none());
        prop_assert!(verified_score_store::source_branch2_shape(
            overflowing_child, middle,
        ).is_none());
    }
}

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn original_leaf_borrowed_rejoins_every_cut_without_a_branch(bytes in prop::collection::vec(any::<u8>(), 2..65)) {
        let root = borrowed(&bytes, 37, 101);
        for cut in 0..=bytes.len() {
            let (left, right) = root.split(cut).unwrap();
            let joined = left.try_coalesce(&right).unwrap();
            prop_assert_eq!(observations(&joined), observations(&root));
            prop_assert_eq!(joined.height(), 0);
            prop_assert_eq!(assert_shape(&joined), 1);
            prop_assert_eq!(left.concat(&right).unwrap().height(), 0);
            let independent = borrowed(&bytes, 37, 101).subview(cut..bytes.len()).unwrap();
            let first = root.subview(0..cut).unwrap();
            let joined = first.try_coalesce(&independent).unwrap();
            prop_assert_eq!(observations(&joined), observations(&root));
            prop_assert_eq!(joined.contiguous_bytes(0..joined.len()).unwrap().as_ptr(), bytes.as_ptr());
        }
    }
}

#[test]
fn original_leaf_frozen_rejoins_preserve_backing_and_discontinuous_origins() {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for at in 0..193 {
        handle = arena
            .append(
                handle,
                CanonicalLocatedByte::new(
                    at as u8,
                    CanonicalByteProvenance::new((at % 7) as u32, 1000 + at * 3),
                ),
            )
            .unwrap();
    }
    let frozen = arena.freeze(handle);
    let root = SourceTreeWindow::from_frozen(frozen.clone());
    let expected = observations(&root);
    let (left, tail) = root.split(63).unwrap();
    let (middle, right) = tail.split(64).unwrap();
    let joined_left = left
        .try_coalesce(&middle)
        .unwrap()
        .try_coalesce(&right)
        .unwrap();
    let joined_right = left
        .try_coalesce(&middle.try_coalesce(&right).unwrap())
        .unwrap();
    for joined in [&joined_left, &joined_right] {
        assert_eq!(observations(joined), expected);
        assert_eq!(assert_shape(joined), 1);
        let SourcePayload::Frozen(value) = joined.root.as_ref().unwrap().payload.as_ref().unwrap()
        else {
            panic!("coalesced frozen windows retain one original frozen leaf")
        };
        assert!(value.shares_backing_with(&frozen));
    }
    for cut in [1, 63, 64, 65, 127, 192] {
        let (left, right) = root.split(cut).unwrap();
        let independent = SourceTreeWindow::from_frozen(frozen.clone())
            .subview(cut..193)
            .unwrap();
        assert_eq!(
            observations(&left.try_coalesce(&independent).unwrap()),
            expected
        );
        assert_eq!(left.concat(&right).unwrap().height(), 0);
    }
    let separately_frozen = SourceTreeWindow::from_frozen(arena.freeze(handle));
    assert!(left
        .try_coalesce(&separately_frozen.subview(63..193).unwrap())
        .is_none());
    drop((root, left, middle, right, tail, frozen, arena));
    assert_eq!(joined_left.located_bytes().collect::<Vec<_>>(), expected);
    assert_eq!(joined_right.located_bytes().collect::<Vec<_>>(), expected);
}

#[test]
fn original_leaf_coalescing_rejects_identity_gaps_overlap_and_preserves_empty_views() {
    let bytes = vec![1, 2, 3, 4];
    let duplicate = bytes.clone();
    let left = borrowed(&bytes, 7, usize::MAX - 4).subview(0..2).unwrap();
    for other in [
        borrowed(&duplicate, 7, usize::MAX - 4)
            .subview(2..4)
            .unwrap(),
        borrowed(&bytes, 8, usize::MAX - 4).subview(2..4).unwrap(),
        borrowed(&bytes, 7, 0).subview(2..4).unwrap(),
        borrowed(&bytes[..3], 7, usize::MAX - 4)
            .subview(2..3)
            .unwrap(),
        borrowed(&bytes, 7, usize::MAX - 4).subview(1..4).unwrap(),
        borrowed(&bytes, 7, usize::MAX - 4).subview(3..4).unwrap(),
    ] {
        assert!(left.try_coalesce(&other).is_none());
    }
    let root = borrowed(&bytes, 7, usize::MAX - 4);
    for cut in 0..=bytes.len() {
        let empty = root.subview(cut..cut).unwrap();
        let independent = borrowed(&bytes, 7, usize::MAX - 4)
            .subview(cut..cut)
            .unwrap();
        let joined = empty.try_coalesce(&independent).unwrap();
        assert!(joined.is_empty());
        assert_shape(&joined);
    }
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
    assert!(SourceTreeWindow::from_borrowed(&[17, 17], 1, usize::MAX).is_none());
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
fn physical_last_byte_at_native_max_keeps_indexed_end_authority_separate() {
    use crate::rust::rholang::indexed_protobuf::IndexedByteRegion;
    use crate::rust::rholang::protobuf_cursor::CanonicalByteRange;

    let bytes = [73];
    let indexed = IndexedByteRegion::from_borrowed(&bytes, 19, usize::MAX);
    let anchor = CanonicalByteProvenance::new(23, 101);
    let anchored = indexed
        .subview_with_end(CanonicalByteRange { start: 0, end: 1 }, anchor)
        .unwrap();
    assert_eq!(anchored.position_at(0).unwrap().physical.offset, usize::MAX);
    assert_eq!(anchored.position_at(1).unwrap().physical, anchor);
    assert!(indexed.position_at(1).is_none());
    assert!(indexed
        .subview_with_end(CanonicalByteRange { start: 1, end: 1 }, anchor)
        .is_none());
    assert!(indexed
        .subview_with_end(CanonicalByteRange { start: 0, end: 0 }, anchor)
        .is_some());
    let source = SourceTreeWindow::from_borrowed(&bytes, 19, usize::MAX).unwrap();
    assert_eq!(source.located_bytes().collect::<Vec<_>>(), vec![
        CanonicalLocatedByte::new(73, CanonicalByteProvenance::new(19, usize::MAX)),
    ]);
    assert!(source.provenance_at(1).is_none());
    assert!(indexed.position_at(1).is_none());
    let independent = SourceTreeWindow::from_borrowed(&bytes, 19, usize::MAX).unwrap();
    let empty = source.subview(1..1).unwrap();
    let other_empty = independent.subview(1..1).unwrap();
    assert!(empty.try_coalesce(&other_empty).unwrap().is_empty());
    assert_shape(&source);
}

#[test]
fn physical_last_byte_boundary_survives_every_split_and_original_owner_rejoin() {
    let bytes = [11, 13, 17, 19];
    let source = SourceTreeWindow::from_borrowed(&bytes, 29, usize::MAX - 3).unwrap();
    for cut in 0..=bytes.len() {
        let (left, right) = source.split(cut).unwrap();
        let joined = left.try_coalesce(&right).unwrap();
        assert_eq!(observations(&joined), observations(&source));
        assert_eq!(
            joined.located_bytes().collect::<Vec<_>>(),
            observations(&source)
        );
        assert_eq!(assert_shape(&joined), 1);
        assert_eq!(joined.height(), 0);
    }
}

#[test]
fn selected_borrowed_range_preserves_identity_without_validating_unused_tail() {
    let bytes = [2, 3, 5, 7];
    let base = usize::MAX - 1;
    assert!(SourceTreeWindow::from_borrowed(&bytes, 31, base).is_none());
    let source = SourceTreeWindow::from_borrowed_range(&bytes, 0..2, 31, base).unwrap();
    assert_eq!(observations(&source), vec![
        CanonicalLocatedByte::new(2, CanonicalByteProvenance::new(31, base)),
        CanonicalLocatedByte::new(3, CanonicalByteProvenance::new(31, usize::MAX)),
    ]);
    let tail = SourceTreeWindow::from_borrowed_range(&bytes, 1..2, 31, base).unwrap();
    let joined = source.subview(0..1).unwrap().try_coalesce(&tail).unwrap();
    assert_eq!(assert_shape(&joined), 1);
    assert_eq!(
        joined.contiguous_bytes(0..2).unwrap().as_ptr(),
        bytes.as_ptr()
    );
    let SourcePayload::Borrowed(value) = tail.root.as_ref().unwrap().payload.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert!(std::ptr::eq(value.root_slice(), &bytes[..]));
    assert_eq!(value.start(), 1);
    assert!(SourceTreeWindow::from_borrowed_range(&bytes, 0..0, 31, usize::MAX).is_some());
    assert!(SourceTreeWindow::from_borrowed_range(&bytes, 1..1, 31, usize::MAX).is_none());
    for range in [2..1, 0..5, 5..5, usize::MAX..usize::MAX, 0..usize::MAX] {
        assert!(SourceTreeWindow::from_borrowed_range(&bytes, range, 31, 0).is_none());
    }
}

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn selected_borrowed_ranges_match_checked_physical_byte_oracle(
        bytes in prop::collection::vec(any::<u8>(), 0..65),
        base in prop_oneof![any::<usize>(), (0usize..65).prop_map(|distance| usize::MAX - distance)],
        start in 0usize..70,
        end in 0usize..70,
    ) {
        let expected = if start <= end && end <= bytes.len() && base.checked_add(start).is_some() {
            (start..end).map(|at| base.checked_add(at).map(|offset|
                CanonicalLocatedByte::new(bytes[at], CanonicalByteProvenance::new(41, offset))
            )).collect::<Option<Vec<_>>>()
        } else { None };
        let actual = SourceTreeWindow::from_borrowed_range(&bytes, start..end, 41, base);
        prop_assert_eq!(actual.as_ref().map(observations), expected);
        if let Some(source) = actual {
            assert_shape(&source);
            for cut in 0..=source.len() {
                let (left, right) = source.split(cut).unwrap();
                prop_assert_eq!(observations(&left.concat(&right).unwrap()), observations(&source));
            }
        }
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
fn trimmed_exports_release_excluded_composed_roots() {
    let mut root = SourceTreeWindow::empty();
    let mut leaves = Vec::new();
    for source in 0..3 {
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for offset in 0..4 {
            handle = arena
                .append(
                    handle,
                    CanonicalLocatedByte::new(
                        source as u8,
                        CanonicalByteProvenance::new(source, offset),
                    ),
                )
                .unwrap();
        }
        let leaf = SourceTreeWindow::from_frozen(arena.freeze(handle));
        leaves.push(Arc::downgrade(leaf.root.as_ref().unwrap()));
        root = root.concat(&leaf).unwrap();
    }
    let expected = observations(&root)[4..8].to_vec();
    let ordinary = root.subview(4..8).unwrap();
    let exported = root.trimmed_subview(4..8).unwrap();
    assert_eq!(observations(&exported), expected);
    assert_eq!(exported.start, 0);
    assert_eq!(exported.root.as_ref().unwrap().len, 4);
    assert!(root.trimmed_subview(0..12).unwrap().shares_root_with(&root));
    for range in [8..4, 0..13, usize::MAX..usize::MAX] {
        assert!(root.trimmed_subview(range).is_none());
    }
    let empty = root.trimmed_subview(6..6).unwrap();
    assert!(empty.root.is_none());
    drop(root);
    assert!(leaves.iter().all(|leaf| leaf.upgrade().is_some()));
    drop(ordinary);
    assert!(leaves[0].upgrade().is_none());
    assert!(leaves[1].upgrade().is_some());
    assert!(leaves[2].upgrade().is_none());
    assert_eq!(observations(&exported), expected);
    drop(exported);
    assert!(leaves.iter().all(|leaf| leaf.upgrade().is_none()));
}

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn trimmed_subviews_preserve_mixed_located_windows(
        chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..17), 1..13),
        first in any::<usize>(),
        second in any::<usize>(),
    ) {
        let mut root = SourceTreeWindow::empty();
        let mut expected = Vec::new();
        for (source, chunk) in chunks.iter().enumerate() {
            let located = chunk.iter().enumerate().map(|(at, byte)| CanonicalLocatedByte::new(
                *byte, CanonicalByteProvenance::new(source as u32, 101 + at),
            )).collect::<Vec<_>>();
            let leaf = if source % 2 == 0 {
                borrowed(chunk, source as u32, 101)
            } else {
                let mut arena = CanonicalBytesArena::new();
                let mut handle = arena.empty();
                for byte in &located { handle = arena.append(handle, *byte).unwrap(); }
                SourceTreeWindow::from_frozen(arena.freeze(handle))
            };
            root = root.concat(&leaf).unwrap();
            expected.extend(located);
        }
        let a = first % (root.len() + 1);
        let b = second % (root.len() + 1);
        let range = a.min(b)..a.max(b);
        let view = root.subview(range.clone()).unwrap();
        for start in 0..=view.len() {
            let trimmed = view.trimmed_subview(start..view.len()).unwrap();
            prop_assert_eq!(observations(&trimmed), &expected[range.start + start..range.end]);
            prop_assert_eq!(trimmed.start, 0);
            prop_assert_eq!(trimmed.root.as_ref().map_or(0, |node| node.len), trimmed.len());
            assert_shape(&trimmed);
        }
    }
}

#[test]
fn trimmed_shared_huge_prefix_exports_on_an_unchanged_small_stack() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let atom = [43];
            let mut root = borrowed(&atom, 37, usize::MAX);
            for _ in 1..usize::BITS {
                root = root.concat(&root).unwrap();
            }
            let last = root.len() - 1;
            let exported = root.trimmed_subview(last..root.len()).unwrap();
            drop(root);
            assert_eq!(assert_shape(&exported), 1);
            assert_eq!(observations(&exported), vec![CanonicalLocatedByte::new(
                43,
                CanonicalByteProvenance::new(37, usize::MAX),
            )]);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn retained_leaf_cursors_avoid_frontier_allocation_and_preserve_skip_origins() {
    let bytes = [1, 2, 3, 4, 5];
    let root = borrowed(&bytes, 31, 70);
    let mut cursor = root.located_bytes();
    assert_eq!(cursor.0.pending.capacity(), 0);
    assert!(cursor.skip_exact(2));
    drop(root);
    assert_eq!(
        cursor.collect::<Vec<_>>(),
        (2..5)
            .map(|at| CanonicalLocatedByte::new(
                bytes[at],
                CanonicalByteProvenance::new(31, 70 + at),
            ))
            .collect::<Vec<_>>()
    );
    let cursor = {
        let mut arena = CanonicalBytesArena::new();
        let mut root = arena.empty();
        for at in 0..193 {
            root = arena
                .append(
                    root,
                    CanonicalLocatedByte::new(
                        at as u8,
                        CanonicalByteProvenance::new(32, 1000 + at),
                    ),
                )
                .unwrap();
        }
        let source = SourceTreeWindow::from_frozen(arena.freeze(root));
        let mut cursor = source.located_bytes();
        assert_eq!(cursor.0.pending.capacity(), 0);
        assert!(cursor.skip_exact(127));
        cursor
    };
    assert_eq!(
        cursor.collect::<Vec<_>>(),
        (127..193)
            .map(|at| CanonicalLocatedByte::new(
                at as u8,
                CanonicalByteProvenance::new(32, 1000 + at),
            ))
            .collect::<Vec<_>>()
    );
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
    assert_eq!(
        cursor.next(),
        Some(CanonicalLocatedByte::new(
            53,
            CanonicalByteProvenance::new(11, 91)
        ))
    );
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
        root = SourceTreeWindow::from_root(Some(
            node3(Arc::clone(node), Arc::clone(node), Arc::clone(node)).unwrap(),
        ));
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
            SourcePayload::Borrowed(_) => {}
            _ => panic!("unexpected ternary fixture node"),
        }
    }
    assert_eq!(high_water, 1 + 2 * root.height());
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn_scoped(scope, move || drop(root))
            .unwrap()
            .join()
            .unwrap();
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
