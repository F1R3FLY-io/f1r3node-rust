//! Deterministic MapKey Continue composition above the checked key source.
//!
//! The wire read and key observation are supplied by the separate MapKey
//! source bridge. This file selects the admission, product, score/runtime,
//! and wire-cursor successors from existing model functions. It does not take
//! an arbitrary after-state. Extracting the pre-state and checked Rust
//! observations remains a distinct source-level obligation.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_map_frame_refinement as map,
    rpb3_map_key_source_correspondence as source, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as bridge, rpb3_wire_cursor_source_refinement as wire,
};

verus! {

pub open spec fn map_key_full_source_ready(
    before: integrated::LiveRuntimeProductState,
    observed: source::MapKeySourceObservation,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || before.cursors.len() == 0
        || before.source.product.map.map_frames.len() == 0
        || before.cursors.last().is_none()
        || before.source.wire.last().is_none()
        || before.source.product.map.map_frames.last().is_none()
        || before.source.product.bridge.admission.frames.last().frame_kind != 2
    { false } else {
        source::map_key_source_ready(
            before.source.product.map.map_frames.last().unwrap(),
            before.cursors.last().unwrap(),
            observed,
        )
    }
}

/// The full model action is selected by the actual indexed-field model, then
/// each containing layer is rebuilt from that admission result. This selector
/// may return None when the existing admission driver rejects the action; it
/// never manufactures a successful admission poststate.
pub open spec fn map_key_full_source_step(
    before: integrated::LiveRuntimeProductState,
    observed: source::MapKeySourceObservation,
) -> Option<integrated::LiveRuntimeProductState> {
    if !map_key_full_source_ready(before, observed) { None }
    else {
        let cursor = before.cursors.last().unwrap();
        let reader = before.source.wire.last().unwrap();
        match (wire::wire_runtime_next_field(cursor),
            wire::wire_source_next_field(reader)) {
            (
                Some(wire::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }),
                wire::WireSourceTransition::Field {
                    state: next_reader, field: source_field,
                },
            ) if field == source_field => {
                let map_before = before.source.product.map;
                match map::integrated_admission_driver_step(
                    map_before, map::IntegratedAdmissionAction::MapKey { field },
                ) {
                    Some(map::IntegratedAdmissionOutcome::MapKeyContinued {
                        state: next_map,
                    }) => {
                        let next_bridge = bridge::ScoreAdmissionBridge {
                            admission: next_map.admission,
                            transcript: next_map.transcript,
                        };
                        let provisional_product = integrated::ProductScoreState {
                            bridge: next_bridge,
                            map: next_map,
                            epm_frames: before.source.product.epm_frames,
                            indexed_regions: before.source.product.indexed_regions,
                            accepted_epm_sources:
                                before.source.product.accepted_epm_sources,
                        };
                        let next_product = integrated::ProductScoreState {
                            accepted_epm_sources:
                                integrated::product_epm_receipts_carried(
                                    before.source.product, provisional_product,
                                ),
                            ..provisional_product
                        };
                        let next_runtime = runtime::RuntimeFrameImage {
                            score: next_bridge,
                            ..before.source.runtime
                        };
                        let next_source = integrated::LiveSourceProductState {
                            product: next_product,
                            runtime: next_runtime,
                            arena: before.source.arena,
                            wire: before.source.wire.update(
                                before.source.wire.len() - 1, Some(next_reader),
                            ),
                        };
                        let next_cursor = integrated::runtime_wire_after_field(
                            cursor, concrete_after,
                        );
                        Some(integrated::LiveRuntimeProductState {
                            source: next_source,
                            cursors: before.cursors.update(
                                before.cursors.len() - 1, Some(next_cursor),
                            ),
                        })
                    },
                    _ => None,
                }
            },
            _ => None,
        }
    }
}

/// The selected integrated admission action uses the same MapFrame successor
/// as the lower source bridge and retains the complete previous transcript.
pub proof fn map_key_full_source_selects_admission(
    before: integrated::LiveRuntimeProductState,
    observed: source::MapKeySourceObservation,
)
    requires map_key_full_source_ready(before, observed),
    ensures match map_key_full_source_step(before, observed) {
        Some(after) => {
            let previous = before.source.product.map;
            let next = after.source.product.map;
            let cursor = before.cursors.last().unwrap();
            match wire::wire_runtime_next_field(cursor) {
                Some(wire::WireConcreteTransition::Field { field, .. }) => {
                    &&& map::integrated_admission_driver_step(
                        previous, map::IntegratedAdmissionAction::MapKey { field },
                    ) == Some(map::IntegratedAdmissionOutcome::MapKeyContinued {
                        state: next,
                    })
                    &&& source::map_key_source_step(
                        previous.map_frames.last().unwrap(), cursor, observed,
                    ) == next.map_frames.last()
                    &&& match map::map_admission_key_continue(
                        previous.map_frames.last().unwrap(),
                        previous.admission, field,
                    ) {
                        Some((next_frame, next_admission)) => {
                            &&& next.map_frames.last() == Some(next_frame)
                            &&& next.admission == next_admission
                        },
                        None => false,
                    }
                    &&& next.transcript == previous.transcript
                    &&& next.admission.arena == previous.admission.arena
                    &&& next.admission.frames.len() == previous.admission.frames.len()
                },
                _ => false,
            }
        },
        None => true,
    },
{
    let cursor = before.cursors.last().unwrap();
    let frame = before.source.product.map.map_frames.last().unwrap();
    source::map_key_source_derives_continue(frame, cursor, observed);
}

