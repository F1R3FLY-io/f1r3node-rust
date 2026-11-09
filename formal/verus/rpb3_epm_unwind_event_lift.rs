//! A source-shaped accepted Unwind run expands to full scalar EPM Leave events.
//! Rust-to-model projection of the batch guard and linked source remains open.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_push_transport_source_refinement as transport,
    rpb3_epm_restore_run_expansion as restore,
};

verus! {

pub open spec fn unwind_event(
    start: usize, width: usize, step: int,
) -> epm::EpmEvent {
    transport::epm_push_chunk_event(
        transport::EpmPushTransportChunk::Unwind { start, width }, step,
    )
}

/// The virtual frames change only path history and the ACT event index.
/// ACT decoding, physical backing, and the protobuf value-table cursor are
/// identical to the accepted batch's pre-state.
pub proof fn unwind_virtual_frame_has_layout(
    before: epm::EpmFrame,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
)
    requires
        batch::unwind_batch_source_guard(before, run, prefix, start, width, facts),
        0 <= step <= width as int,
    ensures {
        let view = batch::unwind_virtual_frame(before, run, prefix, step as usize);
        &&& epm::frame_layout_wf(view)
        &&& view.cursor.at as int == before.cursor.at as int + step
        &&& epm::cursor_event_count(view) == epm::cursor_event_count(before)
        &&& view.cursor.act_plan == before.cursor.act_plan
        &&& view.cursor.validated_plan == before.cursor.validated_plan
        &&& view.cursor.map_values_seen == before.cursor.map_values_seen
        &&& view.cursor.value_table_offset == before.cursor.value_table_offset
        &&& epm::same_frame_source(before, view)
    },
{
    let view = batch::unwind_virtual_frame(before, run, prefix, step as usize);
    if step == 0 {
        assert(view == before);
    } else {
        assert(batch::historical_run_wf(run));
        assert(view.path.path_bytes.len() == view.path.path_positions.len());
        assert(epm::same_frame_source(before, view));
        assert(view.mode == before.mode);
        assert(view.cursor.act_plan == before.cursor.act_plan);
        assert(view.cursor.validated_plan == before.cursor.validated_plan);
        assert(view.cursor.declared_value_count == before.cursor.declared_value_count);
        assert(view.cursor.value_table_offset == before.cursor.value_table_offset);
        assert(view.cursor.map_values_seen == before.cursor.map_values_seen);
        assert(view.cursor.at as int == before.cursor.at as int + step);
        match before.cursor.act_plan {
            Some(plan) => {
                assert(epm::act_plan_matches_frame(view, plan)
                    == epm::act_plan_matches_frame(before, plan));
                assert(epm::act_plan_events(view, plan)
                    == epm::act_plan_events(before, plan));
            },
            None => {},
        }
        assert(epm::cursor_event_count(view) == epm::cursor_event_count(before));
        assert(view.cursor.at <= epm::cursor_event_count(view));
        assert(epm::frame_layout_wf(view));
    }
}

pub proof fn accepted_unwind_one_full_epm_event(
    before: epm::EpmFrame,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
    admission_next_id: u64,
)
    requires
        batch::unwind_batch_source_guard(before, run, prefix, start, width, facts),
        0 <= step < width as int,
    ensures {
        let from = batch::unwind_virtual_frame(before, run, prefix, step as usize);
        let to = batch::unwind_virtual_frame(before, run, prefix, (step + 1) as usize);
        epm::event_step(
            from, to, unwind_event(start, width, step),
            admission_next_id, epm::EpmAction::Continue,
        )
    },
{
    unwind_virtual_frame_has_layout(before, run, prefix, start, width, facts, step);
    unwind_virtual_frame_has_layout(before, run, prefix, start, width, facts, step + 1);
    batch::accepted_unwind_batch_one_scalar_restore_step(
        before, run, prefix, start, width, facts, step,
    );
    let from = batch::unwind_virtual_frame(before, run, prefix, step as usize);
    let to = batch::unwind_virtual_frame(before, run, prefix, (step + 1) as usize);
    let offset = (start as int + width as int - 1 - step) as usize;
    let chunk = transport::EpmPushTransportChunk::Unwind { start, width };
    assert(before.trie_end > before.trie_start);
    assert(before.mode != epm::EpmMode::Empty);
    assert(unwind_event(start, width, step)
        == epm::EpmEvent::Leave { source_offset: offset });
    assert(epm::topology_offset_valid(from, offset));
    assert(epm::topology_position(from, offset)
        == epm::trie_position(before, offset));
    assert(run.current.positions[run.current.positions.len() - 1 - step]
        == epm::trie_position(before, offset));
    assert(epm::leave_return(from, to, epm::topology_position(from, offset)));
    assert(transport::epm_push_chunk_events(chunk)[step]
        == unwind_event(start, width, step));
    assert(epm::cursor_event_at(
        before, (before.cursor.at as int + step) as usize,
    ) == epm::wire_event(unwind_event(start, width, step)));
    assert(epm::cursor_event_at(from, from.cursor.at)
        == epm::cursor_event_at(
            before, (before.cursor.at as int + step) as usize,
        ));
    assert(epm::cursor_event_step(from, to, unwind_event(start, width, step)));
}

pub proof fn accepted_unwind_full_epm_event_sequence(
    before: epm::EpmFrame,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    admission_next_id: u64,
)
    requires batch::unwind_batch_source_guard(before, run, prefix, start, width, facts),
    ensures
        forall|step: int| 0 <= step < width as int ==>
            #[trigger] epm::event_step(
                batch::unwind_virtual_frame(before, run, prefix, step as usize),
                batch::unwind_virtual_frame(before, run, prefix, (step + 1) as usize),
                unwind_event(start, width, step),
                admission_next_id, epm::EpmAction::Continue,
            ),
{
    assert forall|step: int| 0 <= step < width as int implies
        #[trigger] epm::event_step(
            batch::unwind_virtual_frame(before, run, prefix, step as usize),
            batch::unwind_virtual_frame(before, run, prefix, (step + 1) as usize),
            unwind_event(start, width, step),
            admission_next_id, epm::EpmAction::Continue,
        ) by {
        accepted_unwind_one_full_epm_event(
            before, run, prefix, start, width, facts, step, admission_next_id,
        );
    }
}

