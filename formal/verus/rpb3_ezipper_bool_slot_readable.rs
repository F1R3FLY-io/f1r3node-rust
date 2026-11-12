//! EZipper connective flag readability over the integrated admission product.
//!
//! The EZipper score candidate `bridge_ezipper_score_step` appends the PathMap
//! score nodes through `bridge_pathmap_score_append`, which reads the
//! connective flag of the EZipper (program 32) as `bridge_bool_slot(top, 4, 5)`
//! and fails when that read is None. This module shows that the read is Some on
//! every EZipper frame of every state that the integrated dispatcher reaches.
//!
//! `bridge_bool_slot(frame, 4, 5)` is Some in exactly two cases: no scalar
//! occurrence of slot 4 has been recorded and slot 4 is in neither `seen` nor
//! `scalar_slots` (the flag reads false), or exactly one occurrence of slot 4
//! has been recorded, with tag 5, bool kind and value 1, and slot 4 is in both
//! sets (the flag reads true). The invariant `ezipper_flags_readable` states
//! the read itself for every EZipper frame of the admission stack; it is
//! inductive with no strengthening:
//! - the scalar record step is the only step that records an occurrence. On an
//!   EZipper, its generated descriptor pins slot 4 to tag 5, a non-repeated
//!   bool whose default is forbidden, so the recorded value is 1; the continue
//!   guard rejects a second record of the slot, so the frame moves from the
//!   first case to the second (`recorded_occurrence_keeps_flag_readable`);
//! - the plain continue and push steps insert their slot into `seen` without a
//!   descriptor check, so they keep the read only at a slot other than 4. Their
//!   callers use slot 0 (MapKey), slot 1 (MapValue) and, on an EZipper, the
//!   message slot of tag 1, which is slot 0
//!   (`ezipper_plain_push_targets_payload_slot`,
//!   `ezipper_indexed_push_targets_payload_slot`);
//! - every pushed child starts with no occurrences and empty slot sets, and
//!   every other step keeps the fields the read depends on, except the
//!   EZipper score step, which changes only the score handle.
//!
//! Results:
//! - `integrated_step_keeps_flags_readable`: every action of `integrated_step`
//!   preserves the invariant, through `live_runtime_step_keeps_flags_readable`
//!   for the Live actions and one lemma per remaining action family.
//! - `integrated_trace_keeps_flags_readable`: every state of an integrated
//!   trace whose first state satisfies the invariant satisfies it; a first
//!   state whose frames have recorded nothing at slot 4 does
//!   (`unrecorded_start_is_readable`).
//! - `reachable_ezipper_flag_slot_is_readable`: on every active state of such
//!   a trace whose top frame is an EZipper, `bridge_bool_slot(top, 4, 5)` is
//!   Some.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission,
    rpb3_canonical_path_common_product as common,
    rpb3_canonical_path_driver_product_integration as path_driver,
    rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as epm_terminal,
    rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_batch_cursor_source_extraction as act_source,
    rpb3_epm_live_handoff as handoff,
    rpb3_epm_obligation_return_live as obligation_return,
    rpb3_epm_push_live_action_refinement as push_live,
    rpb3_epm_push_source_action_refinement as push_source,
    rpb3_epm_restore_run_expansion as restore,
    rpb3_epm_source_accept_refinement as epm_source_accept,
    rpb3_epm_unwind_event_lift as unwind,
    rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_integrated_dispatcher as dispatcher,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_map_frame_refinement as map,
    rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_score_admission_bridge as score_bridge,
    rpb3_wire_cursor_source_refinement as wire_source,
    LiveRuntimeProductAction, LiveRuntimeProductState, ProductScoreState,
};

verus! {

/// The EZipper frame: a Message frame of program 32.
pub open spec fn ezipper_frame(frame: admission::AdmissionFrameState) -> bool {
    &&& frame.frame_kind == 1
    &&& frame.family == admission::AdmissionFamily::Other
    &&& frame.program_type_id == 32
}

/// The connective flag of an EZipper frame, slot 4 with tag 5, reads as a bool.
pub open spec fn ezipper_flag_slot_readable(frame: admission::AdmissionFrameState) -> bool {
    ezipper_frame(frame) ==> score_bridge::bridge_bool_slot(frame, 4, 5).is_some()
}

/// Every EZipper frame of a stack has a readable connective flag.
pub open spec fn ezipper_flags_readable(frames: Seq<admission::AdmissionFrameState>) -> bool {
    forall|i: int| 0 <= i < frames.len() ==> #[trigger] ezipper_flag_slot_readable(frames[i])
}

/// The invariant on one bridge: its admission frames.
pub open spec fn bridge_ezipper_flags_readable(
    bridge: score_bridge::ScoreAdmissionBridge,
) -> bool {
    ezipper_flags_readable(bridge.admission.frames)
}

/// The invariant on the live runtime product.
pub open spec fn live_ezipper_flags_readable(state: LiveRuntimeProductState) -> bool {
    bridge_ezipper_flags_readable(state.source.product.bridge)
}

/// The invariant on an integrated state. An accepted state names the product
/// that produced it, as `integrated_accepted_wf` does.
pub open spec fn integrated_ezipper_flags_readable(state: dispatcher::IntegratedState) -> bool {
    match state {
        dispatcher::IntegratedState::Active { product } => live_ezipper_flags_readable(product),
        dispatcher::IntegratedState::Accepted { accepted } => match accepted {
            dispatcher::IntegratedAccepted::LiveRoot { post_root_score, .. } =>
                live_ezipper_flags_readable(post_root_score),
            dispatcher::IntegratedAccepted::LiveNoScoreRoot { pre_complete, .. } =>
                live_ezipper_flags_readable(pre_complete),
            dispatcher::IntegratedAccepted::CanonicalPathRoot { pre_complete, .. } =>
                live_ezipper_flags_readable(pre_complete),
        },
    }
}

/// `after` keeps every field of `before` that `ezipper_flag_slot_readable`
/// reads.
pub open spec fn ezipper_flag_view_kept(
    before: admission::AdmissionFrameState,
    after: admission::AdmissionFrameState,
) -> bool {
    &&& after.frame_kind == before.frame_kind
    &&& after.family == before.family
    &&& after.program_type_id == before.program_type_id
    &&& after.scalar_occurrences == before.scalar_occurrences
    &&& after.seen.contains(4) == before.seen.contains(4)
    &&& after.scalar_slots.contains(4) == before.scalar_slots.contains(4)
}

/// Appending an occurrence of another slot does not change the scalar read of
/// `slot`.
pub proof fn scalar_slot_skips_other_slot_push(
    occurrences: Seq<admission::AdmissionScalarOccurrence>,
    occurrence: admission::AdmissionScalarOccurrence,
    slot: usize,
    tag: u32,
    scalar_kind: u8,
)
    requires occurrence.slot != slot,
    ensures
        score_bridge::bridge_scalar_slot(occurrences.push(occurrence), slot, tag, scalar_kind)
            == score_bridge::bridge_scalar_slot(occurrences, slot, tag, scalar_kind),
    decreases occurrences.len(),
{
    let pushed = occurrences.push(occurrence);
    if occurrences.len() == 0 {
        assert(pushed.drop_first() =~= occurrences);
        assert(score_bridge::bridge_scalar_slot(pushed.drop_first(), slot, tag, scalar_kind)
            == Some(None::<u64>));
    } else {
        assert(pushed[0] == occurrences[0]);
        assert(pushed.drop_first() =~= occurrences.drop_first().push(occurrence));
        scalar_slot_skips_other_slot_push(
            occurrences.drop_first(), occurrence, slot, tag, scalar_kind,
        );
    }
}

/// Appending the first occurrence of `slot`, with the read's tag and kind, to
/// occurrences that hold none makes the read that occurrence's value.
pub proof fn scalar_slot_takes_first_slot_push(
    occurrences: Seq<admission::AdmissionScalarOccurrence>,
    occurrence: admission::AdmissionScalarOccurrence,
    slot: usize,
    tag: u32,
    scalar_kind: u8,
)
    requires
        score_bridge::bridge_scalar_slot(occurrences, slot, tag, scalar_kind)
            == Some(None::<u64>),
        occurrence.slot == slot,
        occurrence.tag == tag,
        occurrence.scalar_kind == scalar_kind,
    ensures
        score_bridge::bridge_scalar_slot(occurrences.push(occurrence), slot, tag, scalar_kind)
            == Some(Some(occurrence.varint_value)),
    decreases occurrences.len(),
{
    let pushed = occurrences.push(occurrence);
    if occurrences.len() == 0 {
        assert(pushed.drop_first() =~= occurrences);
    } else {
        assert(pushed[0] == occurrences[0]);
        assert(pushed.drop_first() =~= occurrences.drop_first().push(occurrence));
        assert(score_bridge::bridge_scalar_slot(occurrences.drop_first(), slot, tag, scalar_kind)
            == Some(None::<u64>));
        scalar_slot_takes_first_slot_push(
            occurrences.drop_first(), occurrence, slot, tag, scalar_kind,
        );
    }
}

/// A frame that keeps the flag view of a readable frame is readable.
pub proof fn flag_view_kept_frame_readable(
    before: admission::AdmissionFrameState,
    after: admission::AdmissionFrameState,
)
    requires
        ezipper_flag_slot_readable(before),
        ezipper_flag_view_kept(before, after),
    ensures ezipper_flag_slot_readable(after),
{
    if ezipper_frame(after) {
        assert(score_bridge::bridge_bool_slot(after, 4, 5)
            == score_bridge::bridge_bool_slot(before, 4, 5));
    }
}

/// A frame that has recorded no occurrence and holds slot 4 in neither set
/// reads its flag as false.
pub proof fn unrecorded_frame_flag_readable(frame: admission::AdmissionFrameState)
    requires
        frame.scalar_occurrences.len() == 0,
        !frame.seen.contains(4),
        !frame.scalar_slots.contains(4),
    ensures ezipper_flag_slot_readable(frame),
{
    assert(score_bridge::bridge_scalar_slot(frame.scalar_occurrences, 4, 5, 1)
        == Some(None::<u64>));
}

/// Replacing the top frame by a readable frame keeps the invariant.
pub proof fn top_replaced_flags_readable(
    frames: Seq<admission::AdmissionFrameState>,
    top: admission::AdmissionFrameState,
)
    requires
        ezipper_flags_readable(frames),
        frames.len() > 0,
        ezipper_flag_slot_readable(top),
    ensures ezipper_flags_readable(frames.update(frames.len() - 1, top)),
{
    let after = frames.update(frames.len() - 1, top);
    assert forall|i: int| 0 <= i < after.len() implies
        #[trigger] ezipper_flag_slot_readable(after[i]) by {
        if i < frames.len() - 1 {
            assert(after[i] == frames[i]);
        }
    }
}

