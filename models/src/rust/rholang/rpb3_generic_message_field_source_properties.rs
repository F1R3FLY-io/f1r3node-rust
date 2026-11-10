use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use prost::Message;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::protobuf_encoder;
use crate::rust::rholang::schema_test_adapters::schema_fixtures;

fn decode_field_varint(bytes: &[u8], start: usize) -> (u64, usize) {
    let mut value = 0u64;
    for width in 0..10usize {
        let byte = *bytes
            .get(start + width)
            .expect("successful field has full varint");
        value |= u64::from(byte & 0x7f) << (7 * width);
        if byte & 0x80 == 0 {
            return (value, start + width + 1);
        }
    }
    panic!("successful field has an overlong varint")
}

fn varint_scalar_fields() -> Vec<(&'static RhoMatchProgram, u32, ProtobufKind)> {
    let mut cases = Vec::new();
    for (_, resident) in RHO_MATCH_REGISTRY {
        let program = wire_program_by_type_id(resident.type_id).unwrap();
        for (slot, field) in program.fields.iter().enumerate() {
            let mut add = |tag, kind| {
                if matches!(
                    kind,
                    ProtobufKind::Bool
                        | ProtobufKind::Int32
                        | ProtobufKind::Sint32
                        | ProtobufKind::Uint32
                        | ProtobufKind::Int64
                        | ProtobufKind::Sint64
                        | ProtobufKind::Uint64
                ) {
                    cases.push((program, tag, kind));
                }
            };
            match field.kind {
                RhoMatchFieldKind::Oneof(arms) => {
                    for arm in arms {
                        add(arm.tag, arm.protobuf_kind);
                    }
                }
                _ => add(
                    program.protobuf_fields[slot].tag,
                    program.protobuf_fields[slot].kind,
                ),
            }
        }
    }
    cases
}

fn check_varint_continue_read_binding(
    program: &'static RhoMatchProgram,
    tag: u32,
    kind: ProtobufKind,
    seed: u8,
    source: u32,
    offset: usize,
) {
    let value = if kind == ProtobufKind::Bool {
        1
    } else {
        u64::from(seed % 63) + 1
    };
    let mut bytes = Vec::new();
    prost::encoding::encode_varint(u64::from(tag) << 3, &mut bytes);
    prost::encoding::encode_varint(value, &mut bytes);
    let region = IndexedByteRegion::from_borrowed(&bytes, source, offset);
    let mut frames = vec![Frame::Message(
        MessageFrame::from_message_region(region, program).unwrap(),
    )];
    let mut scores = CanonicalScoreArena::new();
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let mut scratch = AdmissionScratch::allocate();
    let ticket = admission_live_core::PreStepTicket::before(&frames, &scores, &stats).unwrap();
    scratch.field_preimage.capture(frames.last().unwrap());
    scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
    let action = frames
        .last_mut()
        .unwrap()
        .step_into(&mut stats, &mut scores, &mut scratch)
        .unwrap();
    assert!(matches!(action, AdmissionStepKind::MessageContinue));
    let receipt = scratch.field.as_ref().unwrap();
    assert!(receipt.matches_varint_source_read());
    assert!(matches!(frames.last(), Some(Frame::Message(frame))
        if receipt.matches_frame(frame, &FrameAction::Continue)));
    assert!(
        emit_admission_noncompletion_event(action, &scratch, &frames, &scores, &ticket).is_ok()
    );

    // Change both sides of the old self-consistency comparison. The retained
    // body and pre-read cursor still name the original encoded value.
    let forged = if kind == ProtobufKind::Bool {
        0
    } else if value == 1 {
        2
    } else {
        1
    };
    let receipt = scratch.field.as_mut().unwrap();
    receipt.field.value = IndexedCanonicalFieldValue::Varint(forged);
    let scalar = semantic_scalar(&receipt.field, receipt.resolved.protobuf_kind()).unwrap();
    let Frame::Message(frame) = frames.last_mut().unwrap() else {
        unreachable!("the scalar Continue retains its message frame")
    };
    let occurrence = frame.semantic_slots[receipt.resolved.slot()]
        .last_mut()
        .unwrap();
    occurrence.value = SemanticValue::Scalar(scalar);
    assert!(matches!(&occurrence.value, SemanticValue::Scalar(actual)
        if semantic_scalar(&receipt.field, receipt.resolved.protobuf_kind())
            .is_ok_and(|expected| expected.same_source_value(actual))));
    assert!(!receipt.matches_varint_source_read());
    assert!(!receipt.matches_frame(frame, &FrameAction::Continue));
    assert!(matches!(
        emit_admission_noncompletion_event(action, &scratch, &frames, &scores, &ticket),
        Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
    ));
}

#[test]
fn every_generated_varint_scalar_continue_binds_the_indexed_read() {
    let cases = varint_scalar_fields();
    assert!(cases.len() >= 10);
    for (program, tag, kind) in cases {
        check_varint_continue_read_binding(program, tag, kind, 17, 7, 101);
    }
}

fn active_indexed_source(
    active: &rpb3_root_finish_trace::ActiveFrameState,
) -> (
    &[u8],
    &[CanonicalByteProvenance],
    CanonicalByteProvenance,
    CanonicalByteProvenance,
) {
    use rpb3_root_finish_trace::{ActiveFrameState, NonMessageFrameState};

    match active {
        ActiveFrameState::Message {
            frame,
            bytes,
            anchors,
        } => (bytes, anchors, frame.region_start, frame.region_end),
        ActiveFrameState::NonMessage {
            frame,
            bytes,
            anchors,
        } => {
            let (start, end) = match frame {
                NonMessageFrameState::Map(frame) => (frame.region_start, frame.region_end),
                NonMessageFrameState::EPathMap(frame) => (frame.region_start, frame.region_end),
                NonMessageFrameState::CanonicalPath(frame) => {
                    (frame.region_start, frame.region_end)
                }
            };
            (bytes, anchors, start, end)
        }
    }
}

fn assert_semantic_slots_follow_source(
    frame: &rpb3_root_finish_trace::RootFrameState,
    accepted: &[(usize, CanonicalByteProvenance)],
    label: &str,
) {
    assert_eq!(
        frame.semantic_slots.len(),
        frame.program.fields.len(),
        "{label}"
    );
    assert!(
        accepted
            .iter()
            .all(|(slot, _)| *slot < frame.program.fields.len()),
        "{label}"
    );
    assert!(
        accepted.windows(2).all(|pair| pair[0].0 <= pair[1].0),
        "{label}"
    );
    let flattened: Vec<_> = frame
        .semantic_slots
        .iter()
        .enumerate()
        .flat_map(|(slot, values)| {
            values.iter().map(move |value| {
                assert_eq!(value.slot, slot, "{label}");
                (slot, value.field_position)
            })
        })
        .collect();
    assert_eq!(flattened.as_slice(), accepted, "{label}");
}

