//! Terminal CanonicalPath root Complete from the retained live product.
//!
//! The live product requires a nonempty stack. The no-parent driver return is
//! therefore a separate terminal image, selected from the live pre-pop state,
//! its indexed EOF, and the observed reducer value. This module does not
//! extract those ghost observations from arbitrary Rust execution.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_root_source_refinement as root_source,
    rpb3_epathmap_event_refinement as epm, rpb3_live_canonical_path_source_refinement as live,
};

verus! {

pub struct LiveCanonicalPathTerminalOutcome {
    pub returned: root_source::CanonicalPathRootReturnImage,
    pub indexed_end: epm::SourceAnchor,
    pub ghost remaining_frames: Seq<admission::AdmissionFrameState>,
}

/// The live frame and indexed region are retained before `step` returns
/// Complete. `reducer` represents the successful `value(true, end)` result;
/// binding it to the actual reducer call is an outstanding Rust extraction.
pub open spec fn live_canonical_path_terminal_source_ready(
    before: live::LiveCanonicalPathState,
    reducer: root_source::CanonicalPathReducerEofObservation,
) -> bool {
    &&& live::live_canonical_path_wf(before)
    &&& before.base.source.product.bridge.admission.frames.len() == 1
    &&& before.path_frames[0].is_some()
    &&& before.outstanding[0].is_none()
    &&& before.path_frames[0].unwrap().waiting.is_none()
    &&& before.path_frames[0].unwrap().cursor as int
        == before.path_frames[0].unwrap().input.bytes.len()
    &&& reducer.has_value
    &&& reducer.at == before.path_frames[0].unwrap().input.end
    &&& !reducer.active_obligation
    &&& reducer.value_result.is_some()
}

/// The checked root selector constructs the no-parent result. There is no
/// caller-supplied poststate and no attempt to represent an empty live state.
pub open spec fn live_canonical_path_terminal_complete_from_source(
    before: live::LiveCanonicalPathState,
    reducer: root_source::CanonicalPathReducerEofObservation,
) -> Option<LiveCanonicalPathTerminalOutcome> {
    if !live_canonical_path_terminal_source_ready(before, reducer) {
        None
    } else {
        let path_frame = before.path_frames[0].unwrap();
        match root_source::canonical_path_root_complete_from_source(
            before.base.source.runtime, path_frame, reducer,
        ) {
            None => None,
            Some(returned) => Some(LiveCanonicalPathTerminalOutcome {
                returned,
                indexed_end: path_frame.input.end,
                remaining_frames: Seq::empty(),
            }),
        }
    }
}

/// The product's indexed start anchor, frame bytes, cursor and source score
/// bridge establish the standalone root selector's source-matching guard.
pub proof fn live_canonical_path_terminal_enables_root_source(
    before: live::LiveCanonicalPathState,
    reducer: root_source::CanonicalPathReducerEofObservation,
)
    requires live_canonical_path_terminal_source_ready(before, reducer),
    ensures root_source::canonical_path_root_source_ready(
        before.base.source.runtime, before.path_frames[0].unwrap(), reducer,
    ),
{
    let path_frame = before.path_frames[0].unwrap();
    let product = before.base.source.product;
    let root = product.bridge.admission.frames[0];
    assert(live::path_frame_matches_live(before, 0));
    assert(path_frame.input == product.indexed_regions[0]);
    assert(before.base.source.runtime.score == product.bridge);
    assert(crate::product_frame_source_anchors_match(product));
    assert(root.source_bytes == product.indexed_regions[0].bytes);
    assert(root.physical_source
        == crate::product_indexed_start_anchor(product.indexed_regions[0]).source);
    assert(root.physical_start
        == crate::product_indexed_start_anchor(product.indexed_regions[0]).offset);
    assert(crate::product_indexed_start_anchor(path_frame.input)
        == root_source::indexed_root_start(path_frame.input));
    assert(root_source::canonical_path_root_runtime_matches(
        before.base.source.runtime, path_frame,
    ));
}

pub proof fn live_canonical_path_terminal_complete_is_exact(
    before: live::LiveCanonicalPathState,
    reducer: root_source::CanonicalPathReducerEofObservation,
)
    requires live_canonical_path_terminal_source_ready(before, reducer),
    ensures match live_canonical_path_terminal_complete_from_source(before, reducer) {
        Some(outcome) => {
            let pre = before.base.source.product.bridge.admission;
            let path_frame = before.path_frames[0].unwrap();
            &&& outcome.remaining_frames.len() == 0
            &&& outcome.returned.remaining_frames == 0
            &&& outcome.returned.completed as int == pre.completed as int + 1
            &&& outcome.returned.pushed == pre.pushed
            &&& outcome.returned.arena == pre.arena
            &&& outcome.returned.transcript
                == before.base.source.product.bridge.transcript
            &&& outcome.returned.summary == reducer.value_result.unwrap()
            &&& outcome.indexed_end == path_frame.input.end
            &&& outcome.indexed_end
                == before.base.source.product.indexed_regions[0].end
            &&& outcome.indexed_end == reducer.at
            &&& path_frame.cursor as int == path_frame.input.bytes.len()
            &&& outcome.returned.nested_started == outcome.returned.nested_resolved
        },
        None => false,
    },
{
    live_canonical_path_terminal_enables_root_source(before, reducer);
    let path_frame = before.path_frames[0].unwrap();
    root_source::canonical_path_root_complete_is_exact(
        before.base.source.runtime, path_frame, reducer,
    );
    assert(live::path_frame_matches_live(before, 0));
    assert(path_frame.input == before.base.source.product.indexed_regions[0]);
    assert(before.base.source.runtime.score == before.base.source.product.bridge);
}

}
