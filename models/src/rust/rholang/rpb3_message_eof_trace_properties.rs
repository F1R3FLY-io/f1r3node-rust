//! Generated source replay for a message frame's field, return, and EOF path.
//!
//! The test-only driver records the actual indexed field result, parent
//! continuation, borrowed EOF frame, and append-only score arena. Replaying
//! those records establishes the shape expected by the formal generic-message
//! EOF fold. It is executable extraction evidence, not a universal Rust proof.

use proptest::prelude::*;
use rpb3_root_finish_trace::{
    DriverStepKind, MessageFieldState, RootFinishTrace, RootOccurrenceValue, RootWaitState,
};

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::rholang::protobuf_encoder;

fn generated_par(value: i16) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(value))),
    }])
}

fn replay_field(
    before: &rpb3_root_finish_trace::RootFrameState,
    after: &rpb3_root_finish_trace::RootFrameState,
    field: &MessageFieldState,
    pushed: bool,
) {
    let descriptor = resolve_protobuf_field(before.program, field.tag)
        .expect("recorded field has a generated descriptor");
    let slot = descriptor.slot();
    assert_eq!(slot, field.slot);
    assert!(slot < before.semantic_slots.len());
    assert!(!before.waiting);
    assert_eq!(field.program.type_id, before.program_type_id);
    assert_eq!(field.frame_after, *after);
    assert_eq!(field.body.len(), field.body_positions.len());
    assert_eq!(field.encoded_range.start, before.cursor_bytes_read);
    assert!(field.encoded_range.start < field.encoded_range.end);
    assert!(field.encoded_range.end <= field.body.len());
    assert_eq!(field.cursor_before.bytes_read, before.cursor_bytes_read);
    assert_eq!(field.cursor_after.bytes_read, field.encoded_range.end);
    assert_eq!(field.cursor_before.fields_read, before.cursor_fields_read);
    assert_eq!(
        field.cursor_after.fields_read,
        before.cursor_fields_read + 1
    );
    assert_eq!(
        field.body_positions[field.encoded_range.start],
        field.indexed_field_position,
    );
    assert_eq!(field.indexed_field_position, field.field_position);
    assert!(before.last_slot.is_none_or(|last| last <= slot));
    assert!(descriptor.repeated() || !before.seen_slots.contains(&slot));

    let mut expected = before.clone();
    expected.cursor_empty = field.encoded_range.end == field.body.len();
    expected.cursor_fields_read = field.cursor_after.fields_read;
    expected.cursor_bytes_read = field.cursor_after.bytes_read;
    expected.cursor_bytes_traversed = field.cursor_after.bytes_traversed;
    expected.cursor_bytes_fast_forwarded = field.cursor_after.bytes_fast_forwarded;
    expected.last_slot = Some(slot);
    if !expected.seen_slots.contains(&slot) {
        expected.seen_slots.push(slot);
    }
    if pushed {
        assert_eq!(after.semantic_slots[slot], before.semantic_slots[slot]);
        let RootWaitState::Child {
            slot: waiting_slot,
            variant,
            body,
            body_start,
            body_end,
            field_position,
            ..
        } = after
            .waiting_detail
            .as_ref()
            .expect("generated Send/Par push retains child request")
        else {
            panic!("generated Send/Par push retained another wait kind");
        };
        assert_eq!(*waiting_slot, slot);
        assert_eq!(*field_position, field.field_position);
        let expected_variant = match before.program.fields[slot].kind {
            RhoMatchFieldKind::Oneof(arms) => arms
                .iter()
                .find(|arm| arm.tag == field.tag)
                .map(|arm| arm.name),
            _ => None,
        };
        assert_eq!(*variant, expected_variant);
        assert_eq!(Some(body.len()), field.payload_len);
        assert_eq!(Some(*body_start), field.payload_position);
        let payload_start = field.encoded_range.end - body.len();
        assert_eq!(
            body.as_slice(),
            &field.body[payload_start..field.encoded_range.end]
        );
        assert_eq!(
            *body_end,
            field
                .body_positions
                .get(field.encoded_range.end)
                .copied()
                .unwrap_or(field.body_end_position)
        );
        expected.waiting = true;
        expected.waiting_detail = after.waiting_detail.clone();
    } else {
        let old_len = before.semantic_slots[slot].len();
        assert_eq!(after.semantic_slots[slot].len(), old_len + 1);
        let appended = after.semantic_slots[slot][old_len].clone();
        assert_eq!(appended.slot, slot);
        assert_eq!(appended.field_position, field.field_position);
        assert!(appended.body.is_none());
        assert!(matches!(&appended.value, RootOccurrenceValue::Scalar(_)));
        let expected_variant = match before.program.fields[slot].kind {
            RhoMatchFieldKind::Oneof(arms) => arms
                .iter()
                .find(|arm| arm.tag == field.tag)
                .map(|arm| arm.name),
            _ => None,
        };
        assert_eq!(appended.variant, expected_variant);
        expected.semantic_slots[slot].push(appended);
    }
    assert_eq!(expected, *after);
}

