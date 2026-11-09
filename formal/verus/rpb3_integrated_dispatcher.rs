//! One dispatcher relation over the integrated live admission product.
//!
//! `integrated_step` keeps three action families exclusive through its action
//! constructor and through guards keyed on the top frame kind: Live product
//! actions (Message, Map, and scalar EPathMap frames), CanonicalPath driver
//! actions (kind-4 frames and the pre-pop child of a kind-4 parent), and ACT
//! Forward and Unwind batches (a kind-3 top frame). No integrated action
//! carries a successor state. `integrated_step_is_function` proves that a
//! fixed `(before, action)` selects at most one successor. The observed steps
//! of `MessagePush` and `MapValuePush` are plain-or-indexed disjunctions whose
//! sides have different successors, so the action names the side it takes in
//! an explicit route.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_act_source_product_integration as act, rpb3_admission_cursor as admission,
    rpb3_canonical_path_common_product as common,
    rpb3_canonical_path_driver_product_integration as path_driver,
    rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as epm_terminal,
    rpb3_epm_active_line_work_source as active, rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_batch_cursor_source_extraction as act_source,
    rpb3_epm_push_live_action_refinement as push_live,
    rpb3_epm_push_source_action_refinement as push_source,
    rpb3_epm_push_transport_source_refinement as transport,
    rpb3_epm_restore_run_expansion as restore, rpb3_epm_unwind_event_lift as unwind,
    rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_terminal_refinement as terminal,
    rpb3_map_frame_refinement as map, rpb3_path_parser_refinement as parser,
    rpb3_runtime_frame_source_refinement as runtime, rpb3_score_admission_bridge as score_bridge,
    rpb3_score_arena_source_refinement as arena, rpb3_score_transcript as score,
    rpb3_wire_cursor_source_refinement as wire, LiveRuntimeProductAction,
    LiveRuntimeProductState, LiveSourceProductState, ProductScoreState,
};

verus! {

/// `route` selects which side of the plain-or-indexed disjunction of the
/// observed MessagePush and MapValuePush steps the integrated action takes. It
/// is an observation of the Rust push variant, not a successor.
pub enum IntegratedPushRoute {
    Plain,
    Indexed,
}

pub enum IntegratedAction {
    Live {
        live_action: LiveRuntimeProductAction,
        route: IntegratedPushRoute,
    },
    /// `driver_before` is the retained driver image the CanonicalPath arm is
    /// fed. `integrated_path_case` validates it against the current product;
    /// the poststate is the driver step's own output, never a supplied image.
    CanonicalPath {
        driver_before: path_driver::CanonicalPathDriverProductState,
        driver_action: path_driver::CanonicalPathDriverSourceAction,
    },
    ActForward {
        plan_source: act_source::AcceptedActPlanSourceImage,
        work: active::ActiveLineWorkSource,
        width: usize,
        commit: act_source::AcceptedLineCommitSource,
        continuation: Seq<epm::ActWorkItem>,
        fuel: nat,
        pending: epm::PathObligation,
        state_id: usize,
        latest_restore_state_id: Option<usize>,
        forward_facts: restore::ForwardCapacityFacts,
    },
    ActUnwind {
        run: batch::HistoricalRestoreRunImage,
        prefix: Seq<epm::PathCheckpoint>,
        start: usize,
        width: usize,
        unwind_facts: restore::UnwindCapacityFacts,
    },
}

/// A completed root has no live post-frame. Its image names the state that
/// produced it, so that `integrated_accept_refines_root` can state the exact
/// root facts over the accepted trace.
pub enum IntegratedAccepted {
    LiveRoot {
        post_root_score: LiveRuntimeProductState,
        live_returned: runtime::RuntimeRootReturnImage,
    },
    CanonicalPathRoot {
        pre_complete: LiveRuntimeProductState,
        path_returned: terminal::LiveCanonicalPathTerminalOutcome,
    },
}

pub enum IntegratedState {
    Active { product: LiveRuntimeProductState },
    Accepted { accepted: IntegratedAccepted },
}

pub open spec fn integrated_accepted_wf(accepted: IntegratedAccepted) -> bool {
    match accepted {
        IntegratedAccepted::LiveRoot { post_root_score, .. } =>
            integrated::live_runtime_product_wf(post_root_score),
        IntegratedAccepted::CanonicalPathRoot { pre_complete, .. } =>
            integrated::live_runtime_product_wf(pre_complete),
    }
}

pub open spec fn integrated_wf(state: IntegratedState) -> bool {
    match state {
        IntegratedState::Active { product } => integrated::live_runtime_product_wf(product),
        IntegratedState::Accepted { accepted } => integrated_accepted_wf(accepted),
    }
}

pub open spec fn integrated_top_frame_kind(state: LiveRuntimeProductState) -> u8 {
    state.source.product.bridge.admission.frames.last().frame_kind
}

/// `frames[len-2]` is the CanonicalPath parent of the pre-pop child that a
/// `ChildReturn` driver action consumes.
pub open spec fn integrated_parent_is_canonical_path(
    state: LiveRuntimeProductState,
) -> bool {
    let frames = state.source.product.bridge.admission.frames;
    frames.len() >= 2 && frames[frames.len() - 2].frame_kind == 4
}

/// The top frame kind each Live action runs on: Message frames are kind 1, Map
/// frames kind 2, and EPathMap frames kind 3.
pub open spec fn integrated_live_required_top_kind(action: LiveRuntimeProductAction) -> u8 {
    match action {
        LiveRuntimeProductAction::Continue
        | LiveRuntimeProductAction::ContinueIndexed
        | LiveRuntimeProductAction::MessagePush { .. }
        | LiveRuntimeProductAction::MessageEpmPush { .. }
        | LiveRuntimeProductAction::RootScore
        | LiveRuntimeProductAction::RootComplete { .. }
        | LiveRuntimeProductAction::MessageChildScore
        | LiveRuntimeProductAction::MessageChildComplete
        | LiveRuntimeProductAction::MapValueAccept
        | LiveRuntimeProductAction::EpmMapValueAccept { .. } => 1,
        LiveRuntimeProductAction::MapKey
        | LiveRuntimeProductAction::MapValuePush { .. }
        | LiveRuntimeProductAction::MapEntryFinish => 2,
        LiveRuntimeProductAction::EpmContinue { .. }
        | LiveRuntimeProductAction::EpmPush { .. }
        | LiveRuntimeProductAction::EpmEnd { .. }
        | LiveRuntimeProductAction::EpmFinishAccept => 3,
    }
}

/// The top frame kind an integrated action is keyed on. `ChildReturn` has none:
/// its guard reads the CanonicalPath parent under the child that just finished.
pub open spec fn integrated_action_required_top_kind(action: IntegratedAction) -> Option<u8> {
    match action {
        IntegratedAction::Live { live_action: live, .. } =>
            Some(integrated_live_required_top_kind(live)),
        IntegratedAction::CanonicalPath { driver_action: path, .. } => match path {
            path_driver::CanonicalPathDriverSourceAction::ChildReturn { .. } => None,
            _ => Some(4u8),
        },
        IntegratedAction::ActForward { .. } | IntegratedAction::ActUnwind { .. } => Some(3u8),
    }
}

/// The guard keyed on the top frame kind. Each action family runs only on its
/// frames: Live on Message/Map/EPathMap tops (per action), CanonicalPath on a
/// kind-4 top or the pre-pop child of a kind-4 parent, and ACT on a kind-3 top.
pub open spec fn integrated_kind_guard(
    before: LiveRuntimeProductState, action: IntegratedAction,
) -> bool {
    &&& before.source.product.bridge.admission.frames.len() > 0
    &&& match integrated_action_required_top_kind(action) {
        Some(required) => integrated_top_frame_kind(before) == required,
        None => integrated_parent_is_canonical_path(before),
    }
}

/// The observed MessagePush step with the plain or indexed arm selected by
/// `route` instead of the disjunction. `integrated_message_push_route_implies_observed`
/// connects it back to the source-observed relation.
pub open spec fn integrated_message_push_route_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
    route: IntegratedPushRoute,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
    {
        false
    } else {
        match runtime::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
            runtime::SourceFramePushObservationResult::Success { storage } => match route {
                IntegratedPushRoute::Plain =>
                    integrated::live_runtime_product_message_push_step(
                        before, after, request, storage,
                    ),
                IntegratedPushRoute::Indexed =>
                    integrated::live_runtime_product_message_push_indexed_step(
                        before, after, request, storage,
                    ),
            },
            _ => false,
        }
    }
}

pub open spec fn integrated_map_value_push_route_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
    route: IntegratedPushRoute,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
    {
        false
    } else {
        match runtime::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
            runtime::SourceFramePushObservationResult::Success { storage } => match route {
                IntegratedPushRoute::Plain =>
                    integrated::live_runtime_product_map_value_push_step(
                        before, after, request, storage,
                    ),
                IntegratedPushRoute::Indexed =>
                    integrated::live_runtime_product_map_value_push_indexed_step(
                        before, after, request, storage,
                    ),
            },
            _ => false,
        }
    }
}

/// `live_runtime_product_step` with the two relational push arms replaced by
/// their route-selected refinements. Every other arm is the existing relation.
pub open spec fn integrated_live_relation(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
) -> bool {
    match action {
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => integrated_message_push_route_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes, route,
        ),
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => integrated_map_value_push_route_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes, route,
        ),
        _ => integrated::live_runtime_product_step(before, after, action),
    }
}

