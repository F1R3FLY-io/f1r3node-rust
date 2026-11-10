use proptest::prelude::*;
use rspace_plus_plus::rspace::pathmap_codec::CanonicalCompactTrieLineDirection;

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

impl<'src> EPathMapFrame<'src> {
    fn from_epm1_region(
        owner: &'static RhoMatchProgram,
        field: u32,
        snapshot: AdmissionRegion<'src>,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        Self::with_epm1_region(owner, field, snapshot, Ok)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EpmBatchObservation {
    forward_batches: usize,
    forward_bytes: usize,
    max_forward: usize,
    unwind_batches: usize,
    unwind_bytes: usize,
    max_unwind: usize,
}

thread_local! {
    static SCALAR_EPM_TRANSPORT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static EPM_BATCHES: std::cell::Cell<Option<EpmBatchObservation>> = const {
        std::cell::Cell::new(None)
    };
}

pub(super) fn scalar_epm_transport() -> bool { SCALAR_EPM_TRANSPORT.with(std::cell::Cell::get) }

pub(super) fn record_epm_batch(direction: CanonicalCompactTrieLineDirection, width: usize) {
    assert!(width > 0);
    EPM_BATCHES.with(|cell| {
        if let Some(mut observed) = cell.get() {
            match direction {
                CanonicalCompactTrieLineDirection::Forward => {
                    observed.forward_batches += 1;
                    observed.forward_bytes += width;
                    observed.max_forward = observed.max_forward.max(width);
                }
                CanonicalCompactTrieLineDirection::Unwind => {
                    observed.unwind_batches += 1;
                    observed.unwind_bytes += width;
                    observed.max_unwind = observed.max_unwind.max(width);
                }
            }
            cell.set(Some(observed));
        }
    });
}

fn with_epm_transport<T>(scalar: bool, action: impl FnOnce() -> T) -> (T, EpmBatchObservation) {
    struct Restore {
        scalar: bool,
        observed: Option<EpmBatchObservation>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            SCALAR_EPM_TRANSPORT.with(|cell| cell.set(self.scalar));
            EPM_BATCHES.with(|cell| cell.set(self.observed));
        }
    }
    let _restore = Restore {
        scalar: SCALAR_EPM_TRANSPORT.with(|cell| cell.replace(scalar)),
        observed: EPM_BATCHES.with(|cell| cell.replace(Some(EpmBatchObservation::default()))),
    };
    let result = action();
    (result, EPM_BATCHES.with(|cell| cell.get().unwrap()))
}

pub(super) fn with_epm_transport_mode<T>(scalar: bool, action: impl FnOnce() -> T) -> T {
    with_epm_transport(scalar, action).0
}

fn expr(value: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(value),
    }])
}

fn integer(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

fn admit_detached_path_child<'src>(
    request: FrameRequest<'src>,
) -> Result<FrameSummary<'src>, CanonicalProtobufAdmissionError> {
    let (region, owner, field) = match request {
        FrameRequest::EPathMap {
            region,
            owner,
            field,
        } => (region, owner, field),
        request @ FrameRequest::Message { .. } => {
            return drive_admission(request, 0).map(|(summary, _, _)| summary);
        }
        _ => panic!("detached path child has an unexpected frame kind"),
    };
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let mut stack = vec![EPathMapFrame::from_epm1_region(owner, field, region)?];
    loop {
        let action = epathmap_step(stack.last_mut().unwrap(), &mut stats)?;
        match action {
            FrameAction::Continue => {}
            FrameAction::Push(FrameRequest::EPathMap {
                region,
                owner,
                field,
            }) => stack.push(EPathMapFrame::from_epm1_region(owner, field, region)?),
            FrameAction::Push(request @ FrameRequest::Message { .. }) => {
                let (summary, _, _) = drive_admission(request, 0)?;
                stack.last_mut().unwrap().accept(summary, &mut stats)?;
            }
            FrameAction::Push(_) => panic!("EPathMap child pushed an unexpected frame kind"),
            FrameAction::Complete(summary) => {
                stack.pop();
                if let Some(parent) = stack.last_mut() {
                    parent.accept(summary, &mut stats)?;
                } else {
                    return Ok(summary);
                }
            }
        }
    }
}

#[test]
fn deferred_request_transport_is_smaller_than_frames() {
    assert!(std::mem::size_of::<FrameRequest>() < std::mem::size_of::<Frame>());
    assert!(std::mem::size_of::<FrameAction>() < std::mem::size_of::<Frame>());
}

#[test]
fn deferred_request_constructor_errors_precede_publication() {
    static INVALID_PROGRAM: RhoMatchProgram = RhoMatchProgram {
        type_id: 0,
        type_name: "InvalidAdmissionControl",
        fields: &[super::super::match_schema::RhoMatchField {
            tag: 1,
            name: "invalid",
            kind: RhoMatchFieldKind::Bool,
        }],
        protobuf_fields: &[],
    };
    let input = IndexedByteRegion::from_borrowed(&[], 37, 101);
    let message_error = MessageFrame::from_message_region(input.clone(), &INVALID_PROGRAM)
        .err()
        .unwrap();
    let epm_error = EPathMapFrame::from_epm1_region(&PAR_PROTOBUF_REGION_PROGRAM, 7, input.clone())
        .err()
        .unwrap();
    for (request, expected) in [
        (
            FrameRequest::Message {
                region: input.clone(),
                program: &INVALID_PROGRAM,
            },
            message_error,
        ),
        (
            FrameRequest::EPathMap {
                region: input,
                owner: &PAR_PROTOBUF_REGION_PROGRAM,
                field: 7,
            },
            epm_error,
        ),
    ] {
        let mut frames = Vec::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            messages: usize::MAX,
            map_entries: usize::MAX,
            epathmaps: usize::MAX,
            ..Default::default()
        };
        let before = stats;
        assert_eq!(
            push_admission_frame(&mut frames, request, &mut stats).unwrap_err(),
            expected
        );
        assert!(frames.is_empty());
        assert_eq!(stats, before);
    }
    let mut frames = Vec::new();
    let mut stats = CanonicalProtobufAdmissionStats {
        messages: usize::MAX,
        ..Default::default()
    };
    let before = stats;
    let request = FrameRequest::Message {
        region: IndexedByteRegion::from_borrowed(&[], 37, 101),
        program: &PAR_PROTOBUF_REGION_PROGRAM,
    };
    assert_eq!(
        push_admission_frame(&mut frames, request, &mut stats).unwrap_err(),
        CanonicalProtobufAdmissionError::SizeOverflow
    );
    assert!(frames.is_empty());
    assert_eq!(stats, before);
}

