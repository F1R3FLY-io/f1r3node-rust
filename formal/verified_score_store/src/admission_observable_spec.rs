//! Observable admission machine. Shared verbatim by formal/verified_score_store
//! (runtime monitor) and formal/verus (simulation against integrated_step) via #[path].
//! Self-contained: no crate:: paths, no exec code, no cfg attributes.
//! Frame kinds: 1 Message, 2 Map, 3 EPathMap, 4 CanonicalPath.

use vstd::prelude::*;

verus! {

pub struct ObservableChild {
    pub slot: nat,
    pub body_len: nat,
}

pub struct ObservableWait {
    pub child_kind: u8,
    pub slot: Option<nat>,
}

pub struct ObservableFrame {
    pub kind: u8,
    pub body_len: nat,
    pub cursor: Option<nat>,
    pub slot_count: nat,
    pub last_slot: Option<nat>,
    pub waiting: Option<ObservableWait>,
    pub children: Seq<ObservableChild>,
}

pub enum ObservableState {
    Active { frames: Seq<ObservableFrame>, score_len: nat },
    Accepted { root: ObservableFrame, score_len: nat },
}

pub struct ObservableSnapshot {
    pub depth: nat,
    pub top_kind: Option<u8>,
    pub score_len: nat,
}

pub struct ObservableField {
    pub slot: nat,
    pub repeated: bool,
    pub cursor_after: nat,
}

pub struct ObservableFrameRequest {
    pub kind: u8,
    pub body_len: nat,
    pub slot_count: nat,
}

pub enum ObservableAcceptRecord {
    Root,
    MessageSlot { slot: nat, occurrences_after: nat },
    MapValue,
    Reduced,
}

pub enum ObservableEvent {
    MessageContinue {
        before: ObservableSnapshot,
        field: ObservableField,
        after: ObservableSnapshot,
    },
    MessagePush {
        before: ObservableSnapshot,
        field: ObservableField,
        child: ObservableFrameRequest,
        after: ObservableSnapshot,
    },
    MessageComplete {
        before: ObservableSnapshot,
        record: ObservableAcceptRecord,
        after: ObservableSnapshot,
    },
    OtherContinue {
        before: ObservableSnapshot,
        field: Option<ObservableField>,
        after: ObservableSnapshot,
    },
    OtherPush {
        before: ObservableSnapshot,
        field: Option<ObservableField>,
        child: ObservableFrameRequest,
        after: ObservableSnapshot,
    },
    OtherComplete {
        before: ObservableSnapshot,
        record: ObservableAcceptRecord,
        after: ObservableSnapshot,
    },
}

pub enum ObservableClass {
    MessageContinue,
    MessagePush,
    MessageComplete,
    OtherContinue,
    OtherPush,
    OtherComplete,
}

pub open spec fn observable_tracks_fields(kind: u8) -> bool {
    kind == 1 || kind == 2
}

pub open spec fn observable_class_matches(message: bool, kind: u8) -> bool {
    if message { kind == 1 } else { kind == 2 || kind == 3 || kind == 4 }
}

pub open spec fn observable_push_allowed(parent_kind: u8, child_kind: u8) -> bool {
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

pub open spec fn observable_slot_admissible(
    previous: Option<nat>, slot: nat, slot_count: nat, repeated: bool,
) -> bool {
    &&& slot < slot_count
    &&& match previous {
        None => true,
        Some(last) => slot > last || (slot == last && repeated),
    }
}

pub open spec fn observable_slot_run(children: Seq<ObservableChild>, slot: nat) -> nat
    decreases children.len(),
{
    if children.len() == 0 {
        0
    } else if children.last().slot == slot {
        observable_slot_run(children.drop_last(), slot) + 1
    } else {
        0
    }
}

pub open spec fn observable_snapshot(state: ObservableState) -> ObservableSnapshot {
    match state {
        ObservableState::Active { frames, score_len } => ObservableSnapshot {
            depth: frames.len(),
            top_kind: if frames.len() == 0 { None } else { Some(frames.last().kind) },
            score_len,
        },
        ObservableState::Accepted { score_len, .. } => ObservableSnapshot {
            depth: 0,
            top_kind: None,
            score_len,
        },
    }
}

pub open spec fn observable_event_before(event: ObservableEvent) -> ObservableSnapshot {
    match event {
        ObservableEvent::MessageContinue { before, .. } => before,
        ObservableEvent::MessagePush { before, .. } => before,
        ObservableEvent::MessageComplete { before, .. } => before,
        ObservableEvent::OtherContinue { before, .. } => before,
        ObservableEvent::OtherPush { before, .. } => before,
        ObservableEvent::OtherComplete { before, .. } => before,
    }
}

pub open spec fn observable_event_after(event: ObservableEvent) -> ObservableSnapshot {
    match event {
        ObservableEvent::MessageContinue { after, .. } => after,
        ObservableEvent::MessagePush { after, .. } => after,
        ObservableEvent::MessageComplete { after, .. } => after,
        ObservableEvent::OtherContinue { after, .. } => after,
        ObservableEvent::OtherPush { after, .. } => after,
        ObservableEvent::OtherComplete { after, .. } => after,
    }
}

pub open spec fn observable_event_class(event: ObservableEvent) -> ObservableClass {
    match event {
        ObservableEvent::MessageContinue { .. } => ObservableClass::MessageContinue,
        ObservableEvent::MessagePush { .. } => ObservableClass::MessagePush,
        ObservableEvent::MessageComplete { .. } => ObservableClass::MessageComplete,
        ObservableEvent::OtherContinue { .. } => ObservableClass::OtherContinue,
        ObservableEvent::OtherPush { .. } => ObservableClass::OtherPush,
        ObservableEvent::OtherComplete { .. } => ObservableClass::OtherComplete,
    }
}

pub open spec fn observable_event_record(event: ObservableEvent) -> Option<ObservableAcceptRecord> {
    match event {
        ObservableEvent::MessageComplete { record, .. } => Some(record),
        ObservableEvent::OtherComplete { record, .. } => Some(record),
        _ => None,
    }
}

pub open spec fn observable_event_is_push(event: ObservableEvent) -> bool {
    event is MessagePush || event is OtherPush
}

pub open spec fn observable_top(state: ObservableState) -> ObservableFrame {
    match state {
        ObservableState::Active { frames, .. } => frames.last(),
        ObservableState::Accepted { root, .. } => root,
    }
}

pub open spec fn observable_frames_of(state: ObservableState) -> Seq<ObservableFrame> {
    match state {
        ObservableState::Active { frames, .. } => frames,
        ObservableState::Accepted { .. } => Seq::<ObservableFrame>::empty(),
    }
}

pub open spec fn observable_fresh_frame(request: ObservableFrameRequest) -> ObservableFrame {
    ObservableFrame {
        kind: request.kind,
        body_len: request.body_len,
        cursor: if observable_tracks_fields(request.kind) { Some(0nat) } else { None },
        slot_count: request.slot_count,
        last_slot: None,
        waiting: None,
        children: Seq::<ObservableChild>::empty(),
    }
}

pub open spec fn observable_request_admissible(
    parent: ObservableFrame, request: ObservableFrameRequest,
) -> bool {
    &&& observable_push_allowed(parent.kind, request.kind)
    &&& request.body_len <= parent.body_len
    &&& (request.kind == 3 ==> request.slot_count == 0)
}

pub open spec fn observable_field_admissible(frame: ObservableFrame, field: ObservableField) -> bool {
    &&& observable_tracks_fields(frame.kind)
    &&& frame.waiting is None
    &&& match frame.cursor {
        Some(cursor) => cursor < field.cursor_after && field.cursor_after <= frame.body_len,
        None => false,
    }
    &&& observable_slot_admissible(frame.last_slot, field.slot, frame.slot_count, field.repeated)
}

pub open spec fn observable_map_key_field(frame: ObservableFrame, field: ObservableField) -> bool {
    frame.slot_count == 2 && field.slot == 0 && !field.repeated
}

pub open spec fn observable_map_value_field(frame: ObservableFrame, field: ObservableField) -> bool {
    frame.slot_count == 2 && field.slot == 1 && !field.repeated
}

pub open spec fn observable_frame_after_field(
    frame: ObservableFrame, field: ObservableField,
) -> ObservableFrame {
    ObservableFrame { cursor: Some(field.cursor_after), last_slot: Some(field.slot), ..frame }
}

pub open spec fn observable_wait_slot(field: Option<ObservableField>) -> Option<nat> {
    match field {
        Some(f) => Some(f.slot),
        None => None,
    }
}

pub open spec fn observable_push_parent_ok(
    top: ObservableFrame, field: Option<ObservableField>,
) -> bool {
    if observable_tracks_fields(top.kind) {
        match field {
            Some(f) => observable_field_admissible(top, f)
                && (top.kind == 2 ==> observable_map_value_field(top, f)),
            None => false,
        }
    } else {
        field is None
    }
}

pub open spec fn observable_parent_after_push(
    top: ObservableFrame, field: Option<ObservableField>, request: ObservableFrameRequest,
) -> ObservableFrame {
    let moved = match field {
        Some(f) => observable_frame_after_field(top, f),
        None => top,
    };
    ObservableFrame {
        waiting: Some(ObservableWait { child_kind: request.kind, slot: observable_wait_slot(field) }),
        ..moved
    }
}

pub open spec fn observable_frame_finished(frame: ObservableFrame) -> bool {
    &&& frame.waiting is None
    &&& (observable_tracks_fields(frame.kind) ==> frame.cursor == Some(frame.body_len))
}

pub open spec fn observable_score_ok(message: bool, score_len: nat, score_after: nat) -> bool {
    if message { score_after >= score_len } else { score_after == score_len }
}

pub open spec fn observable_accept_record_valid(
    parent: ObservableFrame, wait: ObservableWait, child: ObservableFrame,
    record: ObservableAcceptRecord,
) -> bool {
    match record {
        ObservableAcceptRecord::Root => false,
        ObservableAcceptRecord::MessageSlot { slot, occurrences_after } => {
            &&& parent.kind == 1
            &&& wait.slot == Some(slot)
            &&& occurrences_after == observable_slot_run(parent.children, slot) + 1
        },
        ObservableAcceptRecord::MapValue => {
            &&& parent.kind == 2
            &&& child.kind == 1
            &&& wait.slot == Some(1nat)
        },
        ObservableAcceptRecord::Reduced => parent.kind == 3 || parent.kind == 4,
    }
}

pub open spec fn observable_child_summary(wait: ObservableWait, child: ObservableFrame) -> ObservableChild {
    ObservableChild {
        slot: match wait.slot {
            Some(slot) => slot,
            None => 0nat,
        },
        body_len: child.body_len,
    }
}

pub open spec fn observable_parent_after_accept(
    parent: ObservableFrame, wait: ObservableWait, child: ObservableFrame,
) -> ObservableFrame {
    if observable_tracks_fields(parent.kind) {
        ObservableFrame {
            waiting: None,
            children: parent.children.push(observable_child_summary(wait, child)),
            ..parent
        }
    } else {
        ObservableFrame { waiting: None, ..parent }
    }
}

pub open spec fn observable_continue_next(
    frames: Seq<ObservableFrame>, score_len: nat, message: bool, field: Option<ObservableField>,
) -> Option<ObservableState> {
    let top = frames.last();
    if !observable_class_matches(message, top.kind) || top.waiting is Some {
        None
    } else if top.kind == 3 {
        if field is None {
            Some(ObservableState::Active { frames, score_len })
        } else {
            None
        }
    } else {
        match field {
            None => None,
            Some(f) => {
                if observable_field_admissible(top, f)
                    && (top.kind == 2 ==> observable_map_key_field(top, f))
                {
                    Some(ObservableState::Active {
                        frames: frames.update(frames.len() - 1, observable_frame_after_field(top, f)),
                        score_len,
                    })
                } else {
                    None
                }
            },
        }
    }
}

pub open spec fn observable_push_next(
    frames: Seq<ObservableFrame>, score_len: nat, message: bool,
    field: Option<ObservableField>, request: ObservableFrameRequest,
) -> Option<ObservableState> {
    let top = frames.last();
    if !observable_class_matches(message, top.kind)
        || top.waiting is Some
        || !observable_request_admissible(top, request)
        || !observable_push_parent_ok(top, field)
    {
        None
    } else {
        Some(ObservableState::Active {
            frames: frames.update(frames.len() - 1, observable_parent_after_push(top, field, request))
                .push(observable_fresh_frame(request)),
            score_len,
        })
    }
}

pub open spec fn observable_complete_next(
    frames: Seq<ObservableFrame>, score_len: nat, message: bool,
    record: ObservableAcceptRecord, score_after: nat,
) -> Option<ObservableState> {
    let top = frames.last();
    if !observable_class_matches(message, top.kind)
        || !observable_frame_finished(top)
        || !observable_score_ok(message, score_len, score_after)
    {
        None
    } else if frames.len() == 1 {
        if record is Root && (top.kind == 1 || top.kind == 4) {
            Some(ObservableState::Accepted { root: top, score_len: score_after })
        } else {
            None
        }
    } else {
        let parent = frames[frames.len() - 2];
        match parent.waiting {
            None => None,
            Some(wait) => {
                if wait.child_kind == top.kind
                    && observable_accept_record_valid(parent, wait, top, record)
                {
                    Some(ObservableState::Active {
                        frames: frames.drop_last().update(
                            frames.len() - 2,
                            observable_parent_after_accept(parent, wait, top),
                        ),
                        score_len: score_after,
                    })
                } else {
                    None
                }
            },
        }
    }
}

pub open spec fn observable_core_next(
    state: ObservableState, event: ObservableEvent,
) -> Option<ObservableState> {
    match state {
        ObservableState::Accepted { .. } => None,
        ObservableState::Active { frames, score_len } => {
            if frames.len() == 0 {
                None
            } else {
                match event {
                    ObservableEvent::MessageContinue { field, .. } =>
                        observable_continue_next(frames, score_len, true, Some(field)),
                    ObservableEvent::OtherContinue { field, .. } =>
                        observable_continue_next(frames, score_len, false, field),
                    ObservableEvent::MessagePush { field, child, .. } =>
                        observable_push_next(frames, score_len, true, Some(field), child),
                    ObservableEvent::OtherPush { field, child, .. } =>
                        observable_push_next(frames, score_len, false, field, child),
                    ObservableEvent::MessageComplete { record, after, .. } =>
                        observable_complete_next(frames, score_len, true, record, after.score_len),
                    ObservableEvent::OtherComplete { record, after, .. } =>
                        observable_complete_next(frames, score_len, false, record, after.score_len),
                }
            }
        },
    }
}

pub open spec fn observable_next(
    state: ObservableState, event: ObservableEvent,
) -> Option<ObservableState> {
    if observable_event_before(event) != observable_snapshot(state) {
        None
    } else {
        match observable_core_next(state, event) {
            None => None,
            Some(next) => {
                if observable_event_after(event) == observable_snapshot(next) {
                    Some(next)
                } else {
                    None
                }
            },
        }
    }
}

pub open spec fn observable_step(
    before: ObservableState, after: ObservableState, event: ObservableEvent,
) -> bool {
    observable_next(before, event) == Some(after)
}

pub open spec fn observable_trace(
    start: ObservableState, events: Seq<ObservableEvent>,
) -> Option<ObservableState>
    decreases events.len(),
{
    if events.len() == 0 {
        Some(start)
    } else {
        match observable_next(start, events[0]) {
            None => None,
            Some(next) => observable_trace(next, events.subrange(1, events.len() as int)),
        }
    }
}

pub open spec fn observable_root_admissible(root: ObservableFrameRequest) -> bool {
    root.kind == 1 || (root.kind == 4 && root.slot_count == 0)
}

pub open spec fn observable_initial(root: ObservableFrameRequest, score_len: nat) -> ObservableState {
    ObservableState::Active {
        frames: Seq::<ObservableFrame>::empty().push(observable_fresh_frame(root)),
        score_len,
    }
}

pub open spec fn observable_children_sorted(children: Seq<ObservableChild>) -> bool {
    forall|i: int, j: int| #![trigger children[i], children[j]]
        0 <= i < j < children.len() ==> children[i].slot <= children[j].slot
}

pub open spec fn observable_frame_wf(frame: ObservableFrame) -> bool {
    &&& 1 <= frame.kind <= 4
    &&& if observable_tracks_fields(frame.kind) {
        &&& match frame.cursor {
            Some(cursor) => cursor <= frame.body_len,
            None => false,
        }
        &&& match frame.last_slot {
            Some(last) => {
                &&& last < frame.slot_count
                &&& forall|i: int| 0 <= i < frame.children.len()
                    ==> (#[trigger] frame.children[i]).slot <= last
            },
            None => frame.children.len() == 0,
        }
        &&& observable_children_sorted(frame.children)
    } else {
        &&& frame.cursor is None
        &&& frame.slot_count == 0
        &&& frame.last_slot is None
        &&& frame.children.len() == 0
    }
}

pub open spec fn observable_link_wf(parent: ObservableFrame, child: ObservableFrame) -> bool {
    &&& observable_push_allowed(parent.kind, child.kind)
    &&& child.body_len <= parent.body_len
    &&& match parent.waiting {
        None => false,
        Some(wait) => {
            &&& wait.child_kind == child.kind
            &&& if observable_tracks_fields(parent.kind) {
                &&& wait.slot is Some
                &&& wait.slot == parent.last_slot
                &&& (parent.kind == 2 ==> wait.slot == Some(1nat))
            } else {
                wait.slot is None
            }
        },
    }
}

pub open spec fn observable_stack_wf(frames: Seq<ObservableFrame>) -> bool {
    &&& frames.len() > 0
    &&& (frames[0].kind == 1 || frames[0].kind == 4)
    &&& forall|i: int| 0 <= i < frames.len() ==> observable_frame_wf(#[trigger] frames[i])
    &&& frames.last().waiting is None
    &&& forall|i: int| #![trigger frames[i]] 0 <= i < frames.len() - 1
        ==> observable_link_wf(frames[i], frames[i + 1])
}

pub open spec fn observable_wf(state: ObservableState) -> bool {
    match state {
        ObservableState::Active { frames, .. } => observable_stack_wf(frames),
        ObservableState::Accepted { root, .. } => {
            &&& observable_frame_wf(root)
            &&& root.waiting is None
            &&& (root.kind == 1 || root.kind == 4)
        },
    }
}

pub open spec fn observable_image_field(before_top: ObservableFrame, moved: ObservableFrame) -> ObservableField {
    let slot = match moved.last_slot {
        Some(slot) => slot,
        None => 0nat,
    };
    ObservableField {
        slot,
        repeated: before_top.last_slot == Some(slot),
        cursor_after: match moved.cursor {
            Some(cursor) => cursor,
            None => 0nat,
        },
    }
}

pub open spec fn observable_image_request(child: ObservableFrame) -> ObservableFrameRequest {
    ObservableFrameRequest { kind: child.kind, body_len: child.body_len, slot_count: child.slot_count }
}

pub open spec fn observable_image_record(frames: Seq<ObservableFrame>) -> ObservableAcceptRecord {
    if frames.len() <= 1 {
        ObservableAcceptRecord::Root
    } else {
        let parent = frames[frames.len() - 2];
        let slot = match parent.waiting {
            Some(wait) => match wait.slot {
                Some(slot) => slot,
                None => 0nat,
            },
            None => 0nat,
        };
        if parent.kind == 1 {
            ObservableAcceptRecord::MessageSlot {
                slot,
                occurrences_after: observable_slot_run(parent.children, slot) + 1,
            }
        } else if parent.kind == 2 {
            ObservableAcceptRecord::MapValue
        } else {
            ObservableAcceptRecord::Reduced
        }
    }
}

pub open spec fn observable_image_event(
    class: ObservableClass, before: ObservableState, after: ObservableState,
) -> ObservableEvent {
    let frames = observable_frames_of(before);
    let next_frames = observable_frames_of(after);
    let top = frames.last();
    let snapshot_before = observable_snapshot(before);
    let snapshot_after = observable_snapshot(after);
    match class {
        ObservableClass::MessageContinue => ObservableEvent::MessageContinue {
            before: snapshot_before,
            field: observable_image_field(top, next_frames.last()),
            after: snapshot_after,
        },
        ObservableClass::MessagePush => ObservableEvent::MessagePush {
            before: snapshot_before,
            field: observable_image_field(top, next_frames[frames.len() - 1]),
            child: observable_image_request(next_frames.last()),
            after: snapshot_after,
        },
        ObservableClass::MessageComplete => ObservableEvent::MessageComplete {
            before: snapshot_before,
            record: observable_image_record(frames),
            after: snapshot_after,
        },
        ObservableClass::OtherContinue => ObservableEvent::OtherContinue {
            before: snapshot_before,
            field: if top.kind == 2 { Some(observable_image_field(top, next_frames.last())) } else { None },
            after: snapshot_after,
        },
        ObservableClass::OtherPush => ObservableEvent::OtherPush {
            before: snapshot_before,
            field: if top.kind == 2 {
                Some(observable_image_field(top, next_frames[frames.len() - 1]))
            } else {
                None
            },
            child: observable_image_request(next_frames.last()),
            after: snapshot_after,
        },
        ObservableClass::OtherComplete => ObservableEvent::OtherComplete {
            before: snapshot_before,
            record: observable_image_record(frames),
            after: snapshot_after,
        },
    }
}

pub open spec fn observable_field_same_except_repeated(first: ObservableField, second: ObservableField) -> bool {
    first.slot == second.slot && first.cursor_after == second.cursor_after
}

pub open spec fn observable_option_field_same_except_repeated(
    first: Option<ObservableField>, second: Option<ObservableField>,
) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(a), Some(b)) => observable_field_same_except_repeated(a, b),
        _ => false,
    }
}

pub open spec fn observable_event_same_except_repeated(first: ObservableEvent, second: ObservableEvent) -> bool {
    match (first, second) {
        (ObservableEvent::MessageContinue { before: b1, field: f1, after: a1 },
         ObservableEvent::MessageContinue { before: b2, field: f2, after: a2 }) =>
            b1 == b2 && a1 == a2 && observable_field_same_except_repeated(f1, f2),
        (ObservableEvent::MessagePush { before: b1, field: f1, child: c1, after: a1 },
         ObservableEvent::MessagePush { before: b2, field: f2, child: c2, after: a2 }) =>
            b1 == b2 && a1 == a2 && c1 == c2 && observable_field_same_except_repeated(f1, f2),
        (ObservableEvent::OtherContinue { before: b1, field: f1, after: a1 },
         ObservableEvent::OtherContinue { before: b2, field: f2, after: a2 }) =>
            b1 == b2 && a1 == a2 && observable_option_field_same_except_repeated(f1, f2),
        (ObservableEvent::OtherPush { before: b1, field: f1, child: c1, after: a1 },
         ObservableEvent::OtherPush { before: b2, field: f2, child: c2, after: a2 }) =>
            b1 == b2 && a1 == a2 && c1 == c2 && observable_option_field_same_except_repeated(f1, f2),
        _ => first == second,
    }
}

pub open spec fn observable_children_extend(before: Seq<ObservableChild>, after: Seq<ObservableChild>) -> bool {
    after == before
        || (after.len() == before.len() + 1 && after.subrange(0, before.len() as int) == before)
}

pub proof fn lemma_slot_run_push_same(children: Seq<ObservableChild>, child: ObservableChild)
    ensures observable_slot_run(children.push(child), child.slot) == observable_slot_run(children, child.slot) + 1,
{
    let pushed = children.push(child);
    assert(pushed.last() == child);
    assert(pushed.drop_last() =~= children);
}

pub proof fn lemma_slot_run_last_differs(children: Seq<ObservableChild>, slot: nat)
    requires children.len() == 0 || children.last().slot != slot,
    ensures observable_slot_run(children, slot) == 0,
{
}

pub proof fn lemma_slot_run_bounded(children: Seq<ObservableChild>, slot: nat)
    ensures observable_slot_run(children, slot) <= children.len(),
    decreases children.len(),
{
    if children.len() > 0 && children.last().slot == slot {
        lemma_slot_run_bounded(children.drop_last(), slot);
    }
}

pub proof fn lemma_observable_stack_wf_set_top(frames: Seq<ObservableFrame>, top: ObservableFrame)
    requires
        observable_stack_wf(frames),
        observable_frame_wf(top),
        top.kind == frames.last().kind,
        top.body_len == frames.last().body_len,
        top.waiting is None,
    ensures observable_stack_wf(frames.update(frames.len() - 1, top)),
{
    let last = frames.len() - 1;
    let next = frames.update(last, top);
    assert(next.len() == frames.len());
    assert(next.last() == top);
    assert forall|i: int| 0 <= i < next.len() implies observable_frame_wf(#[trigger] next[i]) by {
        if i != last {
            assert(next[i] == frames[i]);
        }
    }
    assert forall|i: int| #![trigger next[i]] 0 <= i < next.len() - 1
        implies observable_link_wf(next[i], next[i + 1]) by {
        assert(next[i] == frames[i]);
        assert(observable_link_wf(frames[i], frames[i + 1]));
        if i + 1 != last {
            assert(next[i + 1] == frames[i + 1]);
        }
    }
    assert(next[0].kind == frames[0].kind);
}

pub proof fn lemma_observable_stack_wf_push(
    frames: Seq<ObservableFrame>, parent: ObservableFrame, child: ObservableFrame,
)
    requires
        observable_stack_wf(frames),
        observable_frame_wf(parent),
        observable_frame_wf(child),
        parent.kind == frames.last().kind,
        parent.body_len == frames.last().body_len,
        observable_link_wf(parent, child),
        child.waiting is None,
    ensures observable_stack_wf(frames.update(frames.len() - 1, parent).push(child)),
{
    let last = frames.len() - 1;
    let middle = frames.update(last, parent);
    let next = middle.push(child);
    assert(next.len() == frames.len() + 1);
    assert(next[last] == parent);
    assert(next.last() == child);
    assert forall|i: int| 0 <= i < next.len() implies observable_frame_wf(#[trigger] next[i]) by {
        if i < last {
            assert(next[i] == frames[i]);
        }
    }
    assert forall|i: int| #![trigger next[i]] 0 <= i < next.len() - 1
        implies observable_link_wf(next[i], next[i + 1]) by {
        if i < last {
            assert(next[i] == frames[i]);
            assert(observable_link_wf(frames[i], frames[i + 1]));
            if i + 1 < last {
                assert(next[i + 1] == frames[i + 1]);
            }
        }
    }
    assert(next[0].kind == frames[0].kind);
}

pub proof fn lemma_observable_stack_wf_pop(frames: Seq<ObservableFrame>, parent: ObservableFrame)
    requires
        observable_stack_wf(frames),
        frames.len() >= 2,
        observable_frame_wf(parent),
        parent.kind == frames[frames.len() - 2].kind,
        parent.body_len == frames[frames.len() - 2].body_len,
        parent.waiting is None,
    ensures observable_stack_wf(frames.drop_last().update(frames.len() - 2, parent)),
{
    let index = frames.len() - 2;
    let next = frames.drop_last().update(index, parent);
    assert(frames.drop_last().len() == frames.len() - 1);
    assert(next.len() == frames.len() - 1);
    assert(next.last() == parent);
    assert forall|i: int| 0 <= i < next.len() implies observable_frame_wf(#[trigger] next[i]) by {
        if i < index {
            assert(next[i] == frames[i]);
        }
    }
    assert forall|i: int| #![trigger next[i]] 0 <= i < next.len() - 1
        implies observable_link_wf(next[i], next[i + 1]) by {
        assert(next[i] == frames[i]);
        assert(observable_link_wf(frames[i], frames[i + 1]));
        if i + 1 < index {
            assert(next[i + 1] == frames[i + 1]);
        }
    }
    assert(next[0].kind == frames[0].kind);
}

pub proof fn lemma_observable_frame_after_field_wf(frame: ObservableFrame, field: ObservableField)
    requires
        observable_frame_wf(frame),
        observable_field_admissible(frame, field),
    ensures observable_frame_wf(observable_frame_after_field(frame, field)),
{
    let moved = observable_frame_after_field(frame, field);
    assert(moved.children == frame.children);
    assert forall|i: int| 0 <= i < moved.children.len()
        implies (#[trigger] moved.children[i]).slot <= field.slot by {
        match frame.last_slot {
            Some(last) => {
                assert(frame.children[i].slot <= last);
            },
            None => {},
        }
    }
}

pub proof fn lemma_observable_push_frames_wf(
    top: ObservableFrame, field: Option<ObservableField>, request: ObservableFrameRequest,
)
    requires
        observable_frame_wf(top),
        observable_request_admissible(top, request),
        observable_push_parent_ok(top, field),
    ensures
        observable_frame_wf(observable_parent_after_push(top, field, request)),
        observable_frame_wf(observable_fresh_frame(request)),
        observable_link_wf(observable_parent_after_push(top, field, request), observable_fresh_frame(request)),
        observable_parent_after_push(top, field, request).kind == top.kind,
        observable_parent_after_push(top, field, request).body_len == top.body_len,
{
    match field {
        Some(f) => {
            lemma_observable_frame_after_field_wf(top, f);
        },
        None => {},
    }
}

pub proof fn lemma_observable_parent_after_accept_wf(
    parent: ObservableFrame, wait: ObservableWait, child: ObservableFrame,
)
    requires
        observable_frame_wf(parent),
        observable_link_wf(parent, child),
        parent.waiting == Some(wait),
    ensures
        observable_frame_wf(observable_parent_after_accept(parent, wait, child)),
        observable_parent_after_accept(parent, wait, child).kind == parent.kind,
        observable_parent_after_accept(parent, wait, child).body_len == parent.body_len,
        observable_parent_after_accept(parent, wait, child).waiting is None,
{
    if observable_tracks_fields(parent.kind) {
        let summary = observable_child_summary(wait, child);
        let children = parent.children.push(summary);
        match parent.last_slot {
            Some(last) => {
                assert(summary.slot == last);
                assert forall|i: int| 0 <= i < children.len()
                    implies (#[trigger] children[i]).slot <= last by {
                    if i < parent.children.len() {
                        assert(children[i] == parent.children[i]);
                    }
                }
                assert forall|i: int, j: int| #![trigger children[i], children[j]]
                    0 <= i < j < children.len() implies children[i].slot <= children[j].slot by {
                    if j < parent.children.len() {
                        assert(children[i] == parent.children[i]);
                        assert(children[j] == parent.children[j]);
                    } else {
                        assert(children[j] == summary);
                        assert(children[i] == parent.children[i]);
                    }
                }
                assert(observable_children_sorted(children));
            },
            None => {},
        }
    }
}

pub proof fn lemma_observable_continue_next_wf(
    frames: Seq<ObservableFrame>, score_len: nat, message: bool, field: Option<ObservableField>,
)
    requires observable_stack_wf(frames),
    ensures
        match observable_continue_next(frames, score_len, message, field) {
            Some(next) => observable_wf(next),
            None => true,
        },
{
    let top = frames.last();
    assert(observable_frame_wf(frames[frames.len() - 1]));
    match field {
        Some(f) => {
            if top.kind != 3 && observable_field_admissible(top, f) {
                lemma_observable_frame_after_field_wf(top, f);
                lemma_observable_stack_wf_set_top(frames, observable_frame_after_field(top, f));
            }
        },
        None => {},
    }
}

pub proof fn lemma_observable_push_next_wf(
    frames: Seq<ObservableFrame>, score_len: nat, message: bool,
    field: Option<ObservableField>, request: ObservableFrameRequest,
)
    requires observable_stack_wf(frames),
    ensures
        match observable_push_next(frames, score_len, message, field, request) {
            Some(next) => observable_wf(next),
            None => true,
        },
{
    let top = frames.last();
    assert(observable_frame_wf(frames[frames.len() - 1]));
    if observable_request_admissible(top, request) && observable_push_parent_ok(top, field) {
        lemma_observable_push_frames_wf(top, field, request);
        lemma_observable_stack_wf_push(
            frames,
            observable_parent_after_push(top, field, request),
            observable_fresh_frame(request),
        );
    }
}

pub proof fn lemma_observable_complete_next_wf(
    frames: Seq<ObservableFrame>, score_len: nat, message: bool,
    record: ObservableAcceptRecord, score_after: nat,
)
    requires observable_stack_wf(frames),
    ensures
        match observable_complete_next(frames, score_len, message, record, score_after) {
            Some(next) => observable_wf(next),
            None => true,
        },
{
    let top = frames.last();
    if frames.len() == 1 {
        assert(observable_frame_wf(frames[0]));
        assert(top == frames[0]);
    } else {
        let index = frames.len() - 2;
        let parent = frames[index];
        assert(observable_frame_wf(parent));
        assert(observable_link_wf(frames[index], frames[index + 1]));
        assert(frames[index + 1] == top);
        match parent.waiting {
            Some(wait) => {
                lemma_observable_parent_after_accept_wf(parent, wait, top);
                lemma_observable_stack_wf_pop(frames, observable_parent_after_accept(parent, wait, top));
            },
            None => {},
        }
    }
}

pub proof fn lemma_observable_initial_wf(root: ObservableFrameRequest, score_len: nat)
    requires observable_root_admissible(root),
    ensures observable_wf(observable_initial(root, score_len)),
{
    let frame = observable_fresh_frame(root);
    let frames = Seq::<ObservableFrame>::empty().push(frame);
    assert(frames.len() == 1);
    assert(frames[0] == frame);
    assert(observable_frame_wf(frame));
}

pub proof fn lemma_observable_continue_preserves_wf(state: ObservableState, event: ObservableEvent)
    requires observable_wf(state), event is MessageContinue || event is OtherContinue,
    ensures match observable_next(state, event) { Some(next) => observable_wf(next), None => true },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            match event {
                ObservableEvent::MessageContinue { field, .. } => {
                    lemma_observable_continue_next_wf(frames, score_len, true, Some(field));
                },
                ObservableEvent::OtherContinue { field, .. } => {
                    lemma_observable_continue_next_wf(frames, score_len, false, field);
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_push_preserves_wf(state: ObservableState, event: ObservableEvent)
    requires observable_wf(state), observable_event_is_push(event),
    ensures match observable_next(state, event) { Some(next) => observable_wf(next), None => true },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            match event {
                ObservableEvent::MessagePush { field, child, .. } => {
                    lemma_observable_push_next_wf(frames, score_len, true, Some(field), child);
                },
                ObservableEvent::OtherPush { field, child, .. } => {
                    lemma_observable_push_next_wf(frames, score_len, false, field, child);
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_complete_preserves_wf(state: ObservableState, event: ObservableEvent)
    requires observable_wf(state), event is MessageComplete || event is OtherComplete,
    ensures match observable_next(state, event) { Some(next) => observable_wf(next), None => true },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            match event {
                ObservableEvent::MessageComplete { record, after, .. } => {
                    lemma_observable_complete_next_wf(frames, score_len, true, record, after.score_len);
                },
                ObservableEvent::OtherComplete { record, after, .. } => {
                    lemma_observable_complete_next_wf(frames, score_len, false, record, after.score_len);
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_next_preserves_wf(state: ObservableState, event: ObservableEvent)
    requires observable_wf(state),
    ensures match observable_next(state, event) { Some(next) => observable_wf(next), None => true },
{
    match event {
        ObservableEvent::MessageContinue { .. } => lemma_observable_continue_preserves_wf(state, event),
        ObservableEvent::OtherContinue { .. } => lemma_observable_continue_preserves_wf(state, event),
        ObservableEvent::MessagePush { .. } => lemma_observable_push_preserves_wf(state, event),
        ObservableEvent::OtherPush { .. } => lemma_observable_push_preserves_wf(state, event),
        ObservableEvent::MessageComplete { .. } => lemma_observable_complete_preserves_wf(state, event),
        ObservableEvent::OtherComplete { .. } => lemma_observable_complete_preserves_wf(state, event),
    }
}

pub proof fn lemma_observable_trace_preserves_wf(start: ObservableState, events: Seq<ObservableEvent>)
    requires observable_wf(start),
    ensures match observable_trace(start, events) { Some(end) => observable_wf(end), None => true },
    decreases events.len(),
{
    if events.len() > 0 {
        lemma_observable_next_preserves_wf(start, events[0]);
        match observable_next(start, events[0]) {
            Some(next) => {
                lemma_observable_trace_preserves_wf(next, events.subrange(1, events.len() as int));
            },
            None => {},
        }
    }
}

pub proof fn lemma_observable_step_deterministic(
    before: ObservableState, first: ObservableState, second: ObservableState, event: ObservableEvent,
)
    requires observable_step(before, first, event), observable_step(before, second, event),
    ensures first == second,
{
}

pub proof fn lemma_observable_accepted_is_terminal(state: ObservableState, event: ObservableEvent)
    requires state is Accepted,
    ensures observable_next(state, event) is None,
{
}

pub proof fn lemma_observable_image_field_exact(
    top: ObservableFrame, field: ObservableField, moved: ObservableFrame,
)
    requires
        observable_field_admissible(top, field),
        moved.last_slot == Some(field.slot),
        moved.cursor == Some(field.cursor_after),
    ensures
        observable_field_same_except_repeated(field, observable_image_field(top, moved)),
        observable_field_admissible(top, observable_image_field(top, moved)),
        !field.repeated ==> !observable_image_field(top, moved).repeated,
        observable_frame_after_field(top, observable_image_field(top, moved))
            == observable_frame_after_field(top, field),
{
    let image = observable_image_field(top, moved);
    assert(image.slot == field.slot);
    assert(image.cursor_after == field.cursor_after);
    match top.last_slot {
        Some(last) => {
            if field.slot == last {
                assert(image.repeated);
            } else {
                assert(field.slot > last);
            }
        },
        None => {},
    }
}

pub proof fn lemma_observable_continue_determined(state: ObservableState, event: ObservableEvent)
    requires
        event is MessageContinue || event is OtherContinue,
        observable_next(state, event) is Some,
    ensures
        match observable_next(state, event) {
            Some(next) => {
                let image = observable_image_event(observable_event_class(event), state, next);
                &&& observable_event_same_except_repeated(event, image)
                &&& observable_next(state, image) == Some(next)
            },
            None => true,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let top = frames.last();
            match event {
                ObservableEvent::MessageContinue { field, .. } => {
                    let moved = observable_frame_after_field(top, field);
                    let next_frames = frames.update(frames.len() - 1, moved);
                    assert(next_frames.last() == moved);
                    lemma_observable_image_field_exact(top, field, moved);
                },
                ObservableEvent::OtherContinue { field, .. } => {
                    match field {
                        Some(f) => {
                            let moved = observable_frame_after_field(top, f);
                            let next_frames = frames.update(frames.len() - 1, moved);
                            assert(next_frames.last() == moved);
                            lemma_observable_image_field_exact(top, f, moved);
                        },
                        None => {},
                    }
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_push_determined(state: ObservableState, event: ObservableEvent)
    requires
        observable_event_is_push(event),
        observable_next(state, event) is Some,
    ensures
        match observable_next(state, event) {
            Some(next) => {
                let image = observable_image_event(observable_event_class(event), state, next);
                &&& observable_event_same_except_repeated(event, image)
                &&& observable_next(state, image) == Some(next)
            },
            None => true,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let top = frames.last();
            match event {
                ObservableEvent::MessagePush { field, child, .. } => {
                    let parent = observable_parent_after_push(top, Some(field), child);
                    let next_frames = frames.update(frames.len() - 1, parent).push(observable_fresh_frame(child));
                    assert(next_frames[frames.len() - 1] == parent);
                    assert(next_frames.last() == observable_fresh_frame(child));
                    lemma_observable_image_field_exact(top, field, parent);
                },
                ObservableEvent::OtherPush { field, child, .. } => {
                    let parent = observable_parent_after_push(top, field, child);
                    let next_frames = frames.update(frames.len() - 1, parent).push(observable_fresh_frame(child));
                    assert(next_frames[frames.len() - 1] == parent);
                    assert(next_frames.last() == observable_fresh_frame(child));
                    match field {
                        Some(f) => {
                            lemma_observable_image_field_exact(top, f, parent);
                        },
                        None => {},
                    }
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_complete_determined(state: ObservableState, event: ObservableEvent)
    requires
        event is MessageComplete || event is OtherComplete,
        observable_next(state, event) is Some,
    ensures
        match observable_next(state, event) {
            Some(next) => {
                let image = observable_image_event(observable_event_class(event), state, next);
                &&& observable_event_same_except_repeated(event, image)
                &&& observable_next(state, image) == Some(next)
            },
            None => true,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            match event {
                ObservableEvent::MessageComplete { record, .. } => {
                    assert(observable_image_record(frames) == record);
                },
                ObservableEvent::OtherComplete { record, .. } => {
                    assert(observable_image_record(frames) == record);
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_event_determined(state: ObservableState, event: ObservableEvent)
    requires observable_next(state, event) is Some,
    ensures
        match observable_next(state, event) {
            Some(next) => {
                let image = observable_image_event(observable_event_class(event), state, next);
                &&& observable_event_same_except_repeated(event, image)
                &&& observable_next(state, image) == Some(next)
            },
            None => true,
        },
{
    match event {
        ObservableEvent::MessageContinue { .. } => lemma_observable_continue_determined(state, event),
        ObservableEvent::OtherContinue { .. } => lemma_observable_continue_determined(state, event),
        ObservableEvent::MessagePush { .. } => lemma_observable_push_determined(state, event),
        ObservableEvent::OtherPush { .. } => lemma_observable_push_determined(state, event),
        ObservableEvent::MessageComplete { .. } => lemma_observable_complete_determined(state, event),
        ObservableEvent::OtherComplete { .. } => lemma_observable_complete_determined(state, event),
    }
}

pub proof fn lemma_observable_next_repeated_irrelevant(
    state: ObservableState, first: ObservableEvent, second: ObservableEvent,
)
    requires
        observable_event_same_except_repeated(first, second),
        observable_next(state, first) is Some,
        observable_next(state, second) is Some,
    ensures observable_next(state, first) == observable_next(state, second),
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let top = frames.last();
            match (first, second) {
                (ObservableEvent::MessageContinue { field: a, .. },
                 ObservableEvent::MessageContinue { field: b, .. }) => {
                    assert(observable_frame_after_field(top, a) == observable_frame_after_field(top, b));
                },
                (ObservableEvent::OtherContinue { field: Some(a), .. },
                 ObservableEvent::OtherContinue { field: Some(b), .. }) => {
                    assert(observable_frame_after_field(top, a) == observable_frame_after_field(top, b));
                },
                (ObservableEvent::MessagePush { field: a, child: c, .. },
                 ObservableEvent::MessagePush { field: b, child: d, .. }) => {
                    assert(observable_parent_after_push(top, Some(a), c)
                        == observable_parent_after_push(top, Some(b), d));
                },
                (ObservableEvent::OtherPush { field: Some(a), child: c, .. },
                 ObservableEvent::OtherPush { field: Some(b), child: d, .. }) => {
                    assert(observable_parent_after_push(top, Some(a), c)
                        == observable_parent_after_push(top, Some(b), d));
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_accept_appends_child_summary(state: ObservableState, event: ObservableEvent)
    requires
        observable_wf(state),
        event is MessageComplete || event is OtherComplete,
        observable_snapshot(state).depth >= 2,
        observable_next(state, event) is Some,
    ensures
        match (state, observable_next(state, event)) {
            (ObservableState::Active { frames, .. }, Some(ObservableState::Active { frames: next_frames, .. })) => {
                let parent = frames[frames.len() - 2];
                let child = frames.last();
                &&& next_frames.len() + 1 == frames.len()
                &&& next_frames.subrange(0, next_frames.len() - 1) == frames.subrange(0, frames.len() - 2)
                &&& next_frames.last().waiting is None
                &&& next_frames.last().children == if observable_tracks_fields(parent.kind) {
                        parent.children.push(ObservableChild {
                            slot: match parent.waiting {
                                Some(wait) => match wait.slot { Some(slot) => slot, None => 0nat },
                                None => 0nat,
                            },
                            body_len: child.body_len,
                        })
                    } else {
                        parent.children
                    }
            },
            _ => false,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let index = frames.len() - 2;
            let parent = frames[index];
            let child = frames.last();
            match parent.waiting {
                Some(wait) => {
                    let new_parent = observable_parent_after_accept(parent, wait, child);
                    let next_frames = frames.drop_last().update(index, new_parent);
                    assert(next_frames.len() == frames.len() - 1);
                    assert(next_frames.last() == new_parent);
                    assert(next_frames.subrange(0, next_frames.len() - 1) =~= frames.subrange(0, index));
                },
                None => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_slot_record_counts_children(
    state: ObservableState, event: ObservableEvent, slot: nat, occurrences_after: nat,
)
    requires
        observable_wf(state),
        observable_event_record(event) == Some(ObservableAcceptRecord::MessageSlot { slot, occurrences_after }),
        observable_next(state, event) is Some,
    ensures
        match observable_next(state, event) {
            Some(ObservableState::Active { frames, .. }) => {
                &&& frames.last().children.last()
                    == ObservableChild { slot, body_len: observable_top(state).body_len }
                &&& observable_slot_run(frames.last().children, slot) == occurrences_after
            },
            _ => false,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let index = frames.len() - 2;
            let parent = frames[index];
            let child = frames.last();
            match parent.waiting {
                Some(wait) => {
                    let summary = observable_child_summary(wait, child);
                    let new_parent = observable_parent_after_accept(parent, wait, child);
                    let next_frames = frames.drop_last().update(index, new_parent);
                    assert(next_frames.last() == new_parent);
                    assert(new_parent.children == parent.children.push(summary));
                    assert(new_parent.children.last() == summary);
                    lemma_slot_run_push_same(parent.children, summary);
                },
                None => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_set_top_keeps_children(frames: Seq<ObservableFrame>, top: ObservableFrame)
    requires
        frames.len() > 0,
        top.children == frames.last().children,
    ensures
        forall|i: int| 0 <= i < frames.len() ==> observable_children_extend(
            frames[i].children,
            #[trigger] frames.update(frames.len() - 1, top)[i].children,
        ),
{
    let last = frames.len() - 1;
    let next_frames = frames.update(last, top);
    assert forall|i: int| 0 <= i < frames.len()
        implies observable_children_extend(frames[i].children, #[trigger] next_frames[i].children) by {
        if i != last {
            assert(next_frames[i] == frames[i]);
        }
    }
}

pub proof fn lemma_observable_push_keeps_parent_children(
    frames: Seq<ObservableFrame>, parent: ObservableFrame, child: ObservableFrame,
)
    requires
        frames.len() > 0,
        parent.children == frames.last().children,
    ensures
        forall|i: int| 0 <= i < frames.len() ==> observable_children_extend(
            frames[i].children,
            #[trigger] frames.update(frames.len() - 1, parent).push(child)[i].children,
        ),
{
    let last = frames.len() - 1;
    let next_frames = frames.update(last, parent).push(child);
    assert forall|i: int| 0 <= i < frames.len()
        implies observable_children_extend(frames[i].children, #[trigger] next_frames[i].children) by {
        if i != last {
            assert(next_frames[i] == frames[i]);
        } else {
            assert(next_frames[i] == parent);
        }
    }
}

pub proof fn lemma_observable_continue_keeps_children(state: ObservableState, event: ObservableEvent)
    requires
        observable_wf(state),
        event is MessageContinue || event is OtherContinue,
    ensures
        match (state, observable_next(state, event)) {
            (ObservableState::Active { frames, .. }, Some(ObservableState::Active { frames: next_frames, .. })) =>
                forall|i: int| 0 <= i < next_frames.len() && i < frames.len()
                    ==> observable_children_extend(frames[i].children, #[trigger] next_frames[i].children),
            (ObservableState::Active { frames, .. }, Some(ObservableState::Accepted { root, .. })) =>
                root.children == frames[0].children,
            _ => true,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let top = frames.last();
            match event {
                ObservableEvent::MessageContinue { field, .. } => {
                    lemma_observable_set_top_keeps_children(frames, observable_frame_after_field(top, field));
                },
                ObservableEvent::OtherContinue { field: Some(field), .. } => {
                    lemma_observable_set_top_keeps_children(frames, observable_frame_after_field(top, field));
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_push_keeps_children(state: ObservableState, event: ObservableEvent)
    requires
        observable_wf(state),
        observable_event_is_push(event),
    ensures
        match (state, observable_next(state, event)) {
            (ObservableState::Active { frames, .. }, Some(ObservableState::Active { frames: next_frames, .. })) =>
                forall|i: int| 0 <= i < next_frames.len() && i < frames.len()
                    ==> observable_children_extend(frames[i].children, #[trigger] next_frames[i].children),
            (ObservableState::Active { frames, .. }, Some(ObservableState::Accepted { root, .. })) =>
                root.children == frames[0].children,
            _ => true,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let top = frames.last();
            match event {
                ObservableEvent::MessagePush { field, child, .. } => {
                    lemma_observable_push_keeps_parent_children(
                        frames,
                        observable_parent_after_push(top, Some(field), child),
                        observable_fresh_frame(child),
                    );
                },
                ObservableEvent::OtherPush { field, child, .. } => {
                    lemma_observable_push_keeps_parent_children(
                        frames,
                        observable_parent_after_push(top, field, child),
                        observable_fresh_frame(child),
                    );
                },
                _ => {},
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_complete_extends_children(state: ObservableState, event: ObservableEvent)
    requires
        observable_wf(state),
        event is MessageComplete || event is OtherComplete,
    ensures
        match (state, observable_next(state, event)) {
            (ObservableState::Active { frames, .. }, Some(ObservableState::Active { frames: next_frames, .. })) =>
                forall|i: int| 0 <= i < next_frames.len() && i < frames.len()
                    ==> observable_children_extend(frames[i].children, #[trigger] next_frames[i].children),
            (ObservableState::Active { frames, .. }, Some(ObservableState::Accepted { root, .. })) =>
                root.children == frames[0].children,
            _ => true,
        },
{
    match state {
        ObservableState::Active { frames, score_len } => {
            let top = frames.last();
            if frames.len() == 1 {
                assert(top == frames[0]);
            } else {
                let index = frames.len() - 2;
                let parent = frames[index];
                match parent.waiting {
                    Some(wait) => {
                        let new_parent = observable_parent_after_accept(parent, wait, top);
                        let next_frames = frames.drop_last().update(index, new_parent);
                        assert forall|i: int| 0 <= i < next_frames.len() && i < frames.len()
                            implies observable_children_extend(frames[i].children, #[trigger] next_frames[i].children) by {
                            if i == index {
                                assert(next_frames[i] == new_parent);
                                assert(new_parent.children.subrange(0, parent.children.len() as int)
                                    =~= parent.children);
                            } else {
                                assert(next_frames[i] == frames[i]);
                            }
                        }
                    },
                    None => {},
                }
            }
        },
        ObservableState::Accepted { .. } => {},
    }
}

pub proof fn lemma_observable_children_never_dropped(state: ObservableState, event: ObservableEvent)
    requires observable_wf(state),
    ensures
        match (state, observable_next(state, event)) {
            (ObservableState::Active { frames, .. }, Some(ObservableState::Active { frames: next_frames, .. })) =>
                forall|i: int| 0 <= i < next_frames.len() && i < frames.len()
                    ==> observable_children_extend(frames[i].children, #[trigger] next_frames[i].children),
            (ObservableState::Active { frames, .. }, Some(ObservableState::Accepted { root, .. })) =>
                root.children == frames[0].children,
            _ => true,
        },
{
    match event {
        ObservableEvent::MessageContinue { .. } => lemma_observable_continue_keeps_children(state, event),
        ObservableEvent::OtherContinue { .. } => lemma_observable_continue_keeps_children(state, event),
        ObservableEvent::MessagePush { .. } => lemma_observable_push_keeps_children(state, event),
        ObservableEvent::OtherPush { .. } => lemma_observable_push_keeps_children(state, event),
        ObservableEvent::MessageComplete { .. } => lemma_observable_complete_extends_children(state, event),
        ObservableEvent::OtherComplete { .. } => lemma_observable_complete_extends_children(state, event),
    }
}

pub proof fn lemma_observable_trace_push(
    start: ObservableState, events: Seq<ObservableEvent>, event: ObservableEvent,
)
    ensures
        observable_trace(start, events.push(event)) == match observable_trace(start, events) {
            Some(middle) => observable_next(middle, event),
            None => None,
        },
    decreases events.len(),
{
    let pushed = events.push(event);
    if events.len() == 0 {
        assert(pushed[0] == event);
        match observable_next(start, event) {
            Some(next) => {
                assert(observable_trace(next, pushed.subrange(1, pushed.len() as int)) == Some(next));
            },
            None => {},
        }
    } else {
        let tail = events.subrange(1, events.len() as int);
        assert(pushed[0] == events[0]);
        assert(pushed.subrange(1, pushed.len() as int) =~= tail.push(event));
        match observable_next(start, events[0]) {
            Some(next) => {
                lemma_observable_trace_push(next, tail, event);
            },
            None => {},
        }
    }
}

pub proof fn lemma_observable_trace_prefix(start: ObservableState, events: Seq<ObservableEvent>, count: int)
    requires 0 <= count < events.len(),
    ensures
        observable_trace(start, events.subrange(0, count + 1))
            == match observable_trace(start, events.subrange(0, count)) {
                Some(middle) => observable_next(middle, events[count]),
                None => None,
            },
{
    assert(events.subrange(0, count + 1) =~= events.subrange(0, count).push(events[count]));
    lemma_observable_trace_push(start, events.subrange(0, count), events[count]);
}

pub open spec fn witness_frame(
    kind: u8, body_len: nat, cursor: Option<nat>, slot_count: nat, last_slot: Option<nat>,
    waiting: Option<ObservableWait>, children: Seq<ObservableChild>,
) -> ObservableFrame {
    ObservableFrame { kind, body_len, cursor, slot_count, last_slot, waiting, children }
}

pub open spec fn witness_wait(child_kind: u8, slot: Option<nat>) -> Option<ObservableWait> {
    Some(ObservableWait { child_kind, slot })
}

pub open spec fn witness_child(slot: nat, body_len: nat) -> Seq<ObservableChild> {
    Seq::<ObservableChild>::empty().push(ObservableChild { slot, body_len })
}

pub open spec fn witness_frames_one(first: ObservableFrame) -> Seq<ObservableFrame> {
    Seq::<ObservableFrame>::empty().push(first)
}

pub open spec fn witness_frames_two(first: ObservableFrame, second: ObservableFrame) -> Seq<ObservableFrame> {
    Seq::<ObservableFrame>::empty().push(first).push(second)
}

pub open spec fn witness_frames_three(
    first: ObservableFrame, second: ObservableFrame, third: ObservableFrame,
) -> Seq<ObservableFrame> {
    Seq::<ObservableFrame>::empty().push(first).push(second).push(third)
}

pub open spec fn witness_snapshot(depth: nat, top_kind: Option<u8>, score_len: nat) -> ObservableSnapshot {
    ObservableSnapshot { depth, top_kind, score_len }
}

pub open spec fn witness_field(slot: nat, repeated: bool, cursor_after: nat) -> ObservableField {
    ObservableField { slot, repeated, cursor_after }
}

pub open spec fn witness_request(kind: u8, body_len: nat, slot_count: nat) -> ObservableFrameRequest {
    ObservableFrameRequest { kind, body_len, slot_count }
}

pub open spec fn witness_message_root() -> ObservableFrameRequest {
    ObservableFrameRequest { kind: 1, body_len: 10, slot_count: 3 }
}

pub open spec fn witness_message_events() -> Seq<ObservableEvent> {
    Seq::<ObservableEvent>::empty()
        .push(ObservableEvent::MessageContinue {
            before: witness_snapshot(1, Some(1), 0),
            field: witness_field(0, false, 3),
            after: witness_snapshot(1, Some(1), 0),
        })
        .push(ObservableEvent::MessagePush {
            before: witness_snapshot(1, Some(1), 0),
            field: witness_field(1, false, 8),
            child: witness_request(1, 4, 2),
            after: witness_snapshot(2, Some(1), 0),
        })
        .push(ObservableEvent::MessageContinue {
            before: witness_snapshot(2, Some(1), 0),
            field: witness_field(0, false, 4),
            after: witness_snapshot(2, Some(1), 0),
        })
        .push(ObservableEvent::MessageComplete {
            before: witness_snapshot(2, Some(1), 0),
            record: ObservableAcceptRecord::MessageSlot { slot: 1, occurrences_after: 1 },
            after: witness_snapshot(1, Some(1), 1),
        })
        .push(ObservableEvent::MessageContinue {
            before: witness_snapshot(1, Some(1), 1),
            field: witness_field(2, false, 10),
            after: witness_snapshot(1, Some(1), 1),
        })
        .push(ObservableEvent::MessageComplete {
            before: witness_snapshot(1, Some(1), 1),
            record: ObservableAcceptRecord::Root,
            after: witness_snapshot(0, None, 2),
        })
}

pub open spec fn witness_message_accepted_root() -> ObservableFrame {
    ObservableFrame {
        kind: 1, body_len: 10, cursor: Some(10nat), slot_count: 3, last_slot: Some(2nat), waiting: None,
        children: Seq::<ObservableChild>::empty().push(ObservableChild { slot: 1, body_len: 4 }),
    }
}

pub open spec fn witness_message_state(index: nat) -> ObservableState {
    if index == 0 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 10, Some(0), 3, None, None, Seq::empty())),
            score_len: 0,
        }
    } else if index == 1 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 10, Some(3), 3, Some(0), None, Seq::empty())),
            score_len: 0,
        }
    } else if index == 2 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(1, 10, Some(8), 3, Some(1), witness_wait(1, Some(1)), Seq::empty()),
                witness_frame(1, 4, Some(0), 2, None, None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 3 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(1, 10, Some(8), 3, Some(1), witness_wait(1, Some(1)), Seq::empty()),
                witness_frame(1, 4, Some(4), 2, Some(0), None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 4 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 10, Some(8), 3, Some(1), None, witness_child(1, 4))),
            score_len: 1,
        }
    } else if index == 5 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 10, Some(10), 3, Some(2), None, witness_child(1, 4))),
            score_len: 1,
        }
    } else {
        ObservableState::Accepted { root: witness_message_accepted_root(), score_len: 2 }
    }
}

pub proof fn lemma_observable_witness_message_steps()
    ensures
        observable_next(witness_message_state(0), witness_message_events()[0]) == Some(witness_message_state(1)),
        observable_next(witness_message_state(1), witness_message_events()[1]) == Some(witness_message_state(2)),
        observable_next(witness_message_state(2), witness_message_events()[2]) == Some(witness_message_state(3)),
        observable_next(witness_message_state(3), witness_message_events()[3]) == Some(witness_message_state(4)),
        observable_next(witness_message_state(4), witness_message_events()[4]) == Some(witness_message_state(5)),
        observable_next(witness_message_state(5), witness_message_events()[5]) == Some(witness_message_state(6)),
{
    let events = witness_message_events();
    assert(events.len() == 6);
    assert(observable_next(witness_message_state(0), events[0]) == Some(witness_message_state(1))) by {
        let frames = observable_frames_of(witness_message_state(0));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(0, false, 3)))
            =~= observable_frames_of(witness_message_state(1)));
    }
    assert(observable_next(witness_message_state(1), events[1]) == Some(witness_message_state(2))) by {
        let frames = observable_frames_of(witness_message_state(1));
        let request = witness_request(1, 4, 2);
        assert(frames.update(
            frames.len() - 1,
            observable_parent_after_push(frames.last(), Some(witness_field(1, false, 8)), request),
        ).push(observable_fresh_frame(request)) =~= observable_frames_of(witness_message_state(2)));
    }
    assert(observable_next(witness_message_state(2), events[2]) == Some(witness_message_state(3))) by {
        let frames = observable_frames_of(witness_message_state(2));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(0, false, 4)))
            =~= observable_frames_of(witness_message_state(3)));
    }
    assert(observable_next(witness_message_state(3), events[3]) == Some(witness_message_state(4))) by {
        let frames = observable_frames_of(witness_message_state(3));
        let wait = ObservableWait { child_kind: 1, slot: Some(1) };
        assert(frames.drop_last().update(
            frames.len() - 2,
            observable_parent_after_accept(frames[frames.len() - 2], wait, frames.last()),
        ) =~= observable_frames_of(witness_message_state(4)));
    }
    assert(observable_next(witness_message_state(4), events[4]) == Some(witness_message_state(5))) by {
        let frames = observable_frames_of(witness_message_state(4));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(2, false, 10)))
            =~= observable_frames_of(witness_message_state(5)));
    }
    assert(observable_next(witness_message_state(5), events[5]) == Some(witness_message_state(6)));
}

pub proof fn lemma_observable_witness_message_prefixes()
    ensures
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 0))
            == Some(witness_message_state(0)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 1))
            == Some(witness_message_state(1)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 2))
            == Some(witness_message_state(2)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 3))
            == Some(witness_message_state(3)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 4))
            == Some(witness_message_state(4)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 5))
            == Some(witness_message_state(5)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 6))
            == Some(witness_message_state(6)),
{
    let events = witness_message_events();
    let start = observable_initial(witness_message_root(), 0);
    lemma_observable_witness_message_steps();
    assert(events.len() == 6);
    assert(start == witness_message_state(0));
    assert(observable_trace(start, events.subrange(0, 0)) == Some(start));
    lemma_observable_trace_prefix(start, events, 0);
    lemma_observable_trace_prefix(start, events, 1);
    lemma_observable_trace_prefix(start, events, 2);
    lemma_observable_trace_prefix(start, events, 3);
    lemma_observable_trace_prefix(start, events, 4);
    lemma_observable_trace_prefix(start, events, 5);
}

