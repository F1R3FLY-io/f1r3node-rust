use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::canonical_path::{encode_trie_path, validate_canonical_path};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::canonical_path_admission::{
    CanonicalPathAdmissionError, CanonicalPathAdmissionErrorKind, CanonicalPathObligation,
    CanonicalPathReducer, CanonicalPathReducerHandle,
};
use crate::rust::rholang::protobuf_encoder;

fn expr(value: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(value),
    }])
}

fn integer(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

fn payload(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![tag];
    prost::encoding::encode_varint(body.len() as u64, &mut bytes);
    bytes.extend_from_slice(body);
    bytes
}

fn admit(
    bytes: &[u8],
) -> Result<(CanonicalPathSummary, CanonicalProtobufAdmissionStats), CanonicalProtobufAdmissionError>
{
    validate_canonical_path_indexed(IndexedByteRegion::from_borrowed(bytes, 0, 0))
}

fn origin(index: usize) -> CanonicalByteProvenance {
    CanonicalByteProvenance::new(10 + (index % 3) as u32, 100 + index * 13)
}

fn fragmented(bytes: &[u8]) -> OwnedIndexedByteRegion {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (index, byte) in bytes.iter().copied().enumerate() {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, origin(index)))
            .unwrap();
    }
    OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), CanonicalByteProvenance::new(99, 800))
}

struct IndexedCompletedPayload {
    obligation: CanonicalPathObligation,
    payload: OwnedIndexedByteRegion,
    payload_end: CanonicalPathSource,
}

fn indexed_completed(
    stream: &CanonicalPathStreamReducer,
    input: &OwnedIndexedByteRegion,
) -> Result<Option<IndexedCompletedPayload>, CanonicalPathAdmissionError> {
    Ok(stream.completed_obligation(input)?.map(|completed| {
        assert!(completed.payload.end <= stream.consumed_len());
        let payload = input
            .subview_with_end(
                completed.payload,
                CanonicalByteProvenance::new(
                    completed.payload_end.source,
                    completed.payload_end.offset,
                ),
            )
            .expect("a completed payload lies within the consumed input");
        IndexedCompletedPayload {
            obligation: completed.obligation,
            payload,
            payload_end: completed.payload_end,
        }
    }))
}

fn assert_consumed_prefix(
    stream: &CanonicalPathStreamReducer,
    input: &OwnedIndexedByteRegion,
    history_arena: &CanonicalBytesArena,
    checkpoint: CanonicalPathReducerHandle,
) {
    assert_eq!(stream.consumed_len(), checkpoint.bytes().len());
    let consumed = input
        .subview(CanonicalByteRange {
            start: 0,
            end: stream.consumed_len(),
        })
        .unwrap();
    assert_eq!(
        consumed.located_bytes().collect::<Vec<_>>(),
        history_arena
            .located_bytes(checkpoint.bytes())
            .collect::<Vec<_>>()
    );
}

fn assert_differential(bytes: &[u8]) {
    let reference = validate_canonical_path(bytes)
        .map(|metadata| metadata.stable)
        .ok();
    let borrowed = admit(bytes)
        .map(|(summary, _)| summary.canonical_path_stable)
        .ok();
    let frozen = validate_canonical_path_indexed(fragmented(bytes))
        .map(|(summary, _)| summary.canonical_path_stable)
        .ok();
    assert_eq!(borrowed, reference, "borrowed {bytes:?}");
    assert_eq!(frozen, reference, "frozen {bytes:?}");
}

