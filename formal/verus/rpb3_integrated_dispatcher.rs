//! One dispatcher relation over the integrated live admission product.
//!
//! `integrated_step` keeps five action families exclusive through its action
//! constructor and through guards keyed on the top frame kind: Live product
//! actions (Message, Map, and scalar EPathMap frames), CanonicalPath driver
//! actions (kind-4 frames and the pre-pop child of a kind-4 parent), ACT
//! Forward and Unwind batches (a kind-3 top frame), the EPathMap obligation
//! returns, keyed on the returning child's kind: a raw-Par Message child (a
//! kind-1 top frame) or a NestedEpm1 EPathMap child (a kind-3 top frame)
//! returns into its EPathMap parent, and the EPathMap handoff, which gives a
//! pushed EPathMap child (a kind-3 top frame) its initial EPathMap frame. Their
//! relations are in `rpb3_epm_obligation_return_live` and
//! `rpb3_epm_live_handoff`. No integrated action carries a successor state: the
//! handoff's backing and ACT plan images are observations that
//! `linked_map_epm_handoff` validates. `integrated_step_is_function` proves
//! that a fixed `(before, action)` selects at most one successor. The observed
//! steps of `MessagePush` and `MapValuePush` are plain-or-indexed disjunctions
//! whose sides have different successors, so the action names the side it
//! takes in an explicit route.

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
    rpb3_epm_live_handoff as handoff,
    rpb3_epm_obligation_return_live as obligation_return,
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
    /// A raw-Par child (kind 1, program 1) at its End returns into its EPathMap
    /// parent waiting on a RawParEscape obligation: `finish_message_complete`
    /// runs `process_admission_completion` and the obligation arm of the
    /// EPathMap frame's `accept`. The relation is
    /// `live_runtime_epm_raw_par_return_step`.
    EpmRawParReturn,
    /// A NestedEpm1 EPathMap child ends and returns into its EPathMap parent
    /// waiting on a NestedEpm1 obligation in one driver iteration:
    /// `finish_other_complete` runs the same `process_admission_completion`.
    /// The relation is `live_runtime_epm_nested_return_step`.
    EpmNestedReturn,
    /// A pushed EPathMap child (kind 3) whose EPathMap entry is still `None`
    /// gets its initial EPathMap frame, which production builds inside the
    /// push (`EPathMapFrame::prepare_epm1_cursor`, then
    /// `EPathMapFrame::with_prepared_epm1_region`). `backing`, `plan` and
    /// `validated` are observations that `linked_map_epm_handoff` validates,
    /// not successors. The relation is `live_runtime_epm_handoff_step`.
    EpmHandoff {
        backing: epm::IndexedBackingImage,
        plan: Option<epm::ActPlanImage>,
        validated: Option<epm::ValidatedActPlanImage>,
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

/// The top frame kind an integrated action is keyed on. The EPathMap obligation
/// returns are keyed on the returning child's kind: kind 1 for the raw-Par
/// Message child and kind 3 for the NestedEpm1 EPathMap child. The EPathMap
/// handoff is keyed on the pushed EPathMap child, kind 3. `ChildReturn` has
/// none: its guard reads the CanonicalPath parent under the child that just
/// finished.
pub open spec fn integrated_action_required_top_kind(action: IntegratedAction) -> Option<u8> {
    match action {
        IntegratedAction::Live { live_action: live, .. } =>
            Some(integrated_live_required_top_kind(live)),
        IntegratedAction::CanonicalPath { driver_action: path, .. } => match path {
            path_driver::CanonicalPathDriverSourceAction::ChildReturn { .. } => None,
            _ => Some(4u8),
        },
        IntegratedAction::ActForward { .. } | IntegratedAction::ActUnwind { .. } => Some(3u8),
        IntegratedAction::EpmRawParReturn => Some(1u8),
        IntegratedAction::EpmNestedReturn => Some(3u8),
        IntegratedAction::EpmHandoff { .. } => Some(3u8),
    }
}

/// The guard keyed on the top frame kind. Each action family runs only on its
/// frames: Live on Message/Map/EPathMap tops (per action), CanonicalPath on a
/// kind-4 top or the pre-pop child of a kind-4 parent, ACT on a kind-3 top,
/// the EPathMap obligation returns on the returning child: a kind-1 top for
/// `EpmRawParReturn` and a kind-3 top for `EpmNestedReturn`, and the EPathMap
/// handoff on the pushed kind-3 top.
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
                IntegratedAction::EpmRawParReturn => match after {
                    IntegratedState::Active { product: next } =>
                        obligation_return::live_runtime_epm_raw_par_return_step(product, next),
                    IntegratedState::Accepted { .. } => false,
                },
                IntegratedAction::EpmNestedReturn => match after {
                    IntegratedState::Active { product: next } =>
                        obligation_return::live_runtime_epm_nested_return_step(product, next),
                    IntegratedState::Accepted { .. } => false,
                },
                IntegratedAction::EpmHandoff { backing, plan, validated } => match after {
                    IntegratedState::Active { product: next } =>
                        handoff::live_runtime_epm_handoff_step(
                            product, next, backing, plan, validated,
                        ),
                    IntegratedState::Accepted { .. } => false,
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

/// At a MessagePush action, `integrated_live_relation` is the route-selected
/// step at the action's fields. The relation is unfolded once, at the action's
/// constructor; the route-selected step stays hidden, so it is matched by
/// congruence on the fields and its push relations are never unfolded.
pub proof fn integrated_live_relation_message_push_is_route_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        action == (LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
        }),
        integrated_live_relation(before, after, action, route),
    ensures
        integrated_message_push_route_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes, route,
        ),
{
    hide(integrated_message_push_route_step);
}

/// At a MessagePush action, the observed MessagePush step at the action's
/// fields is a `live_runtime_product_step`. The Live step is unfolded once, at
/// the action's constructor; the observed step stays hidden, so it is matched
/// by congruence on the fields and its push relations are never unfolded.
pub proof fn integrated_message_push_observed_is_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        action == (LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
        }),
        integrated::live_runtime_product_message_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
    ensures integrated::live_runtime_product_step(before, after, action),
{
    hide(integrated::live_runtime_product_message_push_observed_step);
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
    hide(integrated_live_relation);
    hide(integrated::live_runtime_product_step);
    hide(integrated_message_push_route_step);
    hide(integrated::live_runtime_product_message_push_observed_step);
    match action {
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            integrated_live_relation_message_push_is_route_step(
                before, after, action, route, request, actual_top_kind, after_len,
                after_capacity, stats_frame_variant_bytes,
            );
            integrated_message_push_route_implies_observed(
                before, after, request, actual_top_kind, after_len, after_capacity,
                stats_frame_variant_bytes, route,
            );
            integrated_message_push_observed_is_live_step(
                before, after, action, request, actual_top_kind, after_len,
                after_capacity, stats_frame_variant_bytes,
            );
        },
        _ => {},
    }
}

/// At a MapValuePush action, `integrated_live_relation` is the route-selected
/// step at the action's fields. The relation is unfolded once, at the action's
/// constructor; the route-selected step stays hidden, so it is matched by
/// congruence on the fields and its push relations are never unfolded.
pub proof fn integrated_live_relation_map_value_push_is_route_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    route: IntegratedPushRoute,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        action == (LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
        }),
        integrated_live_relation(before, after, action, route),
    ensures
        integrated_map_value_push_route_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes, route,
        ),
{
    hide(integrated_map_value_push_route_step);
}

/// At a MapValuePush action, the observed MapValuePush step at the action's
/// fields is a `live_runtime_product_step`. The Live step is unfolded once, at
/// the action's constructor; the observed step stays hidden, so it is matched
/// by congruence on the fields and its push relations are never unfolded.
pub proof fn integrated_map_value_push_observed_is_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        action == (LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
        }),
        integrated::live_runtime_product_map_value_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
    ensures integrated::live_runtime_product_step(before, after, action),
{
    hide(integrated::live_runtime_product_map_value_push_observed_step);
}

