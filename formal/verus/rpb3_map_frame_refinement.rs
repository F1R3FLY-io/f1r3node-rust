use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
mod rpb3_admission_cursor;
#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor;
#[path = "rpb3_generated_descriptor_projection.rs"]
mod rpb3_generated_descriptor_projection;
#[cfg(not(rpb3_integrated))]
mod rpb3_score_transcript;
use {
    rpb3_admission_cursor as admission, rpb3_generated_descriptor_projection as generated,
    rpb3_score_transcript as score,
};

#[cfg(rpb3_integrated)]
use crate::rpb3_score_transcript;

verus! {

pub struct MapValuePending {
    pub id: u64,
    pub field_position: int,
    pub ghost field_source: usize,
    pub payload_start: usize,
    pub payload_end: usize,
    pub child_type_id: u16,
}

pub struct MapValueSummary {
    pub obligation_id: u64,
    pub score_handle: usize,
    pub score_origin_body_id: u64,
    pub child_body_id: u64,
    pub semantic_default: bool,
    pub canonical_path_stable: bool,
}

pub struct MapSummaryModel {
    pub ghost key_bytes: Seq<u8>,
    pub key_position: int,
    pub ghost key_source: usize,
    pub value: Option<MapValueSummary>,
}

pub struct MapFrameModel {
    pub owner_type_id: u16,
    pub outer_field: u32,
    pub value_type_id: u16,
    pub physical_start: usize,
    pub ghost physical_source: usize,
    pub region_end: usize,
    pub ghost source_bytes: Seq<u8>,
    pub ghost source_image: Option<admission::AdmissionIndexedSourceImage>,
    pub cursor: usize,
    pub last_tag: u32,
    pub ghost key: Option<Seq<u8>>,
    pub ghost key_field: Option<admission::IndexedCanonicalFieldModel>,
    pub key_position: Option<int>,
    pub ghost key_source: Option<usize>,
    pub saw_value: bool,
    pub ghost value_field: Option<admission::IndexedCanonicalFieldModel>,
    pub value_position: Option<int>,
    pub ghost value_source: Option<usize>,
    pub pending: Option<MapValuePending>,
    pub value: Option<MapValueSummary>,
    pub ghost accepted_child: Option<admission::AdmissionFrameState>,
    pub ghost accepted_arena: Seq<admission::AdmissionScoreBinding>,
}

pub open spec fn map_source_position(frame: MapFrameModel, at: usize) -> Option<int> {
    match frame.source_image {
        None => Some(frame.physical_start as int + at as int),
        Some(image) => match admission::admission_indexed_source_position(image, at) {
            None => None,
            Some(anchor) => Some(anchor.offset as int),
        },
    }
}

pub open spec fn map_source_id(frame: MapFrameModel, at: usize) -> Option<usize> {
    match frame.source_image {
        None => Some(frame.physical_source),
        Some(image) => match admission::admission_indexed_source_position(image, at) {
            None => None,
            Some(anchor) => Some(anchor.source),
        },
    }
}

pub open spec fn map_schema_valid(frame: MapFrameModel) -> bool {
    frame.owner_type_id == 20
        && frame.outer_field == 4
        && frame.value_type_id == 1
        && match generated::generated_descriptor(
            frame.owner_type_id, frame.outer_field,
        ) {
            None => false,
            Some(descriptor) => descriptor.slot == 3
                && descriptor.repeated
                && descriptor.wire == 2
                && descriptor.frame_kind == 2
                && descriptor.child_type_id == Some(frame.value_type_id),
        }
}

pub open spec fn map_frame_wf(frame: MapFrameModel) -> bool {
    &&& map_schema_valid(frame)
    &&& frame.source_bytes.len() == frame.region_end as int
    &&& match frame.source_image {
        None => frame.physical_start as int + frame.region_end as int <= usize::MAX as int,
        Some(image) => admission::admission_indexed_source_wf(image)
            && image.bytes == frame.source_bytes
            && admission::admission_indexed_source_position(image, 0).is_some()
            && admission::admission_indexed_source_position(image, 0)
                .unwrap().source == frame.physical_source
            && admission::admission_indexed_source_position(image, 0)
                .unwrap().offset == frame.physical_start,
    }
    &&& frame.cursor <= frame.region_end
    &&& frame.last_tag <= 2
    &&& (frame.last_tag == 0 ==> frame.cursor == 0
        && frame.key.is_none() && !frame.saw_value)
    &&& (frame.last_tag == 1 ==> frame.key.is_some() && !frame.saw_value)
    &&& (frame.last_tag == 2 ==> frame.saw_value)
    &&& frame.key.is_some() == frame.key_field.is_some()
    &&& frame.key.is_some() == frame.key_position.is_some()
    &&& frame.key.is_some() == frame.key_source.is_some()
    &&& (frame.key.is_none() || frame.last_tag >= 1)
    &&& (match frame.key {
        None => true,
        Some(bytes) => bytes.len() > 0 && admission::utf8_valid_at(bytes, 0),
    })
    &&& (match frame.key_field {
        None => true,
        Some(field) => {
            &&& field.number == 1
            &&& field.wire == 2
            &&& field.encoded_start == 0
            &&& field.encoded_end <= frame.cursor
            &&& (frame.value_field.is_some() || field.encoded_end == frame.cursor)
            &&& admission::indexed_field_at_cursor(
                admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: frame.region_end,
                    next: field.encoded_start,
                    source_bytes: frame.source_bytes,
                }, field,
            )
            &&& frame.key == Some(field.payload_bytes)
            &&& frame.key_position == map_source_position(frame, field.encoded_start)
            &&& frame.key_source == map_source_id(frame, field.encoded_start)
        },
    })
    &&& frame.saw_value == (frame.pending.is_some() || frame.value.is_some())
    &&& frame.saw_value == frame.value_field.is_some()
    &&& frame.saw_value == frame.value_position.is_some()
    &&& frame.saw_value == frame.value_source.is_some()
    &&& (!frame.saw_value || frame.last_tag == 2)
    &&& (match frame.value_field {
        None => true,
        Some(field) => {
            &&& field.number == 2
            &&& field.wire == 2
            &&& field.encoded_end == frame.cursor
            &&& admission::indexed_field_at_cursor(
                admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: frame.region_end,
                    next: field.encoded_start,
                    source_bytes: frame.source_bytes,
                }, field,
            )
            &&& frame.value_position == map_source_position(frame, field.encoded_start)
            &&& frame.value_source == map_source_id(frame, field.encoded_start)
            &&& (match frame.key_field {
                None => field.encoded_start == 0,
                Some(key_field) => key_field.encoded_end == field.encoded_start,
            })
        },
    })
    &&& !(frame.pending.is_some() && frame.value.is_some())
    &&& frame.value.is_some() == frame.accepted_child.is_some()
    &&& (frame.value.is_some() || frame.accepted_arena.len() == 0)
    &&& (match frame.pending {
        None => true,
        Some(wait) => wait.id < u64::MAX
            && wait.child_type_id == frame.value_type_id
            && wait.payload_start <= wait.payload_end <= frame.region_end
            && match frame.value_field {
                None => false,
                Some(field) => wait.payload_start == field.payload_start
                    && wait.payload_end == field.payload_end
                    && Some(wait.field_position)
                        == map_source_position(frame, field.encoded_start)
                    && Some(wait.field_source)
                        == map_source_id(frame, field.encoded_start),
            },
    })
    &&& (match frame.value {
        None => true,
        Some(value) => !value.semantic_default
            && match (frame.value_field, frame.accepted_child) {
                (Some(field), Some(child)) => {
                    &&& child.frame_kind == 1
                    &&& child.family == admission::AdmissionFamily::Other
                    &&& child.program_type_id == frame.value_type_id
                    &&& child.return_id == Some(value.obligation_id)
                    &&& Some(child.physical_start as int)
                        == map_source_position(frame, field.payload_start)
                    &&& Some(child.physical_source)
                        == map_source_id(frame, field.payload_start)
                    &&& child.region_start == 0
                    &&& child.region_end as int
                        == field.payload_end as int - field.payload_start as int
                    &&& child.source_bytes == field.payload_bytes
                    &&& child.cursor == child.region_end
                    &&& child.waiting_id.is_none()
                    &&& child.nested_started == child.nested_resolved
                    &&& admission::admission_frame_wf(child)
                    &&& admission::admission_slots_complete(child)
                    &&& admission::admission_family_admissible(child)
                    &&& !admission::admission_raw_par_semantic_default(child)
                    &&& admission::admission_child_summary_agrees(
                        child,
                        admission::AdmissionReturnedKind::Par {
                            semantic_default: false,
                            canonical_path_stable: value.canonical_path_stable,
                        }, frame.accepted_arena,
                    )
                    &&& child.score_handle == Some(value.score_handle)
                    &&& child.body_id == value.child_body_id
                    &&& frame.accepted_arena[value.score_handle as int].body_id
                        == value.score_origin_body_id
                },
                _ => false,
            },
    })
}

pub open spec fn map_aux_projection(
    frame: MapFrameModel, auxiliary: admission::AdmissionFrameState,
) -> bool {
    &&& auxiliary.frame_kind == 2
    &&& auxiliary.family == admission::AdmissionFamily::Other
    &&& auxiliary.program_type_id == frame.value_type_id
    &&& auxiliary.slot_count == 2
    &&& auxiliary.physical_start == frame.physical_start
    &&& auxiliary.physical_source == frame.physical_source
    &&& auxiliary.region_start == 0
    &&& auxiliary.region_end == frame.region_end
    &&& auxiliary.source_bytes == frame.source_bytes
    &&& auxiliary.cursor == frame.cursor
    &&& auxiliary.last_slot == (if frame.last_tag == 0 { None } else {
        Some((frame.last_tag - 1) as usize)
    })
    &&& (forall|slot: int| auxiliary.seen.contains(slot)
        == ((slot == 0 && frame.key.is_some()) || (slot == 1 && frame.saw_value)))
    &&& (forall|slot: int| auxiliary.scalar_slots.contains(slot)
        == (slot == 0 && frame.key.is_some()))
    &&& (forall|slot: int| !auxiliary.required.contains(slot))
    &&& auxiliary.scalar_occurrences.len() == 0
    &&& auxiliary.score_handle.is_none()
    &&& auxiliary.nested_started == 0
    &&& auxiliary.nested_resolved == 0
    &&& (match frame.value {
        None => auxiliary.returns.len() == 0,
        Some(value) => auxiliary.returns.len() == 1
            && auxiliary.returns[0].slot == 1
            && auxiliary.returns[0].tag == 2
            && auxiliary.returns[0].body_id == value.child_body_id
            && auxiliary.returns[0].score_handle == Some(value.score_handle)
            && auxiliary.returns[0].score_origin_body_id
                == Some(value.score_origin_body_id)
            && auxiliary.returns[0].kind
                == admission::AdmissionReturnedKind::Par {
                    semantic_default: false,
                    canonical_path_stable: value.canonical_path_stable,
                },
    })
    &&& (match frame.pending {
        None => auxiliary.waiting_id.is_none()
            && auxiliary.waiting_kind == 0
            && auxiliary.waiting_slot.is_none()
            && auxiliary.waiting_tag.is_none(),
        Some(wait) => auxiliary.waiting_id == Some(wait.id)
            && auxiliary.waiting_kind == 1
            && auxiliary.waiting_slot == Some(1)
            && auxiliary.waiting_tag == Some(2),
    })
}

pub open spec fn map_initial(
    source_bytes: Seq<u8>, physical_start: usize,
) -> MapFrameModel {
    MapFrameModel {
        owner_type_id: 20,
        outer_field: 4,
        value_type_id: 1,
        physical_start,
        physical_source: 0,
        region_end: source_bytes.len() as usize,
        source_bytes,
        source_image: None,
        cursor: 0,
        last_tag: 0,
        key: None,
        key_field: None,
        key_position: None,
        key_source: None,
        saw_value: false,
        value_field: None,
        value_position: None,
        value_source: None,
        pending: None,
        value: None,
        accepted_child: None,
        accepted_arena: Seq::empty(),
    }
}

pub open spec fn map_initial_with_source(
    source_bytes: Seq<u8>, physical_source: usize, physical_start: usize,
) -> MapFrameModel {
    MapFrameModel {
        physical_source,
        ..map_initial(source_bytes, physical_start)
    }
}

