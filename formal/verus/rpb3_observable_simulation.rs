//! Simulation between `rpb3_integrated_dispatcher::integrated_step` and the observable
//! admission machine shared with `formal/verified_score_store`: projection, segments,
//! forward simulation, structural backward simulation, successor agreement and
//! structural coverage.

use vstd::prelude::*;

use crate::{
    admission_observable_spec as observable, rpb3_admission_cursor as admission,
    rpb3_canonical_path_driver_product_integration as path_driver,
    rpb3_epathmap_event_refinement as epm, rpb3_epm_live_handoff as handoff,
    rpb3_epm_unwind_event_lift as unwind, rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_integrated_dispatcher as dispatcher, rpb3_map_frame_refinement as map,
    rpb3_score_admission_bridge as score_bridge, rpb3_wire_cursor_source_refinement as wire_source,
    LiveRuntimeProductAction, LiveRuntimeProductState,
};

verus! {

pub open spec fn project_child(
    summary: admission::AdmissionReturnedSummary,
) -> observable::ObservableChild {
    observable::ObservableChild { slot: summary.slot as nat, body_len: summary.body_bytes.len() }
}

pub open spec fn project_children(
    returns: Seq<admission::AdmissionReturnedSummary>,
) -> Seq<observable::ObservableChild> {
    Seq::new(returns.len(), |index: int| project_child(returns[index]))
}

pub open spec fn project_slot(slot: Option<usize>) -> Option<nat> {
    match slot {
        Some(value) => Some(value as nat),
        None => None,
    }
}

#[verifier::opaque]
pub open spec fn project_frame(
    frames: Seq<admission::AdmissionFrameState>, index: int,
) -> observable::ObservableFrame {
    let frame = frames[index];
    let fields = observable::observable_tracks_fields(frame.frame_kind);
    observable::ObservableFrame {
        kind: frame.frame_kind,
        body_len: frame.region_end as nat,
        cursor: if fields { Some(frame.cursor as nat) } else { None },
        slot_count: if fields { frame.slot_count as nat } else { 0nat },
        last_slot: if fields { project_slot(frame.last_slot) } else { None },
        waiting: if index + 1 < frames.len() {
            Some(observable::ObservableWait {
                child_kind: frames[index + 1].frame_kind,
                slot: if fields { project_slot(frame.waiting_slot) } else { None },
            })
        } else {
            None
        },
        children: if fields {
            project_children(frame.returns)
        } else {
            Seq::<observable::ObservableChild>::empty()
        },
    }
}

pub open spec fn project_frames(
    frames: Seq<admission::AdmissionFrameState>,
) -> Seq<observable::ObservableFrame> {
    Seq::new(frames.len(), |index: int| project_frame(frames, index))
}

pub open spec fn project_active(
    frames: Seq<admission::AdmissionFrameState>, score_len: nat,
) -> observable::ObservableState {
    observable::ObservableState::Active { frames: project_frames(frames), score_len }
}

pub open spec fn project_root(
    frames: Seq<admission::AdmissionFrameState>, score_len: nat,
) -> observable::ObservableState {
    observable::ObservableState::Accepted { root: project_frame(frames, 0), score_len }
}

pub open spec fn live_frames(
    product: LiveRuntimeProductState,
) -> Seq<admission::AdmissionFrameState> {
    product.source.product.bridge.admission.frames
}

pub open spec fn live_score_len(product: LiveRuntimeProductState) -> nat {
    product.source.product.bridge.transcript.entries.len()
}

pub open spec fn accepted_product(
    accepted: dispatcher::IntegratedAccepted,
) -> LiveRuntimeProductState {
    match accepted {
        dispatcher::IntegratedAccepted::LiveRoot { post_root_score, .. } => post_root_score,
        dispatcher::IntegratedAccepted::LiveNoScoreRoot { pre_complete, .. } => pre_complete,
        dispatcher::IntegratedAccepted::CanonicalPathRoot { pre_complete, .. } => pre_complete,
    }
}

pub open spec fn state_product(state: dispatcher::IntegratedState) -> LiveRuntimeProductState {
    match state {
        dispatcher::IntegratedState::Active { product } => product,
        dispatcher::IntegratedState::Accepted { accepted } => accepted_product(accepted),
    }
}

pub open spec fn state_frames(
    state: dispatcher::IntegratedState,
) -> Seq<admission::AdmissionFrameState> {
    live_frames(state_product(state))
}

pub open spec fn state_score_len(state: dispatcher::IntegratedState) -> nat {
    live_score_len(state_product(state))
}

pub open spec fn project(state: dispatcher::IntegratedState) -> observable::ObservableState {
    match state {
        dispatcher::IntegratedState::Active { .. } =>
            project_active(state_frames(state), state_score_len(state)),
        dispatcher::IntegratedState::Accepted { .. } =>
            project_root(state_frames(state), state_score_len(state)),
    }
}

pub open spec fn raw_snapshot(
    frames: Seq<admission::AdmissionFrameState>, score_len: nat,
) -> observable::ObservableSnapshot {
    observable::ObservableSnapshot {
        depth: frames.len(),
        top_kind: if frames.len() == 0 { None } else { Some(frames.last().frame_kind) },
        score_len,
    }
}

pub enum IntegratedArm {
    LiveContinue,
    LiveContinueIndexed,
    LiveMapKey,
    LiveRootScore,
    LiveRootComplete,
    LiveRootCompleteNoScore,
    LiveMessageChildScore,
    LiveMessagePush,
    LiveMessageEpmPush,
    LiveMapValuePush,
    LiveMessageChildComplete,
    LiveMapValueAccept,
    LiveMapEntryFinish,
    LiveEpmMapValueAccept,
    LiveEpmContinue,
    LiveEpmPush,
    LiveEpmEnd,
    LiveEpmFinishAccept,
    PathPush,
    PathChildReturn,
    PathRootComplete,
    ActForward,
    ActUnwind,
    EpmRawParReturn,
    EpmNestedReturn,
    EpmHandoff,
}

pub open spec fn live_action_arm(action: LiveRuntimeProductAction) -> IntegratedArm {
    match action {
        LiveRuntimeProductAction::Continue => IntegratedArm::LiveContinue,
        LiveRuntimeProductAction::ContinueIndexed => IntegratedArm::LiveContinueIndexed,
        LiveRuntimeProductAction::MapKey => IntegratedArm::LiveMapKey,
        LiveRuntimeProductAction::RootScore => IntegratedArm::LiveRootScore,
        LiveRuntimeProductAction::RootComplete { .. } => IntegratedArm::LiveRootComplete,
        LiveRuntimeProductAction::RootCompleteNoScore { .. } =>
            IntegratedArm::LiveRootCompleteNoScore,
        LiveRuntimeProductAction::MessageChildScore => IntegratedArm::LiveMessageChildScore,
        LiveRuntimeProductAction::MessagePush { .. } => IntegratedArm::LiveMessagePush,
        LiveRuntimeProductAction::MessageEpmPush { .. } => IntegratedArm::LiveMessageEpmPush,
        LiveRuntimeProductAction::MapValuePush { .. } => IntegratedArm::LiveMapValuePush,
        LiveRuntimeProductAction::MessageChildComplete => IntegratedArm::LiveMessageChildComplete,
        LiveRuntimeProductAction::MapValueAccept => IntegratedArm::LiveMapValueAccept,
        LiveRuntimeProductAction::MapEntryFinish => IntegratedArm::LiveMapEntryFinish,
        LiveRuntimeProductAction::EpmMapValueAccept { .. } => IntegratedArm::LiveEpmMapValueAccept,
        LiveRuntimeProductAction::EpmContinue { .. } => IntegratedArm::LiveEpmContinue,
        LiveRuntimeProductAction::EpmPush { .. } => IntegratedArm::LiveEpmPush,
        LiveRuntimeProductAction::EpmEnd { .. } => IntegratedArm::LiveEpmEnd,
        LiveRuntimeProductAction::EpmFinishAccept => IntegratedArm::LiveEpmFinishAccept,
    }
}

pub open spec fn action_arm(action: dispatcher::IntegratedAction) -> IntegratedArm {
    match action {
        dispatcher::IntegratedAction::Live { live_action, .. } => live_action_arm(live_action),
        dispatcher::IntegratedAction::CanonicalPath { driver_action, .. } => match driver_action {
            path_driver::CanonicalPathDriverSourceAction::Push { .. } => IntegratedArm::PathPush,
            path_driver::CanonicalPathDriverSourceAction::ChildReturn { .. } =>
                IntegratedArm::PathChildReturn,
            path_driver::CanonicalPathDriverSourceAction::RootComplete { .. } =>
                IntegratedArm::PathRootComplete,
        },
        dispatcher::IntegratedAction::ActForward { .. } => IntegratedArm::ActForward,
        dispatcher::IntegratedAction::ActUnwind { .. } => IntegratedArm::ActUnwind,
        dispatcher::IntegratedAction::EpmRawParReturn => IntegratedArm::EpmRawParReturn,
        dispatcher::IntegratedAction::EpmNestedReturn => IntegratedArm::EpmNestedReturn,
        dispatcher::IntegratedAction::EpmHandoff { .. } => IntegratedArm::EpmHandoff,
    }
}

pub open spec fn segment_internal_arm(arm: IntegratedArm) -> bool {
    arm is LiveEpmContinue || arm is LiveEpmEnd || arm is ActForward || arm is ActUnwind
        || arm is EpmHandoff
}

pub open spec fn segment_score_arm(arm: IntegratedArm) -> bool {
    arm is LiveMessageChildScore
}

pub open spec fn arm_is_field(arm: IntegratedArm) -> bool {
    arm is LiveContinue || arm is LiveContinueIndexed || arm is LiveMapKey
}

pub open spec fn arm_is_push(arm: IntegratedArm) -> bool {
    arm is LiveMessagePush || arm is LiveMessageEpmPush || arm is LiveMapValuePush
        || arm is LiveEpmPush || arm is PathPush
}

pub open spec fn arm_is_accept(arm: IntegratedArm) -> bool {
    arm is LiveMessageChildComplete || arm is LiveMapValueAccept || arm is LiveMapEntryFinish
        || arm is LiveEpmMapValueAccept || arm is LiveEpmFinishAccept || arm is PathChildReturn
        || arm is EpmRawParReturn || arm is EpmNestedReturn
}

pub open spec fn arm_is_root(arm: IntegratedArm) -> bool {
    arm is LiveRootComplete || arm is LiveRootCompleteNoScore || arm is PathRootComplete
}

pub open spec fn accept_class(child_kind: u8) -> observable::ObservableClass {
    if child_kind == 1 {
        observable::ObservableClass::MessageComplete
    } else {
        observable::ObservableClass::OtherComplete
    }
}

pub open spec fn accept_score_rule(child_kind: u8, score_len: nat, score_after: nat) -> bool {
    if child_kind == 1 { score_after >= score_len } else { score_after == score_len }
}

pub open spec fn arm_visible_class(
    arm: IntegratedArm, top_kind: u8,
) -> Option<observable::ObservableClass> {
    match arm {
        IntegratedArm::LiveContinue | IntegratedArm::LiveContinueIndexed =>
            Some(observable::ObservableClass::MessageContinue),
        IntegratedArm::LiveMapKey => Some(observable::ObservableClass::OtherContinue),
        IntegratedArm::LiveMessagePush | IntegratedArm::LiveMessageEpmPush =>
            Some(observable::ObservableClass::MessagePush),
        IntegratedArm::LiveMapValuePush | IntegratedArm::LiveEpmPush | IntegratedArm::PathPush =>
            Some(observable::ObservableClass::OtherPush),
        IntegratedArm::LiveMessageChildComplete | IntegratedArm::LiveMapValueAccept
        | IntegratedArm::LiveEpmMapValueAccept | IntegratedArm::LiveRootComplete
        | IntegratedArm::LiveRootCompleteNoScore | IntegratedArm::EpmRawParReturn =>
            Some(observable::ObservableClass::MessageComplete),
        IntegratedArm::LiveMapEntryFinish | IntegratedArm::LiveEpmFinishAccept
        | IntegratedArm::EpmNestedReturn | IntegratedArm::PathRootComplete =>
            Some(observable::ObservableClass::OtherComplete),
        IntegratedArm::PathChildReturn => Some(accept_class(top_kind)),
        IntegratedArm::LiveRootScore | IntegratedArm::LiveMessageChildScore
        | IntegratedArm::LiveEpmContinue | IntegratedArm::LiveEpmEnd | IntegratedArm::ActForward
        | IntegratedArm::ActUnwind | IntegratedArm::EpmHandoff => None,
    }
}

pub open spec fn visible_class(
    before: dispatcher::IntegratedState, action: dispatcher::IntegratedAction,
) -> Option<observable::ObservableClass> {
    arm_visible_class(action_arm(action), state_frames(before).last().frame_kind)
}

pub open spec fn internal_prefix(actions: Seq<dispatcher::IntegratedAction>, count: int) -> bool {
    forall|index: int| 0 <= index < count
        ==> segment_internal_arm(action_arm(#[trigger] actions[index]))
}

pub open spec fn segment_visible_index(actions: Seq<dispatcher::IntegratedAction>) -> int {
    if actions.len() >= 2 && action_arm(actions.last()) is EpmHandoff {
        actions.len() - 2
    } else {
        actions.len() - 1
    }
}

pub open spec fn integrated_segment(
    states: Seq<dispatcher::IntegratedState>, actions: Seq<dispatcher::IntegratedAction>,
) -> bool {
    let visible = segment_visible_index(actions);
    &&& dispatcher::integrated_trace(states, actions)
    &&& actions.len() > 0
    &&& {
        ||| internal_prefix(actions, actions.len() as int)
        ||| {
            &&& visible_class(states[visible], actions[visible]) is Some
            &&& {
                ||| internal_prefix(actions, visible)
                ||| {
                    &&& visible >= 1
                    &&& internal_prefix(actions, visible - 1)
                    &&& segment_score_arm(action_arm(actions[visible - 1]))
                    &&& visible_class(states[visible], actions[visible])
                        == Some(observable::ObservableClass::MessageComplete)
                }
            }
        }
    }
}

pub open spec fn segment_class(
    states: Seq<dispatcher::IntegratedState>, actions: Seq<dispatcher::IntegratedAction>,
) -> observable::ObservableClass {
    let visible = segment_visible_index(actions);
    match visible_class(states[visible], actions[visible]) {
        Some(class) => class,
        None => observable::ObservableClass::OtherContinue,
    }
}

pub open spec fn segment_image_event(
    states: Seq<dispatcher::IntegratedState>, actions: Seq<dispatcher::IntegratedAction>,
) -> observable::ObservableEvent {
    observable::observable_image_event(
        segment_class(states, actions), project(states[0]), project(states.last()),
    )
}

pub open spec fn step_refines(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    class: observable::ObservableClass,
) -> bool {
    observable::observable_next(
        project(before), observable::observable_image_event(class, project(before), project(after)),
    ) == Some(project(after))
}

pub open spec fn visible_step_refines(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
) -> bool {
    match visible_class(before, action) {
        Some(class) => step_refines(before, after, class),
        None => false,
    }
}

pub open spec fn lower_frames_kept(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    count: int,
) -> bool {
    forall|index: int| 0 <= index < count ==> (#[trigger] after[index]) == before[index]
}

pub open spec fn slot_order_kept(previous: Option<usize>, slot: usize) -> bool {
    match previous {
        Some(last) => slot >= last,
        None => true,
    }
}

pub open spec fn raw_push_kind_allowed(parent_kind: u8, child_kind: u8) -> bool {
    if parent_kind == 1 {
        child_kind == 1 || child_kind == 2 || child_kind == 3
    } else if parent_kind == 2 {
        child_kind == 1
    } else if parent_kind == 3 || parent_kind == 4 {
        child_kind == 1 || child_kind == 3
    } else {
        false
    }
}

pub open spec fn stutter_transition(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
) -> bool {
    &&& before.len() > 0
    &&& after.len() == before.len()
    &&& lower_frames_kept(before, after, before.len() - 1)
    &&& before.last().frame_kind == 3
    &&& after.last().frame_kind == 3
    &&& after.last().region_end == before.last().region_end
}

pub open spec fn score_transition(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
) -> bool {
    &&& before.len() > 0
    &&& after.len() == before.len()
    &&& lower_frames_kept(before, after, before.len() - 1)
    &&& after.last().frame_kind == before.last().frame_kind
    &&& after.last().region_end == before.last().region_end
    &&& after.last().cursor == before.last().cursor
    &&& after.last().last_slot == before.last().last_slot
    &&& after.last().returns == before.last().returns
}

pub open spec fn field_transition(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
) -> bool {
    let top = before.last();
    let moved = after.last();
    &&& before.len() > 0
    &&& after.len() == before.len()
    &&& lower_frames_kept(before, after, before.len() - 1)
    &&& (top.frame_kind == 1 || top.frame_kind == 2)
    &&& moved.frame_kind == top.frame_kind
    &&& moved.region_end == top.region_end
    &&& moved.slot_count == top.slot_count
    &&& moved.returns == top.returns
    &&& top.cursor < moved.cursor
    &&& moved.cursor <= top.region_end
    &&& match moved.last_slot {
        Some(slot) => slot < top.slot_count && slot_order_kept(top.last_slot, slot),
        None => false,
    }
    &&& (top.frame_kind == 2 ==> {
        &&& top.slot_count == 2
        &&& moved.last_slot == Some(0usize)
        &&& top.last_slot is None
    })
}

pub open spec fn push_transition_core(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
) -> bool {
    let top = before.last();
    let parent = after[before.len() - 1];
    let child = after.last();
    &&& before.len() > 0
    &&& after.len() == before.len() + 1
    &&& lower_frames_kept(before, after, before.len() - 1)
    &&& parent.frame_kind == top.frame_kind
    &&& parent.region_end == top.region_end
    &&& raw_push_kind_allowed(top.frame_kind, child.frame_kind)
    &&& ((child.frame_kind == 1 || child.frame_kind == 2) ==> {
        &&& child.cursor == 0
        &&& child.last_slot is None
        &&& child.returns.len() == 0
    })
    &&& ((top.frame_kind == 1 || top.frame_kind == 2) ==> {
        &&& parent.slot_count == top.slot_count
        &&& parent.returns == top.returns
        &&& top.cursor < parent.cursor
        &&& parent.cursor <= top.region_end
        &&& match parent.last_slot {
            Some(slot) => {
                &&& slot < top.slot_count
                &&& slot_order_kept(top.last_slot, slot)
                &&& parent.waiting_slot == Some(slot)
            },
            None => false,
        }
        &&& (top.frame_kind == 2 ==> {
            &&& top.slot_count == 2
            &&& parent.last_slot == Some(1usize)
            &&& top.last_slot != Some(1usize)
        })
    })
}

pub open spec fn push_transition(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
) -> bool {
    &&& push_transition_core(before, after)
    &&& after.last().region_end <= before.last().region_end
}

pub open spec fn accept_transition(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
) -> bool {
    let child = before.last();
    let parent = before[before.len() - 2];
    let resolved = after.last();
    &&& before.len() >= 2
    &&& after.len() == before.len() - 1
    &&& lower_frames_kept(before, after, before.len() - 2)
    &&& (child.frame_kind == 1 || child.frame_kind == 2 || child.frame_kind == 3)
    &&& ((child.frame_kind == 1 || child.frame_kind == 2) ==> child.cursor == child.region_end)
    &&& (parent.frame_kind == 1 || parent.frame_kind == 2 || parent.frame_kind == 3
        || parent.frame_kind == 4)
    &&& resolved.frame_kind == parent.frame_kind
    &&& resolved.region_end == parent.region_end
    &&& ((parent.frame_kind == 1 || parent.frame_kind == 2) ==> {
        &&& resolved.cursor == parent.cursor
        &&& resolved.slot_count == parent.slot_count
        &&& resolved.last_slot == parent.last_slot
        &&& resolved.returns.len() == parent.returns.len() + 1
        &&& resolved.returns.drop_last() == parent.returns
        &&& parent.waiting_slot == Some(resolved.returns.last().slot)
        &&& resolved.returns.last().body_bytes.len() == child.region_end as nat
        &&& (parent.frame_kind == 2 ==> {
            &&& child.frame_kind == 1
            &&& parent.waiting_slot == Some(1usize)
        })
    })
}

pub open spec fn root_transition_core(
    before: Seq<admission::AdmissionFrameState>, accepted: Seq<admission::AdmissionFrameState>,
) -> bool {
    let root = before[0];
    let kept = accepted[0];
    &&& before.len() == 1
    &&& accepted.len() == 1
    &&& (root.frame_kind == 1 || root.frame_kind == 4)
    &&& (root.frame_kind == 1 ==> root.cursor == root.region_end)
    &&& kept.frame_kind == root.frame_kind
    &&& kept.region_end == root.region_end
    &&& (root.frame_kind == 1 ==> {
        &&& kept.cursor == root.cursor
        &&& kept.last_slot == root.last_slot
        &&& kept.returns == root.returns
    })
}

pub open spec fn root_transition(
    before: Seq<admission::AdmissionFrameState>, accepted: Seq<admission::AdmissionFrameState>,
) -> bool {
    &&& root_transition_core(before, accepted)
    &&& (before[0].frame_kind == 1 ==> accepted[0].slot_count == before[0].slot_count)
}

pub open spec fn stutter_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& after is Active
    &&& stutter_transition(state_frames(before), state_frames(after))
    &&& state_score_len(after) == state_score_len(before)
}

pub open spec fn score_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& after is Active
    &&& state_frames(before).len() > 1
    &&& state_frames(before).last().frame_kind == 1
    &&& score_transition(state_frames(before), state_frames(after))
    &&& state_score_len(before) <= state_score_len(after)
}

pub open spec fn field_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& after is Active
    &&& field_transition(state_frames(before), state_frames(after))
    &&& state_score_len(after) == state_score_len(before)
}

pub open spec fn push_core_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& after is Active
    &&& push_transition_core(state_frames(before), state_frames(after))
    &&& state_score_len(after) == state_score_len(before)
}

