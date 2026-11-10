use proptest::prelude::*;

use super::*;

fn origin(index: usize) -> CanonicalByteProvenance {
    CanonicalByteProvenance::new(7 + (index % 5) as u32, 101 + index * 17)
}

fn populated(bytes: &[u8]) -> (CanonicalBytesArena, CanonicalBytesHandle) {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (index, byte) in bytes.iter().copied().enumerate() {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, origin(index)))
            .unwrap();
    }
    (arena, handle)
}

fn assert_shared(parent: &IndexedByteRegion<'_>, child: &IndexedByteRegion<'_>, start: usize) {
    match (&parent.backing, &child.backing) {
        (IndexedByteBacking::Borrowed(parent), IndexedByteBacking::Borrowed(child)) => {
            assert!(std::ptr::eq(parent.root_slice(), child.root_slice()));
            assert_eq!(child.start(), parent.start() + start);
        }
        (
            IndexedByteBacking::Contiguous { bytes: parent, .. },
            IndexedByteBacking::Contiguous { bytes: child, .. },
        ) => assert!(child.is_empty() || child.as_ptr() == parent[start..].as_ptr()),
        (
            IndexedByteBacking::Arena { arena: parent, .. },
            IndexedByteBacking::Arena { arena: child, .. },
        ) => assert!(std::ptr::eq(*parent, *child)),
        (
            IndexedByteBacking::Frozen { root: parent, .. },
            IndexedByteBacking::Frozen { root: child, .. },
        ) => assert!(child.shares_backing_with(parent)),
        (
            IndexedByteBacking::Composed { root: parent, .. },
            IndexedByteBacking::Composed { root: child, .. },
        ) => assert!(child.shares_root_with(parent)),
        _ => panic!("anchoring changed backing representation"),
    }
}

fn assert_window(root: &IndexedByteRegion<'_>, range: CanonicalByteRange) {
    let anchor = CanonicalByteProvenance::new(901, 23);
    let plain = root.subview(range).unwrap();
    let anchored = root.subview_with_end(range, anchor).unwrap();
    assert_shared(root, &anchored, range.start);
    assert_eq!(anchored.len(), range.end - range.start);
    assert!(anchored.content_eq(&plain));
    assert_eq!(anchored.compare(&plain), std::cmp::Ordering::Equal);
    assert_eq!(
        anchored.located_bytes().collect::<Vec<_>>(),
        plain.located_bytes().collect::<Vec<_>>()
    );
    for offset in 0..anchored.len() {
        assert_eq!(anchored.position_at(offset), plain.position_at(offset));
    }
    assert_eq!(anchored.end_position().physical, anchor);
    assert_eq!(anchored.end_position().kind, IndexedBytePositionKind::End);
    assert_eq!(anchored.position_at(anchored.len() + 1), None);
    let len = anchored.len();
    for nested_range in [
        CanonicalByteRange { start: 0, end: 0 },
        CanonicalByteRange { start: 0, end: len },
        CanonicalByteRange {
            start: len,
            end: len,
        },
        CanonicalByteRange {
            start: len / 3,
            end: len,
        },
        CanonicalByteRange {
            start: 0,
            end: len / 2,
        },
    ] {
        let nested = anchored.subview(nested_range).unwrap();
        let expected = plain.subview(nested_range).unwrap();
        assert_shared(&anchored, &nested, nested_range.start);
        assert!(nested.content_eq(&expected));
        for offset in 0..nested.len() {
            assert_eq!(nested.position_at(offset), expected.position_at(offset));
        }
        let end = if nested_range.end == len {
            anchor
        } else {
            plain.position_at(nested_range.end).unwrap().physical
        };
        assert_eq!(nested.end_position().physical, end);
        let replacement = CanonicalByteProvenance::new(902, 29);
        let reanchored = anchored
            .subview_with_end(nested_range, replacement)
            .unwrap();
        assert_shared(&anchored, &reanchored, nested_range.start);
        assert!(reanchored.content_eq(&nested));
        assert_eq!(reanchored.end_position().physical, replacement);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn anchored_windows_preserve_bytes_locations_bounds_and_shared_backing(
        bytes in prop::collection::vec(any::<u8>(), 0..194),
        first in any::<usize>(), second in any::<usize>(),
    ) {
        let a = first % (bytes.len() + 1);
        let b = second % (bytes.len() + 1);
        let range = CanonicalByteRange { start: a.min(b), end: a.max(b) };
        let (arena, handle) = populated(&bytes);
        let before = arena.stats();
        let boundary = CanonicalByteProvenance::new(71, 9001);
        let views = [
            IndexedByteRegion::from_borrowed(&bytes, 3, 41),
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), 4),
            IndexedByteRegion::from_arena(&arena, handle, boundary),
            IndexedByteRegion::from_frozen(arena.freeze(handle), boundary),
            IndexedByteRegion::from_source(
                SourceTreeWindow::from_borrowed_range(&bytes, 0..bytes.len() / 2, 77, 501)
                    .unwrap().concat(&SourceTreeWindow::from_frozen(
                        arena.freeze(handle).subview(bytes.len() / 2..bytes.len()).unwrap()
                    )).unwrap(),
                boundary,
            ),
        ];
        for root in &views {
            assert_window(root, range);
        }
        prop_assert_eq!(arena.stats(), before);
    }
}

