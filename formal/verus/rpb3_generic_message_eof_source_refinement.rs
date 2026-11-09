//! Source-shaped message EOF selection across generated protobuf programs.
//!
//! `MessageFinishSource::before` in production borrows the immutable message
//! frame across `frame.finish(scores)` and retains the score-arena length.
//! `MessageEofObservation` checks the returned kind and component tuple. The
//! observation below names the additional ghost projection needed to compare
//! that execution with the existing score-admission selector. It applies to
//! every generated message program; the selected model result may carry a
//! score or may leave the score arena unchanged.
//!
//! This is a conditional formal bridge, not a Rust extraction theorem. The
//! production checks do not yet extract the complete field/accept trace,
//! appended score entries, or equality of the returned noncomponent summary
//! with the modeled result. They also do not establish that every successful
//! production reducer enables `bridge_score_finish_selected`.

use vstd::prelude::*;

#[path = "rpb3_generated_descriptor_projection.rs"]
mod rpb3_generated_descriptor_projection;
use rpb3_generated_descriptor_projection as generated;

use crate::{
    rpb3_admission_cursor as admission, rpb3_score_admission_bridge as score_bridge,
    rpb3_score_transcript as score,
};

verus! {

/// The component fields are the source-visible subset of
/// `MessageEofObservation::component`. Canonical-path stability is retained as
/// an observed Rust result; the generic score bridge does not derive it.
pub struct GenericComponentEofSource {
    pub family: admission::AdmissionFamily,
    pub complete: bool,
    pub canonical_path_stable: bool,
    pub score_handle: usize,
}

/// Each field event is the checked result of one indexed cursor advance.
/// `Accepted` is the corresponding `MessageFrame::accept` continuation, which
/// must follow a push and does not advance the parent's cursor. The scalar
/// occurrence and returned summary are projections of the actual retained
/// semantic slot, not EOF values chosen independently of the visited fields.
pub enum GenericMessageSourceEvent {
    Scalar {
        field: admission::IndexedCanonicalFieldModel,
    },
    Push {
        field: admission::IndexedCanonicalFieldModel,
        body_id: u64,
    },
    Accepted {
        returned: admission::AdmissionReturnedSummary,
    },
}

pub struct GenericMessageSourceFold {
    pub cursor: usize,
    pub last_slot: Option<usize>,
    pub ghost seen: Set<int>,
    pub ghost scalar_slots: Set<int>,
    pub ghost scalar_occurrences: Seq<admission::AdmissionScalarOccurrence>,
    pub ghost returns: Seq<admission::AdmissionReturnedSummary>,
    pub ghost pending: Option<(usize, u32, u64, Seq<u8>)>,
}

pub open spec fn generic_message_source_initial() -> GenericMessageSourceFold {
    GenericMessageSourceFold {
        cursor: 0,
        last_slot: None,
        seen: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        pending: None,
    }
}

/// A successful fold establishes field order and strict byte progress, plus
/// one accepted return for each pushed child before the next field or EOF.
/// It does not assume that a particular producer supplied these events.
pub open spec fn generic_message_source_event_step(
    before: GenericMessageSourceFold,
    event: GenericMessageSourceEvent,
    program_type_id: u16,
    family: admission::AdmissionFamily,
    slot_count: usize,
    body_len: usize,
    indexed: admission::AdmissionIndexedSourceImage,
) -> Option<GenericMessageSourceFold> {
    match event {
        GenericMessageSourceEvent::Scalar { field } => {
            match generated::generated_descriptor(program_type_id, field.number) {
                Some(descriptor) => {
                    let slot = descriptor.slot;
                    let repeated = descriptor.repeated;
                    let cursor = admission::IndexedCanonicalFieldCursorModel {
                        region_start: 0,
                        region_end: body_len,
                        next: before.cursor,
                        source_bytes: indexed.bytes,
                    };
                    if before.pending.is_some()
                        || !admission::admission_indexed_source_wf(indexed)
                        || !admission::indexed_field_at_cursor(cursor, field)
                        || !admission::admission_scalar_descriptor_valid(
                            program_type_id, field, slot, repeated, descriptor.scalar_kind,
                        )
                        || slot >= slot_count
                        || !admission::slot_admissible(
                            before.last_slot, slot, slot_count, repeated,
                        )
                        || (!repeated && before.seen.contains(slot as int))
                        || !admission::admission_field_tag_valid(
                            family, slot, field.number, false, repeated,
                        )
                    {
                        None
                    } else {
                        match admission::admission_indexed_source_position(
                            indexed, field.encoded_start,
                        ) {
                            None => None,
                            Some(position) => {
                                let occurrence = admission::AdmissionScalarOccurrence {
                                    slot,
                                    tag: field.number,
                                    scalar_kind: descriptor.scalar_kind,
                                    varint_value: field.decoded_varint,
                                    physical_source: position.source,
                                    physical_start: position.offset as int,
                                    bytes: field.payload_bytes,
                                };
                                Some(GenericMessageSourceFold {
                                    cursor: field.encoded_end,
                                    last_slot: Some(slot),
                                    seen: before.seen.insert(slot as int),
                                    scalar_slots: before.scalar_slots.insert(slot as int),
                                    scalar_occurrences: before.scalar_occurrences.push(occurrence),
                                    returns: before.returns,
                                    pending: None,
                                })
                            },
                        }
                    }
                },
                None => None,
            }
        },
        GenericMessageSourceEvent::Push { field, body_id } => {
            match generated::generated_descriptor(program_type_id, field.number) {
                Some(descriptor) => {
                    let slot = descriptor.slot;
                    let repeated = descriptor.repeated;
                    let cursor = admission::IndexedCanonicalFieldCursorModel {
                        region_start: 0,
                        region_end: body_len,
                        next: before.cursor,
                        source_bytes: indexed.bytes,
                    };
                    if before.pending.is_some()
                        || !admission::admission_indexed_source_wf(indexed)
                        || !admission::indexed_field_at_cursor(cursor, field)
                        || descriptor.frame_kind == 0 || descriptor.wire != field.wire
                        || slot >= slot_count
                        || !admission::slot_admissible(
                            before.last_slot, slot, slot_count, repeated,
                        )
                        || (!repeated && before.seen.contains(slot as int))
                        || !admission::admission_field_tag_valid(
                            family, slot, field.number, true, repeated,
                        )
                    {
                        None
                    } else {
                        Some(GenericMessageSourceFold {
                            cursor: field.encoded_end,
                            last_slot: Some(slot),
                            seen: before.seen.insert(slot as int),
                            scalar_slots: before.scalar_slots,
                            scalar_occurrences: before.scalar_occurrences,
                            returns: before.returns,
                            pending: Some((slot, field.number, body_id, field.payload_bytes)),
                        })
                    }
                },
                None => None,
            }
        },
        GenericMessageSourceEvent::Accepted { returned } => {
            match before.pending {
                Some((slot, tag, body_id, body))
                    if slot == returned.slot && tag == returned.tag
                        && body_id == returned.body_id
                        && body == returned.body_bytes => {
                    Some(GenericMessageSourceFold {
                        cursor: before.cursor,
                        last_slot: before.last_slot,
                        seen: before.seen,
                        scalar_slots: before.scalar_slots,
                        scalar_occurrences: before.scalar_occurrences,
                        returns: before.returns.push(returned),
                        pending: None,
                    })
                },
                _ => None,
            }
        },
    }
}

pub open spec fn generic_message_source_fold(
    events: Seq<GenericMessageSourceEvent>,
    program_type_id: u16,
    family: admission::AdmissionFamily,
    slot_count: usize,
    body_len: usize,
    indexed: admission::AdmissionIndexedSourceImage,
) -> Option<GenericMessageSourceFold>
    decreases events.len(),
{
    if events.len() == 0 {
        Some(generic_message_source_initial())
    } else {
        match generic_message_source_fold(
            events.subrange(0, events.len() - 1), program_type_id, family,
            slot_count, body_len, indexed,
        ) {
            Some(before) => generic_message_source_event_step(
                before, events.last(), program_type_id, family, slot_count,
                body_len, indexed,
            ),
            None => None,
        }
    }
}

/// Pre-finish frame fields and append-only score images around Rust EOF.
/// `indexed` and `events` must be projected from the indexed body and live
/// `MessageFrame` transitions. The EOF semantic collections are reconstructed
/// by the checked fold instead of supplied independently by the caller.
pub struct GenericMessageEofSourceObservation {
    pub program_type_id: u16,
    pub family: admission::AdmissionFamily,
    pub body_id: u64,
    pub ghost indexed: admission::AdmissionIndexedSourceImage,
    pub body_len: usize,
    pub cursor_after: usize,
    pub slot_count: usize,
    pub ghost events: Seq<GenericMessageSourceEvent>,
    pub ghost score_entries_before: Seq<score::ScoreEntry>,
    pub ghost score_entries_after: Seq<score::ScoreEntry>,
    pub component: Option<GenericComponentEofSource>,
}

/// A score is selected by the model, including the nonsemantic score for an
/// incomplete component, or no model score candidate is selected. `NoScore`
/// makes no claim about the returned Rust semantic variant.
pub enum GenericMessageEofModelOutcome {
    Scored {
        after: score_bridge::ScoreAdmissionBridge,
        score_handle: usize,
        score_bytes: Seq<u8>,
        semantic_complete: bool,
        body_id: u64,
        body_bytes: Seq<u8>,
    },
    NoScore {
        after: score_bridge::ScoreAdmissionBridge,
        body_id: u64,
        body_bytes: Seq<u8>,
    },
}

pub open spec fn component_family(family: admission::AdmissionFamily) -> bool {
    match family {
        admission::AdmissionFamily::Send
        | admission::AdmissionFamily::Receive
        | admission::AdmissionFamily::New
        | admission::AdmissionFamily::Match
        | admission::AdmissionFamily::Bundle
        | admission::AdmissionFamily::If
        | admission::AdmissionFamily::CostTokenStack
        | admission::AdmissionFamily::Expr
        | admission::AdmissionFamily::Connective
        | admission::AdmissionFamily::GUnforgeable => true,
        _ => false,
    }
}

/// Independent pre-state equalities, rather than an assumed selected result.
/// The `component` requirement includes incomplete semantic components; their
/// `complete` flag is checked against the selected score's semantic bit below.
#[verifier::opaque]
pub open spec fn generic_message_eof_source_matches_before(
    before: score_bridge::ScoreAdmissionBridge,
    observed: GenericMessageEofSourceObservation,
) -> bool {
    if !score_bridge::bridge_well_formed(before)
        || !admission::admission_state_wf(before.admission)
        || before.admission.frames.len() == 0
    {
        false
    } else {
        let top = before.admission.frames.last();
        let folded = generic_message_source_fold(
            observed.events, observed.program_type_id, observed.family,
            observed.slot_count, observed.body_len, observed.indexed,
        );
        &&& top.frame_kind == 1
        &&& top.waiting_id.is_none()
        &&& top.score_handle.is_none()
        &&& top.cursor == top.region_end
        &&& top.program_type_id == observed.program_type_id
        &&& top.family == observed.family
        &&& top.body_id == observed.body_id
        &&& admission::admission_indexed_source_wf(observed.indexed)
        &&& top.source_bytes == observed.indexed.bytes
        &&& observed.body_len as int == observed.indexed.bytes.len()
        &&& top.cursor == observed.cursor_after
        &&& top.slot_count == observed.slot_count
        &&& match folded {
            Some(trace) => trace.pending.is_none()
                && trace.cursor == observed.cursor_after
                && top.last_slot == trace.last_slot
                && top.seen == trace.seen
                && top.scalar_slots == trace.scalar_slots
                && top.scalar_occurrences == trace.scalar_occurrences
                && top.returns == trace.returns,
            None => false,
        }
        &&& before.transcript.entries == observed.score_entries_before
        &&& observed.score_entries_before.len() <= observed.score_entries_after.len()
        &&& observed.score_entries_after.subrange(
            0, observed.score_entries_before.len() as int,
        ) == observed.score_entries_before
        &&& match observed.component {
            Some(component) => component_family(top.family)
                && component.family == top.family,
            None => !component_family(top.family),
        }
    }
}

/// The selected result is computed by the shared formal score transition.
/// The observed score image and component tuple are checked *against* that result;
/// neither is used to choose a score or to set completeness. `None` means the
/// source/model binding failed, not that Rust admission failed.
#[verifier::opaque]
pub open spec fn generic_message_eof_from_source(
    before: score_bridge::ScoreAdmissionBridge,
    observed: GenericMessageEofSourceObservation,
) -> Option<GenericMessageEofModelOutcome> {
    if !generic_message_eof_source_matches_before(before, observed) {
        None
    } else {
        match score_bridge::bridge_score_finish_selected(before) {
            Some(after) => {
                if after.admission.frames.len() != before.admission.frames.len()
                    || after.admission.frames.len() == 0
                    || after.transcript.entries != observed.score_entries_after
                {
                    None
                } else {
                    match after.admission.frames.last().score_handle {
                        Some(handle) if (handle as int) < after.admission.arena.len() => {
                            let complete = after.admission.arena[handle as int].semantic;
                            let score_bytes = after.admission.arena[handle as int]
                                .transcript_bytes;
                            if match observed.component {
                                Some(component) => component.score_handle == handle
                                    && component.complete == complete
                                    && complete == admission::admission_family_complete(
                                        before.admission.frames.last(),
                                    )
                                    && after.admission.arena[handle as int].body_id
                                        == before.admission.frames.last().body_id,
                                None => true,
                            } {
                                Some(GenericMessageEofModelOutcome::Scored {
                                    after,
                                    score_handle: handle,
                                    score_bytes,
                                    semantic_complete: complete,
                                    body_id: observed.body_id,
                                    body_bytes: observed.indexed.bytes,
                                })
                            } else {
                                None
                            }
                        },
                        _ => None,
                    }
                }
            },
            None => {
                if observed.score_entries_after == observed.score_entries_before
                    && observed.component.is_none()
                {
                    Some(GenericMessageEofModelOutcome::NoScore {
                        after: before,
                        body_id: observed.body_id,
                        body_bytes: observed.indexed.bytes,
                    })
                } else {
                    None
                }
            },
        }
    }
}

/// An accepted source observation has the unique selected model score,
/// transcript delta, body, and semantic-completeness bit. The caller must
/// establish that the Rust reducer produced an accepted observation.
pub proof fn generic_message_eof_selected_result_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: GenericMessageEofSourceObservation,
)
    requires
        generic_message_eof_source_matches_before(before, observed),
        generic_message_eof_from_source(before, observed).is_some(),
    ensures
        generic_message_source_fold(
            observed.events, observed.program_type_id, observed.family,
            observed.slot_count, observed.body_len, observed.indexed,
        ).is_some(),
        match generic_message_eof_from_source(before, observed) {
        Some(GenericMessageEofModelOutcome::Scored {
            after, score_handle, score_bytes, semantic_complete, body_id, body_bytes,
        }) => {
            &&& score_bridge::bridge_score_finish_selected(before) == Some(after)
            &&& score_bridge::bridge_score_source_step(before, after)
            &&& score_bridge::bridge_well_formed(after)
            &&& admission::admission_state_wf(after.admission)
            &&& after.transcript.entries == observed.score_entries_after
            &&& after.admission.frames.last().score_handle == Some(score_handle)
            &&& (score_handle as int) < after.admission.arena.len()
            &&& score_bytes == after.admission.arena[score_handle as int].transcript_bytes
            &&& score_bytes == after.transcript.entries[score_handle as int].transcript
            &&& semantic_complete == after.admission.arena[score_handle as int].semantic
            &&& body_id == before.admission.frames.last().body_id
            &&& body_bytes == before.admission.frames.last().source_bytes
            &&& match observed.component {
                Some(component) => component.family == before.admission.frames.last().family
                    && component.score_handle == score_handle
                    && component.complete == semantic_complete
                    && semantic_complete == admission::admission_family_complete(
                        before.admission.frames.last(),
                    )
                    && after.admission.arena[score_handle as int].body_id
                        == before.admission.frames.last().body_id,
                None => !component_family(before.admission.frames.last().family),
            }
        },
        Some(GenericMessageEofModelOutcome::NoScore { after, body_id, body_bytes }) => {
            &&& score_bridge::bridge_score_finish_selected(before).is_none()
            &&& after.admission == before.admission
            &&& after.transcript == before.transcript
            &&& observed.score_entries_after == observed.score_entries_before
            &&& body_id == before.admission.frames.last().body_id
            &&& body_bytes == before.admission.frames.last().source_bytes
            &&& !component_family(before.admission.frames.last().family)
        },
        None => false,
        },
{
    reveal(generic_message_eof_from_source);
    reveal(generic_message_eof_source_matches_before);
    score_bridge::bridge_score_finish_selected_preserves_checked(before);
    if let Some(GenericMessageEofModelOutcome::Scored {
        after, score_handle, ..
    }) = generic_message_eof_from_source(before, observed) {
        assert(score_bridge::bridge_well_formed(after));
        assert((score_handle as int) < after.admission.arena.len());
        assert(after.admission.arena[score_handle as int].transcript_bytes
            == after.transcript.entries[score_handle as int].transcript);
    }
}

/// For a reachable ten-family component EOF, a selected model score has a
/// unique source transition. This includes CostTokenStack; incomplete semantic
/// components select their nonsemantic score when the selector is enabled.
/// Selector enabledness and the Rust-to-ghost observation remain separate.
pub proof fn generic_component_eof_selected_unique(
    before: score_bridge::ScoreAdmissionBridge,
)
    requires
        score_bridge::bridge_reducer_reachable(before),
        score_bridge::bridge_component_score_eof_scope(before),
        score_bridge::bridge_score_finish_selected(before).is_some(),
    ensures {
        let after = score_bridge::bridge_score_finish_selected(before).unwrap();
        &&& score_bridge::bridge_score_source_step(before, after)
        &&& forall|other: score_bridge::ScoreAdmissionBridge|
            score_bridge::bridge_score_source_step(before, other)
                ==> score_bridge::bridge_same(after, other)
    },
{
    score_bridge::bridge_score_finish_selected_is_source_step(before);
    let after = score_bridge::bridge_score_finish_selected(before).unwrap();
    score_bridge::bridge_reachable_component_score_eof_source_finish_unique(before, after);
}

} // verus!