/// The relation's MapValuePush arm is the route-selected step,
/// `integrated_map_value_push_route_implies_observed` turns it into the
/// observed step, and that is the Live step's MapValuePush arm. The two
/// unfoldings are separate lemmas; all four relations stay hidden here, so this
/// proof uses only the three lemma contracts and unfolds nothing.
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
    hide(integrated_live_relation);
    hide(integrated::live_runtime_product_step);
    hide(integrated_map_value_push_route_step);
    hide(integrated::live_runtime_product_map_value_push_observed_step);
    match action {
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            integrated_live_relation_map_value_push_is_route_step(
                before, after, action, route, request, actual_top_kind, after_len,
                after_capacity, stats_frame_variant_bytes,
            );
            integrated_map_value_push_route_implies_observed(
                before, after, request, actual_top_kind, after_len, after_capacity,
                stats_frame_variant_bytes, route,
            );
            integrated_map_value_push_observed_is_live_step(
                before, after, action, request, actual_top_kind, after_len,
                after_capacity, stats_frame_variant_bytes,
            );
        },
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
            IntegratedAction::EpmRawParReturn => match (left, right) {
                (
                    IntegratedState::Active { product: next_left },
                    IntegratedState::Active { product: next_right },
                ) => obligation_return::live_runtime_epm_raw_par_return_is_functional(
                    product, next_left, next_right,
                ),
                _ => {},
            },
            IntegratedAction::EpmNestedReturn => match (left, right) {
                (
                    IntegratedState::Active { product: next_left },
                    IntegratedState::Active { product: next_right },
                ) => obligation_return::live_runtime_epm_nested_return_is_functional(
                    product, next_left, next_right,
                ),
                _ => {},
            },
            IntegratedAction::EpmHandoff { backing, plan, validated } => match (left, right) {
                (
                    IntegratedState::Active { product: next_left },
                    IntegratedState::Active { product: next_right },
                ) => handoff::live_runtime_epm_handoff_is_functional(
                    product, next_left, next_right, backing, plan, validated,
                ),
                _ => {},
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
            IntegratedAction::EpmRawParReturn => {
                if let IntegratedState::Active { product: next } = after {
                    obligation_return::live_runtime_epm_raw_par_return_preserves_wf(product, next);
                }
            },
            IntegratedAction::EpmNestedReturn => {
                if let IntegratedState::Active { product: next } = after {
                    obligation_return::live_runtime_epm_nested_return_preserves_wf(product, next);
                }
            },
            IntegratedAction::EpmHandoff { backing, plan, validated } => {
                if let IntegratedState::Active { product: next } = after {
                    handoff::live_runtime_epm_handoff_preserves_wf(
                        product, next, backing, plan, validated,
                    );
                }
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

/// The raw-Par obligation return lifts to the dispatcher: a
/// `live_runtime_epm_raw_par_return_step` runs on a well-formed state whose top
/// frame is the returning kind-1 child, so it is an `EpmRawParReturn`
/// integrated step, and its successor is well-formed.
pub proof fn integrated_epm_raw_par_return_is_integrated_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires obligation_return::live_runtime_epm_raw_par_return_step(before, after),
    ensures
        integrated_wf(IntegratedState::Active { product: before }),
        integrated_step(
            IntegratedState::Active { product: before },
            IntegratedState::Active { product: after },
            IntegratedAction::EpmRawParReturn,
        ),
        integrated_wf(IntegratedState::Active { product: after }),
{
    obligation_return::live_runtime_epm_raw_par_return_top_kind(before, after);
    obligation_return::live_runtime_epm_raw_par_return_preserves_wf(before, after);
}

/// The nested obligation return lifts to the dispatcher: a
/// `live_runtime_epm_nested_return_step` runs on a well-formed state whose top
/// frame is the returning kind-3 child, so it is an `EpmNestedReturn`
/// integrated step, and its successor is well-formed.
pub proof fn integrated_epm_nested_return_is_integrated_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires obligation_return::live_runtime_epm_nested_return_step(before, after),
    ensures
        integrated_wf(IntegratedState::Active { product: before }),
        integrated_step(
            IntegratedState::Active { product: before },
            IntegratedState::Active { product: after },
            IntegratedAction::EpmNestedReturn,
        ),
        integrated_wf(IntegratedState::Active { product: after }),
{
    obligation_return::live_runtime_epm_nested_return_top_kind(before, after);
    obligation_return::live_runtime_epm_nested_return_preserves_wf(before, after);
}

/// The EPathMap handoff lifts to the dispatcher: a
/// `live_runtime_epm_handoff_step` runs on a well-formed state whose top frame
/// is the pushed kind-3 child, so it is an `EpmHandoff` integrated step with
/// the same observations, and its successor is well-formed.
pub proof fn integrated_epm_handoff_is_integrated_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires handoff::live_runtime_epm_handoff_step(before, after, backing, plan, validated),
    ensures
        integrated_wf(IntegratedState::Active { product: before }),
        integrated_step(
            IntegratedState::Active { product: before },
            IntegratedState::Active { product: after },
            IntegratedAction::EpmHandoff { backing, plan, validated },
        ),
        integrated_wf(IntegratedState::Active { product: after }),
{
    handoff::live_runtime_epm_handoff_top_kind(before, after, backing, plan, validated);
    handoff::live_runtime_epm_handoff_preserves_wf(before, after, backing, plan, validated);
}

// ---------------------------------------------------------------------------
// A10: traces
// ---------------------------------------------------------------------------

pub open spec fn integrated_trace(
    states: Seq<IntegratedState>, actions: Seq<IntegratedAction>,
) -> bool {
    &&& states.len() == actions.len() + 1
    &&& integrated_wf(states[0])
    &&& forall|i: int| 0 <= i < actions.len()
        ==> #[trigger] integrated_step(states[i], states[i + 1], actions[i])
}

pub proof fn integrated_trace_wf(
    states: Seq<IntegratedState>,
    actions: Seq<IntegratedAction>,
    index: int,
)
    requires
        integrated_trace(states, actions),
        0 <= index < states.len(),
    ensures integrated_wf(states[index]),
    decreases index,
{
    if index > 0 {
        let previous = index - 1;
        integrated_trace_wf(states, actions, previous);
        assert(0 <= previous < actions.len());
        assert(integrated_step(states[previous], states[previous + 1], actions[previous]));
        assert(previous + 1 == index);
        integrated_step_preserves_wf(states[previous], states[previous + 1], actions[previous]);
    }
}

pub proof fn integrated_trace_wf_append(
    states: Seq<IntegratedState>,
    actions: Seq<IntegratedAction>,
    after: IntegratedState,
    action: IntegratedAction,
)
    requires
        integrated_trace(states, actions),
        integrated_step(states.last(), after, action),
    ensures
        integrated_trace(states.push(after), actions.push(action)),
        integrated_wf(after),
{
    let extended_states = states.push(after);
    let extended_actions = actions.push(action);
    assert(states.len() > 0);
    integrated_trace_wf(states, actions, states.len() - 1);
    integrated_step_preserves_wf(states.last(), after, action);
    assert(extended_states.len() == extended_actions.len() + 1);
    assert(extended_states[0] == states[0]);
    assert forall|i: int| 0 <= i < extended_actions.len() implies
        #[trigger] integrated_step(extended_states[i], extended_states[i + 1], extended_actions[i]) by {
        if i < actions.len() {
            assert(extended_states[i] == states[i]);
            assert(extended_states[i + 1] == states[i + 1]);
            assert(extended_actions[i] == actions[i]);
            assert(integrated_step(states[i], states[i + 1], actions[i]));
        } else {
            assert(i == actions.len());
            assert(extended_states[i] == states.last());
            assert(extended_states[i + 1] == after);
            assert(extended_actions[i] == action);
        }
    }
}

/// An accepting trace ends in an `Accepted` state after at least one step.
pub open spec fn integrated_trace_accepts(
    states: Seq<IntegratedState>, actions: Seq<IntegratedAction>,
) -> bool {
    &&& integrated_trace(states, actions)
    &&& actions.len() > 0
    &&& states.last() is Accepted
}

/// The refinement target of an accepting trace: the last step really is the
/// root completion of the frame stack, with the exact root facts of each case.
pub open spec fn integrated_root_completion(
    pre: LiveRuntimeProductState,
    action: IntegratedAction,
    accepted: IntegratedAccepted,
) -> bool {
    let frames = pre.source.product.bridge.admission.frames;
    match accepted {
        IntegratedAccepted::LiveRoot { post_root_score, live_returned: returned } => match action {
            IntegratedAction::Live {
                live_action: LiveRuntimeProductAction::RootComplete { returned: action_returned }, ..
            } => {
                &&& action_returned == returned
                &&& integrated::live_runtime_product_root_complete_step(
                    pre, post_root_score, returned,
                )
                &&& frames.len() == 1
                &&& frames[0].frame_kind == 1
                &&& returned.remaining_frames == 0
                &&& returned.pre_pop.score == post_root_score.source.product.bridge
                &&& returned.pre_pop.score.admission.frames.len() == 1
                &&& returned.score_bytes == returned.pre_pop.score.admission.arena[
                    returned.summary.score_handle as int
                ].transcript_bytes
            },
            _ => false,
        },
        IntegratedAccepted::CanonicalPathRoot { pre_complete, path_returned: returned } => match action {
            IntegratedAction::CanonicalPath { driver_before, driver_action: path } => {
                &&& pre_complete == pre
                &&& driver_before.product == pre
                &&& path_driver::canonical_path_driver_product_wf(driver_before)
                &&& path_driver::canonical_path_driver_product_step(driver_before, path)
                    == Some(path_driver::CanonicalPathDriverProductOutcome::Terminal { returned })
                &&& frames.len() == 1
                &&& frames[0].frame_kind == 4
            },
            _ => false,
        },
    }
}

pub proof fn live_root_completion_is_exact(
    pre: LiveRuntimeProductState,
    post: LiveRuntimeProductState,
    returned: runtime::RuntimeRootReturnImage,
)
    requires integrated::live_runtime_product_root_complete_step(pre, post, returned),
    ensures
        pre.source.product.bridge.admission.frames.len() == 1,
        pre.source.product.bridge.admission.frames[0].frame_kind == 1,
        returned.remaining_frames == 0,
        returned.pre_pop.score == post.source.product.bridge,
        returned.pre_pop.score.admission.frames.len() == 1,
        returned.score_bytes == returned.pre_pop.score.admission.arena[
            returned.summary.score_handle as int
        ].transcript_bytes,
{
    assert(pre.source.runtime.score == pre.source.product.bridge);
    assert(runtime::runtime_message_driver_step(pre.source.runtime, runtime::RuntimeMessageEvent::Eof)
        == Some(runtime::RuntimeMessageOutcome::RootReturned { returned }));
    assert(score_bridge::bridge_score_finish_selected(pre.source.runtime.score)
        == Some(post.source.product.bridge));
    assert(runtime::runtime_message_eof_root_return(pre.source.runtime, post.source.product.bridge)
        == Some(returned));
    runtime::runtime_root_return_is_pre_pop_exact(pre.source.runtime, post.source.product.bridge);
}

pub proof fn integrated_accept_refines_root(
    states: Seq<IntegratedState>, actions: Seq<IntegratedAction>,
)
    requires integrated_trace_accepts(states, actions),
    ensures ({
        let last_step = actions.len() - 1;
        &&& integrated_wf(states[last_step])
        &&& match (states[last_step], states[last_step + 1]) {
            (IntegratedState::Active { product }, IntegratedState::Accepted { accepted }) =>
                integrated_root_completion(product, actions[last_step], accepted),
            _ => false,
        }
    }),
{
    let last_step = actions.len() - 1;
    integrated_trace_wf(states, actions, last_step);
    assert(0 <= last_step < actions.len());
    assert(integrated_step(states[last_step], states[last_step + 1], actions[last_step]));
    assert(states[last_step + 1] == states.last());
    match states[last_step] {
        IntegratedState::Accepted { .. } => {
            assert(false);
        },
        IntegratedState::Active { product } => {
            let after = states[last_step + 1];
            let action = actions[last_step];
            match action {
                IntegratedAction::Live { live_action: live, route } => {
                    match (live, after) {
                        (
                            LiveRuntimeProductAction::RootComplete { returned },
                            IntegratedState::Accepted {
                                accepted: IntegratedAccepted::LiveRoot { post_root_score, .. },
                            },
                        ) => {
                            live_root_completion_is_exact(product, post_root_score, returned);
                        },
                        _ => {},
                    }
                },
                IntegratedAction::CanonicalPath { driver_before, driver_action: path } => {
                    path_driver::canonical_path_driver_product_active_sync(driver_before, path);
                },
                IntegratedAction::ActForward { .. } | IntegratedAction::ActUnwind { .. }
                | IntegratedAction::EpmRawParReturn | IntegratedAction::EpmNestedReturn
                | IntegratedAction::EpmHandoff { .. } => {},
            }
        },
    }
}

/// One accepted step from a well-formed state is an accepting trace, so the
/// premise of `integrated_accept_refines_root` is inhabited whenever such a
/// step exists.
pub proof fn integrated_accepted_step_is_accepting_trace(
    before: IntegratedState,
    after: IntegratedState,
    action: IntegratedAction,
)
    requires
        integrated_wf(before),
        integrated_step(before, after, action),
        after is Accepted,
    ensures integrated_trace_accepts(seq![before, after], seq![action]),
{
    let states = seq![before, after];
    let actions = seq![action];
    assert(states.len() == 2);
    assert(actions.len() == 1);
    assert(states[0] == before);
    assert(states[1] == after);
    assert(actions[0] == action);
    assert forall|i: int| 0 <= i < actions.len() implies
        #[trigger] integrated_step(states[i], states[i + 1], actions[i]) by {
        assert(i == 0);
        assert(states[i + 1] == after);
    }
    assert(states.last() == after);
}

// ---------------------------------------------------------------------------
// A10: non-vacuity witnesses
// ---------------------------------------------------------------------------

