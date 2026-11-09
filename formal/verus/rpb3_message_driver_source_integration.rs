//! Source-driven MessageFrame actions through generated field and EOF paths.
//!
//! The Send sequence is derived from indexed field observations, child-return
//! observations, and append-only score images recorded by the production
//! driver. Generated scalar and child actions select live product successors
//! from their indexed source receipt. A successful EOF replay selects the
//! existing score transition; no caller supplies a finished transition or its
//! semantic-completeness bit.
//!
//! This is a formal validation of a projected source sequence. It does not
//! prove that every successful Rust run yields the requisite indexed fields,
//! returned-summary images, arena bindings, and driver observations. That
//! universal extraction remains an explicit criterion-5935 obligation.

use vstd::prelude::*;

#[path = "rpb3_generated_descriptor_projection.rs"]
mod rpb3_generated_descriptor_projection;
use rpb3_generated_descriptor_projection as generated;

#[cfg(rpb3_live_push)]
use crate as integrated;
#[cfg(rpb3_live_push)]
use crate::rpb3_map_frame_refinement as map;
use crate::{
    rpb3_admission_cursor as admission, rpb3_generic_message_eof_source_refinement as message,
    rpb3_score_admission_bridge as bridge, rpb3_score_transcript as score,
};
#[cfg(rpb3_live_push)]
use crate::{
    rpb3_epm_source_accept_refinement as epm_source_accept,
    rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_wire_cursor_source_refinement as wire_source,
};
verus! {

/// Source fields common to the root and child Send frame. `eof_before` is the
/// actual pre-finish arena/frame projection; the replay compares its mutable
/// fields to the indexed event fold and its score transcript to the cumulative
/// source score image before accepting EOF.
pub struct SendDriverSource {
    pub body_id: u64,
    pub body_len: usize,
    pub ghost indexed: admission::AdmissionIndexedSourceImage,
    pub ghost score_entries_at_entry: Seq<score::ScoreEntry>,
    pub ghost eof_before: bridge::ScoreAdmissionBridge,
}

/// `Continue` and `Push` are produced by `message_field_step`; `Accepted` is
/// produced only after the worklist child returns. A child's own actions are
/// summarized at the parent boundary by its accepted summary and score image.
/// `oneof_arm` is compared with the generated descriptor, including `None`.
pub enum SendDriverSourceAction {
    Continue {
        field: admission::IndexedCanonicalFieldModel,
        oneof_arm: Option<u32>,
    },
    Push {
        field: admission::IndexedCanonicalFieldModel,
        oneof_arm: Option<u32>,
        child_start: admission::AdmissionSourceAnchor,
        child_end: admission::AdmissionSourceAnchor,
        body_id: u64,
    },
    Accepted {
        returned: admission::AdmissionReturnedSummary,
        ghost score_entries_after: Seq<score::ScoreEntry>,
    },
    Eof {
        component: message::GenericComponentEofSource,
        ghost score_entries_after: Seq<score::ScoreEntry>,
    },
}

pub struct SendDriverReplay {
    pub ghost fields: message::GenericMessageSourceFold,
    pub ghost field_events: Seq<message::GenericMessageSourceEvent>,
    pub ghost score_entries: Seq<score::ScoreEntry>,
    pub terminal: bool,
    pub ghost eof_observation: Option<message::GenericMessageEofSourceObservation>,
    pub ghost outcome: Option<message::GenericMessageEofModelOutcome>,
}

pub open spec fn send_driver_initial(source: SendDriverSource) -> SendDriverReplay {
    SendDriverReplay {
        fields: message::generic_message_source_initial(),
        field_events: Seq::empty(),
        score_entries: source.score_entries_at_entry,
        terminal: false,
        eof_observation: None,
        outcome: None,
    }
}

pub open spec fn send_score_prefix(
    before: Seq<score::ScoreEntry>, after: Seq<score::ScoreEntry>,
) -> bool {
    before.len() <= after.len()
        && after.subrange(0, before.len() as int) == before
}

/// Descriptor lookup makes the protobuf slot, repetition bit, wire type, and
/// oneof arm a function of the observed program ID and wire tag. The indexed
/// field validator in `generic_message_source_event_step` checks source bytes,
/// exact cursor progress, and the descriptor's scalar/child class.
pub open spec fn send_field_arm_matches(
    field: admission::IndexedCanonicalFieldModel,
    oneof_arm: Option<u32>,
    child: bool,
) -> bool {
    match generated::generated_descriptor(15, field.number) {
        Some(descriptor) => descriptor.oneof_arm == oneof_arm
            && descriptor.wire == field.wire
            && (descriptor.frame_kind != 0) == child,
        None => false,
    }
}

pub open spec fn send_driver_source_step(
    source: SendDriverSource,
    before: SendDriverReplay,
    action: SendDriverSourceAction,
) -> Option<SendDriverReplay> {
    if before.terminal
        || !admission::admission_indexed_source_wf(source.indexed)
        || source.indexed.bytes.len() != source.body_len as int
    {
        None
    } else {
        match action {
            SendDriverSourceAction::Continue { field, oneof_arm } => {
                if !send_field_arm_matches(field, oneof_arm, false) {
                    None
                } else {
                    let event = message::GenericMessageSourceEvent::Scalar { field };
                    match message::generic_message_source_event_step(
                        before.fields, event, 15, admission::AdmissionFamily::Send,
                        6, source.body_len, source.indexed,
                    ) {
                        Some(fields) => Some(SendDriverReplay {
                            fields,
                            field_events: before.field_events.push(event),
                            score_entries: before.score_entries,
                            terminal: false,
                            eof_observation: None,
                            outcome: None,
                        }),
                        None => None,
                    }
                }
            },
            SendDriverSourceAction::Push {
                field, oneof_arm, child_start, child_end, body_id,
            } => {
                if !send_field_arm_matches(field, oneof_arm, true)
                    || admission::admission_indexed_source_position(
                        source.indexed, field.payload_start,
                    ) != Some(child_start)
                    || admission::admission_indexed_source_position(
                        source.indexed, field.payload_end,
                    ) != Some(child_end)
                {
                    None
                } else {
                    let event = message::GenericMessageSourceEvent::Push { field, body_id };
                    match message::generic_message_source_event_step(
                        before.fields, event, 15, admission::AdmissionFamily::Send,
                        6, source.body_len, source.indexed,
                    ) {
                        Some(fields) => Some(SendDriverReplay {
                            fields,
                            field_events: before.field_events.push(event),
                            score_entries: before.score_entries,
                            terminal: false,
                            eof_observation: None,
                            outcome: None,
                        }),
                        None => None,
                    }
                }
            },
            SendDriverSourceAction::Accepted { returned, score_entries_after } => {
                let score_bound = match returned.score_handle {
                    Some(handle) => (handle as int) < score_entries_after.len(),
                    None => false,
                };
                if !send_score_prefix(before.score_entries, score_entries_after)
                    || !score_bound
                    || returned.score_origin_body_id != Some(returned.body_id)
                    || !admission::admission_return_matches(
                        admission::AdmissionFamily::Send, returned.slot, returned.kind,
                    )
                {
                    None
                } else {
                    let event = message::GenericMessageSourceEvent::Accepted { returned };
                    match message::generic_message_source_event_step(
                        before.fields, event, 15, admission::AdmissionFamily::Send,
                        6, source.body_len, source.indexed,
                    ) {
                        Some(fields) => Some(SendDriverReplay {
                            fields,
                            field_events: before.field_events.push(event),
                            score_entries: score_entries_after,
                            terminal: false,
                            eof_observation: None,
                            outcome: None,
                        }),
                        None => None,
                    }
                }
            },
            SendDriverSourceAction::Eof { component, score_entries_after } => {
                let observed = message::GenericMessageEofSourceObservation {
                    program_type_id: 15,
                    family: admission::AdmissionFamily::Send,
                    body_id: source.body_id,
                    indexed: source.indexed,
                    body_len: source.body_len,
                    cursor_after: before.fields.cursor,
                    slot_count: 6,
                    events: before.field_events,
                    score_entries_before: before.score_entries,
                    score_entries_after,
                    component: Some(component),
                };
                if before.fields.pending.is_some()
                    || before.fields.cursor != source.body_len
                    || component.family != admission::AdmissionFamily::Send
                    || !send_score_prefix(before.score_entries, score_entries_after)
                {
                    None
                } else {
                    match message::generic_message_eof_from_source(
                        source.eof_before, observed,
                    ) {
                        Some(outcome) => match outcome {
                            message::GenericMessageEofModelOutcome::Scored {
                                after, score_handle, semantic_complete, ..
                            } if score_handle == component.score_handle
                                && semantic_complete == component.complete
                                && after.transcript.entries == score_entries_after => {
                                Some(SendDriverReplay {
                                    fields: before.fields,
                                    field_events: before.field_events,
                                    score_entries: score_entries_after,
                                    terminal: true,
                                    eof_observation: Some(observed),
                                    outcome: Some(outcome),
                                })
                            },
                            _ => None,
                        },
                        None => None,
                    }
                }
            },
        }
    }
}

/// Replaying a source action prefix computes every successor from its prior
/// state. There is no independently supplied post-state sequence to splice.
pub open spec fn send_driver_source_replay(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
) -> Option<SendDriverReplay>
    decreases actions.len(),
{
    if actions.len() == 0 {
        Some(send_driver_initial(source))
    } else {
        match send_driver_source_replay(
            source, actions.subrange(0, (actions.len() - 1) as int),
        ) {
            Some(before) => send_driver_source_step(source, before, actions.last()),
            None => None,
        }
    }
}

/// The score bridge is selected by the complete source replay, not provided
/// as an adjacent product post-state. `None` also covers an unsuccessful or
/// nonterminal source sequence.
pub open spec fn send_driver_replay_score_bridge(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
) -> Option<bridge::ScoreAdmissionBridge> {
    match send_driver_source_replay(source, actions) {
        Some(final_state) if final_state.terminal => match final_state.outcome {
            Some(message::GenericMessageEofModelOutcome::Scored { after, .. }) => Some(after),
            _ => None,
        },
        _ => None,
    }
}

/// Extend the already live product with the *selected* score bridge. The
/// indexed regions, message stack, wire reader, and concrete cursor remain
/// fixed during this score append. This construction is shared by root and
/// child Message EOF and does not choose an arbitrary product successor.
#[cfg(rpb3_live_push)]
pub open spec fn send_live_score_after(
    before: integrated::LiveRuntimeProductState,
    selected: bridge::ScoreAdmissionBridge,
) -> integrated::LiveRuntimeProductState {
    integrated::LiveRuntimeProductState {
        source: integrated::LiveSourceProductState {
            product: integrated::product_epm_source_scored_product(
                before.source.product, selected,
            ),
            runtime: runtime_source::RuntimeFrameImage {
                score: selected,
                ..before.source.runtime
            },
            arena: integrated::product_runtime_arena_from_score(selected.transcript),
            wire: before.source.wire,
        },
        cursors: before.cursors,
    }
}

/// A field Continue computes its successor from the independent wire,
/// generated-admission, indexed-backing, and runtime-frame selectors. No
/// adjacent live product post-state is supplied by the caller.
#[cfg(rpb3_live_push)]
pub open spec fn send_live_continue_selected(
    before: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    indexed: admission::AdmissionIndexedSourceImage,
) -> Option<integrated::LiveRuntimeProductState> {
    if !integrated::live_runtime_product_wf(before)
        || before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
        || indexed != integrated::product_admission_source_image(
            before.source.product.indexed_regions.last(),
        )
    {
        None
    } else {
        match (before.cursors.last(), before.source.wire.last()) {
            (Some(cursor), Some(reader)) => {
                match (
                    wire_source::wire_runtime_next_field(cursor),
                    wire_source::wire_source_next_field(reader),
                ) {
                    (
                        Some(wire_source::WireConcreteTransition::Field {
                            state: concrete_after, field: concrete_field, start: _,
                        }),
                        wire_source::WireSourceTransition::Field {
                            state: next_reader, field: source_field,
                        },
                    ) if concrete_field == field && source_field == field => {
                        let action = map::IntegratedAdmissionAction::Message {
                            action: admission::ConcreteFrameActionModel::ContinueIndexed {
                                field, parent_source: indexed,
                            },
                        };
                        let next_cursor = integrated::runtime_wire_after_field(
                            cursor, concrete_after,
                        );
                        if wire_source::wire_runtime_cursor_project(next_cursor)
                            != Some(next_reader)
                        {
                            None
                        } else {
                            match (
                                map::integrated_admission_driver_step(
                                    before.source.product.map, action,
                                ),
                                runtime_source::runtime_continue_indexed(
                                    before.source.runtime, field, indexed,
                                ),
                            ) {
                                (Some(outcome), Some(next_runtime)) => match (
                                    map::integrated_nonterminal_state(outcome),
                                    integrated::product_map_regions(
                                        before.source.product, action, outcome,
                                    ),
                                    integrated::product_map_epm_frames(
                                        before.source.product, outcome,
                                    ),
                                ) {
                                    (Some(next_map), Some(regions), Some(epm_frames)) => {
                                        let candidate = integrated::ProductScoreState {
                                            bridge: bridge::ScoreAdmissionBridge {
                                                admission: next_map.admission,
                                                transcript: next_map.transcript,
                                            },
                                            map: next_map,
                                            epm_frames,
                                            indexed_regions: regions,
                                            accepted_epm_sources: Seq::empty(),
                                        };
                                        let product = integrated::ProductScoreState {
                                            accepted_epm_sources:
                                                integrated::product_epm_receipts_carried(
                                                    before.source.product, candidate,
                                                ),
                                            ..candidate
                                        };
                                        Some(integrated::LiveRuntimeProductState {
                                            source: integrated::LiveSourceProductState {
                                                product,
                                                runtime: next_runtime,
                                                arena: before.source.arena,
                                                wire: before.source.wire.update(
                                                    before.source.wire.len() - 1,
                                                    Some(next_reader),
                                                ),
                                            },
                                            cursors: before.cursors.update(
                                                before.cursors.len() - 1,
                                                Some(next_cursor),
                                            ),
                                        })
                                    },
                                    _ => None,
                                },
                                _ => None,
                            }
                        }
                    },
                    _ => None,
                }
            },
            _ => None,
        }
    }
}

/// A selected Continue successor is an integrated ContinueIndexed action.
/// The source replay additionally fixes the descriptor slot, oneof arm, and
/// field bytes; those are checked before this constructor is invoked.
#[cfg(rpb3_live_push)]
pub proof fn send_live_continue_selected_dispatches(
    before: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    indexed: admission::AdmissionIndexedSourceImage,
)
    requires send_live_continue_selected(before, field, indexed).is_some(),
    ensures integrated::live_runtime_product_step(
        before, send_live_continue_selected(before, field, indexed).unwrap(),
        integrated::LiveRuntimeProductAction::ContinueIndexed,
    ),
{
    let after = send_live_continue_selected(before, field, indexed).unwrap();
    assert(integrated::product_map_step(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::ContinueIndexed {
                field, parent_source: indexed,
            },
        },
    ));
    assert(integrated::live_source_product_continue_indexed_step(
        before.source, after.source,
    ));
    assert(integrated::live_runtime_product_continue_indexed_step(before, after));
}