/// Replacing the top frame and pushing a child, both readable, keeps the
/// invariant.
pub proof fn child_pushed_flags_readable(
    frames: Seq<admission::AdmissionFrameState>,
    top: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
)
    requires
        ezipper_flags_readable(frames),
        frames.len() > 0,
        ezipper_flag_slot_readable(top),
        ezipper_flag_slot_readable(child),
    ensures ezipper_flags_readable(frames.update(frames.len() - 1, top).push(child)),
{
    let after = frames.update(frames.len() - 1, top).push(child);
    assert forall|i: int| 0 <= i < after.len() implies
        #[trigger] ezipper_flag_slot_readable(after[i]) by {
        if i < frames.len() - 1 {
            assert(after[i] == frames[i]);
        } else if i == frames.len() - 1 {
            assert(after[i] == top);
        } else {
            assert(after[i] == child);
        }
    }
}

/// Popping the top frame and replacing its parent by a readable frame keeps the
/// invariant.
pub proof fn parent_replaced_flags_readable(
    frames: Seq<admission::AdmissionFrameState>,
    parent: admission::AdmissionFrameState,
)
    requires
        ezipper_flags_readable(frames),
        frames.len() > 1,
        ezipper_flag_slot_readable(parent),
    ensures
        ezipper_flags_readable(
            frames.subrange(0, frames.len() - 1).update(frames.len() - 2, parent),
        ),
{
    let after = frames.subrange(0, frames.len() - 1).update(frames.len() - 2, parent);
    assert forall|i: int| 0 <= i < after.len() implies
        #[trigger] ezipper_flag_slot_readable(after[i]) by {
        if i < frames.len() - 2 {
            assert(after[i] == frames[i]);
        } else {
            assert(after[i] == parent);
        }
    }
}

/// The EPathMap byte cursor advance of the top frame keeps the invariant.
pub proof fn cursor_advanced_flags_readable(
    frames: Seq<admission::AdmissionFrameState>,
    consumed: usize,
)
    requires
        ezipper_flags_readable(frames),
        frames.len() > 0,
    ensures
        ezipper_flags_readable(
            frames.update(
                frames.len() - 1, epm::epm_advanced_admission_cursor(frames.last(), consumed),
            ),
        ),
{
    let top = frames.last();
    let advanced = epm::epm_advanced_admission_cursor(top, consumed);
    assert(ezipper_flag_slot_readable(frames[frames.len() - 1]));
    flag_view_kept_frame_readable(top, advanced);
    top_replaced_flags_readable(frames, advanced);
}

/// The generated descriptor of program 32 at slot 4 is the connective flag:
/// tag 5, a non-repeated bool whose default is forbidden, so a valid payload
/// is the value 1.
pub proof fn ezipper_flag_slot_descriptor_is_pinned(
    field: admission::IndexedCanonicalFieldModel,
    repeated: bool,
    scalar_kind: u8,
)
    requires admission::admission_scalar_descriptor_valid(32, field, 4, repeated, scalar_kind),
    ensures
        field.number == 5,
        !repeated,
        scalar_kind == 1,
        field.decoded_varint == 1,
{
}

/// Recording one scalar occurrence on a readable frame keeps it readable when
/// the occurrence's descriptor is valid for the frame's program and the slot
/// was not seen unless it is repeated. At slot 4 of an EZipper, the descriptor
/// makes the occurrence the flag with value 1, and the frame, which held no
/// occurrence of slot 4, gets exactly one; at another slot, the read of slot 4
/// skips the occurrence and the slot sets keep slot 4's membership.
pub proof fn recorded_occurrence_keeps_flag_readable(
    prior: admission::AdmissionFrameState,
    updated: admission::AdmissionFrameState,
    field: admission::IndexedCanonicalFieldModel,
    occurrence: admission::AdmissionScalarOccurrence,
    slot: usize,
    repeated: bool,
    scalar_kind: u8,
)
    requires
        ezipper_flag_slot_readable(prior),
        repeated || !prior.seen.contains(slot as int),
        admission::admission_scalar_descriptor_valid(
            prior.program_type_id, field, slot, repeated, scalar_kind,
        ),
        occurrence.slot == slot,
        occurrence.tag == field.number,
        occurrence.scalar_kind == scalar_kind,
        occurrence.varint_value == field.decoded_varint,
        updated.frame_kind == prior.frame_kind,
        updated.family == prior.family,
        updated.program_type_id == prior.program_type_id,
        updated.seen == prior.seen.insert(slot as int),
        updated.scalar_slots == prior.scalar_slots.insert(slot as int),
        updated.scalar_occurrences == prior.scalar_occurrences.push(occurrence),
    ensures ezipper_flag_slot_readable(updated),
{
    if ezipper_frame(prior) {
        if slot == 4 {
            ezipper_flag_slot_descriptor_is_pinned(field, repeated, scalar_kind);
            assert(!prior.seen.contains(4));
            assert(score_bridge::bridge_scalar_slot(prior.scalar_occurrences, 4, 5, 1)
                == Some(None::<u64>));
            scalar_slot_takes_first_slot_push(prior.scalar_occurrences, occurrence, 4, 5, 1);
            assert(updated.seen.contains(4));
            assert(updated.scalar_slots.contains(4));
            assert(score_bridge::bridge_bool_slot(updated, 4, 5) == Some(true));
        } else {
            scalar_slot_skips_other_slot_push(prior.scalar_occurrences, occurrence, 4, 5, 1);
            assert(updated.seen.contains(4) == prior.seen.contains(4));
            assert(updated.scalar_slots.contains(4) == prior.scalar_slots.contains(4));
            assert(score_bridge::bridge_bool_slot(updated, 4, 5)
                == score_bridge::bridge_bool_slot(prior, 4, 5));
        }
    }
}

