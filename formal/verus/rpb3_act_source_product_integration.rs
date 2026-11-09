//! ACT source projection at the integrated product's active EPM frame.
//!
//! The accepted plan and indexed bytes select the scalar event range. The
//! `prepare().commit()` image selects its physical source range and post-ACT
//! position. The only work-queue premise identifies the active DFS suffix of
//! the validated plan; it supplies neither expected events nor a postcursor.

use vstd::prelude::*;

use crate::{
    rpb3_epathmap_event_refinement as epm, rpb3_epm_active_line_work_source as active,
    rpb3_epm_batch_cursor_composition as batch, rpb3_epm_batch_cursor_source_extraction as source,
    rpb3_epm_push_transport_source_refinement as transport,
    rpb3_epm_restore_run_expansion as restore, ProductScoreState,
};

verus! {

pub open spec fn active_product_epm(state: ProductScoreState) -> Option<epm::EpmFrame> {
    if state.epm_frames.len() == 0 { None } else { state.epm_frames.last() }
}

/// This is only the EPM component of an integrated transition. The bridge,
/// map, and admission reducer are handled by the integrated action theorem.
pub open spec fn replace_active_epm(
    before: ProductScoreState, frame: epm::EpmFrame,
) -> ProductScoreState
    recommends before.epm_frames.len() > 0,
{
    ProductScoreState {
        bridge: before.bridge,
        map: before.map,
        epm_frames: before.epm_frames.update(before.epm_frames.len() - 1, Some(frame)),
        indexed_regions: before.indexed_regions,
        accepted_epm_sources: before.accepted_epm_sources,
    }
}

/// The raw stream, its value attachment, and the prefix are calculated from
/// the accepted frame. Only `continuation` (the live DFS work stack) and its
/// finite expansion fuel remain source-placement observations.
pub open spec fn derived_line_placement(
    frame: epm::EpmFrame,
    commit: source::AcceptedLineCommitSource,
    continuation: Seq<epm::ActWorkItem>,
    fuel: nat,
) -> Option<source::ActLineWorkPlacement> {
    if frame.cursor.act_plan.is_none() { None } else {
        let plan = frame.cursor.act_plan.unwrap();
        let raw_option = epm::act_expand_work(
            plan, seq![epm::ActWorkItem::Enter(0)],
            epm::act_work_budget(plan, 0) + 1,
        );
        if raw_option.is_none() || commit.before.record as int >= plan.records.len() {
            None
        } else {
            let raw = raw_option.unwrap();
            let attached_option = epm::attach_act_value_bodies(
                frame.snapshot.bytes, frame.mode, raw, frame.value_table_start,
            );
            if attached_option.is_none() || frame.cursor.at as int > raw.len() { None } else {
                let record = plan.records[commit.before.record as int];
                let active_work = match commit.direction {
                    source::LineDirection::Forward => epm::act_line_forward_work(
                        record, commit.before.line_index,
                    ) + continuation,
                    source::LineDirection::Unwind => epm::act_line_leave_work(
                        record, commit.before.leave_index,
                    ) + continuation,
                };
                Some(source::ActLineWorkPlacement {
                    plan,
                    record,
                    prefix: raw.subrange(0, frame.cursor.at as int),
                    continuation,
                    active_work,
                    suffix_events: raw.subrange(frame.cursor.at as int, raw.len() as int),
                    raw_events: raw,
                    attached: attached_option.unwrap(),
                    fuel,
                })
            }
        }
    }
}

/// Capacity and parser conditions of the production Forward path, excluding
/// cursor-event equality. That equality is established from the ACT source.
pub open spec fn forward_capacity_without_event_premise(
    frame: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
) -> bool {
    &&& epm::frame_layout_wf(frame)
    &&& !frame.cursor.done
    &&& frame.wait.is_none()
    &&& frame.path.pending == Some(pending)
    &&& !frame.path.root_left
    &&& latest_restore_state_id != Some(state_id)
    &&& start as int + width as int <= frame.trie_end - frame.trie_start
    &&& frame.path.parser.stamp as int == frame.path.path_bytes.len()
    &&& epm::parser_gate_matches(frame.path.gate, frame.path.parser)
    &&& epm::parser::parser_forward_capacity(frame.path.parser, width as int)
    &&& restore::forward_capacity_facts_match(
        batch::path_source(frame), pending, batch::trie_source(frame),
        start, width, facts,
    )
}

pub open spec fn accepted_product_forward_source(
    before: ProductScoreState,
    plan_source: source::AcceptedActPlanSourceImage,
    work: active::ActiveLineWorkSource,
    width: usize,
    commit: source::AcceptedLineCommitSource,
    continuation: Seq<epm::ActWorkItem>,
    fuel: nat,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
) -> bool {
    if active_product_epm(before).is_none() { false } else {
        let frame = active_product_epm(before).unwrap();
        let placement = derived_line_placement(frame, commit, continuation, fuel);
        &&& crate::product_score_wf(before)
        &&& source::accepted_act_structure_projects_frame(frame, plan_source)
        &&& active::active_line_work_source_shape(work)
        &&& active::prepared_commit_source_projection(work, width, commit)
        &&& work.direction == source::LineDirection::Forward
        &&& work.event_index == frame.cursor.at
        &&& placement.is_some()
        &&& source::accepted_indexed_line_source(frame, commit, placement.unwrap())
        &&& forward_capacity_without_event_premise(
            frame, commit.start, width, pending, state_id,
            latest_restore_state_id, facts,
        )
    }
}

/// One prepared ACT Forward batch determines the exact scalar cursor range,
/// indexed source anchors, and the EPM component of the product successor.
/// No expected event sequence or poststate is supplied by the caller.
pub proof fn prepared_forward_batch_aligns_act_source(
    frame: epm::EpmFrame,
    work: active::ActiveLineWorkSource,
    width: usize,
    commit: source::AcceptedLineCommitSource,
    continuation: Seq<epm::ActWorkItem>,
    fuel: nat,
)
    requires
        active::active_line_work_source_shape(work),
        active::prepared_commit_source_projection(work, width, commit),
        work.direction == source::LineDirection::Forward,
        work.event_index == frame.cursor.at,
        !frame.cursor.done,
        derived_line_placement(frame, commit, continuation, fuel).is_some(),
        source::accepted_indexed_line_source(
            frame, commit,
            derived_line_placement(frame, commit, continuation, fuel).unwrap(),
        ),
    ensures {
        let chunk = transport::EpmPushTransportChunk::Forward {
            start: commit.start, width,
        };
        &&& commit.expanded_events as int == 2 * width as int
        &&& source::commit_width(commit) == width as int
        &&& batch::batch_cursor_source_aligned(frame, chunk)
        &&& forall|i: int| 0 <= i < transport::epm_push_chunk_len(chunk) ==>
            #[trigger] epm::cursor_event_at(
                frame, (frame.cursor.at as int + i) as usize,
            ) == epm::wire_event(transport::epm_push_chunk_event(chunk, i))
    },
{
    let placement = derived_line_placement(frame, commit, continuation, fuel).unwrap();
    let chunk = transport::EpmPushTransportChunk::Forward {
        start: commit.start, width,
    };
    active::prepared_line_derives_scalar_schedule_guard(work, width, commit);
    source::accepted_indexed_line_batch_has_exact_cursor_range(frame, commit, placement);
    assert(epm::frame_layout_wf(frame));
    assert(!frame.cursor.done);
    assert(transport::epm_push_chunk_events(chunk).len()
        == 2 * width as nat);
    assert(commit.expanded_events as int == 2 * width as int);
    assert(frame.cursor.at as int + transport::epm_push_chunk_events(chunk).len()
        <= epm::cursor_event_count(frame) as int);
    assert forall|i: int| 0 <= i < transport::epm_push_chunk_len(chunk) implies
        #[trigger] epm::cursor_event_at(
            frame, (frame.cursor.at as int + i) as usize,
        ) == epm::wire_event(transport::epm_push_chunk_event(chunk, i)) by {
        assert(epm::cursor_event_at(frame, (frame.cursor.at as int + i) as usize)
            == Some(source::line_batch_wire(commit, placement, i)));
        assert(source::line_batch_wire(commit, placement, i)
            == source::forward_line_wire(placement.record, commit.before.line_index, i));
        assert(commit.start as int == placement.record.line_start as int
            + commit.before.line_index as int);
    }
    assert(batch::batch_cursor_source_aligned(frame, chunk));
}