/// Independent source projection of the active Send frame. The exact slot
/// collections and score prefix come from the retained frame and append-only
/// arena, while the fold comes from indexed field/Accept observations.
#[cfg(rpb3_live_push)]
pub open spec fn send_live_frame_matches_fold(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
) -> bool {
    if !integrated::live_runtime_product_wf(live)
        || live.source.product.bridge.admission.frames.len() == 0
        || live.source.product.indexed_regions.len() == 0
    {
        false
    } else {
        let frame = live.source.product.bridge.admission.frames.last();
        &&& frame.frame_kind == 1
        &&& frame.program_type_id == 15
        &&& frame.family == admission::AdmissionFamily::Send
        &&& frame.body_id == source.body_id
        &&& frame.source_bytes == source.indexed.bytes
        &&& frame.slot_count == 6
        &&& frame.cursor == folded.fields.cursor
        &&& frame.last_slot == folded.fields.last_slot
        &&& frame.seen == folded.fields.seen
        &&& frame.scalar_slots == folded.fields.scalar_slots
        &&& frame.scalar_occurrences == folded.fields.scalar_occurrences
        &&& frame.returns == folded.fields.returns
        &&& live.source.product.bridge.transcript.entries == folded.score_entries
        &&& integrated::product_admission_source_image(
            live.source.product.indexed_regions.last(),
        ) == source.indexed
    }
}

