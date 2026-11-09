//! An accepted ACT line publication derives its scalar cursor events from the
//! validated plan's work, rather than assuming the final event range equals a
//! batch template. The work-placement witness is still a Rust extraction
//! obligation: `source_position` must identify the active plan record and the
//! event index at which that record's remaining line work begins. The indexed
//! source-position image and `prepare().commit()` must also project to the
//! selected trie subview and its physical anchors.

use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_epathmap_event_refinement.rs"]
mod epm;
#[cfg(rpb3_integrated)]
use crate::rpb3_epathmap_event_refinement as epm;

verus! {

pub enum LineDirection { Forward, Unwind }
pub enum LineStage { Value, LineEdge, LineValue, Children, Leave }

/// Scalar fields exposed by `CanonicalCompactTrieIndexedTopologyCursor::source_position`.
pub struct LineCursorSourcePosition {
    pub record: usize,
    pub node_id: usize,
    pub next_record: usize,
    pub stack_depth: usize,
    pub remaining_children: usize,
    pub stage: LineStage,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub line_index: usize,
    pub leave_index: usize,
}

pub struct AcceptedLineCommitSource {
    pub direction: LineDirection,
    pub start: usize,
    pub end: usize,
    pub expanded_events: usize,
    pub last_source: epm::SourceAnchor,
    pub ghost before: LineCursorSourcePosition,
    pub ghost after: LineCursorSourcePosition,
    /// Physical positions returned by the indexed trie view, in trie offsets.
    pub ghost indexed_positions: Seq<epm::SourceAnchor>,
}

pub open spec fn commit_width(commit: AcceptedLineCommitSource) -> int {
    commit.end as int - commit.start as int
}

pub open spec fn commit_last_offset(commit: AcceptedLineCommitSource) -> int {
    match commit.direction {
        LineDirection::Forward => commit.end as int - 1,
        LineDirection::Unwind => commit.start as int,
    }
}

/// Direct transcription of `CanonicalCompactTrieLineCommit::matches_scalar_schedule`.
/// Its range and stage checks explain why a Forward batch excludes the final
/// Value and an Unwind batch excludes the record's final Leave.
pub open spec fn commit_matches_source_positions(
    commit: AcceptedLineCommitSource,
) -> bool {
    let before = commit.before;
    let after = commit.after;
    if before.line_start.is_none() || before.line_end.is_none() {
        false
    } else {
        let line_start = before.line_start.unwrap();
        let line_end = before.line_end.unwrap();
        let width = commit_width(commit);
        &&& width > 0
        &&& before.record == after.record
        &&& before.node_id == after.node_id
        &&& before.next_record == after.next_record
        &&& before.stack_depth == after.stack_depth
        &&& before.remaining_children == after.remaining_children
        &&& before.line_start == after.line_start
        &&& before.line_end == after.line_end
        &&& line_start <= commit.start
        &&& commit.end <= line_end
        &&& match commit.direction {
            LineDirection::Forward => {
                &&& before.stage == LineStage::LineEdge
                &&& after.stage == LineStage::LineEdge
                &&& line_start as int + before.line_index as int == commit.start as int
                &&& commit.end < line_end
                &&& after.line_index as int == before.line_index as int + width
                &&& before.leave_index == after.leave_index
                &&& commit.expanded_events as int == 2 * width
            },
            LineDirection::Unwind => {
                &&& (before.stage == LineStage::Leave
                    || (before.stage == LineStage::Children
                        && before.remaining_children == 0))
                &&& after.stage == LineStage::Leave
                &&& line_end as int - before.leave_index as int == commit.end as int
                &&& after.leave_index as int == before.leave_index as int + width
                &&& before.line_index == after.line_index
                &&& commit.expanded_events as int == width
            },
        }
    }
}

pub open spec fn emitted(item: epm::ActWorkItem) -> Option<epm::WireTopologyEvent> {
    match item {
        epm::ActWorkItem::Emit(event) => Some(event),
        _ => None,
    }
}

pub open spec fn forward_line_wire(record: epm::ActRecordImage, index: usize, step: int)
    -> epm::WireTopologyEvent
    recommends 0 <= step, (index as int) + step / 2 < (record.line_len as int),
{
    let offset = (record.line_start as int + index as int + step / 2) as usize;
    if step % 2 == 0 {
        epm::WireTopologyEvent::Edge {
            source_offset: offset, origin: epm::EdgeOrigin::Direct,
        }
    } else {
        epm::WireTopologyEvent::Value {
            has_value: false, value_body: None, source_offset: offset,
        }
    }
}

pub open spec fn unwind_line_wire(record: epm::ActRecordImage, index: usize, step: int)
    -> epm::WireTopologyEvent
    recommends 0 <= step, (index as int) + step < (record.line_len as int),
{
    epm::WireTopologyEvent::Leave {
        source_offset: (record.line_start as int + record.line_len as int
            - 1 - index as int - step) as usize,
    }
}

/// An ACT work item has no value body until the later attachment pass. The
/// source-position range ensures none of the selected Forward Values is the
/// line's final (potentially value-bearing) Value.
pub proof fn forward_line_work_has_scalar_event(
    record: epm::ActRecordImage, index: usize, width: usize, step: int,
)
    requires
        width > 0,
        (index as int) + (width as int) < (record.line_len as int),
        0 <= step < 2 * (width as int),
    ensures
        step < epm::act_line_forward_work(record, index).len(),
        emitted(epm::act_line_forward_work(record, index)[step])
            == Some(forward_line_wire(record, index, step)),
    decreases step,
{
    let work = epm::act_line_forward_work(record, index);
    let tail = epm::act_line_forward_work(record, (index as int + 1) as usize);
    assert(index < record.line_len);
    assert(work.len() == 2 + tail.len());
    if step >= 2 {
        forward_line_work_has_scalar_event(
            record, (index as int + 1) as usize,
            (width as int - 1) as usize, step - 2,
        );
        assert(work[step] == tail[step - 2]);
        assert((step - 2) / 2 + 1 == step / 2);
        assert((step - 2) % 2 == step % 2);
        assert(forward_line_wire(record, (index as int + 1) as usize, step - 2)
            == forward_line_wire(record, index, step));
    } else if step == 0 {
        assert(work[step] == epm::ActWorkItem::Emit(forward_line_wire(record, index, step)));
    } else {
        assert(index as int + 1 < record.line_len as int);
        assert(work[step] == epm::ActWorkItem::Emit(forward_line_wire(record, index, step)));
    }
}

pub proof fn unwind_line_work_has_scalar_event(
    record: epm::ActRecordImage, index: usize, width: usize, step: int,
)
    requires
        width > 0,
        (index as int) + (width as int) <= (record.line_len as int),
        0 <= step < (width as int),
    ensures
        step < epm::act_line_leave_work(record, index).len(),
        emitted(epm::act_line_leave_work(record, index)[step])
            == Some(unwind_line_wire(record, index, step)),
    decreases step,
{
    let work = epm::act_line_leave_work(record, index);
    let tail = epm::act_line_leave_work(record, (index as int + 1) as usize);
    assert(index < record.line_len);
    assert(work.len() == 1 + tail.len());
    if step > 0 {
        unwind_line_work_has_scalar_event(
            record, (index as int + 1) as usize,
            (width as int - 1) as usize, step - 1,
        );
        assert(work[step] == tail[step - 1]);
        assert(unwind_line_wire(record, (index as int + 1) as usize, step - 1)
            == unwind_line_wire(record, index, step));
    } else {
        assert(work[step] == epm::ActWorkItem::Emit(unwind_line_wire(record, index, step)));
    }
}

/// The event prefix of an active work queue consists entirely of `Emit`
/// items. This derives each raw event from the plan work rather than supplying
/// a pointwise event-equality premise.
pub proof fn expanded_emit_prefix_at(
    plan: epm::ActPlanImage,
    work: Seq<epm::ActWorkItem>,
    fuel: nat,
    events: Seq<epm::WireTopologyEvent>,
    at: int,
)
    requires
        0 <= at < work.len(),
        at < fuel,
        epm::act_expand_work(plan, work, fuel) == Some(events),
        forall|i: int| 0 <= i <= at ==> emitted(work[i]).is_some(),
    ensures
        at < events.len(),
        Some(events[at]) == emitted(work[at]),
    decreases at,
{
    let head = emitted(work[0]).unwrap();
    let rest = work.drop_first();
    let tail_fuel = (fuel as int - 1) as nat;
    let tail = epm::act_expand_work(plan, rest, tail_fuel).unwrap();
    assert(events == seq![head] + tail);
    if at > 0 {
        assert forall|i: int| 0 <= i <= at - 1
            implies emitted(rest[i]).is_some() by {
            assert(rest[i] == work[i + 1]);
        }
        expanded_emit_prefix_at(plan, rest, tail_fuel, tail, at - 1);
    }
}

pub open spec fn batch_safe_wire(event: epm::WireTopologyEvent) -> bool {
    match event {
        epm::WireTopologyEvent::Value { has_value, .. } => !has_value,
        _ => true,
    }
}

pub proof fn attachment_preserves_event_count(
    bytes: Seq<u8>, mode: epm::EpmMode,
    raw: Seq<epm::WireTopologyEvent>, value_offset: usize,
    attached: epm::AttachedActEvents,
)
    requires epm::attach_act_value_bodies(bytes, mode, raw, value_offset) == Some(attached),
    ensures attached.events.len() == raw.len(),
    decreases raw.len(),
{
    if raw.len() > 0 {
        let rest = raw.drop_first();
        match raw[0] {
            epm::WireTopologyEvent::Value { has_value: true, .. }
                if mode == epm::EpmMode::Map => {
                let span = epm::map_value_body_at(bytes, value_offset).unwrap();
                let tail = epm::attach_act_value_bodies(
                    bytes, mode, rest, span.end,
                ).unwrap();
                attachment_preserves_event_count(bytes, mode, rest, span.end, tail);
            },
            _ => {
                let tail = epm::attach_act_value_bodies(
                    bytes, mode, rest, value_offset,
                ).unwrap();
                attachment_preserves_event_count(bytes, mode, rest, value_offset, tail);
            },
        }
    }
}

/// Attaching map value bodies changes only value-bearing Values. It preserves
/// the index and exact event of every Edge, Leave, or Value(false).
pub proof fn attachment_preserves_batch_wire_at(
    bytes: Seq<u8>, mode: epm::EpmMode,
    raw: Seq<epm::WireTopologyEvent>, value_offset: usize,
    attached: epm::AttachedActEvents, at: int,
)
    requires
        epm::attach_act_value_bodies(bytes, mode, raw, value_offset) == Some(attached),
        0 <= at < raw.len(),
        batch_safe_wire(raw[at]),
    ensures
        attached.events.len() == raw.len(),
        attached.events[at] == raw[at],
    decreases raw.len(),
{
    attachment_preserves_event_count(bytes, mode, raw, value_offset, attached);
    let rest = raw.drop_first();
    if at == 0 {
        match raw[0] {
            epm::WireTopologyEvent::Value { has_value: true, .. } => {
                assert(false);
            },
            _ => {},
        }
    } else {
        match raw[0] {
            epm::WireTopologyEvent::Value { has_value: true, .. }
                if mode == epm::EpmMode::Map => {
                let span = epm::map_value_body_at(bytes, value_offset).unwrap();
                let tail = epm::attach_act_value_bodies(
                    bytes, mode, rest, span.end,
                ).unwrap();
                attachment_preserves_batch_wire_at(
                    bytes, mode, rest, span.end, tail, at - 1,
                );
            },
            _ => {
                let tail = epm::attach_act_value_bodies(
                    bytes, mode, rest, value_offset,
                ).unwrap();
                attachment_preserves_batch_wire_at(
                    bytes, mode, rest, value_offset, tail, at - 1,
                );
            },
        }
    }
}

/// Offset after the first `consumed` raw topology events. This fold reads
/// each Map length prefix from the immutable snapshot. It does not inspect an
/// attached event's already-selected body span.
pub open spec fn raw_value_offset_after(
    bytes: Seq<u8>, mode: epm::EpmMode,
    raw: Seq<epm::WireTopologyEvent>, value_offset: usize, consumed: nat,
) -> Option<usize>
    decreases consumed,
{
    if consumed == 0 {
        Some(value_offset)
    } else if raw.len() == 0 {
        None
    } else {
        let next = match raw[0] {
            epm::WireTopologyEvent::Value { has_value: true, .. }
                if mode == epm::EpmMode::Map =>
                match epm::map_value_body_at(bytes, value_offset) {
                    Some(span) => Some(span.end),
                    None => None,
                },
            _ => Some(value_offset),
        };
        match next {
            Some(offset) => raw_value_offset_after(
                bytes, mode, raw.drop_first(), offset, (consumed - 1) as nat,
            ),
            None => None,
        }
    }
}

/// Attachment determines the selected Map body from the snapshot and the
/// offset obtained by consuming the preceding raw ACT events. In particular,
/// the body span is not supplied as an independent ghost equality.
pub proof fn attached_map_value_at_prefix(
    bytes: Seq<u8>, raw: Seq<epm::WireTopologyEvent>,
    initial_offset: usize, attached: epm::AttachedActEvents,
    at: nat, current_offset: usize, source_offset: usize,
    span: epm::ByteSpan,
)
    requires
        epm::attach_act_value_bodies(
            bytes, epm::EpmMode::Map, raw, initial_offset,
        ) == Some(attached),
        at < raw.len(),
        raw_value_offset_after(
            bytes, epm::EpmMode::Map, raw, initial_offset, at,
        ) == Some(current_offset),
        epm::map_value_body_at(bytes, current_offset) == Some(span),
        raw[at as int] == (epm::WireTopologyEvent::Value {
            has_value: true, value_body: None, source_offset,
        }),
    ensures
        attached.events[at as int] == (epm::WireTopologyEvent::Value {
            has_value: true,
            value_body: Some(span),
            source_offset,
        }),
    decreases at,
{
    attachment_preserves_event_count(
        bytes, epm::EpmMode::Map, raw, initial_offset, attached,
    );
    if at == 0 {
        assert(current_offset == initial_offset);
    } else {
        let rest = raw.drop_first();
        match raw[0] {
            epm::WireTopologyEvent::Value { has_value: true, .. } => {
                let first_span = epm::map_value_body_at(bytes, initial_offset).unwrap();
                let tail = epm::attach_act_value_bodies(
                    bytes, epm::EpmMode::Map, rest, first_span.end,
                ).unwrap();
                attached_map_value_at_prefix(
                    bytes, rest, first_span.end, tail, (at - 1) as nat,
                    current_offset, source_offset, span,
                );
            },
            _ => {
                let tail = epm::attach_act_value_bodies(
                    bytes, epm::EpmMode::Map, rest, initial_offset,
                ).unwrap();
                attached_map_value_at_prefix(
                    bytes, rest, initial_offset, tail, (at - 1) as nat,
                    current_offset, source_offset, span,
                );
            },
        }
    }
}

/// Scalar projection of the receipt returned by one production
/// `next_event_with_source` call. Its before/after ACT positions are captured
/// inside that call, not passed back to the cursor by the caller.
pub struct MapValueNextEventSource {
    pub act_before: LineCursorSourcePosition,
    pub act_after: Option<LineCursorSourcePosition>,
    pub act_source_offset: usize,
    pub act_source: epm::SourceAnchor,
    pub event_index_before: usize,
    pub event_index_after: usize,
    pub map_values_seen_before: usize,
    pub map_values_seen_after: usize,
    pub pre_value_table_offset: usize,
    pub length_prefix_start: usize,
    pub length_prefix_end: usize,
    pub body_len: usize,
    pub body: epm::ByteSpan,
    pub post_value_table_offset: usize,
    pub snapshot_len: usize,
}

/// The borrowed `validated_plan_source()` view of an admitted Rust cursor.
/// `records` is the sequence obtained by reading indices 0..record_count;
/// it is observational ghost data, not a second plan retained by the cursor.
pub struct AcceptedActPlanSourceImage {
    /// `indexed_source()` from the same borrowed view as `records`.
    pub ghost indexed: epm::IndexedRegionImage,
    pub ghost records: Seq<epm::ValidatedTopologyRecordImage>,
    pub empty: bool,
    /// `validated_plan_source().topology_event_count()`, checked while the
    /// accepted Rust plan is validated, before scalar cursor traversal.
    pub topology_event_count: usize,
    pub value_count: usize,
    pub first_nonzero: Option<(usize, u64)>,
    pub first_nonsequential: Option<(usize, u64, u64)>,
}

pub open spec fn validated_image_from_source(
    source: AcceptedActPlanSourceImage,
) -> epm::ValidatedActPlanImage {
    epm::ValidatedActPlanImage {
        records: source.records,
        empty: source.empty,
        value_count: source.value_count,
        first_nonzero: source.first_nonzero,
    }
}

pub open spec fn source_value_count(
    records: Seq<epm::ValidatedTopologyRecordImage>, at: nat,
) -> nat
    decreases records.len() - at,
{
    if at >= records.len() {
        0
    } else {
        (if records[at as int].flags as int % 2 == 1 { 1nat } else { 0nat })
            + source_value_count(records, at + 1)
    }
}

/// Exact event-count fold used by the accepted Rust validator. Every record
/// emits Value and Leave, each nonroot record has one incoming Edge, and each
/// compressed-line byte emits Edge, Value, and Leave.
pub open spec fn source_topology_event_count(
    records: Seq<epm::ValidatedTopologyRecordImage>, at: nat,
) -> nat
    decreases records.len() - at,
{
    if at >= records.len() { 0 } else {
        3 * records[at as int].line_len as nat
            + (if at == 0 { 2nat } else { 3nat })
            + source_topology_event_count(records, at + 1)
    }
}

pub open spec fn source_first_nonzero(
    records: Seq<epm::ValidatedTopologyRecordImage>, at: nat,
) -> Option<(usize, u64)>
    decreases records.len() - at,
{
    if at >= records.len() {
        None
    } else {
        let record = records[at as int];
        if record.flags as int % 2 == 1 && record.value != 0 {
            Some((record.value_source_offset, record.value))
        } else {
            source_first_nonzero(records, at + 1)
        }
    }
}

pub open spec fn source_first_nonsequential(
    records: Seq<epm::ValidatedTopologyRecordImage>, at: nat, ordinal: nat,
) -> Option<(usize, u64, u64)>
    decreases records.len() - at,
{
    if at >= records.len() {
        None
    } else {
        let record = records[at as int];
        if record.flags as int % 2 == 1 {
            if record.value as int != ordinal as int {
                Some((record.value_source_offset, record.value, ordinal as u64))
            } else {
                source_first_nonsequential(records, at + 1, ordinal + 1)
            }
        } else {
            source_first_nonsequential(records, at + 1, ordinal)
        }
    }
}

/// The live validator's stack shape while it pushes records in DFS preorder.
/// Only child counts and record ordinals are needed to recover the topology.
pub struct SourceValidationFrame {
    pub record: usize,
    pub remaining_children: usize,
}

pub struct SourceValidationStructure {
    pub ghost stack: Seq<SourceValidationFrame>,
    pub ghost children: Seq<Seq<usize>>,
    pub ghost subtree_ends: Seq<usize>,
    pub ghost finish_order: Seq<usize>,
}

/// Rust pops a completed frame before reading the next preorder child. Each
/// pop fixes that record's exclusive subtree end and its producer finish slot.
pub open spec fn close_completed_source_frames(
    state: SourceValidationStructure, next_record: usize,
) -> SourceValidationStructure
    decreases state.stack.len(),
{
    if state.stack.len() == 0 || state.stack.last().remaining_children > 0 {
        state
    } else {
        let frame = state.stack.last();
        close_completed_source_frames(SourceValidationStructure {
            stack: state.stack.drop_last(),
            children: state.children,
            subtree_ends: state.subtree_ends.update(frame.record as int, next_record),
            finish_order: state.finish_order.push(frame.record),
        }, next_record)
    }
}

/// This folds the same `next_child`/pop schedule as
/// `validate_direct_canonical_act`, without accepting caller-supplied child
/// lists, subtree ends, or finish order.
pub open spec fn replay_source_validation_preorder(
    records: Seq<epm::ValidatedTopologyRecordImage>,
    at: usize, state: SourceValidationStructure,
) -> Option<SourceValidationStructure>
    decreases records.len() - at as int,
{
    if at as int == records.len() {
        let closed = close_completed_source_frames(state, at);
        if closed.stack.len() == 0 && closed.finish_order.len() == records.len() {
            Some(closed)
        } else {
            None
        }
    } else if at as int > records.len() || at == usize::MAX {
        None
    } else {
        let closed = close_completed_source_frames(state, at);
        if closed.stack.len() == 0 {
            None
        } else {
            let parent = closed.stack.last();
            let next_parent = SourceValidationFrame {
                record: parent.record,
                remaining_children: (parent.remaining_children - 1) as usize,
            };
            replay_source_validation_preorder(records, (at + 1) as usize, SourceValidationStructure {
                stack: closed.stack.drop_last().push(next_parent).push(
                    SourceValidationFrame {
                        record: at,
                        remaining_children: records[at as int].child_count as usize,
                    }
                ),
                children: closed.children.update(
                    parent.record as int, closed.children[parent.record as int].push(at),
                ),
                subtree_ends: closed.subtree_ends,
                finish_order: closed.finish_order,
            })
        }
    }
}

pub open spec fn source_validation_structure(
    records: Seq<epm::ValidatedTopologyRecordImage>,
) -> Option<SourceValidationStructure> {
    if records.len() == 0 || records.len() > usize::MAX as int {
        None
    } else {
        replay_source_validation_preorder(records, 1, SourceValidationStructure {
            stack: seq![SourceValidationFrame {
                record: 0,
                remaining_children: records[0].child_count as usize,
            }],
            children: Seq::new(records.len() as nat, |i: int| Seq::<usize>::empty()),
            subtree_ends: Seq::new(records.len() as nat, |i: int| 0usize),
            finish_order: Seq::empty(),
        })
    }
}

/// The structural part of the Rust-to-`ActPlanImage` extraction is now a
/// deterministic source-stack result. GX producer replay and per-edge origin
/// still require their separate parser refinement.
pub open spec fn source_structure_projects_formal_plan(
    source: AcceptedActPlanSourceImage, plan: epm::ActPlanImage,
) -> bool {
    match source_validation_structure(source.records) {
        None => false,
        Some(structure) => {
            &&& plan.records.len() == source.records.len()
            &&& plan.finish_order == structure.finish_order
            &&& forall|i: int| 0 <= i < plan.records.len() ==> {
                &&& plan.records[i].children == structure.children[i]
                &&& plan.records[i].subtree_end == structure.subtree_ends[i]
            }
        },
    }
}

/// A concrete branched witness checks the modeled push/pop order: both leaf
/// records finish before the root, and subtree ends are exclusive ordinals.
pub proof fn two_leaf_source_validation_has_exact_structure(
    records: Seq<epm::ValidatedTopologyRecordImage>,
)
    requires
        records.len() == 3,
        records[0].child_count == 2,
        records[1].child_count == 0,
        records[2].child_count == 0,
    ensures
        source_validation_structure(records).is_some(),
        source_validation_structure(records).unwrap().children[0] == seq![1usize, 2usize],
        source_validation_structure(records).unwrap().children[1] == Seq::<usize>::empty(),
        source_validation_structure(records).unwrap().children[2] == Seq::<usize>::empty(),
        source_validation_structure(records).unwrap().subtree_ends == seq![3usize, 2usize, 3usize],
        source_validation_structure(records).unwrap().finish_order == seq![1usize, 2usize, 0usize],
{
    reveal_with_fuel(replay_source_validation_preorder, 4);
    reveal_with_fuel(close_completed_source_frames, 4);
    let result = source_validation_structure(records);
    assert(result.is_some());
    let structure = result.unwrap();
    assert(structure.finish_order == seq![1usize, 2usize, 0usize]);
    assert(structure.subtree_ends == seq![3usize, 2usize, 3usize]);
    assert(structure.children[0] == seq![1usize, 2usize]);
    assert(structure.children[1] == Seq::<usize>::empty());
    assert(structure.children[2] == Seq::<usize>::empty());
}

/// Per-record obligations read the actual indexed ACT bytes with the formal
/// decoder. DFS child placement and producer replay remain separate obligations.
pub open spec fn source_record_matches_decoded_act(
    bytes: Seq<u8>, record: epm::ValidatedTopologyRecordImage,
) -> bool {
    match epm::act_semantic_at(bytes, record.node_id) {
        None => false,
        Some(decoded) => {
            &&& record.flags < 4
            &&& record.node_id >= 16
            &&& record.node_id as int + decoded.encoded_len as int <= bytes.len() - 8
            &&& record.value_source_offset == decoded.value_source_offset
            &&& record.line_start == decoded.line_start
            &&& record.line_len == decoded.line_len
            &&& record.child_count as int == decoded.edges.len()
            &&& if record.flags as int % 2 == 1 {
                decoded.value == Some(record.value)
            } else {
                decoded.value.is_none() && record.value == 0
            }
        },
    }
}

pub open spec fn accepted_source_records_match_snapshot(
    frame: epm::EpmFrame, source: AcceptedActPlanSourceImage,
) -> bool {
    if frame.trie_start > frame.trie_end
        || frame.trie_end > frame.snapshot.bytes.len()
    {
        false
    } else {
        let bytes = frame.snapshot.bytes.subrange(
            frame.trie_start as int, frame.trie_end as int,
        );
        &&& epm::act_magic_and_trailer(bytes)
        &&& !source.empty
        &&& source.records.len() > 0
        &&& source.topology_event_count as nat
            == source_topology_event_count(source.records, 0)
        &&& source.records[0].node_id as int == epm::little_endian_uint(bytes, 8, 8)
        &&& source.value_count as int == source_value_count(source.records, 0)
        &&& source.first_nonzero == source_first_nonzero(source.records, 0)
        &&& source.first_nonsequential
            == source_first_nonsequential(source.records, 0, 0)
        &&& forall|i: int| 0 <= i < source.records.len()
            ==> source_record_matches_decoded_act(bytes, source.records[i])
    }
}

/// This equality is the precise Rust-to-Verus extraction premise: the formal
/// cursor's validated image is the image exposed by the accepted Rust plan
/// accessor. It supplies no expected events, selected body, or postcursor.
pub open spec fn accepted_act_plan_source_image(
    frame: epm::EpmFrame, source: AcceptedActPlanSourceImage,
) -> bool {
    &&& epm::indexed_image_wf(frame.snapshot)
    &&& epm::indexed_image_wf(source.indexed)
    &&& frame.trie_start <= frame.trie_end
    &&& frame.trie_end <= frame.snapshot.bytes.len()
    &&& source.indexed.bytes == frame.snapshot.bytes.subrange(
        frame.trie_start as int, frame.trie_end as int,
    )
    &&& source.indexed.positions == frame.snapshot.positions.subrange(
        frame.trie_start as int, frame.trie_end as int,
    )
    &&& source.indexed.end == if frame.trie_end as int == frame.snapshot.bytes.len() {
        frame.snapshot.end
    } else {
        frame.snapshot.positions[frame.trie_end as int]
    }
    &&& frame.cursor.validated_plan == Some(validated_image_from_source(source))
    &&& accepted_source_records_match_snapshot(frame, source)
    &&& source.value_count == frame.cursor.declared_value_count
    &&& source.first_nonzero.is_none()
}

pub open spec fn accepted_act_structure_projects_frame(
    frame: epm::EpmFrame, source: AcceptedActPlanSourceImage,
) -> bool {
    &&& accepted_act_plan_source_image(frame, source)
    &&& frame.cursor.act_plan.is_some()
    &&& source_structure_projects_formal_plan(
        source, frame.cursor.act_plan.unwrap(),
    )
}

/// The remaining count refinement identifies the formal DFS expansion length
/// with the checked total observed from the accepted Rust plan. It does not
/// supply any selected event, body, or postcursor.
pub open spec fn accepted_act_event_count_matches_source(
    frame: epm::EpmFrame, source: AcceptedActPlanSourceImage,
) -> bool {
    &&& accepted_act_structure_projects_frame(frame, source)
    &&& match frame.cursor.act_plan {
        None => false,
        Some(plan) => match epm::act_plan_events(frame, plan) {
            None => false,
            Some(events) => events.len() == source.topology_event_count as nat,
        },
    }
}

/// An accepted Map receipt identifies the same validated-plan record exposed
/// by the production accessor. For line values, the record itself also fixes
/// the ACT source offset; the no-line Value source is the node header offset.
pub proof fn selected_map_value_projects_validated_record(
    frame: epm::EpmFrame, source: MapValueNextEventSource,
    plan_source: AcceptedActPlanSourceImage,
)
    requires
        selected_map_value_attachment_source(frame, source),
        accepted_act_plan_source_image(frame, plan_source),
    ensures
        source.act_before.record < plan_source.records.len(),
        plan_source.records[source.act_before.record as int].node_id
            == source.act_before.node_id,
        plan_source.records[source.act_before.record as int].flags as int % 2 == 1,
        source.act_before.stage == LineStage::LineValue ==>
            plan_source.records[source.act_before.record as int].value_source_offset
                == source.act_source_offset,
{
    let plan = frame.cursor.act_plan.unwrap();
    let validated = frame.cursor.validated_plan.unwrap();
    let index = source.act_before.record as int;
    assert(validated == validated_image_from_source(plan_source));
    assert(epm::act_plan_projects_validated(frame, plan, validated));
    assert(index < plan.records.len());
    assert(index < validated.records.len());
    assert(epm::act_record_projects_validated(
        frame.snapshot.bytes.subrange(frame.trie_start as int, frame.trie_end as int),
        plan.records[index], validated.records[index],
    ));
    assert(plan.records[index].value.is_some());
    assert(validated.records[index].flags as int % 2 == 1);
    if source.act_before.stage == LineStage::LineValue {
        assert(source.act_source_offset == plan.records[index].value_source_offset);
    }
}

pub open spec fn map_value_event_from_source(
    source: MapValueNextEventSource,
) -> epm::EpmEvent {
    epm::EpmEvent::Value {
        has_value: true,
        value_body: Some(source.body),
        source_offset: source.act_source_offset,
    }
}

/// `ValidatedCompactTrieTopologyState::next_event` emits a Value immediately
/// after changing this one active frame. It neither descends nor pops in the
/// Value/LineValue arms, so `source_position()` keeps every other field.
pub open spec fn act_value_successor_position(
    before: LineCursorSourcePosition, record: epm::ActRecordImage,
) -> Option<LineCursorSourcePosition> {
    match before.stage {
        LineStage::Value => Some(LineCursorSourcePosition {
            stage: if record.line_len > 0 {
                LineStage::LineEdge
            } else {
                LineStage::Children
            },
            ..before
        }),
        LineStage::LineValue if before.line_index < record.line_len => {
            let next_index = (before.line_index as int + 1) as usize;
            Some(LineCursorSourcePosition {
                line_index: next_index,
                stage: if next_index < record.line_len {
                    LineStage::LineEdge
                } else {
                    LineStage::Children
                },
                ..before
            })
        },
        _ => None,
    }
}

/// The raw ACT event sequence and attachment are selected by the frame's
/// validated plan and snapshot; neither is caller-supplied ghost state. The
/// receipt's pre-offset must equal the fold of the preceding raw events.
pub open spec fn selected_map_value_attachment_source(
    frame: epm::EpmFrame, source: MapValueNextEventSource,
) -> bool {
    if frame.cursor.act_plan.is_none() {
        false
    } else {
        let plan = frame.cursor.act_plan.unwrap();
        let raw = epm::act_expand_work(
            plan, seq![epm::ActWorkItem::Enter(0)],
            epm::act_work_budget(plan, 0) + 1,
        );
        match raw {
            None => false,
            Some(raw) => {
                let attached = epm::attach_act_value_bodies(
                    frame.snapshot.bytes, frame.mode, raw,
                    frame.value_table_start,
                );
                &&& attached.is_some()
                &&& epm::frame_layout_wf(frame)
                &&& frame.mode == epm::EpmMode::Map
                &&& source.snapshot_len as int == frame.snapshot.bytes.len()
                &&& source.event_index_before == frame.cursor.at
                &&& source.map_values_seen_before == frame.cursor.map_values_seen
                &&& source.pre_value_table_offset == frame.cursor.value_table_offset
                &&& source.event_index_before < usize::MAX
                &&& source.event_index_after == source.event_index_before + 1
                &&& source.map_values_seen_before < usize::MAX
                &&& source.map_values_seen_after == source.map_values_seen_before + 1
                &&& source.length_prefix_start == source.pre_value_table_offset
                &&& source.length_prefix_start < source.length_prefix_end
                &&& source.length_prefix_end == source.body.start
                &&& source.body.start <= source.body.end
                &&& source.body_len as int == source.body.end as int - source.body.start as int
                &&& source.post_value_table_offset == source.body.end
                &&& source.body.end <= source.snapshot_len
                &&& source.act_source_offset < frame.trie_end - frame.trie_start
                &&& source.act_source == epm::trie_position(frame, source.act_source_offset)
                &&& (source.act_before.stage == LineStage::Value
                    || source.act_before.stage == LineStage::LineValue)
                &&& (source.act_before.stage != LineStage::Value
                    || source.act_before.node_id == source.act_source_offset)
                &&& source.act_before.record < plan.records.len()
                &&& source.act_before.node_id
                    == plan.records[source.act_before.record as int].node_id
                &&& source.act_before.line_start == if
                    plan.records[source.act_before.record as int].line_len > 0 {
                    Some(plan.records[source.act_before.record as int].line_start)
                } else { None }
                &&& plan.records[source.act_before.record as int].line_start as int
                    + plan.records[source.act_before.record as int].line_len as int
                    <= usize::MAX as int
                &&& source.act_before.line_end == if
                    plan.records[source.act_before.record as int].line_len > 0 {
                    Some((plan.records[source.act_before.record as int].line_start as int
                        + plan.records[source.act_before.record as int].line_len as int) as usize)
                } else { None }
                &&& match source.act_before.stage {
                    LineStage::Value => {
                        &&& plan.records[source.act_before.record as int].line_len == 0
                        &&& plan.records[source.act_before.record as int].value.is_some()
                        &&& source.act_source_offset
                            == plan.records[source.act_before.record as int].node_id
                    },
                    LineStage::LineValue => {
                        let record = plan.records[source.act_before.record as int];
                        &&& record.line_len > 0
                        &&& source.act_before.line_index as int + 1
                            == record.line_len as int
                        &&& record.value.is_some()
                        &&& source.act_source_offset == record.value_source_offset
                    },
                    _ => false,
                }
                &&& source.act_after == act_value_successor_position(
                    source.act_before, plan.records[source.act_before.record as int],
                )
                &&& source.event_index_before < raw.len()
                &&& raw[source.event_index_before as int]
                    == (epm::WireTopologyEvent::Value {
                        has_value: true, value_body: None,
                        source_offset: source.act_source_offset,
                    })
                &&& raw_value_offset_after(
                    frame.snapshot.bytes, frame.mode, raw,
                    frame.value_table_start, source.event_index_before as nat,
                ) == Some(source.pre_value_table_offset)
                &&& epm::map_value_body_at(
                    frame.snapshot.bytes, source.pre_value_table_offset,
                ) == Some(source.body)
            },
        }
    }
}

pub proof fn selected_map_value_attachment_binds_body(
    frame: epm::EpmFrame, source: MapValueNextEventSource,
)
    requires selected_map_value_attachment_source(frame, source),
    ensures epm::cursor_event_at(frame, frame.cursor.at)
        == Some(epm::WireTopologyEvent::Value {
            has_value: true, value_body: Some(source.body),
            source_offset: source.act_source_offset,
        }),
{
    let plan = frame.cursor.act_plan.unwrap();
    let raw = epm::act_expand_work(
        plan, seq![epm::ActWorkItem::Enter(0)],
        epm::act_work_budget(plan, 0) + 1,
    ).unwrap();
    let attached = epm::attach_act_value_bodies(
        frame.snapshot.bytes, frame.mode, raw, frame.value_table_start,
    ).unwrap();
    attached_map_value_at_prefix(
        frame.snapshot.bytes, raw, frame.value_table_start,
        attached, frame.cursor.at as nat,
        frame.cursor.value_table_offset, source.act_source_offset, source.body,
    );
    assert(epm::act_plan_events(frame, plan).is_some());
    assert(epm::act_plan_events(frame, plan) == Some(attached.events));
    assert(attached.events.len() == raw.len()) by {
        attachment_preserves_event_count(
            frame.snapshot.bytes, frame.mode, raw, frame.value_table_start,
            attached,
        );
    }
    assert(frame.cursor.at < attached.events.len());
    assert(epm::cursor_event_count(frame) == attached.events.len() as usize);
    assert(epm::cursor_event_at(frame, frame.cursor.at)
        == Some(attached.events[frame.cursor.at as int]));
}

/// The accepted snapshot and plan fix the raw event and length-prefixed Map
/// body at the current cursor index. Once the actual pre-event ACT position is
/// fixed, no different receipt body, ACT successor, or cursor poststate can
/// satisfy the source relation. This excludes an arbitrary ghost poststate.
pub proof fn selected_map_value_source_is_unique(
    frame: epm::EpmFrame,
    first: MapValueNextEventSource,
    second: MapValueNextEventSource,
)
    requires
        selected_map_value_attachment_source(frame, first),
        selected_map_value_attachment_source(frame, second),
        first.act_before == second.act_before,
    ensures first == second,
{
    let plan = frame.cursor.act_plan.unwrap();
    let raw = epm::act_expand_work(
        plan, seq![epm::ActWorkItem::Enter(0)],
        epm::act_work_budget(plan, 0) + 1,
    ).unwrap();
    assert(first.event_index_before == second.event_index_before);
    assert(raw[first.event_index_before as int]
        == (epm::WireTopologyEvent::Value {
            has_value: true, value_body: None,
            source_offset: first.act_source_offset,
        }));
    assert(raw[second.event_index_before as int]
        == (epm::WireTopologyEvent::Value {
            has_value: true, value_body: None,
            source_offset: second.act_source_offset,
        }));
    assert(first.act_source_offset == second.act_source_offset);
    assert(first.pre_value_table_offset == second.pre_value_table_offset);
    assert(epm::map_value_body_at(
        frame.snapshot.bytes, first.pre_value_table_offset,
    ) == Some(first.body));
    assert(epm::map_value_body_at(
        frame.snapshot.bytes, second.pre_value_table_offset,
    ) == Some(second.body));
    assert(first.body == second.body);
    assert(first.act_after == second.act_after);
    assert(first == second);
}

/// The one remaining placement premise links a Rust `source_position` to the
/// corresponding validated-plan work queue. It is structural: it names the
/// active work suffix and its emitted prefix, never the expected batch events.
pub struct ActLineWorkPlacement {
    pub ghost plan: epm::ActPlanImage,
    pub ghost record: epm::ActRecordImage,
    pub ghost prefix: Seq<epm::WireTopologyEvent>,
    pub ghost continuation: Seq<epm::ActWorkItem>,
    pub ghost active_work: Seq<epm::ActWorkItem>,
    pub ghost suffix_events: Seq<epm::WireTopologyEvent>,
    pub ghost raw_events: Seq<epm::WireTopologyEvent>,
    pub ghost attached: epm::AttachedActEvents,
    pub fuel: nat,
}

pub open spec fn act_line_work_placement(
    frame: epm::EpmFrame,
    commit: AcceptedLineCommitSource,
    placement: ActLineWorkPlacement,
) -> bool {
    let plan = placement.plan;
    &&& frame.mode != epm::EpmMode::Empty
    &&& frame.cursor.act_plan == Some(plan)
    &&& epm::act_plan_matches_frame(frame, plan)
    &&& commit.before.record < plan.records.len()
    &&& placement.record == plan.records[commit.before.record as int]
    &&& commit.before.node_id == placement.record.node_id
    &&& commit.before.line_start == Some(placement.record.line_start)
    &&& placement.record.line_start as int + placement.record.line_len as int
        <= usize::MAX as int
    &&& commit.before.line_end == Some(
        (placement.record.line_start as int + placement.record.line_len as int) as usize
    )
    &&& frame.cursor.at as int == placement.prefix.len()
    &&& (commit.expanded_events as int) <= placement.fuel
    &&& placement.active_work == match commit.direction {
        LineDirection::Forward => epm::act_line_forward_work(
            placement.record, commit.before.line_index,
        ) + placement.continuation,
        LineDirection::Unwind => epm::act_line_leave_work(
            placement.record, commit.before.leave_index,
        ) + placement.continuation,
    }
    &&& epm::act_expand_work(
        plan, placement.active_work, placement.fuel,
    ) == Some(placement.suffix_events)
    &&& placement.raw_events == placement.prefix + placement.suffix_events
    &&& epm::act_expand_work(
        plan, seq![epm::ActWorkItem::Enter(0)], epm::act_work_budget(plan, 0) + 1,
    ) == Some(placement.raw_events)
    &&& epm::attach_act_value_bodies(
        frame.snapshot.bytes, frame.mode, placement.raw_events,
        frame.value_table_start,
    ) == Some(placement.attached)
    &&& placement.attached.next_value_offset == frame.snapshot.bytes.len()
    &&& (frame.mode != epm::EpmMode::Map
        || placement.attached.map_values_seen == frame.cursor.declared_value_count)
    &&& forall|i: int| 0 <= i < placement.attached.events.len()
        ==> epm::act_event_source_valid(
            frame.snapshot.bytes.subrange(frame.trie_start as int, frame.trie_end as int),
            placement.attached.events[i],
        )
}

pub open spec fn accepted_indexed_line_source(
    frame: epm::EpmFrame,
    commit: AcceptedLineCommitSource,
    placement: ActLineWorkPlacement,
) -> bool {
    &&& epm::frame_layout_wf(frame)
    &&& commit_matches_source_positions(commit)
    &&& act_line_work_placement(frame, commit, placement)
    &&& (commit.end as int) <= frame.trie_end - frame.trie_start
    &&& commit.indexed_positions == frame.snapshot.positions.subrange(
        frame.trie_start as int, frame.trie_end as int,
    )
    &&& commit.last_source == commit.indexed_positions[commit_last_offset(commit)]
    &&& frame.cursor.at as int + commit.expanded_events as int
        <= epm::cursor_event_count(frame) as int
}

pub open spec fn line_batch_wire(
    commit: AcceptedLineCommitSource,
    placement: ActLineWorkPlacement,
    step: int,
) -> epm::WireTopologyEvent {
    match commit.direction {
        LineDirection::Forward => forward_line_wire(
            placement.record, commit.before.line_index, step,
        ),
        LineDirection::Unwind => unwind_line_wire(
            placement.record, commit.before.leave_index, step,
        ),
    }
}

pub open spec fn line_batch_source_offset(
    commit: AcceptedLineCommitSource, step: int,
) -> usize {
    match commit.direction {
        LineDirection::Forward => (commit.start as int + step / 2) as usize,
        LineDirection::Unwind =>
            (commit.end as int - 1 - step) as usize,
    }
}

pub proof fn accepted_indexed_line_step_is_plan_event(
    frame: epm::EpmFrame,
    commit: AcceptedLineCommitSource,
    placement: ActLineWorkPlacement,
    step: int,
)
    requires
        accepted_indexed_line_source(frame, commit, placement),
        0 <= step < (commit.expanded_events as int),
    ensures
        epm::cursor_event_at(frame, (frame.cursor.at as int + step) as usize)
            == Some(line_batch_wire(commit, placement, step)),
        match line_batch_wire(commit, placement, step) {
            epm::WireTopologyEvent::Edge { source_offset, .. }
            | epm::WireTopologyEvent::Leave { source_offset }
            | epm::WireTopologyEvent::Value { source_offset, .. } =>
                source_offset == line_batch_source_offset(commit, step),
        },
        commit.indexed_positions[line_batch_source_offset(commit, step) as int]
            == epm::trie_position(frame, line_batch_source_offset(commit, step)),
{
    let work = placement.active_work;
    let raw = placement.raw_events;
    let absolute = frame.cursor.at as int + step;
    let suffix = placement.suffix_events;
    let plan = placement.plan;
    match commit.direction {
        LineDirection::Forward => {
            let line_work = epm::act_line_forward_work(
                placement.record, commit.before.line_index,
            );
            assert(work == line_work + placement.continuation);
            forward_line_work_has_scalar_event(
                placement.record, commit.before.line_index,
                commit_width(commit) as usize, step,
            );
            assert forall|i: int| 0 <= i <= step
                implies emitted(work[i]).is_some() by {
                forward_line_work_has_scalar_event(
                    placement.record, commit.before.line_index,
                    commit_width(commit) as usize, i,
                );
                assert(i < line_work.len());
                assert(work[i] == line_work[i]);
            }
            assert(step < line_work.len());
            assert(step < work.len());
        },
        LineDirection::Unwind => {
            let line_work = epm::act_line_leave_work(
                placement.record, commit.before.leave_index,
            );
            assert(work == line_work + placement.continuation);
            unwind_line_work_has_scalar_event(
                placement.record, commit.before.leave_index,
                commit_width(commit) as usize, step,
            );
            assert forall|i: int| 0 <= i <= step
                implies emitted(work[i]).is_some() by {
                unwind_line_work_has_scalar_event(
                    placement.record, commit.before.leave_index,
                    commit_width(commit) as usize, i,
                );
                assert(i < line_work.len());
                assert(work[i] == line_work[i]);
            }
            assert(step < line_work.len());
            assert(step < work.len());
        },
    }
    expanded_emit_prefix_at(plan, work, placement.fuel, suffix, step);
    assert(emitted(work[step]) == Some(line_batch_wire(commit, placement, step)));
    assert(suffix[step] == line_batch_wire(commit, placement, step));
    assert(raw[absolute] == suffix[step]);
    assert(batch_safe_wire(raw[absolute]));
    attachment_preserves_batch_wire_at(
        frame.snapshot.bytes, frame.mode, raw, frame.value_table_start,
        placement.attached, absolute,
    );
    assert(epm::act_plan_events(frame, plan) == Some(placement.attached.events));
    assert(epm::cursor_event_at(frame, absolute as usize)
        == Some(placement.attached.events[absolute]));
    assert((line_batch_source_offset(commit, step) as int) < commit.indexed_positions.len());
    assert(commit.indexed_positions[line_batch_source_offset(commit, step) as int]
        == epm::trie_position(frame, line_batch_source_offset(commit, step)));
}

pub proof fn accepted_indexed_line_batch_has_exact_cursor_range(
    frame: epm::EpmFrame,
    commit: AcceptedLineCommitSource,
    placement: ActLineWorkPlacement,
)
    requires accepted_indexed_line_source(frame, commit, placement),
    ensures
        forall|step: int| 0 <= step < (commit.expanded_events as int) ==>
            #[trigger] epm::cursor_event_at(
                frame, (frame.cursor.at as int + step) as usize,
            ) == Some(line_batch_wire(commit, placement, step)),
        commit.last_source
            == epm::trie_position(frame, commit_last_offset(commit) as usize),
{
    assert forall|step: int| 0 <= step < (commit.expanded_events as int) implies
        #[trigger] epm::cursor_event_at(
            frame, (frame.cursor.at as int + step) as usize,
        ) == Some(line_batch_wire(commit, placement, step)) by {
        accepted_indexed_line_step_is_plan_event(frame, commit, placement, step);
    }
    assert(0 <= commit_last_offset(commit) < commit.indexed_positions.len());
    assert(commit.indexed_positions[commit_last_offset(commit)]
        == epm::trie_position(frame, commit_last_offset(commit) as usize));
}