pub open spec fn map_initial_indexed(
    source_image: admission::AdmissionIndexedSourceImage,
) -> Option<MapFrameModel> {
    if !admission::admission_indexed_source_wf(source_image)
        || source_image.bytes.len() > usize::MAX as int
    {
        None
    } else {
        let start = admission::admission_indexed_source_position(
            source_image, 0,
        ).unwrap();
        Some(MapFrameModel {
            source_image: Some(source_image),
            physical_source: start.source,
            ..map_initial(source_image.bytes, start.offset)
        })
    }
}

pub proof fn map_initial_indexed_wf(
    source_image: admission::AdmissionIndexedSourceImage,
)
    requires map_initial_indexed(source_image).is_some(),
    ensures map_frame_wf(map_initial_indexed(source_image).unwrap()),
{}

pub open spec fn map_discontinuous_empty_value_image()
    -> admission::AdmissionIndexedSourceImage {
    admission::AdmissionIndexedSourceImage {
        bytes: seq![0x12u8, 0x00u8],
        positions: seq![
            admission::AdmissionSourceAnchor { source: 0, offset: 0 },
            admission::AdmissionSourceAnchor { source: 0, offset: 1 },
        ],
        end: admission::AdmissionSourceAnchor { source: 1, offset: 100 },
    }
}

pub open spec fn map_discontinuous_empty_value_field()
    -> admission::IndexedCanonicalFieldModel {
    admission::IndexedCanonicalFieldModel {
        number: 2,
        encoded_start: 0,
        encoded_end: 2,
        payload_start: 2,
        payload_end: 2,
        key_width: 1,
        payload_width: 1,
        payload_length: 0,
        wire: 2,
        decoded_varint: 0,
        payload_bytes: Seq::empty(),
    }
}

pub proof fn map_discontinuous_empty_value_push_possible()
    ensures
        map_initial_indexed(map_discontinuous_empty_value_image()).is_some(),
        map_value_push_step(
            map_initial_indexed(map_discontinuous_empty_value_image()).unwrap(),
            map_discontinuous_empty_value_field(), 0,
        ).is_some(),
        admission::admission_indexed_source_position(
            map_discontinuous_empty_value_image(), 2,
        ) == Some(admission::AdmissionSourceAnchor { source: 1, offset: 100 }),
{
    let image = map_discontinuous_empty_value_image();
    let field = map_discontinuous_empty_value_field();
    assert(map_initial_indexed(image).is_some());
    let frame = map_initial_indexed(image).unwrap();
    map_initial_indexed_wf(image);
    assert(admission::decoded_varint_prefix(seq![0x12u8], 0) == 0);
    assert(admission::decoded_varint_prefix(seq![0x12u8], 1) == 18);
    assert(admission::canonical_varint_bytes(seq![0x12u8], 18));
    assert(admission::decoded_varint_prefix(seq![0x00u8], 0) == 0);
    assert(admission::decoded_varint_prefix(seq![0x00u8], 1) == 0);
    assert(admission::canonical_varint_bytes(seq![0x00u8], 0));
    assert(image.bytes == seq![0x12u8, 0x00u8]);
    assert(image.bytes.subrange(0, 1) == seq![0x12u8]);
    assert(image.bytes.subrange(1, 2) == seq![0x00u8]);
    assert(image.bytes.subrange(2, 2) == Seq::<u8>::empty());
    assert(field.number == 2 && field.wire == 2);
    assert(field.payload_bytes == Seq::<u8>::empty());
    assert(admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: 2,
            next: 0,
            source_bytes: image.bytes,
        }, field,
    ));
}

pub proof fn map_initial_wf(source_bytes: Seq<u8>, physical_start: usize)
    requires
        source_bytes.len() <= usize::MAX as int,
        physical_start as int + source_bytes.len() <= usize::MAX as int,
    ensures map_frame_wf(map_initial(source_bytes, physical_start)),
{}

pub open spec fn map_field_at_cursor(
    frame: MapFrameModel, field: admission::IndexedCanonicalFieldModel,
) -> bool {
    admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: frame.region_end,
            next: frame.cursor,
            source_bytes: frame.source_bytes,
        },
        field,
    ) && field.number >= frame.last_tag
}

pub open spec fn map_key_step(
    frame: MapFrameModel, field: admission::IndexedCanonicalFieldModel,
) -> Option<MapFrameModel> {
    if !map_frame_wf(frame)
        || frame.pending.is_some()
        || !map_field_at_cursor(frame, field)
        || field.number != 1
        || field.wire != 2
        || frame.key.is_some()
        || field.payload_bytes.len() == 0
        || !admission::utf8_valid_at(field.payload_bytes, 0)
        || map_source_position(frame, field.encoded_start).is_none()
        || map_source_id(frame, field.encoded_start).is_none()
    {
        None
    } else {
        Some(MapFrameModel {
            owner_type_id: frame.owner_type_id,
            outer_field: frame.outer_field,
            value_type_id: frame.value_type_id,
            physical_start: frame.physical_start,
            physical_source: frame.physical_source,
            region_end: frame.region_end,
            source_bytes: frame.source_bytes,
            source_image: frame.source_image,
            cursor: field.encoded_end,
            last_tag: 1,
            key: Some(field.payload_bytes),
            key_field: Some(field),
            key_position: map_source_position(frame, field.encoded_start),
            key_source: map_source_id(frame, field.encoded_start),
            saw_value: frame.saw_value,
            value_field: frame.value_field,
            value_position: frame.value_position,
            value_source: frame.value_source,
            pending: frame.pending,
            value: frame.value,
            accepted_child: frame.accepted_child,
            accepted_arena: frame.accepted_arena,
        })
    }
}

pub open spec fn map_value_push_step(
    frame: MapFrameModel, field: admission::IndexedCanonicalFieldModel,
    obligation_id: u64,
) -> Option<(MapFrameModel, MapValuePending)> {
    if !map_frame_wf(frame)
        || frame.pending.is_some()
        || frame.saw_value
        || obligation_id == u64::MAX
        || !map_field_at_cursor(frame, field)
        || field.number != 2
        || field.wire != 2
        || generated::generated_slot_count(frame.value_type_id).is_none()
        || map_source_position(frame, field.encoded_start).is_none()
        || map_source_id(frame, field.encoded_start).is_none()
    {
        None
    } else {
        let pending = MapValuePending {
            id: obligation_id,
            field_position: map_source_position(frame, field.encoded_start).unwrap(),
            field_source: map_source_id(frame, field.encoded_start).unwrap(),
            payload_start: field.payload_start,
            payload_end: field.payload_end,
            child_type_id: frame.value_type_id,
        };
        Some((MapFrameModel {
            owner_type_id: frame.owner_type_id,
            outer_field: frame.outer_field,
            value_type_id: frame.value_type_id,
            physical_start: frame.physical_start,
            physical_source: frame.physical_source,
            region_end: frame.region_end,
            source_bytes: frame.source_bytes,
            source_image: frame.source_image,
            cursor: field.encoded_end,
            last_tag: 2,
            key: frame.key,
            key_field: frame.key_field,
            key_position: frame.key_position,
            key_source: frame.key_source,
            saw_value: true,
            value_field: Some(field),
            value_position: Some(pending.field_position),
            value_source: Some(pending.field_source),
            pending: Some(pending),
            value: None,
            accepted_child: None,
            accepted_arena: Seq::empty(),
        }, pending))
    }
}

pub open spec fn map_value_accept_step(
    frame: MapFrameModel, returned_id: u64,
    child: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<MapFrameModel> {
    if !map_frame_wf(frame) {
        None
    } else {
        match frame.pending {
            None => None,
            Some(wait) => {
                let semantic_default = admission::admission_raw_par_semantic_default(child);
                let stability = admission::admission_raw_par_path_stability(child);
                let canonical_path_stable = stability.unwrap_or(false);
                if wait.id != returned_id
                    || wait.child_type_id != child.program_type_id
                    || child.frame_kind != 1
                    || child.family != admission::AdmissionFamily::Other
                    || child.return_id != Some(returned_id)
                    || Some(child.physical_start as int)
                        != map_source_position(frame, wait.payload_start)
                    || Some(child.physical_source)
                        != map_source_id(frame, wait.payload_start)
                    || child.region_start != 0
                    || child.region_end as int
                        != wait.payload_end as int - wait.payload_start as int
                    || child.source_bytes != frame.source_bytes.subrange(
                        wait.payload_start as int, wait.payload_end as int,
                    )
                    || child.cursor != child.region_end
                    || child.waiting_id.is_some()
                    || child.nested_started != child.nested_resolved
                    || !admission::admission_frame_wf(child)
                    || !admission::admission_slots_complete(child)
                    || !admission::admission_family_admissible(child)
                    || stability.is_none()
                    || !admission::admission_child_summary_agrees(
                        child,
                        admission::AdmissionReturnedKind::Par {
                            semantic_default,
                            canonical_path_stable,
                        },
                        arena,
                    )
                    || semantic_default
                {
                    None
                } else {
                    match child.score_handle {
                        None => None,
                        Some(handle) => Some(MapFrameModel {
                            owner_type_id: frame.owner_type_id,
                            outer_field: frame.outer_field,
                            value_type_id: frame.value_type_id,
                            physical_start: frame.physical_start,
                            physical_source: frame.physical_source,
                            region_end: frame.region_end,
                            source_bytes: frame.source_bytes,
                            source_image: frame.source_image,
                            cursor: frame.cursor,
                            last_tag: frame.last_tag,
                            key: frame.key,
                            key_field: frame.key_field,
                            key_position: frame.key_position,
                            key_source: frame.key_source,
                            saw_value: frame.saw_value,
                            value_field: frame.value_field,
                            value_position: frame.value_position,
                            value_source: frame.value_source,
                            pending: None,
                            value: Some(MapValueSummary {
                                obligation_id: wait.id,
                                score_handle: handle,
                                score_origin_body_id: arena[handle as int].body_id,
                                child_body_id: child.body_id,
                                semantic_default,
                                canonical_path_stable,
                            }),
                            accepted_child: Some(child),
                            accepted_arena: arena,
                        }),
                    }
                }
            },
        }
    }
}

pub open spec fn map_finish(frame: MapFrameModel) -> Option<MapSummaryModel> {
    if !map_frame_wf(frame)
        || frame.cursor != frame.region_end
        || frame.pending.is_some()
        || (frame.saw_value && frame.value.is_none())
    {
        None
    } else {
        Some(MapSummaryModel {
            key_bytes: match frame.key {
                None => Seq::empty(),
                Some(key) => key,
            },
            key_position: match frame.key_position {
                None => frame.physical_start as int,
                Some(position) => position,
            },
            key_source: frame.key_source.unwrap_or(frame.physical_source),
            value: frame.value,
        })
    }
}

pub struct MapEofReceiptModel {
    pub owner_type_id: u16,
    pub outer_field: u32,
    pub value_type_id: u16,
    pub physical_start: usize,
    pub ghost physical_source: usize,
    pub region_end: usize,
    pub ghost source_bytes: Seq<u8>,
    pub ghost source_image: Option<admission::AdmissionIndexedSourceImage>,
    pub cursor: usize,
    pub key_present: bool,
    pub key_position: Option<int>,
    pub ghost key_source: Option<usize>,
    pub saw_value: bool,
    pub value_position: Option<int>,
    pub ghost value_source: Option<usize>,
    pub summary: MapSummaryModel,
}

pub open spec fn map_eof_snapshot(
    frame: MapFrameModel, terminal_error_free: bool,
) -> Option<MapEofReceiptModel> {
    if !terminal_error_free {
        None
    } else {
        match map_finish(frame) {
            None => None,
            Some(summary) => Some(MapEofReceiptModel {
                owner_type_id: frame.owner_type_id,
                outer_field: frame.outer_field,
                value_type_id: frame.value_type_id,
                physical_start: frame.physical_start,
                physical_source: frame.physical_source,
                region_end: frame.region_end,
                source_bytes: frame.source_bytes,
                source_image: frame.source_image,
                cursor: frame.cursor,
                key_present: frame.key.is_some(),
                key_position: frame.key_position,
                key_source: frame.key_source,
                saw_value: frame.saw_value,
                value_position: frame.value_position,
                value_source: frame.value_source,
                summary,
            }),
        }
    }
}

pub open spec fn map_eof_receipts_match(
    before: MapEofReceiptModel, produced: MapEofReceiptModel,
) -> bool {
    &&& before.owner_type_id == produced.owner_type_id
    &&& before.outer_field == produced.outer_field
    &&& before.value_type_id == produced.value_type_id
    &&& before.physical_start == produced.physical_start
    &&& before.physical_source == produced.physical_source
    &&& before.region_end == produced.region_end
    &&& before.source_bytes == produced.source_bytes
    &&& before.source_image == produced.source_image
    &&& before.cursor == produced.cursor
    &&& before.key_present == produced.key_present
    &&& before.key_position == produced.key_position
    &&& before.key_source == produced.key_source
    &&& before.saw_value == produced.saw_value
    &&& before.value_position == produced.value_position
    &&& before.value_source == produced.value_source
    &&& before.summary == produced.summary
}

pub enum MapEofReturnedModel {
    Root { summary: MapSummaryModel },
    AcceptedMessage { summary: MapSummaryModel },
    AcceptedOther,
}

pub open spec fn map_eof_return_matches(
    produced: MapEofReceiptModel, returned: MapEofReturnedModel,
) -> bool {
    match returned {
        MapEofReturnedModel::Root { summary }
        | MapEofReturnedModel::AcceptedMessage { summary } =>
            produced.summary == summary,
        MapEofReturnedModel::AcceptedOther => false,
    }
}

pub open spec fn map_eof_checked_complete(
    frame: MapFrameModel, terminal_error_free: bool,
    produced: MapEofReceiptModel, returned: MapEofReturnedModel,
) -> bool {
    match map_eof_snapshot(frame, terminal_error_free) {
        None => false,
        Some(before) => map_eof_receipts_match(before, produced)
            && map_eof_return_matches(produced, returned),
    }
}

pub open spec fn map_summary_source_projection(
    frame: MapFrameModel, summary: MapSummaryModel,
) -> bool {
    &&& map_frame_wf(frame)
    &&& frame.cursor == frame.region_end
    &&& frame.pending.is_none()
    &&& (match frame.key_field {
        None => summary.key_bytes.len() == 0
            && summary.key_position == frame.physical_start as int
            && summary.key_source == frame.physical_source,
        Some(field) => summary.key_bytes == frame.source_bytes.subrange(
                field.payload_start as int, field.payload_end as int,
            )
            && Some(summary.key_position)
                == map_source_position(frame, field.encoded_start)
            && Some(summary.key_source)
                == map_source_id(frame, field.encoded_start),
    })
    &&& (match frame.value_field {
        None => summary.value.is_none(),
        Some(field) => summary.value.is_some()
            && summary.value == frame.value
            && field.payload_bytes == frame.source_bytes.subrange(
                field.payload_start as int, field.payload_end as int,
            )
            && !summary.value.unwrap().semantic_default,
    })
}

pub open spec fn map_source_key_witness(
    frame: admission::AdmissionFrameState, key_bytes: Seq<u8>,
    field: admission::IndexedCanonicalFieldModel,
) -> bool {
    admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: frame.region_end,
            next: 0,
            source_bytes: frame.source_bytes,
        }, field,
    )
        && field.number == 1
        && field.wire == 2
        && field.payload_bytes == key_bytes
        && key_bytes.len() > 0
        && admission::utf8_valid_at(key_bytes, 0)
}

