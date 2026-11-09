//! Finite, arbitrary-length traces for the retained CanonicalPath live product.
//!
//! Root, Push, and child return use the deterministic selectors in the live
//! wrapper. Descendant work is an explicit source-certified boundary: its
//! integrated product must already be well formed and it must preserve every
//! retained kind-4 frame, its indexed image, and the immediate child's return
//! header. This file does not extract those observations from arbitrary Rust.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission,
    rpb3_epathmap_event_refinement as epm,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_wire_cursor_source_refinement as wire,
};

verus! {

/// A descendant driver action may change the non-path child subtree by one
/// stack frame (or only mutate its top frame). The source-certified base state
/// remains an explicit input until all child driver actions are integrated.
pub open spec fn canonical_descendant_source_ready(
    before: path_live::LiveCanonicalPathState,
    observed_base: crate::LiveRuntimeProductState,
) -> bool {
    if !path_live::live_canonical_path_wf(before)
        || !crate::live_runtime_product_wf(observed_base)
        || before.path_frames.len() == 0
        || before.path_frames[0].is_none()
        || before.base.source.product.bridge.admission.frames.last().frame_kind == 4
    { false } else {
        let old_len = before.path_frames.len();
        let new_len = observed_base.source.product.bridge.admission.frames.len();
        &&& new_len > 0
        &&& new_len <= old_len + 1
        &&& old_len <= new_len + 1
        &&& forall|i: int| 0 <= i < old_len && before.path_frames[i].is_some()
            ==> i < new_len
        &&& forall|i: int| 0 <= i < new_len ==> {
            if i < old_len {
                match before.path_frames[i] {
                    None => observed_base.source.product.bridge.admission.frames[i]
                        .frame_kind != 4,
                    Some(_) => {
                        &&& observed_base.source.product.bridge.admission.frames[i]
                            == before.base.source.product.bridge.admission.frames[i]
                        &&& observed_base.source.product.indexed_regions[i]
                            == before.base.source.product.indexed_regions[i]
                        &&& observed_base.source.wire[i] == before.base.source.wire[i]
                        &&& observed_base.cursors[i] == before.base.cursors[i]
                    },
                }
            } else {
                observed_base.source.product.bridge.admission.frames[i].frame_kind != 4
            }
        }
        &&& forall|i: int| 0 <= i < old_len && before.outstanding[i].is_some()
            ==> {
                &&& i + 1 < new_len
                &&& observed_base.source.product.bridge.admission.frames[i + 1].return_id
                    == before.base.source.product.bridge.admission.frames[i + 1].return_id
                &&& observed_base.source.product.bridge.admission.frames[i + 1].frame_kind
                    == before.base.source.product.bridge.admission.frames[i + 1].frame_kind
                &&& observed_base.source.product.indexed_regions[i + 1]
                    == before.base.source.product.indexed_regions[i + 1]
            }
    }
}

/// The active path sidecars are neither reconstructed from a completed child
/// nor supplied as an arbitrary after-state. They are retained by index; a
/// newly pushed non-path descendant receives no path sidecar.
pub open spec fn canonical_descendant_from_source(
    before: path_live::LiveCanonicalPathState,
    observed_base: crate::LiveRuntimeProductState,
) -> Option<path_live::LiveCanonicalPathState> {
    if !canonical_descendant_source_ready(before, observed_base) { None } else {
        let old_len = before.path_frames.len();
        let new_len = observed_base.source.product.bridge.admission.frames.len();
        Some(path_live::LiveCanonicalPathState {
            base: observed_base,
            path_frames: Seq::new(new_len, |i: int|
                if i < old_len { before.path_frames[i] } else { None }),
            outstanding: Seq::new(new_len, |i: int|
                if i < old_len { before.outstanding[i] } else { None }),
        })
    }
}

pub proof fn canonical_descendant_preserves_retained_path(
    before: path_live::LiveCanonicalPathState,
    observed_base: crate::LiveRuntimeProductState,
)
    requires canonical_descendant_source_ready(before, observed_base),
    ensures match canonical_descendant_from_source(before, observed_base) {
        Some(after) => {
            &&& path_live::live_canonical_path_wf(after)
            &&& after.base.source.product.indexed_regions[0]
                == before.base.source.product.indexed_regions[0]
            &&& forall|i: int| 0 <= i < before.path_frames.len()
                && before.path_frames[i].is_some() ==> {
                    &&& after.path_frames[i] == before.path_frames[i]
                    &&& after.outstanding[i] == before.outstanding[i]
                    &&& after.base.source.product.indexed_regions[i]
                        == before.base.source.product.indexed_regions[i]
                }
        },
        None => false,
    },
{
    let after = canonical_descendant_from_source(before, observed_base).unwrap();
    let old_len = before.path_frames.len();
    let new_len = after.path_frames.len();
    assert(after.base == observed_base);
    assert(after.path_frames.len() == new_len);
    assert(after.outstanding.len() == new_len);
    assert forall|i: int| 0 <= i < new_len implies
        path_live::path_frame_matches_live(after, i) by {
        if i < old_len {
            assert(path_live::path_frame_matches_live(before, i));
            assert(after.path_frames[i] == before.path_frames[i]);
            assert(after.outstanding[i] == before.outstanding[i]);
            match before.path_frames[i] {
                None => {
                    assert(before.outstanding[i].is_none());
                    assert(after.base.source.product.bridge.admission.frames[i]
                        .frame_kind != 4);
                    assert(path_live::path_frame_matches_live(after, i));
                },
                Some(old_path) => {
                    assert(path_live::path_frame_matches_live(before, i));
                    assert(after.path_frames[i] == Some(old_path));
                    assert(after.base.source.product.bridge.admission.frames[i]
                        == before.base.source.product.bridge.admission.frames[i]);
                    assert(after.base.source.product.indexed_regions[i]
                        == before.base.source.product.indexed_regions[i]);
                    assert(after.base.source.wire[i] == before.base.source.wire[i]);
                    assert(after.base.cursors[i] == before.base.cursors[i]);
                    assert(old_path.input == after.base.source.product.indexed_regions[i]);
                    assert(old_path.cursor
                        == after.base.source.product.bridge.admission.frames[i].cursor);
                    if before.outstanding[i].is_some() {
                        assert(old_path.waiting.is_some());
                        assert(i + 1 < new_len);
                        assert(after.base.source.product.bridge.admission.frames[i + 1].return_id
                            == before.base.source.product.bridge.admission.frames[i + 1]
                                .return_id);
                        assert(after.base.source.product.bridge.admission.frames[i + 1].frame_kind
                            == before.base.source.product.bridge.admission.frames[i + 1]
                                .frame_kind);
                        assert(after.base.source.product.indexed_regions[i + 1]
                            == before.base.source.product.indexed_regions[i + 1]);
                    } else {
                        assert(old_path.waiting.is_none());
                    }
                    assert(path_live::path_frame_matches_live(after, i));
                },
            }
        } else {
            assert(after.path_frames[i].is_none());
            assert(after.outstanding[i].is_none());
            assert(after.base.source.product.bridge.admission.frames[i].frame_kind != 4);
            assert(path_live::path_frame_matches_live(after, i));
        }
    }
    assert(path_live::live_canonical_path_wf(after));
    assert(before.path_frames[0].is_some());
    assert(after.base.source.product.indexed_regions[0]
        == before.base.source.product.indexed_regions[0]);
    assert forall|i: int| 0 <= i < old_len && before.path_frames[i].is_some()
        implies after.path_frames[i] == before.path_frames[i]
            && after.outstanding[i] == before.outstanding[i]
            && after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i] by {}
}

