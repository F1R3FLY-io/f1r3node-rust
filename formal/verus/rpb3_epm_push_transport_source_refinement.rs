//! The transport chunks derive scalar events from source cursor positions and
//! accepted Forward/Unwind widths. The linked-state sequence is a constrained
//! proof witness: showing that actual Rust reducer updates produce this
//! sequence remains an extraction obligation. The returned reducer frame,
//! request, and child also still need projection from the Rust push path.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_push_source_action_refinement as source,
    rpb3_runtime_frame_source_refinement as runtime,
};

verus! {

pub enum EpmPushTransportChunk {
    Scalar { event: epm::EpmEvent },
    Forward { start: usize, width: usize },
    Unwind { start: usize, width: usize },
}

pub open spec fn epm_push_chunk_len(chunk: EpmPushTransportChunk) -> nat {
    match chunk {
        EpmPushTransportChunk::Scalar { .. } => 1,
        EpmPushTransportChunk::Forward { width, .. } => 2 * width as nat,
        EpmPushTransportChunk::Unwind { width, .. } => width as nat,
    }
}

pub open spec fn epm_push_chunk_event(
    chunk: EpmPushTransportChunk, index: int,
) -> epm::EpmEvent {
    match chunk {
        EpmPushTransportChunk::Scalar { event } => event,
        EpmPushTransportChunk::Forward { start, .. } => {
            let source_offset = (start as int + index / 2) as usize;
            if index % 2 == 0 {
                epm::EpmEvent::Edge {
                    source_offset,
                    origin: epm::EdgeOrigin::Direct,
                    effect: epm::PathEdgeEffect::Payload,
                }
            } else {
                epm::EpmEvent::Value {
                    has_value: false, value_body: None, source_offset,
                }
            }
        },
        EpmPushTransportChunk::Unwind { start, width } =>
            epm::EpmEvent::Leave {
                source_offset: (start as int + width as int - 1 - index) as usize,
            },
    }
}

pub open spec fn epm_push_chunk_events(chunk: EpmPushTransportChunk) -> Seq<epm::EpmEvent> {
    Seq::new(epm_push_chunk_len(chunk), |i: int| epm_push_chunk_event(chunk, i))
}

pub open spec fn epm_push_batch_commit_last_offset(
    chunk: EpmPushTransportChunk,
) -> Option<usize> {
    match chunk {
        EpmPushTransportChunk::Scalar { .. } => None,
        EpmPushTransportChunk::Forward { start, width } =>
            if width > 0 && start as int + width as int <= usize::MAX as int {
                Some((start as int + width as int - 1) as usize)
            } else { None },
        EpmPushTransportChunk::Unwind { start, width } =>
            if width > 0 && start as int + width as int <= usize::MAX as int {
                Some(start)
            } else { None },
    }
}

pub proof fn epm_push_batch_commit_matches_last_scalar_source(
    chunk: EpmPushTransportChunk,
)
    requires
        match chunk {
            EpmPushTransportChunk::Scalar { .. } => false,
            EpmPushTransportChunk::Forward { start, width }
            | EpmPushTransportChunk::Unwind { start, width } =>
                width > 0 && start as int + width as int <= usize::MAX as int,
        },
    ensures
        epm_push_batch_commit_last_offset(chunk).is_some(),
        match epm_push_chunk_event(chunk, epm_push_chunk_len(chunk) as int - 1) {
            epm::EpmEvent::Value {
                has_value: false, value_body: None, source_offset,
            } => match chunk {
                EpmPushTransportChunk::Forward { .. } =>
                    Some(source_offset) == epm_push_batch_commit_last_offset(chunk),
                _ => false,
            },
            epm::EpmEvent::Leave { source_offset } => match chunk {
                EpmPushTransportChunk::Unwind { .. } =>
                    Some(source_offset) == epm_push_batch_commit_last_offset(chunk),
                _ => false,
            },
            _ => false,
        },
{
    match chunk {
        EpmPushTransportChunk::Scalar { .. } => assert(false),
        EpmPushTransportChunk::Forward { start, width } => {
            assert((2 * width as int - 1) / 2 == width as int - 1) by (nonlinear_arith)
                requires width > 0;
            assert((2 * width as int - 1) % 2 == 1) by (nonlinear_arith)
                requires width > 0;
        },
        EpmPushTransportChunk::Unwind { start, width } => {},
    }
}

#[verifier::opaque]
pub open spec fn epm_push_transport_events(
    chunks: Seq<EpmPushTransportChunk>,
) -> Seq<epm::EpmEvent>
    decreases chunks.len(),
{
    if chunks.len() == 0 {
        Seq::empty()
    } else {
        epm_push_transport_events(chunks.drop_last())
            + epm_push_chunk_events(chunks.last())
    }
}

#[verifier::opaque]
pub open spec fn epm_push_transport_source_wf(
    initial: epm::EpmFrame,
    chunks: Seq<EpmPushTransportChunk>,
) -> bool {
    let events = epm_push_transport_events(chunks);
    &&& epm::frame_layout_wf(initial)
    &&& initial.cursor.at as int + events.len()
        <= epm::cursor_event_count(initial) as int
    &&& forall|i: int| 0 <= i < chunks.len() ==> match #[trigger] chunks[i] {
        EpmPushTransportChunk::Scalar { event } => match event {
            epm::EpmEvent::End => false,
            _ => true,
        },
        EpmPushTransportChunk::Forward { start, width } =>
            width > 0 && start as int + width as int <= initial.trie_end - initial.trie_start,
        EpmPushTransportChunk::Unwind { start, width } =>
            width > 0 && start as int + width as int <= initial.trie_end - initial.trie_start,
    }
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger]
        epm::cursor_event_at(initial, (initial.cursor.at as int + i) as usize)
            == epm::wire_event(events[i])
}