#[test]
fn deferred_request_variants_publish_exactly_once() {
    let map = EPathMap::new_map([(integer(1), integer(2))], Vec::new(), false, None);
    let mut frames = Vec::new();
    let mut stats = CanonicalProtobufAdmissionStats::default();
    let input = IndexedByteRegion::from_borrowed(&[], 37, 101);
    let requests = [
        FrameRequest::Message {
            region: input.clone(),
            program: &PAR_PROTOBUF_REGION_PROGRAM,
        },
        FrameRequest::Map {
            region: input.clone(),
            owner: &PAR_PROTOBUF_REGION_PROGRAM,
            field: 7,
            value_type_id: PAR_PROTOBUF_REGION_PROGRAM.type_id,
        },
        FrameRequest::EPathMap {
            region: IndexedByteRegion::from_borrowed(map.trie_snapshot(), 39, 201),
            owner: &PAR_PROTOBUF_REGION_PROGRAM,
            field: 8,
        },
        FrameRequest::CanonicalPath(input),
    ];
    for (index, request) in requests.into_iter().enumerate() {
        let before_len = frames.len();
        let before_capacity = frames.capacity();
        let request_kind = request.request_kind();
        let observed = push_admission_frame(&mut frames, request, &mut stats).unwrap();
        assert_eq!(observed.request_kind, request_kind);
        assert_eq!(observed.before_len, before_len);
        assert_eq!(observed.before_capacity, before_capacity);
        assert_eq!(observed.after_len, frames.len());
        assert_eq!(observed.after_capacity, frames.capacity());
        assert_eq!(observed.frame_variant_bytes, std::mem::size_of::<Frame>());
        assert_eq!(
            observed.stats_frame_variant_bytes,
            stats.frame_variant_bytes
        );
        assert_eq!(
            observed.used_bytes,
            frames.len() * observed.frame_variant_bytes
        );
        assert_eq!(
            observed.reserved_bytes,
            frames.capacity() * observed.frame_variant_bytes
        );
        assert_eq!(observed.after_len, observed.before_len + 1);
        assert!(observed.after_capacity >= observed.before_capacity);
        assert!(observed.after_capacity >= observed.after_len);
        assert_eq!(frames.len(), index + 1);
        assert_eq!(stats.messages, 1);
        assert_eq!(stats.map_entries, usize::from(index >= 1));
        assert_eq!(stats.epathmaps, usize::from(index >= 2));
        assert_eq!(stats.max_frames, frames.len());
        assert_eq!(
            stats.max_frame_storage_bytes,
            frames.len() * std::mem::size_of::<Frame>()
        );
        assert_eq!(
            stats.max_frame_capacity_bytes,
            frames.capacity() * std::mem::size_of::<Frame>()
        );
    }
    let Frame::Message(message) = &frames[0] else {
        panic!("expected message")
    };
    assert!(std::ptr::eq(message.program, &PAR_PROTOBUF_REGION_PROGRAM));
    assert!(message.body.is_empty());
    assert_eq!(message.region_start, CanonicalByteProvenance::new(37, 101));
    assert!(message.waiting.is_none());
    let Frame::Map(entry) = &frames[1] else {
        panic!("expected map entry")
    };
    assert!(std::ptr::eq(entry.owner, &PAR_PROTOBUF_REGION_PROGRAM));
    assert_eq!(entry.outer_field, 7);
    assert_eq!(entry.value_type_id, PAR_PROTOBUF_REGION_PROGRAM.type_id);
    assert!(entry.region.is_empty());
    assert_eq!(entry.region_start, CanonicalByteProvenance::new(37, 101));
    assert!(!entry.waiting_value);
    let Frame::EPathMap(epm) = &frames[2] else {
        panic!("expected EPathMap")
    };
    assert!(std::ptr::eq(epm.owner, &PAR_PROTOBUF_REGION_PROGRAM));
    assert_eq!(epm.field, 8);
    assert_eq!(epm.mode, EPathMapMode::Map);
    assert_eq!(epm.region_start, CanonicalByteProvenance::new(39, 201));
    assert_eq!(
        epm.snapshot.bytes().collect::<Vec<_>>(),
        map.trie_snapshot()
    );
    assert!(epm.waiting.is_none());
    let Frame::CanonicalPath(path) = &frames[3] else {
        panic!("expected canonical path")
    };
    assert!(path.input.is_empty());
    assert_eq!(
        path.input.end_position().physical,
        CanonicalByteProvenance::new(37, 101)
    );
    assert_eq!(path.cursor.bytes_yielded(), 0);
    assert_eq!(path.reducer.consumed_len(), 0);
    assert!(path.waiting.is_none());
}