pub enum LiveCanonicalPathTraceAction {
    PathPush {
        source: admission::AdmissionCanonicalPathPushSource,
        child: admission::AdmissionFrameState,
        storage: admission::AdmissionCanonicalPathPushStorage,
        child_image: epm::IndexedRegionImage,
        child_reader: Option<wire::WireRuntimeCursorImage>,
    },
    PathChildReturn {
        observed: path_live::LiveCanonicalPathReturnObservation,
    },
    Descendant {
        observed_base: crate::LiveRuntimeProductState,
    },
}

pub open spec fn live_canonical_path_trace_step(
    before: path_live::LiveCanonicalPathState,
    after: path_live::LiveCanonicalPathState,
    action: LiveCanonicalPathTraceAction,
) -> bool {
    match action {
        LiveCanonicalPathTraceAction::PathPush {
            source, child, storage, child_image, child_reader,
        } => path_live::live_canonical_path_push_source_ready(
            before, source, child, storage, child_image, child_reader,
        ) && path_live::live_canonical_path_push_from_source(
            before, source, child, storage, child_image, child_reader,
        ) == Some(after),
        LiveCanonicalPathTraceAction::PathChildReturn { observed } =>
            path_live::live_canonical_path_return_source_ready(before, observed)
                && path_live::live_canonical_path_return_from_source(before, observed).is_some()
                && path_live::live_canonical_path_return_from_source(
                    before, observed,
                ).unwrap().state == after,
        LiveCanonicalPathTraceAction::Descendant { observed_base } =>
            canonical_descendant_source_ready(before, observed_base)
                && canonical_descendant_from_source(before, observed_base) == Some(after),
    }
}

