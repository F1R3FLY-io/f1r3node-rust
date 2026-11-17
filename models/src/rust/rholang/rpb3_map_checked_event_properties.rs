use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn checked_map_key_event_uses_the_live_pre_cursor(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x0a, 0x01, key];
        let region = IndexedByteRegion::from_borrowed(&bytes, source, offset);
        let mut frames = vec![Frame::Map(MapFrame::from_map_region(
            &NEW_RHO_MATCH_PROGRAM,
            4,
            PAR_PROTOBUF_REGION_PROGRAM.type_id,
            region,
        ))];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let mut before = DriverStepSource::before(&frames, &scores);

        let FrameStep::Other {
            action: FrameAction::Continue,
            map_source: Some(MapStepSource::Field(field)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("a canonical map key must continue with its checked source field");
        };
        let map_source = MapStepSource::Field(field);
        let MapStepSource::Field(field) = &map_source else { unreachable!() };
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherContinue,
            map_source: Some(&map_source),
            accepted_children: None,
        };
        prop_assert!(emit_checked_admission_event(event).is_ok());
        prop_assert_eq!(before.map_cursor, Some(field.cursor_before));
        prop_assert_eq!(field.cursor_after.bytes_read, bytes.len());
        prop_assert_eq!(field.field_position, CanonicalByteProvenance::new(source, offset));
        prop_assert_eq!(field.payload_position, CanonicalByteProvenance::new(source, offset + 2));

        before.map_cursor.as_mut().unwrap().bytes_read += 1;
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherContinue,
            map_source: Some(&map_source),
            accepted_children: None,
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));
    }

    #[test]
    fn checked_map_value_push_uses_the_selected_payload(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x0a, 0x01, key, 0x12, 0x02, 0x08, 0x01];
        let region = IndexedByteRegion::from_borrowed(&bytes, source, offset);
        let mut frames = vec![Frame::Map(MapFrame::from_map_region(
            &NEW_RHO_MATCH_PROGRAM,
            4,
            PAR_PROTOBUF_REGION_PROGRAM.type_id,
            region,
        ))];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let FrameStep::Other {
            action: FrameAction::Continue,
            map_source: Some(MapStepSource::Field(_)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("the first map field must store its key");
        };
        let before = DriverStepSource::before(&frames, &scores);
        let FrameStep::Other {
            action: FrameAction::Push(request),
            map_source: Some(MapStepSource::Field(field)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("the second map field must select a Par child");
        };
        let mut map_source = MapStepSource::Field(field);
        let (pushed, pushed_request) =
            push_admission_frame_with_source(&mut frames, request, &mut stats).unwrap();
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &pushed_request,
            },
            map_source: Some(&map_source),
            accepted_children: None,
        };
        prop_assert!(emit_checked_admission_event(event).is_ok());
        let shadow = [0x08, 0x01];
        let FrameRequest::Message { region: selected, program } = &pushed_request else {
            panic!("map value did not push a Message child")
        };
        let alternate = IndexedByteRegion::from_borrowed(&shadow, source, offset + 5);
        prop_assert!(!selected.same_view(&alternate));
        let forged_request = FrameRequest::Message {
            region: alternate,
            program: *program,
        };
        prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &forged_request,
            },
            map_source: Some(&map_source),
            accepted_children: None,
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "forged map child view must be rejected");
        let MapStepSource::Field(field) = &map_source else { unreachable!() };
        prop_assert_eq!(before.map_cursor, Some(field.cursor_before));
        prop_assert_eq!(field.payload_len, 2);
        prop_assert_eq!(field.field_position, CanonicalByteProvenance::new(source, offset + 3));
        prop_assert_eq!(field.payload_position, CanonicalByteProvenance::new(source, offset + 5));

        let MapStepSource::Field(field) = &mut map_source else { unreachable!() };
        field.payload_len = 3;
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &pushed_request,
            },
            map_source: Some(&map_source),
            accepted_children: None,
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));
    }

    #[test]
    fn checked_map_eof_event_uses_the_live_cursor(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x0a, 0x01, key];
        let region = IndexedByteRegion::from_borrowed(&bytes, source, offset);
        let mut frames = vec![Frame::Map(MapFrame::from_map_region(
            &NEW_RHO_MATCH_PROGRAM,
            4,
            PAR_PROTOBUF_REGION_PROGRAM.type_id,
            region,
        ))];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let FrameStep::Other {
            action: FrameAction::Continue,
            map_source: Some(MapStepSource::Field(_)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("the map key must be admitted before EOF");
        };
        let Frame::Map(frame) = frames.last_mut().unwrap() else { unreachable!() };
        frame.waiting_value = true;
        prop_assert!(DriverStepSource::before(&frames, &scores).map_eof_before.is_none());
        let Frame::Map(frame) = frames.last_mut().unwrap() else { unreachable!() };
        frame.waiting_value = false;
        let mut before = DriverStepSource::before(&frames, &scores);
        let FrameStep::Other {
            action: FrameAction::Complete(summary),
            map_source: Some(MapStepSource::Eof(eof)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("the map EOF must retain its checked source");
        };
        prop_assert_eq!(before.map_cursor, Some(eof.cursor_after));
        prop_assert!(before.map_eof_before.as_ref().is_some_and(|source| source.matches_source(&eof)));
        let key_position = eof.summary.key_position;
        let mut map_source = MapStepSource::Eof(eof);
        let (outcome, mut completion_source, accept_effect) = complete_admission_step_with_source(
            &before, &mut frames, summary, None, &scores, &mut stats, None,
        ).unwrap();
        prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::Root(_)));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        };
        prop_assert!(emit_checked_admission_event(event).is_ok());
        completion_source.score_len_before += 1;
        prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "mutated map completion score count must be rejected");
        completion_source.score_len_before -= 1;

        before.map_cursor.as_mut().unwrap().bytes_read += 1;
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));

        before.map_cursor.as_mut().unwrap().bytes_read -= 1;
        let MapStepSource::Eof(eof) = &mut map_source else { unreachable!() };
        eof.summary.key_position = CanonicalByteProvenance::new(source, offset + 1);
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));

        let MapStepSource::Eof(eof) = &mut map_source else { unreachable!() };
        eof.summary.key_position = key_position;
        eof.outer_field += 1;
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));
    }

    #[test]
    fn checked_map_eof_prestate_excludes_a_cached_cursor_error(
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x80];
        let region = IndexedByteRegion::from_borrowed(&bytes, source, offset);
        let mut frames = vec![Frame::Map(MapFrame::from_map_region(
            &NEW_RHO_MATCH_PROGRAM,
            4,
            PAR_PROTOBUF_REGION_PROGRAM.type_id,
            region,
        ))];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        prop_assert!(frames.last_mut().unwrap().step(&mut stats, &mut scores).is_err());
        let Frame::Map(frame) = frames.last().unwrap() else { unreachable!() };
        prop_assert!(frame.cursor.is_empty());
        prop_assert!(!frame.cursor.is_terminal_error_free());
        prop_assert!(DriverStepSource::before(&frames, &scores).map_eof_before.is_none());
    }

    #[test]
    fn admitted_map_entries_follow_strict_source_key_order(
        first in 0x21u8..0x7f,
        second in 0x21u8..0x7f,
    ) {
        prop_assume!(first < second);
        let entry = |key| [0x22, 0x03, 0x0a, 0x01, key];
        let mut ordered = entry(first).to_vec();
        ordered.extend_from_slice(&entry(second));
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&ordered),
            &NEW_RHO_MATCH_PROGRAM,
        ).is_ok());

        let mut reversed = entry(second).to_vec();
        reversed.extend_from_slice(&entry(first));
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&reversed),
            &NEW_RHO_MATCH_PROGRAM,
        ).is_err());

        let mut duplicate = entry(first).to_vec();
        duplicate.extend_from_slice(&entry(first));
        prop_assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&duplicate),
            &NEW_RHO_MATCH_PROGRAM,
        ).is_err());
    }

    #[test]
    fn accepted_map_eof_event_matches_the_parent_return_receipt(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x22, 0x03, 0x0a, 0x01, key];
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
        let FrameStep::MessagePush { request, .. } =
            frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("the New injection must push its Map entry");
        };
        push_admission_frame(&mut frames, request, &mut stats).unwrap();
        let FrameStep::Other {
            action: FrameAction::Continue,
            map_source: Some(MapStepSource::Field(_)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("the Map entry key must continue");
        };
        let before = DriverStepSource::before(&frames, &scores);
        let FrameStep::Other {
            action: FrameAction::Complete(summary),
            map_source: Some(MapStepSource::Eof(eof)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("the Map entry must complete with its EOF receipt");
        };
        let mut map_source = MapStepSource::Eof(eof);
        let (outcome, completion_source, accept_effect) = complete_admission_step_with_source(
            &before, &mut frames, summary, None, &scores, &mut stats, None,
        ).unwrap();
        prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::AcceptedMessage(_)));
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        };
        prop_assert!(emit_checked_admission_event(event).is_ok());
        prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: None,
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "missing map completion effect must be rejected");

        let MapStepSource::Eof(eof) = &mut map_source else { unreachable!() };
        eof.summary.key_position = CanonicalByteProvenance::new(source, offset + 3);
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: Some(&map_source),
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));
    }

    #[test]
    fn fragmented_map_value_push_keeps_the_selected_physical_anchor(
        key in 0x21u8..0x7f,
        source in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x0a, 0x01, key, 0x12, 0x02, 0x08, 0x01];
        let anchors = (0..bytes.len())
            .map(|at| CanonicalByteProvenance::new(source + at as u32, offset + at * 17))
            .collect::<Vec<_>>();
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in bytes.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let end = CanonicalByteProvenance::new(source + 100, offset + 10_000);
        let region = IndexedByteRegion::from_frozen(arena.freeze(handle), end);
        let mut frames = vec![Frame::Map(MapFrame::from_map_region(
            &NEW_RHO_MATCH_PROGRAM,
            4,
            PAR_PROTOBUF_REGION_PROGRAM.type_id,
            region,
        ))];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let FrameStep::Other {
            action: FrameAction::Continue,
            map_source: Some(MapStepSource::Field(_)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("fragmented map key did not continue");
        };
        let before = DriverStepSource::before(&frames, &scores);
        let FrameStep::Other {
            action: FrameAction::Push(request),
            map_source: Some(MapStepSource::Field(field)),
        } = frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap() else {
            panic!("fragmented map value did not select a child");
        };
        let mut map_source = MapStepSource::Field(field);
        let (pushed, pushed_request) =
            push_admission_frame_with_source(&mut frames, request, &mut stats).unwrap();
        let event = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &pushed_request,
            },
            map_source: Some(&map_source),
            accepted_children: None,
        };
        prop_assert!(emit_checked_admission_event(event).is_ok());
        let MapStepSource::Field(field) = &map_source else { unreachable!() };
        prop_assert_eq!(field.field_position, anchors[3]);
        prop_assert_eq!(field.payload_position, anchors[5]);
        let IndexedCanonicalFieldValue::LengthDelimited(payload) = &field.field.value else {
            unreachable!()
        };
        prop_assert_eq!(payload.end_position().physical, end);

        let MapStepSource::Field(field) = &mut map_source else { unreachable!() };
        field.payload_position = anchors[4];
        let changed = CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &pushed_request,
            },
            map_source: Some(&map_source),
            accepted_children: None,
        };
        prop_assert!(matches!(
            emit_checked_admission_event(changed),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));
    }
}
