//! Independent byte and physical-anchor properties for indexed source views.
//!
//! This is included as a test-only child of indexed_protobuf.rs. Expected
//! images come from generated input tuples, not from another indexed cursor.

use proptest::prelude::*;

use super::*;

type ExpectedByte = (u8, CanonicalByteProvenance);

fn expected_end(
    bytes: &[ExpectedByte],
    end: CanonicalByteProvenance,
    at: usize,
) -> CanonicalByteProvenance {
    bytes.get(at).map_or(end, |(_, position)| *position)
}

fn assert_image(
    view: &IndexedByteRegion<'_>,
    expected: &[ExpectedByte],
    end: CanonicalByteProvenance,
) {
    assert_eq!(view.len(), expected.len());
    assert_eq!(
        view.bytes().collect::<Vec<_>>(),
        expected.iter().map(|(byte, _)| *byte).collect::<Vec<_>>(),
    );
    assert_eq!(
        view.located_bytes().collect::<Vec<_>>(),
        expected
            .iter()
            .map(|(byte, position)| CanonicalLocatedByte::new(*byte, *position))
            .collect::<Vec<_>>(),
    );
    for (at, (byte, position)) in expected.iter().enumerate() {
        assert_eq!(view.byte_at(at), Some(*byte));
        assert_eq!(
            view.position_at(at),
            Some(IndexedBytePosition {
                logical_offset: at,
                physical: *position,
                kind: IndexedBytePositionKind::Byte,
            }),
        );
    }
    assert_eq!(
        view.position_at(expected.len()),
        Some(IndexedBytePosition {
            logical_offset: expected.len(),
            physical: end,
            kind: IndexedBytePositionKind::End,
        }),
    );
    assert_eq!(
        view.end_position(),
        view.position_at(expected.len()).unwrap()
    );
    assert_eq!(view.byte_at(expected.len()), None);
    assert_eq!(view.byte_at(expected.len() + 1), None);
    assert_eq!(view.byte_at(usize::MAX), None);
    assert!(view.position_at(expected.len() + 1).is_none());
}

fn assert_same_view_image<'a>(
    left: &IndexedByteRegion<'a>,
    right: &IndexedByteRegion<'a>,
    expected: &[ExpectedByte],
    end: CanonicalByteProvenance,
) {
    assert!(left.same_view(right));
    assert_image(right, expected, end);
}

