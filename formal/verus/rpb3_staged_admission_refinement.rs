use vstd::prelude::*;

verus! {

pub enum DriverAction {
    Continue { top: nat },
    Push { parent: nat, child: nat },
    Complete { completed: nat, parent: Option<nat>, eof_checked: bool },
}

pub enum DriverEvent {
    Continue { top: nat },
    Push { parent: nat, child: nat },
    Complete { completed: nat, parent: Option<nat> },
}

pub struct DriverState {
    pub frames: Seq<nat>,
    pub accepted: Seq<(nat, nat)>,
    pub events: Seq<DriverEvent>,
    pub returned_root: Option<nat>,
}

pub open spec fn action_valid(before: DriverState, action: DriverAction) -> bool {
    before.returned_root.is_none() && match action {
        DriverAction::Continue { top } =>
            before.frames.len() > 0 && before.frames[before.frames.len() - 1] == top,
        DriverAction::Push { parent, child } =>
            before.frames.len() > 0
                && before.frames[before.frames.len() - 1] == parent
                && !before.frames.contains(child),
        DriverAction::Complete { completed, parent, eof_checked } =>
            eof_checked
                && before.frames.len() > 0
                && before.frames[before.frames.len() - 1] == completed
                && match parent {
                    Some(p) => before.frames.len() > 1
                        && before.frames[before.frames.len() - 2] == p,
                    None => before.frames.len() == 1,
                },
    }
}

pub open spec fn action_event(action: DriverAction) -> DriverEvent {
    match action {
        DriverAction::Continue { top } => DriverEvent::Continue { top },
        DriverAction::Push { parent, child } => DriverEvent::Push { parent, child },
        DriverAction::Complete { completed, parent, eof_checked: _ } =>
            DriverEvent::Complete { completed, parent },
    }
}

pub open spec fn action_is_completion(action: DriverAction) -> bool {
    match action {
        DriverAction::Complete { completed: _, parent: _, eof_checked: _ } => true,
        _ => false,
    }
}

pub open spec fn driver_after(before: DriverState, action: DriverAction) -> DriverState {
    match action {
        DriverAction::Continue { top: _ } => DriverState {
            frames: before.frames,
            accepted: before.accepted,
            events: before.events.push(action_event(action)),
            returned_root: before.returned_root,
        },
        DriverAction::Push { parent: _, child } => DriverState {
            frames: before.frames.push(child),
            accepted: before.accepted,
            events: before.events.push(action_event(action)),
            returned_root: before.returned_root,
        },
        DriverAction::Complete { completed, parent, eof_checked: _ } => DriverState {
            frames: if before.frames.len() > 0 {
                before.frames.drop_last()
            } else {
                before.frames
            },
            accepted: match parent {
                Some(p) => before.accepted.push((p, completed)),
                None => before.accepted,
            },
            events: before.events.push(action_event(action)),
            returned_root: match parent {
                Some(_) => before.returned_root,
                None => Some(completed),
            },
        },
    }
}

pub open spec fn driver_step(before: DriverState, after: DriverState, action: DriverAction) -> bool {
    action_valid(before, action) && after == driver_after(before, action)
}

pub enum Stage {
    Observed,
    Parsed,
    Prepared,
    Applied,
    Checked,
    Committed,
    Aborted,
}

pub struct StagedState {
    pub before: DriverState,
    pub visible: DriverState,
    pub working_frames: Seq<nat>,
    pub working_accepted: Seq<(nat, nat)>,
    pub action: DriverAction,
    pub scratch_event: Option<DriverEvent>,
    pub eof_checked: bool,
    pub stage: Stage,
}

pub open spec fn start(before: DriverState, action: DriverAction) -> StagedState {
    StagedState {
        before,
        visible: before,
        working_frames: before.frames,
        working_accepted: before.accepted,
        action,
        scratch_event: None,
        eof_checked: false,
        stage: Stage::Observed,
    }
}

pub open spec fn staged_wf(state: StagedState) -> bool {
    match state.stage {
        Stage::Observed => {
            &&& state.visible == state.before
            &&& state.working_frames == state.before.frames
            &&& state.working_accepted == state.before.accepted
            &&& state.scratch_event.is_none()
            &&& !state.eof_checked
        },
        Stage::Parsed | Stage::Prepared => {
            &&& action_valid(state.before, state.action)
            &&& state.visible == state.before
            &&& state.working_frames == state.before.frames
            &&& state.working_accepted == state.before.accepted
            &&& state.scratch_event == Some(action_event(state.action))
            &&& state.eof_checked == action_is_completion(state.action)
        },
        Stage::Applied | Stage::Checked => {
            &&& action_valid(state.before, state.action)
            &&& state.visible == state.before
            &&& state.working_frames == driver_after(state.before, state.action).frames
            &&& state.working_accepted == driver_after(state.before, state.action).accepted
            &&& state.scratch_event == Some(action_event(state.action))
            &&& state.eof_checked == action_is_completion(state.action)
        },
        Stage::Committed => {
            &&& action_valid(state.before, state.action)
            &&& state.visible == driver_after(state.before, state.action)
            &&& state.working_frames == state.visible.frames
            &&& state.working_accepted == state.visible.accepted
            &&& state.scratch_event == Some(action_event(state.action))
            &&& state.eof_checked == action_is_completion(state.action)
        },
        Stage::Aborted => state.visible == state.before,
    }
}

pub open spec fn staged_step(before: StagedState, after: StagedState) -> bool {
    &&& staged_wf(before)
    &&& staged_wf(after)
    &&& after.before == before.before
    &&& after.action == before.action
    &&& match (before.stage, after.stage) {
        (Stage::Observed, Stage::Parsed)
        | (Stage::Parsed, Stage::Prepared)
        | (Stage::Prepared, Stage::Applied)
        | (Stage::Applied, Stage::Checked)
        | (Stage::Checked, Stage::Committed) => action_valid(before.before, before.action),
        (Stage::Observed, Stage::Aborted)
        | (Stage::Parsed, Stage::Aborted)
        | (Stage::Prepared, Stage::Aborted)
        | (Stage::Applied, Stage::Aborted)
        | (Stage::Checked, Stage::Aborted) => true,
        _ => false,
    }
}

pub proof fn start_is_private(before: DriverState, action: DriverAction)
    ensures
        staged_wf(start(before, action)),
        start(before, action).visible == before,
        start(before, action).visible.events == before.events,
{}

pub proof fn staged_step_refines_one_driver_transition(
    before: StagedState,
    after: StagedState,
)
    requires staged_step(before, after),
    ensures
        after.visible == before.visible
            || driver_step(before.visible, after.visible, before.action),
{
    match (before.stage, after.stage) {
        (Stage::Checked, Stage::Committed) => {
            assert(before.visible == before.before);
            assert(after.visible == driver_after(before.before, before.action));
            assert(driver_step(before.visible, after.visible, before.action));
        },
        _ => assert(after.visible == before.visible),
    }
}

pub proof fn staged_step_preserves_event_order(
    before: StagedState,
    after: StagedState,
)
    requires staged_step(before, after),
    ensures
        after.visible.events == before.visible.events
            || after.visible.events == before.visible.events.push(action_event(before.action)),
{
    staged_step_refines_one_driver_transition(before, after);
}

pub proof fn staged_push_retains_parent_and_appends_child(
    before: DriverState,
    after: DriverState,
    parent: nat,
    child: nat,
)
    requires driver_step(before, after, DriverAction::Push { parent, child }),
    ensures
        before.frames[before.frames.len() - 1] == parent,
        !before.frames.contains(child),
        after.frames == before.frames.push(child),
        after.accepted == before.accepted,
        after.events == before.events.push(DriverEvent::Push { parent, child }),
{}

pub proof fn staged_eof_pop_accept_is_exact(
    before: DriverState,
    after: DriverState,
    completed: nat,
    parent: Option<nat>,
)
    requires driver_step(before, after, DriverAction::Complete {
        completed, parent, eof_checked: true,
    }),
    ensures
        before.frames[before.frames.len() - 1] == completed,
        after.frames == before.frames.drop_last(),
        after.events == before.events.push(DriverEvent::Complete { completed, parent }),
        match parent {
            Some(p) => {
                &&& before.frames.len() > 1
                &&& before.frames[before.frames.len() - 2] == p
                &&& after.frames[after.frames.len() - 1] == p
                &&& after.accepted == before.accepted.push((p, completed))
                &&& after.returned_root == before.returned_root
            },
            None => {
                &&& before.frames.len() == 1
                &&& after.frames.len() == 0
                &&& after.accepted == before.accepted
                &&& after.returned_root == Some(completed)
            },
        },
{}

pub proof fn staged_abort_does_not_publish(before: StagedState, after: StagedState)
    requires staged_step(before, after), after.stage is Aborted,
    ensures
        after.visible == before.before,
        after.visible.events == before.before.events,
        after.visible.returned_root == before.before.returned_root,
{}

pub open spec fn staged_trace(states: Seq<StagedState>) -> bool {
    &&& states.len() > 0
    &&& states[0] == start(states[0].before, states[0].action)
    &&& forall|i: int| 0 <= i < states.len() - 1
        ==> #[trigger] staged_step(states[i], states[i + 1])
}

pub proof fn staged_trace_refines_driver(states: Seq<StagedState>)
    requires staged_trace(states),
    ensures forall|i: int| 0 <= i < states.len() - 1
        ==> states[i + 1].visible == states[i].visible
            || driver_step(states[i].visible, states[i + 1].visible, states[i].action),
{
    assert forall|i: int| 0 <= i < states.len() - 1 implies
        states[i + 1].visible == states[i].visible
            || driver_step(states[i].visible, states[i + 1].visible, states[i].action) by {
        staged_step_refines_one_driver_transition(states[i], states[i + 1]);
    }
}

}