pub open spec fn map_source_key_full_witness(
    frame: admission::AdmissionFrameState, key_bytes: Seq<u8>,
    field: admission::IndexedCanonicalFieldModel,
) -> bool {
    map_source_key_witness(frame, key_bytes, field)
        && if frame.seen.contains(1) {
            exists|value_field: admission::IndexedCanonicalFieldModel|
                admission::indexed_field_at_cursor(
                    admission::IndexedCanonicalFieldCursorModel {
                        region_start: 0,
                        region_end: frame.region_end,
                        next: field.encoded_end,
                        source_bytes: frame.source_bytes,
                    }, value_field,
                )
                    && value_field.number == 2
                    && value_field.wire == 2
                    && value_field.encoded_end == frame.region_end
        } else {
            field.encoded_end == frame.region_end
        }
}

pub proof fn map_source_key_full_exists(
    frame: admission::AdmissionFrameState, key_bytes: Seq<u8>,
)
    requires
        frame.seen.contains(0),
        admission::admission_map_source_matches(frame, key_bytes),
    ensures exists|field: admission::IndexedCanonicalFieldModel|
        map_source_key_full_witness(frame, key_bytes, field),
{
    let field = choose|field: admission::IndexedCanonicalFieldModel|
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: frame.region_end,
                next: 0,
                source_bytes: frame.source_bytes,
            }, field,
        )
            && field.number == 1
            && field.wire == 2
            && field.payload_bytes == key_bytes
            && key_bytes.len() > 0
            && admission::utf8_valid_at(key_bytes, 0)
            && if frame.seen.contains(1) {
                exists|value_field: admission::IndexedCanonicalFieldModel|
                    admission::indexed_field_at_cursor(
                        admission::IndexedCanonicalFieldCursorModel {
                            region_start: 0,
                            region_end: frame.region_end,
                            next: field.encoded_end,
                            source_bytes: frame.source_bytes,
                        }, value_field,
                    )
                        && value_field.number == 2
                        && value_field.wire == 2
                        && value_field.encoded_end == frame.region_end
            } else {
                field.encoded_end == frame.region_end
            };
    assert(map_source_key_witness(frame, key_bytes, field));
    assert(map_source_key_full_witness(frame, key_bytes, field));
}

pub enum MapActionModel {
    Key { field: admission::IndexedCanonicalFieldModel },
    Value { field: admission::IndexedCanonicalFieldModel, obligation_id: u64 },
    Accept {
        returned_id: u64,
        child: admission::AdmissionFrameState,
        ghost arena: Seq<admission::AdmissionScoreBinding>,
    },
    Finish,
}

pub enum MapOutcomeModel {
    Continued { frame: MapFrameModel },
    Pushed { frame: MapFrameModel, pending: MapValuePending },
    Accepted { frame: MapFrameModel },
    Completed { summary: MapSummaryModel },
}

pub open spec fn map_cursor_step(
    frame: MapFrameModel,
    next_field: Option<admission::IndexedCanonicalFieldModel>,
    obligation_id: u64,
) -> Option<MapOutcomeModel> {
    if !map_frame_wf(frame) || frame.pending.is_some() {
        None
    } else {
        match next_field {
            None => match map_finish(frame) {
                None => None,
                Some(summary) => Some(MapOutcomeModel::Completed { summary }),
            },
            Some(field) => if !map_field_at_cursor(frame, field) {
                None
            } else if field.number == 1 {
                match map_key_step(frame, field) {
                    None => None,
                    Some(next) => Some(MapOutcomeModel::Continued { frame: next }),
                }
            } else if field.number == 2 {
                match map_value_push_step(frame, field, obligation_id) {
                    None => None,
                    Some((next, pending)) => Some(MapOutcomeModel::Pushed {
                        frame: next, pending,
                    }),
                }
            } else { None },
        }
    }
}

pub open spec fn map_driver_step(
    frame: MapFrameModel, action: MapActionModel,
) -> Option<MapOutcomeModel> {
    match action {
        MapActionModel::Key { field } => match map_key_step(frame, field) {
            None => None,
            Some(next) => Some(MapOutcomeModel::Continued { frame: next }),
        },
        MapActionModel::Value { field, obligation_id } => match map_value_push_step(
            frame, field, obligation_id,
        ) {
            None => None,
            Some((next, pending)) => Some(MapOutcomeModel::Pushed {
                frame: next, pending,
            }),
        },
        MapActionModel::Accept { returned_id, child, arena } => match map_value_accept_step(
            frame, returned_id, child, arena,
        ) {
            None => None,
            Some(next) => Some(MapOutcomeModel::Accepted { frame: next }),
        },
        MapActionModel::Finish => match map_finish(frame) {
            None => None,
            Some(summary) => Some(MapOutcomeModel::Completed { summary }),
        },
    }
}

pub open spec fn map_admission_key_continue(
    frame: MapFrameModel, state: admission::AdmissionState,
    field: admission::IndexedCanonicalFieldModel,
) -> Option<(MapFrameModel, admission::AdmissionState)> {
    if !admission::admission_state_wf(state)
        || state.frames.len() == 0
        || !map_frame_wf(frame)
        || !map_aux_projection(frame, state.frames[state.frames.len() - 1])
    {
        None
    } else {
        match map_key_step(frame, field) {
            None => None,
            Some(next_frame) => match admission::admission_continue_step(
                state, field.encoded_end, 0, false, 1,
            ) {
                None => None,
                Some(next_state) => if map_aux_projection(
                    next_frame, next_state.frames[next_state.frames.len() - 1],
                ) {
                    Some((next_frame, next_state))
                } else { None },
            },
        }
    }
}

pub open spec fn map_admission_value_push(
    frame: MapFrameModel, state: admission::AdmissionState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState, frame_bytes: usize,
) -> Option<(MapFrameModel, admission::AdmissionState)> {
    if !admission::admission_state_wf(state)
        || state.frames.len() == 0
        || !map_frame_wf(frame)
        || !map_aux_projection(frame, state.frames[state.frames.len() - 1])
        || child.family != admission::AdmissionFamily::Other
        || child.frame_kind != 1
        || child.program_type_id != frame.value_type_id
        || child.region_start != 0
        || child.physical_source != frame.physical_source
        || child.physical_start as int
            != frame.physical_start as int + field.payload_start as int
        || child.region_end as int
            != field.payload_end as int - field.payload_start as int
        || child.source_bytes != field.payload_bytes
        || !admission::admission_root_initial(child)
    {
        None
    } else {
        match map_value_push_step(frame, field, state.next_obligation_id) {
            None => None,
            Some((next_frame, _)) => match admission::admission_push_step(
                state, field.encoded_end, child, 1, false, 2, frame_bytes,
            ) {
                None => None,
                Some(next_state) => if map_aux_projection(
                    next_frame, next_state.frames[next_state.frames.len() - 2],
                ) {
                    Some((next_frame, next_state))
                } else { None },
            },
        }
    }
}

pub open spec fn map_admission_value_push_indexed(
    frame: MapFrameModel, state: admission::AdmissionState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
) -> Option<(MapFrameModel, admission::AdmissionState)> {
    if !admission::admission_state_wf(state)
        || state.frames.len() == 0
        || !map_frame_wf(frame)
        || !map_aux_projection(frame, state.frames[state.frames.len() - 1])
        || admission::admission_indexed_field_child_source(
            state.frames[state.frames.len() - 1], parent_source, field,
        ).is_none()
        || frame.source_image != Some(parent_source)
        || frame.source_bytes != parent_source.bytes
        || admission::admission_indexed_source_position(parent_source, 0).is_none()
        || frame.physical_start != admission::admission_indexed_source_position(
            parent_source, 0,
        ).unwrap().offset
        || frame.physical_source != admission::admission_indexed_source_position(
            parent_source, 0,
        ).unwrap().source
        || child.family != admission::AdmissionFamily::Other
        || child.frame_kind != 1
        || child.program_type_id != frame.value_type_id
        || child.region_start != 0
        || child.physical_source != admission::admission_indexed_source_position(
            parent_source, field.payload_start,
        ).unwrap().source
        || child.physical_start != admission::admission_indexed_source_position(
            parent_source, field.payload_start,
        ).unwrap().offset
        || child.region_end as int
            != field.payload_end as int - field.payload_start as int
        || child.source_bytes != field.payload_bytes
        || !admission::admission_root_initial(child)
    {
        None
    } else {
        match map_value_push_step(frame, field, state.next_obligation_id) {
            None => None,
            Some((next_frame, _)) => match admission::admission_push_step(
                state, field.encoded_end, child, 1, false, 2, frame_bytes,
            ) {
                None => None,
                Some(next_state) => if map_aux_projection(
                    next_frame, next_state.frames[next_state.frames.len() - 2],
                ) {
                    Some((next_frame, next_state))
                } else { None },
            },
        }
    }
}

