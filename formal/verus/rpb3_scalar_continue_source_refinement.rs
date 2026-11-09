//! Source projections for root initialization and scalar Continue.
//!
//! The observation corresponds to `MessageFieldObservation` after an indexed
//! field read and `record_scalar`. Its indexed source positions represent
//! `AdmissionRegion::position_at` for every logical byte, including fragmented
//! backing. This file proves only conditional local refinements:
//! the Rust-to-Verus projection and enabledness of the generated descriptor
//! still need an independent source-level binding before the integrated gate
//! can use it for criterion 5935.
use vstd::prelude::*;

#[path = "rpb3_generated_descriptor_projection.rs"]
mod generated;

#[cfg(not(rpb3_integrated))]
mod rpb3_admission_cursor;
#[cfg(not(rpb3_integrated))]
use rpb3_admission_cursor as admission;

#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;

verus! {

/// Independently observed facts at the first driver push. Physical positions
/// may be discontinuous; no root frame is supplied as an assumption.
pub struct RootInitSourceObservation {
    pub ghost source: admission::AdmissionIndexedSourceImage,
    pub ghost body_bytes: Seq<u8>,
    pub ghost body_positions: Seq<admission::AdmissionSourceAnchor>,
    pub body_start: admission::AdmissionSourceAnchor,
    pub body_end: admission::AdmissionSourceAnchor,
    pub program_type_id: u16,
    pub program_identity: usize,
    pub generated_program_identity: usize,
    pub ghost family: admission::AdmissionFamily,
    pub slot_count: usize,
    pub ghost required: Set<int>,
    pub cursor_initial: usize,
    pub last_slot_initial: Option<usize>,
    pub ghost seen_initial: Set<int>,
    pub ghost scalar_slots_initial: Set<int>,
    pub ghost scalar_occurrences_initial: Seq<admission::AdmissionScalarOccurrence>,
    pub ghost returns_initial: Seq<admission::AdmissionReturnedSummary>,
    pub ghost score_arena_initial: Seq<admission::AdmissionScoreBinding>,
    pub stack_len_before: usize,
    pub stack_len_after: usize,
}

pub open spec fn root_init_source_projected_frame(
    observed: RootInitSourceObservation,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: observed.body_start.source,
        physical_start: observed.body_start.offset,
        program_type_id: observed.program_type_id,
        region_start: 0,
        region_end: observed.body_bytes.len() as usize,
        source_bytes: observed.source.bytes,
        cursor: observed.cursor_initial,
        slot_count: observed.slot_count,
        last_slot: observed.last_slot_initial,
        seen: observed.seen_initial,
        required: observed.required,
        scalar_slots: observed.scalar_slots_initial,
        scalar_occurrences: observed.scalar_occurrences_initial,
        returns: observed.returns_initial,
        family: observed.family,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind: 1,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

/// The specialized formal families are an exact inverse of their generated
/// type IDs. Other covers raw Par, wrappers, and structural message programs.
pub open spec fn root_init_expected_family(type_id: u16) -> admission::AdmissionFamily {
    match type_id {
        8 => admission::AdmissionFamily::Bundle,
        9 => admission::AdmissionFamily::CostSignatureFrame,
        12 => admission::AdmissionFamily::CostRegionFrame,
        13 => admission::AdmissionFamily::CostTokenStack,
        15 => admission::AdmissionFamily::Send,
        16 => admission::AdmissionFamily::ReceiveBindFrame,
        19 => admission::AdmissionFamily::Receive,
        20 => admission::AdmissionFamily::New,
        21 => admission::AdmissionFamily::MatchCaseFrame,
        22 => admission::AdmissionFamily::Match,
        23 => admission::AdmissionFamily::If,
        24 => admission::AdmissionFamily::Expr,
        25 => admission::AdmissionFamily::BigRationalFrame,
        26 => admission::AdmissionFamily::FixedPointFrame,
        35 => admission::AdmissionFamily::EVarFrame,
        55 => admission::AdmissionFamily::Connective,
        60 => admission::AdmissionFamily::GUnforgeable,
        _ => admission::AdmissionFamily::Other,
    }
}

/// Source facts are separate from the target root-initial predicate. Binding
/// these fields to every Rust root push remains a production extraction task.
pub open spec fn root_init_source_matches(
    observed: RootInitSourceObservation,
) -> bool {
    &&& admission::admission_indexed_source_wf(observed.source)
    &&& observed.source.bytes == observed.body_bytes
    &&& observed.source.positions == observed.body_positions
    &&& observed.source.end == observed.body_end
    &&& observed.body_bytes.len() <= usize::MAX as int
    &&& admission::admission_indexed_source_position(observed.source, 0)
        == Some(observed.body_start)
    &&& observed.program_identity == observed.generated_program_identity
    &&& generated::generated_slot_count(observed.program_type_id)
        == Some(observed.slot_count)
    &&& observed.family == root_init_expected_family(observed.program_type_id)
    &&& match admission::admission_family_program_id(observed.family) {
        Some(id) => id == observed.program_type_id,
        None => true,
    }
    &&& match admission::admission_family_slot_count(observed.family) {
        Some(count) => count == observed.slot_count,
        None => true,
    }
    &&& forall|slot: int| 0 <= slot < observed.slot_count as int
        ==> observed.required.contains(slot) == (exists|tag: u32|
            #[trigger] admission::admission_required_descriptor_slot(
                observed.program_type_id, tag, slot,
            ))
    &&& forall|slot: int| observed.required.contains(slot)
        ==> 0 <= slot < observed.slot_count as int
    &&& observed.cursor_initial == 0
    &&& observed.last_slot_initial.is_none()
    &&& observed.seen_initial == Set::empty()
    &&& observed.scalar_slots_initial == Set::empty()
    &&& observed.scalar_occurrences_initial.len() == 0
    &&& observed.returns_initial.len() == 0
    &&& observed.score_arena_initial == Seq::empty()
    &&& observed.stack_len_before == 0
    &&& observed.stack_len_after == 1
}

pub proof fn root_init_source_derives_initial_state(
    observed: RootInitSourceObservation,
)
    requires root_init_source_matches(observed),
    ensures
        admission::admission_root_initial(root_init_source_projected_frame(observed)),
        admission::admission_state_wf(admission::admission_initial_state(
            root_init_source_projected_frame(observed),
        )),
        admission::admission_initial_state(root_init_source_projected_frame(observed))
            .arena == observed.score_arena_initial,
        admission::admission_initial_state(root_init_source_projected_frame(observed))
            .frames.len() == observed.stack_len_after as int,
{
    let root = root_init_source_projected_frame(observed);
    assert(root.required == observed.required);
    assert(root.program_type_id == observed.program_type_id);
    assert(root.slot_count == observed.slot_count);
    assert(forall|slot: int| 0 <= slot < observed.slot_count as int
        ==> observed.required.contains(slot) == (exists|tag: u32|
            #[trigger] admission::admission_required_descriptor_slot(
                observed.program_type_id, tag, slot,
            )));
    assert(admission::admission_required_slots_generated(root)) by {
        assert forall|slot: int| 0 <= slot < root.slot_count as int
            implies root.required.contains(slot) == (exists|tag: u32|
                #[trigger] admission::admission_required_descriptor_slot(
                    root.program_type_id, tag, slot,
                )) by {
            assert(observed.required.contains(slot) == (exists|tag: u32|
                #[trigger] admission::admission_required_descriptor_slot(
                    observed.program_type_id, tag, slot,
                )));
        }
    }
    assert(admission::admission_frame_wf(root));
    assert(admission::admission_root_initial(root));
    admission::admission_initial_wf(root);
}