fn accepted_child_projects(
    field: &rpb3_root_finish_trace::MessageFieldState,
    waiting: &rpb3_root_finish_trace::RootWaitState,
    occurrence: &rpb3_root_finish_trace::RootOccurrenceState,
    pop: &rpb3_root_finish_trace::PopAcceptState,
    child: &rpb3_root_finish_trace::ActiveFrameState,
    map_eof: Option<&rpb3_root_finish_trace::MapEofStepState>,
    epm_last_event: Option<&rpb3_root_finish_trace::EPathMapEventState>,
) -> bool {
    use rpb3_root_finish_trace::{NonMessageFrameState, RootOccurrenceValue, RootWaitState};

    let (bytes, anchors, start, end) = active_indexed_source(child);
    let Some(payload_start) = field.encoded_range.end.checked_sub(bytes.len()) else {
        return false;
    };
    if field.child_kind != Some(pop.child_kind)
        || field.slot != occurrence.slot
        || field.field_position != occurrence.field_position
        || field.child_region_start != Some(start)
        || field.child_region_len != Some(bytes.len())
        || field.payload_view_bytes.as_deref() != Some(bytes)
        || field
            .body_positions
            .get(payload_start..field.encoded_range.end)
            != Some(anchors)
        || field
            .body_positions
            .get(field.encoded_range.end)
            .copied()
            .unwrap_or(field.body_end_position)
            != end
        || resolve_protobuf_field(field.program, field.tag).map(|resolved| resolved.slot())
            != Some(field.slot)
    {
        return false;
    }

    match (waiting, &occurrence.value, child) {
        (
            RootWaitState::Child {
                slot,
                variant,
                body,
                body_start,
                body_end,
                field_position,
            },
            RootOccurrenceValue::Message(returned),
            rpb3_root_finish_trace::ActiveFrameState::Message { frame, .. },
        ) => {
            pop.child_kind == FrameRequestKind::Message
                && *slot == field.slot
                && *field_position == field.field_position
                && *variant == occurrence.variant
                && body == bytes
                && *body_start == start
                && *body_end == end
                && occurrence.body.as_deref() == Some(bytes)
                && occurrence.body_start == Some(start)
                && occurrence.body_end == Some(end)
                && pop.typed_summary.as_ref() == Some(returned.as_ref())
                && pop.child_frame.as_ref() == Some(frame)
                && pop.map_summary.is_none()
        }
        (
            RootWaitState::Map {
                slot,
                field_position,
            },
            RootOccurrenceValue::Map {
                key,
                key_position,
                value,
            },
            rpb3_root_finish_trace::ActiveFrameState::NonMessage {
                frame: NonMessageFrameState::Map(frame),
                ..
            },
        ) => {
            let Some(returned) = pop.map_summary.as_ref() else {
                return false;
            };
            let Some(source) = map_eof.map(|eof| &eof.frame) else {
                return false;
            };
            let mut after_eof = source.clone();
            let returned_value = after_eof.value.take();
            pop.child_kind == FrameRequestKind::Map
                && *slot == field.slot
                && *field_position == field.field_position
                && occurrence.variant.is_none()
                && occurrence.body.is_none()
                && occurrence.body_start.is_none()
                && occurrence.body_end.is_none()
                && pop.typed_summary.is_none()
                && pop.child_nonmessage.as_ref() == Some(&NonMessageFrameState::Map(after_eof))
                && key == &returned.key
                && *key_position == returned.key_position
                && value == &returned.value
                && source.body == bytes
                && source.region_start == start
                && source.region_end == end
                && source.key.as_deref().unwrap_or(&[]) == returned.key
                && source.key_position.unwrap_or(start) == returned.key_position
                && returned_value == returned.value
                && frame.value == source.value
                && frame.key == source.key
                && frame.key_position == source.key_position
        }
        (
            RootWaitState::EPathMap {
                slot,
                field_position,
            },
            RootOccurrenceValue::EPathMap {
                mode,
                snapshot,
                snapshot_start,
                entries_canonical_path_stable,
            },
            rpb3_root_finish_trace::ActiveFrameState::NonMessage {
                frame: NonMessageFrameState::EPathMap(frame),
                ..
            },
        ) => {
            let mut after_eof =
                epm_last_event.map_or_else(|| frame.clone(), |event| event.after.clone());
            let Some(terminal_checks) = after_eof.cursor.terminal_checks.checked_add(1) else {
                return false;
            };
            after_eof.cursor.terminal_checks = terminal_checks;
            pop.child_kind == FrameRequestKind::EPathMap
                && *slot == field.slot
                && *field_position == field.field_position
                && occurrence.variant.is_none()
                && occurrence.body.as_ref() == Some(snapshot)
                && occurrence.body_start == Some(start)
                && occurrence.body_end == Some(end)
                && pop.typed_summary.is_none()
                && pop.map_summary.is_none()
                && pop.child_nonmessage.as_ref() == Some(&NonMessageFrameState::EPathMap(after_eof))
                && pop.epathmap_child_snapshot.as_ref() == Some(snapshot)
                && pop.epathmap_returned_snapshot.as_ref() == Some(snapshot)
                && *snapshot_start == start
                && frame.snapshot == *snapshot
                && *mode == frame.mode
                && pop.epathmap_returned_mode == Some(*mode)
                && frame.entries_canonical_path_stable == *entries_canonical_path_stable
                && pop.epathmap_returned_entries_stable == Some(*entries_canonical_path_stable)
        }
        _ => false,
    }
}