#[test]
fn deferred_obligation_preparation_overflow_precedes_constructor_failure() {
    let key = [0x10, 1, 0xff];
    for payload_overflow in [false, true] {
        let mut parent = CanonicalPathFrame::new(IndexedByteRegion::from_borrowed(&key, 37, 101));
        let mut stats = CanonicalProtobufAdmissionStats {
            canonical_path_obligations: if payload_overflow { 0 } else { usize::MAX },
            canonical_path_payload_bytes: if payload_overflow { usize::MAX } else { 0 },
            ..Default::default()
        };
        let mut expected = stats;
        if payload_overflow {
            expected.canonical_path_obligations = 1;
        }
        assert_eq!(
            parent.step(&mut stats).err().unwrap(),
            CanonicalProtobufAdmissionError::SizeOverflow
        );
        assert_eq!(stats, expected);
        assert!(parent.waiting.is_none());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn deferred_epathmap_constructor_matches_direct_constructor(bytes in prop::collection::vec(any::<u8>(), 0..192)) {
        for input in [IndexedByteRegion::from_borrowed(&bytes, 37, 101), fragmented(&bytes)] {
            let direct = EPathMapFrame::from_epm1_region(&PAR_PROTOBUF_REGION_PROGRAM, 7, input.clone());
            let mut frames = Vec::new();
            let mut constructor_stats = CanonicalProtobufAdmissionStats::default();
            let deferred = push_admission_frame(
                &mut frames,
                FrameRequest::EPathMap {
                    region: input, owner: &PAR_PROTOBUF_REGION_PROGRAM, field: 7,
                },
                &mut constructor_stats,
            ).map(|_| ());
            match (direct, deferred) {
                (Err(expected), Err(actual)) => {
                    prop_assert_eq!(actual, expected);
                    prop_assert!(frames.is_empty());
                    prop_assert_eq!(constructor_stats, CanonicalProtobufAdmissionStats::default());
                }
                (Ok(expected), Ok(())) => {
                    prop_assert_eq!(frames.len(), 1);
                    prop_assert_eq!(constructor_stats.epathmaps, 1);
                    let Frame::EPathMap(actual) = &frames[0] else {
                        panic!("deferred constructor must publish an EPathMap frame");
                    };
                    prop_assert_eq!(actual.mode, expected.mode);
                    prop_assert_eq!(actual.region_start, expected.region_start);
                    prop_assert_eq!(actual.snapshot.len(), expected.snapshot.len());
                    prop_assert_eq!(actual.snapshot.end_position(), expected.snapshot.end_position());
                    prop_assert_eq!(actual.snapshot.bytes().collect::<Vec<_>>(), expected.snapshot.bytes().collect::<Vec<_>>());
                }
                _ => prop_assert!(false, "deferred constructor differs from direct constructor"),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FullAdmissionPathObservation {
    edges: usize,
    states: usize,
    frames: usize,
    state_owner_bytes: usize,
    frame_bytes: usize,
    restore_bytes: usize,
    repeated_owner_work: usize,
    observations: usize,
}

thread_local! {
    static FULL_ADMISSION_PATH: std::cell::Cell<Option<FullAdmissionPathObservation>> = const {
        std::cell::Cell::new(None)
    };
}

pub(super) fn record_full_admission_path(path: &CanonicalPathTopologyAdmission) {
    FULL_ADMISSION_PATH.with(|cell| {
        if let Some(mut observation) = cell.get() {
            let stats = path.stats();
            let (state_owner_bytes, frame_bytes, restore_bytes, work) = path.checkpoint_resources();
            observation.edges = observation.edges.max(stats.edges);
            observation.states = observation.states.max(stats.states);
            observation.frames = observation.frames.max(stats.frames);
            observation.state_owner_bytes = observation.state_owner_bytes.max(state_owner_bytes);
            observation.frame_bytes = observation.frame_bytes.max(frame_bytes);
            observation.restore_bytes = observation.restore_bytes.max(restore_bytes);
            observation.repeated_owner_work = observation.repeated_owner_work.max(
                stats.region_descents
                    + work.active_queries
                    + work.completed_queries
                    + work.rebuilds
                    + work.reifications,
            );
            observation.observations += 1;
            cell.set(Some(observation));
        }
    });
}

fn observe_full_admission<T>(action: impl FnOnce() -> T) -> (T, FullAdmissionPathObservation) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { FULL_ADMISSION_PATH.with(|cell| cell.set(None)); }
    }
    FULL_ADMISSION_PATH.with(|cell| {
        assert!(cell.get().is_none());
        cell.set(Some(FullAdmissionPathObservation::default()));
    });
    let _reset = Reset;
    let result = action();
    (result, FULL_ADMISSION_PATH.with(|cell| cell.get().unwrap()))
}

fn full_admission_fixture(
    depth: usize,
    width: usize,
    endpoints: usize,
    malformed: bool,
) -> Vec<u8> {
    use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};
    use crate::rust::rholang::canonical_path_admission::frame_storage_tests::nested_persistent_key;

    fn field(number: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        prost::encoding::encode_key(
            number,
            prost::encoding::WireType::LengthDelimited,
            &mut bytes,
        );
        prost::encoding::encode_varint(body.len() as u64, &mut bytes);
        bytes.extend_from_slice(body);
        bytes
    }

    let mut key = nested_persistent_key(depth, width);
    let mut trie = pathmap::PathMap::new();
    for index in 0..endpoints {
        *key.last_mut().unwrap() = u8::try_from(index).unwrap();
        trie.insert(&key, integer(7));
    }
    if malformed {
        trie.insert(&key[..key.len() - 1], integer(7));
    }
    let snapshot = epathmap_trie_codec::encode(&EPathMapRepr::Map(trie));
    field(5, &field(32, &field(9, &snapshot)))
}

#[test]
fn full_admission_shared_region_checkpoint_matrix() {
    use crate::rust::par_bag::ParBag;
    use crate::rust::rholang::protobuf_decoder;

    for depth in [0, 8, 32] {
        for endpoints in [1, 16, 256] {
            for width in [16, 1_024] {
                let bytes = full_admission_fixture(depth, width, endpoints, false);
                let expected = protobuf_decoder::decode_par(bytes.as_slice()).unwrap();
                assert_eq!(protobuf_encoder::encode_to_vec(&expected), bytes);
                let (expression, _) = expected.first::<Expr>().unwrap().unwrap();
                let Some(ExprInstance::EPathmapBody(map)) = &expression.expr_instance else {
                    panic!("fixture must contain one EPathMap");
                };
                assert_eq!(map.len(), endpoints);
                let compact = expected.components().compact_bytes();
                let restored = ParBag::from_compact_bytes(&compact).unwrap();
                assert_eq!(restored, *expected.components());
                let mut expected_score = Vec::new();
                expected
                    .components()
                    .append_par_score_key(false, &mut expected_score);
                let mut control = None;
                for (backing, input) in [
                    ("borrowed", IndexedByteRegion::from_borrowed(&bytes, 0, 0)),
                    ("fragmented", fragmented(&bytes)),
                ] {
                    let (result, observed) = observe_full_admission(|| {
                        validate_canonical_protobuf_inner(input, &PAR_PROTOBUF_REGION_PROGRAM, None)
                    });
                    let admission = result.unwrap();
                    let stats = admission.stats();
                    assert_eq!(admission.score_bytes().unwrap(), expected_score);
                    assert_eq!(stats.epathmaps, 1);
                    assert_eq!(stats.byte_native_validations, 1);
                    assert_eq!(stats.reconstructed_key_bytes, 0);
                    assert_eq!(stats.legacy_endpoint_replays, 0);
                    assert_eq!(stats.typed_decodes, 0);
                    assert_eq!(observed.repeated_owner_work, 0);
                    assert_eq!(observed.states, observed.edges + 1);
                    assert!(observed.frames <= 2 * observed.edges + 1);
                    assert!(observed.observations >= endpoints);
                    assert!(stats.max_frames <= 8);
                    if let Some(before) = control {
                        assert_eq!(observed, before);
                    } else {
                        control = Some(observed);
                    }
                    println!(
                        "full-admission depth={depth} endpoints={endpoints} width={width} backing={backing} encoded={} edges={} states={} frames={} state_owner_bytes={} frame_bytes={} restore_bytes={} outer_frame_capacity={} repeated_owner_work={} observations={}",
                        bytes.len(),
                        observed.edges,
                        observed.states,
                        observed.frames,
                        observed.state_owner_bytes,
                        observed.frame_bytes,
                        observed.restore_bytes,
                        stats.max_frame_capacity_bytes,
                        observed.repeated_owner_work,
                        observed.observations,
                    );
                }
            }
        }
    }
}

#[test]
fn full_admission_incomplete_endpoint_preserves_failure_provenance() {
    let good = full_admission_fixture(32, 1_024, 256, false);
    let bad = full_admission_fixture(32, 1_024, 256, true);
    for input in [
        IndexedByteRegion::from_borrowed(&good, 0, 0),
        fragmented(&good),
    ] {
        validate_canonical_protobuf_inner(input, &PAR_PROTOBUF_REGION_PROGRAM, None).unwrap();
    }
    let mut first = None;
    for (backing, input) in [
        ("borrowed", IndexedByteRegion::from_borrowed(&bad, 0, 0)),
        ("fragmented", fragmented(&bad)),
    ] {
        let (result, observed) = observe_full_admission(|| {
            validate_canonical_protobuf_inner(input, &PAR_PROTOBUF_REGION_PROGRAM, None)
        });
        let CanonicalProtobufAdmissionError::CanonicalPath {
            position, detail, ..
        } = result.unwrap_err()
        else {
            panic!("incomplete valued endpoint must fail canonical path admission");
        };
        assert!(detail.contains("Truncated"), "{detail}");
        assert_eq!(observed.repeated_owner_work, 0);
        assert_eq!(observed.states, observed.edges + 1);
        assert!(observed.frames <= 2 * observed.edges + 1);
        assert!(observed.observations > 0);
        if let Some((offset, previous)) = first {
            let expected = origin(offset);
            assert_eq!(
                position,
                CanonicalPathSource::in_source(expected.source, expected.offset)
            );
            assert_eq!(observed, previous);
        } else {
            assert_eq!(position.source, 0);
            first = Some((position.offset, observed));
        }
        println!(
            "full-admission malformed backing={backing} position={position:?} observation={observed:?}"
        );
    }
}

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

fn joint_epm_key(nested: bool, width: usize, index: u8) -> Vec<u8> {
    let mut data = vec![37; width];
    *data.last_mut().unwrap() = index;
    let value = expr(ExprInstance::GByteArray(data));
    if nested {
        let map = EPathMap::new_map([(integer(7), value)], Vec::new(), false, None);
        payload(0x10, map.trie_snapshot())
    } else {
        let raw = value.with_sends(vec![Send {
            chan: Some(integer(7)),
            ..Send::default()
        }]);
        let key = encode_trie_path(&raw);
        assert_eq!(key[0], 0x0f);
        key
    }
}

fn joint_epm_fixture(map: bool, keys: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};

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

    let snapshot = if map {
        let mut trie = pathmap::PathMap::new();
        for key in keys {
            trie.insert(&key, integer(11));
        }
        epathmap_trie_codec::encode(&EPathMapRepr::Map(trie))
    } else {
        let mut trie = pathmap::PathMap::new();
        for key in keys {
            trie.insert(&key, ());
        }
        epathmap_trie_codec::encode(&EPathMapRepr::<Par>::Set(trie))
    };
    field(5, &field(32, &field(9, &snapshot)))
}

fn composed_joint_input(bytes: &[u8]) -> IndexedByteRegion<'_> {
    let mut root = super::super::canonical_source_tree::SourceTreeWindow::empty();
    for (index, byte) in bytes.chunks(1).enumerate() {
        let offset = if index % 17 == 0 {
            usize::MAX
        } else {
            101 - index % 97
        };
        let leaf = super::super::canonical_source_tree::SourceTreeWindow::from_borrowed(
            byte,
            (index % 7) as u32,
            offset,
        )
        .unwrap();
        root = root.concat(&leaf).unwrap();
    }
    IndexedByteRegion::from_source(root, CanonicalByteProvenance::new(99, 313))
}

fn assert_joint_epm_transport(
    input: IndexedByteRegion<'_>,
) -> (
    Result<CanonicalProtobufAdmission, CanonicalProtobufAdmissionError>,
    EpmBatchObservation,
) {
    let ((scalar, scalar_path), scalar_batches) = with_epm_transport(true, || {
        observe_full_admission(|| {
            validate_canonical_protobuf_inner(input.clone(), &PAR_PROTOBUF_REGION_PROGRAM, None)
        })
    });
    let ((batched, batched_path), batches) = with_epm_transport(false, || {
        observe_full_admission(|| {
            validate_canonical_protobuf_inner(input, &PAR_PROTOBUF_REGION_PROGRAM, None)
        })
    });
    assert_eq!(scalar_batches, EpmBatchObservation::default());
    assert_eq!(
        batched, scalar,
        "joint transport must retain every admission field and error"
    );
    assert_eq!(batched_path.edges, scalar_path.edges);
    assert_eq!(batched_path.states, scalar_path.states);
    assert_eq!(batched_path.frames, scalar_path.frames);
    assert_eq!(
        batched_path.repeated_owner_work,
        scalar_path.repeated_owner_work
    );
    assert_eq!(batched_path.observations, scalar_path.observations);
    (batched, batches)
}

