use proptest::prelude::*;

use super::*;
use crate::rhoapi::{New, Send};
use crate::rust::rholang::protobuf_encoder;

struct ScalarSourcePrefixEntry<'src> {
    variant: Option<&'static str>,
    value: SemanticScalar<'src>,
    field_position: CanonicalByteProvenance,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_new_map_push_binds_the_entire_message_preimage(
        key in 0x21u8..0x7f,
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = [0x22, 0x03, 0x0a, 0x01, key];
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = Vec::new();
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            ..CanonicalProtobufAdmissionStats::default()
        };
        push_admission_frame(
            &mut frames,
            FrameRequest::Message {
                region,
                program: &NEW_RHO_MATCH_PROGRAM,
            },
            &mut stats,
        ).unwrap();
        let ticket = admission_live_core::PreStepTicket::before(&frames, &scores, &stats).unwrap();
        let mut scratch = AdmissionScratch::allocate();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let kind = frames.last_mut().unwrap().step_into(
            &mut stats, &mut scores, &mut scratch,
        ).unwrap();
        prop_assert!(matches!(kind, AdmissionStepKind::MessagePush));
        process_admission_noncompletion(
            kind, &mut scratch, &mut frames, &scores, &mut stats,
        ).unwrap();
        let field = scratch.field.as_ref().unwrap();
        let pushed = scratch.pushed.as_ref().unwrap();
        let selected_slot = field.resolved.slot();
        let other_slot = (0..scratch.field_preimage.seen.len())
            .find(|slot| *slot != selected_slot).unwrap();
        prop_assert!(scratch.field_preimage.matches_checked_push_receipt(field, &frames, pushed));
        prop_assert!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ).is_ok());

        scratch.field_preimage.seen[other_slot] = true;
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message push iteration disagrees with its source image"
        ))));
        scratch.field_preimage.seen[other_slot] = false;

        scratch.field_preimage.slot_lengths[other_slot] += 1;
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message push iteration disagrees with its source image"
        ))));
        scratch.field_preimage.slot_lengths[other_slot] -= 1;

        let Frame::Message(parent) = &mut frames[pushed.before_len - 1] else {
            unreachable!()
        };
        let Some(MessageWait::Map { slot, .. }) = &mut parent.waiting else {
            unreachable!()
        };
        *slot = other_slot;
        prop_assert!(!scratch.field_preimage.matches_checked_push_receipt(
            scratch.field.as_ref().unwrap(), &frames, pushed,
        ));
        let Frame::Message(parent) = &mut frames[pushed.before_len - 1] else {
            unreachable!()
        };
        let Some(MessageWait::Map { slot, .. }) = &mut parent.waiting else {
            unreachable!()
        };
        *slot = selected_slot;
        prop_assert!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ).is_ok());
    }

    #[test]
    fn generated_new_push_preserves_every_prior_scalar_source(
        suffix in 0u16..1000,
        key in 0x21u8..0x7f,
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let uri = format!("rho:a{suffix:03}");
        let mut bytes = protobuf_encoder::encode_to_vec(&New {
            uri: vec![uri],
            ..New::default()
        });
        bytes.extend_from_slice(&[0x22, 0x03, 0x0a, 0x01, key]);
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = Vec::with_capacity(1);
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            input_bytes: bytes.len(),
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            byte_native_validations: 1,
            ..CanonicalProtobufAdmissionStats::default()
        };
        push_admission_frame(
            &mut frames,
            FrameRequest::Message {
                region,
                program: &NEW_RHO_MATCH_PROGRAM,
            },
            &mut stats,
        ).unwrap();
        let mut scratch = AdmissionScratch::allocate();

        let first_ticket = admission_live_core::PreStepTicket::before(&frames, &scores, &stats).unwrap();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let first_kind = frames.last_mut().unwrap().step_into(
            &mut stats, &mut scores, &mut scratch,
        ).unwrap();
        prop_assert!(matches!(first_kind, AdmissionStepKind::MessageContinue));
        prop_assert!(emit_admission_noncompletion_event(
            first_kind, &scratch, &frames, &scores, &first_ticket,
        ).is_ok());
        let Frame::Message(parent) = frames.last().unwrap() else {
            unreachable!()
        };
        let prior = scalar_source_prefix(parent).unwrap();
        prop_assert!(prior.iter().any(|slot| !slot.is_empty()));

        scratch.clear();
        let second_ticket = admission_live_core::PreStepTicket::before(&frames, &scores, &stats).unwrap();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let second_kind = frames.last_mut().unwrap().step_into(
            &mut stats, &mut scores, &mut scratch,
        ).unwrap();
        prop_assert!(matches!(second_kind, AdmissionStepKind::MessagePush));
        process_admission_noncompletion(
            second_kind, &mut scratch, &mut frames, &scores, &mut stats,
        ).unwrap();
        prop_assert!(emit_admission_noncompletion_event(
            second_kind, &scratch, &frames, &scores, &second_ticket,
        ).is_ok());
        let Frame::Message(parent) = &frames[frames.len() - 2] else {
            unreachable!()
        };
        let after = scalar_source_prefix(parent).unwrap();
        prop_assert_eq!(after.len(), prior.len());
        for (old_slot, new_slot) in prior.iter().zip(&after) {
            prop_assert_eq!(new_slot.len(), old_slot.len());
            for (old, new) in old_slot.iter().zip(new_slot) {
                prop_assert_eq!(old.variant, new.variant);
                prop_assert_eq!(old.field_position, new.field_position);
                prop_assert!(old.value.same_source_value(&new.value));
            }
        }
    }
}