pub proof fn lemma_observable_witness_message_accepts()
    ensures
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events())
            == Some(ObservableState::Accepted { root: witness_message_accepted_root(), score_len: 2 }),
{
    lemma_observable_witness_message_prefixes();
    assert(witness_message_events().subrange(0, 6) =~= witness_message_events());
}

pub open spec fn witness_map_root() -> ObservableFrameRequest {
    ObservableFrameRequest { kind: 1, body_len: 12, slot_count: 2 }
}

pub open spec fn witness_map_events() -> Seq<ObservableEvent> {
    Seq::<ObservableEvent>::empty()
        .push(ObservableEvent::MessagePush {
            before: witness_snapshot(1, Some(1), 0),
            field: witness_field(0, true, 7),
            child: witness_request(2, 6, 2),
            after: witness_snapshot(2, Some(2), 0),
        })
        .push(ObservableEvent::OtherContinue {
            before: witness_snapshot(2, Some(2), 0),
            field: Some(witness_field(0, false, 2)),
            after: witness_snapshot(2, Some(2), 0),
        })
        .push(ObservableEvent::OtherPush {
            before: witness_snapshot(2, Some(2), 0),
            field: Some(witness_field(1, false, 6)),
            child: witness_request(1, 3, 1),
            after: witness_snapshot(3, Some(1), 0),
        })
        .push(ObservableEvent::MessageContinue {
            before: witness_snapshot(3, Some(1), 0),
            field: witness_field(0, false, 3),
            after: witness_snapshot(3, Some(1), 0),
        })
        .push(ObservableEvent::MessageComplete {
            before: witness_snapshot(3, Some(1), 0),
            record: ObservableAcceptRecord::MapValue,
            after: witness_snapshot(2, Some(2), 1),
        })
        .push(ObservableEvent::OtherComplete {
            before: witness_snapshot(2, Some(2), 1),
            record: ObservableAcceptRecord::MessageSlot { slot: 0, occurrences_after: 1 },
            after: witness_snapshot(1, Some(1), 1),
        })
        .push(ObservableEvent::MessageContinue {
            before: witness_snapshot(1, Some(1), 1),
            field: witness_field(1, false, 12),
            after: witness_snapshot(1, Some(1), 1),
        })
        .push(ObservableEvent::MessageComplete {
            before: witness_snapshot(1, Some(1), 1),
            record: ObservableAcceptRecord::Root,
            after: witness_snapshot(0, None, 2),
        })
}