pub open spec fn push_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& push_core_step(before, after)
    &&& push_transition(state_frames(before), state_frames(after))
}

pub open spec fn accept_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& after is Active
    &&& accept_transition(state_frames(before), state_frames(after))
    &&& state_score_len(after) == state_score_len(before)
}

pub open spec fn root_core_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& after is Accepted
    &&& root_transition_core(state_frames(before), state_frames(after))
    &&& accept_score_rule(
        state_frames(before)[0].frame_kind, state_score_len(before), state_score_len(after),
    )
}

pub open spec fn root_step(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
) -> bool {
    &&& root_core_step(before, after)
    &&& root_transition(state_frames(before), state_frames(after))
}

pub open spec fn raw_class_kind(message: bool, kind: u8) -> bool {
    if message { kind == 1 } else { kind == 2 || kind == 3 || kind == 4 }
}

pub open spec fn raw_field_guard(
    top: admission::AdmissionFrameState, field: observable::ObservableField,
) -> bool {
    &&& (top.frame_kind == 1 || top.frame_kind == 2)
    &&& (top.cursor as nat) < field.cursor_after
    &&& field.cursor_after <= top.region_end as nat
    &&& field.slot < top.slot_count as nat
    &&& admission::slot_admissible(
        top.last_slot, field.slot as usize, top.slot_count, field.repeated,
    )
}

pub open spec fn raw_continue_shape(
    frames: Seq<admission::AdmissionFrameState>, message: bool,
    field: Option<observable::ObservableField>,
) -> bool {
    let top = frames.last();
    &&& frames.len() > 0
    &&& raw_class_kind(message, top.frame_kind)
    &&& if top.frame_kind == 3 {
        field is None
    } else {
        match field {
            Some(moved) => {
                &&& raw_field_guard(top, moved)
                &&& (top.frame_kind == 2 ==> {
                    &&& top.slot_count == 2
                    &&& moved.slot == 0
                    &&& !moved.repeated
                })
            },
            None => false,
        }
    }
}

