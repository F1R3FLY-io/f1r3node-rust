//! Scalar/batched EPathMap transport equivalence on generated branching tries.
//!
//! The key set and physical byte anchors are built independently of admission.
//! The ACT topology cursor supplies the expected value count and source byte
//! positions; the production driver supplies both transport traces.

use proptest::prelude::*;
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieTopologyCursor, CanonicalCompactTrieTopologyEvent,
};

use super::rpb3_root_finish_trace::{
    ActiveFrameState, DriverStepKind, EPathMapEventKind, EPathMapEventState, EPathMapFrameState,
    NonMessageFrameState, RootFinishTrace,
};
use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};
use crate::rust::canonical_path::encode_trie_path;
use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::canonical_path_admission::LocatedPathByte;

fn length_delimited_field(number: u32, body: &[u8]) -> Vec<u8> {
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

fn key(width: usize, marker: u8, escaped: bool) -> Vec<u8> {
    let mut bytes = vec![if escaped { 73 } else { 37 }; width];
    *bytes.last_mut().expect("generated key width is positive") = marker;
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(bytes)),
    }]);
    let par = if escaped {
        par.with_connective_used(true)
    } else {
        par
    };
    let encoded = encode_trie_path(&par);
    assert_eq!(encoded[0], if escaped { 15 } else { 6 });
    encoded
}

fn fixture(
    width: usize,
    stable_count: usize,
    escaped_count: usize,
    seed: u8,
) -> (Vec<u8>, Vec<u8>) {
    let mut trie = pathmap::PathMap::new();
    let mut keys = Vec::new();
    for index in 0..stable_count {
        let encoded = key(width, seed.wrapping_add(index as u8), false);
        trie.insert(&encoded, ());
        keys.push(encoded);
    }
    for index in 0..escaped_count {
        let encoded = key(width + index, seed.wrapping_add(index as u8), true);
        trie.insert(&encoded, ());
        keys.push(encoded);
    }
    assert_eq!(keys.len(), stable_count + escaped_count);
    for (at, left) in keys.iter().enumerate() {
        assert!(keys[..at].iter().all(|right| right != left));
    }
    let snapshot = epathmap_trie_codec::encode(&EPathMapRepr::<Par>::Set(trie));
    let arena = rpb3_root_finish_trace::epm_arena_range(snapshot.len(), |at| snapshot[at]);
    let mut topology = CanonicalCompactTrieTopologyCursor::new(&snapshot[arena])
        .expect("generated Set snapshot has canonical ACT topology");
    let mut values = 0;
    while let Some(event) = topology.next_event() {
        if matches!(event, CanonicalCompactTrieTopologyEvent::Value {
            value: Some(_),
            ..
        }) {
            values += 1;
        }
    }
    assert_eq!(values, keys.len(), "each generated key owns one trie value");
    let outer = length_delimited_field(
        5,
        &length_delimited_field(32, &length_delimited_field(9, &snapshot)),
    );
    (outer, snapshot)
}

fn assert_semantic_boundary(scalar: &EPathMapEventState, batched: &EPathMapEventState) {
    assert_eq!(batched.source, scalar.source);
    assert_eq!(batched.after.owner, scalar.after.owner);
    assert_eq!(batched.after.field, scalar.after.field);
    assert_eq!(batched.after.snapshot, scalar.after.snapshot);
    assert_eq!(batched.after.region_start, scalar.after.region_start);
    assert_eq!(batched.after.region_end, scalar.after.region_end);
    assert_eq!(batched.after.mode, scalar.after.mode);
    assert_eq!(batched.after.cursor, scalar.after.cursor);
    // Test-only bulk-operation counters differ by transport mode. Every
    // semantic reducer count and the retained obligation must still agree.
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
}

fn expect_scalar<'a>(
    scalar: &'a [EPathMapEventState],
    next: &mut usize,
    kind: EPathMapEventKind,
    source: CanonicalByteProvenance,
) -> &'a EPathMapEventState {
    let event = scalar.get(*next).expect("batch event has a scalar witness");
    assert_eq!(event.kind, kind);
    assert_eq!(event.source, source);
    *next += 1;
    event
}

