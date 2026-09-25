use proptest::prelude::*;

use super::*;
use crate::rust::epathmap_trie_codec::payload_mapping_tests::capture_allocations;

fn region(bytes: &[u8], offset: usize) -> AdmissionRegion<'_> {
    IndexedByteRegion::from_borrowed(bytes, 71, offset)
}

fn component(score: CanonicalScoreHandle) -> FrameSummary<'static> {
    FrameSummary::Message(MessageSummary {
        semantic: SemanticSummary::Component(ComponentSummary {
            family: ParComponentTag::Expr,
            score,
            complete: true,
            canonical_path_stable: true,
        }),
    })
}

fn await_component<'src>(frame: &mut MessageFrame<'src>, slot: usize, body: AdmissionRegion<'src>) {
    frame.waiting = Some(MessageWait::Child {
        slot,
        variant: None,
        field_position: body.position_at(0).unwrap().physical,
        body,
    });
}

fn detail(error: CanonicalProtobufAdmissionError, expected: &str, offset: usize) {
    let CanonicalProtobufAdmissionError::Schema {
        detail, position, ..
    } = error
    else {
        panic!("expected schema error");
    };
    assert_eq!(detail, expected);
    assert_eq!(
        position,
        region(&[], offset).position_at(0).unwrap().physical
    );
}