pub open spec fn raw_push_shape(
    frames: Seq<admission::AdmissionFrameState>, message: bool,
    field: Option<observable::ObservableField>, request: observable::ObservableFrameRequest,
) -> bool {
    let top = frames.last();
    &&& frames.len() > 0
    &&& raw_class_kind(message, top.frame_kind)
    &&& raw_push_kind_allowed(top.frame_kind, request.kind)
    &&& request.body_len <= top.region_end as nat
    &&& if top.frame_kind == 1 || top.frame_kind == 2 {
        match field {
            Some(moved) => {
                &&& raw_field_guard(top, moved)
                &&& (top.frame_kind == 2 ==> {
                    &&& top.slot_count == 2
                    &&& moved.slot == 1
                    &&& !moved.repeated
                })
            },
            None => false,
        }
    } else {
        field is None
    }
}

pub open spec fn raw_complete_shape(
    frames: Seq<admission::AdmissionFrameState>, message: bool,
    record: observable::ObservableAcceptRecord, score_len: nat, score_after: nat,
) -> bool {
    let top = frames.last();
    let parent = frames[frames.len() - 2];
    &&& frames.len() > 0
    &&& raw_class_kind(message, top.frame_kind)
    &&& ((top.frame_kind == 1 || top.frame_kind == 2) ==> top.cursor == top.region_end)
    &&& (if message { score_after >= score_len } else { score_after == score_len })
    &&& match record {
        observable::ObservableAcceptRecord::Root => {
            &&& frames.len() == 1
            &&& (top.frame_kind == 1 || top.frame_kind == 4)
        },
        observable::ObservableAcceptRecord::MessageSlot { slot, .. } => {
            &&& frames.len() >= 2
            &&& parent.frame_kind == 1
            &&& match parent.waiting_slot {
                Some(waiting) => waiting as nat == slot,
                None => false,
            }
        },
        observable::ObservableAcceptRecord::MapValue => {
            &&& frames.len() >= 2
            &&& parent.frame_kind == 2
            &&& top.frame_kind == 1
            &&& parent.waiting_slot == Some(1usize)
        },
        observable::ObservableAcceptRecord::Reduced => {
            &&& frames.len() >= 2
            &&& (parent.frame_kind == 3 || parent.frame_kind == 4)
        },
    }
}

pub open spec fn raw_after_push(
    frames: Seq<admission::AdmissionFrameState>, child_kind: u8, score_len: nat,
) -> observable::ObservableSnapshot {
    observable::ObservableSnapshot {
        depth: frames.len() + 1,
        top_kind: Some(child_kind),
        score_len,
    }
}

pub open spec fn raw_after_complete(
    frames: Seq<admission::AdmissionFrameState>, score_after: nat,
) -> observable::ObservableSnapshot {
    if frames.len() <= 1 {
        observable::ObservableSnapshot { depth: 0, top_kind: None, score_len: score_after }
    } else {
        observable::ObservableSnapshot {
            depth: (frames.len() - 1) as nat,
            top_kind: Some(frames[frames.len() - 2].frame_kind),
            score_len: score_after,
        }
    }
}

pub open spec fn raw_event_shape(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
) -> bool {
    let frames = state_frames(state);
    let score_len = state_score_len(state);
    &&& state is Active
    &&& match event {
        observable::ObservableEvent::MessageContinue { before, field, after } => {
            &&& before == raw_snapshot(frames, score_len)
            &&& raw_continue_shape(frames, true, Some(field))
            &&& after == raw_snapshot(frames, score_len)
        },
        observable::ObservableEvent::OtherContinue { before, field, after } => {
            &&& before == raw_snapshot(frames, score_len)
            &&& raw_continue_shape(frames, false, field)
            &&& after == raw_snapshot(frames, score_len)
        },
        observable::ObservableEvent::MessagePush { before, field, child, after } => {
            &&& before == raw_snapshot(frames, score_len)
            &&& raw_push_shape(frames, true, Some(field), child)
            &&& after == raw_after_push(frames, child.kind, score_len)
        },
        observable::ObservableEvent::OtherPush { before, field, child, after } => {
            &&& before == raw_snapshot(frames, score_len)
            &&& raw_push_shape(frames, false, field, child)
            &&& after == raw_after_push(frames, child.kind, score_len)
        },
        observable::ObservableEvent::MessageComplete { before, record, after } => {
            &&& before == raw_snapshot(frames, score_len)
            &&& raw_complete_shape(frames, true, record, score_len, after.score_len)
            &&& after == raw_after_complete(frames, after.score_len)
        },
        observable::ObservableEvent::OtherComplete { before, record, after } => {
            &&& before == raw_snapshot(frames, score_len)
            &&& raw_complete_shape(frames, false, record, score_len, after.score_len)
            &&& after == raw_after_complete(frames, after.score_len)
        },
    }
}

pub open spec fn canonical_path_top_is_root(state: dispatcher::IntegratedState) -> bool {
    state is Active && state_frames(state).len() >= 2 ==> state_frames(state).last().frame_kind != 4
}

pub open spec fn event_lane(
    frames: Seq<admission::AdmissionFrameState>, event: observable::ObservableEvent,
    arm: IntegratedArm,
) -> bool {
    let top_kind = frames.last().frame_kind;
    let parent_kind = frames[frames.len() - 2].frame_kind;
    match event {
        observable::ObservableEvent::MessageContinue { .. } =>
            arm is LiveContinue || arm is LiveContinueIndexed,
        observable::ObservableEvent::OtherContinue { field, .. } =>
            if field is Some { arm is LiveMapKey } else { segment_internal_arm(arm) },
        observable::ObservableEvent::MessagePush { child, .. } =>
            if child.kind == 3 { arm is LiveMessageEpmPush } else { arm is LiveMessagePush },
        observable::ObservableEvent::OtherPush { .. } =>
            if top_kind == 2 {
                arm is LiveMapValuePush
            } else if top_kind == 3 {
                arm is LiveEpmPush
            } else {
                arm is PathPush
            },
        observable::ObservableEvent::MessageComplete { record, .. } => match record {
            observable::ObservableAcceptRecord::Root =>
                arm is LiveRootComplete || arm is LiveRootCompleteNoScore,
            observable::ObservableAcceptRecord::MessageSlot { .. } =>
                arm is LiveMessageChildComplete,
            observable::ObservableAcceptRecord::MapValue => arm is LiveMapValueAccept,
            observable::ObservableAcceptRecord::Reduced =>
                if parent_kind == 3 {
                    arm is LiveEpmMapValueAccept || arm is EpmRawParReturn
                } else {
                    arm is PathChildReturn
                },
        },
        observable::ObservableEvent::OtherComplete { record, .. } => match record {
            observable::ObservableAcceptRecord::Root => arm is PathRootComplete,
            observable::ObservableAcceptRecord::MessageSlot { .. } =>
                if top_kind == 2 { arm is LiveMapEntryFinish } else { arm is LiveEpmFinishAccept },
            observable::ObservableAcceptRecord::MapValue => false,
            observable::ObservableAcceptRecord::Reduced =>
                if parent_kind == 3 { arm is EpmNestedReturn } else { arm is PathChildReturn },
        },
    }
}

pub open spec fn integrated_lane_exists(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
) -> bool {
    let product = state_product(state);
    let frames = state_frames(state);
    &&& state is Active
    &&& exists|action: dispatcher::IntegratedAction|
        event_lane(frames, event, action_arm(action))
            && #[trigger] dispatcher::integrated_kind_guard(product, action)
    &&& forall|action: dispatcher::IntegratedAction|
        event_lane(frames, event, action_arm(action))
            ==> #[trigger] dispatcher::integrated_kind_guard(product, action)
}

pub open spec fn raw_par_return_event(
    frames: Seq<admission::AdmissionFrameState>, event: observable::ObservableEvent,
) -> bool {
    &&& frames.len() >= 2
    &&& frames[frames.len() - 2].frame_kind == 3
    &&& match event {
        observable::ObservableEvent::MessageComplete { record, .. } => record is Reduced,
        _ => false,
    }
}

pub open spec fn nested_return_event(
    frames: Seq<admission::AdmissionFrameState>, event: observable::ObservableEvent,
) -> bool {
    &&& frames.len() >= 2
    &&& frames[frames.len() - 2].frame_kind == 3
    &&& match event {
        observable::ObservableEvent::OtherComplete { record, .. } => record is Reduced,
        _ => false,
    }
}

pub proof fn lemma_project_frame_congruence(
    first: Seq<admission::AdmissionFrameState>, second: Seq<admission::AdmissionFrameState>,
    index: int,
)
    requires
        0 <= index < first.len(),
        index < second.len(),
        second[index] == first[index],
        (index + 1 < second.len()) == (index + 1 < first.len()),
        index + 1 < first.len() ==> second[index + 1].frame_kind == first[index + 1].frame_kind,
    ensures project_frame(second, index) == project_frame(first, index),
{
    reveal(project_frame);
    let left = project_frame(second, index);
    let right = project_frame(first, index);
    assert(left.kind == right.kind);
    assert(left.body_len == right.body_len);
    assert(left.cursor == right.cursor);
    assert(left.slot_count == right.slot_count);
    assert(left.last_slot == right.last_slot);
    assert(left.waiting == right.waiting);
    assert(left.children == right.children);
}

pub proof fn lemma_project_frame_reads(frames: Seq<admission::AdmissionFrameState>, index: int)
    requires 0 <= index < frames.len(),
    ensures
        project_frames(frames).len() == frames.len(),
        project_frames(frames)[index] == project_frame(frames, index),
        project_frame(frames, index).kind == frames[index].frame_kind,
        project_frame(frames, index).body_len == frames[index].region_end as nat,
        project_frame(frames, index).waiting == (if index + 1 < frames.len() {
            Some(observable::ObservableWait {
                child_kind: frames[index + 1].frame_kind,
                slot: if observable::observable_tracks_fields(frames[index].frame_kind) {
                    project_slot(frames[index].waiting_slot)
                } else {
                    None
                },
            })
        } else {
            None
        }),
        observable::observable_tracks_fields(frames[index].frame_kind) ==> {
            &&& project_frame(frames, index).cursor == Some(frames[index].cursor as nat)
            &&& project_frame(frames, index).slot_count == frames[index].slot_count as nat
            &&& project_frame(frames, index).last_slot == project_slot(frames[index].last_slot)
            &&& project_frame(frames, index).children == project_children(frames[index].returns)
        },
        !observable::observable_tracks_fields(frames[index].frame_kind) ==> {
            &&& project_frame(frames, index).cursor is None
            &&& project_frame(frames, index).slot_count == 0
            &&& project_frame(frames, index).last_slot is None
            &&& project_frame(frames, index).children.len() == 0
        },
{
    reveal(project_frame);
    let projected = project_frame(frames, index);
    assert(project_frames(frames).len() == frames.len());
    assert(project_frames(frames)[index] == projected);
    assert(projected.kind == frames[index].frame_kind);
    assert(projected.body_len == frames[index].region_end as nat);
    if !observable::observable_tracks_fields(frames[index].frame_kind) {
        assert(projected.children == Seq::<observable::ObservableChild>::empty());
    }
}

pub proof fn lemma_project_children_push(
    returns: Seq<admission::AdmissionReturnedSummary>, summary: admission::AdmissionReturnedSummary,
)
    ensures
        project_children(returns.push(summary))
            == project_children(returns).push(project_child(summary)),
{
    let pushed = project_children(returns.push(summary));
    let extended = project_children(returns).push(project_child(summary));
    assert(pushed.len() == returns.len() + 1);
    assert(extended.len() == returns.len() + 1);
    assert forall|index: int| 0 <= index < pushed.len() implies pushed[index]
        == extended[index] by {
        if index < returns.len() {
            assert(returns.push(summary)[index] == returns[index]);
            assert(pushed[index] == project_child(returns[index]));
            assert(extended[index] == project_children(returns)[index]);
        } else {
            assert(returns.push(summary)[index] == summary);
            assert(pushed[index] == project_child(summary));
        }
    }
    assert(pushed =~= extended);
}

