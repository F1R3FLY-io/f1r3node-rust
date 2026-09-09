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
        (
            IndexedByteBacking::Borrowed { bytes: left, .. },
            IndexedByteBacking::Borrowed { bytes: right, .. },
        ) => {
            assert!(std::ptr::eq(*left, *right));
        }
        (
            IndexedByteBacking::Frozen { root: left, .. },
            IndexedByteBacking::Frozen { root: right, .. },
        ) => {
            assert!(left.shares_backing_with(right));
        }
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
        for root in [&borrowed, &frozen] {
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
    let malformed = IndexedByteRegion {
        backing: IndexedByteBacking::Borrowed {
            bytes: &bytes,
            start: 1,
            len: usize::MAX,
            source: 7,
            absolute_start: 81,
            end_source: None,
            end_offset: 0,
        },
    };
    assert!(malformed.try_coalesce(&borrowed).is_none());
}

#[test]
fn terminal_window_layout_reports_original_and_normalized_headers() {
    macro_rules! production_layout {
        ($name:ident, [$($field:ident: $kind:ty),*], [$($anchor:ident: $anchor_kind:ty),*]) => {
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
            }
        };
    }
    production_layout!(OriginalProductionBacking, [], [end_override: Option<CanonicalByteProvenance>]);
    production_layout!(NormalizedProductionBacking, [start: usize, len: usize], [end_source: Option<u32>, end_offset: usize]);
    let before = std::mem::size_of::<OriginalProductionBacking<'_>>();
    let after = std::mem::size_of::<NormalizedProductionBacking<'_>>();
    let actual_test_layout = std::mem::size_of::<IndexedByteRegion<'_>>();
    println!("terminal-window layout: mirrored_production_before={before}, mirrored_production_after={after}, actual_test_region={actual_test_layout}, frozen_view={}", std::mem::size_of::<FrozenCanonicalBytes>());
    assert!(actual_test_layout >= after);
    #[cfg(target_pointer_width = "64")]
    {
        assert_eq!(before, 64);
        assert_eq!(after, 64);
        assert_eq!(actual_test_layout, 72);
    }
}
