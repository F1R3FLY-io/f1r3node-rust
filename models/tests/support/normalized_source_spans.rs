use proptest::prelude::*;
use rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource;

use super::*;

fn range(start: usize, end: usize) -> CanonicalByteRange { CanonicalByteRange { start, end } }

fn origin(index: usize) -> CanonicalByteProvenance {
    CanonicalByteProvenance::new(17 + (index % 7) as u32, 103 + index * 19)
}

fn frozen(
    bytes: &[u8],
) -> (
    CanonicalBytesArena,
    CanonicalBytesHandle,
    OwnedIndexedByteRegion,
) {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (index, byte) in bytes.iter().copied().enumerate() {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, origin(index)))
            .unwrap();
    }
    let view = IndexedByteRegion::from_frozen(arena.freeze(handle), origin(bytes.len()));
    (arena, handle, view)
}

fn located(view: &IndexedByteRegion<'_>) -> Vec<CanonicalLocatedByte> {
    view.located_bytes().collect()
}

fn assert_view(
    view: &IndexedByteRegion<'_>,
    expected: &[CanonicalLocatedByte],
    end: CanonicalByteProvenance,
) {
    let bytes: Vec<_> = expected.iter().map(|byte| byte.byte).collect();
    assert_eq!(view.len(), expected.len());
    assert_eq!(view.bytes().collect::<Vec<_>>(), bytes);
    assert_eq!(located(view), expected);
    for (offset, byte) in expected.iter().enumerate() {
        assert_eq!(view.byte_at(offset), Some(byte.byte));
        assert_eq!(
            view.position_at(offset),
            Some(IndexedBytePosition {
                logical_offset: offset,
                physical: byte.provenance,
                kind: IndexedBytePositionKind::Byte,
            })
        );
    }
    assert_eq!(view.byte_at(expected.len()), None);
    assert_eq!(view.position_at(expected.len()).unwrap().physical, end);
    assert_eq!(
        view.position_at(expected.len()).unwrap().kind,
        IndexedBytePositionKind::End
    );
    assert_eq!(view.position_at(expected.len() + 1), None);
    let mut output = vec![0; expected.len()];
    assert!(view.copy_exact(0, &mut output));
    assert_eq!(output, bytes);
    let mut too_long = vec![93; expected.len() + 1];
    assert!(!view.copy_exact(0, &mut too_long));
    assert!(too_long.iter().all(|byte| *byte == 93));
    assert!(!view.copy_exact(usize::MAX, &mut [0]));
    if let Some(contiguous) = view.contiguous_range(0..expected.len()) {
        assert_eq!(contiguous, bytes);
    }
    assert!(view.contiguous_range(0..expected.len() + 1).is_none());
    assert!(view.contiguous_range(1..0).is_none());
    assert!(view.contiguous_range(usize::MAX..usize::MAX).is_none());
    let mut reader = view.reader();
    for byte in &bytes {
        assert_eq!(reader.read_byte().unwrap(), *byte);
    }
    assert_eq!(reader.offset(), bytes.len());
    assert_eq!(reader.read_byte().unwrap_err().failure.physical, end);
    assert_eq!(reader.offset(), bytes.len());
    if bytes.len() >= 4 {
        assert_eq!(
            view.reader().read_fixed32().unwrap(),
            u32::from_le_bytes(bytes[..4].try_into().unwrap())
        );
    }
    if bytes.len() >= 8 {
        assert_eq!(
            view.reader().read_fixed64().unwrap(),
            u64::from_le_bytes(bytes[..8].try_into().unwrap())
        );
    }
    let mut cursor = view.located_bytes();
    assert!(cursor.skip_exact(expected.len() / 2));
    assert_eq!(cursor.collect::<Vec<_>>(), expected[expected.len() / 2..]);
}