pub open spec fn integrated_live_case(
    before: LiveRuntimeProductState,
    after: IntegratedState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
) -> bool {
    match action {
        LiveRuntimeProductAction::RootComplete { returned } => match after {
            IntegratedState::Accepted {
                accepted: IntegratedAccepted::LiveRoot { post_root_score, live_returned: accepted_returned },
            } => accepted_returned == returned
                && integrated::live_runtime_product_step(before, post_root_score, action),
            _ => false,
        },
        _ => match after {
            IntegratedState::Active { product: next } =>
                integrated_live_relation(before, next, action, route),
            _ => false,
        },
    }
}

/// The integrated state a driver outcome selects. A non-terminal outcome keeps
/// running on the retained product; the root Complete outcome is accepted.
pub open spec fn integrated_path_outcome_state(
    before: LiveRuntimeProductState,
    outcome: path_driver::CanonicalPathDriverProductOutcome,
) -> IntegratedState {
    match outcome {
        path_driver::CanonicalPathDriverProductOutcome::Active { state } =>
            IntegratedState::Active { product: state.product },
        path_driver::CanonicalPathDriverProductOutcome::Terminal { returned } =>
            IntegratedState::Accepted {
                accepted: IntegratedAccepted::CanonicalPathRoot {
                    pre_complete: before, path_returned: returned,
                },
            },
    }
}

pub open spec fn integrated_path_case(
    before: LiveRuntimeProductState,
    after: IntegratedState,
    driver_before: path_driver::CanonicalPathDriverProductState,
    action: path_driver::CanonicalPathDriverSourceAction,
) -> bool {
    &&& driver_before.product == before
    &&& path_driver::canonical_path_driver_product_wf(driver_before)
    &&& match path_driver::canonical_path_driver_product_step(driver_before, action) {
        Some(outcome) => after == integrated_path_outcome_state(before, outcome),
        None => false,
    }
}

/// The admission half of an internal EPM step: the scalar `linked_epm_continue`
/// sets the top frame's byte cursor to the post-step value-table offset.
pub open spec fn integrated_epm_cursor_admission(
    state: admission::AdmissionState, consumed: usize,
) -> admission::AdmissionState
    recommends state.frames.len() > 0,
{
    admission::AdmissionState {
        frames: state.frames.update(
            state.frames.len() - 1,
            epm::epm_advanced_admission_cursor(state.frames.last(), consumed),
        ),
        arena: state.arena,
        root_end: state.root_end,
        pushed: state.pushed,
        completed: state.completed,
        next_obligation_id: state.next_obligation_id,
    }
}

pub open spec fn integrated_act_forward_frame(
    before: ProductScoreState,
    width: usize,
    commit: act_source::AcceptedLineCommitSource,
    pending: epm::PathObligation,
    state_id: usize,
) -> epm::EpmFrame {
    batch::forward_batch_after(
        act::active_product_epm(before).unwrap(), commit.start, width, pending, state_id,
    )
}

pub open spec fn integrated_act_forward_product(
    before: ProductScoreState, frame: epm::EpmFrame,
) -> ProductScoreState
    recommends before.epm_frames.len() > 0, before.bridge.admission.frames.len() > 0,
{
    unwind_product::product_after_internal_epm_continue(before, epm::LinkedEpmState {
        epm: frame,
        admission: integrated_epm_cursor_admission(
            before.bridge.admission, frame.cursor.value_table_offset,
        ),
        indexed_regions: before.indexed_regions,
    })
}

pub open spec fn integrated_act_forward_after(
    before: LiveRuntimeProductState,
    width: usize,
    commit: act_source::AcceptedLineCommitSource,
    pending: epm::PathObligation,
    state_id: usize,
) -> LiveRuntimeProductState {
    let frame = integrated_act_forward_frame(
        before.source.product, width, commit, pending, state_id,
    );
    let product = integrated_act_forward_product(before.source.product, frame);
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime::runtime_after_admission_step(
                before.source.runtime, product.bridge.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire,
        },
        cursors: before.cursors,
    }
}

/// Forward batches are guarded by the ACT source facts plus the ordinary
/// internal-EPM conditions that `linked_epm_continue` also demands.
pub open spec fn integrated_act_forward_guard(
    before: LiveRuntimeProductState,
    plan_source: act_source::AcceptedActPlanSourceImage,
    work: active::ActiveLineWorkSource,
    width: usize,
    commit: act_source::AcceptedLineCommitSource,
    continuation: Seq<epm::ActWorkItem>,
    fuel: nat,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
) -> bool {
    let frames = before.source.product.bridge.admission.frames;
    &&& integrated::live_runtime_product_wf(before)
    &&& act::accepted_product_forward_source(
        before.source.product, plan_source, work, width, commit, continuation, fuel,
        pending, state_id, latest_restore_state_id, facts,
    )
    &&& frames.last().waiting_id.is_none()
    &&& frames.last().cursor
        <= act::active_product_epm(before.source.product).unwrap().cursor.value_table_offset
}

pub open spec fn integrated_act_unwind_source(
    before: LiveRuntimeProductState,
) -> epm::LinkedEpmState
    recommends integrated::product_linked_top(before.source.product).is_some(),
{
    integrated::product_linked_top(before.source.product).unwrap()
}

pub open spec fn integrated_act_unwind_guard(
    before: LiveRuntimeProductState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    &&& integrated::product_linked_top(before.source.product).is_some()
    &&& unwind_product::live_runtime_unwind_source_guard(
        before, integrated_act_unwind_source(before), run, prefix, start, width, facts,
    )
}

pub open spec fn integrated_act_unwind_after(
    before: LiveRuntimeProductState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    width: usize,
) -> LiveRuntimeProductState {
    unwind_product::live_runtime_unwind_virtual_state(
        before, integrated_act_unwind_source(before), run, prefix, width,
    )
}