pub open spec fn witness_map_accepted() -> ObservableState {
    ObservableState::Accepted {
        root: witness_frame(1, 12, Some(12), 2, Some(1), None, witness_child(0, 6)),
        score_len: 2,
    }
}

pub open spec fn witness_map_state(index: nat) -> ObservableState {
    if index == 0 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 12, Some(0), 2, None, None, Seq::empty())),
            score_len: 0,
        }
    } else if index == 1 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(1, 12, Some(7), 2, Some(0), witness_wait(2, Some(0)), Seq::empty()),
                witness_frame(2, 6, Some(0), 2, None, None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 2 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(1, 12, Some(7), 2, Some(0), witness_wait(2, Some(0)), Seq::empty()),
                witness_frame(2, 6, Some(2), 2, Some(0), None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 3 {
        ObservableState::Active {
            frames: witness_frames_three(
                witness_frame(1, 12, Some(7), 2, Some(0), witness_wait(2, Some(0)), Seq::empty()),
                witness_frame(2, 6, Some(6), 2, Some(1), witness_wait(1, Some(1)), Seq::empty()),
                witness_frame(1, 3, Some(0), 1, None, None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 4 {
        ObservableState::Active {
            frames: witness_frames_three(
                witness_frame(1, 12, Some(7), 2, Some(0), witness_wait(2, Some(0)), Seq::empty()),
                witness_frame(2, 6, Some(6), 2, Some(1), witness_wait(1, Some(1)), Seq::empty()),
                witness_frame(1, 3, Some(3), 1, Some(0), None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 5 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(1, 12, Some(7), 2, Some(0), witness_wait(2, Some(0)), Seq::empty()),
                witness_frame(2, 6, Some(6), 2, Some(1), None, witness_child(1, 3)),
            ),
            score_len: 1,
        }
    } else if index == 6 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 12, Some(7), 2, Some(0), None, witness_child(0, 6))),
            score_len: 1,
        }
    } else if index == 7 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(1, 12, Some(12), 2, Some(1), None, witness_child(0, 6))),
            score_len: 1,
        }
    } else {
        witness_map_accepted()
    }
}

