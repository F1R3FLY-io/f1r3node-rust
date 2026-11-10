//! Opt-in sequence checks for an accepted fragmented EPathMap snapshot.
//!
//! The receipt is compared with the live parent slot at acceptance, the same
//! slot at the next driver action, and the later score construction. These
//! observations are made at distinct points in the admission driver. This
//! generated one-entry fixture checks receipt handoff; it need not create a
//! canonical-path obligation.

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::canonical_score_bytes::CanonicalScoreSourceKind;
use crate::rust::rholang::protobuf_encoder;

fn generated_par(value: i16) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(value))),
    }])
}

fn located_body(
    bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    terminal: CanonicalByteProvenance,
) -> OwnedIndexedByteRegion {
    assert_eq!(bytes.len(), anchors.len());
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (&byte, &anchor) in bytes.iter().zip(anchors) {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, anchor))
            .expect("generated fragment fits the source arena");
    }
    OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), terminal)
}

fn assert_retained_slot(
    frame: &rpb3_root_finish_trace::RootFrameState,
    slot: usize,
    snapshot: &[u8],
    anchor: CanonicalByteProvenance,
) {
    assert_eq!(frame.program_type_id, EPATHMAP_RHO_MATCH_PROGRAM.type_id);
    let occurrence = frame.semantic_slots[slot]
        .last()
        .expect("accepted snapshot remains in its semantic slot");
    assert_eq!(occurrence.body.as_deref(), Some(snapshot));
    assert_eq!(occurrence.body_start, Some(anchor));
    let rpb3_root_finish_trace::RootOccurrenceValue::EPathMap {
        snapshot: stored,
        snapshot_start,
        ..
    } = &occurrence.value
    else {
        panic!("accepted slot retains an EPathMap occurrence");
    };
    assert_eq!(stored, snapshot);
    assert_eq!(*snapshot_start, anchor);
}

fn frame_region_image(
    frame: &rpb3_root_finish_trace::ActiveFrameState,
) -> (&[u8], &[CanonicalByteProvenance]) {
    match frame {
        rpb3_root_finish_trace::ActiveFrameState::Message { bytes, anchors, .. }
        | rpb3_root_finish_trace::ActiveFrameState::NonMessage { bytes, anchors, .. } => {
            (bytes, anchors)
        }
    }
}

