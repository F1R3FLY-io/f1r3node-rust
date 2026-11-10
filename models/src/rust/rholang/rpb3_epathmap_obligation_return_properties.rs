//! Generated source-to-return checks for direct EPathMap path obligations.
//!
//! The child summary is observed before pop, independently of the waiting
//! parent and its after-accept frame. The fixture forces both path obligation
//! kinds through fragmented indexed input; it does not establish a universal
//! Rust-to-Verus simulation.

use proptest::prelude::*;
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieTopologyCursor, CanonicalCompactTrieTopologyEvent,
};

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::canonical_path::encode_trie_path;
use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::protobuf_encoder;

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn payload(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut result = vec![tag];
    prost::encoding::encode_varint(body.len() as u64, &mut result);
    result.extend_from_slice(body);
    result
}

fn field(number: u32, body: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    prost::encoding::encode_key(
        number,
        prost::encoding::WireType::LengthDelimited,
        &mut result,
    );
    prost::encoding::encode_varint(body.len() as u64, &mut result);
    result.extend_from_slice(body);
    result
}

fn fragmented(
    bytes: &[u8],
    source_base: u32,
) -> (
    OwnedIndexedByteRegion,
    Vec<CanonicalByteProvenance>,
    CanonicalByteProvenance,
) {
    let anchors: Vec<_> = (0..bytes.len())
        .map(|index| {
            CanonicalByteProvenance::new(source_base + (index % 23) as u32, 83 + index * 19)
        })
        .collect();
    let terminal = CanonicalByteProvenance::new(source_base + 100, 20_000);
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (&byte, &anchor) in bytes.iter().zip(&anchors) {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, anchor))
            .expect("bounded obligation fixture fits");
    }
    (
        OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), terminal),
        anchors,
        terminal,
    )
}

#[derive(Clone, Copy)]
enum NestedChildCase {
    Empty,
    SetStable,
    SetUnstable,
    MapStable,
    MapUnstable,
}

fn obligation_fixture(
    nested: bool,
    width: usize,
    marker: u8,
    child_case: NestedChildCase,
) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut data = vec![37; width];
    *data.last_mut().expect("generated width is positive") = marker;
    let value = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(data)),
    }]);
    let (key, child_body) = if nested {
        let child = match child_case {
            NestedChildCase::Empty => EPathMap::default(),
            NestedChildCase::SetStable => {
                EPathMap::from_set_iter([integer(7)], Vec::new(), false, None)
            }
            NestedChildCase::SetUnstable => {
                EPathMap::from_set_iter([Par::default()], Vec::new(), false, None)
            }
            NestedChildCase::MapStable => {
                EPathMap::new_map([(integer(7), value)], Vec::new(), false, None)
            }
            NestedChildCase::MapUnstable => {
                EPathMap::new_map([(integer(7), Par::default())], Vec::new(), false, None)
            }
        };
        let snapshot = child.trie_snapshot().to_vec();
        (payload(0x10, &snapshot), snapshot)
    } else {
        let raw = value.with_sends(vec![Send {
            chan: Some(integer(7)),
            ..Send::default()
        }]);
        let body = protobuf_encoder::encode_to_vec(&raw);
        (encode_trie_path(&raw), body)
    };
    let mut trie = pathmap::PathMap::new();
    trie.insert(&key, integer(11));
    let snapshot = epathmap_trie_codec::encode(&EPathMapRepr::Map(trie));
    (
        field(5, &field(32, &field(9, &snapshot))),
        key,
        child_body,
        snapshot,
    )
}

fn one_offset(haystack: &[u8], needle: &[u8]) -> usize {
    let offsets: Vec<_> = haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(at, window)| (window == needle).then_some(at))
        .collect();
    assert_eq!(
        offsets.len(),
        1,
        "generated child body has one source window"
    );
    offsets[0]
}

fn indexed_act_obligation_anchors(
    snapshot: &[u8],
    snapshot_anchors: &[CanonicalByteProvenance],
    key: &[u8],
    child_body: &[u8],
) -> Vec<CanonicalByteProvenance> {
    let arena = rpb3_root_finish_trace::epm_arena_range(snapshot.len(), |at| snapshot[at]);
    let mut cursor = CanonicalCompactTrieTopologyCursor::new(&snapshot[arena.clone()])
        .expect("generated EPM1 has a canonical ACT topology");
    let mut path = Vec::<(u8, CanonicalByteProvenance)>::new();
    let mut matches = Vec::new();
    while let Some(event) = cursor.next_event() {
        match event {
            CanonicalCompactTrieTopologyEvent::Edge {
                byte,
                source_offset,
            } => {
                path.push((byte, snapshot_anchors[arena.start + source_offset]));
            }
            CanonicalCompactTrieTopologyEvent::Value { value: Some(_), .. } => {
                if path.iter().map(|(byte, _)| *byte).eq(key.iter().copied()) {
                    assert!(path.len() >= child_body.len());
                    let suffix = &path[path.len() - child_body.len()..];
                    assert!(suffix
                        .iter()
                        .map(|(byte, _)| *byte)
                        .eq(child_body.iter().copied()));
                    matches.push(suffix.iter().map(|(_, anchor)| *anchor).collect::<Vec<_>>());
                }
            }
            CanonicalCompactTrieTopologyEvent::Value { value: None, .. } => {}
            CanonicalCompactTrieTopologyEvent::Leave { .. } => {
                path.pop();
            }
        }
    }
    assert_eq!(matches.len(), 1, "generated key has one ACT value endpoint");
    matches.pop().unwrap()
}

