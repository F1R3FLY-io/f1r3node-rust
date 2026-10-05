use vstd::prelude::*;

#[cfg(rpb3_finish_standalone)]
#[path = "rpb3_admission_cursor.rs"]
mod rpb3_admission_cursor;
#[cfg(rpb3_finish_standalone)]
#[path = "rpb3_score_transcript.rs"]
mod rpb3_score_transcript;
#[cfg(rpb3_finish_standalone)]
#[path = "rpb3_score_admission_bridge.rs"]
mod rpb3_score_admission_bridge;
use {
    rpb3_admission_cursor as admission, rpb3_score_admission_bridge as bridge,
    rpb3_score_transcript as score,
};

#[cfg(not(rpb3_finish_standalone))]
use crate::rpb3_admission_cursor;
#[cfg(not(rpb3_finish_standalone))]
use crate::rpb3_score_admission_bridge;
#[cfg(not(rpb3_finish_standalone))]
use crate::rpb3_score_transcript;

verus! {

pub struct RootComponentScoreImage {
    pub family_tag: u8,
    pub score_handle: usize,
    pub complete: bool,
}

pub struct RootCompleteActionImage {
    pub summary: RootComponentScoreImage,
    pub ghost pre_pop: bridge::ScoreAdmissionBridge,
}

pub struct RootDriverReturnImage {
    pub component: RootComponentScoreImage,
    pub ghost pre_pop: bridge::ScoreAdmissionBridge,
    pub ghost score_bytes: Seq<u8>,
    pub remaining_frames: usize,
}

pub open spec fn component_tag_for_family(family: admission::AdmissionFamily) -> Option<u8> {
    match family {
        admission::AdmissionFamily::Send => Some(1),
        admission::AdmissionFamily::Receive => Some(2),
        admission::AdmissionFamily::New => Some(4),
        admission::AdmissionFamily::Expr => Some(5),
        admission::AdmissionFamily::Match => Some(6),
        admission::AdmissionFamily::GUnforgeable => Some(7),
        admission::AdmissionFamily::Connective => Some(8),
        admission::AdmissionFamily::Bundle => Some(11),
        admission::AdmissionFamily::If => Some(12),
        admission::AdmissionFamily::CostTokenStack => Some(13),
        _ => None,
    }
}

pub open spec fn message_eof_ready(before: bridge::ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() == 1
        && before.admission.frames[0].frame_kind == 1
        && before.admission.frames[0].cursor == before.admission.root_end
        && before.admission.frames[0].region_end == before.admission.root_end
        && before.admission.frames[0].waiting_id.is_none()
        && before.admission.frames[0].score_handle.is_none()
}

pub open spec fn empty_send_finish_root() -> admission::AdmissionFrameState {
    let example = admission::admission_incomplete_root_example().frames[0];
    admission::AdmissionFrameState {
        body_id: 0,
        score_handle: None,
        ..example
    }
}

pub open spec fn empty_send_finish_before() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(empty_send_finish_root())
}

pub open spec fn empty_send_finish_after() -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_nonsemantic_step(empty_send_finish_before()) {
        None => None,
        Some((after, _, _)) => Some(after),
    }
}