pub proof fn accepted_product_forward_batch_projects_source(
    before: ProductScoreState,
    plan_source: source::AcceptedActPlanSourceImage,
    work: active::ActiveLineWorkSource,
    width: usize,
    commit: source::AcceptedLineCommitSource,
    continuation: Seq<epm::ActWorkItem>,
    fuel: nat,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires accepted_product_forward_source(
        before, plan_source, work, width, commit, continuation, fuel,
        pending, state_id, latest_restore_state_id, facts,
    ),
    ensures {
        let frame = active_product_epm(before).unwrap();
        let chunk = transport::EpmPushTransportChunk::Forward {
            start: commit.start, width,
        };
        let after_frame = batch::forward_batch_after(
            frame, commit.start, width, pending, state_id,
        );
        let after = replace_active_epm(before, after_frame);
        &&& after.epm_frames.last() == Some(after_frame)
        &&& after_frame.cursor.at as int == frame.cursor.at as int + 2 * width as int
        &&& commit.after == active::position_after_commit(
            work.position, source::LineDirection::Forward, width,
        )
        &&& after_frame.cursor.at as int
            == work.event_index as int + commit.expanded_events as int
        &&& after_frame.last_source == commit.last_source
        &&& after_frame.cursor.act_plan == frame.cursor.act_plan
        &&& after_frame.cursor.validated_plan == frame.cursor.validated_plan
        &&& after_frame.snapshot == before.indexed_regions.last()
        &&& after_frame.snapshot.bytes
            == before.bridge.admission.frames.last().source_bytes
        &&& after_frame.path.path_bytes == batch::path_source(frame).bytes
            + batch::trie_source(frame).bytes.subrange(
                commit.start as int, commit.end as int,
            )
        &&& forall|i: int| 0 <= i < transport::epm_push_chunk_len(chunk) ==>
            #[trigger] epm::cursor_event_at(
                frame, (frame.cursor.at as int + i) as usize,
            ) == epm::wire_event(transport::epm_push_chunk_event(chunk, i))
    },
{
    let frame = active_product_epm(before).unwrap();
    assert(!frame.cursor.done);
    prepared_forward_batch_aligns_act_source(
        frame, work, width, commit, continuation, fuel,
    );
    assert(commit.expanded_events as int == 2 * width as int);
    assert(source::commit_width(commit) == width as int);
    assert(batch::forward_batch_source_guard(
        frame, commit.start, width, pending, state_id,
        latest_restore_state_id, facts,
    ));
    batch::accepted_forward_batch_composes_cursor_and_restore_run(
        frame, commit.start, width, pending, state_id,
        latest_restore_state_id, facts,
    );
    let after_frame = batch::forward_batch_after(
        frame, commit.start, width, pending, state_id,
    );
    assert(after_frame.last_source == epm::trie_position(
        frame, (commit.start as int + width as int - 1) as usize,
    ));
    assert(commit.last_source == epm::trie_position(
        frame, source::commit_last_offset(commit) as usize,
    ));
    assert(source::commit_last_offset(commit) as int
        == commit.start as int + width as int - 1);
}