fn obligation_push_terminal(
    trace: &rpb3_root_finish_trace::RootFinishTrace,
    kind: CanonicalPathObligationKind,
    body: &[u8],
    anchors: &[CanonicalByteProvenance],
    child_kind: FrameRequestKind,
) -> rpb3_root_finish_trace::EPathMapFrameState {
    let pushes: Vec<_> = trace
        .nonmessage_push_bodies
        .iter()
        .filter(|push| {
            push.obligation.is_some_and(|wait| wait.kind == kind)
                && trace.driver_steps[push.step_index].before_top_kind == FrameRequestKind::EPathMap
        })
        .collect();
    assert_eq!(
        pushes.len(),
        1,
        "generated obligation has one EPathMap Push"
    );
    let push = pushes[0];
    assert_eq!(push.body.as_slice(), body);
    assert_eq!(push.start, anchors[0]);
    assert_eq!(push.end, *anchors.last().unwrap());
    let step = &trace.driver_steps[push.step_index];
    assert!(matches!(
        step.kind,
        rpb3_root_finish_trace::DriverStepKind::Push(_)
    ));
    assert_eq!(step.after_len, step.before_len + 1);
    assert_eq!(step.after_top_kind, Some(child_kind));
    assert_eq!(step.after_top_region, Some((anchors[0], body.len())));
    assert_eq!(step.score_len_before, step.score_len_after);
    let pushed = step
        .active_frames_after
        .as_ref()
        .expect("active stack enabled")
        .last()
        .expect("Push adds one child");
    let (pushed_bytes, pushed_anchors) = match (child_kind, pushed) {
        (
            FrameRequestKind::Message,
            rpb3_root_finish_trace::ActiveFrameState::Message { bytes, anchors, .. },
        )
        | (
            FrameRequestKind::EPathMap,
            rpb3_root_finish_trace::ActiveFrameState::NonMessage { bytes, anchors, .. },
        ) => (bytes, anchors),
        _ => panic!("obligation Push selected the wrong child frame kind"),
    };
    assert_eq!(pushed_bytes.as_slice(), body);
    assert_eq!(pushed_anchors.as_slice(), anchors);
    let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(before)) =
        step.before_nonmessage.as_ref()
    else {
        panic!("Push began at an EPathMap parent");
    };
    assert!(before.waiting_obligation.is_none());
    assert!(!before.waiting_map_value);

    let events: Vec<_> = trace
        .epathmap_events
        .iter()
        .filter(|event| event.step_index == push.step_index)
        .collect();
    let (terminal, prefix) = events
        .split_last()
        .expect("EPathMap Push has a terminal event");
    assert!(
        !prefix.is_empty(),
        "generated obligation has preceding topology events"
    );
    assert!(prefix
        .iter()
        .all(|event| event.after.waiting_obligation.is_none() && !event.after.waiting_map_value));
    assert!(matches!(&terminal.kind,
        rpb3_root_finish_trace::EPathMapEventKind::Edge(byte)
            if *byte == *body.last().unwrap()));
    assert_eq!(terminal.source, *anchors.last().unwrap());
    assert_eq!(terminal.after.waiting_obligation, push.obligation);
    assert!(!terminal.after.waiting_map_value);
    assert!(terminal.after.cursor.topology_events > before.cursor.topology_events);
    let progress = terminal
        .after
        .active_obligation
        .expect("terminal Edge completed the obligation");
    let wait = push.obligation.unwrap();
    assert_eq!(progress.obligation.id, wait.id);
    assert_eq!(progress.obligation.kind, kind);
    assert_eq!(progress.obligation.payload_len, body.len() as u64);
    assert_eq!(progress.remaining, 0);
    assert!(progress.awaiting_resolution);
    let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(suspended)) =
        step.suspended_parent_after.as_ref()
    else {
        panic!("Push retained its EPathMap parent");
    };
    assert_eq!(&terminal.after, suspended);
    assert_eq!(suspended.snapshot, before.snapshot);
    assert_eq!(suspended.mode, before.mode);
    assert_eq!(
        suspended.entries_canonical_path_stable,
        before.entries_canonical_path_stable
    );
    terminal.after.clone()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn canonical_path_parent_nested_epathmap_resolution_overflow_follows_reducer_accept(
        width in 1usize..65,
        marker in any::<u8>(),
        source_base in 100u32..200,
    ) {
        let (_, key, child_body, _) =
            obligation_fixture(true, width, marker, NestedChildCase::MapStable);
        let input = IndexedByteRegion::from_borrowed(&key, source_base, 300);
        let child = IndexedByteRegion::from_borrowed(&child_body, source_base + 1, 500);
        let summary = || FrameSummary::EPathMap(EPathMapSummary {
            mode: EPathMapMode::Map,
            snapshot: child.clone(),
            entries_canonical_path_stable: true,
        });

        let mut successful = CanonicalPathFrame::new(input.clone());
        let mut success_stats = CanonicalProtobufAdmissionStats {
            canonical_path_resolutions: 17,
            ..Default::default()
        };
        assert!(matches!(successful.step(&mut success_stats), Ok(FrameAction::Push(FrameRequest::EPathMap { .. }))));
        assert!(matches!(successful.accept(summary(), &mut success_stats),
            Ok(FrameAcceptEffect::ResolvedObligation { resolution: CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: true,
            }, .. })));
        prop_assert_eq!(success_stats.canonical_path_resolutions, 18);
        prop_assert!(successful.waiting.is_none());
        prop_assert!(successful.waiting_child.is_none());
        prop_assert!(successful.reducer.active_obligation().is_none());

        let mut overflowing = CanonicalPathFrame::new(input);
        let mut overflow_stats = CanonicalProtobufAdmissionStats {
            canonical_path_resolutions: usize::MAX,
            ..Default::default()
        };
        assert!(matches!(overflowing.step(&mut overflow_stats), Ok(FrameAction::Push(FrameRequest::EPathMap { .. }))));
        prop_assert!(matches!(overflowing.accept(summary(), &mut overflow_stats),
            Err(CanonicalProtobufAdmissionError::SizeOverflow)));
        prop_assert_eq!(overflow_stats.canonical_path_resolutions, usize::MAX);
        prop_assert!(overflowing.waiting.is_none());
        prop_assert!(overflowing.waiting_child.is_none());
        prop_assert!(overflowing.reducer.active_obligation().is_none());
    }

    #[test]
    fn epathmap_parent_nested_epathmap_resolution_overflow_follows_reducer_accept(
        width in 1usize..65,
        marker in any::<u8>(),
        source_base in 100u32..200,
    ) {
        let (_, key, child_body, outer_snapshot) =
            obligation_fixture(true, width, marker, NestedChildCase::MapStable);
        let root_bytes = payload(crate::rust::canonical_path::tag::EPATHMAP_MAP, &outer_snapshot);
        let (root, anchors, _) = fragmented(&root_bytes, source_base);
        let snapshot_at = one_offset(&root_bytes, &outer_snapshot);
        let child_anchors = indexed_act_obligation_anchors(
            &outer_snapshot,
            &anchors[snapshot_at..snapshot_at + outer_snapshot.len()],
            &key,
            &child_body,
        );

        for (resolutions_before, overflows) in [(17usize, false), (usize::MAX, true)] {
            let mut stats = CanonicalProtobufAdmissionStats {
                canonical_path_resolutions: resolutions_before,
                frame_variant_bytes: std::mem::size_of::<Frame>(),
                ..Default::default()
            };
            let mut path = CanonicalPathFrame::new(root.clone());
            let FrameAction::Push(parent_request) = path.step(&mut stats).unwrap() else {
                panic!("generated CanonicalPath root did not select its EPathMap child")
            };
            prop_assert_eq!(parent_request.request_kind(), FrameRequestKind::EPathMap);
            let mut frames = vec![Frame::CanonicalPath(path)];
            push_admission_frame(&mut frames, parent_request, &mut stats).unwrap();
            let Frame::EPathMap(parent) = &mut frames[1] else { unreachable!() };
            let FrameAction::Push(child_request) = epathmap_step(parent, &mut stats).unwrap() else {
                panic!("generated EPathMap parent did not select its nested EPathMap child")
            };
            prop_assert_eq!(child_request.request_kind(), FrameRequestKind::EPathMap);
            prop_assert_eq!(child_request.region().bytes().collect::<Vec<_>>(), child_body.clone());
            for (index, anchor) in child_anchors.iter().enumerate() {
                prop_assert_eq!(child_request.region().position_at(index).unwrap().physical, *anchor);
            }
            prop_assert_eq!(child_request.region().end_position().physical, *child_anchors.last().unwrap());
            let selected_child = child_request.region().clone();
            let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
            let Some(EPathMapWait::Obligation(wait)) = parent.waiting.as_ref() else {
                panic!("EPathMap parent did not retain its nested obligation")
            };
            prop_assert_eq!(wait.kind, CanonicalPathObligationKind::NestedEpm1);
            let wait_id = wait.id;
            prop_assert!(parent.waiting_child.as_ref().unwrap().same_view(child_request.region()));

            push_admission_frame(&mut frames, child_request, &mut stats).unwrap();
            let Frame::EPathMap(child) = &mut frames[2] else { unreachable!() };
            let FrameAction::Push(value_request) = epathmap_step(child, &mut stats).unwrap() else {
                panic!("stable Map child did not select its value")
            };
            prop_assert_eq!(value_request.request_kind(), FrameRequestKind::Message);
            let (value_summary, scores, _) =
                drive_admission(value_request.clone(), value_request.region().len()).unwrap();
            let Frame::EPathMap(child) = &mut frames[2] else { unreachable!() };
            prop_assert!(matches!(child.accept(value_summary, &mut stats),
                Ok(FrameAcceptEffect::EPathMapValue(_))));
            let Frame::EPathMap(child) = &mut frames[2] else { unreachable!() };
            let FrameAction::Complete(summary) = epathmap_step(child, &mut stats).unwrap() else {
                panic!("stable Map child did not complete after its value")
            };
            let FrameSummary::EPathMap(completed) = &summary else { unreachable!() };
            prop_assert_eq!(completed.mode, EPathMapMode::Map);
            prop_assert!(completed.entries_canonical_path_stable);
            prop_assert!(completed.snapshot.same_view(&selected_child));
            let Frame::EPathMap(child) = &frames[2] else { unreachable!() };
            prop_assert!(child.cursor.stats().terminal_checks > 0);

            let driver = DriverStepSource::before(&frames, &scores);
            let Frame::EPathMap(parent_before) = &frames[1] else { unreachable!() };
            let parent_bytes: Vec<_> = parent_before.snapshot.bytes().collect();
            let parent_anchors: Vec<_> = (0..parent_before.snapshot.len())
                .map(|at| parent_before.snapshot.position_at(at).unwrap().physical)
                .collect();
            let parent_end = parent_before.snapshot.end_position().physical;
            let parent_cursor = parent_before.cursor.stats();
            let parent_mode = parent_before.mode;
            let parent_entries_stable = parent_before.entries_canonical_path_stable;
            let score_entries = scores.source_len();
            rpb3_root_finish_trace::start_with_active_frames();
            let result = complete_admission_step_with_source(
                &driver, &mut frames, summary, None, &scores, &mut stats, None,
            );
            let trace = rpb3_root_finish_trace::take();
            prop_assert_eq!(frames.len(), 2, "driver must pop exactly the completed child");
            prop_assert_eq!(scores.source_len(), score_entries);
            prop_assert_eq!(trace.pop_accepts.len(), 1);
            let pop = &trace.pop_accepts[0];
            prop_assert_eq!(pop.before_len, 3);
            prop_assert_eq!(pop.child_kind, FrameRequestKind::EPathMap);
            prop_assert_eq!(pop.parent_kind, Some(FrameRequestKind::EPathMap));
            prop_assert_eq!(pop.epathmap_returned_snapshot.as_deref(), Some(child_body.as_slice()));
            prop_assert_eq!(pop.epathmap_returned_mode, Some(EPathMapMode::Map));
            prop_assert_eq!(pop.epathmap_returned_entries_stable, Some(true));
            prop_assert_eq!(pop.source_len_before, score_entries);
            let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(observed_before)) =
                pop.parent_nonmessage_before.as_ref() else {
                    panic!("driver must capture the suspended EPathMap parent before pop")
                };
            prop_assert_eq!(observed_before.snapshot.as_slice(), parent_bytes.as_slice());
            prop_assert_eq!(observed_before.waiting_obligation.map(|wait| wait.id), Some(wait_id));
            if overflows {
                prop_assert!(matches!(result, Err(CanonicalProtobufAdmissionError::SizeOverflow)));
                prop_assert_eq!(stats.canonical_path_resolutions, usize::MAX);
                prop_assert!(pop.applied_obligation.is_none(), "failed checked add has no successful accept effect");
                prop_assert!(pop.parent_nonmessage_after.is_none());
            } else {
                let (outcome, mut completion_source, accept_effect) =
                    result.expect("nested child must return to its parent");
                prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::AcceptedOther));
                prop_assert!(accept_effect.is_some());
                prop_assert!(emit_checked_admission_event(CheckedAdmissionEvent {
                    before: &driver,
                    after_frames: &frames,
                    after_scores: &scores,
                    action: CheckedAdmissionAction::OtherComplete {
                        outcome: &outcome,
                        source: &completion_source,
                        effect: accept_effect.as_ref(),
                        stats: &stats,
                    },
                    map_source: None,
                }).is_ok(), "accepted nested EPathMap completion must pass the checked event");
                prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                    before: &driver,
                    after_frames: &frames,
                    after_scores: &scores,
                    action: CheckedAdmissionAction::OtherComplete {
                        outcome: &outcome,
                        source: &completion_source,
                        effect: None,
                        stats: &stats,
                    },
                    map_source: None,
                }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "missing accept effect must be rejected");
                completion_source.resolutions_before_accept += 1;
                prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                    before: &driver,
                    after_frames: &frames,
                    after_scores: &scores,
                    action: CheckedAdmissionAction::OtherComplete {
                        outcome: &outcome,
                        source: &completion_source,
                        effect: accept_effect.as_ref(),
                        stats: &stats,
                    },
                    map_source: None,
                }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "changed resolution count must be rejected");
                completion_source.resolutions_before_accept -= 1;
                let Some(CompleteParentSource::EPathMap {
                    returned: EPathMapReturnSource::Obligation {
                        resolution: CanonicalPathObligationResolution::NestedEpm1 {
                            entries_stable,
                            ..
                        },
                        ..
                    },
                    ..
                }) = completion_source.parent.as_mut() else {
                    panic!("nested child completion lost its EPathMap resolution source")
                };
                *entries_stable = !*entries_stable;
                prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                    before: &driver,
                    after_frames: &frames,
                    after_scores: &scores,
                    action: CheckedAdmissionAction::OtherComplete {
                        outcome: &outcome,
                        source: &completion_source,
                        effect: accept_effect.as_ref(),
                        stats: &stats,
                    },
                    map_source: None,
                }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "changed nested EPathMap stability must be rejected");
                let (id, resolution) = pop.applied_obligation
                    .expect("driver recorded the EPathMap parent's nested resolution");
                prop_assert_eq!(id, wait_id);
                prop_assert_eq!(resolution, CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: true,
                });
                prop_assert_eq!(stats.canonical_path_resolutions, resolutions_before + 1);
                prop_assert_eq!(pop.after_len, Some(2));
                prop_assert_eq!(pop.source_len_after, Some(score_entries));
                let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(observed_after)) =
                    pop.parent_nonmessage_after.as_ref() else {
                        panic!("driver must record its accepted EPathMap parent")
                    };
                prop_assert_eq!(observed_after.snapshot.as_slice(), parent_bytes.as_slice());
                prop_assert!(observed_after.waiting_obligation.is_none());
                prop_assert!(observed_after.active_obligation.is_none());
            }
            let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
            prop_assert_eq!(parent.snapshot.bytes().collect::<Vec<_>>(), parent_bytes);
            for (at, anchor) in parent_anchors.iter().enumerate() {
                prop_assert_eq!(parent.snapshot.position_at(at).unwrap().physical, *anchor);
            }
            prop_assert_eq!(parent.snapshot.end_position().physical, parent_end);
            prop_assert_eq!(parent.cursor.stats(), parent_cursor);
            prop_assert_eq!(parent.mode, parent_mode);
            prop_assert_eq!(parent.entries_canonical_path_stable, parent_entries_stable);
            prop_assert!(parent.waiting.is_none());
            prop_assert!(parent.waiting_child.is_none());
            prop_assert!(parent.path.active_obligation().is_none());
        }
    }

    #[test]
    fn fragmented_epathmap_obligation_return_uses_the_popped_child_summary(
        width in 1usize..65,
        marker in any::<u8>(),
        source_base in 100u32..200,
    ) {
        for nested in [false, true] {
            let (bytes, key, child_body, outer_snapshot) =
                obligation_fixture(nested, width, marker, NestedChildCase::MapStable);
            let (input, anchors, _terminal) = fragmented(&bytes, source_base);
            let snapshot_at = one_offset(&bytes, &outer_snapshot);
            let expected_anchors = indexed_act_obligation_anchors(
                &outer_snapshot,
                &anchors[snapshot_at..snapshot_at + outer_snapshot.len()],
                &key,
                &child_body,
            );
            let expected_start = expected_anchors[0];
            let expected_end = *expected_anchors.last().unwrap();
            let kind = if nested { CanonicalPathObligationKind::NestedEpm1 }
                else { CanonicalPathObligationKind::RawParEscape };
            let child_kind = if nested { FrameRequestKind::EPathMap }
                else { FrameRequestKind::Message };

            let root_bytes = payload(crate::rust::canonical_path::tag::EPATHMAP_MAP, &outer_snapshot);
            let (root, _, _) = fragmented(&root_bytes, source_base + 200);
            let mut push_stats = CanonicalProtobufAdmissionStats::default();
            let mut path = CanonicalPathFrame::new(root);
            let FrameAction::Push(parent_request) = path.step(&mut push_stats).unwrap() else {
                panic!("generated CanonicalPath root did not select its EPathMap parent")
            };
            let mut frames = vec![Frame::CanonicalPath(path)];
            push_admission_frame(&mut frames, parent_request, &mut push_stats).unwrap();
            let scores = CanonicalScoreArena::new();
            let before = DriverStepSource::before(&frames, &scores);
            let Frame::EPathMap(parent) = &mut frames[1] else { unreachable!() };
            let FrameAction::Push(child_request) = epathmap_step(parent, &mut push_stats).unwrap() else {
                panic!("generated EPathMap parent did not select its path obligation")
            };
            let Frame::EPathMap(parent) = &frames[1] else { unreachable!() };
            let selected = child_request.region().clone();
            prop_assert_eq!(child_request.request_kind(), child_kind);
            let selected_bytes = selected.bytes().collect::<Vec<_>>();
            prop_assert_eq!(selected_bytes.as_slice(), child_body.as_slice());
            prop_assert!(parent.waiting_child.as_ref().is_some_and(|child| child.same_view(&selected)));
            let Some(EPathMapWait::Obligation(wait)) = parent.waiting.as_ref() else {
                panic!("EPathMap parent did not retain its path obligation")
            };
            let progress = parent.path.active_obligation().expect("path obligation remains active");
            prop_assert_eq!(wait.id, progress.obligation.id);
            prop_assert_eq!(wait.kind, kind);
            prop_assert_eq!(progress.obligation.kind, kind);
            prop_assert_eq!(progress.remaining, 0);
            prop_assert!(progress.awaiting_resolution);
            prop_assert!(parent.matches_push_request(&child_request));

            let wrong_view = IndexedByteRegion::from_borrowed(&child_body, source_base + 400, 30_000);
            prop_assert!(!selected.same_view(&wrong_view));
            let wrong_view_request = if nested {
                FrameRequest::EPathMap {
                    region: wrong_view,
                    owner: parent.owner,
                    field: parent.field,
                }
            } else {
                FrameRequest::Message {
                    region: wrong_view,
                    program: &PAR_PROTOBUF_REGION_PROGRAM,
                }
            };
            prop_assert!(!parent.matches_push_request(&wrong_view_request));
            prop_assert!(!parent.matches_push_request(&FrameRequest::CanonicalPath(selected.clone())));
            if nested {
                prop_assert!(!std::ptr::eq(parent.owner, &EPATHMAP_RHO_MATCH_PROGRAM));
                let wrong_owner = FrameRequest::EPathMap {
                    region: selected.clone(),
                    owner: &EPATHMAP_RHO_MATCH_PROGRAM,
                    field: parent.field,
                };
                prop_assert!(!parent.matches_push_request(&wrong_owner));
                prop_assert_ne!(parent.field, parent.field + 1);
                let wrong_field = FrameRequest::EPathMap {
                    region: selected,
                    owner: parent.owner,
                    field: parent.field + 1,
                };
                prop_assert!(!parent.matches_push_request(&wrong_field));
            } else {
                prop_assert!(!std::ptr::eq(&PAR_PROTOBUF_REGION_PROGRAM, &EPATHMAP_RHO_MATCH_PROGRAM));
                let wrong_program = FrameRequest::Message {
                    region: selected,
                    program: &EPATHMAP_RHO_MATCH_PROGRAM,
                };
                prop_assert!(!parent.matches_push_request(&wrong_program));
            }
            let (pushed, pushed_request) = push_admission_frame_with_source(
                &mut frames, child_request.clone(), &mut push_stats,
            ).unwrap();
            prop_assert!(child_request.matches_pushed_frame(frames.last().unwrap()));
            prop_assert!(emit_checked_admission_event(CheckedAdmissionEvent {
                before: &before,
                after_frames: &frames,
                after_scores: &scores,
                action: CheckedAdmissionAction::OtherPush {
                    pushed: &pushed,
                    request: &pushed_request,
                },
                map_source: None,
            }).is_ok(), "selected EPathMap child push must pass the checked event");
            prop_assert!(matches!(emit_checked_admission_event(CheckedAdmissionEvent {
                before: &before,
                after_frames: &frames,
                after_scores: &scores,
                action: CheckedAdmissionAction::OtherPush {
                    pushed: &pushed,
                    request: &wrong_view_request,
                },
                map_source: None,
            }), Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))), "forged child view must be rejected");
            match frames.last_mut().unwrap() {
                Frame::Message(pushed) => {
                    prop_assert!(!nested);
                    pushed.slots.admit(0, false).expect("fresh Par child has slot zero");
                }
                Frame::EPathMap(pushed) => {
                    prop_assert!(nested);
                    pushed.mode = EPathMapMode::Set;
                }
                _ => unreachable!(),
            }
            prop_assert!(!child_request.matches_pushed_frame(frames.last().unwrap()));

            let (scalar_result, scalar_trace) = standalone_path_admission::with_epm_transport_mode(true, || {
                rpb3_root_finish_trace::start_with_active_frames();
                let result = validate_canonical_protobuf_inner(
                    input.clone(), &PAR_PROTOBUF_REGION_PROGRAM, None,
                );
                (result, rpb3_root_finish_trace::take())
            });
            rpb3_root_finish_trace::start_with_active_frames();
            let result = validate_canonical_protobuf_inner(
                input, &PAR_PROTOBUF_REGION_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            prop_assert!(scalar_result.is_ok(), "scalar obligation fixture rejected: {:?}", scalar_result.as_ref().err());
            prop_assert_eq!(&scalar_result, &result);
            prop_assert!(result.is_ok(), "obligation fixture rejected: {:?}", result.as_ref().err());
            let admitted = result.unwrap();
            let stats = admitted.stats();
            prop_assert_eq!(stats.canonical_path_obligations, 1);
            prop_assert_eq!(stats.canonical_path_resolutions, 1);
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace, &PAR_PROTOBUF_REGION_PROGRAM, "generated EPathMap obligation",
            );
            let scalar_terminal = obligation_push_terminal(
                &scalar_trace, kind, &child_body, &expected_anchors, child_kind,
            );
            let batched_terminal = obligation_push_terminal(
                &trace, kind, &child_body, &expected_anchors, child_kind,
            );
            prop_assert_eq!(scalar_terminal.cursor, batched_terminal.cursor);
            prop_assert_eq!(scalar_terminal.active_obligation, batched_terminal.active_obligation);
            prop_assert_eq!(scalar_terminal.waiting_obligation, batched_terminal.waiting_obligation);
            prop_assert_eq!(scalar_terminal.last_source, batched_terminal.last_source);
            let scalar_has_no_batch_events = scalar_trace.epathmap_events.iter().all(|event|
                !matches!(&event.kind,
                    rpb3_root_finish_trace::EPathMapEventKind::Forward { .. }
                    | rpb3_root_finish_trace::EPathMapEventKind::Unwind { .. }));
            prop_assert!(scalar_has_no_batch_events);

            let pushes: Vec<_> = trace.nonmessage_push_bodies.iter()
                .filter(|body| body.obligation.is_some_and(|wait| wait.kind == kind))
                .collect();
            prop_assert_eq!(pushes.len(), 1);
            let push = pushes[0];
            prop_assert_eq!(push.body.as_slice(), child_body.as_slice());
            prop_assert_eq!(push.start, expected_start);
            prop_assert_eq!(push.end, expected_end);
            let push_step = &trace.driver_steps[push.step_index];
            prop_assert_eq!(push_step.before_top_kind, FrameRequestKind::EPathMap);
            prop_assert_eq!(push_step.after_top_kind, Some(child_kind));
            prop_assert_eq!(push_step.after_top_region, Some((expected_start, child_body.len())));
            let before = push_step.active_frames_before.as_ref().expect("active stack enabled");
            let after = push_step.active_frames_after.as_ref().expect("active stack enabled");
            prop_assert_eq!(after.len(), before.len() + 1);
            let (pushed_bytes, child_anchors) = match after.last().unwrap() {
                rpb3_root_finish_trace::ActiveFrameState::Message { bytes, anchors, .. } => {
                    prop_assert!(!nested, "nested EPM obligation pushed a Message");
                    prop_assert!(std::ptr::eq(
                        push_step.after_top_program.expect("raw-Par child program"),
                        &PAR_PROTOBUF_REGION_PROGRAM,
                    ));
                    (bytes, anchors)
                }
                rpb3_root_finish_trace::ActiveFrameState::NonMessage { bytes, anchors, .. } => {
                    prop_assert!(nested, "raw-Par obligation pushed a nonmessage frame");
                    prop_assert!(push_step.after_top_program.is_none());
                    (bytes, anchors)
                }
            };
            prop_assert_eq!(pushed_bytes.as_slice(), child_body.as_slice());
            prop_assert_eq!(child_anchors.as_slice(), expected_anchors.as_slice());

            let pops: Vec<_> = trace.pop_accepts.iter().filter(|pop| {
                pop.child_kind == child_kind && matches!(
                    pop.parent_nonmessage_before.as_ref(),
                    Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent))
                        if parent.waiting_obligation.is_some_and(|wait| wait.kind == kind)
                )
            }).collect();
            prop_assert_eq!(pops.len(), 1);
            let pop = pops[0];
            let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent_before)) =
                pop.parent_nonmessage_before.as_ref() else { panic!("EPathMap waited for child"); };
            let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent_after)) =
                pop.parent_nonmessage_after.as_ref() else { panic!("EPathMap accepted child"); };
            prop_assert_eq!(parent_before.snapshot.as_slice(), parent_after.snapshot.as_slice());
            prop_assert_eq!(parent_before.region_start, parent_after.region_start);
            prop_assert_eq!(parent_before.region_end, parent_after.region_end);
            prop_assert_eq!(parent_before.cursor, parent_after.cursor);
            prop_assert_eq!(parent_before.mode, parent_after.mode);
            prop_assert_eq!(parent_before.entries_canonical_path_stable,
                parent_after.entries_canonical_path_stable);
            prop_assert_eq!(parent_before.last_source, parent_after.last_source);
            prop_assert!(parent_before.waiting_obligation.is_some());
            prop_assert!(parent_after.waiting_obligation.is_none());
            prop_assert!(parent_after.active_obligation.is_none());
            prop_assert_eq!(pop.source_len_before, pop.source_len_after.unwrap());
            let wait = parent_before.waiting_obligation.expect("real EPM obligation wait");
            prop_assert_eq!(push.obligation, Some(wait));
            let (applied_id, applied_resolution) = pop.applied_obligation
                .expect("EPM parent applied its child's resolution");
            prop_assert_eq!(applied_id, wait.id);

            if nested {
                let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(child)) =
                    pop.child_nonmessage.as_ref() else { panic!("nested EPM child completed"); };
                let parsed = epathmap_trie_codec::inspect_canonical_snapshot_value_regions(&child_body)
                    .expect("generated EPM child snapshot is canonical");
                prop_assert_eq!(child.snapshot.as_slice(), child_body.as_slice());
                prop_assert_eq!(child.mode, parsed.mode);
                prop_assert!(child.entries_canonical_path_stable);
                prop_assert!(child.cursor.terminal_checks > 0);
                prop_assert!(child.waiting_obligation.is_none());
                prop_assert!(child.active_obligation.is_none());
                prop_assert_eq!(pop.epathmap_returned_snapshot.as_deref(), Some(child_body.as_slice()));
                prop_assert_eq!(pop.epathmap_returned_mode, Some(child.mode));
                prop_assert_eq!(pop.epathmap_returned_entries_stable,
                    Some(child.entries_canonical_path_stable));
                let CanonicalPathObligationResolution::NestedEpm1 { mode, entries_stable } =
                    applied_resolution else { panic!("nested EPM return applies nested resolution"); };
                prop_assert_eq!(mode, NestedEpmMode::Map);
                prop_assert_eq!(entries_stable, child.entries_canonical_path_stable);
            } else {
                let Some(rpb3_root_finish_trace::RootReturnedState::Par(returned)) =
                    pop.typed_summary.as_ref() else { panic!("raw-Par child returned Par summary"); };
                prop_assert!(pop.epathmap_returned_mode.is_none());
                prop_assert!(pop.child_frame.is_some());
                let CanonicalPathObligationResolution::RawPar { canonical_path_stable } =
                    applied_resolution else { panic!("raw-Par return applies raw-Par resolution"); };
                prop_assert_eq!(canonical_path_stable, returned.canonical_path_stable);
            }
        }
    }

    #[test]
    fn canonical_nested_epm_modes_and_unstable_entries_return_before_path_rejection(
        width in 1usize..65,
        marker in any::<u8>(),
        source_base in 200u32..300,
    ) {
        for (case, expected_mode, expected_stable, expected_error) in [
            (NestedChildCase::Empty, EPathMapMode::Empty, true, "NestedEpmNotMap"),
            (NestedChildCase::SetStable, EPathMapMode::Set, true, "NestedEpmNotMap"),
            (NestedChildCase::SetUnstable, EPathMapMode::Set, false, "NestedEpmNotMap"),
            (NestedChildCase::MapUnstable, EPathMapMode::Map, false, "NestedEpmNotStable"),
        ] {
            let (bytes, key, child_body, outer_snapshot) =
                obligation_fixture(true, width, marker, case);
            let (input, anchors, _terminal) = fragmented(&bytes, source_base);
            let snapshot_at = one_offset(&bytes, &outer_snapshot);
            let expected_anchors = indexed_act_obligation_anchors(
                &outer_snapshot,
                &anchors[snapshot_at..snapshot_at + outer_snapshot.len()],
                &key,
                &child_body,
            );
            rpb3_root_finish_trace::start_with_active_frames();
            let result = validate_canonical_protobuf_inner(
                input, &PAR_PROTOBUF_REGION_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            let error = result.err().expect("a non-Map or unstable nested key is rejected");
            let detail = match error {
                CanonicalProtobufAdmissionError::CanonicalPath { detail, .. } => detail,
                other => panic!("nested child rejection must identify its canonical path error: {other:?}"),
            };
            prop_assert!(detail.contains(expected_error), "wrong nested child rejection: {detail}");

            let pushes: Vec<_> = trace.nonmessage_push_bodies.iter().filter(|push| {
                push.obligation.is_some_and(|wait| wait.kind == CanonicalPathObligationKind::NestedEpm1)
                    && trace.driver_steps[push.step_index].before_top_kind == FrameRequestKind::EPathMap
            }).collect();
            prop_assert_eq!(pushes.len(), 1);
            let push = pushes[0];
            prop_assert_eq!(push.body.as_slice(), child_body.as_slice());
            prop_assert_eq!(push.start, expected_anchors[0]);
            prop_assert_eq!(push.end, *expected_anchors.last().unwrap());
            let push_step = &trace.driver_steps[push.step_index];
            prop_assert_eq!(push_step.after_top_kind, Some(FrameRequestKind::EPathMap));
            let pushed = push_step.active_frames_after.as_ref()
                .expect("active stack enabled").last().unwrap();
            let rpb3_root_finish_trace::ActiveFrameState::NonMessage {
                bytes: pushed_bytes, anchors: pushed_anchors, ..
            } = pushed else { panic!("nested obligation pushes EPathMap"); };
            prop_assert_eq!(pushed_bytes.as_slice(), child_body.as_slice());
            prop_assert_eq!(pushed_anchors.as_slice(), expected_anchors.as_slice());

            let pops: Vec<_> = trace.pop_accepts.iter().filter(|pop| {
                pop.child_kind == FrameRequestKind::EPathMap && matches!(
                    pop.parent_nonmessage_before.as_ref(),
                    Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent))
                        if parent.waiting_obligation.is_some_and(|wait|
                            wait.kind == CanonicalPathObligationKind::NestedEpm1)
                )
            }).collect();
            prop_assert_eq!(pops.len(), 1);
            let pop = pops[0];
            let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(parent)) =
                pop.parent_nonmessage_before.as_ref() else { panic!("outer EPM waits for child"); };
            prop_assert_eq!(push.obligation, parent.waiting_obligation);
            let Some(rpb3_root_finish_trace::NonMessageFrameState::EPathMap(child)) =
                pop.child_nonmessage.as_ref() else { panic!("nested EPM child completed"); };
            let parsed = epathmap_trie_codec::inspect_canonical_snapshot_value_regions(&child_body)
                .expect("generated child has a canonical EPM1 image");
            prop_assert_eq!(child.snapshot.as_slice(), child_body.as_slice());
            prop_assert_eq!(parsed.mode, expected_mode);
            prop_assert_eq!(child.mode, expected_mode);
            prop_assert_eq!(child.entries_canonical_path_stable, expected_stable);
            prop_assert!(child.cursor.terminal_checks > 0);
            prop_assert!(child.waiting_obligation.is_none());
            prop_assert_eq!(pop.epathmap_returned_mode, Some(child.mode));
            prop_assert_eq!(pop.epathmap_returned_entries_stable,
                Some(child.entries_canonical_path_stable));
            if let Some((applied_id, applied_resolution)) = pop.applied_obligation {
                prop_assert_eq!(Some(applied_id), parent.waiting_obligation.map(|wait| wait.id));
                let CanonicalPathObligationResolution::NestedEpm1 { mode, entries_stable } =
                    applied_resolution else { panic!("applied nested return has nested resolution"); };
                prop_assert_eq!(mode, match expected_mode {
                    EPathMapMode::Empty => NestedEpmMode::Empty,
                    EPathMapMode::Set => NestedEpmMode::Set,
                    EPathMapMode::Map => NestedEpmMode::Map,
                });
                prop_assert_eq!(entries_stable, expected_stable);
            }
        }
    }

    #[test]
    fn fragmented_canonical_path_pushes_nested_epathmap_by_obligation_kind(
        width in 1usize..65,
        marker in any::<u8>(),
        source_base in 300u32..400,
    ) {
        let (_root, key, child_body, _snapshot) =
            obligation_fixture(true, width, marker, NestedChildCase::MapStable);
        let (input, anchors, _terminal) = fragmented(&key, source_base);
        let child_at = one_offset(&key, &child_body);
        rpb3_root_finish_trace::start_with_active_frames();
        let result = validate_canonical_path_indexed(input);
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(result.is_ok(), "nested canonical path rejected: {:?}", result.as_ref().err());
        let (_, stats) = result.unwrap();
        prop_assert_eq!(stats.canonical_path_obligations, 1);
        prop_assert_eq!(stats.canonical_path_resolutions, 1);
        rpb3_root_finish_trace::assert_nonmessage_event_projection(
            &trace, "generated CanonicalPath nested EPM obligation",
        );
        let pushes: Vec<_> = trace.nonmessage_push_bodies.iter()
            .filter(|body| {
                body.obligation.is_some_and(|wait| wait.kind == CanonicalPathObligationKind::NestedEpm1)
                    && trace.driver_steps[body.step_index].before_top_kind == FrameRequestKind::CanonicalPath
            })
            .collect();
        prop_assert_eq!(pushes.len(), 1);
        let push = pushes[0];
        prop_assert_eq!(push.obligation.map(|wait| wait.kind),
            Some(CanonicalPathObligationKind::NestedEpm1));
        prop_assert_eq!(push.body.as_slice(), child_body.as_slice());
        prop_assert_eq!(push.start, anchors[child_at]);
        prop_assert_eq!(push.end, anchors[child_at + child_body.len() - 1]);
        let step = &trace.driver_steps[push.step_index];
        prop_assert_eq!(step.after_top_kind, Some(FrameRequestKind::EPathMap));
        prop_assert!(step.after_top_program.is_none());
        let pushed = step.active_frames_after.as_ref().expect("active stack enabled").last().unwrap();
        let rpb3_root_finish_trace::ActiveFrameState::NonMessage { bytes, anchors: child_anchors, .. } = pushed
            else { panic!("nested path obligation pushes an EPathMap frame"); };
        prop_assert_eq!(bytes.as_slice(), child_body.as_slice());
        prop_assert_eq!(child_anchors.as_slice(), &anchors[child_at..child_at + child_body.len()]);
    }
}