/// These are the ordinary source-state preconditions of linked EPM execution.
/// The post-state admission cursor and indexed-source invariants are proved
/// below, not supplied as a linked-transition premise.
pub open spec fn linked_unwind_source_guard(
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    &&& batch::unwind_batch_source_guard(
        source.epm, run, prefix, start, width, facts,
    )
    &&& admission::admission_state_wf(source.admission)
    &&& epm::linked_indexed_sources(source)
    &&& source.admission.frames.len() > 0
    &&& source.admission.frames.last().frame_kind == 3
    &&& source.admission.frames.last().waiting_id.is_none()
    &&& source.epm.snapshot == source.indexed_regions.last()
    &&& source.admission.frames.last().cursor
        <= source.epm.cursor.value_table_offset
}

pub open spec fn linked_unwind_virtual_admission(
    source: epm::LinkedEpmState, step: usize,
) -> admission::AdmissionState
    recommends source.admission.frames.len() > 0,
{
    if step == 0 {
        source.admission
    } else {
        admission::AdmissionState {
            frames: source.admission.frames.update(
                source.admission.frames.len() - 1,
                epm::epm_advanced_admission_cursor(
                    source.admission.frames.last(),
                    source.epm.cursor.value_table_offset,
                ),
            ),
            arena: source.admission.arena,
            root_end: source.admission.root_end,
            pushed: source.admission.pushed,
            completed: source.admission.completed,
            next_obligation_id: source.admission.next_obligation_id,
        }
    }
}