pub proof fn map_admission_value_push_indexed_source_anchor_exact(
    frame: MapFrameModel,
    state: admission::AdmissionState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    ensures match map_admission_value_push_indexed(
        frame, state, field, child, parent_source, frame_bytes,
    ) {
        None => true,
        Some((next_frame, next_state)) => {
            &&& frame.physical_source
                == admission::admission_indexed_source_position(
                    parent_source, 0,
                ).unwrap().source
            &&& frame.physical_start
                == admission::admission_indexed_source_position(
                    parent_source, 0,
                ).unwrap().offset
            &&& next_frame.pending.unwrap().field_source
                == admission::admission_indexed_source_position(
                    parent_source, field.encoded_start,
                ).unwrap().source
            &&& next_frame.pending.unwrap().field_position
                == admission::admission_indexed_source_position(
                    parent_source, field.encoded_start,
                ).unwrap().offset as int
            &&& next_state.frames.last().physical_source
                == admission::admission_indexed_source_position(
                    parent_source, field.payload_start,
                ).unwrap().source
            &&& next_state.frames.last().physical_start
                == admission::admission_indexed_source_position(
                    parent_source, field.payload_start,
                ).unwrap().offset
        },
    },
{}

pub proof fn map_admission_value_push_indexed_rejects_wrong_source(
    frame: MapFrameModel,
    state: admission::AdmissionState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires
        admission::admission_indexed_source_wf(parent_source),
        (field.payload_start as int) <= parent_source.bytes.len(),
        child.physical_source != admission::admission_indexed_source_position(
            parent_source, field.payload_start,
        ).unwrap().source,
    ensures map_admission_value_push_indexed(
        frame, state, field, child, parent_source, frame_bytes,
    ).is_none(),
{}

pub open spec fn map_admission_value_complete(
    frame: MapFrameModel, state: admission::AdmissionState,
) -> Option<(MapFrameModel, admission::AdmissionState)> {
    if !admission::admission_state_wf(state)
        || state.frames.len() <= 1
        || !map_frame_wf(frame)
        || !map_aux_projection(frame, state.frames[state.frames.len() - 2])
    {
        None
    } else {
        let child = state.frames[state.frames.len() - 1];
        match frame.pending {
            None => None,
            Some(wait) => match map_value_accept_step(
                frame, wait.id, child, state.arena,
            ) {
                None => None,
                Some(next_frame) => match admission::admission_complete_child_step(
                    state, wait.id, 1,
                    admission::AdmissionReturnedKind::Par {
                        semantic_default: false,
                        canonical_path_stable: next_frame.value.unwrap().canonical_path_stable,
                    },
                ) {
                    None => None,
                    Some(next_state) => if map_aux_projection(
                        next_frame, next_state.frames[next_state.frames.len() - 1],
                    ) {
                        Some((next_frame, next_state))
                    } else { None },
                },
            },
        }
    }
}

pub proof fn map_admission_value_accept_pop_source_anchor_exact(
    frame: MapFrameModel,
    state: admission::AdmissionState,
)
    ensures match map_admission_value_complete(frame, state) {
        None => true,
        Some((next_frame, next_state)) => {
            &&& next_state.frames.last().physical_source
                == state.frames[state.frames.len() - 2].physical_source
            &&& next_state.frames.last().physical_start
                == state.frames[state.frames.len() - 2].physical_start
            &&& next_frame.physical_source == frame.physical_source
            &&& next_frame.physical_start == frame.physical_start
            &&& next_frame.value_source == frame.value_source
            &&& next_frame.value_position == frame.value_position
        },
    },
{}

pub open spec fn map_admission_entry_complete(
    frame: MapFrameModel, state: admission::AdmissionState,
) -> Option<(MapSummaryModel, admission::AdmissionState)> {
    if !admission::admission_state_wf(state)
        || state.frames.len() <= 1
        || !map_frame_wf(frame)
        || !map_aux_projection(frame, state.frames[state.frames.len() - 1])
    {
        None
    } else {
        let map_child = state.frames[state.frames.len() - 1];
        let parent = state.frames[state.frames.len() - 2];
        if parent.family != admission::AdmissionFamily::New
            || parent.program_type_id != frame.owner_type_id
            || parent.waiting_slot != Some(3)
            || parent.waiting_tag != Some(frame.outer_field)
            || parent.waiting_kind != 2
            || !admission::admission_map_summary_complete(map_child)
        {
            None
        } else {
            match map_finish(frame) {
                None => None,
                Some(summary) => if !map_summary_source_projection(frame, summary)
                    || !admission::admission_map_source_matches(
                        map_child, summary.key_bytes,
                    )
                {
                    None
                } else {
                    match map_child.return_id {
                        None => None,
                        Some(returned_id) => match admission::admission_complete_child_step(
                            state, returned_id, 2, admission::AdmissionReturnedKind::Map {
                                key_bytes: summary.key_bytes,
                                value_score_handle: admission::admission_map_value_score_handle(map_child),
                            },
                        ) {
                            None => None,
                            Some(next_state) => Some((summary, next_state)),
                        },
                    }
                },
            }
        }
    }
}

pub proof fn map_admission_entry_complete_source_anchor_exact(
    frame: MapFrameModel,
    state: admission::AdmissionState,
)
    ensures match map_admission_entry_complete(frame, state) {
        None => true,
        Some((summary, next_state)) => {
            &&& summary.key_source
                == frame.key_source.unwrap_or(frame.physical_source)
            &&& next_state.frames.last().physical_source
                == state.frames[state.frames.len() - 2].physical_source
            &&& next_state.frames.last().physical_start
                == state.frames[state.frames.len() - 2].physical_start
        },
    },
{}

pub open spec fn map_fresh_child_from_field(
    parent: admission::AdmissionFrameState,
    field: admission::IndexedCanonicalFieldModel,
) -> Option<admission::AdmissionFrameState> {
    let cursor = admission::IndexedCanonicalFieldCursorModel {
        region_start: parent.region_start,
        region_end: parent.region_end,
        next: parent.cursor,
        source_bytes: parent.source_bytes,
    };
    if !admission::admission_frame_wf(parent)
        || parent.frame_kind != 1
        || parent.family != admission::AdmissionFamily::New
        || parent.program_type_id != 20
        || field.number != 4
        || field.wire != 2
        || !admission::indexed_field_at_cursor(cursor, field)
    {
        None
    } else {
        match generated::generated_descriptor(parent.program_type_id, field.number) {
            None => None,
            Some(descriptor) => if descriptor.slot != 3
                || !descriptor.repeated
                || descriptor.wire != field.wire
                || descriptor.frame_kind != 2
                || descriptor.child_type_id != Some(1)
            {
                None
            } else {
                Some(admission::AdmissionFrameState {
                    physical_source: parent.physical_source,
                    physical_start: (
                        parent.physical_start as int + field.payload_start as int
                    ) as usize,
                    program_type_id: descriptor.child_type_id.unwrap(),
                    region_start: 0,
                    region_end: (
                        field.payload_end as int - field.payload_start as int
                    ) as usize,
                    source_bytes: field.payload_bytes,
                    cursor: 0,
                    slot_count: 2,
                    last_slot: None,
                    seen: Set::empty(),
                    required: Set::empty(),
                    scalar_slots: Set::empty(),
                    scalar_occurrences: Seq::empty(),
                    returns: Seq::empty(),
                    family: admission::AdmissionFamily::Other,
                    waiting_id: None,
                    waiting_kind: 0,
                    waiting_slot: None,
                    waiting_tag: None,
                    return_id: None,
                    frame_kind: descriptor.frame_kind,
                    body_id: 0,
                    score_handle: None,
                    nested_started: 0,
                    nested_resolved: 0,
                })
            },
        }
    }
}

pub open spec fn integrated_source_message_action(
    state: admission::AdmissionState,
    action: admission::ConcreteFrameActionModel,
) -> Option<admission::ConcreteFrameActionModel> {
    match action {
        admission::ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            if state.frames.len() == 0 {
                None
            } else {
                let parent = state.frames[state.frames.len() - 1];
                match generated::generated_descriptor(parent.program_type_id, field.number) {
                    Some(descriptor) if descriptor.frame_kind == 2 =>
                        match map_fresh_child_from_field(parent, field) {
                            None => None,
                            Some(fresh) => Some(admission::ConcreteFrameActionModel::Push {
                                field, child: fresh, frame_bytes,
                            }),
                        },
                    _ => Some(admission::ConcreteFrameActionModel::Push {
                        field, child, frame_bytes,
                    }),
                }
            }
        },
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            if state.frames.len() == 0
                || !admission::admission_indexed_source_wf(parent_source)
                || field.payload_start as int > parent_source.bytes.len()
            {
                None
            } else {
                let parent = state.frames[state.frames.len() - 1];
                let anchor = admission::admission_indexed_source_position(
                    parent_source, field.payload_start,
                ).unwrap();
                match generated::generated_descriptor(parent.program_type_id, field.number) {
                    Some(descriptor) if descriptor.frame_kind == 2 =>
                        match map_fresh_child_from_field(parent, field) {
                            None => None,
                            Some(fresh) => Some(admission::ConcreteFrameActionModel::PushIndexed {
                                field,
                                child: admission::AdmissionFrameState {
                                    physical_source: anchor.source,
                                    physical_start: anchor.offset,
                                    ..fresh
                                },
                                parent_source,
                                frame_bytes,
                            }),
                        },
                    _ => Some(admission::ConcreteFrameActionModel::PushIndexed {
                        field, child, parent_source, frame_bytes,
                    }),
                }
            }
        },
        admission::ConcreteFrameActionModel::Continue { field } =>
            Some(admission::ConcreteFrameActionModel::Continue { field }),
        admission::ConcreteFrameActionModel::ContinueIndexed { field, parent_source } =>
            Some(admission::ConcreteFrameActionModel::ContinueIndexed {
                field, parent_source,
            }),
        admission::ConcreteFrameActionModel::Complete =>
            Some(admission::ConcreteFrameActionModel::Complete),
    }
}

pub proof fn map_fresh_child_is_source_initial(
    parent: admission::AdmissionFrameState,
    field: admission::IndexedCanonicalFieldModel,
)
    ensures match map_fresh_child_from_field(parent, field) {
        None => true,
        Some(child) => {
            &&& child.frame_kind == 2
            &&& child.program_type_id == 1
            &&& child.slot_count == 2
            &&& child.family == admission::AdmissionFamily::Other
            &&& child.body_id == 0
            &&& child.cursor == 0
            &&& child.last_slot.is_none()
            &&& child.seen == Set::empty()
            &&& child.scalar_slots == Set::empty()
            &&& child.returns.len() == 0
            &&& child.waiting_id.is_none()
            &&& child.return_id.is_none()
            &&& admission::admission_frame_wf(child)
            &&& admission::admission_child_physical_contained(parent, child)
            &&& (parent.physical_start as int + field.payload_end as int
                <= usize::MAX as int ==> {
                &&& map_frame_wf(map_initial_with_source(
                    child.source_bytes, child.physical_source, child.physical_start,
                ))
                &&& map_aux_projection(
                    map_initial_with_source(
                        child.source_bytes, child.physical_source, child.physical_start,
                    ), child,
                )
            })
        },
    },
{}

pub proof fn integrated_new_map_push_uses_fresh_child(
    state: admission::AdmissionState,
    field: admission::IndexedCanonicalFieldModel,
    offered: admission::AdmissionFrameState,
    frame_bytes: usize,
)
    requires
        state.frames.len() > 0,
        state.frames[state.frames.len() - 1].program_type_id == 20,
        field.number == 4,
        map_fresh_child_from_field(
            state.frames[state.frames.len() - 1], field,
        ).is_some(),
    ensures match integrated_source_message_action(
        state, admission::ConcreteFrameActionModel::Push {
            field, child: offered, frame_bytes,
        },
    ) {
        Some(admission::ConcreteFrameActionModel::Push {
            field: actual_field, child: actual_child,
            frame_bytes: actual_frame_bytes,
        }) => {
            &&& actual_field == field
            &&& actual_frame_bytes == frame_bytes
            &&& Some(actual_child) == map_fresh_child_from_field(
                state.frames[state.frames.len() - 1], field,
            )
        },
        _ => false,
    },
{}

pub struct IntegratedAdmissionState {
    pub admission: admission::AdmissionState,
    pub ghost map_frames: Seq<Option<MapFrameModel>>,
    pub ghost transcript: score::ScoreArena,
}

