use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_score_transcript;
mod rpb3_score_admission_bridge;
pub mod rpb3_path_parser_refinement;
mod rpb3_epathmap_event_refinement;
mod rpb3_epm_source_accept_refinement;
mod rpb3_epm_nested_source_refinement;

use {
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_nested_source_refinement as nested,
    rpb3_epm_source_accept_refinement as source_accept, rpb3_score_admission_bridge as bridge,
    rpb3_score_transcript as score,
};

verus! {

pub open spec fn program31_connective_value(
    frame: admission::AdmissionFrameState,
) -> Option<bool> {
    if frame.program_type_id != 31 || frame.frame_kind != 1 {
        None
    } else if !frame.seen.contains(1) {
        if admission::admission_scalar_slot_absent(frame, 1) { Some(false) }
        else { None }
    } else {
        match admission::admission_unique_scalar_occurrence(frame, 1, 4, 1) {
            Some(occurrence) if occurrence.varint_value == 1 => Some(true),
            _ => None,
        }
    }
}

pub open spec fn program31_score_replay(
    before: bridge::ScoreAdmissionBridge,
    epm_before: epm::LinkedEpmState,
) -> Option<(bridge::ScoreAdmissionBridge, usize)> {
    if !bridge::bridge_well_formed(before)
        || source_accept::epm_source_finish_accept(epm_before) != Some(before.admission)
        || before.admission.frames.len() == 0
    {
        None
    } else {
        let top = before.admission.frames.last();
        if top.program_type_id != 31 || top.frame_kind != 1
            || top.score_handle.is_some()
            || admission::admission_epathmap_path_stability(top).is_none()
        {
            None
        } else {
            match program31_connective_value(top) {
                None => None,
                Some(connective_used) => {
                    let snapshot = top.returns.last().body_bytes;
                    match bridge::bridge_append_unattached(
                        before, score::ScoreNode::BytesAtom(snapshot),
                    ) {
                        None => None,
                        Some((with_snapshot, snapshot_handle)) => {
                            match bridge::bridge_append_unattached(
                                with_snapshot,
                                score::ScoreNode::Integer(
                                    if connective_used { 1 } else { 0 },
                                ),
                            ) {
                                None => None,
                                Some((with_flag, flag_handle)) => {
                                    match bridge::bridge_constructor_step(
                                        with_flag, 13,
                                        seq![snapshot_handle, flag_handle],
                                    ) {
                                        None => None,
                                        Some((after, _, score_handle)) =>
                                            Some((after, score_handle)),
                                    }
                                },
                            }
                        },
                    }
                },
            }
        }
    }
}

pub proof fn program31_score_replay_exact(
    before: bridge::ScoreAdmissionBridge,
    epm_before: epm::LinkedEpmState,
)
    ensures match program31_score_replay(before, epm_before) {
        Some((after, score_handle)) => {
            let top = before.admission.frames.last();
            let snapshot = top.returns.last().body_bytes;
            let connective_used = program31_connective_value(top).unwrap();
            &&& bridge::bridge_well_formed(after)
            &&& after.admission.frames.last().score_handle == Some(score_handle)
            &&& after.admission.arena[score_handle as int].body_id == top.body_id
            &&& after.admission.arena[score_handle as int].transcript_bytes
                == nested::program31_score_bytes(snapshot, connective_used)
            &&& score::handle_transcript(after.transcript, score_handle)
                == Some(nested::program31_score_bytes(snapshot, connective_used))
        },
        None => true,
    },
{
    if let Some((after, score_handle)) = program31_score_replay(before, epm_before) {
        source_accept::epm_source_finish_accept_preserves_wf(epm_before);
        assert(admission::admission_state_wf(before.admission));
        let top = before.admission.frames.last();
        let snapshot = top.returns.last().body_bytes;
        let connective_used = program31_connective_value(top).unwrap();
        let (with_snapshot, snapshot_handle) = bridge::bridge_append_unattached(
            before, score::ScoreNode::BytesAtom(snapshot),
        ).unwrap();
        bridge::bridge_append_unattached_exact(
            before, score::ScoreNode::BytesAtom(snapshot),
        );
        bridge::bridge_append_unattached_preserves_admission_wf(
            before, score::ScoreNode::BytesAtom(snapshot),
        );
        assert(score::append_bytes(before.transcript, snapshot)
            == Some((with_snapshot.transcript, snapshot_handle)));
        let flag_kind = score::ScoreNode::Integer(if connective_used { 1 } else { 0 });
        let (with_flag, flag_handle) = bridge::bridge_append_unattached(
            with_snapshot, flag_kind,
        ).unwrap();
        bridge::bridge_append_unattached_exact(with_snapshot, flag_kind);
        bridge::bridge_append_unattached_preserves_admission_wf(with_snapshot, flag_kind);
        assert(score::append_integer(
            with_snapshot.transcript, if connective_used { 1 } else { 0 },
        ) == Some((with_flag.transcript, flag_handle)));
        let children = seq![snapshot_handle, flag_handle];
        bridge::bridge_constructor_exact(with_flag, 13, children);
        let (with_constructor, constructor_handle) = bridge::bridge_append_unattached(
            with_flag, score::ScoreNode::Integer(13),
        ).unwrap();
        assert(score::append_integer(with_flag.transcript, 13)
            == Some((with_constructor.transcript, constructor_handle)));
        let all_children = seq![constructor_handle] + children;
        let (with_node, node_handle) = bridge::bridge_append_unattached(
            with_constructor, score::ScoreNode::Node(all_children),
        ).unwrap();
        assert(score::append_node(with_constructor.transcript, all_children)
            == Some((with_node.transcript, node_handle)));
        assert(all_children =~= seq![constructor_handle, snapshot_handle, flag_handle]);
        assert(after.transcript == with_node.transcript);
        assert(score_handle == node_handle);
        assert(nested::program31_score_append(
            before.transcript, snapshot, connective_used,
        ) == Some((after.transcript, score_handle)));
        nested::program31_score_append_has_source_transcript(
            before.transcript, snapshot, connective_used,
        );
        assert(after.transcript.entries[score_handle as int].transcript
            == nested::program31_score_bytes(snapshot, connective_used));
        assert(after.admission.arena[score_handle as int].transcript_bytes
            == after.transcript.entries[score_handle as int].transcript);
    }
}

}

fn main() {}