/// The checked Rust total is a sum of 3 units per record and 3 per line byte,
/// less the root's absent incoming edge. Keeping the range sum on the formal
/// plan makes child subtree conservation independent of concrete event bytes.
pub open spec fn plan_event_units_range(
    plan: epm::ActPlanImage, start: nat, end: nat,
) -> nat
    decreases end - start,
{
    if start >= end || start >= plan.records.len() { 0 } else {
        3 + 3 * plan.records[start as int].line_len as nat
            + plan_event_units_range(plan, start + 1, end)
    }
}

pub open spec fn plan_enter_event_weight(
    plan: epm::ActPlanImage, index: usize,
) -> nat {
    if index >= plan.records.len() || plan.records[index as int].subtree_end <= index {
        0
    } else {
        let record = plan.records[index as int];
        2 + 3 * record.line_len as nat
            + plan_event_units_range(
                plan, index as nat + 1, record.subtree_end as nat,
            )
    }
}

pub open spec fn plan_work_event_weight(
    plan: epm::ActPlanImage, work: Seq<epm::ActWorkItem>,
) -> nat
    decreases work.len(),
{
    if work.len() == 0 { 0 } else {
        let head = match work[0] {
            epm::ActWorkItem::Emit(_) => 1,
            epm::ActWorkItem::Enter(index) => plan_enter_event_weight(plan, index),
        };
        head + plan_work_event_weight(plan, work.drop_first())
    }
}