pub proof fn lemma_slot_admissible_projects(
    previous: Option<usize>, slot: usize, slot_count: usize, repeated: bool,
)
    ensures
        admission::slot_admissible(previous, slot, slot_count, repeated)
            == observable::observable_slot_admissible(
                project_slot(previous), slot as nat, slot_count as nat, repeated,
            ),
{
    match previous {
        Some(last) => {
            assert(project_slot(previous) == Some(last as nat));
            assert((slot > last) == (slot as nat > last as nat));
            assert((slot == last) == (slot as nat == last as nat));
        },
        None => {
            assert(project_slot(previous) is None);
        },
    }
    assert((slot < slot_count) == ((slot as nat) < slot_count as nat));
}

pub proof fn lemma_project_snapshot(state: dispatcher::IntegratedState)
    requires state is Active,
    ensures
        observable::observable_snapshot(project(state))
            == raw_snapshot(state_frames(state), state_score_len(state)),
{
    let frames = state_frames(state);
    let score_len = state_score_len(state);
    let projected = project_frames(frames);
    assert(project(state) == observable::ObservableState::Active { frames: projected, score_len });
    assert(projected.len() == frames.len());
    if frames.len() > 0 {
        lemma_project_frame_reads(frames, frames.len() - 1);
        assert(projected.last() == project_frame(frames, frames.len() - 1));
        assert(projected.last().kind == frames.last().frame_kind);
    }
}

