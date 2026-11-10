//! Whole-stack preservation while generated EPathMap map values resolve.
//!
//! This fixture forces nonmessage frames to suspend across nested map-value
//! admission. Whole-stack images are captured only for this bounded test.

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::protobuf_encoder;

fn gint(value: i16) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(value))),
    }])
}

fn fragmented_root(bytes: &[u8], source_base: u32) -> OwnedIndexedByteRegion {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (index, &byte) in bytes.iter().enumerate() {
        let anchor =
            CanonicalByteProvenance::new(source_base + (index % 31) as u32, 43 + index * 17);
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, anchor))
            .expect("bounded nested EPathMap source fits");
    }
    OwnedIndexedByteRegion::from_frozen(
        arena.freeze(handle),
        CanonicalByteProvenance::new(900, 30000),
    )
}

fn source_image(
    frame: &rpb3_root_finish_trace::ActiveFrameState,
) -> (&[u8], &[CanonicalByteProvenance]) {
    match frame {
        rpb3_root_finish_trace::ActiveFrameState::Message { bytes, anchors, .. }
        | rpb3_root_finish_trace::ActiveFrameState::NonMessage { bytes, anchors, .. } => {
            (bytes, anchors)
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn nested_epathmap_map_values_preserve_active_ancestor_images(
        inner_first in any::<i16>(),
        inner_second in any::<i16>(),
        outer_key in any::<i16>(),
        source_base in 100u32..200,
    ) {
        let inner = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [(gint(1), gint(inner_first)), (gint(2), gint(inner_second))],
                Vec::new(), false, None,
            ))),
        };
        let outer = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [(gint(outer_key), Par::default().with_exprs(vec![inner]))],
                Vec::new(), false, None,
            ))),
        };
        let bytes = protobuf_encoder::encode_to_vec(&outer);
        rpb3_root_finish_trace::start_with_active_frames();
        let result = validate_canonical_protobuf_inner(
            fragmented_root(&bytes, source_base),
            &EXPR_RHO_MATCH_PROGRAM,
            None,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(result.is_ok(), "nested EPathMap rejected: {:?}", result.as_ref().err());
        prop_assert_eq!(result.unwrap().stats().epathmaps, 2);
        rpb3_root_finish_trace::assert_descriptor_driver_projection(
            &trace,
            &EXPR_RHO_MATCH_PROGRAM,
            "nested EPathMap active frames",
        );

        let mut saw_nonmessage_push = false;
        let mut saw_child_return_to_nonmessage = false;
        let mut saw_nonmessage_complete = false;
        let mut message_epm_pushes = 0usize;
        prop_assert!(!trace.driver_steps.is_empty());
        for (index, step) in trace.driver_steps.iter().enumerate() {
            let before = step.active_frames_before.as_ref().expect("opt-in before stack");
            let after = step.active_frames_after.as_ref().expect("opt-in after stack");
            prop_assert_eq!(before.len(), step.before_len);
            prop_assert_eq!(after.len(), step.after_len);
            if index > 0 {
                prop_assert_eq!(
                    trace.driver_steps[index - 1].active_frames_after.as_ref().unwrap(),
                    before,
                );
            }
            match &step.kind {
                rpb3_root_finish_trace::DriverStepKind::Continue => {
                    prop_assert_eq!(after.len(), before.len());
                    prop_assert_eq!(&before[..before.len() - 1], &after[..after.len() - 1]);
                    prop_assert_eq!(source_image(before.last().unwrap()), source_image(after.last().unwrap()));
                }
                rpb3_root_finish_trace::DriverStepKind::Push(_) => {
                    prop_assert_eq!(after.len(), before.len() + 1);
                    prop_assert_eq!(&before[..before.len() - 1], &after[..before.len() - 1]);
                    prop_assert_eq!(source_image(before.last().unwrap()), source_image(&after[before.len() - 1]));
                    saw_nonmessage_push |= matches!(before.last(),
                        Some(rpb3_root_finish_trace::ActiveFrameState::NonMessage { .. }));
                    if let (
                        Some(rpb3_root_finish_trace::ActiveFrameState::Message { .. }),
                        Some(rpb3_root_finish_trace::ActiveFrameState::NonMessage {
                            frame: rpb3_root_finish_trace::NonMessageFrameState::EPathMap(child),
                            bytes: child_bytes,
                            anchors: child_anchors,
                        }),
                    ) = (before.last(), after.last()) {
                        message_epm_pushes += 1;
                        let field = trace.message_fields.iter()
                            .find(|field| field.step_index == index)
                            .expect("message EPathMap push has a field observation");
                        prop_assert_eq!(field.wire, CanonicalWireType::LengthDelimited);
                        let payload_len = field.payload_len
                            .expect("EPathMap field has a length-delimited payload");
                        let payload_end = field.encoded_range.end;
                        let payload_start = payload_end.checked_sub(payload_len)
                            .expect("payload is within the encoded field");
                        prop_assert!(payload_start >= field.encoded_range.start);
                        prop_assert!(payload_end <= field.body.len());
                        prop_assert_eq!(child_bytes.as_slice(),
                            &field.body[payload_start..payload_end]);
                        prop_assert_eq!(child_anchors.as_slice(),
                            &field.body_positions[payload_start..payload_end]);
                        let expected_start = field.body_positions.get(payload_start)
                            .copied().unwrap_or(field.body_end_position);
                        let expected_end = field.body_positions.get(payload_end)
                            .copied().unwrap_or(field.body_end_position);
                        prop_assert_eq!(child.region_start, expected_start);
                        prop_assert_eq!(child.region_end, expected_end);
                        prop_assert_eq!(step.after_top_region,
                            Some((expected_start, payload_len)));
                        prop_assert!(std::ptr::eq(child.owner, field.program));
                        prop_assert_eq!(child.owner.type_id, field.program.type_id);
                        prop_assert_eq!(child.field, field.tag);
                        prop_assert_eq!(step.after_top_kind,
                            Some(FrameRequestKind::EPathMap));
                        prop_assert!(step.after_top_program.is_none());
                        let rpb3_root_finish_trace::ActiveFrameState::Message {
                            frame: parent_after, ..
                        } = &after[before.len() - 1] else {
                            panic!("EPathMap push retains its message parent");
                        };
                        let Some(rpb3_root_finish_trace::RootWaitState::EPathMap {
                            slot, field_position,
                        }) = &parent_after.waiting_detail else {
                            panic!("EPathMap push retains its matching parent wait");
                        };
                        prop_assert_eq!(*slot, field.slot);
                        prop_assert_eq!(*field_position, field.field_position);
                    }
                }
                rpb3_root_finish_trace::DriverStepKind::Complete => {
                    prop_assert_eq!(after.len() + 1, before.len());
                    if after.is_empty() {
                        prop_assert_eq!(before.len(), 1);
                    } else {
                        prop_assert_eq!(&before[..before.len() - 2], &after[..after.len() - 1]);
                        prop_assert_eq!(
                            source_image(&before[before.len() - 2]),
                            source_image(after.last().unwrap()),
                        );
                        saw_child_return_to_nonmessage |= matches!(after.last(),
                            Some(rpb3_root_finish_trace::ActiveFrameState::NonMessage { .. }));
                    }
                    saw_nonmessage_complete |= matches!(before.last(),
                        Some(rpb3_root_finish_trace::ActiveFrameState::NonMessage { .. }));
                }
            }
        }
        prop_assert!(saw_nonmessage_push);
        prop_assert!(message_epm_pushes > 0);
        prop_assert!(saw_child_return_to_nonmessage);
        prop_assert!(saw_nonmessage_complete);
        prop_assert_eq!(trace.driver_steps.first().unwrap().before_len, 1);
        prop_assert!(trace.driver_steps.last().unwrap()
            .active_frames_after.as_ref().unwrap().is_empty());
    }
}
