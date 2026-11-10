use std::ops::Range;

use proptest::prelude::*;
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieCursorSourcePosition as ActSourcePosition,
    CanonicalCompactTrieCursorSourceStage as ActSourceStage,
};

use super::rpb3_root_finish_trace::{
    ActiveFrameState, DriverStepKind, EPathMapEventKind, EPathMapEventState, EPathMapFrameState,
    NonMessageFrameState, NonMessagePushBodyState, RootFinishTrace,
};
use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::encode_trie_path;
use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;

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

fn fixture(width: usize, marker: u8, map_mode: bool) -> (Vec<u8>, Vec<u8>) {
    let mut stable_data = vec![37; width];
    *stable_data.last_mut().unwrap() = marker;
    let stable = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(stable_data)),
    }]);
    let mut escape_data = vec![73; width];
    *escape_data.last_mut().unwrap() = marker.wrapping_add(1);
    let escaped = Par::default()
        .with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(escape_data)),
        }])
        .with_connective_used(true);
    let stable_key = encode_trie_path(&stable);
    let escape_key = encode_trie_path(&escaped);
    assert_eq!(stable_key[0], 6);
    assert_eq!(escape_key[0], 15);
    let snapshot = if map_mode {
        let mut trie = pathmap::PathMap::new();
        trie.insert(&stable_key, Par::default());
        trie.insert(&escape_key, Par::default());
        epathmap_trie_codec::encode(&EPathMapRepr::Map(trie))
    } else {
        let mut trie = pathmap::PathMap::new();
        trie.insert(&stable_key, ());
        trie.insert(&escape_key, ());
        epathmap_trie_codec::encode(&EPathMapRepr::<Par>::Set(trie))
    };
    let outer = field(5, &field(32, &field(9, &snapshot)));
    (outer, snapshot)
}

fn map_value_fixture(width: usize, marker: u8) -> (Vec<u8>, Vec<u8>) {
    let first_key = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![marker; width])),
    }]);
    let second_key = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![
            marker.wrapping_add(1);
            width
        ])),
    }]);
    let third_key = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![
            marker.wrapping_add(2);
            width
        ])),
    }]);
    let short_value = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![marker; 3])),
    }]);
    let long_value = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![marker.wrapping_add(3); 150])),
    }]);
    let mut trie = pathmap::PathMap::new();
    trie.insert(&encode_trie_path(&first_key), Par::default());
    trie.insert(&encode_trie_path(&second_key), short_value);
    trie.insert(&encode_trie_path(&third_key), long_value);
    let snapshot = epathmap_trie_codec::encode(&EPathMapRepr::Map(trie));
    (field(5, &field(32, &field(9, &snapshot))), snapshot)
}

// Parse the wire image directly; neither the production cursor nor its value
// region inspector supplies the expected offsets for this property.
fn independently_parse_map_value_table(
    snapshot: &[u8],
) -> (Range<usize>, Vec<(Range<usize>, Range<usize>)>) {
    fn varint(snapshot: &[u8], at: &mut usize) -> usize {
        let mut value = 0u128;
        for shift in (0..70).step_by(7) {
            let byte = *snapshot.get(*at).expect("complete EPM1 varint");
            *at += 1;
            value |= u128::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return usize::try_from(value).expect("EPM1 length fits usize");
            }
        }
        panic!("EPM1 varint exceeds ten bytes");
    }

    assert_eq!(&snapshot[..4], b"EPM1");
    assert_eq!(snapshot[4], 1);
    assert_eq!(
        EPathMapMode::try_from(snapshot[5]).unwrap(),
        EPathMapMode::Map
    );
    let mut at = 6;
    let arena_len = varint(snapshot, &mut at);
    let trie_start = at;
    at = at.checked_add(arena_len).expect("ACT arena end fits");
    assert!(at <= snapshot.len());
    let trie_range = trie_start..at;
    let value_count = varint(snapshot, &mut at);
    assert_eq!(value_count, 3);
    let mut records = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let prefix_start = at;
        let body_len = varint(snapshot, &mut at);
        let body_start = at;
        at = at.checked_add(body_len).expect("value body end fits");
        assert!(at <= snapshot.len());
        records.push((prefix_start..body_start, body_start..at));
    }
    assert_eq!(at, snapshot.len());
    (trie_range, records)
}

fn expected_map_value_act_after(before: ActSourcePosition) -> ActSourcePosition {
    match before.stage {
        ActSourceStage::Value => {
            assert!(before.line_start.is_none());
            assert!(before.line_end.is_none());
            ActSourcePosition {
                stage: ActSourceStage::Children,
                ..before
            }
        }
        ActSourceStage::LineValue => {
            let line_len = before.line_end.unwrap() - before.line_start.unwrap();
            let next_index = before.line_index.checked_add(1).unwrap();
            assert_eq!(next_index, line_len);
            ActSourcePosition {
                line_index: next_index,
                stage: ActSourceStage::Children,
                ..before
            }
        }
        _ => panic!("a Map Value has a Value or final LineValue ACT source stage"),
    }
}

fn assert_boundary(scalar: &EPathMapEventState, batched: &EPathMapEventState) {
    assert_eq!(batched.source, scalar.source);
    assert_eq!(batched.step_index, scalar.step_index);
    assert!(std::ptr::eq(batched.after.owner, scalar.after.owner));
    assert_eq!(batched.after.field, scalar.after.field);
    assert_eq!(batched.after.snapshot, scalar.after.snapshot);
    assert_eq!(batched.after.region_start, scalar.after.region_start);
    assert_eq!(batched.after.region_end, scalar.after.region_end);
    assert_eq!(batched.after.mode, scalar.after.mode);
    assert_eq!(batched.after.cursor, scalar.after.cursor);
    assert_eq!(batched.after.path.edges, scalar.after.path.edges);
    assert_eq!(batched.after.path.states, scalar.after.path.states);
    assert_eq!(batched.after.path.frames, scalar.after.path.frames);
    assert_eq!(
        batched.after.path.max_topology_depth,
        scalar.after.path.max_topology_depth
    );
    assert_eq!(
        batched.after.active_obligation,
        scalar.after.active_obligation
    );
    assert_eq!(
        batched.after.entries_canonical_path_stable,
        scalar.after.entries_canonical_path_stable
    );
    assert_eq!(
        batched.after.waiting_map_value,
        scalar.after.waiting_map_value
    );
    assert_eq!(
        batched.after.waiting_obligation,
        scalar.after.waiting_obligation
    );
    assert_eq!(batched.after.last_source, scalar.after.last_source);
    assert_eq!(batched.value_reducer_stable, scalar.value_reducer_stable);
    assert_eq!(batched.cursor_source, scalar.cursor_source);
}