fn scalar_source_prefix<'src>(
    frame: &MessageFrame<'src>,
) -> Option<Vec<Vec<ScalarSourcePrefixEntry<'src>>>> {
    frame
        .semantic_slots
        .iter()
        .map(|slot| {
            slot.iter()
                .map(|occurrence| {
                    let SemanticValue::Scalar(value) = &occurrence.value else {
                        return None;
                    };
                    occurrence.body.is_none().then(|| ScalarSourcePrefixEntry {
                        variant: occurrence.variant,
                        value: value.clone(),
                        field_position: occurrence.field_position,
                    })
                })
                .collect()
        })
        .collect()
}

fn scalar_source_prefix_matches(
    before: &[Vec<ScalarSourcePrefixEntry<'_>>],
    frame: &MessageFrame<'_>,
    appended_slot: usize,
) -> bool {
    before.len() == frame.semantic_slots.len()
        && before.iter().enumerate().all(|(slot, prior)| {
            let after = &frame.semantic_slots[slot];
            prior.len().checked_add(usize::from(slot == appended_slot)) == Some(after.len())
                && prior.iter().zip(after).all(|(old, current)| {
                    old.variant == current.variant
                        && current.body.is_none()
                        && old.field_position == current.field_position
                        && matches!(&current.value, SemanticValue::Scalar(value)
                            if old.value.same_source_value(value))
                })
        })
}

proptest! {
    #[test]
    fn generated_send_scalar_iteration_image_rejects_causal_mutants(
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = protobuf_encoder::encode_to_vec(&Send {
            persistent: true,
            ..Send::default()
        });
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = vec![Frame::Message(
            MessageFrame::from_message_region(region.clone(), &SEND_RHO_MATCH_PROGRAM).unwrap(),
        )];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let mut scratch = AdmissionScratch::allocate();
        let ticket = admission_live_core::PreStepTicket::before(&frames, &scores, &stats).unwrap();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let kind = frames
            .last_mut()
            .unwrap()
            .step_into(&mut stats, &mut scores, &mut scratch)
            .unwrap();
        prop_assert!(matches!(kind, AdmissionStepKind::MessageContinue));
        let field = scratch.field.as_ref().unwrap();
        let slot = field.resolved.slot();
        let other = (0..scratch.field_preimage.seen.len())
            .find(|index| *index != slot)
            .unwrap();
        prop_assert!(matches!(frames.last(), Some(Frame::Message(frame))
            if scratch.field_preimage.matches_checked_scalar_receipt(field, frame)));
        prop_assert!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ).is_ok());

        scratch.field_preimage.seen[other] = !scratch.field_preimage.seen[other];
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message scalar iteration disagrees with its source image"
        ))));
        scratch.field_preimage.seen[other] = !scratch.field_preimage.seen[other];

        scratch.field_preimage.slot_lengths[other] += 1;
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message scalar iteration disagrees with its source image"
        ))));
        scratch.field_preimage.slot_lengths[other] -= 1;

        scratch.field.as_mut().unwrap().prestate.cursor.bytes_read += 1;
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message scalar iteration disagrees with its source image"
        ))));
        scratch.field.as_mut().unwrap().prestate.cursor.bytes_read -= 1;

        scratch.field.as_mut().unwrap().prestate.last_slot = Some(slot);
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message scalar iteration disagrees with its source image"
        ))));
        scratch.field.as_mut().unwrap().prestate.last_slot = None;

        scratch.field.as_mut().unwrap().prestate.seen = true;
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message scalar iteration disagrees with its source image"
        ))));
        scratch.field.as_mut().unwrap().prestate.seen = false;

        {
            let Frame::Message(frame) = frames.last_mut().unwrap() else {
                unreachable!()
            };
            let SemanticValue::Scalar(SemanticScalar::Bool(value)) =
                &mut frame.semantic_slots[slot].last_mut().unwrap().value else {
                    unreachable!()
                };
            *value = !*value;
        }
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked admission event disagrees with its source transition"
        ))));
        {
            let Frame::Message(frame) = frames.last_mut().unwrap() else {
                unreachable!()
            };
            let SemanticValue::Scalar(SemanticScalar::Bool(value)) =
                &mut frame.semantic_slots[slot].last_mut().unwrap().value else {
                    unreachable!()
                };
            *value = !*value;
        }
        prop_assert!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ).is_ok());

        scores.integer(7).unwrap();
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked admission event disagrees with its source transition"
        ))));

        let result = drive_admission(
            FrameRequest::Message {
                region,
                program: &SEND_RHO_MATCH_PROGRAM,
            },
            bytes.len(),
        );
        prop_assert!(result.is_ok());
    }

    #[test]
    fn generated_new_repeated_scalar_keeps_older_source_value(
        suffix in 0u16..1000,
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let first_uri = format!("rho:a{suffix:03}");
        let second_uri = format!("rho:z{suffix:03}");
        let bytes = protobuf_encoder::encode_to_vec(&New {
            uri: vec![first_uri.clone(), second_uri.clone()],
            ..New::default()
        });
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = vec![Frame::Message(
            MessageFrame::from_message_region(region, &NEW_RHO_MATCH_PROGRAM).unwrap(),
        )];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let mut scratch = AdmissionScratch::allocate();

        let first_ticket = admission_live_core::PreStepTicket::before(
            &frames, &scores, &stats,
        ).unwrap();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let first_kind = frames.last_mut().unwrap().step_into(
            &mut stats, &mut scores, &mut scratch,
        ).unwrap();
        prop_assert!(matches!(first_kind, AdmissionStepKind::MessageContinue));
        prop_assert!(emit_admission_noncompletion_event(
            first_kind, &scratch, &frames, &scores, &first_ticket,
        ).is_ok());
        let slot = scratch.field.as_ref().unwrap().resolved.slot();
        let Frame::Message(frame) = frames.last().unwrap() else {
            unreachable!()
        };
        let first_occurrence = &frame.semantic_slots[slot][0];
        let SemanticValue::Scalar(SemanticScalar::String(first_view)) = &first_occurrence.value else {
            unreachable!()
        };
        let first_view = first_view.clone();
        let first_position = first_occurrence.field_position;
        prop_assert_eq!(first_view.bytes().collect::<Vec<_>>(), first_uri.as_bytes().to_vec());

        scratch.clear();
        let second_ticket = admission_live_core::PreStepTicket::before(
            &frames, &scores, &stats,
        ).unwrap();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let second_kind = frames.last_mut().unwrap().step_into(
            &mut stats, &mut scores, &mut scratch,
        ).unwrap();
        prop_assert!(matches!(second_kind, AdmissionStepKind::MessageContinue));
        prop_assert!(emit_admission_noncompletion_event(
            second_kind, &scratch, &frames, &scores, &second_ticket,
        ).is_ok());
        let Frame::Message(frame) = frames.last().unwrap() else {
            unreachable!()
        };
        prop_assert_eq!(frame.semantic_slots[slot].len(), 2);
        let first_after = &frame.semantic_slots[slot][0];
        let SemanticValue::Scalar(SemanticScalar::String(first_after_view)) = &first_after.value else {
            unreachable!()
        };
        prop_assert!(first_after_view.same_view(&first_view));
        prop_assert_eq!(first_after.field_position, first_position);
        prop_assert_eq!(first_after_view.bytes().collect::<Vec<_>>(), first_uri.as_bytes().to_vec());
        let SemanticValue::Scalar(SemanticScalar::String(second_view)) =
            &frame.semantic_slots[slot][1].value else {
                unreachable!()
            };
        prop_assert_eq!(second_view.bytes().collect::<Vec<_>>(), second_uri.as_bytes().to_vec());
    }

    #[test]
    fn generated_new_scalar_appends_preserve_all_prior_source_occurrences(
        suffix in 0u16..1000,
        bind_count in 1i32..128,
        locally_free in 1u8..128,
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let first_uri = format!("rho:a{suffix:03}");
        let second_uri = format!("rho:z{suffix:03}");
        let bytes = protobuf_encoder::encode_to_vec(&New {
            bind_count,
            uri: vec![first_uri.clone(), second_uri.clone()],
            locally_free: vec![locally_free],
            ..New::default()
        });
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = vec![Frame::Message(
            MessageFrame::from_message_region(region, &NEW_RHO_MATCH_PROGRAM).unwrap(),
        )];
        let mut scores = CanonicalScoreArena::new();
        let scalar_score = scores.integer(i64::from(bind_count)).unwrap();
        let bytes_score = scores.bytes(&[locally_free]).unwrap();
        scores.node(vec![scalar_score, bytes_score]).unwrap();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let mut scratch = AdmissionScratch::allocate();
        let mut bind_slot = None;
        let mut uri_slot = None;
        let mut previous_field = 0u32;

        for iteration in 0..4 {
            scratch.clear();
            let score_before = scores.source_image();
            let score_checkpoint = scores.append_checkpoint();
            let Frame::Message(before_frame) = frames.last().unwrap() else {
                unreachable!()
            };
            let before = scalar_source_prefix(before_frame).unwrap();
            let ticket = admission_live_core::PreStepTicket::before(
                &frames, &scores, &stats,
            ).unwrap();
            scratch.field_preimage.capture(frames.last().unwrap());
            let read = scratch.field_preimage.cursor_source.unwrap();
            prop_assert_eq!(read.reader_at, read.cursor_yielded);
            prop_assert_eq!(read.cursor_yielded, scratch.field_preimage.cursor.bytes_read);
            prop_assert_eq!(read.fields_read, iteration);
            prop_assert_eq!(read.last_field, previous_field);
            scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
            let kind = frames.last_mut().unwrap().step_into(
                &mut stats, &mut scores, &mut scratch,
            ).unwrap();
            prop_assert!(matches!(kind, AdmissionStepKind::MessageContinue));
            prop_assert_eq!(scores.source_image(), score_before);
            prop_assert!(scores.certified_append_since(&score_checkpoint));
            let field = scratch.field.as_ref().unwrap();
            let slot = field.resolved.slot();
            let Frame::Message(after_frame) = frames.last().unwrap() else {
                unreachable!()
            };
            prop_assert!(scalar_source_prefix_matches(&before, after_frame, slot));
            prop_assert!(scratch.field_preimage.matches_checked_scalar_receipt(field, after_frame));
            prop_assert!(emit_admission_noncompletion_event(
                kind, &scratch, &frames, &scores, &ticket,
            ).is_ok());

            if iteration == 0 {
                let original = scratch.field_preimage.cursor_source.unwrap();
                scratch.field_preimage.cursor_source.as_mut().unwrap().reader_at += 1;
                let forged = scratch.field_preimage.cursor_source.unwrap();
                prop_assert_ne!(forged.reader_at, forged.cursor_yielded);
                prop_assert!(!scratch.field_preimage.matches_checked_scalar_receipt(
                    field, after_frame,
                ));
                prop_assert!(matches!(emit_admission_noncompletion_event(
                    kind, &scratch, &frames, &scores, &ticket,
                ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "checked message scalar iteration disagrees with its source image"
                ))));
                scratch.field_preimage.cursor_source = Some(original);
                prop_assert!(emit_admission_noncompletion_event(
                    kind, &scratch, &frames, &scores, &ticket,
                ).is_ok());

                let original_body = scratch.field_preimage.body.take().unwrap();
                let different_origin = IndexedByteRegion::from_borrowed(
                    &bytes, source_id + 1, offset,
                );
                prop_assert_eq!(
                    original_body.bytes().collect::<Vec<_>>(),
                    different_origin.bytes().collect::<Vec<_>>(),
                );
                prop_assert_ne!(
                    original_body.position_at(0).unwrap().physical,
                    different_origin.position_at(0).unwrap().physical,
                );
                prop_assert!(!original_body.same_view(&different_origin));
                scratch.field_preimage.body = Some(different_origin);
                prop_assert!(!ticket.matches_field_preimage(&scratch.field_preimage));
                prop_assert!(matches!(emit_admission_noncompletion_event(
                    kind, &scratch, &frames, &scores, &ticket,
                ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "checked message scalar iteration disagrees with its source image"
                ))));
                scratch.field_preimage.body = Some(original_body);
                prop_assert!(emit_admission_noncompletion_event(
                    kind, &scratch, &frames, &scores, &ticket,
                ).is_ok());
            }

            previous_field = field.field_number;

            match iteration {
                0 => bind_slot = Some(slot),
                1 => uri_slot = Some(slot),
                2 => {
                    let mut foreign = CanonicalScoreArena::new();
                    let foreign_scalar = foreign.integer(i64::from(bind_count) + 1).unwrap();
                    let foreign_bytes = foreign.bytes(&[locally_free]).unwrap();
                    foreign.node(vec![foreign_scalar, foreign_bytes]).unwrap();
                    prop_assert_eq!(foreign.source_len(), scores.source_len());
                    prop_assert_ne!(foreign.source_image(), scores.source_image());
                    prop_assert!(!foreign.certified_append_since(&score_checkpoint));
                    prop_assert!(matches!(emit_admission_noncompletion_event(
                        kind, &scratch, &frames, &foreign, &ticket,
                    ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "checked admission event disagrees with its live pre-step ticket"
                    ))));
                    let bind_slot = bind_slot.unwrap();
                    let uri_slot = uri_slot.unwrap();
                    prop_assert_eq!(slot, uri_slot);
                    prop_assert_ne!(slot, bind_slot);
                    let Frame::Message(frame) = frames.last_mut().unwrap() else {
                        unreachable!()
                    };
                    let first = &frame.semantic_slots[uri_slot][0];
                    let SemanticValue::Scalar(SemanticScalar::String(first_view)) = &first.value else {
                        unreachable!()
                    };
                    let anchor = first_view.position_at(0).unwrap().physical;
                    let equal_bytes_other_view = IndexedByteRegion::from_borrowed(
                        first_uri.as_bytes(), anchor.source, anchor.offset,
                    );
                    prop_assert_eq!(
                        first_view.bytes().collect::<Vec<_>>(),
                        equal_bytes_other_view.bytes().collect::<Vec<_>>(),
                    );
                    prop_assert!(!first_view.same_view(&equal_bytes_other_view));
                    let original = std::mem::replace(
                        &mut frame.semantic_slots[uri_slot][0].value,
                        SemanticValue::Scalar(SemanticScalar::String(equal_bytes_other_view)),
                    );
                    prop_assert!(!scalar_source_prefix_matches(&before, frame, slot));
                    frame.semantic_slots[uri_slot][0].value = original;
                    prop_assert!(scalar_source_prefix_matches(&before, frame, slot));

                    frame.semantic_slots[bind_slot][0].field_position.offset += 1;
                    prop_assert!(!scalar_source_prefix_matches(&before, frame, slot));
                    frame.semantic_slots[bind_slot][0].field_position.offset -= 1;
                    prop_assert!(scalar_source_prefix_matches(&before, frame, slot));
                },
                3 => {
                    prop_assert_ne!(slot, bind_slot.unwrap());
                    prop_assert_ne!(slot, uri_slot.unwrap());
                },
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn generated_frozen_and_composed_message_pre_read_rejects_changed_end_origin(
        bind_count in 1i32..128,
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
        split_seed in any::<u8>(),
        frozen in any::<bool>(),
    ) {
        let bytes = protobuf_encoder::encode_to_vec(&New {
            bind_count,
            ..New::default()
        });
        prop_assert!(bytes.len() >= 2);
        let end = CanonicalByteProvenance::new(source_id + 2, offset + bytes.len());
        let region = if frozen {
            let mut arena = CanonicalBytesArena::new();
            let mut handle = arena.empty();
            for (index, byte) in bytes.iter().enumerate() {
                handle = arena.append(
                    handle,
                    CanonicalLocatedByte::new(
                        *byte,
                        CanonicalByteProvenance::new(source_id, offset + index),
                    ),
                ).unwrap();
            }
            IndexedByteRegion::from_frozen(arena.freeze(handle), end)
        } else {
            let cut = 1 + usize::from(split_seed) % (bytes.len() - 1);
            let left = SourceTreeWindow::from_borrowed(
                &bytes[..cut], source_id, offset,
            ).unwrap();
            let right = SourceTreeWindow::from_borrowed(
                &bytes[cut..], source_id + 1, offset + cut,
            ).unwrap();
            IndexedByteRegion::from_source(left.concat(&right).unwrap(), end)
        };
        let mut frames = vec![Frame::Message(
            MessageFrame::from_message_region(region, &NEW_RHO_MATCH_PROGRAM).unwrap(),
        )];
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let mut scratch = AdmissionScratch::allocate();
        let ticket = admission_live_core::PreStepTicket::before(
            &frames, &scores, &stats,
        ).unwrap();
        scratch.field_preimage.capture(frames.last().unwrap());
        scratch.driver_source = Some(DriverStepSource::before(&frames, &scores));
        let kind = frames.last_mut().unwrap().step_into(
            &mut stats, &mut scores, &mut scratch,
        ).unwrap();
        prop_assert!(matches!(kind, AdmissionStepKind::MessageContinue));
        prop_assert!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ).is_ok());

        let original_body = scratch.field_preimage.body.take().unwrap();
        let changed_end = CanonicalByteProvenance::new(source_id + 3, end.offset);
        let altered_body = original_body.subview_with_end(
            CanonicalByteRange { start: 0, end: original_body.len() },
            changed_end,
        ).unwrap();
        prop_assert_eq!(
            original_body.located_bytes().collect::<Vec<_>>(),
            altered_body.located_bytes().collect::<Vec<_>>(),
        );
        prop_assert_eq!(original_body.bytes().collect::<Vec<_>>(), bytes.clone());
        prop_assert_ne!(original_body.end_position().physical, altered_body.end_position().physical);
        prop_assert!(!original_body.same_view(&altered_body));
        scratch.field_preimage.body = Some(altered_body);
        prop_assert!(!ticket.matches_field_preimage(&scratch.field_preimage));
        prop_assert!(matches!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ), Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked message scalar iteration disagrees with its source image"
        ))));
        scratch.field_preimage.body = Some(original_body);
        prop_assert!(emit_admission_noncompletion_event(
            kind, &scratch, &frames, &scores, &ticket,
        ).is_ok());
    }
}
