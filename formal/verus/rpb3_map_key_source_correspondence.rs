//! Source-observed map-key Continue, with an independently selected wire field.
//!
//! `map_step` obtains a field from `IndexedCanonicalFieldCursor::next_field`,
//! validates its length-delimited key with `IndexedByteRegion::validate_utf8`,
//! and checks `MapFieldObservation::matches_frame` before returning Continue.
//! This module derives the MapKey model successor from the indexed cursor and
//! the observed scalar checks. The source image, successful Rust decoder and
//! UTF-8 results, and pre-frame projection are extraction premises; in
//! particular this does not claim that the whole Rust post-frame is extracted.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_map_frame_refinement as map,
    rpb3_wire_cursor_source_refinement as wire,
};

verus! {

/// One UTF-8 sequence width, using the byte ranges in
/// `IndexedByteReader::read_utf8`. Zero means that validation rejects here.
pub open spec fn map_key_utf8_width(bytes: Seq<u8>, at: int) -> int {
    if at < 0 || at >= bytes.len() { 0 }
    else {
        let first = bytes[at];
        if first <= 0x7f { 1 }
        else if 0xc2 <= first <= 0xdf
            && at + 1 < bytes.len()
            && 0x80 <= bytes[at + 1] <= 0xbf { 2 }
        else if first == 0xe0
            && at + 2 < bytes.len()
            && 0xa0 <= bytes[at + 1] <= 0xbf
            && 0x80 <= bytes[at + 2] <= 0xbf { 3 }
        else if ((0xe1 <= first <= 0xec) || (0xee <= first <= 0xef))
            && at + 2 < bytes.len()
            && 0x80 <= bytes[at + 1] <= 0xbf
            && 0x80 <= bytes[at + 2] <= 0xbf { 3 }
        else if first == 0xed
            && at + 2 < bytes.len()
            && 0x80 <= bytes[at + 1] <= 0x9f
            && 0x80 <= bytes[at + 2] <= 0xbf { 3 }
        else if first == 0xf0
            && at + 3 < bytes.len()
            && 0x90 <= bytes[at + 1] <= 0xbf
            && 0x80 <= bytes[at + 2] <= 0xbf
            && 0x80 <= bytes[at + 3] <= 0xbf { 4 }
        else if 0xf1 <= first <= 0xf3
            && at + 3 < bytes.len()
            && 0x80 <= bytes[at + 1] <= 0xbf
            && 0x80 <= bytes[at + 2] <= 0xbf
            && 0x80 <= bytes[at + 3] <= 0xbf { 4 }
        else if first == 0xf4
            && at + 3 < bytes.len()
            && 0x80 <= bytes[at + 1] <= 0x8f
            && 0x80 <= bytes[at + 2] <= 0xbf
            && 0x80 <= bytes[at + 3] <= 0xbf { 4 }
        else { 0 }
    }
}

pub open spec fn map_key_utf8_scan(bytes: Seq<u8>, at: int) -> bool
    decreases bytes.len() - at,
{
    if at == bytes.len() { true }
    else if at < 0 || at > bytes.len() { false }
    else {
        let width = map_key_utf8_width(bytes, at);
        width > 0 && map_key_utf8_scan(bytes, at + width)
    }
}

/// The source-oriented width scan and the admission model accept exactly the
/// same complete UTF-8 byte sequences, including restricted E0/ED/F0/F4.
pub proof fn map_key_utf8_scan_matches_admission(bytes: Seq<u8>, at: int)
    requires 0 <= at <= bytes.len(),
    ensures map_key_utf8_scan(bytes, at) == admission::utf8_valid_at(bytes, at),
    decreases bytes.len() - at,
{
    if at < bytes.len() {
        let width = map_key_utf8_width(bytes, at);
        if width > 0 {
            map_key_utf8_scan_matches_admission(bytes, at + width);
        }
    }
}

/// Values checked by the production MapFieldObservation after a successful
/// key field. The `field` itself is selected by the modeled indexed cursor,
/// not supplied as an arbitrary post-state field.
pub struct MapKeySourceObservation {
    pub field_number: u32,
    pub wire: u8,
    pub encoded_start: usize,
    pub encoded_end: usize,
    pub payload_len: usize,
    pub field_source: usize,
    pub field_position: usize,
    pub payload_source: usize,
    pub payload_position: usize,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub fields_before: usize,
    pub fields_after: usize,
    pub key_absent_before: bool,
    pub waiting_absent_before: bool,
    pub key_same_view_after: bool,
    pub key_nonempty_after: bool,
    pub utf8_accepted: bool,
}

/// Rust-to-model extraction boundary. The indexed image in `before` must
/// represent the actual retained MapFrame region and `cursor` must represent
/// the actual `IndexedCanonicalFieldCursor`; these are not inferred here from
/// the booleans in the observation.
pub open spec fn map_key_source_ready(
    before: map::MapFrameModel,
    cursor: wire::WireRuntimeCursorImage,
    observed: MapKeySourceObservation,
) -> bool {
    if !map::map_frame_wf(before)
        || before.pending.is_some()
        || before.key.is_some()
        || !observed.key_absent_before
        || !observed.waiting_absent_before
        || !observed.key_same_view_after
        || !observed.key_nonempty_after
        || !observed.utf8_accepted
        || wire::wire_runtime_derive_cursor(cursor).is_none()
        || !wire::wire_concrete_cursor_wf(
            wire::wire_runtime_derive_cursor(cursor).unwrap(),
        )
        || wire::wire_runtime_cursor_project(cursor).is_none()
    { false } else {
        let source = wire::wire_runtime_cursor_project(cursor).unwrap();
        &&& source.cursor.region_start == 0
        &&& source.cursor.region_end == before.region_end
        &&& source.cursor.next == before.cursor
        &&& source.cursor.source_bytes == before.source_bytes
        &&& source.last_field == before.last_tag
        &&& source.enforce_numeric_order
        &&& observed.cursor_before == before.cursor
        &&& observed.fields_before == cursor.fields_read
        &&& match wire::wire_runtime_next_field(cursor) {
            Some(wire::WireConcreteTransition::Field { state: next, field, start }) => {
                &&& field.number == 1
                &&& field.wire == 2
                &&& field.payload_bytes.len() > 0
                &&& map_key_utf8_scan(field.payload_bytes, 0)
                &&& observed.field_number == field.number
                &&& observed.wire == field.wire
                &&& start.logical_offset == field.encoded_start
                &&& !start.end
                &&& observed.encoded_start == field.encoded_start
                &&& observed.encoded_end == field.encoded_end
                &&& observed.payload_len as int == field.payload_bytes.len()
                &&& map::map_source_id(before, field.payload_start).is_some()
                &&& map::map_source_position(before, field.payload_start).is_some()
                &&& observed.field_source == start.physical.source as usize
                &&& observed.field_position == start.physical.offset
                &&& observed.payload_source
                    == map::map_source_id(before, field.payload_start).unwrap()
                &&& observed.payload_position as int
                    == map::map_source_position(before, field.payload_start).unwrap()
                &&& Some(observed.field_source)
                    == map::map_source_id(before, field.encoded_start)
                &&& Some(observed.field_position as int)
                    == map::map_source_position(before, field.encoded_start)
                &&& observed.cursor_after == field.encoded_end
                &&& observed.cursor_after == next.reader_at
                &&& observed.fields_after == next.fields_read
                &&& observed.fields_before as int + 1
                    == observed.fields_after as int
            },
            _ => false,
        }
    }
}

/// This selector computes a MapFrame successor from the actual modeled wire
/// read. It never takes a caller-provided successor.
pub open spec fn map_key_source_step(
    before: map::MapFrameModel,
    cursor: wire::WireRuntimeCursorImage,
    observed: MapKeySourceObservation,
) -> Option<map::MapFrameModel> {
    if !map_key_source_ready(before, cursor, observed) { None }
    else {
        match wire::wire_runtime_next_field(cursor) {
            Some(wire::WireConcreteTransition::Field { field, .. }) =>
                map::map_key_step(before, field),
            _ => None,
        }
    }
}

pub proof fn map_key_source_derives_continue(
    before: map::MapFrameModel,
    cursor: wire::WireRuntimeCursorImage,
    observed: MapKeySourceObservation,
)
    requires map_key_source_ready(before, cursor, observed),
    ensures match map_key_source_step(before, cursor, observed) {
        Some(after) => {
            &&& map::map_frame_wf(after)
            &&& after.cursor == observed.cursor_after
            &&& after.last_tag == 1
            &&& after.key.is_some()
            &&& after.key.unwrap().len() == observed.payload_len as int
            &&& after.key_position == Some(observed.field_position as int)
            &&& after.key_source == Some(observed.field_source)
            &&& after.saw_value == before.saw_value
            &&& after.pending == before.pending
            &&& after.value == before.value
            &&& after.accepted_arena == before.accepted_arena
            &&& after.source_bytes == before.source_bytes
            &&& after.source_image == before.source_image
        },
        None => false,
    },
{
    let source = wire::wire_runtime_cursor_project(cursor).unwrap();
    wire::wire_runtime_next_field_from_source(cursor);
    if let Some(wire::WireConcreteTransition::Field { field, .. }) =
        wire::wire_runtime_next_field(cursor)
    {
        wire::wire_source_next_field_refines(source);
        assert(admission::indexed_field_at_cursor(source.cursor, field));
        assert(map::map_field_at_cursor(before, field));
        map_key_utf8_scan_matches_admission(field.payload_bytes, 0);
        assert(admission::utf8_valid_at(field.payload_bytes, 0));
        assert(map::map_key_step(before, field).is_some());
        map::map_key_step_exact(before, field);
    }
}

} // verus!