#[test]
fn nonterminal_and_empty_windows_preserve_supplied_completion_anchors() {
    let bytes = [0x10, 2, 0x31, 0x32, 0xff];
    let (arena, handle) = populated(&bytes);
    for root in [
        IndexedByteRegion::from_borrowed(&bytes, 3, 41),
        IndexedByteRegion::from_frozen(arena.freeze(handle), origin(99)),
    ] {
        for (range, completion_index) in [
            (CanonicalByteRange { start: 2, end: 4 }, 3),
            (CanonicalByteRange { start: 2, end: 2 }, 1),
        ] {
            let anchor = root.position_at(completion_index).unwrap().physical;
            let plain = root.subview(range).unwrap();
            let anchored = root.subview_with_end(range, anchor).unwrap();
            assert_ne!(plain.end_position().physical, anchor);
            assert_eq!(anchored.end_position().physical, anchor);
            assert!(plain.content_eq(&anchored));
        }
    }
}

#[test]
fn anchored_windows_reject_bad_ranges_and_checked_offset_overflow() {
    let bytes = [1, 2, 3];
    let (arena, handle) = populated(&bytes);
    for root in [
        IndexedByteRegion::from_borrowed(&bytes, 3, 41),
        IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), 4),
        IndexedByteRegion::from_arena(&arena, handle, origin(99)),
        IndexedByteRegion::from_frozen(arena.freeze(handle), origin(99)),
    ] {
        for range in [
            CanonicalByteRange { start: 2, end: 1 },
            CanonicalByteRange { start: 0, end: 4 },
            CanonicalByteRange {
                start: usize::MAX,
                end: usize::MAX,
            },
        ] {
            assert!(root.subview_with_end(range, origin(8)).is_none());
        }
    }
    let shifted = IndexedByteRegion::from_borrowed(&bytes, 3, usize::MAX);
    let range = CanonicalByteRange { start: 1, end: 2 };
    assert!(shifted.subview(range).is_none());
    assert!(shifted.subview_with_end(range, origin(8)).is_none());
}

#[test]
fn anchored_children_survive_parent_teardown_and_original_arena_mutation() {
    let bytes = [1, 2, 3, 4, 5];
    let anchor = CanonicalByteProvenance::new(91, 203);
    let range = CanonicalByteRange { start: 1, end: 4 };
    let borrowed = {
        let parent = IndexedByteRegion::from_borrowed(&bytes, 8, 71);
        parent.subview_with_end(range, anchor).unwrap()
    };
    let frozen = {
        let (mut arena, handle) = populated(&bytes);
        let parent = IndexedByteRegion::from_frozen(arena.freeze(handle), origin(99));
        let child = parent.subview_with_end(range, anchor).unwrap();
        assert_shared(&parent, &child, range.start);
        for offset in 0..256 {
            let _ = arena
                .append(handle, CanonicalLocatedByte::new(0xff, origin(offset)))
                .unwrap();
        }
        child
    };
    assert_eq!(borrowed.bytes().collect::<Vec<_>>(), bytes[1..4]);
    assert_eq!(frozen.bytes().collect::<Vec<_>>(), bytes[1..4]);
    assert_eq!(borrowed.end_position().physical, anchor);
    assert_eq!(frozen.end_position().physical, anchor);
    for index in 0..3 {
        assert_eq!(
            frozen.position_at(index).unwrap().physical,
            origin(index + 1)
        );
    }
}

#[test]
fn anchored_contiguous_children_preserve_shifted_origins_after_owner_drop() {
    let anchor = CanonicalByteProvenance::new(71, 901);
    let child = {
        let owner = ProtobufRegion::copy_from_slice(&[0, 1, 2, 3, 4, 5, 6]);
        let shifted = owner
            .subregion(CanonicalByteRange { start: 2, end: 6 })
            .unwrap();
        let parent = IndexedByteRegion::from_protobuf_region(shifted, 17);
        assert_eq!(
            parent.end_position().physical,
            CanonicalByteProvenance::new(17, 6)
        );
        let child = parent
            .subview_with_end(CanonicalByteRange { start: 1, end: 3 }, anchor)
            .unwrap();
        assert_shared(&parent, &child, 1);
        child
    };
    assert_eq!(child.bytes().collect::<Vec<_>>(), [3, 4]);
    assert_eq!(child.located_bytes().collect::<Vec<_>>(), [
        CanonicalLocatedByte::new(3, CanonicalByteProvenance::new(17, 3)),
        CanonicalLocatedByte::new(4, CanonicalByteProvenance::new(17, 4)),
    ]);
    for offset in 0..2 {
        assert_eq!(
            child.position_at(offset).unwrap().physical,
            CanonicalByteProvenance::new(17, 3 + offset)
        );
    }
    assert_eq!(child.end_position().physical, anchor);
    let interior = child
        .subview(CanonicalByteRange { start: 0, end: 1 })
        .unwrap();
    assert_eq!(
        interior.end_position().physical,
        CanonicalByteProvenance::new(17, 4)
    );
    let empty = child
        .subview(CanonicalByteRange { start: 2, end: 2 })
        .unwrap();
    assert!(empty.bytes().next().is_none());
    assert_eq!(empty.end_position().physical, anchor);
}

#[test]
fn anchored_metadata_fits_the_original_indexed_region_layout() {
    #[allow(dead_code)]
    enum OriginalBacking<'a> {
        Borrowed {
            bytes: &'a [u8],
            source: u32,
            absolute_start: usize,
        },
        Contiguous {
            region: ProtobufRegion,
            source: u32,
        },
        Arena {
            arena: &'a CanonicalBytesArena<'a>,
            root: CanonicalBytesHandle,
            start: usize,
            len: usize,
            root_end: CanonicalByteProvenance,
        },
        Frozen {
            root: FrozenCanonicalBytes,
            start: usize,
            len: usize,
            root_end: CanonicalByteProvenance,
        },
    }
    assert_eq!(
        std::mem::size_of::<IndexedByteRegion>(),
        std::mem::size_of::<OriginalBacking>()
    );
}
