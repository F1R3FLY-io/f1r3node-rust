use proptest::prelude::*;

use super::rpb3_root_finish_trace::{ActiveFrameState, DriverStepKind, NonMessageFrameState};
use super::*;
use crate::rust::canonical_path::CanonicalTopLevelPathForm;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;

fn assert_root_complete(
    region: IndexedByteRegion<'_>,
    bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
    form: CanonicalTopLevelPathForm,
) {
    assert!(!bytes.is_empty());
    assert_eq!(anchors.len(), bytes.len());
    assert_eq!(region.bytes().collect::<Vec<_>>(), bytes);
    for (index, anchor) in anchors.iter().enumerate() {
        assert_eq!(region.position_at(index).unwrap().physical, *anchor);
    }
    assert_eq!(region.end_position().physical, end);

    rpb3_root_finish_trace::start_with_active_frames();
    let (summary, scores, stats) =
        drive_admission(FrameRequest::CanonicalPath(region), bytes.len()).unwrap();
    let trace = rpb3_root_finish_trace::take();
    let FrameSummary::CanonicalPath(path) = summary else {
        panic!("standalone path returned a different summary kind")
    };
    assert_eq!(path.form, form);
    assert!(path.canonical_path_stable);
    assert_eq!(scores.source_len(), 0);
    assert_eq!(trace.score_arena, Some(Vec::new()));
    assert_eq!(trace.remaining_frames, Some(0));
    assert!(trace.returned.is_none());
    assert_eq!(trace.driver_steps.len(), 1);
    let complete = &trace.driver_steps[0];
    assert!(matches!(complete.kind, DriverStepKind::Complete));
    assert_eq!(complete.before_len, 1);
    assert_eq!(complete.after_len, 0);
    assert_eq!(complete.before_capacity, complete.after_capacity);
    assert_eq!(complete.before_top_kind, FrameRequestKind::CanonicalPath);
    assert_eq!(complete.after_top_kind, None);
    assert_eq!(complete.score_len_before, 0);
    assert_eq!(complete.score_len_after, 0);
    assert!(complete.score_appended.is_empty());
    assert_eq!(complete.active_frames_before.as_ref().unwrap().len(), 1);
    assert!(complete.active_frames_after.as_ref().unwrap().is_empty());
    let ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::CanonicalPath(active),
        bytes: active_bytes,
        anchors: active_anchors,
    } = &complete.active_frames_before.as_ref().unwrap()[0]
    else {
        panic!("root was not an indexed CanonicalPath frame")
    };
    assert_eq!(active_bytes, bytes);
    assert_eq!(active_anchors, anchors);
    assert_eq!(active.input, bytes);
    assert_eq!(active.region_start, anchors[0]);
    assert_eq!(active.region_end, end);
    assert_eq!(active.bytes_yielded, 0);
    assert_eq!(active.bytes_traversed, 0);
    assert!(active.waiting.is_none());
    assert!(active.active_obligation.is_none());

    assert_eq!(trace.canonical_path_bytes.len(), bytes.len());
    for (index, observed) in trace.canonical_path_bytes.iter().enumerate() {
        assert_eq!(observed.step_index, 0);
        assert_eq!(observed.index, index);
        assert_eq!(observed.byte, bytes[index]);
        assert_eq!(observed.source, anchors[index]);
        assert_eq!(observed.skipped, 0);
        assert_eq!(observed.yielded_after, index + 1);
        assert_eq!(observed.traversed_after, index + 1);
    }
    assert!(trace.nonmessage_push_bodies.is_empty());
    assert_eq!(trace.pop_accepts.len(), 1);
    let pop = &trace.pop_accepts[0];
    assert_eq!(pop.child_kind, FrameRequestKind::CanonicalPath);
    assert_eq!(pop.before_len, 1);
    assert_eq!(pop.after_len, Some(0));
    assert_eq!(pop.parent_kind, None);
    assert_eq!(pop.source_len_before, 0);
    assert_eq!(pop.source_len_after, Some(0));
    let Some(NonMessageFrameState::CanonicalPath(eof)) = pop.child_nonmessage.as_ref() else {
        panic!("pre-pop root was not a CanonicalPath frame")
    };
    assert_eq!(eof.input, bytes);
    assert_eq!(eof.region_start, anchors[0]);
    assert_eq!(eof.region_end, end);
    assert_eq!(eof.bytes_yielded, bytes.len());
    assert_eq!(eof.bytes_traversed, bytes.len());
    assert!(eof.waiting.is_none());
    assert!(eof.active_obligation.is_none());
    assert_eq!(stats.input_bytes, bytes.len());
    assert_eq!(stats.canonical_path_bytes_read, bytes.len());
    assert_eq!(stats.canonical_path_bytes_traversed, bytes.len());
    assert_eq!(stats.canonical_path_obligations, 0);
    assert_eq!(stats.canonical_path_resolutions, 0);
    assert_eq!(stats.max_frames, 1);
    assert_eq!(stats.max_frame_storage_bytes, stats.frame_variant_bytes);
}