pub open spec fn epm_push_transport_linked_chain(
    states: Seq<epm::LinkedEpmState>,
    chunks: Seq<EpmPushTransportChunk>,
) -> bool {
    let events = epm_push_transport_events(chunks);
    &&& states.len() == events.len() + 1
    &&& states.len() > 0
    &&& epm::linked_indexed_sources(states[0])
    &&& epm_push_transport_source_wf(states[0].epm, chunks)
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger]
        epm::linked_epm_continue(states[i], states[i + 1], events[i])
}

pub proof fn epm_push_linked_continue_cursor_step(
    before: epm::LinkedEpmState,
    after: epm::LinkedEpmState,
    event: epm::EpmEvent,
)
    requires epm::linked_epm_continue(before, after, event),
    ensures
        after.epm.cursor.at as int == before.epm.cursor.at as int + 1,
        after.epm.cursor.act_plan == before.epm.cursor.act_plan,
        after.epm.cursor.validated_plan == before.epm.cursor.validated_plan,
        after.epm.snapshot == before.epm.snapshot,
        after.indexed_regions == before.indexed_regions,
        after.admission.next_obligation_id == before.admission.next_obligation_id,
        epm::linked_indexed_sources(after),
{
    assert(epm::event_step(
        before.epm, after.epm, event,
        before.admission.next_obligation_id, epm::EpmAction::Continue,
    ));
    assert(epm::cursor_event_step(before.epm, after.epm, event));
    match event {
        epm::EpmEvent::End => assert(false),
        _ => {},
    }
}