/// The production atomic `next_event_with_source` receipt fixes the selected
/// Map Value body and the cursor fields that admission must carry forward.
pub open spec fn map_value_cursor_after(
    frame: epm::EpmFrame, receipt: source::MapValueNextEventSource,
) -> epm::EpmFrame {
    epm::EpmFrame {
        path: frame.path,
        cursor: epm::EpmCursorImage {
            act_plan: frame.cursor.act_plan,
            validated_plan: frame.cursor.validated_plan,
            at: receipt.event_index_after,
            declared_value_count: frame.cursor.declared_value_count,
            map_values_seen: receipt.map_values_seen_after,
            value_table_offset: receipt.post_value_table_offset,
            done: false,
        },
        wait: frame.wait,
        mode: frame.mode,
        entries_stable: frame.entries_stable,
        last_source: receipt.act_source,
        snapshot: frame.snapshot,
        backing: frame.backing,
        trie_start: frame.trie_start,
        trie_end: frame.trie_end,
        value_table_start: frame.value_table_start,
        mode_offset: frame.mode_offset,
    }
}

pub open spec fn map_value_event_from_receipt(
    receipt: source::MapValueNextEventSource,
) -> epm::EpmEvent {
    epm::EpmEvent::Value {
        has_value: true,
        value_body: Some(receipt.body),
        source_offset: receipt.act_source_offset,
    }
}