fn assert_storage_trace(bytes: &[u8], raw_stable: bool, mode: NestedEpmMode, entries_stable: bool) {
    let mut history_arena = CanonicalBytesArena::new();
    let input = fragmented(bytes);
    let mut history = CanonicalPathReducer::new();
    let mut checkpoint = history.root(&history_arena);
    let mut stream = CanonicalPathStreamReducer::new();
    let mut successful = 0;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let located = CanonicalLocatedByte::new(byte, origin(index));
        let left = history.edge(&mut history_arena, checkpoint, located);
        let right = stream.edge(&input, located);
        match (left, right) {
            (Ok((next, left)), Ok(right)) => {
                assert_eq!(left, right);
                checkpoint = next;
                successful += 1;
            }
            (Err(left), Err(right)) => {
                assert_eq!(left, right);
                break;
            }
            _ => panic!("storage modes differ at edge {index}"),
        }
        assert_eq!(
            history.active_obligation(checkpoint),
            stream.active_obligation()
        );
        let left = history
            .freeze_completed_obligation(&history_arena, checkpoint)
            .unwrap();
        let right = indexed_completed(&stream, &input).unwrap();
        match (left, right) {
            (Some(left), Some(right)) => {
                assert_eq!(left.obligation, right.obligation);
                assert_eq!(left.payload_end, right.payload_end);
                assert_eq!(
                    left.payload.located_bytes().collect::<Vec<_>>(),
                    right.payload.located_bytes().collect::<Vec<_>>()
                );
                let pending = checkpoint;
                let consumed_before = stream.consumed_len();
                let stats_before = stream.stats();
                assert_eq!(
                    history
                        .edge(&mut history_arena, checkpoint, located)
                        .unwrap_err(),
                    stream.edge(&input, located).unwrap_err()
                );
                assert_eq!(stream.consumed_len(), consumed_before);
                assert_eq!(stream.stats(), stats_before);
                let resolution = match left.obligation.kind {
                    CanonicalPathObligationKind::RawParEscape => {
                        CanonicalPathObligationResolution::RawPar {
                            canonical_path_stable: raw_stable,
                        }
                    }
                    CanonicalPathObligationKind::NestedEpm1 => {
                        CanonicalPathObligationResolution::NestedEpm1 {
                            mode,
                            entries_stable,
                        }
                    }
                };
                checkpoint = history
                    .resolve_obligation(&history_arena, checkpoint, left.obligation.id, resolution)
                    .unwrap();
                stream
                    .resolve_obligation(&input, right.obligation.id, resolution)
                    .unwrap();
                successful += 1;
                assert!(history
                    .value(pending, true, CanonicalPathSource::new(index))
                    .unwrap()
                    .is_err());
            }
            (None, None) => {}
            _ => panic!("storage modes disagree about child completion at {index}"),
        }
        let endpoint = CanonicalPathSource::in_source(99, index);
        assert_eq!(
            history.value(checkpoint, true, endpoint),
            stream.value(true, endpoint)
        );
        let history_stats = history.stats();
        assert_eq!(history_stats.states, successful + 1);
        assert_eq!(stream.stats().states, 1);
        assert!(history_stats.frames >= stream.stats().frames);
        stream.assert_frame_invariants();
        assert_eq!(history_stats.edges, stream.stats().edges);
        assert_consumed_prefix(&stream, &input, &history_arena, checkpoint);
    }
    let endpoint = CanonicalPathSource::in_source(99, 800);
    stream.assert_frame_invariants();
    assert_eq!(
        history.value(checkpoint, true, endpoint),
        stream.value(true, endpoint)
    );
}