pub proof fn epm_push_batch_commit_preserves_source_anchor(
    before: epm::LinkedEpmState,
    after: epm::LinkedEpmState,
    chunk: EpmPushTransportChunk,
)
    requires
        before.epm.mode != epm::EpmMode::Empty,
        match chunk {
            EpmPushTransportChunk::Scalar { .. } => false,
            EpmPushTransportChunk::Forward { start, width }
            | EpmPushTransportChunk::Unwind { start, width } =>
                width > 0 && start as int + width as int <= usize::MAX as int,
        },
        epm::linked_epm_continue(
            before, after,
            epm_push_chunk_event(chunk, epm_push_chunk_len(chunk) as int - 1),
        ),
    ensures
        after.epm.last_source == epm::trie_position(
            before.epm, epm_push_batch_commit_last_offset(chunk).unwrap(),
        ),
{
    epm_push_batch_commit_matches_last_scalar_source(chunk);
    let offset = epm_push_batch_commit_last_offset(chunk).unwrap();
    let event = epm_push_chunk_event(chunk, epm_push_chunk_len(chunk) as int - 1);
    assert(epm::event_step(
        before.epm, after.epm, event,
        before.admission.next_obligation_id, epm::EpmAction::Continue,
    ));
    match chunk {
        EpmPushTransportChunk::Scalar { .. } => assert(false),
        EpmPushTransportChunk::Forward { .. } => {
            assert(event == epm::EpmEvent::Value {
                has_value: false, value_body: None, source_offset: offset,
            });
        },
        EpmPushTransportChunk::Unwind { .. } => {
            assert(event == epm::EpmEvent::Leave { source_offset: offset });
        },
    }
    assert(after.epm.last_source == epm::topology_position(before.epm, offset));
    assert(epm::topology_position(before.epm, offset)
        == epm::trie_position(before.epm, offset));
}

pub proof fn epm_push_transport_prefix_is_cursor_exact(
    states: Seq<epm::LinkedEpmState>,
    chunks: Seq<EpmPushTransportChunk>,
    upto: int,
)
    requires
        epm_push_transport_linked_chain(states, chunks),
        0 <= upto < states.len(),
    ensures
        states[upto].epm.cursor.at as int
            == states[0].epm.cursor.at as int + upto,
        states[upto].epm.cursor.act_plan == states[0].epm.cursor.act_plan,
        states[upto].epm.cursor.validated_plan == states[0].epm.cursor.validated_plan,
        states[upto].epm.snapshot == states[0].epm.snapshot,
        states[upto].indexed_regions == states[0].indexed_regions,
        states[upto].admission.next_obligation_id
            == states[0].admission.next_obligation_id,
        epm::linked_indexed_sources(states[upto]),
    decreases upto,
{
    if upto > 0 {
        epm_push_transport_prefix_is_cursor_exact(states, chunks, upto - 1);
        let events = epm_push_transport_events(chunks);
        assert(0 <= upto - 1 < events.len());
        assert(forall|i: int| 0 <= i < events.len() ==>
            epm::linked_epm_continue(states[i], states[i + 1], events[i]));
        assert(epm::linked_epm_continue(
            states[upto - 1], states[upto], events[upto - 1],
        ));
        epm_push_linked_continue_cursor_step(
            states[upto - 1], states[upto], events[upto - 1],
        );
    }
}

pub struct EpmPushReturnedSource {
    pub ghost terminal_event: epm::EpmEvent,
    pub ghost reducer_after: epm::EpmFrame,
    pub ghost returned_request: epm::ChildRequest,
    pub ghost child: admission::AdmissionFrameState,
    pub ghost selected_child: epm::IndexedRegionImage,
    pub selected_view_id: usize,
    pub parent_owner_id: usize,
    pub parent_field: u32,
    pub par_program_id: usize,
    pub ghost rust_request: EpmPushRustRequest,
    pub actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    pub after_len: usize,
    pub after_capacity: usize,
    pub stats_frame_variant_bytes: usize,
}

pub enum EpmPushRustRequest {
    Message {
        ghost body: epm::IndexedRegionImage,
        view_id: usize,
        program_id: usize,
    },
    EPathMap {
        ghost body: epm::IndexedRegionImage,
        view_id: usize,
        owner_id: usize,
        field: u32,
    },
}

pub open spec fn epm_push_rust_request_matches(returned: EpmPushReturnedSource) -> bool {
    &&& returned.selected_child == epm::request_body(returned.returned_request)
    &&& epm::indexed_image_wf(returned.selected_child)
    &&& match (returned.returned_request, returned.rust_request) {
        (
            epm::ChildRequest::ParValue { body, .. },
            EpmPushRustRequest::Message { body: requested, view_id, program_id },
        )
        | (
            epm::ChildRequest::RawParEscape { body, .. },
            EpmPushRustRequest::Message { body: requested, view_id, program_id },
        ) => {
            &&& requested == body
            &&& view_id == returned.selected_view_id
            &&& program_id == returned.par_program_id
        },
        (
            epm::ChildRequest::NestedEpm1 { body, .. },
            EpmPushRustRequest::EPathMap { body: requested, view_id, owner_id, field },
        ) => {
            &&& requested == body
            &&& view_id == returned.selected_view_id
            &&& owner_id == returned.parent_owner_id
            &&& field == returned.parent_field
        },
        _ => false,
    }
}