pub open spec fn integrated_step(
    before: IntegratedState,
    after: IntegratedState,
    action: IntegratedAction,
) -> bool {
    match before {
        IntegratedState::Accepted { .. } => false,
        IntegratedState::Active { product } => {
            &&& integrated_kind_guard(product, action)
            &&& match action {
                IntegratedAction::Live { live_action: live, route } =>
                    integrated_live_case(product, after, live, route),
                IntegratedAction::CanonicalPath { driver_before, driver_action: path } =>
                    integrated_path_case(product, after, driver_before, path),
                IntegratedAction::ActForward {
                    plan_source, work, width, commit, continuation, fuel, pending,
                    state_id, latest_restore_state_id, forward_facts: facts,
                } => {
                    &&& integrated_act_forward_guard(
                        product, plan_source, work, width, commit, continuation, fuel,
                        pending, state_id, latest_restore_state_id, facts,
                    )
                    &&& after == IntegratedState::Active {
                        product: integrated_act_forward_after(
                            product, width, commit, pending, state_id,
                        ),
                    }
                },
                IntegratedAction::ActUnwind { run, prefix, start, width, unwind_facts: facts } => {
                    &&& integrated_act_unwind_guard(product, run, prefix, start, width, facts)
                    &&& after == IntegratedState::Active {
                        product: integrated_act_unwind_after(product, run, prefix, width),
                    }
                },
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Uniqueness of the Live arms
// ---------------------------------------------------------------------------

pub proof fn product_epm_receipts_carried_congruent(
    before: ProductScoreState, left: ProductScoreState, right: ProductScoreState,
)
    requires
        left.bridge.admission.frames == right.bridge.admission.frames,
        left.indexed_regions == right.indexed_regions,
    ensures
        integrated::product_epm_receipts_carried(before, left)
            == integrated::product_epm_receipts_carried(before, right),
{
    let carried_left = integrated::product_epm_receipts_carried(before, left);
    let carried_right = integrated::product_epm_receipts_carried(before, right);
    assert(carried_left.len() == carried_right.len());
    assert forall|i: int| 0 <= i < carried_left.len()
        implies carried_left[i] == carried_right[i] by {}
    assert(carried_left =~= carried_right);
}

pub proof fn product_map_step_is_functional(
    before: ProductScoreState,
    left: ProductScoreState,
    right: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        integrated::product_map_step(before, left, action),
        integrated::product_map_step(before, right, action),
    ensures left == right,
{
    assert(left.map == right.map);
    assert(left.bridge.admission == right.bridge.admission);
    assert(left.bridge.transcript == right.bridge.transcript);
    assert(left.bridge == right.bridge);
    assert(left.indexed_regions == right.indexed_regions);
    assert(left.epm_frames == right.epm_frames);
    product_epm_receipts_carried_congruent(before, left, right);
    assert(left.accepted_epm_sources == right.accepted_epm_sources);
    assert(left == right);
}

pub proof fn live_source_continue_is_functional(
    before: LiveSourceProductState,
    left: LiveSourceProductState,
    right: LiveSourceProductState,
)
    requires
        integrated::live_source_product_continue_step(before, left),
        integrated::live_source_product_continue_step(before, right),
    ensures left == right,
{
    let reader = before.wire.last().unwrap();
    if let wire::WireSourceTransition::Field { state: next_reader, field } =
        wire::wire_source_next_field(reader)
    {
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Continue { field },
        };
        product_map_step_is_functional(before.product, left.product, right.product, action);
        assert(left.runtime == right.runtime);
        assert(left.wire == right.wire);
        assert(left == right);
    }
}

pub proof fn live_source_continue_indexed_is_functional(
    before: LiveSourceProductState,
    left: LiveSourceProductState,
    right: LiveSourceProductState,
)
    requires
        integrated::live_source_product_continue_indexed_step(before, left),
        integrated::live_source_product_continue_indexed_step(before, right),
    ensures left == right,
{
    let reader = before.wire.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.product.indexed_regions.last(),
    );
    if let wire::WireSourceTransition::Field { state: next_reader, field } =
        wire::wire_source_next_field(reader)
    {
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::ContinueIndexed { field, parent_source },
        };
        product_map_step_is_functional(before.product, left.product, right.product, action);
        assert(left.runtime == right.runtime);
        assert(left.wire == right.wire);
        assert(left == right);
    }
}

pub proof fn live_source_map_key_is_functional(
    before: LiveSourceProductState,
    left: LiveSourceProductState,
    right: LiveSourceProductState,
)
    requires
        integrated::live_source_product_map_key_step(before, left),
        integrated::live_source_product_map_key_step(before, right),
    ensures left == right,
{
    let reader = before.wire.last().unwrap();
    if let wire::WireSourceTransition::Field { state: next_reader, field } =
        wire::wire_source_next_field(reader)
    {
        let action = map::IntegratedAdmissionAction::MapKey { field };
        product_map_step_is_functional(before.product, left.product, right.product, action);
        assert(left.runtime == right.runtime);
        assert(left.wire == right.wire);
        assert(left == right);
    }
}

pub proof fn live_runtime_continue_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_continue_step(before, left),
        integrated::live_runtime_product_continue_step(before, right),
    ensures left == right,
{
    live_source_continue_is_functional(before.source, left.source, right.source);
    assert(left.cursors == right.cursors);
}

pub proof fn live_runtime_continue_indexed_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_continue_indexed_step(before, left),
        integrated::live_runtime_product_continue_indexed_step(before, right),
    ensures left == right,
{
    live_source_continue_indexed_is_functional(before.source, left.source, right.source);
    assert(left.cursors == right.cursors);
}

pub proof fn live_runtime_map_key_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_key_step(before, left),
        integrated::live_runtime_product_map_key_step(before, right),
    ensures left == right,
{
    live_source_map_key_is_functional(before.source, left.source, right.source);
    assert(left.cursors == right.cursors);
}

/// Common shape of the source-selected score finish successors: the bridge is
/// fixed by `bridge_score_finish_selected`, and every other component is an
/// equation over the bridge or the pre-state.
pub open spec fn integrated_score_finish_shape(
    before: LiveRuntimeProductState, after: LiveRuntimeProductState,
) -> bool {
    &&& score_bridge::bridge_score_finish_selected(before.source.product.bridge)
        == Some(after.source.product.bridge)
    &&& after.source.product.map.admission == after.source.product.bridge.admission
    &&& after.source.product.map.transcript == after.source.product.bridge.transcript
    &&& after.source.product.map.map_frames == before.source.product.map.map_frames
    &&& after.source.product.epm_frames == before.source.product.epm_frames
    &&& after.source.product.indexed_regions == before.source.product.indexed_regions
    &&& after.source.product.accepted_epm_sources
        == integrated::product_epm_receipts_carried(before.source.product, after.source.product)
    &&& after.source.runtime == runtime::RuntimeFrameImage {
        score: after.source.product.bridge,
        ..before.source.runtime
    }
    &&& after.source.arena == integrated::product_runtime_arena_from_score(
        after.source.product.bridge.transcript,
    )
    &&& after.source.wire == before.source.wire
    &&& after.cursors == before.cursors
}

pub proof fn integrated_score_finish_shape_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated_score_finish_shape(before, left),
        integrated_score_finish_shape(before, right),
    ensures left == right,
{
    let left_product = left.source.product;
    let right_product = right.source.product;
    assert(left_product.bridge == right_product.bridge);
    assert(left_product.map == right_product.map);
    product_epm_receipts_carried_congruent(before.source.product, left_product, right_product);
    assert(left_product == right_product);
    assert(left.source.runtime == right.source.runtime);
    assert(left.source == right.source);
    assert(left == right);
}

pub proof fn live_runtime_root_score_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_root_score_step(before, left),
        integrated::live_runtime_product_root_score_step(before, right),
    ensures left == right,
{
    assert(integrated_score_finish_shape(before, left));
    assert(integrated_score_finish_shape(before, right));
    integrated_score_finish_shape_is_functional(before, left, right);
}

pub proof fn live_runtime_message_child_score_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_message_child_score_step(before, left),
        integrated::live_runtime_product_message_child_score_step(before, right),
    ensures left == right,
{
    assert(integrated_score_finish_shape(before, left));
    assert(integrated_score_finish_shape(before, right));
    integrated_score_finish_shape_is_functional(before, left, right);
}

/// The common pop shape of the End-of-frame steps: the map reducer fixes the
/// product, and the runtime, wire, and cursor stacks are fixed equations.
pub open spec fn integrated_map_pop_shape(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: map::IntegratedAdmissionAction,
) -> bool {
    &&& integrated::product_map_step(before.source.product, after.source.product, action)
    &&& after.source.runtime == runtime::runtime_after_admission_step(
        before.source.runtime, after.source.product.bridge.admission,
    )
    &&& after.source.arena == before.source.arena
    &&& after.source.wire == before.source.wire.drop_last()
    &&& after.cursors == before.cursors.drop_last()
}

pub proof fn integrated_map_pop_shape_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    action: map::IntegratedAdmissionAction,
)
    requires
        integrated_map_pop_shape(before, left, action),
        integrated_map_pop_shape(before, right, action),
    ensures left == right,
{
    product_map_step_is_functional(
        before.source.product, left.source.product, right.source.product, action,
    );
    assert(left.source.runtime == right.source.runtime);
    assert(left.source == right.source);
    assert(left == right);
}

pub proof fn live_runtime_message_child_complete_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_message_child_complete_step(before, left),
        integrated::live_runtime_product_message_child_complete_step(before, right),
    ensures left == right,
{
    let action = map::IntegratedAdmissionAction::Message {
        action: admission::ConcreteFrameActionModel::Complete,
    };
    assert(integrated_map_pop_shape(before, left, action));
    assert(integrated_map_pop_shape(before, right, action));
    integrated_map_pop_shape_is_functional(before, left, right, action);
}

pub proof fn live_runtime_map_value_accept_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_value_accept_step(before, left),
        integrated::live_runtime_product_map_value_accept_step(before, right),
    ensures left == right,
{
    let action = map::IntegratedAdmissionAction::MapAccept;
    assert(integrated_map_pop_shape(before, left, action));
    assert(integrated_map_pop_shape(before, right, action));
    integrated_map_pop_shape_is_functional(before, left, right, action);
}

pub proof fn live_runtime_map_entry_finish_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_entry_finish_step(before, left),
        integrated::live_runtime_product_map_entry_finish_step(before, right),
    ensures left == right,
{
    reveal(integrated::live_runtime_product_map_entry_finish_step);
    let action = map::IntegratedAdmissionAction::MapFinish;
    assert(integrated_map_pop_shape(before, left, action));
    assert(integrated_map_pop_shape(before, right, action));
    integrated_map_pop_shape_is_functional(before, left, right, action);
}

// Uniqueness of the push steps. The reader projections and the map reducer are
// functions of the pre-state, so each successor component is a fixed equation.

pub proof fn live_runtime_message_push_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_message_push_step(before, left, request, storage),
        integrated::live_runtime_product_message_push_step(before, right, request, storage),
    ensures left == right,
{
    let parent = before.cursors.last().unwrap();
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Push {
                field,
                child: request.child,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        };
        product_map_step_is_functional(
            before.source.product, left.source.product, right.source.product, action,
        );
        assert(left.source.runtime == right.source.runtime);
        assert(left.source.wire == right.source.wire);
        assert(left.cursors == right.cursors);
        assert(left.source == right.source);
        assert(left == right);
    }
}

pub proof fn live_runtime_message_push_indexed_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_message_push_indexed_step(before, left, request, storage),
        integrated::live_runtime_product_message_push_indexed_step(before, right, request, storage),
    ensures left == right,
{
    let parent = before.cursors.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::PushIndexed {
                field,
                child: request.child,
                parent_source,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        };
        product_map_step_is_functional(
            before.source.product, left.source.product, right.source.product, action,
        );
        assert(left.source.runtime == right.source.runtime);
        assert(left.source.wire == right.source.wire);
        assert(left.cursors == right.cursors);
        assert(left.source == right.source);
        assert(left == right);
    }
}

pub proof fn live_runtime_message_epm_push_indexed_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_message_epm_push_indexed_step(before, left, request, storage),
        integrated::live_runtime_product_message_epm_push_indexed_step(before, right, request, storage),
    ensures left == right,
{
    let parent = before.cursors.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::PushIndexed {
                field,
                child: request.child,
                parent_source,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        };
        product_map_step_is_functional(
            before.source.product, left.source.product, right.source.product, action,
        );
        assert(left.source.runtime == right.source.runtime);
        assert(left.source.wire == right.source.wire);
        assert(left.cursors == right.cursors);
        assert(left.source == right.source);
        assert(left == right);
    }
}