pub proof fn lemma_backward_structural(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
    ensures raw_event_shape(state, event),
{
    hide(dispatcher::integrated_wf);
    let frames = state_frames(state);
    let score_len = state_score_len(state);
    let shadow = project_frames(frames);
    let next = observable::observable_next(project(state), event).unwrap();
    assert(state is Active);
    assert(project(state) == observable::ObservableState::Active { frames: shadow, score_len });
    assert(observable::observable_core_next(project(state), event) == Some(next));
    assert(observable::observable_event_after(event) == observable::observable_snapshot(next));
    lemma_project_snapshot(state);
    assert(observable::observable_event_before(event) == raw_snapshot(frames, score_len));
    assert(shadow.len() == frames.len());
    assert(frames.len() > 0);
    let last = frames.len() - 1;
    let top = frames.last();
    let shadow_top = shadow.last();
    lemma_project_frame_reads(frames, last);
    assert(shadow_top == project_frame(frames, last));
    assert(shadow_top.kind == top.frame_kind);
    assert(shadow_top.body_len == top.region_end as nat);
    assert(shadow_top.waiting is None);
    let message = event is MessageContinue || event is MessagePush || event is MessageComplete;
    let field = match event {
        observable::ObservableEvent::MessageContinue { field, .. } => Some(field),
        observable::ObservableEvent::OtherContinue { field, .. } => field,
        observable::ObservableEvent::MessagePush { field, .. } => Some(field),
        observable::ObservableEvent::OtherPush { field, .. } => field,
        _ => None,
    };
    let pushed = match event {
        observable::ObservableEvent::MessagePush { child, .. } => Some(child),
        observable::ObservableEvent::OtherPush { child, .. } => Some(child),
        _ => None,
    };
    assert(field is Some && observable::observable_field_admissible(shadow_top, field.unwrap())
        ==> raw_field_guard(top, field.unwrap())) by {
        if field is Some && observable::observable_field_admissible(shadow_top, field.unwrap()) {
            let moved = field.unwrap();
            assert(top.frame_kind == 1 || top.frame_kind == 2);
            assert(shadow_top.cursor == Some(top.cursor as nat));
            assert(shadow_top.slot_count == top.slot_count as nat);
            assert(shadow_top.last_slot == project_slot(top.last_slot));
            assert(moved.slot < top.slot_count as nat);
            assert((moved.slot as usize) as nat == moved.slot);
            lemma_slot_admissible_projects(
                top.last_slot, moved.slot as usize, top.slot_count, moved.repeated,
            );
        }
    }
    if event is MessageContinue || event is OtherContinue {
        assert(observable::observable_continue_next(shadow, score_len, message, field)
            == Some(next));
        assert(observable::observable_class_matches(message, shadow_top.kind));
        if top.frame_kind == 3 {
            assert(field is None);
            assert(next == project(state));
        } else {
            assert(field is Some);
            let moved = field.unwrap();
            assert(observable::observable_field_admissible(shadow_top, moved));
            assert(raw_field_guard(top, moved));
            if top.frame_kind == 2 {
                assert(observable::observable_map_key_field(shadow_top, moved));
                assert(shadow_top.slot_count == top.slot_count as nat);
                assert(top.slot_count == 2 && moved.slot == 0 && !moved.repeated);
            }
            let moved_top = observable::observable_frame_after_field(shadow_top, moved);
            let next_frames = shadow.update(last, moved_top);
            assert(next == observable::ObservableState::Active { frames: next_frames, score_len });
            assert(next_frames.last() == moved_top);
        }
        assert(raw_continue_shape(frames, message, field));
        assert(observable::observable_snapshot(next) == raw_snapshot(frames, score_len));
        assert(raw_event_shape(state, event));
    } else if event is MessagePush || event is OtherPush {
        let request = pushed.unwrap();
        assert(observable::observable_push_next(shadow, score_len, message, field, request)
            == Some(next));
        assert(observable::observable_class_matches(message, shadow_top.kind));
        assert(observable::observable_request_admissible(shadow_top, request));
        assert(observable::observable_push_allowed(shadow_top.kind, request.kind));
        assert(raw_push_kind_allowed(top.frame_kind, request.kind));
        assert(request.body_len <= top.region_end as nat);
        assert(observable::observable_push_parent_ok(shadow_top, field));
        if top.frame_kind == 1 || top.frame_kind == 2 {
            assert(field is Some);
            let moved = field.unwrap();
            assert(observable::observable_field_admissible(shadow_top, moved));
            assert(raw_field_guard(top, moved));
            if top.frame_kind == 2 {
                assert(observable::observable_map_value_field(shadow_top, moved));
                assert(shadow_top.slot_count == top.slot_count as nat);
                assert(top.slot_count == 2 && moved.slot == 1 && !moved.repeated);
            }
        } else {
            assert(field is None);
        }
        assert(raw_push_shape(frames, message, field, request));
        let parent_after = observable::observable_parent_after_push(shadow_top, field, request);
        let child_frame = observable::observable_fresh_frame(request);
        let next_frames = shadow.update(last, parent_after).push(child_frame);
        assert(next == observable::ObservableState::Active { frames: next_frames, score_len });
        assert(next_frames.len() == frames.len() + 1);
        assert(next_frames.last() == child_frame);
        assert(observable::observable_snapshot(next)
            == raw_after_push(frames, request.kind, score_len));
        assert(raw_event_shape(state, event));
    } else {
        let record = observable::observable_event_record(event).unwrap();
        let score_after = observable::observable_event_after(event).score_len;
        assert(observable::observable_complete_next(shadow, score_len, message, record, score_after)
            == Some(next));
        assert(observable::observable_class_matches(message, shadow_top.kind));
        assert(observable::observable_frame_finished(shadow_top));
        assert(observable::observable_score_ok(message, score_len, score_after));
        assert(top.frame_kind == 1 || top.frame_kind == 2 ==> top.cursor == top.region_end) by {
            if top.frame_kind == 1 || top.frame_kind == 2 {
                assert(shadow_top.cursor == Some(top.cursor as nat));
                assert(shadow_top.cursor == Some(shadow_top.body_len));
            }
        }
        if frames.len() == 1 {
            assert(record is Root);
            assert(top.frame_kind == 1 || top.frame_kind == 4);
            assert(next == observable::ObservableState::Accepted {
                root: shadow_top,
                score_len: score_after,
            });
        } else {
            let below = frames.len() - 2;
            let parent = frames[below];
            let shadow_parent = shadow[below];
            lemma_project_frame_reads(frames, below);
            assert(shadow_parent == project_frame(frames, below));
            assert(shadow_parent.kind == parent.frame_kind);
            assert(shadow_parent.waiting is Some);
            let wait = shadow_parent.waiting.unwrap();
            assert(observable::observable_accept_record_valid(
                shadow_parent, wait, shadow_top, record,
            ));
            assert(observable::observable_tracks_fields(parent.frame_kind)
                ==> wait.slot == project_slot(parent.waiting_slot));
            match record {
                observable::ObservableAcceptRecord::Root => {
                    assert(!observable::observable_accept_record_valid(
                        shadow_parent, wait, shadow_top, record,
                    ));
                },
                observable::ObservableAcceptRecord::MessageSlot { slot, .. } => {
                    assert(parent.frame_kind == 1);
                    assert(project_slot(parent.waiting_slot) == Some(slot));
                    assert(parent.waiting_slot is Some);
                    assert(parent.waiting_slot.unwrap() as nat == slot);
                },
                observable::ObservableAcceptRecord::MapValue => {
                    assert(parent.frame_kind == 2);
                    assert(top.frame_kind == 1);
                    assert(project_slot(parent.waiting_slot) == Some(1nat));
                    assert(parent.waiting_slot == Some(1usize));
                },
                observable::ObservableAcceptRecord::Reduced => {
                    assert(parent.frame_kind == 3 || parent.frame_kind == 4);
                },
            }
            let parent_after = observable::observable_parent_after_accept(
                shadow_parent, wait, shadow_top,
            );
            let next_frames = shadow.drop_last().update(below, parent_after);
            assert(next == observable::ObservableState::Active {
                frames: next_frames,
                score_len: score_after,
            });
            assert(next_frames.len() == frames.len() - 1);
            assert(next_frames.last() == parent_after);
            assert(parent_after.kind == parent.frame_kind);
        }
        assert(raw_complete_shape(frames, message, record, score_len, score_after));
        assert(observable::observable_snapshot(next) == raw_after_complete(frames, score_after));
        assert(raw_event_shape(state, event));
    }
}

pub proof fn lemma_observable_wf_bounds_canonical_path(state: dispatcher::IntegratedState)
    requires observable::observable_wf(project(state)),
    ensures canonical_path_top_is_root(state),
{
    let frames = state_frames(state);
    if state is Active && frames.len() >= 2 {
        let top = frames.len() - 1;
        let projected = project_frames(frames);
        lemma_project_frame_reads(frames, top);
        assert(observable::observable_stack_wf(projected));
        assert(observable::observable_link_wf(projected[top - 1], projected[top]));
        assert(observable::observable_push_allowed(
            projected[top - 1].kind, frames[top].frame_kind,
        ));
    }
}

pub proof fn lemma_coverage_of_continue(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
        event is MessageContinue || event is OtherContinue,
    ensures integrated_lane_exists(state, event),
{
    let product = state_product(state);
    let frames = state_frames(state);
    let projected = project_frames(frames);
    let score_len = state_score_len(state);
    assert(state is Active);
    assert(observable::observable_core_next(project(state), event) is Some);
    assert(frames.len() > 0);
    let top = frames.len() - 1;
    lemma_project_frame_reads(frames, top);
    let top_kind = frames[top].frame_kind;
    assert(projected.last().kind == top_kind);
    match event {
        observable::ObservableEvent::MessageContinue { field, .. } => {
            assert(observable::observable_continue_next(
                projected, score_len, true, Some(field),
            ) is Some);
            assert(top_kind == 1);
            let witness = dispatcher::IntegratedAction::Live {
                live_action: LiveRuntimeProductAction::Continue,
                route: dispatcher::IntegratedPushRoute::Plain,
            };
            assert(event_lane(frames, event, action_arm(witness)));
            assert(dispatcher::integrated_kind_guard(product, witness));
        },
        observable::ObservableEvent::OtherContinue { field, .. } => {
            assert(observable::observable_continue_next(
                projected, score_len, false, field,
            ) is Some);
            if field is Some {
                assert(top_kind == 2);
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::MapKey,
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            } else {
                assert(top_kind == 3);
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::EpmContinue { event: arbitrary() },
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            }
        },
        _ => {},
    }
    assert forall|action: dispatcher::IntegratedAction|
        event_lane(frames, event, action_arm(action))
        implies #[trigger] dispatcher::integrated_kind_guard(product, action) by {}
}

pub proof fn lemma_coverage_of_push(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
        event is MessagePush || event is OtherPush,
    ensures integrated_lane_exists(state, event),
{
    let product = state_product(state);
    let frames = state_frames(state);
    let projected = project_frames(frames);
    let score_len = state_score_len(state);
    assert(state is Active);
    assert(observable::observable_core_next(project(state), event) is Some);
    assert(frames.len() > 0);
    let top = frames.len() - 1;
    lemma_project_frame_reads(frames, top);
    let top_kind = frames[top].frame_kind;
    assert(projected.last().kind == top_kind);
    match event {
        observable::ObservableEvent::MessagePush { field, child, .. } => {
            assert(observable::observable_push_next(
                projected, score_len, true, Some(field), child,
            ) is Some);
            assert(top_kind == 1);
            if child.kind == 3 {
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::MessageEpmPush {
                        request: arbitrary(),
                        actual_top_kind: arbitrary(),
                        after_len: arbitrary(),
                        after_capacity: arbitrary(),
                        stats_frame_variant_bytes: arbitrary(),
                    },
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            } else {
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::MessagePush {
                        request: arbitrary(),
                        actual_top_kind: arbitrary(),
                        after_len: arbitrary(),
                        after_capacity: arbitrary(),
                        stats_frame_variant_bytes: arbitrary(),
                    },
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            }
        },
        observable::ObservableEvent::OtherPush { field, child, .. } => {
            assert(observable::observable_push_next(
                projected, score_len, false, field, child,
            ) is Some);
            assert(top_kind == 2 || top_kind == 3 || top_kind == 4);
            if top_kind == 2 {
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::MapValuePush {
                        request: arbitrary(),
                        actual_top_kind: arbitrary(),
                        after_len: arbitrary(),
                        after_capacity: arbitrary(),
                        stats_frame_variant_bytes: arbitrary(),
                    },
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            } else if top_kind == 3 {
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::EpmPush { observed: arbitrary() },
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            } else {
                let witness = dispatcher::IntegratedAction::CanonicalPath {
                    driver_before: arbitrary(),
                    driver_action: path_driver::CanonicalPathDriverSourceAction::Push {
                        storage_observed: arbitrary(),
                        source_before: arbitrary(),
                        loop_events: arbitrary(),
                        source_after_scan: arbitrary(),
                        source: arbitrary(),
                        child: arbitrary(),
                        storage: arbitrary(),
                        child_image: arbitrary(),
                        child_reader: arbitrary(),
                    },
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            }
        },
        _ => {},
    }
    assert forall|action: dispatcher::IntegratedAction|
        event_lane(frames, event, action_arm(action))
        implies #[trigger] dispatcher::integrated_kind_guard(product, action) by {}
}

pub proof fn lemma_coverage_of_message_complete(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
        event is MessageComplete,
    ensures integrated_lane_exists(state, event),
{
    let product = state_product(state);
    let frames = state_frames(state);
    let projected = project_frames(frames);
    let score_len = state_score_len(state);
    assert(state is Active);
    assert(observable::observable_core_next(project(state), event) is Some);
    assert(frames.len() > 0);
    let top = frames.len() - 1;
    lemma_project_frame_reads(frames, top);
    let top_kind = frames[top].frame_kind;
    assert(projected.last().kind == top_kind);
    match event {
        observable::ObservableEvent::MessageComplete { record, after, .. } => {
            assert(observable::observable_complete_next(
                projected, score_len, true, record, after.score_len,
            ) is Some);
            assert(top_kind == 1);
            if frames.len() == 1 {
                assert(record is Root);
                let witness = dispatcher::IntegratedAction::Live {
                    live_action: LiveRuntimeProductAction::RootComplete { returned: arbitrary() },
                    route: dispatcher::IntegratedPushRoute::Plain,
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            } else {
                let parent = frames.len() - 2;
                lemma_project_frame_reads(frames, parent);
                let parent_kind = frames[parent].frame_kind;
                assert(projected[parent].kind == parent_kind);
                assert(projected[parent].waiting is Some);
                match record {
                    observable::ObservableAcceptRecord::Root => {
                        assert(false);
                    },
                    observable::ObservableAcceptRecord::MessageSlot { .. } => {
                        let witness = dispatcher::IntegratedAction::Live {
                            live_action: LiveRuntimeProductAction::MessageChildComplete,
                            route: dispatcher::IntegratedPushRoute::Plain,
                        };
                        assert(event_lane(frames, event, action_arm(witness)));
                        assert(dispatcher::integrated_kind_guard(product, witness));
                    },
                    observable::ObservableAcceptRecord::MapValue => {
                        let witness = dispatcher::IntegratedAction::Live {
                            live_action: LiveRuntimeProductAction::MapValueAccept,
                            route: dispatcher::IntegratedPushRoute::Plain,
                        };
                        assert(event_lane(frames, event, action_arm(witness)));
                        assert(dispatcher::integrated_kind_guard(product, witness));
                    },
                    observable::ObservableAcceptRecord::Reduced => {
                        assert(parent_kind == 3 || parent_kind == 4);
                        if parent_kind == 3 {
                            let witness = dispatcher::IntegratedAction::EpmRawParReturn;
                            assert(event_lane(frames, event, action_arm(witness)));
                            assert(dispatcher::integrated_kind_guard(product, witness));
                        } else {
                            let witness = dispatcher::IntegratedAction::CanonicalPath {
                                driver_before: arbitrary(),
                                driver_action:
                                    path_driver::CanonicalPathDriverSourceAction::ChildReturn {
                                        storage_observed: arbitrary(),
                                        source_before_accept: arbitrary(),
                                        source_after_accept: arbitrary(),
                                        observed: arbitrary(),
                                    },
                            };
                            assert(event_lane(frames, event, action_arm(witness)));
                            assert(dispatcher::integrated_kind_guard(product, witness));
                        }
                    },
                }
            }
        },
        _ => {},
    }
    assert forall|action: dispatcher::IntegratedAction|
        event_lane(frames, event, action_arm(action))
        implies #[trigger] dispatcher::integrated_kind_guard(product, action) by {}
}

pub proof fn lemma_coverage_of_other_complete(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
        canonical_path_top_is_root(state),
        event is OtherComplete,
    ensures integrated_lane_exists(state, event),
{
    let product = state_product(state);
    let frames = state_frames(state);
    let projected = project_frames(frames);
    let score_len = state_score_len(state);
    assert(state is Active);
    assert(observable::observable_core_next(project(state), event) is Some);
    assert(frames.len() > 0);
    let top = frames.len() - 1;
    lemma_project_frame_reads(frames, top);
    let top_kind = frames[top].frame_kind;
    assert(projected.last().kind == top_kind);
    match event {
        observable::ObservableEvent::OtherComplete { record, after, .. } => {
            assert(observable::observable_complete_next(
                projected, score_len, false, record, after.score_len,
            ) is Some);
            assert(top_kind == 2 || top_kind == 3 || top_kind == 4);
            if frames.len() == 1 {
                assert(record is Root);
                assert(top_kind == 4);
                let witness = dispatcher::IntegratedAction::CanonicalPath {
                    driver_before: arbitrary(),
                    driver_action: path_driver::CanonicalPathDriverSourceAction::RootComplete {
                        storage_observed: arbitrary(),
                        source_before: arbitrary(),
                        loop_events: arbitrary(),
                        source_after_scan: arbitrary(),
                        observed: arbitrary(),
                    },
                };
                assert(event_lane(frames, event, action_arm(witness)));
                assert(dispatcher::integrated_kind_guard(product, witness));
            } else {
                let parent = frames.len() - 2;
                lemma_project_frame_reads(frames, parent);
                let parent_kind = frames[parent].frame_kind;
                assert(projected[parent].kind == parent_kind);
                assert(projected[parent].waiting is Some);
                assert(top_kind != 4);
                match record {
                    observable::ObservableAcceptRecord::Root => {
                        assert(false);
                    },
                    observable::ObservableAcceptRecord::MessageSlot { .. } => {
                        if top_kind == 2 {
                            let witness = dispatcher::IntegratedAction::Live {
                                live_action: LiveRuntimeProductAction::MapEntryFinish,
                                route: dispatcher::IntegratedPushRoute::Plain,
                            };
                            assert(event_lane(frames, event, action_arm(witness)));
                            assert(dispatcher::integrated_kind_guard(product, witness));
                        } else {
                            let witness = dispatcher::IntegratedAction::Live {
                                live_action: LiveRuntimeProductAction::EpmFinishAccept,
                                route: dispatcher::IntegratedPushRoute::Plain,
                            };
                            assert(event_lane(frames, event, action_arm(witness)));
                            assert(dispatcher::integrated_kind_guard(product, witness));
                        }
                    },
                    observable::ObservableAcceptRecord::MapValue => {
                        assert(false);
                    },
                    observable::ObservableAcceptRecord::Reduced => {
                        assert(parent_kind == 3 || parent_kind == 4);
                        if parent_kind == 3 {
                            if top_kind == 2 {
                                let map_state = product.source.product.map;
                                assert(map_state.admission.frames == frames);
                                assert(map_state.map_frames[top] is Some);
                                assert(frames[parent].waiting_slot == Some(3usize));
                                assert(admission::admission_frame_wf(frames[parent]));
                                assert(false);
                            }
                            let witness = dispatcher::IntegratedAction::EpmNestedReturn;
                            assert(event_lane(frames, event, action_arm(witness)));
                            assert(dispatcher::integrated_kind_guard(product, witness));
                        } else {
                            let witness = dispatcher::IntegratedAction::CanonicalPath {
                                driver_before: arbitrary(),
                                driver_action:
                                    path_driver::CanonicalPathDriverSourceAction::ChildReturn {
                                        storage_observed: arbitrary(),
                                        source_before_accept: arbitrary(),
                                        source_after_accept: arbitrary(),
                                        observed: arbitrary(),
                                    },
                            };
                            assert(event_lane(frames, event, action_arm(witness)));
                            assert(dispatcher::integrated_kind_guard(product, witness));
                        }
                    },
                }
            }
        },
        _ => {},
    }
    assert forall|action: dispatcher::IntegratedAction|
        event_lane(frames, event, action_arm(action))
        implies #[trigger] dispatcher::integrated_kind_guard(product, action) by {}
}

pub proof fn lemma_structural_coverage(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
        canonical_path_top_is_root(state),
    ensures integrated_lane_exists(state, event),
{
    if event is MessageContinue || event is OtherContinue {
        lemma_coverage_of_continue(state, event);
    } else if event is MessagePush || event is OtherPush {
        lemma_coverage_of_push(state, event);
    } else if event is MessageComplete {
        lemma_coverage_of_message_complete(state, event);
    } else {
        lemma_coverage_of_other_complete(state, event);
    }
}

pub proof fn lemma_coverage_names_obligation_returns(
    state: dispatcher::IntegratedState, event: observable::ObservableEvent,
)
    requires
        dispatcher::integrated_wf(state),
        observable::observable_next(project(state), event) is Some,
        canonical_path_top_is_root(state),
    ensures
        raw_par_return_event(state_frames(state), event) ==> {
            &&& event_lane(state_frames(state), event, IntegratedArm::EpmRawParReturn)
            &&& dispatcher::integrated_kind_guard(
                state_product(state), dispatcher::IntegratedAction::EpmRawParReturn,
            )
        },
        nested_return_event(state_frames(state), event) ==> {
            &&& event_lane(state_frames(state), event, IntegratedArm::EpmNestedReturn)
            &&& dispatcher::integrated_kind_guard(
                state_product(state), dispatcher::IntegratedAction::EpmNestedReturn,
            )
        },
{
    let product = state_product(state);
    let frames = state_frames(state);
    lemma_structural_coverage(state, event);
    if raw_par_return_event(frames, event) {
        let returning = dispatcher::IntegratedAction::EpmRawParReturn;
        assert(event_lane(frames, event, action_arm(returning)));
        assert(dispatcher::integrated_kind_guard(product, returning));
    }
    if nested_return_event(frames, event) {
        let returning = dispatcher::IntegratedAction::EpmNestedReturn;
        assert(event_lane(frames, event, action_arm(returning)));
        assert(dispatcher::integrated_kind_guard(product, returning));
    }
}

pub proof fn lemma_stutter_projects_equal(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
)
    requires stutter_transition(before, after),
    ensures project_frames(after) == project_frames(before),
{
    let top = before.len() - 1;
    assert forall|index: int| 0 <= index < before.len() implies
        (#[trigger] project_frames(after)[index]) == project_frames(before)[index] by {
        lemma_project_frame_reads(before, index);
        lemma_project_frame_reads(after, index);
        if index < top {
            lemma_project_frame_congruence(before, after, index);
        } else {
            assert(project_frame(after, index).children
                =~= project_frame(before, index).children);
        }
    }
    assert(project_frames(after) =~= project_frames(before));
}

pub proof fn lemma_internal_iteration_refines(
    frames: Seq<admission::AdmissionFrameState>, score_len: nat,
)
    requires
        frames.len() > 0,
        frames.last().frame_kind == 3,
    ensures
        observable::observable_next(
            project_active(frames, score_len),
            observable::observable_image_event(
                observable::ObservableClass::OtherContinue,
                project_active(frames, score_len),
                project_active(frames, score_len),
            ),
        ) == Some(project_active(frames, score_len)),
{
    lemma_project_frame_reads(frames, frames.len() - 1);
}

pub proof fn lemma_message_field_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat,
)
    requires
        field_transition(before, after),
        before.last().frame_kind == 1,
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                observable::ObservableClass::MessageContinue,
                project_active(before, score_len),
                project_active(after, score_len),
            ),
        ) == Some(project_active(after, score_len)),
{
    let top = before.len() - 1;
    let projected = project_frames(before);
    let moved = project_frames(after);
    lemma_project_frame_reads(before, top);
    lemma_project_frame_reads(after, top);
    let field = observable::observable_image_field(projected[top], moved[top]);
    let expected = projected.update(
        top, observable::observable_frame_after_field(projected[top], field),
    );
    assert forall|index: int| 0 <= index < before.len() implies
        (#[trigger] moved[index]) == expected[index] by {
        lemma_project_frame_reads(before, index);
        lemma_project_frame_reads(after, index);
        if index < top {
            lemma_project_frame_congruence(before, after, index);
        }
    }
    assert(moved =~= expected);
    assert(observable::observable_field_admissible(projected[top], field));
    assert(observable::observable_continue_next(projected, score_len, true, Some(field))
        == Some(observable::ObservableState::Active { frames: expected, score_len }));
}

pub proof fn lemma_map_key_field_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat,
)
    requires
        field_transition(before, after),
        before.last().frame_kind == 2,
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                observable::ObservableClass::OtherContinue,
                project_active(before, score_len),
                project_active(after, score_len),
            ),
        ) == Some(project_active(after, score_len)),
{
    let top = before.len() - 1;
    let projected = project_frames(before);
    let moved = project_frames(after);
    lemma_project_frame_reads(before, top);
    lemma_project_frame_reads(after, top);
    let field = observable::observable_image_field(projected[top], moved[top]);
    let expected = projected.update(
        top, observable::observable_frame_after_field(projected[top], field),
    );
    assert forall|index: int| 0 <= index < before.len() implies
        (#[trigger] moved[index]) == expected[index] by {
        lemma_project_frame_reads(before, index);
        lemma_project_frame_reads(after, index);
        if index < top {
            lemma_project_frame_congruence(before, after, index);
        }
    }
    assert(moved =~= expected);
    assert(observable::observable_field_admissible(projected[top], field));
    assert(observable::observable_map_key_field(projected[top], field));
    assert(observable::observable_continue_next(projected, score_len, false, Some(field))
        == Some(observable::ObservableState::Active { frames: expected, score_len }));
}

pub proof fn lemma_message_parent_push_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat,
)
    requires
        push_transition(before, after),
        before.last().frame_kind == 1,
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                observable::ObservableClass::MessagePush,
                project_active(before, score_len),
                project_active(after, score_len),
            ),
        ) == Some(project_active(after, score_len)),
{
    let top = before.len() - 1;
    let projected = project_frames(before);
    let pushed = project_frames(after);
    lemma_project_frame_reads(before, top);
    lemma_project_frame_reads(after, top);
    lemma_project_frame_reads(after, top + 1);
    let field = observable::observable_image_field(projected[top], pushed[top]);
    let request = observable::observable_image_request(pushed[top + 1]);
    let expected = projected.update(
        top, observable::observable_parent_after_push(projected[top], Some(field), request),
    ).push(observable::observable_fresh_frame(request));
    assert forall|index: int| 0 <= index < after.len() implies
        (#[trigger] pushed[index]) == expected[index] by {
        lemma_project_frame_reads(after, index);
        if index < top {
            lemma_project_frame_reads(before, index);
            lemma_project_frame_congruence(before, after, index);
        } else if index == top + 1 {
            assert(pushed[index].children =~= expected[index].children);
        }
    }
    assert(pushed =~= expected);
    assert(observable::observable_request_admissible(projected[top], request));
    assert(observable::observable_field_admissible(projected[top], field));
    assert(observable::observable_push_next(projected, score_len, true, Some(field), request)
        == Some(observable::ObservableState::Active { frames: expected, score_len }));
}