/// Live: the modeled empty nested EPathMap accept is one integrated step.
pub proof fn live_nested_epm_finish_accept_is_integrated_step()
    ensures ({
        let before = integrated::live_epm_modeled_empty_nested_before();
        let after = integrated::live_epm_modeled_empty_nested_after_accept();
        let action = IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::EpmFinishAccept,
            route: IntegratedPushRoute::Plain,
        };
        &&& integrated_wf(IntegratedState::Active { product: before })
        &&& integrated_step(
            IntegratedState::Active { product: before },
            IntegratedState::Active { product: after },
            action,
        )
    }),
{
    integrated::live_epm_modeled_empty_nested_accept_witness();
    let before = integrated::live_epm_modeled_empty_nested_before();
    let top = integrated::product_linked_top(before.source.product).unwrap();
    let last = before.source.product.epm_frames.len() - 1;
    assert(before.source.product.epm_frames.len() > 0);
    assert(before.source.product.epm_frames[last] == before.source.product.epm_frames.last());
    assert(before.source.product.bridge.admission.frames[last].frame_kind == 3);
}

/// CanonicalPath: the Boolean root-complete driver action is accepted.
pub proof fn boolean_root_canonical_path_complete_is_integrated_step()
    ensures ({
        let common_root = common::live_canonical_path_common_root_from_source(
            common::boolean_root_source(),
        ).unwrap();
        let driver_before = path_driver::CanonicalPathDriverProductState {
            common: common_root,
            product: common_root.live.base,
            canonical_events: 0,
            last_driver_step: None,
        };
        let storage = path_driver::CanonicalPathDriverStorageObservation {
            step_index: 0,
            before_len: 1,
            after_len: 0,
            before_top_kind: 4,
            after_top_kind: None,
            score_entries_before: 0,
            score_entries_after: 0,
        };
        let path = path_driver::CanonicalPathDriverSourceAction::RootComplete {
            storage_observed: storage,
            source_before: parser::initial_parser_source_snapshot(),
            loop_events: seq![common::boolean_root_loop_event()],
            source_after_scan: common::boolean_root_source_snapshot(),
            observed: common::boolean_root_driver_observation(),
        };
        let step = path_driver::canonical_path_driver_product_step(driver_before, path);
        let after = integrated_path_outcome_state(driver_before.product, step.unwrap());
        &&& step.is_some()
        &&& step.unwrap() is Terminal
        &&& integrated_wf(IntegratedState::Active { product: driver_before.product })
        &&& after is Accepted
        &&& integrated_step(
            IntegratedState::Active { product: driver_before.product },
            after,
            IntegratedAction::CanonicalPath { driver_before, driver_action: path },
        )
        &&& integrated_trace_accepts(
            seq![IntegratedState::Active { product: driver_before.product }, after],
            seq![IntegratedAction::CanonicalPath { driver_before, driver_action: path }],
        )
    }),
{
    path_driver::boolean_root_driver_product_witness();
    let common_root = common::live_canonical_path_common_root_from_source(
        common::boolean_root_source(),
    ).unwrap();
    let driver_before = path_driver::CanonicalPathDriverProductState {
        common: common_root,
        product: common_root.live.base,
        canonical_events: 0,
        last_driver_step: None,
    };
    let storage = path_driver::CanonicalPathDriverStorageObservation {
        step_index: 0,
        before_len: 1,
        after_len: 0,
        before_top_kind: 4,
        after_top_kind: None,
        score_entries_before: 0,
        score_entries_after: 0,
    };
    let path = path_driver::CanonicalPathDriverSourceAction::RootComplete {
        storage_observed: storage,
        source_before: parser::initial_parser_source_snapshot(),
        loop_events: seq![common::boolean_root_loop_event()],
        source_after_scan: common::boolean_root_source_snapshot(),
        observed: common::boolean_root_driver_observation(),
    };
    path_driver::canonical_path_driver_product_active_sync(driver_before, path);
    assert(path_driver::canonical_path_driver_product_wf(driver_before));
    let step = path_driver::canonical_path_driver_product_step(driver_before, path);
    assert(step.unwrap() is Terminal);
    assert(common::live_canonical_path_common_wf(driver_before.common));
    assert(path_live::live_canonical_path_wf(driver_before.common.live));
    assert(integrated::live_runtime_product_wf(driver_before.product));
    assert(integrated_kind_guard(
        driver_before.product,
        IntegratedAction::CanonicalPath { driver_before, driver_action: path },
    ));
    let active = IntegratedState::Active { product: driver_before.product };
    let accepted = integrated_path_outcome_state(driver_before.product, step.unwrap());
    let action = IntegratedAction::CanonicalPath { driver_before, driver_action: path };
    assert(integrated_wf(active));
    assert(accepted is Accepted);
    assert(integrated_step(active, accepted, action));
    integrated_accepted_step_is_accepting_trace(active, accepted, action);
}

pub open spec fn act_unwind_witness_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 1,
        physical_start: 0,
        program_type_id: 0,
        region_start: 0,
        region_end: 37,
        source_bytes: batch::unwind_witness_bytes(),
        cursor: 37,
        slot_count: 0,
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
        frame_kind: 3,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn act_unwind_witness_admission() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![act_unwind_witness_frame()],
        arena: Seq::empty(),
        root_end: 37,
        pushed: 1,
        completed: 0,
        next_obligation_id: 0,
    }
}

pub open spec fn act_unwind_witness_product() -> ProductScoreState {
    let admission = act_unwind_witness_admission();
    let transcript = score::empty_arena();
    ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge { admission, transcript },
        map: map::IntegratedAdmissionState {
            admission, map_frames: seq![None], transcript,
        },
        epm_frames: seq![Some(batch::unwind_witness_before(None, true))],
        indexed_regions: seq![batch::unwind_witness_snapshot()],
        accepted_epm_sources: seq![None],
    }
}

pub open spec fn act_unwind_witness_state() -> LiveRuntimeProductState {
    let product = act_unwind_witness_product();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime::RuntimeFrameImage {
                score: product.bridge,
                capacity: 1,
                frame_bytes: 1,
                max_frames: 1,
                max_storage_bytes: 1,
                max_capacity_bytes: 1,
            },
            arena: arena::RuntimeScoreArena { entries: Seq::empty() },
            wire: seq![None],
        },
        cursors: seq![None],
    }
}

pub proof fn act_unwind_witness_admission_is_wf()
    ensures
        admission::admission_state_wf(act_unwind_witness_admission()),
        act_unwind_witness_admission().frames.len() == 1,
{
    batch::unwind_witness_encoded_prefix();
    let frame = act_unwind_witness_frame();
    let state = act_unwind_witness_admission();
    assert(frame.source_bytes.len() == 37);
    assert(admission::admission_frame_wf(frame));
    assert forall|index: int| 0 <= index < state.frames.len()
        implies admission::admission_frame_wf(#[trigger] state.frames[index]) by {
        assert(index == 0);
    }
    assert(admission::admission_state_wf(state));
}

pub proof fn act_unwind_witness_product_is_wf()
    ensures integrated::product_score_wf(act_unwind_witness_product()),
{
    batch::unwind_witness_encoded_prefix();
    batch::unwind_witness_act_layout(None, true);
    score::empty_arena_well_formed();
    act_unwind_witness_admission_is_wf();
    let product = act_unwind_witness_product();
    let frame = act_unwind_witness_frame();
    let snapshot = batch::unwind_witness_snapshot();
    let before = batch::unwind_witness_before(None, true);
    assert(before.snapshot == snapshot);
    assert(snapshot.bytes == batch::unwind_witness_bytes());
    assert(epm::indexed_backing_project(batch::unwind_witness_backing()) == Some(snapshot));
    assert(epm::indexed_image_wf(snapshot));
    assert(snapshot.positions[0] == batch::unwind_witness_anchor(0));
    assert(integrated::product_indexed_start_anchor(snapshot) == batch::unwind_witness_anchor(0));
    assert(map::integrated_admission_wf(product.map)) by {
        assert forall|index: int| 0 <= index < product.map.map_frames.len() implies {
            match #[trigger] product.map.map_frames[index] {
                None => product.map.admission.frames[index].frame_kind != 2,
                Some(map_frame) => product.map.admission.frames[index].frame_kind == 2
                    && index > 0
                    && product.map.admission.frames[index - 1].family
                        == admission::AdmissionFamily::New
                    && product.map.admission.frames[index - 1].program_type_id
                        == map_frame.owner_type_id
                    && product.map.admission.frames[index - 1].waiting_slot == Some(3)
                    && product.map.admission.frames[index - 1].waiting_tag
                        == Some(map_frame.outer_field)
                    && product.map.admission.frames[index - 1].waiting_kind == 2
                    && product.map.admission.frames[index - 1].waiting_id
                        == product.map.admission.frames[index].return_id
                    && product.map.admission.frames[index].return_id.is_some()
                    && product.map.admission.frames[index].body_id as int
                        == product.map.admission.frames[index].return_id.unwrap() as int + 1
                    && map::map_frame_wf(map_frame)
                    && map_frame.accepted_arena.len() <= product.map.admission.arena.len()
                    && map_frame.accepted_arena == product.map.admission.arena.subrange(
                        0, map_frame.accepted_arena.len() as int,
                    )
                    && map::map_aux_projection(map_frame, product.map.admission.frames[index]),
            }
        } by {
            assert(index == 0);
            assert(product.map.map_frames[0] == None::<map::MapFrameModel>);
            assert(product.map.admission.frames[0] == frame);
        }
    }
    assert(integrated::product_epm_source_receipts_wf(product)) by {
        assert forall|i: int| 0 <= i < product.accepted_epm_sources.len()
            implies match #[trigger] product.accepted_epm_sources[i] {
                None => true,
                Some(source) => integrated::product_epm_source_receipt_matches(
                    product.bridge.admission.frames[i], product.indexed_regions[i], source,
                ),
            } by {
            assert(i == 0);
            assert(product.accepted_epm_sources[0] == None::<epm::LinkedEpmState>);
        }
    }
    assert(integrated::product_frame_source_anchors_match(product)) by {
        assert forall|i: int| 0 <= i < product.indexed_regions.len() implies {
            &&& epm::indexed_image_wf(product.indexed_regions[i])
            &&& product.bridge.admission.frames[i].source_bytes == product.indexed_regions[i].bytes
            &&& product.bridge.admission.frames[i].physical_start
                == integrated::product_indexed_start_anchor(product.indexed_regions[i]).offset
            &&& product.bridge.admission.frames[i].physical_source
                == integrated::product_indexed_start_anchor(product.indexed_regions[i]).source
        } by {
            assert(i == 0);
            assert(product.indexed_regions[0] == snapshot);
            assert(product.bridge.admission.frames[0] == frame);
        }
    }
    assert forall|i: int| 0 <= i < product.epm_frames.len() implies {
        &&& epm::indexed_image_wf(product.indexed_regions[i])
        &&& product.indexed_regions[i].bytes == product.bridge.admission.frames[i].source_bytes
        &&& match product.epm_frames[i] {
            None => true,
            Some(epm_frame) => {
                &&& product.bridge.admission.frames[i].frame_kind == 3
                &&& epm_frame.snapshot == product.indexed_regions[i]
                &&& epm::frame_layout_wf(epm_frame)
            },
        }
    } by {
        assert(i == 0);
        assert(product.indexed_regions[0] == snapshot);
        assert(product.bridge.admission.frames[0] == frame);
        assert(product.epm_frames[0] == Some(before));
    }
    integrated::product_score_wf_intro(product);
}