pub proof fn live_runtime_map_value_push_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_map_value_push_step(before, left, request, storage),
        integrated::live_runtime_product_map_value_push_step(before, right, request, storage),
    ensures left == right,
{
    let parent = before.cursors.last().unwrap();
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        let action = map::IntegratedAdmissionAction::MapValue {
            field,
            child: request.child,
            frame_bytes: before.source.runtime.frame_bytes,
        };
        product_map_step_is_functional(
            before.source.product, left.source.product, right.source.product, action,
        );
        assert(left.source.runtime == right.source.runtime);
        assert(left.source.wire == right.source.wire);
        assert(left.cursors == right.cursors);
        assert(left.source == right.source);
        assert(left == right);
    }
}

pub proof fn live_runtime_map_value_push_indexed_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_map_value_push_indexed_step(before, left, request, storage),
        integrated::live_runtime_product_map_value_push_indexed_step(before, right, request, storage),
    ensures left == right,
{
    let parent = before.cursors.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        let action = map::IntegratedAdmissionAction::MapValueIndexed {
            field,
            child: request.child,
            parent_source,
            frame_bytes: before.source.runtime.frame_bytes,
        };
        product_map_step_is_functional(
            before.source.product, left.source.product, right.source.product, action,
        );
        assert(left.source.runtime == right.source.runtime);
        assert(left.source.wire == right.source.wire);
        assert(left.cursors == right.cursors);
        assert(left.source == right.source);
        assert(left == right);
    }
}

// ---------------------------------------------------------------------------
// Uniqueness of the EPathMap Live arms
// ---------------------------------------------------------------------------

/// Under `EpmAction::Continue` no wait, no Push and no Complete is possible, so
/// `event_step` fixes every field of the successor frame.
pub proof fn epm_continue_event_step_is_functional(
    before: epm::EpmFrame,
    left: epm::EpmFrame,
    right: epm::EpmFrame,
    event: epm::EpmEvent,
    next_id: u64,
)
    requires
        epm::event_step(before, left, event, next_id, epm::EpmAction::Continue),
        epm::event_step(before, right, event, next_id, epm::EpmAction::Continue),
    ensures left == right,
{
    match event {
        epm::EpmEvent::Edge { source_offset, origin, effect } => {
            let byte = epm::edge_byte(before, source_offset, origin);
            let source = epm::trie_position(before, source_offset);
            let step = parser::feed_parser_byte(before.path.parser, byte,
                parser::ParserSource { source: source.source, offset: source.offset },
                (before.path.path_bytes.len() + 1) as usize);
            assert(left.path.parser == step.image);
            assert(right.path.parser == step.image);
            assert(left.path.next_obligation == right.path.next_obligation);
            assert(left.path.root_left == right.path.root_left);
            assert(left.path.restore == right.path.restore);
            assert(left.path.path_bytes == right.path.path_bytes);
            assert(left.path.path_positions == right.path.path_positions);
            assert(left.path.gate == right.path.gate);
            match effect {
                epm::PathEdgeEffect::Plain => {
                    assert(left.path.pending == right.path.pending);
                },
                epm::PathEdgeEffect::Start { kind, payload_len, tag } => {
                    let left_pending = left.path.pending.unwrap();
                    let right_pending = right.path.pending.unwrap();
                    assert(left_pending.tag_source == right_pending.tag_source);
                    assert(left_pending.payload_end == right_pending.payload_end);
                    assert(left_pending == right_pending);
                },
                epm::PathEdgeEffect::Payload => {
                    let left_pending = left.path.pending.unwrap();
                    let right_pending = right.path.pending.unwrap();
                    assert(left_pending == right_pending);
                },
            }
            assert(left.path == right.path);
        },
        epm::EpmEvent::Leave { source_offset } => {
            assert(left.path.restore == right.path.restore);
            assert(left.path.pending == right.path.pending);
            assert(left.path.gate == right.path.gate);
            assert(left.path.parser == right.path.parser);
            assert(left.path.path_bytes == right.path.path_bytes);
            assert(left.path.path_positions == right.path.path_positions);
            assert(left.path == right.path);
        },
        epm::EpmEvent::Value { has_value, value_body, source_offset } => {
            assert(value_body.is_none());
            assert(left.path == right.path);
        },
        epm::EpmEvent::End => {
            assert(false);
        },
    }
    assert(left.wait == right.wait);
    assert(left.cursor == right.cursor);
    assert(left == right);
}

pub proof fn linked_epm_continue_is_functional(
    before: epm::LinkedEpmState,
    left: epm::LinkedEpmState,
    right: epm::LinkedEpmState,
    event: epm::EpmEvent,
)
    requires
        epm::linked_epm_continue(before, left, event),
        epm::linked_epm_continue(before, right, event),
    ensures left == right,
{
    epm_continue_event_step_is_functional(
        before.epm, left.epm, right.epm, event, before.admission.next_obligation_id,
    );
    assert(left.epm == right.epm);
    assert(left.admission == right.admission);
    assert(left.indexed_regions == right.indexed_regions);
}

pub proof fn product_epm_continue_is_functional(
    before: ProductScoreState,
    left: ProductScoreState,
    right: ProductScoreState,
    event: epm::EpmEvent,
)
    requires
        integrated::product_epm_continue_step(before, left, event),
        integrated::product_epm_continue_step(before, right, event),
    ensures left == right,
{
    let prior = integrated::product_linked_top(before).unwrap();
    let next_left = integrated::product_linked_top(left).unwrap();
    let next_right = integrated::product_linked_top(right).unwrap();
    linked_epm_continue_is_functional(prior, next_left, next_right, event);
    assert(left.map == right.map);
    assert(left.bridge == right.bridge);
    assert(left.indexed_regions == right.indexed_regions);
    assert(left.epm_frames == right.epm_frames);
    product_epm_receipts_carried_congruent(before, left, right);
    assert(left.accepted_epm_sources == right.accepted_epm_sources);
    assert(left == right);
}

pub proof fn live_runtime_epm_continue_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    event: epm::EpmEvent,
)
    requires
        unwind_product::live_runtime_internal_epm_unwind_step(before, left, event),
        unwind_product::live_runtime_internal_epm_unwind_step(before, right, event),
    ensures left == right,
{
    product_epm_continue_is_functional(
        before.source.product, left.source.product, right.source.product, event,
    );
    assert(left.source.runtime == right.source.runtime);
    assert(left.source.arena == right.source.arena);
    assert(left.source.wire == right.source.wire);
    assert(left.cursors == right.cursors);
    assert(left.source == right.source);
    assert(left == right);
}

pub proof fn accept_child_is_functional(
    before: epm::EpmFrame,
    left: epm::EpmFrame,
    right: epm::EpmFrame,
    return_id: Option<u64>,
    summary: epm::ChildSummary,
)
    requires
        epm::accept_child(before, left, return_id, summary),
        epm::accept_child(before, right, return_id, summary),
    ensures left == right,
{
    match before.wait {
        Some(epm::EpmWait::MapValue { .. }) => {
            assert(left.path == right.path);
        },
        Some(epm::EpmWait::Obligation { id, kind, .. }) => {
            assert(left.path.parser == right.path.parser);
            assert(left.path.gate == right.path.gate);
            assert(left.path.pending == right.path.pending);
            assert(left.path == right.path);
        },
        None => {},
    }
    assert(left.wait == right.wait);
    assert(left == right);
}

/// The Live map-value return of an EPathMap value child is a function of the
/// pre-state: `nested_after` is `None`, so only the `accept_child` arm matters.
pub proof fn product_epm_child_return_is_functional(
    before: ProductScoreState,
    left: ProductScoreState,
    right: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        integrated::product_epm_child_return_step(
            before, left, summary, admission_summary, None,
        ),
        integrated::product_epm_child_return_step(
            before, right, summary, admission_summary, None,
        ),
    ensures left == right,
{
    let prior = integrated::product_linked_parent(before).unwrap();
    let next_left = integrated::product_linked_top(left).unwrap();
    let next_right = integrated::product_linked_top(right).unwrap();
    let return_id = prior.admission.frames.last().return_id;
    accept_child_is_functional(
        prior.epm, next_left.epm, next_right.epm, return_id, summary,
    );
    assert(next_left.admission == next_right.admission);
    assert(next_left.indexed_regions == next_right.indexed_regions);
    assert(left.map == right.map);
    assert(left.bridge == right.bridge);
    assert(left.indexed_regions == right.indexed_regions);
    assert(left.epm_frames == right.epm_frames);
    product_epm_receipts_carried_congruent(before, left, right);
    assert(left == right);
}

pub proof fn live_runtime_epm_map_value_accept_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        integrated::live_runtime_product_epm_map_value_accept_step(
            before, left, summary, admission_summary,
        ),
        integrated::live_runtime_product_epm_map_value_accept_step(
            before, right, summary, admission_summary,
        ),
    ensures left == right,
{
    product_epm_child_return_is_functional(
        before.source.product, left.source.product, right.source.product,
        summary, admission_summary,
    );
    assert(left.source.runtime == right.source.runtime);
    assert(left.source.wire == right.source.wire);
    assert(left.source == right.source);
    assert(left.cursors == right.cursors);
    assert(left == right);
}

pub proof fn live_runtime_epm_end_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    observed: epm_terminal::EpmTerminalSourceObservation,
)
    requires
        integrated::live_runtime_product_epm_end_source_step(before, left, observed),
        integrated::live_runtime_product_epm_end_source_step(before, right, observed),
    ensures left == right,
{
    let prior = integrated::product_linked_top(before.source.product).unwrap();
    let complete = epm_terminal::epm_terminal_complete_from_source(prior.epm, observed).unwrap();
    let next_left = integrated::product_linked_top(left.source.product).unwrap();
    let next_right = integrated::product_linked_top(right.source.product).unwrap();
    assert(next_left.epm == complete.after);
    assert(next_right.epm == complete.after);
    let left_product = left.source.product;
    let right_product = right.source.product;
    assert(left_product.bridge == right_product.bridge);
    assert(left_product.map == right_product.map);
    assert(left_product.indexed_regions == right_product.indexed_regions);
    assert(left_product.epm_frames == right_product.epm_frames);
    product_epm_receipts_carried_congruent(before.source.product, left_product, right_product);
    assert(left_product == right_product);
    assert(left.source == right.source);
    assert(left == right);
}