fn take_scalar<'a>(
    events: &'a [EPathMapEventState],
    at: &mut usize,
    expected: EPathMapEventKind,
    source: CanonicalByteProvenance,
) -> &'a EPathMapEventState {
    let event = events
        .get(*at)
        .expect("batched event has a scalar counterpart");
    assert_eq!(event.kind, expected);
    assert_eq!(event.source, source);
    *at += 1;
    event
}

fn normalized_nonmessage(mut frame: NonMessageFrameState) -> NonMessageFrameState {
    if let NonMessageFrameState::EPathMap(epm) = &mut frame {
        epm.path.bulk_operations = 0;
        epm.path.bulk_bytes = 0;
    }
    frame
}

fn normalized_active_frames(frames: &[ActiveFrameState]) -> Vec<ActiveFrameState> {
    frames
        .iter()
        .cloned()
        .map(|frame| match frame {
            ActiveFrameState::NonMessage {
                frame,
                bytes,
                anchors,
            } => ActiveFrameState::NonMessage {
                frame: normalized_nonmessage(frame),
                bytes,
                anchors,
            },
            message => message,
        })
        .collect()
}

fn same_epm_frame(left: &EPathMapFrameState, right: &EPathMapFrameState) -> bool {
    std::ptr::eq(left.owner, right.owner)
        && left.field == right.field
        && left.region_start == right.region_start
        && left.region_end == right.region_end
        && left.snapshot == right.snapshot
}

fn epm_completion_for_event(trace: &RootFinishTrace, event: &EPathMapEventState) -> usize {
    let step = &trace.driver_steps[event.step_index];
    assert_eq!(step.before_top_kind, FrameRequestKind::EPathMap);
    trace
        .driver_steps
        .iter()
        .enumerate()
        .skip(event.step_index)
        .find(|(_, candidate)| {
            candidate.before_len == step.before_len
                && candidate.before_top_kind == FrameRequestKind::EPathMap
                && matches!(candidate.kind, DriverStepKind::Complete)
        })
        .map(|(index, _)| index)
        .expect("every successful EPathMap activation reaches Complete")
}

fn epm_event_groups(trace: &RootFinishTrace) -> Vec<(usize, Vec<&EPathMapEventState>)> {
    let mut groups: Vec<(usize, Vec<&EPathMapEventState>)> = Vec::new();
    for event in &trace.epathmap_events {
        let completion = epm_completion_for_event(trace, event);
        if let Some((_, events)) = groups.iter_mut().find(|(index, _)| *index == completion) {
            events.push(event);
        } else {
            groups.push((completion, vec![event]));
        }
    }
    groups
}

fn event_frame_anchors<'a>(
    trace: &'a RootFinishTrace,
    event: &EPathMapEventState,
) -> (&'a [u8], &'a [CanonicalByteProvenance]) {
    let step = &trace.driver_steps[event.step_index];
    let Some(ActiveFrameState::NonMessage {
        frame: NonMessageFrameState::EPathMap(before),
        bytes,
        anchors,
    }) = step
        .active_frames_before
        .as_ref()
        .and_then(|frames| frames.last())
    else {
        panic!("EPathMap tick has its live frame on the driver stack");
    };
    assert!(same_epm_frame(before, &event.after));
    assert_eq!(bytes.as_slice(), event.after.snapshot.as_slice());
    assert_eq!(anchors.len(), bytes.len());
    (bytes, anchors)
}

fn scoped_epm_trace(trace: &RootFinishTrace, events: &[&EPathMapEventState]) -> RootFinishTrace {
    RootFinishTrace::with_epathmap_events(
        events.iter().map(|event| (**event).clone()).collect(),
        trace
            .nonmessage_push_bodies
            .iter()
            .filter(|body| {
                events
                    .iter()
                    .any(|event| event.step_index == body.step_index)
            })
            .map(|body| NonMessagePushBodyState {
                step_index: body.step_index,
                body: body.body.clone(),
                start: body.start,
                end: body.end,
                obligation: body.obligation,
            })
            .collect(),
    )
}

fn assert_finite_epm_return_boundaries(trace: &RootFinishTrace) {
    for (step_index, step) in trace.driver_steps.iter().enumerate() {
        if step.before_top_kind != FrameRequestKind::EPathMap {
            continue;
        }
        let events = trace
            .epathmap_events
            .iter()
            .filter(|event| event.step_index == step_index)
            .collect::<Vec<_>>();
        for event in events.iter().take(events.len().saturating_sub(1)) {
            assert!(!event.after.waiting_map_value);
            assert!(event.after.waiting_obligation.is_none());
        }
        match step.kind {
            DriverStepKind::Push(_) => {
                let last = events.last().expect("EPathMap Push follows an ACT event");
                match &last.kind {
                    EPathMapEventKind::Edge(_) => {
                        assert!(!last.after.waiting_map_value);
                        assert!(last.after.waiting_obligation.is_some());
                    }
                    EPathMapEventKind::Value {
                        value_body_range: Some(_),
                        ..
                    } => {
                        assert!(last.after.waiting_map_value);
                        assert!(last.after.waiting_obligation.is_none());
                    }
                    _ => panic!("EPathMap Push must follow a requesting Edge or Value"),
                }
            }
            DriverStepKind::Complete => {
                assert!(events.last().is_none_or(|last| {
                    !last.after.waiting_map_value && last.after.waiting_obligation.is_none()
                }));
            }
            DriverStepKind::Continue => panic!("EPathMap frame returned Continue"),
        }
    }
}