pub open spec fn plan_work_head_weight(
    plan: epm::ActPlanImage, item: epm::ActWorkItem,
) -> nat {
    match item {
        epm::ActWorkItem::Emit(_) => 1,
        epm::ActWorkItem::Enter(index) => plan_enter_event_weight(plan, index),
    }
}

pub proof fn plan_event_units_range_additive(
    plan: epm::ActPlanImage, start: nat, middle: nat, end: nat,
)
    requires start <= middle <= end <= plan.records.len(),
    ensures plan_event_units_range(plan, start, end)
        == plan_event_units_range(plan, start, middle)
            + plan_event_units_range(plan, middle, end),
    decreases middle - start,
{
    if start < middle {
        plan_event_units_range_additive(plan, start + 1, middle, end);
    }
}

pub proof fn plan_work_event_weight_additive(
    plan: epm::ActPlanImage,
    first: Seq<epm::ActWorkItem>, second: Seq<epm::ActWorkItem>,
)
    ensures plan_work_event_weight(plan, first + second)
        == plan_work_event_weight(plan, first) + plan_work_event_weight(plan, second),
    decreases first.len(),
{
    if first.len() > 0 {
        plan_work_event_weight_additive(plan, first.drop_first(), second);
        let joined = first + second;
        assert(joined.len() > 0);
        assert(joined[0] == first[0]);
        assert(joined.drop_first() == first.drop_first() + second) by {
            assert(joined.drop_first() =~= first.drop_first() + second);
        }
        assert(plan_work_event_weight(plan, first)
            == plan_work_head_weight(plan, first[0])
                + plan_work_event_weight(plan, first.drop_first()));
        assert(plan_work_event_weight(plan, joined)
            == plan_work_head_weight(plan, first[0])
                + plan_work_event_weight(plan, first.drop_first() + second));
    } else {
        assert(first == Seq::empty());
        assert(first + second == second);
    }
}

