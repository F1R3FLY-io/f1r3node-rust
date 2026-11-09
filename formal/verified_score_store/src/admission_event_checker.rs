//! Verified runtime monitor for the observable admission machine (admission_observable_spec).
//! `check_event` succeeds exactly on the events `observable_next` accepts, refines one observable
//! step when it does, and leaves the monitor image unchanged on every refusal.

use vstd::prelude::*;

use crate::admission_observable_spec::*;

verus! {

#[derive(Clone, Copy, Debug)]
pub struct ObservedSnapshot {
    pub depth: usize,
    pub top_kind: Option<u8>,
    pub score_len: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ObservedField {
    pub slot: usize,
    pub repeated: bool,
    pub cursor_after: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ObservedFrameRequest {
    pub kind: u8,
    pub body_len: usize,
    pub slot_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum ObservedAcceptRecord {
    Root,
    MessageSlot { slot: usize, occurrences_after: usize },
    MapValue,
    Reduced,
}

#[derive(Clone, Copy, Debug)]
pub enum ObservedEvent {
    MessageContinue {
        before: ObservedSnapshot,
        field: ObservedField,
        after: ObservedSnapshot,
    },
    MessagePush {
        before: ObservedSnapshot,
        field: ObservedField,
        child: ObservedFrameRequest,
        after: ObservedSnapshot,
    },
    MessageComplete {
        before: ObservedSnapshot,
        record: ObservedAcceptRecord,
        after: ObservedSnapshot,
    },
    OtherContinue {
        before: ObservedSnapshot,
        field: Option<ObservedField>,
        after: ObservedSnapshot,
    },
    OtherPush {
        before: ObservedSnapshot,
        field: Option<ObservedField>,
        child: ObservedFrameRequest,
        after: ObservedSnapshot,
    },
    OtherComplete {
        before: ObservedSnapshot,
        record: ObservedAcceptRecord,
        after: ObservedSnapshot,
    },
}

pub open spec fn observed_option_nat(value: Option<usize>) -> Option<nat> {
    match value {
        Some(v) => Some(v as nat),
        None => None,
    }
}

pub open spec fn observed_field_view(field: Option<ObservedField>) -> Option<ObservableField> {
    match field {
        Some(f) => Some(f@),
        None => None,
    }
}

impl View for ObservedSnapshot {
    type V = ObservableSnapshot;

    open spec fn view(&self) -> ObservableSnapshot {
        ObservableSnapshot {
            depth: self.depth as nat,
            top_kind: self.top_kind,
            score_len: self.score_len as nat,
        }
    }
}

impl View for ObservedField {
    type V = ObservableField;

    open spec fn view(&self) -> ObservableField {
        ObservableField {
            slot: self.slot as nat,
            repeated: self.repeated,
            cursor_after: self.cursor_after as nat,
        }
    }
}

impl View for ObservedFrameRequest {
    type V = ObservableFrameRequest;

    open spec fn view(&self) -> ObservableFrameRequest {
        ObservableFrameRequest {
            kind: self.kind,
            body_len: self.body_len as nat,
            slot_count: self.slot_count as nat,
        }
    }
}

impl View for ObservedAcceptRecord {
    type V = ObservableAcceptRecord;

    open spec fn view(&self) -> ObservableAcceptRecord {
        match *self {
            ObservedAcceptRecord::Root => ObservableAcceptRecord::Root,
            ObservedAcceptRecord::MessageSlot { slot, occurrences_after } =>
                ObservableAcceptRecord::MessageSlot {
                    slot: slot as nat,
                    occurrences_after: occurrences_after as nat,
                },
            ObservedAcceptRecord::MapValue => ObservableAcceptRecord::MapValue,
            ObservedAcceptRecord::Reduced => ObservableAcceptRecord::Reduced,
        }
    }
}

impl View for ObservedEvent {
    type V = ObservableEvent;

    open spec fn view(&self) -> ObservableEvent {
        match *self {
            ObservedEvent::MessageContinue { before, field, after } =>
                ObservableEvent::MessageContinue { before: before@, field: field@, after: after@ },
            ObservedEvent::MessagePush { before, field, child, after } =>
                ObservableEvent::MessagePush {
                    before: before@,
                    field: field@,
                    child: child@,
                    after: after@,
                },
            ObservedEvent::MessageComplete { before, record, after } =>
                ObservableEvent::MessageComplete { before: before@, record: record@, after: after@ },
            ObservedEvent::OtherContinue { before, field, after } =>
                ObservableEvent::OtherContinue {
                    before: before@,
                    field: observed_field_view(field),
                    after: after@,
                },
            ObservedEvent::OtherPush { before, field, child, after } =>
                ObservableEvent::OtherPush {
                    before: before@,
                    field: observed_field_view(field),
                    child: child@,
                    after: after@,
                },
            ObservedEvent::OtherComplete { before, record, after } =>
                ObservableEvent::OtherComplete { before: before@, record: record@, after: after@ },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorRejection {
    AlreadyAccepted,
    SnapshotMismatch,
    EventClassMismatch,
    FieldRefused,
    RequestRefused,
    CompletionRefused,
    AcceptRefused,
    ScoreRefused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorRefusal {
    Rejected(MonitorRejection),
    CapacityExhausted,
}

#[derive(Clone, Copy, Debug)]
struct MonitorWait {
    child_kind: u8,
    slot: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct MonitorFrame {
    kind: u8,
    body_len: usize,
    cursor: Option<usize>,
    slot_count: usize,
    last_slot: Option<usize>,
    waiting: Option<MonitorWait>,
    last_child_slot: Option<usize>,
    last_slot_run: usize,
    #[cfg_attr(
        not(verus_keep_ghost),
        expect(
            dead_code,
            reason = "read only by spec and proof code, which verus! erases from rustc builds"
        )
    )]
    children: Ghost<Seq<ObservableChild>>,
}

spec fn monitor_wait_spec(wait: MonitorWait) -> ObservableWait {
    ObservableWait { child_kind: wait.child_kind, slot: observed_option_nat(wait.slot) }
}

spec fn monitor_wait_view(waiting: Option<MonitorWait>) -> Option<ObservableWait> {
    match waiting {
        Some(wait) => Some(monitor_wait_spec(wait)),
        None => None,
    }
}

impl View for MonitorFrame {
    type V = ObservableFrame;

    closed spec fn view(&self) -> ObservableFrame {
        ObservableFrame {
            kind: self.kind,
            body_len: self.body_len as nat,
            cursor: observed_option_nat(self.cursor),
            slot_count: self.slot_count as nat,
            last_slot: observed_option_nat(self.last_slot),
            waiting: monitor_wait_view(self.waiting),
            children: self.children@,
        }
    }
}

spec fn frame_counters_wf(frame: MonitorFrame) -> bool {
    frame.kind == 1 ==> match frame.last_child_slot {
        Some(slot) => {
            &&& frame.children@.len() > 0
            &&& frame.children@.last().slot == slot as nat
            &&& frame.last_slot_run as nat == observable_slot_run(frame.children@, slot as nat)
        },
        None => frame.children@.len() == 0,
    }
}

spec fn same_counters(left: MonitorFrame, right: MonitorFrame) -> bool {
    &&& left.kind == right.kind
    &&& left.last_child_slot == right.last_child_slot
    &&& left.last_slot_run == right.last_slot_run
    &&& left.children@ == right.children@
}

spec fn monitor_frames_view(frames: Seq<MonitorFrame>) -> Seq<ObservableFrame> {
    frames.map_values(|frame: MonitorFrame| frame@)
}

proof fn lemma_frames_view_set_top(frames: Seq<MonitorFrame>, frame: MonitorFrame)
    requires frames.len() > 0
    ensures
        monitor_frames_view(frames.update(frames.len() - 1, frame))
            == monitor_frames_view(frames).update(frames.len() - 1, frame@),
{
    assert(monitor_frames_view(frames.update(frames.len() - 1, frame))
        =~= monitor_frames_view(frames).update(frames.len() - 1, frame@));
}

proof fn lemma_frames_view_push(frames: Seq<MonitorFrame>, frame: MonitorFrame)
    ensures monitor_frames_view(frames.push(frame)) == monitor_frames_view(frames).push(frame@)
{
    assert(monitor_frames_view(frames.push(frame)) =~= monitor_frames_view(frames).push(frame@));
}

proof fn lemma_frames_view_pop_set(frames: Seq<MonitorFrame>, frame: MonitorFrame)
    requires frames.len() >= 2
    ensures
        monitor_frames_view(frames.drop_last().update(frames.len() - 2, frame))
            == monitor_frames_view(frames).drop_last().update(frames.len() - 2, frame@),
{
    assert(monitor_frames_view(frames.drop_last().update(frames.len() - 2, frame))
        =~= monitor_frames_view(frames).drop_last().update(frames.len() - 2, frame@));
}

proof fn lemma_counters_after_accept(
    children: Seq<ObservableChild>, slot: usize, body_len: nat, occurrences_after: usize,
)
    requires occurrences_after as nat == observable_slot_run(children, slot as nat) + 1
    ensures
        children.push(ObservableChild { slot: slot as nat, body_len }).len() > 0,
        children.push(ObservableChild { slot: slot as nat, body_len }).last().slot == slot as nat,
        observable_slot_run(children.push(ObservableChild { slot: slot as nat, body_len }), slot as nat)
            == occurrences_after as nat,
{
    lemma_slot_run_push_same(children, ObservableChild { slot: slot as nat, body_len });
}

pub struct AdmissionMonitor {
    frames: Vec<MonitorFrame>,
    score_len: usize,
    accepted_root: Option<MonitorFrame>,
    max_depth: usize,
    checked_events: u64,
}

pub struct AdmissionMonitorImage {
    pub ghost frames: Seq<MonitorFrame>,
    pub score_len: usize,
    pub ghost accepted_root: Option<MonitorFrame>,
    pub max_depth: usize,
    pub checked_events: u64,
}

impl View for AdmissionMonitor {
    type V = ObservableState;

    closed spec fn view(&self) -> ObservableState {
        match self.accepted_root {
            Some(root) => ObservableState::Accepted { root: root@, score_len: self.score_len as nat },
            None => ObservableState::Active {
                frames: monitor_frames_view(self.frames@),
                score_len: self.score_len as nat,
            },
        }
    }
}

fn tracks_fields(kind: u8) -> (tracks: bool)
    ensures tracks == observable_tracks_fields(kind)
{
    kind == 1 || kind == 2
}

fn root_admissible(root: &ObservedFrameRequest) -> (admissible: bool)
    ensures admissible == observable_root_admissible(root@)
{
    root.kind == 1 || (root.kind == 4 && root.slot_count == 0)
}

fn fresh_frame(request: &ObservedFrameRequest) -> (frame: MonitorFrame)
    ensures
        frame@ == observable_fresh_frame(request@),
        frame_counters_wf(frame),
{
    MonitorFrame {
        kind: request.kind,
        body_len: request.body_len,
        cursor: if tracks_fields(request.kind) { Some(0) } else { None },
        slot_count: request.slot_count,
        last_slot: None,
        waiting: None,
        last_child_slot: None,
        last_slot_run: 0,
        children: Ghost(Seq::<ObservableChild>::empty()),
    }
}

fn class_matches(message: bool, kind: u8) -> (matches: bool)
    ensures matches == observable_class_matches(message, kind)
{
    if message {
        kind == 1
    } else {
        kind == 2 || kind == 3 || kind == 4
    }
}

fn push_allowed(parent_kind: u8, child_kind: u8) -> (allowed: bool)
    ensures allowed == observable_push_allowed(parent_kind, child_kind)
{
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

fn slot_admissible(
    previous: Option<usize>, slot: usize, slot_count: usize, repeated: bool,
) -> (admissible: bool)
    ensures
        admissible == observable_slot_admissible(
            observed_option_nat(previous), slot as nat, slot_count as nat, repeated,
        ),
{
    slot < slot_count && match previous {
        Some(last) => slot > last || (slot == last && repeated),
        None => true,
    }
}

fn field_admissible(frame: &MonitorFrame, field: &ObservedField) -> (admissible: bool)
    ensures admissible == observable_field_admissible(frame@, field@)
{
    let cursor_ok = match frame.cursor {
        Some(cursor) => cursor < field.cursor_after && field.cursor_after <= frame.body_len,
        None => false,
    };
    tracks_fields(frame.kind) && frame.waiting.is_none() && cursor_ok
        && slot_admissible(frame.last_slot, field.slot, frame.slot_count, field.repeated)
}

fn map_key_field(frame: &MonitorFrame, field: &ObservedField) -> (key: bool)
    ensures key == observable_map_key_field(frame@, field@)
{
    frame.slot_count == 2 && field.slot == 0 && !field.repeated
}

fn map_value_field(frame: &MonitorFrame, field: &ObservedField) -> (value: bool)
    ensures value == observable_map_value_field(frame@, field@)
{
    frame.slot_count == 2 && field.slot == 1 && !field.repeated
}

fn request_admissible(parent: &MonitorFrame, request: &ObservedFrameRequest) -> (admissible: bool)
    ensures admissible == observable_request_admissible(parent@, request@)
{
    push_allowed(parent.kind, request.kind) && request.body_len <= parent.body_len
        && (request.kind != 3 || request.slot_count == 0)
}

fn push_parent_ok(top: &MonitorFrame, field: Option<ObservedField>) -> (ok: bool)
    ensures ok == observable_push_parent_ok(top@, observed_field_view(field))
{
    if tracks_fields(top.kind) {
        match field {
            Some(field) => field_admissible(top, &field)
                && (top.kind != 2 || map_value_field(top, &field)),
            None => false,
        }
    } else {
        field.is_none()
    }
}

fn frame_finished(frame: &MonitorFrame) -> (finished: bool)
    ensures finished == observable_frame_finished(frame@)
{
    let cursor_done = match frame.cursor {
        Some(cursor) => cursor == frame.body_len,
        None => false,
    };
    frame.waiting.is_none() && (!tracks_fields(frame.kind) || cursor_done)
}

fn score_ok(message: bool, score_len: usize, score_after: usize) -> (ok: bool)
    ensures ok == observable_score_ok(message, score_len as nat, score_after as nat)
{
    if message {
        score_after >= score_len
    } else {
        score_after == score_len
    }
}

fn frame_after_field(frame: &MonitorFrame, field: &ObservedField) -> (moved: MonitorFrame)
    ensures
        moved@ == observable_frame_after_field(frame@, field@),
        same_counters(*frame, moved),
{
    MonitorFrame { cursor: Some(field.cursor_after), last_slot: Some(field.slot), ..*frame }
}

fn parent_after_push(
    top: &MonitorFrame, field: Option<ObservedField>, request: &ObservedFrameRequest,
) -> (parent: MonitorFrame)
    ensures
        parent@ == observable_parent_after_push(top@, observed_field_view(field), request@),
        same_counters(*top, parent),
{
    match field {
        Some(field) => MonitorFrame {
            cursor: Some(field.cursor_after),
            last_slot: Some(field.slot),
            waiting: Some(MonitorWait { child_kind: request.kind, slot: Some(field.slot) }),
            ..*top
        },
        None => MonitorFrame {
            waiting: Some(MonitorWait { child_kind: request.kind, slot: None }),
            ..*top
        },
    }
}

fn slot_run(frame: &MonitorFrame, slot: usize) -> (run: usize)
    requires
        frame_counters_wf(*frame),
        frame.kind == 1,
    ensures run as nat == observable_slot_run(frame.children@, slot as nat)
{
    match frame.last_child_slot {
        Some(last) => {
            if last == slot {
                frame.last_slot_run
            } else {
                proof {
                    lemma_slot_run_last_differs(frame.children@, slot as nat);
                }
                0
            }
        },
        None => {
            proof {
                lemma_slot_run_last_differs(frame.children@, slot as nat);
            }
            0
        },
    }
}

fn parent_after_accept(
    parent: &MonitorFrame, wait: MonitorWait, child: &MonitorFrame, record: &ObservedAcceptRecord,
) -> (accepted: Option<MonitorFrame>)
    requires
        parent.waiting == Some(wait),
        frame_counters_wf(*parent),
    ensures
        match accepted {
            Some(next_parent) => {
                &&& observable_accept_record_valid(parent@, monitor_wait_spec(wait), child@, record@)
                &&& next_parent@
                    == observable_parent_after_accept(parent@, monitor_wait_spec(wait), child@)
                &&& frame_counters_wf(next_parent)
            },
            None => !observable_accept_record_valid(
                parent@, monitor_wait_spec(wait), child@, record@,
            ),
        },
{
    match *record {
        ObservedAcceptRecord::Root => None,
        ObservedAcceptRecord::MessageSlot { slot, occurrences_after } => {
            let wait_matches = match wait.slot {
                Some(wait_slot) => wait_slot == slot,
                None => false,
            };
            if parent.kind != 1 || !wait_matches {
                return None;
            }
            let run = slot_run(parent, slot);
            if occurrences_after == 0 || occurrences_after - 1 != run {
                return None;
            }
            proof {
                lemma_counters_after_accept(
                    parent.children@, slot, child.body_len as nat, occurrences_after,
                );
            }
            Some(MonitorFrame {
                waiting: None,
                last_child_slot: Some(slot),
                last_slot_run: occurrences_after,
                children: Ghost(parent.children@.push(
                    observable_child_summary(monitor_wait_spec(wait), child@),
                )),
                ..*parent
            })
        },
        ObservedAcceptRecord::MapValue => {
            let value_slot = match wait.slot {
                Some(wait_slot) => wait_slot == 1,
                None => false,
            };
            if parent.kind == 2 && child.kind == 1 && value_slot {
                Some(MonitorFrame {
                    waiting: None,
                    children: Ghost(parent.children@.push(
                        observable_child_summary(monitor_wait_spec(wait), child@),
                    )),
                    ..*parent
                })
            } else {
                None
            }
        },
        ObservedAcceptRecord::Reduced => {
            if parent.kind == 3 || parent.kind == 4 {
                Some(MonitorFrame { waiting: None, ..*parent })
            } else {
                None
            }
        },
    }
}

fn kind_matches(observed: Option<u8>, kind: u8) -> (matches: bool)
    ensures matches == (observed == Some(kind))
{
    match observed {
        Some(observed_kind) => observed_kind == kind,
        None => false,
    }
}

fn event_before(event: &ObservedEvent) -> (before: ObservedSnapshot)
    ensures before@ == observable_event_before(event@)
{
    match *event {
        ObservedEvent::MessageContinue { before, .. } => before,
        ObservedEvent::MessagePush { before, .. } => before,
        ObservedEvent::MessageComplete { before, .. } => before,
        ObservedEvent::OtherContinue { before, .. } => before,
        ObservedEvent::OtherPush { before, .. } => before,
        ObservedEvent::OtherComplete { before, .. } => before,
    }
}

fn event_after(event: &ObservedEvent) -> (after: ObservedSnapshot)
    ensures after@ == observable_event_after(event@)
{
    match *event {
        ObservedEvent::MessageContinue { after, .. } => after,
        ObservedEvent::MessagePush { after, .. } => after,
        ObservedEvent::MessageComplete { after, .. } => after,
        ObservedEvent::OtherContinue { after, .. } => after,
        ObservedEvent::OtherPush { after, .. } => after,
        ObservedEvent::OtherComplete { after, .. } => after,
    }
}

spec fn event_score_after(score_len: usize, event: ObservedEvent) -> usize {
    match event {
        ObservedEvent::MessageComplete { after, .. } => after.score_len,
        ObservedEvent::OtherComplete { after, .. } => after.score_len,
        _ => score_len,
    }
}

fn score_after_event(score_len: usize, event: &ObservedEvent) -> (score_after: usize)
    ensures score_after == event_score_after(score_len, *event)
{
    match *event {
        ObservedEvent::MessageComplete { after, .. } => after.score_len,
        ObservedEvent::OtherComplete { after, .. } => after.score_len,
        _ => score_len,
    }
}

enum MonitorPlan {
    Identity,
    SetTop { frame: MonitorFrame },
    SetTopAndPush { parent: MonitorFrame, child: MonitorFrame },
    PopAndSetParent { parent: MonitorFrame },
    AcceptRoot,
}

impl AdmissionMonitor {
    pub closed spec fn image(&self) -> AdmissionMonitorImage {
        AdmissionMonitorImage {
            frames: self.frames@,
            score_len: self.score_len,
            accepted_root: self.accepted_root,
            max_depth: self.max_depth,
            checked_events: self.checked_events,
        }
    }

    pub closed spec fn max_depth_spec(&self) -> nat {
        self.max_depth as nat
    }

    pub closed spec fn checked_events_spec(&self) -> nat {
        self.checked_events as nat
    }

    pub closed spec fn wf(&self) -> bool {
        &&& observable_wf(self@)
        &&& forall|i: int| 0 <= i < self.frames@.len()
            ==> frame_counters_wf(#[trigger] self.frames@[i])
        &&& match self.accepted_root {
            Some(root) => self.frames@.len() == 0 && frame_counters_wf(root),
            None => self.frames@.len() > 0,
        }
        &&& self.frames@.len() <= self.max_depth
    }

    pub fn new(
        root: &ObservedFrameRequest, score_len: usize, max_depth: usize, initial_capacity: usize,
    ) -> (result: Result<AdmissionMonitor, MonitorRefusal>)
        ensures
            match result {
                Ok(monitor) => {
                    &&& monitor.wf()
                    &&& observable_root_admissible(root@)
                    &&& monitor@ == observable_initial(root@, score_len as nat)
                    &&& monitor.max_depth_spec() == max_depth as nat
                    &&& monitor.checked_events_spec() == 0
                },
                Err(MonitorRefusal::Rejected(_)) => !observable_root_admissible(root@),
                Err(MonitorRefusal::CapacityExhausted) => observable_root_admissible(root@),
            },
    {
        if !root_admissible(root) {
            return Err(MonitorRefusal::Rejected(MonitorRejection::RequestRefused));
        }
        if max_depth == 0 {
            return Err(MonitorRefusal::CapacityExhausted);
        }
        let capacity = if initial_capacity == 0 { 1 } else { initial_capacity };
        let mut frames: Vec<MonitorFrame> = Vec::new();
        if frames.try_reserve(capacity).is_err() {
            return Err(MonitorRefusal::CapacityExhausted);
        }
        let root_frame = fresh_frame(root);
        frames.push(root_frame);
        proof {
            lemma_observable_initial_wf(root@, score_len as nat);
            assert(monitor_frames_view(frames@) =~= Seq::<ObservableFrame>::empty().push(
                observable_fresh_frame(root@),
            ));
        }
        Ok(AdmissionMonitor { frames, score_len, accepted_root: None, max_depth, checked_events: 0 })
    }

    pub fn check_event(&mut self, event: &ObservedEvent) -> (result: Result<(), MonitorRefusal>)
        requires old(self).wf()
        ensures
            final(self).wf(),
            final(self).max_depth_spec() == old(self).max_depth_spec(),
            match result {
                Ok(()) => {
                    &&& observable_step(old(self)@, final(self)@, event@)
                    &&& final(self).checked_events_spec() == old(self).checked_events_spec() + 1
                },
                Err(MonitorRefusal::Rejected(_)) => {
                    &&& observable_next(old(self)@, event@) is None
                    &&& final(self).image() == old(self).image()
                },
                Err(MonitorRefusal::CapacityExhausted) => {
                    &&& observable_next(old(self)@, event@) is Some
                    &&& (old(self).checked_events_spec() == u64::MAX as nat
                        || observable_event_is_push(event@))
                    &&& final(self).image() == old(self).image()
                },
            },
    {
        if self.accepted_root.is_some() {
            return Err(MonitorRefusal::Rejected(MonitorRejection::AlreadyAccepted));
        }
        if !self.snapshot_matches(&event_before(event)) {
            return Err(MonitorRefusal::Rejected(MonitorRejection::SnapshotMismatch));
        }
        let plan = match self.plan_event(event) {
            Ok(plan) => plan,
            Err(rejection) => return Err(MonitorRefusal::Rejected(rejection)),
        };
        let score_after = score_after_event(self.score_len, event);
        if !self.after_matches(&plan, score_after, &event_after(event)) {
            return Err(MonitorRefusal::Rejected(MonitorRejection::SnapshotMismatch));
        }
        if self.checked_events == u64::MAX {
            return Err(MonitorRefusal::CapacityExhausted);
        }
        if let MonitorPlan::SetTopAndPush { .. } = plan {
            if self.frames.len() >= self.max_depth {
                return Err(MonitorRefusal::CapacityExhausted);
            }
            if self.frames.try_reserve(1).is_err() {
                return Err(MonitorRefusal::CapacityExhausted);
            }
        }
        proof {
            lemma_observable_next_preserves_wf(old(self)@, event@);
        }
        self.apply(plan, score_after);
        self.checked_events += 1;
        Ok(())
    }

    pub fn is_accepted(&self) -> (accepted: bool)
        requires self.wf()
        ensures accepted == (self@ is Accepted)
    {
        self.accepted_root.is_some()
    }

    pub fn depth(&self) -> (depth: usize)
        requires self.wf()
        ensures depth as nat == observable_snapshot(self@).depth
    {
        self.frames.len()
    }

    pub fn checked_events(&self) -> (count: u64)
        ensures count as nat == self.checked_events_spec()
    {
        self.checked_events
    }

    spec fn plan_view(&self, plan: MonitorPlan, score_after: usize) -> ObservableState {
        let frames = monitor_frames_view(self.frames@);
        match plan {
            MonitorPlan::Identity => ObservableState::Active { frames, score_len: score_after as nat },
            MonitorPlan::SetTop { frame } => ObservableState::Active {
                frames: frames.update(frames.len() - 1, frame@),
                score_len: score_after as nat,
            },
            MonitorPlan::SetTopAndPush { parent, child } => ObservableState::Active {
                frames: frames.update(frames.len() - 1, parent@).push(child@),
                score_len: score_after as nat,
            },
            MonitorPlan::PopAndSetParent { parent } => ObservableState::Active {
                frames: frames.drop_last().update(frames.len() - 2, parent@),
                score_len: score_after as nat,
            },
            MonitorPlan::AcceptRoot => ObservableState::Accepted {
                root: frames.last(),
                score_len: score_after as nat,
            },
        }
    }

    spec fn plan_wf(&self, plan: MonitorPlan) -> bool {
        match plan {
            MonitorPlan::Identity => true,
            MonitorPlan::SetTop { frame } => frame_counters_wf(frame),
            MonitorPlan::SetTopAndPush { parent, child } => {
                frame_counters_wf(parent) && frame_counters_wf(child)
            },
            MonitorPlan::PopAndSetParent { parent } => {
                self.frames@.len() >= 2 && frame_counters_wf(parent)
            },
            MonitorPlan::AcceptRoot => self.frames@.len() == 1,
        }
    }

    fn plan_continue(
        &self, message: bool, field: Option<ObservedField>,
    ) -> (planned: Result<MonitorPlan, MonitorRejection>)
        requires
            self.wf(),
            self@ is Active,
        ensures
            match planned {
                Ok(plan) => {
                    &&& (plan is Identity || plan is SetTop)
                    &&& self.plan_wf(plan)
                    &&& observable_continue_next(
                        observable_frames_of(self@),
                        self.score_len as nat,
                        message,
                        observed_field_view(field),
                    ) == Some(self.plan_view(plan, self.score_len))
                },
                Err(_) => observable_continue_next(
                    observable_frames_of(self@),
                    self.score_len as nat,
                    message,
                    observed_field_view(field),
                ) is None,
            },
    {
        let top = &self.frames[self.frames.len() - 1];
        if !class_matches(message, top.kind) {
            return Err(MonitorRejection::EventClassMismatch);
        }
        if top.kind == 3 {
            match field {
                Some(_) => Err(MonitorRejection::FieldRefused),
                None => Ok(MonitorPlan::Identity),
            }
        } else {
            match field {
                Some(field) => {
                    if field_admissible(top, &field)
                        && (top.kind != 2 || map_key_field(top, &field))
                    {
                        Ok(MonitorPlan::SetTop { frame: frame_after_field(top, &field) })
                    } else {
                        Err(MonitorRejection::FieldRefused)
                    }
                },
                None => Err(MonitorRejection::FieldRefused),
            }
        }
    }

    fn plan_push(
        &self, message: bool, field: Option<ObservedField>, request: &ObservedFrameRequest,
    ) -> (planned: Result<MonitorPlan, MonitorRejection>)
        requires
            self.wf(),
            self@ is Active,
        ensures
            match planned {
                Ok(plan) => {
                    &&& plan is SetTopAndPush
                    &&& self.plan_wf(plan)
                    &&& observable_push_next(
                        observable_frames_of(self@),
                        self.score_len as nat,
                        message,
                        observed_field_view(field),
                        request@,
                    ) == Some(self.plan_view(plan, self.score_len))
                },
                Err(_) => observable_push_next(
                    observable_frames_of(self@),
                    self.score_len as nat,
                    message,
                    observed_field_view(field),
                    request@,
                ) is None,
            },
    {
        let top = &self.frames[self.frames.len() - 1];
        if !class_matches(message, top.kind) {
            return Err(MonitorRejection::EventClassMismatch);
        }
        if !request_admissible(top, request) {
            return Err(MonitorRejection::RequestRefused);
        }
        if !push_parent_ok(top, field) {
            return Err(MonitorRejection::FieldRefused);
        }
        Ok(MonitorPlan::SetTopAndPush {
            parent: parent_after_push(top, field, request),
            child: fresh_frame(request),
        })
    }

    fn plan_complete(
        &self, message: bool, record: &ObservedAcceptRecord, score_after: usize,
    ) -> (planned: Result<MonitorPlan, MonitorRejection>)
        requires
            self.wf(),
            self@ is Active,
        ensures
            match planned {
                Ok(plan) => {
                    &&& (plan is PopAndSetParent || plan is AcceptRoot)
                    &&& self.plan_wf(plan)
                    &&& observable_complete_next(
                        observable_frames_of(self@),
                        self.score_len as nat,
                        message,
                        record@,
                        score_after as nat,
                    ) == Some(self.plan_view(plan, score_after))
                },
                Err(_) => observable_complete_next(
                    observable_frames_of(self@),
                    self.score_len as nat,
                    message,
                    record@,
                    score_after as nat,
                ) is None,
            },
    {
        let depth = self.frames.len();
        let top = &self.frames[depth - 1];
        if !class_matches(message, top.kind) {
            return Err(MonitorRejection::EventClassMismatch);
        }
        if !frame_finished(top) {
            return Err(MonitorRejection::CompletionRefused);
        }
        if !score_ok(message, self.score_len, score_after) {
            return Err(MonitorRejection::ScoreRefused);
        }
        if depth == 1 {
            match *record {
                ObservedAcceptRecord::Root => {
                    if top.kind == 1 || top.kind == 4 {
                        Ok(MonitorPlan::AcceptRoot)
                    } else {
                        Err(MonitorRejection::AcceptRefused)
                    }
                },
                _ => Err(MonitorRejection::AcceptRefused),
            }
        } else {
            let parent = &self.frames[depth - 2];
            match parent.waiting {
                Some(wait) => {
                    if wait.child_kind != top.kind {
                        return Err(MonitorRejection::AcceptRefused);
                    }
                    match parent_after_accept(parent, wait, top, record) {
                        Some(accepted_parent) => {
                            Ok(MonitorPlan::PopAndSetParent { parent: accepted_parent })
                        },
                        None => Err(MonitorRejection::AcceptRefused),
                    }
                },
                None => Err(MonitorRejection::CompletionRefused),
            }
        }
    }

    fn plan_event(&self, event: &ObservedEvent) -> (planned: Result<MonitorPlan, MonitorRejection>)
        requires
            self.wf(),
            self@ is Active,
        ensures
            match planned {
                Ok(plan) => {
                    &&& self.plan_wf(plan)
                    &&& observable_core_next(self@, event@)
                        == Some(self.plan_view(plan, event_score_after(self.score_len, *event)))
                    &&& (plan is SetTopAndPush ==> observable_event_is_push(event@))
                },
                Err(_) => observable_core_next(self@, event@) is None,
            },
    {
        match *event {
            ObservedEvent::MessageContinue { field, .. } => self.plan_continue(true, Some(field)),
            ObservedEvent::MessagePush { field, child, .. } => {
                self.plan_push(true, Some(field), &child)
            },
            ObservedEvent::MessageComplete { record, after, .. } => {
                self.plan_complete(true, &record, after.score_len)
            },
            ObservedEvent::OtherContinue { field, .. } => self.plan_continue(false, field),
            ObservedEvent::OtherPush { field, child, .. } => self.plan_push(false, field, &child),
            ObservedEvent::OtherComplete { record, after, .. } => {
                self.plan_complete(false, &record, after.score_len)
            },
        }
    }

    fn snapshot_matches(&self, snapshot: &ObservedSnapshot) -> (matches: bool)
        requires self.wf()
        ensures matches == (snapshot@ == observable_snapshot(self@))
    {
        if self.accepted_root.is_some() {
            snapshot.depth == 0 && snapshot.top_kind.is_none() && snapshot.score_len == self.score_len
        } else {
            let depth = self.frames.len();
            snapshot.depth == depth && kind_matches(snapshot.top_kind, self.frames[depth - 1].kind)
                && snapshot.score_len == self.score_len
        }
    }

    fn after_matches(
        &self, plan: &MonitorPlan, score_after: usize, after: &ObservedSnapshot,
    ) -> (matches: bool)
        requires
            self.wf(),
            self@ is Active,
            self.plan_wf(*plan),
        ensures matches == (after@ == observable_snapshot(self.plan_view(*plan, score_after)))
    {
        let depth = self.frames.len();
        let shape_matches = match *plan {
            MonitorPlan::Identity => {
                after.depth == depth && kind_matches(after.top_kind, self.frames[depth - 1].kind)
            },
            MonitorPlan::SetTop { frame } => {
                after.depth == depth && kind_matches(after.top_kind, frame.kind)
            },
            MonitorPlan::SetTopAndPush { child, .. } => {
                after.depth > 0 && after.depth - 1 == depth && kind_matches(after.top_kind, child.kind)
            },
            MonitorPlan::PopAndSetParent { parent } => {
                after.depth == depth - 1 && kind_matches(after.top_kind, parent.kind)
            },
            MonitorPlan::AcceptRoot => after.depth == 0 && after.top_kind.is_none(),
        };
        shape_matches && after.score_len == score_after
    }

    fn apply(&mut self, plan: MonitorPlan, score_after: usize)
        requires
            old(self).wf(),
            old(self)@ is Active,
            old(self).plan_wf(plan),
            observable_wf(old(self).plan_view(plan, score_after)),
            plan is SetTopAndPush ==> old(self).frames@.len() < old(self).max_depth,
        ensures
            final(self)@ == old(self).plan_view(plan, score_after),
            final(self).wf(),
            final(self).checked_events == old(self).checked_events,
            final(self).max_depth == old(self).max_depth,
    {
        let ghost frames = self.frames@;
        match plan {
            MonitorPlan::Identity => {},
            MonitorPlan::SetTop { frame } => {
                let last = self.frames.len() - 1;
                self.frames.set(last, frame);
                proof {
                    lemma_frames_view_set_top(frames, frame);
                    assert forall|i: int| 0 <= i < self.frames@.len() implies frame_counters_wf(
                        #[trigger] self.frames@[i],
                    ) by {
                        if i != last {
                            assert(self.frames@[i] == frames[i]);
                        }
                    }
                }
            },
            MonitorPlan::SetTopAndPush { parent, child } => {
                let last = self.frames.len() - 1;
                self.frames.set(last, parent);
                self.frames.push(child);
                proof {
                    lemma_frames_view_set_top(frames, parent);
                    lemma_frames_view_push(frames.update(last as int, parent), child);
                    assert forall|i: int| 0 <= i < self.frames@.len() implies frame_counters_wf(
                        #[trigger] self.frames@[i],
                    ) by {
                        if i < last {
                            assert(self.frames@[i] == frames[i]);
                        }
                    }
                }
            },
            MonitorPlan::PopAndSetParent { parent } => {
                self.frames.pop();
                let last = self.frames.len() - 1;
                self.frames.set(last, parent);
                proof {
                    lemma_frames_view_pop_set(frames, parent);
                    assert(self.frames@ == frames.drop_last().update(frames.len() - 2, parent));
                    assert forall|i: int| 0 <= i < self.frames@.len() implies frame_counters_wf(
                        #[trigger] self.frames@[i],
                    ) by {
                        if i != last {
                            assert(self.frames@[i] == frames[i]);
                        }
                    }
                }
            },
            MonitorPlan::AcceptRoot => {
                self.accepted_root = self.frames.pop();
                proof {
                    assert(self.frames@.len() == 0);
                }
            },
        }
        self.score_len = score_after;
    }
}

} // verus!

#[cfg(test)]
mod tests {
    use super::{
        AdmissionMonitor, MonitorRefusal, MonitorRejection, ObservedAcceptRecord, ObservedEvent,
        ObservedField, ObservedFrameRequest, ObservedSnapshot,
    };

    fn snapshot(depth: usize, top_kind: Option<u8>, score_len: usize) -> ObservedSnapshot {
        ObservedSnapshot {
            depth,
            top_kind,
            score_len,
        }
    }

    fn field(slot: usize, repeated: bool, cursor_after: usize) -> ObservedField {
        ObservedField {
            slot,
            repeated,
            cursor_after,
        }
    }

    fn request(kind: u8, body_len: usize, slot_count: usize) -> ObservedFrameRequest {
        ObservedFrameRequest {
            kind,
            body_len,
            slot_count,
        }
    }

    fn message_root() -> ObservedFrameRequest { request(1, 10, 3) }

    fn map_root() -> ObservedFrameRequest { request(1, 12, 2) }

    fn path_root() -> ObservedFrameRequest { request(4, 20, 0) }

    fn message_witness() -> [ObservedEvent; 6] {
        [
            ObservedEvent::MessageContinue {
                before: snapshot(1, Some(1), 0),
                field: field(0, false, 3),
                after: snapshot(1, Some(1), 0),
            },
            ObservedEvent::MessagePush {
                before: snapshot(1, Some(1), 0),
                field: field(1, false, 8),
                child: request(1, 4, 2),
                after: snapshot(2, Some(1), 0),
            },
            ObservedEvent::MessageContinue {
                before: snapshot(2, Some(1), 0),
                field: field(0, false, 4),
                after: snapshot(2, Some(1), 0),
            },
            ObservedEvent::MessageComplete {
                before: snapshot(2, Some(1), 0),
                record: ObservedAcceptRecord::MessageSlot {
                    slot: 1,
                    occurrences_after: 1,
                },
                after: snapshot(1, Some(1), 1),
            },
            ObservedEvent::MessageContinue {
                before: snapshot(1, Some(1), 1),
                field: field(2, false, 10),
                after: snapshot(1, Some(1), 1),
            },
            ObservedEvent::MessageComplete {
                before: snapshot(1, Some(1), 1),
                record: ObservedAcceptRecord::Root,
                after: snapshot(0, None, 2),
            },
        ]
    }

    fn map_witness() -> [ObservedEvent; 8] {
        [
            ObservedEvent::MessagePush {
                before: snapshot(1, Some(1), 0),
                field: field(0, true, 7),
                child: request(2, 6, 2),
                after: snapshot(2, Some(2), 0),
            },
            ObservedEvent::OtherContinue {
                before: snapshot(2, Some(2), 0),
                field: Some(field(0, false, 2)),
                after: snapshot(2, Some(2), 0),
            },
            ObservedEvent::OtherPush {
                before: snapshot(2, Some(2), 0),
                field: Some(field(1, false, 6)),
                child: request(1, 3, 1),
                after: snapshot(3, Some(1), 0),
            },
            ObservedEvent::MessageContinue {
                before: snapshot(3, Some(1), 0),
                field: field(0, false, 3),
                after: snapshot(3, Some(1), 0),
            },
            ObservedEvent::MessageComplete {
                before: snapshot(3, Some(1), 0),
                record: ObservedAcceptRecord::MapValue,
                after: snapshot(2, Some(2), 1),
            },
            ObservedEvent::OtherComplete {
                before: snapshot(2, Some(2), 1),
                record: ObservedAcceptRecord::MessageSlot {
                    slot: 0,
                    occurrences_after: 1,
                },
                after: snapshot(1, Some(1), 1),
            },
            ObservedEvent::MessageContinue {
                before: snapshot(1, Some(1), 1),
                field: field(1, false, 12),
                after: snapshot(1, Some(1), 1),
            },
            ObservedEvent::MessageComplete {
                before: snapshot(1, Some(1), 1),
                record: ObservedAcceptRecord::Root,
                after: snapshot(0, None, 2),
            },
        ]
    }

    fn path_witness() -> [ObservedEvent; 7] {
        [
            ObservedEvent::OtherPush {
                before: snapshot(1, Some(4), 0),
                field: None,
                child: request(3, 10, 0),
                after: snapshot(2, Some(3), 0),
            },
            ObservedEvent::OtherContinue {
                before: snapshot(2, Some(3), 0),
                field: None,
                after: snapshot(2, Some(3), 0),
            },
            ObservedEvent::OtherPush {
                before: snapshot(2, Some(3), 0),
                field: None,
                child: request(1, 2, 1),
                after: snapshot(3, Some(1), 0),
            },
            ObservedEvent::MessageContinue {
                before: snapshot(3, Some(1), 0),
                field: field(0, false, 2),
                after: snapshot(3, Some(1), 0),
            },
            ObservedEvent::MessageComplete {
                before: snapshot(3, Some(1), 0),
                record: ObservedAcceptRecord::Reduced,
                after: snapshot(2, Some(3), 1),
            },
            ObservedEvent::OtherComplete {
                before: snapshot(2, Some(3), 1),
                record: ObservedAcceptRecord::Reduced,
                after: snapshot(1, Some(4), 1),
            },
            ObservedEvent::OtherComplete {
                before: snapshot(1, Some(4), 1),
                record: ObservedAcceptRecord::Root,
                after: snapshot(0, None, 1),
            },
        ]
    }

    fn after_snapshot(event: &ObservedEvent) -> ObservedSnapshot {
        match *event {
            ObservedEvent::MessageContinue { after, .. }
            | ObservedEvent::MessagePush { after, .. }
            | ObservedEvent::MessageComplete { after, .. }
            | ObservedEvent::OtherContinue { after, .. }
            | ObservedEvent::OtherPush { after, .. }
            | ObservedEvent::OtherComplete { after, .. } => after,
        }
    }

    fn accept_all(monitor: &mut AdmissionMonitor, events: &[ObservedEvent]) {
        for event in events {
            monitor
                .check_event(event)
                .expect("witness event must be accepted");
            assert_eq!(monitor.depth(), after_snapshot(event).depth);
        }
    }

    fn assert_accepted(root: &ObservedFrameRequest, witness: &[ObservedEvent]) {
        let mut monitor = AdmissionMonitor::new(root, 0, 64, 16).expect("admissible witness root");
        accept_all(&mut monitor, witness);
        assert!(monitor.is_accepted());
        assert_eq!(monitor.depth(), 0);
        assert_eq!(monitor.checked_events(), witness.len() as u64);
    }

    fn assert_refused(
        root: &ObservedFrameRequest,
        witness: &[ObservedEvent],
        prefix: usize,
        event: &ObservedEvent,
        rejection: MonitorRejection,
    ) {
        let mut monitor = AdmissionMonitor::new(root, 0, 64, 16).expect("admissible witness root");
        accept_all(&mut monitor, &witness[..prefix]);
        let depth = monitor.depth();
        let checked_events = monitor.checked_events();
        let accepted = monitor.is_accepted();
        assert_eq!(
            monitor.check_event(event),
            Err(MonitorRefusal::Rejected(rejection))
        );
        assert_eq!(monitor.depth(), depth);
        assert_eq!(monitor.checked_events(), checked_events);
        assert_eq!(monitor.is_accepted(), accepted);
        accept_all(&mut monitor, &witness[prefix..]);
        assert!(monitor.is_accepted());
        assert_eq!(monitor.checked_events(), witness.len() as u64);
    }

    #[test]
    fn witnesses_are_accepted_event_by_event() {
        assert_accepted(&message_root(), &message_witness());
        assert_accepted(&map_root(), &map_witness());
        assert_accepted(&path_root(), &path_witness());
    }

    #[test]
    fn refusals_return_the_exact_rejection_and_leave_the_monitor_unchanged() {
        let message = message_witness();
        let map = map_witness();
        let path = path_witness();
        assert_refused(
            &message_root(),
            &message,
            0,
            &ObservedEvent::MessageContinue {
                before: snapshot(1, Some(1), 0),
                field: field(0, false, 11),
                after: snapshot(1, Some(1), 0),
            },
            MonitorRejection::FieldRefused,
        );
        assert_refused(
            &message_root(),
            &message,
            1,
            &ObservedEvent::MessageContinue {
                before: snapshot(1, Some(1), 0),
                field: field(0, false, 5),
                after: snapshot(1, Some(1), 0),
            },
            MonitorRejection::FieldRefused,
        );
        assert_refused(
            &message_root(),
            &message,
            0,
            &ObservedEvent::MessagePush {
                before: snapshot(1, Some(1), 0),
                field: field(0, false, 3),
                child: request(4, 2, 0),
                after: snapshot(2, Some(4), 0),
            },
            MonitorRejection::RequestRefused,
        );
        assert_refused(
            &message_root(),
            &message,
            2,
            &ObservedEvent::MessageComplete {
                before: snapshot(2, Some(1), 0),
                record: ObservedAcceptRecord::MessageSlot {
                    slot: 1,
                    occurrences_after: 1,
                },
                after: snapshot(1, Some(1), 1),
            },
            MonitorRejection::CompletionRefused,
        );
        assert_refused(
            &message_root(),
            &message,
            3,
            &ObservedEvent::MessageComplete {
                before: snapshot(2, Some(1), 0),
                record: ObservedAcceptRecord::MessageSlot {
                    slot: 1,
                    occurrences_after: 2,
                },
                after: snapshot(1, Some(1), 1),
            },
            MonitorRejection::AcceptRefused,
        );
        assert_refused(
            &path_root(),
            &path,
            5,
            &ObservedEvent::OtherComplete {
                before: snapshot(2, Some(3), 1),
                record: ObservedAcceptRecord::Reduced,
                after: snapshot(1, Some(4), 2),
            },
            MonitorRejection::ScoreRefused,
        );
        assert_refused(
            &message_root(),
            &message,
            0,
            &ObservedEvent::MessageContinue {
                before: snapshot(2, Some(1), 0),
                field: field(0, false, 3),
                after: snapshot(2, Some(1), 0),
            },
            MonitorRejection::SnapshotMismatch,
        );
        assert_refused(
            &message_root(),
            &message,
            6,
            &ObservedEvent::MessageContinue {
                before: snapshot(0, None, 2),
                field: field(0, false, 3),
                after: snapshot(0, None, 2),
            },
            MonitorRejection::AlreadyAccepted,
        );
        assert_refused(
            &map_root(),
            &map,
            1,
            &ObservedEvent::OtherContinue {
                before: snapshot(2, Some(2), 0),
                field: Some(field(1, false, 2)),
                after: snapshot(2, Some(2), 0),
            },
            MonitorRejection::FieldRefused,
        );
        assert_refused(
            &path_root(),
            &path,
            1,
            &ObservedEvent::OtherPush {
                before: snapshot(2, Some(3), 0),
                field: None,
                child: request(4, 2, 0),
                after: snapshot(3, Some(4), 0),
            },
            MonitorRejection::RequestRefused,
        );
        assert_refused(
            &map_root(),
            &map,
            1,
            &ObservedEvent::MessageContinue {
                before: snapshot(2, Some(2), 0),
                field: field(0, false, 2),
                after: snapshot(2, Some(2), 0),
            },
            MonitorRejection::EventClassMismatch,
        );
    }

    #[test]
    fn push_beyond_max_depth_is_a_capacity_refusal() {
        let message = message_witness();
        let mut monitor =
            AdmissionMonitor::new(&message_root(), 0, 1, 1).expect("admissible message root");
        accept_all(&mut monitor, &message[..1]);
        assert_eq!(
            monitor.check_event(&message[1]),
            Err(MonitorRefusal::CapacityExhausted)
        );
        assert_eq!(monitor.depth(), 1);
        assert_eq!(monitor.checked_events(), 1);
        assert!(!monitor.is_accepted());
        accept_all(&mut monitor, &[
            ObservedEvent::MessageContinue {
                before: snapshot(1, Some(1), 0),
                field: field(1, false, 10),
                after: snapshot(1, Some(1), 0),
            },
            ObservedEvent::MessageComplete {
                before: snapshot(1, Some(1), 0),
                record: ObservedAcceptRecord::Root,
                after: snapshot(0, None, 0),
            },
        ]);
        assert!(monitor.is_accepted());
        assert_eq!(monitor.checked_events(), 3);
    }

    #[test]
    fn new_refuses_inadmissible_roots_and_zero_depth() {
        assert_eq!(
            AdmissionMonitor::new(&request(2, 6, 2), 0, 16, 16).err(),
            Some(MonitorRefusal::Rejected(MonitorRejection::RequestRefused))
        );
        assert_eq!(
            AdmissionMonitor::new(&request(4, 20, 1), 0, 16, 16).err(),
            Some(MonitorRefusal::Rejected(MonitorRejection::RequestRefused))
        );
        assert_eq!(
            AdmissionMonitor::new(&message_root(), 0, 0, 16).err(),
            Some(MonitorRefusal::CapacityExhausted)
        );
        let monitor =
            AdmissionMonitor::new(&message_root(), 0, 16, 0).expect("admissible message root");
        assert_eq!(monitor.depth(), 1);
        assert_eq!(monitor.checked_events(), 0);
        assert!(!monitor.is_accepted());
    }
}