fn assert_resolution_recovery(
    depth: usize,
    resolution: CanonicalPathObligationResolution,
    refusal: bool,
) {
    let map = EPathMap::new_map([(integer(1), integer(2))], Vec::new(), false, None);
    let mut key = payload(0x10, map.trie_snapshot());
    for _ in 0..depth {
        let mut region = Vec::new();
        prost::encoding::encode_varint(key.len() as u64, &mut region);
        region.extend_from_slice(&key);
        key = payload(0x0d, &region);
    }
    let mut all_bytes = vec![0x0f, 0];
    all_bytes.extend_from_slice(&key);
    all_bytes.extend_from_slice(if refusal { &[0xff, 0xff, 0xff] } else { &[0] });
    let input = fragmented(&all_bytes);
    let mut history_arena = CanonicalBytesArena::new();
    let mut history = CanonicalPathReducer::new();
    let mut checkpoint = history.root(&history_arena);
    let mut stream = CanonicalPathStreamReducer::new();
    let mut offset = 0;
    for byte in [0x0f, 0] {
        let located = CanonicalLocatedByte::new(byte, origin(offset));
        let (next, transition) = history
            .edge(&mut history_arena, checkpoint, located)
            .unwrap();
        assert_eq!(stream.edge(&input, located).unwrap(), transition);
        checkpoint = next;
        offset += 1;
    }
    let old_id = stream.active_obligation().unwrap().obligation.id;
    let unstable = CanonicalPathObligationResolution::RawPar {
        canonical_path_stable: false,
    };
    checkpoint = history
        .resolve_obligation(&history_arena, checkpoint, old_id, unstable)
        .unwrap();
    stream.resolve_obligation(&input, old_id, unstable).unwrap();

    for byte in key {
        let located = CanonicalLocatedByte::new(byte, origin(offset));
        let (next, transition) = history
            .edge(&mut history_arena, checkpoint, located)
            .unwrap();
        assert_eq!(stream.edge(&input, located).unwrap(), transition);
        checkpoint = next;
        offset += 1;
    }
    let pending = stream.active_obligation().unwrap();
    stream.assert_frame_invariants();
    assert!(pending.awaiting_resolution);
    assert_ne!(pending.obligation.id, old_id);
    assert_eq!(history.active_obligation(checkpoint), Some(pending));
    let frozen = indexed_completed(&stream, &input).unwrap().unwrap();
    let history_frozen = history
        .freeze_completed_obligation(&history_arena, checkpoint)
        .unwrap()
        .unwrap();
    let frozen_bytes = frozen.payload.located_bytes().collect::<Vec<_>>();
    assert_eq!(
        history_frozen.payload.located_bytes().collect::<Vec<_>>(),
        frozen_bytes
    );
    assert_eq!(history_frozen.payload_end, frozen.payload_end);
    let endpoint = CanonicalPathSource::in_source(99, offset);
    let before = stream.value(true, endpoint);
    let history_stats = history.stats();
    let stream_stats = stream.stats();
    let consumed_before = stream.consumed_len();
    let wrong = history
        .resolve_obligation(&history_arena, checkpoint, old_id, resolution)
        .unwrap_err();
    assert_eq!(
        stream
            .resolve_obligation(&input, old_id, resolution)
            .unwrap_err(),
        wrong
    );
    assert_eq!(
        wrong.kind,
        CanonicalPathAdmissionErrorKind::UnresolvedObligation
    );
    assert_eq!(wrong.source, pending.obligation.tag_source);
    assert_eq!(history.stats(), history_stats);
    assert_eq!(stream.stats(), stream_stats);
    assert_eq!(stream.consumed_len(), consumed_before);
    assert_eq!(stream.active_obligation(), Some(pending));
    assert_eq!(stream.value(true, endpoint), before);
    assert_eq!(history.value(checkpoint, true, endpoint), before);
    stream.assert_frame_invariants();

    checkpoint = history
        .resolve_obligation(
            &history_arena,
            checkpoint,
            pending.obligation.id,
            resolution,
        )
        .unwrap();
    stream
        .resolve_obligation(&input, pending.obligation.id, resolution)
        .unwrap();
    if !refusal {
        let located = CanonicalLocatedByte::new(0, origin(offset));
        let (next, transition) = history
            .edge(&mut history_arena, checkpoint, located)
            .unwrap();
        assert_eq!(stream.edge(&input, located).unwrap(), transition);
        checkpoint = next;
        let summary = stream.value(true, endpoint).unwrap().unwrap();
        assert!(!summary.canonical_path_stable);
        assert_eq!(history.value(next, true, endpoint), Some(Ok(summary)));
        assert_eq!(stream.active_obligation(), None);
        stream.assert_frame_invariants();
        assert_consumed_prefix(&stream, &input, &history_arena, checkpoint);
        assert_eq!(
            history_frozen.payload.located_bytes().collect::<Vec<_>>(),
            frozen_bytes
        );
        assert_eq!(
            frozen.payload.located_bytes().collect::<Vec<_>>(),
            frozen_bytes
        );
        return;
    }
    let original_error = stream.value(true, endpoint).unwrap().unwrap_err();
    assert_eq!(original_error.kind, match resolution {
        CanonicalPathObligationResolution::NestedEpm1 {
            mode: NestedEpmMode::Set,
            ..
        } => CanonicalPathAdmissionErrorKind::NestedEpmNotMap,
        CanonicalPathObligationResolution::NestedEpm1 {
            mode: NestedEpmMode::Map,
            entries_stable: false,
        } => CanonicalPathAdmissionErrorKind::NestedEpmNotStable,
        CanonicalPathObligationResolution::RawPar { .. } =>
            CanonicalPathAdmissionErrorKind::RejectedObligation,
        _ => panic!("expected refusal control"),
    });
    assert_eq!(original_error.source, pending.obligation.tag_source);
    assert_eq!(
        history.value(checkpoint, true, endpoint),
        Some(Err(original_error))
    );
    assert_eq!(stream.active_obligation(), None);
    assert!(indexed_completed(&stream, &input).unwrap().is_none());
    let accepted = CanonicalPathObligationResolution::NestedEpm1 {
        mode: NestedEpmMode::Map,
        entries_stable: true,
    };
    for (id, resolution) in [
        (old_id, accepted),
        (pending.obligation.id, accepted),
        (pending.obligation.id, resolution),
    ] {
        let left = history.resolve_obligation(&history_arena, checkpoint, id, resolution);
        let right = stream.resolve_obligation(&input, id, resolution);
        match (left, right) {
            (Ok(next), Ok(())) => checkpoint = next,
            (Err(left), Err(right)) => assert_eq!(left, right),
            other => panic!("post-refusal resolution diverged at depth {depth}: {other:?}"),
        }
        let observed_error = history
            .value(checkpoint, true, endpoint)
            .unwrap()
            .unwrap_err();
        assert_eq!(stream.value(true, endpoint), Some(Err(observed_error)));
        assert_eq!(
            history.active_obligation(checkpoint),
            stream.active_obligation()
        );
        assert_eq!(stream.active_obligation(), None);
        let located = CanonicalLocatedByte::new(0xff, origin(offset));
        let (next, transition) = history
            .edge(&mut history_arena, checkpoint, located)
            .unwrap();
        assert_eq!(stream.edge(&input, located).unwrap(), transition);
        checkpoint = next;
        offset += 1;
        assert_eq!(stream.value(true, endpoint), Some(Err(observed_error)));
        assert_eq!(
            history.value(checkpoint, true, endpoint),
            Some(Err(observed_error))
        );
        assert_eq!(
            frozen.payload.located_bytes().collect::<Vec<_>>(),
            frozen_bytes
        );
        assert_eq!(
            history_frozen.payload.located_bytes().collect::<Vec<_>>(),
            frozen_bytes
        );
        assert_consumed_prefix(&stream, &input, &history_arena, checkpoint);
    }
    stream.assert_frame_invariants();
}