pub proof fn plan_single_emit_weight(
    plan: epm::ActPlanImage, item: epm::ActWorkItem,
)
    requires item is Emit,
    ensures plan_work_event_weight(plan, seq![item]) == 1,
{
    let singleton = seq![item];
    assert(singleton.len() == 1);
    assert(singleton[0] == item);
    assert(singleton.drop_first() == Seq::empty());
    assert(plan_work_event_weight(plan, Seq::empty()) == 0);
}

pub proof fn plan_single_enter_weight(
    plan: epm::ActPlanImage, index: usize,
)
    ensures plan_work_event_weight(
        plan, seq![epm::ActWorkItem::Enter(index)],
    ) == plan_enter_event_weight(plan, index),
{
    let singleton = seq![epm::ActWorkItem::Enter(index)];
    assert(singleton.len() == 1);
    assert(singleton.drop_first() == Seq::empty());
    assert(plan_work_event_weight(plan, Seq::empty()) == 0);
}

pub proof fn plan_forward_line_work_weight(
    plan: epm::ActPlanImage, record: epm::ActRecordImage, index: usize,
)
    requires index <= record.line_len,
    ensures plan_work_event_weight(plan, epm::act_line_forward_work(record, index))
        == 2 * (record.line_len as nat - index as nat),
    decreases record.line_len - index,
{
    if index < record.line_len {
        assert(index < usize::MAX);
        let next = (index as int + 1) as usize;
        plan_forward_line_work_weight(plan, record, next);
        let head = seq![
            epm::ActWorkItem::Emit(epm::WireTopologyEvent::Edge {
                source_offset: (record.line_start as int + index as int) as usize,
                origin: epm::EdgeOrigin::Direct,
            }),
            epm::ActWorkItem::Emit(epm::WireTopologyEvent::Value {
                has_value: next == record.line_len && record.value.is_some(),
                value_body: None,
                source_offset: if next == record.line_len && record.value.is_some() {
                    record.value_source_offset
                } else {
                    (record.line_start as int + index as int) as usize
                },
            }),
        ];
        let tail = epm::act_line_forward_work(record, next);
        assert(epm::act_line_forward_work(record, index) == head + tail);
        plan_work_event_weight_additive(plan, head, tail);
        assert(head.len() == 2);
        assert(head[0] is Emit);
        assert(head.drop_first().len() == 1);
        assert(head.drop_first()[0] is Emit);
        assert(head.drop_first() == seq![head[1]]) by {
            assert(head.drop_first() =~= seq![head[1]]);
        }
        plan_single_emit_weight(plan, head[0]);
        plan_single_emit_weight(plan, head[1]);
        assert(plan_work_event_weight(plan, head.drop_first()) == 1);
        assert(plan_work_event_weight(plan, head) == 2);
    } else {
        assert(epm::act_line_forward_work(record, index) == Seq::empty());
    }
}