pub open spec fn integrated_admission_wf(state: IntegratedAdmissionState) -> bool {
    admission::admission_state_wf(state.admission)
        && score::arena_well_formed(state.transcript)
        && state.admission.arena.len() == state.transcript.entries.len()
        && (forall|i: int| 0 <= i < state.admission.arena.len()
            ==> state.admission.arena[i].encoded_len
                == state.transcript.entries[i].encoded_len
                && state.admission.arena[i].transcript_bytes
                    == state.transcript.entries[i].transcript)
        && state.map_frames.len() == state.admission.frames.len()
        && forall|index: int| 0 <= index < state.map_frames.len() ==> {
            match #[trigger] state.map_frames[index] {
                None => state.admission.frames[index].frame_kind != 2,
                Some(frame) => state.admission.frames[index].frame_kind == 2
                    && index > 0
                    && state.admission.frames[index - 1].family
                        == admission::AdmissionFamily::New
                    && state.admission.frames[index - 1].program_type_id
                        == frame.owner_type_id
                    && state.admission.frames[index - 1].waiting_slot == Some(3)
                    && state.admission.frames[index - 1].waiting_tag
                        == Some(frame.outer_field)
                    && state.admission.frames[index - 1].waiting_kind == 2
                    && state.admission.frames[index - 1].waiting_id
                        == state.admission.frames[index].return_id
                    && state.admission.frames[index].return_id.is_some()
                    && state.admission.frames[index].body_id as int
                        == state.admission.frames[index].return_id.unwrap() as int + 1
                    && map_frame_wf(frame)
                    && frame.accepted_arena.len() <= state.admission.arena.len()
                    && frame.accepted_arena == state.admission.arena.subrange(
                        0, frame.accepted_arena.len() as int,
                    )
                    && map_aux_projection(frame, state.admission.frames[index]),
            }
        }
}

pub open spec fn integrated_admission_result(
    admission_state: admission::AdmissionState,
    map_frames: Seq<Option<MapFrameModel>>,
    transcript: score::ScoreArena,
) -> Option<IntegratedAdmissionState> {
    let next = IntegratedAdmissionState {
        admission: admission_state,
        map_frames,
        transcript,
    };
    if integrated_admission_wf(next) { Some(next) } else { None }
}

pub open spec fn integrated_admission_initial(
    root: admission::AdmissionFrameState,
) -> Option<IntegratedAdmissionState> {
    integrated_admission_result(
        admission::admission_initial_state(root), Seq::empty().push(None),
        score::empty_arena(),
    )
}

pub proof fn integrated_initial_is_well_formed(root: admission::AdmissionFrameState)
    requires admission::admission_root_initial(root),
    ensures integrated_admission_initial(root).is_some(),
{
    admission::admission_initial_wf(root);
    score::empty_arena_well_formed();
}

pub open spec fn map_append_typed_score(
    state: admission::AdmissionState,
    transcript: score::ScoreArena,
    kind: score::ScoreNode,
) -> Option<(admission::AdmissionState, score::ScoreArena, usize)> {
    if state.frames.len() == 0
        || !score::arena_well_formed(transcript)
        || state.arena.len() != transcript.entries.len()
        || (exists|i: int| 0 <= i < state.arena.len()
            && (state.arena[i].encoded_len != transcript.entries[i].encoded_len
                || state.arena[i].transcript_bytes != transcript.entries[i].transcript))
    {
        None
    } else {
        match score::checked_append(transcript, kind) {
            None => None,
            Some((next_transcript, handle)) => {
                let top = state.frames[state.frames.len() - 1];
                let entry = next_transcript.entries[handle as int];
                let binding = admission::AdmissionScoreBinding {
                    body_id: top.body_id,
                    semantic: admission::admission_family_complete(top),
                    encoded_len: entry.encoded_len,
                    transcript_bytes: entry.transcript,
                };
                match admission::admission_append_score_entry_step(state, binding) {
                    None => None,
                    Some(next_state) => Some((next_state, next_transcript, handle)),
                }
            },
        }
    }
}

pub open spec fn map_raw_par_connective_score_step(
    state: IntegratedAdmissionState,
) -> Option<IntegratedAdmissionState> {
    if !integrated_admission_wf(state) || state.admission.frames.len() <= 1 {
        None
    } else {
        let count = state.admission.frames.len();
        let child = state.admission.frames[count - 1];
        let parent = state.admission.frames[count - 2];
        if parent.frame_kind != 2 || parent.program_type_id != 1
            || parent.waiting_slot != Some(1) || parent.waiting_tag != Some(2)
            || child.frame_kind != 1 || child.program_type_id != 1
            || child.family != admission::AdmissionFamily::Other
            || child.returns.len() != 0
            || (exists|slot: int| 0 <= slot < child.slot_count as int
                && child.seen.contains(slot) && slot != 7 && slot != 8)
            || !admission::admission_raw_par_connective_used(child)
            || child.score_handle.is_some()
            || child.cursor != child.region_end || child.waiting_id.is_some()
            || child.nested_started != child.nested_resolved
            || !admission::admission_slots_complete(child)
            || !admission::admission_family_admissible(child)
        {
            None
        } else {
            match map_append_typed_score(
                state.admission, state.transcript, score::ScoreNode::Integer(1),
            ) {
                None => None,
                Some((after_flag, flag_arena, flag_handle)) =>
                    match map_append_typed_score(
                        after_flag, flag_arena, score::ScoreNode::Integer(999),
                    ) {
                        None => None,
                        Some((after_constructor, constructor_arena, constructor_handle)) =>
                            match map_append_typed_score(
                                after_constructor, constructor_arena,
                                score::ScoreNode::Node(
                                    seq![constructor_handle, flag_handle],
                                ),
                            ) {
                                None => None,
                                Some((after_node, node_arena, node_handle)) =>
                                    match admission::admission_attach_score_step(
                                        after_node, node_handle, child.body_id,
                                    ) {
                                        None => None,
                                        Some(attached) => integrated_admission_result(
                                            attached, state.map_frames, node_arena,
                                        ),
                                    },
                            },
                    },
            }
        }
    }
}

pub enum IntegratedAdmissionAction {
    Message { action: admission::ConcreteFrameActionModel },
    MapKey { field: admission::IndexedCanonicalFieldModel },
    MapValue {
        field: admission::IndexedCanonicalFieldModel,
        child: admission::AdmissionFrameState,
        frame_bytes: usize,
    },
    MapValueIndexed {
        field: admission::IndexedCanonicalFieldModel,
        child: admission::AdmissionFrameState,
        parent_source: admission::AdmissionIndexedSourceImage,
        frame_bytes: usize,
    },
    MapParScore,
    MapAccept,
    MapFinish,
}

pub enum IntegratedAdmissionOutcome {
    MessageContinued { state: IntegratedAdmissionState },
    MessagePushed { state: IntegratedAdmissionState },
    MessageParentAccepted { state: IntegratedAdmissionState },
    MapKeyContinued { state: IntegratedAdmissionState },
    MapValuePushed { state: IntegratedAdmissionState },
    MapParScored { state: IntegratedAdmissionState },
    MapValueAccepted { state: IntegratedAdmissionState },
    MapEntryCompleted {
        summary: MapSummaryModel,
        state: IntegratedAdmissionState,
    },
    RootFinished { root_summary: admission::ConcreteRootSummaryModel },
}

pub open spec fn integrated_admission_driver_step(
    state: IntegratedAdmissionState, action: IntegratedAdmissionAction,
) -> Option<IntegratedAdmissionOutcome> {
    if !integrated_admission_wf(state) {
        None
    } else {
        let count = state.admission.frames.len();
        match action {
            IntegratedAdmissionAction::Message { action: message_action } => {
                if state.admission.frames[count - 1].frame_kind != 1 {
                    None
                } else {
                    match integrated_source_message_action(state.admission, message_action) {
                        None => None,
                        Some(source_action) => match admission::concrete_message_driver_step(
                            state.admission, source_action,
                        ) {
                        None => None,
                        Some(admission::ConcreteFrameOutcomeModel::Continued {
                            state: next_admission,
                        }) => match integrated_admission_result(
                            next_admission, state.map_frames, state.transcript,
                        ) {
                            None => None,
                            Some(next) => Some(
                                IntegratedAdmissionOutcome::MessageContinued { state: next },
                            ),
                        },
                        Some(admission::ConcreteFrameOutcomeModel::Pushed {
                            state: next_admission,
                        }) => {
                            let child = next_admission.frames[
                                next_admission.frames.len() - 1
                            ];
                            let map_entry = if child.frame_kind == 2 {
                                match source_action {
                                    admission::ConcreteFrameActionModel::PushIndexed {
                                        field, parent_source, ..
                                    } => match admission::admission_indexed_field_child_source(
                                        state.admission.frames[count - 1], parent_source, field,
                                    ) {
                                        None => None,
                                        Some(image) => map_initial_indexed(image),
                                    },
                                    _ => Some(map_initial_with_source(
                                        child.source_bytes, child.physical_source,
                                        child.physical_start,
                                    )),
                                }
                            } else { None };
                            match integrated_admission_result(
                                next_admission, state.map_frames.push(map_entry),
                                state.transcript,
                            ) {
                                None => None,
                                Some(next) => Some(
                                    IntegratedAdmissionOutcome::MessagePushed { state: next },
                                ),
                            }
                        },
                        Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
                            state: next_admission,
                        }) => match integrated_admission_result(
                            next_admission,
                            state.map_frames.subrange(0, state.map_frames.len() - 1),
                            state.transcript,
                        ) {
                            None => None,
                            Some(next) => Some(
                                IntegratedAdmissionOutcome::MessageParentAccepted { state: next },
                            ),
                        },
                        Some(admission::ConcreteFrameOutcomeModel::RootFinished { summary }) =>
                            Some(IntegratedAdmissionOutcome::RootFinished {
                                root_summary: summary,
                            }),
                        },
                    }
                }
            },
            IntegratedAdmissionAction::MapKey { field } => {
                match state.map_frames[count - 1] {
                    None => None,
                    Some(frame) => match map_admission_key_continue(
                        frame, state.admission, field,
                    ) {
                        None => None,
                        Some((next_frame, next_admission)) => match integrated_admission_result(
                            next_admission,
                            state.map_frames.update(count - 1, Some(next_frame)),
                            state.transcript,
                        ) {
                            None => None,
                            Some(next) => Some(
                                IntegratedAdmissionOutcome::MapKeyContinued { state: next },
                            ),
                        },
                    },
                }
            },
            IntegratedAdmissionAction::MapValue { field, child, frame_bytes } => {
                match state.map_frames[count - 1] {
                    None => None,
                    Some(frame) => match map_admission_value_push(
                        frame, state.admission, field, child, frame_bytes,
                    ) {
                        None => None,
                        Some((next_frame, next_admission)) => match integrated_admission_result(
                            next_admission,
                            state.map_frames.update(count - 1, Some(next_frame)).push(None),
                            state.transcript,
                        ) {
                            None => None,
                            Some(next) => Some(
                                IntegratedAdmissionOutcome::MapValuePushed { state: next },
                            ),
                        },
                    },
                }
            },
            IntegratedAdmissionAction::MapValueIndexed {
                field, child, parent_source, frame_bytes,
            } => {
                match state.map_frames[count - 1] {
                    None => None,
                    Some(frame) => match map_admission_value_push_indexed(
                        frame, state.admission, field, child, parent_source, frame_bytes,
                    ) {
                        None => None,
                        Some((next_frame, next_admission)) => match integrated_admission_result(
                            next_admission,
                            state.map_frames.update(count - 1, Some(next_frame)).push(None),
                            state.transcript,
                        ) {
                            None => None,
                            Some(next) => Some(
                                IntegratedAdmissionOutcome::MapValuePushed { state: next },
                            ),
                        },
                    },
                }
            },
            IntegratedAdmissionAction::MapParScore => {
                match map_raw_par_connective_score_step(state) {
                    None => None,
                    Some(next) => Some(
                        IntegratedAdmissionOutcome::MapParScored { state: next },
                    ),
                }
            },
            IntegratedAdmissionAction::MapAccept => {
                if count <= 1 { None } else {
                    match state.map_frames[count - 2] {
                        None => None,
                        Some(frame) => match map_admission_value_complete(
                            frame, state.admission,
                        ) {
                            None => None,
                            Some((next_frame, next_admission)) =>
                                match integrated_admission_result(
                                    next_admission,
                                    state.map_frames.update(count - 2, Some(next_frame))
                                        .subrange(0, count - 1),
                                    state.transcript,
                                ) {
                                    None => None,
                                    Some(next) => Some(
                                        IntegratedAdmissionOutcome::MapValueAccepted {
                                            state: next,
                                        },
                                    ),
                                },
                        },
                    }
                }
            },
            IntegratedAdmissionAction::MapFinish => {
                match state.map_frames[count - 1] {
                    None => None,
                    Some(frame) => match map_admission_entry_complete(
                        frame, state.admission,
                    ) {
                        None => None,
                        Some((summary, next_admission)) => match integrated_admission_result(
                            next_admission,
                            state.map_frames.subrange(0, count - 1),
                            state.transcript,
                        ) {
                            None => None,
                            Some(next) => Some(
                                IntegratedAdmissionOutcome::MapEntryCompleted {
                                    summary, state: next,
                                },
                            ),
                        },
                    },
                }
            },
        }
    }
}

