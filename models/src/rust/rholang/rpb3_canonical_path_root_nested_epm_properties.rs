use proptest::prelude::*;

use super::rpb3_root_finish_trace::{ActiveFrameState, DriverStepKind, NonMessageFrameState};
use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::{tag, CanonicalTopLevelPathForm};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::canonical_path_admission::{
    CanonicalPathTopologyAdmission, LocatedPathByte,
};

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn fixture(key_number: i16, payload: &[u8]) -> (Vec<u8>, Vec<u8>, usize) {
    let value = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(payload.to_vec())),
    }]);
    let map = EPathMap::new_map(
        [(integer(i64::from(key_number)), value)],
        Vec::new(),
        false,
        None,
    );
    let snapshot = map.trie_snapshot().to_vec();
    let mut key = vec![tag::EPATHMAP_MAP];
    prost::encoding::encode_varint(snapshot.len() as u64, &mut key);
    let body_start = key.len();
    key.extend_from_slice(&snapshot);
    (key, snapshot, body_start)
}

fn assert_nested_root_return(
    region: IndexedByteRegion<'_>,
    key: &[u8],
    snapshot: &[u8],
    body_start: usize,
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
) {
    assert_eq!(&key[body_start..], snapshot);
    assert!(!snapshot.is_empty());
    assert_eq!(region.bytes().collect::<Vec<_>>(), key);
    assert_eq!(region.end_position().physical, end);
    for (index, anchor) in anchors.iter().enumerate() {
        assert_eq!(region.position_at(index).unwrap().physical, *anchor);
    }

    let mut parent = CanonicalPathFrame::new(region.clone());
    let mut push_stats = CanonicalProtobufAdmissionStats {
        frame_variant_bytes: std::mem::size_of::<Frame>(),
        ..Default::default()
    };
    let FrameAction::Push(request) = parent.step(&mut push_stats).unwrap() else {
        panic!("nested EPM root did not emit its obligation request")
    };
    let FrameRequest::EPathMap {
        region: selected, ..
    } = &request
    else {
        panic!("nested EPM root did not select an EPathMap child")
    };
    assert_eq!(selected.bytes().collect::<Vec<_>>(), snapshot);
    assert!(parent.matches_push_request(&request));
    let selected_end = selected.end_position().physical;
    let altered = selected
        .subview_with_end(
            CanonicalByteRange {
                start: 0,
                end: selected.len(),
            },
            CanonicalByteProvenance::new(selected_end.source + 1, selected_end.offset),
        )
        .unwrap();
    assert!(!parent.matches_push_request(&FrameRequest::EPathMap {
        region: altered,
        owner: &PAR_PROTOBUF_REGION_PROGRAM,
        field: 0,
    }));
    assert!(!parent.matches_push_request(&FrameRequest::EPathMap {
        region: selected.clone(),
        owner: &NEW_RHO_MATCH_PROGRAM,
        field: 0,
    }));
    assert!(!parent.matches_push_request(&FrameRequest::EPathMap {
        region: selected.clone(),
        owner: &PAR_PROTOBUF_REGION_PROGRAM,
        field: 1,
    }));
    assert!(!parent.matches_push_request(&FrameRequest::Message {
        region: selected.clone(),
        program: &PAR_PROTOBUF_REGION_PROGRAM,
    }));

    rpb3_root_finish_trace::start_with_active_frames();
    let (summary, scores, stats) =
        drive_admission(FrameRequest::CanonicalPath(region), key.len()).unwrap();
    let trace = rpb3_root_finish_trace::take();
    let FrameSummary::CanonicalPath(path) = summary else {
        panic!("nested EPM root returned a different summary kind")
    };
    assert_eq!(path.form, CanonicalTopLevelPathForm::Bare);
    assert!(path.canonical_path_stable);
    assert_eq!(stats.input_bytes, key.len());
    assert_eq!(stats.canonical_path_obligations, 1);
    assert_eq!(stats.canonical_path_resolutions, 1);
    assert_eq!(stats.canonical_path_bytes_read, key.len());
    assert_eq!(trace.remaining_frames, Some(0));
    assert_eq!(
        trace.score_arena.as_ref().unwrap().len(),
        scores.source_len()
    );
    let pushes = trace
        .nonmessage_push_bodies
        .iter()
        .filter(|push| {
            push.obligation
                .is_some_and(|wait| wait.kind == CanonicalPathObligationKind::NestedEpm1)
                && trace.driver_steps[push.step_index].before_top_kind
                    == FrameRequestKind::CanonicalPath
        })
        .collect::<Vec<_>>();
    assert_eq!(pushes.len(), 1);
    let push = pushes[0];
    assert_eq!(push.body, snapshot);
    assert_eq!(push.start, anchors[body_start]);
    assert_eq!(push.end, anchors[key.len() - 1]);
    let wait = push
        .obligation
        .expect("nested EPM root records an obligation");
    assert_eq!(wait.kind, CanonicalPathObligationKind::NestedEpm1);
    let push_step = &trace.driver_steps[push.step_index];
    assert!(matches!(push_step.kind, DriverStepKind::Push(_)));
    assert_eq!(push_step.before_len, 1);
    assert_eq!(push_step.after_len, 2);
    assert_eq!(push_step.before_top_kind, FrameRequestKind::CanonicalPath);
    assert_eq!(push_step.after_top_kind, Some(FrameRequestKind::EPathMap));
    assert_eq!(
        push_step.after_top_region,
        Some((anchors[body_start], snapshot.len()))
    );
    assert_eq!(push_step.score_len_before, push_step.score_len_after);
    let Some(NonMessageFrameState::CanonicalPath(suspended)) =
        push_step.suspended_parent_after.as_ref()
    else {
        panic!("nested EPM push did not suspend CanonicalPath")
    };
    assert_eq!(suspended.input, key);
    assert_eq!(suspended.region_start, anchors[0]);
    assert_eq!(suspended.region_end, end);
    assert_eq!(suspended.bytes_yielded, key.len());
    let mut scalar = CanonicalPathTopologyAdmission::new();
    for (byte, anchor) in key.iter().zip(anchors) {
        scalar
            .edge(LocatedPathByte::in_source(
                *byte,
                anchor.source,
                anchor.offset,
            ))
            .expect("the accepted indexed key has a scalar parser successor");
    }
    assert_eq!(suspended.parser_source.stamp, key.len());
    assert_eq!(suspended.parser_source, scalar.parser_source_image());
    assert_eq!(suspended.waiting, Some(wait));
    assert!(suspended.active_obligation.is_some_and(|active| {
        active.obligation.id == wait.id
            && active.obligation.kind == wait.kind
            && active.obligation.payload_len == snapshot.len() as u64
            && active.obligation.tag_source
                == CanonicalPathSource::in_source(anchors[0].source, anchors[0].offset)
            && active.awaiting_resolution
    }));
    let before_stack = push_step.active_frames_before.as_ref().unwrap();
    let after_stack = push_step.active_frames_after.as_ref().unwrap();
    assert_eq!(before_stack.len(), 1);
    assert_eq!(after_stack.len(), 2);
    let ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::CanonicalPath(root),
        bytes: root_bytes,
        anchors: root_anchors,
    } = &before_stack[0]
    else {
        panic!("active root was not an indexed CanonicalPath frame")
    };
    assert_eq!(root.input, key);
    assert_eq!(root_bytes, key);
    assert_eq!(root_anchors, anchors);
    let ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::EPathMap(child),
        bytes: child_bytes,
        anchors: child_anchors,
    } = &after_stack[1]
    else {
        panic!("nested obligation did not push an EPathMap frame")
    };
    assert_eq!(child.snapshot, snapshot);
    assert_eq!(child_bytes, snapshot);
    assert_eq!(child_anchors, &anchors[body_start..]);
    assert_eq!(child.region_start, anchors[body_start]);
    assert_eq!(child.region_end, anchors[key.len() - 1]);

    rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
        &trace,
        snapshot,
        &anchors[body_start..],
        anchors[key.len() - 1],
    );
    let accepted = trace
        .pop_accepts
        .iter()
        .filter(|pop| {
            matches!(pop.parent_nonmessage_before.as_ref(),
            Some(NonMessageFrameState::CanonicalPath(parent)) if parent.waiting == Some(wait))
        })
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1);
    let pop = accepted[0];
    assert_eq!(pop.child_kind, FrameRequestKind::EPathMap);
    assert_eq!(pop.before_len, 2);
    assert_eq!(pop.after_len, Some(1));
    assert_eq!(pop.parent_kind, Some(FrameRequestKind::CanonicalPath));
    assert_eq!(pop.epathmap_returned_snapshot.as_deref(), Some(snapshot));
    assert_eq!(pop.epathmap_returned_mode, Some(EPathMapMode::Map));
    assert_eq!(pop.epathmap_returned_entries_stable, Some(true));
    let Some(NonMessageFrameState::EPathMap(completed_child)) = pop.child_nonmessage.as_ref()
    else {
        panic!("popped child was not EPathMap")
    };
    assert_eq!(completed_child.snapshot, snapshot);
    assert_eq!(completed_child.region_start, anchors[body_start]);
    assert_eq!(completed_child.region_end, anchors[key.len() - 1]);
    assert_eq!(completed_child.mode, EPathMapMode::Map);
    assert!(completed_child.entries_canonical_path_stable);
    assert!(completed_child.waiting_obligation.is_none());
    assert!(completed_child.active_obligation.is_none());
    let (applied_id, applied_resolution) = pop
        .applied_obligation
        .expect("CanonicalPath accepted EPathMap resolution");
    assert_eq!(applied_id, wait.id);
    assert_eq!(
        applied_resolution,
        CanonicalPathObligationResolution::NestedEpm1 {
            mode: NestedEpmMode::Map,
            entries_stable: completed_child.entries_canonical_path_stable,
        }
    );
    scalar
        .resolve_obligation(applied_id, applied_resolution)
        .expect("the scalar parser accepts the same nested EPathMap resolution");
    let Some(NonMessageFrameState::CanonicalPath(parent_after)) =
        pop.parent_nonmessage_after.as_ref()
    else {
        panic!("accepted parent was not CanonicalPath")
    };
    assert_eq!(parent_after.input, key);
    assert_eq!(parent_after.region_start, anchors[0]);
    assert_eq!(parent_after.region_end, end);
    assert_eq!(parent_after.bytes_yielded, key.len());
    assert_eq!(parent_after.parser_source, scalar.parser_source_image());
    assert_eq!(
        parent_after.parser_source.stamp,
        suspended.parser_source.stamp
    );
    assert_eq!(
        parent_after.parser_source.path_bytes,
        suspended.parser_source.path_bytes
    );
    assert!(parent_after.waiting.is_none());
    assert!(parent_after.active_obligation.is_none());

    let child_completes = trace
        .driver_steps
        .iter()
        .filter(|step| {
            matches!(step.kind, DriverStepKind::Complete)
                && step.before_len == 2
                && step.after_len == 1
                && step.before_top_kind == FrameRequestKind::EPathMap
                && step.after_top_kind == Some(FrameRequestKind::CanonicalPath)
        })
        .collect::<Vec<_>>();
    assert_eq!(child_completes.len(), 1);
    let Some(NonMessageFrameState::CanonicalPath(resumed)) =
        child_completes[0].after_nonmessage.as_ref()
    else {
        panic!("EPathMap Complete did not resume CanonicalPath")
    };
    assert_eq!(resumed, parent_after);
    let root_complete = trace.driver_steps.last().unwrap();
    assert!(matches!(root_complete.kind, DriverStepKind::Complete));
    assert_eq!(
        root_complete.before_top_kind,
        FrameRequestKind::CanonicalPath
    );
    assert_eq!(root_complete.before_len, 1);
    assert_eq!(root_complete.after_len, 0);
    assert_eq!(root_complete.before_capacity, root_complete.after_capacity);
    assert_eq!(root_complete.after_top_kind, None);
    assert_eq!(
        root_complete.score_len_before,
        root_complete.score_len_after
    );
    assert!(root_complete.score_appended.is_empty());
    assert_eq!(root_complete.score_len_after, scores.source_len());
    let Some(NonMessageFrameState::CanonicalPath(final_parent)) =
        root_complete.before_nonmessage.as_ref()
    else {
        panic!("root Complete did not start from CanonicalPath")
    };
    assert_eq!(final_parent, parent_after);
    assert!(root_complete
        .active_frames_after
        .as_ref()
        .unwrap()
        .is_empty());
    let root_pop = trace.pop_accepts.last().unwrap();
    assert_eq!(root_pop.child_kind, FrameRequestKind::CanonicalPath);
    assert_eq!(root_pop.before_len, 1);
    assert_eq!(root_pop.after_len, Some(0));
    assert_eq!(root_pop.parent_kind, None);
    assert_eq!(
        root_pop.source_len_before,
        root_pop.source_len_after.unwrap()
    );
    assert_eq!(root_pop.source_len_after, Some(scores.source_len()));
}