fn range(start: usize, end: usize) -> CanonicalByteRange { CanonicalByteRange { start, end } }

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn borrowed_root_and_subviews_match_independent_linear_image(
        bytes in prop::collection::vec(any::<u8>(), 0..48),
        source in 1u32..1000,
        base in 0usize..100_000,
        first_cut in any::<u8>(),
        second_cut in any::<u8>(),
        override_offset in 0usize..100_000,
    ) {
        let root = IndexedByteRegion::from_borrowed(&bytes, source, base);
        let expected: Vec<_> = bytes.iter().enumerate()
            .map(|(at, byte)| (*byte, CanonicalByteProvenance::new(source, base + at)))
            .collect();
        let root_end = CanonicalByteProvenance::new(source, base + bytes.len());
        assert_image(&root, &expected, root_end);
        assert_same_view_image(&root, &root.clone(), &expected, root_end);

        let a = usize::from(first_cut) % (bytes.len() + 1);
        let b = usize::from(second_cut) % (bytes.len() + 1);
        let (start, end) = (a.min(b), a.max(b));
        let view = root.subview(range(start, end)).unwrap();
        assert_image(&view, &expected[start..end], expected_end(&expected, root_end, end));
        assert_same_view_image(&view, &view.clone(), &expected[start..end],
            expected_end(&expected, root_end, end));

        let explicit_end = CanonicalByteProvenance::new(4_001, override_offset);
        let overridden = root.subview_with_end(range(start, end), explicit_end).unwrap();
        assert_image(&overridden, &expected[start..end], explicit_end);
        assert!(!view.same_view(&overridden));
    }

    #[test]
    fn frozen_fragmented_root_and_subviews_keep_independent_anchors(
        input in prop::collection::vec((any::<u8>(), 1u32..1000, 0usize..100_000), 0..40),
        end_source in 1_000u32..2_000,
        end_offset in 0usize..100_000,
        first_cut in any::<u8>(),
        second_cut in any::<u8>(),
        override_offset in 0usize..100_000,
    ) {
        let expected: Vec<_> = input.iter()
            .map(|(byte, source, offset)| {
                (*byte, CanonicalByteProvenance::new(*source, *offset))
            })
            .collect();
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (byte, position) in &expected {
            handle = arena.append(handle, CanonicalLocatedByte::new(*byte, *position)).unwrap();
        }
        let root_end = CanonicalByteProvenance::new(end_source, end_offset);
        let root = IndexedByteRegion::from_frozen(arena.freeze(handle), root_end);
        assert_image(&root, &expected, root_end);
        assert_same_view_image(&root, &root.clone(), &expected, root_end);

        let a = usize::from(first_cut) % (expected.len() + 1);
        let b = usize::from(second_cut) % (expected.len() + 1);
        let (start, end) = (a.min(b), a.max(b));
        let view = root.subview(range(start, end)).unwrap();
        assert_image(&view, &expected[start..end], expected_end(&expected, root_end, end));

        let explicit_end = CanonicalByteProvenance::new(4_002, override_offset);
        let overridden = root.subview_with_end(range(start, end), explicit_end).unwrap();
        assert_image(&overridden, &expected[start..end], explicit_end);
        assert!(!view.same_view(&overridden));
    }

    #[test]
    fn composed_two_source_seam_and_crossing_subview_match_input_oracle(
        bytes in prop::collection::vec(any::<u8>(), 2..48),
        split_seed in any::<u8>(),
        left_source in 1u32..1000,
        right_source in 1_000u32..2_000,
        left_base in 0usize..100_000,
        right_base in 0usize..100_000,
        end_offset in 0usize..100_000,
    ) {
        let cut = usize::from(split_seed) % (bytes.len() - 1) + 1;
        let left = SourceTreeWindow::from_borrowed(&bytes[..cut], left_source, left_base).unwrap();
        let right =
            SourceTreeWindow::from_borrowed(&bytes[cut..], right_source, right_base).unwrap();
        let composed = left.concat(&right).unwrap();
        let root_end = CanonicalByteProvenance::new(3_000, end_offset);
        let root = IndexedByteRegion::from_source(composed, root_end);
        let expected: Vec<_> = bytes.iter().enumerate().map(|(at, byte)| {
            let position = if at < cut {
                CanonicalByteProvenance::new(left_source, left_base + at)
            } else {
                CanonicalByteProvenance::new(right_source, right_base + at - cut)
            };
            (*byte, position)
        }).collect();
        assert_image(&root, &expected, root_end);
        assert_ne!(expected[cut - 1].1.source, expected[cut].1.source);
        assert_same_view_image(&root, &root.clone(), &expected, root_end);

        let crossing = root.subview(range(cut - 1, cut + 1)).unwrap();
        assert_image(&crossing, &expected[cut - 1..cut + 1],
            expected_end(&expected, root_end, cut + 1));
        let explicit_end = CanonicalByteProvenance::new(4_003, end_offset);
        let overridden = root.subview_with_end(range(cut - 1, cut + 1), explicit_end).unwrap();
        assert_image(&overridden, &expected[cut - 1..cut + 1], explicit_end);
        assert!(!crossing.same_view(&overridden));
    }

    #[test]
    fn empty_backings_and_explicit_end_have_terminal_position(
        source in 1u32..1000,
        base in 0usize..100_000,
        end_source in 1_000u32..2_000,
        end_offset in 0usize..100_000,
    ) {
        let empty = Vec::<u8>::new();
        let borrowed = IndexedByteRegion::from_borrowed(&empty, source, base);
        assert_image(&borrowed, &[], CanonicalByteProvenance::new(source, base));

        let arena = CanonicalBytesArena::new();
        let frozen_end = CanonicalByteProvenance::new(end_source, end_offset);
        let frozen = IndexedByteRegion::from_frozen(arena.freeze(arena.empty()), frozen_end);
        assert_image(&frozen, &[], frozen_end);
        let composed = IndexedByteRegion::from_source(SourceTreeWindow::empty(), frozen_end);
        assert_image(&composed, &[], frozen_end);

        let explicit_end = CanonicalByteProvenance::new(4_004, end_offset);
        for (view, end) in [
            (&borrowed, CanonicalByteProvenance::new(source, base)),
            (&frozen, frozen_end),
            (&composed, frozen_end),
        ] {
            assert_same_view_image(view, &view.clone(), &[], end);
            let override_view = view.subview_with_end(range(0, 0), explicit_end).unwrap();
            assert_image(&override_view, &[], explicit_end);
            assert!(!view.same_view(&override_view));
        }
    }
}