fn assert_driver_actions_match(scalar: &RootFinishTrace, batched: &RootFinishTrace) {
    assert_finite_epm_return_boundaries(scalar);
    assert_finite_epm_return_boundaries(batched);
    assert_eq!(batched.driver_steps.len(), scalar.driver_steps.len());
    for (scalar, batched) in scalar.driver_steps.iter().zip(&batched.driver_steps) {
        if scalar.before_top_kind == FrameRequestKind::EPathMap {
            assert!(!matches!(scalar.kind, DriverStepKind::Continue));
            assert!(!matches!(batched.kind, DriverStepKind::Continue));
        }
        match (scalar.kind, batched.kind) {
            (DriverStepKind::Continue, DriverStepKind::Continue)
            | (DriverStepKind::Complete, DriverStepKind::Complete) => {}
            (DriverStepKind::Push(expected), DriverStepKind::Push(actual)) => {
                assert_eq!(actual, expected);
            }
            _ => panic!("batch changed the returned driver action"),
        }
        assert_eq!(batched.before_len, scalar.before_len);
        assert_eq!(batched.after_len, scalar.after_len);
        assert_eq!(batched.before_capacity, scalar.before_capacity);
        assert_eq!(batched.after_capacity, scalar.after_capacity);
        assert_eq!(batched.before_top_kind, scalar.before_top_kind);
        assert_eq!(batched.after_top_kind, scalar.after_top_kind);
        assert_eq!(batched.before_top_program, scalar.before_top_program);
        assert_eq!(batched.after_top_program, scalar.after_top_program);
        assert_eq!(batched.after_top_region, scalar.after_top_region);
        assert_eq!(batched.score_len_before, scalar.score_len_before);
        assert_eq!(batched.score_len_after, scalar.score_len_after);
        assert_eq!(batched.score_appended, scalar.score_appended);
        assert_eq!(batched.score_source_before, scalar.score_source_before);
        assert_eq!(batched.score_source_after, scalar.score_source_after);
        assert_eq!(batched.before_message, scalar.before_message);
        assert_eq!(batched.after_message, scalar.after_message);
        assert_eq!(
            batched.before_nonmessage.clone().map(normalized_nonmessage),
            scalar.before_nonmessage.clone().map(normalized_nonmessage),
        );
        assert_eq!(
            batched.after_nonmessage.clone().map(normalized_nonmessage),
            scalar.after_nonmessage.clone().map(normalized_nonmessage),
        );
        assert_eq!(
            batched
                .suspended_parent_after
                .clone()
                .map(normalized_nonmessage),
            scalar
                .suspended_parent_after
                .clone()
                .map(normalized_nonmessage),
        );
        assert_eq!(
            batched
                .active_frames_before
                .as_deref()
                .map(normalized_active_frames),
            scalar
                .active_frames_before
                .as_deref()
                .map(normalized_active_frames),
        );
        assert_eq!(
            batched
                .active_frames_after
                .as_deref()
                .map(normalized_active_frames),
            scalar
                .active_frames_after
                .as_deref()
                .map(normalized_active_frames),
        );
    }
}