fn assert_transport_equivalence(
    scalar: &RootFinishTrace,
    batched: &RootFinishTrace,
    snapshot: &[u8],
    anchors: &[CanonicalByteProvenance],
    terminal: CanonicalByteProvenance,
    escaped_count: usize,
) {
    rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
        scalar, snapshot, anchors, terminal,
    );
    rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
        batched, snapshot, anchors, terminal,
    );
    let arena = rpb3_root_finish_trace::epm_arena_range(snapshot.len(), |at| snapshot[at]);
    let mut next = 0;
    let mut expanded_forward = 0;
    let mut expanded_unwind = 0;
    for batch in &batched.epathmap_events {
        let last = match &batch.kind {
            EPathMapEventKind::Forward {
                width,
                source_range,
            } => {
                assert_eq!(*width, source_range.len());
                assert!(*width > 0);
                expanded_forward += usize::from(*width > 1);
                let mut last = None;
                for offset in source_range.clone() {
                    let physical = anchors[arena.start + offset];
                    expect_scalar(
                        &scalar.epathmap_events,
                        &mut next,
                        EPathMapEventKind::Edge(snapshot[arena.start + offset]),
                        physical,
                    );
                    last = Some(expect_scalar(
                        &scalar.epathmap_events,
                        &mut next,
                        EPathMapEventKind::Value {
                            has_value: false,
                            value_body_range: None,
                        },
                        physical,
                    ));
                }
                last.expect("Forward has positive width")
            }
            EPathMapEventKind::Unwind {
                width,
                source_range,
            } => {
                assert_eq!(*width, source_range.len());
                assert!(*width > 0);
                expanded_unwind += usize::from(*width > 1);
                let mut last = None;
                for offset in source_range.clone().rev() {
                    last = Some(expect_scalar(
                        &scalar.epathmap_events,
                        &mut next,
                        EPathMapEventKind::Leave,
                        anchors[arena.start + offset],
                    ));
                }
                last.expect("Unwind has positive width")
            }
            kind => expect_scalar(
                &scalar.epathmap_events,
                &mut next,
                kind.clone(),
                batch.source,
            ),
        };
        assert_semantic_boundary(last, batch);
    }
    assert_eq!(next, scalar.epathmap_events.len());
    assert!(expanded_forward > 0);
    assert!(expanded_unwind > 0);

    let scalar_returns: Vec<_> = scalar
        .pop_accepts
        .iter()
        .filter_map(|pop| pop.applied_obligation)
        .collect();
    let batch_returns: Vec<_> = batched
        .pop_accepts
        .iter()
        .filter_map(|pop| pop.applied_obligation)
        .collect();
    assert_eq!(scalar_returns, batch_returns);
    assert_eq!(scalar_returns.len(), escaped_count);
    assert!(scalar_returns.iter().all(|(_, resolution)| matches!(
        resolution,
        CanonicalPathObligationResolution::RawPar { .. }
    )));
    let scalar_pushes: Vec<_> = scalar
        .nonmessage_push_bodies
        .iter()
        .filter(|push| push.obligation.is_some())
        .map(|push| (&push.body, push.start, push.end, push.obligation))
        .collect();
    let batch_pushes: Vec<_> = batched
        .nonmessage_push_bodies
        .iter()
        .filter(|push| push.obligation.is_some())
        .map(|push| (&push.body, push.start, push.end, push.obligation))
        .collect();
    assert_eq!(scalar_pushes, batch_pushes);
    assert_eq!(scalar_pushes.len(), escaped_count);
    assert_eq!(scalar.returned, batched.returned);
    assert_eq!(scalar.score_arena, batched.score_arena);
    assert_eq!(scalar.remaining_frames, Some(0));
    assert_eq!(batched.remaining_frames, Some(0));
}