pub proof fn lemma_observable_witness_map_steps()
    ensures
        observable_next(witness_map_state(0), witness_map_events()[0]) == Some(witness_map_state(1)),
        observable_next(witness_map_state(1), witness_map_events()[1]) == Some(witness_map_state(2)),
        observable_next(witness_map_state(2), witness_map_events()[2]) == Some(witness_map_state(3)),
        observable_next(witness_map_state(3), witness_map_events()[3]) == Some(witness_map_state(4)),
        observable_next(witness_map_state(4), witness_map_events()[4]) == Some(witness_map_state(5)),
        observable_next(witness_map_state(5), witness_map_events()[5]) == Some(witness_map_state(6)),
        observable_next(witness_map_state(6), witness_map_events()[6]) == Some(witness_map_state(7)),
        observable_next(witness_map_state(7), witness_map_events()[7]) == Some(witness_map_state(8)),
{
    let events = witness_map_events();
    assert(events.len() == 8);
    assert(observable_next(witness_map_state(0), events[0]) == Some(witness_map_state(1))) by {
        let frames = observable_frames_of(witness_map_state(0));
        let request = witness_request(2, 6, 2);
        assert(frames.update(
            frames.len() - 1,
            observable_parent_after_push(frames.last(), Some(witness_field(0, true, 7)), request),
        ).push(observable_fresh_frame(request)) =~= observable_frames_of(witness_map_state(1)));
    }
    assert(observable_next(witness_map_state(1), events[1]) == Some(witness_map_state(2))) by {
        let frames = observable_frames_of(witness_map_state(1));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(0, false, 2)))
            =~= observable_frames_of(witness_map_state(2)));
    }
    assert(observable_next(witness_map_state(2), events[2]) == Some(witness_map_state(3))) by {
        let frames = observable_frames_of(witness_map_state(2));
        let request = witness_request(1, 3, 1);
        assert(frames.update(
            frames.len() - 1,
            observable_parent_after_push(frames.last(), Some(witness_field(1, false, 6)), request),
        ).push(observable_fresh_frame(request)) =~= observable_frames_of(witness_map_state(3)));
    }
    assert(observable_next(witness_map_state(3), events[3]) == Some(witness_map_state(4))) by {
        let frames = observable_frames_of(witness_map_state(3));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(0, false, 3)))
            =~= observable_frames_of(witness_map_state(4)));
    }
    assert(observable_next(witness_map_state(4), events[4]) == Some(witness_map_state(5))) by {
        let frames = observable_frames_of(witness_map_state(4));
        let wait = ObservableWait { child_kind: 1, slot: Some(1) };
        assert(frames.drop_last().update(
            frames.len() - 2,
            observable_parent_after_accept(frames[frames.len() - 2], wait, frames.last()),
        ) =~= observable_frames_of(witness_map_state(5)));
    }
    assert(observable_next(witness_map_state(5), events[5]) == Some(witness_map_state(6))) by {
        let frames = observable_frames_of(witness_map_state(5));
        let wait = ObservableWait { child_kind: 2, slot: Some(0) };
        assert(frames.drop_last().update(
            frames.len() - 2,
            observable_parent_after_accept(frames[frames.len() - 2], wait, frames.last()),
        ) =~= observable_frames_of(witness_map_state(6)));
    }
    assert(observable_next(witness_map_state(6), events[6]) == Some(witness_map_state(7))) by {
        let frames = observable_frames_of(witness_map_state(6));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(1, false, 12)))
            =~= observable_frames_of(witness_map_state(7)));
    }
    assert(observable_next(witness_map_state(7), events[7]) == Some(witness_map_state(8)));
}