fn assert_same_root(left: &IndexedByteRegion<'_>, right: &IndexedByteRegion<'_>) {
    match (&left.backing, &right.backing) {
        (IndexedByteBacking::Borrowed(left), IndexedByteBacking::Borrowed(right)) => {
            assert!(std::ptr::eq(left.root_slice(), right.root_slice()));
        }
        (
            IndexedByteBacking::Frozen { root: left, .. },
            IndexedByteBacking::Frozen { root: right, .. },
        ) => {
            assert!(left.shares_backing_with(right));
        }
        (
            IndexedByteBacking::Composed { root: left, .. },
            IndexedByteBacking::Composed { root: right, .. },
        ) => assert!(left.shares_root_with(right)),
        _ => panic!("source representation changed"),
    }
}

fn exercise(root: &IndexedByteRegion<'_>, a: usize, b: usize, c: usize, d: usize) {
    let all = located(root);
    let first = root.subview_with_end(range(a, b), origin(1001)).unwrap();
    let middle = root.subview_with_end(range(b, c), origin(1002)).unwrap();
    let last = root.subview_with_end(range(c, d), origin(1003)).unwrap();
    let left_grouped = first
        .try_coalesce(&middle)
        .unwrap()
        .try_coalesce(&last)
        .unwrap();
    let right_grouped = first
        .try_coalesce(&middle.try_coalesce(&last).unwrap())
        .unwrap();
    assert_same_root(root, &left_grouped);
    assert_same_root(root, &right_grouped);
    assert_view(&left_grouped, &all[a..d], origin(1003));
    assert_view(&right_grouped, &all[a..d], origin(1003));
    assert_view(&first, &all[a..b], origin(1001));
    assert_view(&middle, &all[b..c], origin(1002));
    assert_view(&last, &all[c..d], origin(1003));
    let outer = root.subview_with_end(range(a, d), origin(1004)).unwrap();
    let nested = outer.subview(range(b - a, c - a)).unwrap();
    let direct = root.subview(range(b, c)).unwrap();
    assert_same_root(root, &nested);
    assert_eq!(located(&nested), located(&direct));
    assert_eq!(
        nested.end_position().physical,
        if c == d {
            origin(1004)
        } else {
            direct.end_position().physical
        }
    );
    let original_end = root.subview(range(c, d)).unwrap();
    let unanchored = first
        .try_coalesce(&middle)
        .unwrap()
        .try_coalesce(&original_end)
        .unwrap();
    assert_view(
        &unanchored,
        &all[a..d],
        original_end.end_position().physical,
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn borrowed_coalescing_and_nested_views_match_located_oracle(
        bytes in prop::collection::vec(any::<u8>(), 0..257),
        choices in prop::array::uniform4(any::<usize>()),
        source in any::<u32>(),
        base in 0usize..1_000_000,
    ) {
        let root = IndexedByteRegion::from_borrowed(&bytes, source, base);
        let mut points = choices.map(|value| value % (bytes.len() + 1));
        points.sort_unstable();
        exercise(&root, points[0], points[1], points[2], points[3]);
        let composed = IndexedByteRegion::from_source(root.source_window().unwrap(), root.end_position().physical);
        exercise(&composed, points[0], points[1], points[2], points[3]);
    }

    #[test]
    fn fragmented_frozen_coalescing_and_nested_views_match_located_oracle(
        bytes in prop::collection::vec(any::<u8>(), 0..257),
        choices in prop::array::uniform4(any::<usize>()),
    ) {
        let (_, _, root) = frozen(&bytes);
        let mut points = choices.map(|value| value % (bytes.len() + 1));
        points.sort_unstable();
        exercise(&root, points[0], points[1], points[2], points[3]);
        let cut = bytes.len() / 2;
        let prefix = SourceTreeWindow::from_borrowed_range(&bytes, 0..cut, 91, 503).unwrap();
        let suffix = root.subview(range(cut, bytes.len())).unwrap().source_window().unwrap();
        let composed = IndexedByteRegion::from_source(prefix.concat(&suffix).unwrap(), root.end_position().physical);
        exercise(&composed, points[0], points[1], points[2], points[3]);
    }

    #[test]
    fn coalescing_accepts_exact_adjacency_for_all_generated_windows(
        bytes in prop::collection::vec(any::<u8>(), 1..129),
        choices in prop::array::uniform4(any::<usize>()),
    ) {
        let mut left = [choices[0] % (bytes.len() + 1), choices[1] % (bytes.len() + 1)];
        let mut right = [choices[2] % (bytes.len() + 1), choices[3] % (bytes.len() + 1)];
        left.sort_unstable();
        right.sort_unstable();
        let borrowed = IndexedByteRegion::from_borrowed(&bytes, 17, 103);
        let (_, _, frozen) = frozen(&bytes);
        let composed = IndexedByteRegion::from_source(frozen.source_window().unwrap(), frozen.end_position().physical);
        for root in [&borrowed, &frozen, &composed] {
            let first = root.subview(range(left[0], left[1])).unwrap();
            let second = root.subview(range(right[0], right[1])).unwrap();
            let before = (located(&first), located(&second), first.end_position(), second.end_position());
            prop_assert_eq!(first.try_coalesce(&second).is_some(), left[1] == right[0]);
            prop_assert_eq!((located(&first), located(&second), first.end_position(), second.end_position()), before);
        }
    }
}

#[test]
fn equal_contents_and_source_numbers_do_not_establish_backing_identity() {
    let left = vec![4u8; 8];
    let right = vec![4u8; 8];
    let first = IndexedByteRegion::from_borrowed(&left, 7, 81);
    let second = IndexedByteRegion::from_borrowed(&right, 7, 81);
    let (_, _, frozen_first) = frozen(&left);
    let (_, _, frozen_second) = frozen(&right);
    for (first, second) in [(&first, &second), (&frozen_first, &frozen_second)] {
        assert!(first.content_eq(second));
        assert_eq!(located(first), located(second));
        assert!(first
            .subview(range(0, 4))
            .unwrap()
            .try_coalesce(&second.subview(range(4, 8)).unwrap())
            .is_none());
    }
    let altered_source = IndexedByteRegion::from_borrowed(&left, 8, 81);
    let altered_origin = IndexedByteRegion::from_borrowed(&left, 7, 82);
    for other in [&altered_source, &altered_origin] {
        assert!(first
            .subview(range(0, 4))
            .unwrap()
            .try_coalesce(&other.subview(range(4, 8)).unwrap())
            .is_none());
    }
    assert!(first
        .subview(range(0, 4))
        .unwrap()
        .try_coalesce(&frozen_first.subview(range(4, 8)).unwrap())
        .is_none());
}

#[test]
fn empty_and_single_byte_windows_preserve_the_right_anchor() {
    for bytes in [vec![], vec![19], vec![19, 23]] {
        let borrowed = IndexedByteRegion::from_borrowed(&bytes, 71, 300);
        let (_, _, frozen) = frozen(&bytes);
        for root in [&borrowed, &frozen] {
            for split in 0..=bytes.len() {
                exercise(root, 0, split, split, bytes.len());
                exercise(root, split, split, bytes.len(), bytes.len());
            }
            for anchor in [
                CanonicalByteProvenance::new(0, 0),
                CanonicalByteProvenance::new(u32::MAX, 0),
                CanonicalByteProvenance::new(0, usize::MAX),
                CanonicalByteProvenance::new(u32::MAX, usize::MAX),
            ] {
                let right = root
                    .subview_with_end(range(bytes.len(), bytes.len()), anchor)
                    .unwrap();
                let combined = root.try_coalesce(&right).unwrap();
                assert_view(&combined, &located(root), anchor);
                assert_eq!(
                    combined
                        .subview(range(0, bytes.len()))
                        .unwrap()
                        .end_position()
                        .physical,
                    anchor
                );
            }
        }
    }
}

#[test]
fn literal_boundary_byte_coalesces_without_a_new_backing() {
    let bytes: Vec<_> = (0..193).map(|index| index as u8).collect();
    let borrowed = IndexedByteRegion::from_borrowed(&bytes, 3, 87);
    let (_, _, frozen) = frozen(&bytes);
    for root in [&borrowed, &frozen] {
        for split in 1..bytes.len() {
            let prefix = root.subview(range(1, split)).unwrap();
            let boundary = root
                .subview_with_end(range(split, split + 1), origin(777))
                .unwrap();
            let combined = prefix.try_coalesce(&boundary).unwrap();
            assert_same_root(root, &combined);
            assert_view(&combined, &located(root)[1..split + 1], origin(777));
        }
    }
}

#[test]
fn frozen_inner_and_outer_offsets_normalize_across_anchors() {
    let bytes: Vec<_> = (0..160).map(|index| index as u8).collect();
    let (_, _, original) = frozen(&bytes);
    let left_root = original
        .subview_with_end(range(11, 99), origin(800))
        .unwrap();
    let right_root = original
        .subview_with_end(range(37, 140), origin(801))
        .unwrap();
    let left = left_root.subview(range(6, 49)).unwrap();
    let right = right_root
        .subview_with_end(range(23, 54), origin(802))
        .unwrap();
    let combined = left.try_coalesce(&right).unwrap();
    assert_same_root(&original, &combined);
    assert_view(&combined, &located(&original)[17..91], origin(802));
    let mut nested = combined;
    for _ in 0..20_000 {
        nested = nested.subview(range(0, nested.len())).unwrap();
    }
    assert_same_root(&original, &nested);
    assert_view(&nested, &located(&original)[17..91], origin(802));
}

#[test]
fn retained_windows_survive_parent_release_registration_and_sibling_append() {
    let bytes: Vec<_> = (0..193).map(|index| index as u8).collect();
    let (mut arena, handle, root) = frozen(&bytes);
    let expected = located(&root);
    let left = root.subview(range(3, 61)).unwrap();
    let right = root.subview_with_end(range(61, 130), origin(991)).unwrap();
    let mut registered = vec![root.clone()];
    let sibling = arena
        .append(handle, CanonicalLocatedByte::new(99, origin(900)))
        .unwrap();
    registered.push(IndexedByteRegion::from_frozen(
        arena.freeze(sibling),
        origin(901),
    ));
    assert_eq!(located(&registered[0]), expected);
    drop(root);
    drop(registered);
    drop(arena);
    assert_view(
        &left.try_coalesce(&right).unwrap(),
        &expected[3..130],
        origin(991),
    );
    let borrowed = IndexedByteRegion::from_borrowed(&bytes, 7, 37);
    let left = borrowed.subview(range(3, 61)).unwrap();
    let right = borrowed.subview(range(61, 130)).unwrap();
    let expected = located(&borrowed);
    drop(borrowed);
    assert_view(
        &left.try_coalesce(&right).unwrap(),
        &expected[3..130],
        CanonicalByteProvenance::new(7, 167),
    );
}

#[test]
fn invalid_ranges_and_overflow_refuse_without_changing_inputs() {
    let bytes = [3, 5, 7, 11];
    let borrowed = IndexedByteRegion::from_borrowed(&bytes, 7, 81);
    let (_, _, frozen) = frozen(&bytes);
    for root in [&borrowed, &frozen] {
        let left = root.subview(range(0, 2)).unwrap();
        let before = (located(&left), left.end_position());
        for candidate in [range(0, 2), range(1, 3), range(3, 4)] {
            assert!(left
                .try_coalesce(&root.subview(candidate).unwrap())
                .is_none());
        }
        for invalid in [range(2, 1), range(0, 5), range(usize::MAX, usize::MAX)] {
            assert!(root.subview(invalid).is_none());
        }
        assert_eq!((located(&left), left.end_position()), before);
    }
    let overflow = IndexedByteRegion::from_borrowed(&bytes, 7, usize::MAX - 1);
    let left = overflow.subview(range(0, 1)).unwrap();
    let right = overflow.subview(range(1, 4)).unwrap();
    assert!(left.try_coalesce(&right).is_none());
    assert!(overflow.subview(range(2, 3)).is_none());
    let invalid = BorrowedIndexedWindow::new(&bytes, 7, 81).rewindow(usize::MAX, None, 0);
    assert!(invalid.is_none());
    assert_eq!(located(&borrowed).len(), bytes.len());
}

#[test]
fn terminal_window_layout_reports_original_and_normalized_headers() {
    macro_rules! production_layout {
        ($name:ident, [$($field:ident: $kind:ty),*], [$($anchor:ident: $anchor_kind:ty),*], [$($extra:tt)*]) => {
            #[allow(dead_code)]
            enum $name<'a> {
                Borrowed {
                    bytes: &'a [u8],
                    $($field: $kind,)*
                    source: u32,
                    absolute_start: usize,
                    $($anchor: $anchor_kind,)*
                },
                Frozen {
                    root: FrozenCanonicalBytes,
                    start: usize,
                    len: usize,
                    root_end: CanonicalByteProvenance,
                },
                $($extra)*
            }
        };
    }
    production_layout!(OriginalProductionBacking, [], [end_override: Option<CanonicalByteProvenance>], []);
    production_layout!(NormalizedProductionBacking, [start: usize, len: usize], [end_source: Option<u32>, end_offset: usize], []);
    production_layout!(ComposedProductionBacking, [start: usize, len: usize], [end_source: Option<u32>, end_offset: usize], [
        Composed { root: SourceTreeWindow<'a>, start: usize, len: usize, root_end: CanonicalByteProvenance },
    ]);
    let before = std::mem::size_of::<OriginalProductionBacking<'_>>();
    let after = std::mem::size_of::<NormalizedProductionBacking<'_>>();
    let actual_test_layout = std::mem::size_of::<IndexedByteRegion<'_>>();
    let composed = std::mem::size_of::<ComposedProductionBacking<'_>>();
    println!("terminal-window layout: mirrored_production_before={before}, mirrored_production_after={after}, actual_test_region={actual_test_layout}, frozen_view={}", std::mem::size_of::<FrozenCanonicalBytes>());
    assert!(actual_test_layout >= after);
    assert_eq!(composed, after);
    println!("composed source layout: production_region={composed}, byte_cursor={}, located_cursor={}, reader={}, field_cursor={}",
        std::mem::size_of::<IndexedByteCursor<'_>>(),
        std::mem::size_of::<IndexedLocatedByteCursor<'_>>(),
        std::mem::size_of::<IndexedByteReader<'_>>(),
        std::mem::size_of::<IndexedCanonicalFieldCursor<'_>>());
    #[cfg(target_pointer_width = "64")]
    {
        assert_eq!(before, 64);
        assert_eq!(after, 64);
        assert_eq!(actual_test_layout, 72);
    }
}

#[test]
fn composed_mixed_sources_keep_lifetimes_and_exact_skip_accounting() {
    let bytes = (0..129).map(|at| at as u8).collect::<Vec<_>>();
    let (view, expected) = {
        let (mut arena, handle, frozen) = frozen(&bytes);
        let prefix = IndexedByteRegion::from_borrowed(&bytes, 91, 501)
            .subview(range(0, 61))
            .unwrap();
        let suffix = frozen.subview(range(61, 129)).unwrap();
        let expected = located(&prefix)
            .into_iter()
            .chain(located(&suffix))
            .collect::<Vec<_>>();
        let source = prefix
            .source_window()
            .unwrap()
            .concat(&suffix.source_window().unwrap())
            .unwrap();
        assert!(source.height() > 0);
        let parent = IndexedByteRegion::from_source(source, origin(991));
        let child = parent.subview_with_end(range(0, 129), origin(992)).unwrap();
        assert_same_root(&parent, &child);
        for at in 0..129 {
            arena
                .append(handle, CanonicalLocatedByte::new(0xff, origin(at)))
                .unwrap();
        }
        (child, expected)
    };
    assert_view(&view, &expected, origin(992));
    for skip in [0, 1, 60, 61, 62, 128, 129] {
        let mut cursor = view.located_bytes();
        assert!(cursor.skip_exact(skip));
        assert_eq!(
            (cursor.bytes_yielded(), cursor.bytes_traversed()),
            (skip, 0)
        );
        assert!(!cursor.skip_exact(130 - skip));
        assert_eq!(
            (cursor.bytes_yielded(), cursor.bytes_traversed()),
            (skip, 0)
        );
        if skip < expected.len() {
            assert_eq!(cursor.next(), Some(expected[skip]));
            assert_eq!(
                (cursor.bytes_yielded(), cursor.bytes_traversed()),
                (skip + 1, 1)
            );
            assert_eq!(cursor.collect::<Vec<_>>(), expected[skip + 1..]);
        } else {
            assert_eq!(cursor.next(), None);
        }
    }
    let mut nested = view.clone();
    for _ in 0..20_000 {
        nested = nested.subview(range(0, nested.len())).unwrap();
    }
    assert_same_root(&view, &nested);
    assert_view(&nested, &expected, origin(992));
}

#[test]
fn composed_conversion_requires_independent_end_authority_at_native_max() {
    let bytes = [11, 13, 17];
    let root = IndexedByteRegion::from_borrowed(&bytes, 37, usize::MAX);
    let selected = root.subview(range(0, 1)).unwrap();
    assert!(selected.source_window().is_some());
    assert!(selected.position_at(1).is_none());
    assert!(root.source_window().is_none());
    let anchored = selected.subview_with_end(range(0, 1), origin(300)).unwrap();
    let composed = IndexedByteRegion::from_source(
        anchored.source_window().unwrap(),
        anchored.position_at(1).unwrap().physical,
    );
    assert_view(
        &composed,
        &[CanonicalLocatedByte::new(
            11,
            CanonicalByteProvenance::new(37, usize::MAX),
        )],
        origin(300),
    );
    assert!(anchored.subview(range(1, 1)).is_none());
    assert_view(&composed.subview(range(1, 1)).unwrap(), &[], origin(300));
    let empty = composed.subview_with_end(range(1, 1), origin(301)).unwrap();
    assert_view(
        &composed.try_coalesce(&empty).unwrap(),
        &located(&composed),
        origin(301),
    );
    assert!(selected.position_at(1).is_none());
    let (arena, handle, _) = frozen(&bytes);
    assert!(IndexedByteRegion::from_arena(&arena, handle, origin(3))
        .source_window()
        .is_none());
    assert!(
        IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), 3)
            .source_window()
            .is_none()
    );
}