fn assert_scalar_internal_continues(
    trace: &RootFinishTrace,
    snapshot: &[u8],
    anchors: &[CanonicalByteProvenance],
) {
    let arena = rpb3_root_finish_trace::epm_arena_range(snapshot.len(), |at| snapshot[at]);
    let mut cursor = CanonicalCompactTrieTopologyCursor::new(&snapshot[arena.clone()])
        .expect("generated Set has canonical ACT topology");
    let mut topology = Vec::new();
    while let Some(event) = cursor.next_event() {
        topology.push(event);
    }
    let mut event_at = 0;
    let mut receipt_at = 0;
    let mut path = Vec::<LocatedPathByte>::new();
    let mut seen_internal_edge = 0;
    let mut seen_internal_leave = 0;
    let mut seen_internal_empty_value = 0;
    let mut seen_internal_present_value = 0;

    for (step_index, step) in trace.driver_steps.iter().enumerate() {
        let Some(NonMessageFrameState::EPathMap(before)) = &step.before_nonmessage else {
            continue;
        };
        let before_stack = step.active_frames_before.as_ref().unwrap();
        let after_stack = step.active_frames_after.as_ref().unwrap();
        assert_eq!(step.score_len_after, step.score_len_before);
        assert!(step.score_appended.is_empty());
        let first = event_at;
        let mut prior: EPathMapFrameState = before.clone();

        while let Some(event) = trace.epathmap_events.get(event_at) {
            if event.step_index != step_index {
                break;
            }
            let source_event = topology
                .get(prior.cursor.topology_events)
                .expect("scalar event has an ACT source event");
            let (expected_kind, source_offset) = match *source_event {
                CanonicalCompactTrieTopologyEvent::Edge {
                    byte,
                    source_offset,
                } => (EPathMapEventKind::Edge(byte), source_offset),
                CanonicalCompactTrieTopologyEvent::Leave { source_offset } => {
                    (EPathMapEventKind::Leave, source_offset)
                }
                CanonicalCompactTrieTopologyEvent::Value {
                    value,
                    source_offset,
                } => (
                    EPathMapEventKind::Value {
                        has_value: value.is_some(),
                        value_body_range: None,
                    },
                    source_offset,
                ),
            };
            let expected_path_before = path.clone();
            match *source_event {
                CanonicalCompactTrieTopologyEvent::Edge {
                    byte,
                    source_offset,
                } => {
                    let physical = anchors[arena.start + source_offset];
                    path.push(LocatedPathByte::in_source(
                        byte,
                        physical.source,
                        physical.offset,
                    ));
                }
                CanonicalCompactTrieTopologyEvent::Leave { .. } => {
                    path.pop();
                }
                CanonicalCompactTrieTopologyEvent::Value { .. } => {}
            }
            assert_eq!(event.kind, expected_kind);
            assert_eq!(event.source, anchors[arena.start + source_offset]);
            assert_eq!(event.after.last_source.source, event.source.source);
            assert_eq!(event.after.last_source.offset, event.source.offset);
            assert_eq!(
                event.cursor_source.event_index,
                prior.cursor.topology_events + 1
            );
            assert_eq!(
                event.after.cursor.topology_events,
                prior.cursor.topology_events + 1
            );
            assert_eq!(event.after.cursor.value_records, 0);
            assert_eq!(event.after.cursor.terminal_checks, 0);
            assert_eq!(event.cursor_source.mode, EPathMapMode::Set);
            assert_eq!(event.cursor_source.declared_value_count, 0);
            assert_eq!(event.cursor_source.map_values_seen, 0);
            assert_eq!(event.cursor_source.snapshot_len, snapshot.len());
            assert_eq!(event.cursor_source.value_table_offset, snapshot.len());
            assert!(std::ptr::eq(event.after.owner, prior.owner));
            assert_eq!(event.after.field, prior.field);
            assert_eq!(event.after.snapshot, prior.snapshot);
            assert_eq!(event.after.region_start, prior.region_start);
            assert_eq!(event.after.region_end, prior.region_end);
            assert_eq!(event.after.mode, prior.mode);
            assert!(!event.after.waiting_map_value);

            let internal = !matches!(step.kind, DriverStepKind::Push(_))
                || trace
                    .epathmap_events
                    .get(event_at + 1)
                    .is_some_and(|next| next.step_index == step_index);
            if internal {
                assert!(event.after.waiting_obligation.is_none());
                match &event.kind {
                    EPathMapEventKind::Edge(_) => {
                        seen_internal_edge += 1;
                        let CanonicalCompactTrieTopologyEvent::Edge { byte, .. } = *source_event
                        else {
                            unreachable!()
                        };
                        let receipt = trace
                            .epathmap_scalar_edge_no_child
                            .get(receipt_at)
                            .expect("internal scalar Edge has a checked receipt");
                        receipt_at += 1;
                        assert_eq!(receipt.step_index, step_index);
                        assert_eq!(receipt.byte, byte);
                        assert_eq!(receipt.provenance, anchors[arena.start + source_offset]);
                        assert_eq!(
                            receipt.cursor_before.event_index,
                            prior.cursor.topology_events
                        );
                        assert_eq!(receipt.cursor_after, event.cursor_source);
                        assert_eq!(receipt.cursor_before.mode, EPathMapMode::Set);
                        assert_eq!(receipt.cursor_before.declared_value_count, 0);
                        assert_eq!(receipt.cursor_before.map_values_seen, 0);
                        assert_eq!(receipt.cursor_before.value_table_offset, snapshot.len());
                        assert_eq!(receipt.cursor_before.snapshot_len, snapshot.len());
                        assert_eq!(receipt.parser_before.path_bytes, expected_path_before);
                        assert_eq!(receipt.parser_after.path_bytes, path);
                        assert_eq!(receipt.path_before.depth, expected_path_before.len());
                        assert_eq!(receipt.path_after.depth, path.len());
                        assert_eq!(
                            receipt.path_after.last_source,
                            Some(CanonicalPathSource::in_source(
                                receipt.provenance.source,
                                receipt.provenance.offset,
                            ))
                        );
                        assert!(receipt.no_child);
                        assert!(receipt.no_map_value_receipt);
                        assert!(std::ptr::eq(receipt.owner_before, prior.owner));
                        assert!(std::ptr::eq(receipt.owner_after, prior.owner));
                        assert_eq!(receipt.field_before, prior.field);
                        assert_eq!(receipt.field_after, prior.field);
                        assert_eq!(receipt.region_start_before, prior.region_start);
                        assert_eq!(receipt.region_start_after, prior.region_start);
                        assert_eq!(receipt.snapshot_len_before, snapshot.len());
                        assert_eq!(receipt.snapshot_len_after, snapshot.len());
                        assert!(receipt.waiting_before);
                        assert!(receipt.waiting_after);
                        assert_eq!(receipt.waiting_child_len_before, None);
                        assert_eq!(receipt.waiting_child_len_after, None);
                        assert_eq!(
                            receipt.entries_stable_before,
                            prior.entries_canonical_path_stable
                        );
                        assert_eq!(
                            receipt.entries_stable_after,
                            event.after.entries_canonical_path_stable
                        );
                    }
                    EPathMapEventKind::Leave => seen_internal_leave += 1,
                    EPathMapEventKind::Value {
                        has_value: false, ..
                    } => {
                        seen_internal_empty_value += 1;
                    }
                    EPathMapEventKind::Value {
                        has_value: true, ..
                    } => {
                        seen_internal_present_value += 1;
                    }
                    _ => unreachable!(),
                }
            }
            if !matches!(event.kind, EPathMapEventKind::Value {
                has_value: true,
                ..
            }) {
                assert_eq!(
                    event.after.entries_canonical_path_stable,
                    prior.entries_canonical_path_stable
                );
            }
            prior = event.after.clone();
            event_at += 1;
        }

        match step.kind {
            DriverStepKind::Push(_) => {
                assert!(event_at > first);
                let Some(NonMessageFrameState::EPathMap(suspended)) =
                    step.suspended_parent_after.as_ref()
                else {
                    panic!("EPathMap push retains its parent state");
                };
                assert_eq!(suspended, &prior);
                assert_eq!(after_stack.len(), before_stack.len() + 1);
                assert_eq!(
                    &before_stack[..before_stack.len() - 1],
                    &after_stack[..before_stack.len() - 1]
                );
                match (
                    &before_stack[before_stack.len() - 1],
                    &after_stack[before_stack.len() - 1],
                ) {
                    (
                        ActiveFrameState::NonMessage {
                            bytes: before_bytes,
                            anchors: before_anchors,
                            ..
                        },
                        ActiveFrameState::NonMessage {
                            frame: NonMessageFrameState::EPathMap(after),
                            bytes: after_bytes,
                            anchors: after_anchors,
                        },
                    ) => {
                        assert_eq!(after, &prior);
                        assert_eq!(after_bytes, before_bytes);
                        assert_eq!(after_anchors, before_anchors);
                    }
                    _ => panic!("EPathMap push retains its parent frame and region"),
                }
            }
            DriverStepKind::Complete => {
                assert_eq!(prior.cursor.topology_events, topology.len());
                assert!(path.is_empty());
                assert_eq!(after_stack.len() + 1, before_stack.len());
                if after_stack.len() > 1 {
                    assert_eq!(
                        &before_stack[..before_stack.len() - 2],
                        &after_stack[..after_stack.len() - 1]
                    );
                }
            }
            DriverStepKind::Continue => panic!("EPathMap scalar loop continues internally"),
        }
    }
    assert_eq!(event_at, trace.epathmap_events.len());
    assert_eq!(receipt_at, trace.epathmap_scalar_edge_no_child.len());
    assert!(seen_internal_edge > 0);
    assert!(seen_internal_leave > 0);
    assert!(seen_internal_empty_value > 0);
    assert!(seen_internal_present_value > 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn generated_branching_set_expands_batched_events_to_scalar_trace(
        width in 24usize..65,
        stable_count in 1usize..4,
        escaped_count in 1usize..4,
        seed in any::<u8>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (outer, snapshot) = fixture(width, stable_count, escaped_count, seed);
        let anchors: Vec<_> = (0..outer.len())
            .map(|at| CanonicalByteProvenance::new(source + (at % 19) as u32, base + at * 17))
            .collect();
        let terminal = CanonicalByteProvenance::new(source + 100, base);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in outer.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let input = IndexedByteRegion::from_frozen(arena.freeze(handle), terminal);
        let snapshot_at = outer.len() - snapshot.len();
        prop_assert_eq!(&outer[snapshot_at..], snapshot.as_slice());
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
        prop_assert!(scalar_result.is_ok(), "generated scalar Set rejected: {:?}", scalar_result.as_ref().err());
        prop_assert_eq!(&batch_result, &scalar_result);
        let admitted = batch_result.unwrap();
        prop_assert_eq!(admitted.stats().canonical_path_obligations, escaped_count);
        prop_assert_eq!(admitted.stats().canonical_path_resolutions, escaped_count);
        assert_transport_equivalence(
            &scalar_trace, &batch_trace, &snapshot, snapshot_anchors, terminal, escaped_count,
        );
    }

    #[test]
    fn generated_scalar_set_internal_continues_preserve_source_and_frame(
        width in 24usize..65,
        stable_count in 1usize..4,
        escaped_count in 1usize..4,
        seed in any::<u8>(),
        source in 1u32..1000,
        base in 0usize..100_000,
    ) {
        let (outer, snapshot) = fixture(width, stable_count, escaped_count, seed);
        let anchors: Vec<_> = (0..outer.len())
            .map(|at| CanonicalByteProvenance::new(source + (at % 19) as u32, base + at * 17))
            .collect();
        let terminal = CanonicalByteProvenance::new(source + 100, base);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in outer.iter().zip(&anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        let input = IndexedByteRegion::from_frozen(arena.freeze(handle), terminal);
        let snapshot_at = outer.len() - snapshot.len();
        prop_assert_eq!(&outer[snapshot_at..], snapshot.as_slice());

        let (result, trace) = standalone_path_admission::with_epm_transport_mode(true, || {
            rpb3_root_finish_trace::start_with_scalar_edge_receipts();
            let result = validate_canonical_protobuf_inner(
                input, &PAR_PROTOBUF_REGION_PROGRAM, None,
            );
            (result, rpb3_root_finish_trace::take())
        });
        prop_assert!(result.is_ok(), "generated scalar Set rejected: {:?}", result.as_ref().err());
        let admitted = result.unwrap();
        prop_assert_eq!(admitted.stats().canonical_path_obligations, escaped_count);
        assert_scalar_internal_continues(&trace, &snapshot, &anchors[snapshot_at..]);
    }
}