pub proof fn lemma_observable_witness_map_prefixes()
    ensures
        observable_trace(observable_initial(witness_map_root(), 0), witness_map_events().subrange(0, 1))
            == Some(witness_map_state(1)),
        observable_trace(observable_initial(witness_map_root(), 0), witness_map_events().subrange(0, 8))
            == Some(witness_map_state(8)),
{
    let events = witness_map_events();
    let start = observable_initial(witness_map_root(), 0);
    lemma_observable_witness_map_steps();
    assert(events.len() == 8);
    assert(start == witness_map_state(0));
    assert(observable_trace(start, events.subrange(0, 0)) == Some(start));
    lemma_observable_trace_prefix(start, events, 0);
    lemma_observable_trace_prefix(start, events, 1);
    lemma_observable_trace_prefix(start, events, 2);
    lemma_observable_trace_prefix(start, events, 3);
    lemma_observable_trace_prefix(start, events, 4);
    lemma_observable_trace_prefix(start, events, 5);
    lemma_observable_trace_prefix(start, events, 6);
    lemma_observable_trace_prefix(start, events, 7);
}

pub proof fn lemma_observable_witness_map_accepts()
    ensures
        observable_trace(observable_initial(witness_map_root(), 0), witness_map_events())
            == Some(witness_map_accepted()),
{
    lemma_observable_witness_map_prefixes();
    assert(witness_map_events().subrange(0, 8) =~= witness_map_events());
}