pub open spec fn integrated_outcome_wf(outcome: IntegratedAdmissionOutcome) -> bool {
    match outcome {
        IntegratedAdmissionOutcome::MessageContinued { state }
        | IntegratedAdmissionOutcome::MessagePushed { state }
        | IntegratedAdmissionOutcome::MessageParentAccepted { state }
        | IntegratedAdmissionOutcome::MapKeyContinued { state }
        | IntegratedAdmissionOutcome::MapValuePushed { state }
        | IntegratedAdmissionOutcome::MapParScored { state }
        | IntegratedAdmissionOutcome::MapValueAccepted { state } =>
            integrated_admission_wf(state),
        IntegratedAdmissionOutcome::MapEntryCompleted { state, .. } =>
            integrated_admission_wf(state),
        IntegratedAdmissionOutcome::RootFinished { .. } => true,
    }
}

pub open spec fn integrated_nonterminal_state(
    outcome: IntegratedAdmissionOutcome,
) -> Option<IntegratedAdmissionState> {
    match outcome {
        IntegratedAdmissionOutcome::MessageContinued { state }
        | IntegratedAdmissionOutcome::MessagePushed { state }
        | IntegratedAdmissionOutcome::MessageParentAccepted { state }
        | IntegratedAdmissionOutcome::MapKeyContinued { state }
        | IntegratedAdmissionOutcome::MapValuePushed { state }
        | IntegratedAdmissionOutcome::MapParScored { state }
        | IntegratedAdmissionOutcome::MapValueAccepted { state }
        | IntegratedAdmissionOutcome::MapEntryCompleted { state, .. } => Some(state),
        IntegratedAdmissionOutcome::RootFinished { .. } => None,
    }
}

pub open spec fn integrated_replay_from_root(
    root: admission::AdmissionFrameState,
    actions: Seq<IntegratedAdmissionAction>,
) -> Option<IntegratedAdmissionState>
    decreases actions.len(),
{
    if !admission::admission_root_initial(root) {
        None
    } else if actions.len() == 0 {
        integrated_admission_initial(root)
    } else {
        match integrated_replay_from_root(
            root, actions.subrange(0, actions.len() - 1),
        ) {
            None => None,
            Some(prior) => match integrated_admission_driver_step(
                prior, actions[actions.len() - 1],
            ) {
                None => None,
                Some(outcome) => integrated_nonterminal_state(outcome),
            },
        }
    }
}

pub open spec fn integrated_replay_finishes_root(
    root: admission::AdmissionFrameState,
    actions: Seq<IntegratedAdmissionAction>,
) -> bool {
    actions.len() > 0 && match integrated_replay_from_root(
        root, actions.subrange(0, actions.len() - 1),
    ) {
        None => false,
        Some(prior) => match integrated_admission_driver_step(
            prior, actions[actions.len() - 1],
        ) {
            Some(IntegratedAdmissionOutcome::RootFinished { .. }) => true,
            _ => false,
        },
    }
}

pub proof fn integrated_root_finish_returns_derived_summary(
    state: IntegratedAdmissionState,
)
    requires integrated_admission_wf(state),
    ensures match integrated_admission_driver_step(
        state,
        IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ) {
        Some(IntegratedAdmissionOutcome::RootFinished { root_summary }) =>
            admission::concrete_message_root_summary(state.admission)
                == Some(root_summary),
        _ => true,
    },
{}

pub proof fn integrated_driver_preserves_wf(
    state: IntegratedAdmissionState, action: IntegratedAdmissionAction,
)
    ensures match integrated_admission_driver_step(state, action) {
        None => true,
        Some(outcome) => integrated_outcome_wf(outcome),
    },
{}

pub proof fn integrated_new_map_child_body_id_is_driver_assigned(
    state: IntegratedAdmissionState,
    field: admission::IndexedCanonicalFieldModel,
    offered: admission::AdmissionFrameState,
    frame_bytes: usize,
)
    ensures match integrated_admission_driver_step(
        state, IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Push {
                field, child: offered, frame_bytes,
            },
        },
    ) {
        Some(IntegratedAdmissionOutcome::MessagePushed { state: next }) => {
            let child = next.admission.frames[next.admission.frames.len() - 1];
            child.frame_kind != 2 || {
                &&& child.body_id as int
                    == state.admission.next_obligation_id as int + 1
                &&& child.return_id == Some(state.admission.next_obligation_id)
                &&& child.slot_count == 2
                &&& child.seen == Set::empty()
                &&& child.returns.len() == 0
            }
        },
        _ => true,
    },
{}

pub proof fn integrated_replay_preserves_wf(
    root: admission::AdmissionFrameState,
    actions: Seq<IntegratedAdmissionAction>,
)
    ensures match integrated_replay_from_root(root, actions) {
        None => true,
        Some(state) => integrated_admission_wf(state),
    },
    decreases actions.len(),
{
    if actions.len() > 0 {
        let prefix = actions.subrange(0, actions.len() - 1);
        integrated_replay_preserves_wf(root, prefix);
        if let Some(prior) = integrated_replay_from_root(root, prefix) {
            integrated_driver_preserves_wf(prior, actions[actions.len() - 1]);
        }
    }
}

pub proof fn integrated_map_completion_source_bound(state: IntegratedAdmissionState)
    ensures match integrated_admission_driver_step(
        state, IntegratedAdmissionAction::MapFinish,
    ) {
        Some(IntegratedAdmissionOutcome::MapEntryCompleted { summary, .. }) => {
            &&& state.admission.frames.len() > 1
            &&& admission::admission_map_source_matches(
                state.admission.frames[state.admission.frames.len() - 1],
                summary.key_bytes,
            )
            &&& match state.map_frames[state.map_frames.len() - 1] {
                None => false,
                Some(frame) => map_finish(frame) == Some(summary)
                    && map_summary_source_projection(frame, summary),
            }
        },
        _ => true,
    },
{}

pub proof fn integrated_map_finish_pop_shape(
    state: IntegratedAdmissionState,
    next: IntegratedAdmissionState,
    summary: MapSummaryModel,
)
    requires integrated_admission_driver_step(
        state, IntegratedAdmissionAction::MapFinish,
    ) == Some(IntegratedAdmissionOutcome::MapEntryCompleted {
        summary, state: next,
    }),
    ensures
        state.admission.frames.len() > 1,
        next.admission.frames.len() + 1 == state.admission.frames.len(),
        next.admission.completed as int == state.admission.completed as int + 1,
        next.admission.pushed == state.admission.pushed,
        next.admission.arena == state.admission.arena,
        next.admission.root_end == state.admission.root_end,
        next.admission.next_obligation_id == state.admission.next_obligation_id,
        next.transcript == state.transcript,
        next.map_frames == state.map_frames.subrange(
            0, state.map_frames.len() - 1,
        ),
        forall|i: int| 0 <= i < next.admission.frames.len() - 1
            ==> next.admission.frames[i] == state.admission.frames[i],
        next.admission.frames.last().physical_start
            == state.admission.frames[state.admission.frames.len() - 2].physical_start,
        next.admission.frames.last().physical_source
            == state.admission.frames[state.admission.frames.len() - 2].physical_source,
        next.admission.frames.last().frame_kind
            == state.admission.frames[state.admission.frames.len() - 2].frame_kind,
        next.admission.frames.last().region_start
            == state.admission.frames[state.admission.frames.len() - 2].region_start,
        next.admission.frames.last().region_end
            == state.admission.frames[state.admission.frames.len() - 2].region_end,
        next.admission.frames.last().source_bytes
            == state.admission.frames[state.admission.frames.len() - 2].source_bytes,
        next.admission.frames.last().cursor
            == state.admission.frames[state.admission.frames.len() - 2].cursor,
{
    let count = state.admission.frames.len();
    assert(integrated_admission_wf(state));
    assert(state.map_frames.len() == count);
    match state.map_frames[count - 1] {
        None => assert(false),
        Some(frame) => match map_admission_entry_complete(frame, state.admission) {
            None => assert(false),
            Some((actual_summary, next_admission)) => {
                assert(count > 1);
                assert(actual_summary == summary);
                assert(next.admission == next_admission);
                assert(next.transcript == state.transcript);
                assert(next.map_frames == state.map_frames.subrange(0, count - 1));
                map_admission_entry_complete_exact(frame, state.admission);
                let child = state.admission.frames[count - 1];
                assert(child.return_id.is_some());
                let returned_id = child.return_id.unwrap();
                let returned_kind = admission::AdmissionReturnedKind::Map {
                    key_bytes: summary.key_bytes,
                    value_score_handle: admission::admission_map_value_score_handle(child),
                };
                assert(admission::admission_complete_child_step(
                    state.admission, returned_id, 2, returned_kind,
                ) == Some(next_admission));
                admission::admission_complete_child_refines(
                    state.admission, returned_id, 2, returned_kind,
                );
                assert(next_admission.frames.len() == count - 1);
                assert(next_admission.arena == state.admission.arena);
                assert(next_admission.root_end == state.admission.root_end);
                assert(next_admission.next_obligation_id
                    == state.admission.next_obligation_id);
                let parent_index = count - 2;
                assert(parent_index == next_admission.frames.len() - 1);
                assert(next_admission.frames == state.admission.frames.subrange(
                    0, count - 1,
                ).update(parent_index, next_admission.frames[parent_index]));
                assert forall|i: int| 0 <= i < next_admission.frames.len() - 1
                    implies next_admission.frames[i] == state.admission.frames[i] by {
                    assert(i < parent_index);
                }
                let parent = state.admission.frames[parent_index];
                let next_parent = next_admission.frames[parent_index];
                assert(next_parent.physical_start == parent.physical_start);
                assert(next_parent.physical_source == parent.physical_source);
                assert(next_parent.frame_kind == parent.frame_kind);
                assert(next_parent.region_start == parent.region_start);
                assert(next_parent.region_end == parent.region_end);
                assert(next_parent.source_bytes == parent.source_bytes);
                assert(next_parent.cursor == parent.cursor);
            },
        },
    }
}

