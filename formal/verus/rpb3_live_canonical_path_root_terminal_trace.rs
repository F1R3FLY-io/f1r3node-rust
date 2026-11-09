//! CanonicalPath root-to-terminal composition over an arbitrary finite trace.
//!
//! Root, PathPush, and PathChildReturn use their deterministic live selectors.
//! A Descendant action still has an independently source-certified product
//! boundary; this theorem does not derive every such product step from Rust.
//! It excludes newly created kind-4 descendants and rejected reducer values.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_root_source_refinement as root_source,
    rpb3_epathmap_event_refinement as epm, rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_terminal_refinement as terminal,
    rpb3_live_canonical_path_trace_refinement as trace,
};

verus! {

/// Only EOF and reducer-result facts are supplied for the final state. Live
/// well-formedness and the retained indexed root are consequences of `trace`.
pub open spec fn canonical_path_trace_terminal_eof_observed(
    states: Seq<path_live::LiveCanonicalPathState>,
    reducer: root_source::CanonicalPathReducerEofObservation,
) -> bool {
    if states.len() == 0 { false } else {
        let final_state = states.last();
        if final_state.path_frames.len() == 0
            || final_state.path_frames[0].is_none()
        { false } else {
            let path_frame = final_state.path_frames[0].unwrap();
            &&& final_state.base.source.product.bridge.admission.frames.len() == 1
            &&& path_frame.cursor as int == path_frame.input.bytes.len()
            &&& reducer.has_value
            &&& reducer.at == path_frame.input.end
            &&& !reducer.active_obligation
            &&& reducer.value_result.is_some()
        }
    }
}

pub proof fn canonical_path_trace_terminal_source_is_ready(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    reducer: root_source::CanonicalPathReducerEofObservation,
)
    requires
        trace::live_canonical_path_trace(root, states, actions),
        canonical_path_trace_terminal_eof_observed(states, reducer),
    ensures
        terminal::live_canonical_path_terminal_source_ready(
            states.last(), reducer,
        ),
        states.last().base.source.product.indexed_regions[0]
            == path_live::indexed_from_admission(root.indexed_input),
{
    trace::live_canonical_path_trace_prefix_preserves_wf(
        root, states, actions, states.len() - 1,
    );
    let final_state = states.last();
    assert(path_live::live_canonical_path_wf(final_state));
    assert(final_state.path_frames.len() == 1);
    assert(final_state.outstanding.len() == 1);
    assert(path_live::path_frame_matches_live(final_state, 0));
    let path_frame = final_state.path_frames[0].unwrap();
    assert(final_state.outstanding[0].is_none()) by {
        if final_state.outstanding[0].is_some() {
            match path_frame.waiting {
                Some(_) => assert(false),
                None => assert(false),
            }
        }
    }
    assert(path_frame.waiting.is_none());
    assert(terminal::live_canonical_path_terminal_source_ready(
        final_state, reducer,
    ));
}

/// The returned terminal value is selected from the final live pre-pop state;
/// there is no empty `LiveCanonicalPathState` and no caller-supplied terminal
/// poststate. The trace may be any finite length, including zero actions.
pub proof fn canonical_path_root_trace_to_successful_terminal_is_exact(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    reducer: root_source::CanonicalPathReducerEofObservation,
)
    requires
        trace::live_canonical_path_trace(root, states, actions),
        canonical_path_trace_terminal_eof_observed(states, reducer),
    ensures {
        let final_state = states.last();
        let pre = final_state.base.source.product.bridge.admission;
        let indexed_root = final_state.base.source.product.indexed_regions[0];
        let outcome = terminal::live_canonical_path_terminal_complete_from_source(
            final_state, reducer,
        ).unwrap();
        &&& terminal::live_canonical_path_terminal_complete_from_source(
            final_state, reducer,
        ).is_some()
        &&& indexed_root == path_live::indexed_from_admission(root.indexed_input)
        &&& indexed_root.bytes == root.body_bytes
        &&& indexed_root.end == (epm::SourceAnchor {
            source: root.body_end.source,
            offset: root.body_end.offset,
        })
        &&& pre.frames.len() == 1
        &&& pre.frames[0].frame_kind == 4
        &&& pre.frames[0].nested_started == pre.frames[0].nested_resolved
        &&& final_state.outstanding[0].is_none()
        &&& !reducer.active_obligation
        &&& outcome.remaining_frames.len() == 0
        &&& outcome.returned.remaining_frames == 0
        &&& outcome.returned.completed as int == pre.completed as int + 1
        &&& outcome.returned.pushed == pre.pushed
        &&& outcome.returned.arena == pre.arena
        &&& outcome.returned.transcript
            == final_state.base.source.product.bridge.transcript
        &&& outcome.returned.summary == reducer.value_result.unwrap()
        &&& outcome.indexed_end == indexed_root.end
        &&& outcome.indexed_end == reducer.at
    },
{
    canonical_path_trace_terminal_source_is_ready(
        root, states, actions, reducer,
    );
    let final_state = states.last();
    terminal::live_canonical_path_terminal_complete_is_exact(final_state, reducer);
    let pre = final_state.base.source.product.bridge.admission;
    assert(admission::admission_state_wf(pre));
    assert(admission::admission_frame_wf(pre.frames[0]));
    assert(pre.frames[0].waiting_id.is_none());
    assert(pre.frames[0].waiting_kind == 0);
    assert(pre.frames[0].nested_started == pre.frames[0].nested_resolved);
    assert(root.indexed_input.bytes == root.body_bytes);
    assert(root.indexed_input.end == root.body_end);
    assert(final_state.base.source.product.indexed_regions[0]
        == path_live::indexed_from_admission(root.indexed_input));
}

}