/// A concrete empty generated message shows the premise is satisfiable. It
/// is a model witness, not an extracted Rust execution.
pub open spec fn root_init_empty_generated_witness() -> RootInitSourceObservation {
    let anchor = admission::AdmissionSourceAnchor { source: 0, offset: 0 };
    RootInitSourceObservation {
        source: admission::AdmissionIndexedSourceImage {
            bytes: Seq::empty(), positions: Seq::empty(), end: anchor,
        },
        body_bytes: Seq::empty(),
        body_positions: Seq::empty(),
        body_start: anchor,
        body_end: anchor,
        program_type_id: 7,
        program_identity: 7,
        generated_program_identity: 7,
        family: admission::AdmissionFamily::Other,
        slot_count: 0,
        required: Set::empty(),
        cursor_initial: 0,
        last_slot_initial: None,
        seen_initial: Set::empty(),
        scalar_slots_initial: Set::empty(),
        scalar_occurrences_initial: Seq::empty(),
        returns_initial: Seq::empty(),
        score_arena_initial: Seq::empty(),
        stack_len_before: 0,
        stack_len_after: 1,
    }
}

pub proof fn root_init_empty_generated_witness_is_initial()
    ensures
        root_init_source_matches(root_init_empty_generated_witness()),
        admission::admission_state_wf(admission::admission_initial_state(
            root_init_source_projected_frame(root_init_empty_generated_witness()),
        )),
{
    let observed = root_init_empty_generated_witness();
    assert(root_init_source_matches(observed));
    root_init_source_derives_initial_state(observed);
}