pub proof fn integrated_message_parent_accept_pop_shape(
    state: IntegratedAdmissionState,
    next: IntegratedAdmissionState,
)
    requires integrated_admission_driver_step(
        state,
        IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ) == Some(IntegratedAdmissionOutcome::MessageParentAccepted { state: next }),
    ensures
        state.admission.frames.len() > 1,
        state.admission.frames.last().frame_kind == 1,
        state.admission.frames[state.admission.frames.len() - 2].frame_kind == 1,
        next.admission.frames.len() + 1 == state.admission.frames.len(),
        next.admission.completed as int == state.admission.completed as int + 1,
        next.admission.pushed == state.admission.pushed,
        next.admission.arena == state.admission.arena,
        next.admission.root_end == state.admission.root_end,
        next.admission.next_obligation_id == state.admission.next_obligation_id,
        next.transcript == state.transcript,
        next.map_frames == state.map_frames.subrange(
            0, state.map_frames.len() - 1,
        ),
        forall|i: int| 0 <= i < next.admission.frames.len() - 1
            ==> next.admission.frames[i] == state.admission.frames[i],
        next.admission.frames.last().physical_start
            == state.admission.frames[state.admission.frames.len() - 2].physical_start,
        next.admission.frames.last().physical_source
            == state.admission.frames[state.admission.frames.len() - 2].physical_source,
        next.admission.frames.last().frame_kind
            == state.admission.frames[state.admission.frames.len() - 2].frame_kind,
        next.admission.frames.last().region_start
            == state.admission.frames[state.admission.frames.len() - 2].region_start,
        next.admission.frames.last().region_end
            == state.admission.frames[state.admission.frames.len() - 2].region_end,
        next.admission.frames.last().source_bytes
            == state.admission.frames[state.admission.frames.len() - 2].source_bytes,
        next.admission.frames.last().cursor
            == state.admission.frames[state.admission.frames.len() - 2].cursor,
        next.admission.frames.last().waiting_id.is_none(),
        match admission::concrete_derived_summary_with_arena(
            state.admission.frames[state.admission.frames.len() - 2],
            state.admission.frames.last(),
            state.admission.arena,
        ) {
            None => false,
            Some(summary) => state.admission.frames.last().return_id.is_some()
                && admission::admission_complete_child_step(
                    state.admission,
                    state.admission.frames.last().return_id.unwrap(),
                    state.admission.frames.last().frame_kind,
                    summary,
                ) == Some(next.admission),
        },
{
    let count = state.admission.frames.len();
    assert(integrated_admission_wf(state));
    assert(count > 1);
    assert(state.map_frames.len() == count);
    assert(state.admission.frames[count - 1].frame_kind == 1);
    assert(integrated_source_message_action(
        state.admission, admission::ConcreteFrameActionModel::Complete,
    ) == Some(admission::ConcreteFrameActionModel::Complete));
    assert(admission::concrete_message_driver_step(
        state.admission, admission::ConcreteFrameActionModel::Complete,
    ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
        state: next.admission,
    }));
    assert(next.transcript == state.transcript);
    assert(next.map_frames == state.map_frames.subrange(0, count - 1));
    admission::concrete_message_complete_child_simulates(state.admission);
    match admission::concrete_message_complete_child(state.admission) {
        None => assert(false),
        Some(completed) => {
            assert(completed.projection == next.admission);
            let child = state.admission.frames[count - 1];
            let parent_index = count - 2;
            let parent = state.admission.frames[parent_index];
            assert(parent.frame_kind == 1);
            assert(child.return_id.is_some());
            match admission::concrete_derived_summary_with_arena(
                parent, child, state.admission.arena,
            ) {
                None => assert(false),
                Some(summary) => {
                    let returned_id = child.return_id.unwrap();
                    assert(admission::admission_complete_child_step(
                        state.admission, returned_id, child.frame_kind, summary,
                    ) == Some(next.admission));
                    admission::admission_complete_child_refines(
                        state.admission, returned_id, child.frame_kind, summary,
                    );
                    assert(next.admission.frames.len() == count - 1);
                    assert(next.admission.arena == state.admission.arena);
                    assert(next.admission.root_end == state.admission.root_end);
                    assert(next.admission.next_obligation_id
                        == state.admission.next_obligation_id);
                    assert(parent_index == next.admission.frames.len() - 1);
                    assert(next.admission.frames == state.admission.frames.subrange(
                        0, count - 1,
                    ).update(parent_index, next.admission.frames[parent_index]));
                    assert forall|i: int| 0 <= i < next.admission.frames.len() - 1
                        implies next.admission.frames[i] == state.admission.frames[i] by {
                        assert(i < parent_index);
                    }
                    let next_parent = next.admission.frames[parent_index];
                    assert(next_parent.physical_start == parent.physical_start);
                    assert(next_parent.physical_source == parent.physical_source);
                    assert(next_parent.frame_kind == parent.frame_kind);
                    assert(next_parent.region_start == parent.region_start);
                    assert(next_parent.region_end == parent.region_end);
                    assert(next_parent.source_bytes == parent.source_bytes);
                    assert(next_parent.cursor == parent.cursor);
                },
            }
        },
    }
}

pub proof fn map_key_step_exact(
    frame: MapFrameModel, field: admission::IndexedCanonicalFieldModel,
)
    ensures match map_key_step(frame, field) {
        None => true,
        Some(next) => {
            &&& map_frame_wf(next)
            &&& next.cursor == field.encoded_end
            &&& next.last_tag == 1
            &&& next.key == Some(field.payload_bytes)
            &&& next.key_position == map_source_position(frame, field.encoded_start)
            &&& next.key_source == map_source_id(frame, field.encoded_start)
            &&& admission::utf8_valid_at(field.payload_bytes, 0)
            &&& field.payload_bytes.len() > 0
        },
    },
{}

pub proof fn map_key_field_is_shared_witness(
    frame: MapFrameModel, auxiliary: admission::AdmissionFrameState,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        map_frame_wf(frame),
        map_aux_projection(frame, auxiliary),
        map_key_step(frame, field).is_some(),
    ensures admission::admission_map_key_field(auxiliary, field.encoded_end),
{
    assert(map_field_at_cursor(frame, field));
    assert(admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: auxiliary.region_end,
            next: auxiliary.cursor,
            source_bytes: auxiliary.source_bytes,
        }, field,
    ));
    assert(exists|candidate: admission::IndexedCanonicalFieldModel|
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: auxiliary.region_end,
                next: auxiliary.cursor,
                source_bytes: auxiliary.source_bytes,
            }, candidate,
        )
            && candidate.number == 1
            && candidate.wire == 2
            && candidate.encoded_end == field.encoded_end
            && candidate.payload_bytes.len() > 0
            && admission::utf8_valid_at(candidate.payload_bytes, 0));
}

pub proof fn map_value_push_exact(
    frame: MapFrameModel, field: admission::IndexedCanonicalFieldModel,
    obligation_id: u64,
)
    ensures match map_value_push_step(frame, field, obligation_id) {
        None => true,
        Some((next, pending)) => {
            &&& map_frame_wf(next)
            &&& next.cursor == field.encoded_end
            &&& next.last_tag == 2
            &&& next.pending == Some(pending)
            &&& pending.payload_start == field.payload_start
            &&& pending.payload_end == field.payload_end
            &&& pending.child_type_id == frame.value_type_id
            &&& pending.id == obligation_id
            &&& pending.field_position == map_source_position(
                frame, field.encoded_start,
            ).unwrap()
            &&& pending.field_source == map_source_id(
                frame, field.encoded_start,
            ).unwrap()
        },
    },
{}

pub proof fn map_value_field_is_shared_witness(
    frame: MapFrameModel, auxiliary: admission::AdmissionFrameState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState, obligation_id: u64,
)
    requires
        map_frame_wf(frame),
        map_aux_projection(frame, auxiliary),
        map_value_push_step(frame, field, obligation_id).is_some(),
        child.region_end as int
            == field.payload_end as int - field.payload_start as int,
        child.source_bytes == field.payload_bytes,
    ensures admission::admission_map_value_field(
        auxiliary, field.encoded_end, child,
    ),
{
    assert(map_field_at_cursor(frame, field));
    assert(admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: auxiliary.region_end,
            next: auxiliary.cursor,
            source_bytes: auxiliary.source_bytes,
        }, field,
    ));
    assert(exists|candidate: admission::IndexedCanonicalFieldModel|
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: auxiliary.region_end,
                next: auxiliary.cursor,
                source_bytes: auxiliary.source_bytes,
            }, candidate,
        )
            && candidate.number == 2
            && candidate.wire == 2
            && candidate.encoded_end == field.encoded_end
            && child.region_end as int
                == candidate.payload_end as int - candidate.payload_start as int
            && child.source_bytes == candidate.payload_bytes);
}

pub proof fn map_value_accept_exact(
    frame: MapFrameModel, returned_id: u64,
    child: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
)
    ensures match map_value_accept_step(frame, returned_id, child, arena) {
        None => true,
        Some(next) => {
            &&& map_frame_wf(next)
            &&& next.pending.is_none()
            &&& next.value.is_some()
            &&& next.value.unwrap().score_handle == child.score_handle.unwrap()
            &&& next.value.unwrap().semantic_default == false
            &&& next.value.unwrap().child_body_id == child.body_id
            &&& next.value.unwrap().score_origin_body_id
                == arena[child.score_handle.unwrap() as int].body_id
        },
    },
{}

pub proof fn map_finish_exact(frame: MapFrameModel)
    ensures match map_finish(frame) {
        None => true,
        Some(summary) => {
            &&& frame.cursor == frame.region_end
            &&& frame.pending.is_none()
            &&& summary.value == frame.value
            &&& summary.key_bytes == frame.key.unwrap_or(Seq::empty())
            &&& summary.key_position
                == frame.key_position.unwrap_or(frame.physical_start as int)
            &&& summary.key_source
                == frame.key_source.unwrap_or(frame.physical_source)
            &&& (!frame.saw_value || summary.value.is_some())
        },
    },
{}

pub proof fn map_finish_source_bound(frame: MapFrameModel)
    ensures match map_finish(frame) {
        None => true,
        Some(summary) => map_summary_source_projection(frame, summary),
    },
{}

pub proof fn map_finish_implies_shared_summary(
    frame: MapFrameModel, auxiliary: admission::AdmissionFrameState,
)
    requires
        map_aux_projection(frame, auxiliary),
        map_finish(frame).is_some(),
    ensures admission::admission_map_summary_complete(auxiliary),
{
    let summary = map_finish(frame).unwrap();
    map_finish_source_bound(frame);
    map_summary_matches_shared_source(frame, auxiliary, summary);
    assert(map_frame_wf(frame));
    assert(frame.cursor == frame.region_end);
    assert(frame.pending.is_none());
    assert(admission::admission_map_source_matches(auxiliary, summary.key_bytes));
    assert(exists|key_bytes: Seq<u8>|
        admission::admission_map_source_matches(auxiliary, key_bytes));
    assert(auxiliary.seen.contains(0) == auxiliary.scalar_slots.contains(0));
    if auxiliary.seen.contains(1) {
        assert(frame.saw_value);
        assert(frame.value.is_some());
        assert(frame.last_tag == 2);
        assert(auxiliary.last_slot == Some(1));
        assert(auxiliary.returns.len() == 1);
        assert(auxiliary.returns[0].slot == 1);
        assert(auxiliary.returns[0].tag == 2);
        assert(auxiliary.returns[0].kind
            == admission::AdmissionReturnedKind::Par {
                semantic_default: false,
                canonical_path_stable: frame.value.unwrap().canonical_path_stable,
            });
    } else {
        assert(!frame.saw_value);
        assert(frame.value.is_none());
        assert(auxiliary.returns.len() == 0);
        if auxiliary.seen.contains(0) {
            assert(frame.key.is_some());
            assert(frame.last_tag == 1);
            assert(auxiliary.last_slot == Some(0));
        } else {
            assert(frame.key.is_none());
            assert(frame.last_tag == 0);
            assert(auxiliary.last_slot.is_none());
        }
    }
    assert(admission::admission_map_summary_complete(auxiliary));
}

pub proof fn map_varint_prefix_width_unique(
    source: Seq<u8>, start: int, left: usize, right: usize,
    left_value: u64, right_value: u64,
)
    requires
        0 <= start,
        start + left as int <= source.len(),
        start + right as int <= source.len(),
        admission::canonical_varint_bytes(
            source.subrange(start, start + left as int), left_value,
        ),
        admission::canonical_varint_bytes(
            source.subrange(start, start + right as int), right_value,
        ),
    ensures left == right,
{
    let left_bytes = source.subrange(start, start + left as int);
    let right_bytes = source.subrange(start, start + right as int);
    if left < right {
        let index = left as int - 1;
        assert(0 <= index < left_bytes.len());
        assert(index < right_bytes.len() - 1);
        assert(left_bytes[index] == source[start + index]);
        assert(right_bytes[index] == source[start + index]);
        assert(left_bytes[index] < 128);
        assert(right_bytes[index] >= 128);
    } else if right < left {
        let index = right as int - 1;
        assert(0 <= index < right_bytes.len());
        assert(index < left_bytes.len() - 1);
        assert(left_bytes[index] == source[start + index]);
        assert(right_bytes[index] == source[start + index]);
        assert(right_bytes[index] < 128);
        assert(left_bytes[index] >= 128);
    }
}

pub proof fn map_indexed_key_payload_unique(
    source: Seq<u8>, region_end: usize,
    left: admission::IndexedCanonicalFieldModel,
    right: admission::IndexedCanonicalFieldModel,
)
    requires
        source.len() == region_end as int,
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0, region_end, next: 0, source_bytes: source,
            }, left,
        ),
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0, region_end, next: 0, source_bytes: source,
            }, right,
        ),
        left.number == 1,
        left.wire == 2,
        right.number == 1,
        right.wire == 2,
    ensures left.payload_bytes == right.payload_bytes,
{
    map_varint_prefix_width_unique(
        source, 0, left.key_width, right.key_width, 10, 10,
    );
    assert(left.key_width == right.key_width);
    map_varint_prefix_width_unique(
        source, left.key_width as int, left.payload_width,
        right.payload_width, left.payload_length, right.payload_length,
    );
    assert(left.payload_width == right.payload_width);
    assert(left.payload_start == right.payload_start);
    assert(left.payload_length == right.payload_length);
    assert(left.payload_end == right.payload_end);
}