pub proof fn plan_leave_line_work_weight(
    plan: epm::ActPlanImage, record: epm::ActRecordImage, index: usize,
)
    requires index <= record.line_len,
    ensures plan_work_event_weight(plan, epm::act_line_leave_work(record, index))
        == record.line_len as nat - index as nat,
    decreases record.line_len - index,
{
    if index < record.line_len {
        assert(index < usize::MAX);
        let next = (index as int + 1) as usize;
        plan_leave_line_work_weight(plan, record, next);
        let head = seq![epm::ActWorkItem::Emit(epm::WireTopologyEvent::Leave {
            source_offset: (record.line_start as int + record.line_len as int
                - 1 - index as int) as usize,
        })];
        let tail = epm::act_line_leave_work(record, next);
        assert(epm::act_line_leave_work(record, index) == head + tail);
        plan_work_event_weight_additive(plan, head, tail);
        assert(head.len() == 1);
        assert(head[0] is Emit);
        assert(head == seq![head[0]]);
        plan_single_emit_weight(plan, head[0]);
        assert(plan_work_event_weight(plan, head) == 1);
    } else {
        assert(epm::act_line_leave_work(record, index) == Seq::empty());
    }
}

pub open spec fn plan_children_suffix_start(
    record: epm::ActRecordImage, child_number: usize,
) -> nat {
    if (child_number as int) >= record.children.len() {
        record.subtree_end as nat
    } else {
        record.children[child_number as int] as nat
    }
}