/// Empty Send has six generated slots and no required PathMap field. Unlike
/// the zero-slot witness, this exercises the specialized family projection.
pub open spec fn root_init_empty_send_witness() -> RootInitSourceObservation {
    let anchor = admission::AdmissionSourceAnchor { source: 0, offset: 0 };
    RootInitSourceObservation {
        source: admission::AdmissionIndexedSourceImage {
            bytes: Seq::empty(), positions: Seq::empty(), end: anchor,
        },
        body_bytes: Seq::empty(),
        body_positions: Seq::empty(),
        body_start: anchor,
        body_end: anchor,
        program_type_id: 15,
        program_identity: 15,
        generated_program_identity: 15,
        family: admission::AdmissionFamily::Send,
        slot_count: 6,
        required: Set::empty(),
        cursor_initial: 0,
        last_slot_initial: None,
        seen_initial: Set::empty(),
        scalar_slots_initial: Set::empty(),
        scalar_occurrences_initial: Seq::empty(),
        returns_initial: Seq::empty(),
        score_arena_initial: Seq::empty(),
        stack_len_before: 0,
        stack_len_after: 1,
    }
}

pub proof fn root_init_empty_send_witness_is_initial()
    ensures
        root_init_source_matches(root_init_empty_send_witness()),
        admission::admission_state_wf(admission::admission_initial_state(
            root_init_source_projected_frame(root_init_empty_send_witness()),
        )),
{
    let observed = root_init_empty_send_witness();
    assert forall|tag: u32, slot: int|
        !admission::admission_required_descriptor_slot(15, tag, slot) by {
        assert(match generated::generated_descriptor(15, tag) {
            Some(descriptor) => descriptor.frame_kind != 3,
            None => true,
        });
    }
    assert(root_init_source_matches(observed));
    root_init_source_derives_initial_state(observed);
}

pub enum ScalarSourceValue {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    String(Seq<u8>),
    Bytes(Seq<u8>),
}

pub struct ScalarSourceOccurrence {
    pub variant_token: Option<usize>,
    pub field_source: usize,
    pub field_offset: usize,
    pub ghost value: ScalarSourceValue,
}

pub open spec fn scalar_source_le32(bytes: Seq<u8>) -> u32 {
    if bytes.len() == 4 {
        (bytes[0] as int + bytes[1] as int * 256
            + bytes[2] as int * 65536 + bytes[3] as int * 16777216) as u32
    } else { 0 }
}

pub open spec fn scalar_source_le64(bytes: Seq<u8>) -> u64 {
    if bytes.len() == 8 {
        (bytes[0] as int + bytes[1] as int * 256
            + bytes[2] as int * 65536 + bytes[3] as int * 16777216
            + bytes[4] as int * 4294967296 + bytes[5] as int * 1099511627776
            + bytes[6] as int * 281474976710656
            + bytes[7] as int * 72057594037927936) as u64
    } else { 0 }
}