fn assert_persisting_frame_region_unchanged(
    before: &rpb3_root_finish_trace::ActiveFrameState,
    after: &rpb3_root_finish_trace::ActiveFrameState,
) {
    assert_eq!(frame_region_image(before), frame_region_image(after));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn fragmented_epathmap_receipt_survives_parent_and_outer_driver_actions(
        key in any::<i16>(),
        value in any::<i16>(),
        empty_value in any::<bool>(),
        source_base in 100u32..200,
    ) {
        let map_value = if empty_value { Par::default() } else { generated_par(value) };
        let map = EPathMap::new_map(
            [(generated_par(key), map_value)], Vec::new(), false, None,
        );
        let snapshot = map.trie_snapshot().to_vec();
        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        };
        let bytes = protobuf_encoder::encode_to_vec(&expr);
        let offsets: Vec<_> = bytes.windows(snapshot.len())
            .enumerate()
            .filter_map(|(at, window)| (window == snapshot.as_slice()).then_some(at))
            .collect();
        prop_assert_eq!(offsets.len(), 1);
        let snapshot_at = offsets[0];
        let anchors: Vec<_> = (0..bytes.len())
            .map(|index| CanonicalByteProvenance::new(
                source_base + (index % 17) as u32,
                101 + index * 19,
            ))
            .collect();
        let terminal = CanonicalByteProvenance::new(700, 9000);

        rpb3_root_finish_trace::start_with_active_frames();
        let result = validate_canonical_protobuf_inner(
            located_body(&bytes, &anchors, terminal),
            &EXPR_RHO_MATCH_PROGRAM,
            None,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(result.is_ok(), "generated EPathMap rejected: {:?}", result.as_ref().err());
        let admission = result.unwrap();
        let mut saw_nonmessage_complete = false;
        for (at, step) in trace.driver_steps.iter().enumerate() {
            let before = step.active_frames_before.as_ref().expect("opt-in before stack");
            let after = step.active_frames_after.as_ref().expect("opt-in after stack");
            prop_assert_eq!(before.len(), step.before_len);
            prop_assert_eq!(after.len(), step.after_len);
            if at > 0 {
                prop_assert_eq!(
                    trace.driver_steps[at - 1].active_frames_after.as_ref().unwrap(),
                    before,
                );
            }
            let nonmessage_top = matches!(
                before.last(),
                Some(rpb3_root_finish_trace::ActiveFrameState::NonMessage { .. })
            );
            match step.kind {
                rpb3_root_finish_trace::DriverStepKind::Continue => {
                    prop_assert_eq!(after.len(), before.len());
                    prop_assert_eq!(&before[..before.len() - 1], &after[..after.len() - 1]);
                    assert_persisting_frame_region_unchanged(before.last().unwrap(), after.last().unwrap());
                }
                rpb3_root_finish_trace::DriverStepKind::Push(_) => {
                    prop_assert_eq!(after.len(), before.len() + 1);
                    prop_assert_eq!(&before[..before.len() - 1], &after[..before.len() - 1]);
                    assert_persisting_frame_region_unchanged(before.last().unwrap(), &after[before.len() - 1]);
                }
                rpb3_root_finish_trace::DriverStepKind::Complete => {
                    prop_assert_eq!(after.len() + 1, before.len());
                    if !after.is_empty() {
                        prop_assert_eq!(&before[..before.len() - 2], &after[..after.len() - 1]);
                        assert_persisting_frame_region_unchanged(
                            &before[before.len() - 2],
                            after.last().unwrap(),
                        );
                    }
                    saw_nonmessage_complete |= nonmessage_top;
                }
            }
        }
        // This one-entry EPM snapshot has no canonical-path obligation; its
        // EPathMap frame can complete in its first action. The generic
        // Continue/Push branches still check all such outer-frame actions.
        prop_assert!(saw_nonmessage_complete);
        let map_value_returns: Vec<_> = trace.pop_accepts.iter()
            .filter(|pop| matches!(
                pop.parent_nonmessage_before.as_ref(),
                Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent))
                    if parent.waiting_map_value
            ))
            .collect();
        prop_assert_eq!(map_value_returns.len(), 1);
        let map_value_return = map_value_returns[0];
        let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent_before)) =
            map_value_return.parent_nonmessage_before.as_ref() else {
                panic!("map value return has an EPathMap parent");
            };
        let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent_after)) =
            map_value_return.parent_nonmessage_after.as_ref() else {
                panic!("map value return resumes its EPathMap parent");
            };
        let Some(rpb3_root_finish_trace::RootReturnedState::Par(returned)) =
            map_value_return.typed_summary.as_ref() else {
                panic!("map value returns a typed Par summary");
            };
        prop_assert_eq!(returned.canonical_path_stable, !empty_value);
        prop_assert!(parent_before.waiting_map_value);
        prop_assert!(!parent_after.waiting_map_value);
        prop_assert!(parent_after.waiting_obligation.is_none());
        prop_assert_eq!(parent_before.snapshot.as_slice(), parent_after.snapshot.as_slice());
        prop_assert_eq!(parent_before.region_start, parent_after.region_start);
        prop_assert_eq!(parent_before.region_end, parent_after.region_end);
        prop_assert_eq!(parent_before.cursor, parent_after.cursor);
        prop_assert_eq!(parent_before.path, parent_after.path);
        prop_assert_eq!(parent_before.mode, parent_after.mode);
        prop_assert_eq!(parent_after.entries_canonical_path_stable,
            parent_before.entries_canonical_path_stable && returned.canonical_path_stable);
        prop_assert_eq!(map_value_return.source_len_before,
            map_value_return.source_len_after.unwrap());
        rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
            &trace,
            &snapshot,
            &anchors[snapshot_at..snapshot_at + snapshot.len()],
            anchors.get(snapshot_at + snapshot.len()).copied().unwrap_or(terminal),
        );
        prop_assert_eq!(trace.epathmap_accepts.len(), 1);
        prop_assert_eq!(trace.epathmap_scores.len(), 1);
        let receipt = &trace.epathmap_accepts[0];
        let score = &trace.epathmap_scores[0];
        prop_assert_eq!(receipt.returned_snapshot.as_slice(), snapshot.as_slice());
        prop_assert_eq!(receipt.stored_snapshot.as_slice(), snapshot.as_slice());
        prop_assert_eq!(receipt.returned_start, anchors[snapshot_at]);
        prop_assert_eq!(receipt.stored_start, anchors[snapshot_at]);
        prop_assert_eq!(receipt.parent_type_id, EPATHMAP_RHO_MATCH_PROGRAM.type_id);
        prop_assert_eq!(receipt.tag, 9);

        let accept_step = &trace.driver_steps[receipt.step_index];
        prop_assert!(matches!(accept_step.kind, rpb3_root_finish_trace::DriverStepKind::Complete));
        let accepted_parent = accept_step.after_message.as_ref()
            .expect("snapshot child completion resumes EPathMap parent");
        assert_retained_slot(accepted_parent, receipt.slot, &snapshot, anchors[snapshot_at]);
        prop_assert_eq!(score.step_index, receipt.step_index + 1);
        let score_step = &trace.driver_steps[score.step_index];
        prop_assert!(matches!(score_step.kind, rpb3_root_finish_trace::DriverStepKind::Complete));
        let before_score = score_step.before_message.as_ref()
            .expect("EPathMap score step reads the accepted parent");
        assert_retained_slot(before_score, receipt.slot, &snapshot, anchors[snapshot_at]);
        prop_assert_eq!(accepted_parent, before_score);
        prop_assert_eq!(score.snapshot.as_slice(), snapshot.as_slice());
        prop_assert!(trace.driver_steps.len() > score.step_index + 1);
        prop_assert_eq!(
            trace.driver_steps[score.step_index + 1]
                .before_top_program
                .expect("outer Expr resumes after EPathMap score")
                .type_id,
            EXPR_RHO_MATCH_PROGRAM.type_id,
        );

        let entries = trace.score_arena.as_ref().expect("root return retains score arena");
        prop_assert_eq!(
            &entries[score.snapshot_handle.index()].kind,
            &CanonicalScoreSourceKind::BytesIndexed(snapshot.clone()),
        );
        let returned = trace.returned.as_ref().expect("Expr root returns");
        prop_assert_eq!(returned.score_handle, Some(score.score_handle));
        prop_assert_eq!(admission.score_bytes(), returned.score_bytes.as_deref());

        let epm_pop = trace.pop_accepts.iter()
            .find(|event| event.child_kind == FrameRequestKind::EPathMap)
            .expect("snapshot child completes before acceptance");
        prop_assert_eq!(epm_pop.epathmap_returned_snapshot.as_deref(), Some(snapshot.as_slice()));
        let rpb3_root_finish_trace::NonMessageFrameState::EPathMap(completed) =
            epm_pop.child_nonmessage.as_ref().expect("snapshot child state captured")
        else {
            panic!("EPathMap child has its own frame state");
        };
        prop_assert!(completed.active_obligation.is_none());
        prop_assert!(completed.waiting_obligation.is_none());
        prop_assert!(!completed.waiting_map_value);
    }
}