pub open spec fn map_value_push_after(
    frame: epm::EpmFrame,
    receipt: source::MapValueNextEventSource,
    return_id: u64,
) -> epm::EpmFrame {
    epm::EpmFrame {
        wait: Some(epm::EpmWait::MapValue { return_id }),
        entries_stable: frame.entries_stable && frame.path.gate.stable,
        ..map_value_cursor_after(frame, receipt)
    }
}

pub open spec fn map_value_push_action(
    frame: epm::EpmFrame,
    receipt: source::MapValueNextEventSource,
    return_id: u64,
) -> epm::EpmAction {
    epm::EpmAction::Push(epm::ChildRequest::ParValue {
        return_id,
        body: epm::indexed_subview(frame.snapshot, receipt.body),
    })
}

/// Isolate the cursor proof from the integrated admission-frame invariant.
/// The attached body and postcursor are selected entirely by the frame and
/// the atomic production receipt.
pub proof fn accepted_map_value_wire_event(
    frame: epm::EpmFrame,
    receipt: source::MapValueNextEventSource,
)
    requires source::selected_map_value_attachment_source(frame, receipt),
    ensures epm::cursor_event_at(frame, frame.cursor.at)
        == epm::wire_event(map_value_event_from_receipt(receipt)),
{
    source::selected_map_value_attachment_binds_body(frame, receipt);
}

/// The current EPM model stores the full ACT event count in `usize`, while
/// `act_plan_events` is an unbounded mathematical sequence. This condition is
/// separate from the receipt's checked one-step event-index increment.
pub open spec fn accepted_act_event_count_fits_cursor(frame: epm::EpmFrame) -> bool {
    if frame.cursor.act_plan.is_none() { false } else {
        match epm::act_plan_events(frame, frame.cursor.act_plan.unwrap()) {
            Some(events) => events.len() <= usize::MAX as nat,
            None => false,
        }
    }
}

pub proof fn accepted_source_count_bounds_formal_events(
    frame: epm::EpmFrame,
    plan_source: source::AcceptedActPlanSourceImage,
)
    requires source::accepted_act_event_count_matches_source(frame, plan_source),
    ensures accepted_act_event_count_fits_cursor(frame),
{
    let plan = frame.cursor.act_plan.unwrap();
    let events = epm::act_plan_events(frame, plan).unwrap();
    assert(events.len() == plan_source.topology_event_count as nat);
    assert(events.len() <= usize::MAX as nat);
}

pub proof fn accepted_map_value_event_index_bounded(
    frame: epm::EpmFrame,
    receipt: source::MapValueNextEventSource,
)
    requires
        source::selected_map_value_attachment_source(frame, receipt),
        accepted_act_event_count_fits_cursor(frame),
    ensures frame.cursor.at < epm::cursor_event_count(frame),
{
    accepted_map_value_wire_event(frame, receipt);
    assert(frame.mode == epm::EpmMode::Map);
    let plan = frame.cursor.act_plan.unwrap();
    assert(epm::act_plan_events(frame, plan).is_some());
    let events = epm::act_plan_events(frame, plan).unwrap();
    assert(events.len() <= usize::MAX as nat);
    if frame.cursor.at as int >= events.len() {
        assert(epm::cursor_event_at(frame, frame.cursor.at).is_none());
        assert(false);
    }
    assert(epm::cursor_event_count(frame) == events.len() as usize);
}