pub open spec fn scalar_source_typed_value_matches(
    scalar_kind: u8,
    field: admission::IndexedCanonicalFieldModel,
    value: ScalarSourceValue,
) -> bool {
    match (scalar_kind, value) {
        (1, ScalarSourceValue::Bool(actual)) => actual == (field.decoded_varint != 0),
        (2, ScalarSourceValue::I32(actual)) => actual == field.decoded_varint as u32 as i32,
        (3, ScalarSourceValue::I32(actual)) =>
            actual == admission::admission_sint32_value(field.decoded_varint),
        (4, ScalarSourceValue::U32(actual)) => actual == field.decoded_varint as u32,
        (5, ScalarSourceValue::I64(actual)) => actual == field.decoded_varint as i64,
        (6, ScalarSourceValue::I64(actual)) => {
            let shifted = (field.decoded_varint >> 1) as int;
            actual == (if field.decoded_varint & 1 == 0 {
                shifted as i64
            } else {
                (-1int - shifted) as i64
            })
        },
        (7, ScalarSourceValue::U64(actual)) => actual == field.decoded_varint,
        (8, ScalarSourceValue::I32(actual)) =>
            actual == scalar_source_le32(field.payload_bytes) as i32,
        (9, ScalarSourceValue::U32(actual))
            | (10, ScalarSourceValue::U32(actual)) =>
            actual == scalar_source_le32(field.payload_bytes),
        (11, ScalarSourceValue::I64(actual)) =>
            actual == scalar_source_le64(field.payload_bytes) as i64,
        (12, ScalarSourceValue::U64(actual))
            | (13, ScalarSourceValue::U64(actual)) =>
            actual == scalar_source_le64(field.payload_bytes),
        (14, ScalarSourceValue::String(actual))
            | (15, ScalarSourceValue::String(actual)) => actual == field.payload_bytes,
        (16, ScalarSourceValue::Bytes(actual))
            | (17, ScalarSourceValue::Bytes(actual)) => actual == field.payload_bytes,
        _ => false,
    }
}

pub struct ScalarContinueSourceObservation {
    pub ghost source: admission::AdmissionIndexedSourceImage,
    pub ghost body_bytes: Seq<u8>,
    pub ghost body_positions: Seq<admission::AdmissionSourceAnchor>,
    pub body_end: admission::AdmissionSourceAnchor,
    pub ghost field: admission::IndexedCanonicalFieldModel,
    pub program_type_id: u16,
    pub program_identity: usize,
    pub generated_program_identity: usize,
    pub field_identity: usize,
    pub resolved_field_identity: usize,
    pub slot: usize,
    pub repeated: bool,
    pub scalar_kind: u8,
    pub oneof_arm: Option<u32>,
    pub variant_token: Option<usize>,
    pub generated_variant_token: Option<usize>,
    pub ghost scalar_value: ScalarSourceValue,
    pub ghost source_occurrence: ScalarSourceOccurrence,
    pub ghost semantic_slots_before: Seq<Seq<ScalarSourceOccurrence>>,
    pub ghost semantic_slots_after: Seq<Seq<ScalarSourceOccurrence>>,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub last_slot_before: Option<usize>,
    pub seen_before: bool,
    /// `IndexedCanonicalField::field_start.physical` from the Rust field read.
    /// `source.positions` must be built independently from the body region.
    pub field_source: usize,
    pub field_offset: usize,
    pub ghost appended: admission::AdmissionScalarOccurrence,
}