pub proof fn act_unwind_witness_state_is_wf()
    ensures integrated::live_runtime_product_wf(act_unwind_witness_state()),
{
    act_unwind_witness_product_is_wf();
    act_unwind_witness_admission_is_wf();
    score::empty_arena_well_formed();
    let state = act_unwind_witness_state();
    let product = state.source.product;
    assert(product.bridge.admission.frames.len() == 1);
    assert(product.bridge.admission.frames[0] == act_unwind_witness_frame());
    assert(runtime::runtime_frame_image_wf(state.source.runtime));
    assert(arena::runtime_projects_score(state.source.arena, product.bridge.transcript));
    assert forall|i: int| 0 <= i < state.source.wire.len() implies {
        let frame = product.bridge.admission.frames[i];
        match #[trigger] state.source.wire[i] {
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
        assert(i == 0);
    }
    assert(integrated::live_source_product_wf(state.source));
    assert forall|i: int| 0 <= i < state.cursors.len() implies match #[trigger] state.cursors[i] {
        None => true,
        Some(cursor) => integrated::product_wire_indexed_source_agrees(
            cursor, state.source.product.indexed_regions[i],
        ),
    } by {
        assert(i == 0);
        assert(state.cursors[0] == None::<wire::WireRuntimeCursorImage>);
    }
    assert forall|i: int| 0 <= i < state.cursors.len() implies {
        match (#[trigger] state.cursors[i], state.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire::wire_concrete_cursor_wf(wire::wire_runtime_derive_cursor(cursor).unwrap())
                &&& wire::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(i == 0);
        assert(state.cursors[0] == None::<wire::WireRuntimeCursorImage>);
        assert(state.source.wire[0] == None::<wire::WireSourceState>);
    }
    assert(integrated::live_runtime_product_wf(state));
}

/// ACT: a one-frame Live state whose top EPathMap frame is the composition
/// module's Unwind witness; the integrated ActUnwind step is enabled on it.
pub proof fn act_unwind_witness_is_integrated_step()
    ensures ({
        let before = act_unwind_witness_state();
        let run = batch::unwind_witness_run(None, true);
        let prefix = Seq::<epm::PathCheckpoint>::empty();
        let action = IntegratedAction::ActUnwind {
            run, prefix, start: 17, width: 1, unwind_facts: batch::unwind_witness_facts(),
        };
        &&& integrated_wf(IntegratedState::Active { product: before })
        &&& integrated_step(
            IntegratedState::Active { product: before },
            IntegratedState::Active { product: integrated_act_unwind_after(before, run, prefix, 1) },
            action,
        )
    }),
{
    act_unwind_witness_state_is_wf();
    act_unwind_witness_admission_is_wf();
    batch::accepted_unwind_has_no_current_pending_witness();
    batch::unwind_witness_encoded_prefix();
    batch::unwind_witness_act_layout(None, true);
    let before = act_unwind_witness_state();
    let product = before.source.product;
    let snapshot = batch::unwind_witness_snapshot();
    let source = integrated_act_unwind_source(before);
    let run = batch::unwind_witness_run(None, true);
    let prefix = Seq::<epm::PathCheckpoint>::empty();
    let frame = act_unwind_witness_frame();
    assert(product.epm_frames.last() == Some(batch::unwind_witness_before(None, true)));
    assert(integrated::product_linked_top(product).is_some());
    assert(source == integrated::product_linked_top(product).unwrap());
    assert(source.epm == batch::unwind_witness_before(None, true));
    assert(source.admission == act_unwind_witness_admission());
    assert(source.indexed_regions == seq![snapshot]);
    assert(source.admission.frames.last() == frame);
    assert(snapshot.positions[0] == batch::unwind_witness_anchor(0));
    assert(epm::linked_indexed_sources(source)) by {
        assert forall|i: int| 0 <= i < source.admission.frames.len()
            implies epm::indexed_image_wf(source.indexed_regions[i])
                && source.indexed_regions[i].bytes == source.admission.frames[i].source_bytes
                && epm::indexed_position_at(source.indexed_regions[i], 0).source
                    == source.admission.frames[i].physical_source
                && epm::indexed_position_at(source.indexed_regions[i], 0).offset
                    == source.admission.frames[i].physical_start by {
            assert(i == 0);
            assert(source.indexed_regions[0] == snapshot);
            assert(source.admission.frames[0] == frame);
        }
    }
    assert(unwind::linked_unwind_source_guard(
        source, run, prefix, 17, 1, batch::unwind_witness_facts(),
    ));
    assert(integrated_act_unwind_guard(before, run, prefix, 17, 1, batch::unwind_witness_facts()));
}

// ---------------------------------------------------------------------------
// ACT Forward non-vacuity witness
// ---------------------------------------------------------------------------

/// The EPM1 snapshot of the ACT Forward witness, 39 bytes in total: a Map whose
/// ACT trie has one record, the two-byte line `0x2a 0x2b` with a terminal value,
/// and whose value table holds one empty body. The line has two bytes because a
/// Forward batch stops before a line's final value-bearing edge. The snapshot is
/// a Map rather than a Set because `accepted_act_plan_source_image` equates the
/// ACT value count with the declared EPM1 count, which Set mode fixes at zero.
pub open spec fn act_forward_witness_bytes() -> Seq<u8> {
    seq![
        0x45, 0x50, 0x4d, 0x31, 1, 2, 30,
        0x41, 0x43, 0x54, 0x72, 0x65, 0x65, 0x30, 0x33,
        19, 0, 0, 0, 0, 0, 0, 0,
        2, 0x2a, 0x2b, 0xc0, 0, 3,
        0, 0, 0, 0, 0, 0, 0, 0,
        1, 0
    ]
}

/// The ACT trie, the indexed view `bytes[7..37]` of the snapshot.
pub open spec fn act_forward_witness_trie() -> Seq<u8> {
    seq![
        0x41, 0x43, 0x54, 0x72, 0x65, 0x65, 0x30, 0x33,
        19, 0, 0, 0, 0, 0, 0, 0,
        2, 0x2a, 0x2b, 0xc0, 0, 3,
        0, 0, 0, 0, 0, 0, 0, 0
    ]
}

pub open spec fn act_forward_witness_anchor(offset: usize) -> epm::SourceAnchor {
    epm::SourceAnchor { source: 1, offset }
}

pub open spec fn act_forward_witness_backing() -> epm::IndexedBackingImage {
    epm::IndexedBackingImage::Borrowed {
        bytes: act_forward_witness_bytes(),
        source: 1,
        absolute_start: 0,
        start: 0,
        len: 39,
        end_override: None,
    }
}

pub open spec fn act_forward_witness_snapshot() -> epm::IndexedRegionImage {
    epm::IndexedRegionImage {
        bytes: act_forward_witness_bytes(),
        positions: Seq::new(39nat, |i: int| act_forward_witness_anchor(i as usize)),
        end: act_forward_witness_anchor(39),
    }
}

/// One record: the line at trie offsets 17 and 18 (encoded at offset 16), with
/// the terminal value marker at node 19.
pub open spec fn act_forward_witness_plan() -> epm::ActPlanImage {
    epm::ActPlanImage {
        records: seq![epm::ActRecordImage {
            node_id: 19,
            subtree_end: 1,
            value: Some(0),
            value_source_offset: 19,
            line_start: 17,
            line_len: 2,
            line_encoded_start: 16,
            wrapper_id: None,
            wrapper_len: 0,
            incoming_source_offset: None,
            incoming_origin: None,
            children: Seq::empty(),
        }],
        finish_order: seq![0],
        line_hash: |_bytes: Seq<u8>| 0,
    }
}

pub open spec fn act_forward_witness_validated_plan() -> epm::ValidatedActPlanImage {
    epm::ValidatedActPlanImage {
        records: seq![epm::ValidatedTopologyRecordImage {
            node_id: 19,
            value: 0,
            value_source_offset: 19,
            line_start: 17,
            line_len: 2,
            incoming_source_offset: 0,
            child_count: 0,
            incoming_byte: 0,
            flags: 1,
        }],
        empty: false,
        value_count: 1,
        first_nonzero: None,
    }
}

/// The path gate of the parser below, read at the root path: a non-empty stack,
/// not split-complete, no completed segment yet.
pub open spec fn act_forward_witness_gate() -> epm::PathGate {
    epm::PathGate {
        dead: false,
        stack_empty: false,
        split_complete: false,
        completed_segments: 0,
        first_is_list: false,
        stable: true,
        escape_allowed: true,
    }
}

/// A parser focused on the payload of an open RawPar obligation of two bytes,
/// none consumed: the state in which a Forward batch advances interior bytes.
pub open spec fn act_forward_witness_parser() -> parser::ParserImage {
    parser::ParserImage {
        focus: parser::ParserPath {
            status: parser::ParserStatus::Open,
            completed_segments: 0,
            first_tag: None,
            stable: true,
            escape_allowed: true,
            stack: seq![parser::ParserFrame::Obligation {
                id: 0,
                kind: parser::ParserObligationKind::RawParEscape,
                payload_len: 2,
                tag_source: parser::ParserSource { source: 1, offset: 23 },
                payload_start: 0,
                status: parser::ParserObligationStatus::Payload,
            }],
        },
        owners: Seq::empty(),
        next_obligation: 1,
        stamp: 0,
        fault: false,
    }
}

pub open spec fn act_forward_witness_pending() -> epm::PathObligation {
    epm::PathObligation {
        id: 0,
        kind: epm::PathObligationKind::RawParEscape,
        payload_len: 2,
        tag_source: act_forward_witness_anchor(23),
        payload_start: 0,
        payload_end: act_forward_witness_anchor(23),
        awaiting_resolution: false,
    }
}

/// The EPM frame before the batch: the root Value (event 0) has been consumed
/// and the cursor is at event 1, the first Forward edge of the line.
pub open spec fn act_forward_witness_before() -> epm::EpmFrame {
    epm::EpmFrame {
        path: epm::PathTopology {
            next_obligation: 1,
            pending: Some(act_forward_witness_pending()),
            root_left: false,
            restore: Seq::empty(),
            path_bytes: Seq::empty(),
            path_positions: Seq::empty(),
            gate: act_forward_witness_gate(),
            parser: act_forward_witness_parser(),
        },
        cursor: epm::EpmCursorImage {
            act_plan: Some(act_forward_witness_plan()),
            validated_plan: Some(act_forward_witness_validated_plan()),
            at: 1,
            declared_value_count: 1,
            map_values_seen: 0,
            value_table_offset: 38,
            done: false,
        },
        wait: None,
        mode: epm::EpmMode::Map,
        entries_stable: true,
        last_source: act_forward_witness_anchor(26),
        snapshot: act_forward_witness_snapshot(),
        backing: act_forward_witness_backing(),
        trie_start: 7,
        trie_end: 37,
        value_table_start: 38,
        mode_offset: 5,
    }
}