pub proof fn lemma_map_parent_push_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat,
)
    requires
        push_transition(before, after),
        before.last().frame_kind == 2,
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                observable::ObservableClass::OtherPush,
                project_active(before, score_len),
                project_active(after, score_len),
            ),
        ) == Some(project_active(after, score_len)),
{
    let top = before.len() - 1;
    let projected = project_frames(before);
    let pushed = project_frames(after);
    lemma_project_frame_reads(before, top);
    lemma_project_frame_reads(after, top);
    lemma_project_frame_reads(after, top + 1);
    let field = observable::observable_image_field(projected[top], pushed[top]);
    let request = observable::observable_image_request(pushed[top + 1]);
    let expected = projected.update(
        top, observable::observable_parent_after_push(projected[top], Some(field), request),
    ).push(observable::observable_fresh_frame(request));
    assert forall|index: int| 0 <= index < after.len() implies
        (#[trigger] pushed[index]) == expected[index] by {
        lemma_project_frame_reads(after, index);
        if index < top {
            lemma_project_frame_reads(before, index);
            lemma_project_frame_congruence(before, after, index);
        } else if index == top + 1 {
            assert(pushed[index].children =~= expected[index].children);
        }
    }
    assert(pushed =~= expected);
    assert(observable::observable_request_admissible(projected[top], request));
    assert(observable::observable_field_admissible(projected[top], field));
    assert(observable::observable_map_value_field(projected[top], field));
    assert(observable::observable_push_next(projected, score_len, false, Some(field), request)
        == Some(observable::ObservableState::Active { frames: expected, score_len }));
}

pub proof fn lemma_plain_parent_push_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat,
)
    requires
        push_transition(before, after),
        before.last().frame_kind == 3 || before.last().frame_kind == 4,
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                observable::ObservableClass::OtherPush,
                project_active(before, score_len),
                project_active(after, score_len),
            ),
        ) == Some(project_active(after, score_len)),
{
    let top = before.len() - 1;
    let projected = project_frames(before);
    let pushed = project_frames(after);
    lemma_project_frame_reads(before, top);
    lemma_project_frame_reads(after, top);
    lemma_project_frame_reads(after, top + 1);
    let request = observable::observable_image_request(pushed[top + 1]);
    let expected = projected.update(
        top, observable::observable_parent_after_push(projected[top], None, request),
    ).push(observable::observable_fresh_frame(request));
    assert forall|index: int| 0 <= index < after.len() implies
        (#[trigger] pushed[index]) == expected[index] by {
        lemma_project_frame_reads(after, index);
        if index < top {
            lemma_project_frame_reads(before, index);
            lemma_project_frame_congruence(before, after, index);
        } else {
            assert(pushed[index].children =~= expected[index].children);
        }
    }
    assert(pushed =~= expected);
    assert(observable::observable_request_admissible(projected[top], request));
    assert(observable::observable_push_next(projected, score_len, false, None, request)
        == Some(observable::ObservableState::Active { frames: expected, score_len }));
}

pub proof fn lemma_message_parent_accept_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat, score_after: nat,
)
    requires
        accept_transition(before, after),
        before[before.len() - 2].frame_kind == 1,
        accept_score_rule(before.last().frame_kind, score_len, score_after),
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                accept_class(before.last().frame_kind),
                project_active(before, score_len),
                project_active(after, score_after),
            ),
        ) == Some(project_active(after, score_after)),
{
    let child = before.len() - 1;
    let parent = before.len() - 2;
    let projected = project_frames(before);
    let popped = project_frames(after);
    lemma_project_frame_reads(before, child);
    lemma_project_frame_reads(before, parent);
    lemma_project_frame_reads(after, parent);
    let wait = projected[parent].waiting.unwrap();
    let record = observable::observable_image_record(projected);
    let message = before[child].frame_kind == 1;
    let expected = projected.drop_last().update(
        parent,
        observable::observable_parent_after_accept(projected[parent], wait, projected[child]),
    );
    let returned = after[parent].returns.last();
    assert(after[parent].returns =~= before[parent].returns.push(returned));
    lemma_project_children_push(before[parent].returns, returned);
    assert forall|index: int| 0 <= index < after.len() implies
        (#[trigger] popped[index]) == expected[index] by {
        lemma_project_frame_reads(after, index);
        if index < parent {
            lemma_project_frame_reads(before, index);
            lemma_project_frame_congruence(before, after, index);
        }
    }
    assert(popped =~= expected);
    assert(observable::observable_frame_finished(projected[child]));
    assert(observable::observable_accept_record_valid(
        projected[parent], wait, projected[child], record,
    ));
    assert(observable::observable_complete_next(projected, score_len, message, record, score_after)
        == Some(observable::ObservableState::Active { frames: expected, score_len: score_after }));
}

pub proof fn lemma_map_parent_accept_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat, score_after: nat,
)
    requires
        accept_transition(before, after),
        before[before.len() - 2].frame_kind == 2,
        accept_score_rule(before.last().frame_kind, score_len, score_after),
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                accept_class(before.last().frame_kind),
                project_active(before, score_len),
                project_active(after, score_after),
            ),
        ) == Some(project_active(after, score_after)),
{
    let child = before.len() - 1;
    let parent = before.len() - 2;
    let projected = project_frames(before);
    let popped = project_frames(after);
    lemma_project_frame_reads(before, child);
    lemma_project_frame_reads(before, parent);
    lemma_project_frame_reads(after, parent);
    let wait = projected[parent].waiting.unwrap();
    let record = observable::observable_image_record(projected);
    let expected = projected.drop_last().update(
        parent,
        observable::observable_parent_after_accept(projected[parent], wait, projected[child]),
    );
    let returned = after[parent].returns.last();
    assert(after[parent].returns =~= before[parent].returns.push(returned));
    lemma_project_children_push(before[parent].returns, returned);
    assert forall|index: int| 0 <= index < after.len() implies
        (#[trigger] popped[index]) == expected[index] by {
        lemma_project_frame_reads(after, index);
        if index < parent {
            lemma_project_frame_reads(before, index);
            lemma_project_frame_congruence(before, after, index);
        }
    }
    assert(popped =~= expected);
    assert(observable::observable_frame_finished(projected[child]));
    assert(observable::observable_accept_record_valid(
        projected[parent], wait, projected[child], record,
    ));
    assert(observable::observable_complete_next(projected, score_len, true, record, score_after)
        == Some(observable::ObservableState::Active { frames: expected, score_len: score_after }));
}