#[test]
fn wrong_id_recovery_and_deferred_refusal_preserve_nested_owners() {
    for depth in [0, 1, 2, 8] {
        assert_resolution_recovery(
            depth,
            CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: true,
            },
            false,
        );
        for resolution in [
            CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Set,
                entries_stable: true,
            },
            CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: false,
            },
            CanonicalPathObligationResolution::RawPar {
                canonical_path_stable: false,
            },
        ] {
            assert_resolution_recovery(depth, resolution, true);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn generated_keys_agree_with_typed_stability(
        n in any::<i64>(), text in ".{0,48}", bytes in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        for value in [integer(n), expr(ExprInstance::GString(text)), expr(ExprInstance::GByteArray(bytes))] {
            let key = encode_trie_path(&value);
            assert_differential(&key);
            let (summary, stats) = admit(&key).unwrap();
            prop_assert!(summary.canonical_path_stable);
            prop_assert_eq!(stats.messages, 0);
            prop_assert_eq!(stats.canonical_path_bytes_read, key.len());
            prop_assert_eq!(stats.canonical_path_bytes_traversed, key.len());
            prop_assert_eq!(stats.max_frames, 1);
            prop_assert_eq!(stats.max_frame_capacity_bytes, stats.frame_variant_bytes);
        }
    }

    #[test]
    fn escape_then_suffix_preserves_cursor_and_three_outcomes(n in any::<i64>()) {
        let stable = encode_trie_path(&integer(n));
        let escaped_stable = payload(0x0f, &protobuf_encoder::encode_to_vec(&integer(n)));
        let unstable = vec![0x0f, 0];
        let mut split = unstable.clone();
        split.extend_from_slice(&stable);
        split.push(0);
        for key in [&stable, &escaped_stable, &unstable, &split] {
            assert_differential(key);
        }
        prop_assert!(admit(&stable).unwrap().0.canonical_path_stable);
        prop_assert!(admit(&escaped_stable).is_err());
        prop_assert!(!admit(&unstable).unwrap().0.canonical_path_stable);
        let (summary, stats) = admit(&split).unwrap();
        prop_assert!(!summary.canonical_path_stable);
        prop_assert_eq!(stats.canonical_path_obligations, 1);
        prop_assert_eq!(stats.canonical_path_resolutions, 1);
        prop_assert_eq!(stats.messages, 1);
        prop_assert_eq!(stats.canonical_path_payload_bytes, 0);
        prop_assert_eq!(stats.canonical_path_bytes_read, split.len());
        split.pop();
        prop_assert!(admit(&split).is_err());
    }

    #[test]
    fn nested_maps_use_shared_children_and_exact_resolution(n in any::<i64>(), v in any::<i64>()) {
        let map = EPathMap::new_map([(integer(n), integer(v))], Vec::new(), false, None);
        let key = payload(0x10, map.trie_snapshot());
        assert_differential(&key);
        let (summary, stats) = admit(&key).unwrap();
        prop_assert!(summary.canonical_path_stable);
        prop_assert_eq!(stats.canonical_path_obligations, 1);
        prop_assert_eq!(stats.canonical_path_resolutions, 1);
        prop_assert_eq!(stats.epathmaps, 1);
        prop_assert_eq!(stats.canonical_path_payload_bytes, map.trie_snapshot().len());
        prop_assert_eq!(stats.typed_decodes, 0);
        prop_assert_eq!(stats.legacy_endpoint_replays, 0);
        prop_assert_eq!(stats.reconstructed_key_bytes, 0);
    }

    #[test]
    fn stream_storage_matches_persistent_byte_and_resolution_traces(
        n in any::<i64>(), raw_stable in any::<bool>(), entries_stable in any::<bool>(),
    ) {
        let map = EPathMap::new_map([(integer(n), integer(3))], Vec::new(), false, None);
        let mut key = vec![0x0f, 0];
        key.extend(encode_trie_path(&integer(n)));
        key.extend(payload(0x10, map.trie_snapshot()));
        key.push(0);
        for mode in [NestedEpmMode::Empty, NestedEpmMode::Set, NestedEpmMode::Map] {
            assert_storage_trace(&key, raw_stable, mode, entries_stable);
        }
    }
}

