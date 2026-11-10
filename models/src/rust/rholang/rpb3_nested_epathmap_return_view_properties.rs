use proptest::prelude::*;

use super::rpb3_root_finish_trace::{ActiveFrameState, DriverStepKind};
use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::tag;
use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::protobuf_encoder;

fn indexed(
    bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
) -> OwnedIndexedByteRegion {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (&byte, &anchor) in bytes.iter().zip(anchors) {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, anchor))
            .expect("bounded nested return fits the arena");
    }
    OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), end)
}

fn payload(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut key = vec![tag];
    prost::encoding::encode_varint(body.len() as u64, &mut key);
    key.extend_from_slice(body);
    key
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn nested_epathmap_return_requires_its_selected_indexed_map_value(
        key in any::<i16>(),
        payload in prop::collection::vec(any::<u8>(), 1..32),
        source in 100u32..1000,
    ) {
        let value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(payload)),
        }]);
        let value_bytes = protobuf_encoder::encode_to_vec(&value);
        let key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(key))),
        }]);
        let map = EPathMap::new_map([(key_par, value)], Vec::new(), false, None);
        let snapshot = map.trie_snapshot().to_vec();
        let mut root_bytes = vec![tag::EPATHMAP_MAP];
        prost::encoding::encode_varint(snapshot.len() as u64, &mut root_bytes);
        let snapshot_start = root_bytes.len();
        root_bytes.extend_from_slice(&snapshot);
        let anchors: Vec<_> = (0..root_bytes.len())
            .map(|index| CanonicalByteProvenance::new(source + index as u32 % 19, 57 + index * 23))
            .collect();
        let end = CanonicalByteProvenance::new(source + 30, 800_000);
        let root = indexed(&root_bytes, &anchors, end);

        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..Default::default()
        };
        let mut path = CanonicalPathFrame::new(root.clone());
        let FrameAction::Push(map_request) = path.step(&mut stats).unwrap() else {
            panic!("CanonicalPath root did not push EPathMap")
        };
        assert_eq!(map_request.request_kind(), FrameRequestKind::EPathMap);
        assert_eq!(map_request.region().bytes().collect::<Vec<_>>(), snapshot);
        assert_eq!(map_request.region().position_at(0).unwrap().physical, anchors[snapshot_start]);
        assert_eq!(map_request.region().end_position().physical, anchors[root_bytes.len() - 1]);
        let mut frames = Vec::with_capacity(3);
        frames.push(Frame::CanonicalPath(path));
        push_admission_frame(&mut frames, map_request, &mut stats).unwrap();
        let Frame::EPathMap(parent) = &mut frames[1] else { unreachable!() };
        let FrameAction::Push(child_request) = epathmap_step(parent, &mut stats).unwrap() else {
            panic!("generated EPathMap did not push its map value")
        };
        assert_eq!(child_request.request_kind(), FrameRequestKind::Message);
        let selected = child_request.region().clone();
        assert_eq!(selected.bytes().collect::<Vec<_>>(), value_bytes);
        let value_starts: Vec<_> = root_bytes.windows(value_bytes.len())
            .enumerate()
            .filter_map(|(index, window)| (window == value_bytes).then_some(index))
            .collect();
        assert_eq!(value_starts.len(), 1);
        let value_start = value_starts[0];
        for index in 0..value_bytes.len() {
            assert_eq!(selected.position_at(index).unwrap().physical, anchors[value_start + index]);
        }
        let expected_value_end = if value_start + value_bytes.len() == root_bytes.len() {
            anchors[root_bytes.len() - 1]
        } else {
            anchors[value_start + value_bytes.len()]
        };
        assert_eq!(selected.end_position().physical, expected_value_end);
        let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
        assert!(parent.waiting_child.as_ref().unwrap().same_view(&selected));
        let (summary, scores, _) = drive_admission(child_request.clone(), value_bytes.len()).unwrap();
        push_admission_frame(&mut frames, child_request, &mut stats).unwrap();
        let driver = DriverStepSource::before(&frames, &scores);
        let source_image = DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats)
            .expect("the selected nested child is accepted before pop");

        let expected_anchors = (0..selected.len())
            .map(|index| selected.position_at(index).unwrap().physical)
            .collect::<Vec<_>>();
        let shadow = indexed(&value_bytes, &expected_anchors, selected.end_position().physical);
        assert_eq!(shadow.bytes().collect::<Vec<_>>(), value_bytes);
        for index in 0..selected.len() {
            assert_eq!(shadow.position_at(index), selected.position_at(index));
        }
        assert_eq!(shadow.end_position(), selected.end_position());
        assert!(!shadow.same_view(&selected));
        let Frame::Message(child) = &mut frames[2] else { unreachable!() };
        child.body = shadow;
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats).is_none());

        let altered_end = CanonicalByteProvenance::new(source + 31, 900_000);
        let Frame::Message(child) = &mut frames[2] else { unreachable!() };
        child.body = selected.subview_with_end(
            CanonicalByteRange { start: 0, end: selected.len() }, altered_end,
        ).unwrap();
        assert_eq!(child.body.bytes().collect::<Vec<_>>(), value_bytes);
        assert_ne!(child.body.end_position().physical, selected.end_position().physical);
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats).is_none());

        let Frame::Message(child) = &mut frames[2] else { unreachable!() };
        child.body = selected.clone();
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats).is_some());
        frames.truncate(2);
        assert!(source_image.matches_popped(&frames, &scores, &stats));
        let effect = frames[1].accept(summary, &mut stats, &scores).unwrap();
        assert!(source_image.matches_accepted(&frames, &scores, &stats, Some(&effect)));
        let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
        assert!(parent.waiting.is_none());
        assert!(parent.waiting_child.is_none());
        let Frame::CanonicalPath(root_parent) = &frames[0] else { unreachable!() };
        assert!(root_parent.waiting.is_some());
        assert!(root_parent.waiting_child.is_some());

        rpb3_root_finish_trace::start_with_active_frames();
        let (root_summary, _, root_stats) =
            drive_admission(FrameRequest::CanonicalPath(root), root_bytes.len()).unwrap();
        let trace = rpb3_root_finish_trace::take();
        assert!(matches!(root_summary, FrameSummary::CanonicalPath(_)));
        assert_eq!(root_stats.canonical_path_obligations, 1);
        assert_eq!(root_stats.canonical_path_resolutions, 1);
        let push = trace.driver_steps.iter().find(|step| {
            matches!(step.kind, DriverStepKind::Push(_))
                && step.before_len == 2
                && step.after_len == 3
                && step.before_top_kind == FrameRequestKind::EPathMap
                && step.after_top_kind == Some(FrameRequestKind::Message)
        }).expect("full driver pushes EPathMap map value under CanonicalPath");
        let ActiveFrameState::Message { bytes, anchors: child_anchors, .. } =
            &push.active_frames_after.as_ref().unwrap()[2] else { unreachable!() };
        assert_eq!(bytes, &value_bytes);
        assert_eq!(child_anchors, &anchors[value_start..value_start + value_bytes.len()]);
        let return_to_map = trace.pop_accepts.iter().find(|pop| {
            pop.before_len == 3
                && pop.child_kind == FrameRequestKind::Message
                && pop.parent_kind == Some(FrameRequestKind::EPathMap)
        }).expect("map value returns to EPathMap");
        assert_eq!(return_to_map.after_len, Some(2));
        assert!(trace.pop_accepts.iter().any(|pop| {
            pop.before_len == 2
                && pop.child_kind == FrameRequestKind::EPathMap
                && pop.parent_kind == Some(FrameRequestKind::CanonicalPath)
                && pop.after_len == Some(1)
        }));
        assert_eq!(trace.remaining_frames, Some(0));
    }

    #[test]
    fn nested_epathmap_obligation_return_requires_its_selected_child_view(
        inner_key in any::<i16>(),
        outer_value in any::<i16>(),
        payload_bytes in prop::collection::vec(any::<u8>(), 1..24),
        source in 100u32..1000,
    ) {
        let inner_value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(payload_bytes)),
        }]);
        let inner_key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(inner_key))),
        }]);
        let inner = EPathMap::new_map([(inner_key_par, inner_value)], Vec::new(), false, None);
        let inner_snapshot = inner.trie_snapshot().to_vec();
        let nested_key = payload(tag::EPATHMAP_MAP, &inner_snapshot);
        let outer_value_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(outer_value))),
        }]);
        let mut trie = pathmap::PathMap::new();
        trie.insert(&nested_key, outer_value_par);
        let outer_snapshot = epathmap_trie_codec::encode(&EPathMapRepr::Map(trie));
        let root_bytes = payload(tag::EPATHMAP_MAP, &outer_snapshot);
        let anchors: Vec<_> = (0..root_bytes.len())
            .map(|index| CanonicalByteProvenance::new(source + index as u32 % 29, 97 + index * 31))
            .collect();
        let root_end = CanonicalByteProvenance::new(source + 40, 1_000_000);
        let root = indexed(&root_bytes, &anchors, root_end);
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..Default::default()
        };
        let mut root_frame = CanonicalPathFrame::new(root.clone());
        let FrameAction::Push(outer_request) = root_frame.step(&mut stats).unwrap() else {
            panic!("CanonicalPath did not push outer EPathMap")
        };
        assert_eq!(outer_request.request_kind(), FrameRequestKind::EPathMap);
        assert_eq!(outer_request.region().bytes().collect::<Vec<_>>(), outer_snapshot);
        let mut frames = Vec::with_capacity(3);
        frames.push(Frame::CanonicalPath(root_frame));
        push_admission_frame(&mut frames, outer_request, &mut stats).unwrap();
        let Frame::EPathMap(parent) = &mut frames[1] else { unreachable!() };
        let FrameAction::Push(inner_request) = epathmap_step(parent, &mut stats).unwrap() else {
            panic!("outer EPathMap did not push nested EPathMap")
        };
        assert_eq!(inner_request.request_kind(), FrameRequestKind::EPathMap);
        let selected = inner_request.region().clone();
        assert_eq!(selected.bytes().collect::<Vec<_>>(), inner_snapshot);
        let starts: Vec<_> = root_bytes.windows(inner_snapshot.len())
            .enumerate()
            .filter_map(|(index, window)| (window == inner_snapshot).then_some(index))
            .collect();
        assert_eq!(starts.len(), 1);
        let inner_start = starts[0];
        for index in 0..inner_snapshot.len() {
            assert_eq!(selected.position_at(index).unwrap().physical, anchors[inner_start + index]);
        }
        assert_eq!(selected.end_position().physical, anchors[inner_start + inner_snapshot.len() - 1]);
        let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
        let Some(EPathMapWait::Obligation(wait)) = parent.waiting.as_ref() else {
            panic!("outer EPathMap did not retain nested obligation")
        };
        assert_eq!(wait.kind, CanonicalPathObligationKind::NestedEpm1);
        assert!(parent.waiting_child.as_ref().unwrap().same_view(&selected));

        push_admission_frame(&mut frames, inner_request, &mut stats).unwrap();
        let Frame::EPathMap(inner_frame) = &mut frames[2] else { unreachable!() };
        let FrameAction::Push(inner_value_request) = epathmap_step(inner_frame, &mut stats).unwrap() else {
            panic!("inner EPathMap did not push its map value")
        };
        assert_eq!(inner_value_request.request_kind(), FrameRequestKind::Message);
        let (inner_value_summary, scores, _) =
            drive_admission(inner_value_request.clone(), inner_value_request.region().len()).unwrap();
        let Frame::EPathMap(inner_frame) = &mut frames[2] else { unreachable!() };
        assert!(matches!(inner_frame.accept(inner_value_summary, &mut stats).unwrap(),
            FrameAcceptEffect::EPathMapValue(_)));
        let Frame::EPathMap(inner_frame) = &mut frames[2] else { unreachable!() };
        let FrameAction::Complete(summary) = epathmap_step(inner_frame, &mut stats).unwrap() else {
            panic!("inner EPathMap did not complete after its map value")
        };
        let driver = DriverStepSource::before(&frames, &scores);
        let source_image = DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats)
            .expect("selected nested EPathMap child binds before pop");
        let selected_anchors = (0..selected.len())
            .map(|index| selected.position_at(index).unwrap().physical)
            .collect::<Vec<_>>();
        let shadow = indexed(&inner_snapshot, &selected_anchors, selected.end_position().physical);
        assert_eq!(shadow.bytes().collect::<Vec<_>>(), inner_snapshot);
        for index in 0..selected.len() {
            assert_eq!(shadow.position_at(index), selected.position_at(index));
        }
        assert_eq!(shadow.end_position(), selected.end_position());
        assert!(!shadow.same_view(&selected));
        let Frame::EPathMap(child) = &mut frames[2] else { unreachable!() };
        child.snapshot = shadow;
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats).is_none());
        let Frame::EPathMap(child) = &mut frames[2] else { unreachable!() };
        child.snapshot = selected.clone();
        assert!(DriverCompleteSource::before_pop(&driver, &frames, &summary, &scores, &stats).is_some());
        frames.truncate(2);
        assert!(source_image.matches_popped(&frames, &scores, &stats));
        let effect = frames[1].accept(summary, &mut stats, &scores).unwrap();
        assert!(source_image.matches_accepted(&frames, &scores, &stats, Some(&effect)));
        let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
        assert!(parent.waiting.is_none());
        assert!(parent.waiting_child.is_none());
        let Frame::CanonicalPath(root_parent) = &frames[0] else { unreachable!() };
        assert!(root_parent.waiting.is_some());

        rpb3_root_finish_trace::start_with_active_frames();
        let (root_summary, _, root_stats) =
            drive_admission(FrameRequest::CanonicalPath(root), root_bytes.len()).unwrap();
        let trace = rpb3_root_finish_trace::take();
        assert!(matches!(root_summary, FrameSummary::CanonicalPath(_)));
        assert_eq!(root_stats.canonical_path_obligations, 2);
        assert_eq!(root_stats.canonical_path_resolutions, 2);
        let nested_push = trace.driver_steps.iter().find(|step| {
            matches!(step.kind, DriverStepKind::Push(_))
                && step.before_len == 2
                && step.after_len == 3
                && step.before_top_kind == FrameRequestKind::EPathMap
                && step.after_top_kind == Some(FrameRequestKind::EPathMap)
        }).expect("full driver pushes nested EPathMap under CanonicalPath");
        let ActiveFrameState::NonMessage { bytes, anchors: child_anchors, .. } =
            &nested_push.active_frames_after.as_ref().unwrap()[2] else { unreachable!() };
        assert_eq!(bytes, &inner_snapshot);
        assert_eq!(child_anchors, &anchors[inner_start..inner_start + inner_snapshot.len()]);
        let return_to_outer = trace.pop_accepts.iter().find(|pop| {
            pop.before_len == 3
                && pop.child_kind == FrameRequestKind::EPathMap
                && pop.parent_kind == Some(FrameRequestKind::EPathMap)
        }).expect("nested EPathMap returns to outer EPathMap");
        assert_eq!(return_to_outer.after_len, Some(2));
        assert!(trace.pop_accepts.iter().any(|pop| {
            pop.before_len == 2
                && pop.child_kind == FrameRequestKind::EPathMap
                && pop.parent_kind == Some(FrameRequestKind::CanonicalPath)
                && pop.after_len == Some(1)
        }));
        assert_eq!(trace.remaining_frames, Some(0));
    }
}