/// The computed full poststate, when enabled, satisfies the existing live
/// runtime MapKey action and preserves the stack, score arena and indexed
/// regions. No caller-provided after-state appears in this contract.
pub proof fn map_key_full_source_refines_live_continue(
    before: integrated::LiveRuntimeProductState,
    observed: source::MapKeySourceObservation,
)
    requires map_key_full_source_ready(before, observed),
    ensures match map_key_full_source_step(before, observed) {
        Some(after) => {
            &&& integrated::live_runtime_product_map_key_step(before, after)
            &&& integrated::live_runtime_product_wf(after)
            &&& after.source.product.bridge.admission.frames.len()
                == before.source.product.bridge.admission.frames.len()
            &&& after.source.product.bridge.admission.arena
                == before.source.product.bridge.admission.arena
            &&& after.source.product.bridge.transcript
                == before.source.product.bridge.transcript
            &&& after.source.arena == before.source.arena
            &&& after.source.product.indexed_regions
                == before.source.product.indexed_regions
            &&& after.source.runtime.capacity == before.source.runtime.capacity
            &&& after.source.runtime.frame_bytes == before.source.runtime.frame_bytes
        },
        None => true,
    },
{
    map_key_full_source_selects_admission(before, observed);
    if let Some(after) = map_key_full_source_step(before, observed) {
        integrated::live_runtime_product_map_key_preserves_wf(before, after);
    }
}

/// The remaining integrated enabledness premise is a predicate on the
/// deterministic admission result, not an offered poststate. A separate
/// lower-level proof can discharge `map_admission_key_continue` from the
/// checked map source and auxiliary pre-state projection.
pub open spec fn map_key_integrated_result_ready(
    before: integrated::LiveRuntimeProductState,
    observed: source::MapKeySourceObservation,
) -> bool {
    if !map_key_full_source_ready(before, observed) { false }
    else {
        let cursor = before.cursors.last().unwrap();
        let previous = before.source.product.map;
        let frame = previous.map_frames.last().unwrap();
        match wire::wire_runtime_next_field(cursor) {
            Some(wire::WireConcreteTransition::Field { field, .. }) => {
                match map::map_admission_key_continue(
                    frame, previous.admission, field,
                ) {
                    Some((next_frame, next_admission)) =>
                        map::integrated_admission_result(
                            next_admission,
                            previous.map_frames.update(
                                previous.map_frames.len() - 1, Some(next_frame),
                            ),
                            previous.transcript,
                        ).is_some(),
                    None => false,
                }
            },
            _ => false,
        }
    }
}

/// The two remaining local admission guards after the checked source field:
/// the auxiliary Continue selector must be enabled, and its computed top
/// frame must agree with the computed MapFrame. Neither successor is offered
/// by the caller.
pub open spec fn map_key_admission_guards_ready(
    frame: map::MapFrameModel,
    state: admission::AdmissionState,
    cursor: wire::WireRuntimeCursorImage,
    observed: source::MapKeySourceObservation,
) -> bool {
    if !admission::admission_state_wf(state)
        || state.frames.len() == 0
        || !map::map_aux_projection(frame, state.frames.last())
        || !source::map_key_source_ready(frame, cursor, observed)
    { false } else {
        match wire::wire_runtime_next_field(cursor) {
            Some(wire::WireConcreteTransition::Field { field, .. }) => {
                match (source::map_key_source_step(frame, cursor, observed),
                    admission::admission_continue_step(
                        state, field.encoded_end, 0, false, 1,
                    )) {
                    (Some(next_frame), Some(next_state)) =>
                        map::map_aux_projection(
                            next_frame, next_state.frames.last(),
                        ),
                    _ => false,
                }
            },
            _ => false,
        }
    }
}

pub proof fn map_key_source_enables_admission(
    frame: map::MapFrameModel,
    state: admission::AdmissionState,
    cursor: wire::WireRuntimeCursorImage,
    observed: source::MapKeySourceObservation,
)
    requires map_key_admission_guards_ready(frame, state, cursor, observed),
    ensures match wire::wire_runtime_next_field(cursor) {
        Some(wire::WireConcreteTransition::Field { field, .. }) =>
            map::map_admission_key_continue(frame, state, field).is_some(),
        _ => false,
    },
{
    source::map_key_source_derives_continue(frame, cursor, observed);
}

/// This is a genuinely successful full Continue branch: the premise only
/// checks enabledness of the deterministic model transition. The poststate
/// itself is computed by `map_key_full_source_step`.
pub proof fn map_key_full_source_enabled(
    before: integrated::LiveRuntimeProductState,
    observed: source::MapKeySourceObservation,
)
    requires map_key_integrated_result_ready(before, observed),
    ensures map_key_full_source_step(before, observed).is_some(),
{
    let cursor = before.cursors.last().unwrap();
    wire::wire_runtime_next_field_from_source(cursor);
}

} // verus!
