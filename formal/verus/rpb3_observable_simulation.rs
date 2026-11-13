//! Simulation between `rpb3_integrated_dispatcher::integrated_step` and the observable
//! admission machine shared with `formal/verified_score_store`: projection, segments,
//! forward simulation, structural backward simulation, successor agreement and
//! structural coverage.

use vstd::prelude::*;

use crate::{
    admission_observable_spec as observable, rpb3_admission_cursor as admission,
    rpb3_canonical_path_driver_product_integration as path_driver,
    rpb3_integrated_dispatcher as dispatcher, LiveRuntimeProductAction, LiveRuntimeProductState,
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

}