#[test]
fn composed_cross_backing_coalescing_preserves_original_owners_and_right_end() {
    let bytes = [1, 2, 3, 4, 5];
    let borrowed = IndexedByteRegion::from_borrowed(&bytes, 11, 71);
    let (_, _, frozen) = frozen(&bytes);
    for root in [&borrowed, &frozen] {
        for cut in 0..=bytes.len() {
            let left = root.subview_with_end(range(0, cut), origin(701)).unwrap();
            let right = root
                .subview_with_end(range(cut, bytes.len()), origin(702))
                .unwrap();
            let composed_left = IndexedByteRegion::from_source(
                left.source_window().unwrap(),
                left.end_position().physical,
            );
            let composed_right = IndexedByteRegion::from_source(
                right.source_window().unwrap(),
                right.end_position().physical,
            );
            assert_view(
                &composed_left.try_coalesce(&right).unwrap(),
                &located(root),
                origin(702),
            );
            assert_view(
                &left.try_coalesce(&composed_right).unwrap(),
                &located(root),
                origin(702),
            );
        }
    }
}

#[test]
fn composed_framing_matches_original_bytes_and_exact_terminal_errors() {
    for bytes in [
        vec![],
        vec![0x0a, 3, 1, 2, 3, 0x10, 0x7f],
        vec![0x80],
        vec![0x0a, 0x80],
        vec![0x0a, 4, 1, 2],
        vec![0x0a, 0, 0],
        vec![0x08, 0x80, 0],
        vec![0x0a, 0, 0x08, 1],
        vec![0x09, 1, 2, 3, 4, 5, 6, 7, 8],
        vec![0x0d, 1, 2, 3, 4],
        vec![0x09, 1],
        vec![0x0d],
        vec![0x0b],
    ] {
        let borrowed = IndexedByteRegion::from_borrowed(&bytes, 17, 101);
        for cut in 0..=bytes.len() {
            let mut arena = CanonicalBytesArena::new();
            let mut handle = arena.empty();
            for (at, byte) in bytes.iter().enumerate().skip(cut) {
                handle = arena
                    .append(
                        handle,
                        CanonicalLocatedByte::new(
                            *byte,
                            CanonicalByteProvenance::new(17, 101 + at),
                        ),
                    )
                    .unwrap();
            }
            let source = SourceTreeWindow::from_borrowed_range(&bytes, 0..cut, 17, 101)
                .unwrap()
                .concat(&SourceTreeWindow::from_frozen(arena.freeze(handle)))
                .unwrap();
            let composed = IndexedByteRegion::from_source(source, borrowed.end_position().physical);
            let mut left = IndexedCanonicalFieldCursor::new(borrowed.clone());
            let mut right = IndexedCanonicalFieldCursor::new(composed);
            loop {
                match (left.next_field(), right.next_field()) {
                    (Ok(Some(left_field)), Ok(Some(right_field))) => {
                        assert_eq!(left_field.number, right_field.number);
                        assert_eq!(left_field.wire_type, right_field.wire_type);
                        assert_eq!(left_field.encoded_range, right_field.encoded_range);
                        assert_eq!(left_field.payload_range, right_field.payload_range);
                        assert_eq!(left_field.field_start, right_field.field_start);
                        match (left_field.value, right_field.value) {
                            (
                                IndexedCanonicalFieldValue::Varint(a),
                                IndexedCanonicalFieldValue::Varint(b),
                            ) => assert_eq!(a, b),
                            (
                                IndexedCanonicalFieldValue::LengthDelimited(a),
                                IndexedCanonicalFieldValue::LengthDelimited(b),
                            )
                            | (
                                IndexedCanonicalFieldValue::Fixed32(a),
                                IndexedCanonicalFieldValue::Fixed32(b),
                            )
                            | (
                                IndexedCanonicalFieldValue::Fixed64(a),
                                IndexedCanonicalFieldValue::Fixed64(b),
                            ) => assert_view(&b, &located(&a), a.end_position().physical),
                            _ => panic!("unexpected field value in framing fixture"),
                        }
                    }
                    (Ok(None), Ok(None)) => break,
                    (Err(a), Err(b)) => {
                        assert_eq!(a, b);
                        assert_eq!(left.next_field().unwrap_err(), a);
                        assert_eq!(right.next_field().unwrap_err(), b);
                        break;
                    }
                    _ => panic!("composed framing changed the terminal observation"),
                }
                assert_eq!(left.stats(), right.stats());
            }
            assert_eq!(left.stats(), right.stats());
        }
    }
}