fn assert_batch_expands_to_scalar(
    scalar: &RootFinishTrace,
    batched: &RootFinishTrace,
    snapshot: &[u8],
    anchors: &[CanonicalByteProvenance],
    terminal: CanonicalByteProvenance,
) {
    // Extract the loop-source successor and stop invariants against ticks
    // recorded by the real admission driver, in either transport mode.
    let assert_tick_images = |trace: &RootFinishTrace| {
        let outer_snapshot = snapshot;
        let outer_anchors = anchors;
        let outer_terminal = terminal;
        let groups = epm_event_groups(trace);
        assert!(!groups.is_empty());
        let mut found_outer = false;
        for (completion_index, group) in groups {
            let first = group[0];
            let (snapshot_for_frame, anchors_for_frame) = event_frame_anchors(trace, first);
            let snapshot = snapshot_for_frame;
            let anchors = anchors_for_frame;
            let terminal = first.after.region_end;
            found_outer |= snapshot == outer_snapshot
                && anchors == outer_anchors
                && terminal == outer_terminal;
            let mut event_index = 0usize;
            let mut map_values_seen = 0usize;
            let mut path_edges = 0usize;
            let mut depth = 0usize;
            let mut maximum_depth = 0usize;
            let mut root_left = false;
            let mut complete_entry = None;
            let mut previous: Option<&EPathMapEventState> = None;
            let owner = first.after.owner;
            let field = first.after.field;
            for (tick_index, tick) in group.iter().copied().enumerate() {
                if tick.step_index == completion_index && complete_entry.is_none() {
                    complete_entry =
                        Some((event_index, map_values_seen, path_edges, maximum_depth));
                }
                let (expanded, added_edges) = match &tick.kind {
                    EPathMapEventKind::Forward { width, .. } => {
                        (width.checked_mul(2).unwrap(), *width)
                    }
                    EPathMapEventKind::Unwind { width, .. } => (*width, 0),
                    EPathMapEventKind::Edge(_) => (1, 1),
                    _ => (1, 0),
                };
                let map_value = usize::from(matches!(&tick.kind, EPathMapEventKind::Value {
                    has_value: true,
                    value_body_range: Some(_),
                }));
                event_index = event_index.checked_add(expanded).unwrap();
                map_values_seen = map_values_seen.checked_add(map_value).unwrap();
                path_edges = path_edges.checked_add(added_edges).unwrap();
                assert!(!root_left, "ACT emitted another event after the root Leave");
                match &tick.kind {
                    EPathMapEventKind::Forward { width, .. } => depth += *width,
                    EPathMapEventKind::Edge(_) => depth += 1,
                    EPathMapEventKind::Unwind { width, .. } => {
                        depth = depth
                            .checked_sub(*width)
                            .expect("Unwind restores an open path")
                    }
                    EPathMapEventKind::Leave if depth > 0 => depth -= 1,
                    EPathMapEventKind::Leave => root_left = true,
                    EPathMapEventKind::Value { .. } => {}
                }
                maximum_depth = maximum_depth.max(depth);
                assert!(std::ptr::eq(tick.after.owner, owner));
                assert_eq!(tick.after.field, field);
                assert_eq!(tick.after.snapshot.as_slice(), snapshot);
                assert_eq!(tick.after.region_start, anchors[0]);
                assert_eq!(tick.after.region_end, terminal);
                assert_eq!(tick.after.cursor.topology_events, event_index);
                assert_eq!(tick.after.cursor.value_records, map_values_seen);
                assert_eq!(tick.after.cursor.terminal_checks, 0);
                assert_eq!(tick.after.path.edges, path_edges);
                assert_eq!(tick.after.path.max_topology_depth, maximum_depth);
                assert_eq!(
                    tick.after.last_source,
                    CanonicalPathSource::in_source(tick.source.source, tick.source.offset),
                );
                if let Some(prior) = previous {
                    assert!(prior.step_index <= tick.step_index);
                    if prior.step_index == tick.step_index {
                        assert!(
                            tick.after.path.max_topology_depth
                                >= prior.after.path.max_topology_depth
                        );
                        if !matches!(&tick.kind, EPathMapEventKind::Value { .. }) {
                            assert_eq!(
                                tick.after.entries_canonical_path_stable,
                                prior.after.entries_canonical_path_stable,
                            );
                        }
                    }
                }
                let step = &trace.driver_steps[tick.step_index];
                assert_eq!(step.before_top_kind, FrameRequestKind::EPathMap);
                let final_tick = group
                    .get(tick_index + 1)
                    .is_none_or(|next| next.step_index != tick.step_index);
                let stopped_for_push = final_tick && matches!(step.kind, DriverStepKind::Push(_));
                let edge_push =
                    stopped_for_push && matches!(&tick.kind, EPathMapEventKind::Edge(_));
                let value_push =
                    stopped_for_push && matches!(&tick.kind, EPathMapEventKind::Value { .. });
                assert_eq!(tick.after.waiting_obligation.is_some(), edge_push);
                assert_eq!(tick.after.waiting_map_value, value_push);
                if edge_push {
                    let waiting = tick.after.waiting_obligation.unwrap();
                    let progress = tick.after.active_obligation.unwrap();
                    assert_eq!(progress.obligation.id, waiting.id);
                    assert_eq!(progress.obligation.kind, waiting.kind);
                    assert_eq!(progress.remaining, 0);
                    assert!(progress.awaiting_resolution);
                }
                if value_push {
                    assert!(tick.after.active_obligation.is_none());
                }
                previous = Some(tick);
            }
            let complete = &trace.driver_steps[completion_index];
            assert_eq!(complete.before_top_kind, FrameRequestKind::EPathMap);
            assert!(matches!(complete.kind, DriverStepKind::Complete));
            assert_eq!(
                complete.before_len,
                trace.driver_steps[first.step_index].before_len
            );
            let NonMessageFrameState::EPathMap(done) = complete.before_nonmessage.as_ref().unwrap()
            else {
                panic!("EPathMap Complete has an EPathMap source frame");
            };
            assert!(same_epm_frame(done, &first.after));
            assert_eq!(done.snapshot.as_slice(), snapshot);
            assert_eq!(done.region_start, anchors[0]);
            assert_eq!(done.region_end, terminal);
            let (entry_events, entry_values, entry_edges, entry_max_depth) =
                complete_entry.unwrap_or((event_index, map_values_seen, path_edges, maximum_depth));
            assert_eq!(done.cursor.topology_events, entry_events);
            assert_eq!(done.cursor.value_records, entry_values);
            assert_eq!(done.cursor.terminal_checks, 0);
            assert_eq!(done.path.edges, entry_edges);
            assert_eq!(done.path.max_topology_depth, entry_max_depth);
            assert!(root_left);
            assert!(!done.waiting_map_value);
            assert!(done.waiting_obligation.is_none());
            assert!(done.active_obligation.is_none());
            let completion_ordinal = trace.driver_steps[..completion_index]
                .iter()
                .filter(|step| {
                    step.before_top_kind == FrameRequestKind::EPathMap
                        && matches!(step.kind, DriverStepKind::Complete)
                })
                .count();
            let returned_pop = trace
                .pop_accepts
                .iter()
                .filter(|pop| pop.child_kind == FrameRequestKind::EPathMap)
                .nth(completion_ordinal)
                .expect("each EPathMap Complete has an ordered pop acceptance");
            assert_eq!(returned_pop.before_len, complete.before_len);
            let NonMessageFrameState::EPathMap(returned) =
                returned_pop.child_nonmessage.as_ref().unwrap()
            else {
                panic!("EPathMap return has an EPathMap source frame");
            };
            assert!(same_epm_frame(returned, done));
            assert_eq!(returned.snapshot.as_slice(), snapshot);
            assert_eq!(returned.region_start, anchors[0]);
            assert_eq!(returned.region_end, terminal);
            assert_eq!(returned.cursor.topology_events, event_index);
            assert_eq!(returned.cursor.value_records, map_values_seen);
            assert_eq!(returned.cursor.terminal_checks, 1);
            assert_eq!(returned.path.edges, path_edges);
            assert_eq!(returned.path.max_topology_depth, maximum_depth);
            assert!(!returned.waiting_map_value);
            assert!(returned.waiting_obligation.is_none());
            assert!(returned.active_obligation.is_none());
            assert_eq!(
                returned_pop.epathmap_returned_snapshot.as_deref(),
                Some(snapshot)
            );
            assert_eq!(returned_pop.epathmap_returned_mode, Some(returned.mode));
            assert_eq!(
                returned_pop.epathmap_returned_entries_stable,
                Some(returned.entries_canonical_path_stable),
            );
            rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
                &scoped_epm_trace(trace, &group),
                snapshot,
                anchors,
                terminal,
            );
        }
        assert!(found_outer);
    };
    assert_tick_images(scalar);
    assert_tick_images(batched);
    assert_driver_actions_match(scalar, batched);
    let mut at = 0;
    let mut forward = 0;
    let mut unwind = 0;
    for batch in &batched.epathmap_events {
        let (snapshot, anchors) = event_frame_anchors(batched, batch);
        let arena = rpb3_root_finish_trace::epm_arena_range(snapshot.len(), |at| snapshot[at]);
        let last = match &batch.kind {
            EPathMapEventKind::Forward {
                width,
                source_range,
            } => {
                assert_eq!(*width, source_range.len());
                assert!(*width > 0);
                forward += usize::from(*width > 1);
                let mut last = None;
                for offset in source_range.clone() {
                    let index = arena.start + offset;
                    let source = anchors[index];
                    take_scalar(
                        &scalar.epathmap_events,
                        &mut at,
                        EPathMapEventKind::Edge(snapshot[index]),
                        source,
                    );
                    last = Some(take_scalar(
                        &scalar.epathmap_events,
                        &mut at,
                        EPathMapEventKind::Value {
                            has_value: false,
                            value_body_range: None,
                        },
                        source,
                    ));
                }
                last.unwrap()
            }
            EPathMapEventKind::Unwind {
                width,
                source_range,
            } => {
                assert_eq!(*width, source_range.len());
                assert!(*width > 0);
                unwind += usize::from(*width > 1);
                let mut last = None;
                for offset in source_range.clone().rev() {
                    last = Some(take_scalar(
                        &scalar.epathmap_events,
                        &mut at,
                        EPathMapEventKind::Leave,
                        anchors[arena.start + offset],
                    ));
                }
                last.unwrap()
            }
            kind => take_scalar(&scalar.epathmap_events, &mut at, kind.clone(), batch.source),
        };
        assert_boundary(last, batch);
    }
    assert_eq!(at, scalar.epathmap_events.len());
    assert!(forward > 0);
    assert!(unwind > 0);
    let scalar_returns = scalar
        .pop_accepts
        .iter()
        .filter_map(|pop| pop.applied_obligation)
        .collect::<Vec<_>>();
    let batched_returns = batched
        .pop_accepts
        .iter()
        .filter_map(|pop| pop.applied_obligation)
        .collect::<Vec<_>>();
    assert_eq!(batched_returns, scalar_returns);
    assert_eq!(scalar_returns.len(), 1);
    assert!(matches!(
        scalar_returns[0].1,
        CanonicalPathObligationResolution::RawPar { .. }
    ));
    assert_eq!(scalar.remaining_frames, Some(0));
    assert_eq!(batched.remaining_frames, Some(0));
}