fn assert_success_stream(
    trace: &rpb3_root_finish_trace::RootFinishTrace,
    label: &str,
) -> [usize; 3] {
    use rpb3_root_finish_trace::{DriverStepKind, MessageSuccessState, RootReturnedState};

    assert_eq!(
        trace.checked_events.len(),
        trace.driver_steps.len(),
        "{label}"
    );
    for (step_index, (checked, step)) in trace
        .checked_events
        .iter()
        .zip(&trace.driver_steps)
        .enumerate()
    {
        assert_eq!(checked.step_index, step_index, "{label}");
        assert_eq!(checked.kind, step.kind, "{label}");
        assert_eq!(checked.before_top_kind, step.before_top_kind, "{label}");
        assert_eq!(checked.after_top_kind, step.after_top_kind, "{label}");
        assert_eq!(checked.before_len, step.before_len, "{label}");
        assert_eq!(checked.after_len, step.after_len, "{label}");
        assert_eq!(checked.score_len_before, step.score_len_before, "{label}");
        assert_eq!(checked.score_len_after, step.score_len_after, "{label}");
        match (checked.projection, step.before_top_kind, step.kind) {
            (
                CheckedAdmissionProjection::MessageContinue,
                FrameRequestKind::Message,
                DriverStepKind::Continue,
            ) => {}
            (
                CheckedAdmissionProjection::MessagePush(child),
                FrameRequestKind::Message,
                DriverStepKind::Push(pushed),
            ) => assert_eq!(child, pushed.request_kind, "{label}"),
            (
                CheckedAdmissionProjection::MessageComplete {
                    scored,
                    root,
                    accepted_message_parent,
                },
                FrameRequestKind::Message,
                DriverStepKind::Complete,
            ) => {
                assert_eq!(
                    scored,
                    step.score_len_after > step.score_len_before,
                    "{label}"
                );
                assert_eq!(root, step.after_len == 0, "{label}");
                assert_eq!(
                    accepted_message_parent,
                    step.after_message.is_some(),
                    "{label}"
                );
            }
            (CheckedAdmissionProjection::OtherContinue(owner), kind, DriverStepKind::Continue)
                if kind != FrameRequestKind::Message =>
            {
                assert_eq!(owner, kind, "{label}")
            }
            (
                CheckedAdmissionProjection::OtherPush { owner, child },
                kind,
                DriverStepKind::Push(pushed),
            ) if kind != FrameRequestKind::Message => {
                assert_eq!(owner, kind, "{label}");
                assert_eq!(child, pushed.request_kind, "{label}");
            }
            (
                CheckedAdmissionProjection::OtherComplete {
                    owner,
                    root,
                    accepted_message_parent,
                },
                kind,
                DriverStepKind::Complete,
            ) if kind != FrameRequestKind::Message => {
                assert_eq!(owner, kind, "{label}");
                assert_eq!(root, step.after_len == 0, "{label}");
                assert_eq!(
                    accepted_message_parent,
                    step.after_message.is_some(),
                    "{label}"
                );
            }
            _ => panic!("{label}: checked admission event has the wrong total projection"),
        }
    }
    let mut event_at = 0usize;
    let mut field_at = 0usize;
    let mut eof_at = 0usize;
    let mut pop_at = 0usize;
    let mut accepted_projection_counts = [0usize; 3];
    let map_eofs_by_step: BTreeMap<_, _> = trace
        .map_eofs
        .iter()
        .map(|eof| (eof.step_index, eof))
        .collect();
    let epm_last_event_by_step: BTreeMap<_, _> = trace
        .epathmap_events
        .iter()
        .map(|event| (event.step_index, event))
        .collect();
    let mut pending_message_pushes = BTreeMap::new();
    let mut accepted_source_order: BTreeMap<usize, Vec<(usize, CanonicalByteProvenance)>> =
        BTreeMap::new();
    for (step_index, step) in trace.driver_steps.iter().enumerate() {
        let active_before = step.active_frames_before.as_ref().unwrap();
        let active_after = step.active_frames_after.as_ref().unwrap();
        for (depth, (parent, _, _, _, _)) in &pending_message_pushes {
            assert_eq!(&active_before[*depth], *parent, "{label}");
        }
        assert_eq!(active_before.len(), step.before_len, "{label}");
        assert_eq!(active_after.len(), step.after_len, "{label}");
        if step_index > 0 {
            assert_eq!(
                trace.driver_steps[step_index - 1]
                    .active_frames_after
                    .as_ref()
                    .unwrap(),
                active_before,
                "{label}",
            );
        }
        match step.kind {
            DriverStepKind::Continue => {
                assert_eq!(active_before.len(), active_after.len(), "{label}");
                assert_eq!(
                    &active_before[..active_before.len() - 1],
                    &active_after[..active_after.len() - 1],
                    "{label}",
                );
            }
            DriverStepKind::Push(_) => {
                assert_eq!(active_after.len(), active_before.len() + 1, "{label}");
                assert_eq!(
                    &active_before[..active_before.len() - 1],
                    &active_after[..active_after.len() - 2],
                    "{label}",
                );
            }
            DriverStepKind::Complete => {
                assert_eq!(active_before.len(), active_after.len() + 1, "{label}");
                if !active_after.is_empty() {
                    assert_eq!(
                        &active_before[..active_before.len() - 2],
                        &active_after[..active_after.len() - 1],
                        "{label}",
                    );
                }
            }
        }
        if step.before_top_kind == FrameRequestKind::Message {
            match step.kind {
                DriverStepKind::Continue | DriverStepKind::Push(_) => {
                    let MessageSuccessState::Field {
                        step_index: observed_step,
                        program_type_id,
                        field_number,
                        slot,
                        oneof_arm,
                        seen_before,
                        seen_after,
                        cursor_before,
                        cursor_after,
                        child_kind,
                        child_type_id,
                        child_start,
                        child_end,
                        child_len,
                        pushed,
                    } = &trace.message_success[event_at]
                    else {
                        panic!("{label}: Message field omitted its ordered success event")
                    };
                    let field = &trace.message_fields[field_at];
                    assert_eq!(*observed_step, step_index, "{label}");
                    assert_eq!(*program_type_id, field.program.type_id, "{label}");
                    assert_eq!(*field_number, field.tag, "{label}");
                    assert_eq!(*slot, field.slot, "{label}");
                    assert_eq!(*oneof_arm, field.oneof_arm, "{label}");
                    assert_eq!(*seen_before, field.seen_before, "{label}");
                    assert_eq!(*seen_after, field.seen_after, "{label}");
                    assert_eq!(*cursor_before, field.cursor_before, "{label}");
                    assert_eq!(*cursor_after, field.cursor_after, "{label}");
                    assert_eq!(*child_kind, field.child_kind, "{label}");
                    if let DriverStepKind::Push(pushed) = step.kind {
                        let depth = step.before_len - 1;
                        assert_eq!(*child_kind, Some(pushed.request_kind), "{label}");
                        assert!(
                            pending_message_pushes
                                .insert(
                                    depth,
                                    (
                                        &active_after[depth],
                                        active_after.last().unwrap(),
                                        *slot,
                                        pushed.request_kind,
                                        field_at,
                                    ),
                                )
                                .is_none(),
                            "{label}"
                        );
                    }
                    assert_eq!(*child_type_id, field.child_type_id, "{label}");
                    assert_eq!(*child_start, field.child_region_start, "{label}");
                    assert_eq!(*child_len, field.child_region_len, "{label}");
                    let source_order = accepted_source_order
                        .entry(step.before_len - 1)
                        .or_default();
                    match step.kind {
                        DriverStepKind::Continue => {
                            assert!(child_end.is_none(), "{label}");
                            assert!(pushed.is_none(), "{label}");
                            source_order.push((*slot, field.field_position));
                        }
                        DriverStepKind::Push(actual) => {
                            assert_eq!(*pushed, Some(actual), "{label}");
                            assert_eq!(*child_kind, Some(actual.request_kind), "{label}");
                            let active = step.active_frames_after.as_ref().unwrap();
                            let child = active.last().unwrap();
                            let expected_end = match child {
                                rpb3_root_finish_trace::ActiveFrameState::Message {
                                    frame, ..
                                } => frame.region_end,
                                rpb3_root_finish_trace::ActiveFrameState::NonMessage {
                                    frame,
                                    ..
                                } => match frame {
                                    rpb3_root_finish_trace::NonMessageFrameState::Map(value) => {
                                        value.region_end
                                    }
                                    rpb3_root_finish_trace::NonMessageFrameState::EPathMap(
                                        value,
                                    ) => value.region_end,
                                    rpb3_root_finish_trace::NonMessageFrameState::CanonicalPath(
                                        value,
                                    ) => value.region_end,
                                },
                            };
                            assert_eq!(*child_end, Some(expected_end), "{label}");
                        }
                        DriverStepKind::Complete => unreachable!(),
                    }
                    assert_semantic_slots_follow_source(&field.frame_after, source_order, label);
                    event_at += 1;
                    field_at += 1;
                }
                DriverStepKind::Complete => {
                    let MessageSuccessState::Eof {
                        step_index: observed_step,
                        program_type_id,
                        selected_kind,
                        score_len_before,
                        score_len_after,
                        component,
                        complete_index,
                        root,
                    } = &trace.message_success[event_at]
                    else {
                        panic!("{label}: Message EOF omitted its ordered success event")
                    };
                    assert_eq!(*observed_step, step_index, "{label}");
                    assert_eq!(*complete_index, eof_at, "{label}");
                    let complete = &trace.message_completes[*complete_index];
                    let source_order = accepted_source_order
                        .entry(step.before_len - 1)
                        .or_default();
                    assert_semantic_slots_follow_source(&complete.frame, source_order, label);
                    assert_eq!(complete.step_index, step_index, "{label}");
                    assert_eq!(*program_type_id, complete.frame.program_type_id, "{label}");
                    assert_eq!(
                        *selected_kind,
                        semantic_kind(complete.frame.program).unwrap(),
                        "{label}"
                    );
                    assert_eq!(*score_len_before, step.score_len_before, "{label}");
                    assert_eq!(*score_len_after, step.score_len_after, "{label}");
                    assert_eq!(*score_len_after, complete.source_len, "{label}");
                    assert_eq!(*root, step.before_len == 1, "{label}");
                    match (component, &complete.summary) {
                        (Some(observed), RootReturnedState::Component(actual, handle)) => {
                            assert_eq!(observed.family, actual.family, "{label}");
                            assert_eq!(observed.complete, actual.complete, "{label}");
                            assert_eq!(
                                observed.canonical_path_stable, actual.canonical_path_stable,
                                "{label}"
                            );
                            assert_eq!(observed.score_handle, *handle, "{label}");
                        }
                        (None, RootReturnedState::Component(..)) | (Some(_), _) => {
                            panic!(
                                "{label}: Message EOF component source differs from actual summary"
                            )
                        }
                        _ => {}
                    }
                    event_at += 1;
                    eof_at += 1;
                    accepted_source_order.remove(&(step.before_len - 1));
                }
            }
        }
        if matches!(step.kind, DriverStepKind::Complete) {
            let pop = &trace.pop_accepts[pop_at];
            if let Some(parent) = &step.after_message {
                let MessageSuccessState::Accepted {
                    step_index: observed_step,
                    program_type_id,
                    slot,
                    child_kind,
                    child_body,
                    child_start,
                    child_end,
                    field_position,
                    cursor_before,
                    cursor_after,
                    occurrences_before,
                    occurrences_after,
                    stack_len_before,
                    stack_len_after,
                    score_len_before_complete,
                    score_len_after_finish,
                    score_len_after_accept,
                    pop_index,
                } = &trace.message_success[event_at]
                else {
                    panic!("{label}: Message parent Accept omitted its ordered success event")
                };
                let before = pop.parent_before.as_ref().unwrap();
                let depth = step.after_len - 1;
                let (pushed_parent, pushed_child, pushed_slot, pushed_kind, pushed_field_at) =
                    pending_message_pushes
                        .remove(&depth)
                        .expect("accepted Message parent has a checked Push");
                assert_eq!(pushed_parent, &active_before[depth], "{label}");
                assert_eq!(pushed_slot, *slot, "{label}");
                assert_eq!(pushed_kind, *child_kind, "{label}");
                assert_eq!(
                    active_indexed_source(pushed_child),
                    active_indexed_source(active_before.last().unwrap()),
                    "{label}",
                );
                let rpb3_root_finish_trace::ActiveFrameState::Message {
                    frame: parent_before,
                    bytes: parent_bytes_before,
                    anchors: parent_anchors_before,
                } = &active_before[active_before.len() - 2]
                else {
                    panic!("{label}: accepted parent was not a Message frame")
                };
                let rpb3_root_finish_trace::ActiveFrameState::Message {
                    frame: parent_after,
                    bytes: parent_bytes_after,
                    anchors: parent_anchors_after,
                } = active_after.last().unwrap()
                else {
                    panic!("{label}: returned parent was not a Message frame")
                };
                assert_eq!(parent_before, before, "{label}");
                assert_eq!(parent_after, parent, "{label}");
                assert_eq!(parent_bytes_before, parent_bytes_after, "{label}");
                assert_eq!(parent_anchors_before, parent_anchors_after, "{label}");
                let mut expected_parent = parent_before.clone();
                expected_parent.waiting = false;
                expected_parent.waiting_detail = None;
                expected_parent.semantic_slots[*slot]
                    .push(parent_after.semantic_slots[*slot].last().unwrap().clone());
                assert_eq!(expected_parent, *parent_after, "{label}");
                assert_eq!(*observed_step, step_index, "{label}");
                assert_eq!(*pop_index, pop_at, "{label}");
                assert_eq!(*program_type_id, parent.program_type_id, "{label}");
                assert_eq!(
                    *slot,
                    pop.parent_before.as_ref().unwrap().last_slot.unwrap(),
                    "{label}"
                );
                assert_eq!(*child_kind, pop.child_kind, "{label}");
                let (bytes, anchors, start, end) =
                    active_indexed_source(active_before.last().unwrap());
                assert_eq!(child_body, bytes, "{label}");
                assert_eq!(*child_start, start, "{label}");
                assert_eq!(anchors.first().copied().unwrap_or(start), start, "{label}");
                assert_eq!(*child_end, end, "{label}");
                assert_eq!(
                    *field_position,
                    parent.semantic_slots[*slot].last().unwrap().field_position,
                    "{label}"
                );
                assert_eq!(
                    cursor_before.fields_read, before.cursor_fields_read,
                    "{label}"
                );
                assert_eq!(
                    cursor_after.fields_read, parent.cursor_fields_read,
                    "{label}"
                );
                assert_eq!(
                    *occurrences_before,
                    before.semantic_slots[*slot].len(),
                    "{label}"
                );
                assert_eq!(
                    *occurrences_after,
                    parent.semantic_slots[*slot].len(),
                    "{label}"
                );
                assert_eq!(*stack_len_before, step.before_len, "{label}");
                assert_eq!(*stack_len_after, step.after_len, "{label}");
                assert_eq!(*score_len_before_complete, step.score_len_before, "{label}");
                assert_eq!(*score_len_after_finish, step.score_len_after, "{label}");
                assert_eq!(*score_len_after_accept, step.score_len_after, "{label}");
                let accepted = parent.semantic_slots[*slot].last().unwrap();
                let pushed_field = &trace.message_fields[pushed_field_at];
                assert!(
                    accepted_child_projects(
                        pushed_field,
                        before.waiting_detail.as_ref().unwrap(),
                        accepted,
                        pop,
                        active_before.last().unwrap(),
                        map_eofs_by_step.get(&step_index).copied(),
                        epm_last_event_by_step.get(&step_index).copied(),
                    ),
                    "{label}: accepted {:?} child does not project from its field, indexed body, wait, and typed return",
                    pop.child_kind,
                );
                assert_eq!(pushed_field.step_index < step_index, true, "{label}");
                let kind_index = match pop.child_kind {
                    FrameRequestKind::Message => 0,
                    FrameRequestKind::Map => 1,
                    FrameRequestKind::EPathMap => 2,
                    FrameRequestKind::CanonicalPath => {
                        panic!("{label}: Message accepted path child")
                    }
                };
                accepted_projection_counts[kind_index] += 1;
                let mut corrupted = accepted.clone();
                match &mut corrupted.value {
                    rpb3_root_finish_trace::RootOccurrenceValue::Message(_) => {
                        corrupted.body_start = None;
                    }
                    rpb3_root_finish_trace::RootOccurrenceValue::Map { key, .. } => {
                        key.push(0);
                    }
                    rpb3_root_finish_trace::RootOccurrenceValue::EPathMap { snapshot, .. } => {
                        snapshot.push(0);
                    }
                    rpb3_root_finish_trace::RootOccurrenceValue::Scalar(_) => {
                        panic!("{label}: accepted child stored a scalar")
                    }
                }
                assert!(
                    !accepted_child_projects(
                        pushed_field,
                        before.waiting_detail.as_ref().unwrap(),
                        &corrupted,
                        pop,
                        active_before.last().unwrap(),
                        map_eofs_by_step.get(&step_index).copied(),
                        epm_last_event_by_step.get(&step_index).copied(),
                    ),
                    "{label}: corrupted child source was accepted",
                );
                if let rpb3_root_finish_trace::RootOccurrenceValue::Map {
                    value: Some(value), ..
                } = &accepted.value
                {
                    let mut corrupted = accepted.clone();
                    let rpb3_root_finish_trace::RootOccurrenceValue::Map {
                        value: Some(corrupted_value),
                        ..
                    } = &mut corrupted.value
                    else {
                        unreachable!()
                    };
                    let mut scores = CanonicalScoreArena::new();
                    let first = scores.integer(0).unwrap();
                    let second = scores.integer(1).unwrap();
                    corrupted_value.score_handle = if value.score_handle == first {
                        second
                    } else {
                        first
                    };
                    assert!(
                        !accepted_child_projects(
                            pushed_field,
                            before.waiting_detail.as_ref().unwrap(),
                            &corrupted,
                            pop,
                            active_before.last().unwrap(),
                            map_eofs_by_step.get(&step_index).copied(),
                            epm_last_event_by_step.get(&step_index).copied(),
                        ),
                        "{label}: corrupted Map value handle was accepted",
                    );
                }
                if matches!(pop.child_kind, FrameRequestKind::EPathMap) {
                    let mut corrupted = accepted.clone();
                    corrupted.body_start = None;
                    assert!(
                        !accepted_child_projects(
                            pushed_field,
                            before.waiting_detail.as_ref().unwrap(),
                            &corrupted,
                            pop,
                            active_before.last().unwrap(),
                            map_eofs_by_step.get(&step_index).copied(),
                            epm_last_event_by_step.get(&step_index).copied(),
                        ),
                        "{label}: corrupted EPathMap snapshot anchor was accepted",
                    );
                }
                let source_order = accepted_source_order.entry(depth).or_default();
                source_order.push((*slot, *field_position));
                assert_semantic_slots_follow_source(parent, source_order, label);
                assert_eq!(
                    pop.typed_summary.is_some(),
                    pop.child_kind == FrameRequestKind::Message,
                    "{label}"
                );
                event_at += 1;
            }
            pop_at += 1;
        }
        for (depth, (parent, _, _, _, _)) in &pending_message_pushes {
            assert_eq!(&active_after[*depth], *parent, "{label}");
        }
    }
    assert_eq!(event_at, trace.message_success.len(), "{label}");
    assert_eq!(field_at, trace.message_fields.len(), "{label}");
    assert_eq!(eof_at, trace.message_completes.len(), "{label}");
    assert_eq!(pop_at, trace.pop_accepts.len(), "{label}");
    assert!(pending_message_pushes.is_empty(), "{label}");
    assert!(accepted_source_order.is_empty(), "{label}");
    let mut checked_projection_counts = [0usize; 3];
    for event in &trace.checked_events {
        let accepted = match event.projection {
            CheckedAdmissionProjection::MessageComplete {
                accepted_message_parent,
                ..
            }
            | CheckedAdmissionProjection::OtherComplete {
                accepted_message_parent,
                ..
            } => accepted_message_parent,
            _ => false,
        };
        if accepted {
            let kind_index = match event.before_top_kind {
                FrameRequestKind::Message => 0,
                FrameRequestKind::Map => 1,
                FrameRequestKind::EPathMap => 2,
                FrameRequestKind::CanonicalPath => {
                    panic!("{label}: checked path completion claims Message acceptance")
                }
            };
            checked_projection_counts[kind_index] += 1;
        }
    }
    assert_eq!(
        accepted_projection_counts, checked_projection_counts,
        "{label}"
    );
    accepted_projection_counts
}