fn assert_joint_epm_success(bytes: &[u8], width: usize) {
    let typed = super::super::protobuf_decoder::decode_par(bytes).unwrap();
    assert_eq!(protobuf_encoder::encode_to_vec(&typed), bytes);
    let mut score = Vec::new();
    typed.components().append_par_score_key(false, &mut score);
    for input in [
        IndexedByteRegion::from_borrowed(bytes, 7, 101),
        fragmented(bytes),
        composed_joint_input(bytes),
    ] {
        let (result, batches) = assert_joint_epm_transport(input);
        let admitted = result.unwrap();
        assert_eq!(admitted.score_bytes(), Some(score.as_slice()));
        assert_eq!(
            admitted.stats().canonical_path_obligations,
            admitted.stats().canonical_path_resolutions
        );
        assert!(admitted.stats().canonical_path_obligations > 0);
        assert_eq!(admitted.stats().reconstructed_key_bytes, 0);
        assert_eq!(admitted.stats().legacy_endpoint_replays, 0);
        assert_eq!(admitted.stats().typed_decodes, 0);
        assert!(
            batches.forward_batches > 0 && batches.unwind_batches > 0,
            "{batches:?}"
        );
        assert!(batches.forward_bytes >= width / 2, "{batches:?}");
        assert!(
            batches.max_forward > 1 && batches.max_unwind > 1,
            "{batches:?}"
        );
    }
}

#[test]
fn joint_epm_transport_scopes_restore_modes_and_evidence_after_panics() {
    let scratch = shared::rust::test_scratch::acquire("epm-transport-unwind-");
    let marker = scratch.path().join("restored");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rust::rholang::protobuf_admission::standalone_path_admission::joint_epm_transport_unwind_child",
            "--nocapture",
        ])
        .env("EPM_TRANSPORT_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the EPathMap transport child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(
                failure.kind,
                shared::rust::test_process::ProbeFailureKind::Panicked
            );
            assert!(failure.stderr.contains("scoped transport control"));
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the EPathMap transport child did not fail")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), b"inner+outer");
}

#[test]
#[ignore = "isolated EPathMap transport unwind child"]
fn joint_epm_transport_unwind_child() {
    let Some(marker) = std::env::var_os("EPM_TRANSPORT_UNWIND_MARKER") else {
        return;
    };
    struct VerifyOuter {
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOuter {
        fn drop(&mut self) {
            assert!(!scalar_epm_transport());
            let observed = EPM_BATCHES.with(|cell| cell.get().unwrap());
            assert_eq!(observed.forward_batches, 1);
            assert_eq!(observed.forward_bytes, 3);
            assert_eq!(observed.unwind_bytes, 0);
            std::fs::write(&self.marker, b"inner").unwrap();
        }
    }
    struct VerifyFinal {
        marker: std::path::PathBuf,
        mode: bool,
        evidence: Option<EpmBatchObservation>,
    }
    impl Drop for VerifyFinal {
        fn drop(&mut self) {
            assert_eq!(scalar_epm_transport(), self.mode);
            assert_eq!(EPM_BATCHES.with(std::cell::Cell::get), self.evidence);
            assert_eq!(std::fs::read(&self.marker).unwrap(), b"inner");
            std::fs::write(&self.marker, b"inner+outer").unwrap();
        }
    }
    let marker: std::path::PathBuf = marker.into();
    let original_mode = scalar_epm_transport();
    let original_evidence = EPM_BATCHES.with(std::cell::Cell::get);
    let _final = VerifyFinal {
        marker: marker.clone(),
        mode: original_mode,
        evidence: original_evidence,
    };
    with_epm_transport(false, || {
        record_epm_batch(CanonicalCompactTrieLineDirection::Forward, 3);
        let _outer = VerifyOuter {
            marker: marker.clone(),
        };
        with_epm_transport(true, || {
            assert!(scalar_epm_transport());
            record_epm_batch(CanonicalCompactTrieLineDirection::Unwind, 5);
            panic!("scoped transport control");
        });
    });
}

#[test]
fn joint_epm_transport_preserves_opaque_children_scores_and_all_source_backings() {
    for nested in [false, true] {
        for map in [false, true] {
            for width in [31, 257] {
                for count in [1, 3] {
                    let bytes = joint_epm_fixture(
                        map,
                        (0..count).map(|index| joint_epm_key(nested, width, index)),
                    );
                    assert_joint_epm_success(&bytes, width);
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/proptest-regressions/support/standalone_path_admission.txt"
            )),
        )),
        ..ProptestConfig::with_cases(64)
    })]
    #[test]
    fn joint_epm_transport_matches_scalar_for_arbitrary_opaque_widths_and_branches(
        nested in any::<bool>(), map in any::<bool>(), width in 16usize..193,
        count in 1u8..5,
    ) {
        let bytes = joint_epm_fixture(map,
            (0..count).map(|index| joint_epm_key(nested, width, index)));
        assert_joint_epm_success(&bytes, width);
    }
}

#[test]
fn joint_epm_transport_preserves_malformed_child_and_boundary_provenance() {
    let raw = joint_epm_key(false, 129, 1);
    let nested = joint_epm_key(true, 129, 1);
    let stable_body =
        protobuf_encoder::encode_to_vec(&expr(ExprInstance::GByteArray(vec![37; 129])));
    let raw_body = protobuf_encoder::encode_to_vec(
        &expr(ExprInstance::GByteArray(vec![37; 129])).with_sends(vec![Send {
            chan: Some(integer(7)),
            ..Send::default()
        }]),
    );
    let mut invalid_body = raw_body;
    invalid_body.push(0);
    let mut suffix = raw.clone();
    suffix.push(0xff);
    let mut invalid_nested = nested.clone();
    let mut at = 1;
    while invalid_nested[at] & 0x80 != 0 {
        at += 1;
    }
    invalid_nested[at + 1] ^= 0xff;
    let invalid_set_escape = EPathMap::from_set_iter(
        [expr(ExprInstance::GByteArray(vec![37; 129]))],
        Vec::new(),
        false,
        None,
    );
    for key in [
        raw[..raw.len() - 1].to_vec(),
        nested[..nested.len() - 1].to_vec(),
        payload(0x0f, &stable_body),
        payload(0x0f, &invalid_body),
        suffix,
        invalid_nested,
        payload(0x10, invalid_set_escape.trie_snapshot()),
    ] {
        for map in [false, true] {
            let bytes = joint_epm_fixture(map, [key.clone()]);
            for input in [
                IndexedByteRegion::from_borrowed(&bytes, 7, 101),
                fragmented(&bytes),
                composed_joint_input(&bytes),
            ] {
                let (result, batches) = assert_joint_epm_transport(input);
                assert!(
                    result.is_err(),
                    "malformed key unexpectedly admitted: {key:?}"
                );
                assert!(
                    batches.forward_batches > 0,
                    "failure fixture never exercises batching: {batches:?}"
                );
            }
        }
    }
}

struct ComponentStackFixture {
    program: &'static RhoMatchProgram,
    expected: CanonicalComponentAdmission,
    body: Vec<u8>,
    score: Vec<u8>,
}

fn component_stack_fixture<T: crate::rust::par_bag::ParBagComponent>(
    value: &T,
    program: &'static RhoMatchProgram,
) -> ComponentStackFixture {
    use crate::rust::par_bag::{with_component_key, PAR_BAG_KEY_DOMAIN};
    assert!(T::semantic_shape_is_complete(value));
    with_component_key(value, |key| {
        let trailer = key.len() - std::mem::size_of::<u64>();
        let score_len =
            usize::try_from(u64::from_be_bytes(key[trailer..].try_into().unwrap())).unwrap();
        let score_start = PAR_BAG_KEY_DOMAIN.len() + 1;
        let score_end = score_start + score_len;
        ComponentStackFixture {
            program,
            expected: CanonicalComponentAdmission {
                family: T::TAG,
                complete: true,
                canonical_path_stable: T::canonical_path_stable(value),
            },
            body: key[score_end..trailer].to_vec(),
            score: key[score_start..score_end].to_vec(),
        }
    })
}

