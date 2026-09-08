use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::canonical_path::{encode_trie_path, validate_canonical_path};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
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
    let unresolved = frame.current;
    frame.accept(summary, &mut stats).unwrap();
    assert_ne!(frame.current, unresolved);
    assert!(frame
        .reducer
        .value(unresolved, true, CanonicalPathSource::new(2))
        .unwrap()
        .is_err());
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
fn stable_escape_installs_deferred_refusal_in_new_handle() {
    let key = payload(0x0f, &protobuf_encoder::encode_to_vec(&integer(7)));
    let mut frame = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 0, 0));
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let FrameAction::Push(child) = frame.step(&mut stats).unwrap() else {
        panic!("escape child")
    };
    let old = frame.current;
    let (summary, _, _) = drive_admission(child, 0).unwrap();
    frame.accept(summary, &mut stats).unwrap();
    assert_ne!(frame.current, old);
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
fn flat_key_storage_and_frame_layout_are_observed_directly() {
    #[allow(dead_code)]
    enum PreviousFrame<'a> {
        Message(MessageFrame<'a>),
        Map(MapFrame<'a>),
        EPathMap(EPathMapFrame<'a>),
    }
    assert_eq!(
        std::mem::size_of::<Frame>(),
        std::mem::size_of::<PreviousFrame>()
    );
    println!(
        "canonical-key frame bytes: previous={}, current={}",
        std::mem::size_of::<PreviousFrame>(),
        std::mem::size_of::<Frame>()
    );
    for width in [16, 128, 1024, 8192] {
        let key = payload(6, &vec![b'x'; width]);
        let mut frame = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 0, 0));
        let mut stats = CanonicalProtobufAdmissionStats::default();
        assert!(matches!(
            frame.step(&mut stats).unwrap(),
            FrameAction::Complete(_)
        ));
        let reducer = frame.reducer.stats();
        let arena = frame.arena.stats();
        assert_eq!(reducer.edges, key.len());
        assert_eq!(reducer.states, key.len() + 1);
        assert_eq!(arena.payload_slots, key.len());
        assert_eq!(arena.divergent_segments, arena.page_rollovers);
        assert_eq!(arena.byte_pages, arena.page_rollovers + 1);
        assert_eq!(arena.concats, 0);
        assert!(arena.byte_pages <= key.len());
        assert_eq!(arena.byte_pages, arena.provenance_pages);
        println!("canonical-key retained: width={width}, bytes={}, states={}, frames={}, byte_pages={}, provenance_pages={}, payload_slots={}",
            key.len(), reducer.states, reducer.frames, arena.byte_pages,
            arena.provenance_pages, arena.payload_slots);
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