fn assert_scalar_path_source_transitions(trace: &RootFinishTrace, expect_set_pair: bool) {
    let mut saw_edge = false;
    let mut saw_leave = false;
    let mut saw_value = false;
    let mut saw_root_exit = false;
    let mut saw_obligation_push = false;
    let mut saw_set_stable_value = false;
    let mut saw_set_unstable_value = false;
    for (_, group) in epm_event_groups(trace) {
        let (snapshot, anchors) = event_frame_anchors(trace, group[0]);
        rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
            &scoped_epm_trace(trace, &group),
            snapshot,
            anchors,
            group[0].after.region_end,
        );
        let mut previous: Option<&EPathMapEventState> = None;
        for (index, tick) in group.iter().copied().enumerate() {
            assert!(!matches!(
                &tick.kind,
                EPathMapEventKind::Forward { .. } | EPathMapEventKind::Unwind { .. }
            ));
            let step = &trace.driver_steps[tick.step_index];
            let before = previous
                .filter(|prior| prior.step_index == tick.step_index)
                .map_or_else(
                    || {
                        step.epm_path_source_before
                            .expect("live EPathMap source before step")
                    },
                    |prior| prior.path_source,
                );
            let after = tick.path_source;
            let before_entries_stable = previous
                .filter(|prior| prior.step_index == tick.step_index)
                .map_or_else(
                    || match step.before_nonmessage.as_ref() {
                        Some(NonMessageFrameState::EPathMap(frame)) => {
                            frame.entries_canonical_path_stable
                        }
                        _ => panic!("EPathMap step starts with its source frame"),
                    },
                    |prior| prior.after.entries_canonical_path_stable,
                );
            assert_eq!(after.edges, tick.after.path.edges);
            assert_eq!(after.current.depth, after.current.stamp);
            assert_eq!(after.source_present, before.source_present);
            match &tick.kind {
                EPathMapEventKind::Edge(_) => {
                    saw_edge = true;
                    assert!(!before.root_left);
                    assert_eq!(after.current.depth, before.current.depth + 1);
                    assert_eq!(after.edges, before.edges + 1);
                    assert_eq!(after.root_left, before.root_left);
                    let coalesced = before
                        .top
                        .is_some_and(|run| run.latest.state_id == before.current.state_id);
                    assert_eq!(
                        after.restore_len,
                        before.restore_len + usize::from(!coalesced),
                    );
                    let top = after.top.expect("scalar Edge retains its predecessor");
                    assert_eq!(top.latest, before.current);
                    assert_eq!(
                        top.first_depth,
                        if coalesced {
                            before.top.unwrap().first_depth
                        } else {
                            before.current.depth
                        },
                    );
                    assert_eq!(
                        after.current.last_source,
                        Some(CanonicalPathSource::in_source(
                            tick.source.source,
                            tick.source.offset,
                        )),
                    );
                }
                EPathMapEventKind::Leave => {
                    saw_leave = true;
                    assert_eq!(after.edges, before.edges);
                    if let Some(run) = before.top {
                        assert_eq!(after.current, run.latest);
                        assert_eq!(after.current.depth + 1, before.current.depth);
                        assert_eq!(
                            after.restore_len,
                            before.restore_len - usize::from(run.latest.depth == run.first_depth),
                        );
                        assert!(!after.root_left);
                    } else {
                        saw_root_exit = true;
                        assert_eq!(after.current, before.current);
                        assert_eq!(after.restore_len, 0);
                        assert!(after.root_left);
                    }
                }
                EPathMapEventKind::Value { has_value, .. } => {
                    saw_value = true;
                    assert_eq!(after, before, "Value observes but does not mutate the path");
                    assert_eq!(tick.value_reducer_stable.is_some(), *has_value);
                    assert_eq!(
                        tick.after.entries_canonical_path_stable,
                        before_entries_stable && tick.value_reducer_stable.unwrap_or(true),
                    );
                    if tick.after.mode == EPathMapMode::Set {
                        saw_set_stable_value |= tick.value_reducer_stable == Some(true);
                        saw_set_unstable_value |= tick.value_reducer_stable == Some(false);
                    }
                }
                EPathMapEventKind::Forward { .. } | EPathMapEventKind::Unwind { .. } => {
                    unreachable!()
                }
            }
            match (tick.after.active_obligation, after.current.pending) {
                (None, None) => {}
                (Some(progress), Some(pending)) => {
                    assert_eq!(pending.obligation, progress.obligation);
                    assert_eq!(pending.remaining, progress.remaining);
                    assert_eq!(pending.awaiting_resolution, progress.awaiting_resolution);
                }
                _ => panic!("live obligation and retained source disagree"),
            }
            let final_tick = group
                .get(index + 1)
                .is_none_or(|next| next.step_index != tick.step_index);
            if final_tick
                && matches!(step.kind, DriverStepKind::Push(_))
                && matches!(&tick.kind, EPathMapEventKind::Edge(_))
            {
                saw_obligation_push = true;
                let pending = after.current.pending.expect("Edge Push has an obligation");
                assert_eq!(pending.remaining, 0);
                assert!(pending.awaiting_resolution);
                assert_eq!(
                    pending.payload_end_source,
                    Some(CanonicalPathSource::in_source(
                        tick.source.source,
                        tick.source.offset,
                    )),
                );
            }
            previous = Some(tick);
        }
        assert!(previous.unwrap().path_source.root_left);
    }
    assert!(saw_edge && saw_leave && saw_value && saw_root_exit && saw_obligation_push);
    if expect_set_pair {
        assert!(saw_set_stable_value && saw_set_unstable_value);
    }
}