fn component_stack_fixtures() -> Vec<ComponentStackFixture> {
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::cost_signature_atom::Form;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::{
        Bundle, Connective, ConnectiveBody, CostSignature, CostSignatureAtom, CostTokenStack,
        GPrivate, GUnforgeable, If, Match, MatchCase, New, Receive, ReceiveBind,
    };
    use crate::rust::rholang::match_schema_tables::{
        BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVE_RHO_MATCH_PROGRAM, COSTTOKENSTACK_RHO_MATCH_PROGRAM,
        EXPR_RHO_MATCH_PROGRAM, GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM,
        MATCH_RHO_MATCH_PROGRAM, NEW_RHO_MATCH_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM,
        SEND_RHO_MATCH_PROGRAM,
    };
    let empty = Par::default();
    let fixtures = vec![
        component_stack_fixture(
            &Send {
                chan: Some(empty.clone()),
                data: vec![integer(1)],
                persistent: true,
                connective_used: true,
                ..Send::default()
            },
            &SEND_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![integer(2)],
                    source: Some(empty.clone()),
                    ..ReceiveBind::default()
                }],
                body: Some(empty.clone()),
                condition: Some(integer(3)),
                persistent: true,
                peek: true,
                bind_count: 1,
                connective_used: true,
                ..Receive::default()
            },
            &RECEIVE_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &New {
                bind_count: 2,
                p: Some(empty.clone()),
                uri: vec!["rho:a".to_string(), "rho:z".to_string()],
                injections: std::collections::BTreeMap::from([("x".to_string(), integer(4))]),
                ..New::default()
            },
            &NEW_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &Expr {
                expr_instance: Some(ExprInstance::GInt(-7)),
            },
            &EXPR_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &Match {
                target: Some(empty.clone()),
                cases: vec![MatchCase {
                    pattern: Some(integer(5)),
                    source: Some(integer(6)),
                    guard: Some(integer(7)),
                    ..MatchCase::default()
                }],
                connective_used: true,
                ..Match::default()
            },
            &MATCH_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &Bundle {
                body: Some(empty.clone()),
                read_flag: true,
                write_flag: false,
            },
            &BUNDLE_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![integer(8), integer(9)],
                })),
            },
            &CONNECTIVE_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1, 2, 3] })),
            },
            &GUNFORGEABLE_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &If {
                condition: Some(integer(10)),
                if_true: Some(empty),
                if_false: Some(integer(11)),
                connective_used: true,
                ..If::default()
            },
            &IF_RHO_MATCH_PROGRAM,
        ),
        component_stack_fixture(
            &CostTokenStack {
                layers: vec![CostSignature {
                    atoms: vec![CostSignatureAtom {
                        form: Some(Form::Ground(b"cost".to_vec())),
                    }],
                }],
            },
            &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
        ),
    ];
    let families: Vec<_> = (0..=u8::MAX)
        .filter_map(ParComponentTag::from_byte)
        .collect();
    assert_eq!(fixtures.len(), families.len());
    for family in families {
        assert_eq!(
            fixtures
                .iter()
                .filter(|fixture| fixture.expected.family == family)
                .count(),
            1
        );
    }
    fixtures
}

