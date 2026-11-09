//! Source-shaped selection of an EPathMap-origin Push product action.
//!
//! This starts at the terminal scalar event of `epathmap_step`. Earlier ACT
//! events and batches in the same Rust invocation must be related to this
//! prestate by the separate linked EPM Push macro proof. The observation's
//! post-reducer frame and child frame still require Rust extraction; neither
//! is an arbitrary product poststate.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_map_frame_refinement as map, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as bridge, rpb3_wire_cursor_source_refinement as wire,
};

verus! {

pub struct EpmPushSourceObservation {
    pub terminal_event: epm::EpmEvent,
    pub ghost reducer_after: epm::EpmFrame,
    pub ghost child: admission::AdmissionFrameState,
    pub actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    pub after_len: usize,
    pub after_capacity: usize,
    pub stats_frame_variant_bytes: usize,
}

pub struct EpmPushLiveObservation {
    pub ghost push: EpmPushSourceObservation,
    pub ghost child_cursor: Option<wire::WireRuntimeCursorImage>,
}

/// The request is selected by the terminal ACT event and reducer state. A
/// Value with a body requests Par; a completed path obligation requests raw
/// Par or nested EPM1. Leave/End cannot request a child.
pub open spec fn epm_push_source_request(
    prior: epm::LinkedEpmState,
    observed: EpmPushSourceObservation,
) -> Option<epm::ChildRequest> {
    match observed.terminal_event {
        epm::EpmEvent::Value { value_body: Some(span), .. }
            if epm::indexed_image_wf(prior.epm.snapshot)
                && span.start <= span.end
                && span.end <= prior.epm.snapshot.bytes.len() =>
            Some(epm::ChildRequest::ParValue {
                return_id: prior.admission.next_obligation_id,
                body: epm::indexed_subview(prior.epm.snapshot, span),
            }),
        epm::EpmEvent::Edge { .. } => epm::request_for_obligation(
            observed.reducer_after.path, prior.admission.next_obligation_id,
        ),
        _ => None,
    }
}

pub open spec fn epm_push_source_request_kind(
    request: epm::ChildRequest,
) -> runtime::SourceFrameRequestKind {
    match request {
        epm::ChildRequest::NestedEpm1 { .. } => runtime::SourceFrameRequestKind::EPathMap,
        _ => runtime::SourceFrameRequestKind::Message,
    }
}

pub open spec fn epm_push_source_admission_after(
    prior: epm::LinkedEpmState,
    observed: EpmPushSourceObservation,
    request: epm::ChildRequest,
) -> admission::AdmissionState {
    let parent = prior.admission.frames.last();
    let kind = epm::request_frame_kind(request);
    let return_id = epm::request_return_id(request);
    admission::AdmissionState {
        frames: prior.admission.frames.update(
            prior.admission.frames.len() - 1,
            epm::epm_parent_waiting(
                epm::epm_advanced_admission_cursor(
                    parent, observed.reducer_after.cursor.value_table_offset,
                ),
                return_id, kind,
            ),
        ).push(epm::epm_assigned_child(observed.child, return_id)),
        arena: prior.admission.arena,
        root_end: prior.admission.root_end,
        pushed: (prior.admission.pushed as int + 1) as usize,
        completed: prior.admission.completed,
        next_obligation_id: (prior.admission.next_obligation_id as int + 1) as u64,
    }
}

pub open spec fn epm_push_source_linked_after(
    prior: epm::LinkedEpmState,
    observed: EpmPushSourceObservation,
    request: epm::ChildRequest,
) -> epm::LinkedEpmState {
    epm::LinkedEpmState {
        epm: observed.reducer_after,
        admission: epm_push_source_admission_after(prior, observed, request),
        indexed_regions: prior.indexed_regions.push(epm::request_body(request)),
    }
}

pub open spec fn epm_push_source_product_after(
    before: integrated::ProductScoreState,
    observed: EpmPushSourceObservation,
    request: epm::ChildRequest,
) -> integrated::ProductScoreState {
    let prior = integrated::product_linked_top(before).unwrap();
    let next = epm_push_source_linked_after(prior, observed, request);
    let next_bridge = bridge::ScoreAdmissionBridge {
        admission: next.admission,
        transcript: before.bridge.transcript,
    };
    let next_map = map::IntegratedAdmissionState {
        admission: next.admission,
        map_frames: before.map.map_frames.push(None),
        transcript: before.map.transcript,
    };
    let provisional = integrated::ProductScoreState {
        bridge: next_bridge,
        map: next_map,
        epm_frames: before.epm_frames.update(
            before.epm_frames.len() - 1, Some(next.epm),
        ).push(None),
        indexed_regions: next.indexed_regions,
        accepted_epm_sources: before.accepted_epm_sources,
    };
    integrated::ProductScoreState {
        accepted_epm_sources: integrated::product_epm_receipts_carried(before, provisional),
        ..provisional
    }
}

/// These are the exact fields that must be projected from the real terminal
/// `epathmap_step` event, returned `FrameRequest`, pushed frame, and checked
/// storage observation. The constructed admission successor is checked for
/// well-formedness; no caller supplies a product successor.
pub open spec fn epm_push_source_ready(
    before: integrated::LiveRuntimeProductState,
    observed: EpmPushSourceObservation,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || integrated::product_linked_top(before.source.product).is_none()
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
    { false } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        match epm_push_source_request(prior, observed) {
            None => false,
            Some(request) => {
                let body = epm::request_body(request);
                let kind = epm::request_frame_kind(request);
                let next = epm_push_source_linked_after(prior, observed, request);
                &&& admission::admission_state_wf(prior.admission)
                &&& epm::linked_indexed_sources(prior)
                &&& prior.admission.frames.len() > 0
                &&& prior.admission.pushed < usize::MAX
                &&& prior.admission.next_obligation_id < u64::MAX
                &&& prior.admission.frames.last().frame_kind == 3
                &&& prior.admission.frames.last().waiting_id.is_none()
                &&& prior.admission.frames.last().cursor
                    <= prior.epm.cursor.value_table_offset
                &&& prior.epm.snapshot == prior.indexed_regions.last()
                &&& epm::event_step(
                    prior.epm, observed.reducer_after, observed.terminal_event,
                    prior.admission.next_obligation_id, epm::EpmAction::Push(request),
                )
                &&& epm::request_return_id(request)
                    == prior.admission.next_obligation_id
                &&& (kind != 3 || prior.admission.frames.last().nested_started < u64::MAX)
                &&& observed.child.frame_kind == kind
                &&& observed.child.return_id.is_none()
                &&& observed.child.waiting_id.is_none()
                &&& observed.child.last_slot.is_none()
                &&& observed.child.seen == Set::empty()
                &&& observed.child.scalar_slots == Set::empty()
                &&& observed.child.scalar_occurrences.len() == 0
                &&& observed.child.returns.len() == 0
                &&& observed.child.score_handle.is_none()
                &&& observed.child.nested_started == 0
                &&& observed.child.nested_resolved == 0
                &&& observed.child.source_bytes == body.bytes
                &&& observed.child.region_start == 0
                &&& observed.child.cursor == 0
                &&& observed.child.region_end == body.bytes.len()
                &&& (kind != 1 || observed.child.program_type_id == 1)
                &&& admission::admission_frame_wf(observed.child)
                &&& admission::admission_state_wf(next.admission)
                &&& epm::linked_indexed_sources(next)
                &&& observed.stats_frame_variant_bytes
                    == before.source.runtime.frame_bytes
                &&& match runtime::source_frame_push_observe_after(
                    epm_push_source_request_kind(request), observed.actual_top_kind,
                    prior.admission.frames.len() as usize,
                    before.source.runtime.capacity, observed.after_len,
                    observed.after_capacity, before.source.runtime.frame_bytes,
                    observed.stats_frame_variant_bytes,
                ) {
                    runtime::SourceFramePushObservationResult::Success { .. } => true,
                    _ => false,
                }
            },
        }
    }
}