/// Every successful `admission_continue_step` at a slot other than 4, or on a
/// top that is not an EZipper, keeps the invariant: it inserts its slot into
/// the top frame's slot sets and records nothing.
pub proof fn admission_continue_keeps_flags_readable(state: admission::AdmissionState)
    requires ezipper_flags_readable(state.frames),
    ensures
        forall|next_cursor: usize, slot: usize, repeated: bool, field_tag: u32|
            (slot != 4 || !ezipper_frame(state.frames.last())) ==>
            match #[trigger] admission::admission_continue_step(
                state, next_cursor, slot, repeated, field_tag,
            ) {
                Some(next) => ezipper_flags_readable(next.frames),
                None => true,
            },
{
    assert forall|next_cursor: usize, slot: usize, repeated: bool, field_tag: u32|
        (slot != 4 || !ezipper_frame(state.frames.last())) implies
        match #[trigger] admission::admission_continue_step(
            state, next_cursor, slot, repeated, field_tag,
        ) {
            Some(next) => ezipper_flags_readable(next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_continue_step(
            state, next_cursor, slot, repeated, field_tag,
        ) {
            let index = state.frames.len() - 1;
            let top = state.frames[index];
            let updated = next.frames[index];
            assert(top == state.frames.last());
            assert(next.frames == state.frames.update(index, updated));
            assert(ezipper_flag_slot_readable(state.frames[index]));
            if ezipper_frame(top) {
                assert(updated.seen.contains(4) == top.seen.contains(4));
                assert(updated.scalar_slots.contains(4) == top.scalar_slots.contains(4));
                flag_view_kept_frame_readable(top, updated);
            }
            top_replaced_flags_readable(state.frames, updated);
        }
    }
}

/// Every successful scalar record keeps the invariant: a continue step, then
/// one scalar occurrence appended to the top frame, admitted by the generated
/// descriptor of the top frame's program.
pub proof fn scalar_record_keeps_flags_readable(state: admission::AdmissionState)
    requires ezipper_flags_readable(state.frames),
    ensures
        forall|field: admission::IndexedCanonicalFieldModel, slot: usize, repeated: bool,
            scalar_kind: u8, physical_source: usize, physical_start: int|
            match #[trigger] admission::admission_record_scalar_occurrence_at_position_step(
                state, field, slot, repeated, scalar_kind, physical_source, physical_start,
            ) {
                Some(next) => ezipper_flags_readable(next.frames),
                None => true,
            },
{
    assert forall|field: admission::IndexedCanonicalFieldModel, slot: usize, repeated: bool,
        scalar_kind: u8, physical_source: usize, physical_start: int|
        match #[trigger] admission::admission_record_scalar_occurrence_at_position_step(
            state, field, slot, repeated, scalar_kind, physical_source, physical_start,
        ) {
            Some(next) => ezipper_flags_readable(next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_record_scalar_occurrence_at_position_step(
            state, field, slot, repeated, scalar_kind, physical_source, physical_start,
        ) {
            let continued = admission::admission_continue_step(
                state, field.encoded_end, slot, repeated, field.number,
            ).unwrap();
            let index = state.frames.len() - 1;
            let prior = state.frames[index];
            let updated = next.frames[index];
            let occurrence = admission::AdmissionScalarOccurrence {
                slot,
                tag: field.number,
                scalar_kind,
                varint_value: field.decoded_varint,
                physical_source,
                physical_start,
                bytes: field.payload_bytes,
            };
            assert(continued.frames == state.frames.update(index, continued.frames[index]));
            assert(next.frames == state.frames.update(index, updated));
            assert(updated.scalar_occurrences == prior.scalar_occurrences.push(occurrence));
            assert(ezipper_flag_slot_readable(state.frames[index]));
            recorded_occurrence_keeps_flag_readable(
                prior, updated, field, occurrence, slot, repeated, scalar_kind,
            );
            top_replaced_flags_readable(state.frames, updated);
        }
    }
}

/// Every successful `admission_push_step` at a slot other than 4, or on a top
/// that is not an EZipper, keeps the invariant: the parent inserts its slot
/// into `seen` and records nothing, and the pushed child has recorded nothing
/// and has empty slot sets.
pub proof fn admission_push_keeps_flags_readable(state: admission::AdmissionState)
    requires ezipper_flags_readable(state.frames),
    ensures
        forall|parent_next: usize, child: admission::AdmissionFrameState, slot: usize,
            repeated: bool, field_tag: u32, frame_bytes: usize|
            (slot != 4 || !ezipper_frame(state.frames.last())) ==>
            match #[trigger] admission::admission_push_step(
                state, parent_next, child, slot, repeated, field_tag, frame_bytes,
            ) {
                Some(next) => ezipper_flags_readable(next.frames),
                None => true,
            },
{
    assert forall|parent_next: usize, child: admission::AdmissionFrameState, slot: usize,
        repeated: bool, field_tag: u32, frame_bytes: usize|
        (slot != 4 || !ezipper_frame(state.frames.last())) implies
        match #[trigger] admission::admission_push_step(
            state, parent_next, child, slot, repeated, field_tag, frame_bytes,
        ) {
            Some(next) => ezipper_flags_readable(next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_push_step(
            state, parent_next, child, slot, repeated, field_tag, frame_bytes,
        ) {
            let index = state.frames.len() - 1;
            let parent = state.frames[index];
            let waiting = next.frames[index];
            let pushed = next.frames[index + 1];
            assert(parent == state.frames.last());
            assert(next.frames == state.frames.update(index, waiting).push(pushed));
            assert(ezipper_flag_slot_readable(state.frames[index]));
            if ezipper_frame(parent) {
                assert(waiting.seen.contains(4) == parent.seen.contains(4));
                flag_view_kept_frame_readable(parent, waiting);
            }
            unrecorded_frame_flag_readable(pushed);
            child_pushed_flags_readable(state.frames, waiting, pushed);
        }
    }
}

/// On an EZipper top, a successful plain Message Push is the admission push of
/// the EZipper's only message field, tag 1, at slot 0.
pub proof fn ezipper_plain_push_targets_payload_slot(
    frame: admission::ConcreteMessageFrameModel,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    frame_bytes: usize,
)
    requires
        admission::concrete_message_push(frame, field, child, frame_bytes).is_some(),
        frame.projection.frames.len() > 0,
        ezipper_frame(frame.projection.frames.last()),
    ensures
        admission::concrete_message_push(frame, field, child, frame_bytes)
            == admission::admission_push_step(
                frame.projection, field.encoded_end, child, 0, false, 1, frame_bytes,
            ),
{
}

/// On an EZipper top, a successful indexed Message Push is the admission push
/// of the EZipper's only message field, tag 1, at slot 0.
pub proof fn ezipper_indexed_push_targets_payload_slot(
    frame: admission::ConcreteMessageFrameModel,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires
        admission::concrete_message_push_indexed(
            frame, field, child, parent_source, frame_bytes,
        ).is_some(),
        frame.projection.frames.len() > 0,
        ezipper_frame(frame.projection.frames.last()),
    ensures
        admission::concrete_message_push_indexed(frame, field, child, parent_source, frame_bytes)
            == admission::admission_push_step(
                frame.projection, field.encoded_end, child, 0, false, 1, frame_bytes,
            ),
{
}

/// Every successful child Complete keeps the invariant: it pops the child and
/// keeps the parent's flag view.
pub proof fn complete_child_keeps_flags_readable(
    state: admission::AdmissionState,
    returned_id: u64,
    returned_kind: u8,
    summary: admission::AdmissionReturnedKind,
)
    requires ezipper_flags_readable(state.frames),
    ensures
        match admission::admission_complete_child_step(
            state, returned_id, returned_kind, summary,
        ) {
            Some(next) => ezipper_flags_readable(next.frames),
            None => true,
        },
{
    if let Some(next) = admission::admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ) {
        let count = state.frames.len();
        let parent = state.frames[count - 2];
        let accepted = next.frames[count - 2];
        assert(next.frames == state.frames.subrange(0, count - 1).update(count - 2, accepted));
        assert(ezipper_flag_slot_readable(state.frames[count - 2]));
        flag_view_kept_frame_readable(parent, accepted);
        parent_replaced_flags_readable(state.frames, accepted);
    }
}