#[test]
fn joint_epm_transport_preserves_large_opaque_payloads_on_an_unchanged_small_stack() {
    let fixtures =
        [false, true].map(|nested| joint_epm_fixture(true, [joint_epm_key(nested, 8192, 1)]));
    let components = component_stack_fixtures();
    std::thread::Builder::new()
        .name("joint-epm-opaque".to_owned())
        .stack_size(64 * 1024)
        .spawn(move || {
            for (kind, bytes) in ["RawParEscape", "NestedEpm1"].into_iter().zip(fixtures) {
                let (result, batches) =
                    assert_joint_epm_transport(IndexedByteRegion::from_borrowed(&bytes, 7, 101));
                result.unwrap();
                assert!(
                    batches.forward_bytes > 4096 && batches.unwind_bytes > 4096,
                    "{batches:?}"
                );
                eprintln!(
                    "64 KiB admission: kind={kind}, encoded_bytes={}, {batches:?}",
                    bytes.len()
                );
            }
            for fixture in components {
                let admit = || {
                    validate_canonical_protobuf_inner(
                        IndexedByteRegion::from_borrowed(&fixture.body, 7, 101),
                        fixture.program,
                        Some((fixture.expected.family, &fixture.score)),
                    )
                };
                let (scalar, scalar_batches) = with_epm_transport(true, admit);
                let (batched, batches) = with_epm_transport(false, admit);
                assert_eq!(scalar_batches, EpmBatchObservation::default());
                assert_eq!(batched, scalar);
                let admission = batched.unwrap();
                assert_eq!(admission.score_bytes(), Some(fixture.score.as_slice()));
                assert_eq!(admission.component(), Some(fixture.expected));
                eprintln!(
                    "64 KiB admission: kind={:?}, encoded_bytes={}, {batches:?}",
                    fixture.expected.family,
                    fixture.body.len()
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
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
    let mut reference = CanonicalPathStreamReducer::new();
    let mut successful = 0;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let located = CanonicalLocatedByte::new(byte, origin(index));
        let left = history.edge(&mut history_arena, checkpoint, located);
        let right = stream.edge(&input, located);
        assert_eq!(right, reference.reference_edge(&input, located));
        stream.assert_reference_equivalent(&reference);
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
                assert_eq!(
                    stream.edge(&input, located),
                    reference.reference_edge(&input, located),
                );
                stream.assert_reference_equivalent(&reference);
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
                reference
                    .reference_resolve(&input, right.obligation.id, resolution)
                    .unwrap();
                stream.assert_reference_equivalent(&reference);
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
            stream.value(true, endpoint),
            reference.value(true, endpoint)
        );
        assert_eq!(
            history.value(checkpoint, true, endpoint),
            stream.value(true, endpoint)
        );
        let history_stats = history.stats();
        assert!((1..=successful + 1).contains(&history_stats.states));
        assert_eq!(stream.stats().states, 1);
        assert!(history_stats.frames >= stream.stats().frames);
        stream.assert_frame_invariants();
        assert_eq!(history_stats.edges, stream.stats().edges);
        assert_consumed_prefix(&stream, &input, &history_arena, checkpoint);
    }
    let endpoint = CanonicalPathSource::in_source(99, 800);
    stream.assert_frame_invariants();
    stream.assert_reference_equivalent(&reference);
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

    #[test]
    fn focused_owner_graph_matches_eager_after_mutated_nested_steps(
        raw in prop::collection::vec(any::<u8>(), 0..48),
        depth in 0usize..17,
        mutation in any::<usize>(),
        replacement in any::<u8>(),
        raw_stable in any::<bool>(),
        entries_stable in any::<bool>(),
    ) {
        for mut key in [payload(0x05, &raw), payload(0x06, &raw), payload(0x0f, &raw), payload(0x10, &raw), raw] {
            for _ in 0..depth {
                let mut region = Vec::new();
                prost::encoding::encode_varint(key.len() as u64, &mut region);
                region.extend_from_slice(&key);
                key = payload(0x0d, &region);
            }
            let mut changed = key.clone();
            if !changed.is_empty() {
                let at = mutation % changed.len();
                changed[at] = replacement;
            }
            key.extend_from_slice(&[0xff, 0, 0xff]);
            changed.extend_from_slice(&[0xff, 0, 0xff]);
            for bytes in [&key, &changed] {
                for mode in [NestedEpmMode::Empty, NestedEpmMode::Set, NestedEpmMode::Map] {
                    assert_storage_trace(bytes, raw_stable, mode, entries_stable);
                }
            }
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
fn branching_nested_epm_resolves_every_obligation_and_validates_every_map_value() {
    use crate::rust::epathmap_trie_codec::{
        self, inspect_canonical_snapshot_value_regions, EPathMapRepr,
    };
    use crate::rust::rholang::protobuf_decoder;

    let corrupt_value = |snapshot: &mut [u8], index: usize| {
        let regions = inspect_canonical_snapshot_value_regions(snapshot).unwrap();
        let body = &regions.value_bodies[index];
        assert!(!body.is_empty());
        snapshot[body.start] = 0;
    };
    let build = |corrupt: Option<(usize, usize)>| {
        let mut keys = Vec::new();
        for branch in 0..3 {
            let leaf = EPathMap::new_map(
                [(integer(branch + 1), integer(branch + 101))],
                Vec::new(),
                false,
                None,
            );
            let mut leaf_snapshot = leaf.trie_snapshot().to_vec();
            if corrupt == Some((0, branch as usize)) {
                corrupt_value(&mut leaf_snapshot, 0);
            }
            let mut middle = pathmap::PathMap::new();
            middle.insert(&payload(0x10, &leaf_snapshot), integer(branch + 201));
            let mut middle_snapshot = epathmap_trie_codec::encode(&EPathMapRepr::Map(middle));
            if corrupt == Some((1, branch as usize)) {
                corrupt_value(&mut middle_snapshot, 0);
            }
            keys.push(payload(0x10, &middle_snapshot));
        }
        let mut bytes = joint_epm_fixture(true, keys.iter().cloned());
        if let Some((2, index)) = corrupt {
            let start = bytes.windows(4).position(|part| part == b"EPM1").unwrap();
            let regions = inspect_canonical_snapshot_value_regions(&bytes[start..]).unwrap();
            assert_eq!(regions.value_bodies.len(), 3);
            bytes[start + regions.value_bodies[index].start] = 0;
        }
        (bytes, keys)
    };

    let (bytes, keys) = build(None);
    assert!(keys.iter().all(|key| validate_canonical_path(key).is_ok()));
    let typed = protobuf_decoder::decode_par(bytes.as_slice()).unwrap();
    assert_eq!(protobuf_encoder::encode_to_vec(&typed), bytes);
    let mut expected_score = Vec::new();
    typed
        .components()
        .append_par_score_key(false, &mut expected_score);
    for input in [
        IndexedByteRegion::from_borrowed(&bytes, 7, 101),
        fragmented(&bytes),
        composed_joint_input(&bytes),
    ] {
        let (result, _) = assert_joint_epm_transport(input);
        let admitted = result.unwrap();
        assert_eq!(admitted.score_bytes(), Some(expected_score.as_slice()));
        assert_eq!(admitted.stats().epathmaps, 7);
        assert_eq!(admitted.stats().canonical_path_obligations, 6);
        assert_eq!(admitted.stats().canonical_path_resolutions, 6);
        assert!(admitted.stats().max_frames >= 5);
        assert_eq!(admitted.stats().typed_decodes, 0);
    }

    for layer in 0..3 {
        for branch in 0..3 {
            let (bad, keys) = build(Some((layer, branch)));
            assert_eq!(
                keys.iter()
                    .filter(|key| validate_canonical_path(key).is_err())
                    .count(),
                usize::from(layer < 2)
            );
            assert!(protobuf_decoder::decode_par(bad.as_slice()).is_err());
            for input in [
                IndexedByteRegion::from_borrowed(&bad, 7, 101),
                fragmented(&bad),
            ] {
                let (result, _) = assert_joint_epm_transport(input);
                assert!(result.is_err(), "layer={layer} branch={branch}");
            }
        }
    }
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
            FrameRequest::Message { region, .. } | FrameRequest::EPathMap { region, .. } => region,
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
        admit_detached_path_child(child).unwrap();
    }
}

#[test]
fn opaque_batching_keeps_enclosing_truncation_before_child_dispatch() {
    let key = [0x0d, 6, 5, 0x10, 4, 0xaa, 0xbb, 0xcc, 0xdd];
    for input in [
        IndexedByteRegion::from_borrowed(&key, 7, 101),
        fragmented(&key),
    ] {
        let expected = input.position_at(7).unwrap().physical;
        let mut parent = CanonicalPathFrame::new(input);
        let mut stats = CanonicalProtobufAdmissionStats::default();
        let error = match parent.step(&mut stats) {
            Err(error) => error,
            Ok(_) => panic!("truncated owner cannot dispatch its child"),
        };
        match error {
            CanonicalProtobufAdmissionError::CanonicalPath {
                position, detail, ..
            } => {
                assert_eq!(
                    position,
                    CanonicalPathSource::in_source(expected.source, expected.offset)
                );
                assert!(detail.contains("Truncated"));
            }
            other => panic!("enclosing truncation must precede child validation: {other}"),
        }
        assert_eq!(stats.canonical_path_obligations, 0);
        assert!(parent.waiting.is_none());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn checked_opaque_cursor_skips_preserve_next_bytes_and_refuse_without_mutation(
        bytes in prop::collection::vec(any::<u8>(), 0..256),
        initial in any::<usize>(), requested in any::<usize>(),
    ) {
        let initial = initial % (bytes.len() + 1);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (i, byte) in bytes.iter().copied().enumerate() {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, origin(i))).unwrap();
        }
        for input in [
            IndexedByteRegion::from_borrowed(&bytes, 7, 101),
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::new(bytes.clone().into()), 9),
            IndexedByteRegion::from_arena(&arena, handle, origin(bytes.len())),
            fragmented(&bytes),
        ] {
            let expected = input.located_bytes().collect::<Vec<_>>();
            for width in [0, requested % (bytes.len() + 1), bytes.len() - initial, usize::MAX] {
                let mut cursor = input.located_bytes();
                for expected_byte in &expected[..initial] {
                    prop_assert_eq!(cursor.next(), Some(*expected_byte));
                }
                let before_traversed = cursor.bytes_traversed();
                let should_accept = width <= bytes.len() - initial;
                prop_assert_eq!(cursor.skip_exact(width), should_accept);
                let at = if should_accept { initial + width } else { initial };
                prop_assert_eq!(cursor.bytes_yielded(), at);
                if !should_accept {
                    prop_assert_eq!(cursor.bytes_traversed(), before_traversed);
                }
                prop_assert_eq!(cursor.collect::<Vec<_>>(), expected[at..].to_vec());
            }
        }
    }
}

#[test]
fn opaque_batching_seeks_without_traversing_payload_interiors() {
    for width in [0, 1, 2, 16, 128, 8192] {
        let raw = expr(ExprInstance::GByteArray(vec![37; width])).with_sends(vec![Send {
            chan: Some(integer(7)),
            ..Send::default()
        }]);
        let body = protobuf_encoder::encode_to_vec(&raw);
        let mut key = payload(0x0f, &body);
        let consumed = key.len();
        key.push(0xff);
        let header = consumed - body.len();
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (i, byte) in key.iter().copied().enumerate() {
            handle = arena
                .append(handle, CanonicalLocatedByte::new(byte, origin(i)))
                .unwrap();
        }
        for input in [
            IndexedByteRegion::from_borrowed(&key, 7, 101),
            fragmented(&key),
        ] {
            let anchor = input.position_at(consumed - 1).unwrap().physical;
            let mut parent = CanonicalPathFrame::new(input);
            let mut stats = CanonicalProtobufAdmissionStats::default();
            let FrameAction::Push(child) = parent.step(&mut stats).unwrap() else {
                panic!("raw payload must suspend for child validation");
            };
            assert_eq!(parent.cursor.bytes_yielded(), consumed);
            assert_eq!(parent.cursor.bytes_traversed(), header + 1);
            assert_eq!(parent.reducer.stats().bulk_operations, 1);
            assert_eq!(parent.reducer.stats().bulk_bytes, body.len() - 1);
            let FrameRequest::Message { ref region, .. } = child else {
                panic!("expected RawPar child")
            };
            assert_eq!(region.end_position().physical, anchor);
            let (summary, _, _) = drive_admission(child, 0).unwrap();
            parent.accept(summary, &mut stats).unwrap();
            assert!(parent.step(&mut stats).is_err());
        }
        let borrowed_arena = IndexedByteRegion::from_arena(&arena, handle, origin(key.len()));
        let mut parent = CanonicalPathFrame::new(borrowed_arena);
        let mut stats = CanonicalProtobufAdmissionStats::default();
        assert!(matches!(
            parent.step(&mut stats).unwrap(),
            FrameAction::Push(_)
        ));
        assert_eq!(parent.cursor.bytes_yielded(), consumed);
        assert_eq!(parent.cursor.bytes_traversed(), consumed);
    }
}

#[test]
fn opaque_payload_cursor_resource_matrix_preserves_completion() {
    for width in [16, 128, 1024, 8192] {
        let map = EPathMap::new_map(
            [(integer(1), expr(ExprInstance::GByteArray(vec![37; width])))],
            Vec::new(),
            false,
            None,
        );
        for depth in [0, 1, 8, 32] {
            let mut key = payload(0x10, map.trie_snapshot());
            let mut expected_scalar_descents = 0;
            for _ in 0..depth {
                expected_scalar_descents += key.len();
                let mut region = Vec::new();
                prost::encoding::encode_varint(key.len() as u64, &mut region);
                region.extend_from_slice(&key);
                key = payload(0x0d, &region);
            }
            let completed_at = key.len();
            key.push(0xff);
            for (backing, input) in [
                ("borrowed", IndexedByteRegion::from_borrowed(&key, 7, 101)),
                ("frozen", fragmented(&key)),
            ] {
                let expected_anchor = input.position_at(completed_at - 1).unwrap().physical;
                let mut parent = CanonicalPathFrame::new(input);
                let mut stats = CanonicalProtobufAdmissionStats::default();
                let FrameAction::Push(request) = parent.step(&mut stats).unwrap() else {
                    panic!("nested map payload must suspend before the suffix");
                };
                let mut children = Vec::new();
                let mut constructor_stats = CanonicalProtobufAdmissionStats::default();
                push_admission_frame(&mut children, request, &mut constructor_stats).unwrap();
                assert_eq!(children.len(), 1);
                assert_eq!(constructor_stats.epathmaps, 1);
                let Frame::EPathMap(child) = &children[0] else {
                    panic!("nested map request must construct an EPathMap child");
                };
                let region = &child.snapshot;
                assert_eq!(region.len(), map.trie_snapshot().len());
                assert_eq!(region.end_position().physical, expected_anchor);
                assert_eq!(parent.reducer.consumed_len(), completed_at);
                assert_eq!(parent.cursor.bytes_yielded(), completed_at);
                assert!(parent.cursor.bytes_traversed() <= completed_at);
                let skipped = map.trie_snapshot().len() - 1;
                assert_eq!(parent.cursor.bytes_traversed(), completed_at - skipped);
                assert_eq!(parent.reducer.stats().bulk_operations, 1);
                assert_eq!(parent.reducer.stats().bulk_bytes, skipped);
                let mut scalar = CanonicalPathStreamReducer::new();
                for byte in parent.input.located_bytes().take(completed_at) {
                    scalar.reference_edge(&parent.input, byte).unwrap();
                }
                assert_eq!(
                    scalar.active_obligation(),
                    parent.reducer.active_obligation()
                );
                assert_eq!(
                    scalar.completed_obligation(&parent.input),
                    parent.reducer.completed_obligation(&parent.input)
                );
                assert_eq!(scalar.stats().edges, parent.reducer.stats().edges);
                assert_eq!(scalar.stats().region_descents, expected_scalar_descents);
                assert_eq!(parent.reducer.stats().region_descents, 0);
                assert_eq!(stats.canonical_path_obligations, 1);
                let before = parent.reducer.stats();
                assert!(parent.step(&mut stats).is_err());
                assert_eq!(parent.reducer.stats(), before);
                assert_eq!(parent.cursor.bytes_yielded(), completed_at);
                println!(
                    "opaque payload {backing}: width={width}, depth={depth}, payload={}, consumed={}, traversed={}, frame_backing={}, batches={}, skipped={}, region_descents={}, scalar_region_descents={}",
                    region.len(),
                    completed_at,
                    parent.cursor.bytes_traversed(),
                    parent.reducer.retained_frame_bytes(),
                    parent.reducer.stats().bulk_operations,
                    parent.reducer.stats().bulk_bytes,
                    parent.reducer.stats().region_descents,
                    scalar.stats().region_descents,
                );
            }
        }
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
        println!(
            "canonical-key state storage: width={width}, persistent_bytes={}, exclusive_inline_bytes={}",
            history.retained_state_bytes(),
            frame.reducer.retained_state_bytes()
        );
        println!(
            "canonical-key frame storage: width={width}, persistent_bytes={}, exclusive_bytes={}, exclusive_slots={}",
            history.retained_frame_bytes(),
            retained,
            reducer.frames
        );
        println!(
            "canonical-key persistent reference: width={width}, bytes={}, states={}, frames={}, byte_pages={}, provenance_pages={}, payload_slots={}",
            key.len(),
            history.stats().states,
            history.stats().frames,
            arena.byte_pages,
            arena.provenance_pages,
            arena.payload_slots
        );
        println!(
            "canonical-key indexed transport: width={width}, consumed={}, borrowed_input_bytes={}, bytes_read={}, bytes_traversed={}",
            frame.reducer.consumed_len(),
            backing.len(),
            stats.canonical_path_bytes_read,
            stats.canonical_path_bytes_traversed
        );
    }
}

#[test]
fn focused_owner_opening_byte_is_not_charged_to_the_new_child() {
    let mut key = vec![0x03, 0];
    for depth in 1..=32 {
        let mut region = Vec::new();
        prost::encoding::encode_varint(key.len() as u64, &mut region);
        region.extend_from_slice(&key);
        key = payload(0x0d, &region);
        if depth == 1 {
            assert_eq!(key, [0x0d, 3, 2, 0x03, 0]);
        }
        assert!(admit(&key).unwrap().0.canonical_path_stable);
        assert_differential(&key);
        assert_storage_trace(&key, false, NestedEpmMode::Map, true);
    }
}

#[test]
fn focused_owner_outer_expiry_keeps_shallow_error_precedence_and_provenance() {
    for (tag, expected) in [
        (0x03, CanonicalPathAdmissionErrorKind::Truncated),
        (0xff, CanonicalPathAdmissionErrorKind::ReservedTag(0xff)),
    ] {
        let key = [0x0d, 5, 4, 0x0d, 5, 4, tag, 0, 0xff, 0xff];
        for input in [
            IndexedByteRegion::from_borrowed(&key, 7, 101),
            fragmented(&key),
        ] {
            let mut reducer = CanonicalPathStreamReducer::new();
            let error_source = input.position_at(6).unwrap().physical;
            for (index, byte) in input.located_bytes().enumerate() {
                reducer.edge(&input, byte).unwrap();
                reducer.assert_frame_invariants();
                if index >= 6 {
                    let error = reducer
                        .value(true, CanonicalPathSource::in_source(99, 811))
                        .unwrap()
                        .unwrap_err();
                    assert_eq!(error.kind, expected);
                    assert_eq!(error.source.source, error_source.source);
                    assert_eq!(error.source.offset, error_source.offset);
                    assert!(reducer.active_obligation().is_none());
                    assert!(reducer.completed_obligation(&input).unwrap().is_none());
                }
            }
            assert_eq!(reducer.consumed_len(), key.len());
        }
        assert_storage_trace(&key, false, NestedEpmMode::Map, true);
    }
}

#[test]
fn focused_owner_duplicate_refusal_follows_nested_completion_and_pending_resolution() {
    let map = EPathMap::new_map([(integer(1), integer(2))], Vec::new(), false, None);
    for nested_map in [false, true] {
        for depth in [1, 2, 8, 32] {
            let mut child = if nested_map {
                payload(0x10, map.trie_snapshot())
            } else {
                vec![0x03, 0]
            };
            for _ in 0..depth {
                let mut region = Vec::new();
                prost::encoding::encode_varint(child.len() as u64, &mut region);
                region.extend_from_slice(&child);
                child = payload(0x0d, &region);
            }
            let mut region = Vec::new();
            for _ in 0..2 {
                prost::encoding::encode_varint(child.len() as u64, &mut region);
                region.extend_from_slice(&child);
            }
            let mut key = payload(0x0d, &region);
            let boundary = key.len() - 1;
            key.extend_from_slice(&[0xff, 0xff]);
            for input in [
                IndexedByteRegion::from_borrowed(&key, 7, 101),
                fragmented(&key),
            ] {
                let mut reducer = CanonicalPathStreamReducer::new();
                let mut resolutions = 0;
                let mut pending_id = None;
                for (index, byte) in input.located_bytes().enumerate() {
                    reducer.edge(&input, byte).unwrap();
                    if let Some(completed) = reducer.completed_obligation(&input).unwrap() {
                        let payload = input
                            .subview_with_end(
                                completed.payload,
                                CanonicalByteProvenance::new(
                                    completed.payload_end.source,
                                    completed.payload_end.offset,
                                ),
                            )
                            .unwrap();
                        let (waiting, child) = obligation_region_frame(
                            completed.obligation,
                            payload,
                            &PAR_PROTOBUF_REGION_PROGRAM,
                            0,
                            &mut CanonicalProtobufAdmissionStats::default(),
                        )
                        .unwrap();
                        let summary = admit_detached_path_child(child).unwrap();
                        let resolution = waiting.resolution(&summary).unwrap();
                        reducer
                            .resolve_obligation(&input, completed.obligation.id, resolution)
                            .unwrap();
                        resolutions += 1;
                        pending_id = Some(completed.obligation.id);
                    }
                    if index >= boundary {
                        let error = reducer
                            .value(true, CanonicalPathSource::new(0))
                            .unwrap()
                            .unwrap_err();
                        assert_eq!(
                            error.kind,
                            CanonicalPathAdmissionErrorKind::DuplicatePathInRegion
                        );
                        let expected = input.position_at(boundary).unwrap().physical;
                        assert_eq!(error.source.source, expected.source);
                        assert_eq!(error.source.offset, expected.offset);
                    }
                    reducer.assert_frame_invariants();
                }
                assert_eq!(resolutions, if nested_map { 2 } else { 0 });
                if let Some(id) = pending_id {
                    reducer
                        .resolve_obligation(
                            &input,
                            id,
                            CanonicalPathObligationResolution::NestedEpm1 {
                                mode: NestedEpmMode::Set,
                                entries_stable: true,
                            },
                        )
                        .unwrap();
                    let error = reducer
                        .value(true, CanonicalPathSource::new(0))
                        .unwrap()
                        .unwrap_err();
                    assert_eq!(error.kind, CanonicalPathAdmissionErrorKind::NestedEpmNotMap);
                    reducer.assert_frame_invariants();
                }
            }
            assert_storage_trace(&key, false, NestedEpmMode::Map, true);
        }
    }
}

#[test]
fn ordinary_region_work_matrix_tracks_all_owner_walks() {
    let map = EPathMap::new_map([(integer(1), integer(2))], Vec::new(), false, None);
    let map_key = payload(0x10, map.trie_snapshot());
    for width in [1, 16, 128, 1024] {
        for depth in [0, 1, 8, 32] {
            for workload in ["bytes", "children"] {
                let mut key = if workload == "bytes" {
                    encode_trie_path(&expr(ExprInstance::GByteArray(vec![37; width])))
                } else {
                    let mut tuple = vec![0x0c];
                    prost::encoding::encode_varint(width as u64, &mut tuple);
                    for _ in 0..width {
                        tuple.extend_from_slice(&map_key);
                    }
                    tuple
                };
                let mut eager_owner_steps = 0;
                for _ in 0..depth {
                    eager_owner_steps += key.len();
                    let mut region = Vec::new();
                    prost::encoding::encode_varint(key.len() as u64, &mut region);
                    region.extend_from_slice(&key);
                    key = payload(0x0d, &region);
                }
                let mut first_observation = None;
                for (backing, input) in [
                    ("borrowed", IndexedByteRegion::from_borrowed(&key, 7, 101)),
                    ("frozen", fragmented(&key)),
                ] {
                    let mut parent = CanonicalPathFrame::new(input);
                    let mut stats = CanonicalProtobufAdmissionStats::default();
                    let result = loop {
                        match parent.step(&mut stats).unwrap() {
                            FrameAction::Complete(FrameSummary::CanonicalPath(summary)) => {
                                break summary;
                            }
                            FrameAction::Push(child) => {
                                let summary = admit_detached_path_child(child).unwrap();
                                parent.accept(summary, &mut stats).unwrap();
                            }
                            _ => panic!("expected one canonical-key result"),
                        }
                    };
                    let work = parent.reducer.owner_work();
                    let reducer = parent.reducer.stats();
                    assert!(result.canonical_path_stable);
                    assert_eq!(parent.reducer.consumed_len(), key.len());
                    assert_eq!(reducer.edges, key.len());
                    assert_eq!(parent.cursor.bytes_yielded(), key.len());
                    assert_eq!(
                        stats.canonical_path_obligations,
                        if workload == "children" { width } else { 0 }
                    );
                    assert_eq!(
                        stats.canonical_path_resolutions,
                        stats.canonical_path_obligations
                    );
                    if depth == 0 {
                        assert_eq!(
                            work.active_queries
                                + work.completed_queries
                                + work.plans
                                + work.resolutions
                                + work.rebuilds
                                + reducer.region_descents,
                            0
                        );
                    }
                    if workload == "bytes" {
                        assert_eq!(work.plans, 0);
                        assert_eq!(work.resolutions, 0);
                        assert_eq!(reducer.bulk_bytes, 0);
                        assert_eq!(parent.cursor.bytes_traversed(), key.len());
                        let mut reference = CanonicalPathStreamReducer::new();
                        for byte in parent.input.located_bytes() {
                            reference.reference_edge(&parent.input, byte).unwrap();
                            assert!(reference
                                .completed_obligation(&parent.input)
                                .unwrap()
                                .is_none());
                        }
                        assert_eq!(reference.owner_work().active_queries, eager_owner_steps);
                        assert_eq!(reference.owner_work().completed_queries, eager_owner_steps);
                        assert_eq!(reference.stats().region_descents, eager_owner_steps);
                        assert_eq!(reference.owner_work().rebuilds, eager_owner_steps);
                        assert_eq!(
                            reference.value(true, CanonicalPathSource::new(key.len())),
                            Some(Ok(result))
                        );
                    }
                    assert_eq!(work.active_queries, depth);
                    assert_eq!(work.completed_queries, 0);
                    assert_eq!(work.plans, 0);
                    assert_eq!(work.resolutions, 0);
                    assert_eq!(reducer.region_descents, 0);
                    assert_eq!(work.rebuilds, 0);
                    assert_eq!(work.reifications, 0);
                    assert_eq!(work.detachments, depth);
                    assert!(
                        work.settlements
                            <= parent.cursor.bytes_traversed()
                                + stats.canonical_path_resolutions
                                + depth
                    );
                    parent.reducer.assert_frame_invariants();
                    let observation = (
                        work,
                        reducer,
                        parent.cursor.bytes_traversed(),
                        parent.reducer.retained_frame_bytes(),
                    );
                    if let Some(expected) = first_observation {
                        assert_eq!(observation, expected);
                    } else {
                        first_observation = Some(observation);
                    }
                    println!(
                        "ordinary region {workload}/{backing}: width={width}, depth={depth}, encoded={}, visits={}, active_queries={}, completed_queries={}, plans={}, resolutions={}, feed_descents={}, rebuilds={}, detachments={}, reifications={}, settlements={}, state_backing={}, frame_backing={}",
                        key.len(),
                        parent.cursor.bytes_traversed(),
                        work.active_queries,
                        work.completed_queries,
                        work.plans,
                        work.resolutions,
                        reducer.region_descents,
                        work.rebuilds,
                        work.detachments,
                        work.reifications,
                        work.settlements,
                        parent.reducer.retained_state_bytes(),
                        parent.reducer.retained_frame_bytes(),
                    );
                }
            }
        }
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

            let depth = 4_096;
            let mut key = vec![0x03, 0];
            for _ in 0..depth {
                let mut region = Vec::new();
                prost::encoding::encode_varint(key.len() as u64, &mut region);
                region.extend_from_slice(&key);
                key = payload(0x0d, &region);
            }
            for input in [
                IndexedByteRegion::from_borrowed(&key, 0, 0),
                fragmented(&key),
            ] {
                let mut reducer = CanonicalPathStreamReducer::new();
                for byte in input.located_bytes() {
                    reducer.edge(&input, byte).unwrap();
                }
                assert!(
                    reducer
                        .value(true, CanonicalPathSource::new(key.len()))
                        .unwrap()
                        .unwrap()
                        .canonical_path_stable
                );
                assert_eq!(reducer.owner_work().detachments, depth);
                assert_eq!(reducer.owner_work().reifications, 0);
                assert_eq!(reducer.stats().region_descents, 0);
                reducer.assert_frame_invariants();
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