#[test]
fn stream_region_order_and_malformed_traces_match_persistent_storage() {
    for key in [
        vec![],
        vec![3],
        vec![0x0d, 4, 1, 1, 1, 2],
        vec![0x0d, 4, 1, 2, 1, 1],
        vec![0x0d, 4, 1, 1, 1, 1],
        vec![0x0d, 3, 3, 1],
        vec![4, 1, 0xff],
        vec![0x0c, 2, 1, 2],
        vec![0x0c, 1, 0x0d, 4, 1, 1, 1, 2],
        vec![0x0f, 0, 0x0f, 0, 0],
    ] {
        assert_storage_trace(&key, false, NestedEpmMode::Map, true);
        assert_differential(&key);
    }
}

#[test]
fn exact_completion_rejects_truncation_trailing_and_malformed_grammar() {
    for bytes in [
        vec![],
        vec![3],
        vec![3, 0x80, 0],
        vec![0, 1],
        vec![0x0b, 0],
        vec![0x0c, 1, 0],
        vec![0x0f],
        vec![0x0f, 1],
        vec![0x10, 0],
        vec![4, 1, 0xff],
    ] {
        assert_differential(&bytes);
        assert!(admit(&bytes).is_err(), "{bytes:?}");
    }
    for bytes in [vec![0], vec![1], vec![3, 0], vec![0x0f, 0]] {
        assert_differential(&bytes);
        assert!(admit(&bytes).is_ok());
    }
}