pub proof fn product_epm_finish_accept_is_functional(
    before: ProductScoreState,
    left: ProductScoreState,
    right: ProductScoreState,
)
    requires
        integrated::product_epm_finish_accept_step(before, left),
        integrated::product_epm_finish_accept_step(before, right),
    ensures left == right,
{
    assert(left.bridge == right.bridge);
    assert(left.map == right.map);
    assert(left.epm_frames == right.epm_frames);
    assert(left.indexed_regions == right.indexed_regions);
    product_epm_receipts_carried_congruent(before, left, right);
    assert(left.accepted_epm_sources == right.accepted_epm_sources);
    assert(left == right);
}

pub proof fn live_runtime_epm_finish_accept_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_epm_finish_accept_step(before, left),
        integrated::live_runtime_product_epm_finish_accept_step(before, right),
    ensures left == right,
{
    product_epm_finish_accept_is_functional(
        before.source.product, left.source.product, right.source.product,
    );
    assert(left.source.runtime == right.source.runtime);
    assert(left.source.wire == right.source.wire);
    assert(left.source == right.source);
    assert(left.cursors == right.cursors);
    assert(left == right);
}

/// Every Live action except the two relational pushes is a function of the
/// pre-state. `MessageEpmPush` is included: its observed step is indexed-only.
pub proof fn integrated_live_non_push_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires
        !(action is MessagePush),
        !(action is MapValuePush),
        integrated::live_runtime_product_step(before, left, action),
        integrated::live_runtime_product_step(before, right, action),
    ensures left == right,
{
    match action {
        LiveRuntimeProductAction::Continue =>
            live_runtime_continue_is_functional(before, left, right),
        LiveRuntimeProductAction::ContinueIndexed =>
            live_runtime_continue_indexed_is_functional(before, left, right),
        LiveRuntimeProductAction::MapKey =>
            live_runtime_map_key_is_functional(before, left, right),
        LiveRuntimeProductAction::RootScore =>
            live_runtime_root_score_is_functional(before, left, right),
        LiveRuntimeProductAction::RootComplete { returned } =>
            live_runtime_root_score_is_functional(before, left, right),
        LiveRuntimeProductAction::MessageChildScore =>
            live_runtime_message_child_score_is_functional(before, left, right),
        LiveRuntimeProductAction::MessagePush { .. }
        | LiveRuntimeProductAction::MapValuePush { .. } => {},
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            match runtime::source_frame_push_observe_after(
                request.request_kind, actual_top_kind,
                before.source.product.bridge.admission.frames.len() as usize,
                before.source.runtime.capacity, after_len, after_capacity,
                before.source.runtime.frame_bytes, stats_frame_variant_bytes,
            ) {
                runtime::SourceFramePushObservationResult::Success { storage } =>
                    live_runtime_message_epm_push_indexed_is_functional(
                        before, left, right, request, storage,
                    ),
                _ => {},
            }
        },
        LiveRuntimeProductAction::MessageChildComplete =>
            live_runtime_message_child_complete_is_functional(before, left, right),
        LiveRuntimeProductAction::MapValueAccept =>
            live_runtime_map_value_accept_is_functional(before, left, right),
        LiveRuntimeProductAction::MapEntryFinish =>
            live_runtime_map_entry_finish_is_functional(before, left, right),
        LiveRuntimeProductAction::EpmMapValueAccept { summary, admission_summary } =>
            live_runtime_epm_map_value_accept_is_functional(
                before, left, right, summary, admission_summary,
            ),
        LiveRuntimeProductAction::EpmContinue { event } =>
            live_runtime_epm_continue_is_functional(before, left, right, event),
        LiveRuntimeProductAction::EpmPush { observed } => {},
        LiveRuntimeProductAction::EpmEnd { observed } =>
            live_runtime_epm_end_is_functional(before, left, right, observed),
        LiveRuntimeProductAction::EpmFinishAccept =>
            live_runtime_epm_finish_accept_is_functional(before, left, right),
    }
}

pub proof fn integrated_message_push_route_implies_observed(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
    route: IntegratedPushRoute,
)
    requires integrated_message_push_route_step(
        before, after, request, actual_top_kind, after_len, after_capacity,
        stats_frame_variant_bytes, route,
    ),
    ensures integrated::live_runtime_product_message_push_observed_step(
        before, after, request, actual_top_kind, after_len, after_capacity,
        stats_frame_variant_bytes,
    ),
{
}

pub proof fn integrated_map_value_push_route_implies_observed(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
    route: IntegratedPushRoute,
)
    requires integrated_map_value_push_route_step(
        before, after, request, actual_top_kind, after_len, after_capacity,
        stats_frame_variant_bytes, route,
    ),
    ensures integrated::live_runtime_product_map_value_push_observed_step(
        before, after, request, actual_top_kind, after_len, after_capacity,
        stats_frame_variant_bytes,
    ),
{
}

/// The non-push arms of the relation are the Live step itself. The Live step
/// stays hidden here, so this does not unfold its arms.
pub proof fn integrated_live_relation_non_push_refines_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
)
    requires
        !(action is MessagePush),
        !(action is MapValuePush),
        integrated_live_relation(before, after, action, route),
    ensures integrated::live_runtime_product_step(before, after, action),
{
    hide(integrated::live_runtime_product_step);
}

pub proof fn integrated_live_relation_message_push_refines_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
)
    requires
        action is MessagePush,
        integrated_live_relation(before, after, action, route),
    ensures integrated::live_runtime_product_step(before, after, action),
{
    match action {
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => integrated_message_push_route_implies_observed(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes, route,
        ),
        _ => {},
    }
}

pub proof fn integrated_live_relation_map_value_push_refines_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
)
    requires
        action is MapValuePush,
        integrated_live_relation(before, after, action, route),
    ensures integrated::live_runtime_product_step(before, after, action),
{
    match action {
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => integrated_map_value_push_route_implies_observed(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes, route,
        ),
        _ => {},
    }
}

/// A route-selected refinement is a member of the original observed relation,
/// so every Live integrated successor is a `live_runtime_product_step`. Both
/// relations stay hidden here, so the case split does not unfold either one.
pub proof fn integrated_live_relation_refines_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
)
    requires integrated_live_relation(before, after, action, route),
    ensures integrated::live_runtime_product_step(before, after, action),
{
    hide(integrated::live_runtime_product_step);
    hide(integrated_live_relation);
    match action {
        LiveRuntimeProductAction::MessagePush { .. } =>
            integrated_live_relation_message_push_refines_live_step(before, after, action, route),
        LiveRuntimeProductAction::MapValuePush { .. } =>
            integrated_live_relation_map_value_push_refines_live_step(before, after, action, route),
        _ => integrated_live_relation_non_push_refines_live_step(before, after, action, route),
    }
}

// ---------------------------------------------------------------------------
// A9 theorem: the integrated step is a function of (state, action)
// ---------------------------------------------------------------------------