/// `act_plan_events` reads the indexed source and declared count, but no
/// mutable cursor position, wait state, or path-reducer field.
pub open spec fn same_act_layout_source(
    before: epm::EpmFrame, after: epm::EpmFrame,
) -> bool {
    &&& after.path == before.path
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

pub proof fn same_act_layout_source_preserves_events(
    before: epm::EpmFrame, after: epm::EpmFrame,
)
    requires same_act_layout_source(before, after),
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

pub proof fn same_act_layout_source_preserves_wf(
    before: epm::EpmFrame, after: epm::EpmFrame,
)
    requires
        epm::frame_layout_wf(before),
        same_act_layout_source(before, after),
        after.cursor.at <= epm::cursor_event_count(before),
        before.value_table_start <= after.cursor.value_table_offset,
        after.cursor.value_table_offset <= before.snapshot.bytes.len(),
        after.cursor.at != 0,
    ensures epm::frame_layout_wf(after),
{
    same_act_layout_source_preserves_events(before, after);
    if before.cursor.act_plan.is_some() {
        let plan = before.cursor.act_plan.unwrap();
        let validated = before.cursor.validated_plan.unwrap();
        assert(epm::act_plan_projects_validated(before, plan, validated)
            == epm::act_plan_projects_validated(after, plan, validated));
        assert(epm::act_plan_events(after, plan).is_some());
    }
    assert(epm::frame_layout_wf(after));
}

pub proof fn accepted_map_value_post_layout(
    frame: epm::EpmFrame,
    receipt: source::MapValueNextEventSource,
)
    requires
        source::selected_map_value_attachment_source(frame, receipt),
        accepted_act_event_count_fits_cursor(frame),
    ensures epm::frame_layout_wf(map_value_cursor_after(frame, receipt)),
{
    let after_frame = map_value_cursor_after(frame, receipt);
    accepted_map_value_event_index_bounded(frame, receipt);
    assert(same_act_layout_source(frame, after_frame));
    same_act_layout_source_preserves_events(frame, after_frame);
    assert(after_frame.cursor.at <= epm::cursor_event_count(frame));
    assert(after_frame.cursor.at <= epm::cursor_event_count(after_frame));
    same_act_layout_source_preserves_wf(frame, after_frame);
}

pub proof fn accepted_map_value_cursor_projects_body(
    frame: epm::EpmFrame,
    receipt: source::MapValueNextEventSource,
)
    requires
        source::selected_map_value_attachment_source(frame, receipt),
        accepted_act_event_count_fits_cursor(frame),
        !frame.cursor.done,
    ensures {
        let after_frame = map_value_cursor_after(frame, receipt);
        &&& epm::cursor_event_step(
            frame, after_frame, map_value_event_from_receipt(receipt),
        )
        &&& after_frame.cursor.at == receipt.event_index_after
        &&& after_frame.cursor.value_table_offset == receipt.body.end
        &&& after_frame.last_source == epm::trie_position(
            frame, receipt.act_source_offset,
        )
        &&& epm::map_value_body_at(
            frame.snapshot.bytes, frame.cursor.value_table_offset,
        ) == Some(receipt.body)
    },
{
    let after_frame = map_value_cursor_after(frame, receipt);
    accepted_map_value_wire_event(frame, receipt);
    accepted_map_value_post_layout(frame, receipt);
    assert(epm::cursor_event_step(
        frame, after_frame, map_value_event_from_receipt(receipt),
    ));
}

pub proof fn active_product_epm_has_admission_bytes(before: ProductScoreState)
    requires
        crate::product_score_wf(before),
        active_product_epm(before).is_some(),
    ensures {
        let frame = active_product_epm(before).unwrap();
        &&& frame.snapshot == before.indexed_regions.last()
        &&& frame.snapshot.bytes
            == before.bridge.admission.frames.last().source_bytes
    },
{
    let i = before.epm_frames.len() - 1;
    assert(before.epm_frames.len() == before.bridge.admission.frames.len());
    assert(before.indexed_regions.len() == before.epm_frames.len());
    assert(before.epm_frames[i] == Some(active_product_epm(before).unwrap()));
    assert(before.indexed_regions[i].bytes
        == before.bridge.admission.frames[i].source_bytes);
}

pub proof fn accepted_product_map_value_projects_body(
    before: ProductScoreState,
    plan_source: source::AcceptedActPlanSourceImage,
    receipt: source::MapValueNextEventSource,
)
    requires
        crate::product_score_wf(before),
        active_product_epm(before).is_some(),
        source::accepted_act_structure_projects_frame(
            active_product_epm(before).unwrap(), plan_source,
        ),
        source::selected_map_value_attachment_source(
            active_product_epm(before).unwrap(), receipt,
        ),
        !active_product_epm(before).unwrap().cursor.done,
    ensures {
        let frame = active_product_epm(before).unwrap();
        let after_frame = map_value_cursor_after(frame, receipt);
        let after = replace_active_epm(before, after_frame);
        &&& epm::cursor_event_step(
            frame, after_frame, map_value_event_from_receipt(receipt),
        )
        &&& after.epm_frames.last() == Some(after_frame)
        &&& after_frame.cursor.at == receipt.event_index_after
        &&& after_frame.cursor.value_table_offset == receipt.body.end
        &&& after_frame.last_source == epm::trie_position(
            frame, receipt.act_source_offset,
        )
        &&& after_frame.snapshot == before.indexed_regions.last()
        &&& frame.snapshot.bytes
            == before.bridge.admission.frames.last().source_bytes
        &&& epm::map_value_body_at(
            frame.snapshot.bytes, frame.cursor.value_table_offset,
        ) == Some(receipt.body)
        &&& before.bridge.admission.frames.last().source_bytes.subrange(
            receipt.body.start as int, receipt.body.end as int,
        ) == frame.snapshot.bytes.subrange(
            receipt.body.start as int, receipt.body.end as int,
        )
    },
{
    let frame = active_product_epm(before).unwrap();
    assert(epm::frame_layout_wf(frame));
    source::accepted_act_source_count_equals_expansion(frame, plan_source);
    accepted_source_count_bounds_formal_events(frame, plan_source);
    accepted_map_value_cursor_projects_body(frame, receipt);
    active_product_epm_has_admission_bytes(before);
}

/// The same source receipt supplies the concrete Map Value push action. The
/// only extra conditions are the existing path/reducer gate and fresh ID.
pub proof fn accepted_product_map_value_projects_push(
    before: ProductScoreState,
    plan_source: source::AcceptedActPlanSourceImage,
    receipt: source::MapValueNextEventSource,
    return_id: u64,
)
    requires
        crate::product_score_wf(before),
        active_product_epm(before).is_some(),
        source::accepted_act_structure_projects_frame(
            active_product_epm(before).unwrap(), plan_source,
        ),
        source::selected_map_value_attachment_source(
            active_product_epm(before).unwrap(), receipt,
        ),
        !active_product_epm(before).unwrap().cursor.done,
        epm::can_step(active_product_epm(before).unwrap()),
        epm::path_value_ok(active_product_epm(before).unwrap().path),
        return_id < u64::MAX,
    ensures {
        let frame = active_product_epm(before).unwrap();
        let after_frame = map_value_push_after(frame, receipt, return_id);
        let action = map_value_push_action(frame, receipt, return_id);
        let after = replace_active_epm(before, after_frame);
        &&& epm::event_step(
            frame, after_frame, map_value_event_from_receipt(receipt),
            return_id, action,
        )
        &&& after.epm_frames.last() == Some(after_frame)
        &&& after_frame.cursor.at == receipt.event_index_after
        &&& after_frame.cursor.value_table_offset == receipt.body.end
        &&& after_frame.last_source == receipt.act_source
        &&& action == epm::EpmAction::Push(epm::ChildRequest::ParValue {
            return_id,
            body: epm::indexed_subview(
                before.indexed_regions.last(), receipt.body,
            ),
        })
    },
{
    let frame = active_product_epm(before).unwrap();
    source::selected_map_value_projects_validated_record(frame, receipt, plan_source);
    accepted_product_map_value_projects_body(before, plan_source, receipt);
    let after_frame = map_value_push_after(frame, receipt, return_id);
    let action = map_value_push_action(frame, receipt, return_id);
    assert(epm::frame_layout_wf(after_frame));
    assert(epm::cursor_event_step(
        frame, after_frame, map_value_event_from_receipt(receipt),
    ));
    assert(epm::topology_offset_valid(frame, receipt.act_source_offset));
    assert(epm::topology_position(frame, receipt.act_source_offset)
        == receipt.act_source);
    assert(epm::value_return(
        frame, after_frame, true, Some(receipt.body),
        receipt.act_source, return_id, action,
    ));
}

}