#[test]
fn nested_epm_requires_map_mode_and_stable_entries() {
    let maps = [
        EPathMap::default(),
        EPathMap::from_set_iter([integer(1)], Vec::new(), false, None),
        EPathMap::new_map([(integer(1), Par::default())], Vec::new(), false, None),
        EPathMap::new_map([(Par::default(), integer(1))], Vec::new(), false, None),
    ];
    for map in maps {
        let key = payload(0x10, map.trie_snapshot());
        assert_differential(&key);
        assert!(admit(&key).is_err());
    }
}

#[test]
fn child_error_precedes_bad_parent_suffix_and_preserves_physical_anchor() {
    match validate_canonical_path_indexed(fragmented(&[0x10, 0])).unwrap_err() {
        CanonicalProtobufAdmissionError::EPathMap { position, .. } => {
            assert_eq!(position, origin(1));
        }
        other => panic!("expected empty child EPM error, got {other}"),
    }
    let key = [0x0f, 1, 0x80];
    let mut suffixed = key.to_vec();
    suffixed.extend([0xff, 0]);
    assert_eq!(admit(&key).unwrap_err(), admit(&suffixed).unwrap_err());
    let frozen = validate_canonical_path_indexed(fragmented(&suffixed)).unwrap_err();
    match frozen {
        CanonicalProtobufAdmissionError::Framing { physical, .. } => {
            assert_eq!(physical.physical, origin(2));
        }
        other => panic!("expected child framing error, got {other}"),
    }
    let root = validate_canonical_path_indexed(fragmented(&[3])).unwrap_err();
    match root {
        CanonicalProtobufAdmissionError::CanonicalPath { position, .. } => {
            assert_eq!(position, CanonicalPathSource::in_source(99, 800));
        }
        other => panic!("expected root EOF error, got {other}"),
    }
}

#[test]
fn pending_parent_cannot_advance_or_accept_wrong_summary() {
    let bytes = [0x0f, 0];
    let mut frame = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&bytes, 0, 0));
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let FrameAction::Push(child) = frame.step(&mut stats).unwrap() else {
        panic!("empty escape must dispatch a child");
    };
    let cursor = frame.cursor.bytes_yielded();
    assert!(frame.step(&mut stats).is_err());
    assert_eq!(frame.cursor.bytes_yielded(), cursor);
    let (summary, _, _) = drive_admission(child, 0).unwrap();
    assert!(
        frame
            .reducer
            .active_obligation()
            .unwrap()
            .awaiting_resolution
    );
    frame.accept(summary, &mut stats).unwrap();
    assert!(frame.reducer.active_obligation().is_none());
    assert_eq!(frame.reducer.stats().states, 1);
    let FrameAction::Complete(FrameSummary::CanonicalPath(summary)) =
        frame.step(&mut stats).unwrap()
    else {
        panic!("resolved key must complete");
    };
    assert!(!summary.canonical_path_stable);

    let mut wrong = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&bytes, 0, 0));
    assert!(matches!(
        wrong.step(&mut stats).unwrap(),
        FrameAction::Push(_)
    ));
    assert!(wrong
        .accept(FrameSummary::CanonicalPath(summary), &mut stats)
        .is_err());
}

#[test]
fn stable_escape_installs_deferred_refusal_in_current_state() {
    let key = payload(0x0f, &protobuf_encoder::encode_to_vec(&integer(7)));
    let mut frame = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 0, 0));
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let FrameAction::Push(child) = frame.step(&mut stats).unwrap() else {
        panic!("escape child")
    };
    assert!(
        frame
            .reducer
            .active_obligation()
            .unwrap()
            .awaiting_resolution
    );
    let (summary, _, _) = drive_admission(child, 0).unwrap();
    frame.accept(summary, &mut stats).unwrap();
    assert_eq!(frame.reducer.stats().states, 1);
    let error = frame
        .step(&mut stats)
        .err()
        .expect("stable escape must refuse");
    assert!(error.to_string().contains("EscapeOfStablePar"), "{error}");
}