fn replay_message_trace(trace: &RootFinishTrace) {
    let final_scores = trace
        .score_arena
        .as_ref()
        .expect("successful root records score arena");
    let transcripts = rpb3_root_finish_trace::replay_score_source(final_scores);
    let mut fields = 0;
    let mut eof_count = 0;
    let mut accepts = 0;
    let mut saw_continue = false;
    let mut saw_push = false;
    let mut saw_score_append = false;

    for (at, step) in trace.driver_steps.iter().enumerate() {
        let before_stack = step
            .active_frames_before
            .as_ref()
            .expect("full pre-step stack");
        let after_stack = step
            .active_frames_after
            .as_ref()
            .expect("full post-step stack");
        assert_eq!(before_stack.len(), step.before_len);
        assert_eq!(after_stack.len(), step.after_len);
        assert!(step.before_len <= step.before_capacity);
        assert!(step.after_len <= step.after_capacity);
        if at > 0 {
            assert_eq!(
                trace.driver_steps[at - 1].score_len_after,
                step.score_len_before,
                "the append-only arena must be continuous across driver actions",
            );
            assert_eq!(
                trace.driver_steps[at - 1]
                    .active_frames_after
                    .as_ref()
                    .unwrap(),
                before_stack,
                "driver source images must form one continuous run",
            );
        }
        assert_eq!(step.before_top_kind, FrameRequestKind::Message);
        assert!(step.score_len_before <= step.score_len_after);
        assert!(step.score_len_after <= final_scores.len());
        assert_eq!(
            step.score_appended.as_slice(),
            &final_scores[step.score_len_before..step.score_len_after],
        );
        saw_score_append |= !step.score_appended.is_empty();
        if let Some(image) = &step.score_source_before {
            assert_eq!(image.as_slice(), &final_scores[..step.score_len_before]);
        }
        if let Some(image) = &step.score_source_after {
            assert_eq!(image.as_slice(), &final_scores[..step.score_len_after]);
        }

        match step.kind {
            DriverStepKind::Continue | DriverStepKind::Push(_) => {
                assert_eq!(step.score_len_before, step.score_len_after);
                assert!(step.score_appended.is_empty());
                let before = step
                    .before_message
                    .as_ref()
                    .expect("message field pre-state");
                let field = &trace.message_fields[fields];
                fields += 1;
                assert_eq!(field.step_index, at);
                let after = if matches!(step.kind, DriverStepKind::Continue) {
                    assert_eq!(step.after_len, step.before_len);
                    assert_eq!(step.after_capacity, step.before_capacity);
                    assert_eq!(
                        step.score_source_before
                            .as_ref()
                            .expect("Continue arena source"),
                        step.score_source_after
                            .as_ref()
                            .expect("Continue arena result"),
                    );
                    step.after_message
                        .as_ref()
                        .expect("message continue post-state")
                } else {
                    assert_eq!(step.after_len, step.before_len + 1);
                    assert!(step.after_capacity >= step.before_capacity);
                    let DriverStepKind::Push(observed) = step.kind else {
                        unreachable!();
                    };
                    assert_eq!(observed.request_kind, FrameRequestKind::Message);
                    assert_eq!(observed.before_len, step.before_len);
                    assert_eq!(observed.before_capacity, step.before_capacity);
                    assert_eq!(observed.after_len, step.after_len);
                    assert_eq!(observed.after_capacity, step.after_capacity);
                    assert_eq!(
                        observed.frame_variant_bytes,
                        observed.stats_frame_variant_bytes
                    );
                    assert!(observed.used_bytes <= observed.reserved_bytes);
                    let rpb3_root_finish_trace::ActiveFrameState::Message { frame, .. } =
                        &after_stack[before_stack.len() - 1]
                    else {
                        panic!("message push left a nonmessage parent");
                    };
                    frame
                };
                let rpb3_root_finish_trace::ActiveFrameState::Message {
                    frame: active_before,
                    bytes,
                    anchors,
                } = before_stack.last().unwrap()
                else {
                    panic!("field action did not start at a message");
                };
                assert_eq!(active_before, before);
                assert_eq!(&field.body, bytes);
                assert_eq!(&field.body_positions, anchors);
                if let DriverStepKind::Push(_) = step.kind {
                    let rpb3_root_finish_trace::ActiveFrameState::Message {
                        frame: child,
                        bytes: child_bytes,
                        anchors: child_anchors,
                    } = after_stack.last().unwrap()
                    else {
                        panic!("generated message push did not publish a message child");
                    };
                    let RootWaitState::Child {
                        body,
                        body_start,
                        body_end,
                        ..
                    } = after
                        .waiting_detail
                        .as_ref()
                        .expect("parent retains child request")
                    else {
                        panic!("generated message push retained another wait kind");
                    };
                    assert_eq!(child_bytes, body);
                    assert_eq!(child.region_start, *body_start);
                    assert_eq!(child.region_end, *body_end);
                    assert_eq!(child.region_len, body.len());
                    assert_eq!(child_anchors.len(), body.len());
                    assert_eq!(step.after_top_region, Some((*body_start, body.len())));
                }
                replay_field(
                    before,
                    after,
                    field,
                    matches!(step.kind, DriverStepKind::Push(_)),
                );
                saw_continue |= matches!(step.kind, DriverStepKind::Continue);
                saw_push |= matches!(step.kind, DriverStepKind::Push(_));
            }
            DriverStepKind::Complete => {
                assert_eq!(step.after_len + 1, step.before_len);
                assert_eq!(step.after_capacity, step.before_capacity);
                let before = step.before_message.as_ref().expect("message EOF pre-state");
                let rpb3_root_finish_trace::ActiveFrameState::Message {
                    frame: active_before,
                    bytes: active_body,
                    anchors: active_anchors,
                } = before_stack.last().unwrap()
                else {
                    panic!("message EOF did not start from a message frame");
                };
                assert_eq!(active_before, before);
                assert_eq!(active_body.len(), before.region_len);
                assert_eq!(active_anchors.len(), active_body.len());
                let eof = &trace.message_eofs[eof_count];
                let complete = &trace.message_completes[eof_count];
                eof_count += 1;
                assert_eq!(eof.step_index, at);
                assert_eq!(complete.step_index, at);
                assert_eq!(&eof.frame, before);
                assert_eq!(&complete.frame, before);
                assert!(!before.waiting);
                assert!(before.cursor_empty);
                assert_eq!(before.cursor_bytes_read, before.region_len);
                assert_eq!(eof.source_len, step.score_len_before);
                assert_eq!(complete.source_len, step.score_len_after);
                let pop = &trace.pop_accepts[accepts];
                accepts += 1;
                assert_eq!(pop.before_len, step.before_len);
                assert_eq!(pop.after_len, Some(step.after_len));
                assert_eq!(pop.source_len_before, step.score_len_after);
                assert_eq!(pop.source_len_after, Some(step.score_len_after));
                assert_eq!(pop.parent_after, step.after_message);
                assert_eq!(pop.child_body.as_ref(), Some(active_body));
                assert_eq!(pop.child_frame.as_ref(), Some(before));
                assert!(std::ptr::eq(
                    pop.child_program.expect("completed message program"),
                    before.program,
                ));
                assert_eq!(pop.typed_summary.as_ref(), Some(&complete.summary));
                if let Some(summary) = &pop.summary {
                    if let Some(handle) = summary.score_handle {
                        assert!(handle.index() < step.score_len_after);
                        assert_eq!(
                            summary.score_bytes.as_deref(),
                            Some(transcripts[handle.index()].as_slice()),
                        );
                    }
                }
                if let Some(parent_before) = &pop.parent_before {
                    let parent_after = pop.parent_after.as_ref().expect("accepted parent image");
                    let RootWaitState::Child {
                        slot,
                        variant,
                        body,
                        body_start,
                        body_end,
                        field_position,
                        ..
                    } = parent_before
                        .waiting_detail
                        .as_ref()
                        .expect("parent waits for generated child")
                    else {
                        panic!("generated Send/Par parent waited for another frame kind");
                    };
                    assert_eq!(pop.child_body.as_deref(), Some(body.as_slice()));
                    assert_eq!(
                        parent_after.cursor_bytes_read,
                        parent_before.cursor_bytes_read
                    );
                    assert_eq!(parent_after.seen_slots, parent_before.seen_slots);
                    assert_eq!(parent_after.last_slot, parent_before.last_slot);
                    assert_eq!(
                        parent_after.semantic_slots[*slot].len(),
                        parent_before.semantic_slots[*slot].len() + 1
                    );
                    let returned = parent_after.semantic_slots[*slot].last().unwrap();
                    assert_eq!(returned.field_position, *field_position);
                    assert_eq!(returned.body.as_deref(), Some(body.as_slice()));
                    assert_eq!(returned.body_start, Some(*body_start));
                    assert_eq!(returned.body_end, Some(*body_end));
                    assert_eq!(returned.variant, *variant);
                    let child_frame = pop.child_frame.as_ref().expect("accepted message frame");
                    assert_eq!(child_frame.region_start, *body_start);
                    assert_eq!(child_frame.region_end, *body_end);
                    assert_eq!(child_frame.region_len, body.len());
                    assert!(std::ptr::eq(
                        pop.child_program.expect("accepted message program"),
                        child_frame.program,
                    ));
                    assert_eq!(
                        returned.value,
                        RootOccurrenceValue::Message(Box::new(complete.summary.clone()))
                    );
                    let mut expected = parent_before.clone();
                    expected.waiting = false;
                    expected.waiting_detail = None;
                    expected.semantic_slots[*slot].push(returned.clone());
                    assert_eq!(expected, *parent_after);
                }
            }
        }
    }
    assert_eq!(fields, trace.message_fields.len());
    assert_eq!(eof_count, trace.message_eofs.len());
    assert_eq!(eof_count, trace.message_completes.len());
    assert_eq!(accepts, trace.pop_accepts.len());
    assert!(saw_continue && saw_push && saw_score_append);
    assert_eq!(trace.driver_steps[0].score_len_before, 0);
    assert_eq!(
        trace.driver_steps.last().unwrap().score_len_after,
        final_scores.len()
    );
    assert_eq!(trace.remaining_frames, Some(0));
    assert_eq!(trace.pre_pop, trace.returned);
    assert!(after_root_empty(trace));
}