/// The raw ACT event stream of the witness: the root Value, the line's
/// Edge/Value pairs, its Leaves, and the node's Leave.
pub open spec fn act_forward_witness_raw_events() -> Seq<epm::WireTopologyEvent> {
    seq![
        epm::WireTopologyEvent::Value { has_value: false, value_body: None, source_offset: 19 },
        epm::WireTopologyEvent::Edge { source_offset: 17, origin: epm::EdgeOrigin::Direct },
        epm::WireTopologyEvent::Value { has_value: false, value_body: None, source_offset: 17 },
        epm::WireTopologyEvent::Edge { source_offset: 18, origin: epm::EdgeOrigin::Direct },
        epm::WireTopologyEvent::Value { has_value: true, value_body: None, source_offset: 19 },
        epm::WireTopologyEvent::Leave { source_offset: 18 },
        epm::WireTopologyEvent::Leave { source_offset: 17 },
        epm::WireTopologyEvent::Leave { source_offset: 19 },
    ]
}

/// The Map value span selected by the value table: the empty body after the
/// length prefix at offset 38.
pub open spec fn act_forward_witness_value_span() -> epm::ByteSpan {
    epm::ByteSpan { start: 39, end: 39 }
}

/// The attached events: the raw stream with the Map value body at the line's
/// final (value-bearing) Value.
pub open spec fn act_forward_witness_attached_events() -> Seq<epm::WireTopologyEvent> {
    seq![
        epm::WireTopologyEvent::Value { has_value: false, value_body: None, source_offset: 19 },
        epm::WireTopologyEvent::Edge { source_offset: 17, origin: epm::EdgeOrigin::Direct },
        epm::WireTopologyEvent::Value { has_value: false, value_body: None, source_offset: 17 },
        epm::WireTopologyEvent::Edge { source_offset: 18, origin: epm::EdgeOrigin::Direct },
        epm::WireTopologyEvent::Value {
            has_value: true, value_body: Some(act_forward_witness_value_span()), source_offset: 19,
        },
        epm::WireTopologyEvent::Leave { source_offset: 18 },
        epm::WireTopologyEvent::Leave { source_offset: 17 },
        epm::WireTopologyEvent::Leave { source_offset: 19 },
    ]
}

pub open spec fn act_forward_witness_attached() -> epm::AttachedActEvents {
    epm::AttachedActEvents {
        events: act_forward_witness_attached_events(),
        next_value_offset: 39,
        map_values_seen: 1,
    }
}

/// A canonical single-group varint decodes to its own byte value.
pub proof fn act_forward_witness_single_byte_varint(bytes: Seq<u8>, at: usize, byte: u8)
    requires
        (at as int) < bytes.len(),
        (at as int) < usize::MAX,
        bytes[at as int] == byte,
        byte < 128,
    ensures
        epm::canonical_varint_at(bytes, at)
            == Some(epm::CanonicalVarintImage { value: byte as u64, end: (at as int + 1) as usize }),
{
    reveal_with_fuel(epm::canonical_varint_tail, 2);
    assert(bytes[at as int] as int == byte as int);
    assert(byte as int % 128 == byte as int);
    assert(epm::power_of_two((7 * 0int) as nat) == 1);
}

pub proof fn act_forward_witness_encoded_prefix()
    ensures ({
        let before = act_forward_witness_before();
        &&& act_forward_witness_bytes().len() == 39
        &&& epm::indexed_image_wf(before.snapshot)
        &&& epm::indexed_backing_project(before.backing) == Some(before.snapshot)
        &&& before.snapshot.bytes == act_forward_witness_bytes()
        &&& epm::epm_prefix_from_bytes(before.snapshot.bytes)
            == Some(epm::EpmPrefixImage {
                mode: epm::EpmMode::Map,
                trie_start: 7,
                trie_end: 37,
                value_count: 1,
                value_table_start: 38,
            })
    }),
{
    let before = act_forward_witness_before();
    let bytes = act_forward_witness_bytes();
    let snapshot = act_forward_witness_snapshot();
    let projected = epm::indexed_backing_project(act_forward_witness_backing());
    assert(bytes.len() == 39);
    assert(projected.is_some());
    assert(projected.unwrap().bytes =~= snapshot.bytes);
    assert(projected.unwrap().positions =~= snapshot.positions);
    assert(projected.unwrap() == snapshot);
    assert(epm::indexed_backing_project(before.backing) == Some(before.snapshot));
    assert(epm::indexed_image_wf(before.snapshot));
    assert(bytes[6] == 30);
    assert(bytes[37] == 1);
    act_forward_witness_single_byte_varint(bytes, 6, 30);
    act_forward_witness_single_byte_varint(bytes, 37, 1);
    assert(epm::canonical_varint_at(bytes, 6)
        == Some(epm::CanonicalVarintImage { value: 30, end: 7 }));
    assert(epm::canonical_varint_at(bytes, 37)
        == Some(epm::CanonicalVarintImage { value: 1, end: 38 }));
    assert(bytes[5] == 2);
    assert(epm::epm_mode_byte(bytes[5]) == Some(epm::EpmMode::Map));
    assert(epm::epm_prefix_from_bytes(bytes) == Some(epm::EpmPrefixImage {
        mode: epm::EpmMode::Map,
        trie_start: 7,
        trie_end: 37,
        value_count: 1,
        value_table_start: 38,
    }));
}

/// The snapshot's `[7..37]` view is the literal trie.
pub proof fn act_forward_witness_trie_view()
    ensures
        act_forward_witness_bytes().subrange(7, 37) == act_forward_witness_trie(),
        act_forward_witness_trie().len() == 30,
{
    let bytes = act_forward_witness_bytes();
    let trie = act_forward_witness_trie();
    assert(bytes.len() == 39);
    assert(trie.len() == 30);
    assert(bytes.subrange(7, 37) =~= trie);
}

pub proof fn act_forward_witness_act_magic()
    ensures epm::act_magic_and_trailer(act_forward_witness_trie()),
{
    let trie = act_forward_witness_trie();
    assert(trie.len() == 30);
    assert(trie[0] == 0x41);
    assert(trie[1] == 0x43);
    assert(trie[2] == 0x54);
    assert(trie[3] == 0x72);
    assert(trie[4] == 0x65);
    assert(trie[5] == 0x65);
    assert(trie[6] == 0x30);
    assert(trie[7] == 0x33);
    assert forall|i: int| trie.len() - 8 <= i < trie.len() implies trie[i] == 0 by {
        assert(22 <= i < 30);
    }
}

pub open spec fn act_forward_witness_root_semantic_image() -> epm::ActSemanticImage {
    epm::ActSemanticImage {
        value: Some(0),
        value_source_offset: 19,
        line_start: 17,
        line_len: 2,
        line_encoded_start: 16,
        wrapper_id: None,
        wrapper_len: 0,
        first_child: None,
        edges: Seq::empty(),
        encoded_len: 3,
    }
}

pub proof fn act_forward_witness_root_semantic()
    ensures
        epm::act_semantic_at(act_forward_witness_trie(), 19)
            == Some(act_forward_witness_root_semantic_image()),
{
    let trie = act_forward_witness_trie();
    assert(trie.len() == 30);
    assert(trie[19] == 0xc0);
    assert(trie[20] == 0);
    assert(trie[21] == 3);
    assert(trie[16] == 2);
    assert(epm::act_value_at(trie, 19) == Some((Some(0u64), 21usize)));
    assert(epm::act_varint_at(trie, 21)
        == Some(epm::CanonicalVarintImage { value: 3, end: 22 }));
    assert(epm::act_varint_at(trie, 16)
        == Some(epm::CanonicalVarintImage { value: 2, end: 17 }));
}

/// The root id stored in the trie header names the only record's node.
pub proof fn act_forward_witness_root_id()
    ensures epm::little_endian_uint(act_forward_witness_trie(), 8, 8) == 19,
{
    let trie = act_forward_witness_trie();
    assert(trie.len() == 30);
    assert(trie[8] == 19);
    assert(trie[9] == 0);
    assert(trie[10] == 0);
    assert(trie[11] == 0);
    assert(trie[12] == 0);
    assert(trie[13] == 0);
    assert(trie[14] == 0);
    assert(trie[15] == 0);
    reveal_with_fuel(epm::little_endian_uint, 9);
    assert(epm::little_endian_uint(trie, 8, 8) == 19);
}

/// The plan's only record agrees with the decoded root node of the trie.
pub proof fn act_forward_witness_record_matches()
    ensures epm::act_record_matches(
        act_forward_witness_trie(), act_forward_witness_plan().records, 0,
    ),
{
    act_forward_witness_root_semantic();
    let trie = act_forward_witness_trie();
    let plan = act_forward_witness_plan();
    assert(trie.len() == 30);
    assert(plan.records.len() == 1);
    assert(plan.records[0].node_id == 19);
    assert(plan.records[0].children.len() == 0);
    assert(act_forward_witness_root_semantic_image().edges.len() == 0);
    assert(epm::act_record_matches(trie, plan.records, 0));
}

/// Replaying the producer over the single record appends its line at offset
/// 16 and leaves the position at the root node, just before the trailer.
pub proof fn act_forward_witness_producer_matches()
    ensures epm::act_producer_matches_frame(
        act_forward_witness_trie(), act_forward_witness_plan(),
    ),
{
    act_forward_witness_root_semantic();
    let trie = act_forward_witness_trie();
    let plan = act_forward_witness_plan();
    let initial = epm::ActProducerState {
        position: 16, lines: Seq::empty(), line_hash: plan.line_hash,
    };
    let suffix = trie.subrange(17, 19);
    let line = epm::ActProducerLine { encoded_start: 16, suffix };
    let next = epm::ActProducerState {
        position: 19, lines: initial.lines.push(line), line_hash: plan.line_hash,
    };
    assert(trie.len() == 30);
    assert(plan.records.len() == 1);
    assert(plan.finish_order.len() == 1);
    assert(plan.finish_order[0] == 0usize);
    assert(epm::act_finish_order_wf(plan));
    assert(epm::latest_hashed_act_line(initial.lines, suffix, plan.line_hash).is_none());
    assert(epm::act_producer_record_step(trie, plan.records, 0, initial) == Some(next));
    assert(epm::act_producer_replay(trie, plan, 0, initial) == Some(next)) by {
        reveal_with_fuel(epm::act_producer_replay, 2);
    }
    assert(epm::act_producer_matches_frame(trie, plan));
}

