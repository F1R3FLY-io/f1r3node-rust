use proptest::prelude::*;

use super::super::canonical_bytes_arena::CanonicalLocatedByte;
use super::*;

fn assert_checked_map_push_backing_identity<'src>(
    region: IndexedByteRegion<'src>,
    independent: IndexedByteRegion<'src>,
) {
    assert_eq!(
        region.bytes().collect::<Vec<_>>(),
        independent.bytes().collect::<Vec<_>>()
    );
    assert_eq!(
        region.located_bytes().collect::<Vec<_>>(),
        independent.located_bytes().collect::<Vec<_>>()
    );
    assert_eq!(region.end_position(), independent.end_position());
    assert!(!region.same_view(&independent));
    assert!(region.same_view(&region.clone()));
    assert_eq!(
        MapPushViewIdentityClasses::capture(&region, &region, &independent),
        MapPushViewIdentityClasses {
            selected: 0,
            requested: 0,
            retained: 2,
        }
    );
    let distinct_handle = MapPushCheckedViewObservation::capture(&region, &region, &independent);
    let distinct_images = distinct_handle.indexed_images();
    assert_eq!(distinct_images[0], distinct_images[1]);
    assert_eq!(distinct_images[1], distinct_images[2]);
    assert_eq!(distinct_images[0].located.len(), region.len());
    for (at, located) in distinct_images[0].located.iter().enumerate() {
        assert_eq!(region.byte_at(at), Some(located.byte));
        assert_eq!(
            region.position_at(at).map(|position| position.physical),
            Some(located.provenance)
        );
    }
    assert_eq!(distinct_images[0].end, region.end_position().physical);
    assert!(!distinct_handle.all_same());
    assert_eq!(
        MapPushViewIdentityClasses::capture(&region, &independent, &independent),
        MapPushViewIdentityClasses {
            selected: 0,
            requested: 1,
            retained: 1,
        }
    );
    assert_eq!(
        MapPushViewIdentityClasses::capture(&region, &independent, &region),
        MapPushViewIdentityClasses {
            selected: 0,
            requested: 1,
            retained: 0,
        }
    );
    let changed_root_end = region
        .subview_with_end(
            CanonicalByteRange {
                start: 0,
                end: region.len(),
            },
            CanonicalByteProvenance::new(1_000_000, 0),
        )
        .unwrap();
    assert_eq!(
        MapPushViewIdentityClasses::capture(&region, &independent, &changed_root_end),
        MapPushViewIdentityClasses {
            selected: 0,
            requested: 1,
            retained: 2,
        }
    );
    assert!(!region.same_view(
        &region
            .subview(CanonicalByteRange { start: 0, end: 4 })
            .unwrap()
    ));

    let mut frames = Vec::new();
    let mut scores = CanonicalScoreArena::new();
    let mut stats = CanonicalProtobufAdmissionStats {
        frame_variant_bytes: std::mem::size_of::<Frame>(),
        ..CanonicalProtobufAdmissionStats::default()
    };
    push_admission_frame(
        &mut frames,
        FrameRequest::Message {
            region,
            program: &NEW_RHO_MATCH_PROGRAM,
        },
        &mut stats,
    )
    .unwrap();
    let before = DriverStepSource::before(&frames, &scores);
    let FrameStep::MessagePush { request, mut field } = frames
        .last_mut()
        .unwrap()
        .step(&mut stats, &mut scores)
        .unwrap()
    else {
        panic!("the New map field must push a Map child");
    };
    let FrameRequest::Map {
        region: requested, ..
    } = &request
    else {
        panic!("the generated StringMessageMap branch must select Map");
    };
    let IndexedCanonicalFieldValue::LengthDelimited(selected) = &field.field.value else {
        panic!("the Map field must select a length-delimited body");
    };
    assert!(selected.same_view(requested));
    let (selected_range, selected_source) = field.map_selected_subview().unwrap();
    assert_eq!(selected_range, field.field.payload_range);
    assert!(selected_source.same_view(selected));
    assert_eq!(
        MapPushIndexedImage::capture(&selected_source),
        MapPushIndexedImage::capture(selected)
    );
    for at in 0..selected.len() {
        assert_eq!(selected_source.byte_at(at), selected.byte_at(at));
        assert_eq!(selected_source.position_at(at), selected.position_at(at));
        assert_eq!(
            field.body.byte_at(selected_range.start + at),
            selected.byte_at(at)
        );
        assert_eq!(
            field
                .body
                .position_at(selected_range.start + at)
                .map(|position| position.physical),
            selected.position_at(at).map(|position| position.physical)
        );
    }
    assert_eq!(selected_source.end_position(), selected.end_position());
    assert_eq!(
        field
            .body
            .position_at(selected_range.end)
            .map(|position| position.physical),
        Some(selected.end_position().physical)
    );
    let changed_end = selected
        .subview_with_end(
            CanonicalByteRange {
                start: 0,
                end: selected.len(),
            },
            CanonicalByteProvenance::new(1_000_000, 0),
        )
        .unwrap();
    assert_eq!(
        selected.bytes().collect::<Vec<_>>(),
        changed_end.bytes().collect::<Vec<_>>()
    );
    assert_eq!(
        selected.located_bytes().collect::<Vec<_>>(),
        changed_end.located_bytes().collect::<Vec<_>>()
    );
    assert_ne!(selected.end_position(), changed_end.end_position());
    assert!(!selected.same_view(&changed_end));
    let pushed = push_admission_frame(&mut frames, request, &mut stats).unwrap();
    let Frame::Message(parent) = &frames[frames.len() - 2] else {
        panic!("the pushed Map must retain its Message parent");
    };
    let parent_bounds = field.map_parent_logical_bounds(parent).unwrap();
    assert_eq!(parent_bounds.start, 0);
    assert_eq!(parent_bounds.end, parent.body.len());
    assert!(parent.cursor.matches_message_view(&parent.body));
    assert_eq!(parent.cursor.logical_offset(), field.encoded_range.end);
    let narrowed = parent
        .body
        .subview(CanonicalByteRange {
            start: 0,
            end: parent.body.len() - 1,
        })
        .unwrap();
    assert!(!parent.cursor.matches_message_view(&narrowed));
    let Frame::Map(child) = frames.last().unwrap() else {
        panic!("the pushed child must be a Map frame");
    };
    assert!(selected.same_view(&child.region));
    let requested = field.map_request.as_ref().unwrap().region();
    assert_eq!(
        MapPushViewIdentityClasses::capture(selected, requested, &child.region),
        MapPushViewIdentityClasses {
            selected: 0,
            requested: 0,
            retained: 0,
        }
    );
    let observed = MapPushCheckedViewObservation::capture(selected, requested, &child.region);
    let images = observed.indexed_images();
    assert!(observed.all_same());
    assert_eq!(images[0], images[1]);
    assert_eq!(images[1], images[2]);
    assert!(field.matches_pushed_driver(&frames, &pushed));
    assert!(emit_checked_admission_event(CheckedAdmissionEvent {
        before: &before,
        after_frames: &frames,
        after_scores: &scores,
        action: CheckedAdmissionAction::MessageField {
            field: &field,
            pushed: Some(&pushed),
        },
        map_source: None,
    })
    .is_ok());

    let alternate = independent
        .subview(CanonicalByteRange { start: 2, end: 5 })
        .unwrap();
    assert_eq!(
        selected.bytes().collect::<Vec<_>>(),
        alternate.bytes().collect::<Vec<_>>()
    );
    assert_eq!(
        selected.located_bytes().collect::<Vec<_>>(),
        alternate.located_bytes().collect::<Vec<_>>()
    );
    assert_eq!(selected.end_position(), alternate.end_position());
    assert!(!selected.same_view(&alternate));
    let forged = MapPushCheckedViewObservation::capture(selected, &alternate, &child.region);
    let forged_images = forged.indexed_images();
    assert_eq!(forged_images[0], forged_images[1]);
    assert_eq!(forged_images[1], forged_images[2]);
    assert!(!forged.all_same());
    let original_request = field.map_request.take().unwrap();
    let FrameRequest::Map {
        owner,
        field: tag,
        value_type_id,
        ..
    } = &original_request
    else {
        unreachable!();
    };
    field.map_request = Some(FrameRequest::Map {
        region: alternate,
        owner: *owner,
        field: *tag,
        value_type_id: *value_type_id,
    });
    assert!(!field.matches_pushed_driver(&frames, &pushed));
    assert!(emit_checked_admission_event(CheckedAdmissionEvent {
        before: &before,
        after_frames: &frames,
        after_scores: &scores,
        action: CheckedAdmissionAction::MessageField {
            field: &field,
            pushed: Some(&pushed),
        },
        map_source: None,
    })
    .is_err());
    field.map_request = Some(original_request);
    assert!(field.matches_pushed_driver(&frames, &pushed));
    let original_end = field.encoded_range.end;
    field.encoded_range.end -= 1;
    assert!(field.map_selected_subview().is_none());
    assert!(!field.matches_pushed_driver(&frames, &pushed));
    field.encoded_range.end = original_end;
    assert!(field.matches_pushed_driver(&frames, &pushed));
}