pub proof fn lemma_plain_parent_accept_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat, score_after: nat,
)
    requires
        accept_transition(before, after),
        before[before.len() - 2].frame_kind == 3 || before[before.len() - 2].frame_kind == 4,
        accept_score_rule(before.last().frame_kind, score_len, score_after),
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                accept_class(before.last().frame_kind),
                project_active(before, score_len),
                project_active(after, score_after),
            ),
        ) == Some(project_active(after, score_after)),
{
    let child = before.len() - 1;
    let parent = before.len() - 2;
    let projected = project_frames(before);
    let popped = project_frames(after);
    lemma_project_frame_reads(before, child);
    lemma_project_frame_reads(before, parent);
    lemma_project_frame_reads(after, parent);
    let wait = projected[parent].waiting.unwrap();
    let record = observable::observable_image_record(projected);
    let message = before[child].frame_kind == 1;
    let expected = projected.drop_last().update(
        parent,
        observable::observable_parent_after_accept(projected[parent], wait, projected[child]),
    );
    assert forall|index: int| 0 <= index < after.len() implies
        (#[trigger] popped[index]) == expected[index] by {
        lemma_project_frame_reads(after, index);
        if index < parent {
            lemma_project_frame_reads(before, index);
            lemma_project_frame_congruence(before, after, index);
        } else {
            assert(popped[index].children =~= expected[index].children);
        }
    }
    assert(popped =~= expected);
    assert(observable::observable_frame_finished(projected[child]));
    assert(observable::observable_accept_record_valid(
        projected[parent], wait, projected[child], record,
    ));
    assert(observable::observable_complete_next(projected, score_len, message, record, score_after)
        == Some(observable::ObservableState::Active { frames: expected, score_len: score_after }));
}

pub proof fn lemma_root_accept_refines(
    before: Seq<admission::AdmissionFrameState>, accepted: Seq<admission::AdmissionFrameState>,
    score_len: nat, score_after: nat,
)
    requires
        root_transition(before, accepted),
        accept_score_rule(before[0].frame_kind, score_len, score_after),
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                accept_class(before[0].frame_kind),
                project_active(before, score_len),
                project_root(accepted, score_after),
            ),
        ) == Some(project_root(accepted, score_after)),
{
    let projected = project_frames(before);
    let message = before[0].frame_kind == 1;
    lemma_project_frame_reads(before, 0);
    lemma_project_frame_reads(accepted, 0);
    assert(project_frame(accepted, 0).children =~= project_frame(before, 0).children);
    assert(project_frame(accepted, 0) == projected[0]);
    assert(observable::observable_frame_finished(projected[0]));
    assert(observable::observable_complete_next(
        projected, score_len, message, observable::ObservableAcceptRecord::Root, score_after,
    ) == Some(observable::ObservableState::Accepted { root: projected[0], score_len: score_after }));
}

pub proof fn lemma_score_then_accept_composes(
    before: Seq<admission::AdmissionFrameState>, middle: Seq<admission::AdmissionFrameState>,
    after: Seq<admission::AdmissionFrameState>,
)
    requires
        score_transition(before, middle),
        accept_transition(middle, after),
    ensures accept_transition(before, after),
{
    let parent = before.len() - 2;
    assert(middle[parent] == before[parent]);
    assert forall|index: int| 0 <= index < parent implies
        (#[trigger] after[index]) == before[index] by {
        assert(after[index] == middle[index]);
        assert(middle[index] == before[index]);
    }
}

pub proof fn lemma_epm_continue_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveEpmContinue,
    ensures stutter_step(before, after),
{
    hide(crate::live_runtime_product_wf);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::EpmContinue { event }, ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && unwind_product::live_runtime_internal_epm_unwind_step(product, next, event));
            let old = product.source.product;
            let new = next.source.product;
            assert(crate::product_epm_continue_step(old, new, event));
            let prior = crate::product_linked_top(old).unwrap();
            let linked = crate::product_linked_top(new).unwrap();
            assert(epm::linked_epm_continue(prior, linked, event));
            let frames = old.bridge.admission.frames;
            assert(new.bridge.admission.frames == frames.update(
                frames.len() - 1,
                epm::epm_advanced_admission_cursor(
                    frames.last(), linked.epm.cursor.value_table_offset,
                ),
            ));
        },
        _ => {},
    }
}

pub proof fn lemma_epm_end_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveEpmEnd,
    ensures stutter_step(before, after),
{
    hide(crate::live_runtime_product_wf);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::EpmEnd { observed }, ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && crate::live_runtime_product_epm_end_source_step(product, next, observed));
            crate::live_runtime_product_epm_end_source_preserves_wf(product, next, observed);
        },
        _ => {},
    }
}

pub proof fn lemma_act_forward_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is ActForward,
    ensures stutter_step(before, after),
{
    hide(dispatcher::integrated_act_forward_guard);
    match action {
        dispatcher::IntegratedAction::ActForward { width, commit, pending, state_id, .. } => {
            let product = before->product;
            let next = dispatcher::integrated_act_forward_after(
                product, width, commit, pending, state_id,
            );
            assert(before is Active
                && dispatcher::integrated_kind_guard(product, action)
                && after == dispatcher::IntegratedState::Active { product: next });
            let prior = product.source.product;
            let frame = dispatcher::integrated_act_forward_frame(
                prior, width, commit, pending, state_id,
            );
            assert(next.source.product
                == dispatcher::integrated_act_forward_product(prior, frame));
            let frames = prior.bridge.admission.frames;
            assert(next.source.product.bridge.admission.frames == frames.update(
                frames.len() - 1,
                epm::epm_advanced_admission_cursor(
                    frames.last(), frame.cursor.value_table_offset,
                ),
            ));
            assert(next.source.product.bridge.transcript == prior.bridge.transcript);
        },
        _ => {},
    }
}

pub proof fn lemma_act_unwind_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is ActUnwind,
    ensures stutter_step(before, after),
{
    hide(crate::live_runtime_product_wf);
    match action {
        dispatcher::IntegratedAction::ActUnwind { run, prefix, start, width, unwind_facts } => {
            let product = before->product;
            let next = dispatcher::integrated_act_unwind_after(product, run, prefix, width);
            assert(before is Active
                && dispatcher::integrated_kind_guard(product, action)
                && dispatcher::integrated_act_unwind_guard(
                    product, run, prefix, start, width, unwind_facts,
                )
                && after == dispatcher::IntegratedState::Active { product: next });
            let prior = product.source.product;
            let source = dispatcher::integrated_act_unwind_source(product);
            let frames = prior.bridge.admission.frames;
            assert(source.admission == prior.bridge.admission);
            unwind_product::unwind_product_virtual_matches_linked_source(
                prior, source, run, prefix, start, width, unwind_facts, width,
            );
            if width == 0 {
                assert(next.source == product.source);
            } else {
                let view = unwind_product::unwind_product_virtual_state(
                    prior, source, run, prefix, width,
                );
                assert(next.source.product == view);
                assert(view.bridge.admission
                    == unwind::linked_unwind_virtual_admission(source, width));
                assert(view.bridge.admission.frames == frames.update(
                    frames.len() - 1,
                    epm::epm_advanced_admission_cursor(
                        frames.last(), source.epm.cursor.value_table_offset,
                    ),
                ));
            }
        },
        _ => {},
    }
}

pub proof fn lemma_epm_handoff_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is EpmHandoff,
    ensures stutter_step(before, after),
{
    hide(crate::live_runtime_product_wf);
    match action {
        dispatcher::IntegratedAction::EpmHandoff { backing, plan, validated } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && handoff::live_runtime_epm_handoff_step(
                    product, next, backing, plan, validated,
                ));
            reveal(handoff::live_runtime_epm_handoff_step);
            let old = product.source.product;
            let new = next.source.product;
            assert(crate::product_epm_handoff_step(old, new, backing, plan, validated));
            assert(new.bridge.admission == old.bridge.admission);
            assert(new.bridge.transcript == old.bridge.transcript);
        },
        _ => {},
    }
}

pub proof fn lemma_internal_step_preserves_projection(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        segment_internal_arm(action_arm(action)),
    ensures
        stutter_step(before, after),
        project(after) == project(before),
{
    hide(dispatcher::integrated_step);
    let arm = action_arm(action);
    if arm is LiveEpmContinue {
        lemma_epm_continue_extract(before, after, action);
    } else if arm is LiveEpmEnd {
        lemma_epm_end_extract(before, after, action);
    } else if arm is ActForward {
        lemma_act_forward_extract(before, after, action);
    } else if arm is ActUnwind {
        lemma_act_unwind_extract(before, after, action);
    } else if arm is EpmHandoff {
        lemma_epm_handoff_extract(before, after, action);
    }
    assert(stutter_step(before, after));
    lemma_stutter_projects_equal(state_frames(before), state_frames(after));
    assert(before is Active) by {
        reveal(dispatcher::integrated_step);
    }
}

pub proof fn lemma_message_continue_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveContinue,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let top = before_frames.last();
            let after_top = after_frames.last();
            &&& after is Active
            &&& state_score_len(after) == state_score_len(before)
            &&& before_frames.len() > 0
            &&& after_frames.len() == before_frames.len()
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 1)
            &&& top.frame_kind == 1
            &&& after_top.frame_kind == top.frame_kind
            &&& after_top.region_end == top.region_end
            &&& after_top.slot_count == top.slot_count
            &&& after_top.returns == top.returns
            &&& top.cursor < after_top.cursor
            &&& after_top.cursor <= top.region_end
            &&& after_top.last_slot is Some
            &&& after_top.last_slot.unwrap() < top.slot_count
            &&& slot_order_kept(top.last_slot, after_top.last_slot.unwrap())
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::Continue, ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && crate::live_runtime_product_continue_step(product, next)) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            assert(crate::live_source_product_continue_step(product.source, next.source));
            let field = wire_source::wire_source_next_field(
                product.source.wire.last().unwrap(),
            )->field;
            let message = admission::ConcreteFrameActionModel::Continue { field };
            assert(crate::product_map_step(
                old, new, map::IntegratedAdmissionAction::Message { action: message },
            ));
            assert(admission::concrete_message_driver_step(old.map.admission, message)
                == Some(admission::ConcreteFrameOutcomeModel::Continued {
                    state: new.bridge.admission,
                }));
            assert(new.bridge.transcript == old.bridge.transcript);
            let view = admission::concrete_message_view(old.map.admission).unwrap();
            let continued = admission::concrete_message_continue(view, field).unwrap();
            assert(new.bridge.admission == continued.projection);
        },
        _ => {},
    }
}

pub proof fn lemma_message_continue_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveContinue,
    ensures
        field_step(before, after),
        state_frames(before).last().frame_kind == 1,
{
    hide(dispatcher::integrated_step);
    lemma_message_continue_frames(before, after, action);
}

pub proof fn lemma_message_continue_indexed_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveContinueIndexed,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let top = before_frames.last();
            let after_top = after_frames.last();
            &&& after is Active
            &&& state_score_len(after) == state_score_len(before)
            &&& before_frames.len() > 0
            &&& after_frames.len() == before_frames.len()
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 1)
            &&& top.frame_kind == 1
            &&& after_top.frame_kind == top.frame_kind
            &&& after_top.region_end == top.region_end
            &&& after_top.slot_count == top.slot_count
            &&& after_top.returns == top.returns
            &&& top.cursor < after_top.cursor
            &&& after_top.cursor <= top.region_end
            &&& after_top.last_slot is Some
            &&& after_top.last_slot.unwrap() < top.slot_count
            &&& slot_order_kept(top.last_slot, after_top.last_slot.unwrap())
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::ContinueIndexed, ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && crate::live_runtime_product_continue_indexed_step(product, next)) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            assert(crate::live_source_product_continue_indexed_step(product.source, next.source));
            let field = wire_source::wire_source_next_field(
                product.source.wire.last().unwrap(),
            )->field;
            let parent_source = crate::product_admission_source_image(old.indexed_regions.last());
            let message = admission::ConcreteFrameActionModel::ContinueIndexed {
                field, parent_source,
            };
            assert(crate::product_map_step(
                old, new, map::IntegratedAdmissionAction::Message { action: message },
            ));
            assert(admission::concrete_message_driver_step(old.map.admission, message)
                == Some(admission::ConcreteFrameOutcomeModel::Continued {
                    state: new.bridge.admission,
                }));
            assert(new.bridge.transcript == old.bridge.transcript);
            let view = admission::concrete_message_view(old.map.admission).unwrap();
            let continued = admission::concrete_message_continue_indexed(
                view, field, parent_source,
            ).unwrap();
            assert(new.bridge.admission == continued.projection);
        },
        _ => {},
    }
}

