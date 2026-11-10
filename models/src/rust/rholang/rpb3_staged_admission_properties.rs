use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::rholang::protobuf_encoder;

fn generated_send(value: i16) -> Send {
    Send {
        chan: Some(Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(value))),
        }])),
        persistent: true,
        ..Send::default()
    }
}

fn assert_checked_prefix(trace: &rpb3_root_finish_trace::RootFinishTrace) {
    assert!(trace.checked_events.len() <= trace.driver_steps.len());
    if let Some(first) = trace.driver_steps.first() {
        assert_eq!(first.before_len, 1);
    }
    for (index, checked) in trace.checked_events.iter().enumerate() {
        let step = &trace.driver_steps[index];
        if index > 0 {
            let previous = &trace.driver_steps[index - 1];
            assert_eq!(
                previous.active_frames_after.as_ref().unwrap(),
                step.active_frames_before.as_ref().unwrap()
            );
            assert_eq!(previous.score_len_after, step.score_len_before);
        }
        assert_eq!(checked.step_index, index);
        assert_eq!(checked.kind, step.kind);
        assert_eq!(checked.before_len, step.before_len);
        assert_eq!(checked.after_len, step.after_len);
        assert_eq!(checked.before_top_kind, step.before_top_kind);
        assert_eq!(checked.after_top_kind, step.after_top_kind);
        assert_eq!(checked.score_len_before, step.score_len_before);
        assert_eq!(checked.score_len_after, step.score_len_after);

        let before = step.active_frames_before.as_ref().unwrap();
        let after = step.active_frames_after.as_ref().unwrap();
        assert_eq!(before.len(), step.before_len);
        assert_eq!(after.len(), step.after_len);
        match step.kind {
            rpb3_root_finish_trace::DriverStepKind::Continue => {
                assert_eq!(after.len(), before.len());
                assert_eq!(&after[..before.len() - 1], &before[..before.len() - 1]);
            }
            rpb3_root_finish_trace::DriverStepKind::Push(pushed) => {
                assert_eq!(after.len(), before.len() + 1);
                assert_eq!(&after[..before.len() - 1], &before[..before.len() - 1]);
                assert_eq!(pushed.before_len, before.len());
                assert_eq!(pushed.after_len, after.len());
                assert_eq!(Some(pushed.request_kind), step.after_top_kind);
            }
            rpb3_root_finish_trace::DriverStepKind::Complete => {
                assert_eq!(after.len() + 1, before.len());
                let retained_ancestors = after.len().saturating_sub(1);
                assert_eq!(&after[..retained_ancestors], &before[..retained_ancestors]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_staged_admission_event_order_push_and_eof(value in any::<i16>()) {
        let bytes = protobuf_encoder::encode_to_vec(&generated_send(value));
        rpb3_root_finish_trace::start_with_message_success_stream();
        let result = validate_canonical_protobuf_inner(
            IndexedByteRegion::from_borrowed(&bytes, 0, 0),
            &SEND_RHO_MATCH_PROGRAM,
            None,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(result.is_ok());
        assert_checked_prefix(&trace);
        prop_assert_eq!(trace.checked_events.len(), trace.driver_steps.len());
        prop_assert!(trace.returned.is_some());
        prop_assert_eq!(trace.remaining_frames, Some(0));
        prop_assert_eq!(trace.driver_steps.last().unwrap().after_len, 0);

        let mut pop_index = 0;
        for (index, step) in trace.driver_steps.iter().enumerate() {
            if matches!(step.kind, rpb3_root_finish_trace::DriverStepKind::Complete) {
                let pop = &trace.pop_accepts[pop_index];
                pop_index += 1;
                prop_assert_eq!(pop.before_len, step.before_len);
                prop_assert_eq!(pop.after_len, Some(step.after_len));
                if step.before_message.is_some() {
                    let eof = trace.message_eofs.iter()
                        .find(|eof| eof.step_index == index)
                        .expect("message completion has a checked EOF");
                    prop_assert_eq!(&eof.frame, step.before_message.as_ref().unwrap());
                    prop_assert_eq!(eof.source_len, step.score_len_before);
                }
                if step.after_len == 0 {
                    prop_assert!(pop.parent_kind.is_none());
                } else {
                    prop_assert_eq!(pop.parent_kind, step.after_top_kind);
                }
            }
        }
        prop_assert_eq!(pop_index, trace.pop_accepts.len());
    }

    #[test]
    fn generated_staged_admission_error_has_no_root_publication(value in any::<i16>()) {
        let mut bytes = protobuf_encoder::encode_to_vec(&generated_send(value));
        prop_assert_eq!(bytes.pop(), Some(1));
        prop_assert_eq!(bytes.last(), Some(&0x18));
        rpb3_root_finish_trace::start_with_message_success_stream();
        let result = validate_canonical_protobuf_inner(
            IndexedByteRegion::from_borrowed(&bytes, 0, 0),
            &SEND_RHO_MATCH_PROGRAM,
            None,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(result.is_err());
        assert_checked_prefix(&trace);
        prop_assert!(trace.returned.is_none());
        prop_assert!(trace.checked_events.iter().all(|event| event.after_len > 0));
    }
}