pub open spec fn epm_push_rust_child_matches(returned: EpmPushReturnedSource) -> bool {
    &&& epm::indexed_image_wf(returned.selected_child)
    &&& returned.child.source_bytes == returned.selected_child.bytes
    &&& returned.child.physical_source
        == epm::indexed_position_at(returned.selected_child, 0).source
    &&& returned.child.physical_start
        == epm::indexed_position_at(returned.selected_child, 0).offset
    &&& returned.child.region_start == 0
    &&& returned.child.region_end == returned.selected_child.bytes.len()
    &&& returned.child.cursor == 0
    &&& returned.child.frame_kind == epm::request_frame_kind(returned.returned_request)
    &&& (returned.child.frame_kind != 1 || returned.child.program_type_id == 1)
    &&& returned.actual_top_kind
        == Some(source::epm_push_source_request_kind(returned.returned_request))
}

pub proof fn epm_push_rust_request_preserves_selected_child(
    returned: EpmPushReturnedSource,
)
    requires
        epm_push_rust_request_matches(returned),
        epm_push_rust_child_matches(returned),
    ensures
        returned.child.source_bytes
            == epm::request_body(returned.returned_request).bytes,
        returned.child.physical_source
            == epm::indexed_position_at(
                epm::request_body(returned.returned_request), 0,
            ).source,
        returned.child.physical_start
            == epm::indexed_position_at(
                epm::request_body(returned.returned_request), 0,
            ).offset,
        returned.actual_top_kind
            == Some(source::epm_push_source_request_kind(returned.returned_request)),
        match returned.rust_request {
            EpmPushRustRequest::Message { body, view_id, program_id } => {
                &&& body == epm::request_body(returned.returned_request)
                &&& view_id == returned.selected_view_id
                &&& program_id == returned.par_program_id
                &&& match returned.returned_request {
                    epm::ChildRequest::ParValue { .. }
                    | epm::ChildRequest::RawParEscape { .. } => true,
                    _ => false,
                }
            },
            EpmPushRustRequest::EPathMap { body, view_id, owner_id, field } => {
                &&& body == epm::request_body(returned.returned_request)
                &&& view_id == returned.selected_view_id
                &&& owner_id == returned.parent_owner_id
                &&& field == returned.parent_field
                &&& returned.returned_request is NestedEpm1
            },
        },
{
    match returned.returned_request {
        epm::ChildRequest::ParValue { .. } => {},
        epm::ChildRequest::RawParEscape { .. } => {},
        epm::ChildRequest::NestedEpm1 { .. } => {},
    }
}

pub open spec fn epm_push_returned_observation(
    returned: EpmPushReturnedSource,
) -> source::EpmPushSourceObservation {
    source::EpmPushSourceObservation {
        terminal_event: returned.terminal_event,
        reducer_after: returned.reducer_after,
        child: returned.child,
        actual_top_kind: returned.actual_top_kind,
        after_len: returned.after_len,
        after_capacity: returned.after_capacity,
        stats_frame_variant_bytes: returned.stats_frame_variant_bytes,
    }
}

pub open spec fn epm_push_returned_source_matches(
    prior: epm::LinkedEpmState,
    returned: EpmPushReturnedSource,
) -> bool {
    &&& epm_push_rust_request_matches(returned)
    &&& epm_push_rust_child_matches(returned)
    &&& epm::event_step(
        prior.epm, returned.reducer_after, returned.terminal_event,
        prior.admission.next_obligation_id,
        epm::EpmAction::Push(returned.returned_request),
    )
    &&& match returned.terminal_event {
        epm::EpmEvent::Edge { .. }
        | epm::EpmEvent::Value { value_body: Some(_), .. } => true,
        _ => false,
    }
}