#[test]
fn raw_par_child_and_nested_epm_share_one_driver() {
    let mut value = integer(3);
    for _ in 0..16 {
        value = expr(ExprInstance::EPathmapBody(EPathMap::new_map(
            [(integer(1), value)],
            Vec::new(),
            false,
            None,
        )));
    }
    let raw = value.with_sends(vec![Send {
        chan: Some(integer(7)),
        ..Send::default()
    }]);
    let key = encode_trie_path(&raw);
    assert_eq!(key[0], 0x0f);
    assert_differential(&key);
    let (summary, stats) = admit(&key).unwrap();
    assert!(!summary.canonical_path_stable);
    assert_eq!(stats.epathmaps, 16);
    assert_eq!(stats.canonical_path_obligations, 1);
    assert_eq!(stats.canonical_path_resolutions, 1);
    assert!(stats.max_frames > 16);
    assert!(stats.max_frame_capacity_bytes >= stats.max_frame_storage_bytes);
}

#[test]
fn a_previous_obligation_id_cannot_resolve_the_next_child() {
    let key = [0x0f, 0, 0x0f, 0, 0];
    let mut frame = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 0, 0));
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let FrameAction::Push(first) = frame.step(&mut stats).unwrap() else {
        panic!("first child")
    };
    let previous_id = frame.waiting.as_ref().unwrap().id;
    let (summary, _, _) = drive_admission(first, 0).unwrap();
    frame.accept(summary, &mut stats).unwrap();
    let FrameAction::Push(second) = frame.step(&mut stats).unwrap() else {
        panic!("second child")
    };
    assert_ne!(frame.waiting.as_ref().unwrap().id, previous_id);
    frame.waiting.as_mut().unwrap().id = previous_id;
    let (summary, _, _) = drive_admission(second, 0).unwrap();
    assert!(frame.accept(summary, &mut stats).is_err());
    assert_eq!(stats.canonical_path_resolutions, 1);
    assert_differential(&key);
}

#[test]
fn standalone_children_share_borrowed_payloads_and_survive_parent_drop() {
    let map = EPathMap::new_map([(integer(1), integer(2))], Vec::new(), false, None);
    for (tag, body) in [
        (0x0f, protobuf_encoder::encode_to_vec(&integer(7))),
        (0x10, map.trie_snapshot().to_vec()),
    ] {
        let mut key = payload(tag, &body);
        let consumed = key.len();
        let start = consumed - body.len();
        key.extend([0xff, 0]);
        let child = {
            let mut parent =
                CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 31, 101));
            let mut stats = CanonicalProtobufAdmissionStats::default();
            let FrameAction::Push(child) = parent.step(&mut stats).unwrap() else {
                panic!("opaque payload must suspend for child admission");
            };
            assert_eq!(parent.reducer.consumed_len(), consumed);
            assert_eq!(parent.cursor.bytes_yielded(), consumed);
            assert_eq!(stats.canonical_path_obligations, 1);
            assert_eq!(stats.canonical_path_payload_bytes, body.len());
            child
        };
        let region = match &child {
            Frame::Message(message) => &message.body,
            Frame::EPathMap(epm) => &epm.snapshot,
            _ => panic!("unexpected child kind"),
        };
        let retained =
            rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource::contiguous_range(
                region,
                0..region.len(),
            )
            .unwrap();
        assert_eq!(retained.as_ptr(), key[start..].as_ptr());
        assert_eq!(retained, body);
        assert_eq!(
            region.position_at(0).unwrap().physical,
            CanonicalByteProvenance::new(31, 101 + start)
        );
        assert_eq!(
            region.end_position().physical,
            CanonicalByteProvenance::new(31, 101 + consumed - 1)
        );
        drive_admission(child, 0).unwrap();
    }
}