/// Facts required from an independent Rust pre-mutation projection. The
/// future extraction must bind these fields to the indexed read, generated
/// resolution, and appended `SemanticOccurrence`; this predicate alone does
/// not establish that binding. The physical position is indexed, not derived
/// by region-start arithmetic.
#[verifier::opaque]
pub open spec fn scalar_continue_source_before_matches(
    before: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
) -> bool {
    if before.frames.len() == 0 {
        false
    } else {
        let top = before.frames.last();
        let cursor = admission::IndexedCanonicalFieldCursorModel {
            region_start: top.region_start,
            region_end: top.region_end,
            next: top.cursor,
            source_bytes: top.source_bytes,
        };
        &&& top.frame_kind == 1
        &&& top.waiting_id.is_none()
        &&& top.program_type_id == observed.program_type_id
        &&& observed.program_identity == observed.generated_program_identity
        &&& observed.field_identity == observed.resolved_field_identity
        &&& observed.variant_token == observed.generated_variant_token
        &&& match observed.oneof_arm {
            None => observed.variant_token.is_none(),
            Some(tag) => tag == observed.field.number
                && observed.variant_token.is_some() && !observed.repeated,
        }
        &&& scalar_source_typed_value_matches(
            observed.scalar_kind, observed.field, observed.scalar_value,
        )
        &&& observed.source_occurrence == ScalarSourceOccurrence {
            variant_token: observed.variant_token,
            field_source: observed.field_source,
            field_offset: observed.field_offset,
            value: observed.scalar_value,
        }
        &&& observed.semantic_slots_before.len() == top.slot_count as int
        &&& top.cursor == observed.cursor_before
        &&& observed.cursor_after == observed.field.encoded_end
        &&& top.last_slot == observed.last_slot_before
        &&& top.seen.contains(observed.slot as int) == observed.seen_before
        &&& top.score_handle.is_none()
        &&& observed.slot < top.slot_count
        &&& match observed.last_slot_before {
            None => true,
            Some(last) => last <= observed.slot,
        }
        &&& match observed.last_slot_before {
            None => true,
            Some(last) => last != observed.slot || observed.seen_before,
        }
        &&& (observed.repeated || !observed.seen_before)
        &&& !admission::admission_slot_requires_child(top.family, observed.slot)
        &&& admission::admission_field_tag_valid(
            top.family, observed.slot, observed.field.number, false, observed.repeated,
        )
        &&& admission::admission_scalar_descriptor_valid(
            top.program_type_id, observed.field, observed.slot,
            observed.repeated, observed.scalar_kind,
        )
        &&& admission::admission_indexed_source_wf(observed.source)
        &&& observed.source.bytes == observed.body_bytes
        &&& observed.source.positions == observed.body_positions
        &&& observed.source.end == observed.body_end
        &&& observed.source.bytes == top.source_bytes
        &&& admission::indexed_field_at_cursor(cursor, observed.field)
        &&& admission::admission_indexed_source_position(
            observed.source, observed.field.encoded_start,
        ) == Some(admission::AdmissionSourceAnchor {
            source: observed.field_source,
            offset: observed.field_offset,
        })
        &&& observed.appended == admission::AdmissionScalarOccurrence {
            slot: observed.slot,
            tag: observed.field.number,
            scalar_kind: observed.scalar_kind,
            varint_value: observed.field.decoded_varint,
            physical_source: observed.field_source,
            physical_start: observed.field_offset as int,
            bytes: observed.field.payload_bytes,
        }
    }
}

/// Independent projection of the selected Rust frame after `record_scalar`:
/// `last_slot`/`seen` changed at the resolved slot, the cursor consumed one
/// field, and exactly one scalar occurrence was appended. All other frame
/// fields are carried from the pre-mutation source frame.
pub open spec fn scalar_continue_projected_top(
    top: admission::AdmissionFrameState,
    observed: ScalarContinueSourceObservation,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        cursor: observed.cursor_after,
        last_slot: Some(observed.slot),
        seen: top.seen.insert(observed.slot as int),
        scalar_slots: top.scalar_slots.insert(observed.slot as int),
        scalar_occurrences: top.scalar_occurrences.push(observed.appended),
        ..top
    }
}