pub open spec fn epm_push_source_product_step(
    before: integrated::LiveRuntimeProductState,
    observed: EpmPushSourceObservation,
) -> Option<integrated::ProductScoreState> {
    if !epm_push_source_ready(before, observed) { None }
    else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let request = epm_push_source_request(prior, observed).unwrap();
        Some(epm_push_source_product_after(before.source.product, observed, request))
    }
}

/// A selected terminal request enables the existing integrated EPM Push
/// action. Its product successor is constructed from the event and the
/// source-shaped child, rather than accepted as an arbitrary poststate.
pub proof fn epm_push_source_enables_integrated_action(
    before: integrated::LiveRuntimeProductState,
    observed: EpmPushSourceObservation,
)
    requires epm_push_source_ready(before, observed),
    ensures
        epm_push_source_product_step(before, observed).is_some(),
        match epm_push_source_product_step(before, observed) {
            Some(after) => {
                let prior = integrated::product_linked_top(before.source.product).unwrap();
                let request = epm_push_source_request(prior, observed).unwrap();
                let action = epm::EpmAction::Push(request);
                &&& integrated::product_source_step(
                    before.source.product, after,
                    integrated::ProductSourceAction::EpmPush {
                        event: observed.terminal_event,
                        action,
                        child: observed.child,
                    },
                )
                &&& after.bridge.admission.frames.len()
                    == before.source.product.bridge.admission.frames.len() + 1
                &&& after.indexed_regions.last() == epm::request_body(request)
                &&& after.bridge.transcript == before.source.product.bridge.transcript
            },
            None => false,
        },
{
    let product = before.source.product;
    let prior = integrated::product_linked_top(product).unwrap();
    let request = epm_push_source_request(prior, observed).unwrap();
    let next = epm_push_source_linked_after(prior, observed, request);
    let after = epm_push_source_product_after(product, observed, request);
    assert(epm::linked_epm_push(
        prior, next, observed.terminal_event,
        epm::EpmAction::Push(request), observed.child,
    ));
    assert(integrated::product_epm_push_step(
        product, after, observed.terminal_event,
        epm::EpmAction::Push(request), observed.child,
    ));
}

}