#[derive(Clone, Copy)]
enum RefusalCase {
    Empty,
    Set,
    UnstableMap,
}

fn refused_fixture(
    case: RefusalCase,
    key_number: i16,
) -> (Vec<u8>, Vec<u8>, usize, EPathMapMode, bool, &'static str) {
    let (map, mode, stable, rejection) = match case {
        RefusalCase::Empty => (
            EPathMap::default(),
            EPathMapMode::Empty,
            true,
            "NestedEpmNotMap",
        ),
        RefusalCase::Set => (
            EPathMap::from_set_iter([integer(i64::from(key_number))], Vec::new(), false, None),
            EPathMapMode::Set,
            true,
            "NestedEpmNotMap",
        ),
        RefusalCase::UnstableMap => (
            EPathMap::new_map(
                [(integer(i64::from(key_number)), Par::default())],
                Vec::new(),
                false,
                None,
            ),
            EPathMapMode::Map,
            false,
            "NestedEpmNotStable",
        ),
    };
    let snapshot = map.trie_snapshot().to_vec();
    let mut key = vec![tag::EPATHMAP_MAP];
    prost::encoding::encode_varint(snapshot.len() as u64, &mut key);
    let body_start = key.len();
    key.extend_from_slice(&snapshot);
    (key, snapshot, body_start, mode, stable, rejection)
}