/// The required post-projection is stated in terms of the selected frame and
/// score storage, not the model step. A later Rust extraction must establish
/// this predicate for an independently observed poststate.
#[verifier::opaque]
pub open spec fn scalar_continue_source_after_matches(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
) -> bool {
    if before.frames.len() == 0
        || observed.slot as int >= observed.semantic_slots_before.len()
        || observed.slot as int >= observed.semantic_slots_after.len()
    {
        false
    } else {
        let top_index = before.frames.len() - 1;
        &&& observed.semantic_slots_after.len() == observed.semantic_slots_before.len()
        &&& observed.semantic_slots_after[observed.slot as int]
            == observed.semantic_slots_before[observed.slot as int].push(
                observed.source_occurrence,
            )
        &&& forall|other: int| 0 <= other < observed.semantic_slots_before.len()
            && other != observed.slot as int
            ==> observed.semantic_slots_after[other]
                == observed.semantic_slots_before[other]
        &&& after.frames == before.frames.update(
            top_index,
            scalar_continue_projected_top(before.frames[top_index], observed),
        )
        &&& after.arena == before.arena
        &&& after.root_end == before.root_end
        &&& after.pushed == before.pushed
        &&& after.completed == before.completed
        &&& after.next_obligation_id == before.next_obligation_id
    }
}