fn par_history(frame: &MessageFrame<'_>) -> Vec<Vec<(CanonicalScoreHandle, Vec<u8>)>> {
    frame
        .semantic_slots
        .iter()
        .map(|slot| {
            slot.iter()
                .map(|occurrence| {
                    let SemanticValue::Message(summary) = &occurrence.value else {
                        panic!("component")
                    };
                    let SemanticSummary::Component(component) = &summary.semantic else {
                        panic!("score")
                    };
                    let body = occurrence.body.as_ref().unwrap();
                    (
                        component.score,
                        body.located_bytes().map(|byte| byte.byte).collect(),
                    )
                })
                .collect()
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn retained_component_history_matches_independent_cached_trace(
        candidates in proptest::collection::vec((0usize..2, -5i64..6, proptest::collection::vec(any::<u8>(), 0..12)), 0..80),
    ) {
        let mut frame = MessageFrame::from_message_region(region(&[], 0), &PAR_PROTOBUF_REGION_PROGRAM).unwrap();
        let mut scores = CanonicalScoreArena::new();
        let mut previous: [Option<(i64, &[u8])>; 2] = [None, None];
        for (index, (slot, value, bytes)) in candidates.iter().enumerate() {
            let before = par_history(&frame);
            let expected = previous[*slot].is_none_or(|(old, body)| (old, body) <= (*value, bytes.as_slice()));
            let score = scores.integer(*value).unwrap();
            await_component(&mut frame, *slot, region(bytes, index + 50));
            let result = frame.accept(component(score), &scores);
            prop_assert!(frame.waiting.is_none());
            prop_assert_eq!(result.is_ok(), expected);
            if expected {
                previous[*slot] = Some((*value, bytes));
                prop_assert_eq!(frame.semantic_slots[*slot].len(), before[*slot].len() + 1);
                prop_assert_eq!(par_history(&frame)[1 - *slot].clone(), before[1 - *slot].clone());
            } else {
                detail(result.unwrap_err(), "raw Par components are outside canonical score/body order", index + 50);
                prop_assert_eq!(par_history(&frame), before);
            }
        }
    }

    #[test]
    fn retained_map_history_matches_strict_cached_trace(
        candidates in proptest::collection::vec((0usize..2, proptest::collection::vec(any::<u8>(), 0..12)), 0..80),
    ) {
        let mut frame = MessageFrame::from_message_region(region(&[], 0), &NEW_RHO_MATCH_PROGRAM).unwrap();
        let scores = CanonicalScoreArena::new();
        let mut previous: [Option<&[u8]>; 2] = [None, None];
        let mut accepted = [0usize; 2];
        for (index, (slot, key)) in candidates.iter().enumerate() {
            let expected = previous[*slot].is_none_or(|old| old < key.as_slice());
            let key = region(key, index + 70);
            let key_position = key.position_at(0).unwrap().physical;
            frame.waiting = Some(MessageWait::Map { slot: *slot, field_position: key_position });
            let result = frame.accept(FrameSummary::Map(MapSummary { key, key_position, value: None }), &scores);
            prop_assert!(frame.waiting.is_none());
            prop_assert_eq!(result.is_ok(), expected);
            if expected {
                accepted[*slot] += 1;
                previous[*slot] = Some(&candidates[index].1);
            } else {
                detail(result.unwrap_err(), "protobuf map keys are not strictly ascending", index + 70);
            }
            for slot in 0..2 {
                prop_assert_eq!(frame.semantic_slots[slot].len(), accepted[slot]);
                if let Some(occurrence) = frame.semantic_slots[slot].last() {
                    let SemanticValue::Map(entry) = &occurrence.value else { panic!("map") };
                    prop_assert_eq!(entry.key.compare(&region(previous[slot].unwrap(), 0)), std::cmp::Ordering::Equal);
                }
            }
        }
    }
}

#[test]
fn retained_history_preserves_component_equality_and_body_tie_breaking() {
    let mut frame =
        MessageFrame::from_message_region(region(&[], 0), &PAR_PROTOBUF_REGION_PROGRAM).unwrap();
    let mut scores = CanonicalScoreArena::new();
    let score = scores.integer(4).unwrap();
    for bytes in [b"b".as_slice(), b"b", b"c"] {
        await_component(&mut frame, 0, region(bytes, 100));
        frame.accept(component(score), &scores).unwrap();
    }
    await_component(&mut frame, 0, region(b"b", 120));
    detail(
        frame.accept(component(score), &scores).unwrap_err(),
        "raw Par components are outside canonical score/body order",
        120,
    );
    assert_eq!(frame.semantic_slots[0].len(), 3);
    await_component(&mut frame, 1, region(b"a", 130));
    frame.accept(component(score), &scores).unwrap();
}

#[test]
fn wrong_summary_precedence_and_consumed_continuation_are_unchanged() {
    let mut frame =
        MessageFrame::from_message_region(region(&[], 0), &PAR_PROTOBUF_REGION_PROGRAM).unwrap();
    let scores = CanonicalScoreArena::new();
    await_component(&mut frame, 0, region(&[], 200));
    detail(
        frame
            .accept(
                FrameSummary::Message(MessageSummary {
                    semantic: SemanticSummary::Opaque,
                }),
                &scores,
            )
            .unwrap_err(),
        "raw Par component child returned the wrong semantic summary",
        200,
    );
    assert!(frame.waiting.is_none());
    assert!(frame.semantic_slots.iter().all(Vec::is_empty));
    await_component(&mut frame, 0, region(&[], 210));
    let map = || {
        FrameSummary::Map(MapSummary {
            key: region(&[], 0),
            key_position: region(&[], 0).position_at(0).unwrap().physical,
            value: None,
        })
    };
    detail(
        frame.accept(map(), &scores).unwrap_err(),
        "protobuf admission child returned the wrong summary kind",
        0,
    );
    assert!(frame.waiting.is_none());
    detail(
        frame.accept(map(), &scores).unwrap_err(),
        "protobuf admission child returned without a continuation",
        0,
    );
}

#[test]
fn message_construction_allocates_only_seen_and_semantic_slot_tables() {
    for program in [
        &PAR_PROTOBUF_REGION_PROGRAM,
        &NEW_RHO_MATCH_PROGRAM,
        &EXPR_RHO_MATCH_PROGRAM,
    ] {
        let (frame, counts) = capture_allocations(|| {
            MessageFrame::from_message_region(region(&[], 0), program).unwrap()
        });
        assert_eq!(counts.allocations, 2);
        assert!(!counts.overflow);
        assert_eq!(
            counts.allocated as usize,
            program.fields.len()
                * (std::mem::size_of::<bool>() + std::mem::size_of::<Vec<SemanticOccurrence>>())
        );
        assert!(frame.semantic_slots.iter().all(Vec::is_empty));
    }
}

#[test]
fn forged_prior_is_rejected_after_current_summary_validation() {
    let mut scores = CanonicalScoreArena::new();
    let score = scores.integer(1).unwrap();
    for (value, body) in [
        (
            SemanticValue::Scalar(SemanticScalar::Bool(true)),
            Some(region(b"old", 10)),
        ),
        (
            SemanticValue::Message(Box::new(MessageSummary {
                semantic: SemanticSummary::Opaque,
            })),
            Some(region(b"old", 10)),
        ),
        (
            SemanticValue::Message(Box::new(MessageSummary {
                semantic: SemanticSummary::Component(ComponentSummary {
                    family: ParComponentTag::Expr,
                    score,
                    complete: true,
                    canonical_path_stable: true,
                }),
            })),
            None,
        ),
    ] {
        let mut frame =
            MessageFrame::from_message_region(region(&[], 0), &PAR_PROTOBUF_REGION_PROGRAM)
                .unwrap();
        frame.semantic_slots[0].push(SemanticOccurrence {
            variant: None,
            body,
            value,
            field_position: region(&[], 10).position_at(0).unwrap().physical,
        });
        await_component(&mut frame, 0, region(b"new", 30));
        detail(
            frame
                .accept(
                    FrameSummary::Message(MessageSummary {
                        semantic: SemanticSummary::Opaque,
                    }),
                    &scores,
                )
                .unwrap_err(),
            "raw Par component child returned the wrong semantic summary",
            30,
        );
        assert!(frame.waiting.is_none());
        await_component(&mut frame, 0, region(b"new", 40));
        assert!(matches!(
            frame.accept(component(score), &scores),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
        ));
        assert!(frame.waiting.is_none());
        assert_eq!(frame.semantic_slots[0].len(), 1);
    }
    let mut frame =
        MessageFrame::from_message_region(region(&[], 0), &NEW_RHO_MATCH_PROGRAM).unwrap();
    frame.semantic_slots[0].push(SemanticOccurrence {
        variant: None,
        body: None,
        value: SemanticValue::Scalar(SemanticScalar::Bool(true)),
        field_position: region(&[], 0).position_at(0).unwrap().physical,
    });
    frame.waiting = Some(MessageWait::Map {
        slot: 0,
        field_position: region(&[], 0).position_at(0).unwrap().physical,
    });
    let map = FrameSummary::Map(MapSummary {
        key: region(b"new", 40),
        key_position: region(&[], 40).position_at(0).unwrap().physical,
        value: None,
    });
    assert!(matches!(
        frame.accept(map, &scores),
        Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
    ));
    assert!(frame.waiting.is_none());
    assert_eq!(frame.semantic_slots[0].len(), 1);
}