/// Every successful CanonicalPath push keeps the invariant: the kind-4 parent
/// keeps its flag view, and the pushed child is a program-1 frame.
pub proof fn path_push_keeps_flags_readable(state: admission::AdmissionState)
    requires ezipper_flags_readable(state.frames),
    ensures
        forall|source: admission::AdmissionCanonicalPathPushSource,
            child: admission::AdmissionFrameState,
            storage: admission::AdmissionCanonicalPathPushStorage|
            match #[trigger] admission::admission_canonical_path_push_indexed_step(
                state, source, child, storage,
            ) {
                Some(next) => ezipper_flags_readable(next.frames),
                None => true,
            },
{
    assert forall|source: admission::AdmissionCanonicalPathPushSource,
        child: admission::AdmissionFrameState,
        storage: admission::AdmissionCanonicalPathPushStorage|
        match #[trigger] admission::admission_canonical_path_push_indexed_step(
            state, source, child, storage,
        ) {
            Some(next) => ezipper_flags_readable(next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_canonical_path_push_indexed_step(
            state, source, child, storage,
        ) {
            let index = state.frames.len() - 1;
            let parent = state.frames[index];
            let waiting = next.frames[index];
            let pushed = next.frames[index + 1];
            assert(next.frames == state.frames.update(index, waiting).push(pushed));
            assert(ezipper_flag_slot_readable(state.frames[index]));
            flag_view_kept_frame_readable(parent, waiting);
            assert(pushed.program_type_id == 1);
            child_pushed_flags_readable(state.frames, waiting, pushed);
        }
    }
}

/// Every successful CanonicalPath child return keeps the invariant: the child
/// is dropped and the kind-4 parent keeps its flag view.
pub proof fn path_return_keeps_flags_readable(state: admission::AdmissionState)
    requires ezipper_flags_readable(state.frames),
    ensures
        forall|source: admission::AdmissionCanonicalPathReturnSource,
            summary: admission::AdmissionReturnedKind|
            match #[trigger] admission::admission_canonical_path_return_indexed_step(
                state, source, summary,
            ) {
                Some(next) => ezipper_flags_readable(next.frames),
                None => true,
            },
{
    assert forall|source: admission::AdmissionCanonicalPathReturnSource,
        summary: admission::AdmissionReturnedKind|
        match #[trigger] admission::admission_canonical_path_return_indexed_step(
            state, source, summary,
        ) {
            Some(next) => ezipper_flags_readable(next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_canonical_path_return_indexed_step(
            state, source, summary,
        ) {
            let count = state.frames.len();
            let parent = state.frames[count - 2];
            let resolved = next.frames[count - 2];
            assert(next.frames == state.frames.subrange(0, count - 1).update(count - 2, resolved));
            assert(ezipper_flag_slot_readable(state.frames[count - 2]));
            flag_view_kept_frame_readable(parent, resolved);
            parent_replaced_flags_readable(state.frames, resolved);
        }
    }
}

/// On an EZipper top, the finish selector is the EZipper step: groups 1 to 5
/// propose nothing, and group 6 proposes only the EZipper candidate.
pub proof fn ezipper_finish_selects_ezipper_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        ezipper_frame(before.admission.frames.last()),
    ensures
        score_bridge::bridge_score_finish_selected(before)
            == score_bridge::bridge_ezipper_score_step(before),
{
    score_bridge::bridge_score_finish_selection_group_6_earlier_groups_empty(before);
    assert(score_bridge::bridge_var_score_step(before).is_none());
    assert(score_bridge::bridge_epathmap_owner_score_step(before).is_none());
    reveal(score_bridge::bridge_score_finish_selection_group_6);
    reveal(score_bridge::bridge_score_finish_selected);
}

/// Every finish-selected score step keeps the invariant. Frames below the top
/// are kept. An EZipper top is scored by the EZipper step, which changes only
/// its score handle; any other top keeps its kind, family and program, so it
/// stays a frame that is not an EZipper.
pub proof fn score_finish_keeps_flags_readable(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        bridge_ezipper_flags_readable(before),
    ensures
        match score_bridge::bridge_score_finish_selected(before) {
            Some(after) => bridge_ezipper_flags_readable(after),
            None => true,
        },
{
    score_bridge::bridge_score_finish_selected_preserves_checked(before);
    if let Some(after) = score_bridge::bridge_score_finish_selected(before) {
        let frames = before.admission.frames;
        let count = frames.len();
        let top = frames[count - 1];
        let scored = after.admission.frames[count - 1];
        assert(frames.last() == top);
        assert(after.admission.frames.last() == scored);
        assert(ezipper_flag_slot_readable(frames[count - 1]));
        if ezipper_frame(top) {
            ezipper_finish_selects_ezipper_step(before);
            score_bridge::bridge_ezipper_score_step_exact(before);
            flag_view_kept_frame_readable(top, scored);
        }
        assert(ezipper_flag_slot_readable(scored));
        assert forall|i: int| 0 <= i < after.admission.frames.len() implies
            #[trigger] ezipper_flag_slot_readable(after.admission.frames[i]) by {
            if i < count - 1 {
                assert(after.admission.frames[i] == frames[i]);
            } else {
                assert(i == count - 1);
            }
        }
    }
}

/// The EPathMap child return pops the child and resolves its parent, which
/// keeps the parent's flag view.
pub proof fn epm_resolved_parent_keeps_flags_readable(
    frames: Seq<admission::AdmissionFrameState>,
)
    requires
        ezipper_flags_readable(frames),
        frames.len() > 1,
    ensures
        ezipper_flags_readable(
            frames.subrange(0, frames.len() - 1).update(
                frames.len() - 2, epm::epm_parent_resolved(frames[frames.len() - 2]),
            ),
        ),
{
    let parent = frames[frames.len() - 2];
    let resolved = epm::epm_parent_resolved(parent);
    assert(ezipper_flag_slot_readable(frames[frames.len() - 2]));
    flag_view_kept_frame_readable(parent, resolved);
    parent_replaced_flags_readable(frames, resolved);
}

/// The EPathMap push keeps the invariant: its parent keeps its flag view, and
/// the pushed child has recorded nothing and holds slot 4 in neither set.
pub proof fn epm_push_frames_keep_flags_readable(
    frames: Seq<admission::AdmissionFrameState>,
    consumed: usize,
    return_id: u64,
    kind: u8,
    child: admission::AdmissionFrameState,
)
    requires
        ezipper_flags_readable(frames),
        frames.len() > 0,
        child.scalar_occurrences.len() == 0,
        !child.seen.contains(4),
        !child.scalar_slots.contains(4),
    ensures
        ezipper_flags_readable(
            frames.update(
                frames.len() - 1,
                epm::epm_parent_waiting(
                    epm::epm_advanced_admission_cursor(frames.last(), consumed),
                    return_id, kind,
                ),
            ).push(epm::epm_assigned_child(child, return_id)),
        ),
{
    let top = frames.last();
    let waiting = epm::epm_parent_waiting(
        epm::epm_advanced_admission_cursor(top, consumed), return_id, kind,
    );
    let assigned = epm::epm_assigned_child(child, return_id);
    assert(ezipper_flag_slot_readable(frames[frames.len() - 1]));
    flag_view_kept_frame_readable(top, waiting);
    unrecorded_frame_flag_readable(assigned);
    child_pushed_flags_readable(frames, waiting, assigned);
}

/// The EPathMap finish accept keeps the invariant: it pops the EPathMap child
/// into its parent and rewrites only the parent's last return, so the parent
/// keeps its flag view.
pub proof fn epm_accept_frames_keep_flags_readable(before: epm::LinkedEpmState)
    requires ezipper_flags_readable(before.admission.frames),
    ensures
        match epm_source_accept::epm_source_finish_accept(before) {
            Some(after) => ezipper_flags_readable(after.frames),
            None => true,
        },
{
    if let Some(after) = epm_source_accept::epm_source_finish_accept(before) {
        let finished = epm_source_accept::epm_end_linked(before);
        assert(epm_source_accept::epm_source_accept(before, finished) == Some(after));
        let count = before.admission.frames.len();
        let child = before.admission.frames[count - 1];
        let parent = before.admission.frames[count - 2];
        assert(child == before.admission.frames.last());
        let base = admission::admission_complete_child_step(
            before.admission, child.return_id.unwrap(), 3,
            admission::AdmissionReturnedKind::Other,
        ).unwrap();
        let accepted = after.frames[count - 2];
        assert(base.frames.len() == count - 1);
        assert(after.frames == base.frames.update(count - 2, accepted));
        assert(base.frames == before.admission.frames.subrange(0, count - 1).update(
            count - 2, base.frames[count - 2],
        ));
        assert(after.frames =~= before.admission.frames.subrange(0, count - 1).update(
            count - 2, accepted,
        ));
        assert(ezipper_flag_slot_readable(before.admission.frames[count - 2]));
        flag_view_kept_frame_readable(parent, accepted);
        parent_replaced_flags_readable(before.admission.frames, accepted);
    }
}

