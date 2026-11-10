use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::tag;
use crate::rust::rholang::protobuf_encoder;

#[test]
fn canonical_path_step_rejects_divergent_cursor_and_reducer_prefixes() {
    let bytes = [tag::GBYTE_ARRAY, 1, 7];
    let region = IndexedByteRegion::from_borrowed(&bytes, 23, 101);
    let mut cursor_ahead = CanonicalPathFrame::new(region.clone());
    assert!(cursor_ahead.cursor.next().is_some());
    let mut stats = CanonicalProtobufAdmissionStats::default();
    assert!(matches!(
        cursor_ahead.step(&mut stats),
        Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "canonical path cursor and reducer consumed different input prefixes"
        ))
    ));

    let mut reducer_ahead = CanonicalPathFrame::new(region);
    let first = reducer_ahead.input.located_bytes().next().unwrap();
    reducer_ahead
        .reducer
        .edge(&reducer_ahead.input, first)
        .unwrap();
    assert!(matches!(
        reducer_ahead.step(&mut stats),
        Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "canonical path cursor and reducer consumed different input prefixes"
        ))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn canonical_path_return_requires_the_indexed_child_selected_at_push(
        payload in prop::collection::vec(any::<u8>(), 0..96),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let raw = Par::default()
            .with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GByteArray(payload)),
            }])
            .with_connective_used(true);
        let body = protobuf_encoder::encode_to_vec(&raw);
        let shadow = body.clone();
        let mut key = vec![tag::ESCAPE];
        prost::encoding::encode_varint(body.len() as u64, &mut key);
        let body_start = key.len();
        key.extend_from_slice(&body);
        let root = IndexedByteRegion::from_borrowed(&key, source, base);
        let mut parent = CanonicalPathFrame::new(root);
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..Default::default()
        };
        let FrameAction::Push(request) = parent.step(&mut stats).unwrap() else {
            panic!("generated raw-Par escape did not push a child")
        };
        let FrameRequest::Message { region: child_region, .. } = &request else {
            panic!("raw-Par escape did not select a Message child")
        };
        let child_view = child_region.clone();
        assert_eq!(child_view.bytes().collect::<Vec<_>>(), body);
        let child_end = CanonicalByteProvenance::new(source, base + key.len() - 1);
        assert_eq!(child_view.end_position().physical, child_end);
        assert!(parent.waiting_child.as_ref().unwrap().same_view(&child_view));
        assert!(parent.matches_push_request(&request));

        let alternate = IndexedByteRegion::from_borrowed(
            &shadow, source, base + body_start,
        ).subview_with_end(
            CanonicalByteRange { start: 0, end: body.len() }, child_end,
        ).unwrap();
        assert_eq!(alternate.bytes().collect::<Vec<_>>(), body);
        assert!(!alternate.same_view(&child_view));
        assert!(!parent.matches_push_request(&FrameRequest::Message {
            region: alternate.clone(),
            program: &PAR_PROTOBUF_REGION_PROGRAM,
        }));
        assert!(!parent.matches_push_request(&FrameRequest::Message {
            region: child_view.clone(),
            program: &NEW_RHO_MATCH_PROGRAM,
        }));
        assert!(!parent.matches_push_request(&FrameRequest::EPathMap {
            region: child_view.clone(),
            owner: &PAR_PROTOBUF_REGION_PROGRAM,
            field: 0,
        }));

        let (summary, scores, _) = drive_admission(request.clone(), body.len()).unwrap();
        let mut frames = Vec::with_capacity(2);
        frames.push(Frame::CanonicalPath(parent));
        let push_scores = CanonicalScoreArena::new();
        let push_before = DriverStepSource::before(&frames, &push_scores);
        let (pushed, pushed_request) =
            push_admission_frame_with_source(&mut frames, request, &mut stats).unwrap();
        assert!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &push_before,
            after_frames: &frames,
            after_scores: &push_scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &pushed_request,
            },
            map_source: None,
        }).is_ok());
        let wrong_request = FrameRequest::Message {
            region: alternate.clone(),
            program: &PAR_PROTOBUF_REGION_PROGRAM,
        };
        assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &push_before,
            after_frames: &frames,
            after_scores: &push_scores,
            action: CheckedAdmissionAction::OtherPush {
                pushed: &pushed,
                request: &wrong_request,
            },
            map_source: None,
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))));
        let driver = DriverStepSource::before(&frames, &scores);
        let source_image = DriverCompleteSource::before_pop(
            &driver, &frames, &summary, &scores, &stats,
        ).expect("real pushed child matches retained indexed view");

        let Frame::Message(child) = &mut frames[1] else {
            panic!("pushed raw-Par child changed kind")
        };
        let original = child.body.clone();
        assert_eq!(alternate.bytes().collect::<Vec<_>>(), body);
        for index in 0..body.len() {
            assert_eq!(alternate.position_at(index), original.position_at(index));
        }
        assert_eq!(alternate.end_position(), original.end_position());
        assert!(!alternate.same_view(&original));
        child.body = alternate;
        assert!(DriverCompleteSource::before_pop(
            &driver, &frames, &summary, &scores, &stats,
        ).is_none());

        let Frame::Message(child) = &mut frames[1] else { unreachable!() };
        child.body = original.clone();
        let altered_end = CanonicalByteProvenance::new(source, child_end.offset + 1);
        child.body = original.subview_with_end(
            CanonicalByteRange { start: 0, end: body.len() }, altered_end,
        ).unwrap();
        assert_eq!(child.body.bytes().collect::<Vec<_>>(), body);
        assert_ne!(child.body.end_position().physical, child_end);
        assert!(DriverCompleteSource::before_pop(
            &driver, &frames, &summary, &scores, &stats,
        ).is_none());

        let Frame::Message(child) = &mut frames[1] else { unreachable!() };
        child.body = child_view.clone();
        assert!(DriverCompleteSource::before_pop(
            &driver, &frames, &summary, &scores, &stats,
        ).is_some());
        frames.truncate(1);
        assert!(source_image.matches_popped(&frames, &scores, &stats));
        let effect = frames[0].accept(summary, &mut stats, &scores).unwrap();
        assert!(source_image.matches_accepted(&frames, &scores, &stats, Some(&effect)));
        let Frame::CanonicalPath(parent) = &frames[0] else { unreachable!() };
        assert!(parent.waiting.is_none());
        assert!(parent.waiting_child.is_none());
    }
}
