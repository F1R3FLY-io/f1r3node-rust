use proptest::prelude::*;

use super::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn checked_map_push_observation_projects_fresh_child_fields(
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
        let Frame::Message(parent_before) = frames.last().unwrap() else {
            panic!("the root must be a Message frame");
        };
        let expected_cursor_before = parent_before.cursor.stats();
        let expected_last_slot_before = parent_before.slots.last_slot();
        let expected_seen_before = parent_before.slots.seen_slice().to_vec();
        let before = DriverStepSource::before(&frames, &scores);
        let FrameStep::MessagePush { request, mut field } = frames
            .last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("the New map field must push a Map child");
        };
        let FrameRequest::Map {
            region: requested,
            owner,
            field: outer_field,
            value_type_id,
        } = &request else {
            panic!("the generated Map branch must retain its request");
        };
        let requested_start = requested.position_at(0).unwrap().physical;
        let requested_end = requested.end_position().physical;
        let expected_body = requested.bytes().collect::<Vec<_>>();
        let expected_owner = owner.type_id;
        let expected_field = *outer_field;
        let expected_value_type = *value_type_id;
        let actual_last_slot_before = field.last_slot_before;
        let actual_seen_before = field.seen_before;
        let actual_cursor_before = field.cursor_before;
        prop_assert_eq!(field.prestate.cursor, expected_cursor_before);
        prop_assert_eq!(field.prestate.last_slot, expected_last_slot_before);
        prop_assert_eq!(field.prestate.seen, expected_seen_before[field.resolved.slot()]);
        let pushed = push_admission_frame(&mut frames, request, &mut stats).unwrap();
        let saved = field.map_request.as_ref().unwrap();
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        {
            let event = CheckedAdmissionEvent {
                before: &before,
                after_frames: &frames,
                after_scores: &scores,
                action: CheckedAdmissionAction::MessageField {
                    field: &field,
                    pushed: Some(&pushed),
                },
                map_source: None,
            };
            let parent_core = event.map_push_parent_core_observation().unwrap();
            prop_assert!(parent_core.body.same_view(&field.body));
            prop_assert_eq!(parent_core.seen.len(), expected_seen_before.len());
            prop_assert_eq!(parent_core.semantic_slots.len(), expected_seen_before.len());
            prop_assert_eq!(
                parent_core.seen_before_at(field.resolved.slot()),
                Some(expected_seen_before[field.resolved.slot()]),
            );
            let observed = event.map_push_child_observation().unwrap();
            let image = observed.indexed_image();
            prop_assert_eq!(observed.owner_type_id, expected_owner);
            prop_assert_eq!(observed.outer_field, expected_field);
            prop_assert_eq!(observed.value_type_id, expected_value_type);
            prop_assert_eq!(observed.physical_start, requested_start);
            prop_assert_eq!(observed.region.len(), expected_body.len());
            prop_assert_eq!(
                image.located.iter().map(|located| located.byte).collect::<Vec<_>>(),
                expected_body
            );
            prop_assert_eq!(image.end, requested_end);
            prop_assert_eq!(observed.owner_type_id, field.program_type_id);
            prop_assert_eq!(observed.outer_field, field.field_number);
            prop_assert_eq!(observed.value_type_id, field.child_type_id.unwrap());
            prop_assert_eq!(observed.region.len(), field.child_region_len.unwrap());
            prop_assert!(observed.is_initial());
            prop_assert!(emit_checked_admission_event(event).is_ok());
        }

        prop_assert_ne!(field.resolved.slot(), 0);
        let saved_slots = if let Frame::Message(parent) = &mut frames[pushed.before_len - 1] {
            let mut forged = VerifiedMessageSlots::new(parent.slots.len());
            for (slot, seen) in parent.slots.seen_slice().iter().enumerate() {
                if slot == 0 || *seen {
                    forged.admit(slot, true).expect("generated slot order is increasing");
                }
            }
            Some(std::mem::replace(&mut parent.slots, forged))
        } else {
            None
        };
        {
            let event = CheckedAdmissionEvent {
                before: &before,
                after_frames: &frames,
                after_scores: &scores,
                action: CheckedAdmissionAction::MessageField {
                    field: &field,
                    pushed: Some(&pushed),
                },
                map_source: None,
            };
            let forged = event.map_push_parent_core_observation().unwrap();
            prop_assert_ne!(forged.seen_before_at(0), Some(expected_seen_before[0]));
            prop_assert!(event.map_push_child_observation().is_some());
        }
        if let Frame::Message(parent) = &mut frames[pushed.before_len - 1] {
            parent.slots = saved_slots.expect("checked Map push has a message parent");
        }

        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.waiting_value = true;
        }
        prop_assert!(MapPushCheckedChildObservation::from_pushed(saved, frames.last().unwrap()).is_none());
        prop_assert!(!field.matches_pushed_driver(&frames, &pushed));
        {
            let event = CheckedAdmissionEvent {
                before: &before,
                after_frames: &frames,
                after_scores: &scores,
                action: CheckedAdmissionAction::MessageField {
                    field: &field,
                    pushed: Some(&pushed),
                },
                map_source: None,
            };
            prop_assert!(event.map_push_child_observation().is_none());
            prop_assert!(emit_checked_admission_event(event).is_err());
        }
        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.waiting_value = false;
            child.value_type_id = child.value_type_id.wrapping_add(1);
        }
        prop_assert!(MapPushCheckedChildObservation::from_pushed(saved, frames.last().unwrap()).is_none());
        prop_assert!(!field.matches_pushed_driver(&frames, &pushed));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::MessageField {
                field: &field,
                pushed: Some(&pushed),
            },
            map_source: None,
        };
        prop_assert!(event.map_push_child_observation().is_none());
        prop_assert!(emit_checked_admission_event(event).is_err());

        if let Frame::Map(child) = frames.last_mut().unwrap() {
            child.value_type_id = expected_value_type;
        }
        field.child_type_id = Some(expected_value_type.wrapping_add(1));
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::MessageField {
                field: &field,
                pushed: Some(&pushed),
            },
            map_source: None,
        };
        prop_assert!(event.map_push_child_observation().is_none());
        prop_assert!(emit_checked_admission_event(event).is_err());

        field.child_type_id = Some(expected_value_type);
        field.last_slot_before = Some(field.resolved.slot());
        prop_assert_ne!(field.last_slot_before, actual_last_slot_before);
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::MessageField {
                field: &field,
                pushed: Some(&pushed),
            },
            map_source: None,
        };
        prop_assert!(event.map_push_child_observation().is_none());
        prop_assert!(emit_checked_admission_event(event).is_err());

        field.last_slot_before = actual_last_slot_before;
        prop_assert!(field.resolved.repeated());
        field.seen_before = !actual_seen_before;
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::MessageField {
                field: &field,
                pushed: Some(&pushed),
            },
            map_source: None,
        };
        prop_assert!(event.map_push_child_observation().is_none());
        prop_assert!(emit_checked_admission_event(event).is_err());

        field.seen_before = actual_seen_before;
        field.cursor_before.bytes_traversed = actual_cursor_before.bytes_traversed.wrapping_add(1);
        prop_assert!(field.matches_pushed_driver(&frames, &pushed));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::MessageField {
                field: &field,
                pushed: Some(&pushed),
            },
            map_source: None,
        };
        prop_assert!(event.map_push_child_observation().is_none());
        prop_assert!(emit_checked_admission_event(event).is_err());
    }
}