pub open spec fn linked_unwind_virtual_state(
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    step: usize,
) -> epm::LinkedEpmState
    recommends source.admission.frames.len() > 0,
        batch::historical_run_wf(run), step as int <= batch::historical_run_width(run),
        source.epm.cursor.at as int + step as int <= usize::MAX as int,
{
    epm::LinkedEpmState {
        epm: batch::unwind_virtual_frame(source.epm, run, prefix, step),
        admission: linked_unwind_virtual_admission(source, step),
        indexed_regions: source.indexed_regions,
    }
}

pub proof fn linked_unwind_virtual_state_is_source_wf(
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
)
    requires
        linked_unwind_source_guard(source, run, prefix, start, width, facts),
        0 <= step <= width as int,
    ensures {
        let view = linked_unwind_virtual_state(source, run, prefix, step as usize);
        &&& admission::admission_state_wf(view.admission)
        &&& epm::linked_indexed_sources(view)
        &&& view.admission.frames.len() > 0
        &&& view.admission.frames.last().frame_kind == 3
        &&& view.admission.frames.last().waiting_id.is_none()
        &&& view.epm.snapshot == view.indexed_regions.last()
        &&& view.admission.frames.last().cursor
            <= view.epm.cursor.value_table_offset
        &&& view.admission.next_obligation_id == source.admission.next_obligation_id
    },
{
    unwind_virtual_frame_has_layout(
        source.epm, run, prefix, start, width, facts, step,
    );
    let view = linked_unwind_virtual_state(source, run, prefix, step as usize);
    if step == 0 {
        assert(view == source);
    } else {
        let last = source.admission.frames.len() - 1;
        let parent = source.admission.frames.last();
        let consumed = source.epm.cursor.value_table_offset;
        assert(source.epm.snapshot.bytes.len() == parent.source_bytes.len());
        assert(parent.source_bytes.len() == parent.region_end as int);
        assert(consumed <= parent.region_end);
        assert(admission::admission_frame_wf(
            epm::epm_advanced_admission_cursor(parent, consumed),
        ));
        assert(view.admission.frames.len() == source.admission.frames.len());
        assert(view.admission.frames[last] == epm::epm_advanced_admission_cursor(
            parent, consumed,
        ));
        assert forall|i: int| 0 <= i < source.admission.frames.len() - 1
            implies view.admission.frames[i] == source.admission.frames[i] by {
        }
        admission::admission_last_frame_replacement_preserves_wf(
            source.admission, view.admission,
        );
        assert forall|i: int| 0 <= i < view.admission.frames.len()
            implies epm::indexed_image_wf(view.indexed_regions[i])
                && view.indexed_regions[i].bytes == view.admission.frames[i].source_bytes
                && epm::indexed_position_at(view.indexed_regions[i], 0).source
                    == view.admission.frames[i].physical_source
                && epm::indexed_position_at(view.indexed_regions[i], 0).offset
                    == view.admission.frames[i].physical_start by {
            if i != last {
                assert(view.admission.frames[i] == source.admission.frames[i]);
            }
        }
        assert(epm::linked_indexed_sources(view));
    }
}