#[test]
fn all_backings_content_equality_preserves_bytes_across_chunk_partitions() {
    for bytes in [vec![], vec![7], (0..193).map(|at| at as u8).collect()] {
        let (arena, handle, frozen) = frozen(&bytes);
        let borrowed = IndexedByteRegion::from_borrowed(&bytes, 91, 501);
        let cut = bytes.len() / 2;
        let source = SourceTreeWindow::from_borrowed_range(&bytes, 0..cut, 91, 501)
            .unwrap()
            .concat(
                &frozen
                    .subview(range(cut, bytes.len()))
                    .unwrap()
                    .source_window()
                    .unwrap(),
            )
            .unwrap();
        let views = [
            borrowed.clone(),
            frozen.clone(),
            IndexedByteRegion::from_source(source, origin(981)),
            IndexedByteRegion::from_source(
                borrowed.source_window().unwrap(),
                borrowed.end_position().physical,
            ),
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), 101),
            IndexedByteRegion::from_arena(&arena, handle, origin(982)),
        ];
        for left in &views {
            for right in &views {
                assert!(left.content_eq(right));
                for cut in 0..bytes.len() {
                    assert!(!left.content_eq(&right.subview(range(0, cut)).unwrap()));
                }
            }
            for at in 0..bytes.len() {
                let mut changed = bytes.clone();
                changed[at] ^= 1;
                let other = IndexedByteRegion::from_borrowed(&changed, 91, 501);
                assert!(!left.content_eq(&other));
                assert!(!other.content_eq(left));
            }
        }
    }
}