pub open spec fn modeled_message_finish_action(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> Option<RootCompleteActionImage> {
    if !message_eof_ready(before)
        || !bridge::bridge_reducer_reachable(before)
        || !bridge::bridge_score_source_step(before, after)
        || after.admission.frames.len() != 1
    {
        None
    } else {
        match (
            component_tag_for_family(after.admission.frames[0].family),
            admission::admission_family_program_id(after.admission.frames[0].family),
            bridge::bridge_root_handle(after),
            bridge::bridge_root_transcript(after),
        ) {
            (Some(family_tag), Some(program_type_id), Some(score_handle), Some(_))
                if program_type_id == after.admission.frames[0].program_type_id
                    && (score_handle as int) < after.admission.arena.len() =>
            {
                Some(RootCompleteActionImage {
                    summary: RootComponentScoreImage {
                        family_tag,
                        score_handle,
                        complete: admission::admission_family_complete(after.admission.frames[0]),
                    },
                    pre_pop: after,
                })
            },
            _ => None,
        }
    }
}

pub open spec fn modeled_driver_root_pop(
    action: RootCompleteActionImage,
) -> Option<RootDriverReturnImage> {
    if action.pre_pop.admission.frames.len() != 1 {
        None
    } else {
        match bridge::bridge_root_transcript(action.pre_pop) {
            None => None,
            Some(score_bytes) => Some(RootDriverReturnImage {
                component: action.summary,
                pre_pop: action.pre_pop,
                score_bytes,
                remaining_frames: 0,
            }),
        }
    }
}

pub open spec fn modeled_message_eof_to_root_return(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> Option<RootDriverReturnImage> {
    match modeled_message_finish_action(before, after) {
        None => None,
        Some(action) => modeled_driver_root_pop(action),
    }
}

pub proof fn modeled_finish_preserves_score_shape(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires modeled_message_finish_action(before, after).is_some(),
    ensures
        bridge::bridge_score_step_shape(before, after),
        bridge::bridge_well_formed(after),
        after.admission.frames.len() == 1,
        after.admission.frames[0].source_bytes == before.admission.frames[0].source_bytes,
        after.admission.frames[0].program_type_id
            == before.admission.frames[0].program_type_id,
        after.admission.arena.take(before.admission.arena.len() as int)
            == before.admission.arena,
        after.transcript.entries.take(before.transcript.entries.len() as int)
            == before.transcript.entries,
{
    bridge::bridge_reachable_has_exact_binding(before);
    bridge::bridge_score_source_step_preserves_exact(before, after);
    bridge::bridge_score_source_step_preserves_shape(before, after);
}

pub proof fn modeled_root_return_matches_pre_pop(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    ensures match modeled_message_eof_to_root_return(before, after) {
        None => true,
        Some(returned) => {
            &&& returned.remaining_frames == 0
            &&& returned.pre_pop.admission == after.admission
            &&& returned.pre_pop.transcript == after.transcript
            &&& after.admission.frames.len() == 1
            &&& after.admission.frames[0].score_handle
                == Some(returned.component.score_handle)
            &&& bridge::bridge_root_handle(after)
                == Some(returned.component.score_handle)
            &&& bridge::bridge_root_transcript(after) == Some(returned.score_bytes)
            &&& returned.component.complete
                == admission::admission_family_complete(after.admission.frames[0])
            &&& returned.component.complete
                == after.admission.arena[returned.component.score_handle as int].semantic
            &&& (returned.component.score_handle as int) < after.admission.arena.len()
            &&& bridge::bridge_well_formed(after)
            &&& returned.score_bytes
                == after.transcript.entries[returned.component.score_handle as int].transcript
            &&& returned.score_bytes
                == after.admission.arena[returned.component.score_handle as int].transcript_bytes
        },
    },
{
    if let Some(returned) = modeled_message_eof_to_root_return(before, after) {
        assert(modeled_message_finish_action(before, after).is_some());
        assert(bridge::bridge_reducer_reachable(before));
        assert(bridge::bridge_score_source_step(before, after));
        bridge::bridge_reachable_has_exact_binding(before);
        bridge::bridge_score_source_step_preserves_exact(before, after);
        assert(bridge::bridge_root_handle(after)
            == Some(returned.component.score_handle));
        assert(admission::admission_complete_root(after.admission));
        bridge::bridge_root_exact(after);
    }
}

pub proof fn empty_send_finish_has_root_return()
    ensures match empty_send_finish_after() {
        None => false,
        Some(after) => match modeled_message_eof_to_root_return(
            empty_send_finish_before(), after,
        ) {
            None => false,
            Some(returned) => {
                &&& returned.component.family_tag == 1
                &&& !returned.component.complete
                &&& returned.component.score_handle == 1
                &&& returned.remaining_frames == 0
                &&& returned.pre_pop.transcript == after.transcript
            },
        },
    },
{
    let root = empty_send_finish_root();
    let before = empty_send_finish_before();
    assert(admission::admission_root_initial(root));
    bridge::bridge_initial_reachable(root);
    bridge::bridge_initial_exact(root);
    admission::admission_initial_wf(root);
    assert(!admission::admission_family_complete(root));
    let constructor = score::ScoreNode::Integer(i32::MAX as i64);
    assert(score::integer_transcript(i32::MAX as i64).len() == 10);
    assert(score::checked_append(before.transcript, constructor).is_some());
    assert(bridge::bridge_append_unattached(before, constructor).is_some());
    bridge::bridge_append_unattached_exact(before, constructor);
    let (after_constructor, constructor_handle) =
        bridge::bridge_append_unattached(before, constructor).unwrap();
    let node = score::ScoreNode::Node(seq![constructor_handle]);
    assert(constructor_handle == 0);
    assert(after_constructor.transcript.entries.len() == 1);
    assert(score::arena_well_formed(after_constructor.transcript));
    assert(score::child_handles_valid(after_constructor.transcript.entries,
        seq![constructor_handle]));
    assert(after_constructor.transcript.entries[0].transcript.len() == 10);
    let children = seq![constructor_handle];
    assert(children.len() == 1);
    assert(children.drop_first() == Seq::<usize>::empty());
    assert(score::children_transcript(
        after_constructor.transcript.entries, children.drop_first(),
    ) == Seq::<u8>::empty());
    assert(score::children_transcript(after_constructor.transcript.entries, children)
        == after_constructor.transcript.entries[0].transcript);
    assert(score::kind_transcript(after_constructor.transcript.entries, node).len() == 12);
    assert(score::checked_append(after_constructor.transcript, node).is_some());
    assert(bridge::bridge_append_unattached(after_constructor, node).is_some());
    bridge::bridge_append_unattached_exact(after_constructor, node);
    let (after_node, score_handle) =
        bridge::bridge_append_unattached(after_constructor, node).unwrap();
    assert(admission::admission_attach_score_step(
        after_node.admission, score_handle, root.body_id,
    ).is_some());
    assert(bridge::bridge_nonsemantic_step(before).is_some());
    let (after, _, _) = bridge::bridge_nonsemantic_step(before).unwrap();
    bridge::bridge_nonsemantic_exact(before);
    assert(bridge::bridge_score_source_step(before, after));
    bridge::bridge_source_step_extends_reachability(before, after);
    assert(admission::admission_complete_root(after.admission));
    bridge::bridge_root_exact(after);
    modeled_root_return_matches_pre_pop(before, after);
}

}

fn main() {}