pub open spec fn witness_path_root() -> ObservableFrameRequest {
    ObservableFrameRequest { kind: 4, body_len: 20, slot_count: 0 }
}

pub open spec fn witness_path_events() -> Seq<ObservableEvent> {
    Seq::<ObservableEvent>::empty()
        .push(ObservableEvent::OtherPush {
            before: witness_snapshot(1, Some(4), 0),
            field: None,
            child: witness_request(3, 10, 0),
            after: witness_snapshot(2, Some(3), 0),
        })
        .push(ObservableEvent::OtherContinue {
            before: witness_snapshot(2, Some(3), 0),
            field: None,
            after: witness_snapshot(2, Some(3), 0),
        })
        .push(ObservableEvent::OtherPush {
            before: witness_snapshot(2, Some(3), 0),
            field: None,
            child: witness_request(1, 2, 1),
            after: witness_snapshot(3, Some(1), 0),
        })
        .push(ObservableEvent::MessageContinue {
            before: witness_snapshot(3, Some(1), 0),
            field: witness_field(0, false, 2),
            after: witness_snapshot(3, Some(1), 0),
        })
        .push(ObservableEvent::MessageComplete {
            before: witness_snapshot(3, Some(1), 0),
            record: ObservableAcceptRecord::Reduced,
            after: witness_snapshot(2, Some(3), 1),
        })
        .push(ObservableEvent::OtherComplete {
            before: witness_snapshot(2, Some(3), 1),
            record: ObservableAcceptRecord::Reduced,
            after: witness_snapshot(1, Some(4), 1),
        })
        .push(ObservableEvent::OtherComplete {
            before: witness_snapshot(1, Some(4), 1),
            record: ObservableAcceptRecord::Root,
            after: witness_snapshot(0, None, 1),
        })
}