pub proof fn linked_unwind_admission_update_is_idempotent(
    source: epm::LinkedEpmState,
    step: usize,
)
    requires source.admission.frames.len() > 0, step < usize::MAX,
    ensures {
        let from = linked_unwind_virtual_admission(source, step);
        let to = linked_unwind_virtual_admission(source, (step + 1) as usize);
        to == admission::AdmissionState {
            frames: from.frames.update(
                from.frames.len() - 1,
                epm::epm_advanced_admission_cursor(
                    from.frames.last(), source.epm.cursor.value_table_offset,
                ),
            ),
            arena: from.arena,
            root_end: from.root_end,
            pushed: from.pushed,
            completed: from.completed,
            next_obligation_id: from.next_obligation_id,
        }
    },
{
    let from = linked_unwind_virtual_admission(source, step);
    let to = linked_unwind_virtual_admission(source, (step + 1) as usize);
    let last = source.admission.frames.len() - 1;
    let consumed = source.epm.cursor.value_table_offset;
    assert(from.frames.len() == source.admission.frames.len());
    assert(from.frames.len() > 0);
    if step == 0 {
        assert(from == source.admission);
    } else {
        let advanced = epm::epm_advanced_admission_cursor(
            source.admission.frames.last(), consumed,
        );
        let updated = source.admission.frames.update(last, advanced);
        assert(from.frames == updated);
        assert(to.frames == updated);
        assert(updated.len() == source.admission.frames.len());
        assert(updated[last] == advanced);
        assert(updated.last() == advanced);
        assert(epm::epm_advanced_admission_cursor(updated.last(), consumed) == advanced);
        assert forall|i: int| 0 <= i < updated.len() implies
            updated.update(last, advanced)[i] == updated[i] by {
            if i == last {
                assert(updated[i] == advanced);
            }
        }
        assert(updated.update(last, advanced) == updated);
    }
}

pub proof fn accepted_unwind_one_linked_continue(
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
)
    requires
        linked_unwind_source_guard(source, run, prefix, start, width, facts),
        0 <= step < width as int,
    ensures epm::linked_epm_continue(
        linked_unwind_virtual_state(source, run, prefix, step as usize),
        linked_unwind_virtual_state(source, run, prefix, (step + 1) as usize),
        unwind_event(start, width, step),
    ),
{
    linked_unwind_virtual_state_is_source_wf(
        source, run, prefix, start, width, facts, step,
    );
    linked_unwind_virtual_state_is_source_wf(
        source, run, prefix, start, width, facts, step + 1,
    );
    accepted_unwind_one_full_epm_event(
        source.epm, run, prefix, start, width, facts, step,
        source.admission.next_obligation_id,
    );
    let from = linked_unwind_virtual_state(source, run, prefix, step as usize);
    let to = linked_unwind_virtual_state(source, run, prefix, (step + 1) as usize);
    let consumed = source.epm.cursor.value_table_offset;
    linked_unwind_admission_update_is_idempotent(source, step as usize);
    assert(from.admission.frames.len() > 0);
    assert(from.epm.cursor.value_table_offset == consumed);
    assert(to.epm.cursor.value_table_offset == consumed);
    assert(to.admission == admission::AdmissionState {
        frames: from.admission.frames.update(
            from.admission.frames.len() - 1,
            epm::epm_advanced_admission_cursor(
                from.admission.frames.last(), to.epm.cursor.value_table_offset,
            ),
        ),
        arena: from.admission.arena,
        root_end: from.admission.root_end,
        pushed: from.admission.pushed,
        completed: from.admission.completed,
        next_obligation_id: from.admission.next_obligation_id,
    });
}

pub proof fn accepted_unwind_linked_continue_sequence(
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires linked_unwind_source_guard(source, run, prefix, start, width, facts),
    ensures
        linked_unwind_virtual_state(source, run, prefix, 0) == source,
        forall|step: int| 0 <= step < width as int ==>
            #[trigger] epm::linked_epm_continue(
                linked_unwind_virtual_state(source, run, prefix, step as usize),
                linked_unwind_virtual_state(source, run, prefix, (step + 1) as usize),
                unwind_event(start, width, step),
            ),
{
    assert forall|step: int| 0 <= step < width as int implies
        #[trigger] epm::linked_epm_continue(
            linked_unwind_virtual_state(source, run, prefix, step as usize),
            linked_unwind_virtual_state(source, run, prefix, (step + 1) as usize),
            unwind_event(start, width, step),
        ) by {
        accepted_unwind_one_linked_continue(
            source, run, prefix, start, width, facts, step,
        );
    }
}

}