fn assert_map_value_push_subviews(
    trace: &RootFinishTrace,
    snapshot: &[u8],
    snapshot_anchors: &[CanonicalByteProvenance],
    terminal: CanonicalByteProvenance,
    records: &[(Range<usize>, Range<usize>)],
    receipts: &[epathmap_trie_codec::IndexedCanonicalEpm1MapValueSourceReceipt],
) {
    let mut found_outer = false;
    for (_, group) in epm_event_groups(trace) {
        let (frame_snapshot, frame_anchors) = event_frame_anchors(trace, group[0]);
        if frame_snapshot != snapshot || frame_anchors != snapshot_anchors {
            continue;
        }
        found_outer = true;
        assert_eq!(group[0].after.region_end, terminal);
        let values = group
            .iter()
            .filter_map(|event| match &event.kind {
                EPathMapEventKind::Value {
                    has_value: true,
                    value_body_range: Some(range),
                } => Some((*event, range)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(values.len(), records.len());
        assert_eq!(receipts.len(), records.len());
        for (index, ((event, reported_range), (prefix, body_range))) in
            values.iter().zip(records).enumerate()
        {
            let receipt = receipts[index];
            assert_eq!(*reported_range, body_range);
            assert_eq!(event.source, receipt.act_source);
            assert_eq!(event.cursor_source.event_index, receipt.event_index_after);
            assert_eq!(
                event.cursor_source.map_values_seen,
                receipt.map_values_seen_after
            );
            assert_eq!(
                event.cursor_source.value_table_offset,
                receipt.post_value_table_offset
            );
            assert_eq!(receipt.pre_value_table_offset, prefix.start);
            assert_eq!(receipt.length_prefix_start, prefix.start);
            assert_eq!(receipt.length_prefix_end, prefix.end);
            assert_eq!(receipt.body_range.start, body_range.start);
            assert_eq!(receipt.body_range.end, body_range.end);
            assert_eq!(event.cursor_source.mode, EPathMapMode::Map);
            assert_eq!(event.cursor_source.declared_value_count, records.len());
            assert_eq!(event.cursor_source.map_values_seen, index + 1);
            assert_eq!(event.cursor_source.value_table_offset, body_range.end);
            assert_eq!(event.cursor_source.snapshot_len, snapshot.len());
            assert_eq!(
                event.cursor_source.event_index,
                event.after.cursor.topology_events
            );
            assert_eq!(event.after.cursor.value_records, index + 1);
            assert!(event.after.waiting_map_value);

            let step = &trace.driver_steps[event.step_index];
            assert!(matches!(step.kind, DriverStepKind::Push(_)));
            assert_eq!(step.before_top_kind, FrameRequestKind::EPathMap);
            assert_eq!(step.after_top_kind, Some(FrameRequestKind::Message));
            let expected_start = snapshot_anchors
                .get(body_range.start)
                .copied()
                .unwrap_or(terminal);
            let expected_end = snapshot_anchors
                .get(body_range.end)
                .copied()
                .unwrap_or(terminal);
            assert_eq!(
                step.after_top_region,
                Some((expected_start, body_range.len()))
            );
            let pushed = trace
                .nonmessage_push_bodies
                .iter()
                .find(|pushed| pushed.step_index == event.step_index)
                .expect("each Map Value pushes a child body");
            assert_eq!(pushed.body.as_slice(), &snapshot[body_range.clone()]);
            assert_eq!(pushed.start, expected_start);
            assert_eq!(pushed.end, expected_end);
            assert!(pushed.obligation.is_none());
            let Some(ActiveFrameState::Message {
                frame,
                bytes,
                anchors,
            }) = step
                .active_frames_after
                .as_ref()
                .and_then(|frames| frames.last())
            else {
                panic!("Map Value Push activates a Message child");
            };
            assert_eq!(frame.region_start, expected_start);
            assert_eq!(frame.region_end, expected_end);
            assert_eq!(frame.region_len, body_range.len());
            assert_eq!(bytes.as_slice(), &snapshot[body_range.clone()]);
            assert_eq!(anchors.as_slice(), &snapshot_anchors[body_range.clone()]);
            assert_eq!(prefix.end, body_range.start);
        }
        rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
            &scoped_epm_trace(trace, &group),
            snapshot,
            snapshot_anchors,
            terminal,
        );
    }
    assert!(
        found_outer,
        "generated Map snapshot has a live EPathMap frame"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn batched_forward_and_unwind_match_scalar_on_fragmented_obligation_paths(
        width in 24usize..97,
        marker in any::<u8>(),
        map_mode in any::<bool>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (outer, snapshot) = fixture(width, marker, map_mode);
        let anchors = (0..outer.len())
            .map(|index| CanonicalByteProvenance::new(source + (index % 19) as u32, base + index * 17))
            .collect::<Vec<_>>();
        let terminal = CanonicalByteProvenance::new(source + 100, base);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in outer.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let input = IndexedByteRegion::from_frozen(arena.freeze(handle), terminal);
        let snapshot_at = outer.len() - snapshot.len();
        assert_eq!(&outer[snapshot_at..], snapshot);
        let snapshot_anchors = &anchors[snapshot_at..];

        let (scalar_result, scalar_trace) = standalone_path_admission::with_epm_transport_mode(true, || {
            rpb3_root_finish_trace::start_with_active_frames();
            let result = validate_canonical_protobuf_inner(
                input.clone(), &PAR_PROTOBUF_REGION_PROGRAM, None,
            );
            (result, rpb3_root_finish_trace::take())
        });
        let (batch_result, batch_trace) = standalone_path_admission::with_epm_transport_mode(false, || {
            rpb3_root_finish_trace::start_with_active_frames();
            let result = validate_canonical_protobuf_inner(
                input, &PAR_PROTOBUF_REGION_PROGRAM, None,
            );
            (result, rpb3_root_finish_trace::take())
        });
        prop_assert!(scalar_result.is_ok(), "scalar fixture rejected: {:?}", scalar_result.as_ref().err());
        prop_assert_eq!(&batch_result, &scalar_result);
        let admitted = batch_result.unwrap();
        prop_assert_eq!(admitted.stats().canonical_path_obligations, 1);
        prop_assert_eq!(admitted.stats().canonical_path_resolutions, 1);
        assert_batch_expands_to_scalar(
            &scalar_trace, &batch_trace, &snapshot, snapshot_anchors, terminal,
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn generated_scalar_epathmap_source_shapes_and_obligations_follow_act(
        width in 24usize..65,
        marker in any::<u8>(),
        map_mode in any::<bool>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (outer, _) = fixture(width, marker, map_mode);
        let anchors = (0..outer.len())
            .map(|index| CanonicalByteProvenance::new(source + (index % 19) as u32, base + index * 17))
            .collect::<Vec<_>>();
        let terminal = CanonicalByteProvenance::new(source + 100, base);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in outer.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let input = IndexedByteRegion::from_frozen(arena.freeze(handle), terminal);
        let (result, trace) = standalone_path_admission::with_epm_transport_mode(true, || {
            rpb3_root_finish_trace::start_with_active_frames();
            let result = validate_canonical_protobuf_inner(
                input, &PAR_PROTOBUF_REGION_PROGRAM, None,
            );
            (result, rpb3_root_finish_trace::take())
        });
        prop_assert!(result.is_ok(), "scalar fixture rejected: {:?}", result.as_ref().err());
        assert_scalar_path_source_transitions(&trace, !map_mode);
        prop_assert_eq!(trace.remaining_frames, Some(0));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn generated_fragmented_map_values_push_exact_wire_subviews(
        width in 8usize..33,
        marker in any::<u8>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (outer, snapshot) = map_value_fixture(width, marker);
        let (trie_range, records) = independently_parse_map_value_table(&snapshot);
        assert_eq!(records.len(), 3);
        assert_eq!(records.iter().filter(|(_, body)| body.is_empty()).count(), 1);
        assert_eq!(records.iter().filter(|(prefix, body)| prefix.len() == 1 && !body.is_empty()).count(), 1);
        assert_eq!(records.iter().filter(|(prefix, body)| prefix.len() > 1 && body.len() > 127).count(), 1);
        let anchors = (0..outer.len())
            .map(|index| CanonicalByteProvenance::new(source + (index % 19) as u32, base + index * 17))
            .collect::<Vec<_>>();
        let terminal = CanonicalByteProvenance::new(source + 100, base);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in outer.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let input = IndexedByteRegion::from_frozen(arena.freeze(handle), terminal);
        let snapshot_at = outer.len() - snapshot.len();
        assert_eq!(&outer[snapshot_at..], snapshot);
        let snapshot_anchors = &anchors[snapshot_at..];
        let snapshot_region = input.subview(CanonicalByteRange {
            start: snapshot_at,
            end: outer.len(),
        }).expect("the generated EPM1 snapshot is a subview of its outer message");
        let mut indexed = epathmap_trie_codec::IndexedCanonicalEpm1TopologyCursor::new(
            snapshot_region.clone(),
        ).expect("the generated Map snapshot has an indexed cursor");
        let plan_source = indexed.validated_act_plan_source()
            .expect("the generated Map snapshot has an accepted ACT plan");
        let expected_trie_region = snapshot_region.subview(CanonicalByteRange {
            start: trie_range.start,
            end: trie_range.end,
        }).expect("the ACT range is a subview of the indexed snapshot");
        assert!(plan_source.indexed_source().same_view(&expected_trie_region));
        assert!(!plan_source.is_empty());
        assert_eq!(plan_source.value_summary().value_count(), records.len());
        assert_eq!(plan_source.value_summary().first_nonzero(), None);
        assert!(plan_source.value_summary().first_nonsequential().is_some());
        assert!(plan_source.record_count() >= records.len());
        assert_eq!(plan_source.record(0).unwrap().has_incoming(), false);
        let mut plan_values = 0usize;
        for index in 0..plan_source.record_count() {
            let record = plan_source.record(index).unwrap();
            assert!(record.node_id < trie_range.len());
            assert!(record.value_source_offset < trie_range.len());
            assert!(record.line_start + record.line_len <= trie_range.len());
            assert_eq!(record.flags & !3, 0);
            assert_eq!(record.has_incoming(), index != 0);
            if record.has_value() {
                assert_eq!(record.value, 0);
                assert_ne!(snapshot[trie_range.start + record.value_source_offset] & 0x40, 0);
            }
            if record.line_len > 0 {
                assert_ne!(snapshot[trie_range.start + record.node_id] & 0x80, 0);
            }
            plan_values += usize::from(record.has_value());
        }
        assert_eq!(plan_values, records.len());
        assert!(plan_source.record(plan_source.record_count()).is_none());
        let plan_records = (0..plan_source.record_count())
            .map(|index| plan_source.record(index).unwrap())
            .collect::<Vec<_>>();
        let mut receipts = Vec::new();
        let mut event_index = 0usize;
        loop {
            let cursor_before = indexed.active_source_image().expect("active indexed Map cursor");
            let act_before = indexed.topology_source_position();
            let Some(observed) = indexed.next_event_with_source()
                .expect("generated indexed Map event has an exact source receipt") else {
                break;
            };
            event_index += 1;
            match &observed.event {
                IndexedCanonicalEpm1TopologyEvent::Value {
                    has_value: true,
                    value_body_range: Some(range),
                    value_body: Some(body),
                    source_offset,
                    provenance,
                } => {
                    let receipt = observed.map_value_receipt
                        .expect("each Map Value has a cursor source receipt");
                    let value_index = receipts.len();
                    let (prefix, expected_body) = &records[value_index];
                    assert_eq!(receipt.event_index_before, cursor_before.event_index);
                    assert_eq!(receipt.map_values_seen_before, cursor_before.map_values_seen);
                    assert_eq!(receipt.pre_value_table_offset, cursor_before.value_table_offset);
                    assert_eq!(receipt.act_before, act_before.unwrap());
                    let selected_record = plan_records.get(receipt.act_before.record)
                        .expect("the ACT source position names an accepted plan record");
                    assert!(selected_record.has_value());
                    assert_eq!(selected_record.node_id, receipt.act_before.node_id);
                    assert_eq!(selected_record.value, 0);
                    assert_eq!(selected_record.value_source_offset, *source_offset);
                    assert_eq!(selected_record.line_start, receipt.act_before.line_start.unwrap_or(0));
                    assert_eq!(selected_record.line_len,
                        receipt.act_before.line_end.unwrap_or(0)
                            - receipt.act_before.line_start.unwrap_or(0));
                    assert_eq!(receipt.act_after, indexed.topology_source_position());
                    assert_eq!(receipt.act_after, Some(expected_map_value_act_after(receipt.act_before)));
                    assert_eq!(receipt.act_source_offset, *source_offset);
                    assert_eq!(receipt.event_index_before + 1, event_index);
                    assert_eq!(receipt.event_index_after, event_index);
                    assert_eq!(receipt.map_values_seen_before, value_index);
                    assert_eq!(receipt.map_values_seen_after, value_index + 1);
                    assert_eq!(receipt.pre_value_table_offset, prefix.start);
                    assert_eq!(receipt.length_prefix_start, prefix.start);
                    assert_eq!(receipt.length_prefix_end, prefix.end);
                    assert_eq!(receipt.body_len, expected_body.len());
                    assert_eq!(receipt.body_range.start..receipt.body_range.end, expected_body.clone());
                    assert_eq!(receipt.post_value_table_offset, expected_body.end);
                    assert_eq!(receipt.snapshot_len, snapshot.len());
                    assert_eq!(range, expected_body);
                    assert_eq!(receipt.act_source, *provenance);
                    assert_eq!(
                        snapshot_anchors[trie_range.start + receipt.act_source_offset],
                        receipt.act_source,
                    );
                    assert!(body.same_view(&snapshot_region.subview(receipt.body_range).unwrap()));
                    receipts.push(receipt);
                }
                _ => assert!(observed.map_value_receipt.is_none()),
            }
        }
        assert_eq!(receipts.len(), records.len());

        for scalar in [true, false] {
            let (result, trace) = standalone_path_admission::with_epm_transport_mode(scalar, || {
                rpb3_root_finish_trace::start_with_active_frames();
                let result = validate_canonical_protobuf_inner(
                    input.clone(), &PAR_PROTOBUF_REGION_PROGRAM, None,
                );
                (result, rpb3_root_finish_trace::take())
            });
            prop_assert!(result.is_ok(), "Map fixture rejected in scalar={scalar}: {:?}", result.as_ref().err());
            assert_map_value_push_subviews(
                &trace, &snapshot, snapshot_anchors, terminal, &records, &receipts,
            );
            prop_assert_eq!(trace.remaining_frames, Some(0));
        }
    }
}