fn after_root_empty(trace: &RootFinishTrace) -> bool {
    trace
        .driver_steps
        .last()
        .is_some_and(|step| step.active_frames_after.as_ref().is_some_and(Vec::is_empty))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn generated_send_replays_source_fields_accepts_eof_and_score_delta(
        channel in any::<i16>(),
        first_data in any::<i16>(),
        second_data in any::<i16>(),
        persistent in any::<bool>(),
    ) {
        for channel_present in [false, true] {
            let send = Send {
                chan: channel_present.then(|| generated_par(channel)),
                data: vec![generated_par(first_data), generated_par(second_data)],
                persistent,
                ..Send::default()
            };
            let bytes = protobuf_encoder::encode_to_vec(&send);
            rpb3_root_finish_trace::start_with_active_frames();
            let admitted = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &SEND_RHO_MATCH_PROGRAM,
            );
            let trace = rpb3_root_finish_trace::take();
            prop_assert!(admitted.is_ok(), "generated Send rejected: {:?}", admitted.as_ref().err());
            let admitted = admitted.expect("checked generated Send admission");
            prop_assert_eq!(trace.send_finishes.len(), 1);
            let finish = &trace.send_finishes[0];
            prop_assert_eq!(
                matches!(&finish.input, SendFinishInput::Complete { .. }),
                channel_present,
            );
            prop_assert_eq!(finish.body.as_slice(), bytes.as_slice());
            prop_assert_eq!(finish.complete, channel_present);
            let returned = trace.returned.as_ref().expect("generated Send returns a summary");
            let component = returned.component.as_ref().expect("Send returns a component");
            prop_assert_eq!(component.family, ParComponentTag::Send);
            prop_assert_eq!(component.complete, channel_present);
            let root_handle = returned.score_handle.expect("Send returns a score").index();
            prop_assert_eq!(finish.score_handle.index(), root_handle);
            let entries = trace.score_arena.as_ref().expect("Send retains the score arena");
            let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
            rpb3_root_finish_trace::assert_send_finish_source(
                finish, entries, &transcripts, "generated Message EOF replay",
            );
            prop_assert_eq!(returned.score_bytes.as_deref(), Some(transcripts[root_handle].as_slice()));
            prop_assert_eq!(admitted.score_bytes(), Some(transcripts[root_handle].as_slice()));
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace, &SEND_RHO_MATCH_PROGRAM, "generated Message EOF replay",
            );
            replay_message_trace(&trace);
        }
    }
}