pub open spec fn plan_count_shape(plan: epm::ActPlanImage) -> bool {
    &&& plan.records.len() <= usize::MAX as nat
    &&& forall|i: int| 0 <= i < plan.records.len() ==> {
        let record = #[trigger] plan.records[i];
        &&& record.children.len() < usize::MAX as int
        &&& i < (record.subtree_end as int) <= plan.records.len()
        &&& if record.children.len() == 0 {
            record.subtree_end as int == i + 1
        } else {
            &&& record.children[0] as int == i + 1
            &&& record.subtree_end
                == plan.records[record.children.last() as int].subtree_end
            &&& forall|j: int| 0 <= j < record.children.len() ==> {
                let child_index = #[trigger] record.children[j];
                let child = plan.records[child_index as int];
                &&& i < (child_index as int) < (record.subtree_end as int)
                &&& child_index < plan.records.len()
                &&& child_index < child.subtree_end
                &&& child.incoming_source_offset.is_some()
                &&& child.incoming_origin.is_some()
                &&& if j + 1 < record.children.len() {
                    child.subtree_end == record.children[j + 1]
                } else {
                    child.subtree_end == record.subtree_end
                }
            }
        }
    }
}

pub open spec fn plan_parent_child_partition(
    plan: epm::ActPlanImage, parent: usize,
) -> bool {
    let record = plan.records[parent as int];
    &&& record.children.len() < usize::MAX as int
    &&& parent < record.subtree_end <= plan.records.len()
    &&& if record.children.len() == 0 {
        record.subtree_end as int == parent as int + 1
    } else {
        &&& record.children[0] as int == parent as int + 1
        &&& forall|j: int| 0 <= j < record.children.len() ==> {
            let child_index = #[trigger] record.children[j];
            let child = plan.records[child_index as int];
            &&& parent < child_index < record.subtree_end
            &&& child_index < plan.records.len()
            &&& child_index < child.subtree_end
            &&& child.incoming_source_offset.is_some()
            &&& child.incoming_origin.is_some()
            &&& if j + 1 < record.children.len() {
                child.subtree_end == record.children[j + 1]
            } else {
                child.subtree_end == record.subtree_end
            }
        }
    }
}

pub proof fn plan_count_shape_implies_parent_partition(
    plan: epm::ActPlanImage, parent: usize,
)
    requires plan_count_shape(plan), parent < plan.records.len(),
    ensures plan_parent_child_partition(plan, parent),
{
    assert(parent < plan.records.len());
    assert(plan.records[parent as int].children.len() < usize::MAX as int);
}

pub proof fn act_matched_child_implies_count_shape(
    bytes: Seq<u8>, plan: epm::ActPlanImage, i: int, j: int,
)
    requires
        0 <= i < plan.records.len(),
        0 <= j < plan.records[i].children.len(),
        epm::act_record_matches(bytes, plan.records, i),
        epm::act_record_matches(
            bytes, plan.records, plan.records[i].children[j] as int,
        ),
    ensures {
        let record = plan.records[i];
        let child_index = record.children[j];
        let child = plan.records[child_index as int];
        &&& i < (child_index as int) < (record.subtree_end as int)
        &&& child_index < plan.records.len()
        &&& child_index < child.subtree_end
        &&& child.incoming_source_offset.is_some()
        &&& child.incoming_origin.is_some()
        &&& if j + 1 < record.children.len() {
            child.subtree_end == record.children[j + 1]
        } else {
            child.subtree_end == record.subtree_end
        }
    },
{
    let record = plan.records[i];
    let child_index = record.children[j];
    let child = plan.records[child_index as int];
    assert(child_index < plan.records.len());
    assert(epm::act_record_matches(bytes, plan.records, child_index as int));
    if j + 1 < record.children.len() {
        assert(record.children[j + 1] == child.subtree_end);
    } else {
        assert(record.subtree_end
            == plan.records[record.children.last() as int].subtree_end);
        assert(j == record.children.len() - 1);
    }
}