pub proof fn scalar_continue_source_image_exact(
    before: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires scalar_continue_source_before_matches(before, observed),
    ensures
        observed.source.bytes == observed.body_bytes,
        observed.source.positions == observed.body_positions,
        observed.source.end == observed.body_end,
        forall|at: usize| (at as int) < observed.body_bytes.len()
            ==> admission::admission_indexed_source_position(observed.source, at)
                == Some(observed.body_positions[at as int]),
{
    reveal(scalar_continue_source_before_matches);
    assert forall|at: usize| (at as int) < observed.body_bytes.len()
        implies admission::admission_indexed_source_position(observed.source, at)
            == Some(observed.body_positions[at as int]) by {
        assert(admission::admission_indexed_source_wf(observed.source));
    }
}

pub proof fn scalar_continue_source_value_and_identity_exact(
    before: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires scalar_continue_source_before_matches(before, observed),
    ensures
        observed.program_identity == observed.generated_program_identity,
        observed.field_identity == observed.resolved_field_identity,
        observed.variant_token == observed.generated_variant_token,
        scalar_source_typed_value_matches(
            observed.scalar_kind, observed.field, observed.scalar_value,
        ),
        observed.source_occurrence.field_source == observed.field_source,
        observed.source_occurrence.field_offset == observed.field_offset,
{
    reveal(scalar_continue_source_before_matches);
}

pub proof fn scalar_continue_source_slot_delta_exact(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires
        scalar_continue_source_before_matches(before, observed),
        scalar_continue_source_after_matches(before, after, observed),
    ensures
        observed.semantic_slots_after.len() == observed.semantic_slots_before.len(),
        observed.semantic_slots_after[observed.slot as int]
            == observed.semantic_slots_before[observed.slot as int].push(
                observed.source_occurrence,
            ),
        forall|other: int| 0 <= other < observed.semantic_slots_before.len()
            && other != observed.slot as int
            ==> observed.semantic_slots_after[other]
                == observed.semantic_slots_before[other],
        after.arena == before.arena,
{
    reveal(scalar_continue_source_before_matches);
    reveal(scalar_continue_source_after_matches);
}

pub proof fn scalar_continue_source_anchor_exact(
    before: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires scalar_continue_source_before_matches(before, observed),
    ensures
        admission::admission_indexed_source_position(
            observed.source, observed.field.encoded_start,
        ) == Some(admission::AdmissionSourceAnchor {
            source: observed.field_source,
            offset: observed.field_offset,
        }),
        observed.appended.physical_source == observed.field_source,
        observed.appended.physical_start == observed.field_offset as int,
{
    reveal(scalar_continue_source_before_matches);
}

pub proof fn scalar_continue_source_continue_enabled(
    before: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires scalar_continue_source_before_matches(before, observed),
    ensures admission::admission_continue_step(
        before, observed.field.encoded_end, observed.slot, observed.repeated,
        observed.field.number,
    ).is_some(),
{
    reveal(scalar_continue_source_before_matches);
    let top = before.frames.last();
    let cursor = admission::IndexedCanonicalFieldCursorModel {
        region_start: top.region_start,
        region_end: top.region_end,
        next: top.cursor,
        source_bytes: top.source_bytes,
    };
    assert(admission::indexed_field_at_cursor(cursor, observed.field));
    assert(top.cursor == observed.field.encoded_start);
    assert(top.cursor < observed.field.encoded_end <= top.region_end);
    match observed.last_slot_before {
        None => {},
        Some(last) => {
            if last == observed.slot {
                assert(observed.seen_before);
                assert(observed.repeated);
            }
        },
    }
    assert(admission::slot_admissible(
        top.last_slot, observed.slot, top.slot_count, observed.repeated,
    ));
    assert(admission::admission_continue_step(
        before, observed.field.encoded_end, observed.slot, observed.repeated,
        observed.field.number,
    ).is_some());
}

pub proof fn scalar_continue_source_indexed_enabled(
    before: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires scalar_continue_source_before_matches(before, observed),
    ensures admission::admission_record_scalar_occurrence_indexed_step(
        before, observed.source, observed.field,
        observed.slot, observed.repeated, observed.scalar_kind,
    ).is_some(),
{
    scalar_continue_source_continue_enabled(before, observed);
    scalar_continue_source_anchor_exact(before, observed);
    reveal(scalar_continue_source_before_matches);
    let top = before.frames.last();
    let position = admission::admission_indexed_source_position(
        observed.source, observed.field.encoded_start,
    ).unwrap();
    let cursor = admission::IndexedCanonicalFieldCursorModel {
        region_start: top.region_start,
        region_end: top.region_end,
        next: top.cursor,
        source_bytes: top.source_bytes,
    };
    assert(admission::indexed_field_at_cursor(cursor, observed.field));
    assert(admission::admission_scalar_descriptor_valid(
        top.program_type_id, observed.field, observed.slot,
        observed.repeated, observed.scalar_kind,
    ));
    assert(admission::admission_record_scalar_occurrence_at_position_step(
        before, observed.field, observed.slot, observed.repeated,
        observed.scalar_kind, position.source, position.offset as int,
    ).is_some());
    assert(admission::admission_record_scalar_occurrence_indexed_step(
        before, observed.source, observed.field,
        observed.slot, observed.repeated, observed.scalar_kind,
    ).is_some());
}

/// The local projection agrees with the indexed update once the observed
/// generated descriptor and field facts have been bound to the Rust source.
pub proof fn scalar_continue_source_post_refines_enabled_indexed_step(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    observed: ScalarContinueSourceObservation,
)
    requires
        scalar_continue_source_before_matches(before, observed),
        scalar_continue_source_after_matches(before, after, observed),
    ensures admission::admission_record_scalar_occurrence_indexed_step(
        before, observed.source, observed.field,
        observed.slot, observed.repeated, observed.scalar_kind,
    ) == Some(after),
{
    scalar_continue_source_anchor_exact(before, observed);
    scalar_continue_source_continue_enabled(before, observed);
    scalar_continue_source_indexed_enabled(before, observed);
    reveal(scalar_continue_source_before_matches);
    reveal(scalar_continue_source_after_matches);
    let modeled_after = admission::admission_record_scalar_occurrence_indexed_step(
        before, observed.source, observed.field,
        observed.slot, observed.repeated, observed.scalar_kind,
    ).unwrap();
    let top_index = before.frames.len() - 1;
    let middle = admission::admission_continue_step(
        before, observed.field.encoded_end,
        observed.slot, observed.repeated, observed.field.number,
    ).unwrap();
    assert(middle.frames.len() == before.frames.len());
    assert(modeled_after.frames == before.frames.update(
        top_index,
        scalar_continue_projected_top(before.frames[top_index], observed),
    ));
    assert(modeled_after.arena == before.arena);
    assert(modeled_after.root_end == before.root_end);
    assert(modeled_after.pushed == before.pushed);
    assert(modeled_after.completed == before.completed);
    assert(modeled_after.next_obligation_id == before.next_obligation_id);
    assert(modeled_after == after);
}

}

fn main() {}