fn append_map_fixture(
    arena: &mut CanonicalBytesArena<'_>,
    bytes: &[u8],
    source: u32,
    base: usize,
) -> super::super::canonical_bytes_arena::CanonicalBytesHandle {
    let mut handle = arena.empty();
    for (at, byte) in bytes.iter().enumerate() {
        handle = arena
            .append(
                handle,
                CanonicalLocatedByte::new(*byte, CanonicalByteProvenance::new(source, base + at)),
            )
            .unwrap();
    }
    handle
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn checked_map_push_preserves_all_five_backing_identities(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        base in 0usize..100_000,
    ) {
        let bytes = [0x22, 0x03, 0x0a, 0x01, key];
        let duplicate = bytes.to_vec();
        let end = CanonicalByteProvenance::new(source, base + bytes.len());

        assert_checked_map_push_backing_identity(
            IndexedByteRegion::from_borrowed(&bytes, source, base),
            IndexedByteRegion::from_borrowed(&duplicate, source, base),
        );
        assert_checked_map_push_backing_identity(
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), source),
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&bytes), source),
        );

        let mut first_arena = CanonicalBytesArena::new();
        let mut second_arena = CanonicalBytesArena::new();
        let first_handle = append_map_fixture(&mut first_arena, &bytes, source, base);
        let second_handle = append_map_fixture(&mut second_arena, &bytes, source, base);
        assert_checked_map_push_backing_identity(
            IndexedByteRegion::from_arena(&first_arena, first_handle, end),
            IndexedByteRegion::from_arena(&second_arena, second_handle, end),
        );
        assert_checked_map_push_backing_identity(
            IndexedByteRegion::from_frozen(first_arena.freeze(first_handle), end),
            IndexedByteRegion::from_frozen(second_arena.freeze(second_handle), end),
        );
        assert_checked_map_push_backing_identity(
            IndexedByteRegion::from_source(
                SourceTreeWindow::from_borrowed(&bytes, source, base).unwrap(), end,
            ),
            IndexedByteRegion::from_source(
                SourceTreeWindow::from_borrowed(&bytes, source, base).unwrap(), end,
            ),
        );
    }

    #[test]
    fn checked_map_push_binds_selected_backing_and_fresh_child(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x22, 0x03, 0x0a, 0x01, key];
        let duplicate = bytes.to_vec();
        let region = IndexedByteRegion::from_borrowed(&bytes, source, offset);
        let mut frames = Vec::new();
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..CanonicalProtobufAdmissionStats::default()
        };
        push_admission_frame(
            &mut frames,
            FrameRequest::Message {
                region,
                program: &NEW_RHO_MATCH_PROGRAM,
            },
            &mut stats,
        ).unwrap();
        let before = DriverStepSource::before(&frames, &scores);
        let FrameStep::MessagePush { request, mut field } =
            frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("the New map field must push a Map child");
        };
        let FrameRequest::Map { region: requested, .. } = &request else {
            panic!("the generated StringMessageMap branch must select Map");
        };
        let IndexedCanonicalFieldValue::LengthDelimited(selected) = &field.field.value else {
            panic!("the Map field must select a length-delimited body");
        };
        prop_assert!(selected.same_view(requested));
        let saved_view_matches = field.map_request.as_ref().is_some_and(|saved| {
            saved.region().same_view(requested)
        });
        prop_assert!(saved_view_matches);
        let pushed = push_admission_frame(&mut frames, request, &mut stats).unwrap();
        let Frame::Map(child) = frames.last().unwrap() else {
            panic!("the pushed child must be a Map frame");
        };
        prop_assert!(selected.same_view(&child.region));
        prop_assert!(child.cursor.matches_initial_view(&child.region, true));
        prop_assert!(child.key.is_none());
        prop_assert!(!child.saw_value);
        prop_assert!(!child.waiting_value);
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        let checked = emit_checked_admission_event(CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::MessageField {
                field: &field,
                pushed: Some(&pushed),
            },
            map_source: None,
        });
        prop_assert!(checked.is_ok());

        let alternate = IndexedByteRegion::from_borrowed(&duplicate, source, offset)
            .subview(CanonicalByteRange { start: 2, end: 5 }).unwrap();
        prop_assert_eq!(alternate.bytes().collect::<Vec<_>>(),
            selected.bytes().collect::<Vec<_>>());
        prop_assert_eq!(alternate.position_at(0), selected.position_at(0));
        prop_assert_eq!(alternate.end_position(), selected.end_position());
        prop_assert!(!alternate.same_view(selected));
        let original_request = field.map_request.take().unwrap();
        let FrameRequest::Map { owner, field: tag, value_type_id, .. } = &original_request else {
            unreachable!();
        };
        field.map_request = Some(FrameRequest::Map {
            region: alternate,
            owner: *owner,
            field: *tag,
            value_type_id: *value_type_id,
        });
        prop_assert!(!field.matches_pushed_driver(&frames, &pushed));
        field.map_request = Some(original_request);

        let Frame::Map(child) = frames.last_mut().unwrap() else { unreachable!() };
        child.saw_value = true;
        prop_assert!(!field.matches_pushed_driver(&frames, &pushed));
        let Frame::Map(child) = frames.last_mut().unwrap() else { unreachable!() };
        child.saw_value = false;
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
    }
}