pub proof fn integrated_step_is_function(
    before: IntegratedState,
    left: IntegratedState,
    right: IntegratedState,
    action: IntegratedAction,
)
    requires
        integrated_step(before, left, action),
        integrated_step(before, right, action),
    ensures left == right,
{
    match before {
        IntegratedState::Accepted { .. } => {},
        IntegratedState::Active { product } => match action {
            IntegratedAction::Live { live_action: live, route } => match live {
                LiveRuntimeProductAction::RootComplete { returned } => {
                    match (left, right) {
                        (
                            IntegratedState::Accepted {
                                accepted: IntegratedAccepted::LiveRoot {
                                    post_root_score: post_left, ..
                                },
                            },
                            IntegratedState::Accepted {
                                accepted: IntegratedAccepted::LiveRoot {
                                    post_root_score: post_right, ..
                                },
                            },
                        ) => live_runtime_root_score_is_functional(
                            product, post_left, post_right,
                        ),
                        _ => {},
                    }
                },
                LiveRuntimeProductAction::MessagePush {
                    request, actual_top_kind, after_len, after_capacity,
                    stats_frame_variant_bytes,
                } => {
                    match (left, right) {
                        (
                            IntegratedState::Active { product: next_left },
                            IntegratedState::Active { product: next_right },
                        ) => match runtime::source_frame_push_observe_after(
                            request.request_kind, actual_top_kind,
                            product.source.product.bridge.admission.frames.len() as usize,
                            product.source.runtime.capacity, after_len, after_capacity,
                            product.source.runtime.frame_bytes, stats_frame_variant_bytes,
                        ) {
                            runtime::SourceFramePushObservationResult::Success { storage } =>
                                match route {
                                    IntegratedPushRoute::Plain =>
                                        live_runtime_message_push_is_functional(
                                            product, next_left, next_right, request, storage,
                                        ),
                                    IntegratedPushRoute::Indexed =>
                                        live_runtime_message_push_indexed_is_functional(
                                            product, next_left, next_right, request, storage,
                                        ),
                                },
                            _ => {},
                        },
                        _ => {},
                    }
                },
                LiveRuntimeProductAction::MapValuePush {
                    request, actual_top_kind, after_len, after_capacity,
                    stats_frame_variant_bytes,
                } => {
                    match (left, right) {
                        (
                            IntegratedState::Active { product: next_left },
                            IntegratedState::Active { product: next_right },
                        ) => match runtime::source_frame_push_observe_after(
                            request.request_kind, actual_top_kind,
                            product.source.product.bridge.admission.frames.len() as usize,
                            product.source.runtime.capacity, after_len, after_capacity,
                            product.source.runtime.frame_bytes, stats_frame_variant_bytes,
                        ) {
                            runtime::SourceFramePushObservationResult::Success { storage } =>
                                match route {
                                    IntegratedPushRoute::Plain =>
                                        live_runtime_map_value_push_is_functional(
                                            product, next_left, next_right, request, storage,
                                        ),
                                    IntegratedPushRoute::Indexed =>
                                        live_runtime_map_value_push_indexed_is_functional(
                                            product, next_left, next_right, request, storage,
                                        ),
                                },
                            _ => {},
                        },
                        _ => {},
                    }
                },
                _ => {
                    match (left, right) {
                        (
                            IntegratedState::Active { product: next_left },
                            IntegratedState::Active { product: next_right },
                        ) => integrated_live_non_push_is_functional(
                            product, next_left, next_right, live,
                        ),
                        _ => {},
                    }
                },
            },
            IntegratedAction::CanonicalPath { .. }
            | IntegratedAction::ActForward { .. }
            | IntegratedAction::ActUnwind { .. } => {},
        },
    }
}

// ---------------------------------------------------------------------------
// Well-formedness of the ACT Forward successor
// ---------------------------------------------------------------------------

/// The fields `act_plan_events` and `act_plan_matches_frame` read. The path
/// component changes across a Forward batch; these fields do not.
pub open spec fn same_act_source_view(before: epm::EpmFrame, after: epm::EpmFrame) -> bool {
    &&& after.mode == before.mode
    &&& after.snapshot == before.snapshot
    &&& after.backing == before.backing
    &&& after.trie_start == before.trie_start
    &&& after.trie_end == before.trie_end
    &&& after.value_table_start == before.value_table_start
    &&& after.mode_offset == before.mode_offset
    &&& after.cursor.act_plan == before.cursor.act_plan
    &&& after.cursor.validated_plan == before.cursor.validated_plan
    &&& after.cursor.declared_value_count == before.cursor.declared_value_count
}

pub proof fn same_act_source_view_preserves_event_count(
    before: epm::EpmFrame, after: epm::EpmFrame,
)
    requires same_act_source_view(before, after),
    ensures epm::cursor_event_count(before) == epm::cursor_event_count(after),
{
    if before.cursor.act_plan.is_some() {
        let plan = before.cursor.act_plan.unwrap();
        assert(epm::act_plan_matches_frame(before, plan)
            == epm::act_plan_matches_frame(after, plan));
        assert(epm::act_plan_events(before, plan)
            == epm::act_plan_events(after, plan));
    }
}

pub proof fn act_forward_frame_is_layout_wf(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires
        batch::forward_batch_source_guard(
            before, start, width, pending, state_id, latest_restore_state_id, facts,
        ),
    ensures
        epm::frame_layout_wf(batch::forward_batch_after(before, start, width, pending, state_id)),
        epm::same_frame_source(before, batch::forward_batch_after(
            before, start, width, pending, state_id,
        )),
        batch::forward_batch_after(before, start, width, pending, state_id)
            .cursor.value_table_offset == before.cursor.value_table_offset,
{
    batch::accepted_forward_batch_composes_cursor_and_restore_run(
        before, start, width, pending, state_id, latest_restore_state_id, facts,
    );
    let after = batch::forward_batch_after(before, start, width, pending, state_id);
    let chunk = transport::EpmPushTransportChunk::Forward { start, width };
    let base = batch::path_source(before);
    let source = batch::trie_source(before);
    assert(same_act_source_view(before, after));
    same_act_source_view_preserves_event_count(before, after);
    if before.cursor.act_plan.is_some() {
        let plan = before.cursor.act_plan.unwrap();
        let validated = before.cursor.validated_plan.unwrap();
        assert(epm::act_plan_projects_validated(before, plan, validated)
            == epm::act_plan_projects_validated(after, plan, validated));
        assert(epm::act_plan_matches_frame(before, plan)
            == epm::act_plan_matches_frame(after, plan));
        assert(epm::act_plan_events(before, plan)
            == epm::act_plan_events(after, plan));
    }
    assert(transport::epm_push_chunk_events(chunk).len() == 2 * width as int);
    assert(after.cursor.at as int == before.cursor.at as int + 2 * width as int);
    assert(after.path.path_positions.len() == after.path.path_bytes.len()) by {
        assert(source.bytes.len() == source.positions.len());
        assert(after.path.path_positions.len()
            == base.positions.len() + source.positions.subrange(
                start as int, start as int + width as int,
            ).len());
    }
    assert(epm::frame_layout_wf(after));
}

/// The top admission frame's byte cursor can always be moved to a position
/// inside its region: only the replaced frame's cursor field changes.
pub proof fn integrated_epm_cursor_admission_preserves_wf(
    before: admission::AdmissionState, consumed: usize,
)
    requires
        admission::admission_state_wf(before),
        before.frames.last().frame_kind == 3,
        consumed <= before.frames.last().region_end,
    ensures
        admission::admission_state_wf(integrated_epm_cursor_admission(before, consumed)),
{
    let after = integrated_epm_cursor_admission(before, consumed);
    let last = before.frames.len() - 1;
    let parent = before.frames[last];
    assert(admission::admission_frame_wf(parent));
    assert(parent.region_start == 0);
    assert(admission::admission_frame_wf(epm::epm_advanced_admission_cursor(parent, consumed)));
    assert(after.frames.len() == before.frames.len());
    assert(after.frames[after.frames.len() - 1]
        == epm::epm_advanced_admission_cursor(parent, consumed));
    assert forall|i: int| 0 <= i < before.frames.len() - 1
        implies after.frames[i] == before.frames[i] by {}
    admission::admission_last_frame_replacement_preserves_wf(before, after);
}

/// The whole-product invariant after an internal Forward batch at the active
/// EPathMap frame. Only the top frame's cursor field, the EPM frame, and the
/// carried receipts change.
pub proof fn integrated_act_forward_product_preserves_wf(
    before: ProductScoreState, frame: epm::EpmFrame,
)
    requires
        integrated::product_score_wf(before),
        before.epm_frames.len() > 0,
        before.epm_frames.last().is_some(),
        epm::frame_layout_wf(frame),
        frame.snapshot == before.epm_frames.last().unwrap().snapshot,
    ensures ({
        let after = integrated_act_forward_product(before, frame);
        &&& integrated::product_score_wf(after)
        &&& after.bridge.transcript == before.bridge.transcript
        &&& after.bridge.admission.frames.len() == before.bridge.admission.frames.len()
        &&& after.indexed_regions == before.indexed_regions
        &&& after.bridge.admission.frames.last().frame_kind == 3
        &&& forall|i: int| 0 <= i < after.bridge.admission.frames.len() - 1
            ==> #[trigger] after.bridge.admission.frames[i] == before.bridge.admission.frames[i]
        &&& after.bridge.admission.frames.last().cursor == frame.cursor.value_table_offset
    }),
{
    let after = integrated_act_forward_product(before, frame);
    let old_admission = before.bridge.admission;
    let last = old_admission.frames.len() - 1;
    let parent = old_admission.frames[last];
    let consumed = frame.cursor.value_table_offset;
    let next_admission = integrated_epm_cursor_admission(old_admission, consumed);
    let linked = epm::LinkedEpmState {
        epm: frame,
        admission: next_admission,
        indexed_regions: before.indexed_regions,
    };

    assert(before.epm_frames.len() == old_admission.frames.len());
    assert(before.epm_frames[last] == before.epm_frames.last());
    let active = before.epm_frames[last].unwrap();
    assert(epm::indexed_image_wf(before.indexed_regions[last]));
    assert(parent.frame_kind == 3);
    assert(active.snapshot == before.indexed_regions[last]);
    assert(before.indexed_regions[last].bytes == parent.source_bytes);
    assert(admission::admission_state_wf(old_admission));
    assert(admission::admission_frame_wf(parent));
    assert(consumed <= frame.snapshot.bytes.len());
    assert(frame.snapshot.bytes == parent.source_bytes);
    assert(consumed <= parent.region_end);
    integrated_epm_cursor_admission_preserves_wf(old_admission, consumed);

    assert(after == unwind_product::product_after_internal_epm_continue(before, linked));
    assert(after.bridge.admission == next_admission);
    assert(after.map.admission == next_admission);
    assert(after.bridge.transcript == before.bridge.transcript);
    assert(after.map.transcript == before.map.transcript);
    assert(after.map.map_frames == before.map.map_frames);
    assert(after.indexed_regions == before.indexed_regions);
    assert(after.epm_frames == before.epm_frames.update(last, Some(frame)));

    let carried_candidate = ProductScoreState { accepted_epm_sources: Seq::empty(), ..after };
    product_epm_receipts_carried_congruent(before, after, carried_candidate);
    assert(after.accepted_epm_sources == integrated::product_epm_receipts_carried(before, after));

    assert(next_admission.frames.len() == old_admission.frames.len());
    assert(next_admission.frames[last] == epm::epm_advanced_admission_cursor(parent, consumed));
    assert forall|i: int| 0 <= i < last
        implies next_admission.frames[i] == old_admission.frames[i] by {}
    assert forall|i: int| 0 <= i < after.bridge.admission.frames.len() - 1
        implies #[trigger] after.bridge.admission.frames[i] == before.bridge.admission.frames[i] by {
        assert(next_admission.frames[i] == old_admission.frames[i]);
    }

    assert(next_admission.arena == old_admission.arena);
    integrated::unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);

    assert(before.map.map_frames[last].is_none());
    assert(next_admission.frames[last].frame_kind == 3);
    assert(map::integrated_admission_wf(after.map)) by {
        assert forall|index: int| 0 <= index < after.map.map_frames.len() implies {
            match #[trigger] after.map.map_frames[index] {
                None => after.map.admission.frames[index].frame_kind != 2,
                Some(map_frame) => after.map.admission.frames[index].frame_kind == 2
                    && index > 0
                    && after.map.admission.frames[index - 1].family
                        == admission::AdmissionFamily::New
                    && after.map.admission.frames[index - 1].program_type_id
                        == map_frame.owner_type_id
                    && after.map.admission.frames[index - 1].waiting_slot == Some(3)
                    && after.map.admission.frames[index - 1].waiting_tag
                        == Some(map_frame.outer_field)
                    && after.map.admission.frames[index - 1].waiting_kind == 2
                    && after.map.admission.frames[index - 1].waiting_id
                        == after.map.admission.frames[index].return_id
                    && after.map.admission.frames[index].return_id.is_some()
                    && after.map.admission.frames[index].body_id as int
                        == after.map.admission.frames[index].return_id.unwrap() as int + 1
                    && map::map_frame_wf(map_frame)
                    && map_frame.accepted_arena.len() <= after.map.admission.arena.len()
                    && map_frame.accepted_arena == after.map.admission.arena.subrange(
                        0, map_frame.accepted_arena.len() as int,
                    )
                    && map::map_aux_projection(map_frame, after.map.admission.frames[index]),
            }
        } by {
            assert(after.map.map_frames[index] == before.map.map_frames[index]);
            if index < last {
                assert(next_admission.frames[index] == old_admission.frames[index]);
                if index > 0 {
                    assert(next_admission.frames[index - 1] == old_admission.frames[index - 1]);
                }
            } else {
                assert(index == last);
            }
        }
    }

    assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(epm_frame) => {
                &&& after.bridge.admission.frames[i].frame_kind == 3
                &&& epm_frame.snapshot == after.indexed_regions[i]
                &&& epm::frame_layout_wf(epm_frame)
            },
        }
    } by {
        assert(after.indexed_regions[i] == before.indexed_regions[i]);
        if i < last {
            assert(after.epm_frames[i] == before.epm_frames[i]);
            assert(next_admission.frames[i] == old_admission.frames[i]);
        } else {
            assert(i == last);
            assert(after.epm_frames[i] == Some(frame));
            assert(next_admission.frames[i] == epm::epm_advanced_admission_cursor(parent, consumed));
        }
    }

    assert(integrated::product_frame_source_anchors_match(after)) by {
        assert forall|i: int| 0 <= i < after.indexed_regions.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.bridge.admission.frames[i].source_bytes == after.indexed_regions[i].bytes
            &&& after.bridge.admission.frames[i].physical_start
                == integrated::product_indexed_start_anchor(after.indexed_regions[i]).offset
            &&& after.bridge.admission.frames[i].physical_source
                == integrated::product_indexed_start_anchor(after.indexed_regions[i]).source
        } by {
            assert(after.indexed_regions[i] == before.indexed_regions[i]);
            if i < last {
                assert(next_admission.frames[i] == old_admission.frames[i]);
            } else {
                assert(i == last);
                assert(next_admission.frames[i]
                    == epm::epm_advanced_admission_cursor(parent, consumed));
            }
        }
    }

    assert(integrated::product_epm_source_receipts_wf(before));
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    integrated::product_epm_receipts_carried_preserves(before, after);
    integrated::product_score_wf_intro(after);
}