pub proof fn act_matched_record_implies_count_shape(
    bytes: Seq<u8>, plan: epm::ActPlanImage, i: int,
)
    requires
        0 <= i < plan.records.len(),
        forall|k: int| 0 <= k < plan.records.len() ==>
            epm::act_record_matches(bytes, plan.records, k),
        plan.records[i].children.len() < usize::MAX as int,
    ensures {
        let record = plan.records[i];
        &&& record.children.len() < usize::MAX as int
        &&& i < (record.subtree_end as int) <= plan.records.len()
        &&& if record.children.len() == 0 {
            record.subtree_end as int == i + 1
        } else {
            &&& record.children[0] as int == i + 1
            &&& record.subtree_end
                == plan.records[record.children.last() as int].subtree_end
            &&& forall|j: int| 0 <= j < record.children.len() ==> {
                let child_index = #[trigger] record.children[j];
                let child = plan.records[child_index as int];
                &&& i < (child_index as int) < (record.subtree_end as int)
                &&& child_index < plan.records.len()
                &&& child_index < child.subtree_end
                &&& child.incoming_source_offset.is_some()
                &&& child.incoming_origin.is_some()
                &&& if j + 1 < record.children.len() {
                    child.subtree_end == record.children[j + 1]
                } else {
                    child.subtree_end == record.subtree_end
                }
            }
        }
    },
{
    let record = plan.records[i];
    assert(epm::act_record_matches(bytes, plan.records, i));
    if record.children.len() > 0 {
        assert forall|j: int| 0 <= j < record.children.len() implies {
            let child_index = #[trigger] record.children[j];
            let child = plan.records[child_index as int];
            &&& i < (child_index as int) < (record.subtree_end as int)
            &&& child_index < plan.records.len()
            &&& child_index < child.subtree_end
            &&& child.incoming_source_offset.is_some()
            &&& child.incoming_origin.is_some()
            &&& if j + 1 < record.children.len() {
                child.subtree_end == record.children[j + 1]
            } else {
                child.subtree_end == record.subtree_end
            }
        } by {
            let child_index = record.children[j];
            assert(child_index < plan.records.len());
            assert(epm::act_record_matches(
                bytes, plan.records, child_index as int,
            ));
            act_matched_child_implies_count_shape(bytes, plan, i, j);
        }
    }
}

pub proof fn act_records_imply_count_shape(
    bytes: Seq<u8>, plan: epm::ActPlanImage,
)
    requires
        plan.records.len() <= usize::MAX as nat,
        forall|i: int| 0 <= i < plan.records.len() ==> {
            &&& epm::act_record_matches(bytes, plan.records, i)
            &&& plan.records[i].children.len() < usize::MAX as int
    },
    ensures plan_count_shape(plan),
{
    assert forall|k: int| 0 <= k < plan.records.len() implies
        epm::act_record_matches(bytes, plan.records, k) by {
        assert(epm::act_record_matches(bytes, plan.records, k));
    }
    assert forall|i: int| 0 <= i < plan.records.len() implies {
        let record = #[trigger] plan.records[i];
        &&& record.children.len() < usize::MAX as int
        &&& i < (record.subtree_end as int) <= plan.records.len()
        &&& if record.children.len() == 0 {
            record.subtree_end as int == i + 1
        } else {
            &&& record.children[0] as int == i + 1
            &&& record.subtree_end
                == plan.records[record.children.last() as int].subtree_end
            &&& forall|j: int| 0 <= j < record.children.len() ==> {
                let child_index = #[trigger] record.children[j];
                let child = plan.records[child_index as int];
                &&& i < (child_index as int) < (record.subtree_end as int)
                &&& child_index < plan.records.len()
                &&& child_index < child.subtree_end
                &&& child.incoming_source_offset.is_some()
                &&& child.incoming_origin.is_some()
                &&& if j + 1 < record.children.len() {
                    child.subtree_end == record.children[j + 1]
                } else {
                    child.subtree_end == record.subtree_end
                }
            }
        }
    } by {
        assert(epm::act_record_matches(bytes, plan.records, i));
        assert(plan.records[i].children.len() < usize::MAX as int);
        act_matched_record_implies_count_shape(bytes, plan, i);
    }
}

/// Child ordinal intervals partition their parent's preorder suffix. This is
/// the structural condition already checked by `act_record_matches`; no
/// event-count or expansion result occurs in its premises.
pub proof fn plan_child_preorder_step(
    plan: epm::ActPlanImage, parent: usize, child_number: usize,
)
    requires
        plan_parent_child_partition(plan, parent),
        parent < plan.records.len(),
        (child_number as int) < plan.records[parent as int].children.len(),
    ensures {
        let record = plan.records[parent as int];
        let child_index = record.children[child_number as int];
        let child = plan.records[child_index as int];
        let next = (child_number as int + 1) as usize;
        &&& child_number < usize::MAX
        &&& parent < child_index < record.subtree_end
        &&& child_index < plan.records.len()
        &&& child_index < child.subtree_end <= record.subtree_end
        &&& child.incoming_source_offset.is_some()
        &&& child.incoming_origin.is_some()
        &&& plan_children_suffix_start(record, next) == child.subtree_end as nat
    },
{
    let record = plan.records[parent as int];
    let child_index = record.children[child_number as int];
    let child = plan.records[child_index as int];
    assert(record.children.len() < usize::MAX as int);
    assert(child_number < usize::MAX);
    if (child_number as int + 1) < record.children.len() {
        assert(child.subtree_end == record.children[child_number as int + 1]);
    } else {
        assert(child.subtree_end == record.subtree_end);
    }
}

pub proof fn plan_child_head_matches_preorder_range(
    plan: epm::ActPlanImage, child_index: usize,
    edge: epm::ActWorkItem,
)
    requires
        child_index < plan.records.len(),
        child_index < plan.records[child_index as int].subtree_end
            <= plan.records.len(),
        edge is Emit,
    ensures plan_work_event_weight(
        plan, seq![edge, epm::ActWorkItem::Enter(child_index)],
    ) == plan_event_units_range(
        plan, child_index as nat,
        plan.records[child_index as int].subtree_end as nat,
    ),
{
    let child = plan.records[child_index as int];
    let head = seq![edge, epm::ActWorkItem::Enter(child_index)];
    let edge_part = seq![edge];
    let enter_part = seq![epm::ActWorkItem::Enter(child_index)];
    assert(head.len() == 2);
    assert(head == edge_part + enter_part);
    plan_single_emit_weight(plan, edge);
    plan_single_enter_weight(plan, child_index);
    plan_work_event_weight_additive(plan, edge_part, enter_part);
    assert(plan_work_event_weight(plan, head)
        == 1 + plan_enter_event_weight(plan, child_index));
    assert(plan_enter_event_weight(plan, child_index)
        == 2 + 3 * child.line_len as nat
            + plan_event_units_range(
                plan, child_index as nat + 1, child.subtree_end as nat,
            ));
    assert(plan_event_units_range(plan, child_index as nat, child.subtree_end as nat)
        == 3 + 3 * child.line_len as nat
            + plan_event_units_range(
                plan, child_index as nat + 1, child.subtree_end as nat,
            ));
}

pub proof fn plan_children_work_weight_matches_preorder_range(
    plan: epm::ActPlanImage,
    parent: usize, child_number: usize,
)
    requires
        plan_parent_child_partition(plan, parent),
        parent < plan.records.len(),
        child_number as int <= plan.records[parent as int].children.len(),
    ensures {
        let record = plan.records[parent as int];
        plan_work_event_weight(
            plan, epm::act_children_work(plan, record, child_number),
        ) == plan_event_units_range(
            plan, plan_children_suffix_start(record, child_number),
            record.subtree_end as nat,
        )
    },
    decreases plan.records[parent as int].children.len() - child_number as int,
{
    let record = plan.records[parent as int];
    if (child_number as int) < record.children.len() {
        plan_child_preorder_step(plan, parent, child_number);
        let child_index = record.children[child_number as int];
        let child = plan.records[child_index as int];
        let next = (child_number as int + 1) as usize;
        plan_children_work_weight_matches_preorder_range(plan, parent, next);
        assert(plan_children_suffix_start(record, next) == child.subtree_end as nat);
        plan_event_units_range_additive(
            plan, child_index as nat, child.subtree_end as nat,
            record.subtree_end as nat,
        );
        let edge = epm::ActWorkItem::Emit(epm::WireTopologyEvent::Edge {
            source_offset: child.incoming_source_offset.unwrap(),
            origin: child.incoming_origin.unwrap(),
        });
        let head = seq![edge, epm::ActWorkItem::Enter(child_index)];
        let tail = epm::act_children_work(plan, record, next);
        plan_work_event_weight_additive(plan, head, tail);
        plan_child_head_matches_preorder_range(plan, child_index, edge);
        assert(epm::act_children_work(plan, record, child_number) == head + tail);
        assert(plan_children_suffix_start(record, child_number)
            == child_index as nat);
    } else {
        assert(epm::act_children_work(plan, record, child_number)
            == Seq::empty());
        assert(plan_children_suffix_start(record, child_number)
            == record.subtree_end as nat);
    }
}

pub proof fn plan_record_work_weight_from_parts(
    plan: epm::ActPlanImage, index: usize,
)
    requires
        index < plan.records.len(),
        index < plan.records[index as int].subtree_end,
        plan_children_suffix_start(plan.records[index as int], 0)
            == index as nat + 1,
        plan_work_event_weight(
            plan, epm::act_line_forward_work(plan.records[index as int], 0),
        ) == 2 * plan.records[index as int].line_len as nat,
        plan_work_event_weight(
            plan, epm::act_line_leave_work(plan.records[index as int], 0),
        ) == plan.records[index as int].line_len as nat,
        plan_work_event_weight(
            plan, epm::act_children_work(
                plan, plan.records[index as int], 0,
            ),
        ) == plan_event_units_range(
            plan, index as nat + 1,
            plan.records[index as int].subtree_end as nat,
        ),
    ensures plan_work_event_weight(plan, epm::act_record_work(plan, index))
        == plan_enter_event_weight(plan, index),
{
    let record = plan.records[index as int];
    let value = seq![epm::ActWorkItem::Emit(epm::WireTopologyEvent::Value {
        has_value: record.line_len == 0 && record.value.is_some(),
        value_body: None,
        source_offset: record.node_id,
    })];
    let forward = epm::act_line_forward_work(record, 0);
    let children = epm::act_children_work(plan, record, 0);
    let leave = epm::act_line_leave_work(record, 0);
    let final_leave = seq![epm::ActWorkItem::Emit(epm::WireTopologyEvent::Leave {
        source_offset: record.node_id,
    })];
    plan_single_emit_weight(plan, value[0]);
    plan_single_emit_weight(plan, final_leave[0]);
    plan_work_event_weight_additive(plan, value, forward);
    plan_work_event_weight_additive(plan, value + forward, children);
    plan_work_event_weight_additive(plan, value + forward + children, leave);
    plan_work_event_weight_additive(
        plan, value + forward + children + leave, final_leave,
    );
    assert(plan_work_event_weight(plan, value) == 1);
    assert(plan_work_event_weight(plan, final_leave) == 1);
    assert(plan_work_event_weight(plan, forward)
        == 2 * record.line_len as nat);
    assert(plan_work_event_weight(plan, leave)
        == record.line_len as nat);
    assert(plan_work_event_weight(plan, children)
        == plan_event_units_range(
            plan, index as nat + 1, record.subtree_end as nat,
        ));
    assert(epm::act_record_work(plan, index)
        == value + forward + children + leave + final_leave);
    assert(plan_work_event_weight(plan, epm::act_record_work(plan, index))
        == 2 + 3 * record.line_len as nat
            + plan_event_units_range(
                plan, index as nat + 1, record.subtree_end as nat,
            ));
}