/// The admission root of `admission_initial_state` satisfies the invariant.
pub proof fn initial_root_flags_readable(root: admission::AdmissionFrameState)
    requires admission::admission_root_initial(root),
    ensures ezipper_flags_readable(admission::admission_initial_state(root).frames),
{
    let frames = admission::admission_initial_state(root).frames;
    unrecorded_frame_flag_readable(root);
    assert forall|i: int| 0 <= i < frames.len() implies
        #[trigger] ezipper_flag_slot_readable(frames[i]) by {
        assert(frames[i] == root);
    }
}

/// A Message Continue or ContinueIndexed map step records one scalar
/// occurrence on the top frame.
pub proof fn product_message_continue_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        match action {
            admission::ConcreteFrameActionModel::Continue { .. }
            | admission::ConcreteFrameActionModel::ContinueIndexed { .. } => true,
            _ => false,
        },
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action },
        ),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let state = before.map.admission;
    let next = after.map.admission;
    assert(admission::concrete_message_driver_step(state, action)
        == Some(admission::ConcreteFrameOutcomeModel::Continued { state: next }));
    assert(after.bridge.admission == next);
    scalar_record_keeps_flags_readable(state);
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.projection == state);
    match action {
        admission::ConcreteFrameActionModel::Continue { field } => {
            admission::concrete_message_continue_simulates(view, field);
            let moved = admission::concrete_message_continue(view, field).unwrap();
            assert(next == moved.projection);
        },
        admission::ConcreteFrameActionModel::ContinueIndexed { field, parent_source } => {
            admission::concrete_message_continue_indexed_simulates(view, field, parent_source);
            let moved = admission::concrete_message_continue_indexed(
                view, field, parent_source,
            ).unwrap();
            assert(next == moved.projection);
        },
        _ => {},
    }
}

/// A Message Push or PushIndexed map step makes the top wait and pushes a
/// fresh child. On an EZipper top it pushes at slot 0.
pub proof fn product_message_push_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        match action {
            admission::ConcreteFrameActionModel::Push { .. }
            | admission::ConcreteFrameActionModel::PushIndexed { .. } => true,
            _ => false,
        },
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action },
        ),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let state = before.map.admission;
    let next = after.map.admission;
    let source_action = map::integrated_source_message_action(state, action).unwrap();
    assert(map::integrated_admission_driver_step(
        before.map, map::IntegratedAdmissionAction::Message { action },
    ) == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
    assert(admission::concrete_message_driver_step(state, source_action)
        == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: next }));
    assert(after.bridge.admission == next);
    admission_push_keeps_flags_readable(state);
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.projection == state);
    match source_action {
        admission::ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            admission::concrete_message_push_simulates(view, field, child, frame_bytes);
            assert(admission::concrete_message_push(view, field, child, frame_bytes)
                == Some(next));
            if ezipper_frame(state.frames.last()) {
                ezipper_plain_push_targets_payload_slot(view, field, child, frame_bytes);
            }
        },
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            assert(admission::concrete_message_push_indexed(
                view, field, child, parent_source, frame_bytes,
            ) == Some(next));
            if ezipper_frame(state.frames.last()) {
                ezipper_indexed_push_targets_payload_slot(
                    view, field, child, parent_source, frame_bytes,
                );
            }
        },
        _ => {},
    }
}

/// A MapKey map step is the continue step of the key field, at slot 0, on the
/// Map top.
pub proof fn product_map_key_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::MapKey { field },
        ),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let count = before.bridge.admission.frames.len();
    let current = before.map.map_frames[count - 1].unwrap();
    let continued = map::map_admission_key_continue(current, before.map.admission, field).unwrap();
    assert(after.bridge.admission == continued.1);
    assert(admission::admission_continue_step(
        before.map.admission, field.encoded_end, 0, false, 1,
    ) == Some(continued.1));
    admission_continue_keeps_flags_readable(before.map.admission);
}

/// A MapValue or MapValueIndexed map step makes the Map top wait and pushes a
/// fresh value child, at slot 1.
pub proof fn product_map_value_push_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        match action {
            map::IntegratedAdmissionAction::MapValue { .. }
            | map::IntegratedAdmissionAction::MapValueIndexed { .. } => true,
            _ => false,
        },
        integrated::product_map_step(before, after, action),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let count = before.bridge.admission.frames.len();
    let current = before.map.map_frames[count - 1].unwrap();
    assert(map::integrated_admission_driver_step(before.map, action)
        == Some(map::IntegratedAdmissionOutcome::MapValuePushed { state: after.map }));
    assert(after.bridge.admission == after.map.admission);
    admission_push_keeps_flags_readable(before.map.admission);
    match action {
        map::IntegratedAdmissionAction::MapValue { field, child, frame_bytes } => {
            let pushed = map::map_admission_value_push(
                current, before.map.admission, field, child, frame_bytes,
            ).unwrap();
            assert(after.map.admission == pushed.1);
            assert(admission::admission_push_step(
                before.map.admission, field.encoded_end, child, 1, false, 2, frame_bytes,
            ) == Some(pushed.1));
        },
        map::IntegratedAdmissionAction::MapValueIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            let pushed = map::map_admission_value_push_indexed(
                current, before.map.admission, field, child, parent_source, frame_bytes,
            ).unwrap();
            assert(after.map.admission == pushed.1);
            assert(admission::admission_push_step(
                before.map.admission, field.encoded_end, child, 1, false, 2, frame_bytes,
            ) == Some(pushed.1));
        },
        _ => {},
    }
}

/// A Message Complete, MapAccept or MapFinish map step is a child Complete of
/// the admission state.
pub proof fn product_map_pop_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        match action {
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            }
            | map::IntegratedAdmissionAction::MapAccept
            | map::IntegratedAdmissionAction::MapFinish => true,
            _ => false,
        },
        integrated::product_map_step(before, after, action),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let state = before.map.admission;
    let count = state.frames.len();
    let next = after.map.admission;
    assert(after.bridge.admission == next);
    match action {
        map::IntegratedAdmissionAction::Message { .. } => {
            assert(map::integrated_source_message_action(
                state, admission::ConcreteFrameActionModel::Complete,
            ) == Some(admission::ConcreteFrameActionModel::Complete));
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
                    state: after.map,
                }));
            assert(admission::concrete_message_driver_step(
                state, admission::ConcreteFrameActionModel::Complete,
            ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: next }));
            admission::concrete_message_complete_child_simulates(state);
            let child = state.frames.last();
            let parent = state.frames[count - 2];
            let summary = admission::concrete_derived_summary_with_arena(
                parent, child, state.arena,
            ).unwrap();
            assert(admission::admission_complete_child_step(
                state, child.return_id.unwrap(), child.frame_kind, summary,
            ) == Some(next));
            complete_child_keeps_flags_readable(
                state, child.return_id.unwrap(), child.frame_kind, summary,
            );
        },
        map::IntegratedAdmissionAction::MapAccept => {
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapValueAccepted {
                    state: after.map,
                }));
            let parent_frame = before.map.map_frames[count - 2].unwrap();
            let accepted = map::map_admission_value_complete(parent_frame, state).unwrap();
            let wait = parent_frame.pending.unwrap();
            let summary = admission::AdmissionReturnedKind::Par {
                semantic_default: false,
                canonical_path_stable: accepted.0.value.unwrap().canonical_path_stable,
            };
            assert(next == accepted.1);
            assert(admission::admission_complete_child_step(state, wait.id, 1, summary)
                == Some(next));
            complete_child_keeps_flags_readable(state, wait.id, 1, summary);
        },
        map::IntegratedAdmissionAction::MapFinish => {
            let map_frame = before.map.map_frames[count - 1].unwrap();
            let finished = map::map_admission_entry_complete(map_frame, state).unwrap();
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapEntryCompleted {
                    summary: finished.0,
                    state: after.map,
                }));
            let child = state.frames.last();
            let returned = admission::AdmissionReturnedKind::Map {
                key_bytes: finished.0.key_bytes,
                value_score_handle: admission::admission_map_value_score_handle(child),
            };
            assert(next == finished.1);
            assert(admission::admission_complete_child_step(
                state, child.return_id.unwrap(), 2, returned,
            ) == Some(next));
            complete_child_keeps_flags_readable(state, child.return_id.unwrap(), 2, returned);
        },
        _ => {},
    }
}