pub proof fn epm_push_returned_request_is_selected(
    prior: epm::LinkedEpmState,
    returned: EpmPushReturnedSource,
)
    requires
        epm::linked_indexed_sources(prior),
        epm_push_returned_source_matches(prior, returned),
    ensures source::epm_push_source_request(
        prior, epm_push_returned_observation(returned),
    ) == Some(returned.returned_request),
{
    match returned.terminal_event {
        epm::EpmEvent::Edge { .. } => {
            let expected = epm::request_for_obligation(
                returned.reducer_after.path, prior.admission.next_obligation_id,
            );
            assert(expected.is_some());
            assert(expected == Some(returned.returned_request));
        },
        epm::EpmEvent::Value { value_body: Some(span), .. } => {
            assert(returned.returned_request == epm::ChildRequest::ParValue {
                return_id: prior.admission.next_obligation_id,
                body: epm::indexed_subview(prior.epm.snapshot, span),
            });
        },
        _ => assert(false),
    }
}

pub proof fn epm_push_returned_event_is_cursor_event(
    prior: epm::LinkedEpmState,
    returned: EpmPushReturnedSource,
)
    requires epm_push_returned_source_matches(prior, returned),
    ensures epm::cursor_event_at(prior.epm, prior.epm.cursor.at)
        == epm::wire_event(returned.terminal_event),
{
    assert(epm::event_step(
        prior.epm, returned.reducer_after, returned.terminal_event,
        prior.admission.next_obligation_id,
        epm::EpmAction::Push(returned.returned_request),
    ));
    assert(epm::cursor_event_step(
        prior.epm, returned.reducer_after, returned.terminal_event,
    ));
}

pub proof fn epm_push_transport_terminal_is_source_selected(
    states: Seq<epm::LinkedEpmState>,
    chunks: Seq<EpmPushTransportChunk>,
    returned: EpmPushReturnedSource,
)
    requires
        epm_push_transport_linked_chain(states, chunks),
        epm_push_returned_source_matches(states.last(), returned),
    ensures {
        let events = epm_push_transport_events(chunks);
        let prior = states.last();
        let observed = epm_push_returned_observation(returned);
        &&& prior.epm.cursor.at as int
            == states[0].epm.cursor.at as int + events.len()
        &&& epm::cursor_event_at(prior.epm, prior.epm.cursor.at)
            == epm::wire_event(returned.terminal_event)
        &&& source::epm_push_source_request(prior, observed)
            == Some(returned.returned_request)
        &&& returned.child.source_bytes
            == epm::request_body(returned.returned_request).bytes
        &&& returned.child.physical_source
            == epm::indexed_position_at(
                epm::request_body(returned.returned_request), 0,
            ).source
        &&& returned.child.physical_start
            == epm::indexed_position_at(
                epm::request_body(returned.returned_request), 0,
            ).offset
        &&& returned.actual_top_kind
            == Some(source::epm_push_source_request_kind(returned.returned_request))
        &&& match returned.rust_request {
            EpmPushRustRequest::Message { body, view_id, program_id } => {
                &&& body == epm::request_body(returned.returned_request)
                &&& view_id == returned.selected_view_id
                &&& program_id == returned.par_program_id
                &&& match returned.returned_request {
                    epm::ChildRequest::ParValue { .. }
                    | epm::ChildRequest::RawParEscape { .. } => true,
                    _ => false,
                }
            },
            EpmPushRustRequest::EPathMap { body, view_id, owner_id, field } => {
                &&& body == epm::request_body(returned.returned_request)
                &&& view_id == returned.selected_view_id
                &&& owner_id == returned.parent_owner_id
                &&& field == returned.parent_field
                &&& returned.returned_request is NestedEpm1
            },
        }
    },
{
    let events = epm_push_transport_events(chunks);
    epm_push_transport_prefix_is_cursor_exact(states, chunks, events.len() as int);
    let prior = states.last();
    epm_push_returned_request_is_selected(prior, returned);
    epm_push_returned_event_is_cursor_event(prior, returned);
    epm_push_rust_request_preserves_selected_child(returned);
}

}
