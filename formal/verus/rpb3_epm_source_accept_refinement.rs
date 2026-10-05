use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_epathmap_event_refinement.rs"]
pub mod epm;
#[cfg(not(rpb3_integrated))]
use epm::admission;

#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;
#[cfg(rpb3_integrated)]
use crate::rpb3_epathmap_event_refinement as epm;

verus! {

pub open spec fn epm_mode_code(mode: epm::EpmMode) -> u8 {
    match mode {
        epm::EpmMode::Empty => 0,
        epm::EpmMode::Set => 1,
        epm::EpmMode::Map => 2,
    }
}

pub open spec fn epm_source_end(
    before: epm::LinkedEpmState, finished: epm::LinkedEpmState,
) -> bool {
    epm::linked_epm_complete(
        before, finished,
        epm::EpmAction::Complete {
            mode: before.epm.mode,
            entries_stable: before.epm.entries_stable,
        },
    )
}

pub open spec fn epm_end_frame(before: epm::EpmFrame) -> epm::EpmFrame {
    epm::EpmFrame {
        path: before.path,
        cursor: epm::EpmCursorImage {
            act_plan: before.cursor.act_plan,
            validated_plan: before.cursor.validated_plan,
            at: before.cursor.at,
            declared_value_count: before.cursor.declared_value_count,
            map_values_seen: before.cursor.map_values_seen,
            value_table_offset: before.cursor.value_table_offset,
            done: true,
        },
        wait: before.wait,
        mode: before.mode,
        entries_stable: before.entries_stable,
        last_source: before.last_source,
        snapshot: before.snapshot,
        backing: before.backing,
        trie_start: before.trie_start,
        trie_end: before.trie_end,
        value_table_start: before.value_table_start,
        mode_offset: before.mode_offset,
    }
}

pub open spec fn epm_end_linked(before: epm::LinkedEpmState) -> epm::LinkedEpmState {
    epm::LinkedEpmState {
        epm: epm_end_frame(before.epm),
        admission: before.admission,
        indexed_regions: before.indexed_regions,
    }
}

pub open spec fn epm_source_finish_accept(
    before: epm::LinkedEpmState,
) -> Option<admission::AdmissionState> {
    epm_source_accept(before, epm_end_linked(before))
}

pub open spec fn epm_program31_parent(
    state: admission::AdmissionState,
) -> bool {
    if state.frames.len() < 2 { false } else {
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames[state.frames.len() - 1];
        &&& admission::admission_epathmap_snapshot_descriptor()
        &&& parent.frame_kind == 1
        &&& parent.family == admission::AdmissionFamily::Other
        &&& parent.program_type_id == 31
        &&& parent.slot_count == 4
        &&& parent.waiting_slot == Some(3)
        &&& parent.waiting_tag == Some(9)
        &&& parent.waiting_kind == 3
        &&& child.frame_kind == 3
        &&& child.score_handle.is_none()
    }
}

pub open spec fn epm_source_accept(
    before: epm::LinkedEpmState, finished: epm::LinkedEpmState,
) -> Option<admission::AdmissionState> {
    if !epm_source_end(before, finished)
        || !epm_program31_parent(before.admission)
        || !epm::linked_indexed_sources(before)
        || before.epm.snapshot != before.indexed_regions.last()
    {
        None
    } else {
        let child = before.admission.frames.last();
        if child.return_id.is_none()
            || child.source_bytes != before.epm.snapshot.bytes
        {
            None
        } else {
            match admission::admission_complete_child_step(
                before.admission, child.return_id.unwrap(), 3,
                admission::AdmissionReturnedKind::Other,
            ) {
                None => None,
                Some(base) => {
                    let parent = base.frames.last();
                    let prior_return = parent.returns.last();
                    let returned = admission::AdmissionReturnedSummary {
                        slot: prior_return.slot,
                        tag: prior_return.tag,
                        body_id: prior_return.body_id,
                        body_bytes: prior_return.body_bytes,
                        score_handle: prior_return.score_handle,
                        score_origin_body_id: prior_return.score_origin_body_id,
                        kind: admission::AdmissionReturnedKind::EPathMapSnapshot {
                            mode: epm_mode_code(before.epm.mode),
                            entries_canonical_path_stable: before.epm.entries_stable,
                        },
                    };
                    let accepted_parent = admission::AdmissionFrameState {
                        physical_source: parent.physical_source,
                        physical_start: parent.physical_start,
                        program_type_id: parent.program_type_id,
                        region_start: parent.region_start,
                        region_end: parent.region_end,
                        source_bytes: parent.source_bytes,
                        cursor: parent.cursor,
                        slot_count: parent.slot_count,
                        last_slot: parent.last_slot,
                        seen: parent.seen,
                        required: parent.required,
                        scalar_slots: parent.scalar_slots,
                        scalar_occurrences: parent.scalar_occurrences,
                        returns: parent.returns.drop_last().push(returned),
                        family: parent.family,
                        waiting_id: parent.waiting_id,
                        waiting_kind: parent.waiting_kind,
                        waiting_slot: parent.waiting_slot,
                        waiting_tag: parent.waiting_tag,
                        return_id: parent.return_id,
                        frame_kind: parent.frame_kind,
                        body_id: parent.body_id,
                        score_handle: parent.score_handle,
                        nested_started: parent.nested_started,
                        nested_resolved: parent.nested_resolved,
                    };
                    Some(admission::AdmissionState {
                        frames: base.frames.update(base.frames.len() - 1, accepted_parent),
                        arena: base.arena,
                        root_end: base.root_end,
                        pushed: base.pushed,
                        completed: base.completed,
                        next_obligation_id: base.next_obligation_id,
                    })
                },
            }
        }
    }
}

pub open spec fn epm_source_accepts_into_program31(
    before: epm::LinkedEpmState, finished: epm::LinkedEpmState,
    after: admission::AdmissionState,
) -> bool {
    epm_source_accept(before, finished) == Some(after)
}

pub proof fn epm_source_accept_carries_end_summary(
    before: epm::LinkedEpmState, finished: epm::LinkedEpmState,
    after: admission::AdmissionState,
)
    requires epm_source_accepts_into_program31(before, finished, after),
    ensures
        epm_source_end(before, finished),
        after.frames.len() + 1 == before.admission.frames.len(),
        after.completed as int == before.admission.completed as int + 1,
        after.frames.last().returns.last().body_bytes == before.epm.snapshot.bytes,
        match after.frames.last().returns.last().kind {
            admission::AdmissionReturnedKind::EPathMapSnapshot {
                mode, entries_canonical_path_stable,
            } => mode == epm_mode_code(before.epm.mode)
                && entries_canonical_path_stable == before.epm.entries_stable,
            _ => false,
        },
{}

pub proof fn epm_source_finish_accept_preserves_wf(
    before: epm::LinkedEpmState,
)
    requires epm_source_finish_accept(before).is_some(),
    ensures admission::admission_state_wf(epm_source_finish_accept(before).unwrap()),
{
    let finished = epm_end_linked(before);
    let after = epm_source_accept(before, finished).unwrap();
    let child = before.admission.frames.last();
    let base = admission::admission_complete_child_step(
        before.admission, child.return_id.unwrap(), 3,
        admission::AdmissionReturnedKind::Other,
    ).unwrap();
    admission::admission_complete_child_preserves_wf(
        before.admission, child.return_id.unwrap(), 3,
        admission::AdmissionReturnedKind::Other,
    );
    assert(admission::admission_state_wf(base));
    assert(after.frames.len() == base.frames.len());
    assert(after.arena == base.arena);
    let old_parent = base.frames.last();
    let new_parent = after.frames.last();
    assert(admission::admission_frame_wf(old_parent));
    assert(new_parent.family == admission::AdmissionFamily::Other);
    assert(new_parent.returns.len() == old_parent.returns.len());
    assert(new_parent.returns.len() > 0);
    assert(new_parent.physical_start == old_parent.physical_start);
    assert(new_parent.program_type_id == old_parent.program_type_id);
    assert(new_parent.region_start == old_parent.region_start);
    assert(new_parent.region_end == old_parent.region_end);
    assert(new_parent.source_bytes == old_parent.source_bytes);
    assert(new_parent.cursor == old_parent.cursor);
    assert(new_parent.slot_count == old_parent.slot_count);
    assert(new_parent.last_slot == old_parent.last_slot);
    assert(new_parent.seen == old_parent.seen);
    assert(new_parent.required == old_parent.required);
    assert(new_parent.scalar_slots == old_parent.scalar_slots);
    assert(new_parent.scalar_occurrences == old_parent.scalar_occurrences);
    assert(new_parent.family == old_parent.family);
    assert(new_parent.waiting_id == old_parent.waiting_id);
    assert(new_parent.waiting_kind == old_parent.waiting_kind);
    assert(new_parent.waiting_slot == old_parent.waiting_slot);
    assert(new_parent.waiting_tag == old_parent.waiting_tag);
    assert(new_parent.return_id == old_parent.return_id);
    assert(new_parent.frame_kind == old_parent.frame_kind);
    assert(new_parent.body_id == old_parent.body_id);
    assert(new_parent.score_handle == old_parent.score_handle);
    assert(new_parent.nested_started == old_parent.nested_started);
    assert(new_parent.nested_resolved == old_parent.nested_resolved);
    assert forall|i: int| 0 <= i < new_parent.returns.len() implies {
        &&& new_parent.returns[i].slot == old_parent.returns[i].slot
        &&& new_parent.returns[i].score_handle == old_parent.returns[i].score_handle
        &&& new_parent.returns[i].score_origin_body_id
            == old_parent.returns[i].score_origin_body_id
    } by {
        if i < new_parent.returns.len() - 1 {
            assert(new_parent.returns[i] == old_parent.returns[i]);
        } else {
            assert(i == new_parent.returns.len() - 1);
        }
    }
    assert forall|i: int| 0 <= i < new_parent.returns.len() implies {
        &&& new_parent.returns[i].slot < new_parent.slot_count
        &&& new_parent.seen.contains(new_parent.returns[i].slot as int)
        &&& admission::admission_return_matches(
            new_parent.family, new_parent.returns[i].slot, new_parent.returns[i].kind,
        )
    } by {
        assert(old_parent.returns[i].slot < old_parent.slot_count);
        assert(old_parent.seen.contains(old_parent.returns[i].slot as int));
        assert(admission::admission_return_matches(
            new_parent.family, new_parent.returns[i].slot, new_parent.returns[i].kind,
        ));
    }
    assert(admission::admission_required_slots_generated(new_parent)
        == admission::admission_required_slots_generated(old_parent));
    assert(admission::admission_required_slots_generated(new_parent));
    assert(admission::admission_frame_wf(new_parent));
    assert(after.root_end == base.root_end);
    assert(after.pushed == base.pushed);
    assert(after.completed == base.completed);
    assert(after.next_obligation_id == base.next_obligation_id);
    let last = after.frames.len() - 1;
    assert forall|index: int| 0 <= index < after.frames.len()
        implies admission::admission_frame_wf(after.frames[index]) by {
        if index < last {
            assert(after.frames[index] == base.frames[index]);
        } else {
            assert(index == last);
            assert(after.frames[index] == new_parent);
        }
    }
    assert forall|index: int, i: int|
        0 <= index < after.frames.len()
            && 0 <= i < after.frames[index].returns.len()
        implies after.frames[index].returns[i].score_handle.is_some()
            == after.frames[index].returns[i].score_origin_body_id.is_some()
            && match after.frames[index].returns[i].score_handle {
                Some(handle) => (handle as int) < after.arena.len()
                    && after.arena[handle as int].body_id
                        == after.frames[index].returns[i].score_origin_body_id.unwrap(),
                None => true,
            } by {
        if index < last {
            assert(after.frames[index] == base.frames[index]);
        } else {
            assert(index == last);
            assert(new_parent.returns[i].score_handle == old_parent.returns[i].score_handle);
            assert(new_parent.returns[i].score_origin_body_id
                == old_parent.returns[i].score_origin_body_id);
        }
    }
    assert forall|index: int| 1 <= index < after.frames.len()
        implies if after.frames[index - 1].frame_kind == 1
            || after.frames[index - 1].frame_kind == 2 {
            #[trigger] admission::admission_child_physical_contained(
                after.frames[index - 1], after.frames[index],
            )
        } else { true } by {
        if index < last {
            assert(after.frames[index - 1] == base.frames[index - 1]);
            assert(after.frames[index] == base.frames[index]);
        } else {
            assert(index == last);
            assert(after.frames[index - 1] == base.frames[index - 1]);
            assert(base.frames[index] == old_parent);
            if after.frames[index - 1].frame_kind == 1
                || after.frames[index - 1].frame_kind == 2 {
                assert(base.frames[index - 1].frame_kind == 1
                    || base.frames[index - 1].frame_kind == 2);
                assert(admission::admission_child_physical_contained(
                    base.frames[index - 1], base.frames[index],
                ));
                assert(admission::admission_child_physical_contained(
                    base.frames[index - 1], old_parent,
                ));
            }
        }
    }
    assert(admission::admission_state_wf(after));
}

}

fn main() {}