fn gint(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn append_varint(mut value: u64, bytes: &mut Vec<u8>) {
    while value >= 0x80 {
        bytes.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
}

fn encoded_length_field(tag: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    append_varint((u64::from(tag) << 3) | 2, &mut bytes);
    append_varint(u64::try_from(payload.len()).unwrap(), &mut bytes);
    bytes.extend_from_slice(payload);
    bytes
}

fn count_nonmessage_actions(
    trace: &rpb3_root_finish_trace::RootFinishTrace,
    actions: &mut [usize; 10],
) {
    for event in &trace.checked_events {
        let index = match event.projection {
            CheckedAdmissionProjection::OtherContinue(_) => Some(4),
            CheckedAdmissionProjection::OtherPush { .. } => Some(5),
            CheckedAdmissionProjection::OtherComplete { .. } => Some(6),
            _ => None,
        };
        if let Some(index) = index {
            actions[index] += 1;
        }
    }
}

fn check_run(
    label: &str,
    bytes: &[u8],
    program: &'static RhoMatchProgram,
    programs: &mut BTreeSet<u16>,
    tags: &mut BTreeSet<(u16, u32)>,
    actions: &mut [usize; 10],
) {
    check_run_region(
        label,
        IndexedByteRegion::from_borrowed(bytes, 0, 0),
        program,
        programs,
        tags,
        actions,
    );
}

fn check_run_region(
    label: &str,
    region: AdmissionRegion<'_>,
    program: &'static RhoMatchProgram,
    programs: &mut BTreeSet<u16>,
    tags: &mut BTreeSet<(u16, u32)>,
    actions: &mut [usize; 10],
) {
    rpb3_root_finish_trace::start_with_message_success_stream();
    let result = validate_canonical_protobuf_inner(region, program, None);
    let trace = rpb3_root_finish_trace::take();
    assert!(result.is_ok(), "{label}: {result:?}");
    assert!(trace.returned.is_some(), "{label}");
    let accepted = assert_success_stream(&trace, label);
    for (kind, count) in accepted.into_iter().enumerate() {
        actions[7 + kind] += count;
    }
    count_nonmessage_actions(&trace, actions);
    programs.insert(program.type_id);
    for field in &trace.message_fields {
        let step = &trace.driver_steps[field.step_index];
        let before = step.before_message.as_ref().expect("message pre-state");
        let after = &field.frame_after;
        let resolved = resolve_protobuf_field(before.program, field.tag)
            .expect("executed tag belongs to the generated wire descriptor");
        programs.insert(before.program_type_id);
        tags.insert((before.program_type_id, field.tag));
        assert_eq!(before.program_type_id, field.program.type_id, "{label}");
        assert_eq!(before.program_type_id, after.program_type_id, "{label}");
        assert_eq!(before.region_start, after.region_start, "{label}");
        assert_eq!(before.region_end, after.region_end, "{label}");
        assert_eq!(before.region_len, after.region_len, "{label}");
        assert_eq!(field.slot, resolved.slot(), "{label}");
        assert_eq!(field.slot_count, before.program.fields.len(), "{label}");
        assert_eq!(field.slot_count, before.semantic_slots.len(), "{label}");
        assert_eq!(field.slot_count, after.semantic_slots.len(), "{label}");
        assert!(field.slot < field.slot_count, "{label}");
        assert_eq!(field.last_slot_before, before.last_slot, "{label}");
        assert_eq!(field.last_slot_after, after.last_slot, "{label}");
        assert_eq!(field.last_slot_after, Some(field.slot), "{label}");
        assert!(
            field.last_slot_before.is_none_or(|last| last <= field.slot),
            "{label}"
        );
        assert_eq!(
            field.seen_before,
            before.seen_slots.contains(&field.slot),
            "{label}"
        );
        assert_eq!(
            field.seen_after,
            after.seen_slots.contains(&field.slot),
            "{label}"
        );
        assert!(field.seen_after, "{label}");
        assert!(resolved.repeated() || !field.seen_before, "{label}");
        let expected_seen = (0..field.slot_count)
            .filter(|slot| before.seen_slots.contains(slot) || *slot == field.slot)
            .collect::<Vec<_>>();
        assert_eq!(after.seen_slots, expected_seen, "{label}");
        assert_eq!(
            field.slot_occurrences_before,
            before.semantic_slots[field.slot].len(),
            "{label}"
        );
        assert_eq!(
            field.slot_occurrences_after,
            after.semantic_slots[field.slot].len(),
            "{label}"
        );
        for other in 0..field.slot_count {
            if other != field.slot {
                assert_eq!(
                    after.semantic_slots[other], before.semantic_slots[other],
                    "{label}"
                );
            }
        }
        assert_eq!(
            field.cursor_before.fields_read, before.cursor_fields_read,
            "{label}"
        );
        assert_eq!(
            field.cursor_before.bytes_read, before.cursor_bytes_read,
            "{label}"
        );
        assert_eq!(
            field.cursor_after.fields_read, after.cursor_fields_read,
            "{label}"
        );
        assert_eq!(
            field.cursor_after.bytes_read, after.cursor_bytes_read,
            "{label}"
        );
        assert_eq!(
            field.cursor_after.fields_read,
            field.cursor_before.fields_read + 1,
            "{label}"
        );
        assert_eq!(
            field.encoded_range.start, field.cursor_before.bytes_read,
            "{label}"
        );
        assert_eq!(
            field.encoded_range.end, field.cursor_after.bytes_read,
            "{label}"
        );
        let (key, key_end) = decode_field_varint(&field.body, field.encoded_range.start);
        assert_eq!(key >> 3, u64::from(field.tag), "{label}");
        let key_width = key_end - field.encoded_range.start;
        assert!((1..=5).contains(&key_width), "{label}");
        let (wire, payload_start, payload_end, payload_len, decoded_varint) = match key & 7 {
            0 => {
                let (value, end) = decode_field_varint(&field.body, key_end);
                (CanonicalWireType::Varint, key_end, end, None, Some(value))
            }
            1 => (
                CanonicalWireType::Fixed64,
                key_end,
                key_end + 8,
                Some(8),
                None,
            ),
            2 => {
                let (len, start) = decode_field_varint(&field.body, key_end);
                let len = usize::try_from(len).expect("accepted length fits the machine");
                (
                    CanonicalWireType::LengthDelimited,
                    start,
                    start + len,
                    Some(len),
                    None,
                )
            }
            5 => (
                CanonicalWireType::Fixed32,
                key_end,
                key_end + 4,
                Some(4),
                None,
            ),
            _ => panic!("successful field has a supported wire type"),
        };
        assert_eq!(field.wire, wire, "{label}");
        assert_eq!(field.payload_len, payload_len, "{label}");
        assert_eq!(field.encoded_range.end, payload_end, "{label}");
        assert!(payload_start <= payload_end, "{label}");
        assert!(payload_end <= field.body.len(), "{label}");
        assert_eq!(
            field.payload_view_bytes.as_deref(),
            payload_len.map(|_| &field.body[payload_start..payload_end]),
            "{label}"
        );
        assert_eq!(field.decoded_varint, decoded_varint, "{label}");
        if field.child_kind.is_some() {
            assert_eq!(
                field.payload_position,
                Some(
                    field
                        .body_positions
                        .get(payload_start)
                        .copied()
                        .unwrap_or(field.body_end_position),
                ),
                "{label}"
            );
        }
        assert_eq!(
            field.wire,
            canonical_wire_type(resolved.protobuf_kind().wire_type().unwrap()),
            "{label}"
        );
        let expected_oneof_arm = match resolved.payload() {
            ResolvedProtobufPayload::Oneof(arm) => Some(arm.tag),
            ResolvedProtobufPayload::Declared(_) => None,
        };
        assert_eq!(field.oneof_arm, expected_oneof_arm, "{label}");
        let expected_child_type_id = match resolved.payload() {
            ResolvedProtobufPayload::Oneof(arm) => match arm.payload {
                RhoMatchPayloadKind::Message(type_id) => Some(type_id),
                _ => None,
            },
            ResolvedProtobufPayload::Declared(_) => match resolved.field().kind {
                RhoMatchFieldKind::OptionalMessage(type_id)
                | RhoMatchFieldKind::MessageSequence(type_id)
                | RhoMatchFieldKind::StringMessageMap(type_id) => Some(type_id),
                _ => None,
            },
        };
        let expected_child_kind = match resolved.payload() {
            ResolvedProtobufPayload::Oneof(arm) => match arm.payload {
                RhoMatchPayloadKind::Message(_) => Some(FrameRequestKind::Message),
                _ => None,
            },
            ResolvedProtobufPayload::Declared(_) => match resolved.field().kind {
                RhoMatchFieldKind::OptionalMessage(_) | RhoMatchFieldKind::MessageSequence(_) => {
                    Some(FrameRequestKind::Message)
                }
                RhoMatchFieldKind::StringMessageMap(_) => Some(FrameRequestKind::Map),
                RhoMatchFieldKind::PathMap => Some(FrameRequestKind::EPathMap),
                _ => None,
            },
        };
        assert_eq!(field.child_kind, expected_child_kind, "{label}");
        match step.kind {
            rpb3_root_finish_trace::DriverStepKind::Continue => {
                actions[0] += 1;
                assert_eq!(field.child_kind, None, "{label}");
                assert_eq!(field.child_type_id, None, "{label}");
                assert_eq!(field.child_region_start, None, "{label}");
                assert_eq!(field.child_region_len, None, "{label}");
                assert_eq!(field.waiting_after, None, "{label}");
                assert!(!after.waiting, "{label}");
                assert_eq!(
                    field.slot_occurrences_after,
                    field.slot_occurrences_before + 1,
                    "{label}"
                );
            }
            rpb3_root_finish_trace::DriverStepKind::Push(push) => {
                assert_eq!(push.before_len, step.before_len, "{label}");
                assert_eq!(push.after_len, step.after_len, "{label}");
                assert_eq!(push.before_capacity, step.before_capacity, "{label}");
                assert_eq!(push.after_capacity, step.after_capacity, "{label}");
                assert_eq!(push.after_len, push.before_len + 1, "{label}");
                assert!(push.after_capacity >= push.before_capacity, "{label}");
                assert_eq!(
                    push.frame_variant_bytes, push.stats_frame_variant_bytes,
                    "{label}"
                );
                assert!(push.used_bytes <= push.reserved_bytes, "{label}");
                let action_index = match push.request_kind {
                    FrameRequestKind::Message => 1,
                    FrameRequestKind::Map => 2,
                    FrameRequestKind::EPathMap => 3,
                    FrameRequestKind::CanonicalPath => {
                        panic!("message field pushed a path directly")
                    }
                };
                actions[action_index] += 1;
                assert_eq!(field.child_kind, Some(push.request_kind), "{label}");
                assert_eq!(field.waiting_after, field.child_kind, "{label}");
                assert_eq!(field.child_type_id, expected_child_type_id, "{label}");
                assert_eq!(field.child_region_start, field.payload_position, "{label}");
                assert_eq!(field.child_region_len, field.payload_len, "{label}");
                assert_eq!(
                    step.after_top_region,
                    field.child_region_start.zip(field.child_region_len),
                    "{label}"
                );
                assert!(after.waiting, "{label}");
                assert_eq!(
                    field.slot_occurrences_after, field.slot_occurrences_before,
                    "{label}"
                );
                let active = step
                    .active_frames_after
                    .as_ref()
                    .expect("active source image");
                let rpb3_root_finish_trace::ActiveFrameState::Message { frame: parent, .. } =
                    &active[active.len() - 2]
                else {
                    panic!("message field push lost its suspended parent")
                };
                assert_eq!(parent, after, "{label}");
                let (child_bytes, child_anchors, child_start, child_end) =
                    match active.last().expect("pushed child") {
                        rpb3_root_finish_trace::ActiveFrameState::Message {
                            frame,
                            bytes,
                            anchors,
                        } => (bytes, anchors, frame.region_start, frame.region_end),
                        rpb3_root_finish_trace::ActiveFrameState::NonMessage {
                            frame,
                            bytes,
                            anchors,
                        } => {
                            let (start, end) = match frame {
                                rpb3_root_finish_trace::NonMessageFrameState::Map(value) => {
                                    (value.region_start, value.region_end)
                                }
                                rpb3_root_finish_trace::NonMessageFrameState::EPathMap(value) => {
                                    (value.region_start, value.region_end)
                                }
                                rpb3_root_finish_trace::NonMessageFrameState::CanonicalPath(
                                    value,
                                ) => (value.region_start, value.region_end),
                            };
                            (bytes, anchors, start, end)
                        }
                    };
                let payload_len = field.child_region_len.unwrap();
                let payload_start = field.encoded_range.end - payload_len;
                let expected_start = field
                    .body_positions
                    .get(payload_start)
                    .copied()
                    .unwrap_or(field.body_end_position);
                let expected_end = field
                    .body_positions
                    .get(field.encoded_range.end)
                    .copied()
                    .unwrap_or(field.body_end_position);
                assert_eq!(field.child_region_start, Some(expected_start), "{label}");
                assert_eq!(child_start, expected_start, "{label}");
                assert_eq!(child_end, expected_end, "{label}");
                assert_eq!(
                    child_bytes,
                    &field.body[payload_start..field.encoded_range.end],
                    "{label}"
                );
                assert_eq!(child_anchors.len(), child_bytes.len(), "{label}");
                assert_eq!(
                    child_anchors.as_slice(),
                    &field.body_positions[payload_start..field.encoded_range.end],
                    "{label}"
                );
                assert_eq!(
                    child_anchors.first().copied().or(field.child_region_start),
                    field.child_region_start,
                    "{label}"
                );
            }
            rpb3_root_finish_trace::DriverStepKind::Complete => {
                panic!("message field receipt attached to EOF")
            }
        }
    }
}

#[test]
fn generated_message_field_source_receipts_cover_all_message_types_and_action_kinds() {
    let mut programs = BTreeSet::new();
    let mut tags = BTreeSet::new();
    let mut actions = [0usize; 10];
    let fixtures = schema_fixtures();
    for fixture in &fixtures {
        let label = format!("{}.{}", fixture.message_name, fixture.fixture_name);
        check_run(
            &label,
            &fixture.bytes,
            fixture.program,
            &mut programs,
            &mut tags,
            &mut actions,
        );
    }
    for (label, bytes, program) in [
        (
            "Par",
            protobuf_encoder::encode_to_vec(&gint(7)),
            &PAR_PROTOBUF_REGION_PROGRAM,
        ),
        (
            "EPathMap",
            EPathMap::default().encode_to_vec(),
            &EPATHMAP_RHO_MATCH_PROGRAM,
        ),
    ] {
        check_run(
            label,
            &bytes,
            program,
            &mut programs,
            &mut tags,
            &mut actions,
        );
    }
    for field in PAR_PROTOBUF_REGION_PROGRAM.fields {
        let bytes = match field.kind {
            RhoMatchFieldKind::MessageSequence(type_id) => {
                let child = fixtures
                    .iter()
                    .find(|fixture| fixture.program.type_id == type_id)
                    .expect("every raw Par child has a generated canonical fixture");
                encoded_length_field(field.tag, &child.bytes)
            }
            RhoMatchFieldKind::Bytes => encoded_length_field(field.tag, &[1]),
            RhoMatchFieldKind::Bool => {
                let mut bytes = Vec::new();
                append_varint(u64::from(field.tag) << 3, &mut bytes);
                bytes.push(1);
                bytes
            }
            _ => panic!("raw Par descriptor has a field without a corpus constructor"),
        };
        check_run(
            &format!("Par.{}", field.name),
            &bytes,
            &PAR_PROTOBUF_REGION_PROGRAM,
            &mut programs,
            &mut tags,
            &mut actions,
        );
    }
    let remainder = fixtures
        .iter()
        .find(|fixture| fixture.program.type_id == 6)
        .expect("the generated Var fixture is registered");
    let mut epathmap_bytes = encoded_length_field(3, &[1]);
    epathmap_bytes.extend_from_slice(&[0x20, 1]);
    epathmap_bytes.extend_from_slice(&encoded_length_field(5, &remainder.bytes));
    epathmap_bytes.extend_from_slice(&EPathMap::default().encode_to_vec());
    check_run(
        "EPathMap.all_fields",
        &epathmap_bytes,
        &EPATHMAP_RHO_MATCH_PROGRAM,
        &mut programs,
        &mut tags,
        &mut actions,
    );
    let mut map_entry = encoded_length_field(1, b"name");
    map_entry.extend_from_slice(&encoded_length_field(
        2,
        &protobuf_encoder::encode_to_vec(&gint(3)),
    ));
    check_run(
        "New.explicitMapKey",
        &encoded_length_field(4, &map_entry),
        &NEW_RHO_MATCH_PROGRAM,
        &mut programs,
        &mut tags,
        &mut actions,
    );
    let nested_map = EPathMap::new_map([(gint(1), gint(2))], Vec::new(), false, None);
    let snapshot = nested_map.trie_snapshot();
    let mut nested_key = vec![crate::rust::canonical_path::tag::EPATHMAP_MAP];
    append_varint(snapshot.len() as u64, &mut nested_key);
    nested_key.extend_from_slice(snapshot);
    rpb3_root_finish_trace::start_with_message_success_stream();
    let (summary, _, _) = drive_admission(
        FrameRequest::CanonicalPath(IndexedByteRegion::from_borrowed(&nested_key, 0, 0)),
        nested_key.len(),
    )
    .unwrap();
    let trace = rpb3_root_finish_trace::take();
    assert!(matches!(summary, FrameSummary::CanonicalPath(_)));
    assert_success_stream(&trace, "CanonicalPath.nestedEPathMap");
    count_nonmessage_actions(&trace, &mut actions);
    assert_eq!(programs.len(), RHO_MATCH_REGISTRY.len());
    assert_eq!(programs.len(), 64);
    let mut expected_tags = BTreeSet::new();
    for (name, registered) in RHO_MATCH_REGISTRY {
        let program = if *name == "Par" {
            &PAR_PROTOBUF_REGION_PROGRAM
        } else {
            *registered
        };
        for field in program.fields {
            match field.kind {
                RhoMatchFieldKind::Oneof(arms) => {
                    for arm in arms {
                        expected_tags.insert((program.type_id, arm.tag));
                    }
                }
                _ => {
                    expected_tags.insert((program.type_id, field.tag));
                }
            }
        }
    }
    assert_eq!(expected_tags.len(), 217);
    assert_eq!(tags, expected_tags);
    assert!(
        actions.iter().all(|count| *count > 0),
        "missing field, nonmessage, or typed accepted-child projection class: {actions:?}"
    );
    eprintln!(
        "executed admission coverage: {} types, {} type/tag pairs, scalar/message/map/EPM fields, nonmessage actions, and Message/Map/EPM accepted-child projections {:?}",
        programs.len(), tags.len(), actions
    );
}

#[test]
fn checked_admission_events_cover_canonical_path_root_completion() {
    for payload_len in 0..64usize {
        let mut bytes = vec![6, payload_len as u8];
        bytes.extend((0..payload_len).map(|index| index as u8));
        let region = IndexedByteRegion::from_borrowed(&bytes, 17, 31);
        rpb3_root_finish_trace::start_with_message_success_stream();
        let (summary, _, _) =
            drive_admission(FrameRequest::CanonicalPath(region), bytes.len()).unwrap();
        let trace = rpb3_root_finish_trace::take();
        assert!(matches!(summary, FrameSummary::CanonicalPath(_)));
        assert_success_stream(&trace, "CanonicalPath.root");
        assert_eq!(trace.checked_events.len(), 1);
        assert_eq!(
            trace.checked_events[0].projection,
            CheckedAdmissionProjection::OtherComplete {
                owner: FrameRequestKind::CanonicalPath,
                root: true,
                accepted_message_parent: false,
            }
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_varint_scalar_continue_rejects_coordinated_value_mutation(
        selector in any::<u16>(),
        value_seed in any::<u8>(),
        source in 1u32..1000,
        offset in 0usize..100_000,
    ) {
        let cases = varint_scalar_fields();
        let (program, tag, kind) = cases[usize::from(selector) % cases.len()];
        check_varint_continue_read_binding(program, tag, kind, value_seed, source, offset);
    }

    #[test]
    fn generated_message_receipts_preserve_fragmented_source_projection(
        fixture_selector in any::<u16>(),
        source_base in 1u32..1000,
        offset_base in 0usize..100_000,
    ) {
        let fixtures = schema_fixtures();
        let nonempty = fixtures.iter().filter(|fixture| !fixture.bytes.is_empty())
            .collect::<Vec<_>>();
        let fixture = nonempty[usize::from(fixture_selector) % nonempty.len()];
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (at, &byte) in fixture.bytes.iter().enumerate() {
            let anchor = CanonicalByteProvenance::new(
                source_base + u32::try_from(at % 17).unwrap(),
                offset_base + at * 37,
            );
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let end = CanonicalByteProvenance::new(
            source_base + 100,
            offset_base + fixture.bytes.len() * 37,
        );
        let region = OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), end);
        let mut programs = BTreeSet::new();
        let mut tags = BTreeSet::new();
        let mut actions = [0usize; 10];
        check_run_region(
            fixture.message_name,
            region,
            fixture.program,
            &mut programs,
            &mut tags,
            &mut actions,
        );
        prop_assert!(programs.contains(&fixture.program.type_id));
        prop_assert!(!tags.is_empty());
    }
}