pub proof fn live_canonical_path_trace_step_preserves_wf(
    before: path_live::LiveCanonicalPathState,
    after: path_live::LiveCanonicalPathState,
    action: LiveCanonicalPathTraceAction,
)
    requires live_canonical_path_trace_step(before, after, action),
    ensures
        path_live::live_canonical_path_wf(after),
        after.base.source.product.indexed_regions[0]
            == before.base.source.product.indexed_regions[0],
{
    match action {
        LiveCanonicalPathTraceAction::PathPush {
            source, child, storage, child_image, child_reader,
        } => {
            path_live::live_canonical_path_push_preserves_wf(
                before, source, child, storage, child_image, child_reader,
            );
            path_live::live_canonical_path_push_preserves_ancestors(
                before, source, child, storage, child_image, child_reader,
            );
            assert(after == path_live::live_canonical_path_push_from_source(
                before, source, child, storage, child_image, child_reader,
            ).unwrap());
            if before.path_frames.len() == 1 {
                assert(after.base.source.product.indexed_regions[0]
                    == before.base.source.product.indexed_regions[0]);
            } else {
                assert(0 < before.path_frames.len() - 1);
            }
        },
        LiveCanonicalPathTraceAction::PathChildReturn { observed } => {
            path_live::live_canonical_path_return_preserves_wf(before, observed);
            path_live::live_canonical_path_return_preserves_ancestors(before, observed);
            assert(after == path_live::live_canonical_path_return_from_source(
                before, observed,
            ).unwrap().state);
            if before.path_frames.len() == 2 {
                assert(after.base.source.product.indexed_regions[0]
                    == before.base.source.product.indexed_regions[0]);
            } else {
                assert(0 < before.path_frames.len() - 2);
            }
        },
        LiveCanonicalPathTraceAction::Descendant { observed_base } => {
            canonical_descendant_preserves_retained_path(before, observed_base);
            assert(after == canonical_descendant_from_source(before, observed_base).unwrap());
        },
    }
}

pub open spec fn live_canonical_path_trace(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<LiveCanonicalPathTraceAction>,
) -> bool {
    &&& admission::admission_canonical_path_root_source_wf(root)
    &&& states.len() == actions.len() + 1
    &&& states[0] == path_live::live_canonical_path_root_from_source(root).unwrap()
    &&& forall|i: int| 0 <= i < actions.len()
        ==> live_canonical_path_trace_step(states[i], states[i + 1], actions[i])
}

pub proof fn live_canonical_path_root_trace_exists(
    root: admission::AdmissionCanonicalPathRootSource,
)
    requires admission::admission_canonical_path_root_source_wf(root),
    ensures live_canonical_path_trace(
        root,
        seq![path_live::live_canonical_path_root_from_source(root).unwrap()],
        Seq::empty(),
    ),
{
    path_live::live_canonical_path_root_is_wf(root);
}

pub proof fn live_canonical_path_trace_prefix_preserves_wf(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<LiveCanonicalPathTraceAction>,
    index: int,
)
    requires live_canonical_path_trace(root, states, actions),
        0 <= index < states.len(),
    ensures
        path_live::live_canonical_path_wf(states[index]),
        states[index].base.source.product.indexed_regions[0]
            == states[0].base.source.product.indexed_regions[0],
        states[index].base.source.product.indexed_regions[0]
            == path_live::indexed_from_admission(root.indexed_input),
    decreases index,
{
    if index == 0 {
        path_live::live_canonical_path_root_is_wf(root);
    } else {
        live_canonical_path_trace_prefix_preserves_wf(root, states, actions, index - 1);
        live_canonical_path_trace_step_preserves_wf(
            states[index - 1], states[index], actions[index - 1],
        );
    }
}

}