pub proof fn integrated_act_forward_preserves_wf(
    before: LiveRuntimeProductState,
    plan_source: act_source::AcceptedActPlanSourceImage,
    work: active::ActiveLineWorkSource,
    width: usize,
    commit: act_source::AcceptedLineCommitSource,
    continuation: Seq<epm::ActWorkItem>,
    fuel: nat,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires
        integrated_act_forward_guard(
            before, plan_source, work, width, commit, continuation, fuel,
            pending, state_id, latest_restore_state_id, facts,
        ),
    ensures
        integrated::live_runtime_product_wf(
            integrated_act_forward_after(before, width, commit, pending, state_id),
        ),
{
    let product = before.source.product;
    let frame = act::active_product_epm(product).unwrap();
    act::prepared_forward_batch_aligns_act_source(
        frame, work, width, commit, continuation, fuel,
    );
    assert(batch::forward_batch_source_guard(
        frame, commit.start, width, pending, state_id,
        latest_restore_state_id, facts,
    ));
    act_forward_frame_is_layout_wf(
        frame, commit.start, width, pending, state_id,
        latest_restore_state_id, facts,
    );
    let next_frame = integrated_act_forward_frame(product, width, commit, pending, state_id);
    assert(next_frame == batch::forward_batch_after(
        frame, commit.start, width, pending, state_id,
    ));
    assert(next_frame.snapshot == frame.snapshot);
    assert(product.epm_frames.last() == Some(frame));
    assert(product.epm_frames.last().unwrap().snapshot == frame.snapshot);
    integrated_act_forward_product_preserves_wf(product, next_frame);

    let after = integrated_act_forward_after(before, width, commit, pending, state_id);
    let next_product = after.source.product;
    assert(next_product == integrated_act_forward_product(product, next_frame));
    let old_frames = product.bridge.admission.frames;
    let new_frames = next_product.bridge.admission.frames;
    let last = old_frames.len() - 1;
    assert(old_frames.len() > 0);
    assert(new_frames.len() == old_frames.len());
    assert(next_product.bridge.transcript == product.bridge.transcript);
    assert(after.source.runtime.score == next_product.bridge);
    assert(runtime::runtime_frame_image_wf(after.source.runtime));
    assert(arena::runtime_projects_score(after.source.arena, next_product.bridge.transcript));
    assert(after.source.wire == before.source.wire);
    assert(before.source.wire.len() > 0);
    assert(before.source.wire.last().is_none()) by {
        assert(before.source.wire[last].is_none()
            || product.bridge.admission.frames[last].frame_kind == 1
            || product.bridge.admission.frames[last].frame_kind == 2);
        assert(product.bridge.admission.frames[last].frame_kind == 3);
    }
    assert forall|i: int| 0 <= i < after.source.wire.len() implies {
        let frame = next_product.bridge.admission.frames[i];
        match #[trigger] after.source.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        if i == last {
            assert(after.source.wire[i].is_none());
            assert(new_frames[i].frame_kind == 3);
        } else {
            assert(new_frames[i] == old_frames[i]);
        }
    }
    assert(integrated::live_source_product_wf(after.source));
    assert(after.cursors == before.cursors);
    assert(integrated::live_runtime_product_wf(after));
}

// ---------------------------------------------------------------------------
// A9 theorem: the integrated step preserves well-formedness
// ---------------------------------------------------------------------------

