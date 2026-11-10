use proptest::prelude::*;

use super::rpb3_root_finish_trace::{
    ActiveFrameState, DriverStepKind, NonMessageFrameState, RootReturnedState,
};
use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::{tag, CanonicalTopLevelPathForm};
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::canonical_path_admission::{
    CanonicalPathStreamReducer, CanonicalPathTopologyAdmission, LocatedPathByte,
};
use crate::rust::rholang::protobuf_encoder;

fn assert_direct_escape(
    region: IndexedByteRegion<'_>,
    key: &[u8],
    body: &[u8],
    body_start: usize,
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
) {
    assert_eq!(key[0], tag::ESCAPE);
    assert_eq!(&key[body_start..], body);
    assert!(!body.is_empty());
    assert_eq!(region.bytes().collect::<Vec<_>>(), key);
    assert_eq!(region.end_position().physical, end);
    for (index, anchor) in anchors.iter().enumerate() {
        assert_eq!(region.position_at(index).unwrap().physical, *anchor);
    }

    rpb3_root_finish_trace::start_with_active_frames();
    let (summary, scores, stats) =
        drive_admission(FrameRequest::CanonicalPath(region), key.len()).unwrap();
    let trace = rpb3_root_finish_trace::take();
    let FrameSummary::CanonicalPath(path) = summary else {
        panic!("direct escape root returned a different summary kind")
    };
    assert_eq!(path.form, CanonicalTopLevelPathForm::Bare);
    assert!(!path.canonical_path_stable);
    assert_eq!(stats.canonical_path_obligations, 1);
    assert_eq!(stats.canonical_path_resolutions, 1);
    assert_eq!(stats.canonical_path_bytes_read, key.len());
    assert!(stats.max_frames >= 2);
    assert_eq!(trace.remaining_frames, Some(0));
    assert!(trace.returned.is_none());
    assert_eq!(
        trace.score_arena.as_ref().unwrap().len(),
        scores.source_len()
    );
    assert_eq!(trace.nonmessage_push_bodies.len(), 1);

    let push = &trace.nonmessage_push_bodies[0];
    assert_eq!(push.body, body);
    assert_eq!(push.start, anchors[body_start]);
    assert_eq!(push.end, anchors[key.len() - 1]);
    let wait = push
        .obligation
        .expect("direct escape records one obligation");
    assert_eq!(wait.kind, CanonicalPathObligationKind::RawParEscape);
    let push_step = &trace.driver_steps[push.step_index];
    assert!(matches!(push_step.kind, DriverStepKind::Push(_)));
    assert_eq!(push_step.before_len, 1);
    assert_eq!(push_step.after_len, 2);
    assert_eq!(push_step.before_top_kind, FrameRequestKind::CanonicalPath);
    assert_eq!(push_step.after_top_kind, Some(FrameRequestKind::Message));
    assert!(std::ptr::eq(
        push_step.after_top_program.unwrap(),
        &PAR_PROTOBUF_REGION_PROGRAM,
    ));
    assert_eq!(
        push_step.after_top_region,
        Some((anchors[body_start], body.len()))
    );
    assert_eq!(push_step.score_len_before, push_step.score_len_after);
    assert!(push_step.score_appended.is_empty());
    let Some(NonMessageFrameState::CanonicalPath(suspended)) =
        push_step.suspended_parent_after.as_ref()
    else {
        panic!("direct escape did not suspend the root path")
    };
    assert_eq!(suspended.input, key);
    assert_eq!(suspended.region_start, anchors[0]);
    assert_eq!(suspended.region_end, end);
    assert_eq!(suspended.waiting, Some(wait));
    assert_eq!(suspended.bytes_yielded, key.len());
    assert!(suspended.active_obligation.is_some_and(|active| {
        active.obligation.id == wait.id
            && active.obligation.kind == wait.kind
            && active.obligation.payload_len == body.len() as u64
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
        panic!("initial active root was not CanonicalPath")
    };
    assert_eq!(root.input, key);
    assert_eq!(root_bytes, key);
    assert_eq!(root_anchors, anchors);
    let mut scalar = CanonicalPathTopologyAdmission::new();
    assert_eq!(root.parser_source, scalar.parser_source_image());
    let ActiveFrameState::Message {
        bytes: child_bytes,
        anchors: child_anchors,
        ..
    } = &after_stack[1]
    else {
        panic!("raw-Par escape did not push a Message child")
    };
    assert_eq!(child_bytes, body);
    assert_eq!(child_anchors, &anchors[body_start..]);
    assert_eq!(after_stack[0], ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::CanonicalPath(suspended.clone()),
        bytes: key.to_vec(),
        anchors: anchors.to_vec(),
    });

    let parent_pops = trace
        .pop_accepts
        .iter()
        .filter(|pop| {
            matches!(pop.parent_nonmessage_before.as_ref(),
            Some(NonMessageFrameState::CanonicalPath(parent)) if parent.waiting == Some(wait))
        })
        .collect::<Vec<_>>();
    assert_eq!(parent_pops.len(), 1);
    let pop = parent_pops[0];
    assert_eq!(pop.child_kind, FrameRequestKind::Message);
    assert_eq!(pop.before_len, 2);
    assert_eq!(pop.after_len, Some(1));
    assert_eq!(pop.parent_kind, Some(FrameRequestKind::CanonicalPath));
    assert_eq!(pop.source_len_before, pop.source_len_after.unwrap());
    assert_eq!(
        pop.parent_nonmessage_before.as_ref(),
        Some(&NonMessageFrameState::CanonicalPath(suspended.clone()))
    );
    let child_completes = trace
        .driver_steps
        .iter()
        .filter(|step| {
            matches!(step.kind, DriverStepKind::Complete)
                && step.before_len == 2
                && step.after_len == 1
                && step.before_top_kind == FrameRequestKind::Message
                && step.after_top_kind == Some(FrameRequestKind::CanonicalPath)
        })
        .collect::<Vec<_>>();
    assert_eq!(child_completes.len(), 1);
    let child_complete = child_completes[0];
    assert_eq!(
        child_complete.score_len_after,
        pop.source_len_after.unwrap()
    );
    let Some(NonMessageFrameState::CanonicalPath(child_return_parent)) =
        child_complete.after_nonmessage.as_ref()
    else {
        panic!("child Complete did not resume the CanonicalPath parent")
    };
    let Some(RootReturnedState::Par(returned)) = pop.typed_summary.as_ref() else {
        panic!("raw-Par child did not return a Par summary")
    };
    assert!(!returned.canonical_path_stable);
    let (applied_id, applied_resolution) = pop
        .applied_obligation
        .expect("root accepted the raw-Par resolution");
    assert_eq!(applied_id, wait.id);
    assert_eq!(
        applied_resolution,
        CanonicalPathObligationResolution::RawPar {
            canonical_path_stable: returned.canonical_path_stable,
        }
    );
    let Some(NonMessageFrameState::CanonicalPath(parent_after)) =
        pop.parent_nonmessage_after.as_ref()
    else {
        panic!("accepted parent was not CanonicalPath")
    };
    assert_eq!(parent_after.input, key);
    assert_eq!(parent_after.region_start, anchors[0]);
    assert_eq!(parent_after.region_end, end);
    assert_eq!(parent_after.bytes_yielded, key.len());
    assert!(parent_after.waiting.is_none());
    assert!(parent_after.active_obligation.is_none());
    assert_eq!(child_return_parent, parent_after);
    let child_complete_index = trace
        .driver_steps
        .iter()
        .position(|step| std::ptr::eq(step, child_complete))
        .unwrap();
    assert!(push.step_index < child_complete_index);
    let child_before = child_complete.active_frames_before.as_ref().unwrap();
    let child_after = child_complete.active_frames_after.as_ref().unwrap();
    assert_eq!(child_before.len(), 2);
    assert_eq!(child_after.len(), 1);
    assert_eq!(child_before[0], after_stack[0]);
    assert_eq!(child_after[0], ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::CanonicalPath(parent_after.clone()),
        bytes: key.to_vec(),
        anchors: anchors.to_vec(),
    });

    let path_complete = trace.driver_steps.last().unwrap();
    assert!(matches!(path_complete.kind, DriverStepKind::Complete));
    assert_eq!(
        path_complete.before_top_kind,
        FrameRequestKind::CanonicalPath
    );
    assert_eq!(path_complete.before_len, 1);
    assert_eq!(path_complete.after_len, 0);
    assert_eq!(path_complete.before_capacity, path_complete.after_capacity);
    assert_eq!(path_complete.after_top_kind, None);
    assert_eq!(
        path_complete.score_len_before,
        path_complete.score_len_after
    );
    assert!(path_complete.score_appended.is_empty());
    assert_eq!(path_complete.score_len_after, scores.source_len());
    let Some(NonMessageFrameState::CanonicalPath(completed)) =
        path_complete.before_nonmessage.as_ref()
    else {
        panic!("final Complete did not start from the root path")
    };
    assert_eq!(completed, parent_after);
    assert!(child_complete_index < trace.driver_steps.len() - 1);
    assert_eq!(
        path_complete.active_frames_before.as_ref().unwrap(),
        child_after
    );
    assert!(path_complete
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

    // Every source step exposes one synchronized storage/score image. Child
    // work can span several driver calls, but none may silently replace the
    // suspended parent or its selected indexed body between those calls.
    for adjacent in trace.driver_steps.windows(2) {
        assert_eq!(
            adjacent[0].active_frames_after,
            adjacent[1].active_frames_before
        );
        assert_eq!(adjacent[0].score_len_after, adjacent[1].score_len_before);
        // The trace records full score images only for Message Continue;
        // absent images on Push/Complete are not empty score arenas.
        if let (Some(after), Some(before)) = (
            adjacent[0].score_source_after.as_ref(),
            adjacent[1].score_source_before.as_ref(),
        ) {
            assert_eq!(after, before);
        }
    }

    assert!(!trace.canonical_path_bytes.is_empty());
    let mut next = 0;
    let mut traversed = 0;
    let mut bulk_events = 0;
    for observed in &trace.canonical_path_bytes {
        assert_eq!(observed.step_index, push.step_index);
        assert_eq!(observed.index, next);
        assert_eq!(observed.byte, key[next]);
        assert_eq!(observed.source, anchors[next]);
        if observed.skipped != 0 {
            bulk_events += 1;
            assert_eq!(observed.index, body_start - 1);
            assert_eq!(observed.skipped, body.len() - 1);
        }
        next += observed.skipped + 1;
        assert_eq!(observed.yielded_after, next);
        assert!(observed.traversed_after > traversed);
        assert!(observed.traversed_after <= traversed + observed.skipped + 1);
        assert!(observed.traversed_after <= observed.yielded_after);
        for index in observed.index..observed.yielded_after {
            let anchor = anchors[index];
            scalar
                .edge(LocatedPathByte::in_source(
                    key[index],
                    anchor.source,
                    anchor.offset,
                ))
                .expect("successful stream scan has a scalar topology successor");
        }
        let stream = &observed.parser_source_after;
        assert_eq!(stream.path_bytes.len(), observed.yielded_after);
        assert_eq!(stream.stamp, observed.yielded_after);
        assert!(stream.owner_chain.is_empty());
        assert_eq!(*stream, scalar.parser_source_image());
        traversed = observed.traversed_after;
    }
    assert_eq!(bulk_events, usize::from(body.len() > 1));
    assert_eq!(next, key.len());
    assert_eq!(stats.canonical_path_bytes_traversed, traversed);
    assert_eq!(suspended.parser_source, scalar.parser_source_image());
    assert_eq!(suspended.parser_source.stamp, key.len());
    scalar
        .resolve_obligation(applied_id, applied_resolution)
        .expect("the scalar reducer accepts the same child resolution");
    assert_eq!(parent_after.parser_source, scalar.parser_source_image());
    assert_eq!(
        parent_after.parser_source.stamp,
        suspended.parser_source.stamp
    );
    assert_eq!(
        parent_after.parser_source.path_bytes,
        suspended.parser_source.path_bytes
    );
    assert_eq!(completed.parser_source, scalar.parser_source_image());
}

#[test]
fn dead_stream_parser_snapshot_distinguishes_consumed_from_graph_stamp() {
    let bytes = [0xffu8, 0x02u8];
    let input = IndexedByteRegion::from_borrowed(&bytes, 77, 19);
    let mut reducer = CanonicalPathStreamReducer::new();
    let mut cursor = input.located_bytes();
    reducer.edge(&input, cursor.next().unwrap()).unwrap();
    let dead = reducer.parser_source_image(&input);
    assert!(dead.root_gate.dead);
    assert_eq!(dead.path_bytes.len(), 1);
    assert_eq!(dead.stamp, 1);
    reducer.edge(&input, cursor.next().unwrap()).unwrap();
    let later = reducer.parser_source_image(&input);
    assert!(later.root_gate.dead);
    assert_eq!(later.path_bytes.len(), 2);
    assert_eq!(later.stamp, 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn direct_raw_par_escape_completes_through_child_return(
        payload in prop::collection::vec(any::<u8>(), 0..200),
        source in 1u32..1000,
        base in 0usize..100_000,
        fragment_seed in 0usize..100_000,
    ) {
        let child = Par::default()
            .with_exprs(vec![Expr { expr_instance: Some(ExprInstance::GByteArray(payload)) }])
            .with_connective_used(true);
        let body = protobuf_encoder::encode_to_vec(&child);
        let mut key = vec![tag::ESCAPE];
        prost::encoding::encode_varint(body.len() as u64, &mut key);
        let body_start = key.len();
        key.extend_from_slice(&body);
        let borrowed_anchors = (0..key.len())
            .map(|index| CanonicalByteProvenance::new(source, base + index))
            .collect::<Vec<_>>();
        let borrowed_end = CanonicalByteProvenance::new(source, base + key.len());
        assert_direct_escape(
            IndexedByteRegion::from_borrowed(&key, source, base),
            &key, &body, body_start, &borrowed_anchors, borrowed_end,
        );

        let fragmented_anchors = (0..key.len())
            .map(|index| CanonicalByteProvenance::new(source + 1 + (index % 17) as u32, fragment_seed + index * 19))
            .collect::<Vec<_>>();
        let fragmented_end = CanonicalByteProvenance::new(source + 100, fragment_seed);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in key.iter().zip(&fragmented_anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        assert_direct_escape(
            IndexedByteRegion::from_frozen(arena.freeze(handle), fragmented_end),
            &key, &body, body_start, &fragmented_anchors, fragmented_end,
        );
    }
}