pub proof fn act_forward_witness_encoded_act_plan()
    ensures
        epm::act_plan_matches_frame(act_forward_witness_before(), act_forward_witness_plan()),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_trie_view();
    act_forward_witness_act_magic();
    act_forward_witness_root_id();
    act_forward_witness_record_matches();
    act_forward_witness_producer_matches();
    let before = act_forward_witness_before();
    let bytes = act_forward_witness_bytes();
    let trie = act_forward_witness_trie();
    let plan = act_forward_witness_plan();
    assert(before.snapshot.bytes == bytes);
    assert(before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int) == trie);
    assert(plan.records.len() == 1);
    assert forall|i: int| 0 <= i < plan.records.len()
        implies epm::act_record_matches(trie, plan.records, i) by {
        assert(i == 0);
    }
    assert(epm::act_plan_matches_frame(before, plan));
}

pub proof fn act_forward_witness_validated_plan_matches()
    ensures epm::act_plan_projects_validated(
        act_forward_witness_before(),
        act_forward_witness_plan(),
        act_forward_witness_validated_plan(),
    ),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_trie_view();
    let before = act_forward_witness_before();
    let plan = act_forward_witness_plan();
    let validated = act_forward_witness_validated_plan();
    let trie = act_forward_witness_trie();
    reveal_with_fuel(epm::act_value_count, 2);
    assert(epm::act_value_count(plan.records, 0) == 1);
    assert(before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int) == trie);
    assert(epm::act_record_projects_validated(trie, plan.records[0], validated.records[0]));
    assert forall|i: int| 0 <= i < plan.records.len() implies
        epm::act_record_projects_validated(
            before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int),
            plan.records[i], validated.records[i],
        ) by {
        assert(i == 0);
    }
    assert(epm::act_plan_projects_validated(before, plan, validated));
}

/// Expanding the plan's root work from `Enter(0)` yields the raw event stream.
pub proof fn act_forward_witness_raw_expansion()
    ensures ({
        let plan = act_forward_witness_plan();
        epm::act_expand_work(
            plan, seq![epm::ActWorkItem::Enter(0)], epm::act_work_budget(plan, 0) + 1,
        ) == Some(act_forward_witness_raw_events())
    }),
{
    let plan = act_forward_witness_plan();
    let raw = act_forward_witness_raw_events();
    reveal_with_fuel(epm::act_line_forward_work, 3);
    reveal_with_fuel(epm::act_line_leave_work, 3);
    reveal_with_fuel(epm::act_work_budget, 2);
    reveal_with_fuel(epm::act_expand_work, 10);
    assert(epm::act_record_work(plan, 0).len() == 8);
    assert(epm::act_expand_work(
        plan, seq![epm::ActWorkItem::Enter(0)], epm::act_work_budget(plan, 0) + 1,
    ) == Some(raw));
}

/// Attaching the Map value table binds the empty body to the final line Value.
pub proof fn act_forward_witness_attachment()
    ensures epm::attach_act_value_bodies(
        act_forward_witness_bytes(), epm::EpmMode::Map, act_forward_witness_raw_events(), 38,
    ) == Some(act_forward_witness_attached()),
{
    let bytes = act_forward_witness_bytes();
    assert(bytes.len() == 39);
    assert(bytes[38] == 0);
    act_forward_witness_single_byte_varint(bytes, 38, 0);
    assert(epm::map_value_body_at(bytes, 38) == Some(act_forward_witness_value_span()));
    reveal_with_fuel(epm::attach_act_value_bodies, 9);
    let attached_option = epm::attach_act_value_bodies(
        bytes, epm::EpmMode::Map, act_forward_witness_raw_events(), 38,
    );
    assert(attached_option.is_some());
    assert(attached_option.unwrap().next_value_offset == 39);
    assert(attached_option.unwrap().map_values_seen == 1);
    assert(attached_option.unwrap().events =~= act_forward_witness_attached_events());
    assert(attached_option == Some(act_forward_witness_attached()));
}

pub proof fn act_forward_witness_attached_events_valid()
    ensures forall|i: int| 0 <= i < act_forward_witness_attached_events().len() ==>
        epm::act_event_source_valid(
            act_forward_witness_trie(), #[trigger] act_forward_witness_attached_events()[i],
        ),
{
    let trie = act_forward_witness_trie();
    let events = act_forward_witness_attached_events();
    assert(trie.len() == 30);
    assert(events.len() == 8);
    assert forall|i: int| 0 <= i < events.len() implies
        epm::act_event_source_valid(trie, events[i]) by {
        assert(0 <= i < 8);
    }
}

pub proof fn act_forward_witness_expanded_act_events()
    ensures epm::act_plan_events(act_forward_witness_before(), act_forward_witness_plan())
        == Some(act_forward_witness_attached_events()),
{
    act_forward_witness_encoded_act_plan();
    act_forward_witness_raw_expansion();
    act_forward_witness_attachment();
    act_forward_witness_attached_events_valid();
    act_forward_witness_encoded_prefix();
    act_forward_witness_trie_view();
    let before = act_forward_witness_before();
    let bytes = act_forward_witness_bytes();
    let trie = act_forward_witness_trie();
    let plan = act_forward_witness_plan();
    let events = act_forward_witness_attached_events();
    assert(before.snapshot.bytes == bytes);
    assert(before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int) == trie);
    assert forall|i: int| 0 <= i < events.len() implies epm::act_event_source_valid(
        before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int),
        events[i],
    ) by {
        assert(epm::act_event_source_valid(trie, events[i]));
    }
    assert(epm::act_plan_events(before, plan) == Some(events));
}

/// The witness frame is layout-well-formed with eight cursor events, and the
/// trie's first line byte sits at snapshot offset 24.
pub proof fn act_forward_witness_concrete_act_layout()
    ensures ({
        let before = act_forward_witness_before();
        &&& epm::frame_layout_wf(before)
        &&& epm::act_plan_events(before, act_forward_witness_plan())
            == Some(act_forward_witness_attached_events())
        &&& epm::cursor_event_count(before) == 8usize
        &&& epm::trie_position(before, 17) == act_forward_witness_anchor(24)
    }),
{
    let before = act_forward_witness_before();
    let plan = act_forward_witness_plan();
    act_forward_witness_encoded_prefix();
    act_forward_witness_encoded_act_plan();
    act_forward_witness_validated_plan_matches();
    act_forward_witness_expanded_act_events();
    let events = act_forward_witness_attached_events();
    assert(events.len() == 8);
    assert(epm::cursor_event_count(before) == 8usize);
    assert(before.snapshot.positions.len() == 39);
    assert(before.snapshot.positions[24] == act_forward_witness_anchor(24));
    assert(epm::trie_position(before, 17) == act_forward_witness_anchor(24));
    assert(epm::frame_layout_wf(before));
}

pub open spec fn act_forward_witness_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 1,
        physical_start: 0,
        program_type_id: 0,
        region_start: 0,
        region_end: 39,
        source_bytes: act_forward_witness_bytes(),
        cursor: 38,
        slot_count: 0,
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
        frame_kind: 3,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn act_forward_witness_admission() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![act_forward_witness_frame()],
        arena: Seq::empty(),
        root_end: 39,
        pushed: 1,
        completed: 0,
        next_obligation_id: 0,
    }
}

pub open spec fn act_forward_witness_product() -> ProductScoreState {
    let admission = act_forward_witness_admission();
    let transcript = score::empty_arena();
    ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge { admission, transcript },
        map: map::IntegratedAdmissionState {
            admission, map_frames: seq![None], transcript,
        },
        epm_frames: seq![Some(act_forward_witness_before())],
        indexed_regions: seq![act_forward_witness_snapshot()],
        accepted_epm_sources: seq![None],
    }
}

pub open spec fn act_forward_witness_state() -> LiveRuntimeProductState {
    let product = act_forward_witness_product();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime::RuntimeFrameImage {
                score: product.bridge,
                capacity: 1,
                frame_bytes: 1,
                max_frames: 1,
                max_storage_bytes: 1,
                max_capacity_bytes: 1,
            },
            arena: arena::RuntimeScoreArena { entries: Seq::empty() },
            wire: seq![None],
        },
        cursors: seq![None],
    }
}

pub proof fn act_forward_witness_admission_is_wf()
    ensures
        admission::admission_state_wf(act_forward_witness_admission()),
        act_forward_witness_admission().frames.len() == 1,
{
    act_forward_witness_encoded_prefix();
    let frame = act_forward_witness_frame();
    let state = act_forward_witness_admission();
    assert(frame.source_bytes.len() == 39);
    assert(admission::admission_frame_wf(frame));
    assert forall|index: int| 0 <= index < state.frames.len()
        implies admission::admission_frame_wf(#[trigger] state.frames[index]) by {
        assert(index == 0);
    }
    assert(admission::admission_state_wf(state));
}

pub proof fn act_forward_witness_product_is_wf()
    ensures integrated::product_score_wf(act_forward_witness_product()),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_concrete_act_layout();
    score::empty_arena_well_formed();
    act_forward_witness_admission_is_wf();
    let product = act_forward_witness_product();
    let frame = act_forward_witness_frame();
    let snapshot = act_forward_witness_snapshot();
    let before = act_forward_witness_before();
    assert(before.snapshot == snapshot);
    assert(snapshot.bytes == act_forward_witness_bytes());
    assert(epm::indexed_image_wf(snapshot));
    assert(snapshot.positions[0] == act_forward_witness_anchor(0));
    assert(integrated::product_indexed_start_anchor(snapshot) == act_forward_witness_anchor(0));
    assert(map::integrated_admission_wf(product.map)) by {
        assert forall|index: int| 0 <= index < product.map.map_frames.len() implies {
            match #[trigger] product.map.map_frames[index] {
                None => product.map.admission.frames[index].frame_kind != 2,
                Some(map_frame) => product.map.admission.frames[index].frame_kind == 2
                    && index > 0
                    && product.map.admission.frames[index - 1].family
                        == admission::AdmissionFamily::New
                    && product.map.admission.frames[index - 1].program_type_id
                        == map_frame.owner_type_id
                    && product.map.admission.frames[index - 1].waiting_slot == Some(3)
                    && product.map.admission.frames[index - 1].waiting_tag
                        == Some(map_frame.outer_field)
                    && product.map.admission.frames[index - 1].waiting_kind == 2
                    && product.map.admission.frames[index - 1].waiting_id
                        == product.map.admission.frames[index].return_id
                    && product.map.admission.frames[index].return_id.is_some()
                    && product.map.admission.frames[index].body_id as int
                        == product.map.admission.frames[index].return_id.unwrap() as int + 1
                    && map::map_frame_wf(map_frame)
                    && map_frame.accepted_arena.len() <= product.map.admission.arena.len()
                    && map_frame.accepted_arena == product.map.admission.arena.subrange(
                        0, map_frame.accepted_arena.len() as int,
                    )
                    && map::map_aux_projection(map_frame, product.map.admission.frames[index]),
            }
        } by {
            assert(index == 0);
            assert(product.map.map_frames[0] == None::<map::MapFrameModel>);
            assert(product.map.admission.frames[0] == frame);
        }
    }
    assert(integrated::product_epm_source_receipts_wf(product)) by {
        assert forall|i: int| 0 <= i < product.accepted_epm_sources.len()
            implies match #[trigger] product.accepted_epm_sources[i] {
                None => true,
                Some(source) => integrated::product_epm_source_receipt_matches(
                    product.bridge.admission.frames[i], product.indexed_regions[i], source,
                ),
            } by {
            assert(i == 0);
            assert(product.accepted_epm_sources[0] == None::<epm::LinkedEpmState>);
        }
    }
    assert(integrated::product_frame_source_anchors_match(product)) by {
        assert forall|i: int| 0 <= i < product.indexed_regions.len() implies {
            &&& epm::indexed_image_wf(product.indexed_regions[i])
            &&& product.bridge.admission.frames[i].source_bytes == product.indexed_regions[i].bytes
            &&& product.bridge.admission.frames[i].physical_start
                == integrated::product_indexed_start_anchor(product.indexed_regions[i]).offset
            &&& product.bridge.admission.frames[i].physical_source
                == integrated::product_indexed_start_anchor(product.indexed_regions[i]).source
        } by {
            assert(i == 0);
            assert(product.indexed_regions[0] == snapshot);
            assert(product.bridge.admission.frames[0] == frame);
        }
    }
    assert forall|i: int| 0 <= i < product.epm_frames.len() implies {
        &&& epm::indexed_image_wf(product.indexed_regions[i])
        &&& product.indexed_regions[i].bytes == product.bridge.admission.frames[i].source_bytes
        &&& match product.epm_frames[i] {
            None => true,
            Some(epm_frame) => {
                &&& product.bridge.admission.frames[i].frame_kind == 3
                &&& epm_frame.snapshot == product.indexed_regions[i]
                &&& epm::frame_layout_wf(epm_frame)
            },
        }
    } by {
        assert(i == 0);
        assert(product.indexed_regions[0] == snapshot);
        assert(product.bridge.admission.frames[0] == frame);
        assert(product.epm_frames[0] == Some(before));
    }
    integrated::product_score_wf_intro(product);
}