/// The internal EPathMap continue advances the top frame's byte cursor.
pub proof fn product_epm_continue_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    event: epm::EpmEvent,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        integrated::product_epm_continue_step(before, after, event),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let prior = integrated::product_linked_top(before).unwrap();
    let next = integrated::product_linked_top(after).unwrap();
    assert(epm::linked_epm_continue(prior, next, event));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.update(
        prior.admission.frames.len() - 1,
        epm::epm_advanced_admission_cursor(
            prior.admission.frames.last(), next.epm.cursor.value_table_offset,
        ),
    ));
    assert(before.bridge.admission.frames.len() > 0);
    cursor_advanced_flags_readable(
        before.bridge.admission.frames, next.epm.cursor.value_table_offset,
    );
}

/// The EPathMap Map-value return pops the child and resolves its EPathMap
/// parent.
pub proof fn product_epm_child_return_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        integrated::product_score_wf(before),
        bridge_ezipper_flags_readable(before.bridge),
        integrated::product_epm_child_return_step(
            before, after, summary, admission_summary, None,
        ),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let prior = integrated::product_linked_parent(before).unwrap();
    let next = integrated::product_linked_top(after).unwrap();
    assert(epm::linked_epm_child_return(
        prior, next, summary, admission_summary, before.epm_frames.last(), None,
    ));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.subrange(
        0, prior.admission.frames.len() - 1,
    ).update(
        prior.admission.frames.len() - 2,
        epm::epm_parent_resolved(prior.admission.frames[prior.admission.frames.len() - 2]),
    ));
    assert(before.bridge.admission.frames.len() > 1);
    epm_resolved_parent_keeps_flags_readable(before.bridge.admission.frames);
}

/// The EPathMap finish accept is `epm_source_finish_accept` of the linked top.
pub proof fn product_epm_finish_accept_keeps_flags_readable(
    before: ProductScoreState,
    after: ProductScoreState,
)
    requires
        bridge_ezipper_flags_readable(before.bridge),
        integrated::product_epm_finish_accept_step(before, after),
    ensures bridge_ezipper_flags_readable(after.bridge),
{
    let prior = integrated::product_linked_top(before).unwrap();
    assert(prior.admission == before.bridge.admission);
    assert(epm_source_accept::epm_source_finish_accept(prior) == Some(after.bridge.admission));
    epm_accept_frames_keep_flags_readable(prior);
}

/// Live Continue: the Message Continue map step of the top field.
pub proof fn live_continue_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_continue_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    assert(integrated::live_source_product_continue_step(before.source, after.source));
    match before.source.wire.last() {
        Some(reader) => match wire_source::wire_source_next_field(reader) {
            wire_source::WireSourceTransition::Field { field, .. } =>
                product_message_continue_keeps_flags_readable(
                    before.source.product, after.source.product,
                    admission::ConcreteFrameActionModel::Continue { field },
                ),
            _ => {},
        },
        None => {},
    }
}

/// Live ContinueIndexed: the Message ContinueIndexed map step of the top
/// field over the top indexed region.
pub proof fn live_continue_indexed_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_continue_indexed_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    assert(integrated::live_source_product_continue_indexed_step(before.source, after.source));
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    match before.source.wire.last() {
        Some(reader) => match wire_source::wire_source_next_field(reader) {
            wire_source::WireSourceTransition::Field { field, .. } =>
                product_message_continue_keeps_flags_readable(
                    before.source.product, after.source.product,
                    admission::ConcreteFrameActionModel::ContinueIndexed {
                        field, parent_source,
                    },
                ),
            _ => {},
        },
        None => {},
    }
}

/// Live MapKey: the MapKey map step of the top field.
pub proof fn live_map_key_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_key_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    assert(integrated::live_source_product_map_key_step(before.source, after.source));
    match before.source.wire.last() {
        Some(reader) => match wire_source::wire_source_next_field(reader) {
            wire_source::WireSourceTransition::Field { field, .. } =>
                product_map_key_keeps_flags_readable(
                    before.source.product, after.source.product, field,
                ),
            _ => {},
        },
        None => {},
    }
}

/// Live RootScore, RootComplete and MessageChildScore: the finish-selected
/// score step of the bridge.
pub proof fn live_score_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_wf(before),
        score_bridge::bridge_score_finish_selected(before.source.product.bridge)
            == Some(after.source.product.bridge),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    let product = before.source.product;
    assert(integrated::product_score_wf(product));
    assert(admission::admission_state_wf(product.bridge.admission));
    score_finish_keeps_flags_readable(product.bridge);
}

/// Live RootCompleteNoScore: the step keeps the whole product.
pub proof fn live_root_noscore_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    returned: runtime_source::RuntimeNoScoreRootReturnImage,
)
    requires
        integrated::live_runtime_product_root_complete_noscore_step(before, after, returned),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    integrated::live_runtime_product_root_complete_noscore_keeps_state(before, after, returned);
}

/// Live MessagePush: the plain or indexed Message Push map step of the top
/// field, whichever side of the observed step holds.
pub proof fn live_message_push_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        integrated::live_runtime_product_message_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    let old = before.source.product;
    let new = after.source.product;
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            old.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
            wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
            if integrated::live_runtime_product_message_push_step(
                before, after, request, storage,
            ) {
                product_message_push_keeps_flags_readable(
                    old, new, admission::ConcreteFrameActionModel::Push {
                        field,
                        child: request.child,
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            } else {
                assert(integrated::live_runtime_product_message_push_indexed_step(
                    before, after, request, storage,
                ));
                product_message_push_keeps_flags_readable(
                    old, new, admission::ConcreteFrameActionModel::PushIndexed {
                        field,
                        child: request.child,
                        parent_source: integrated::product_admission_source_image(
                            old.indexed_regions.last(),
                        ),
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            }
        }
    }
}

/// Live MessageEpmPush: the indexed Message Push map step of a PathMap field,
/// whose child is an EPathMap frame.
pub proof fn live_message_epm_push_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        integrated::live_runtime_product_message_epm_push_indexed_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    let old = before.source.product;
    let new = after.source.product;
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            old.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        assert(integrated::live_runtime_product_message_epm_push_indexed_step(
            before, after, request, storage,
        ));
        if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
            wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
            product_message_push_keeps_flags_readable(
                old, new, admission::ConcreteFrameActionModel::PushIndexed {
                    field,
                    child: request.child,
                    parent_source: integrated::product_admission_source_image(
                        old.indexed_regions.last(),
                    ),
                    frame_bytes: before.source.runtime.frame_bytes,
                },
            );
        }
    }
}

/// Live MapValuePush: the plain or indexed MapValue map step of the top
/// field, whichever side of the observed step holds.
pub proof fn live_map_value_push_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        integrated::live_runtime_product_map_value_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    let old = before.source.product;
    let new = after.source.product;
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            old.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
            wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
            if integrated::live_runtime_product_map_value_push_step(
                before, after, request, storage,
            ) {
                product_map_value_push_keeps_flags_readable(
                    old, new, map::IntegratedAdmissionAction::MapValue {
                        field,
                        child: request.child,
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            } else {
                assert(integrated::live_runtime_product_map_value_push_indexed_step(
                    before, after, request, storage,
                ));
                product_map_value_push_keeps_flags_readable(
                    old, new, map::IntegratedAdmissionAction::MapValueIndexed {
                        field,
                        child: request.child,
                        parent_source: integrated::product_admission_source_image(
                            old.indexed_regions.last(),
                        ),
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            }
        }
    }
}

/// Live MessageChildComplete: the Message Complete map step, which pops the
/// finished child into its Message parent.
pub proof fn live_message_child_complete_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_message_child_complete_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    product_map_pop_keeps_flags_readable(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    );
}

/// Live MapValueAccept: the MapAccept map step.
pub proof fn live_map_value_accept_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_value_accept_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    product_map_pop_keeps_flags_readable(
        before.source.product, after.source.product, map::IntegratedAdmissionAction::MapAccept,
    );
}

/// Live MapEntryFinish: the MapFinish map step.
pub proof fn live_map_entry_finish_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_entry_finish_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    reveal(integrated::live_runtime_product_map_entry_finish_step);
    product_map_pop_keeps_flags_readable(
        before.source.product, after.source.product, map::IntegratedAdmissionAction::MapFinish,
    );
}

/// Live EpmMapValueAccept: the EPathMap Map-value return.
pub proof fn live_epm_map_value_accept_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        integrated::live_runtime_product_epm_map_value_accept_step(
            before, after, summary, admission_summary,
        ),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    product_epm_child_return_keeps_flags_readable(
        before.source.product, after.source.product, summary, admission_summary,
    );
}

