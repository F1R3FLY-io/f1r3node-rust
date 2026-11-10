use proptest::prelude::*;

use super::*;

fn assert_map_child_forgery_rejected<'src>(
    frames: &[Frame<'src>],
    field: &MessageFieldSourceReceipt<'src>,
    pushed: &FramePushObservation,
) {
    let saved = field.map_request.as_ref().unwrap();
    assert!(!saved.matches_pushed_frame(frames.last().unwrap()));
    assert!(!field.matches_pushed_driver(frames, pushed));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn checked_map_push_child_starts_fresh_and_rejects_forged_state(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        base in 0usize..100_000,
    ) {
        let bytes = [0x22, 0x03, 0x0a, 0x01, key];
        let mut frames = Vec::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..CanonicalProtobufAdmissionStats::default()
        };
        let mut scores = CanonicalScoreArena::new();
        push_admission_frame(
            &mut frames,
            FrameRequest::Message {
                region: IndexedByteRegion::from_borrowed(&bytes, source, base),
                program: &NEW_RHO_MATCH_PROGRAM,
            },
            &mut stats,
        ).unwrap();
        let FrameStep::MessagePush { request, field } = frames
            .last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("the New map field must push a Map child");
        };
        let pushed = push_admission_frame(&mut frames, request, &mut stats).unwrap();
        let saved = field.map_request.as_ref().unwrap();
        let Frame::Map(child) = frames.last().unwrap() else {
            panic!("the checked Map request must create a Map frame");
        };
        prop_assert!(saved.matches_pushed_frame(frames.last().unwrap()));
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        prop_assert!(child.cursor.matches_initial_view(&child.region, true));
        prop_assert!(child.key.is_none());
        prop_assert!(child.key_position.is_none());
        prop_assert!(!child.saw_value);
        prop_assert!(child.value_position.is_none());
        prop_assert!(child.value.is_none());
        prop_assert!(!child.waiting_value);

        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.saw_value = true;
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.saw_value = false;
            child.waiting_value = true;
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.waiting_value = false;
            child.key = Some(child.region.clone());
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.key = None;
            child.key_position = Some(child.region_start);
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.key_position = None;
            child.value_position = Some(child.region_start);
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.value_position = None;
            child.outer_field = child.outer_field.wrapping_add(1);
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.outer_field = child.outer_field.wrapping_sub(1);
            child.value_type_id = child.value_type_id.wrapping_add(1);
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.value_type_id = child.value_type_id.wrapping_sub(1);
            child.cursor = IndexedCanonicalFieldCursor::in_prost_slot_order(child.region.clone());
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.cursor = IndexedCanonicalFieldCursor::new(child.region.clone());
            child.owner = &PAR_PROTOBUF_REGION_PROGRAM;
        }
        assert_map_child_forgery_rejected(&frames, &field, &pushed);
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.owner = &NEW_RHO_MATCH_PROGRAM;
        }
        prop_assert!(saved.matches_pushed_frame(frames.last().unwrap()));
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
    }
}