/// One source Continue computes both the field-fold successor and its live
/// product successor. A mismatch between the resulting live frame and folded
/// source facts fails the selection; no post-state relation is a premise.
#[cfg(rpb3_live_push)]
pub open spec fn send_source_live_continue_selected(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    oneof_arm: Option<u32>,
) -> Option<(SendDriverReplay, integrated::LiveRuntimeProductState)> {
    if !send_live_frame_matches_fold(source, folded, live)
        || folded.fields.pending.is_some()
    {
        None
    } else {
        match (
            send_driver_source_step(
                source, folded,
                SendDriverSourceAction::Continue { field, oneof_arm },
            ),
            send_live_continue_selected(live, field, source.indexed),
        ) {
            (Some(next_fold), Some(next_live))
                if send_live_frame_matches_fold(source, next_fold, next_live) =>
                Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_source_live_continue_selected_exact(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    oneof_arm: Option<u32>,
)
    requires send_source_live_continue_selected(
        source, folded, live, field, oneof_arm,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = send_source_live_continue_selected(
            source, folded, live, field, oneof_arm,
        ).unwrap();
        &&& send_live_frame_matches_fold(source, folded, live)
        &&& send_live_frame_matches_fold(source, next_fold, next_live)
        &&& send_field_arm_matches(field, oneof_arm, false)
        &&& send_driver_source_step(
            source, folded, SendDriverSourceAction::Continue { field, oneof_arm },
        ) == Some(next_fold)
        &&& integrated::live_runtime_product_step(
            live, next_live, integrated::LiveRuntimeProductAction::ContinueIndexed,
        )
    },
{
    send_live_continue_selected_dispatches(live, field, source.indexed);
}

/// A successful Push constructs the child product from the indexed payload,
/// the generated admission action, the checked storage observation, and the
/// runtime parent/child cursors. The request is the Rust-observed fresh child;
/// its bytes and physical start are checked against the field event. The
/// receipt carries the body ID assigned when that child is published.
#[cfg(rpb3_live_push)]
pub open spec fn send_live_push_selected(
    before: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    indexed: admission::AdmissionIndexedSourceImage,
    child_start: admission::AdmissionSourceAnchor,
    body_id: u64,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> Option<integrated::LiveRuntimeProductState> {
    if !integrated::live_runtime_product_wf(before)
        || !integrated::product_frame_source_anchors_match(before.source.product)
        || before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
        || indexed != integrated::product_admission_source_image(
            before.source.product.indexed_regions.last(),
        )
        || runtime_source::source_frame_request_kind_code(request.request_kind)
            != request.child.frame_kind
        || request.field != field
        || (request.child.frame_kind != 1 && request.child.frame_kind != 2)
        || before.source.product.bridge.admission.next_obligation_id == u64::MAX
        || body_id as int != before.source.product.bridge.admission.next_obligation_id as int + 1
        || request.child.source_bytes != field.payload_bytes
        || request.child.physical_source != child_start.source
        || request.child.physical_start != child_start.offset
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
    {
        None
    } else {
        match before.cursors.last() {
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field: concrete_field, start: _,
                }) if concrete_field == field => {
                    match integrated::live_runtime_child_cursor(parent, field) {
                        Some(child) => {
                            let next_parent = integrated::runtime_wire_after_field(
                                parent, concrete_after,
                            );
                            match (
                                wire_source::wire_runtime_cursor_project(next_parent),
                                wire_source::wire_runtime_cursor_project(child),
                                runtime_source::source_frame_push_observe_after(
                                    request.request_kind, actual_top_kind,
                                    before.source.product.bridge.admission.frames.len() as usize,
                                    before.source.runtime.capacity, after_len, after_capacity,
                                    before.source.runtime.frame_bytes,
                                    stats_frame_variant_bytes,
                                ),
                            ) {
                                (
                                    Some(next_parent_reader), Some(child_reader),
                                    runtime_source::SourceFramePushObservationResult::Success {
                                        storage,
                                    },
                                ) => {
                                    let frame_action =
                                        admission::ConcreteFrameActionModel::PushIndexed {
                                            field,
                                            child: request.child,
                                            parent_source: indexed,
                                            frame_bytes: before.source.runtime.frame_bytes,
                                        };
                                    let action = map::IntegratedAdmissionAction::Message {
                                        action: frame_action,
                                    };
                                    if !integrated::product_wire_indexed_source_agrees(
                                        parent, before.source.product.indexed_regions.last(),
                                    ) || map::integrated_source_message_action(
                                        before.source.product.map.admission, frame_action,
                                    ) != Some(frame_action)
                                    {
                                        None
                                    } else {
                                        match (
                                            map::integrated_admission_driver_step(
                                                before.source.product.map, action,
                                            ),
                                            runtime_source::source_checked_frame_request_push_indexed(
                                                before.source.runtime, request, indexed, storage,
                                            ),
                                        ) {
                                            (Some(outcome), Some(next_runtime)) => match (
                                                map::integrated_nonterminal_state(outcome),
                                                integrated::product_map_regions(
                                                    before.source.product, action, outcome,
                                                ),
                                                integrated::product_map_epm_frames(
                                                    before.source.product, outcome,
                                                ),
                                            ) {
                                                (
                                                    Some(next_map), Some(regions),
                                                    Some(epm_frames),
                                                ) => {
                                                    let candidate = integrated::ProductScoreState {
                                                        bridge: bridge::ScoreAdmissionBridge {
                                                            admission: next_map.admission,
                                                            transcript: next_map.transcript,
                                                        },
                                                        map: next_map,
                                                        epm_frames,
                                                        indexed_regions: regions,
                                                        accepted_epm_sources: Seq::empty(),
                                                    };
                                                    let product = integrated::ProductScoreState {
                                                        accepted_epm_sources:
                                                            integrated::product_epm_receipts_carried(
                                                                before.source.product, candidate,
                                                            ),
                                                        ..candidate
                                                    };
                                                    Some(integrated::LiveRuntimeProductState {
                                                        source: integrated::LiveSourceProductState {
                                                            product,
                                                            runtime: next_runtime,
                                                            arena: before.source.arena,
                                                            wire: before.source.wire.update(
                                                                before.source.wire.len() - 1,
                                                                Some(next_parent_reader),
                                                            ).push(Some(child_reader)),
                                                        },
                                                        cursors: before.cursors.update(
                                                            before.cursors.len() - 1,
                                                            Some(next_parent),
                                                        ).push(Some(child)),
                                                    })
                                                },
                                                _ => None,
                                            },
                                            _ => None,
                                        }
                                    }
                                },
                                _ => None,
                            }
                        },
                        None => None,
                    }
                },
                _ => None,
            },
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_live_push_selected_dispatches(
    before: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    indexed: admission::AdmissionIndexedSourceImage,
    child_start: admission::AdmissionSourceAnchor,
    body_id: u64,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires send_live_push_selected(
        before, field, indexed, child_start, body_id, request,
        actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
    ).is_some(),
    ensures integrated::live_runtime_product_step(
        before,
        send_live_push_selected(
            before, field, indexed, child_start, body_id, request,
            actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
        ).unwrap(),
        integrated::LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        },
    ),
{
    let after = send_live_push_selected(
        before, field, indexed, child_start, body_id, request,
        actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
    ).unwrap();
    assert(integrated::live_runtime_product_message_push_indexed_observed_step(
        before, after, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ));
}

/// Once the child has selected its EOF score, the admission driver's Complete
/// selector computes the parent Accept state. This is the live counterpart of
/// a `SendDriverSourceAction::Accepted` and removes no-score child ambiguity.
#[cfg(rpb3_live_push)]
pub open spec fn send_live_child_complete_selected(
    before: integrated::LiveRuntimeProductState,
) -> Option<integrated::LiveRuntimeProductState> {
    if !integrated::live_runtime_product_wf(before)
        || before.cursors.len() <= 1
        || before.source.product.bridge.admission.frames.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 1
        || before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind != 1
        || !integrated::product_map_pop_owned(before.source.product)
    {
        None
    } else {
        match before.cursors.last() {
            Some(cursor) if wire_source::wire_runtime_next_field(cursor)
                == Some(wire_source::WireConcreteTransition::End {
                    state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                }) => {
                let action = map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                };
                match map::integrated_admission_driver_step(
                    before.source.product.map, action,
                ) {
                    Some(outcome) => match (
                        map::integrated_nonterminal_state(outcome),
                        integrated::product_map_regions(
                            before.source.product, action, outcome,
                        ),
                        integrated::product_map_epm_frames(
                            before.source.product, outcome,
                        ),
                    ) {
                        (Some(next_map), Some(regions), Some(epm_frames)) => {
                            let candidate = integrated::ProductScoreState {
                                bridge: bridge::ScoreAdmissionBridge {
                                    admission: next_map.admission,
                                    transcript: next_map.transcript,
                                },
                                map: next_map,
                                epm_frames,
                                indexed_regions: regions,
                                accepted_epm_sources: Seq::empty(),
                            };
                            let product = integrated::ProductScoreState {
                                accepted_epm_sources:
                                    integrated::product_epm_receipts_carried(
                                        before.source.product, candidate,
                                    ),
                                ..candidate
                            };
                            Some(integrated::LiveRuntimeProductState {
                                source: integrated::LiveSourceProductState {
                                    product,
                                    runtime: runtime_source::runtime_after_admission_step(
                                        before.source.runtime, next_map.admission,
                                    ),
                                    arena: before.source.arena,
                                    wire: before.source.wire.drop_last(),
                                },
                                cursors: before.cursors.drop_last(),
                            })
                        },
                        _ => None,
                    },
                    None => None,
                }
            },
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_live_child_complete_selected_dispatches(
    before: integrated::LiveRuntimeProductState,
)
    requires send_live_child_complete_selected(before).is_some(),
    ensures integrated::live_runtime_product_step(
        before, send_live_child_complete_selected(before).unwrap(),
        integrated::LiveRuntimeProductAction::MessageChildComplete,
    ),
{
    let after = send_live_child_complete_selected(before).unwrap();
    assert(integrated::product_map_step(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ));
    assert(integrated::live_runtime_product_message_child_complete_step(before, after));
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_map_child_complete_selected(
    before: integrated::LiveRuntimeProductState,
) -> Option<integrated::LiveRuntimeProductState> {
    if !integrated::live_runtime_product_wf(before)
        || before.cursors.len() <= 1
        || before.source.product.bridge.admission.frames.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 2
        || before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind != 1
        || !integrated::product_map_pop_owned(before.source.product)
    {
        None
    } else {
        match before.cursors.last() {
            Some(cursor) if wire_source::wire_runtime_next_field(cursor)
                == Some(wire_source::WireConcreteTransition::End {
                    state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                }) => {
                let action = map::IntegratedAdmissionAction::MapFinish;
                match map::integrated_admission_driver_step(
                    before.source.product.map, action,
                ) {
                    Some(outcome) => match (
                        map::integrated_nonterminal_state(outcome),
                        integrated::product_map_regions(before.source.product, action, outcome),
                        integrated::product_map_epm_frames(before.source.product, outcome),
                    ) {
                        (Some(next_map), Some(regions), Some(epm_frames)) => {
                            let candidate = integrated::ProductScoreState {
                                bridge: bridge::ScoreAdmissionBridge {
                                    admission: next_map.admission,
                                    transcript: next_map.transcript,
                                },
                                map: next_map,
                                epm_frames,
                                indexed_regions: regions,
                                accepted_epm_sources: Seq::empty(),
                            };
                            let product = integrated::ProductScoreState {
                                accepted_epm_sources:
                                    integrated::product_epm_receipts_carried(
                                        before.source.product, candidate,
                                    ),
                                ..candidate
                            };
                            Some(integrated::LiveRuntimeProductState {
                                source: integrated::LiveSourceProductState {
                                    product,
                                    runtime: runtime_source::runtime_after_admission_step(
                                        before.source.runtime, next_map.admission,
                                    ),
                                    arena: before.source.arena,
                                    wire: before.source.wire.drop_last(),
                                },
                                cursors: before.cursors.drop_last(),
                            })
                        },
                        _ => None,
                    },
                    None => None,
                }
            },
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_map_child_complete_selected_dispatches(
    before: integrated::LiveRuntimeProductState,
)
    requires generated_map_child_complete_selected(before).is_some(),
    ensures integrated::live_runtime_product_step(
        before, generated_map_child_complete_selected(before).unwrap(),
        integrated::LiveRuntimeProductAction::MapEntryFinish,
    ),
{
    hide(wire_source::wire_runtime_next_field);
    hide(wire_source::wire_runtime_derive_cursor);
    hide(wire_source::wire_runtime_cursor_project);
    hide(wire_source::wire_concrete_cursor_wf);
    hide(wire_source::wire_cursor_shape);
    hide(integrated::product_wire_indexed_source_agrees);
    let after = generated_map_child_complete_selected(before).unwrap();
    assert(integrated::product_map_step(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::MapFinish,
    ));
    reveal(integrated::live_runtime_product_map_entry_finish_step);
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_epm_child_complete_selected(
    before: integrated::LiveRuntimeProductState,
) -> Option<integrated::LiveRuntimeProductState> {
    if !integrated::live_runtime_product_wf(before)
        || before.cursors.len() <= 1
        || before.cursors.last().is_some()
        || before.source.product.bridge.admission.frames.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 3
        || before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind != 1
    {
        None
    } else {
        match integrated::product_linked_top(before.source.product) {
            Some(prior) => match epm_source_accept::epm_source_finish_accept(prior) {
                Some(accepted) if accepted.frames.len() > 0 => {
                    let next_map = map::IntegratedAdmissionState {
                        admission: accepted,
                        map_frames: before.source.product.map.map_frames.drop_last(),
                        transcript: before.source.product.map.transcript,
                    };
                    let candidate = integrated::ProductScoreState {
                        bridge: bridge::ScoreAdmissionBridge {
                            admission: accepted,
                            transcript: before.source.product.bridge.transcript,
                        },
                        map: next_map,
                        epm_frames: before.source.product.epm_frames.drop_last(),
                        indexed_regions: before.source.product.indexed_regions.drop_last(),
                        accepted_epm_sources: Seq::empty(),
                    };
                    let product = integrated::ProductScoreState {
                        accepted_epm_sources:
                            integrated::product_epm_receipts_carried(
                                before.source.product, candidate,
                            ).update(accepted.frames.len() - 1, Some(prior)),
                        ..candidate
                    };
                    Some(integrated::LiveRuntimeProductState {
                        source: integrated::LiveSourceProductState {
                            product,
                            runtime: runtime_source::runtime_after_admission_step(
                                before.source.runtime, accepted,
                            ),
                            arena: before.source.arena,
                            wire: before.source.wire.drop_last(),
                        },
                        cursors: before.cursors.drop_last(),
                    })
                },
                _ => None,
            },
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_epm_child_complete_selected_dispatches(
    before: integrated::LiveRuntimeProductState,
)
    requires generated_epm_child_complete_selected(before).is_some(),
    ensures integrated::live_runtime_product_step(
        before, generated_epm_child_complete_selected(before).unwrap(),
        integrated::LiveRuntimeProductAction::EpmFinishAccept,
    ),
{
    let after = generated_epm_child_complete_selected(before).unwrap();
    assert(integrated::product_epm_finish_accept_step(
        before.source.product, after.source.product,
    ));
    assert(integrated::live_runtime_product_epm_finish_accept_step(before, after));
}

/// A scored child waiting at its EOF is identified by the parent's retained
/// Send slot/tag, the indexed child bytes, the assigned model body ID, and the
/// arena prefix seen before the child ran. The later Accept uses that same
/// identity instead of an unrelated returned summary.
#[cfg(rpb3_live_push)]
pub open spec fn send_live_waiting_parent_matches_fold(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
) -> bool {
    if !integrated::live_runtime_product_wf(live)
        || live.source.product.bridge.admission.frames.len() < 2
        || live.source.product.indexed_regions.len() < 2
    {
        false
    } else {
        let parent = live.source.product.bridge.admission.frames[
            live.source.product.bridge.admission.frames.len() - 2
        ];
        let child = live.source.product.bridge.admission.frames.last();
        &&& parent.frame_kind == 1
        &&& parent.program_type_id == 15
        &&& parent.family == admission::AdmissionFamily::Send
        &&& parent.body_id == source.body_id
        &&& parent.source_bytes == source.indexed.bytes
        &&& parent.slot_count == 6
        &&& parent.cursor == folded.fields.cursor
        &&& parent.last_slot == folded.fields.last_slot
        &&& parent.seen == folded.fields.seen
        &&& parent.scalar_slots == folded.fields.scalar_slots
        &&& parent.scalar_occurrences == folded.fields.scalar_occurrences
        &&& parent.returns == folded.fields.returns
        &&& integrated::product_admission_source_image(
            live.source.product.indexed_regions[
                live.source.product.indexed_regions.len() - 2
            ],
        ) == source.indexed
        &&& send_score_prefix(
            folded.score_entries, live.source.product.bridge.transcript.entries,
        )
        &&& match folded.fields.pending {
            Some((slot, tag, body_id, body)) => {
                &&& parent.waiting_slot == Some(slot)
                &&& parent.waiting_tag == Some(tag)
                &&& parent.waiting_id == child.return_id
                &&& child.body_id == body_id
                &&& child.source_bytes == body
            },
            None => false,
        }
    }
}

/// Pair a source Push with the selected live child publication. The child
/// request carries only pre/post-independent Rust observations (kind, frame,
/// capacity); the successor is constructed by `send_live_push_selected`.
#[cfg(rpb3_live_push)]
pub open spec fn send_source_live_push_selected(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    oneof_arm: Option<u32>,
    child_start: admission::AdmissionSourceAnchor,
    child_end: admission::AdmissionSourceAnchor,
    body_id: u64,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> Option<(SendDriverReplay, integrated::LiveRuntimeProductState)> {
    if !send_live_frame_matches_fold(source, folded, live)
        || folded.fields.pending.is_some()
    {
        None
    } else {
        match (
            send_driver_source_step(
                source, folded,
                SendDriverSourceAction::Push {
                    field, oneof_arm, child_start, child_end, body_id,
                },
            ),
            send_live_push_selected(
                live, field, source.indexed, child_start, body_id, request,
                actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
            ),
        ) {
            (Some(next_fold), Some(next_live))
                if send_live_waiting_parent_matches_fold(
                    source, next_fold, next_live,
                ) => Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_source_live_push_selected_exact(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    oneof_arm: Option<u32>,
    child_start: admission::AdmissionSourceAnchor,
    child_end: admission::AdmissionSourceAnchor,
    body_id: u64,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires send_source_live_push_selected(
        source, folded, live, field, oneof_arm,
        child_start, child_end, body_id, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = send_source_live_push_selected(
            source, folded, live, field, oneof_arm,
            child_start, child_end, body_id, request, actual_top_kind,
            after_len, after_capacity, stats_frame_variant_bytes,
        ).unwrap();
        &&& send_live_frame_matches_fold(source, folded, live)
        &&& send_live_waiting_parent_matches_fold(source, next_fold, next_live)
        &&& send_field_arm_matches(field, oneof_arm, true)
        &&& send_driver_source_step(
            source, folded,
            SendDriverSourceAction::Push {
                field, oneof_arm, child_start, child_end, body_id,
            },
        ) == Some(next_fold)
        &&& integrated::live_runtime_product_step(
            live, next_live,
            integrated::LiveRuntimeProductAction::MessagePush {
                request, actual_top_kind, after_len, after_capacity,
                stats_frame_variant_bytes,
            },
        )
    },
{
    send_live_push_selected_dispatches(
        live, field, source.indexed, child_start, body_id, request,
        actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn send_source_live_accept_selected(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
) -> Option<(SendDriverReplay, integrated::LiveRuntimeProductState)> {
    if !send_live_waiting_parent_matches_fold(source, folded, live)
        || score_entries_after != live.source.product.bridge.transcript.entries
        || live.source.product.bridge.admission.frames.last().body_id != returned.body_id
        || live.source.product.bridge.admission.frames.last().score_handle
            != returned.score_handle
    {
        None
    } else {
        match (
            send_driver_source_step(
                source, folded,
                SendDriverSourceAction::Accepted { returned, score_entries_after },
            ),
            send_live_child_complete_selected(live),
        ) {
            (Some(next_fold), Some(next_live))
                if send_live_frame_matches_fold(source, next_fold, next_live) =>
                Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_source_live_accept_selected_exact(
    source: SendDriverSource,
    folded: SendDriverReplay,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
)
    requires send_source_live_accept_selected(
        source, folded, live, returned, score_entries_after,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = send_source_live_accept_selected(
            source, folded, live, returned, score_entries_after,
        ).unwrap();
        &&& send_live_waiting_parent_matches_fold(source, folded, live)
        &&& send_live_frame_matches_fold(source, next_fold, next_live)
        &&& send_driver_source_step(
            source, folded,
            SendDriverSourceAction::Accepted { returned, score_entries_after },
        ) == Some(next_fold)
        &&& integrated::live_runtime_product_step(
            live, next_live,
            integrated::LiveRuntimeProductAction::MessageChildComplete,
        )
    },
{
    send_live_child_complete_selected_dispatches(live);
}

/// The full Send source replay supplies the score selector's result. Given
/// independent live root-frame and EOF-reader observations, constructing the
/// successor above establishes the existing integrated RootScore dispatcher
/// edge; it is not assumed as a premise.
#[cfg(rpb3_live_push)]
pub proof fn send_replay_selects_live_root_score_step(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
    before: integrated::LiveRuntimeProductState,
)
    requires
        send_driver_replay_score_bridge(source, actions).is_some(),
        integrated::live_runtime_product_wf(before),
        before.source.product.bridge == source.eof_before,
        before.source.product.bridge.admission.frames.len() == 1,
        before.source.product.bridge.admission.frames[0].frame_kind == 1,
        before.source.product.epm_frames[0].is_none(),
        before.cursors.len() == 1,
        before.cursors[0].is_some(),
        wire_source::wire_runtime_next_field(before.cursors[0].unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors[0].unwrap(),
                ).unwrap(),
            }),
    ensures {
        let selected = send_driver_replay_score_bridge(source, actions).unwrap();
        let after = send_live_score_after(before, selected);
        &&& bridge::bridge_score_finish_selected(source.eof_before) == Some(selected)
        &&& integrated::live_runtime_product_root_score_step(before, after)
        &&& integrated::live_runtime_product_step(
            before, after, integrated::LiveRuntimeProductAction::RootScore,
        )
        &&& after.source.product.bridge.transcript.entries
            == send_driver_source_replay(source, actions).unwrap().score_entries
    },
{
    send_driver_source_terminal_exact(source, actions);
    let selected = send_driver_replay_score_bridge(source, actions).unwrap();
    bridge::bridge_score_finish_selected_is_source_step(source.eof_before);
    let after = send_live_score_after(before, selected);
    assert(bridge::bridge_score_source_step(
        before.source.product.bridge, after.source.product.bridge,
    ));
    assert(integrated::product_root_score_step(
        before.source.product, after.source.product,
    ));
    assert(integrated::live_runtime_product_root_score_step(before, after));
}

/// Select the whole root EOF result from the source replay and runtime EOF
/// reducer. The returned summary is computed, rather than supplied as an
/// independent terminal claim.
#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn send_live_root_return_selected(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
    before: integrated::LiveRuntimeProductState,
) -> Option<(integrated::LiveRuntimeProductState, runtime_source::RuntimeRootReturnImage)> {
    match (
        send_driver_replay_score_bridge(source, actions),
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        ),
    ) {
        (
            Some(selected),
            Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }),
        ) => Some((send_live_score_after(before, selected), returned)),
        _ => None,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_live_root_return_selected_components(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
    before: integrated::LiveRuntimeProductState,
)
    requires send_live_root_return_selected(source, actions, before).is_some(),
    ensures match (
        send_driver_replay_score_bridge(source, actions),
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        ),
    ) {
        (Some(selected),
            Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned })) =>
            send_live_root_return_selected(source, actions, before)
                == Some((send_live_score_after(before, selected), returned)),
            _ => false,
    },
{
    reveal(send_live_root_return_selected);
    match (
        send_driver_replay_score_bridge(source, actions),
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        ),
    ) {
        (Some(_), Some(runtime_source::RuntimeMessageOutcome::RootReturned { .. })) => {},
        _ => assert(false),
    }
}

#[cfg(rpb3_live_push)]
pub proof fn send_replay_selects_live_root_return_step(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
    before: integrated::LiveRuntimeProductState,
)
    requires
        send_live_root_return_selected(source, actions, before).is_some(),
        integrated::live_runtime_product_wf(before),
        before.source.product.bridge == source.eof_before,
        before.source.product.bridge.admission.frames.len() == 1,
        before.source.product.bridge.admission.frames[0].frame_kind == 1,
        before.source.product.bridge.admission.frames[0].waiting_id.is_none(),
        before.source.product.bridge.admission.frames[0].score_handle.is_none(),
        before.source.product.epm_frames[0].is_none(),
        before.cursors.len() == 1,
        before.cursors[0].is_some(),
        wire_source::wire_runtime_next_field(before.cursors[0].unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors[0].unwrap(),
                ).unwrap(),
            }),
    ensures {
        let (after, returned) = send_live_root_return_selected(
            source, actions, before,
        ).unwrap();
        &&& integrated::live_runtime_product_step(
            before, after,
            integrated::LiveRuntimeProductAction::RootComplete { returned },
        )
    },
{
    send_live_root_return_selected_components(source, actions, before);
    assert(send_driver_replay_score_bridge(source, actions).is_some());
    send_replay_selects_live_root_score_step(source, actions, before);
    let selected = send_driver_replay_score_bridge(source, actions).unwrap();
    let (after, returned) = send_live_root_return_selected(source, actions, before).unwrap();
    assert(after == send_live_score_after(before, selected));
    assert(runtime_source::runtime_message_driver_step(
        before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
    ) == Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }));
    assert(integrated::live_runtime_product_root_score_step(before, after));
    assert(integrated::live_runtime_product_root_complete_step(
        before, after, returned,
    ));
}

/// The same selected EOF construction supplies the integrated child-score
/// action while the child frame remains live. Pop/Accept is a later action.
#[cfg(rpb3_live_push)]
pub proof fn send_replay_selects_live_child_score_step(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
    before: integrated::LiveRuntimeProductState,
)
    requires
        send_driver_replay_score_bridge(source, actions).is_some(),
        integrated::live_runtime_product_wf(before),
        before.source.product.bridge == source.eof_before,
        before.source.product.bridge.admission.frames.len() > 1,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
        before.source.product.map.map_frames.last().is_none(),
        before.source.product.bridge.admission.frames.last().waiting_id.is_none(),
        before.source.product.bridge.admission.frames.last().score_handle.is_none(),
        before.source.product.bridge.admission.frames.last().cursor
            == before.source.product.bridge.admission.frames.last().region_end,
        runtime_source::runtime_noscore_child_kind(before.source.runtime).is_none(),
        before.cursors.len() > 1,
        before.cursors.last().is_some(),
        wire_source::wire_runtime_next_field(before.cursors.last().unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors.last().unwrap(),
                ).unwrap(),
            }),
    ensures {
        let selected = send_driver_replay_score_bridge(source, actions).unwrap();
        let after = send_live_score_after(before, selected);
        &&& bridge::bridge_score_finish_selected(source.eof_before) == Some(selected)
        &&& integrated::live_runtime_product_message_child_score_step(before, after)
        &&& integrated::live_runtime_product_step(
            before, after, integrated::LiveRuntimeProductAction::MessageChildScore,
        )
        &&& after.source.product.bridge.transcript.entries
            == send_driver_source_replay(source, actions).unwrap().score_entries
    },
{
    send_driver_source_terminal_exact(source, actions);
    let selected = send_driver_replay_score_bridge(source, actions).unwrap();
    bridge::bridge_score_finish_selected_is_source_step(source.eof_before);
    let after = send_live_score_after(before, selected);
    assert(bridge::bridge_score_source_step(
        before.source.product.bridge, after.source.product.bridge,
    ));
    assert(integrated::product_message_child_score_step(
        before.source.product, after.source.product,
    ));
    assert(integrated::live_runtime_product_message_child_score_step(before, after));
}

/// A successful run can terminate only at its single observed EOF; the same
/// frame, body, score prefix, component handle, and completeness bit are then
/// selected by the shared score-admission model. This proof is deliberately
/// conditional on *replay success*, not on a caller's asserted transition.
pub proof fn send_driver_source_terminal_exact(
    source: SendDriverSource,
    actions: Seq<SendDriverSourceAction>,
)
    ensures match send_driver_source_replay(source, actions) {
        Some(final_state) if final_state.terminal => {
            &&& actions.len() > 0
            &&& match actions.last() {
                SendDriverSourceAction::Eof { .. } => true,
                _ => false,
            }
            &&& final_state.eof_observation.is_some()
            &&& final_state.outcome.is_some()
            &&& final_state.eof_observation.unwrap().indexed == source.indexed
            &&& final_state.eof_observation.unwrap().body_id == source.body_id
            &&& final_state.eof_observation.unwrap().events == final_state.field_events
            &&& final_state.eof_observation.unwrap().score_entries_after
                == final_state.score_entries
            &&& match final_state.outcome.unwrap() {
                message::GenericMessageEofModelOutcome::Scored {
                    after, score_handle, semantic_complete, body_id, body_bytes, ..
                } => {
                    &&& bridge::bridge_score_finish_selected(source.eof_before) == Some(after)
                    &&& after.transcript.entries == final_state.score_entries
                    &&& score_handle == final_state.eof_observation.unwrap()
                        .component.unwrap().score_handle
                    &&& semantic_complete == final_state.eof_observation.unwrap()
                        .component.unwrap().complete
                    &&& semantic_complete == admission::admission_family_complete(
                        source.eof_before.admission.frames.last(),
                    )
                    &&& semantic_complete == admission::admission_returned_par(
                        source.eof_before.admission.frames.last(), 0,
                    )
                    &&& body_id == source.body_id
                    &&& body_bytes == source.indexed.bytes
                },
                _ => false,
            }
        },
        _ => true,
    },
    decreases actions.len(),
{
    if actions.len() > 0 {
        let prefix = actions.subrange(0, (actions.len() - 1) as int);
        send_driver_source_terminal_exact(source, prefix);
        if let Some(final_state) = send_driver_source_replay(source, actions) {
            if final_state.terminal {
                let before = send_driver_source_replay(source, prefix).unwrap();
                let observed = final_state.eof_observation.unwrap();
                let outcome = final_state.outcome.unwrap();
                assert(message::generic_message_eof_from_source(
                    source.eof_before, observed,
                ) == Some(outcome));
                reveal(message::generic_message_eof_from_source);
                reveal(message::generic_message_eof_source_matches_before);
                message::generic_message_eof_selected_result_exact(
                    source.eof_before, observed,
                );
                assert(before.terminal == false);
            }
        }
    }
}

pub struct GeneratedMessageFieldSource {
    pub program_type_id: u16,
    pub family: admission::AdmissionFamily,
    pub slot_count: usize,
    pub body_id: u64,
    pub body_len: usize,
    pub ghost indexed: admission::AdmissionIndexedSourceImage,
}

pub struct GeneratedMessageScalarReceipt {
    pub field: admission::IndexedCanonicalFieldModel,
    pub slot_count: usize,
    pub slot: usize,
    pub repeated: bool,
    pub oneof_arm: Option<u32>,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub last_slot_before: Option<usize>,
    pub last_slot_after: Option<usize>,
    pub seen_before: bool,
    pub seen_after: bool,
}

pub open spec fn generated_message_family_for_type(
    program_type_id: u16,
) -> admission::AdmissionFamily {
    match program_type_id {
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

pub open spec fn generated_message_scalar_receipt_matches(
    source: GeneratedMessageFieldSource,
    before: message::GenericMessageSourceFold,
    receipt: GeneratedMessageScalarReceipt,
) -> bool {
    &&& admission::admission_indexed_source_wf(source.indexed)
    &&& source.body_len as int == source.indexed.bytes.len()
    &&& generated::generated_slot_count(source.program_type_id) == Some(source.slot_count)
    &&& receipt.slot_count == source.slot_count
    &&& source.family == generated_message_family_for_type(source.program_type_id)
    &&& match generated::generated_descriptor(
        source.program_type_id, receipt.field.number,
    ) {
        Some(descriptor) => {
            &&& descriptor.frame_kind == 0
            &&& descriptor.slot == receipt.slot
            &&& descriptor.repeated == receipt.repeated
            &&& descriptor.oneof_arm == receipt.oneof_arm
            &&& descriptor.wire == receipt.field.wire
        },
        None => false,
    }
    &&& before.pending.is_none()
    &&& receipt.cursor_before == before.cursor
    &&& receipt.cursor_after == receipt.field.encoded_end
    &&& receipt.last_slot_before == before.last_slot
    &&& receipt.last_slot_after == Some(receipt.slot)
    &&& receipt.seen_before == before.seen.contains(receipt.slot as int)
    &&& receipt.seen_after
}

pub open spec fn generated_message_scalar_source_step(
    source: GeneratedMessageFieldSource,
    before: message::GenericMessageSourceFold,
    receipt: GeneratedMessageScalarReceipt,
) -> Option<message::GenericMessageSourceFold> {
    if !generated_message_scalar_receipt_matches(source, before, receipt) {
        None
    } else {
        match message::generic_message_source_event_step(
            before,
            message::GenericMessageSourceEvent::Scalar { field: receipt.field },
            source.program_type_id, source.family, source.slot_count,
            source.body_len, source.indexed,
        ) {
            Some(after) if after.cursor == receipt.cursor_after
                && after.last_slot == receipt.last_slot_after
                && after.seen.contains(receipt.slot as int) == receipt.seen_after =>
                Some(after),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_live_frame_matches_fold(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
) -> bool {
    if !integrated::live_runtime_product_wf(live)
        || live.source.product.bridge.admission.frames.len() == 0
        || live.source.product.indexed_regions.len() == 0
    {
        false
    } else {
        let frame = live.source.product.bridge.admission.frames.last();
        &&& frame.frame_kind == 1
        &&& frame.program_type_id == source.program_type_id
        &&& frame.family == source.family
        &&& frame.body_id == source.body_id
        &&& frame.source_bytes == source.indexed.bytes
        &&& frame.slot_count == source.slot_count
        &&& frame.cursor == folded.cursor
        &&& frame.last_slot == folded.last_slot
        &&& frame.seen == folded.seen
        &&& frame.scalar_slots == folded.scalar_slots
        &&& frame.scalar_occurrences == folded.scalar_occurrences
        &&& frame.returns == folded.returns
        &&& integrated::product_admission_source_image(
            live.source.product.indexed_regions.last(),
        ) == source.indexed
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_scalar_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessageScalarReceipt,
) -> Option<(message::GenericMessageSourceFold,
    integrated::LiveRuntimeProductState)> {
    if !generated_message_live_frame_matches_fold(source, folded, live) {
        None
    } else {
        match (
            generated_message_scalar_source_step(source, folded, receipt),
            send_live_continue_selected(live, receipt.field, source.indexed),
        ) {
            (Some(next_fold), Some(next_live))
                if generated_message_live_frame_matches_fold(
                    source, next_fold, next_live,
                ) => Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_scalar_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessageScalarReceipt,
)
    requires generated_message_scalar_live_selected(
        source, folded, live, receipt,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = generated_message_scalar_live_selected(
            source, folded, live, receipt,
        ).unwrap();
        &&& generated_message_scalar_receipt_matches(source, folded, receipt)
        &&& generated_message_live_frame_matches_fold(source, folded, live)
        &&& generated_message_live_frame_matches_fold(source, next_fold, next_live)
        &&& generated_message_scalar_source_step(source, folded, receipt)
            == Some(next_fold)
        &&& next_fold.cursor == receipt.cursor_after
        &&& next_fold.last_slot == receipt.last_slot_after
        &&& next_fold.seen == folded.seen.insert(receipt.slot as int)
        &&& next_fold.scalar_slots
            == folded.scalar_slots.insert(receipt.slot as int)
        &&& next_fold.returns == folded.returns
        &&& integrated::live_runtime_product_step(
            live, next_live, integrated::LiveRuntimeProductAction::ContinueIndexed,
        )
    },
{
    send_live_continue_selected_dispatches(live, receipt.field, source.indexed);
}

#[cfg(rpb3_live_push)]
pub struct GeneratedMessagePushReceipt {
    pub field: admission::IndexedCanonicalFieldModel,
    pub slot_count: usize,
    pub slot: usize,
    pub repeated: bool,
    pub oneof_arm: Option<u32>,
    pub child_type_id: Option<u16>,
    pub child_start: admission::AdmissionSourceAnchor,
    pub child_end: admission::AdmissionSourceAnchor,
    /// Identity assigned to the published child, distinct from the fresh
    /// request frame's model-default body ID.
    pub body_id: u64,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub last_slot_before: Option<usize>,
    pub last_slot_after: Option<usize>,
    pub seen_before: bool,
    pub seen_after: bool,
    pub ghost request: runtime_source::SourceFrameRequestImage,
    pub actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    pub after_len: usize,
    pub after_capacity: usize,
    pub stats_frame_variant_bytes: usize,
}

/// The request contains a fresh child. The admission worklist assigns its
/// return obligation and body identity only when it publishes that child.
#[cfg(rpb3_live_push)]
pub proof fn generated_message_published_child_identity_exact(
    before: admission::AdmissionState,
    parent_next: usize,
    fresh: admission::AdmissionFrameState,
    slot: usize,
    repeated: bool,
    field_tag: u32,
    frame_bytes: usize,
)
    requires admission::admission_push_step(
        before, parent_next, fresh, slot, repeated, field_tag, frame_bytes,
    ).is_some(),
    ensures {
        let after = admission::admission_push_step(
            before, parent_next, fresh, slot, repeated, field_tag, frame_bytes,
        ).unwrap();
        let obligation = before.next_obligation_id;
        &&& fresh.return_id.is_none()
        &&& obligation != u64::MAX
        &&& after.frames.last() == admission::AdmissionFrameState {
            return_id: Some(obligation),
            body_id: (obligation as int + 1) as u64,
            ..fresh
        }
        &&& after.frames[before.frames.len() - 1].waiting_id == Some(obligation)
        &&& after.next_obligation_id as int == obligation as int + 1
        &&& after.frames.last() != fresh
    },
{}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_push_receipt_matches(
    source: GeneratedMessageFieldSource,
    before: message::GenericMessageSourceFold,
    receipt: GeneratedMessagePushReceipt,
) -> bool {
    &&& admission::admission_indexed_source_wf(source.indexed)
    &&& source.body_len as int == source.indexed.bytes.len()
    &&& generated::generated_slot_count(source.program_type_id) == Some(source.slot_count)
    &&& receipt.slot_count == source.slot_count
    &&& source.family == generated_message_family_for_type(source.program_type_id)
    &&& match generated::generated_descriptor(
        source.program_type_id, receipt.field.number,
    ) {
        Some(descriptor) => {
            &&& descriptor.frame_kind != 0
            &&& descriptor.frame_kind == receipt.request.child.frame_kind
            &&& descriptor.slot == receipt.slot
            &&& descriptor.repeated == receipt.repeated
            &&& descriptor.oneof_arm == receipt.oneof_arm
            &&& descriptor.wire == receipt.field.wire
            &&& descriptor.child_type_id == receipt.child_type_id
            &&& (descriptor.frame_kind != 1
                || receipt.child_type_id
                    == Some(receipt.request.child.program_type_id))
        },
        None => false,
    }
    &&& before.pending.is_none()
    &&& receipt.cursor_before == before.cursor
    &&& receipt.cursor_after == receipt.field.encoded_end
    &&& receipt.last_slot_before == before.last_slot
    &&& receipt.last_slot_after == Some(receipt.slot)
    &&& receipt.seen_before == before.seen.contains(receipt.slot as int)
    &&& receipt.seen_after
    &&& receipt.request.field == receipt.field
    &&& receipt.request.child.source_bytes == receipt.field.payload_bytes
    &&& receipt.request.child.physical_source == receipt.child_start.source
    &&& receipt.request.child.physical_start == receipt.child_start.offset
    &&& runtime_source::source_frame_request_kind_code(
        receipt.request.request_kind,
    ) == receipt.request.child.frame_kind
    &&& admission::admission_indexed_source_position(
        source.indexed, receipt.field.payload_start,
    ) == Some(receipt.child_start)
    &&& admission::admission_indexed_source_position(
        source.indexed, receipt.field.payload_end,
    ) == Some(receipt.child_end)
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_push_source_step(
    source: GeneratedMessageFieldSource,
    before: message::GenericMessageSourceFold,
    receipt: GeneratedMessagePushReceipt,
) -> Option<message::GenericMessageSourceFold> {
    if !generated_message_push_receipt_matches(source, before, receipt) {
        None
    } else {
        match message::generic_message_source_event_step(
            before,
            message::GenericMessageSourceEvent::Push {
                field: receipt.field, body_id: receipt.body_id,
            },
            source.program_type_id, source.family, source.slot_count,
            source.body_len, source.indexed,
        ) {
            Some(after) if after.cursor == receipt.cursor_after
                && after.last_slot == receipt.last_slot_after
                && after.seen.contains(receipt.slot as int) == receipt.seen_after =>
                Some(after),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_live_waiting_parent_matches_fold(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    before: integrated::LiveRuntimeProductState,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || !integrated::live_runtime_product_wf(live)
        || live.source.product.bridge.admission.frames.len() < 2
        || live.source.product.indexed_regions.len() < 2
    {
        false
    } else {
        let obligation = before.source.product.bridge.admission.next_obligation_id;
        let parent = live.source.product.bridge.admission.frames[
            live.source.product.bridge.admission.frames.len() - 2
        ];
        let child = live.source.product.bridge.admission.frames.last();
        &&& obligation != u64::MAX
        &&& receipt.body_id as int == obligation as int + 1
        &&& receipt.request.child.return_id.is_none()
        &&& parent.frame_kind == 1
        &&& parent.program_type_id == source.program_type_id
        &&& parent.family == source.family
        &&& parent.body_id == source.body_id
        &&& parent.source_bytes == source.indexed.bytes
        &&& parent.slot_count == source.slot_count
        &&& parent.cursor == folded.cursor
        &&& parent.last_slot == folded.last_slot
        &&& parent.seen == folded.seen
        &&& parent.scalar_slots == folded.scalar_slots
        &&& parent.scalar_occurrences == folded.scalar_occurrences
        &&& parent.returns == folded.returns
        &&& parent.waiting_kind == receipt.request.child.frame_kind
        &&& parent.waiting_slot == Some(receipt.slot)
        &&& parent.waiting_tag == Some(receipt.field.number)
        &&& parent.waiting_id == Some(obligation)
        &&& child == admission::AdmissionFrameState {
            return_id: Some(obligation),
            body_id: receipt.body_id,
            ..receipt.request.child
        }
        &&& integrated::product_admission_source_image(
            live.source.product.indexed_regions[
                live.source.product.indexed_regions.len() - 2
            ],
        ) == source.indexed
        &&& folded.pending == Some((
            receipt.slot, receipt.field.number, receipt.body_id,
            receipt.field.payload_bytes,
        ))
    }
}

/// Checkpoint 4: one checked non-EPathMap Push keeps the source fold aligned
/// with the *published* child. The child in the request is still fresh.
#[cfg(rpb3_live_push)]
pub proof fn generated_message_push_checked_step_binds_fold(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
)
    requires
        generated_message_live_frame_matches_fold(source, folded, live),
        generated_message_push_source_step(source, folded, receipt).is_some(),
        send_live_push_selected(
            live, receipt.field, source.indexed, receipt.child_start,
            receipt.body_id, receipt.request, receipt.actual_top_kind,
            receipt.after_len, receipt.after_capacity,
            receipt.stats_frame_variant_bytes,
        ).is_some(),
    ensures generated_message_live_waiting_parent_matches_fold(
        source,
        generated_message_push_source_step(source, folded, receipt).unwrap(),
        live,
        send_live_push_selected(
            live, receipt.field, source.indexed, receipt.child_start,
            receipt.body_id, receipt.request, receipt.actual_top_kind,
            receipt.after_len, receipt.after_capacity,
            receipt.stats_frame_variant_bytes,
        ).unwrap(),
        receipt,
    ),
{
    hide(wire_source::wire_runtime_next_field);
    hide(integrated::live_runtime_child_cursor);
    hide(integrated::runtime_wire_after_field);
    hide(wire_source::wire_runtime_cursor_project);
    hide(integrated::product_wire_indexed_source_agrees);
    hide(wire_source::wire_runtime_derive_cursor);
    hide(wire_source::wire_concrete_cursor_wf);
    hide(wire_source::wire_cursor_shape);
    hide(admission::admission_state_wf);
    hide(admission::admission_frame_wf);
    hide(map::integrated_admission_wf);
    hide(bridge::bridge_well_formed);
    hide(runtime_source::runtime_frame_image_wf);
    hide(map::integrated_source_message_action);
    hide(integrated::product_frame_source_anchors_match);
    hide(integrated::product_epm_source_receipts_wf);
    hide(score::arena_well_formed);
    hide(admission::indexed_field_at_cursor);
    let next_fold = generated_message_push_source_step(
        source, folded, receipt,
    ).unwrap();
    let next_live = send_live_push_selected(
        live, receipt.field, source.indexed, receipt.child_start,
        receipt.body_id, receipt.request, receipt.actual_top_kind,
        receipt.after_len, receipt.after_capacity,
        receipt.stats_frame_variant_bytes,
    ).unwrap();
    let action = integrated::LiveRuntimeProductAction::MessagePush {
        request: receipt.request,
        actual_top_kind: receipt.actual_top_kind,
        after_len: receipt.after_len,
        after_capacity: receipt.after_capacity,
        stats_frame_variant_bytes: receipt.stats_frame_variant_bytes,
    };
    send_live_push_selected_dispatches(
        live, receipt.field, source.indexed, receipt.child_start,
        receipt.body_id, receipt.request, receipt.actual_top_kind,
        receipt.after_len, receipt.after_capacity,
        receipt.stats_frame_variant_bytes,
    );
    integrated::live_runtime_product_step_preserves_wf(
        live, next_live, action,
    );
    let prior = live.source.product.bridge.admission;
    assert(admission::admission_push_step(
        prior, receipt.field.encoded_end, receipt.request.child,
        receipt.slot, receipt.repeated, receipt.field.number,
        live.source.runtime.frame_bytes,
    ).is_some());
    admission::admission_push_refines(
        prior, receipt.field.encoded_end, receipt.request.child,
        receipt.slot, receipt.repeated, receipt.field.number,
        live.source.runtime.frame_bytes,
    );
    assert(next_fold.pending == Some((
        receipt.slot, receipt.field.number, receipt.body_id,
        receipt.field.payload_bytes,
    )));
    assert(generated_message_live_waiting_parent_matches_fold(
        source, next_fold, live, next_live, receipt,
    ));
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_push_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
) -> Option<(message::GenericMessageSourceFold,
    integrated::LiveRuntimeProductState)> {
    if !generated_message_live_frame_matches_fold(source, folded, live)
        || (receipt.request.child.frame_kind != 1
            && receipt.request.child.frame_kind != 2)
    {
        None
    } else {
        match (
            generated_message_push_source_step(source, folded, receipt),
            send_live_push_selected(
                live, receipt.field, source.indexed, receipt.child_start,
                receipt.body_id, receipt.request, receipt.actual_top_kind,
                receipt.after_len, receipt.after_capacity,
                receipt.stats_frame_variant_bytes,
            ),
        ) {
            (Some(next_fold), Some(next_live))
                if generated_message_live_waiting_parent_matches_fold(
                    source, next_fold, live, next_live, receipt,
                ) => Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_push_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
)
    requires generated_message_push_live_selected(
        source, folded, live, receipt,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = generated_message_push_live_selected(
            source, folded, live, receipt,
        ).unwrap();
        &&& generated_message_push_receipt_matches(source, folded, receipt)
        &&& generated_message_live_frame_matches_fold(source, folded, live)
        &&& generated_message_live_waiting_parent_matches_fold(
            source, next_fold, live, next_live, receipt,
        )
        &&& generated_message_push_source_step(source, folded, receipt)
            == Some(next_fold)
        &&& next_fold.cursor == receipt.cursor_after
        &&& next_fold.last_slot == receipt.last_slot_after
        &&& next_fold.seen == folded.seen.insert(receipt.slot as int)
        &&& next_fold.returns == folded.returns
        &&& integrated::live_runtime_product_step(
            live, next_live,
            integrated::LiveRuntimeProductAction::MessagePush {
                request: receipt.request,
                actual_top_kind: receipt.actual_top_kind,
                after_len: receipt.after_len,
                after_capacity: receipt.after_capacity,
                stats_frame_variant_bytes: receipt.stats_frame_variant_bytes,
            },
        )
    },
{
    send_live_push_selected_dispatches(
        live, receipt.field, source.indexed, receipt.child_start,
        receipt.body_id, receipt.request, receipt.actual_top_kind,
        receipt.after_len, receipt.after_capacity,
        receipt.stats_frame_variant_bytes,
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_epm_push_selected(
    before: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    indexed: admission::AdmissionIndexedSourceImage,
    child_start: admission::AdmissionSourceAnchor,
    body_id: u64,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> Option<integrated::LiveRuntimeProductState> {
    if !integrated::live_runtime_product_wf(before)
        || !integrated::product_frame_source_anchors_match(before.source.product)
        || before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
        || indexed != integrated::product_admission_source_image(
            before.source.product.indexed_regions.last(),
        )
        || request.request_kind != runtime_source::SourceFrameRequestKind::EPathMap
        || request.field != field
        || request.child.frame_kind != 3
        || before.source.product.bridge.admission.next_obligation_id == u64::MAX
        || body_id as int != before.source.product.bridge.admission.next_obligation_id as int + 1
        || request.child.source_bytes != field.payload_bytes
        || request.child.physical_source != child_start.source
        || request.child.physical_start != child_start.offset
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
    {
        None
    } else {
        match before.cursors.last() {
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field: concrete_field, start: _,
                }) if concrete_field == field => {
                    let next_parent = integrated::runtime_wire_after_field(
                        parent, concrete_after,
                    );
                    match (
                        wire_source::wire_runtime_cursor_project(next_parent),
                        runtime_source::source_frame_push_observe_after(
                            request.request_kind, actual_top_kind,
                            before.source.product.bridge.admission.frames.len() as usize,
                            before.source.runtime.capacity, after_len, after_capacity,
                            before.source.runtime.frame_bytes,
                            stats_frame_variant_bytes,
                        ),
                    ) {
                        (Some(next_parent_reader),
                            runtime_source::SourceFramePushObservationResult::Success {
                                storage,
                            }) => {
                            let frame_action =
                                admission::ConcreteFrameActionModel::PushIndexed {
                                    field,
                                    child: request.child,
                                    parent_source: indexed,
                                    frame_bytes: before.source.runtime.frame_bytes,
                                };
                            let action = map::IntegratedAdmissionAction::Message {
                                action: frame_action,
                            };
                            if !integrated::product_wire_indexed_source_agrees(
                                parent, before.source.product.indexed_regions.last(),
                            ) || map::integrated_source_message_action(
                                before.source.product.map.admission, frame_action,
                            ) != Some(frame_action)
                            {
                                None
                            } else {
                                match (
                                    map::integrated_admission_driver_step(
                                        before.source.product.map, action,
                                    ),
                                    runtime_source::source_checked_frame_request_push_indexed(
                                        before.source.runtime, request, indexed, storage,
                                    ),
                                ) {
                                    (Some(outcome), Some(next_runtime)) => match (
                                        map::integrated_nonterminal_state(outcome),
                                        integrated::product_map_regions(
                                            before.source.product, action, outcome,
                                        ),
                                        integrated::product_map_epm_frames(
                                            before.source.product, outcome,
                                        ),
                                    ) {
                                        (Some(next_map), Some(regions), Some(epm_frames)) => {
                                            let candidate = integrated::ProductScoreState {
                                                bridge: bridge::ScoreAdmissionBridge {
                                                    admission: next_map.admission,
                                                    transcript: next_map.transcript,
                                                },
                                                map: next_map,
                                                epm_frames,
                                                indexed_regions: regions,
                                                accepted_epm_sources: Seq::empty(),
                                            };
                                            let product = integrated::ProductScoreState {
                                                accepted_epm_sources:
                                                    integrated::product_epm_receipts_carried(
                                                        before.source.product, candidate,
                                                    ),
                                                ..candidate
                                            };
                                            Some(integrated::LiveRuntimeProductState {
                                                source: integrated::LiveSourceProductState {
                                                    product,
                                                    runtime: next_runtime,
                                                    arena: before.source.arena,
                                                    wire: before.source.wire.update(
                                                        before.source.wire.len() - 1,
                                                        Some(next_parent_reader),
                                                    ).push(None),
                                                },
                                                cursors: before.cursors.update(
                                                    before.cursors.len() - 1,
                                                    Some(next_parent),
                                                ).push(None),
                                            })
                                        },
                                        _ => None,
                                    },
                                    _ => None,
                                }
                            }
                        },
                        _ => None,
                    }
                },
                _ => None,
            },
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_epm_push_selected_dispatches(
    before: integrated::LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
    indexed: admission::AdmissionIndexedSourceImage,
    child_start: admission::AdmissionSourceAnchor,
    body_id: u64,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires generated_message_epm_push_selected(
        before, field, indexed, child_start, body_id, request,
        actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
    ).is_some(),
    ensures integrated::live_runtime_product_step(
        before,
        generated_message_epm_push_selected(
            before, field, indexed, child_start, body_id, request,
            actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
        ).unwrap(),
        integrated::LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        },
    ),
{
    let after = generated_message_epm_push_selected(
        before, field, indexed, child_start, body_id, request,
        actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
    ).unwrap();
    assert(integrated::live_runtime_product_message_epm_push_indexed_observed_step(
        before, after, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ));
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_epm_push_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
) -> Option<(message::GenericMessageSourceFold,
    integrated::LiveRuntimeProductState)> {
    if !generated_message_live_frame_matches_fold(source, folded, live)
        || receipt.request.child.frame_kind != 3
    {
        None
    } else {
        match (
            generated_message_push_source_step(source, folded, receipt),
            generated_message_epm_push_selected(
                live, receipt.field, source.indexed, receipt.child_start,
                receipt.body_id, receipt.request, receipt.actual_top_kind,
                receipt.after_len, receipt.after_capacity,
                receipt.stats_frame_variant_bytes,
            ),
        ) {
            (Some(next_fold), Some(next_live))
                if generated_message_live_waiting_parent_matches_fold(
                    source, next_fold, live, next_live, receipt,
                ) => Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_epm_push_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
)
    requires generated_message_epm_push_live_selected(
        source, folded, live, receipt,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = generated_message_epm_push_live_selected(
            source, folded, live, receipt,
        ).unwrap();
        &&& generated_message_push_receipt_matches(source, folded, receipt)
        &&& generated_message_live_frame_matches_fold(source, folded, live)
        &&& generated_message_live_waiting_parent_matches_fold(
            source, next_fold, live, next_live, receipt,
        )
        &&& generated_message_push_source_step(source, folded, receipt)
            == Some(next_fold)
        &&& next_fold.cursor == receipt.cursor_after
        &&& next_fold.last_slot == receipt.last_slot_after
        &&& next_fold.seen == folded.seen.insert(receipt.slot as int)
        &&& next_fold.returns == folded.returns
        &&& integrated::live_runtime_product_step(
            live, next_live,
            integrated::LiveRuntimeProductAction::MessageEpmPush {
                request: receipt.request,
                actual_top_kind: receipt.actual_top_kind,
                after_len: receipt.after_len,
                after_capacity: receipt.after_capacity,
                stats_frame_variant_bytes: receipt.stats_frame_variant_bytes,
            },
        )
    },
{
    generated_message_epm_push_selected_dispatches(
        live, receipt.field, source.indexed, receipt.child_start,
        receipt.body_id, receipt.request, receipt.actual_top_kind,
        receipt.after_len, receipt.after_capacity,
        receipt.stats_frame_variant_bytes,
    );
}

#[cfg(rpb3_live_push)]
pub enum GeneratedMessageFieldAction {
    Scalar { receipt: GeneratedMessageScalarReceipt },
    Push { receipt: GeneratedMessagePushReceipt },
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_field_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    event: GeneratedMessageFieldAction,
) -> Option<(message::GenericMessageSourceFold,
    integrated::LiveRuntimeProductState,
    integrated::LiveRuntimeProductAction)> {
    match event {
        GeneratedMessageFieldAction::Scalar { receipt } => match
            generated_message_scalar_live_selected(source, folded, live, receipt)
        {
            Some((next_fold, next_live)) => Some((
                next_fold, next_live,
                integrated::LiveRuntimeProductAction::ContinueIndexed,
            )),
            None => None,
        },
        GeneratedMessageFieldAction::Push { receipt } => {
            if receipt.request.child.frame_kind == 3 {
                match generated_message_epm_push_live_selected(
                    source, folded, live, receipt,
                ) {
                    Some((next_fold, next_live)) => Some((
                        next_fold, next_live,
                        integrated::LiveRuntimeProductAction::MessageEpmPush {
                            request: receipt.request,
                            actual_top_kind: receipt.actual_top_kind,
                            after_len: receipt.after_len,
                            after_capacity: receipt.after_capacity,
                            stats_frame_variant_bytes: receipt.stats_frame_variant_bytes,
                        },
                    )),
                    None => None,
                }
            } else {
                match generated_message_push_live_selected(
                    source, folded, live, receipt,
                ) {
                    Some((next_fold, next_live)) => Some((
                        next_fold, next_live,
                        integrated::LiveRuntimeProductAction::MessagePush {
                            request: receipt.request,
                            actual_top_kind: receipt.actual_top_kind,
                            after_len: receipt.after_len,
                            after_capacity: receipt.after_capacity,
                            stats_frame_variant_bytes: receipt.stats_frame_variant_bytes,
                        },
                    )),
                    None => None,
                }
            }
        },
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_field_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    event: GeneratedMessageFieldAction,
)
    requires generated_message_field_live_selected(
        source, folded, live, event,
    ).is_some(),
    ensures {
        let (next_fold, next_live, action) =
            generated_message_field_live_selected(
                source, folded, live, event,
            ).unwrap();
        &&& integrated::live_runtime_product_step(live, next_live, action)
        &&& generated_message_live_frame_matches_fold(source, folded, live)
        &&& match event {
            GeneratedMessageFieldAction::Scalar { receipt } => {
                &&& generated_message_scalar_source_step(
                    source, folded, receipt,
                ) == Some(next_fold)
                &&& next_fold.seen == folded.seen.insert(receipt.slot as int)
            },
            GeneratedMessageFieldAction::Push { receipt } => {
                &&& generated_message_push_source_step(
                    source, folded, receipt,
                ) == Some(next_fold)
                &&& next_fold.seen == folded.seen.insert(receipt.slot as int)
            },
        }
    },
{
    match event {
        GeneratedMessageFieldAction::Scalar { receipt } => {
            generated_message_scalar_live_selected_exact(
                source, folded, live, receipt,
            );
        },
        GeneratedMessageFieldAction::Push { receipt } => {
            if receipt.request.child.frame_kind == 3 {
                generated_message_epm_push_live_selected_exact(
                    source, folded, live, receipt,
                );
            } else {
                generated_message_push_live_selected_exact(
                    source, folded, live, receipt,
                );
            }
        },
    }
}

/// A fresh Map request uses body ID zero, but the selected published child
/// uses the next obligation's successor ID. Forging only the receipt's
/// published ID back to zero must disable the very same live Push selector.
#[cfg(rpb3_live_push)]
pub proof fn generated_message_first_push_rejects_forged_published_body_id(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    receipt: GeneratedMessagePushReceipt,
)
    requires
        live.source.product.bridge.admission.next_obligation_id == 0,
        receipt.body_id == 1,
        receipt.request.child.body_id == 0,
        generated_message_field_live_selected(
            source, folded, live, GeneratedMessageFieldAction::Push { receipt },
        ).is_some(),
    ensures generated_message_field_live_selected(
        source, folded, live, GeneratedMessageFieldAction::Push {
            receipt: GeneratedMessagePushReceipt { body_id: 0, ..receipt },
        },
    ).is_none(),
{
    let forged = GeneratedMessagePushReceipt { body_id: 0, ..receipt };
    if forged.request.child.frame_kind == 3 {
        assert(generated_message_epm_push_selected(
            live, forged.field, source.indexed, forged.child_start,
            forged.body_id, forged.request, forged.actual_top_kind,
            forged.after_len, forged.after_capacity,
            forged.stats_frame_variant_bytes,
        ).is_none());
    } else {
        assert(send_live_push_selected(
            live, forged.field, source.indexed, forged.child_start,
            forged.body_id, forged.request, forged.actual_top_kind,
            forged.after_len, forged.after_capacity,
            forged.stats_frame_variant_bytes,
        ).is_none());
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_completed_child_matches_fold(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
) -> bool {
    if !integrated::live_runtime_product_wf(live)
        || live.source.product.bridge.admission.frames.len() < 2
        || live.source.product.indexed_regions.len() < 2
    {
        false
    } else {
        let parent = live.source.product.bridge.admission.frames[
            live.source.product.bridge.admission.frames.len() - 2
        ];
        let child = live.source.product.bridge.admission.frames.last();
        &&& parent.frame_kind == 1
        &&& parent.program_type_id == source.program_type_id
        &&& parent.family == source.family
        &&& parent.body_id == source.body_id
        &&& parent.source_bytes == source.indexed.bytes
        &&& parent.slot_count == source.slot_count
        &&& parent.cursor == folded.cursor
        &&& parent.last_slot == folded.last_slot
        &&& parent.seen == folded.seen
        &&& parent.scalar_slots == folded.scalar_slots
        &&& parent.scalar_occurrences == folded.scalar_occurrences
        &&& parent.returns == folded.returns
        &&& integrated::product_admission_source_image(
            live.source.product.indexed_regions[
                live.source.product.indexed_regions.len() - 2
            ],
        ) == source.indexed
        &&& live.source.product.bridge.transcript.entries == score_entries_after
        &&& child.body_id == returned.body_id
        &&& child.source_bytes == returned.body_bytes
        &&& match (child.frame_kind, returned.kind) {
            (1, _) => child.score_handle == returned.score_handle,
            (2, admission::AdmissionReturnedKind::Map {
                key_bytes, value_score_handle,
            }) => admission::admission_map_summary_complete(child)
                && admission::admission_map_source_matches(child, key_bytes)
                && value_score_handle == admission::admission_map_value_score_handle(child),
            (3, admission::AdmissionReturnedKind::EPathMapSnapshot { .. }) =>
                child.score_handle.is_none(),
            _ => false,
        }
        &&& parent.waiting_slot == Some(returned.slot)
        &&& parent.waiting_tag == Some(returned.tag)
        &&& parent.waiting_id == child.return_id
        &&& folded.pending == Some((
            returned.slot, returned.tag, returned.body_id,
            returned.body_bytes,
        ))
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_child_finish_action(
    frame_kind: u8,
) -> integrated::LiveRuntimeProductAction {
    if frame_kind == 2 {
        integrated::LiveRuntimeProductAction::MapEntryFinish
    } else if frame_kind == 3 {
        integrated::LiveRuntimeProductAction::EpmFinishAccept
    } else {
        integrated::LiveRuntimeProductAction::MessageChildComplete
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_child_finish_selected(
    live: integrated::LiveRuntimeProductState,
) -> Option<integrated::LiveRuntimeProductState> {
    if live.source.product.bridge.admission.frames.len() == 0 {
        None
    } else {
        match live.source.product.bridge.admission.frames.last().frame_kind {
            1 => send_live_child_complete_selected(live),
            2 => generated_map_child_complete_selected(live),
            3 => generated_epm_child_complete_selected(live),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_accept_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
) -> Option<(message::GenericMessageSourceFold,
    integrated::LiveRuntimeProductState)> {
    if !generated_message_completed_child_matches_fold(
        source, folded, live, returned, score_entries_after,
    ) {
        None
    } else {
        match (
            message::generic_message_source_event_step(
                folded,
                message::GenericMessageSourceEvent::Accepted { returned },
                source.program_type_id, source.family, source.slot_count,
                source.body_len, source.indexed,
            ),
            generated_message_child_finish_selected(live),
        ) {
            (Some(next_fold), Some(next_live))
                if generated_message_live_frame_matches_fold(
                    source, next_fold, next_live,
                ) => Some((next_fold, next_live)),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_accept_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
)
    requires generated_message_accept_live_selected(
        source, folded, live, returned, score_entries_after,
    ).is_some(),
    ensures {
        let (next_fold, next_live) = generated_message_accept_live_selected(
            source, folded, live, returned, score_entries_after,
        ).unwrap();
        &&& generated_message_completed_child_matches_fold(
            source, folded, live, returned, score_entries_after,
        )
        &&& generated_message_live_frame_matches_fold(
            source, next_fold, next_live,
        )
        &&& message::generic_message_source_event_step(
            folded,
            message::GenericMessageSourceEvent::Accepted { returned },
            source.program_type_id, source.family, source.slot_count,
            source.body_len, source.indexed,
        ) == Some(next_fold)
        &&& next_fold.cursor == folded.cursor
        &&& next_fold.seen == folded.seen
        &&& next_fold.returns == folded.returns.push(returned)
        &&& integrated::live_runtime_product_step(
            live, next_live,
            generated_message_child_finish_action(
                live.source.product.bridge.admission.frames.last().frame_kind,
            ),
        )
    },
{
    let child_kind = live.source.product.bridge.admission.frames.last().frame_kind;
    if child_kind == 1 {
        send_live_child_complete_selected_dispatches(live);
    } else if child_kind == 2 {
        generated_map_child_complete_selected_dispatches(live);
    } else {
        generated_epm_child_complete_selected_dispatches(live);
    }
}

#[cfg(rpb3_live_push)]
pub struct GeneratedMessageEofSource {
    pub ghost fields: Seq<message::GenericMessageSourceEvent>,
    pub ghost score_entries_after: Seq<score::ScoreEntry>,
    pub component: Option<message::GenericComponentEofSource>,
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_eof_observation(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    eof: GeneratedMessageEofSource,
) -> message::GenericMessageEofSourceObservation {
    message::GenericMessageEofSourceObservation {
        program_type_id: source.program_type_id,
        family: source.family,
        body_id: source.body_id,
        indexed: source.indexed,
        body_len: source.body_len,
        cursor_after: folded.cursor,
        slot_count: source.slot_count,
        events: eof.fields,
        score_entries_before: live.source.product.bridge.transcript.entries,
        score_entries_after: eof.score_entries_after,
        component: eof.component,
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_eof_from_live_source(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    eof: GeneratedMessageEofSource,
) -> Option<message::GenericMessageEofModelOutcome> {
    if !generated_message_live_frame_matches_fold(source, folded, live)
        || message::generic_message_source_fold(
            eof.fields, source.program_type_id, source.family,
            source.slot_count, source.body_len, source.indexed,
        ) != Some(folded)
        || folded.pending.is_some()
    {
        None
    } else {
        message::generic_message_eof_from_source(
            live.source.product.bridge,
            generated_message_eof_observation(source, folded, live, eof),
        )
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_scored_eof_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    eof: GeneratedMessageEofSource,
) -> Option<(integrated::LiveRuntimeProductState,
    integrated::LiveRuntimeProductAction)> {
    match generated_message_eof_from_live_source(source, folded, live, eof) {
        Some(message::GenericMessageEofModelOutcome::Scored { after, .. }) => {
            let next = send_live_score_after(live, after);
            if live.source.product.bridge.admission.frames.len() == 1
                && integrated::live_runtime_product_root_score_step(live, next)
            {
                Some((next, integrated::LiveRuntimeProductAction::RootScore))
            } else if live.source.product.bridge.admission.frames.len() > 1
                && integrated::live_runtime_product_message_child_score_step(live, next)
            {
                Some((next, integrated::LiveRuntimeProductAction::MessageChildScore))
            } else {
                None
            }
        },
        _ => None,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_scored_eof_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    eof: GeneratedMessageEofSource,
)
    requires generated_message_scored_eof_live_selected(
        source, folded, live, eof,
    ).is_some(),
    ensures {
        let (next, action) = generated_message_scored_eof_live_selected(
            source, folded, live, eof,
        ).unwrap();
        &&& integrated::live_runtime_product_step(live, next, action)
        &&& match generated_message_eof_from_live_source(
            source, folded, live, eof,
        ) {
            Some(message::GenericMessageEofModelOutcome::Scored {
                after, score_handle, semantic_complete, ..
            }) => {
                &&& next.source.product.bridge == after
                &&& next.source.product.bridge.transcript.entries
                    == eof.score_entries_after
                &&& after.admission.frames.last().score_handle
                    == Some(score_handle)
                &&& match eof.component {
                    Some(component) => component.complete == semantic_complete
                        && component.score_handle == score_handle,
                    None => true,
                }
            },
            _ => false,
        }
    },
{
    let (next, action) = generated_message_scored_eof_live_selected(
        source, folded, live, eof,
    ).unwrap();
    let observed = generated_message_eof_observation(source, folded, live, eof);
    reveal(message::generic_message_eof_from_source);
    reveal(message::generic_message_eof_source_matches_before);
    message::generic_message_eof_selected_result_exact(
        live.source.product.bridge, observed,
    );
    match action {
        integrated::LiveRuntimeProductAction::RootScore => {
            assert(integrated::live_runtime_product_root_score_step(live, next));
        },
        integrated::LiveRuntimeProductAction::MessageChildScore => {
            assert(integrated::live_runtime_product_message_child_score_step(live, next));
        },
        _ => assert(false),
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_noscore_eof_live_selected(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    eof: GeneratedMessageEofSource,
) -> Option<integrated::LiveRuntimeProductState> {
    match generated_message_eof_from_live_source(source, folded, live, eof) {
        Some(message::GenericMessageEofModelOutcome::NoScore { after, .. })
            if after == live.source.product.bridge
                && live.source.product.bridge.admission.frames.len() > 1
                && runtime_source::runtime_noscore_child_kind(
                    live.source.runtime,
                ).is_some() => match send_live_child_complete_selected(live) {
                    Some(next) if next.source.product.bridge.transcript.entries
                        == live.source.product.bridge.transcript.entries
                        && runtime_source::runtime_message_driver_step(
                        live.source.runtime,
                        runtime_source::RuntimeMessageEvent::Eof,
                    ) == Some(runtime_source::RuntimeMessageOutcome::ParentAccepted {
                        image: next.source.runtime,
                    }) => Some(next),
                    _ => None,
                },
        _ => None,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_noscore_eof_live_selected_exact(
    source: GeneratedMessageFieldSource,
    folded: message::GenericMessageSourceFold,
    live: integrated::LiveRuntimeProductState,
    eof: GeneratedMessageEofSource,
)
    requires generated_message_noscore_eof_live_selected(
        source, folded, live, eof,
    ).is_some(),
    ensures {
        let next = generated_message_noscore_eof_live_selected(
            source, folded, live, eof,
        ).unwrap();
        &&& next.source.product.bridge.transcript.entries
            == live.source.product.bridge.transcript.entries
        &&& integrated::live_runtime_product_step(
            live, next,
            integrated::LiveRuntimeProductAction::MessageChildComplete,
        )
    },
{
    send_live_child_complete_selected_dispatches(live);
}

#[cfg(rpb3_live_push)]
pub struct GeneratedMessageEventReplay {
    pub ghost events: Seq<message::GenericMessageSourceEvent>,
    pub ghost fields: message::GenericMessageSourceFold,
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_event_replay_initial()
    -> GeneratedMessageEventReplay {
    GeneratedMessageEventReplay {
        events: Seq::empty(),
        fields: message::generic_message_source_initial(),
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_event_replay_wf(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
) -> bool {
    message::generic_message_source_fold(
        replay.events, source.program_type_id, source.family,
        source.slot_count, source.body_len, source.indexed,
    ) == Some(replay.fields)
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_field_source_event(
    action: GeneratedMessageFieldAction,
) -> message::GenericMessageSourceEvent {
    match action {
        GeneratedMessageFieldAction::Scalar { receipt } =>
            message::GenericMessageSourceEvent::Scalar { field: receipt.field },
        GeneratedMessageFieldAction::Push { receipt } =>
            message::GenericMessageSourceEvent::Push {
                field: receipt.field, body_id: receipt.body_id,
            },
    }
}

#[cfg(rpb3_live_push)]
pub enum GeneratedMessageSourceOperation {
    Field { action: GeneratedMessageFieldAction },
    Accepted { returned: admission::AdmissionReturnedSummary },
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_source_step(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    operation: GeneratedMessageSourceOperation,
) -> Option<GeneratedMessageEventReplay> {
    if !generated_message_event_replay_wf(source, replay) {
        None
    } else {
        let selected = match operation {
            GeneratedMessageSourceOperation::Field { action } => match action {
                GeneratedMessageFieldAction::Scalar { receipt } => match
                    generated_message_scalar_source_step(source, replay.fields, receipt)
                {
                    Some(fields) => Some((
                        fields,
                        message::GenericMessageSourceEvent::Scalar {
                            field: receipt.field,
                        },
                    )),
                    None => None,
                },
                GeneratedMessageFieldAction::Push { receipt } => match
                    generated_message_push_source_step(source, replay.fields, receipt)
                {
                    Some(fields) => Some((
                        fields,
                        message::GenericMessageSourceEvent::Push {
                            field: receipt.field, body_id: receipt.body_id,
                        },
                    )),
                    None => None,
                },
            },
            GeneratedMessageSourceOperation::Accepted { returned } => {
                let event = message::GenericMessageSourceEvent::Accepted { returned };
                match message::generic_message_source_event_step(
                    replay.fields, event, source.program_type_id, source.family,
                    source.slot_count, source.body_len, source.indexed,
                ) {
                    Some(fields) => Some((fields, event)),
                    None => None,
                }
            },
        };
        match selected {
            Some((fields, event)) => {
                let next = GeneratedMessageEventReplay {
                    events: replay.events.push(event),
                    fields,
                };
                if generated_message_event_replay_wf(source, next) {
                    Some(next)
                } else {
                    None
                }
            },
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_from_source_operations(
    source: GeneratedMessageFieldSource,
    operations: Seq<GeneratedMessageSourceOperation>,
) -> Option<GeneratedMessageEventReplay>
    decreases operations.len(),
{
    if operations.len() == 0 {
        Some(generated_message_event_replay_initial())
    } else {
        match generated_message_replay_from_source_operations(
            source, operations.subrange(0, operations.len() - 1),
        ) {
            Some(replay) => generated_message_replay_source_step(
                source, replay, operations.last(),
            ),
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_replay_from_source_operations_exact(
    source: GeneratedMessageFieldSource,
    operations: Seq<GeneratedMessageSourceOperation>,
)
    requires generated_message_replay_from_source_operations(
        source, operations,
    ).is_some(),
    ensures {
        let replay = generated_message_replay_from_source_operations(
            source, operations,
        ).unwrap();
        &&& generated_message_event_replay_wf(source, replay)
        &&& replay.events.len() == operations.len()
        &&& (operations.len() == 0 ==> replay.events.len() == 0)
    },
    decreases operations.len(),
{
    if operations.len() > 0 {
        let prefix = operations.subrange(0, operations.len() - 1);
        generated_message_replay_from_source_operations_exact(source, prefix);
        let before = generated_message_replay_from_source_operations(
            source, prefix,
        ).unwrap();
        let after = generated_message_replay_from_source_operations(
            source, operations,
        ).unwrap();
        assert(after.events.len() == before.events.len() + 1);
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_field_selected(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    field_action: GeneratedMessageFieldAction,
) -> Option<(GeneratedMessageEventReplay,
    integrated::LiveRuntimeProductState,
    integrated::LiveRuntimeProductAction)> {
    if !generated_message_event_replay_wf(source, replay) {
        None
    } else {
        match generated_message_field_live_selected(
            source, replay.fields, live, field_action,
        ) {
            Some((next_fields, next_live, action)) => {
                let next_replay = GeneratedMessageEventReplay {
                    events: replay.events.push(
                        generated_message_field_source_event(field_action),
                    ),
                    fields: next_fields,
                };
                if generated_message_event_replay_wf(source, next_replay) {
                    Some((next_replay, next_live, action))
                } else {
                    None
                }
            },
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_replay_field_selected_exact(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    field_action: GeneratedMessageFieldAction,
)
    requires generated_message_replay_field_selected(
        source, replay, live, field_action,
    ).is_some(),
    ensures {
        let (next_replay, next_live, action) =
            generated_message_replay_field_selected(
                source, replay, live, field_action,
            ).unwrap();
        &&& generated_message_event_replay_wf(source, replay)
        &&& generated_message_event_replay_wf(source, next_replay)
        &&& next_replay.events == replay.events.push(
            generated_message_field_source_event(field_action),
        )
        &&& integrated::live_runtime_product_step(live, next_live, action)
        &&& match field_action {
            GeneratedMessageFieldAction::Scalar { receipt } => {
                &&& action == integrated::LiveRuntimeProductAction::ContinueIndexed
                &&& next_replay.fields.seen
                    == replay.fields.seen.insert(receipt.slot as int)
            },
            GeneratedMessageFieldAction::Push { receipt } => {
                &&& next_replay.fields.seen
                    == replay.fields.seen.insert(receipt.slot as int)
                &&& match action {
                    integrated::LiveRuntimeProductAction::MessagePush { .. } =>
                        receipt.request.child.frame_kind == 1
                            || receipt.request.child.frame_kind == 2,
                    integrated::LiveRuntimeProductAction::MessageEpmPush { .. } =>
                        receipt.request.child.frame_kind == 3,
                    _ => false,
                }
            },
        }
    },
{
    generated_message_field_live_selected_exact(
        source, replay.fields, live, field_action,
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_accept_selected(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
) -> Option<(GeneratedMessageEventReplay,
    integrated::LiveRuntimeProductState)> {
    if !generated_message_event_replay_wf(source, replay) {
        None
    } else {
        match generated_message_accept_live_selected(
            source, replay.fields, live, returned, score_entries_after,
        ) {
            Some((next_fields, next_live)) => {
                let next_replay = GeneratedMessageEventReplay {
                    events: replay.events.push(
                        message::GenericMessageSourceEvent::Accepted { returned },
                    ),
                    fields: next_fields,
                };
                if generated_message_event_replay_wf(source, next_replay) {
                    Some((next_replay, next_live))
                } else {
                    None
                }
            },
            None => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_replay_accept_selected_exact(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    returned: admission::AdmissionReturnedSummary,
    score_entries_after: Seq<score::ScoreEntry>,
)
    requires generated_message_replay_accept_selected(
        source, replay, live, returned, score_entries_after,
    ).is_some(),
    ensures {
        let (next_replay, next_live) =
            generated_message_replay_accept_selected(
                source, replay, live, returned, score_entries_after,
            ).unwrap();
        &&& generated_message_event_replay_wf(source, replay)
        &&& generated_message_event_replay_wf(source, next_replay)
        &&& next_replay.events == replay.events.push(
            message::GenericMessageSourceEvent::Accepted { returned },
        )
        &&& next_replay.fields.cursor == replay.fields.cursor
        &&& next_replay.fields.seen == replay.fields.seen
        &&& next_replay.fields.returns == replay.fields.returns.push(returned)
        &&& integrated::live_runtime_product_step(
            live, next_live,
            generated_message_child_finish_action(
                live.source.product.bridge.admission.frames.last().frame_kind,
            ),
        )
    },
{
    generated_message_accept_live_selected_exact(
        source, replay.fields, live, returned, score_entries_after,
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_live_step(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    operation: GeneratedMessageSourceOperation,
    score_entries_after: Seq<score::ScoreEntry>,
) -> Option<(GeneratedMessageEventReplay,
    integrated::LiveRuntimeProductState,
    integrated::LiveRuntimeProductAction)> {
    match operation {
        GeneratedMessageSourceOperation::Field { action } =>
            generated_message_replay_field_selected(source, replay, live, action),
        GeneratedMessageSourceOperation::Accepted { returned } => {
            if live.source.product.bridge.admission.frames.len() == 0 {
                None
            } else {
                match generated_message_replay_accept_selected(
                    source, replay, live, returned, score_entries_after,
                ) {
                    Some((next_replay, next_live)) => Some((
                        next_replay, next_live,
                        generated_message_child_finish_action(
                            live.source.product.bridge.admission.frames.last().frame_kind,
                        ),
                    )),
                    None => None,
                }
            }
        },
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_replay_live_step_is_exact(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    operation: GeneratedMessageSourceOperation,
    score_entries_after: Seq<score::ScoreEntry>,
)
    requires generated_message_replay_live_step(
        source, replay, live, operation, score_entries_after,
    ).is_some(),
    ensures {
        let (next_replay, next_live, action) = generated_message_replay_live_step(
            source, replay, live, operation, score_entries_after,
        ).unwrap();
        &&& generated_message_replay_source_step(source, replay, operation)
            == Some(next_replay)
        &&& next_replay.events.len() == replay.events.len() + 1
        &&& integrated::live_runtime_product_step(live, next_live, action)
        &&& match operation {
            GeneratedMessageSourceOperation::Field { .. } => true,
            GeneratedMessageSourceOperation::Accepted { .. } =>
                action == generated_message_child_finish_action(
                    live.source.product.bridge.admission.frames.last().frame_kind,
                ),
        }
    },
{
    match operation {
        GeneratedMessageSourceOperation::Field { action } => {
            generated_message_replay_field_selected_exact(source, replay, live, action);
        },
        GeneratedMessageSourceOperation::Accepted { returned } => {
            generated_message_replay_accept_selected_exact(
                source, replay, live, returned, score_entries_after,
            );
        },
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_scored_eof_selected(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
    component: Option<message::GenericComponentEofSource>,
) -> Option<(integrated::LiveRuntimeProductState,
    integrated::LiveRuntimeProductAction)> {
    if !generated_message_event_replay_wf(source, replay) {
        None
    } else {
        generated_message_scored_eof_live_selected(
            source, replay.fields, live,
            GeneratedMessageEofSource {
                fields: replay.events,
                score_entries_after,
                component,
            },
        )
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_replay_scored_eof_selected_exact(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
    component: Option<message::GenericComponentEofSource>,
)
    requires generated_message_replay_scored_eof_selected(
        source, replay, live, score_entries_after, component,
    ).is_some(),
    ensures {
        let (next, action) = generated_message_replay_scored_eof_selected(
            source, replay, live, score_entries_after, component,
        ).unwrap();
        &&& generated_message_event_replay_wf(source, replay)
        &&& integrated::live_runtime_product_step(live, next, action)
    },
{
    generated_message_scored_eof_live_selected_exact(
        source, replay.fields, live,
        GeneratedMessageEofSource {
            fields: replay.events,
            score_entries_after,
            component,
        },
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_replay_noscore_eof_selected(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
) -> Option<integrated::LiveRuntimeProductState> {
    if !generated_message_event_replay_wf(source, replay) {
        None
    } else {
        generated_message_noscore_eof_live_selected(
            source, replay.fields, live,
            GeneratedMessageEofSource {
                fields: replay.events,
                score_entries_after,
                component: None,
            },
        )
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_replay_noscore_eof_selected_exact(
    source: GeneratedMessageFieldSource,
    replay: GeneratedMessageEventReplay,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
)
    requires generated_message_replay_noscore_eof_selected(
        source, replay, live, score_entries_after,
    ).is_some(),
    ensures {
        let next = generated_message_replay_noscore_eof_selected(
            source, replay, live, score_entries_after,
        ).unwrap();
        &&& generated_message_event_replay_wf(source, replay)
        &&& integrated::live_runtime_product_step(
            live, next,
            integrated::LiveRuntimeProductAction::MessageChildComplete,
        )
    },
{
    generated_message_noscore_eof_live_selected_exact(
        source, replay.fields, live,
        GeneratedMessageEofSource {
            fields: replay.events,
            score_entries_after,
            component: None,
        },
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_source_operations_scored_eof_selected(
    source: GeneratedMessageFieldSource,
    operations: Seq<GeneratedMessageSourceOperation>,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
    component: Option<message::GenericComponentEofSource>,
) -> Option<(integrated::LiveRuntimeProductState,
    integrated::LiveRuntimeProductAction)> {
    match generated_message_replay_from_source_operations(source, operations) {
        Some(replay) => generated_message_replay_scored_eof_selected(
            source, replay, live, score_entries_after, component,
        ),
        None => None,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_source_operations_scored_eof_selected_exact(
    source: GeneratedMessageFieldSource,
    operations: Seq<GeneratedMessageSourceOperation>,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
    component: Option<message::GenericComponentEofSource>,
)
    requires generated_message_source_operations_scored_eof_selected(
        source, operations, live, score_entries_after, component,
    ).is_some(),
    ensures {
        let (next, action) =
            generated_message_source_operations_scored_eof_selected(
                source, operations, live, score_entries_after, component,
            ).unwrap();
        &&& generated_message_replay_from_source_operations(
            source, operations,
        ).is_some()
        &&& integrated::live_runtime_product_step(live, next, action)
    },
{
    let replay = generated_message_replay_from_source_operations(
        source, operations,
    ).unwrap();
    generated_message_replay_from_source_operations_exact(source, operations);
    generated_message_replay_scored_eof_selected_exact(
        source, replay, live, score_entries_after, component,
    );
}

#[cfg(rpb3_live_push)]
pub open spec fn generated_message_source_operations_noscore_eof_selected(
    source: GeneratedMessageFieldSource,
    operations: Seq<GeneratedMessageSourceOperation>,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
) -> Option<integrated::LiveRuntimeProductState> {
    match generated_message_replay_from_source_operations(source, operations) {
        Some(replay) => generated_message_replay_noscore_eof_selected(
            source, replay, live, score_entries_after,
        ),
        None => None,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn generated_message_source_operations_noscore_eof_selected_exact(
    source: GeneratedMessageFieldSource,
    operations: Seq<GeneratedMessageSourceOperation>,
    live: integrated::LiveRuntimeProductState,
    score_entries_after: Seq<score::ScoreEntry>,
)
    requires generated_message_source_operations_noscore_eof_selected(
        source, operations, live, score_entries_after,
    ).is_some(),
    ensures {
        let next = generated_message_source_operations_noscore_eof_selected(
            source, operations, live, score_entries_after,
        ).unwrap();
        &&& generated_message_replay_from_source_operations(
            source, operations,
        ).is_some()
        &&& integrated::live_runtime_product_step(
            live, next,
            integrated::LiveRuntimeProductAction::MessageChildComplete,
        )
    },
{
    let replay = generated_message_replay_from_source_operations(
        source, operations,
    ).unwrap();
    generated_message_replay_from_source_operations_exact(source, operations);
    generated_message_replay_noscore_eof_selected_exact(
        source, replay, live, score_entries_after,
    );
}

} // verus!
