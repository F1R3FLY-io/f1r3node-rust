use vstd::prelude::*;

mod rpb3_epathmap_event_refinement;

use rpb3_epathmap_event_refinement as epm;

verus! {

// One successful invocation of protobuf_admission::epathmap_step may consume
// arbitrarily many internal topology events before returning Push. These
// transitions are internal to that invocation, not driver Continue actions.
//
// This relation is over model states. The Rust-to-model extraction obligations
// remain separate: project the actual pre/post frame and indexed region,
// expand each accepted Forward/Unwind batch into its scalar topology events
// with exact source positions, and show that the returned FrameRequest and
// parent waiting state are the terminal linked_epm_push transition. Neither
// this relation nor the theorem below assumes that extraction has been proved.
pub open spec fn linked_epm_internal_chain(
    states: Seq<epm::LinkedEpmState>,
    events: Seq<epm::EpmEvent>,
) -> bool {
    &&& states.len() == events.len() + 1
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger] epm::linked_epm_continue(
        states[i], states[i + 1], events[i],
    )
}

pub open spec fn linked_epm_push_macro(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    terminal_event: epm::EpmEvent,
    action: epm::EpmAction,
    child: epm::admission::AdmissionFrameState,
    after: epm::LinkedEpmState,
) -> bool {
    &&& linked_epm_internal_chain(states, internal_events)
    &&& states.len() > 0
    &&& epm::linked_epm_push(states.last(), after, terminal_event, action, child)
}

pub proof fn linked_epm_internal_prefix_preserves(
    states: Seq<epm::LinkedEpmState>,
    events: Seq<epm::EpmEvent>,
    upto: int,
)
    requires
        linked_epm_internal_chain(states, events),
        0 <= upto < states.len(),
        states[0].admission.frames.len() > 0,
    ensures
        states[upto].admission.arena == states[0].admission.arena,
        states[upto].indexed_regions == states[0].indexed_regions,
        states[upto].admission.frames.len() == states[0].admission.frames.len(),
        states[upto].admission.frames.last().source_bytes
            == states[0].admission.frames.last().source_bytes,
        states[0].admission.frames.last().cursor
            <= states[upto].admission.frames.last().cursor,
        forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
            ==> states[upto].admission.frames[i] == states[0].admission.frames[i],
    decreases upto,
{
    if upto > 0 {
        linked_epm_internal_prefix_preserves(states, events, upto - 1);
        let previous = upto - 1;
        assert(0 <= previous < events.len());
        assert(linked_epm_internal_chain(states, events));
        assert(epm::linked_epm_continue(
            states[previous], states[previous + 1], events[previous],
        ));
        epm::linked_epm_continue_preserves_source_and_progress(
            states[upto - 1], states[upto], events[upto - 1],
        );
        assert(states[upto].admission.frames.len() > 0);
        assert forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
            implies states[upto].admission.frames[i] == states[0].admission.frames[i] by {
            assert(states[upto - 1].admission.frames[i]
                == states[0].admission.frames[i]);
            assert(states[upto].admission.frames[i]
                == states[upto - 1].admission.frames[i]);
        }
    }
}

// This proves a macro-level frame/source invariant from the already linked
// scalar transitions. The final Push step is one modeled event, rather than
// an assumed equality between the first and last state of the whole loop.
pub proof fn linked_epm_push_macro_preserves_source_and_progress(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    terminal_event: epm::EpmEvent,
    action: epm::EpmAction,
    child: epm::admission::AdmissionFrameState,
    after: epm::LinkedEpmState,
)
    requires linked_epm_push_macro(
        states, internal_events, terminal_event, action, child, after,
    ),
    ensures
        after.admission.arena == states[0].admission.arena,
        after.admission.frames.len() == states[0].admission.frames.len() + 1,
        states[0].admission.frames.last().cursor
            <= after.admission.frames[after.admission.frames.len() - 2].cursor,
        after.admission.frames[after.admission.frames.len() - 2].source_bytes
            == states[0].admission.frames.last().source_bytes,
        forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
            ==> after.admission.frames[i] == states[0].admission.frames[i],
        match action {
            epm::EpmAction::Push(request) => {
                let body = epm::request_body(request);
                let pushed = after.admission.frames.last();
                &&& after.indexed_regions == states[0].indexed_regions.push(body)
                &&& pushed.source_bytes == body.bytes
                &&& pushed.physical_source == epm::indexed_position_at(body, 0).source
                &&& pushed.physical_start == epm::indexed_position_at(body, 0).offset
                &&& after.admission.frames[after.admission.frames.len() - 2].waiting_id
                    == Some(epm::request_return_id(request))
                &&& after.admission.frames[after.admission.frames.len() - 2].waiting_kind
                    == epm::request_frame_kind(request)
            },
            _ => false,
        },
{
    let last = states.len() - 1;
    assert(states.len() > 0);
    assert(epm::linked_epm_push(
        states[last], after, terminal_event, action, child,
    ));
    if internal_events.len() == 0 {
        assert(last == 0);
        assert(states[last].admission.frames.len() > 0);
    } else {
        assert(0 < internal_events.len());
        assert(epm::linked_epm_continue(states[0], states[1], internal_events[0]));
        assert(states[0].admission.frames.len() > 0);
    }
    linked_epm_internal_prefix_preserves(states, internal_events, last);
    epm::linked_epm_push_preserves_parent_progress(
        states[last], after, terminal_event, action, child,
    );
    assert(after.admission.frames.len() > 1);
    assert forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
        implies after.admission.frames[i] == states[0].admission.frames[i] by {
        assert(states[last].admission.frames[i] == states[0].admission.frames[i]);
        assert(after.admission.frames[i] == states[last].admission.frames[i]);
    }
    match action {
        epm::EpmAction::Push(request) => {
            let body = epm::request_body(request);
            let pushed = after.admission.frames.last();
            assert(after.indexed_regions == states[last].indexed_regions.push(body));
            assert(epm::linked_indexed_sources(after));
            assert(after.indexed_regions.last() == body);
            assert(pushed.source_bytes == body.bytes);
            assert(pushed.physical_source == epm::indexed_position_at(body, 0).source);
            assert(pushed.physical_start == epm::indexed_position_at(body, 0).offset);
        },
        _ => assert(false),
    }
}

}

fn main() {}
