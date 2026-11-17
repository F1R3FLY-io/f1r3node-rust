use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::tag;
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::protobuf_encoder;

fn fragmented(
    bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
) -> OwnedIndexedByteRegion {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (&byte, &anchor) in bytes.iter().zip(anchors) {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, anchor))
            .expect("bounded terminal fixture fits the indexed arena");
    }
    OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), end)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_epathmap_terminal_finish_accept_is_one_message_parent_step(
        key in any::<i16>(),
        value in any::<i16>(),
        source in 100u32..1000,
    ) {
        let map_value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(value))),
        }]);
        let key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(key))),
        }]);
        let map = EPathMap::new_map([(key_par, map_value)], Vec::new(), false, None);
        let snapshot = map.trie_snapshot().to_vec();
        let bytes = protobuf_encoder::encode_to_vec(&map);
        let anchors = (0..bytes.len())
            .map(|at| CanonicalByteProvenance::new(source + at as u32 % 23, 101 + at * 29))
            .collect::<Vec<_>>();
        let root = fragmented(
            &bytes, &anchors, CanonicalByteProvenance::new(source + 50, 800_000),
        );
        let mut frames = Vec::new();
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..Default::default()
        };
        push_admission_frame(&mut frames, FrameRequest::Message {
            region: root,
            program: &EPATHMAP_RHO_MATCH_PROGRAM,
        }, &mut stats).unwrap();
        let FrameStep::MessagePush { request, .. } =
            frames[0].step(&mut stats, &mut scores).unwrap() else {
                panic!("generated EPathMap wrapper must push its indexed EPathMap body")
            };
        prop_assert_eq!(request.request_kind(), FrameRequestKind::EPathMap);
        let selected = request.region().clone();
        prop_assert_eq!(selected.bytes().collect::<Vec<_>>(), snapshot.clone());
        push_admission_frame(&mut frames, request, &mut stats).unwrap();
        let Frame::EPathMap(child) = &mut frames[1] else { unreachable!() };
        let FrameAction::Push(value_request) = epathmap_step(child, &mut stats).unwrap() else {
            panic!("generated one-entry Map must select its value")
        };
        let (value_summary, value_scores, _) =
            drive_admission(value_request.clone(), value_request.region().len()).unwrap();
        prop_assert_eq!(scores.source_len(), 0);
        scores = value_scores;
        let Frame::EPathMap(child) = &mut frames[1] else { unreachable!() };
        prop_assert!(matches!(child.accept(value_summary, &mut stats),
            Ok(FrameAcceptEffect::EPathMapValue(_))));
        let before = DriverStepSource::before(&frames, &scores);
        let Frame::EPathMap(child) = &frames[1] else { unreachable!() };
        let invocation_before = EPathMapLoopStateSource::capture(child).unwrap();
        prop_assert!(invocation_before.matches_frame(child));
        rpb3_root_finish_trace::start_with_active_frames();
        let step_before = rpb3_root_finish_trace::driver_step_before(&frames, &scores);
        let Frame::EPathMap(child) = &mut frames[1] else { unreachable!() };
        let mut scratch = AdmissionScratch::allocate();
        let kind = epathmap_step_into(child, &mut stats, &mut scratch).unwrap();
        let final_pre_end = scratch.epm_terminal_before.take()
            .expect("finite EPathMap loop retains its final pre-End state");
        let FrameStep::Other {
            action: FrameAction::Complete(summary),
            map_source: None,
        } = scratch.take_test_step(kind) else {
            panic!("terminal EPathMap loop must select Complete")
        };
        let Frame::EPathMap(child) = &frames[1] else { unreachable!() };
        let terminal = child.terminal_source.as_ref().unwrap();
        prop_assert_eq!(final_pre_end.cursor.event_index,
            terminal.cursor_source.event_index);
        prop_assert_eq!(final_pre_end.cursor.map_values_seen,
            terminal.cursor_source.map_values_seen);
        prop_assert_eq!(final_pre_end.cursor.value_table_offset,
            terminal.cursor_source.value_table_offset);
        prop_assert_eq!(final_pre_end.cursor.snapshot_len,
            terminal.cursor_source.snapshot_len);
        prop_assert!(invocation_before.cursor.event_index <= final_pre_end.cursor.event_index);
        prop_assert!(matches!(final_pre_end.waiting, EPathMapLoopWaitSource::None));
        prop_assert!(final_pre_end.obligation.is_none());
        prop_assert_eq!(child.cursor.stats().terminal_checks, 1);

        let (outcome, mut completion_source, effect) = complete_admission_step_with_source(
            &before, &mut frames, summary, None, &scores, &mut stats, step_before,
        ).unwrap();
        let trace = rpb3_root_finish_trace::take();
        prop_assert_eq!(trace.driver_steps.len(), 1);
        let completed = &trace.driver_steps[0];
        prop_assert!(matches!(completed.kind, rpb3_root_finish_trace::DriverStepKind::Complete));
        prop_assert_eq!(completed.before_len, 2);
        prop_assert_eq!(completed.after_len, 1);
        prop_assert_eq!(completed.before_top_kind, FrameRequestKind::EPathMap);
        prop_assert_eq!(completed.after_top_kind, Some(FrameRequestKind::Message));
        prop_assert_eq!(completed.score_len_before, completed.score_len_after);
        prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::AcceptedMessage(_)));
        prop_assert!(matches!(effect.as_ref(), Some(FrameAcceptEffect::Stored)));
        prop_assert_eq!(completion_source.stack_len_before, 2);
        prop_assert_eq!(completion_source.score_len_before, scores.source_len());
        prop_assert_eq!(completion_source.score_len_after_finish, scores.source_len());
        let Some(CompleteTerminalSource::EPathMap {
            snapshot: retained,
            cursor_source: retained_cursor,
            ..
        }) = completion_source.terminal.as_deref() else {
            panic!("accepted EPathMap must retain its terminal source")
        };
        prop_assert!(retained.same_view(&selected));
        prop_assert_eq!(retained_cursor.event_index, final_pre_end.cursor.event_index);
        prop_assert_eq!(retained_cursor.map_values_seen, final_pre_end.cursor.map_values_seen);
        let Some(CompleteParentSource::Message {
            returned: FrameSummarySource::EPathMap(parent_returned),
            ..
        }) = completion_source.parent.as_ref() else {
            panic!("EPathMap return must retain the suspended Message parent source")
        };
        prop_assert!(parent_returned.snapshot.same_view(&selected));
        prop_assert_eq!(parent_returned.mode, EPathMapMode::Map);
        let Frame::Message(parent) = &frames[0] else { unreachable!() };
        prop_assert!(parent.waiting.is_none());
        let stored_matches = matches!(parent.semantic_slots[3].last(),
            Some(SemanticOccurrence { value: SemanticValue::EPathMap(stored), .. })
                if stored.snapshot.same_view(&selected));
        prop_assert!(stored_matches);
        let accepted = emit_checked_admission_event(CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        }).is_ok();
        prop_assert!(accepted);
        let Some(CompleteTerminalSource::EPathMap { returned, .. }) =
            completion_source.terminal.as_deref_mut() else { unreachable!() };
        let shadow_anchors = (0..selected.len())
            .map(|at| selected.position_at(at).unwrap().physical)
            .collect::<Vec<_>>();
        returned.snapshot = fragmented(
            &snapshot, &shadow_anchors, selected.end_position().physical,
        );
        prop_assert_eq!(returned.snapshot.bytes().collect::<Vec<_>>(), snapshot);
        prop_assert!(!returned.snapshot.same_view(&selected));
        let rejected = matches!(emit_checked_admission_event(CheckedAdmissionEvent {
            before: &before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_)));
        prop_assert!(rejected);
    }

    #[test]
    fn generated_epathmap_terminal_end_binds_summary_and_checked_stats(
        key in any::<i16>(),
        payload in prop::collection::vec(any::<u8>(), 1..33),
        source in 100u32..1000,
        offset in 0usize..100_000,
    ) {
        let value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(payload)),
        }]);
        let key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(key))),
        }]);
        let snapshot = EPathMap::new_map([(key_par, value)], Vec::new(), false, None)
            .trie_snapshot().to_vec();
        let mut root_bytes = vec![tag::EPATHMAP_MAP];
        prost::encoding::encode_varint(snapshot.len() as u64, &mut root_bytes);
        let snapshot_start = root_bytes.len();
        root_bytes.extend_from_slice(&snapshot);
        let anchors = (0..root_bytes.len())
            .map(|at| CanonicalByteProvenance::new(source + at as u32 % 29, offset + at * 31))
            .collect::<Vec<_>>();
        let root_end = CanonicalByteProvenance::new(source + 50, offset + 1_000_000);
        let root = fragmented(&root_bytes, &anchors, root_end);

        for branch in 0u8..4 {
            let mut stats = CanonicalProtobufAdmissionStats {
                frame_variant_bytes: std::mem::size_of::<Frame>(),
                ..Default::default()
            };
            let mut root_frame = CanonicalPathFrame::new(root.clone());
            let FrameAction::Push(request) = root_frame.step(&mut stats).unwrap() else {
                panic!("CanonicalPath root did not select its EPathMap child")
            };
            prop_assert_eq!(request.request_kind(), FrameRequestKind::EPathMap);
            let selected = request.region().clone();
            prop_assert_eq!(selected.bytes().collect::<Vec<_>>(), snapshot.clone());
            prop_assert_eq!(selected.len(), snapshot.len());
            for index in 0..snapshot.len() {
                prop_assert_eq!(selected.position_at(index).unwrap().physical,
                    anchors[snapshot_start + index]);
            }
            prop_assert_eq!(selected.end_position().physical, *anchors.last().unwrap());
            let mut frames = vec![Frame::CanonicalPath(root_frame)];
            push_admission_frame(&mut frames, request, &mut stats).unwrap();
            let Frame::EPathMap(child) = &mut frames[1] else { unreachable!() };
            let FrameAction::Push(value_request) = epathmap_step(child, &mut stats).unwrap() else {
                panic!("stable Map EPathMap did not push its Message value")
            };
            prop_assert_eq!(value_request.request_kind(), FrameRequestKind::Message);
            prop_assert!(child.matches_push_request(&value_request));
            let wrong_view = value_request.region().subview(CanonicalByteRange {
                start: 0,
                end: value_request.region().len() - 1,
            }).unwrap();
            let wrong_view_request = FrameRequest::Message {
                region: wrong_view,
                program: &PAR_PROTOBUF_REGION_PROGRAM,
            };
            prop_assert!(!child.matches_push_request(&wrong_view_request));
            let wrong_program_request = FrameRequest::Message {
                region: value_request.region().clone(),
                program: &SEND_RHO_MATCH_PROGRAM,
            };
            prop_assert!(!child.matches_push_request(&wrong_program_request));
            let wrong_kind_request = FrameRequest::EPathMap {
                region: value_request.region().clone(),
                owner: child.owner,
                field: child.field,
            };
            prop_assert!(!child.matches_push_request(&wrong_kind_request));
            let mut probe_frames = Vec::new();
            let mut probe_stats = CanonicalProtobufAdmissionStats::default();
            push_admission_frame(&mut probe_frames, value_request.clone(), &mut probe_stats).unwrap();
            prop_assert!(value_request.matches_pushed_frame(probe_frames.last().unwrap()));
            let Frame::Message(pushed) = probe_frames.last_mut().unwrap() else { unreachable!() };
            pushed.cursor = IndexedCanonicalFieldCursor::in_prost_slot_order(
                value_request.region().subview(CanonicalByteRange {
                    start: 0,
                    end: value_request.region().len() - 1,
                }).unwrap(),
            );
            prop_assert!(!value_request.matches_pushed_frame(probe_frames.last().unwrap()));
            let (value_summary, mut scores, _) =
                drive_admission(value_request.clone(), value_request.region().len()).unwrap();
            let score_len = scores.source_len();
            let Frame::EPathMap(child) = &mut frames[1] else { unreachable!() };
            prop_assert!(matches!(child.accept(value_summary, &mut stats),
                Ok(FrameAcceptEffect::EPathMapValue(_))));
            prop_assert!(child.snapshot.same_view(&selected));
            prop_assert_eq!(child.mode, EPathMapMode::Map);
            prop_assert!(child.entries_canonical_path_stable);
            prop_assert!(child.waiting.is_none());
            prop_assert!(child.waiting_child.is_none());
            prop_assert!(child.path.active_obligation().is_none());
            prop_assert!(child.terminal_source.is_none());

            let read_before = if branch == 1 { usize::MAX } else { 17 };
            let traversed_before = if branch == 2 { usize::MAX } else { 19 };
            let events_before = if branch == 3 { usize::MAX } else { 23 };
            stats.protobuf_bytes_read = read_before;
            stats.protobuf_bytes_traversed = traversed_before;
            stats.epathmap_events = events_before;
            let driver_source = DriverStepSource::before(&frames, &scores);
            rpb3_root_finish_trace::start_with_active_frames();
            let step_before = rpb3_root_finish_trace::driver_step_before(&frames, &scores);
            let Frame::EPathMap(child) = &mut frames[1] else { unreachable!() };
            let invocation_before = EPathMapLoopStateSource::capture(child).unwrap();
            prop_assert!(invocation_before.matches_frame(child));
            let mut step_scratch = AdmissionScratch::allocate();
            let kind = epathmap_step_into(child, &mut stats, &mut step_scratch);
            let terminal_before = step_scratch.epm_terminal_before.take()
                .expect("terminal attempt retains its final pre-End loop source");
            let action = kind.map(|kind| {
                let FrameStep::Other { action, map_source: None } =
                    step_scratch.take_test_step(kind) else { unreachable!() };
                action
            });
            let cursor = child.cursor.stats();
            let read_delta = cursor.prefix_bytes_read + cursor.value_table_bytes_read;
            let traversed_delta =
                cursor.prefix_bytes_traversed + cursor.value_table_bytes_traversed;
            let events_delta = cursor.topology_events;
            prop_assert_eq!(read_delta, snapshot.len());
            prop_assert!(traversed_delta > 0 && traversed_delta <= read_delta);
            prop_assert!(events_delta > 0);
            prop_assert_eq!(cursor.terminal_checks, 1);
            prop_assert_eq!(cursor.value_records, 1);
            let cursor_source = child.cursor.terminal_source_image()
                .expect("successful indexed EPM1 EOF has a terminal source image");
            prop_assert_eq!(cursor_source.mode, EPathMapMode::Map);
            prop_assert_eq!(cursor_source.declared_value_count, 1);
            prop_assert_eq!(cursor_source.map_values_seen, 1);
            prop_assert_eq!(cursor_source.event_index, events_delta);
            prop_assert_eq!(cursor_source.value_table_offset, snapshot.len());
            prop_assert_eq!(cursor_source.snapshot_len, snapshot.len());
            prop_assert_eq!(cursor_source.terminal_checks, 1);
            prop_assert!(matches!(terminal_before.waiting, EPathMapLoopWaitSource::None));
            prop_assert!(terminal_before.obligation.is_none());
            prop_assert_eq!(terminal_before.region_start, child.region_start);
            prop_assert_eq!(terminal_before.snapshot_len, child.snapshot.len());
            prop_assert_eq!(terminal_before.cursor.mode, cursor_source.mode);
            prop_assert_eq!(terminal_before.cursor.declared_value_count,
                cursor_source.declared_value_count);
            prop_assert_eq!(terminal_before.cursor.map_values_seen,
                cursor_source.map_values_seen);
            prop_assert_eq!(terminal_before.cursor.event_index,
                cursor_source.event_index);
            prop_assert_eq!(terminal_before.cursor.value_table_offset,
                cursor_source.value_table_offset);
            prop_assert_eq!(terminal_before.cursor.snapshot_len,
                cursor_source.snapshot_len);
            prop_assert!(invocation_before.cursor.event_index <= terminal_before.cursor.event_index);
            prop_assert_eq!(terminal_before.last_source, child.last_source);
            prop_assert_eq!(terminal_before.entries_canonical_path_stable,
                child.entries_canonical_path_stable);
            prop_assert!(child.cursor.active_source_image().is_none());
            prop_assert!(child.snapshot.same_view(&selected));
            prop_assert!(child.waiting.is_none());
            prop_assert!(child.waiting_child.is_none());
            prop_assert_eq!(scores.source_len(), score_len);

            match branch {
                0 => {
                    let FrameAction::Complete(FrameSummary::EPathMap(summary)) = action.unwrap() else {
                        panic!("successful terminal EPathMap step did not Complete")
                    };
                    prop_assert_eq!(summary.mode, EPathMapMode::Map);
                    prop_assert!(summary.entries_canonical_path_stable);
                    prop_assert!(summary.snapshot.same_view(&selected));
                    prop_assert_eq!(stats.protobuf_bytes_read, read_before + read_delta);
                    prop_assert_eq!(stats.protobuf_bytes_traversed,
                        traversed_before + traversed_delta);
                    prop_assert_eq!(stats.epathmap_events, events_before + events_delta);
                    let terminal = child.terminal_source.as_ref()
                        .expect("successful EPathMap Complete retains its terminal source");
                    prop_assert!(terminal.snapshot.same_view(&selected));
                    prop_assert_eq!(terminal.region_start, anchors[snapshot_start]);
                    prop_assert_eq!(terminal.end, *anchors.last().unwrap());
                    prop_assert_eq!(terminal.cursor, cursor);
                    prop_assert_eq!(terminal.cursor_source, cursor_source);
                    prop_assert_eq!(terminal.last_source, child.last_source);
                    prop_assert_eq!(terminal.mode, summary.mode);
                    prop_assert_eq!(terminal.entries_canonical_path_stable,
                        summary.entries_canonical_path_stable);
                    prop_assert_eq!(terminal.protobuf_read_before, read_before);
                    prop_assert_eq!(terminal.protobuf_traversed_before, traversed_before);
                    prop_assert_eq!(terminal.events_before, events_before);
                    let returned = FrameSummary::EPathMap(summary);
                    prop_assert!(terminal.matches_complete(child, &returned, &stats));
                    let corrupted_images = [
                        IndexedCanonicalEpm1TerminalSourceImage {
                            mode: EPathMapMode::Set,
                            ..cursor_source
                        },
                        IndexedCanonicalEpm1TerminalSourceImage {
                            declared_value_count: cursor_source.declared_value_count + 1,
                            ..cursor_source
                        },
                        IndexedCanonicalEpm1TerminalSourceImage {
                            map_values_seen: cursor_source.map_values_seen + 1,
                            ..cursor_source
                        },
                        IndexedCanonicalEpm1TerminalSourceImage {
                            event_index: cursor_source.event_index + 1,
                            ..cursor_source
                        },
                        IndexedCanonicalEpm1TerminalSourceImage {
                            value_table_offset: cursor_source.value_table_offset + 1,
                            ..cursor_source
                        },
                        IndexedCanonicalEpm1TerminalSourceImage {
                            snapshot_len: cursor_source.snapshot_len + 1,
                            ..cursor_source
                        },
                        IndexedCanonicalEpm1TerminalSourceImage {
                            terminal_checks: cursor_source.terminal_checks + 1,
                            ..cursor_source
                        },
                    ];
                    for corrupted in corrupted_images {
                        child.terminal_source.as_mut().unwrap().cursor_source = corrupted;
                        prop_assert!(!child.terminal_source.as_ref().unwrap()
                            .matches_complete(child, &returned, &stats));
                    }
                    child.terminal_source.as_mut().unwrap().cursor_source = cursor_source;
                    let (outcome, mut completion_source, accept_effect) = complete_admission_step_with_source(
                        &driver_source, &mut frames, returned, None, &scores, &mut stats,
                        step_before,
                    ).unwrap();
                    let atomic_trace = rpb3_root_finish_trace::take();
                    prop_assert_eq!(atomic_trace.driver_steps.len(), 1);
                    let completed_step = &atomic_trace.driver_steps[0];
                    prop_assert!(matches!(completed_step.kind, rpb3_root_finish_trace::DriverStepKind::Complete));
                    prop_assert_eq!(completed_step.before_len, 2);
                    prop_assert_eq!(completed_step.after_len, 1);
                    prop_assert_eq!(completed_step.before_top_kind, FrameRequestKind::EPathMap);
                    prop_assert_eq!(completed_step.after_top_kind, Some(FrameRequestKind::CanonicalPath));
                    prop_assert_eq!(completed_step.score_len_before, completed_step.score_len_after);
                    prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::AcceptedOther));
                    prop_assert!(emit_checked_admission_event(CheckedAdmissionEvent {
                        before: &driver_source,
                        after_frames: &frames,
                        after_scores: &scores,
                        action: CheckedAdmissionAction::OtherComplete {
                            outcome: &outcome,
                            source: &completion_source,
                            effect: accept_effect.as_ref(),
                            stats: &stats,
                        },
                        map_source: None,
                        accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
                    }).is_ok(), "EPathMap child completion must pass the checked event");
                    let Some(CompleteTerminalSource::EPathMap {
                        cursor_source: retained_cursor,
                        snapshot: retained_snapshot,
                        ..
                    }) = completion_source.terminal.as_deref() else {
                        panic!("accepted EPathMap must retain its post-End source image")
                    };
                    prop_assert_eq!(retained_cursor.event_index, terminal_before.cursor.event_index);
                    prop_assert_eq!(retained_cursor.map_values_seen,
                        terminal_before.cursor.map_values_seen);
                    prop_assert_eq!(retained_cursor.value_table_offset,
                        terminal_before.cursor.value_table_offset);
                    prop_assert!(retained_snapshot.same_view(&selected));
                    prop_assert!(completion_source.parent.as_ref().is_some_and(|parent|
                        parent.matches_effect(accept_effect.as_ref().unwrap())));
                    let Some(CompleteTerminalSource::EPathMap { cursor_source: retained_cursor, .. }) =
                        completion_source.terminal.as_deref_mut() else {
                            panic!("EPathMap return must retain its terminal source")
                        };
                    retained_cursor.event_index += 1;
                    let rejected = matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                        before: &driver_source,
                        after_frames: &frames,
                        after_scores: &scores,
                        action: CheckedAdmissionAction::OtherComplete {
                            outcome: &outcome,
                            source: &completion_source,
                            effect: accept_effect.as_ref(),
                            stats: &stats,
                        },
                        map_source: None,
                        accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
                    }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_)));
                    prop_assert!(rejected);
                    let Some(CompleteTerminalSource::EPathMap { cursor_source: retained_cursor, .. }) =
                        completion_source.terminal.as_deref_mut() else { unreachable!() };
                    *retained_cursor = cursor_source;
                    let Some(CompleteTerminalSource::EPathMap { events_before: retained_events, .. }) =
                        completion_source.terminal.as_deref_mut() else { unreachable!() };
                    *retained_events += 1;
                    let rejected = matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                        before: &driver_source,
                        after_frames: &frames,
                        after_scores: &scores,
                        action: CheckedAdmissionAction::OtherComplete {
                            outcome: &outcome,
                            source: &completion_source,
                            effect: accept_effect.as_ref(),
                            stats: &stats,
                        },
                        map_source: None,
                        accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
                    }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_)));
                    prop_assert!(rejected);
                    let Some(CompleteTerminalSource::EPathMap { events_before: retained_events, .. }) =
                        completion_source.terminal.as_deref_mut() else { unreachable!() };
                    *retained_events = events_before;
                    let Some(FrameAcceptEffect::ResolvedObligation { id, resolution }) =
                        accept_effect.as_ref() else { panic!("nested EPathMap must resolve the path") };
                    let CanonicalPathObligationResolution::NestedEpm1 { mode, .. } = resolution else {
                        unreachable!()
                    };
                    let forged_effect = FrameAcceptEffect::ResolvedObligation {
                        id: *id,
                        resolution: CanonicalPathObligationResolution::NestedEpm1 {
                            mode: *mode,
                            entries_stable: false,
                        },
                    };
                    let rejected = matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                        before: &driver_source,
                        after_frames: &frames,
                        after_scores: &scores,
                        action: CheckedAdmissionAction::OtherComplete {
                            outcome: &outcome,
                            source: &completion_source,
                            effect: Some(&forged_effect),
                            stats: &stats,
                        },
                        map_source: None,
                        accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
                    }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_)));
                    prop_assert!(rejected);
                    prop_assert_eq!(frames.len(), 1);
                    let Frame::CanonicalPath(parent) = &frames[0] else { unreachable!() };
                    prop_assert!(parent.reducer.active_obligation().is_none());
                    prop_assert_eq!(scores.source_len(), score_len);
                    let root_driver_source = DriverStepSource::before(&frames, &scores);
                    let root_action = frames[0].step(&mut stats, &mut scores).unwrap();
                    let FrameStep::Other {
                        action: FrameAction::Complete(root_summary),
                        map_source: None,
                    } = root_action else {
                        panic!("accepted EPathMap did not complete its CanonicalPath root")
                    };
                    let (returned_root, root_source, root_effect) = complete_admission_step_with_source(
                        &root_driver_source, &mut frames, root_summary, None,
                        &scores, &mut stats, None,
                    ).unwrap();
                    prop_assert!(matches!(&returned_root, CompleteAdmissionOutcome::Root(FrameSummary::CanonicalPath(_))));
                    prop_assert!(emit_checked_admission_event(CheckedAdmissionEvent {
                        before: &root_driver_source,
                        after_frames: &frames,
                        after_scores: &scores,
                        action: CheckedAdmissionAction::OtherComplete {
                            outcome: &returned_root,
                            source: &root_source,
                            effect: root_effect.as_ref(),
                            stats: &stats,
                        },
                        map_source: None,
                        accepted_children: CheckedChildSummaries::from_completion(&returned_root, &frames),
                    }).is_ok(), "CanonicalPath root completion must pass the checked event");
                    prop_assert!(frames.is_empty());
                    prop_assert_eq!(scores.source_len(), score_len);
                }
                1 => {
                    prop_assert!(rpb3_root_finish_trace::take().driver_steps.is_empty());
                    prop_assert!(matches!(action, Err(CanonicalProtobufAdmissionError::SizeOverflow)));
                    prop_assert!(child.terminal_source.is_none());
                    prop_assert_eq!(stats.protobuf_bytes_read, usize::MAX);
                    prop_assert_eq!(stats.protobuf_bytes_traversed, traversed_before);
                    prop_assert_eq!(stats.epathmap_events, events_before);
                }
                2 => {
                    prop_assert!(rpb3_root_finish_trace::take().driver_steps.is_empty());
                    prop_assert!(matches!(action, Err(CanonicalProtobufAdmissionError::SizeOverflow)));
                    prop_assert!(child.terminal_source.is_none());
                    prop_assert_eq!(stats.protobuf_bytes_read, read_before + read_delta);
                    prop_assert_eq!(stats.protobuf_bytes_traversed, usize::MAX);
                    prop_assert_eq!(stats.epathmap_events, events_before);
                }
                3 => {
                    prop_assert!(rpb3_root_finish_trace::take().driver_steps.is_empty());
                    prop_assert!(matches!(action, Err(CanonicalProtobufAdmissionError::SizeOverflow)));
                    prop_assert!(child.terminal_source.is_none());
                    prop_assert_eq!(stats.protobuf_bytes_read, read_before + read_delta);
                    prop_assert_eq!(stats.protobuf_bytes_traversed,
                        traversed_before + traversed_delta);
                    prop_assert_eq!(stats.epathmap_events, usize::MAX);
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn generated_empty_epathmap_two_scalar_events_complete_once_into_message_parent(
        source in 100u32..1000,
        offset in 0usize..100_000,
    ) {
        let empty = EPathMap::default();
        let snapshot = empty.trie_snapshot().to_vec();
        prop_assert_eq!(snapshot.as_slice(),
            &[0x45, 0x50, 0x4d, 0x31, 1, 0, 0, 0][..]);
        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(empty)),
        };
        let bytes = protobuf_encoder::encode_to_vec(&expr);
        let anchors = (0..bytes.len())
            .map(|at| CanonicalByteProvenance::new(
                source + at as u32 % 29, offset + at * 31,
            ))
            .collect::<Vec<_>>();
        let root = fragmented(
            &bytes, &anchors,
            CanonicalByteProvenance::new(source + 50, offset + 1_000_000),
        );
        let mut frames = Vec::new();
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..Default::default()
        };
        push_admission_frame(&mut frames, FrameRequest::Message {
            region: root,
            program: &EXPR_RHO_MATCH_PROGRAM,
        }, &mut stats).unwrap();
        let FrameStep::MessagePush { request: wrapper_request, .. } =
            frames[0].step(&mut stats, &mut scores).unwrap() else {
                panic!("Expr must push its EPathMap Message wrapper")
            };
        prop_assert_eq!(wrapper_request.request_kind(), FrameRequestKind::Message);
        push_admission_frame(&mut frames, wrapper_request, &mut stats).unwrap();
        let Frame::Message(wrapper) = &frames[1] else { unreachable!() };
        prop_assert!(std::ptr::eq(wrapper.program, &EPATHMAP_RHO_MATCH_PROGRAM));
        let FrameStep::MessagePush { request: child_request, .. } =
            frames[1].step(&mut stats, &mut scores).unwrap() else {
                panic!("EPathMap wrapper must push its indexed EPathMap body")
            };
        prop_assert_eq!(child_request.request_kind(), FrameRequestKind::EPathMap);
        let selected = child_request.region().clone();
        prop_assert_eq!(selected.bytes().collect::<Vec<_>>(), snapshot);
        let mode_anchor = selected.position_at(5).unwrap().physical;
        prop_assert!(anchors.contains(&mode_anchor));

        // An independent production cursor exposes the two modeled scalar
        // events at the actual fragmented mode-byte provenance.
        let mut probe = IndexedCanonicalEpm1TopologyCursor::new(selected.clone()).unwrap();
        prop_assert_eq!(probe.active_source_image().unwrap().event_index, 0);
        prop_assert!(probe.terminal_source_image().is_none());
        let first = probe.next_event_with_source().unwrap().unwrap();
        let first_matches = matches!(&first.event,
            IndexedCanonicalEpm1TopologyEvent::Value {
                has_value: false,
                value_body_range: None,
                value_body: None,
                source_offset: 5,
                provenance,
            } if *provenance == mode_anchor);
        prop_assert!(first_matches);
        prop_assert!(first.map_value_receipt.is_none());
        prop_assert_eq!(probe.active_source_image().unwrap().event_index, 1);
        prop_assert!(probe.terminal_source_image().is_none());
        let second = probe.next_event_with_source().unwrap().unwrap();
        let second_matches = matches!(&second.event,
            IndexedCanonicalEpm1TopologyEvent::Leave { provenance }
                if *provenance == mode_anchor);
        prop_assert!(second_matches);
        prop_assert!(second.map_value_receipt.is_none());
        prop_assert_eq!(probe.active_source_image().unwrap().event_index, 2);
        prop_assert!(probe.terminal_source_image().is_none());
        prop_assert!(probe.next_event_with_source().unwrap().is_none());
        let indexed_end = probe.terminal_source_image().unwrap();
        prop_assert_eq!(indexed_end.mode, EPathMapMode::Empty);
        prop_assert_eq!(indexed_end.declared_value_count, 0);
        prop_assert_eq!(indexed_end.map_values_seen, 0);
        prop_assert_eq!(indexed_end.event_index, 2);
        prop_assert_eq!(indexed_end.value_table_offset, selected.len());
        prop_assert_eq!(indexed_end.snapshot_len, 8);
        prop_assert_eq!(indexed_end.terminal_checks, 1);
        let indexed_stats = probe.stats();
        prop_assert_eq!(indexed_stats.prefix_bytes_read, 8);
        prop_assert_eq!(indexed_stats.prefix_bytes_traversed, 8);
        prop_assert_eq!(indexed_stats.value_table_bytes_read, 0);
        prop_assert_eq!(indexed_stats.value_table_bytes_traversed, 0);
        prop_assert_eq!(indexed_stats.topology_events, 2);
        prop_assert_eq!(indexed_stats.value_records, 0);

        push_admission_frame(&mut frames, child_request, &mut stats).unwrap();
        prop_assert_eq!(frames.len(), 3);
        let Frame::EPathMap(child) = &frames[2] else { unreachable!() };
        let invocation_before = EPathMapLoopStateSource::capture(child).unwrap();
        prop_assert_eq!(invocation_before.cursor.event_index, 0);
        prop_assert_eq!(invocation_before.cursor.value_table_offset, 8);
        prop_assert!(invocation_before.matches_frame(child));
        let driver_before = DriverStepSource::before(&frames, &scores);
        let stats_before = stats;
        rpb3_root_finish_trace::start_with_active_frames();
        let step_before = rpb3_root_finish_trace::driver_step_before(&frames, &scores);
        let Frame::EPathMap(child) = &mut frames[2] else { unreachable!() };
        let mut scratch = AdmissionScratch::allocate();
        let kind = epathmap_step_into(child, &mut stats, &mut scratch).unwrap();
        let final_pre_end = scratch.epm_terminal_before.take()
            .expect("empty scalar loop retains the final pre-End source");
        let FrameStep::Other {
            action: FrameAction::Complete(summary),
            map_source: None,
        } = scratch.take_test_step(kind) else {
            panic!("two empty scalar events must select one Complete")
        };
        let Frame::EPathMap(child) = &frames[2] else { unreachable!() };
        prop_assert_eq!(final_pre_end.cursor.mode, EPathMapMode::Empty);
        prop_assert_eq!(final_pre_end.cursor.event_index, 2);
        prop_assert_eq!(final_pre_end.cursor.map_values_seen, 0);
        prop_assert_eq!(final_pre_end.cursor.value_table_offset, 8);
        prop_assert_eq!(final_pre_end.cursor.snapshot_len, 8);
        prop_assert_eq!(final_pre_end.last_source,
            CanonicalPathSource::in_source(mode_anchor.source, mode_anchor.offset));
        prop_assert_eq!(final_pre_end.path, child.path.batch_source_shape());
        prop_assert!(child.path.batch_exact_source_observation().root_left);
        prop_assert!(matches!(final_pre_end.waiting, EPathMapLoopWaitSource::None));
        prop_assert!(final_pre_end.waiting_child_len.is_none());
        prop_assert!(final_pre_end.obligation.is_none());
        let FrameSummary::EPathMap(returned) = &summary else { unreachable!() };
        prop_assert_eq!(returned.mode, EPathMapMode::Empty);
        prop_assert!(returned.snapshot.same_view(&selected));
        prop_assert!(returned.entries_canonical_path_stable);
        let terminal = child.terminal_source.as_ref().unwrap();
        prop_assert!(terminal.matches_complete(child, &summary, &stats));
        prop_assert_eq!(terminal.cursor_source, indexed_end);
        prop_assert_eq!(terminal.last_source, final_pre_end.last_source);
        prop_assert_eq!(stats.epathmap_events, stats_before.epathmap_events + 2);
        prop_assert_eq!(stats.protobuf_bytes_read, stats_before.protobuf_bytes_read + 8);
        prop_assert_eq!(stats.protobuf_bytes_traversed,
            stats_before.protobuf_bytes_traversed + 8);
        prop_assert_eq!(stats.canonical_path_obligations,
            stats_before.canonical_path_obligations);
        prop_assert_eq!(stats.canonical_path_resolutions,
            stats_before.canonical_path_resolutions);
        prop_assert_eq!(scores.source_len(), 0);

        let (outcome, completion_source, effect) = complete_admission_step_with_source(
            &driver_before, &mut frames, summary, None, &scores, &mut stats, step_before,
        ).unwrap();
        let trace = rpb3_root_finish_trace::take();
        prop_assert_eq!(trace.driver_steps.len(), 1);
        let completed = &trace.driver_steps[0];
        prop_assert!(matches!(completed.kind, rpb3_root_finish_trace::DriverStepKind::Complete));
        prop_assert_eq!(completed.before_len, 3);
        prop_assert_eq!(completed.after_len, 2);
        prop_assert_eq!(completed.before_top_kind, FrameRequestKind::EPathMap);
        prop_assert_eq!(completed.after_top_kind, Some(FrameRequestKind::Message));
        prop_assert_eq!(completed.score_len_before, completed.score_len_after);
        prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::AcceptedMessage(_)));
        prop_assert!(matches!(effect.as_ref(), Some(FrameAcceptEffect::Stored)));
        prop_assert_eq!(completion_source.stack_len_before, 3);
        prop_assert_eq!(completion_source.score_len_before, 0);
        prop_assert_eq!(completion_source.score_len_after_finish, 0);
        let Some(CompleteTerminalSource::EPathMap {
            snapshot: retained, cursor_source: retained_cursor, ..
        }) = completion_source.terminal.as_deref() else {
            panic!("accepted empty EPathMap retains its terminal source")
        };
        prop_assert!(retained.same_view(&selected));
        prop_assert_eq!(*retained_cursor, indexed_end);
        let Frame::Message(parent) = &frames[1] else { unreachable!() };
        prop_assert!(parent.waiting.is_none());
        let stored_matches = matches!(parent.semantic_slots[3].last(),
            Some(SemanticOccurrence { value: SemanticValue::EPathMap(stored), .. })
                if stored.snapshot.same_view(&selected));
        prop_assert!(stored_matches);
        let accepted = emit_checked_admission_event(CheckedAdmissionEvent {
            before: &driver_before,
            after_frames: &frames,
            after_scores: &scores,
            action: CheckedAdmissionAction::OtherComplete {
                outcome: &outcome,
                source: &completion_source,
                effect: effect.as_ref(),
                stats: &stats,
            },
            map_source: None,
            accepted_children: CheckedChildSummaries::from_completion(&outcome, &frames),
        }).is_ok();
        prop_assert!(accepted);
    }

    #[test]
    fn generated_empty_and_set_epathmap_cursor_images_end_at_snapshot_boundary(
        key in any::<i16>(),
        source in 100u32..1000,
        offset in 0usize..100_000,
    ) {
        let key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(key))),
        }]);
        let snapshots = [
            (EPathMap::default().trie_snapshot().to_vec(), EPathMapMode::Empty),
            (EPathMap::from_set_iter([key_par], Vec::new(), false, None)
                .trie_snapshot().to_vec(), EPathMapMode::Set),
        ];
        for (snapshot, mode) in snapshots {
            let anchors = (0..snapshot.len())
                .map(|at| CanonicalByteProvenance::new(source + at as u32 % 29, offset + at * 31))
                .collect::<Vec<_>>();
            let end = CanonicalByteProvenance::new(source + 50, offset + 1_000_000);
            let mut cursor = IndexedCanonicalEpm1TopologyCursor::new(
                fragmented(&snapshot, &anchors, end)
            ).unwrap();
            prop_assert!(cursor.terminal_source_image().is_none());
            while cursor.next_event().unwrap().is_some() {
                prop_assert!(cursor.terminal_source_image().is_none());
            }
            let image = cursor.terminal_source_image()
                .expect("successful Empty or Set EOF has a terminal source image");
            prop_assert_eq!(image.mode, mode);
            prop_assert_eq!(image.declared_value_count, 0);
            prop_assert_eq!(image.map_values_seen, 0);
            prop_assert_eq!(image.event_index, cursor.stats().topology_events);
            prop_assert_eq!(image.value_table_offset, snapshot.len());
            prop_assert_eq!(image.snapshot_len, snapshot.len());
            prop_assert_eq!(image.terminal_checks, 1);
            prop_assert!(cursor.next_event().unwrap().is_none());
            prop_assert_eq!(cursor.terminal_source_image(), Some(image));
        }
    }
}
