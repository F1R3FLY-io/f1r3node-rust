//! Opt-in whole-stack observations for short generated RPB3 admissions.
//!
//! The driver trace normally records only its top frame. This property asks
//! for full active-frame images so it can check suspended ancestors across
//! every action and the exact parent slot delta on child return.

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::rholang::protobuf_encoder;

fn generated_par(value: i16) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(value))),
    }])
}

fn assert_message_child_accept(
    before: &rpb3_root_finish_trace::RootFrameState,
    after: &rpb3_root_finish_trace::RootFrameState,
    pop: &rpb3_root_finish_trace::PopAcceptState,
) {
    use rpb3_root_finish_trace::{RootOccurrenceValue, RootWaitState};

    let Some(RootWaitState::Child {
        slot,
        variant,
        body,
        body_start,
        body_end,
        field_position,
    }) = before.waiting_detail.as_ref()
    else {
        panic!("generated Send/Par child return resumes a message wait");
    };
    assert!(before.waiting);
    assert!(!after.waiting);
    assert!(after.waiting_detail.is_none());
    assert_eq!(before.program_type_id, after.program_type_id);
    assert_eq!(pop.child_body.as_deref(), Some(body.as_slice()));
    assert_eq!(
        pop.child_frame
            .as_ref()
            .expect("message child frame")
            .region_start,
        *body_start,
    );
    assert_eq!(
        pop.child_frame
            .as_ref()
            .expect("message child frame")
            .region_end,
        *body_end,
    );
    assert!(before.program.fields.get(*slot).is_some());
    let prior = &before.semantic_slots[*slot];
    let accepted = &after.semantic_slots[*slot];
    assert_eq!(accepted.len(), prior.len() + 1);
    assert_eq!(&accepted[..prior.len()], prior.as_slice());
    let new = accepted
        .last()
        .expect("child acceptance appends one occurrence");
    assert_eq!(new.slot, *slot);
    assert_eq!(new.variant, *variant);
    assert_eq!(new.field_position, *field_position);
    assert_eq!(new.body.as_deref(), Some(body.as_slice()));
    assert_eq!(new.body_start, Some(*body_start));
    assert_eq!(new.body_end, Some(*body_end));
    assert_eq!(
        new.value,
        RootOccurrenceValue::Message(Box::new(
            pop.typed_summary
                .clone()
                .expect("message child typed summary")
        )),
    );

    // Construct the only permitted parent delta from the independently
    // observed child wait and returned summary. Equality checks every cursor,
    // byte boundary, seen bit, and nonselected semantic slot in one step.
    let mut permitted = before.clone();
    permitted.waiting = false;
    permitted.waiting_detail = None;
    permitted.semantic_slots[*slot].push(new.clone());
    assert_eq!(&permitted, after);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn nested_send_preserves_suspended_frames_through_every_driver_action(
        channel in any::<i16>(),
        first_data in any::<i16>(),
        second_data in any::<i16>(),
        persistent in any::<bool>(),
    ) {
        let send = Send {
            chan: Some(generated_par(channel)),
            data: vec![generated_par(first_data), generated_par(second_data)],
            persistent,
            ..Send::default()
        };
        let bytes = protobuf_encoder::encode_to_vec(&send);
        rpb3_root_finish_trace::start_with_active_frames();
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &SEND_RHO_MATCH_PROGRAM,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(admission.is_ok(), "generated Send rejected: {:?}", admission.as_ref().err());

        // The generated descriptor and independent wire oracle establish the
        // meanings of the observed field slots and child bodies.
        rpb3_root_finish_trace::assert_descriptor_driver_projection(
            &trace,
            &SEND_RHO_MATCH_PROGRAM,
            "whole-stack generated Send",
        );

        let mut saw_deep_continue = false;
        let mut saw_deep_push = false;
        let mut saw_nested_accept = false;
        let mut pop_at = 0;
        prop_assert!(!trace.driver_steps.is_empty());
        for (at, step) in trace.driver_steps.iter().enumerate() {
            let before = step.active_frames_before.as_ref().expect("opt-in before stack");
            let after = step.active_frames_after.as_ref().expect("opt-in after stack");
            prop_assert_eq!(before.len(), step.before_len);
            prop_assert_eq!(after.len(), step.after_len);
            if at > 0 {
                let previous = trace.driver_steps[at - 1]
                    .active_frames_after.as_ref().expect("prior after stack");
                prop_assert_eq!(previous.as_slice(), before.as_slice());
            }
            match &step.kind {
                rpb3_root_finish_trace::DriverStepKind::Continue => {
                    prop_assert_eq!(after.len(), before.len());
                    prop_assert_eq!(&before[..before.len() - 1], &after[..after.len() - 1]);
                    saw_deep_continue |= before.len() >= 3;
                }
                rpb3_root_finish_trace::DriverStepKind::Push(_) => {
                    prop_assert_eq!(after.len(), before.len() + 1);
                    prop_assert_eq!(&before[..before.len() - 1], &after[..before.len() - 1]);
                    saw_deep_push |= before.len() >= 2;
                }
                rpb3_root_finish_trace::DriverStepKind::Complete => {
                    let pop = &trace.pop_accepts[pop_at];
                    pop_at += 1;
                    prop_assert_eq!(pop.before_len, before.len());
                    prop_assert_eq!(pop.after_len, Some(after.len()));
                    prop_assert_eq!(after.len() + 1, before.len());
                    if after.is_empty() {
                        prop_assert_eq!(before.len(), 1);
                    } else {
                        prop_assert_eq!(&before[..before.len() - 2], &after[..after.len() - 1]);
                        let rpb3_root_finish_trace::ActiveFrameState::Message { frame: parent_before, bytes: before_bytes, anchors: before_anchors } =
                            &before[before.len() - 2] else {
                                panic!("generated Send has only message-frame parents");
                            };
                        let rpb3_root_finish_trace::ActiveFrameState::Message { frame: parent_after, bytes: after_bytes, anchors: after_anchors } =
                            &after[after.len() - 1] else {
                                panic!("generated Send resumes a message frame");
                            };
                        prop_assert_eq!(before_bytes, after_bytes);
                        prop_assert_eq!(before_anchors, after_anchors);
                        assert_message_child_accept(parent_before, parent_after, pop);
                        saw_nested_accept |= before.len() >= 3;
                    }
                }
            }
        }
        prop_assert_eq!(pop_at, trace.pop_accepts.len());
        prop_assert!(saw_deep_continue && saw_deep_push && saw_nested_accept);
        prop_assert_eq!(trace.driver_steps.first().unwrap().before_len, 1);
        prop_assert!(trace.driver_steps.last().unwrap()
            .active_frames_after.as_ref().unwrap().is_empty());
    }
}