/// Live EpmContinue: the internal EPathMap continue of the product.
pub proof fn live_epm_continue_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    event: epm::EpmEvent,
)
    requires
        integrated::live_runtime_product_wf(before),
        unwind_product::live_runtime_internal_epm_unwind_step(before, after, event),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    assert(unwind_product::live_internal_epm_unwind_step(before.source, after.source, event));
    product_epm_continue_keeps_flags_readable(
        before.source.product, after.source.product, event,
    );
}

/// Live EpmPush: the EPathMap top waits, and the requested child, which has
/// recorded nothing and has empty slot sets, is pushed.
pub proof fn live_epm_push_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: push_source::EpmPushLiveObservation,
)
    requires
        push_live::epm_push_live_after(before, observed) == Some(after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    assert(push_live::epm_push_live_ready(before, observed));
    assert(push_source::epm_push_source_ready(before, observed.push));
    let old = before.source.product;
    let new = after.source.product;
    let prior = integrated::product_linked_top(old).unwrap();
    let request = push_source::epm_push_source_request(prior, observed.push).unwrap();
    let consumed = observed.push.reducer_after.cursor.value_table_offset;
    let return_id = epm::request_return_id(request);
    let kind = epm::request_frame_kind(request);
    assert(new == push_source::epm_push_source_product_after(old, observed.push, request));
    assert(prior.admission == old.bridge.admission);
    assert(new.bridge.admission.frames == prior.admission.frames.update(
        prior.admission.frames.len() - 1,
        epm::epm_parent_waiting(
            epm::epm_advanced_admission_cursor(prior.admission.frames.last(), consumed),
            return_id, kind,
        ),
    ).push(epm::epm_assigned_child(observed.push.child, return_id)));
    assert(observed.push.child.seen == Set::<int>::empty());
    assert(observed.push.child.scalar_slots == Set::<int>::empty());
    epm_push_frames_keep_flags_readable(
        prior.admission.frames, consumed, return_id, kind, observed.push.child,
    );
}

/// Live EpmEnd: the EPathMap completion keeps the bridge.
pub proof fn live_epm_end_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: epm_terminal::EpmTerminalSourceObservation,
)
    requires
        integrated::live_runtime_product_epm_end_source_step(before, after, observed),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    integrated::live_runtime_product_epm_end_source_preserves_wf(before, after, observed);
}

/// Live EpmFinishAccept: the EPathMap finish accept of the product.
pub proof fn live_epm_finish_accept_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_epm_finish_accept_step(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    assert(integrated::live_source_product_epm_finish_accept_step(before.source, after.source));
    product_epm_finish_accept_keeps_flags_readable(before.source.product, after.source.product);
}

/// Every Live product action keeps the invariant: one lemma per action.
pub proof fn live_runtime_step_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires
        integrated::live_runtime_product_step(before, after, action),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    match action {
        LiveRuntimeProductAction::Continue =>
            live_continue_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::ContinueIndexed =>
            live_continue_indexed_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::MapKey =>
            live_map_key_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::RootScore =>
            live_score_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::RootComplete { .. } =>
            live_score_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::RootCompleteNoScore { returned } =>
            live_root_noscore_keeps_flags_readable(before, after, returned),
        LiveRuntimeProductAction::MessageChildScore =>
            live_score_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_message_push_keeps_flags_readable(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_message_epm_push_keeps_flags_readable(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_map_value_push_keeps_flags_readable(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageChildComplete =>
            live_message_child_complete_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::MapValueAccept =>
            live_map_value_accept_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::MapEntryFinish =>
            live_map_entry_finish_keeps_flags_readable(before, after),
        LiveRuntimeProductAction::EpmMapValueAccept { summary, admission_summary } =>
            live_epm_map_value_accept_keeps_flags_readable(
                before, after, summary, admission_summary,
            ),
        LiveRuntimeProductAction::EpmContinue { event } =>
            live_epm_continue_keeps_flags_readable(before, after, event),
        LiveRuntimeProductAction::EpmPush { observed } =>
            live_epm_push_keeps_flags_readable(before, after, observed),
        LiveRuntimeProductAction::EpmEnd { observed } =>
            live_epm_end_keeps_flags_readable(before, after, observed),
        LiveRuntimeProductAction::EpmFinishAccept =>
            live_epm_finish_accept_keeps_flags_readable(before, after),
    }
}

/// The Live arm of the dispatcher: RootComplete accepts its post-score
/// product, RootCompleteNoScore accepts the product it completed, and every
/// other Live action is a `live_runtime_product_step`.
pub proof fn integrated_live_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: dispatcher::IntegratedState,
    action: LiveRuntimeProductAction,
    route: dispatcher::IntegratedPushRoute,
)
    requires
        live_ezipper_flags_readable(before),
        dispatcher::integrated_live_case(before, after, action, route),
    ensures integrated_ezipper_flags_readable(after),
{
    match (action, after) {
        (
            LiveRuntimeProductAction::RootComplete { .. },
            dispatcher::IntegratedState::Accepted {
                accepted: dispatcher::IntegratedAccepted::LiveRoot { post_root_score, .. },
            },
        ) => live_runtime_step_keeps_flags_readable(before, post_root_score, action),
        (
            LiveRuntimeProductAction::RootCompleteNoScore { .. },
            dispatcher::IntegratedState::Accepted {
                accepted: dispatcher::IntegratedAccepted::LiveNoScoreRoot { pre_complete, .. },
            },
        ) => live_runtime_step_keeps_flags_readable(before, pre_complete, action),
        (_, dispatcher::IntegratedState::Active { product: next }) => {
            dispatcher::integrated_live_relation_refines_live_step(before, next, action, route);
            live_runtime_step_keeps_flags_readable(before, next, action);
        },
        _ => {},
    }
}

/// The CanonicalPath arm: a Push makes the kind-4 top wait and pushes a
/// program-1 child, a child return drops the child and resolves its kind-4
/// parent, and the root Complete accepts the prestate itself.
pub proof fn integrated_path_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: dispatcher::IntegratedState,
    driver_before: path_driver::CanonicalPathDriverProductState,
    action: path_driver::CanonicalPathDriverSourceAction,
)
    requires
        live_ezipper_flags_readable(before),
        dispatcher::integrated_path_case(before, after, driver_before, action),
    ensures integrated_ezipper_flags_readable(after),
{
    let common_before = driver_before.common;
    let admission_before = before.source.product.bridge.admission;
    assert(common_before.live.base == before);
    match path_driver::canonical_path_driver_product_step(driver_before, action).unwrap() {
        path_driver::CanonicalPathDriverProductOutcome::Active { state } => {
            assert(after == dispatcher::IntegratedState::Active { product: state.product });
            match action {
                path_driver::CanonicalPathDriverSourceAction::Push {
                    storage_observed, source_before, loop_events, source_after_scan,
                    source, child, storage, child_image, child_reader,
                } => {
                    let selected = common::canonical_path_common_push_macro_from_loop_source(
                        common_before, storage_observed.step_index, source_before,
                        loop_events, source_after_scan, source, child, storage,
                        child_image, child_reader,
                    ).unwrap();
                    assert(state.product == selected.live.base);
                    let scanned = common::canonical_path_loop_source_fold(
                        common_before, loop_events,
                    ).unwrap();
                    assert(scanned.live == common_before.live);
                    assert(common::canonical_path_common_push_from_source(
                        scanned, source, child, storage, child_image, child_reader,
                    ) == Some(selected));
                    assert(path_live::live_canonical_path_push_from_source(
                        scanned.live, source, child, storage, child_image, child_reader,
                    ) == Some(selected.live));
                    assert(selected.live.base.source.product.bridge.admission
                        == admission::admission_canonical_path_push_indexed_step(
                            admission_before, source, child, storage,
                        ).unwrap());
                    path_push_keeps_flags_readable(admission_before);
                },
                path_driver::CanonicalPathDriverSourceAction::ChildReturn { observed, .. } => {
                    let selected = common::canonical_path_common_return_from_source(
                        common_before, observed,
                    ).unwrap();
                    assert(state.product == selected.live.base);
                    let result = path_live::live_canonical_path_return_from_source(
                        common_before.live, observed,
                    ).unwrap();
                    assert(selected.live == result.state);
                    let source = path_live::live_canonical_path_retained_return_source(
                        common_before.live, observed,
                    );
                    assert(result.state.base.source.product.bridge.admission
                        == admission::admission_canonical_path_return_indexed_step(
                            admission_before, source, observed.summary,
                        ).unwrap());
                    path_return_keeps_flags_readable(admission_before);
                },
                path_driver::CanonicalPathDriverSourceAction::RootComplete { .. } => {},
            }
        },
        path_driver::CanonicalPathDriverProductOutcome::Terminal { .. } => {},
    }
}