pub proof fn lemma_message_continue_indexed_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveContinueIndexed,
    ensures
        field_step(before, after),
        state_frames(before).last().frame_kind == 1,
{
    hide(dispatcher::integrated_step);
    lemma_message_continue_indexed_frames(before, after, action);
}

pub proof fn lemma_message_push_plain_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessagePush,
        action->route is Plain,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let top = before_frames.last();
            let parent = after_frames[before_frames.len() - 1];
            let child = after_frames.last();
            &&& after is Active
            &&& state_score_len(after) == state_score_len(before)
            &&& before_frames.len() > 0
            &&& after_frames.len() == before_frames.len() + 1
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 1)
            &&& top.frame_kind == 1
            &&& parent.frame_kind == top.frame_kind
            &&& parent.region_end == top.region_end
            &&& (child.frame_kind == 1 || child.frame_kind == 2)
            &&& child.cursor == 0
            &&& child.last_slot is None
            &&& child.returns.len() == 0
            &&& child.region_end <= top.region_end
            &&& parent.slot_count == top.slot_count
            &&& parent.returns == top.returns
            &&& top.cursor < parent.cursor
            &&& parent.cursor <= top.region_end
            &&& parent.last_slot is Some
            &&& parent.last_slot.unwrap() < top.slot_count
            &&& slot_order_kept(top.last_slot, parent.last_slot.unwrap())
            &&& parent.waiting_slot == parent.last_slot
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessagePush {
                request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
            },
            route: dispatcher::IntegratedPushRoute::Plain,
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && dispatcher::integrated_message_push_route_step(
                    product, next, request, actual_top_kind, after_len, after_capacity,
                    stats_frame_variant_bytes, dispatcher::IntegratedPushRoute::Plain,
                )) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            let message = admission::ConcreteFrameActionModel::Push {
                field: request.field,
                child: request.child,
                frame_bytes: product.source.runtime.frame_bytes,
            };
            assert(crate::product_map_step(
                old, new, map::IntegratedAdmissionAction::Message { action: message },
            ));
            assert(admission::concrete_message_driver_step(old.map.admission, message)
                == Some(admission::ConcreteFrameOutcomeModel::Pushed {
                    state: new.bridge.admission,
                }));
            assert(new.bridge.transcript == old.bridge.transcript);
        },
        _ => {},
    }
}

pub proof fn lemma_message_push_indexed_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessagePush,
        action->route is Indexed,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let top = before_frames.last();
            let parent = after_frames[before_frames.len() - 1];
            let child = after_frames.last();
            &&& after is Active
            &&& state_score_len(after) == state_score_len(before)
            &&& before_frames.len() > 0
            &&& after_frames.len() == before_frames.len() + 1
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 1)
            &&& top.frame_kind == 1
            &&& parent.frame_kind == top.frame_kind
            &&& parent.region_end == top.region_end
            &&& (child.frame_kind == 1 || child.frame_kind == 2)
            &&& child.cursor == 0
            &&& child.last_slot is None
            &&& child.returns.len() == 0
            &&& child.region_end <= top.region_end
            &&& parent.slot_count == top.slot_count
            &&& parent.returns == top.returns
            &&& top.cursor < parent.cursor
            &&& parent.cursor <= top.region_end
            &&& parent.last_slot is Some
            &&& parent.last_slot.unwrap() < top.slot_count
            &&& slot_order_kept(top.last_slot, parent.last_slot.unwrap())
            &&& parent.waiting_slot == parent.last_slot
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessagePush {
                request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
            },
            route: dispatcher::IntegratedPushRoute::Indexed,
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && dispatcher::integrated_message_push_route_step(
                    product, next, request, actual_top_kind, after_len, after_capacity,
                    stats_frame_variant_bytes, dispatcher::IntegratedPushRoute::Indexed,
                )) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            let message = admission::ConcreteFrameActionModel::PushIndexed {
                field: request.field,
                child: request.child,
                parent_source: crate::product_admission_source_image(old.indexed_regions.last()),
                frame_bytes: product.source.runtime.frame_bytes,
            };
            assert(crate::product_map_step(
                old, new, map::IntegratedAdmissionAction::Message { action: message },
            ));
            assert(admission::concrete_message_driver_step(old.map.admission, message)
                == Some(admission::ConcreteFrameOutcomeModel::Pushed {
                    state: new.bridge.admission,
                }));
            assert(new.bridge.transcript == old.bridge.transcript);
        },
        _ => {},
    }
}

pub proof fn lemma_message_push_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessagePush,
    ensures
        push_step(before, after),
        state_frames(before).last().frame_kind == 1,
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessagePush { .. },
            route: dispatcher::IntegratedPushRoute::Plain,
        } => {
            lemma_message_push_plain_frames(before, after, action);
        },
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessagePush { .. },
            route: dispatcher::IntegratedPushRoute::Indexed,
        } => {
            lemma_message_push_indexed_frames(before, after, action);
        },
        _ => {},
    }
}

pub proof fn lemma_message_epm_push_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessageEpmPush,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let top = before_frames.last();
            let parent = after_frames[before_frames.len() - 1];
            let child = after_frames.last();
            &&& after is Active
            &&& state_score_len(after) == state_score_len(before)
            &&& before_frames.len() > 0
            &&& after_frames.len() == before_frames.len() + 1
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 1)
            &&& top.frame_kind == 1
            &&& parent.frame_kind == top.frame_kind
            &&& parent.region_end == top.region_end
            &&& child.frame_kind == 3
            &&& child.region_end <= top.region_end
            &&& parent.slot_count == top.slot_count
            &&& parent.returns == top.returns
            &&& top.cursor < parent.cursor
            &&& parent.cursor <= top.region_end
            &&& parent.last_slot is Some
            &&& parent.last_slot.unwrap() < top.slot_count
            &&& slot_order_kept(top.last_slot, parent.last_slot.unwrap())
            &&& parent.waiting_slot == parent.last_slot
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessageEpmPush {
                request, actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
            },
            ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && dispatcher::integrated_kind_guard(product, action)
                && crate::live_runtime_product_message_epm_push_indexed_observed_step(
                    product, next, request, actual_top_kind, after_len, after_capacity,
                    stats_frame_variant_bytes,
                )) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            let message = admission::ConcreteFrameActionModel::PushIndexed {
                field: request.field,
                child: request.child,
                parent_source: crate::product_admission_source_image(old.indexed_regions.last()),
                frame_bytes: product.source.runtime.frame_bytes,
            };
            assert(crate::product_map_step(
                old, new, map::IntegratedAdmissionAction::Message { action: message },
            ));
            assert(admission::concrete_message_driver_step(old.map.admission, message)
                == Some(admission::ConcreteFrameOutcomeModel::Pushed {
                    state: new.bridge.admission,
                }));
            assert(new.bridge.transcript == old.bridge.transcript);
        },
        _ => {},
    }
}

pub proof fn lemma_message_epm_push_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessageEpmPush,
    ensures
        push_step(before, after),
        state_frames(before).last().frame_kind == 1,
        state_frames(after).last().frame_kind == 3,
{
    hide(dispatcher::integrated_step);
    lemma_message_epm_push_frames(before, after, action);
}

pub proof fn lemma_message_child_score_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessageChildScore,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let top = before_frames.last();
            let scored_top = after_frames.last();
            &&& after is Active
            &&& before_frames.len() > 1
            &&& top.frame_kind == 1
            &&& after_frames.len() == before_frames.len()
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 1)
            &&& scored_top.frame_kind == top.frame_kind
            &&& scored_top.region_end == top.region_end
            &&& scored_top.cursor == top.cursor
            &&& scored_top.last_slot == top.last_slot
            &&& scored_top.returns == top.returns
            &&& state_score_len(after) >= state_score_len(before)
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessageChildScore, ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && crate::live_runtime_product_message_child_score_step(product, next)) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            assert(crate::product_message_child_score_step(old, new));
            assert(score_bridge::bridge_well_formed(old.bridge));
            assert(admission::admission_state_wf(old.bridge.admission));
            score_bridge::bridge_score_source_step_preserves_shape(old.bridge, new.bridge);
        },
        _ => {},
    }
}

pub proof fn lemma_message_child_score_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessageChildScore,
    ensures score_step(before, after),
{
    hide(dispatcher::integrated_step);
    lemma_message_child_score_frames(before, after, action);
}

pub proof fn lemma_message_child_complete_frames(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessageChildComplete,
    ensures
        ({
            let before_frames = state_frames(before);
            let after_frames = state_frames(after);
            let completed_child = before_frames.last();
            let waiting_parent = before_frames[before_frames.len() - 2];
            let resumed_parent = after_frames.last();
            &&& after is Active
            &&& state_score_len(after) == state_score_len(before)
            &&& before_frames.len() >= 2
            &&& after_frames.len() == before_frames.len() - 1
            &&& lower_frames_kept(before_frames, after_frames, before_frames.len() - 2)
            &&& completed_child.frame_kind == 1
            &&& waiting_parent.frame_kind == 1
            &&& completed_child.cursor == completed_child.region_end
            &&& resumed_parent.frame_kind == waiting_parent.frame_kind
            &&& resumed_parent.region_end == waiting_parent.region_end
            &&& resumed_parent.cursor == waiting_parent.cursor
            &&& resumed_parent.slot_count == waiting_parent.slot_count
            &&& resumed_parent.last_slot == waiting_parent.last_slot
            &&& resumed_parent.returns.len() == waiting_parent.returns.len() + 1
            &&& resumed_parent.returns.drop_last() == waiting_parent.returns
            &&& waiting_parent.waiting_slot == Some(resumed_parent.returns.last().slot)
            &&& resumed_parent.returns.last().body_bytes.len() == completed_child.region_end as nat
        }),
{
    hide(dispatcher::integrated_step);
    match action {
        dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessageChildComplete, ..
        } => {
            let product = before->product;
            let next = after->product;
            assert(before is Active && after is Active
                && crate::live_runtime_product_message_child_complete_step(product, next)) by {
                reveal(dispatcher::integrated_step);
            }
            let old = product.source.product;
            let new = next.source.product;
            let message = admission::ConcreteFrameActionModel::Complete;
            assert(crate::product_map_step(
                old, new, map::IntegratedAdmissionAction::Message { action: message },
            ));
            assert(admission::concrete_message_driver_step(old.map.admission, message)
                == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
                    state: new.bridge.admission,
                }));
            assert(new.bridge.transcript == old.bridge.transcript);
            let state = old.map.admission;
            admission::concrete_message_complete_child_simulates(state);
            let waiting = state.frames[state.frames.len() - 2];
            let completed = state.frames[state.frames.len() - 1];
            let resumed = new.bridge.admission.frames.last();
            assert(resumed.returns.drop_last() =~= waiting.returns);
        },
        _ => {},
    }
}

pub proof fn lemma_message_child_complete_extract(
    before: dispatcher::IntegratedState, after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_wf(before),
        dispatcher::integrated_step(before, after, action),
        action_arm(action) is LiveMessageChildComplete,
    ensures
        accept_step(before, after),
        state_frames(before).last().frame_kind == 1,
        state_frames(before)[state_frames(before).len() - 2].frame_kind == 1,
{
    hide(dispatcher::integrated_step);
    lemma_message_child_complete_frames(before, after, action);
}

pub proof fn lemma_accept_shape_refines(
    before: Seq<admission::AdmissionFrameState>, after: Seq<admission::AdmissionFrameState>,
    score_len: nat, score_after: nat,
)
    requires
        accept_transition(before, after),
        accept_score_rule(before.last().frame_kind, score_len, score_after),
    ensures
        observable::observable_next(
            project_active(before, score_len),
            observable::observable_image_event(
                accept_class(before.last().frame_kind),
                project_active(before, score_len),
                project_active(after, score_after),
            ),
        ) == Some(project_active(after, score_after)),
{
    let parent_kind = before[before.len() - 2].frame_kind;
    if parent_kind == 1 {
        lemma_message_parent_accept_refines(before, after, score_len, score_after);
    } else if parent_kind == 2 {
        lemma_map_parent_accept_refines(before, after, score_len, score_after);
    } else {
        lemma_plain_parent_accept_refines(before, after, score_len, score_after);
    }
}

}