fn assert_deferred_refusal(
    region: IndexedByteRegion<'_>,
    key: &[u8],
    snapshot: &[u8],
    body_start: usize,
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
    mode: EPathMapMode,
    stable: bool,
    rejection: &str,
) {
    assert_eq!(region.bytes().collect::<Vec<_>>(), key);
    assert_eq!(region.end_position().physical, end);
    assert_eq!(&key[body_start..], snapshot);
    for (index, anchor) in anchors.iter().enumerate() {
        assert_eq!(region.position_at(index).unwrap().physical, *anchor);
    }
    rpb3_root_finish_trace::start_with_active_frames();
    let error = match drive_admission(FrameRequest::CanonicalPath(region), key.len()) {
        Ok(_) => panic!("refused nested EPM root unexpectedly completed"),
        Err(error) => error,
    };
    let trace = rpb3_root_finish_trace::take();
    let CanonicalProtobufAdmissionError::CanonicalPath {
        position, detail, ..
    } = error
    else {
        panic!("refused nested EPM root returned a non-path error")
    };
    assert_eq!(
        position,
        CanonicalPathSource::in_source(anchors[0].source, anchors[0].offset)
    );
    assert_eq!(
        detail,
        format!(
            "{rejection} at snapshot source {} byte {}",
            anchors[0].source, anchors[0].offset
        )
    );
    assert!(trace.remaining_frames.is_none());
    assert!(trace.score_arena.is_none());
    assert!(trace.returned.is_none());
    assert!(!trace.driver_steps.iter().any(|step| {
        matches!(step.kind, DriverStepKind::Complete)
            && step.before_top_kind == FrameRequestKind::CanonicalPath
            && step.after_len == 0
    }));
    assert!(!trace
        .pop_accepts
        .iter()
        .any(|pop| pop.child_kind == FrameRequestKind::CanonicalPath));

    let pushes = trace
        .nonmessage_push_bodies
        .iter()
        .filter(|push| {
            push.obligation
                .is_some_and(|wait| wait.kind == CanonicalPathObligationKind::NestedEpm1)
                && trace.driver_steps[push.step_index].before_top_kind
                    == FrameRequestKind::CanonicalPath
        })
        .collect::<Vec<_>>();
    assert_eq!(pushes.len(), 1);
    let push = pushes[0];
    assert_eq!(push.body, snapshot);
    assert_eq!(push.start, anchors[body_start]);
    assert_eq!(push.end, anchors[key.len() - 1]);
    let wait = push.obligation.unwrap();
    let push_step = &trace.driver_steps[push.step_index];
    assert!(matches!(push_step.kind, DriverStepKind::Push(_)));
    assert_eq!(push_step.before_top_kind, FrameRequestKind::CanonicalPath);
    assert_eq!(push_step.after_top_kind, Some(FrameRequestKind::EPathMap));
    let Some(NonMessageFrameState::CanonicalPath(suspended)) =
        push_step.suspended_parent_after.as_ref()
    else {
        panic!("refused nested EPM did not suspend its path root")
    };
    assert_eq!(suspended.input, key);
    assert_eq!(suspended.region_start, anchors[0]);
    assert_eq!(suspended.region_end, end);
    assert_eq!(suspended.waiting, Some(wait));
    assert!(suspended.active_obligation.is_some_and(|active| {
        active.obligation.id == wait.id
            && active.obligation.kind == wait.kind
            && active.obligation.payload_len == snapshot.len() as u64
            && active.awaiting_resolution
    }));
    let after_stack = push_step.active_frames_after.as_ref().unwrap();
    assert_eq!(after_stack.len(), 2);
    let ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::EPathMap(child),
        bytes: child_bytes,
        anchors: child_anchors,
    } = &after_stack[1]
    else {
        panic!("refused nested EPM pushed another child kind")
    };
    assert_eq!(child.snapshot, snapshot);
    assert_eq!(child_bytes, snapshot);
    assert_eq!(child_anchors, &anchors[body_start..]);
    assert_eq!(child.region_start, anchors[body_start]);
    assert_eq!(child.region_end, anchors[key.len() - 1]);
    rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
        &trace,
        snapshot,
        &anchors[body_start..],
        anchors[key.len() - 1],
    );

    let accepted = trace
        .pop_accepts
        .iter()
        .filter(|pop| {
            matches!(pop.parent_nonmessage_before.as_ref(),
            Some(NonMessageFrameState::CanonicalPath(parent)) if parent.waiting == Some(wait))
        })
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1);
    let pop = accepted[0];
    assert_eq!(pop.child_kind, FrameRequestKind::EPathMap);
    assert_eq!(pop.before_len, 2);
    assert_eq!(pop.after_len, Some(1));
    assert_eq!(pop.parent_kind, Some(FrameRequestKind::CanonicalPath));
    assert_eq!(pop.epathmap_returned_snapshot.as_deref(), Some(snapshot));
    assert_eq!(pop.epathmap_returned_mode, Some(mode));
    assert_eq!(pop.epathmap_returned_entries_stable, Some(stable));
    let Some(NonMessageFrameState::EPathMap(completed_child)) = pop.child_nonmessage.as_ref()
    else {
        panic!("refused EPM child did not Complete")
    };
    assert_eq!(completed_child.snapshot, snapshot);
    assert_eq!(completed_child.region_start, anchors[body_start]);
    assert_eq!(completed_child.region_end, anchors[key.len() - 1]);
    assert_eq!(completed_child.mode, mode);
    assert_eq!(completed_child.entries_canonical_path_stable, stable);
    assert!(completed_child.waiting_obligation.is_none());
    assert!(completed_child.active_obligation.is_none());
    let (applied_id, applied_resolution) = pop.applied_obligation.unwrap();
    assert_eq!(applied_id, wait.id);
    assert_eq!(
        applied_resolution,
        CanonicalPathObligationResolution::NestedEpm1 {
            mode: match mode {
                EPathMapMode::Empty => NestedEpmMode::Empty,
                EPathMapMode::Set => NestedEpmMode::Set,
                EPathMapMode::Map => NestedEpmMode::Map,
            },
            entries_stable: completed_child.entries_canonical_path_stable,
        }
    );
    let Some(NonMessageFrameState::CanonicalPath(parent_after)) =
        pop.parent_nonmessage_after.as_ref()
    else {
        panic!("refused nested EPM did not resume CanonicalPath")
    };
    assert_eq!(parent_after.input, key);
    assert_eq!(parent_after.region_start, anchors[0]);
    assert_eq!(parent_after.region_end, end);
    assert_eq!(parent_after.bytes_yielded, key.len());
    assert!(parent_after.waiting.is_none());
    assert!(parent_after.active_obligation.is_none());
    let child_complete = trace.driver_steps.last().unwrap();
    assert!(matches!(child_complete.kind, DriverStepKind::Complete));
    assert_eq!(child_complete.before_top_kind, FrameRequestKind::EPathMap);
    assert_eq!(child_complete.before_len, 2);
    assert_eq!(
        child_complete.after_top_kind,
        Some(FrameRequestKind::CanonicalPath)
    );
    assert_eq!(child_complete.after_len, 1);
    let Some(NonMessageFrameState::CanonicalPath(resumed)) =
        child_complete.after_nonmessage.as_ref()
    else {
        panic!("child Complete did not resume the refused root")
    };
    assert_eq!(resumed, parent_after);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn nested_epm_root_applies_stable_map_resolution_before_complete(
        key_number in any::<i16>(),
        payload in prop::collection::vec(any::<u8>(), 0..96),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (key, snapshot, body_start) = fixture(key_number, &payload);
        let borrowed_anchors = (0..key.len())
            .map(|index| CanonicalByteProvenance::new(source, base + index))
            .collect::<Vec<_>>();
        let borrowed_end = CanonicalByteProvenance::new(source, base + key.len());
        assert_nested_root_return(
            IndexedByteRegion::from_borrowed(&key, source, base),
            &key, &snapshot, body_start, &borrowed_anchors, borrowed_end,
        );

        let fragmented_anchors = (0..key.len())
            .map(|index| CanonicalByteProvenance::new(source + 1 + (index % 23) as u32, base + index * 17))
            .collect::<Vec<_>>();
        let fragmented_end = CanonicalByteProvenance::new(source + 100, base);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in key.iter().zip(&fragmented_anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        assert_nested_root_return(
            IndexedByteRegion::from_frozen(arena.freeze(handle), fragmented_end),
            &key, &snapshot, body_start, &fragmented_anchors, fragmented_end,
        );
    }

    #[test]
    fn nested_epm_root_defers_non_map_and_unstable_map_refusals_until_after_child_complete(
        key_number in any::<i16>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        for case in [RefusalCase::Empty, RefusalCase::Set, RefusalCase::UnstableMap] {
            let (key, snapshot, body_start, mode, stable, rejection) =
                refused_fixture(case, key_number);
            let anchors = (0..key.len())
                .map(|index| CanonicalByteProvenance::new(source + (index % 23) as u32, base + index * 17))
                .collect::<Vec<_>>();
            let end = CanonicalByteProvenance::new(source + 100, base);
            let mut arena = CanonicalBytesArena::new();
            let mut handle = arena.empty();
            for (&byte, &anchor) in key.iter().zip(&anchors) {
                handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
            }
            assert_deferred_refusal(
                IndexedByteRegion::from_frozen(arena.freeze(handle), end),
                &key, &snapshot, body_start, &anchors, end, mode, stable, rejection,
            );
        }
    }
}
