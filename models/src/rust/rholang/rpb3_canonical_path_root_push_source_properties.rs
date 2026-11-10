use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;

fn frozen(
    bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
) -> OwnedIndexedByteRegion {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (&byte, &anchor) in bytes.iter().zip(anchors) {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, anchor))
            .expect("bounded root push fixture fits the arena");
    }
    OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), end)
}

fn assert_nonempty_root_binding<'src>(
    selected: IndexedByteRegion<'src>,
    alternate: IndexedByteRegion<'src>,
    bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
    altered_end: CanonicalByteProvenance,
) {
    assert!(!bytes.is_empty());
    assert_eq!(selected.bytes().collect::<Vec<_>>(), bytes);
    assert_eq!(selected.end_position().physical, end);
    assert_eq!(alternate.bytes().collect::<Vec<_>>(), bytes);
    assert_eq!(alternate.end_position(), selected.end_position());
    for index in 0..bytes.len() {
        assert_eq!(
            selected.position_at(index).unwrap().physical,
            anchors[index]
        );
        assert_eq!(alternate.position_at(index), selected.position_at(index));
    }
    assert!(!alternate.same_view(&selected));

    let scores = CanonicalScoreArena::new();
    let request = FrameRequest::CanonicalPath(selected.clone());
    let source_image = RootInitializationSource::before_push(&request, &scores)
        .expect("CanonicalPath is an allowed root request");
    assert!(source_image
        .canonical_path_input
        .as_ref()
        .unwrap()
        .same_view(&selected));
    let mut stats = CanonicalProtobufAdmissionStats {
        frame_variant_bytes: std::mem::size_of::<Frame>(),
        ..Default::default()
    };
    let mut frames = Vec::with_capacity(1);
    let pushed = push_admission_frame(&mut frames, request.clone(), &mut stats).unwrap();
    assert!(request.matches_pushed_frame(&frames[0]));
    assert!(source_image.matches_root(&pushed, &frames, &scores));

    let Frame::CanonicalPath(frame) = &mut frames[0] else {
        unreachable!()
    };
    assert!(frame.cursor.next().is_some());
    assert!(!request.matches_pushed_frame(&frames[0]));
    assert!(!source_image.matches_root(&pushed, &frames, &scores));
    let Frame::CanonicalPath(frame) = &mut frames[0] else {
        unreachable!()
    };
    frame.cursor = frame.input.located_bytes();
    assert!(request.matches_pushed_frame(&frames[0]));
    assert!(source_image.matches_root(&pushed, &frames, &scores));

    let Frame::CanonicalPath(frame) = &mut frames[0] else {
        unreachable!()
    };
    frame.input = alternate;
    assert_eq!(frame.input.len(), selected.len());
    assert_eq!(frame.input.end_position(), selected.end_position());
    assert!(!source_image.matches_root(&pushed, &frames, &scores));

    let Frame::CanonicalPath(frame) = &mut frames[0] else {
        unreachable!()
    };
    frame.input = selected
        .subview_with_end(
            CanonicalByteRange {
                start: 0,
                end: selected.len(),
            },
            altered_end,
        )
        .unwrap();
    assert_eq!(frame.input.bytes().collect::<Vec<_>>(), bytes);
    assert_ne!(frame.input.end_position().physical, end);
    assert!(!source_image.matches_root(&pushed, &frames, &scores));

    let Frame::CanonicalPath(frame) = &mut frames[0] else {
        unreachable!()
    };
    frame.input = selected.clone();
    assert!(source_image.matches_root(&pushed, &frames, &scores));
    let (summary, _, _) =
        drive_admission(FrameRequest::CanonicalPath(selected), bytes.len()).unwrap();
    assert!(matches!(summary, FrameSummary::CanonicalPath(_)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn canonical_path_root_push_binds_the_selected_fragmented_indexed_view(
        payload in prop::collection::vec(any::<u8>(), 0..64),
        source in 100u32..1000,
    ) {
        let mut bytes = vec![6, payload.len() as u8];
        bytes.extend_from_slice(&payload);
        let anchors = (0..bytes.len())
            .map(|index| CanonicalByteProvenance::new(source + index as u32 % 23, 47 + index * 29))
            .collect::<Vec<_>>();
        let end = CanonicalByteProvenance::new(source + 100, 90_000);
        let selected = frozen(&bytes, &anchors, end);
        let alternate = frozen(&bytes, &anchors, end);
        let altered_end = CanonicalByteProvenance::new(source + 101, 90_001);
        assert_nonempty_root_binding(selected, alternate, &bytes, &anchors, end, altered_end);
    }

    #[test]
    fn canonical_path_root_push_binds_borrowed_and_composed_indexed_views(
        payload in prop::collection::vec(any::<u8>(), 0..64),
        source in 100u32..1000,
        base in 0usize..100_000,
    ) {
        let mut bytes = vec![6, payload.len() as u8];
        bytes.extend_from_slice(&payload);
        let shadow = bytes.clone();
        let anchors = (0..bytes.len())
            .map(|index| CanonicalByteProvenance::new(source, base + index))
            .collect::<Vec<_>>();
        let end = CanonicalByteProvenance::new(source, base + bytes.len());
        let altered_end = CanonicalByteProvenance::new(source + 1, base + bytes.len());
        assert_nonempty_root_binding(
            IndexedByteRegion::from_borrowed(&bytes, source, base),
            IndexedByteRegion::from_borrowed(&shadow, source, base),
            &bytes, &anchors, end, altered_end,
        );
        assert_nonempty_root_binding(
            IndexedByteRegion::from_source(
                SourceTreeWindow::from_borrowed(&bytes, source, base).unwrap(), end,
            ),
            IndexedByteRegion::from_source(
                SourceTreeWindow::from_borrowed(&shadow, source, base).unwrap(), end,
            ),
            &bytes, &anchors, end, altered_end,
        );
    }

    #[test]
    fn canonical_path_root_push_accepts_canonical_empty_composed_view_and_checks_end(
        source in 100u32..1000,
        offset in 0usize..100_000,
    ) {
        let end = CanonicalByteProvenance::new(source, offset);
        let altered_end = CanonicalByteProvenance::new(source + 1, offset);
        let selected = IndexedByteRegion::from_source(SourceTreeWindow::empty(), end);
        let same_empty = IndexedByteRegion::from_source(SourceTreeWindow::empty(), end);
        assert_eq!(selected.len(), 0);
        assert_eq!(selected.position_at(0).unwrap().physical, end);
        assert!(same_empty.same_view(&selected));
        let scores = CanonicalScoreArena::new();
        let request = FrameRequest::CanonicalPath(selected.clone());
        let source_image = RootInitializationSource::before_push(&request, &scores).unwrap();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..Default::default()
        };
        let mut frames = Vec::with_capacity(1);
        let pushed = push_admission_frame(&mut frames, request, &mut stats).unwrap();
        assert!(source_image.matches_root(&pushed, &frames, &scores));
        let Frame::CanonicalPath(frame) = &mut frames[0] else { unreachable!() };
        frame.input = same_empty;
        assert!(source_image.matches_root(&pushed, &frames, &scores));
        let Frame::CanonicalPath(frame) = &mut frames[0] else { unreachable!() };
        frame.input = IndexedByteRegion::from_source(SourceTreeWindow::empty(), altered_end);
        assert_eq!(frame.input.position_at(0).unwrap().physical, altered_end);
        assert!(!source_image.matches_root(&pushed, &frames, &scores));
    }
}