/// The ACT Forward arm: the batch advances the EPathMap top's byte cursor.
pub proof fn act_forward_keeps_flags_readable(
    before: LiveRuntimeProductState,
    width: usize,
    commit: act_source::AcceptedLineCommitSource,
    pending: epm::PathObligation,
    state_id: usize,
)
    requires
        before.source.product.bridge.admission.frames.len() > 0,
        live_ezipper_flags_readable(before),
    ensures
        live_ezipper_flags_readable(
            dispatcher::integrated_act_forward_after(before, width, commit, pending, state_id),
        ),
{
    let product = before.source.product;
    let frame = dispatcher::integrated_act_forward_frame(
        product, width, commit, pending, state_id,
    );
    let consumed = frame.cursor.value_table_offset;
    let after = dispatcher::integrated_act_forward_after(before, width, commit, pending, state_id);
    assert(after.source.product == dispatcher::integrated_act_forward_product(product, frame));
    assert(after.source.product.bridge.admission
        == dispatcher::integrated_epm_cursor_admission(product.bridge.admission, consumed));
    cursor_advanced_flags_readable(product.bridge.admission.frames, consumed);
}

/// The ACT Unwind arm: the batch of `width` events leaves the admission state
/// or advances the EPathMap top's byte cursor.
pub proof fn act_unwind_keeps_flags_readable(
    before: LiveRuntimeProductState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires
        dispatcher::integrated_act_unwind_guard(before, run, prefix, start, width, facts),
        live_ezipper_flags_readable(before),
    ensures
        live_ezipper_flags_readable(
            dispatcher::integrated_act_unwind_after(before, run, prefix, width),
        ),
{
    let product = before.source.product;
    let source = dispatcher::integrated_act_unwind_source(before);
    unwind_product::unwind_product_virtual_matches_linked_source(
        product, source, run, prefix, start, width, facts, width,
    );
    let view = unwind_product::unwind_product_virtual_state(product, source, run, prefix, width);
    let after = dispatcher::integrated_act_unwind_after(before, run, prefix, width);
    assert(after.source.product == view);
    assert(source.admission == product.bridge.admission);
    assert(view.bridge.admission == unwind::linked_unwind_virtual_admission(source, width));
    if width > 0 {
        assert(product.bridge.admission.frames.len() > 0);
        cursor_advanced_flags_readable(
            product.bridge.admission.frames, source.epm.cursor.value_table_offset,
        );
    }
}

/// The EPathMap obligation returns pop the returning child and resolve its
/// EPathMap parent.
pub proof fn obligation_return_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        obligation_return::epm_obligation_return_effects(before, after),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    let frames = before.source.product.bridge.admission.frames;
    let count = frames.len();
    let popped = after.source.product.bridge.admission.frames;
    let resolved = epm::epm_parent_resolved(frames[count - 2]);
    epm_resolved_parent_keeps_flags_readable(frames);
    assert(popped.last() == resolved);
    assert(popped =~= frames.subrange(0, count - 1).update(count - 2, resolved));
}

/// The EPathMap handoff keeps the bridge.
pub proof fn handoff_keeps_flags_readable(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires
        handoff::live_runtime_epm_handoff_step(before, after, backing, plan, validated),
        live_ezipper_flags_readable(before),
    ensures live_ezipper_flags_readable(after),
{
    reveal(handoff::live_runtime_epm_handoff_step);
    assert(score_bridge::bridge_same(before.source.product.bridge, after.source.product.bridge));
}

/// Every action of the integrated dispatcher keeps the invariant: one lemma
/// per action family.
pub proof fn integrated_step_keeps_flags_readable(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_ezipper_flags_readable(before),
        dispatcher::integrated_step(before, after, action),
    ensures integrated_ezipper_flags_readable(after),
{
    match before {
        dispatcher::IntegratedState::Accepted { .. } => {},
        dispatcher::IntegratedState::Active { product } => match action {
            dispatcher::IntegratedAction::Live { live_action, route } =>
                integrated_live_keeps_flags_readable(product, after, live_action, route),
            dispatcher::IntegratedAction::CanonicalPath { driver_before, driver_action } =>
                integrated_path_keeps_flags_readable(product, after, driver_before, driver_action),
            dispatcher::IntegratedAction::ActForward {
                width, commit, pending, state_id, ..
            } => act_forward_keeps_flags_readable(product, width, commit, pending, state_id),
            dispatcher::IntegratedAction::ActUnwind {
                run, prefix, start, width, unwind_facts,
            } => act_unwind_keeps_flags_readable(
                product, run, prefix, start, width, unwind_facts,
            ),
            dispatcher::IntegratedAction::EpmRawParReturn => {
                if let dispatcher::IntegratedState::Active { product: next } = after {
                    obligation_return::live_runtime_epm_raw_par_return_has_effects(product, next);
                    obligation_return_keeps_flags_readable(product, next);
                }
            },
            dispatcher::IntegratedAction::EpmNestedReturn => {
                if let dispatcher::IntegratedState::Active { product: next } = after {
                    obligation_return::live_runtime_epm_nested_return_has_effects(product, next);
                    obligation_return_keeps_flags_readable(product, next);
                }
            },
            dispatcher::IntegratedAction::EpmHandoff { backing, plan, validated } => {
                if let dispatcher::IntegratedState::Active { product: next } = after {
                    handoff_keeps_flags_readable(product, next, backing, plan, validated);
                }
            },
        },
    }
}

/// Every state of an integrated trace whose first state satisfies the
/// invariant satisfies it.
pub proof fn integrated_trace_keeps_flags_readable(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    index: int,
)
    requires
        dispatcher::integrated_trace(states, actions),
        integrated_ezipper_flags_readable(states[0]),
        0 <= index < states.len(),
    ensures integrated_ezipper_flags_readable(states[index]),
    decreases index,
{
    if index > 0 {
        let previous = index - 1;
        integrated_trace_keeps_flags_readable(states, actions, previous);
        assert(0 <= previous < actions.len());
        assert(dispatcher::integrated_step(
            states[previous], states[previous + 1], actions[previous],
        ));
        assert(previous + 1 == index);
        integrated_step_keeps_flags_readable(
            states[previous], states[previous + 1], actions[previous],
        );
    }
}

/// An active state whose admission frames have recorded no scalar occurrence
/// and hold slot 4 in neither slot set satisfies the invariant; the admission
/// root of `admission_initial_state` has this shape.
pub proof fn unrecorded_start_is_readable(product: LiveRuntimeProductState)
    requires
        forall|i: int| 0 <= i < product.source.product.bridge.admission.frames.len() ==> {
            &&& (#[trigger] product.source.product.bridge.admission.frames[i])
                .scalar_occurrences.len() == 0
            &&& !product.source.product.bridge.admission.frames[i].seen.contains(4)
            &&& !product.source.product.bridge.admission.frames[i].scalar_slots.contains(4)
        },
    ensures
        integrated_ezipper_flags_readable(dispatcher::IntegratedState::Active { product }),
{
    let frames = product.source.product.bridge.admission.frames;
    assert forall|i: int| 0 <= i < frames.len() implies
        #[trigger] ezipper_flag_slot_readable(frames[i]) by {
        unrecorded_frame_flag_readable(frames[i]);
    }
}

/// On every active state of an integrated trace whose first state satisfies
/// the invariant, a top frame that is an EZipper reads its connective flag:
/// `bridge_bool_slot(top, 4, 5)` is Some.
pub proof fn reachable_ezipper_flag_slot_is_readable(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    index: int,
)
    requires
        dispatcher::integrated_trace(states, actions),
        integrated_ezipper_flags_readable(states[0]),
        0 <= index < states.len(),
    ensures
        match states[index] {
            dispatcher::IntegratedState::Active { product } => {
                let frames = product.source.product.bridge.admission.frames;
                frames.len() > 0 && ezipper_frame(frames.last())
                    ==> score_bridge::bridge_bool_slot(frames.last(), 4, 5).is_some()
            },
            dispatcher::IntegratedState::Accepted { .. } => true,
        },
{
    integrated_trace_keeps_flags_readable(states, actions, index);
    if let dispatcher::IntegratedState::Active { product } = states[index] {
        let frames = product.source.product.bridge.admission.frames;
        if frames.len() > 0 {
            assert(ezipper_flag_slot_readable(frames[frames.len() - 1]));
        }
    }
}

}