#[test]
fn flat_key_storage_and_frame_layout_are_observed_directly() {
    #[allow(dead_code)]
    enum ExistingVariantsFrame<'a> {
        Message(MessageFrame<'a>),
        Map(MapFrame<'a>),
        EPathMap(EPathMapFrame<'a>),
    }
    assert_eq!(
        std::mem::size_of::<Frame>(),
        std::mem::size_of::<ExistingVariantsFrame>()
    );
    println!(
        "canonical-key frame bytes: existing_variants={}, including_canonical_path={}",
        std::mem::size_of::<ExistingVariantsFrame>(),
        std::mem::size_of::<Frame>()
    );
    println!(
        "canonical-key transport layout: indexed_region={}, canonical_path_frame={}",
        std::mem::size_of::<IndexedByteRegion>(),
        std::mem::size_of::<CanonicalPathFrame>()
    );
    let mut exclusive_frame_capacity = None;
    for width in [16, 128, 1024, 8192] {
        let key = payload(6, &vec![b'x'; width]);
        let mut frame = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 0, 0));
        let mut stats = CanonicalProtobufAdmissionStats::default();
        assert!(matches!(
            frame.step(&mut stats).unwrap(),
            FrameAction::Complete(_)
        ));
        let reducer = frame.reducer.stats();
        assert_eq!(reducer.edges, key.len());
        assert_eq!(reducer.states, 1);
        assert_eq!(frame.reducer.consumed_len(), key.len());
        let backing = rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource::contiguous_range(
            &frame.input,
            0..key.len(),
        )
        .unwrap();
        assert_eq!(backing.as_ptr(), key.as_ptr());
        assert_eq!(stats.canonical_path_bytes_read, key.len());
        assert_eq!(stats.canonical_path_bytes_traversed, key.len());
        let mut history_arena = CanonicalBytesArena::new();
        let mut history = CanonicalPathReducer::new();
        let mut current = history.root(&history_arena);
        for (index, byte) in key.iter().copied().enumerate() {
            (current, _) = history
                .edge(
                    &mut history_arena,
                    current,
                    CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(0, index)),
                )
                .unwrap();
        }
        assert_eq!(history.stats().states, key.len() + 1);
        assert_eq!(history.stats().frames, key.len());
        assert_eq!(reducer.frames, 2);
        frame.reducer.assert_frame_invariants();
        let retained = frame.reducer.retained_frame_bytes();
        assert_eq!(*exclusive_frame_capacity.get_or_insert(retained), retained);
        let arena = history_arena.stats();
        assert_eq!(arena.payload_slots, key.len());
        assert_eq!(arena.byte_pages, arena.provenance_pages);
        assert!(
            history.retained_state_bytes()
                >= history.stats().states * frame.reducer.retained_state_bytes()
        );
        println!("canonical-key state storage: width={width}, persistent_bytes={}, exclusive_inline_bytes={}",
            history.retained_state_bytes(), frame.reducer.retained_state_bytes());
        println!("canonical-key frame storage: width={width}, persistent_bytes={}, exclusive_bytes={}, exclusive_slots={}",
            history.retained_frame_bytes(), retained, reducer.frames);
        println!("canonical-key persistent reference: width={width}, bytes={}, states={}, frames={}, byte_pages={}, provenance_pages={}, payload_slots={}",
            key.len(), history.stats().states, history.stats().frames, arena.byte_pages,
            arena.provenance_pages, arena.payload_slots);
        println!("canonical-key indexed transport: width={width}, consumed={}, borrowed_input_bytes={}, bytes_read={}, bytes_traversed={}",
            frame.reducer.consumed_len(), backing.len(), stats.canonical_path_bytes_read, stats.canonical_path_bytes_traversed);
    }
}

#[test]
fn deep_tuple_admission_and_teardown_use_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut key = Vec::with_capacity(40_001);
            for _ in 0..20_000 {
                key.extend([0x0c, 1]);
            }
            key.push(1);
            let (summary, stats) = admit(&key).unwrap();
            assert!(summary.canonical_path_stable);
            assert_eq!(stats.max_frames, 1);
            assert_eq!(stats.canonical_path_bytes_traversed, key.len());
        })
        .unwrap()
        .join()
        .unwrap();
}