fn assert_empty_root_rejected(region: IndexedByteRegion<'_>, end: CanonicalByteProvenance) {
    assert_eq!(region.position_at(0).unwrap().physical, end);
    rpb3_root_finish_trace::start_with_active_frames();
    let error = match drive_admission(FrameRequest::CanonicalPath(region), 0) {
        Ok(_) => panic!("empty root unexpectedly completed"),
        Err(error) => error,
    };
    let trace = rpb3_root_finish_trace::take();
    let CanonicalProtobufAdmissionError::CanonicalPath {
        position, detail, ..
    } = error
    else {
        panic!("empty root returned a non-path error")
    };
    assert_eq!(
        position,
        CanonicalPathSource::in_source(end.source, end.offset)
    );
    assert!(detail.contains("EmptyPath"));
    assert!(trace.driver_steps.is_empty());
    assert!(trace.pop_accepts.is_empty());
    assert!(trace.canonical_path_bytes.is_empty());
    assert!(trace.score_arena.is_none());
    assert!(trace.remaining_frames.is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn borrowed_root_completes_from_indexed_bytes(
        payload in prop::collection::vec(any::<u8>(), 0..64),
        split in any::<bool>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (bytes, form) = if split {
            let mut bytes = payload.iter().map(|byte| if byte & 1 == 0 { 1 } else { 2 }).collect::<Vec<_>>();
            bytes.push(0);
            (bytes, CanonicalTopLevelPathForm::Split)
        } else {
            let mut bytes = vec![6, payload.len() as u8];
            bytes.extend_from_slice(&payload);
            (bytes, CanonicalTopLevelPathForm::Bare)
        };
        let anchors = (0..bytes.len())
            .map(|index| CanonicalByteProvenance::new(source, base + index))
            .collect::<Vec<_>>();
        let end = CanonicalByteProvenance::new(source, base + bytes.len());
        assert_root_complete(IndexedByteRegion::from_borrowed(&bytes, source, base), &bytes, &anchors, end, form);
    }

    #[test]
    fn fragmented_root_completes_with_physical_anchors(
        payload in prop::collection::vec(any::<u8>(), 0..64),
        split in any::<bool>(),
        source_seeds in prop::collection::vec((1u32..1000, 0usize..100_000), 65),
        end_source in 2000u32..3000,
        end_offset in 0usize..100_000,
    ) {
        let (bytes, form) = if split {
            let mut bytes = payload.iter().map(|byte| if byte & 1 == 0 { 1 } else { 2 }).collect::<Vec<_>>();
            bytes.push(0);
            (bytes, CanonicalTopLevelPathForm::Split)
        } else {
            let mut bytes = vec![6, payload.len() as u8];
            bytes.extend_from_slice(&payload);
            (bytes, CanonicalTopLevelPathForm::Bare)
        };
        let anchors = source_seeds.iter().take(bytes.len()).enumerate()
            .map(|(index, (source, offset))| CanonicalByteProvenance::new(*source * 2 + (index % 2) as u32, *offset))
            .collect::<Vec<_>>();
        let end = CanonicalByteProvenance::new(end_source, end_offset);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in bytes.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        assert_root_complete(IndexedByteRegion::from_frozen(arena.freeze(handle), end), &bytes, &anchors, end, form);
    }

    #[test]
    fn empty_root_rejects_at_indexed_terminal_anchor(
        source in 1u32..1000,
        offset in 0usize..100_000,
    ) {
        let end = CanonicalByteProvenance::new(source, offset);
        assert_empty_root_rejected(IndexedByteRegion::from_source(SourceTreeWindow::empty(), end), end);
        let arena = CanonicalBytesArena::new();
        assert_empty_root_rejected(IndexedByteRegion::from_frozen(arena.freeze(arena.empty()), end), end);
    }
}