pub proof fn map_source_key_unique(
    frame: admission::AdmissionFrameState, first: Seq<u8>, second: Seq<u8>,
)
    requires
        admission::admission_map_source_matches(frame, first),
        admission::admission_map_source_matches(frame, second),
    ensures first == second,
{
    if frame.seen.contains(0) {
        map_source_key_full_exists(frame, first);
        map_source_key_full_exists(frame, second);
        assert(exists|field: admission::IndexedCanonicalFieldModel|
            map_source_key_full_witness(frame, first, field));
        assert(exists|field: admission::IndexedCanonicalFieldModel|
            map_source_key_full_witness(frame, second, field));
        let first_field = choose|field: admission::IndexedCanonicalFieldModel|
            map_source_key_full_witness(frame, first, field);
        let second_field = choose|field: admission::IndexedCanonicalFieldModel|
            map_source_key_full_witness(frame, second, field);
        assert(map_source_key_witness(frame, first, first_field));
        assert(map_source_key_witness(frame, second, second_field));
        map_indexed_key_payload_unique(
            frame.source_bytes, frame.region_end, first_field, second_field,
        );
    }
}

pub proof fn map_source_choice_exact(
    frame: admission::AdmissionFrameState, key_bytes: Seq<u8>,
)
    requires
        admission::admission_map_summary_complete(frame),
        admission::admission_map_source_matches(frame, key_bytes),
    ensures admission::admission_map_key_bytes(frame) == key_bytes,
{
    assert(admission::admission_map_source_matches(
        frame, admission::admission_map_key_bytes(frame),
    ));
    map_source_key_unique(
        frame, key_bytes, admission::admission_map_key_bytes(frame),
    );
}

pub proof fn map_summary_matches_shared_source(
    frame: MapFrameModel, auxiliary: admission::AdmissionFrameState,
    summary: MapSummaryModel,
)
    requires
        map_aux_projection(frame, auxiliary),
        map_summary_source_projection(frame, summary),
    ensures admission::admission_map_source_matches(auxiliary, summary.key_bytes),
{
    match frame.key_field {
        Some(key_field) => {
            assert(auxiliary.seen.contains(0));
            assert(summary.key_bytes == key_field.payload_bytes);
            assert(admission::indexed_field_at_cursor(
                admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: auxiliary.region_end,
                    next: 0,
                    source_bytes: auxiliary.source_bytes,
                }, key_field,
            ));
            assert(map_source_key_witness(
                auxiliary, summary.key_bytes, key_field,
            ));
            match frame.value_field {
                Some(value_field) => {
                    assert(auxiliary.seen.contains(1));
                    assert(admission::indexed_field_at_cursor(
                        admission::IndexedCanonicalFieldCursorModel {
                            region_start: 0,
                            region_end: auxiliary.region_end,
                            next: key_field.encoded_end,
                            source_bytes: auxiliary.source_bytes,
                        }, value_field,
                    ));
                    assert(value_field.number == 2);
                    assert(value_field.wire == 2);
                    assert(value_field.encoded_end == auxiliary.region_end);
                    assert(exists|field: admission::IndexedCanonicalFieldModel|
                        admission::indexed_field_at_cursor(
                            admission::IndexedCanonicalFieldCursorModel {
                                region_start: 0,
                                region_end: auxiliary.region_end,
                                next: key_field.encoded_end,
                                source_bytes: auxiliary.source_bytes,
                            }, field,
                        )
                            && field.number == 2
                            && field.wire == 2
                            && field.encoded_end == auxiliary.region_end);
                },
                None => {
                    assert(!auxiliary.seen.contains(1));
                    assert(key_field.encoded_end == auxiliary.region_end);
                },
            }
            assert(map_source_key_full_witness(
                auxiliary, summary.key_bytes, key_field,
            ));
            assert(exists|field: admission::IndexedCanonicalFieldModel|
                map_source_key_full_witness(auxiliary, summary.key_bytes, field));
        },
        None => {
            assert(!auxiliary.seen.contains(0));
            assert(summary.key_bytes == Seq::<u8>::empty());
            match frame.value_field {
                Some(value_field) => {
                    assert(auxiliary.seen.contains(1));
                    assert(admission::indexed_field_at_cursor(
                        admission::IndexedCanonicalFieldCursorModel {
                            region_start: 0,
                            region_end: auxiliary.region_end,
                            next: 0,
                            source_bytes: auxiliary.source_bytes,
                        }, value_field,
                    ));
                    assert(value_field.number == 2);
                    assert(value_field.wire == 2);
                    assert(value_field.encoded_end == auxiliary.region_end);
                    assert(exists|field: admission::IndexedCanonicalFieldModel|
                        admission::indexed_field_at_cursor(
                            admission::IndexedCanonicalFieldCursorModel {
                                region_start: 0,
                                region_end: auxiliary.region_end,
                                next: 0,
                                source_bytes: auxiliary.source_bytes,
                            }, field,
                        )
                            && field.number == 2
                            && field.wire == 2
                            && field.encoded_end == auxiliary.region_end);
                },
                None => {
                    assert(!auxiliary.seen.contains(1));
                    assert(auxiliary.region_end == 0);
                },
            }
        },
    }
    assert(admission::admission_map_source_matches(
        auxiliary, summary.key_bytes,
    ));
}

pub proof fn map_admission_entry_complete_exact(
    frame: MapFrameModel, state: admission::AdmissionState,
)
    ensures match map_admission_entry_complete(frame, state) {
        None => true,
        Some((summary, next)) => {
            &&& map_finish(frame) == Some(summary)
            &&& map_summary_source_projection(frame, summary)
            &&& admission::admission_map_source_matches(
                state.frames[state.frames.len() - 1], summary.key_bytes,
            )
            &&& admission::admission_map_summary_complete(
                state.frames[state.frames.len() - 1]
            )
            &&& admission::concrete_derived_summary(
                state.frames[state.frames.len() - 2],
                state.frames[state.frames.len() - 1],
            ) == Some(admission::AdmissionReturnedKind::Map {
                key_bytes: summary.key_bytes,
                value_score_handle: admission::admission_map_value_score_handle(
                    state.frames[state.frames.len() - 1],
                ),
            })
            &&& admission::admission_complete_child_step(
                state,
                state.frames[state.frames.len() - 1].return_id.unwrap(),
                2, admission::AdmissionReturnedKind::Map {
                    key_bytes: summary.key_bytes,
                    value_score_handle: admission::admission_map_value_score_handle(
                        state.frames[state.frames.len() - 1],
                    ),
                },
            ) == Some(next)
        },
    },
{
    if let Some((summary, _)) = map_admission_entry_complete(frame, state) {
        let map_child = state.frames[state.frames.len() - 1];
        assert(admission::admission_map_summary_complete(map_child));
        assert(admission::admission_map_source_matches(
            map_child, summary.key_bytes,
        ));
        map_source_choice_exact(map_child, summary.key_bytes);
    }
}

pub proof fn map_complete_gate_exact(frame: MapFrameModel)
    ensures map_finish(frame).is_some() ==> {
        &&& map_frame_wf(frame)
        &&& frame.cursor == frame.region_end
        &&& frame.pending.is_none()
        &&& (!frame.saw_value || frame.value.is_some())
    },
{}

pub proof fn map_pending_rejects_finish(frame: MapFrameModel)
    requires frame.pending.is_some(),
    ensures map_finish(frame).is_none(),
{}

pub proof fn map_unaccepted_value_rejects_finish(frame: MapFrameModel)
    requires frame.saw_value, frame.value.is_none(),
    ensures map_finish(frame).is_none(),
{}

pub proof fn map_eof_snapshot_rejects_ineligible(
    frame: MapFrameModel, terminal_error_free: bool,
)
    ensures
        !terminal_error_free || frame.cursor != frame.region_end
            || frame.pending.is_some() || (frame.saw_value && frame.value.is_none())
            ==> map_eof_snapshot(frame, terminal_error_free).is_none(),
{}

pub proof fn map_eof_checked_complete_source_bound(
    frame: MapFrameModel, terminal_error_free: bool,
    produced: MapEofReceiptModel, returned: MapEofReturnedModel,
)
    requires map_eof_checked_complete(frame, terminal_error_free, produced, returned),
    ensures
        terminal_error_free,
        map_frame_wf(frame),
        frame.cursor == frame.region_end,
        frame.pending.is_none(),
        produced.owner_type_id == frame.owner_type_id,
        produced.outer_field == frame.outer_field,
        produced.value_type_id == frame.value_type_id,
        produced.physical_start == frame.physical_start,
        produced.physical_source == frame.physical_source,
        produced.region_end == frame.region_end,
        produced.source_bytes == frame.source_bytes,
        produced.source_image == frame.source_image,
        produced.cursor == frame.cursor,
        produced.key_present == frame.key.is_some(),
        produced.key_position == frame.key_position,
        produced.key_source == frame.key_source,
        produced.saw_value == frame.saw_value,
        produced.value_position == frame.value_position,
        produced.value_source == frame.value_source,
        map_finish(frame) == Some(produced.summary),
        map_summary_source_projection(frame, produced.summary),
        match returned {
            MapEofReturnedModel::Root { summary }
            | MapEofReturnedModel::AcceptedMessage { summary } =>
                summary == produced.summary,
            MapEofReturnedModel::AcceptedOther => false,
        },
{
    let before = map_eof_snapshot(frame, terminal_error_free).unwrap();
    assert(map_finish(frame) == Some(before.summary));
    assert(map_eof_receipts_match(before, produced));
    assert(before.summary == produced.summary);
    map_finish_source_bound(frame);
}

pub proof fn map_eof_checked_complete_rejects_other(
    frame: MapFrameModel, terminal_error_free: bool,
    produced: MapEofReceiptModel,
)
    ensures !map_eof_checked_complete(
        frame, terminal_error_free, produced, MapEofReturnedModel::AcceptedOther,
    ),
{}

pub proof fn map_eof_checked_complete_rejects_forged_owner(
    frame: MapFrameModel, terminal_error_free: bool,
    produced: MapEofReceiptModel, returned: MapEofReturnedModel,
)
    requires produced.owner_type_id != frame.owner_type_id,
    ensures !map_eof_checked_complete(frame, terminal_error_free, produced, returned),
{}

pub proof fn map_eof_checked_complete_rejects_forged_region_or_cursor(
    frame: MapFrameModel, terminal_error_free: bool,
    produced: MapEofReceiptModel, returned: MapEofReturnedModel,
)
    requires
        produced.source_bytes != frame.source_bytes
            || produced.source_image != frame.source_image
            || produced.physical_start != frame.physical_start
            || produced.cursor != frame.cursor,
    ensures !map_eof_checked_complete(frame, terminal_error_free, produced, returned),
{}

pub proof fn map_eof_checked_complete_rejects_forged_summary(
    frame: MapFrameModel, terminal_error_free: bool,
    produced: MapEofReceiptModel, returned: MapEofReturnedModel,
)
    requires map_finish(frame).is_some(), produced.summary != map_finish(frame).unwrap(),
    ensures !map_eof_checked_complete(frame, terminal_error_free, produced, returned),
{}

pub proof fn map_cursor_eof_has_checked_source_receipt(frame: MapFrameModel)
    ensures match map_cursor_step(frame, None, 0) {
        Some(MapOutcomeModel::Completed { summary }) => {
            &&& map_eof_snapshot(frame, true).is_some()
            &&& map_eof_snapshot(frame, true).unwrap().summary == summary
            &&& map_eof_checked_complete(
                frame, true, map_eof_snapshot(frame, true).unwrap(),
                MapEofReturnedModel::Root { summary },
            )
            &&& map_eof_checked_complete(
                frame, true, map_eof_snapshot(frame, true).unwrap(),
                MapEofReturnedModel::AcceptedMessage { summary },
            )
            &&& !map_eof_checked_complete(
                frame, false, map_eof_snapshot(frame, true).unwrap(),
                MapEofReturnedModel::Root { summary },
            )
        },
        _ => true,
    },
{}

pub proof fn map_decreasing_key_rejected(
    frame: MapFrameModel, field: admission::IndexedCanonicalFieldModel,
)
    requires frame.last_tag == 2, field.number == 1,
    ensures map_key_step(frame, field).is_none(),
{}

}

fn main() {}