pub proof fn act_forward_witness_state_is_wf()
    ensures integrated::live_runtime_product_wf(act_forward_witness_state()),
{
    act_forward_witness_product_is_wf();
    act_forward_witness_admission_is_wf();
    score::empty_arena_well_formed();
    let state = act_forward_witness_state();
    let product = state.source.product;
    assert(product.bridge.admission.frames.len() == 1);
    assert(product.bridge.admission.frames[0] == act_forward_witness_frame());
    assert(runtime::runtime_frame_image_wf(state.source.runtime));
    assert(arena::runtime_projects_score(state.source.arena, product.bridge.transcript));
    assert forall|i: int| 0 <= i < state.source.wire.len() implies {
        let frame = product.bridge.admission.frames[i];
        match #[trigger] state.source.wire[i] {
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
        assert(i == 0);
    }
    assert(integrated::live_source_product_wf(state.source));
    assert forall|i: int| 0 <= i < state.cursors.len() implies match #[trigger] state.cursors[i] {
        None => true,
        Some(cursor) => integrated::product_wire_indexed_source_agrees(
            cursor, state.source.product.indexed_regions[i],
        ),
    } by {
        assert(i == 0);
        assert(state.cursors[0] == None::<wire::WireRuntimeCursorImage>);
    }
    assert forall|i: int| 0 <= i < state.cursors.len() implies {
        match (#[trigger] state.cursors[i], state.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire::wire_concrete_cursor_wf(wire::wire_runtime_derive_cursor(cursor).unwrap())
                &&& wire::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(i == 0);
        assert(state.cursors[0] == None::<wire::WireRuntimeCursorImage>);
        assert(state.source.wire[0] == None::<wire::WireSourceState>);
    }
    assert(integrated::live_runtime_product_wf(state));
}

/// The Rust cursor position at the first Forward edge: the single record's
/// line is entered and no line byte has been consumed.
pub open spec fn act_forward_witness_position() -> act_source::LineCursorSourcePosition {
    act_source::LineCursorSourcePosition {
        record: 0,
        node_id: 19,
        next_record: 1,
        stack_depth: 1,
        remaining_children: 0,
        stage: act_source::LineStage::LineEdge,
        line_start: Some(17),
        line_end: Some(19),
        line_index: 0,
        leave_index: 0,
    }
}

/// The bounded offer: the interior line bytes only, here byte 17, because the
/// final byte carries the value-bearing Value event.
pub open spec fn act_forward_witness_work() -> active::ActiveLineWorkSource {
    active::ActiveLineWorkSource {
        position: act_forward_witness_position(),
        event_index: 1,
        direction: act_source::LineDirection::Forward,
        start: 17,
        end: 18,
        remaining_work_events: Some(2),
        admissible_source_width: 1,
        admissible_work_events: 2,
    }
}

pub open spec fn act_forward_witness_commit() -> act_source::AcceptedLineCommitSource {
    act_source::AcceptedLineCommitSource {
        direction: act_source::LineDirection::Forward,
        start: 17,
        end: 18,
        expanded_events: 2,
        last_source: act_forward_witness_anchor(24),
        before: act_forward_witness_position(),
        after: active::position_after_commit(
            act_forward_witness_position(), act_source::LineDirection::Forward, 1,
        ),
        indexed_positions: act_forward_witness_snapshot().positions.subrange(7, 37),
    }
}

/// The accepted plan's borrowed source view, over the frame's trie bytes.
pub open spec fn act_forward_witness_plan_source() -> act_source::AcceptedActPlanSourceImage {
    let snapshot = act_forward_witness_snapshot();
    act_source::AcceptedActPlanSourceImage {
        indexed: epm::IndexedRegionImage {
            bytes: snapshot.bytes.subrange(7, 37),
            positions: snapshot.positions.subrange(7, 37),
            end: snapshot.positions[37],
        },
        records: act_forward_witness_validated_plan().records,
        empty: false,
        topology_event_count: 8,
        value_count: 1,
        first_nonzero: None,
        first_nonsequential: None,
    }
}

/// The live DFS work after the line's Forward events: its leaves and the
/// record's final Leave.
pub open spec fn act_forward_witness_continuation() -> Seq<epm::ActWorkItem> {
    seq![
        epm::ActWorkItem::Emit(epm::WireTopologyEvent::Leave { source_offset: 18 }),
        epm::ActWorkItem::Emit(epm::WireTopologyEvent::Leave { source_offset: 17 }),
        epm::ActWorkItem::Emit(epm::WireTopologyEvent::Leave { source_offset: 19 }),
    ]
}

pub open spec fn act_forward_witness_facts() -> restore::ForwardCapacityFacts {
    restore::ForwardCapacityFacts {
        source_present: true,
        root_left: false,
        state_open: true,
        fault_free: true,
        stamp: 0,
        owner_boundary_remaining: 1,
        edges_before: 0,
    }
}

pub open spec fn act_forward_witness_placement() -> act_source::ActLineWorkPlacement {
    let plan = act_forward_witness_plan();
    let record = plan.records[0];
    let raw = act_forward_witness_raw_events();
    let continuation = act_forward_witness_continuation();
    act_source::ActLineWorkPlacement {
        plan,
        record,
        prefix: raw.subrange(0, 1),
        continuation,
        active_work: epm::act_line_forward_work(record, 0) + continuation,
        suffix_events: raw.subrange(1, 8),
        raw_events: raw,
        attached: act_forward_witness_attached(),
        fuel: 7,
    }
}

/// The active work (the line's Forward events, then the continuation) expands
/// to exactly the raw events after the cursor.
pub proof fn act_forward_witness_suffix_expansion()
    ensures ({
        let plan = act_forward_witness_plan();
        epm::act_expand_work(
            plan,
            epm::act_line_forward_work(plan.records[0], 0) + act_forward_witness_continuation(),
            7,
        ) == Some(act_forward_witness_raw_events().subrange(1, 8))
    }),
{
    assert({
        let plan = act_forward_witness_plan();
        epm::act_expand_work(
            plan,
            epm::act_line_forward_work(plan.records[0], 0) + act_forward_witness_continuation(),
            7,
        ) == Some(act_forward_witness_raw_events().subrange(1, 8))
    }) by (compute_only);
}

/// The plan source's record, counts and first-violation folds agree with the
/// bytes of the frame's trie.
pub proof fn act_forward_witness_source_records_match_snapshot()
    ensures act_source::accepted_source_records_match_snapshot(
        act_forward_witness_before(), act_forward_witness_plan_source(),
    ),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_trie_view();
    act_forward_witness_act_magic();
    act_forward_witness_root_semantic();
    let before = act_forward_witness_before();
    let source = act_forward_witness_plan_source();
    let trie = act_forward_witness_trie();
    let bytes = before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int);
    assert(bytes == trie);
    reveal_with_fuel(act_source::source_topology_event_count, 2);
    reveal_with_fuel(act_source::source_value_count, 2);
    reveal_with_fuel(act_source::source_first_nonzero, 2);
    reveal_with_fuel(act_source::source_first_nonsequential, 2);
    reveal_with_fuel(epm::little_endian_uint, 9);
    assert(source.records.len() == 1);
    assert(epm::little_endian_uint(trie, 8, 8) == 19);
    assert(act_source::source_topology_event_count(source.records, 0) == 8);
    assert(act_source::source_value_count(source.records, 0) == 1);
    assert(act_source::source_first_nonzero(source.records, 0) == None::<(usize, u64)>);
    assert(act_source::source_first_nonsequential(source.records, 0, 0)
        == None::<(usize, u64, u64)>);
    assert(act_source::source_record_matches_decoded_act(trie, source.records[0]));
    assert forall|i: int| 0 <= i < source.records.len()
        implies act_source::source_record_matches_decoded_act(bytes, source.records[i]) by {
        assert(i == 0);
    }
    assert(act_source::accepted_source_records_match_snapshot(before, source));
}

/// Replaying the single leaf record closes its frame at ordinal 1, so the
/// formal plan's structure is exactly the source's validation result.
pub proof fn act_forward_witness_source_structure()
    ensures act_source::source_structure_projects_formal_plan(
        act_forward_witness_plan_source(), act_forward_witness_plan(),
    ),
{
    let source = act_forward_witness_plan_source();
    let plan = act_forward_witness_plan();
    reveal_with_fuel(act_source::replay_source_validation_preorder, 2);
    reveal_with_fuel(act_source::close_completed_source_frames, 2);
    assert(source.records.len() == 1);
    assert(source.records[0].child_count == 0);
    let result = act_source::source_validation_structure(source.records);
    assert(result.is_some());
    let structure = result.unwrap();
    assert(structure.finish_order =~= seq![0usize]);
    assert(structure.subtree_ends[0] == 1);
    assert(structure.children[0] =~= Seq::<usize>::empty());
    assert(plan.records.len() == 1);
    assert forall|i: int| 0 <= i < plan.records.len() implies {
        &&& plan.records[i].children == structure.children[i]
        &&& plan.records[i].subtree_end == structure.subtree_ends[i]
    } by {
        assert(i == 0);
    }
    assert(plan.finish_order == structure.finish_order);
    assert(act_source::source_structure_projects_formal_plan(source, plan));
}