pub open spec fn witness_path_accepted() -> ObservableState {
    ObservableState::Accepted {
        root: witness_frame(4, 20, None, 0, None, None, Seq::empty()),
        score_len: 1,
    }
}

pub open spec fn witness_path_state(index: nat) -> ObservableState {
    if index == 0 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(4, 20, None, 0, None, None, Seq::empty())),
            score_len: 0,
        }
    } else if index == 1 || index == 2 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(4, 20, None, 0, None, witness_wait(3, None), Seq::empty()),
                witness_frame(3, 10, None, 0, None, None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 3 {
        ObservableState::Active {
            frames: witness_frames_three(
                witness_frame(4, 20, None, 0, None, witness_wait(3, None), Seq::empty()),
                witness_frame(3, 10, None, 0, None, witness_wait(1, None), Seq::empty()),
                witness_frame(1, 2, Some(0), 1, None, None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 4 {
        ObservableState::Active {
            frames: witness_frames_three(
                witness_frame(4, 20, None, 0, None, witness_wait(3, None), Seq::empty()),
                witness_frame(3, 10, None, 0, None, witness_wait(1, None), Seq::empty()),
                witness_frame(1, 2, Some(2), 1, Some(0), None, Seq::empty()),
            ),
            score_len: 0,
        }
    } else if index == 5 {
        ObservableState::Active {
            frames: witness_frames_two(
                witness_frame(4, 20, None, 0, None, witness_wait(3, None), Seq::empty()),
                witness_frame(3, 10, None, 0, None, None, Seq::empty()),
            ),
            score_len: 1,
        }
    } else if index == 6 {
        ObservableState::Active {
            frames: witness_frames_one(witness_frame(4, 20, None, 0, None, None, Seq::empty())),
            score_len: 1,
        }
    } else {
        witness_path_accepted()
    }
}

pub proof fn lemma_observable_witness_path_steps()
    ensures
        observable_next(witness_path_state(0), witness_path_events()[0]) == Some(witness_path_state(1)),
        observable_next(witness_path_state(1), witness_path_events()[1]) == Some(witness_path_state(2)),
        observable_next(witness_path_state(2), witness_path_events()[2]) == Some(witness_path_state(3)),
        observable_next(witness_path_state(3), witness_path_events()[3]) == Some(witness_path_state(4)),
        observable_next(witness_path_state(4), witness_path_events()[4]) == Some(witness_path_state(5)),
        observable_next(witness_path_state(5), witness_path_events()[5]) == Some(witness_path_state(6)),
        observable_next(witness_path_state(6), witness_path_events()[6]) == Some(witness_path_state(7)),
{
    let events = witness_path_events();
    assert(events.len() == 7);
    assert(observable_next(witness_path_state(0), events[0]) == Some(witness_path_state(1))) by {
        let frames = observable_frames_of(witness_path_state(0));
        let request = witness_request(3, 10, 0);
        assert(frames.update(frames.len() - 1, observable_parent_after_push(frames.last(), None, request))
            .push(observable_fresh_frame(request)) =~= observable_frames_of(witness_path_state(1)));
    }
    assert(observable_next(witness_path_state(1), events[1]) == Some(witness_path_state(2)));
    assert(observable_next(witness_path_state(2), events[2]) == Some(witness_path_state(3))) by {
        let frames = observable_frames_of(witness_path_state(2));
        let request = witness_request(1, 2, 1);
        assert(frames.update(frames.len() - 1, observable_parent_after_push(frames.last(), None, request))
            .push(observable_fresh_frame(request)) =~= observable_frames_of(witness_path_state(3)));
    }
    assert(observable_next(witness_path_state(3), events[3]) == Some(witness_path_state(4))) by {
        let frames = observable_frames_of(witness_path_state(3));
        assert(frames.update(frames.len() - 1, observable_frame_after_field(frames.last(), witness_field(0, false, 2)))
            =~= observable_frames_of(witness_path_state(4)));
    }
    assert(observable_next(witness_path_state(4), events[4]) == Some(witness_path_state(5))) by {
        let frames = observable_frames_of(witness_path_state(4));
        let wait = ObservableWait { child_kind: 1, slot: None };
        assert(frames.drop_last().update(
            frames.len() - 2,
            observable_parent_after_accept(frames[frames.len() - 2], wait, frames.last()),
        ) =~= observable_frames_of(witness_path_state(5)));
    }
    assert(observable_next(witness_path_state(5), events[5]) == Some(witness_path_state(6))) by {
        let frames = observable_frames_of(witness_path_state(5));
        let wait = ObservableWait { child_kind: 3, slot: None };
        assert(frames.drop_last().update(
            frames.len() - 2,
            observable_parent_after_accept(frames[frames.len() - 2], wait, frames.last()),
        ) =~= observable_frames_of(witness_path_state(6)));
    }
    assert(observable_next(witness_path_state(6), events[6]) == Some(witness_path_state(7)));
}

pub proof fn lemma_observable_witness_path_prefixes()
    ensures
        observable_trace(observable_initial(witness_path_root(), 0), witness_path_events().subrange(0, 1))
            == Some(witness_path_state(1)),
        observable_trace(observable_initial(witness_path_root(), 0), witness_path_events().subrange(0, 5))
            == Some(witness_path_state(5)),
        observable_trace(observable_initial(witness_path_root(), 0), witness_path_events().subrange(0, 7))
            == Some(witness_path_state(7)),
{
    let events = witness_path_events();
    let start = observable_initial(witness_path_root(), 0);
    lemma_observable_witness_path_steps();
    assert(events.len() == 7);
    assert(start == witness_path_state(0));
    assert(observable_trace(start, events.subrange(0, 0)) == Some(start));
    lemma_observable_trace_prefix(start, events, 0);
    lemma_observable_trace_prefix(start, events, 1);
    lemma_observable_trace_prefix(start, events, 2);
    lemma_observable_trace_prefix(start, events, 3);
    lemma_observable_trace_prefix(start, events, 4);
    lemma_observable_trace_prefix(start, events, 5);
    lemma_observable_trace_prefix(start, events, 6);
}

pub proof fn lemma_observable_witness_canonical_path_accepts()
    ensures
        observable_trace(observable_initial(witness_path_root(), 0), witness_path_events())
            == Some(witness_path_accepted()),
{
    lemma_observable_witness_path_prefixes();
    assert(witness_path_events().subrange(0, 7) =~= witness_path_events());
}

pub open spec fn witness_refusal_state(index: nat) -> ObservableState {
    if index == 1 {
        witness_message_state(0)
    } else if index == 2 {
        witness_message_state(1)
    } else if index == 3 {
        witness_message_state(0)
    } else if index == 4 {
        witness_message_state(2)
    } else if index == 5 {
        witness_message_state(3)
    } else if index == 6 {
        witness_path_state(5)
    } else if index == 7 {
        witness_message_state(0)
    } else if index == 8 {
        witness_message_state(6)
    } else if index == 9 {
        witness_map_state(1)
    } else {
        witness_path_state(1)
    }
}

pub open spec fn witness_refusal_event(index: nat) -> ObservableEvent {
    if index == 1 {
        ObservableEvent::MessageContinue {
            before: witness_snapshot(1, Some(1), 0),
            field: witness_field(0, false, 11),
            after: witness_snapshot(1, Some(1), 0),
        }
    } else if index == 2 {
        ObservableEvent::MessageContinue {
            before: witness_snapshot(1, Some(1), 0),
            field: witness_field(0, false, 5),
            after: witness_snapshot(1, Some(1), 0),
        }
    } else if index == 3 {
        ObservableEvent::MessagePush {
            before: witness_snapshot(1, Some(1), 0),
            field: witness_field(0, false, 3),
            child: witness_request(4, 2, 0),
            after: witness_snapshot(2, Some(4), 0),
        }
    } else if index == 4 {
        ObservableEvent::MessageComplete {
            before: witness_snapshot(2, Some(1), 0),
            record: ObservableAcceptRecord::MessageSlot { slot: 1, occurrences_after: 1 },
            after: witness_snapshot(1, Some(1), 1),
        }
    } else if index == 5 {
        ObservableEvent::MessageComplete {
            before: witness_snapshot(2, Some(1), 0),
            record: ObservableAcceptRecord::MessageSlot { slot: 1, occurrences_after: 2 },
            after: witness_snapshot(1, Some(1), 1),
        }
    } else if index == 6 {
        ObservableEvent::OtherComplete {
            before: witness_snapshot(2, Some(3), 1),
            record: ObservableAcceptRecord::Reduced,
            after: witness_snapshot(1, Some(4), 2),
        }
    } else if index == 7 {
        ObservableEvent::MessageContinue {
            before: witness_snapshot(2, Some(1), 0),
            field: witness_field(0, false, 3),
            after: witness_snapshot(2, Some(1), 0),
        }
    } else if index == 8 {
        ObservableEvent::MessageContinue {
            before: witness_snapshot(0, None, 2),
            field: witness_field(0, false, 3),
            after: witness_snapshot(0, None, 2),
        }
    } else if index == 9 {
        ObservableEvent::OtherContinue {
            before: witness_snapshot(2, Some(2), 0),
            field: Some(witness_field(1, false, 2)),
            after: witness_snapshot(2, Some(2), 0),
        }
    } else {
        ObservableEvent::OtherPush {
            before: witness_snapshot(2, Some(3), 0),
            field: None,
            child: witness_request(4, 2, 0),
            after: witness_snapshot(3, Some(4), 0),
        }
    }
}

pub proof fn lemma_observable_refusal_witnesses()
    ensures
        observable_next(witness_refusal_state(1), witness_refusal_event(1)) is None,
        observable_next(witness_refusal_state(2), witness_refusal_event(2)) is None,
        observable_next(witness_refusal_state(3), witness_refusal_event(3)) is None,
        observable_next(witness_refusal_state(4), witness_refusal_event(4)) is None,
        observable_next(witness_refusal_state(5), witness_refusal_event(5)) is None,
        observable_next(witness_refusal_state(6), witness_refusal_event(6)) is None,
        observable_next(witness_refusal_state(7), witness_refusal_event(7)) is None,
        observable_next(witness_refusal_state(8), witness_refusal_event(8)) is None,
        observable_next(witness_refusal_state(9), witness_refusal_event(9)) is None,
        observable_next(witness_refusal_state(10), witness_refusal_event(10)) is None,
{
}

pub proof fn lemma_observable_refusal_states_reachable()
    ensures
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 0))
            == Some(witness_refusal_state(1)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 1))
            == Some(witness_refusal_state(2)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 0))
            == Some(witness_refusal_state(3)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 2))
            == Some(witness_refusal_state(4)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 3))
            == Some(witness_refusal_state(5)),
        observable_trace(observable_initial(witness_path_root(), 0), witness_path_events().subrange(0, 5))
            == Some(witness_refusal_state(6)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events().subrange(0, 0))
            == Some(witness_refusal_state(7)),
        observable_trace(observable_initial(witness_message_root(), 0), witness_message_events())
            == Some(witness_refusal_state(8)),
        observable_trace(observable_initial(witness_map_root(), 0), witness_map_events().subrange(0, 1))
            == Some(witness_refusal_state(9)),
        observable_trace(observable_initial(witness_path_root(), 0), witness_path_events().subrange(0, 1))
            == Some(witness_refusal_state(10)),
{
    lemma_observable_witness_message_prefixes();
    lemma_observable_witness_message_accepts();
    lemma_observable_witness_map_prefixes();
    lemma_observable_witness_path_prefixes();
}

} // verus!