pub proof fn plan_record_work_weight_matches_subtree(
    plan: epm::ActPlanImage, index: usize,
)
    requires
        plan_count_shape(plan),
        index < plan.records.len(),
    ensures plan_work_event_weight(plan, epm::act_record_work(plan, index))
        == plan_enter_event_weight(plan, index),
{
    let record = plan.records[index as int];
    plan_count_shape_implies_parent_partition(plan, index);
    plan_forward_line_work_weight(plan, record, 0);
    plan_leave_line_work_weight(plan, record, 0);
    plan_children_work_weight_matches_preorder_range(plan, index, 0);
    assert(plan_children_suffix_start(record, 0) == index as nat + 1);
    plan_record_work_weight_from_parts(plan, index);
}

/// Every `Emit` contributes one event; expanding an `Enter` replaces its
/// subtree weight with the record's concrete work of equal weight. This proof
/// uses the actual ACT interpreter and never supplies an expected event list.
pub proof fn act_expansion_preserves_event_weight(
    plan: epm::ActPlanImage,
    work: Seq<epm::ActWorkItem>, fuel: nat,
    events: Seq<epm::WireTopologyEvent>,
)
    requires
        epm::act_expand_work(plan, work, fuel) == Some(events),
        plan_count_shape(plan),
    ensures events.len() == plan_work_event_weight(plan, work),
    decreases fuel,
{
    if work.len() > 0 {
        assert(fuel > 0);
        let rest = work.drop_first();
        let tail_fuel = (fuel as int - 1) as nat;
        match work[0] {
            epm::ActWorkItem::Emit(event) => {
                let tail = epm::act_expand_work(plan, rest, tail_fuel).unwrap();
                assert(epm::act_expand_work(plan, rest, tail_fuel) == Some(tail));
                act_expansion_preserves_event_weight(
                    plan, rest, tail_fuel, tail,
                );
                assert(events == seq![event] + tail);
            },
            epm::ActWorkItem::Enter(index) => {
                assert(index < plan.records.len());
                let record_work = epm::act_record_work(plan, index);
                assert(epm::act_expand_work(
                    plan, record_work + rest, tail_fuel,
                ) == Some(events));
                act_expansion_preserves_event_weight(
                    plan, record_work + rest, tail_fuel, events,
                );
                plan_record_work_weight_matches_subtree(plan, index);
                plan_work_event_weight_additive(plan, record_work, rest);
            },
        }
    }
}

pub proof fn source_suffix_units_match_plan_range(
    plan: epm::ActPlanImage,
    records: Seq<epm::ValidatedTopologyRecordImage>,
    at: nat,
)
    requires
        plan.records.len() == records.len(),
        1 <= at <= records.len(),
        forall|i: int| 0 <= i < records.len() ==>
            plan.records[i].line_len == records[i].line_len,
    ensures plan_event_units_range(plan, at, plan.records.len())
        == source_topology_event_count(records, at),
    decreases records.len() - at,
{
    if at < records.len() {
        source_suffix_units_match_plan_range(plan, records, at + 1);
    }
}

pub proof fn accepted_source_plan_has_count_shape(
    frame: epm::EpmFrame, source: AcceptedActPlanSourceImage,
)
    requires
        epm::frame_layout_wf(frame),
        accepted_act_structure_projects_frame(frame, source),
    ensures {
        let plan = frame.cursor.act_plan.unwrap();
        &&& plan_count_shape(plan)
        &&& plan.records.len() == source.records.len()
        &&& plan.records[0].subtree_end == plan.records.len()
        &&& forall|i: int| 0 <= i < plan.records.len() ==>
            plan.records[i].line_len == source.records[i].line_len
    },
{
    let plan = frame.cursor.act_plan.unwrap();
    let bytes = frame.snapshot.bytes.subrange(
        frame.trie_start as int, frame.trie_end as int,
    );
    let validated = frame.cursor.validated_plan.unwrap();
    assert(validated == validated_image_from_source(source));
    assert(epm::act_plan_projects_validated(frame, plan, validated));
    assert(epm::act_plan_matches_frame(frame, plan));
    assert(plan.records.len() == source.records.len());
    assert(plan.records.len() <= usize::MAX as nat);
    assert(plan.records[0].subtree_end == plan.records.len());
    assert forall|i: int| 0 <= i < plan.records.len() implies
        plan.records[i].line_len == source.records[i].line_len by {
        assert(epm::act_record_projects_validated(
            bytes, plan.records[i], source.records[i],
        ));
    }
    assert forall|i: int| 0 <= i < plan.records.len() implies {
        &&& epm::act_record_matches(bytes, plan.records, i)
        &&& plan.records[i].children.len() < usize::MAX as int
    } by {
        assert(epm::act_record_matches(bytes, plan.records, i));
        assert(epm::act_record_projects_validated(
            bytes, plan.records[i], source.records[i],
        ));
        assert(plan.records[i].children.len() == source.records[i].child_count as int);
    }
    act_records_imply_count_shape(bytes, plan);
}

pub proof fn plan_raw_expansion_has_count(
    plan: epm::ActPlanImage,
    fuel: nat,
    raw: Seq<epm::WireTopologyEvent>,
)
    requires
        plan_count_shape(plan),
        epm::act_expand_work(
            plan, seq![epm::ActWorkItem::Enter(0)], fuel,
        ) == Some(raw),
    ensures raw.len() == plan_enter_event_weight(plan, 0),
{
    let initial = seq![epm::ActWorkItem::Enter(0)];
    act_expansion_preserves_event_weight(plan, initial, fuel, raw);
    plan_single_enter_weight(plan, 0);
    assert(plan_work_event_weight(plan, initial)
        == plan_enter_event_weight(plan, 0));
}

pub proof fn accepted_plan_raw_expansion_has_count(
    frame: epm::EpmFrame,
)
    requires
        epm::frame_layout_wf(frame),
        frame.cursor.act_plan.is_some(),
        plan_count_shape(frame.cursor.act_plan.unwrap()),
    ensures {
        let plan = frame.cursor.act_plan.unwrap();
        let raw = epm::act_expand_work(
            plan, seq![epm::ActWorkItem::Enter(0)],
            epm::act_work_budget(plan, 0) + 1,
        ).unwrap();
        raw.len() == plan_enter_event_weight(plan, 0)
    },
{
    let plan = frame.cursor.act_plan.unwrap();
    let fuel = epm::act_work_budget(plan, 0) + 1;
    let initial = seq![epm::ActWorkItem::Enter(0)];
    let raw = epm::act_expand_work(plan, initial, fuel).unwrap();
    assert(epm::act_expand_work(plan, initial, fuel) == Some(raw));
    plan_raw_expansion_has_count(plan, fuel, raw);
}

pub proof fn accepted_plan_attachment_preserves_raw_count(
    frame: epm::EpmFrame,
)
    requires
        epm::frame_layout_wf(frame),
        frame.cursor.act_plan.is_some(),
    ensures {
        let plan = frame.cursor.act_plan.unwrap();
        let raw = epm::act_expand_work(
            plan, seq![epm::ActWorkItem::Enter(0)],
            epm::act_work_budget(plan, 0) + 1,
        ).unwrap();
        epm::act_plan_events(frame, plan).unwrap().len() == raw.len()
    },
{
    let plan = frame.cursor.act_plan.unwrap();
    let raw = epm::act_expand_work(
        plan, seq![epm::ActWorkItem::Enter(0)],
        epm::act_work_budget(plan, 0) + 1,
    ).unwrap();
    let attached = epm::attach_act_value_bodies(
        frame.snapshot.bytes, frame.mode, raw, frame.value_table_start,
    ).unwrap();
    assert(epm::act_plan_events(frame, plan) == Some(attached.events));
    attachment_preserves_event_count(
        frame.snapshot.bytes, frame.mode, raw, frame.value_table_start, attached,
    );
}

/// An accepted Rust plan's checked count is the length of the formal ACT
/// expansion. The selected Map Value is not mentioned here: this is a global
/// conservation law for the source-validated preorder tree.
pub proof fn accepted_act_source_count_equals_expansion(
    frame: epm::EpmFrame, source: AcceptedActPlanSourceImage,
)
    requires
        epm::frame_layout_wf(frame),
        accepted_act_structure_projects_frame(frame, source),
    ensures accepted_act_event_count_matches_source(frame, source),
{
    let plan = frame.cursor.act_plan.unwrap();
    accepted_source_plan_has_count_shape(frame, source);
    source_suffix_units_match_plan_range(plan, source.records, 1);
    assert(plan_enter_event_weight(plan, 0)
        == source_topology_event_count(source.records, 0));
    accepted_plan_raw_expansion_has_count(frame);
    let raw = epm::act_expand_work(
        plan, seq![epm::ActWorkItem::Enter(0)],
        epm::act_work_budget(plan, 0) + 1,
    ).unwrap();
    assert(raw.len() == source.topology_event_count as nat);
    accepted_plan_attachment_preserves_raw_count(frame);
}

}