/// The accepted plan source projects the concrete frame: indexed view, record
/// agreement, declared count and structure.
pub proof fn act_forward_witness_plan_source_projects_frame()
    ensures act_source::accepted_act_structure_projects_frame(
        act_forward_witness_before(), act_forward_witness_plan_source(),
    ),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_source_records_match_snapshot();
    act_forward_witness_source_structure();
    let before = act_forward_witness_before();
    let source = act_forward_witness_plan_source();
    assert(before.snapshot.bytes.len() == 39);
    assert(before.snapshot.positions.len() == 39);
    assert(source.indexed.bytes.len() == 30);
    assert(source.indexed.positions.len() == 30);
    assert(epm::indexed_image_wf(source.indexed));
    assert(before.cursor.validated_plan == Some(act_source::validated_image_from_source(source)));
    assert(act_source::accepted_act_plan_source_image(before, source));
    assert(act_source::accepted_act_structure_projects_frame(before, source));
}

/// The Forward offer's shape and its prepared commit, in source fields.
pub proof fn act_forward_witness_work_is_prepared()
    ensures
        active::active_line_work_source_shape(act_forward_witness_work()),
        active::prepared_commit_source_projection(
            act_forward_witness_work(), 1, act_forward_witness_commit(),
        ),
        act_source::commit_matches_source_positions(act_forward_witness_commit()),
{
    let work = act_forward_witness_work();
    let commit = act_forward_witness_commit();
    assert(active::offered_width(work) == 1);
    assert(active::event_factor(work.direction) == 2);
    assert(active::active_line_work_source_shape(work));
    assert(commit.after == active::position_after_commit(work.position, work.direction, 1));
    assert(active::prepared_commit_source_projection(work, 1, commit));
    assert(act_source::commit_matches_source_positions(commit));
}

/// The frame derives exactly the witness placement from the commit and the
/// continuation.
pub proof fn act_forward_witness_derived_placement()
    ensures act::derived_line_placement(
        act_forward_witness_before(),
        act_forward_witness_commit(),
        act_forward_witness_continuation(),
        7,
    ) == Some(act_forward_witness_placement()),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_raw_expansion();
    act_forward_witness_attachment();
    let before = act_forward_witness_before();
    let commit = act_forward_witness_commit();
    let plan = act_forward_witness_plan();
    let raw = act_forward_witness_raw_events();
    assert(before.snapshot.bytes == act_forward_witness_bytes());
    assert(before.cursor.act_plan == Some(plan));
    assert(raw.len() == 8);
    assert(plan.records.len() == 1);
    assert(commit.before.record == 0);
    assert(commit.before.line_index == 0);
    assert(act::derived_line_placement(before, commit, act_forward_witness_continuation(), 7)
        == Some(act_forward_witness_placement()));
}

/// The witness placement is a source placement of the commit in the frame.
pub proof fn act_forward_witness_line_work_placement()
    ensures act_source::act_line_work_placement(
        act_forward_witness_before(),
        act_forward_witness_commit(),
        act_forward_witness_placement(),
    ),
{
    act_forward_witness_encoded_prefix();
    act_forward_witness_trie_view();
    act_forward_witness_encoded_act_plan();
    act_forward_witness_raw_expansion();
    act_forward_witness_attachment();
    act_forward_witness_suffix_expansion();
    act_forward_witness_attached_events_valid();
    let before = act_forward_witness_before();
    let commit = act_forward_witness_commit();
    let placement = act_forward_witness_placement();
    let trie = act_forward_witness_trie();
    let plan = placement.plan;
    let raw = placement.raw_events;
    assert(raw.len() == 8);
    assert(raw =~= placement.prefix + placement.suffix_events);
    assert(placement.prefix.len() == 1);
    assert(plan.records.len() == 1);
    assert(placement.record == plan.records[0]);
    assert(placement.attached == act_forward_witness_attached());
    assert(before.snapshot.bytes.len() == 39);
    let frame_trie =
        before.snapshot.bytes.subrange(before.trie_start as int, before.trie_end as int);
    assert(frame_trie == trie);
    assert forall|i: int| 0 <= i < placement.attached.events.len()
        implies epm::act_event_source_valid(frame_trie, placement.attached.events[i]) by {
        assert(epm::act_event_source_valid(trie, act_forward_witness_attached_events()[i]));
    }
    assert(act_source::act_line_work_placement(before, commit, placement));
}

/// The commit is an accepted indexed line batch of the frame: its positions
/// are the frame's trie positions, and the last byte is at snapshot offset 24.
pub proof fn act_forward_witness_indexed_line_source()
    ensures act_source::accepted_indexed_line_source(
        act_forward_witness_before(),
        act_forward_witness_commit(),
        act_forward_witness_placement(),
    ),
{
    act_forward_witness_concrete_act_layout();
    act_forward_witness_work_is_prepared();
    act_forward_witness_line_work_placement();
    let before = act_forward_witness_before();
    let commit = act_forward_witness_commit();
    assert(before.snapshot == act_forward_witness_snapshot());
    assert(before.snapshot.positions.len() == 39);
    assert(before.snapshot.positions[24] == act_forward_witness_anchor(24));
    assert(commit.indexed_positions
        == before.snapshot.positions.subrange(before.trie_start as int, before.trie_end as int));
    assert(act_source::commit_last_offset(commit) == 17);
    assert(commit.indexed_positions[17] == before.snapshot.positions[24]);
    assert(commit.last_source == commit.indexed_positions[act_source::commit_last_offset(commit)]);
    assert(epm::cursor_event_count(before) == 8usize);
    assert(act_source::accepted_indexed_line_source(
        before, commit, act_forward_witness_placement(),
    ));
}

/// One interior byte fits the open two-byte payload, and the frame's capacity
/// facts agree with its path and trie sources.
pub proof fn act_forward_witness_forward_capacity()
    ensures act::forward_capacity_without_event_premise(
        act_forward_witness_before(),
        17,
        1,
        act_forward_witness_pending(),
        7,
        None,
        act_forward_witness_facts(),
    ),
{
    act_forward_witness_concrete_act_layout();
    act_forward_witness_encoded_prefix();
    let before = act_forward_witness_before();
    let parser_image = act_forward_witness_parser();
    let pending = act_forward_witness_pending();
    let facts = act_forward_witness_facts();
    assert(parser::parser_root(parser_image) == parser_image.focus);
    assert(epm::parser_gate_matches(before.path.gate, parser_image));
    assert(parser::parser_forward_capacity(parser_image, 1));
    let base = batch::path_source(before);
    let source = batch::trie_source(before);
    assert(restore::source_tree_wf(base));
    assert(before.snapshot.bytes.len() == 39);
    assert(before.snapshot.positions.len() == 39);
    assert(source.bytes.len() == 30);
    assert(source.positions.len() == 30);
    assert(epm::indexed_image_wf(source));
    assert(restore::forward_capacity_facts_match(base, pending, source, 17, 1, facts));
    assert(act::forward_capacity_without_event_premise(before, 17, 1, pending, 7, None, facts));
}

/// The product's active frame accepts the Forward batch's source facts.
pub proof fn act_forward_witness_forward_source()
    ensures act::accepted_product_forward_source(
        act_forward_witness_product(),
        act_forward_witness_plan_source(),
        act_forward_witness_work(),
        1,
        act_forward_witness_commit(),
        act_forward_witness_continuation(),
        7,
        act_forward_witness_pending(),
        7,
        None,
        act_forward_witness_facts(),
    ),
{
    act_forward_witness_product_is_wf();
    act_forward_witness_plan_source_projects_frame();
    act_forward_witness_work_is_prepared();
    act_forward_witness_derived_placement();
    act_forward_witness_indexed_line_source();
    act_forward_witness_forward_capacity();
    let product = act_forward_witness_product();
    let frame = act_forward_witness_before();
    let work = act_forward_witness_work();
    let commit = act_forward_witness_commit();
    let continuation = act_forward_witness_continuation();
    assert(act::active_product_epm(product) == Some(frame));
    assert(work.direction == act_source::LineDirection::Forward);
    assert(work.event_index == frame.cursor.at);
    assert(commit.start == 17);
    assert(act::accepted_product_forward_source(
        product,
        act_forward_witness_plan_source(),
        work,
        1,
        commit,
        continuation,
        7,
        act_forward_witness_pending(),
        7,
        None,
        act_forward_witness_facts(),
    ));
}

/// The ACT Forward arm is inhabited: a concrete well-formed product at an EPM
/// frame takes one Forward batch of one interior line byte.
pub proof fn act_forward_witness_is_integrated_step()
    ensures ({
        let before = act_forward_witness_state();
        let commit = act_forward_witness_commit();
        let pending = act_forward_witness_pending();
        let action = IntegratedAction::ActForward {
            plan_source: act_forward_witness_plan_source(),
            work: act_forward_witness_work(),
            width: 1,
            commit,
            continuation: act_forward_witness_continuation(),
            fuel: 7,
            pending,
            state_id: 7,
            latest_restore_state_id: None,
            forward_facts: act_forward_witness_facts(),
        };
        &&& integrated_wf(IntegratedState::Active { product: before })
        &&& integrated_step(
            IntegratedState::Active { product: before },
            IntegratedState::Active {
                product: integrated_act_forward_after(before, 1, commit, pending, 7),
            },
            action,
        )
    }),
{
    act_forward_witness_state_is_wf();
    act_forward_witness_forward_source();
    let before = act_forward_witness_state();
    let plan_source = act_forward_witness_plan_source();
    let work = act_forward_witness_work();
    let commit = act_forward_witness_commit();
    let continuation = act_forward_witness_continuation();
    let pending = act_forward_witness_pending();
    let facts = act_forward_witness_facts();
    let product = before.source.product;
    let frames = product.bridge.admission.frames;
    assert(product == act_forward_witness_product());
    assert(frames.len() == 1);
    assert(frames.last() == act_forward_witness_frame());
    assert(frames.last().waiting_id.is_none());
    assert(act::active_product_epm(product) == Some(act_forward_witness_before()));
    assert(frames.last().cursor
        <= act::active_product_epm(product).unwrap().cursor.value_table_offset);
    assert(integrated_act_forward_guard(
        before, plan_source, work, 1, commit, continuation, 7, pending, 7, None, facts,
    ));
    let action = IntegratedAction::ActForward {
        plan_source,
        work,
        width: 1,
        commit,
        continuation,
        fuel: 7,
        pending,
        state_id: 7,
        latest_restore_state_id: None,
        forward_facts: facts,
    };
    assert(integrated_top_frame_kind(before) == 3);
    assert(integrated_kind_guard(before, action));
}

}
