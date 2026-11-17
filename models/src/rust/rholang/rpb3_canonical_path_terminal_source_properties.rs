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
            .expect("bounded terminal fixture fits the arena");
    }
    OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), end)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn canonical_path_terminal_source_binds_once_to_complete_and_root_return(
        payload in prop::collection::vec(any::<u8>(), 0..64),
        source in 100u32..1000,
        read_before in 0usize..10_000,
        traversed_before in 0usize..10_000,
    ) {
        let mut bytes = vec![6, payload.len() as u8];
        bytes.extend_from_slice(&payload);
        let anchors = (0..bytes.len())
            .map(|index| CanonicalByteProvenance::new(source + index as u32 % 17, 29 + index * 37))
            .collect::<Vec<_>>();
        let end = CanonicalByteProvenance::new(source + 100, 70_000);
        let input = frozen(&bytes, &anchors, end);
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            canonical_path_bytes_read: read_before,
            canonical_path_bytes_traversed: traversed_before,
            ..Default::default()
        };
        let mut frames = vec![Frame::CanonicalPath(CanonicalPathFrame::new(input.clone()))];
        let mut driver = DriverStepSource::before(&frames, &scores);
        let action = frames[0].step(&mut stats, &mut scores).unwrap();
        let FrameStep::Other {
            action: FrameAction::Complete(summary),
            map_source: None,
        } = action else {
            panic!("generated terminal key did not complete")
        };
        let FrameSummary::CanonicalPath(value) = &summary else { unreachable!() };
        let value = *value;
        let Frame::CanonicalPath(frame) = &frames[0] else { unreachable!() };
        let source_image = frame.terminal_source.as_ref().expect("successful terminal has source");
        assert!(source_image.input.same_view(&input));
        assert_eq!(source_image.end, end);
        assert_eq!(source_image.summary, value);
        assert_eq!(source_image.bytes_yielded, bytes.len());
        assert_eq!(source_image.bytes_traversed, bytes.len());
        assert_eq!(source_image.bytes_read_before, read_before);
        assert_eq!(source_image.bytes_traversed_before, traversed_before);
        assert_eq!(stats.canonical_path_bytes_read, read_before + bytes.len());
        assert_eq!(stats.canonical_path_bytes_traversed, traversed_before + bytes.len());
        assert!(source_image.matches_complete(frame, &summary, &stats));
        let root_return = DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats)
            .expect("terminal source binds before root pop");
        assert!(root_return.matches_root_return(&[], &summary, &scores, &stats));
        stats.canonical_path_bytes_read += 1;
        assert!(!root_return.matches_root_return(&[], &summary, &scores, &stats));
        stats.canonical_path_bytes_read -= 1;

        let changed = FrameSummary::CanonicalPath(CanonicalPathSummary {
            form: value.form,
            canonical_path_stable: !value.canonical_path_stable,
        });
        assert!(!source_image.matches_complete(frame, &changed, &stats));
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &changed, &scores, &stats).is_none());
        let altered_input = frozen(&bytes, &anchors, end);
        assert_eq!(altered_input.bytes().collect::<Vec<_>>(), bytes);
        for index in 0..bytes.len() {
            assert_eq!(altered_input.position_at(index), input.position_at(index));
        }
        assert_eq!(altered_input.end_position(), input.end_position());
        assert!(!altered_input.same_view(&input));
        let Frame::CanonicalPath(frame) = &mut frames[0] else { unreachable!() };
        frame.input = altered_input;
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats).is_none());
        let Frame::CanonicalPath(frame) = &mut frames[0] else { unreachable!() };
        frame.input = input;
        let (returned, mut completion_source, accept_effect) = complete_admission_step_with_source(
            &driver, &mut frames, summary, None, &scores, &mut stats, None,
        ).unwrap();
        assert!(accept_effect.is_none());
        assert!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &returned,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&returned, &frames),
        }).is_ok());
        let Some(terminal) = completion_source.terminal.as_deref_mut() else {
            panic!("CanonicalPath return must retain its terminal source")
        };
        let CompleteTerminalSource::CanonicalPath { end: retained_end, .. } = terminal else {
            panic!("CanonicalPath return retained the wrong terminal kind")
        };
        *retained_end = CanonicalByteProvenance::new(source + 101, 70_000);
        assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &returned,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&returned, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))));
        let Some(CompleteTerminalSource::CanonicalPath { end: retained_end, .. }) =
            completion_source.terminal.as_deref_mut() else { unreachable!() };
        *retained_end = end;
        let Some(CompleteTerminalSource::CanonicalPath { returned: retained_summary, .. }) =
            completion_source.terminal.as_deref_mut() else { unreachable!() };
        retained_summary.canonical_path_stable = !value.canonical_path_stable;
        assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &returned,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&returned, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))));
        let Some(CompleteTerminalSource::CanonicalPath { returned: retained_summary, .. }) =
            completion_source.terminal.as_deref_mut() else { unreachable!() };
        *retained_summary = value;
        driver.top_program = Some(&PAR_PROTOBUF_REGION_PROGRAM);
        assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &returned,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&returned, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))));
        driver.top_program = None;
        completion_source.completed_kind = FrameRequestKind::EPathMap;
        assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &returned,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&returned, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))));
        completion_source.completed_kind = FrameRequestKind::CanonicalPath;
        stats.canonical_path_bytes_read += 1;
        assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &returned,
                source: &completion_source,
                effect: accept_effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&returned, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))));
        assert!(matches!(returned, CompleteAdmissionOutcome::Root(FrameSummary::CanonicalPath(actual)) if actual == value));
        assert!(frames.is_empty());
    }

    #[test]
    fn canonical_path_terminal_source_is_absent_on_stat_overflow_or_path_error(
        payload in prop::collection::vec(any::<u8>(), 0..64),
        source in 100u32..1000,
        baseline in 0usize..10_000,
    ) {
        let mut bytes = vec![6, payload.len() as u8];
        bytes.extend_from_slice(&payload);
        let end = CanonicalByteProvenance::new(source + 1, 80_000);
        let input = IndexedByteRegion::from_borrowed(&bytes, source, 300);
        let mut read_overflow = CanonicalPathFrame::new(input.clone());
        let mut read_stats = CanonicalProtobufAdmissionStats {
            canonical_path_bytes_read: usize::MAX,
            canonical_path_bytes_traversed: baseline,
            ..Default::default()
        };
        assert!(matches!(read_overflow.step(&mut read_stats), Err(CanonicalProtobufAdmissionError::SizeOverflow)));
        assert!(read_overflow.terminal_source.is_none());
        assert_eq!(read_stats.canonical_path_bytes_read, usize::MAX);
        assert_eq!(read_stats.canonical_path_bytes_traversed, baseline);

        let mut traversed_overflow = CanonicalPathFrame::new(input);
        let mut traversed_stats = CanonicalProtobufAdmissionStats {
            canonical_path_bytes_read: baseline,
            canonical_path_bytes_traversed: usize::MAX,
            ..Default::default()
        };
        assert!(matches!(traversed_overflow.step(&mut traversed_stats), Err(CanonicalProtobufAdmissionError::SizeOverflow)));
        assert!(traversed_overflow.terminal_source.is_none());
        assert_eq!(traversed_stats.canonical_path_bytes_read, baseline + bytes.len());
        assert_eq!(traversed_stats.canonical_path_bytes_traversed, usize::MAX);

        let mut empty = CanonicalPathFrame::new(IndexedByteRegion::from_source(SourceTreeWindow::empty(), end));
        let mut empty_stats = CanonicalProtobufAdmissionStats::default();
        let error = match empty.step(&mut empty_stats) {
            Ok(_) => panic!("empty path cannot produce a summary"),
            Err(error) => error,
        };
        assert!(matches!(error, CanonicalProtobufAdmissionError::CanonicalPath { .. }));
        assert!(empty.terminal_source.is_none());
        assert_eq!(empty_stats.canonical_path_bytes_read, 0);
        assert_eq!(empty_stats.canonical_path_bytes_traversed, 0);
    }
}