pub proof fn integrated_step_preserves_wf(
    before: IntegratedState,
    after: IntegratedState,
    action: IntegratedAction,
)
    requires
        integrated_wf(before),
        integrated_step(before, after, action),
    ensures integrated_wf(after),
{
    match before {
        IntegratedState::Accepted { .. } => {},
        IntegratedState::Active { product } => match action {
            IntegratedAction::Live { live_action: live, route } => {
                match (live, after) {
                    (
                        LiveRuntimeProductAction::RootComplete { returned },
                        IntegratedState::Accepted {
                            accepted: IntegratedAccepted::LiveRoot {
                                post_root_score, ..
                            },
                        },
                    ) => {
                        integrated::live_runtime_product_step_preserves_wf(
                            product, post_root_score, live,
                        );
                    },
                    (_, IntegratedState::Active { product: next }) => {
                        integrated_live_relation_refines_live_step(product, next, live, route);
                        integrated::live_runtime_product_step_preserves_wf(product, next, live);
                    },
                    _ => {},
                }
            },
            IntegratedAction::CanonicalPath { driver_before, driver_action: path } => {
                path_driver::canonical_path_driver_product_active_preserves_wf(driver_before, path);
                if let Some(path_driver::CanonicalPathDriverProductOutcome::Active { state }) =
                    path_driver::canonical_path_driver_product_step(driver_before, path)
                {
                    assert(path_driver::canonical_path_driver_product_wf(state));
                    assert(state.product == state.common.live.base);
                    assert(common::live_canonical_path_common_wf(state.common));
                    assert(path_live::live_canonical_path_wf(state.common.live));
                    assert(integrated::live_runtime_product_wf(state.product));
                }
            },
            IntegratedAction::ActForward {
                plan_source, work, width, commit, continuation, fuel, pending,
                state_id, latest_restore_state_id, forward_facts: facts,
            } => {
                integrated_act_forward_preserves_wf(
                    product, plan_source, work, width, commit, continuation, fuel,
                    pending, state_id, latest_restore_state_id, facts,
                );
            },
            IntegratedAction::ActUnwind { run, prefix, start, width, unwind_facts: facts } => {
                unwind_product::live_runtime_unwind_virtual_state_wf(
                    product, integrated_act_unwind_source(product),
                    run, prefix, start, width, facts, width,
                );
            },
        },
    }
}

/// Guards are exclusive: two actions that are keyed on a top frame kind and
/// that are both guarded on one state are keyed on the same kind.
pub proof fn integrated_kind_guards_are_exclusive(
    before: LiveRuntimeProductState,
    left: IntegratedAction,
    right: IntegratedAction,
)
    requires
        integrated_kind_guard(before, left),
        integrated_kind_guard(before, right),
        integrated_action_required_top_kind(left).is_some(),
        integrated_action_required_top_kind(right).is_some(),
    ensures
        integrated_action_required_top_kind(left) == integrated_action_required_top_kind(right),
{
}

pub proof fn product_linked_top_is_epathmap_frame(state: ProductScoreState)
    requires
        integrated::product_score_wf(state),
        integrated::product_linked_top(state).is_some(),
    ensures
        state.bridge.admission.frames.len() > 0,
        state.bridge.admission.frames.last().frame_kind == 3,
{
    let last = state.epm_frames.len() - 1;
    assert(epm::indexed_image_wf(state.indexed_regions[last]));
    assert(state.epm_frames[last] == state.epm_frames.last());
}

pub proof fn product_map_message_step_requires_message_top(
    before: ProductScoreState,
    after: ProductScoreState,
    message: admission::ConcreteFrameActionModel,
)
    requires
        integrated::product_score_wf(before),
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action: message },
        ),
    ensures
        before.bridge.admission.frames.len() > 0,
        before.bridge.admission.frames.last().frame_kind == 1,
{
    assert(map::integrated_admission_driver_step(
        before.map, map::IntegratedAdmissionAction::Message { action: message },
    ).is_some());
    assert(before.map.admission == before.bridge.admission);
}

pub proof fn live_runtime_message_push_is_message_top(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires integrated::live_runtime_product_message_push_step(before, after, request, storage),
    ensures
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
{
    let parent = before.cursors.last().unwrap();
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        product_map_message_step_requires_message_top(
            before.source.product, after.source.product,
            admission::ConcreteFrameActionModel::Push {
                field,
                child: request.child,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        );
    }
}

pub proof fn live_runtime_message_push_indexed_is_message_top(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_message_push_indexed_step(before, after, request, storage),
    ensures
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
{
    let parent = before.cursors.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        product_map_message_step_requires_message_top(
            before.source.product, after.source.product,
            admission::ConcreteFrameActionModel::PushIndexed {
                field,
                child: request.child,
                parent_source,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        );
    }
}

pub proof fn live_runtime_message_epm_push_indexed_is_message_top(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        integrated::live_runtime_product_message_epm_push_indexed_step(before, after, request, storage),
    ensures
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
{
    let parent = before.cursors.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire::WireConcreteTransition::Field {
        state: concrete_after, field, start: _,
    }) = wire::wire_runtime_next_field(parent) {
        product_map_message_step_requires_message_top(
            before.source.product, after.source.product,
            admission::ConcreteFrameActionModel::PushIndexed {
                field,
                child: request.child,
                parent_source,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        );
    }
}

pub proof fn live_source_continue_is_message_top(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires integrated::live_source_product_continue_step(before, after),
    ensures
        before.product.bridge.admission.frames.len() > 0,
        before.product.bridge.admission.frames.last().frame_kind == 1,
{
    let reader = before.wire.last().unwrap();
    if let wire::WireSourceTransition::Field { state: next_reader, field } =
        wire::wire_source_next_field(reader)
    {
        product_map_message_step_requires_message_top(
            before.product, after.product,
            admission::ConcreteFrameActionModel::Continue { field },
        );
    }
}

pub proof fn live_source_continue_indexed_is_message_top(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires integrated::live_source_product_continue_indexed_step(before, after),
    ensures
        before.product.bridge.admission.frames.len() > 0,
        before.product.bridge.admission.frames.last().frame_kind == 1,
{
    let reader = before.wire.last().unwrap();
    let parent_source = integrated::product_admission_source_image(
        before.product.indexed_regions.last(),
    );
    if let wire::WireSourceTransition::Field { state: next_reader, field } =
        wire::wire_source_next_field(reader)
    {
        product_map_message_step_requires_message_top(
            before.product, after.product,
            admission::ConcreteFrameActionModel::ContinueIndexed { field, parent_source },
        );
    }
}

pub proof fn live_runtime_continue_is_message_top(
    before: LiveRuntimeProductState, after: LiveRuntimeProductState,
)
    requires integrated::live_runtime_product_continue_step(before, after),
    ensures
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
{
    live_source_continue_is_message_top(before.source, after.source);
}

pub proof fn live_runtime_continue_indexed_is_message_top(
    before: LiveRuntimeProductState, after: LiveRuntimeProductState,
)
    requires integrated::live_runtime_product_continue_indexed_step(before, after),
    ensures
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
{
    live_source_continue_indexed_is_message_top(before.source, after.source);
}

/// The guard table excludes no Live step: every `live_runtime_product_step`
/// runs on the top frame kind the dispatcher keys that action on.
pub proof fn integrated_live_step_establishes_kind_guard(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires integrated::live_runtime_product_step(before, after, action),
    ensures
        before.source.product.bridge.admission.frames.len() > 0,
        integrated_top_frame_kind(before) == integrated_live_required_top_kind(action),
{
    match action {
        LiveRuntimeProductAction::Continue =>
            live_runtime_continue_is_message_top(before, after),
        LiveRuntimeProductAction::ContinueIndexed =>
            live_runtime_continue_indexed_is_message_top(before, after),
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            match runtime::source_frame_push_observe_after(
                request.request_kind, actual_top_kind,
                before.source.product.bridge.admission.frames.len() as usize,
                before.source.runtime.capacity, after_len, after_capacity,
                before.source.runtime.frame_bytes, stats_frame_variant_bytes,
            ) {
                runtime::SourceFramePushObservationResult::Success { storage } => {
                    if integrated::live_runtime_product_message_push_step(
                        before, after, request, storage,
                    ) {
                        live_runtime_message_push_is_message_top(before, after, request, storage);
                    } else {
                        live_runtime_message_push_indexed_is_message_top(
                            before, after, request, storage,
                        );
                    }
                },
                _ => {},
            }
        },
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            match runtime::source_frame_push_observe_after(
                request.request_kind, actual_top_kind,
                before.source.product.bridge.admission.frames.len() as usize,
                before.source.runtime.capacity, after_len, after_capacity,
                before.source.runtime.frame_bytes, stats_frame_variant_bytes,
            ) {
                runtime::SourceFramePushObservationResult::Success { storage } =>
                    live_runtime_message_epm_push_indexed_is_message_top(
                        before, after, request, storage,
                    ),
                _ => {},
            }
        },
        LiveRuntimeProductAction::EpmContinue { event } => {
            let product = before.source.product;
            let prior = integrated::product_linked_top(product).unwrap();
            let next = integrated::product_linked_top(after.source.product).unwrap();
            assert(epm::linked_epm_continue(prior, next, event));
            assert(prior.admission == product.bridge.admission);
        },
        LiveRuntimeProductAction::EpmPush { observed } => {
            assert(push_live::epm_push_live_ready(before, observed));
            assert(push_source::epm_push_source_ready(before, observed.push));
            product_linked_top_is_epathmap_frame(before.source.product);
        },
        LiveRuntimeProductAction::EpmEnd { observed } => {
            product_linked_top_is_epathmap_frame(before.source.product);
        },
        LiveRuntimeProductAction::EpmFinishAccept => {
            assert(integrated::live_source_product_wf(before.source));
            assert(integrated::product_linked_top(before.source.product).is_some());
            product_linked_top_is_epathmap_frame(before.source.product);
        },
        LiveRuntimeProductAction::RootScore
        | LiveRuntimeProductAction::RootComplete { .. } => {
            let frames = before.source.product.bridge.admission.frames;
            assert(frames.len() == 1);
            assert(frames[0] == frames.last());
        },
        LiveRuntimeProductAction::MapEntryFinish => {
            reveal(integrated::live_runtime_product_map_entry_finish_step);
        },
        _ => {},
    }
}

}
